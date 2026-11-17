import { media } from '@kit.MediaKit';
import { OperationScope, VesperCancellationToken, nativeError } from './Operation';
import { StartupCache, ResourceInfo } from './StartupCache';
import { ByteRange, Transport } from './Transport';
import { VesperPlayerError } from './Types';
import { url, util } from '@kit.ArkTS';
import { fileIo as fs } from '@kit.CoreFileKit';
import { relativeFile, regularFile, assetFile } from './FileStore';

export interface ResourceProvider {
  metadata(uri: string, cancellation: VesperCancellationToken): Promise<ResourceInfo>;
  read(uri: string, range: ByteRange, cancellation: VesperCancellationToken): Promise<ArrayBuffer>;
  snapshot?(uri: string, maximumBytes: number, cancellation: VesperCancellationToken): Promise<ResourceSnapshot>;
  inventory?(): string;
}
export interface ResourceSnapshot { info: ResourceInfo; bytes: ArrayBuffer; }
interface PrimedHeader { info: ResourceInfo; bytes: ArrayBuffer; }

/** A local adaptive asset has no network fallback, including absolute child URLs. */
export class FileResourceProvider implements ResourceProvider {
  readonly nativeUrl: string;
  private root: string;
  constructor(entry: string) {
    const separator = entry.lastIndexOf('/');
    if (separator < 0) { throw new VesperPlayerError('invalid_path', 'Offline manifest must be an absolute sandbox path'); }
    this.root = entry.substring(0, separator);
    this.nativeUrl = `http://vesper.offline/${relativeFile(entry.substring(separator + 1))}`;
  }
  private path(uri: string): Promise<string> {
    const parsed = new url.URL(uri);
    if (parsed.origin !== 'http://vesper.offline' || parsed.search.length > 0 || parsed.hash.length > 0) {
      throw new VesperPlayerError('offline_resource', 'Offline assets cannot access an external resource');
    }
    return assetFile(this.root, relativeFile(parsed.pathname.substring(1)));
  }
  async metadata(uri: string, cancellation: VesperCancellationToken): Promise<ResourceInfo> {
    if (cancellation.isCancelled) { throw new VesperPlayerError('cancelled', 'Offline read was cancelled'); }
    const total = await regularFile(await this.path(uri), 1024 * 1024 * 1024);
    const type = uri.endsWith('.mpd') ? 'application/dash+xml' : uri.endsWith('.m3u8') ? 'application/vnd.apple.mpegurl'
      : uri.endsWith('.ts') ? 'video/mp2t' : uri.endsWith('.mp4') || uri.endsWith('.m4s') ? 'video/mp4'
      : uri.endsWith('.vtt') ? 'text/vtt' : 'application/octet-stream';
    return { totalBytes: total, contentType: type, finalUrl: uri };
  }
  async read(uri: string, range: ByteRange, cancellation: VesperCancellationToken): Promise<ArrayBuffer> {
    if (cancellation.isCancelled) { throw new VesperPlayerError('cancelled', 'Offline read was cancelled'); }
    const file = await fs.open(await this.path(uri), fs.OpenMode.READ_ONLY | fs.OpenMode.NOFOLLOW);
    try {
      const buffer = new ArrayBuffer(range.end - range.start + 1);
      const count = await fs.read(file.fd, buffer, { offset: range.start, length: buffer.byteLength });
      if (count < 1 || count > buffer.byteLength) { throw new VesperPlayerError('offline_resource', 'Offline read ended unexpectedly'); }
      return buffer.slice(0, count);
    } finally { await fs.close(file); }
  }
}

