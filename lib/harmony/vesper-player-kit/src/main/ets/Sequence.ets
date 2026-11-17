import { NativeSession } from './NativeSession';
import { VesperPlayerController } from './Controller';
import { VesperSourceHandle, VesperSourceLease } from './SourceSession';
import { VesperPlayerError, VesperSourceActivationOptions, VesperSourceActivationResult } from './Types';
import { VesperCancellationToken, OperationScope, nativeError } from './Operation';

export interface VesperSequenceConfiguration {
  sequenceId: string; mode?: string; historyLimit?: number; forwardWindow?: number;
  refillThreshold?: number; maxItems?: number; maxPendingRequests?: number; maxEvents?: number;
  requestTimeoutMs?: number; sourceExpiryLeadMs?: number;
}
export interface VesperSequenceItem {
  itemId: string; providerNamespace: string; contentIdentity: string; mediaKind: string;
  source?: VesperSourceHandle;
}
export interface VesperSequenceRequest {
  type: string; sequenceId: string; sessionGeneration: string; requestId: string;
  resolutionAttemptId?: string; itemId?: string; expectedSourceRevision?: string;
  direction?: string; anchorItemId?: string; maxCount?: number; deadlineRemainingMs: number;
}
interface PendingRequest { request: VesperSequenceRequest; deadlineRemainingMs: number; }
interface SourceState { state: string; sourceReference?: string; sourceRevision?: string; }
interface AcceptedItem { itemId: string; sourceState: SourceState; }
interface ItemSnapshot { item: AcceptedItem; index: number; isActive: boolean; }
export interface VesperSequenceSnapshot {
  sequenceId: string; sessionGeneration: string; activationEpoch: string; activeItemId?: string;
  items: ItemSnapshot[]; pendingRequests: PendingRequest[]; previousEndReached: boolean; nextEndReached: boolean;
}
export interface VesperSequenceNavigation { outcome: string; itemId?: string; activationEpoch?: string; requestId?: string; }
export interface VesperSequenceResolution { source: VesperSourceHandle; sourceRevision: string; }
interface CacheIdentity {
  providerNamespace: string; contentIdentity: string; renditionIdentity: string; resourceIdentity: string;
  accessPartition: string; sourceRevision: string;
}
interface ResolvedSource { sourceReference: string; cacheIdentity: CacheIdentity; warmupGoal: string; expiresAtEpochMs?: number; }
interface ItemWire {
  itemId: string; providerNamespace: string; contentIdentity: string; mediaKind: string;
  resolvedSource?: ResolvedSource;
}
interface WarmupIntent {
  sessionGeneration: string; itemId: string; sourceReference: string; sourceRevision: string; warmupTaskId: string; warmupGoal: string;
}
interface EventsBatch { events: Object[]; }
interface WarmupBatch { intents: WarmupIntent[]; }
interface ResolutionWire {
  sessionGeneration: string; requestId: string; resolutionAttemptId?: string; itemId?: string;
  expectedSourceRevision?: string; sourceRevision: string; sourceReference: string;
  cacheIdentity: CacheIdentity; warmupGoal: string;
  expiresAtEpochMs?: number;
}
interface CommandWire {
  type: string; items?: ItemWire[]; activeItemId?: string; sessionGeneration?: string;
  requestId?: string; anchorItemId?: string; endReached?: boolean; itemId?: string;
  activationEpoch?: string; sourceRevision?: string; source?: ResolutionWire;
  taskId?: string; warmupGoal?: string; status?: string; expectedBytes?: string; actualBytes?: string;
  cacheHit?: boolean; cacheEntries?: number; cacheBytes?: string; evictedEntries?: string; reasonCode?: string;
}

