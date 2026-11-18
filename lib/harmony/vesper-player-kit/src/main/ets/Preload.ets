import { util, url } from '@kit.ArkTS';
import { NativeSession } from './NativeSession';
import { VesperSourceLease } from './SourceSession';
import { StartupCache, ResourceInfo, WarmResource } from './StartupCache';
import { Transport, ByteRange } from './Transport';
import { OperationScope, VesperCancellationToken, nativeError } from './Operation';
import { VesperPlayerError } from './Types';

export interface VesperPreloadOptions { timeoutMs?: number; maxBytes?: number; lifetimeMs?: number; }
export interface VesperPreloadConfiguration { workers?: number; pendingCapacity?: number; }
export interface VesperPreloadSnapshot {
  taskId: string; sourceId: string; status: string; receivedBytes: number; reuseLevel: string; reason?: string;
}
interface DashRepresentation { baseURL: string; segmentBase?: DashSegmentBase; }
interface DashSegmentBase { initialization: ByteRange; indexRange: ByteRange; }
interface DashAdaptation { kind: string; representations: DashRepresentation[]; }
interface DashPeriod { adaptationSets: DashAdaptation[]; }
interface DashManifest { type: string; periods: DashPeriod[]; }
interface Sidx { timescale: number; references: Object[]; }
interface Segment { duration: number; range: ByteRange; }

/** A shared task's logical completion is separate from its physical worker exit. */
export class VesperPreloadTask {
  readonly completion: Promise<void>;
  readonly sourceId: string;
  readonly taskId: string;
  readonly cancellation: VesperCancellationToken = new VesperCancellationToken();
  private cache: StartupCache;
  private state: string = 'queued';
  private received: number = 0;
  private reason?: string;
  private resolveCompletion: () => void = (): void => {};
  private rejectCompletion: (error: VesperPlayerError) => void = (): void => {};
  constructor(id: string, sourceId: string, cache: StartupCache) {
    this.taskId = id; this.sourceId = sourceId; this.cache = cache;
    this.completion = new Promise<void>((resolve, reject) => { this.resolveCompletion = resolve; this.rejectCompletion = reject; });
    // Hosts may observe a task through snapshots without awaiting its completion.
    this.completion.catch(() => {});
  }
  cancel(): void { this.cancellation.cancel(); }
  snapshot(): VesperPreloadSnapshot {
    return { taskId: this.taskId, sourceId: this.sourceId, status: this.state,
      receivedBytes: this.received, reuseLevel: this.cache.reuseLevel(), reason: this.reason };
  }
  /** @internal */
  running(): void { if (this.state === 'queued') { this.state = 'running'; } }
  /** @internal */
  progress(bytes: number): void { this.received += bytes; }
  /** @internal */
  complete(): void { if (this.state !== 'queued' && this.state !== 'running') { return; } this.state = 'completed'; this.resolveCompletion(); }
  /** @internal */
  fail(error: VesperPlayerError): void {
    if (this.state !== 'queued' && this.state !== 'running') { return; }
    this.state = error.code === 'cancelled' || error.code === 'closed' ? 'cancelled' : error.code === 'unsupported' ? 'unsupported' : 'failed';
    this.reason = error.code; this.rejectCompletion(error);
  }
}

interface Job {
  task: VesperPreloadTask; lease: VesperSourceLease; scope: OperationScope; maximum: number;
  lifetime: number; generation: number; cache: StartupCache; physical: boolean;
  heldBytes: number;
}

