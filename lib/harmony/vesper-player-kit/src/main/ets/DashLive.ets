import { url, util } from '@kit.ArkTS';
import { NativeSession } from './NativeSession';
import { ResourceProvider, ResourceSnapshot } from './ResourceLoader';
import { ResourceInfo } from './StartupCache';
import { ByteRange } from './Transport';
import { OperationScope, VesperCancellationToken, monotonicMs, nativeError } from './Operation';
import { VesperPlayerError } from './Types';

const MANIFEST_BYTES: number = 256 * 1024;
// Current and previous two-track windows, their init files and sixteen pinned clients.
const RETAINED_RESOURCES: number = 2 * (2 * 256 + 2) + 16;
interface LivePlaylist { uri: string; text: string; }
interface LiveMapping { uri: string; upstreamUri: string; }
interface LiveProjection { playlists: LivePlaylist[]; resources: LiveMapping[]; refreshMs: number; mediaSequences: string[]; }
interface LiveResource { upstreamUri: string; generation: number; pins: number; }
interface ResourcePin { resource: LiveResource; remove: () => void; }
interface ManifestKind { dynamic: boolean; }
export interface DashPlaybackPreparation { snapshot: ResourceSnapshot; live?: DashLiveResourceProvider; }

async function manifestSnapshot(provider: ResourceProvider, uri: string, token: VesperCancellationToken): Promise<ResourceSnapshot> {
  if (provider.snapshot !== undefined) { return provider.snapshot(uri, MANIFEST_BYTES, token); }
  const info = await provider.metadata(uri, token);
  if (!Number.isSafeInteger(info.totalBytes) || info.totalBytes < 1 || info.totalBytes > MANIFEST_BYTES) {
    throw new VesperPlayerError('capacity', 'DASH manifest exceeds 256 KiB');
  }
  const output = new Uint8Array(info.totalBytes); let offset = 0;
  while (offset < output.byteLength) {
    if (token.isCancelled) { throw new VesperPlayerError('cancelled', 'DASH preparation was cancelled'); }
    const bytes = await provider.read(uri, { start: offset, end: Math.min(output.byteLength, offset + 65536) - 1 }, token);
    if (bytes.byteLength < 1 || bytes.byteLength > output.byteLength - offset) { throw new VesperPlayerError('invalid_response', 'DASH manifest ended unexpectedly'); }
    output.set(new Uint8Array(bytes), offset); offset += bytes.byteLength;
  }
  return { info: info, bytes: output.buffer as ArrayBuffer };
}

function decode(bytes: ArrayBuffer): string { return util.TextDecoder.create('utf-8', { fatal: true }).decodeWithStream(new Uint8Array(bytes)); }

/** Classifies a complete snapshot in Rust, without rounding DASH ticks in JavaScript. */
export async function prepareDashPlayback(provider: ResourceProvider, uri: string, scope: OperationScope, allowUnverifiedLive: boolean = false): Promise<DashPlaybackPreparation> {
  const token = new VesperCancellationToken(); const remove = scope.onFailure((): void => { token.cancel(); });
  const bridge = new NativeSession('{"kind":"dashLive"}'); let live: DashLiveResourceProvider | undefined;
  try {
    const snapshot = await manifestSnapshot(provider, uri, token); scope.check();
    const kind = bridge.invoke<ManifestKind>(JSON.stringify({ op: 'classify', mpd: decode(snapshot.bytes), manifestUrl: snapshot.info.finalUrl }));
    if (!kind.dynamic) { bridge.close(); return { snapshot: snapshot }; }
    // The internal prototype is exercised by contract tests. The public route
    // stays closed: API 20 omitted separate audio and API 26 stopped refreshing it.
    if (!allowUnverifiedLive) { throw new VesperPlayerError('unsupported', 'Dynamic DASH has no verified native audio/video route on the supported Harmony runtimes'); }
    live = new DashLiveResourceProvider(provider, uri, bridge);
    live.initialize(snapshot); scope.check();
    return { snapshot: snapshot, live: live };
  } catch (error) {
    if (live !== undefined) { await live.dispose(); } else { bridge.close(); }
    throw error as Error;
  } finally { remove(); token.cancel(); }
}