/** Membership and navigation are authoritative Rust operations; playback remains explicit. */
export class VesperPlaybackSequence {
  readonly sequenceId: string;
  private bridge: NativeSession;
  private leases: Map<string, VesperSourceLease> = new Map();
  private controller?: VesperPlayerController;
  private unsubscribe: () => void = (): void => {};
  private pendingActivation?: VesperCancellationToken;
  private resolverWork: Map<string, OperationScope> = new Map();
  private closed: boolean = false;
  constructor(configuration: VesperSequenceConfiguration) {
    this.sequenceId = configuration.sequenceId;
    this.bridge = new NativeSession(JSON.stringify({ kind: 'sequence', configuration: configuration }));
  }
  snapshot(): VesperSequenceSnapshot { this.check(); return this.bridge.invoke<VesperSequenceSnapshot>('{"op":"snapshot"}'); }
  drainEvents(): Object[] {
    this.check(); const batch = this.bridge.invoke<EventsBatch>('{"op":"events"}'); return batch.events;
  }
  private check(): void { if (this.closed) { throw new VesperPlayerError('closed', 'Sequence was closed'); } }
  private command<T>(command: CommandWire): T { this.check(); return this.bridge.invoke<T>(JSON.stringify({ op: 'command', command: command })); }
  private reference(lease: VesperSourceLease): string { return `${lease.sessionId}:${lease.sourceId}`; }
  private resolved(lease: VesperSourceLease, revision: string, itemId: string): ResolvedSource {
    return { sourceReference: this.reference(lease), warmupGoal: lease.descriptor.protocol === 'dash' ? 'dashSegmentBaseStartup' : 'progressiveRange',
      expiresAtEpochMs: lease.expiresAtEpochMs,
      cacheIdentity: { providerNamespace: 'host', contentIdentity: itemId, renditionIdentity: 'native',
        resourceIdentity: 'media', accessPartition: this.reference(lease), sourceRevision: revision } };
  }
  private acceptItems(items: VesperSequenceItem[], operation: string, request?: VesperSequenceRequest,
    activeItemId?: string, endReached: boolean = false): void {
    this.check();
    if (items.length > 512) { throw new VesperPlayerError('capacity', 'Sequence batch exceeds 512 items'); }
    const candidates: Map<string, VesperSourceLease> = new Map();
    const wire: ItemWire[] = [];
    try {
      for (const item of items) {
        let source: ResolvedSource | undefined;
        if (item.source !== undefined) {
          const lease = item.source.acquireLease(); const reference = this.reference(lease);
          if (!this.leases.has(reference) && !candidates.has(reference)) { candidates.set(reference, lease); }
          else { lease.release(); }
          const retained = candidates.get(reference) ?? this.leases.get(reference);
          if (retained === undefined) { throw new VesperPlayerError('stale_source', 'Sequence source is unavailable'); }
          source = this.resolved(retained, '1', item.itemId);
        }
        wire.push({ itemId: item.itemId, providerNamespace: item.providerNamespace,
          contentIdentity: item.contentIdentity, mediaKind: item.mediaKind, resolvedSource: source });
      }
      this.pendingActivation?.cancel();
      if (operation === 'replace') { this.command<Object>({ type: operation, items: wire, activeItemId: activeItemId }); }
      else {
        if (request === undefined) { throw new VesperPlayerError('invalid_argument', 'Page response requires its pending request'); }
        this.command<Object>({ type: operation, items: wire, sessionGeneration: request.sessionGeneration,
          requestId: request.requestId, anchorItemId: request.anchorItemId, endReached: endReached });
      }
      for (const entry of candidates) { this.leases.set(entry[0], entry[1]); }
      candidates.clear(); this.reconcile();
    } finally { for (const lease of candidates.values()) { lease.release(); } }
  }
  replace(items: VesperSequenceItem[], activeItemId?: string): void { this.acceptItems(items, 'replace', undefined, activeItemId); }
  append(request: VesperSequenceRequest, items: VesperSequenceItem[], endReached: boolean): void { this.acceptItems(items, 'append', request, undefined, endReached); }
  prepend(request: VesperSequenceRequest, items: VesperSequenceItem[], endReached: boolean): void { this.acceptItems(items, 'prepend', request, undefined, endReached); }
  remove(itemId: string): void { this.pendingActivation?.cancel(); this.command<Object>({ type: 'remove', itemId: itemId }); this.reconcile(); }
  markSourceExpired(itemId: string, sourceRevision: string): void {
    this.pendingActivation?.cancel(); this.command<Object>({ type: 'markSourceExpired', itemId: itemId, sourceRevision: sourceRevision }); this.reconcile();
  }
  jump(itemId: string): VesperSequenceNavigation {
    this.pendingActivation?.cancel(); const result = this.command<VesperSequenceNavigation>({ type: 'setActive', itemId: itemId }); this.reconcile(); return result;
  }
  next(): VesperSequenceNavigation { this.pendingActivation?.cancel(); const result = this.command<VesperSequenceNavigation>({ type: 'next' }); this.reconcile(); return result; }
  previous(): VesperSequenceNavigation { this.pendingActivation?.cancel(); const result = this.command<VesperSequenceNavigation>({ type: 'previous' }); this.reconcile(); return result; }
  attach(controller: VesperPlayerController): void {
    this.check(); this.detach(); controller.claimSequence(this.bridge.identity); this.controller = controller;
    this.unsubscribe = controller.subscribe((snapshot): void => { if (snapshot.disposed) { this.detach(); } });
  }
  detach(): void {
    this.pendingActivation?.cancel(); this.pendingActivation = undefined; this.unsubscribe(); this.unsubscribe = (): void => {};
    this.controller?.detachSequence(this.bridge.identity); this.controller = undefined;
  }
  async activate(options: VesperSourceActivationOptions = {}, cancellation?: VesperCancellationToken): Promise<VesperSourceActivationResult> {
    this.check(); const controller = this.controller;
    if (controller === undefined) { throw new VesperPlayerError('detached', 'Sequence is detached'); }
    this.command<Object>({ type: 'tick' }); const snapshot = this.snapshot();
    const active = snapshot.items.find((item: ItemSnapshot) => item.item.itemId === snapshot.activeItemId);
    const state = active?.item.sourceState;
    if (state === undefined || state.state !== 'resolved' || state.sourceReference === undefined || state.sourceRevision === undefined) {
      throw new VesperPlayerError('source_unresolved', 'Active item requires source resolution');
    }
    const retained = this.leases.get(state.sourceReference);
    if (retained === undefined || active === undefined) { throw new VesperPlayerError('stale_source', 'Active source lease is unavailable'); }
    this.pendingActivation?.cancel(); const token = new VesperCancellationToken(); this.pendingActivation = token;
    const remove = cancellation?.observe(() => { token.cancel(); }) ?? ((): void => {});
    try {
      const result = await controller.activateSequence(this.bridge.identity, retained.retain(), options, token);
      this.command<Object>({ type: 'validateActivationCallback', itemId: active.item.itemId,
        activationEpoch: snapshot.activationEpoch, sourceRevision: state.sourceRevision });
      return result;
    } finally { remove(); if (this.pendingActivation === token) { this.pendingActivation = undefined; } }
  }
  async resolvePending(resolver: (request: VesperSequenceRequest) => Promise<VesperSequenceResolution>): Promise<void> {
    this.command<Object>({ type: 'tick' });
    const work: Promise<void>[] = [];
    for (const pending of this.snapshot().pendingRequests) {
      const request = pending.request;
      if (request.type !== 'sourceResolutionRequired' || this.resolverWork.size >= 4) { continue; }
      const key = `${request.sessionGeneration}:${request.requestId}`;
      if (this.resolverWork.has(key)) { continue; }
      const scope = new OperationScope(Math.max(1, Math.min(request.deadlineRemainingMs, 60000)));
      this.resolverWork.set(key, scope);
      // The physical resolver remains counted after its logical deadline.
      const physical = Promise.resolve().then(() => resolver(request)).then((resolved: VesperSequenceResolution): void => {
        scope.check(); this.check(); const lease = resolved.source.acquireLease(); let accepted = false;
        try {
          const source = this.resolved(lease, resolved.sourceRevision, request.itemId ?? '');
          this.command<Object>({ type: 'submitResolvedSource', source: {
            sessionGeneration: request.sessionGeneration, requestId: request.requestId,
            resolutionAttemptId: request.resolutionAttemptId, itemId: request.itemId,
            expectedSourceRevision: request.expectedSourceRevision, sourceRevision: resolved.sourceRevision,
            sourceReference: source.sourceReference, cacheIdentity: source.cacheIdentity, warmupGoal: source.warmupGoal,
            expiresAtEpochMs: source.expiresAtEpochMs
          } });
          if (!this.leases.has(source.sourceReference)) { this.leases.set(source.sourceReference, lease); accepted = true; }
          this.reconcile();
        } finally { if (!accepted) { lease.release(); } }
      });
      physical.finally(() => { scope.close(); this.resolverWork.delete(key); }).catch(() => {});
      work.push(scope.wait(physical));
    }
    await Promise.all(work);
  }
  async warmup(): Promise<void> {
    const batch = this.bridge.invoke<WarmupBatch>('{"op":"preloadIntents"}');
    if (batch.intents.length > 16) { throw new VesperPlayerError('capacity', 'Sequence warmup batch exceeds sixteen intents'); }
    const work = batch.intents.map(async (intent: WarmupIntent): Promise<void> => {
      const lease = this.leases.get(intent.sourceReference);
      if (lease === undefined) { return; }
      try {
        const task = lease.preload({}); await task.completion;
        if (!this.closed) {
          const result = task.snapshot();
          this.command<Object>({ type: 'reportWarmup', sessionGeneration: intent.sessionGeneration,
            taskId: intent.warmupTaskId, itemId: intent.itemId, sourceRevision: intent.sourceRevision,
            warmupGoal: intent.warmupGoal, status: 'completed', expectedBytes: `${result.receivedBytes}`,
            actualBytes: `${result.receivedBytes}`, cacheHit: result.reuseLevel === 'playbackReusable',
            cacheEntries: 1, cacheBytes: `${result.receivedBytes}`, evictedEntries: '0' });
        }
      } catch (error) {
        if (!this.closed) { this.command<Object>({ type: 'reportWarmup', sessionGeneration: intent.sessionGeneration,
          taskId: intent.warmupTaskId, itemId: intent.itemId, sourceRevision: intent.sourceRevision,
          warmupGoal: intent.warmupGoal, status: 'failed', expectedBytes: '0', actualBytes: '0',
          reasonCode: nativeError(error, 'sequence warmup').code }); }
      }
    });
    await Promise.all(work);
  }
  private reconcile(): void {
    const references = new Set<string>();
    for (const entry of this.snapshot().items) { if (entry.item.sourceState.sourceReference !== undefined) { references.add(entry.item.sourceState.sourceReference); } }
    for (const entry of this.leases) { if (!references.has(entry[0])) { entry[1].release(); this.leases.delete(entry[0]); } }
  }
  close(): void {
    if (this.closed) { return; } this.detach();
    for (const scope of this.resolverWork.values()) { scope.fail(new VesperPlayerError('closed', 'Sequence was closed')); }
    for (const entry of this.leases) { entry[1].release(); this.leases.delete(entry[0]); }
    this.bridge.close(); this.closed = true;
  }
}