/** Each playback instance owns a provider and the registration's accepted access context. */
export class NetworkResourceProvider implements ResourceProvider {
  private transport: Transport;
  private cache: StartupCache;
  private headers: Record<string, string>;
  private origin: string;
  cacheBytes: number = 0;
  timeline: string = 'unknown';
  private versions: Map<string, ResourceInfo> = new Map();
  // Source-local initialization/SIDX retention: at most sixteen 64 KiB prefixes.
  private primed: Map<string, PrimedHeader> = new Map();
  constructor(transport: Transport, cache: StartupCache, headers: Record<string, string>, sourceUri: string) {
    this.transport = transport; this.cache = cache; this.headers = headers; this.origin = new url.URL(sourceUri).origin;
  }
  private validateScope(uri: string): void {
    if (Object.keys(this.headers).length > 0 && new url.URL(uri).origin !== this.origin) {
      throw new VesperPlayerError('access_scope', 'Authenticated child resources require the accepted source origin');
    }
  }
  private diskEligible(uri: string, info?: ResourceInfo): boolean {
    return this.timeline !== 'live' && !/\.(?:m3u8|mpd)(?:[?#]|$)/i.test(uri) && !/mpegurl|dash\+xml/i.test(info?.contentType ?? '');
  }
  private remember(uri: string, info: ResourceInfo): ResourceInfo {
    if (this.versions.size >= 64 && !this.versions.has(uri)) { const first = Array.from(this.versions.keys())[0]; this.versions.delete(first); }
    this.versions.set(uri, info); return info;
  }
  /** A mutable manifest is one complete HTTP response, including its own length. */
  async snapshot(uri: string, maximumBytes: number, cancellation: VesperCancellationToken): Promise<ResourceSnapshot> {
    this.validateScope(uri);
    const response = await this.transport.get(uri, { headers: this.headers, maxBytes: maximumBytes, cancellation: cancellation });
    if (response.status !== 200 || response.bytes.byteLength < 1 ||
      response.totalBytes !== undefined && response.totalBytes !== response.bytes.byteLength) {
      throw new VesperPlayerError('invalid_response', 'Manifest snapshot is not a complete HTTP response');
    }
    const info: ResourceInfo = { totalBytes: response.bytes.byteLength, contentType: response.contentType,
      finalUrl: response.finalUrl, etag: response.etag };
    if (/\.m3u8(?:[?#]|$)/i.test(uri) || /mpegurl/i.test(info.contentType)) {
      const text = util.TextDecoder.create('utf-8', { fatal: true }).decodeWithStream(new Uint8Array(response.bytes));
      if (text.startsWith('#EXTM3U') && text.includes('#EXT-X-TARGETDURATION')) { this.timeline = text.includes('#EXT-X-ENDLIST') ? 'vod' : 'live'; }
    }
    return { info: this.remember(uri, info), bytes: response.bytes };
  }
  async metadata(uri: string, cancellation: VesperCancellationToken): Promise<ResourceInfo> {
    this.validateScope(uri);
    const primed = this.primed.get(uri); if (primed !== undefined) { return this.remember(uri, primed.info); }
    const memory = this.cache.metadata(uri); if (memory !== undefined) { return this.remember(uri, memory); }
    const cached = this.diskEligible(uri) ? await this.cache.diskMetadata(uri).catch(() => undefined) : undefined;
    if (cached !== undefined && this.diskEligible(uri, cached)) { return this.remember(uri, cached); }
    const response = await this.transport.get(uri, { headers: this.headers, maxBytes: 1,
      range: { start: 0, end: 0 }, cancellation: cancellation, allowFullResponse: true });
    if (response.totalBytes === undefined || response.totalBytes < 1) { throw new VesperPlayerError('invalid_response', 'Resource length is unavailable'); }
    const info: ResourceInfo = { totalBytes: response.totalBytes, contentType: response.contentType,
      finalUrl: response.finalUrl, etag: response.etag };
    return this.remember(uri, info);
  }
  async read(uri: string, range: ByteRange, cancellation: VesperCancellationToken): Promise<ArrayBuffer> {
    this.validateScope(uri);
    const primed = this.primed.get(uri);
    if (primed !== undefined && range.start < primed.bytes.byteLength) {
      const bytes = primed.bytes.slice(range.start, Math.min(range.end + 1, primed.bytes.byteLength));
      this.cacheBytes += bytes.byteLength; return bytes;
    }
    const cached = this.cache.read(uri, range.start, range.end - range.start + 1);
    if (cached !== undefined) { this.cacheBytes += cached.byteLength; return cached; }
    const version = this.versions.get(uri);
    const disk = this.diskEligible(uri, version) ? await this.cache.diskRead(uri, range.start, range.end - range.start + 1).catch(() => undefined) : undefined;
    if (disk !== undefined) { this.cacheBytes += disk.byteLength; return disk; }
    const response = await this.transport.get(uri, { headers: this.headers,
      maxBytes: range.end - range.start + 1, range: range, cancellation: cancellation, allowFullResponse: true,
      validator: this.diskEligible(uri, version) ? version?.etag : undefined });
    if (version !== undefined && this.diskEligible(uri, version) && (version.totalBytes !== response.totalBytes || version.etag !== response.etag)) {
      throw new VesperPlayerError('resource_changed', 'Resource version changed during native playback');
    }
    if (range.start === 0 && response.totalBytes === response.bytes.byteLength
      && (/\.m3u8(?:[?#]|$)/.test(uri) || /mpegurl/i.test(response.contentType))) {
      const manifest = util.TextDecoder.create('utf-8', { fatal: true }).decodeWithStream(new Uint8Array(response.bytes));
      if (manifest.startsWith('#EXTM3U') && manifest.includes('#EXT-X-TARGETDURATION')) { this.timeline = manifest.includes('#EXT-X-ENDLIST') ? 'vod' : 'live'; }
    }
    if (response.totalBytes !== undefined) {
      await this.cache.diskWrite(uri, range.start, response.bytes, { totalBytes: response.totalBytes, finalUrl: response.finalUrl,
        contentType: response.contentType, etag: response.etag }).catch(() => {});
    }
    return response.bytes;
  }
  async prime(uri: string, maximum: number, cancellation: VesperCancellationToken): Promise<void> {
    this.validateScope(uri);
    if (!Number.isSafeInteger(maximum) || maximum < 1 || maximum > 65536) {
      throw new VesperPlayerError('invalid_argument', 'Audio initialization/SIDX prefix exceeds 64 KiB');
    }
    if (cancellation.isCancelled) { throw new VesperPlayerError('cancelled', 'Audio header preparation was cancelled'); }
    const ready = this.primed.get(uri); if (ready !== undefined && ready.bytes.byteLength >= maximum) { return; }
    const info = await this.metadata(uri, cancellation);
    const output = new Uint8Array(Math.min(maximum, info.totalBytes)); let offset = 0;
    while (offset < output.byteLength) {
      const bytes = await this.read(uri, { start: offset, end: output.byteLength - 1 }, cancellation);
      if (cancellation.isCancelled) { throw new VesperPlayerError('cancelled', 'Audio header preparation was cancelled'); }
      if (bytes.byteLength < 1 || bytes.byteLength > output.byteLength - offset) {
        throw new VesperPlayerError('invalid_response', 'Audio initialization/SIDX ended unexpectedly');
      }
      output.set(new Uint8Array(bytes), offset); offset += bytes.byteLength;
    }
    if (this.primed.size >= 16 && !this.primed.has(uri)) { this.primed.delete(Array.from(this.primed.keys())[0]); }
    this.primed.set(uri, { info: info, bytes: output.buffer as ArrayBuffer });
  }
}

interface ReadRequest { offset: number; length: number; }
interface LoaderObservation { action: string; id: number; offset?: number; length?: number; accepted?: number; nativeRange?: string; }
interface OpenResource {
  request: media.MediaSourceLoadingRequest;
  cancellation: VesperCancellationToken;
  queue: ReadRequest[];
  running: boolean;
  info?: ResourceInfo;
  lastBuffer?: ArrayBuffer;
  nativeRange?: string;
}

/** Native callbacks return immediately. Every resource/read/response is bounded. */
export class NativeResourceLoader {
  private provider: ResourceProvider;
  private report: (error: VesperPlayerError) => void;
  private resources: Map<number, OpenResource> = new Map();
  private nextId: number = 1;
  private disposed: boolean = false;
  reads: number = 0;
  deliveredBytes: number = 0;
  private headers: number = 0;
  private finished: number = 0;
  private responses: Record<string, number> = {};
  private observations: LoaderObservation[] = [];
  private observe(value: LoaderObservation): void {
    if (this.observations.length >= 32) { this.observations.shift(); }
    this.observations.push(value);
  }
  constructor(provider: ResourceProvider, report: (error: VesperPlayerError) => void) {
    this.provider = provider; this.report = report;
  }
  delegate(): media.MediaSourceLoader {
    // The native bridge invokes functions without a JavaScript receiver.
    return { open: (request: media.MediaSourceLoadingRequest): number => this.open(request),
      read: (id: number, offset: number, length: number): void => this.read(id, offset, length),
      close: (id: number): void => this.close(id) };
  }
  private open(request: media.MediaSourceLoadingRequest): number {
    if (this.disposed || this.resources.size >= 16 || this.nextId >= Number.MAX_SAFE_INTEGER) {
      this.report(new VesperPlayerError('capacity', 'Resource loader capacity reached')); return -1;
    }
    const id = this.nextId++;
    const rangeKey = Object.keys(request.header ?? {}).find((key: string) => key.toLowerCase() === 'range');
    const rangeValue = rangeKey === undefined ? undefined : request.header?.[rangeKey];
    const nativeRange = rangeValue !== undefined && /^bytes=\d{1,16}-\d{0,16}$/.test(rangeValue) ? rangeValue : undefined;
    this.resources.set(id, { request: request, cancellation: new VesperCancellationToken(), queue: [], running: false, nativeRange: nativeRange });
    this.observe({ action: 'open', id: id });
    return id;
  }
  private read(id: number, offset: number, length: number): void {
    const state = this.resources.get(id);
    if (state === undefined || this.disposed) { return; }
    if (!Number.isSafeInteger(offset) || offset < 0 || !Number.isSafeInteger(length) || length < -1
      || (length >= 0 && !Number.isSafeInteger(offset + length)) || state.queue.length >= 4) {
      this.reject(id, state, new VesperPlayerError('invalid_request', 'Resource read is invalid or its queue is full')); return;
    }
    state.queue.push({ offset: offset, length: length }); this.reads++;
    this.observe({ action: 'read', id: id, offset: offset, length: length, nativeRange: state.nativeRange });
    if (!state.running) { state.running = true; setTimeout(() => { this.process(id, state); }, 0); }
  }
  private async process(id: number, state: OpenResource): Promise<void> {
    const scope = new OperationScope(10000, state.cancellation);
    try {
      if (this.resources.get(id) !== state) { return; }
      if (state.info === undefined) {
        state.info = await scope.wait(this.provider.metadata(state.request.url, state.cancellation));
        scope.check();
      }
      while (state.queue.length > 0) {
        const read = state.queue.shift();
        if (read === undefined) { break; }
        let offset = read.offset;
        const total = state.info.totalBytes;
        const end = read.length === -1 ? total : Math.min(offset + read.length, total);
        if (offset > total) { throw new VesperPlayerError('range_mismatch', 'Resource read starts beyond EOF'); }
        let steps = 0;
        let sentHeader = false;
        while (offset < end) {
          scope.check();
          const range: ByteRange = { start: offset, end: Math.min(offset + 65536, end) - 1 };
          let bytes = await scope.wait(this.provider.read(state.request.url, range, state.cancellation));
          if (bytes.byteLength < 1 || bytes.byteLength > range.end - range.start + 1) { throw new VesperPlayerError('invalid_response', 'Resource provider returned an invalid interval'); }
          let consumed = 0;
          while (consumed < bytes.byteLength) {
            scope.check();
            if (this.resources.get(id) !== state) { return; }
            const offered = consumed === 0 ? bytes : bytes.slice(consumed);
            if (!sentHeader) {
              // Native can replace a pending read while network data is in
              // flight. Pair its response header with the first delivered byte.
              const redirect = state.info.finalUrl === state.request.url ? undefined : state.info.finalUrl;
              const header: Record<string, string> = { 'content-length': `${end - read.offset}`,
                'Content-Type': state.info.contentType, 'Accept-Ranges': 'bytes' };
              if (read.offset > 0 || end < total) { header['Content-Range'] = `bytes ${read.offset}-${end - 1}/${total}`; }
              state.request.respondHeader(id, header, redirect);
              sentHeader = true;
              this.headers++;
            }
            // respondData synchronously copies into native shared memory on the
            // supported SDK route. Only the latest bounded buffer is retained.
            state.lastBuffer = offered;
            const accepted = state.request.respondData(id, offset, offered);
            this.observe({ action: 'data', id: id, offset: offset, length: offered.byteLength, accepted: accepted });
            const result = accepted > 0 ? 'positive' : `${accepted}`;
            this.responses[result] = (this.responses[result] ?? 0) + 1;
            if (accepted === 0 || accepted === -2 || accepted === -3) { break; }
            if (accepted < 0 || accepted > offered.byteLength) { throw new VesperPlayerError('native_failure', 'Native resource delivery failed', accepted); }
            offset += accepted; consumed += accepted; this.deliveredBytes += accepted;
            if (++steps >= 16) { steps = 0; await scope.wait(new Promise<void>((resolve) => { setTimeout(resolve, 0); })); }
          }
          if (consumed < bytes.byteLength) { break; }
        }
        if (offset >= total) { state.request.finishLoading(id, media.LoadingRequestError.LOADING_ERROR_SUCCESS); this.finished++; this.observe({ action: 'finished', id: id, offset: offset }); }
      }
    } catch (error) {
      if (this.resources.get(id) === state && !state.cancellation.isCancelled) { this.reject(id, state, nativeError(error, 'resource loading')); }
    } finally { scope.close(); state.running = false; }
  }
  private reject(id: number, state: OpenResource, error: VesperPlayerError): void {
    this.report(error);
    state.cancellation.cancel(); state.queue = [];
    try { state.request.finishLoading(id, media.LoadingRequestError.LOADING_ERROR_NO_RESOURCE); } catch (_) {}
  }
  private close(id: number): void {
    const state = this.resources.get(id);
    if (state === undefined) { return; }
    this.resources.delete(id); state.cancellation.cancel(); state.queue = []; state.lastBuffer = undefined;
    this.observe({ action: 'close', id: id });
  }
  /** Called only after physical AVPlayer release, or before it has accepted this loader. */
  dispose(): void {
    if (this.disposed) { return; }
    this.disposed = true;
    for (const id of Array.from(this.resources.keys())) { this.close(id); }
  }
  activeResources(): number { return this.resources.size; }
  inventory(): string { return JSON.stringify({ reads: this.reads, deliveredBytes: this.deliveredBytes, resources: this.resources.size,
    headers: this.headers, finished: this.finished, responses: this.responses, observations: this.observations }); }
}
