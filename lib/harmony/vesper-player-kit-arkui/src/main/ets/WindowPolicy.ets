import { window } from '@kit.ArkUI';
import { VesperWindowAdapter, VesperWindowValues } from './WindowOwnership';

/** The native adapter performs no policy decisions. */
export class VesperNativeWindowAdapter implements VesperWindowAdapter {
  readonly identity: string;
  private nativeWindow: window.Window;
  constructor(nativeWindow: window.Window) {
    this.identity = 'window-' + nativeWindow.getWindowProperties().id; this.nativeWindow = nativeWindow;
  }
  read(): VesperWindowValues {
    const properties = this.nativeWindow.getWindowProperties();
    return { keepScreenOn: properties.isKeepScreenOn, brightness: properties.brightness };
  }
  setKeepScreenOn(value: boolean): Promise<void> { return this.nativeWindow.setWindowKeepScreenOn(value); }
  setBrightness(value: number): Promise<void> { return this.nativeWindow.setWindowBrightness(value); }
}
