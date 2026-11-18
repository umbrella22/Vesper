import { PiPWindow } from '@kit.ArkUI';
import { common } from '@kit.AbilityKit';
import { VesperPlayerError, VesperPlayerSnapshot } from '@vesper/player-kit';
import { VesperPlayerBinding } from './Binding';

export interface VesperPiPSnapshot {
  state: string; autoStart: boolean; errorCode?: number; unknownControl?: string;
  contentWidth?: number; contentHeight?: number; contentEpoch?: string; sizeOrigin?: string; sizeState?: string; sizeErrorCode?: number;
}
interface ContentSize { width: number; height: number; origin: string; }
interface SizeFailure { code?: number; }
function deadline(work: Promise<void>, onExpired?: () => void): Promise<void> {
  let timer: number = 0;
  const expired = new Promise<void>((_, reject) => { timer = setTimeout(() => {
    if (onExpired !== undefined) { onExpired(); }
    reject(new VesperPlayerError('timeout', 'PiP transition deadline expired'));
  }, 10000); });
  return Promise.race([work, expired]).finally(() => { clearTimeout(timer); });
}

/** PiP borrows the original XComponent; it never creates another player. */
export class VesperPictureInPicture {
  private native?: PiPWindow.PiPController;
  private binding: VesperPlayerBinding;
  private creation: Promise<void> = Promise.resolve();
  private tail: Promise<void> = Promise.resolve();
  private pending: number = 0;
  private closing: boolean = false;
  private closeWork?: Promise<void>;
  private removeCleanup: () => void;
  private unsubscribe: () => void = (): void => {};
  private value: VesperPiPSnapshot = { state: 'creating', autoStart: false };
  private nativeState: number = 0;
  private fallbackSize?: ContentSize;
  private sizeKey: string = '';
  private constructor(binding: VesperPlayerBinding) {
    this.binding = binding; this.removeCleanup = binding.controller.registerCleanup(() => this.close());
    try {
      const rect = binding.surface.getXComponentSurfaceRect();
      const width = Math.round(rect.surfaceWidth); const height = Math.round(rect.surfaceHeight);
      if (Number.isSafeInteger(width) && Number.isSafeInteger(height) && width > 0 && height > 0) {
        this.fallbackSize = { width: width, height: height, origin: 'surface' };
      }
    } catch (error) { this.value.sizeErrorCode = (error as SizeFailure).code; }
  }
  private contentSize(snapshot: VesperPlayerSnapshot): ContentSize | undefined {
    return Number.isSafeInteger(snapshot.width) && snapshot.width > 0 && Number.isSafeInteger(snapshot.height) && snapshot.height > 0 ?
      { width: snapshot.width, height: snapshot.height, origin: 'media' } : this.fallbackSize;
  }
  private updateContentSize(snapshot: VesperPlayerSnapshot, force: boolean = false): void {
    if (this.closing || snapshot.disposed || this.native === undefined) { return; }
    const size = this.contentSize(snapshot);
    if (size === undefined) { this.value.sizeState = 'unavailable'; return; }
    const key = `${snapshot.playbackEpoch ?? ''}:${size.origin}:${size.width}:${size.height}`;
    if (!force && key === this.sizeKey) { return; } this.sizeKey = key;
    try {
      this.native.updateContentSize(size.width, size.height);
      this.value.contentWidth = size.width; this.value.contentHeight = size.height; this.value.contentEpoch = snapshot.playbackEpoch;
      this.value.sizeOrigin = size.origin; this.value.sizeState = 'applied'; this.value.sizeErrorCode = undefined;
    } catch (error) { this.value.sizeState = 'failed'; this.value.sizeErrorCode = (error as SizeFailure).code; }
  }
  static async create(context: common.UIAbilityContext, binding: VesperPlayerBinding): Promise<VesperPictureInPicture> {
    if (!PiPWindow.isPiPEnabled()) { throw new VesperPlayerError('unsupported', 'PiP is unavailable on this system'); }
    if (binding.surfaceId().length === 0) { throw new VesperPlayerError('not_ready', 'Attach the original Surface before configuring PiP'); }
    const pip = new VesperPictureInPicture(binding);
    const initial = binding.controller.snapshot(); const size = pip.contentSize(initial);
    const configuration: PiPWindow.PiPConfiguration = { context: context, componentController: binding.surface,
      templateType: PiPWindow.PiPTemplateType.VIDEO_PLAY };
    if (size !== undefined) { configuration.contentWidth = size.width; configuration.contentHeight = size.height; }
    pip.creation = Promise.resolve().then(() => PiPWindow.create(configuration)).then((native: PiPWindow.PiPController): void => {
      pip.native = native; if (pip.closing) { return; }
      pip.value.state = 'ready';
      pip.value.sizeOrigin = size?.origin ?? 'platformDefault';
      native.on('stateChange', (state: PiPWindow.PiPState): void => {
        pip.nativeState = state; pip.value.state = `native:${state}`;
        if (state === PiPWindow.PiPState.STARTED) { pip.updateContentSize(binding.controller.snapshot(), true); }
      });
      native.on('controlEvent', (event: PiPWindow.ControlEventParam): void => {
        if (pip.closing) { return; }
        if (event.controlType === PiPWindow.PiPControlType.VIDEO_PLAY_PAUSE) {
          binding.report(event.status === PiPWindow.PiPControlStatus.PLAY ? binding.controller.play() : binding.controller.pause());
        } else if (event.controlType === PiPWindow.PiPControlType.MUTE_SWITCH) { binding.report(binding.controller.setMuted(event.status === PiPWindow.PiPControlStatus.CLOSE)); }
        else { pip.value.unknownControl = `${event.controlType}:${event.status}`; }
      });
      pip.unsubscribe = binding.controller.subscribe((snapshot): void => {
        pip.updateContentSize(snapshot);
        if (!pip.closing) { try { native.updatePiPControlStatus(PiPWindow.PiPControlType.VIDEO_PLAY_PAUSE,
          snapshot.state === 'playing' ? PiPWindow.PiPControlStatus.PLAY : PiPWindow.PiPControlStatus.PAUSE); } catch (_) {} }
      });
    });
    try { await deadline(pip.creation); return pip; }
    catch (error) { pip.close().catch(() => {}); throw error as Error; }
  }
  snapshot(): VesperPiPSnapshot { return { state: this.value.state, autoStart: this.value.autoStart,
    errorCode: this.value.errorCode, unknownControl: this.value.unknownControl,
    contentWidth: this.value.contentWidth, contentHeight: this.value.contentHeight, contentEpoch: this.value.contentEpoch,
    sizeOrigin: this.value.sizeOrigin, sizeState: this.value.sizeState, sizeErrorCode: this.value.sizeErrorCode }; }
  private transition(action: (native: PiPWindow.PiPController) => Promise<void>): Promise<void> {
    if (this.closing || this.pending >= 4) { return Promise.reject(new VesperPlayerError(this.closing ? 'closed' : 'capacity', 'PiP transition is unavailable')); }
    this.pending++;
    let expired: boolean = false;
    const work = this.tail.then(async (): Promise<void> => {
      if (expired) { throw new VesperPlayerError('timeout', 'PiP transition deadline expired'); }
      if (this.closing || this.native === undefined) { throw new VesperPlayerError('closed', 'PiP is closing'); }
      await action(this.native);
    });
    // Keep ownership until native work settles, even after the caller's deadline.
    this.tail = work.then(() => { this.pending--; }, () => { this.pending--; });
    return deadline(work, (): void => { expired = true; });
  }
  private async untilState(expected: number): Promise<void> {
    for (let count = 0; count < 500; count++) {
      if (this.nativeState === expected) { return; }
      if (this.nativeState === PiPWindow.PiPState.ERROR) { throw new VesperPlayerError('native_failure', 'PiP reported a native lifecycle error'); }
      await new Promise<void>((resolve) => { setTimeout(resolve, 20); });
    }
    throw new VesperPlayerError('timeout', 'PiP state confirmation deadline expired');
  }
  start(): Promise<void> { return this.transition(async (native): Promise<void> => { await native.startPiP(); await this.untilState(PiPWindow.PiPState.STARTED); }); }
  private async stopNative(native: PiPWindow.PiPController): Promise<void> {
    if (this.nativeState === 0 || this.nativeState === PiPWindow.PiPState.STOPPED) { return; }
    if (this.nativeState !== PiPWindow.PiPState.ABOUT_TO_STOP) { await native.stopPiP(); }
    await this.untilState(PiPWindow.PiPState.STOPPED);
  }
  stop(): Promise<void> { return this.transition((native) => this.stopNative(native)); }
  setAutoStart(enabled: boolean): Promise<void> { return this.transition(async (native): Promise<void> => {
    native.setAutoStartEnabled(enabled); this.value.autoStart = enabled;
  }); }
  close(): Promise<void> {
    if (this.closeWork !== undefined) { return deadline(this.closeWork); } this.closing = true;
    const work = Promise.resolve().then(async (): Promise<void> => {
      await this.creation.catch(() => {}); await this.tail;
      if (this.native !== undefined) {
        this.native.setAutoStartEnabled(false);
        await this.stopNative(this.native);
        this.native.off('controlEvent'); this.native.off('stateChange'); this.native = undefined;
      }
      this.unsubscribe(); this.removeCleanup(); this.value.state = 'closed'; this.value.autoStart = false;
    });
    this.closeWork = work; work.catch(() => { this.closeWork = undefined; }); return deadline(work);
  }
}
