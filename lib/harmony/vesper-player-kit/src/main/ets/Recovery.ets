import { VesperPlayerController } from './Controller';
import { VesperPlayerError, VesperPlayerSnapshot, VesperSourceActivationOptions, VesperSourceActivationResult } from './Types';
import { VesperCancellationToken, OperationScope, monotonicMs, nativeError } from './Operation';
import { RecoveryIntent, captureRecoveryIntent, matchRecoveryTrack } from './RecoveryIntent';

export interface VesperPlaybackRecoveryOptions { maxAttempts?: number; deadlineMs?: number; backoffMs?: number; }
export interface VesperPlaybackRecoverySnapshot {
  state: string; attempts: number; originalEpoch?: string; playbackEpoch?: string; activationId?: string;
  sourceId?: string; positionMs: number; errorCode?: string; nativeCode?: number;
  restorations?: VesperPlaybackRecoveryRestoration[];
}
export interface VesperPlaybackRecoveryRestoration { kind: string; status: string; reason?: string; }
/** The callback must invoke the current controller/sequence activation before its first suspension. */
export type VesperRecoveryActivation = (options: VesperSourceActivationOptions,
  cancellation: VesperCancellationToken) => Promise<VesperSourceActivationResult>;

/** Only known transient failures are eligible. Access, format and source failures require host action. */
export function isTransientPlaybackError(error: VesperPlayerError): boolean {
  return ['timeout', 'network_failure', 'http_408', 'http_429', 'http_500', 'http_502', 'http_503', 'http_504'].includes(error.code) ||
    error.code === 'native_failure' && [5400104, 5411001, 5411002, 5411003, 5411004].includes(error.nativeCode ?? 0);
}

