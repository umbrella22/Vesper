import { examplePalette, ExamplePalette } from '../model/ExampleSupport';

@Component
export struct ExampleAction {
  @Prop title: string = '';
  @Prop palette: ExamplePalette = examplePalette(false);
  @Prop primary: boolean = false;
  @Prop isEnabled: boolean = true;
  @Prop danger: boolean = false;
  @Prop icon?: Resource = undefined;
  onAction: () => void = (): void => {};
  build() {
    Button({ type: ButtonType.Capsule }) {
      Row({ space: 8 }) {
        if (this.icon !== undefined) { SymbolGlyph(this.icon).fontSize(19).fontColor([this.primary ? this.palette.onAccent : this.palette.accent]) }
        Text(this.title).fontSize(15).fontWeight(FontWeight.Medium).maxLines(2).textOverflow({ overflow: TextOverflow.Ellipsis })
          .fontColor(this.primary ? this.palette.onAccent : this.danger ? this.palette.danger : this.palette.title)
      }.justifyContent(FlexAlign.Center)
    }.constraintSize({ minHeight: 48 }).padding({ left: 16, right: 16, top: 10, bottom: 10 })
      .backgroundColor(this.primary ? this.palette.accent : this.palette.field)
      .fontColor(this.primary ? this.palette.onAccent : this.danger ? this.palette.danger : this.palette.title)
      .enabled(this.isEnabled).opacity(this.isEnabled ? 1 : 0.45).onClick(this.onAction).accessibilityText(this.title)
  }
}

@Component
export struct ExampleIconAction {
  @Prop title: string = '';
  @Prop icon: Resource = $r('sys.symbol.plus');
  @Prop palette: ExamplePalette = examplePalette(false);
  @Prop isEnabled: boolean = true;
  @Prop danger: boolean = false;
  onAction: () => void = (): void => {};
  build() {
    Button({ type: ButtonType.Circle }) {
      SymbolGlyph(this.icon).fontSize(22).fontColor([this.danger ? this.palette.danger : this.palette.accent])
    }.width(48).height(48).backgroundColor(Color.Transparent).enabled(this.isEnabled).opacity(this.isEnabled ? 1 : 0.38)
      .onClick(this.onAction).accessibilityText(this.title).id(this.title)
  }
}

@Component
export struct ExampleHeading {
  @Prop title: string = '';
  @Prop detail: string = '';
  @Prop palette: ExamplePalette = examplePalette(false);
  build() {
    Column({ space: 5 }) {
      Text(this.title).fontSize(20).fontWeight(FontWeight.Medium).fontColor(this.palette.title).width('100%')
      if (this.detail.length > 0) { Text(this.detail).fontSize(13).lineHeight(19).fontColor(this.palette.secondary).width('100%') }
    }.alignItems(HorizontalAlign.Start).width('100%')
  }
}

@Component
export struct ExampleValue {
  @Prop label: string = '';
  @Prop value: string = '';
  @Prop palette: ExamplePalette = examplePalette(false);
  build() {
    Row({ space: 16 }) {
      Text(this.label).fontSize(13).fontColor(this.palette.secondary).layoutWeight(1)
      Text(this.value).fontSize(13).fontFamily('monospace').fontColor(this.palette.title)
        .textAlign(TextAlign.End).maxLines(2).constraintSize({ maxWidth: '62%' })
    }.width('100%').constraintSize({ minHeight: 36 })
  }
}
