import { VesperPlayerController, VesperPlayerSnapshot, VesperPlayerError } from '@vesper/player-kit';
import { VesperOperationFeedback, playbackFeedback } from './Feedback';
import { VesperWindowPolicy } from './WindowOwnership';

export interface VesperBindingConfiguration { onDisappear?: string; releaseController?: boolean; keepScreenOnDuringPlayback?: boolean; }

/** A stable XComponent and controller survive control layout and window changes. */
@Observed
export class VesperPlayerBinding {
  private static identity: number = 0;
  readonly surfaceIdentity: string = `vesper-native-surface-${++VesperPlayerBinding.identity}`;
  readonly controller: VesperPlayerController;
  readonly surface: XComponentController = new XComponentController();
  snapshot: VesperPlayerSnapshot;
  error: string = '';
  feedback: VesperOperationFeedback | undefined = undefined;
  rangeObservationError: string = '';
  windowFeedback: VesperOperationFeedback | undefined = undefined;
  private windowPolicy?: VesperWindowPolicy;
  private windowVisible: boolean = true;
  private keepScreenOn: boolean = true;
  private windowKey: string = '';
  private unregisterWindow: () => void = (): void => {};
  private readonly configuration: VesperBindingConfiguration;
  private unsubscribe: () => void;
  private currentSurface: string = '';
  private closed: boolean = false;
  private closing?: Promise<void>;
  private commandGeneration: number = 0;
  constructor(controller: VesperPlayerController, configuration: VesperBindingConfiguration = {}) {
    if (!['keep', 'pause', 'dispose'].includes(configuration.onDisappear ?? 'keep')) { throw new VesperPlayerError('invalid_argument', 'Unknown view disappearance policy'); }
    this.controller = controller; this.configuration = { onDisappear: configuration.onDisappear, releaseController: configuration.releaseController };
    this.keepScreenOn = configuration.keepScreenOnDuringPlayback ?? true;
    this.snapshot = controller.snapshot();
    this.unsubscribe = this.observe();
  }
  private observe(): () => void {
    return this.controller.subscribe((snapshot: VesperPlayerSnapshot): void => {
      if (snapshot.state === 'initializing' && (this.snapshot.state !== 'initializing' || snapshot.playbackEpoch !== this.snapshot.playbackEpoch)) {
        this.commandGeneration++; this.error = ''; this.feedback = undefined;
      }
      this.snapshot = snapshot;
      this.updateWindow();
    });
  }
  attach(): void {
    if (this.closed) { return; } const surface = this.surface.getXComponentSurfaceId();
    if (surface.length === 0) { throw new VesperPlayerError('not_ready', 'XComponent Surface is not ready'); }
    // ArkUI wraps @Observed instances after construction. Subscribe through that
    // proxy when the Surface mounts so native callbacks notify the UI owner.
    if (this.currentSurface.length === 0) { this.unsubscribe(); this.unsubscribe = this.observe(); }
    this.controller.attachSurface(surface); this.currentSurface = surface;
  }
  detach(surface: string): void {
    if (surface !== this.currentSurface) { return; }
    if (!this.controller.snapshot().disposed) { this.controller.detachSurface(); }
    this.currentSurface = '';
  }
  surfaceId(): string { return this.currentSurface; }
  attachWindowPolicy(policy: VesperWindowPolicy): void {
    if (this.closed || this.windowPolicy !== undefined) { throw new VesperPlayerError('invalid_state', 'Binding window policy is already owned or closed'); }
    const unregister = this.controller.registerCleanup((): Promise<void> => policy.close());
    this.windowPolicy = policy; this.unregisterWindow = unregister; this.windowKey = ''; this.updateWindow();
  }
  setPresentationVisible(visible: boolean): void { this.windowVisible = visible; this.updateWindow(); }
  setKeepScreenOnDuringPlayback(enabled: boolean): void { this.keepScreenOn = enabled; this.updateWindow(); }
  private updateWindow(): void {
    if (this.windowPolicy === undefined || this.closed || this.snapshot.disposed) { return; }
    const video = this.snapshot.state === 'playing' && this.snapshot.width > 0 && this.snapshot.height > 0;
    const key = this.windowVisible + ':' + video + ':' + this.keepScreenOn;
    if (key === this.windowKey) { return; } this.windowKey = key;
    this.windowPolicy.update(this.windowVisible, video, this.keepScreenOn).then((): void => { this.windowFeedback = undefined; },
      (error: Error): void => { this.windowFeedback = playbackFeedback(error); });
  }
  report(work: Promise<void>): void {
    const generation = ++this.commandGeneration;
    work.then(() => { if (generation === this.commandGeneration) { this.clearFeedback(); } },
      (error: Error) => {
        if (generation === this.commandGeneration) {
          this.feedback = playbackFeedback(error);
          this.error = this.feedback.category === 'cancelled' ? '' : `${error}`;
        }
      });
  }
  clearFeedback(): void { this.error = ''; this.feedback = undefined; }
  disappear(): void {
    if (this.closed || this.snapshot.disposed) { return; }
    if (this.configuration.onDisappear === 'pause') { this.report(this.controller.pause()); }
    else if (this.configuration.onDisappear === 'dispose') { this.report(this.close(true)); }
  }
  close(releaseController: boolean = this.configuration.releaseController ?? false): Promise<void> {
    if (this.closing !== undefined) { return this.closing; } this.closed = true; this.unsubscribe();
    const work = Promise.resolve().then(async (): Promise<void> => {
      await this.windowPolicy?.close(); this.unregisterWindow();
      if (releaseController) { await this.controller.dispose(); }
      else { this.detach(this.currentSurface); }
    });
    this.closing = work; work.catch(() => { this.closing = undefined; }); return work;
  }
}