/** Opt-in, bounded recovery through explicit activation, fenced by source, epoch and user intent. */
export class VesperPlaybackRecovery {
  private readonly controller: VesperPlayerController;
  private readonly activate: VesperRecoveryActivation;
  private readonly maximum: number;
  private readonly deadlineMs: number;
  private readonly backoff: number;
  private value: VesperPlaybackRecoverySnapshot = { state: 'watching', attempts: 0, positionMs: 0 };
  private current?: VesperPlayerSnapshot;
  private token?: VesperCancellationToken;
  private work?: Promise<void>;
  private wake?: () => void;
  private deadline: number = 0;
  private revision: number = 0;
  private expectedIntent?: number;
  private invoking: boolean = false;
  private cancelledEpoch?: string;
  private closed: boolean = false;
  private restorations: VesperPlaybackRecoveryRestoration[] = [];
  private unsubscribe: () => void = (): void => {};
  private unregister: () => void = (): void => {};
  constructor(controller: VesperPlayerController, activate: VesperRecoveryActivation, options: VesperPlaybackRecoveryOptions = {}) {
    this.controller = controller; this.activate = activate;
    this.maximum = options.maxAttempts ?? 2; this.deadlineMs = options.deadlineMs ?? 30000; this.backoff = options.backoffMs ?? 500;
    if (!Number.isInteger(this.maximum) || this.maximum < 1 || this.maximum > 3 || !Number.isInteger(this.deadlineMs) ||
      this.deadlineMs < 1 || this.deadlineMs > 60000 || !Number.isInteger(this.backoff) || this.backoff < 0 || this.backoff > 5000) {
      throw new VesperPlayerError('invalid_argument', 'Recovery requires 1–3 attempts, a deadline up to 60 seconds and backoff up to 5 seconds');
    }
    try {
      this.unregister = controller.registerCleanup((): Promise<void> => { this.close(); return Promise.resolve(); });
      this.unsubscribe = controller.subscribe((snapshot: VesperPlayerSnapshot): void => { this.observe(snapshot); });
    } catch (error) { this.close(); throw error as Error; }
  }
  snapshot(): VesperPlaybackRecoverySnapshot { return { state: this.value.state, attempts: this.value.attempts,
    originalEpoch: this.value.originalEpoch, playbackEpoch: this.value.playbackEpoch, activationId: this.value.activationId,
    sourceId: this.value.sourceId, positionMs: this.value.positionMs, errorCode: this.value.errorCode, nativeCode: this.value.nativeCode,
    restorations: this.restorations.map((item: VesperPlaybackRecoveryRestoration): VesperPlaybackRecoveryRestoration =>
      { return { kind: item.kind, status: item.status, reason: item.reason }; }) }; }
  private observe(snapshot: VesperPlayerSnapshot): void {
    if (this.closed) { return; }
    const previous = this.current;
    this.current = snapshot;
    if (snapshot.disposed) { this.cancel('disposed'); return; }
    if (snapshot.playbackEpoch !== previous?.playbackEpoch) {
      if (this.invoking && snapshot.sourceId === previous?.sourceId && snapshot.sessionId === previous?.sessionId) {
        this.value.playbackEpoch = snapshot.playbackEpoch; this.value.activationId = snapshot.activationId;
        this.expectedIntent = snapshot.intentRevision;
      } else {
        this.cancel('sourceChanged'); this.cancelledEpoch = undefined; this.deadline = 0;
        this.restorations = [];
        this.value = { state: 'watching', attempts: 0, positionMs: 0, sourceId: snapshot.sourceId,
          originalEpoch: snapshot.playbackEpoch, playbackEpoch: snapshot.playbackEpoch, activationId: snapshot.activationId };
      }
    }
    if (this.token !== undefined && this.invoking && snapshot.sourceId === previous?.sourceId && snapshot.sessionId === previous?.sessionId) {
      this.expectedIntent = snapshot.intentRevision;
    }
    if (this.token !== undefined && (snapshot.intentRevision !== this.expectedIntent || !snapshot.playWhenReady)) {
      this.cancel('userIntentChanged');
    }
    this.wake?.();
    if (this.work !== undefined || snapshot.error === undefined || snapshot.playbackEpoch === undefined ||
      this.cancelledEpoch === snapshot.playbackEpoch || !snapshot.playWhenReady) { return; }
    this.value.errorCode = snapshot.error.code; this.value.nativeCode = snapshot.error.nativeCode;
    if (!isTransientPlaybackError(snapshot.error)) {
      this.value.state = ['http_401', 'http_403', 'expired', 'source_expired', 'resource_changed'].includes(snapshot.error.code) ||
        snapshot.error.nativeCode === 5411005 ? 'sourceRefreshRequired' : 'hostActionRequired'; return;
    }
    if (this.value.attempts >= this.maximum || this.deadline !== 0 && monotonicMs() >= this.deadline) { this.value.state = 'exhausted'; return; }
    if (this.deadline === 0) { this.deadline = monotonicMs() + this.deadlineMs; }
    const target = snapshot.timeline === 'vod' ? Math.max(0, Math.round(snapshot.positionMs)) : 0;
    const rate = snapshot.timeline === 'vod' ? snapshot.playbackRate : 1;
    this.value.positionMs = target;
    this.expectedIntent = snapshot.intentRevision;
    const token = new VesperCancellationToken(); this.token = token;
    const revision = this.revision;
    // Start after the failing activation has retired its own native instance.
    const intent = captureRecoveryIntent(snapshot);
    const work = Promise.resolve().then(() => this.recover(target, rate, intent, token, revision));
    this.work = work;
    work.catch((error: Error): void => {
      if (revision === this.revision) {
        const failure = nativeError(error, 'playback recovery'); this.value.errorCode = failure.code;
        this.value.nativeCode = failure.nativeCode; this.value.state = 'exhausted';
      }
    }).finally((): void => {
      if (this.work === work) { this.work = undefined; }
      if (this.token === token) { this.token = undefined; }
      if (!this.closed && revision !== this.revision) { this.observe(this.controller.snapshot()); }
    });
  }
  private async recover(position: number, rate: number, intent: RecoveryIntent, token: VesperCancellationToken, revision: number): Promise<void> {
    const scope = new OperationScope(Math.max(1, Math.ceil(this.deadline - monotonicMs())), token);
    try {
      for (; this.value.attempts < this.maximum && revision === this.revision;) {
        scope.check(); this.value.state = 'waiting';
        let timer: number = -1;
        try { await scope.wait(new Promise<void>((resolve) => { timer = setTimeout(resolve, this.backoff * Math.pow(2, this.value.attempts)); })); }
        finally { clearTimeout(timer); }
        scope.check(); this.value.attempts++; this.value.state = 'activating';
        try {
          this.invoking = true;
          let activation: Promise<VesperSourceActivationResult>;
          try { activation = this.activate({ playWhenReady: true, startPositionMs: position, playbackRate: rate,
            timeoutMs: Math.max(1, Math.ceil(this.deadline - monotonicMs())) }, token); }
          finally { this.invoking = false; }
          const result = await scope.wait(activation);
          scope.check();
          if (result.playbackEpoch !== this.value.playbackEpoch || result.sourceId !== this.value.sourceId || result.sessionId !== this.current?.sessionId) {
            throw new VesperPlayerError('stale_epoch', 'Recovery activation no longer owns the expected source');
          }
          this.value.state = 'restoring'; this.restorations = [];
          await this.restore(intent, token, scope, result.playbackEpoch);
          this.value.state = 'awaitingFrame'; await this.waitForFrame(result.playbackEpoch, position, scope);
          scope.check(); this.value.state = this.restorations.every((item: VesperPlaybackRecoveryRestoration): boolean => item.status === 'restored') ? 'recovered' : 'recoveredPartial'; return;
        } catch (error) {
          scope.check(); const failure = nativeError(error, 'recovery activation');
          this.value.errorCode = failure.code; this.value.nativeCode = failure.nativeCode;
          if (!isTransientPlaybackError(failure)) { this.value.state = 'hostActionRequired'; return; }
        }
      }
      if (revision === this.revision) { this.value.state = 'exhausted'; }
    } finally { scope.close(); }
  }
  private async restore(intent: RecoveryIntent, token: VesperCancellationToken, scope: OperationScope, epoch: string): Promise<void> {
    const remaining = (): number => Math.max(1, Math.min(10000, Math.ceil(this.deadline - monotonicMs())));
    const apply = async (kind: string, action: () => Promise<void>): Promise<void> => {
      scope.check();
      if (this.controller.snapshot().playbackEpoch !== epoch) { throw new VesperPlayerError('stale_epoch', 'Recovery no longer owns this epoch'); }
      try {
        this.invoking = true;
        let work: Promise<void>;
        try { work = action(); } finally { this.invoking = false; }
        await scope.wait(work); scope.check();
        this.restorations.push({ kind: kind, status: 'restored' });
      } catch (error) {
        scope.check();
        this.restorations.push({ kind: kind, status: 'failed', reason: nativeError(error, 'recovery choice').code });
      }
    };
    const constraints = intent.constraints;
    if (constraints !== undefined) {
      await apply('constraints', (): Promise<void> => this.controller.setTrackConstraints(constraints, token, remaining()).then(() => {}));
    }
    for (const track of intent.tracks) {
      scope.check();
      const match = matchRecoveryTrack(track, this.controller.snapshot().tracks);
      const selection = match.selection;
      if (selection === undefined) { this.restorations.push({ kind: track.kind, status: 'unmatched', reason: match.reason }); continue; }
      await apply(track.kind, (): Promise<void> => this.controller.selectTrack(selection, token, remaining()));
    }
  }
  private waitForFrame(epoch: string, position: number, scope: OperationScope): Promise<void> {
    return scope.wait(new Promise<void>((resolve, reject) => {
      this.wake = (): void => {
        const snapshot = this.controller.snapshot();
        if (snapshot.playbackEpoch !== epoch) { reject(new VesperPlayerError('stale_epoch', 'Recovery frame belongs to an older epoch')); }
        else if (snapshot.error !== undefined) { reject(snapshot.error); }
        else if (snapshot.firstFrame && snapshot.state === 'playing' && snapshot.positionMs >= Math.max(0, position - 1500)) { resolve(); }
      };
      this.wake();
    })).finally((): void => { this.wake = undefined; });
  }
  cancel(reason: string = 'hostCancelled'): void {
    this.revision++; this.cancelledEpoch = this.current?.playbackEpoch;
    this.token?.cancel(); this.token = undefined; this.value.state = 'cancelled'; this.value.errorCode = reason;
  }
  close(): void {
    if (this.closed) { return; } this.closed = true; this.cancel('closed'); this.unsubscribe(); this.unregister();
  }
}
