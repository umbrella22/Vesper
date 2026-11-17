import { rcp } from '@kit.RemoteCommunicationKit';
import { url } from '@kit.ArkTS';
import { OperationScope, VesperCancellationToken, monotonicMs, nativeError } from './Operation';
import { VesperPlayerError } from './Types';
import { VesperDiagnosticRequest, diagnosticCode } from './DiagnosticReport';

export interface ByteRange { start: number; end: number; }
export interface TransportOptions {
  headers: Record<string, string>;
  maxBytes: number;
  range?: ByteRange;
  timeoutMs?: number;
  cancellation?: VesperCancellationToken;
  validator?: string;
  allowFullResponse?: boolean;
  attempts?: number;
}

export class TransportResponse {
  bytes: ArrayBuffer;
  finalUrl: string;
  status: number;
  totalBytes?: number;
  contentType: string;
  etag?: string;
  constructor(bytes: ArrayBuffer, finalUrl: string, response: rcp.Response) {
    this.bytes = bytes; this.finalUrl = finalUrl; this.status = response.statusCode;
    this.contentType = singleHeader(response.headers, 'content-type') ?? 'application/octet-stream';
    this.etag = singleHeader(response.headers, 'etag');
    const range = singleHeader(response.headers, 'content-range');
    const match = range?.match(/^bytes (\d+)-(\d+)\/(\d+)$/);
    const length = singleHeader(response.headers, 'content-length');
    if (match !== undefined && match !== null) { this.totalBytes = Number(match[3]); }
    else if (length !== undefined && this.status === 200) { this.totalBytes = Number(length); }
    if (this.totalBytes !== undefined && (!Number.isSafeInteger(this.totalBytes) || this.totalBytes < 0)) {
      throw new VesperPlayerError('invalid_response', 'Resource length is not a safe byte count');
    }
  }
}

function singleHeader(headers: rcp.ResponseHeaders, name: string): string | undefined {
  const value = headers[name];
  if (typeof value === 'string') { return value; }
  if (value === undefined) { return undefined; }
  throw new VesperPlayerError('invalid_response', 'Ambiguous response header');
}

interface TransportJob { run: () => void; }

