import { socket } from '@kit.NetworkKit';
import { util, url } from '@kit.ArkTS';
import { ResourceProvider } from './ResourceLoader';
import { ResourceInfo } from './StartupCache';
import { NativeSession } from './NativeSession';
import { ByteRange } from './Transport';
import { OperationScope, VesperCancellationToken, nativeError } from './Operation';
import { VesperPlayerError } from './Types';

const HEADER_BYTES: number = 8192;
const CHUNK_BYTES: number = 256 * 1024;
const MANIFEST_BYTES: number = 256 * 1024;
const RESOURCE_BYTES: number = 1024 * 1024 * 1024;

interface OriginListener {
  remote: string; server: socket.TCPSocketServer; ready: Promise<string>; local?: string; closed: boolean;
}
interface HttpClient {
  connection: socket.TCPSocketConnection; origin: OriginListener; token: VesperCancellationToken;
  scope: OperationScope; header: Uint8Array; started: boolean; sentHeader: boolean; closed: boolean;
  pending: Promise<void>; work?: Promise<void>; closing?: Promise<void>;
}
interface HttpRequest { method: string; target: string; headers: Record<string, string>; }
interface ManifestProjection { manifest: string; origins: string[]; excludedCodecs: string[]; }

function encoded(text: string): ArrayBuffer { return new util.TextEncoder().encodeInto(text).buffer as ArrayBuffer; }

/** Inclusive single HTTP range. Unsatisfiable or malformed ranges produce 416. */
export function resourceResponseRange(header: string | undefined, total: number): ByteRange | undefined {
  if (!Number.isSafeInteger(total) || total < 1 || total > RESOURCE_BYTES) { return undefined; }
  if (header === undefined) { return { start: 0, end: total - 1 }; }
  const match = header.match(/^bytes=(\d*)-(\d*)$/);
  if (match === null || match[1].length > 16 || match[2].length > 16) { return undefined; }
  if (match[1].length === 0) {
    const suffix = Number(match[2]);
    return Number.isSafeInteger(suffix) && suffix > 0 ? { start: Math.max(0, total - suffix), end: total - 1 } : undefined;
  }
  const start = Number(match[1]); const end = match[2].length > 0 ? Number(match[2]) : total - 1;
  return Number.isSafeInteger(start) && Number.isSafeInteger(end) && start < total && end >= start
    ? { start: start, end: Math.min(total - 1, end) } : undefined;
}

