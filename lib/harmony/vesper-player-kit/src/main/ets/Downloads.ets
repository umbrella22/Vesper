import { common } from '@kit.AbilityKit';
import { fileIo as fs } from '@kit.CoreFileKit';
import { util, url } from '@kit.ArkTS';
import { NativeSession } from './NativeSession';
import { VesperExportArtifact, VesperExportFile, VesperExportOptions } from './Exports';
import { exportOutputBytes, EXPORT_MAXIMUM_BYTES } from './ExportStream';
import { DownloadPartial, downloadDigest, downloadResourceFingerprint, clearDownloadPartial, validDownloadPartial, verifiedDownloadPrefix, writeDownloadChunk } from './DownloadPartial';
import { VesperSourceHandle, VesperSourceLease } from './SourceSession';
import { VesperPlayerSource, VesperPlayerError } from './Types';
import { VesperCancellationToken, OperationScope, nativeError } from './Operation';
import { Transport, TransportResponse } from './Transport';
import { relativeFile, directory, regularFile, writeAll, fileHash, atomicText, boundedText, AssetPins, assetFile } from './FileStore';

export interface VesperDownloadProfile { variantId?: string; preferredAudioLanguage?: string; }
export interface VesperDownloadSnapshot {
  taskId: string; assetId: string; status: string; receivedBytes: string; totalBytes?: string;
  receivedSegments: number; totalSegments?: number; reason?: string;
  title?: string; resumeState?: string; reusableBytes?: string; exportEligibility?: VesperExportEligibility;
}
export interface VesperExportEligibility { available: boolean; reason: string; maximumBytes: number; outputBytes: number; }
interface FilePlan {
  id: string; uri: string; path: string; offset?: string; length?: string; size?: string;
  generatedText?: string; segment: boolean;
}
interface AssetIndex { format: string; totalBytes: string; files: FilePlan[]; }
interface Need { kind: string; uri: string; }
interface Planned { planId?: string; index?: AssetIndex; needs: Need[]; }
interface TaskResponse { task: VesperDownloadSnapshot; attemptId?: string; }
interface SnapshotResponse { tasks: VesperDownloadSnapshot[]; }
interface TextRecord { uri: string; text: string; }
interface LengthRecord { uri: string; length: string; }
interface Inventory { texts: TextRecord[]; lengths: LengthRecord[]; }
interface DownloadCommand { op: string; taskId?: string; planId?: string; assetId?: string; attemptId?: string; receivedBytes?: string; receivedSegments?: number; }
interface RestoreRecord { taskId: string; assetId: string; receivedBytes: string; totalBytes?: string; verifiedComplete: boolean; removed: boolean; }
interface Validator { length: number; etag?: string; }
interface StoredFile extends DownloadPartial { path: string; size: number; segment: boolean; generated: boolean; checksum?: string; }
interface StoredTask { task: VesperDownloadSnapshot; title?: string; format: string; entry: string; files: StoredFile[]; }
interface Checkpoint { schemaVersion: number; tasks: StoredTask[]; }
interface Task {
  snapshot: VesperDownloadSnapshot; format: string; entry: string; files: StoredFile[];
  plan?: AssetIndex; planId?: string; lease?: VesperSourceLease;
  validators: Map<string, Validator>; job?: DownloadJob; title?: string;
}
interface DownloadJob { task: Task; attempt: string; token: VesperCancellationToken; physical: boolean; done: Promise<void>; finish: () => void; }
const MAX_OUTPUT_FILES: number = 4096;
const DISCOVERY_CONCURRENCY: number = 4;

function byteCount(value: string): number {
  if (!/^[0-9]{1,16}$/.test(value)) { throw new VesperPlayerError('invalid_argument', 'Download byte count is invalid'); }
  const count = Number(value);
  if (!Number.isSafeInteger(count) || count < 0 || count > 1024 * 1024 * 1024) { throw new VesperPlayerError('capacity', 'Finite download exceeds 1 GiB'); }
  return count;
}
function identity(value: string): string {
  if (!/^[1-9][0-9]{0,19}$/.test(value) || value.length === 20 && value > '18446744073709551615') { throw new VesperPlayerError('invalid_argument', 'Download identity is invalid'); }
  return value;
}

