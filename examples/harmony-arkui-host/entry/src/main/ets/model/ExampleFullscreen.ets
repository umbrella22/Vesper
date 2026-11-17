import { window } from '@kit.ArkUI';

interface FullscreenBaseline { layout: boolean; orientation: window.Orientation; }

/** App-owned fullscreen policy; the Stage and Surface stay mounted. */
export class ExampleFullscreen {
  private nativeWindow: window.Window;
  private baseline?: FullscreenBaseline;
  private active: boolean = false;
  private closed: boolean = false;
  private change?: Promise<void>;
  private closing?: Promise<void>;
  constructor(nativeWindow: window.Window) { this.nativeWindow = nativeWindow; }

  async set(value: boolean, landscape: boolean): Promise<void> {
    if (this.closed) { throw new Error('Fullscreen presentation is closed'); }
    if (this.change !== undefined) { throw new Error('Fullscreen presentation is changing'); }
    if (value === this.active) { return; }
    const work = this.apply(value, landscape); this.change = work;
    try { await work; } finally { if (this.change === work) { this.change = undefined; } }
  }
  private async apply(value: boolean, landscape: boolean): Promise<void> {
    if (!value) { await this.restore(); return; }
    this.baseline = { layout: this.nativeWindow.getWindowProperties().isLayoutFullScreen,
      orientation: this.nativeWindow.getPreferredOrientation() };
    try {
      if (landscape) { await this.nativeWindow.setPreferredOrientation(window.Orientation.LANDSCAPE); }
      await this.nativeWindow.setWindowLayoutFullScreen(true);
      this.active = true;
    } catch (error) {
      await this.restore(); throw error as Error;
    }
  }
  private async restore(): Promise<void> {
    const baseline = this.baseline; if (baseline === undefined) { return; }
    let failure: Error | undefined;
    try { await this.nativeWindow.setWindowLayoutFullScreen(baseline.layout); } catch (error) { failure = error as Error; }
    try { await this.nativeWindow.setPreferredOrientation(baseline.orientation); } catch (error) { failure = failure ?? error as Error; }
    if (failure !== undefined) { throw failure; }
    this.baseline = undefined; this.active = false;
  }
  close(): Promise<void> {
    this.closed = true;
    if (this.closing !== undefined) { return this.closing; }
    // Finish an in-flight native change before restoring the original policy.
    const work = (this.change ?? Promise.resolve()).catch((): void => {}).then((): Promise<void> => this.restore());
    this.closing = work;
    work.catch((): void => { this.closing = undefined; });
    return work;
  }
}