/** Session-owned workers never create AVPlayer, Surface, decoder or audio resources. */
export class Preloader {
  private static nextId: number = 0;
  private transport: Transport = new Transport();
  private queue: Job[] = [];
  private jobs: Map<string, Job> = new Map();
  private active: number = 0;
  private workers: number;
  private pending: number;
  private closed: boolean = false;
  constructor(configuration: VesperPreloadConfiguration) {
    this.workers = configuration.workers ?? 2; this.pending = configuration.pendingCapacity ?? 4;
    if (!Number.isInteger(this.workers) || this.workers < 1 || this.workers > 4
      || !Number.isInteger(this.pending) || this.pending < 0 || this.pending > 32) {
      throw new VesperPlayerError('invalid_argument', 'Preload workers or pending capacity are invalid');
    }
  }
  submit(lease: VesperSourceLease, options: VesperPreloadOptions): VesperPreloadTask {
    if (this.closed) { throw new VesperPlayerError('closed', 'Preloader is closed'); }
    const existing = this.jobs.get(lease.sourceId);
    if (existing !== undefined) { lease.release(); return existing.task; }
    if (this.queue.length >= this.pending && (this.active >= this.workers || this.queue.length > 0)) { throw new VesperPlayerError('capacity', 'Preload queue is full'); }
    const maximum = options.maxBytes ?? 8 * 1024 * 1024;
    const lifetime = options.lifetimeMs ?? 30000;
    if (!Number.isSafeInteger(maximum) || maximum < 1 || maximum > 16 * 1024 * 1024
      || !Number.isInteger(lifetime) || lifetime < 1 || lifetime > 30000 || Preloader.nextId >= Number.MAX_SAFE_INTEGER) {
      throw new VesperPlayerError('invalid_argument', 'Preload byte limit or lifetime is invalid');
    }
    const cache = lease.startupCache();
    const task = new VesperPreloadTask(`${++Preloader.nextId}`, lease.sourceId, cache);
    const scope = new OperationScope(options.timeoutMs ?? 5000, task.cancellation);
    const reservation = lease.descriptor.protocol === 'progressive' ? Math.min(maximum, 65536, cache.maximumBytes()) : Math.min(maximum, cache.maximumBytes());
    const job: Job = { task: task, lease: lease, scope: scope, maximum: reservation, lifetime: lifetime,
      generation: cache.generationId(), cache: cache, physical: false, heldBytes: 0 };
    scope.onFailure((error: VesperPlayerError): void => {
      task.fail(error); task.cancellation.cancel();
      if (!job.physical) { const index = this.queue.indexOf(job); if (index >= 0) { this.queue.splice(index, 1); this.finish(job); } }
    });
    this.jobs.set(lease.sourceId, job); this.queue.push(job); this.pump();
    if (!job.physical && this.queue.length > this.pending) { job.task.cancel(); }
    return task;
  }
  private pump(): void {
    while (this.active < this.workers && this.queue.length > 0) {
      const job = this.queue[0];
      if (job.maximum < 1) { this.queue.shift(); job.task.fail(new VesperPlayerError('unsupported', 'Startup memory cache is disabled')); this.finish(job); continue; }
      if (!job.cache.reserve(job.maximum)) { return; }
      job.heldBytes = job.maximum;
      this.queue.shift(); job.physical = true; this.active++; job.task.running();
      this.execute(job).then(() => { job.task.complete(); }, (error: Error) => { job.task.fail(nativeError(error, 'preload')); }).finally(() => {
        if (job.heldBytes > 0) { job.cache.releaseReservation(job.heldBytes); job.heldBytes = 0; }
        this.active--; this.finish(job); this.pump();
      });
    }
  }
  private finish(job: Job): void {
    job.scope.close(); this.jobs.delete(job.lease.sourceId); job.lease.release();
  }
  private async execute(job: Job): Promise<void> {
    const source = job.lease.descriptor;
    if (source.kind !== 'remote' || !['progressive', 'dash'].includes(source.protocol)) {
      throw new VesperPlayerError('unsupported', 'Standalone startup preload supports progressive and static SegmentBase DASH');
    }
    const resources: WarmResource[] = [];
    const headers = source.headers ?? {};
    const origin = new url.URL(source.uri).origin;
    let received = 0;
    const fetch = async (uri: string, range?: ByteRange): Promise<WarmResource> => {
      job.scope.check(); job.lease.validateActivation();
      if (Object.keys(headers).length > 0 && new url.URL(uri).origin !== origin) { throw new VesperPlayerError('access_scope', 'Authenticated preload resources require the source origin'); }
      const response = await this.transport.get(uri, { headers: headers,
        maxBytes: Math.min(1024 * 1024, job.maximum - received), range: range, cancellation: job.task.cancellation });
      job.scope.check(); received += response.bytes.byteLength; job.task.progress(response.bytes.byteLength);
      const info: ResourceInfo = { totalBytes: response.totalBytes ?? response.bytes.byteLength,
        contentType: response.contentType, finalUrl: response.finalUrl, etag: response.etag };
      const resource: WarmResource = { uri: uri, offset: range?.start ?? 0, bytes: response.bytes, info: info };
      const previous = resources.find((item: WarmResource) => item.uri === uri);
      if (previous !== undefined && (previous.info.totalBytes !== info.totalBytes || previous.info.etag !== info.etag)) {
        throw new VesperPlayerError('resource_changed', 'Startup resource changed during preload');
      }
      if (resources.length >= 64) { throw new VesperPlayerError('capacity', 'Startup resource capacity reached'); }
      resources.push(resource); return resource;
    };
    if (source.protocol === 'progressive') {
      const metadata = await this.transport.get(source.uri, { headers: headers, maxBytes: 1,
        range: { start: 0, end: 0 }, cancellation: job.task.cancellation });
      job.scope.check(); const total = metadata.totalBytes;
      if (total === undefined || total < 1) { throw new VesperPlayerError('unsupported', 'Progressive length is unknown'); }
      await fetch(source.uri, { start: 0, end: Math.min(total, job.maximum) - 1 });
    } else {
      const bridge = new NativeSession('{"kind":"dash"}');
      try {
        const manifestResource = await fetch(source.uri);
        const mpd = util.TextDecoder.create('utf-8', { fatal: true }).decodeToString(new Uint8Array(manifestResource.bytes));
        const manifest = bridge.invoke<DashManifest>(JSON.stringify({ operation: 'parse_manifest', mpd: mpd, manifestUrl: manifestResource.info.finalUrl }));
        if (manifest.type !== 'static' || manifest.periods.length !== 1) { throw new VesperPlayerError('unsupported', 'Startup cache requires static single-Period DASH'); }
        let audio = false; let video = false;
        for (const adaptation of manifest.periods[0].adaptationSets) {
          if (adaptation.kind !== 'audio' && adaptation.kind !== 'video') { continue; }
          if ((adaptation.kind === 'audio' && audio) || (adaptation.kind === 'video' && video)) { continue; }
          const representation = adaptation.representations[0];
          if (representation === undefined || representation.segmentBase === undefined) { throw new VesperPlayerError('unsupported', 'Startup representation has no SegmentBase'); }
          const base = representation.segmentBase;
          await fetch(representation.baseURL, base.initialization);
          const index = await fetch(representation.baseURL, base.indexRange);
          const sidx = bridge.invoke<Sidx>(JSON.stringify({ operation: 'parse_sidx', data: Array.from(new Uint8Array(index.bytes)) }));
          const segments = bridge.invoke<Segment[]>(JSON.stringify({ operation: 'media_segments', segmentBase: base, sidx: sidx }));
          if (segments.length === 0) { throw new VesperPlayerError('unsupported', 'Startup representation has no finite media interval'); }
          let duration = 0;
          for (const segment of segments) {
            if (duration >= 5) { break; }
            let offset = segment.range.start;
            while (offset <= segment.range.end) {
              const end = Math.min(segment.range.end, offset + 1024 * 1024 - 1);
              await fetch(representation.baseURL, { start: offset, end: end }); offset = end + 1;
            }
            duration += segment.duration;
          }
          if (adaptation.kind === 'audio') { audio = true; } else { video = true; }
        }
        if (!audio && !video) { throw new VesperPlayerError('unsupported', 'No playable startup representation'); }
      } finally { bridge.close(); }
    }
    job.scope.check(); job.lease.validateActivation();
    // Transfer physical reservation into resident cache bytes without an await.
    job.cache.releaseReservation(job.heldBytes); job.heldBytes = 0;
    job.cache.commit(resources, job.generation, job.lifetime); job.cache.setStartupProfile(source.protocol === 'dash');
  }
  cancelSource(sourceId: string): void { this.jobs.get(sourceId)?.task.cancel(); }
  close(): void { this.closed = true; for (const job of Array.from(this.jobs.values())) { job.task.cancel(); } }
  inventory(): string { return JSON.stringify({ active: this.active, pending: this.queue.length, retainedTasks: this.jobs.size }); }
}
