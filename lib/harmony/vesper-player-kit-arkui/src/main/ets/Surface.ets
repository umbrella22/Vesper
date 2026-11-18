import { VesperPlayerBinding } from './Binding';

/** Presentation only; disappearance policy belongs to the binding. */
@Component
export struct VesperPlayerSurface {
  @ObjectLink binding: VesperPlayerBinding;
  private attached: string = '';
  aboutToDisappear(): void { this.binding.disappear(); }
  build() {
    XComponent({ id: this.binding.surfaceIdentity, type: XComponentType.SURFACE, controller: this.binding.surface })
      .width('100%').height('100%').backgroundColor(Color.Black)
      .onLoad(() => { try { this.binding.attach(); this.attached = this.binding.surfaceId(); } catch (error) { this.binding.report(Promise.reject(error)); } })
      .onDestroy(() => { try { this.binding.detach(this.attached); } catch (error) { this.binding.report(Promise.reject(error)); } })
      .accessibilityText('Video presentation')
  }
}