function parseRequest(text: string): HttpRequest {
  const lines = text.split('\r\n'); const first = lines.shift()?.split(' ') ?? [];
  if (first.length !== 3 || !['GET', 'HEAD'].includes(first[0]) || !['HTTP/1.0', 'HTTP/1.1'].includes(first[2]) ||
    !first[1].startsWith('/') || first[1].startsWith('//') || /[\\#\u0000-\u0020\u007f]/.test(first[1])) {
    throw new VesperPlayerError('invalid_request', 'Loopback HTTP request is invalid');
  }
  const headers: Record<string, string> = {};
  for (const line of lines) {
    if (line.length === 0) { continue; }
    const separator = line.indexOf(':'); const name = line.substring(0, separator).toLowerCase();
    const value = line.substring(separator + 1).trim();
    if (separator < 1 || !/^[!#$%&'*+.^_`|~0-9a-z-]+$/.test(name) || headers[name] !== undefined || /[\r\n\0]/.test(value)) {
      throw new VesperPlayerError('invalid_request', 'Loopback HTTP headers are invalid');
    }
    headers[name] = value;
  }
  if (headers['transfer-encoding'] !== undefined || headers['content-length'] !== undefined && headers['content-length'] !== '0') {
    throw new VesperPlayerError('invalid_request', 'Loopback HTTP request bodies are unsupported');
  }
  return { method: first[0], target: first[1], headers: headers };
}

/** Native HTTP keeps compressed-byte backpressure off the ArkTS/Binder response path. */
export class NativeResourceServer {
  private provider: ResourceProvider;
  private report: (error: VesperPlayerError) => void;
  private protocol: string;
  private secret: string = util.generateRandomUUID();
  private origins: Map<string, OriginListener> = new Map();
  private clients: Set<HttpClient> = new Set();
  private bridge: NativeSession = new NativeSession('{"kind":"playbackManifest"}');
  private disposed: boolean = false;
  private excluded: Set<string> = new Set();
  private failures: number = 0;
  private rejected: number = 0;
  private completed: number = 0;
  private closeErrors: Record<string, number> = {};
  private requestErrors: Record<string, number> = {};
  deliveredBytes: number = 0;
  reads: number = 0;
  constructor(provider: ResourceProvider, protocol: string, report: (error: VesperPlayerError) => void) {
    this.provider = provider; this.protocol = protocol; this.report = report;
  }
  headers(): Record<string, string> { return {}; }
  async start(uri: string): Promise<string> { return this.localUrl(uri); }
  private async origin(remote: string): Promise<string> {
    if (this.disposed) { throw new VesperPlayerError('disposed', 'Playback HTTP server is closed'); }
    const existing = this.origins.get(remote); if (existing !== undefined) { return existing.ready; }
    if (this.origins.size >= 8) { throw new VesperPlayerError('capacity', 'Playback HTTP server exceeds eight origins'); }
    const server = socket.constructTCPSocketServerInstance();
    const entry: OriginListener = { remote: remote, server: server, ready: Promise.resolve(''), closed: false };
    this.origins.set(remote, entry);
    server.on('connect', (connection: socket.TCPSocketConnection): void => { this.accept(entry, connection); });
    server.on('error', (): void => { this.failures++; });
    entry.ready = Promise.resolve().then(async (): Promise<string> => {
      await server.listen({ address: '127.0.0.1', port: 0, family: 1 });
      const address = await server.getLocalAddress();
      const port = address.port;
      if (address.address !== '127.0.0.1' || port === undefined || port < 1) { throw new VesperPlayerError('native_failure', 'Playback HTTP listener did not bind loopback'); }
      entry.local = `http://127.0.0.1:${port}`;
      if (this.disposed) { throw new VesperPlayerError('disposed', 'Playback HTTP server is closed'); }
      return entry.local;
    });
    return entry.ready;
  }
  private async localUrl(uri: string): Promise<string> {
    const parsed = new url.URL(uri);
    if (!['http:', 'https:'].includes(parsed.protocol) || parsed.username.length > 0 || parsed.password.length > 0) {
      throw new VesperPlayerError('unsupported', 'Playback HTTP resources require HTTP(S) without embedded credentials');
    }
    return await this.origin(parsed.origin) + '/' + this.secret + parsed.pathname + parsed.search;
  }
  private accept(origin: OriginListener, connection: socket.TCPSocketConnection): void {
    if (this.disposed || this.clients.size >= 16) {
      this.rejected++; connection.close().catch((): void => { this.failures++; }); return;
    }
    const token = new VesperCancellationToken(); const scope = new OperationScope(60000, token);
    const client: HttpClient = { connection: connection, origin: origin, token: token, scope: scope,
      header: new Uint8Array(0), started: false, sentHeader: false, closed: false, pending: Promise.resolve() };
    this.clients.add(client);
    scope.onFailure((): void => { token.cancel(); this.closeClient(client).catch((): void => {}); });
    connection.on('message', (message: socket.SocketMessageInfo): void => { this.receive(client, message.message); });
    connection.on('close', (): void => {
      client.closed = true; token.cancel(); scope.close();
      client.pending.then((): void => { this.clients.delete(client); });
    });
    connection.on('error', (): void => { token.cancel(); this.closeClient(client).catch((): void => {}); });
  }
  private receive(client: HttpClient, bytes: ArrayBuffer): void {
    if (client.started || client.closed) { client.token.cancel(); this.closeClient(client).catch((): void => {}); return; }
    if (client.header.byteLength + bytes.byteLength > HEADER_BYTES) { this.rejected++; this.closeClient(client).catch((): void => {}); return; }
    const buffer = new Uint8Array(client.header.byteLength + bytes.byteLength);
    buffer.set(client.header); buffer.set(new Uint8Array(bytes), client.header.byteLength); client.header = buffer;
    let text: string;
    try { text = util.TextDecoder.create('utf-8', { fatal: true }).decodeWithStream(buffer); }
    catch (_) { this.rejected++; this.closeClient(client).catch((): void => {}); return; }
    const boundary = text.indexOf('\r\n\r\n'); if (boundary < 0) { return; }
    client.started = true;
    client.work = this.respond(client, text, boundary).catch((error: Error): void => {
      if (!client.token.isCancelled && !this.disposed) {
        this.failures++; const failure = nativeError(error, 'playback HTTP response');
        const raw = `${failure.code}:${failure.nativeCode ?? ''}`;
        const code = this.requestErrors[raw] !== undefined || Object.keys(this.requestErrors).length < 16 ? raw : 'other';
        this.requestErrors[code] = (this.requestErrors[code] ?? 0) + 1;
        // Media Kit may retry a failed HTTP transfer or choose another rendition.
        // Reject invalid input immediately; transport failures remain diagnostic
        // until the native player reports that playback itself has failed.
        if (['unsupported', 'invalid_response', 'access_scope', 'resource_changed', 'capacity'].includes(failure.code) ||
          /^http_(?:4(?!08|29)\d\d)$/.test(failure.code)) { this.report(failure); }
      }
    }).finally(async (): Promise<void> => { await this.closeClient(client).catch((): void => {}); });
  }
  private async header(client: HttpClient, status: string, fields: string): Promise<void> {
    client.scope.check(); client.sentHeader = true;
    await this.physical(client, client.connection.send({ data: encoded(`HTTP/1.1 ${status}\r\nConnection: close\r\n${fields}\r\n`) }));
  }
  private physical<T>(client: HttpClient, work: Promise<T>): Promise<T> {
    const settled = work.then((): void => {}, (): void => {});
    client.pending = Promise.all([client.pending, settled]).then((): void => {});
    return client.scope.wait(work);
  }
  private async respond(client: HttpClient, text: string, boundary: number): Promise<void> {
    let request: HttpRequest;
    try {
      if (boundary + 4 !== text.length) { throw new Error('Request body or pipelining'); }
      request = parseRequest(text);
      if (!request.target.startsWith('/' + this.secret + '/') || request.headers['host'] !== client.origin.local?.substring(7)) {
        this.rejected++; await this.header(client, '403 Forbidden', 'Content-Length: 0\r\n'); return;
      }
    } catch (_) { this.rejected++; await this.header(client, '400 Bad Request', 'Content-Length: 0\r\n'); return; }
    // The suffix is an origin path, including a possible leading double slash.
    const uri = new url.URL(client.origin.remote + request.target.substring(this.secret.length + 1)).toString();
    this.reads++;
    try {
      let info = await this.physical(client, this.provider.metadata(uri, client.token)); client.scope.check();
      const manifest = /mpegurl|dash\+xml/i.test(info.contentType) || /\.(m3u8|mpd)(?:[?#]|$)/i.test(uri);
      let snapshot: ArrayBuffer | undefined;
      if (manifest && this.provider.snapshot !== undefined) {
        const value = await this.physical(client, this.provider.snapshot(uri, MANIFEST_BYTES, client.token)); client.scope.check();
        info = value.info; snapshot = value.bytes;
      }
      if (!Number.isSafeInteger(info.totalBytes) || info.totalBytes < 1 || info.totalBytes > RESOURCE_BYTES || /[\r\n\0]/.test(info.contentType)) {
        throw new VesperPlayerError('invalid_response', 'Playback HTTP resource metadata is invalid');
      }
      if (info.finalUrl !== uri) {
        const location = await this.localUrl(info.finalUrl); client.scope.check();
        await this.header(client, '302 Found', `Content-Length: 0\r\nLocation: ${location}\r\n`); return;
      }
      const body = manifest ? await this.manifest(uri, info, client, snapshot) : undefined; client.scope.check();
      const total = body?.byteLength ?? info.totalBytes;
      const range = resourceResponseRange(request.headers['range'], total);
      if (range === undefined) { await this.header(client, '416 Range Not Satisfiable', `Content-Length: 0\r\nContent-Range: bytes */${total}\r\n`); return; }
      const partial = request.headers['range'] !== undefined;
      await this.header(client, partial ? '206 Partial Content' : '200 OK',
        `Content-Type: ${info.contentType}\r\nContent-Length: ${range.end - range.start + 1}\r\nAccept-Ranges: bytes\r\n` +
        (partial ? `Content-Range: bytes ${range.start}-${range.end}/${total}\r\n` : ''));
      if (request.method === 'HEAD') { this.completed++; return; }
      let offset = range.start;
      while (offset <= range.end) {
        client.scope.check(); const end = Math.min(range.end, offset + CHUNK_BYTES - 1);
        const bytes = body === undefined ? await this.physical(client, this.provider.read(uri, { start: offset, end: end }, client.token)) : body.slice(offset, end + 1);
        client.scope.check();
        if (bytes.byteLength < 1 || bytes.byteLength > end - offset + 1) { throw new VesperPlayerError('invalid_response', 'Playback HTTP resource returned an invalid interval'); }
        await this.physical(client, client.connection.send({ data: bytes }));
        offset += bytes.byteLength; this.deliveredBytes += bytes.byteLength;
      }
      this.completed++;
    } catch (error) {
      if (!client.token.isCancelled && !this.disposed && !client.sentHeader) { await this.header(client, '502 Bad Gateway', 'Content-Length: 0\r\n').catch((): void => {}); }
      throw error as Error;
    }
  }
  private async manifest(uri: string, info: ResourceInfo, client: HttpClient, snapshot?: ArrayBuffer): Promise<ArrayBuffer> {
    if (info.totalBytes > MANIFEST_BYTES) { throw new VesperPlayerError('capacity', 'Playback manifest exceeds 256 KiB'); }
    const output = snapshot === undefined ? new Uint8Array(info.totalBytes) : new Uint8Array(snapshot);
    if (output.byteLength !== info.totalBytes) { throw new VesperPlayerError('invalid_response', 'Manifest snapshot length is inconsistent'); }
    let offset = snapshot === undefined ? 0 : info.totalBytes;
    while (offset < info.totalBytes) {
      const bytes = await this.physical(client, this.provider.read(uri, { start: offset, end: Math.min(info.totalBytes, offset + CHUNK_BYTES) - 1 }, client.token));
      client.scope.check();
      if (bytes.byteLength < 1 || bytes.byteLength > output.byteLength - offset) { throw new VesperPlayerError('invalid_response', 'Playback manifest ended unexpectedly'); }
      output.set(new Uint8Array(bytes), offset); offset += bytes.byteLength;
    }
    const text = util.TextDecoder.create('utf-8', { fatal: true }).decodeWithStream(output);
    const protocol = /\.m3u8(?:[?#]|$)/i.test(uri) || /mpegurl/i.test(info.contentType) ? 'hls' : /\.mpd(?:[?#]|$)/i.test(uri) || /dash\+xml/i.test(info.contentType) ? 'dash' : this.protocol;
    const first = this.bridge.invoke<ManifestProjection>(JSON.stringify({ protocol: protocol, manifest: text, manifestUrl: uri }));
    const routes: Record<string, string> = {};
    for (const remote of first.origins) { routes[remote] = await this.origin(remote) + '/' + this.secret; client.scope.check(); }
    const projected = this.bridge.invoke<ManifestProjection>(JSON.stringify({ protocol: protocol, manifest: text, manifestUrl: uri, routes: routes }));
    for (const codec of projected.excludedCodecs) { if (this.excluded.size < 16) { this.excluded.add(codec); } }
    return encoded(projected.manifest);
  }
  private closeClient(client: HttpClient): Promise<void> {
    client.token.cancel(); client.scope.close();
    if (client.closed) { return client.pending.then((): void => { this.clients.delete(client); }); }
    if (client.closing !== undefined) { return client.closing; }
    const work = Promise.resolve().then((): Promise<void> => client.connection.close()).catch((error: Error): void => {
      const failure = nativeError(error, 'playback HTTP close');
      const raw = `${failure.nativeCode ?? failure.code}`;
      const code = this.closeErrors[raw] !== undefined || Object.keys(this.closeErrors).length < 16 ? raw : 'other';
      this.closeErrors[code] = (this.closeErrors[code] ?? 0) + 1;
      if (!client.closed) { throw error; }
    }).then(async (): Promise<void> => {
      await client.pending;
      client.closed = true; this.clients.delete(client);
    });
    client.closing = work;
    work.catch((): void => { client.closing = undefined; this.failures++; });
    return work;
  }
  /** Joins listener creation, response work and physical socket closure before lease release. */
  async dispose(): Promise<void> {
    this.disposed = true;
    for (const client of this.clients) { client.token.cancel(); }
    const listeners = Array.from(this.origins.values()).map(async (entry: OriginListener): Promise<void> => {
      await entry.ready.catch((): string => '');
      if (!entry.closed) { await entry.server.close(); entry.closed = true; }
    });
    // Observe every listener failure while connections are being closed.
    const joined = Promise.all(listeners); joined.catch((): void => {});
    for (const client of Array.from(this.clients)) {
      await this.closeClient(client); await client.work;
    }
    await joined;
    this.origins.clear(); this.bridge.close(); this.secret = '';
  }
  activeResources(): number { return this.clients.size; }
  inventory(): string { return JSON.stringify({ route: 'native-http', reads: this.reads, deliveredBytes: this.deliveredBytes,
    resources: this.clients.size, listeners: this.origins.size, completed: this.completed, failures: this.failures,
    rejected: this.rejected, closeErrors: this.closeErrors, requestErrors: this.requestErrors, excludedCodecs: Array.from(this.excluded),
    provider: this.provider.inventory?.() }); }
}
