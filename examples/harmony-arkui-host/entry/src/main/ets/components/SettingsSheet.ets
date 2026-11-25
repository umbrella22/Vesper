import { ExampleModel } from '../model/ExampleModel';
import { examplePalette, ExamplePalette } from '../model/ExampleSupport';

/** App settings shown in a half-modal sheet from the title-bar gear. */
@Component
export struct SettingsSheet {
  @ObjectLink model: ExampleModel;
  @Prop palette: ExamplePalette = examplePalette(false);
  @Prop reducedMotion: boolean = false;
  onReducedMotionChange: (value: boolean) => void = (): void => {};
  private text(key: string): string { return this.model.text('example_' + key); }
  @Builder label(key: string) {
    Text(this.text(key)).fontSize(16).fontColor(this.palette.title).layoutWeight(1)
  }
  build() {
    Column({ space: 8 }) {
      Row() {
        this.label('appearance')
        Select([{ value: this.text('system') }, { value: this.text('light') }, { value: this.text('dark') }])
          .selected(['system', 'light', 'dark'].indexOf(this.model.theme))
          .value(this.text(this.model.theme === 'dark' ? 'dark' : this.model.theme === 'light' ? 'light' : 'system'))
          .onSelect((index: number): void => { this.model.setTheme(['system', 'light', 'dark'][index]); }).accessibilityText(this.text('appearance'))
      }.width('100%').constraintSize({ minHeight: 56 })
      Row() {
        this.label('inline_controls')
        Select([{ value: this.text('compact') }, { value: this.text('expanded') }, { value: this.text('classic') }])
          .selected(['compact', 'expanded', 'standard'].indexOf(this.model.layout))
          .value(this.text(this.model.layout === 'standard' ? 'classic' : this.model.layout === 'expanded' ? 'expanded' : 'compact'))
          .onSelect((index: number): void => { this.model.layout = ['compact', 'expanded', 'standard'][index]; }).accessibilityText(this.text('inline_controls'))
      }.width('100%').constraintSize({ minHeight: 56 })
      Row() {
        this.label('fullscreen_controls')
        Select([{ value: this.text('expanded') }, { value: this.text('compact') }])
          .selected(this.model.fullscreenLayout === 'compact' ? 1 : 0)
          .value(this.text(this.model.fullscreenLayout === 'compact' ? 'compact' : 'expanded'))
          .onSelect((index: number): void => { this.model.fullscreenLayout = index === 1 ? 'compact' : 'expanded'; }).accessibilityText(this.text('fullscreen_controls'))
      }.width('100%').constraintSize({ minHeight: 56 })
      Row() {
        this.label('blue_accent')
        Toggle({ type: ToggleType.Switch, isOn: this.model.accented }).accessibilityText(this.text('blue_accent'))
          .onChange((enabled: boolean): void => { this.model.accented = enabled; })
      }.width('100%').constraintSize({ minHeight: 56 })
      Row() {
        this.label('reduced_motion')
        Toggle({ type: ToggleType.Switch, isOn: this.reducedMotion }).accessibilityText(this.text('reduced_motion'))
          .onChange((enabled: boolean): void => { this.onReducedMotionChange(enabled); })
      }.width('100%').constraintSize({ minHeight: 56 })
      Text(this.text('appearance_note')).fontSize(13).lineHeight(19).fontColor(this.palette.secondary).width('100%')
    }.padding({ left: 24, right: 24, bottom: 24 }).width('100%')
  }
}