/** Four physical requests and sixteen pending jobs, with bounded body buffers. */
export class Transport {
  private defaultAttempts: number = 1;
  private active: number = 0;
  private pending: TransportJob[] = [];
  requests: number = 0;
  bytesReceived: number = 0;
  private recent: VesperDiagnosticRequest[] = [];
  droppedDiagnosticRequests: number = 0;
  diagnosticRequests(): VesperDiagnosticRequest[] { return this.recent.map((value: VesperDiagnosticRequest): VesperDiagnosticRequest => ({
    requestId: value.requestId, atMonotonicMs: value.atMonotonicMs, durationMs: value.durationMs, status: value.status,
    rangeStart: value.rangeStart, rangeEnd: value.rangeEnd, receivedBytes: value.receivedBytes, outcome: value.outcome })); }
  private observeRequest(id: number, started: number, options: TransportOptions, size: number, status?: number, outcome: string = 'response'): void {
    if (this.recent.length >= 32) { this.recent.shift(); this.droppedDiagnosticRequests++; }
    this.recent.push({ requestId: id, atMonotonicMs: started, durationMs: monotonicMs() - started,
      status: status, rangeStart: options.range?.start, rangeEnd: options.range?.end, receivedBytes: size, outcome: outcome });
  }
  setDefaultAttempts(attempts: number): void {
    if (!Number.isInteger(attempts) || attempts < 1 || attempts > 3) { throw new VesperPlayerError('invalid_argument', 'Transport attempts must be between one and three'); }
    this.defaultAttempts = attempts;
  }
  configuredAttempts(): number { return this.defaultAttempts; }
  get(input: string, options: TransportOptions): Promise<TransportResponse> {
    options = { headers: options.headers, maxBytes: options.maxBytes, range: options.range, timeoutMs: options.timeoutMs,
      cancellation: options.cancellation, validator: options.validator, allowFullResponse: options.allowFullResponse,
      attempts: options.attempts ?? this.defaultAttempts };
    if (!Number.isSafeInteger(options.maxBytes) || options.maxBytes < 1 || options.maxBytes > 1024 * 1024) {
      return Promise.reject(new VesperPlayerError('invalid_argument', 'Transport body limit must be between 1 byte and 1 MiB'));
    }
    if (options.range !== undefined && (!Number.isSafeInteger(options.range.start) || !Number.isSafeInteger(options.range.end)
      || options.range.start < 0 || options.range.end < options.range.start || options.range.end - options.range.start + 1 > options.maxBytes)) {
      return Promise.reject(new VesperPlayerError('invalid_argument', 'Transport byte range is invalid'));
    }
    if (this.pending.length >= 16) { return Promise.reject(new VesperPlayerError('capacity', 'Network request queue is full')); }
    if (!Number.isInteger(options.attempts ?? 1) || (options.attempts ?? 1) < 1 || (options.attempts ?? 1) > 3) {
      return Promise.reject(new VesperPlayerError('invalid_argument', 'Transport attempts must be between one and three'));
    }
    const scope = new OperationScope(options.timeoutMs ?? 10000, options.cancellation);
    const physical = new Promise<TransportResponse>((resolve, reject) => {
      const job: TransportJob = { run: (): void => {
        this.active++;
        this.attempt(input, options, scope).then(resolve, reject).finally(() => {
          this.active--; scope.close(); this.pump();
        });
      } };
      this.pending.push(job); this.pump();
    });
    // Return physical completion. An outer operation can settle its public
    // promise early while retaining this slot and buffer until native exits.
    return physical;
  }
  private pump(): void {
    while (this.active < 4 && this.pending.length > 0) { this.pending.shift()?.run(); }
  }
  private async attempt(input: string, options: TransportOptions, scope: OperationScope): Promise<TransportResponse> {
    for (let attempt = 0; attempt < (options.attempts ?? 1); attempt++) {
      try { return await this.perform(input, options, scope); }
      catch (error) {
        scope.check(); const failure = error as VesperPlayerError;
        if (attempt + 1 >= (options.attempts ?? 1) || (failure.code !== undefined && !/^http_(?:408|429|5\d\d)$/.test(failure.code))) { throw error as Error; }
        await scope.wait(new Promise<void>((resolve) => { setTimeout(resolve, 200 * (attempt + 1)); }));
      }
    }
    throw new VesperPlayerError('network_failure', 'Network retry limit reached');
  }
  private async perform(input: string, options: TransportOptions, scope: OperationScope): Promise<TransportResponse> {
    scope.check();
    let current = new url.URL(input);
    const session = rcp.createSession({ requestConfiguration: {
      transfer: { autoRedirect: false, timeout: { connectMs: 5000, transferMs: 10000 } },
      proxy: 'system'
    } });
    const remove = scope.onFailure(() => { session.cancel(); });
    let received = 0;
    try {
      for (let hop = 0; hop <= 5; hop++) {
        scope.check();
        if (!['https:', 'http:'].includes(current.protocol) || current.username.length > 0 || current.password.length > 0) {
          throw new VesperPlayerError('unsupported', 'Network URL scheme or embedded credentials are unsupported');
        }
        const headers: Record<string, string> = {};
        for (const name of Object.keys(options.headers)) { headers[name] = options.headers[name]; }
        headers['Accept-Encoding'] = 'identity';
        if (options.range !== undefined) { headers['Range'] = `bytes=${options.range.start}-${options.range.end}`; }
        if (options.validator !== undefined) { headers['If-Range'] = options.validator; }
        const request = new rcp.Request(current.toString(), 'GET', headers);
        const bodyLimit = options.range !== undefined && options.allowFullResponse ? 1024 * 1024 : options.maxBytes;
        const output = new Uint8Array(bodyLimit);
        let size = 0;
        let physicalReceived = 0;
        request.destination = (chunk: ArrayBuffer): void => {
          physicalReceived += chunk.byteLength;
          received += chunk.byteLength;
          if (received > bodyLimit) { scope.fail(new VesperPlayerError('capacity', 'Network response exceeded its body budget')); return; }
          output.set(new Uint8Array(chunk), size); size += chunk.byteLength;
        };
        const requestId = ++this.requests; const started = monotonicMs();
        let response: rcp.Response;
        try { response = await session.fetch(request); this.observeRequest(requestId, started, options, physicalReceived, response.statusCode); }
        catch (error) { this.observeRequest(requestId, started, options, physicalReceived, undefined, diagnosticCode(nativeError(error, 'request').code) ?? 'unavailable'); throw error as Error; }
        scope.check();
        this.bytesReceived += size;
        if ([301, 302, 303, 307, 308].includes(response.statusCode)) {
          const location = singleHeader(response.headers, 'location');
          if (location === undefined || hop === 5) { throw new VesperPlayerError('redirect', 'Redirect target is missing or the hop limit was reached'); }
          const next = new url.URL(location, current.toString());
          if (current.protocol === 'https:' && next.protocol !== 'https:') { throw new VesperPlayerError('redirect', 'TLS downgrade is forbidden'); }
          if (next.origin !== current.origin && Object.keys(options.headers).length > 0) {
            throw new VesperPlayerError('redirect', 'Authenticated cross-origin redirects require a new access context');
          }
          current = next; continue;
        }
        if (response.statusCode !== 200 && response.statusCode !== 206) {
          throw new VesperPlayerError(`http_${response.statusCode}`, 'Network resource request was rejected');
        }
        const encoding = singleHeader(response.headers, 'content-encoding');
        if (encoding !== undefined && encoding.toLowerCase() !== 'identity') { throw new VesperPlayerError('invalid_response', 'Encoded byte ranges are unsupported'); }
        if (options.range !== undefined) {
          const expected = options.range;
          if (response.statusCode === 200 && options.allowFullResponse) {
            const length = singleHeader(response.headers, 'content-length');
            if (length === undefined || Number(length) !== size || expected.end >= size) { throw new VesperPlayerError('range_mismatch', 'Ignored Range response does not contain the requested interval'); }
            if (options.validator !== undefined && singleHeader(response.headers, 'etag') !== options.validator) {
              throw new VesperPlayerError('resource_changed', 'If-Range response belongs to a new resource version');
            }
            return new TransportResponse(output.buffer.slice(expected.start, expected.end + 1) as ArrayBuffer, current.toString(), response);
          }
          const actual = singleHeader(response.headers, 'content-range')?.match(/^bytes (\d+)-(\d+)\/(\d+)$/);
          if (response.statusCode !== 206 || actual === undefined || actual === null
            || Number(actual[1]) !== expected.start || Number(actual[2]) !== expected.end || Number(actual[3]) <= expected.end
            || !Number.isSafeInteger(Number(actual[3])) || size !== expected.end - expected.start + 1) {
            throw new VesperPlayerError('range_mismatch', 'Server did not return the exact requested interval');
          }
        }
        return new TransportResponse(output.buffer.slice(0, size) as ArrayBuffer, current.toString(), response);
      }
      throw new VesperPlayerError('redirect', 'Redirect hop limit reached');
    } finally { remove(); session.close(); }
  }
  inventory(): string { return JSON.stringify({ active: this.active, pending: this.pending.length, requests: this.requests, bytes: this.bytesReceived }); }
}