/** Rust plans finite assets and owns task state; ArkTS owns bounded network/file work. */
export class VesperDownloadManager {
  private static roots: Set<string> = new Set();
  private readonly root: string;
  private bridge: NativeSession;
  private transport: Transport = new Transport();
  private tasks: Map<string, Task> = new Map();
  private queue: DownloadJob[] = [];
  private active: number = 0;
  private planning: number = 0;
  private closed: boolean = false;
  private closing: boolean = false;
  private closeWork?: Promise<void>;
  private mutationTail: Promise<void> = Promise.resolve();
  private pendingMutations: number = 0;
  private checkpointTail: Promise<void> = Promise.resolve();
  private pendingCheckpoints: number = 0;
  private listeners: Set<(snapshot: VesperDownloadSnapshot) => void> = new Set();
  private listenerFailures: number = 0;
  private quarantinedRecords: number = 0;
  private constructor(root: string) { this.root = root; this.bridge = new NativeSession('{"kind":"downloads","maxTasks":32}'); }
  static async open(context: common.Context): Promise<VesperDownloadManager> {
    const root = `${context.filesDir}/vesper-downloads`;
    if (VesperDownloadManager.roots.has(root) || VesperDownloadManager.roots.size >= 8) { throw new VesperPlayerError('in_use', 'Download directory already has an owner or owner capacity was reached'); }
    VesperDownloadManager.roots.add(root);
    let manager: VesperDownloadManager | undefined;
    try {
      manager = new VesperDownloadManager(root); await directory(root); await VesperExportArtifact.recover(root); await manager.restore(); return manager;
    } catch (error) { manager?.bridge.close(); VesperDownloadManager.roots.delete(root); throw nativeError(error, 'download restore'); }
  }
  private check(): void { if (this.closed) { throw new VesperPlayerError('closed', 'Download manager is closed'); } }
  private io<T>(work: () => Promise<T>): Promise<T> {
    this.check();
    if (this.closing) { return Promise.reject(new VesperPlayerError('closed', 'Download manager is closing')); }
    if (this.pendingMutations >= 8) { return Promise.reject(new VesperPlayerError('capacity', 'Download file operation queue is full')); }
    this.pendingMutations++;
    const result = this.mutationTail.then(work);
    this.mutationTail = result.then(() => { this.pendingMutations--; }, () => { this.pendingMutations--; });
    const scope = new OperationScope(60000); return scope.wait(result).finally(() => { scope.close(); });
  }
  snapshots(): VesperDownloadSnapshot[] { return Array.from(this.tasks.values()).map((task: Task) => this.copy(task.snapshot)); }
  subscribe(listener: (snapshot: VesperDownloadSnapshot) => void): () => void {
    this.check(); if (this.listeners.size >= 64) { throw new VesperPlayerError('capacity', 'Download listener capacity reached'); }
    this.listeners.add(listener); return (): void => { this.listeners.delete(listener); };
  }
  private copy(value: VesperDownloadSnapshot): VesperDownloadSnapshot {
    const result = JSON.parse(JSON.stringify(value)) as VesperDownloadSnapshot;
    const task = this.tasks.get(value.taskId);
    if (task !== undefined) {
      const reusable = task.files.reduce((sum: number, file: StoredFile): number => sum + (file.generated ? 0 : file.checksum !== undefined ? file.size : file.partialBytes ?? 0), 0);
      const bytes = exportOutputBytes(task.format, task.entry, task.files);
      result.title = task.title; result.reusableBytes = `${reusable}`;
      result.resumeState = value.status === 'Completed' ? 'complete' : task.lease === undefined || value.status === 'Failed' ? 'source-refresh-required' :
        task.job !== undefined ? 'in-progress' : reusable > 0 ? 'version-check-required' : 'restart-required';
      result.exportEligibility = { available: value.status === 'Completed' && bytes <= EXPORT_MAXIMUM_BYTES,
        reason: value.status !== 'Completed' ? 'not-completed' : bytes > EXPORT_MAXIMUM_BYTES ? 'size-limit' : 'eligible', maximumBytes: EXPORT_MAXIMUM_BYTES, outputBytes: bytes };
    }
    return result;
  }
  private publish(task: Task): void {
    for (const listener of Array.from(this.listeners)) { try { listener(this.copy(task.snapshot)); } catch (_) { this.listenerFailures++; } }
  }
  private call(command: DownloadCommand): TaskResponse { return this.bridge.invoke<TaskResponse>(JSON.stringify(command)); }
  private task(id: string): Task {
    this.check(); const task = this.tasks.get(identity(id));
    if (task === undefined) { throw new VesperPlayerError('stale_task', 'Download task does not exist'); } return task;
  }
  private access(lease: VesperSourceLease, uri: string): Record<string, string> {
    lease.validateActivation(); const headers = lease.descriptor.headers ?? {};
    if (Object.keys(headers).length > 0 && new url.URL(uri).origin !== new url.URL(lease.descriptor.uri).origin) {
      throw new VesperPlayerError('access_scope', 'Authenticated download children require the accepted source origin');
    }
    return headers;
  }
  private async discover(lease: VesperSourceLease, profile: VesperDownloadProfile): Promise<Planned> {
    if (this.planning >= 2) { throw new VesperPlayerError('capacity', 'Download planning capacity reached'); }
    this.planning++;
    const texts: TextRecord[] = []; const lengths: LengthRecord[] = [];
    const scope = new OperationScope(60000);
    try {
      for (let step = 0; step < 160; step++) {
        this.check(); scope.check(); lease.validateActivation();
        const inventory: Inventory = { texts: texts, lengths: lengths };
        const command = JSON.stringify({ op: 'plan', source: lease.descriptor, profile: profile, inventory: inventory });
        if (command.length > 900 * 1024) { throw new VesperPlayerError('capacity', 'Download planning inventory exceeds its byte budget'); }
        const value = this.bridge.invoke<Planned>(command);
        if (value.needs.length === 0) { return value; }
        if (value.needs.length > 128) { throw new VesperPlayerError('capacity', 'Download discovery batch exceeds its capacity'); }
        const pending: Set<string> = new Set();
        for (const need of value.needs) {
          const key = `${need.kind}:${need.uri}`;
          if (pending.has(key)) { throw new VesperPlayerError('invalid_response', 'Download discovery duplicated a resource'); }
          pending.add(key);
        }
        let next: number = 0;
        let failure: VesperPlayerError | undefined;
        const worker = async (): Promise<void> => {
          while (next < value.needs.length && failure === undefined) {
            const need = value.needs[next++];
            try { await this.discoverNeed(need, lease, texts, lengths, scope); }
            catch (error) { failure = nativeError(error, 'download discovery'); }
          }
        };
        const workers: Promise<void>[] = [];
        for (let index = 0; index < Math.min(DISCOVERY_CONCURRENCY, value.needs.length); index++) { workers.push(worker()); }
        // Join every physical request before releasing this discovery attempt.
        await Promise.all(workers);
        if (failure !== undefined) { throw failure; }
      }
      throw new VesperPlayerError('capacity', 'Download discovery step limit reached');
    } finally { scope.close(); this.planning--; }
  }
  private async discoverNeed(need: Need, lease: VesperSourceLease, texts: TextRecord[], lengths: LengthRecord[], scope: OperationScope): Promise<void> {
          scope.check(); this.check(); lease.validateActivation();
          const headers = this.access(lease, need.uri);
          if (need.kind === 'text') {
            if (texts.some((record: TextRecord) => record.uri === need.uri)) { throw new VesperPlayerError('invalid_response', 'Download discovery repeated an existing manifest'); }
            const response = await this.transport.get(need.uri, { headers: headers, maxBytes: 128 * 1024 });
            scope.check(); this.check(); lease.validateActivation();
            // Resolving relative manifest references against a redirected base
            // requires a refreshed source descriptor from the host.
            if (response.finalUrl !== need.uri) { throw new VesperPlayerError('access_context_required', 'Refresh the source to the final manifest URL before downloading'); }
            texts.push({ uri: need.uri, text: util.TextDecoder.create('utf-8', { fatal: true }).decodeWithStream(new Uint8Array(response.bytes)) });
          } else if (need.kind === 'length') {
            if (lengths.some((record: LengthRecord) => record.uri === need.uri)) { throw new VesperPlayerError('invalid_response', 'Download discovery repeated an existing resource'); }
            const response = await this.transport.get(need.uri, { headers: headers, maxBytes: 1, range: { start: 0, end: 0 }, allowFullResponse: true, attempts: 3 });
            scope.check(); this.check(); lease.validateActivation();
            if (response.totalBytes === undefined || response.totalBytes < 1) { throw new VesperPlayerError('invalid_response', 'Download resource length is unavailable'); }
            byteCount(`${response.totalBytes}`); lengths.push({ uri: need.uri, length: `${response.totalBytes}` });
          } else { throw new VesperPlayerError('unsupported', 'Unknown planning inventory request'); }
  }
  create(handle: VesperSourceHandle, assetId: string, profile: VesperDownloadProfile = {}): Promise<VesperDownloadSnapshot> {
    return this.io(() => this.createNow(handle, assetId, profile));
  }
  private async createNow(handle: VesperSourceHandle, assetId: string, profile: VesperDownloadProfile): Promise<VesperDownloadSnapshot> {
    this.check(); if (this.tasks.size >= 32) { throw new VesperPlayerError('capacity', 'Retained download task capacity reached'); }
    const lease = handle.acquireLease(); let planned: Planned | undefined;
    try {
      planned = await this.discover(lease, profile); this.check();
      if (planned.planId === undefined || planned.index === undefined) { throw new VesperPlayerError('invalid_response', 'Download plan is incomplete'); }
      const index = planned.index; byteCount(index.totalBytes);
      const files = this.admit(index);
      const result = this.call({ op: 'create', planId: planned.planId, assetId: assetId });
      const task: Task = { snapshot: result.task, format: index.format, entry: files[0].path,
        files: files, plan: index, planId: planned.planId, lease: lease, validators: new Map(), title: lease.descriptor.label };
      this.tasks.set(result.task.taskId, task); await directory(this.assetRoot(task)); await this.checkpoint(); this.publish(task);
      return this.copy(task.snapshot);
    } catch (error) {
      if (planned?.planId !== undefined && !Array.from(this.tasks.values()).some((task: Task) => task.planId === planned?.planId)) {
        this.bridge.invoke<Object>(JSON.stringify({ op: 'discardPlan', planId: planned.planId }));
      }
      if (!Array.from(this.tasks.values()).some((task: Task) => task.lease === lease)) { lease.release(); }
      throw nativeError(error, 'download creation');
    }
  }
  private admit(index: AssetIndex): StoredFile[] {
    if (index.files.length < 1 || index.files.length > MAX_OUTPUT_FILES) { throw new VesperPlayerError('capacity', 'Download output count is invalid'); }
    const seen: Set<string> = new Set();
    return index.files.map((file: FilePlan): StoredFile => {
      const path = relativeFile(file.path);
      if (seen.has(path)) { throw new VesperPlayerError('invalid_path', 'Download output paths collide'); } seen.add(path);
      const generated = file.generatedText !== undefined && file.generatedText !== null;
      const size = generated ? new util.TextEncoder().encodeInto(file.generatedText ?? '').byteLength : byteCount(file.size ?? file.length ?? '');
      if (size < 1 || (generated && size > 128 * 1024)) { throw new VesperPlayerError('invalid_response', 'Download output size is invalid'); }
      return { path: path, size: size, segment: file.segment, generated: generated };
    });
  }
  private assetRoot(task: Task): string { return `${this.root}/asset-${identity(task.snapshot.taskId)}`; }
  start(taskId: string): VesperDownloadSnapshot { return this.enqueue(this.task(taskId), 'start'); }
  resume(taskId: string): VesperDownloadSnapshot { return this.enqueue(this.task(taskId), 'resume'); }
  private enqueue(task: Task, op: string): VesperDownloadSnapshot {
    if (this.closing) { throw new VesperPlayerError('closed', 'Download manager is closing'); }
    if (task.job !== undefined) { throw new VesperPlayerError('in_use', 'Previous physical download attempt has not exited'); }
    if (this.queue.length >= 16) { throw new VesperPlayerError('capacity', 'Download queue is full'); }
    if (task.lease === undefined || task.plan === undefined) { throw new VesperPlayerError('access_context_required', 'Refresh the source before resuming a restored task'); }
    task.lease.validateActivation();
    const result = this.call({ op: op, taskId: task.snapshot.taskId });
    if (result.attemptId === undefined) { throw new VesperPlayerError('invalid_response', 'Download attempt identity is missing'); }
    task.snapshot = result.task;
    let finish = (): void => {};
    const done = new Promise<void>((resolve) => { finish = resolve; });
    const job: DownloadJob = { task: task, attempt: result.attemptId, token: new VesperCancellationToken(), physical: false, done: done, finish: finish };
    task.job = job; this.queue.push(job); this.publish(task); this.pump(); return this.copy(task.snapshot);
  }
  private pump(): void {
    while (this.active < 2 && this.queue.length > 0) {
      const job = this.queue.shift(); if (job === undefined) { return; }
      job.physical = true; this.active++;
      this.execute(job).catch((error: Error) => {
        if (!job.token.isCancelled) {
          try { job.task.snapshot = this.call({ op: 'fail', taskId: job.task.snapshot.taskId, attemptId: job.attempt }).task; }
          catch (_) {}
          job.task.snapshot.reason = nativeError(error, 'download execution').code;
        }
      }).finally(async (): Promise<void> => {
        this.active--; job.task.job = undefined;
        try { await this.checkpoint(); } catch (_) { job.task.snapshot.reason = 'checkpoint_failed'; }
        this.publish(job.task); job.finish(); this.pump();
      });
    }
  }
  private guard(job: DownloadJob): void {
    this.check(); if (job.token.isCancelled) { throw new VesperPlayerError('cancelled', 'Download attempt was cancelled'); }
    job.task.lease?.validateActivation();
  }
  private async execute(job: DownloadJob): Promise<void> {
    const task = job.task; const plan = task.plan; const lease = task.lease;
    if (plan === undefined || lease === undefined) { throw new VesperPlayerError('access_context_required', 'Download access context is missing'); }
    this.guard(job);
    if (task.snapshot.status === 'Preparing') { task.snapshot = this.call({ op: 'prepared', taskId: task.snapshot.taskId, attemptId: job.attempt }).task; }
    let received = 0; let segments = 0;
    for (let index = 0; index < task.files.length; index++) {
      this.guard(job); const file = task.files[index]; const remote = plan.files[index];
      const path = await assetFile(this.assetRoot(task), file.path, true);
      const fingerprint = file.generated ? '' : await downloadResourceFingerprint(remote.uri);
      let expected: Validator | undefined;
      if (!file.generated && (file.checksum !== undefined || (file.partialBytes ?? 0) > 0)) {
        if (file.resourceFingerprint === fingerprint && file.etag !== undefined && !file.etag.startsWith('W/')) {
          const base = remote.offset === undefined || remote.offset === null ? 0 : byteCount(remote.offset);
          try {
            const check = await this.transport.get(remote.uri, { headers: this.access(lease, remote.uri), maxBytes: 1,
              range: { start: base, end: base }, validator: file.etag, allowFullResponse: true, cancellation: job.token, attempts: 3 });
            this.guard(job);
            if (check.etag !== file.etag || check.totalBytes !== file.resourceLength) { throw new VesperPlayerError('resource_changed', 'Resume validator changed'); }
            expected = { length: check.totalBytes ?? 0, etag: check.etag };
          } catch (error) {
            if ((error as VesperPlayerError).code === 'resource_changed') { file.checksum = undefined; clearDownloadPartial(file); }
            throw error as Error;
          }
        } else { file.checksum = undefined; clearDownloadPartial(file); }
      }
      let verified = false;
      if (file.checksum !== undefined) {
        try { verified = await regularFile(path, file.size) === file.size && await fileHash(path, file.size) === file.checksum; } catch (_) {}
      }
      if (!verified) {
        file.checksum = undefined;
        if (file.generated) { await atomicText(path, remote.generatedText ?? ''); }
        else {
          let offset = expected === undefined ? 0 : await verifiedDownloadPrefix(`${path}.part`, file, file.size, (): void => { this.guard(job); });
          if (offset === 0) { clearDownloadPartial(file); file.partialBytes = 0; file.partialHashes = []; }
          file.resourceFingerprint = fingerprint;
          const output = await fs.open(`${path}.part`, fs.OpenMode.CREATE | fs.OpenMode.WRITE_ONLY | fs.OpenMode.NOFOLLOW);
          try {
            await fs.truncate(output.fd, offset);
            const base = remote.offset === undefined || remote.offset === null ? 0 : byteCount(remote.offset);
            while (offset < file.size) {
              this.guard(job); const count = Math.min(65536, file.size - offset);
              const response: TransportResponse = await this.transport.get(remote.uri, { headers: this.access(lease, remote.uri),
                maxBytes: count, range: { start: base + offset, end: base + offset + count - 1 },
                cancellation: job.token, validator: expected?.etag, allowFullResponse: true, attempts: 3 });
              this.guard(job);
              if (response.totalBytes === undefined || (remote.length === undefined || remote.length === null) && response.totalBytes !== file.size) {
                throw new VesperPlayerError('resource_changed', 'Download length changed from its accepted plan');
              }
              if (expected !== undefined && (response.totalBytes !== expected.length || response.etag !== expected.etag)) {
                throw new VesperPlayerError('resource_changed', 'Download validator changed during transfer');
              }
              expected = { length: response.totalBytes, etag: response.etag };
              // Without a validator a multi-request file could combine revisions.
              if (file.size > 65536 && (expected.etag === undefined || expected.etag.startsWith('W/'))) { throw new VesperPlayerError('validator_required', 'Multi-range downloads require a strong ETag'); }
              await writeDownloadChunk(output.fd, response.bytes, offset);
              const hash = await downloadDigest(new Uint8Array(response.bytes));
              offset += response.bytes.byteLength; file.partialBytes = offset;
              if (file.partialHashes === undefined) { file.partialHashes = []; }
              file.partialHashes.push(hash); file.etag = expected.etag; file.resourceLength = expected.length;
              // Commit the first chunk so downloads below the periodic checkpoint
              // interval can also recover a verified prefix after process loss.
              if (file.partialHashes.length === 1 || file.partialHashes.length % 16 === 0) {
                await fs.fsync(output.fd); this.guard(job);
                task.snapshot = this.call({ op: 'progress', taskId: task.snapshot.taskId, attemptId: job.attempt,
                  receivedBytes: `${received + offset}`, receivedSegments: segments }).task;
                await this.checkpoint(); this.publish(task);
              }
            }
            this.guard(job); await fs.fsync(output.fd); file.etag = expected?.etag;
          } finally { try { await fs.fsync(output.fd); } finally { await fs.close(output); } }
          this.guard(job); await fs.rename(`${path}.part`, path);
        }
        this.guard(job); file.checksum = await fileHash(path, file.size); file.partialBytes = undefined; file.partialHashes = undefined; this.guard(job);
      }
      if (!file.generated) { received += file.size; } if (file.segment) { segments++; }
      task.snapshot = this.call({ op: 'progress', taskId: task.snapshot.taskId, attemptId: job.attempt,
        receivedBytes: `${received}`, receivedSegments: segments }).task;
      await this.checkpoint(); this.publish(task);
    }
    this.guard(job); task.snapshot = this.call({ op: 'complete', taskId: task.snapshot.taskId, attemptId: job.attempt }).task;
    await this.checkpoint();
  }
  pause(taskId: string): Promise<VesperDownloadSnapshot> { return this.io(() => this.pauseNow(taskId)); }
  private async pauseNow(taskId: string): Promise<VesperDownloadSnapshot> {
    const task = this.task(taskId);
    task.snapshot = this.call({ op: 'pause', taskId: taskId }).task; this.cancel(task); this.publish(task);
    await this.checkpoint(); return this.copy(task.snapshot);
  }
  private cancel(task: Task): void {
    const job = task.job; if (job === undefined) { return; } job.token.cancel();
    if (!job.physical) { const index = this.queue.indexOf(job); if (index >= 0) { this.queue.splice(index, 1); } task.job = undefined; job.finish(); }
  }
  async wait(taskId: string, timeoutMs: number = 60000): Promise<VesperDownloadSnapshot> {
    const task = this.task(taskId); const scope = new OperationScope(timeoutMs);
    try { if (task.job !== undefined) { await scope.wait(task.job.done); } return this.copy(task.snapshot); } finally { scope.close(); }
  }
  refreshSource(taskId: string, handle: VesperSourceHandle, profile: VesperDownloadProfile = {}): Promise<VesperDownloadSnapshot> {
    return this.io(() => this.refreshNow(taskId, handle, profile));
  }
  private async refreshNow(taskId: string, handle: VesperSourceHandle, profile: VesperDownloadProfile): Promise<VesperDownloadSnapshot> {
    const task = this.task(taskId);
    if (task.job !== undefined) { throw new VesperPlayerError('in_use', 'Pause and wait for the previous physical attempt before refreshing'); }
    const lease = handle.acquireLease(); let planned: Planned | undefined;
    try {
      planned = await this.discover(lease, profile); this.check();
      if (planned.index === undefined || planned.planId === undefined) { throw new VesperPlayerError('invalid_response', 'Download plan is incomplete'); }
      const files = this.admit(planned.index);
      // Only same-locator members may keep local bytes. Execution still checks
      // the strong ETag and resource length before reusing a completed or partial file.
      for (let index = 0; index < files.length; index++) {
        const previous = task.files.find((file: StoredFile): boolean => file.path === files[index].path && file.size === files[index].size && file.generated === files[index].generated);
        if (task.snapshot.reason !== 'resource_changed' && previous !== undefined && !previous.generated && previous.resourceFingerprint === await downloadResourceFingerprint(planned.index.files[index].uri)) {
          files[index] = previous;
        }
      }
      task.snapshot = this.call({ op: 'refresh', taskId: taskId, planId: planned.planId }).task;
      const oldPlan = task.planId; task.lease?.release(); task.lease = lease;
      task.plan = planned.index; task.planId = planned.planId; task.files = files; task.format = planned.index.format; task.entry = files[0].path; task.title = lease.descriptor.label;
      if (oldPlan !== undefined) { this.bridge.invoke<Object>(JSON.stringify({ op: 'discardPlan', planId: oldPlan })); }
      await this.checkpoint(); this.publish(task); return this.copy(task.snapshot);
    } catch (error) {
      if (task.lease !== lease) { lease.release(); if (planned?.planId !== undefined) { this.bridge.invoke<Object>(JSON.stringify({ op: 'discardPlan', planId: planned.planId })); } }
      throw nativeError(error, 'download source refresh');
    }
  }
  remove(taskId: string): Promise<void> { return this.io(() => this.removeNow(taskId)); }
  private async removeNow(taskId: string): Promise<void> {
    const task = this.task(taskId); const root = this.assetRoot(task);
    const finishDelete = AssetPins.beginDelete(root);
    try {
    task.snapshot = this.call({ op: 'remove', taskId: taskId }).task; this.cancel(task);
    if (task.job !== undefined) { const scope = new OperationScope(10000); try { await scope.wait(task.job.done); } finally { scope.close(); } }
    if (await fs.access(root)) { await fs.rmdir(root); }
    task.lease?.release(); task.lease = undefined; task.files = []; task.plan = undefined;
    await this.checkpoint(); this.publish(task);
    } finally { finishDelete(); }
  }
  offlineSource(taskId: string): Promise<VesperPlayerSource> { return this.io(() => this.offlineNow(taskId)); }
  exportAsset(taskId: string, format: string = 'original', options: VesperExportOptions = {}): Promise<VesperExportArtifact> {
    return this.io(async (): Promise<VesperExportArtifact> => {
      const task = this.task(taskId);
      if (format !== 'original') { throw new VesperPlayerError('unsupported', 'Remux export is not installed; use the original asset format'); }
      if (task.snapshot.status !== 'Completed' || !await this.verify(task)) { throw new VesperPlayerError('offline_incomplete', 'Export requires a verified complete asset'); }
      const files = task.files.map((file: StoredFile): VesperExportFile => ({ path: file.path, size: file.size, checksum: file.checksum }));
      return VesperExportArtifact.create(this.assetRoot(task), this.root, task.format, task.entry, files, options);
    });
  }
  private async offlineNow(taskId: string): Promise<VesperPlayerSource> {
    const task = this.task(taskId);
    if (task.snapshot.status !== 'Completed' || !await this.verify(task)) { throw new VesperPlayerError('offline_incomplete', 'Offline asset is incomplete or failed verification'); }
    const protocol = task.format === 'hls' ? 'hls' : task.format === 'dash' ? 'dash' : 'file';
    return { kind: 'local', protocol: protocol, uri: `${this.assetRoot(task)}/${relativeFile(task.entry)}`, label: task.title ?? task.snapshot.assetId };
  }
  private async verify(task: Task): Promise<boolean> {
    if (task.files.length === 0) { return false; }
    for (const file of task.files) {
      try { const path = await assetFile(this.assetRoot(task), file.path);
        if (file.checksum === undefined || await regularFile(path, file.size) !== file.size || await fileHash(path, file.size) !== file.checksum) { return false; } }
      catch (_) { return false; }
    }
    return true;
  }
  private checkpoint(): Promise<void> {
    if (this.pendingCheckpoints >= 8) { return Promise.reject(new VesperPlayerError('capacity', 'Download checkpoint queue is full')); }
    // Capture each snapshot before queuing I/O; concurrent worker completions
    // cannot make an older write overwrite a newer checkpoint.
    const value: Checkpoint = { schemaVersion: 1, tasks: Array.from(this.tasks.values()).map((task: Task): StoredTask => ({
      task: this.copy(task.snapshot), title: task.title, format: task.format, entry: task.entry,
      files: task.files.map((file: StoredFile): StoredFile => ({ path: file.path, size: file.size, segment: file.segment,
        generated: file.generated, checksum: file.checksum, etag: file.etag, partialBytes: file.partialBytes,
        partialHashes: file.partialHashes?.slice(), resourceFingerprint: file.resourceFingerprint, resourceLength: file.resourceLength })) })) };
    const text = JSON.stringify(value);
    if (text.length > 4 * 1024 * 1024) { return Promise.reject(new VesperPlayerError('capacity', 'Download checkpoint exceeds 4 MiB')); }
    this.pendingCheckpoints++;
    const work = this.checkpointTail.then(() => atomicText(`${this.root}/checkpoint.json`, text));
    this.checkpointTail = work.then(() => { this.pendingCheckpoints--; }, () => { this.pendingCheckpoints--; }); return work;
  }
  private async restore(): Promise<void> {
    const path = `${this.root}/checkpoint.json`;
    if (!await fs.access(path)) { return; }
    const value = JSON.parse(await boundedText(path, 4 * 1024 * 1024)) as Checkpoint;
    if (value.schemaVersion !== 1 || !Array.isArray(value.tasks) || value.tasks.length > 32) { throw new VesperPlayerError('invalid_checkpoint', 'Download checkpoint schema or task count is invalid'); }
    const ids: Set<string> = new Set();
    const restore: RestoreRecord[] = [];
    for (const record of value.tasks) {
      let id: string;
      try { id = identity(record.task.taskId); }
      catch (_) { this.quarantinedRecords++; continue; }
      if (ids.has(id)) { this.quarantinedRecords++; continue; }
      ids.add(id);
      try {
      if (typeof record.task.assetId !== 'string' || record.task.assetId.length > 256 || !Array.isArray(record.files) || record.files.length > MAX_OUTPUT_FILES
        || !['progressive', 'hls', 'dash'].includes(record.format)) { throw new VesperPlayerError('invalid_checkpoint', 'Download checkpoint contains an invalid task'); }
      const seen: Set<string> = new Set();
      for (const file of record.files) {
        relativeFile(file.path); byteCount(`${file.size}`);
        if (seen.has(file.path) || file.checksum !== undefined && !/^[a-f0-9]{64}$/.test(file.checksum)) { throw new VesperPlayerError('invalid_checkpoint', 'Download checkpoint file identity is invalid'); }
        if (!validDownloadPartial(file, file.size)) { throw new VesperPlayerError('invalid_checkpoint', 'Download prefix checkpoint is invalid'); }
        seen.add(file.path);
      }
      relativeFile(record.entry); byteCount(record.task.receivedBytes); byteCount(record.task.totalBytes ?? '0');
      if (record.task.status !== 'Removed' && (!seen.has(record.entry) || record.entry !== record.files[0]?.path
        || record.format === 'hls' && !record.entry.endsWith('.m3u8')
        || record.format === 'dash' && !record.entry.endsWith('.mpd'))) {
        throw new VesperPlayerError('invalid_checkpoint', 'Offline entry is not a verified asset member');
      }
      if (record.title !== undefined && typeof record.title !== 'string') { throw new VesperPlayerError('invalid_checkpoint', 'Download title is invalid'); }
      const task: Task = { snapshot: record.task, title: record.title, format: record.format, entry: record.entry, files: record.files, validators: new Map() };
      const complete = record.task.status === 'Completed' && await this.verify(task);
      if (record.task.status === 'Removed') { task.files = []; }
      else if (!complete) { task.snapshot.reason = 'source_refresh_required'; }
      this.tasks.set(id, task);
      restore.push({ taskId: id, assetId: record.task.assetId, receivedBytes: record.task.receivedBytes,
        totalBytes: record.task.totalBytes, verifiedComplete: complete, removed: record.task.status === 'Removed' });
      } catch (_) {
        this.quarantinedRecords++;
        // Preserve the identity high-water mark without trusting paths or asset metadata.
        const snapshot: VesperDownloadSnapshot = { taskId: id, assetId: `quarantined-${id}`, status: 'Removed',
          receivedBytes: '0', receivedSegments: 0, reason: 'invalid_checkpoint_record' };
        this.tasks.set(id, { snapshot: snapshot, format: 'progressive', entry: 'quarantined.bin', files: [], validators: new Map() });
        restore.push({ taskId: id, assetId: snapshot.assetId, receivedBytes: '0', verifiedComplete: false, removed: true });
      }
    }
    const result = this.bridge.invoke<SnapshotResponse>(JSON.stringify({ op: 'restore', tasks: restore }));
    for (const snapshot of result.tasks) { const task = this.tasks.get(snapshot.taskId); if (task !== undefined) { const reason = task.snapshot.reason; task.snapshot = snapshot; task.snapshot.reason = reason; } }
  }
  inventory(): string { return JSON.stringify({ active: this.active, pending: this.queue.length, planning: this.planning,
    retainedTasks: this.tasks.size, quarantinedRecords: this.quarantinedRecords, listenerFailures: this.listenerFailures, network: JSON.parse(this.transport.inventory()) }); }
  close(): Promise<void> {
    if (this.closeWork !== undefined) { return this.closeWork; }
    this.closing = true;
    const physical = this.closeNow(); const scope = new OperationScope(10000);
    const work = scope.wait(physical).finally(() => { scope.close(); }); this.closeWork = work;
    physical.catch(() => { this.closeWork = undefined; }); return work;
  }
  private async closeNow(): Promise<void> {
    await this.mutationTail;
    if (!this.closed) {
      if (this.planning > 0) { throw new VesperPlayerError('in_use', 'Download planning must physically finish before close'); }
      for (const task of this.tasks.values()) {
        if (task.job !== undefined) { task.snapshot = this.call({ op: 'pause', taskId: task.snapshot.taskId }).task; this.cancel(task); }
      }
      this.closed = true; this.listeners.clear();
    }
    const scope = new OperationScope(10000);
    try {
      await scope.wait(Promise.all(Array.from(this.tasks.values()).map((task: Task) => task.job?.done ?? Promise.resolve())).then(() => {}));
      await this.checkpoint();
      for (const task of this.tasks.values()) { task.lease?.release(); task.lease = undefined; }
      this.bridge.close(); VesperDownloadManager.roots.delete(this.root);
    } finally { scope.close(); }
  }
}
