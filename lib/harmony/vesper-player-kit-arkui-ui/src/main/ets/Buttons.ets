import { VesperPlayerStageSkin, VesperStageButtonStyle, VesperStageButtonVariant,
  VesperStageIconRole, VesperStageIconConfiguration } from './Skin';

/** Decorative icon content is bounded and never owns input or accessibility. */
@Component
export struct VesperStageIcon {
  @Prop role: VesperStageIconRole = VesperStageIconRole.Play;
  @Prop skin: VesperPlayerStageSkin | undefined = undefined;
  @Prop iconSize: number = 22;
  @Prop color: ResourceColor | undefined = undefined;
  private defaults: VesperPlayerStageSkin = new VesperPlayerStageSkin();
  private current(): VesperPlayerStageSkin { return this.skin ?? this.defaults; }
  private configuration(): VesperStageIconConfiguration {
    return { role: this.role, size: this.iconSize, color: this.color ?? this.current().colors.foreground };
  }
  @Builder
  custom(content: WrappedBuilder<[VesperStageIconConfiguration]>) {
    content.builder(this.configuration())
  }
  build() {
    Stack() {
      if (this.current().iconContent !== undefined && this.current().customIconRoles.includes(this.role)) {
        this.custom(this.current().iconContent!)
      } else {
        SymbolGlyph(this.current().icons.resolve(this.role)).fontSize(this.iconSize + 'vp')
          .fontColor([this.color ?? this.current().colors.foreground])
      }
    }.width(this.iconSize).height(this.iconSize).clip(true).hitTestBehavior(HitTestMode.None).accessibilityLevel('no')
  }
}

/** The complete target belongs to the button, including transparent padding. */
@Component
export struct VesperStageIconButton {
  @Prop actionId: string = '';
  @Prop role: VesperStageIconRole = VesperStageIconRole.Play;
  @Prop label: ResourceStr = '';
  @Prop isEnabled: boolean = true;
  @Prop selected: boolean = false;
  @Prop skin: VesperPlayerStageSkin | undefined = undefined;
  @Prop variant: VesperStageButtonVariant = VesperStageButtonVariant.Standard;
  @Prop style: VesperStageButtonStyle | undefined = undefined;
  @Prop reducedMotion: boolean = false;
  onAction: () => void = (): void => {};
  onFocusChange?: (focused: boolean) => void;
  @State pressed: boolean = false;
  private defaults: VesperPlayerStageSkin = new VesperPlayerStageSkin();
  private current(): VesperPlayerStageSkin { return this.skin ?? this.defaults; }
  private geometry(): VesperStageButtonStyle { return this.style ?? this.current().metrics.button(this.variant); }
  private accessibleLabel(): string {
    return typeof this.label === 'string' ? this.label :
      this.getUIContext().getHostContext()?.resourceManager.getStringSync(this.label.id) ?? '';
  }
  build() {
    Button({ type: ButtonType.Normal }) {
      Stack() {
        Rect().width(this.geometry().size).height(this.geometry().size)
          .fill(this.current().colors.buttonBackground).opacity(this.geometry().backgroundOpacity)
          .radius(this.geometry().borderRadius)
        VesperStageIcon({ role: this.role, skin: this.current(), iconSize: this.geometry().iconSize })
          .translate({ x: this.role === VesperStageIconRole.Play ? this.geometry().iconSize * 0.03 : 0 })
      }.width(this.geometry().size).height(this.geometry().size)
        .scale({ x: this.pressed && !this.reducedMotion ? 0.96 : 1, y: this.pressed && !this.reducedMotion ? 0.96 : 1 })
        .animation({ duration: this.reducedMotion ? 0 : 160, curve: Curve.EaseOut })
        .hitTestBehavior(HitTestMode.None).accessibilityLevel('no')
    }.width(this.geometry().hitTarget).height(this.geometry().hitTarget).padding(0)
      .backgroundColor(Color.Transparent).borderRadius(this.geometry().borderRadius).stateEffect(false)
      .enabled(this.isEnabled).opacity(this.isEnabled ? 1 : 0.45).focusable(true)
      .accessibilityText(this.accessibleLabel()).accessibilitySelected(this.selected).id(this.actionId)
      .onFocus((): void => { this.onFocusChange?.(true); }).onBlur((): void => { this.pressed = false; this.onFocusChange?.(false); })
      .onTouch((event: TouchEvent): void => { this.pressed = event.type === TouchType.Down && this.isEnabled; })
      .onClick((): void => { if (this.isEnabled) { this.onAction(); } })
  }
}