/** Per-activation HLS projection. Compressed fMP4 bytes are forwarded unchanged. */
export class DashLiveResourceProvider implements ResourceProvider {
  readonly nativeUrl: string;
  private root: string;
  private upstream: ResourceProvider;
  private manifestUri: string;
  private bridge: NativeSession;
  private lifetime: VesperCancellationToken = new VesperCancellationToken();
  private playlists: Map<string, ArrayBuffer> = new Map();
  private resources: Map<string, LiveResource> = new Map();
  private pins: Map<VesperCancellationToken, ResourcePin> = new Map();
  private pending?: Promise<void>;
  private disposed: boolean = false;
  private generation: number = 0;
  private nextRefresh: number = 0;
  private refreshedAt: number = 0;
  private refreshMs: number = 2000;
  private refreshes: number = 0;
  private refreshFailures: number = 0;
  private consecutiveFailures: number = 0;
  private mediaSequences: string[] = [];
  private playlistSnapshots: Record<string, number> = {};
  private playlistSequences: Record<string, string> = {};
  private resourceOpens: Record<string, number> = {};
  constructor(upstream: ResourceProvider, manifestUri: string, bridge: NativeSession) {
    this.upstream = upstream; this.manifestUri = manifestUri; this.bridge = bridge;
    this.root = new url.URL(manifestUri).origin + '/.vesper-live/' + util.generateRandomUUID() + '/';
    this.nativeUrl = this.root + 'master.m3u8';
  }
  initialize(snapshot: ResourceSnapshot): void { this.apply(snapshot); }
  private check(token?: VesperCancellationToken): void {
    if (this.disposed || token?.isCancelled) { throw new VesperPlayerError('cancelled', 'Live DASH activation or request is closed'); }
  }
  private apply(snapshot: ResourceSnapshot): void {
    this.check();
    const projection = this.bridge.invoke<LiveProjection>(JSON.stringify({ op: 'refresh', mpd: decode(snapshot.bytes),
      manifestUrl: snapshot.info.finalUrl, virtualRoot: this.root }));
    const next = new Map<string, LiveResource>();
    for (const entry of this.resources) {
      if (entry[1].generation >= this.generation || entry[1].pins > 0) { next.set(entry[0], entry[1]); }
    }
    const generation = this.generation + 1;
    for (const mapping of projection.resources) {
      const existing = next.get(mapping.uri);
      if (existing !== undefined && existing.upstreamUri !== mapping.upstreamUri) {
        throw new VesperPlayerError('resource_changed', 'Live DASH resource identity changed');
      }
      if (existing !== undefined) { existing.generation = generation; }
      else { next.set(mapping.uri, { upstreamUri: mapping.upstreamUri, generation: generation, pins: 0 }); }
    }
    if (next.size > RETAINED_RESOURCES) { throw new VesperPlayerError('capacity', 'Live DASH retained-resource budget reached'); }
    const playlists = new Map<string, ArrayBuffer>();
    for (const playlist of projection.playlists) { playlists.set(playlist.uri, new util.TextEncoder().encodeInto(playlist.text).buffer as ArrayBuffer); }
    this.resources = next; this.playlists = playlists; this.generation = generation;
    this.refreshMs = projection.refreshMs; this.mediaSequences = projection.mediaSequences;
    this.refreshedAt = monotonicMs(); this.nextRefresh = this.refreshedAt + this.refreshMs;
    this.refreshes++; this.consecutiveFailures = 0;
  }
  private async refresh(): Promise<void> {
    this.check();
    if (monotonicMs() < this.nextRefresh) { return; }
    if (this.pending !== undefined) { return this.pending; }
    const work = Promise.resolve().then(async (): Promise<void> => {
      try {
        const snapshot = await manifestSnapshot(this.upstream, this.manifestUri, this.lifetime);
        this.check(); this.apply(snapshot);
      } catch (error) {
        this.check(); this.refreshFailures++; this.consecutiveFailures++;
        const failure = nativeError(error, 'live DASH refresh');
        const transient = ['timeout', 'native_failure', 'http_408', 'http_429', 'http_500', 'http_502', 'http_503', 'http_504'].includes(failure.code);
        // A short refresh outage may reuse the last coherent window. No stale
        // window is served after three failures or ten seconds without success.
        if (!transient || this.consecutiveFailures >= 3 || monotonicMs() - this.refreshedAt >= 10000) { throw failure; }
        this.nextRefresh = monotonicMs() + 1000;
      }
    });
    this.pending = work;
    try { await work; } finally { if (this.pending === work) { this.pending = undefined; } }
  }
  private resource(uri: string, token: VesperCancellationToken): LiveResource {
    this.check(token);
    const pinned = this.pins.get(token);
    if (pinned !== undefined) {
      if (this.resources.get(uri) !== pinned.resource) { throw new VesperPlayerError('invalid_request', 'Live DASH request changed resources'); }
      return pinned.resource;
    }
    const resource = this.resources.get(uri);
    if (resource === undefined) { throw new VesperPlayerError('http_410', 'Live DASH resource is outside the retained windows'); }
    if (this.pins.size >= 16) { throw new VesperPlayerError('capacity', 'Live DASH exceeds sixteen active resource requests'); }
    resource.pins++;
    const remove = token.observe((): void => { resource.pins--; this.pins.delete(token); });
    this.pins.set(token, { resource: resource, remove: remove });
    return resource;
  }
  async metadata(uri: string, token: VesperCancellationToken): Promise<ResourceInfo> {
    this.check(token);
    if (this.playlists.has(uri)) {
      await this.refresh(); this.check(token);
      return { finalUrl: uri, totalBytes: this.playlists.get(uri)?.byteLength ?? 0, contentType: 'application/vnd.apple.mpegurl' };
    }
    const resource = this.resource(uri, token);
    const kind = uri.startsWith(this.root + 'audio/') ? 'audio' : 'video';
    this.resourceOpens[kind] = (this.resourceOpens[kind] ?? 0) + 1;
    const info = await this.upstream.metadata(resource.upstreamUri, token); this.check(token);
    return { finalUrl: uri, totalBytes: info.totalBytes, contentType: info.contentType, etag: info.etag };
  }
  async snapshot(uri: string, maximumBytes: number, token: VesperCancellationToken): Promise<ResourceSnapshot> {
    this.check(token);
    if (!this.playlists.has(uri)) { throw new VesperPlayerError('invalid_request', 'Only generated live playlists have snapshots'); }
    await this.refresh(); this.check(token);
    const bytes = this.playlists.get(uri);
    if (bytes === undefined || bytes.byteLength > maximumBytes) { throw new VesperPlayerError('capacity', 'Live playlist exceeds its response budget'); }
    const name = uri === this.nativeUrl ? 'master' : uri === this.root + 'audio.m3u8' ? 'audio' : 'video';
    this.playlistSnapshots[name] = (this.playlistSnapshots[name] ?? 0) + 1;
    this.playlistSequences[name] = this.mediaSequences[name === 'audio' ? 1 : 0];
    return { info: { finalUrl: uri, totalBytes: bytes.byteLength, contentType: 'application/vnd.apple.mpegurl' }, bytes: bytes.slice(0) };
  }
  async read(uri: string, range: ByteRange, token: VesperCancellationToken): Promise<ArrayBuffer> {
    this.check(token);
    const playlist = this.playlists.get(uri);
    if (playlist !== undefined) { return playlist.slice(range.start, range.end + 1); }
    const resource = this.resource(uri, token);
    const bytes = await this.upstream.read(resource.upstreamUri, range, token); this.check(token);
    return bytes;
  }
  cancel(): void { this.disposed = true; this.lifetime.cancel(); }
  async dispose(): Promise<void> {
    this.cancel(); await this.pending?.catch((): void => {});
    for (const pin of this.pins.values()) { pin.remove(); pin.resource.pins--; }
    this.pins.clear(); this.resources.clear(); this.playlists.clear(); this.bridge.close();
  }
  inventory(): string { return JSON.stringify({ profile: 'dash-live-avc-aac-timeline', refreshes: this.refreshes,
    refreshFailures: this.refreshFailures, retainedResources: this.resources.size, activePins: this.pins.size,
    mediaSequences: this.mediaSequences, playlistSnapshots: this.playlistSnapshots,
    playlistSequences: this.playlistSequences, resourceOpens: this.resourceOpens, disposed: this.disposed }); }
}