/** Text shortcuts reserve the same target and focus behavior as icon actions. */
@Component
export struct VesperStagePillButton {
  @Prop actionId: string = '';
  @Prop label: ResourceStr = '';
  @Prop value: ResourceStr = '';
  @Prop isEnabled: boolean = true;
  @Prop skin: VesperPlayerStageSkin | undefined = undefined;
  @Prop reducedMotion: boolean = false;
  @State pressed: boolean = false;
  onAction: () => void = (): void => {};
  onFocusChange?: (focused: boolean) => void;
  private defaults: VesperPlayerStageSkin = new VesperPlayerStageSkin();
  private current(): VesperPlayerStageSkin { return this.skin ?? this.defaults; }
  private accessibleLabel(): string {
    return typeof this.label === 'string' ? this.label :
      this.getUIContext().getHostContext()?.resourceManager.getStringSync(this.label.id) ?? '';
  }
  @Builder pillBackground() {
    Rect().width('100%').height('100%').fill(this.current().colors.buttonBackground)
      .opacity(0.10).radius(this.current().metrics.standard.borderRadius)
  }
  build() {
    Button({ type: ButtonType.Normal }) {
      Text(this.value).fontSize(13).fontWeight(FontWeight.Medium).fontColor(this.current().colors.foreground)
        .fontFeature('"tnum" 1').maxLines(1).textOverflow({ overflow: TextOverflow.Ellipsis })
        .height(34).padding({ left: 10, right: 10 }).constraintSize({ minWidth: 48 })
        .background(this.pillBackground)
        .scale({ x: this.pressed && !this.reducedMotion ? 0.96 : 1, y: this.pressed && !this.reducedMotion ? 0.96 : 1 })
        .animation({ duration: this.reducedMotion ? 0 : 160, curve: Curve.EaseOut })
        .hitTestBehavior(HitTestMode.None).accessibilityLevel('no')
    }.height(this.current().metrics.toolbar.hitTarget).padding(0).backgroundColor(Color.Transparent)
      .constraintSize({ minWidth: this.current().metrics.toolbar.hitTarget }).enabled(this.isEnabled).opacity(this.isEnabled ? 1 : 0.45).focusable(true)
      .accessibilityText(this.accessibleLabel()).id(this.actionId).onFocus((): void => { this.onFocusChange?.(true); })
      .onBlur((): void => { this.pressed = false; this.onFocusChange?.(false); })
      .onTouch((event: TouchEvent): void => { this.pressed = event.type === TouchType.Down && this.isEnabled; })
      .onClick((): void => { if (this.isEnabled) { this.onAction(); } })
  }
}

@Component
export struct VesperStagePrimaryPlayButton {
  @Prop isPlaying: boolean = false;
  @Prop isCompleted: boolean = false;
  @Prop isEnabled: boolean = true;
  @Prop label: ResourceStr | undefined = undefined;
  @Prop skin: VesperPlayerStageSkin | undefined = undefined;
  @Prop style: VesperStageButtonStyle | undefined = undefined;
  @Prop reducedMotion: boolean = false;
  onAction: () => void = (): void => {};
  build() {
    VesperStageIconButton({
      role: this.isPlaying ? VesperStageIconRole.Pause : this.isCompleted ? VesperStageIconRole.Replay : VesperStageIconRole.Play,
      label: this.label ?? (this.isPlaying ? $r('app.string.vesper_pause') : this.isCompleted ? $r('app.string.vesper_replay') : $r('app.string.vesper_play')),
      isEnabled: this.isEnabled, skin: this.skin, style: this.style, reducedMotion: this.reducedMotion,
      variant: VesperStageButtonVariant.Primary, onAction: this.onAction
    })
  }
}
