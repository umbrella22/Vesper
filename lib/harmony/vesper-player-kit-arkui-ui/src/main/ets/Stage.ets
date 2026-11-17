import { common } from '@kit.AbilityKit';
import { accessibility } from '@kit.AccessibilityKit';
import { VesperPlayerBinding, VesperPlayerSurface, VesperPictureInPicture, VesperOperationFeedback, playbackFeedback, VesperTimeRange } from '@vesper/player-kit-arkui';
import { VesperStageEnvironment, VesperStageInsets, VesperGestureStart, VesperGestureIntent, gestureIntent } from './Gestures';
import { VesperPlayerStageSkin, VesperStageColors, VesperStageMetrics, VesperStageIconRole, VesperStageButtonVariant } from './Skin';
import { VesperStageIcon, VesperStageIconButton, VesperStagePillButton } from './Buttons';
import { VesperStagePresentation, VesperSubtitleStyle, StageLayout, stageLayout, stageQuality, stageLoadedRanges, stageCaptionGeometry, StageCaptionGeometry } from './Presentation';
import { stageText, feedbackResource } from './Strings';
import { StageControlTimer, stageClock, stageSeekTarget } from './Controls';
import { StageValuePump, StageTemporaryRate, stageGestureAxis } from './Interaction';
import { StageSettings } from './StageSettings';

interface StageAction { role: VesperStageIconRole; label: Resource; action: () => void; isEnabled?: boolean; variant?: VesperStageButtonVariant; }

/** Native controls retain one binding and Surface across presentation changes. */
@Component
export struct VesperPlayerStage {
  @ObjectLink binding: VesperPlayerBinding;
  @Prop @Watch('layoutChanged') controlLayout: string = 'standard';
  @Prop @Watch('layoutChanged') isFullscreen: boolean = false;
  @Prop @Watch('presentationChanged') interactionEnabled: boolean = true;
  @Prop skin: VesperPlayerStageSkin | undefined = undefined;
  @Prop safeArea: VesperStageInsets = { top: 0, right: 0, bottom: 0, left: 0 };
  @Prop title: string = '';
  @Prop @Watch('controlledChanged') presentation: VesperStagePresentation | undefined = undefined;
  @Prop subtitleStyle: VesperSubtitleStyle | undefined = undefined;
  onSubtitleStyleChange?: (style: VesperSubtitleStyle) => void;
  @Prop reducedMotion: boolean = false;
  @Prop availableHeight: number = 800;
  @Prop expandedLeadingWidth: number = 0;
  @BuilderParam contentOverlay: () => void = this.emptyContent;
  @BuilderParam expandedLeadingActions: () => void = this.emptyContent;
  pictureInPicture?: VesperPictureInPicture;
  onPictureInPicture?: () => void;
  environment?: VesperStageEnvironment;
  onFullscreenChange: (fullscreen: boolean) => void = (): void => {};
  onNavigateBack?: () => void;
  onSettingsRequest?: (pane: string) => void;
  onPresentationChange?: (value: VesperStagePresentation) => void;
  onRetry: () => void = (): void => {};
  onChangeSource?: () => void;
  @State controlsVisible: boolean = true;
  @State settings: boolean = false;
  @State settingsPane: string = 'menu';
  @State locked: boolean = false;
  @State dragging: boolean = false;
  @State seekPosition: number = 0;
  @State hudKind: string = '';
  @State hudValue: number = 0;
  @State holding: boolean = false;
  @State stageWidth: number = 0;
  @State stageHeight: number = 0;
  @State focused: boolean = false;
  @State assistive: boolean = false;
  @State bottomHeight: number = 48;
  @State headerHeight: number = 48;
  @State localSubtitleStyle: VesperSubtitleStyle | undefined = undefined;
  private settingsTrigger: string = '';
  private defaultSkin: VesperPlayerStageSkin = new VesperPlayerStageSkin();
  private defaultCaptions: VesperSubtitleStyle = new VesperSubtitleStyle();
  private stageLeft: number = 0;
  private gestureStart?: VesperGestureStart;
  private gestureAxis: string = '';
  private gestureValue?: VesperGestureIntent;
  private unsubscribe: () => void = (): void => {};
  private lastState: string = '';
  private lastEpoch?: string;
  private hudTimer: number = -1;
  private presentationRevision: number = 0;
  private disappeared: boolean = false;
  private rangePending: boolean = false;
  private rangeAt: number = 0;
  private accessibilityChanged: (enabled: boolean) => void = (enabled: boolean): void => {
    this.assistive = enabled; if (enabled) { this.showControls(); } else { this.scheduleHide(); }
  };
  private volume: StageValuePump = new StageValuePump((value: number): Promise<void> => this.binding.controller.setVolume(value), (error: Error): void => this.reportError(error));
  private brightnessPump: StageValuePump = new StageValuePump((value: number): Promise<void> => this.environment?.brightness?.set(value) ?? Promise.resolve(), (error: Error): void => this.reportError(error));
  private temporary: StageTemporaryRate = new StageTemporaryRate((rate: number): Promise<void> => this.binding.controller.setPlaybackRate(rate), (): string => this.binding.controller.snapshot().playbackEpoch ?? '', (error: Error): void => this.reportError(error));
  private hideTimer: StageControlTimer = new StageControlTimer((): void => {
    if (this.mayHide()) { this.updatePresentation(false, this.isLocked(), this.currentPane()); }
  });
  @Builder emptyContent() {}
  private currentSkin(): VesperPlayerStageSkin { return this.skin ?? this.defaultSkin; }
  private colors(): VesperStageColors { return this.currentSkin().colors; }
  private metrics(): VesperStageMetrics { return this.currentSkin().metrics; }
  private captions(): VesperSubtitleStyle { return this.subtitleStyle ?? this.localSubtitleStyle ?? this.defaultCaptions; }
  private visible(): boolean { return this.presentation?.controlsVisible ?? this.controlsVisible; }
  private isLocked(): boolean { return this.presentation?.locked ?? this.locked; }
  private currentPane(): string | undefined { return this.presentation !== undefined ? this.presentation.settingsPane : this.settings ? this.settingsPane : undefined; }
  private settingsOpen(): boolean { return this.currentPane() !== undefined; }
  private updatePresentation(visible: boolean, locked: boolean, pane?: string): void {
    const next: VesperStagePresentation = { controlsVisible: visible, locked: locked, settingsPane: pane };
    if (this.presentation === undefined) {
      this.controlsVisible = visible; this.locked = locked; this.settings = pane !== undefined; this.settingsPane = pane ?? 'menu';
    }
    this.onPresentationChange?.(next);
  }
  private controlledChanged(): void {
    this.cancelInteraction(); this.scheduleHide();
  }
  private text(resource: Resource): string { return stageText(this.getUIContext(), resource); }
  private textWidth(text: string, size: number): number {
    return this.getUIContext().px2vp(Number(this.getUIContext().getMeasureUtils().measureTextSize({ textContent: text, fontSize: size + 'fp' }).width)) + 4;
  }
  private textHeight(size: number): number {
    return this.getUIContext().px2vp(Number(this.getUIContext().getMeasureUtils().measureTextSize({ textContent: 'Ag国', fontSize: size + 'fp' }).height));
  }
  private timeText(compact: boolean = this.compact()): string {
    return this.binding.snapshot.timeline === 'live' ? this.text($r('app.string.vesper_live')) :
      stageClock(this.dragging ? this.seekPosition : this.binding.snapshot.positionMs) + (compact ? '/' : ' / ') + stageClock(this.binding.snapshot.durationMs);
  }
  private qualityText(): string {
    const quality = stageQuality(this.binding.snapshot);
    if (quality.status === 'switching') { return this.text($r('app.string.vesper_switching')); }
    const height = quality.height === undefined ? '' : quality.height + 'p';
    if (quality.mode === 'bitrate' && quality.bitrate !== undefined && quality.bitrate > 0) {
      return this.text($r('app.string.vesper_quality_locked')) + ' · ' + Math.round(quality.bitrate / 1000) + ' kbps';
    }
    const label = quality.mode === 'auto' ? this.text($r('app.string.vesper_auto')) :
      quality.mode === 'track' ? this.text($r('app.string.vesper_quality_locked')) : this.text($r('app.string.vesper_quality'));
    return label + (height ? ' · ' + height : '');
  }
  private rateText(): string { return this.binding.snapshot.playbackRate.toFixed(1) + 'x'; }
  private layout(): StageLayout {
    const metrics = this.metrics();
    return stageLayout(Math.max(0, this.stageWidth - this.safeArea.left - this.safeArea.right - this.metrics().horizontalInset * 2),
      Math.max(0, this.stageHeight - this.safeArea.top - this.safeArea.bottom - metrics.topInset - 8),
      this.controlLayout, metrics.expanded.hitTarget, metrics.buttonSpacing, this.textWidth(this.timeText(true), 12),
      Math.max(metrics.toolbar.hitTarget, this.textWidth(this.rateText(), 13) + 20),
      Math.max(metrics.toolbar.hitTarget, this.textWidth(this.qualityText(), 13) + 20), this.expandedLeadingWidth,
      { fullscreenTarget: metrics.expandedFullscreen.hitTarget, compactTarget: metrics.compact.hitTarget,
        compactFullscreenTarget: metrics.compactFullscreen.hitTarget,
        buttonHeight: Math.max(metrics.expanded.hitTarget, metrics.expandedFullscreen.hitTarget, metrics.toolbar.hitTarget),
        compactButtonHeight: Math.max(metrics.compact.hitTarget, metrics.compactFullscreen.hitTarget, 44),
        timeHeight: this.textHeight(12),
        headerHeight: Math.max(metrics.navigation.hitTarget, metrics.toolbar.hitTarget, this.textHeight(16) + 3 + this.textHeight(12)),
        primaryHeight: Math.max(metrics.primary.hitTarget, metrics.standard.hitTarget),
        primaryWidth: metrics.primary.hitTarget + metrics.standard.hitTarget * 2 + metrics.buttonSpacing * 2,
        seekable: this.canSeek() });
  }
  private compact(): boolean { return this.layout().compact; }
  private reportError(error: Error): void { this.binding.report(Promise.reject(error)); }
  private mayHide(): boolean {
    // Surface attachment can move the binding observer after this subscriber.
    // Use the state received here so timer behavior is independent of that order.
    return this.interactionEnabled && this.lastState === 'playing' && this.visible() && !this.settingsOpen() &&
      !this.isLocked() && !this.dragging && !this.holding && !this.focused && !this.assistive && this.gestureStart === undefined;
  }
  aboutToAppear(): void {
    this.disappeared = false;
    this.binding.setPresentationVisible(this.interactionEnabled);
    try { this.assistive = accessibility.isScreenReaderOpenSync(); accessibility.on('screenReaderStateChange', this.accessibilityChanged); }
    catch (error) { this.reportError(error as Error); }
    this.unsubscribe = this.binding.controller.subscribe((snapshot): void => {
      if (snapshot.playbackEpoch !== this.lastEpoch) { this.lastEpoch = snapshot.playbackEpoch; this.cancelInteraction(); this.rangeAt = 0; }
      this.refreshRanges();
      if (snapshot.state === this.lastState) { return; }
      this.lastState = snapshot.state;
      if (snapshot.state !== 'playing') { this.updatePresentation(true, this.isLocked(), this.currentPane()); this.cancelInteraction(); }
      this.scheduleHide();
    });
  }
  aboutToDisappear(): void {
    this.disappeared = true; this.unsubscribe(); this.cancelInteraction();
    this.binding.setPresentationVisible(false);
    try { accessibility.off('screenReaderStateChange', this.accessibilityChanged); } catch (error) { this.reportError(error as Error); }
  }
  private refreshRanges(): void {
    if (this.rangePending || !this.interactionEnabled || this.disappeared || Date.now() - this.rangeAt < 2000 ||
      !this.binding.controller.capabilities().timeRanges || !['prepared', 'playing', 'paused'].includes(this.binding.snapshot.state)) { return; }
    this.rangeAt = Date.now(); this.rangePending = true;
    this.binding.controller.timeRanges().catch((error: Error): void => { this.binding.rangeObservationError = playbackFeedback(error).code; })
      .finally((): void => { this.rangePending = false; });
  }
  private presentationChanged(): void {
    this.binding.setPresentationVisible(this.interactionEnabled);
    this.cancelInteraction(); if (!this.interactionEnabled) { this.updatePresentation(this.visible(), this.isLocked()); }
    this.scheduleHide();
  }
  private layoutChanged(): void { this.cancelInteraction(); this.showControls(); }
  private cancelInteraction(): void {
    this.presentationRevision++; this.temporary.end(); this.volume.cancel(); this.brightnessPump.cancel();
    this.gestureStart = undefined; this.gestureValue = undefined; this.gestureAxis = ''; this.holding = false; this.dragging = false; this.hudKind = '';
    if (this.hudTimer !== -1) { clearTimeout(this.hudTimer); this.hudTimer = -1; } this.hideTimer.clear();
  }
  private scheduleHide(): void {
    this.hideTimer.arm(this.mayHide());
  }
  private showControls(): void { this.updatePresentation(true, this.isLocked(), this.currentPane()); this.scheduleHide(); }
  private focusChanged(focused: boolean): void { this.focused = focused; if (focused) { this.hideTimer.clear(); } else { this.scheduleHide(); } }
  private hasError(): boolean { return this.binding.snapshot.error !== undefined || this.binding.snapshot.state === 'error'; }
  private failure(): VesperOperationFeedback { return playbackFeedback(this.binding.snapshot.error ?? new Error()); }
  private canSeek(): boolean { return stageSeekTarget(this.binding.snapshot.timeline, this.binding.snapshot.durationMs, this.binding.snapshot.positionMs, 0) !== undefined && !this.hasError(); }
  private canPlay(): boolean { return ['prepared', 'playing', 'paused', 'completed'].includes(this.binding.snapshot.state) && !this.hasError(); }
  private skip(delta: number): void {
    const target = stageSeekTarget(this.binding.snapshot.timeline, this.binding.snapshot.durationMs, this.binding.snapshot.positionMs, delta);
    if (target !== undefined) { this.binding.report(this.binding.controller.seek(target)); this.showHud('seek', target); this.dismissHud(); }
  }
  private showHud(kind: string, value: number): void {
    if (this.hudTimer !== -1) { clearTimeout(this.hudTimer); this.hudTimer = -1; }
    this.hudKind = kind; this.hudValue = value;
  }
  private dismissHud(): void { this.hudTimer = setTimeout((): void => { this.hudTimer = -1; this.hudKind = ''; }, 520); }
  private beginGesture(event: GestureEvent): void {
    if (this.isLocked() || this.settingsOpen() || this.dragging || this.holding || !this.interactionEnabled || event.fingerList.length < 1) { return; }
    this.hideTimer.clear(); this.gestureAxis = ''; this.gestureValue = undefined;
    // Background regions have local origins; side selection uses Stage coordinates.
    this.gestureStart = { x: event.fingerList[0].globalX - this.stageLeft, width: this.stageWidth, height: this.stageHeight,
      positionMs: this.binding.snapshot.positionMs, durationMs: this.binding.snapshot.durationMs,
      volume: this.binding.snapshot.volume, brightness: this.environment?.brightness?.current(), timeline: this.binding.snapshot.timeline };
  }
  private moveGesture(event: GestureEvent): void {
    if (this.gestureStart === undefined) { return; }
    if (this.gestureAxis.length === 0) { this.gestureAxis = stageGestureAxis(event.offsetX, event.offsetY); }
    if (this.gestureAxis.length === 0) { return; }
    this.gestureValue = gestureIntent(this.gestureStart, event.offsetX, event.offsetY, this.gestureAxis);
    const value = this.gestureValue;
    if (value === undefined) { return; }
    this.showHud(value.kind, value.value);
    if (value.kind === 'volume') { this.volume.submit(value.value); }
    else if (value.kind === 'brightness') { this.brightnessPump.submit(value.value); }
  }
  private endGesture(): void {
    const intent = this.gestureValue; this.gestureValue = undefined; this.gestureStart = undefined;
    if (!this.isLocked() && this.interactionEnabled && intent?.kind === 'seek') { this.binding.report(this.binding.controller.seek(intent.value)); }
    this.dismissHud(); this.showControls();
  }
  private beginHold(): void {
    if (this.isLocked() || this.settingsOpen() || !this.interactionEnabled || this.binding.snapshot.timeline !== 'vod' || this.binding.snapshot.state !== 'playing') { return; }
    this.holding = this.temporary.begin(this.binding.snapshot.playbackRate);
    if (this.holding) { this.hideTimer.clear(); this.showHud('speed', 2); }
  }
  private endHold(): void { this.temporary.end(); this.holding = false; this.dismissHud(); this.scheduleHide(); }
  private togglePlayback(): void {
    if (this.canPlay() && this.interactionEnabled) { this.binding.report(this.binding.snapshot.state === 'playing' ? this.binding.controller.pause() : this.binding.controller.play()); this.showControls(); }
  }

  private startPictureInPicture(): void {
    this.cancelInteraction(); this.updatePresentation(true, this.isLocked());
    if (this.onPictureInPicture !== undefined) { this.onPictureInPicture(); }
    else if (this.pictureInPicture !== undefined) { this.binding.report(this.pictureInPicture.start()); }
  }
  private openSettings(pane: string = 'menu'): void {
    this.settingsTrigger = this.binding.surfaceIdentity + (pane === 'speed' ? '-speed' : pane === 'video' ? '-quality' : '-more');
    this.cancelInteraction(); const revision = this.presentationRevision;
    this.temporary.end().then((): void => {
      if (!this.disappeared && this.interactionEnabled && revision === this.presentationRevision) {
        this.onSettingsRequest?.(pane); this.updatePresentation(true, false, pane);
      }
    });
  }
  private closeSettings(): void { this.updatePresentation(true, this.isLocked()); this.scheduleHide(); }
  private toggleControls(): void {
    if (!this.isLocked() && !this.settingsOpen() && this.interactionEnabled) {
      this.updatePresentation(!this.visible(), false); this.scheduleHide();
    }
  }
  private sheetHeight(): number {
    const pane = this.currentPane() ?? 'menu';
    const rows = pane === 'speed' ? 10 : pane === 'captionStyle' ? 11 : pane === 'menu' ? (this.environment?.brightness === undefined ? 8 : 9) :
      this.binding.snapshot.tracks.filter((track): boolean => track.kind === pane).length +
      (pane === 'video' ? this.binding.snapshot.availableBitrates.length + 1 : pane === 'subtitle' ? 2 : 0);
    const scale = (this.getUIContext().getHostContext() as common.UIAbilityContext)?.config.fontSizeScale ?? 1;
    return Math.max(160, Math.min(this.availableHeight * 0.85, 104 + Math.max(1, rows) * Math.max(56, 40 * scale) + 56));
  }
  @Builder
  touchArea(width: Length, height: Length) {
    Column() {}.width(width).height(height).layoutWeight(1).accessibilityLevel('no')
      .gesture(GestureGroup(GestureMode.Exclusive,
        PanGesture({ direction: PanDirection.All, distance: 8, fingers: 1 })
          .onActionStart((event: GestureEvent): void => { this.beginGesture(event); })
          .onActionUpdate((event: GestureEvent): void => { this.moveGesture(event); })
          .onActionEnd((): void => { this.endGesture(); }).onActionCancel((): void => { this.cancelInteraction(); this.scheduleHide(); }),
        LongPressGesture({ repeat: false, duration: 450 }).onAction((): void => { this.beginHold(); })
          .onActionEnd((): void => { this.endHold(); }).onActionCancel((): void => { this.endHold(); }),
        TapGesture({ count: 2 }).onAction((): void => { this.togglePlayback(); }),
        TapGesture().onAction((): void => { this.toggleControls(); })))
  }
  @Builder
  action($$: StageAction) {
    VesperStageIconButton({ role: $$.role, label: $$.label, skin: this.currentSkin(),
      actionId: this.binding.surfaceIdentity + '-' + $$.role,
      isEnabled: $$.isEnabled ?? true, variant: $$.variant ?? VesperStageButtonVariant.Toolbar, reducedMotion: this.reducedMotion,
      onFocusChange: (focused: boolean): void => { this.focusChanged(focused); },
      onAction: (): void => { this.showControls(); $$.action(); } })
  }
  @Builder
  playButton(variant: VesperStageButtonVariant) {
    this.action({ role: this.binding.snapshot.state === 'playing' ? VesperStageIconRole.Pause : this.binding.snapshot.state === 'completed' ? VesperStageIconRole.Replay : VesperStageIconRole.Play,
      label: this.binding.snapshot.state === 'playing' ? $r('app.string.vesper_pause') : this.binding.snapshot.state === 'completed' ? $r('app.string.vesper_replay') : $r('app.string.vesper_play'),
      action: (): void => { this.togglePlayback(); }, isEnabled: this.canPlay(), variant: variant })
  }
  @Builder
  fullscreenButton() {
    this.action({ role: this.isFullscreen ? VesperStageIconRole.ExitFullscreen : VesperStageIconRole.Fullscreen,
      label: this.isFullscreen ? $r('app.string.vesper_exit_fullscreen') : $r('app.string.vesper_fullscreen'),
      variant: this.compact() ? VesperStageButtonVariant.CompactFullscreen : VesperStageButtonVariant.ExpandedFullscreen,
      action: (): void => { this.onFullscreenChange(!this.isFullscreen); } })
  }
  @Builder
  timeline() {
    Stack({ alignContent: Alignment.Start }) {
      Rect().height(this.metrics().timelineTrackHeight).width('100%').fill(this.colors().timelineInactive)
      ForEach(stageLoadedRanges(this.binding.snapshot), (range: VesperTimeRange) => {
        Rect().height(this.metrics().timelineTrackHeight).fill(this.colors().secondaryForeground).opacity(0.5)
          .width((range.endMs - range.startMs) / (this.binding.snapshot.durationMs ?? 1) * 100 + '%')
          .position({ x: range.startMs / (this.binding.snapshot.durationMs ?? 1) * 100 + '%', y: (this.compact() ? 22 : 14) - this.metrics().timelineTrackHeight / 2 })
      }, (range: VesperTimeRange): string => range.startMs + ':' + range.endMs)
      Row() {}.height(this.metrics().timelineTrackHeight)
        .width(Math.min(100, Math.max(0, (this.dragging ? this.seekPosition : this.binding.snapshot.positionMs) / (this.binding.snapshot.durationMs ?? 1) * 100)) + '%')
        .linearGradient({ angle: 90, colors: [[this.colors().timelineStart, 0], [this.colors().timelineEnd, 1]] })
      Slider({ value: this.dragging ? this.seekPosition : this.binding.snapshot.positionMs,
        min: 0, max: Math.max(1, this.binding.snapshot.durationMs ?? 1), step: 100, style: SliderStyle.OutSet })
        .height(this.compact() ? 44 : 28).enabled(this.canSeek()).trackThickness(this.metrics().timelineTrackHeight)
        .blockSize({ width: this.dragging ? this.metrics().timelineLargeThumbSize : this.metrics().timelineThumbSize,
          height: this.dragging ? this.metrics().timelineLargeThumbSize : this.metrics().timelineThumbSize })
        .trackColor(Color.Transparent).selectedColor(Color.Transparent).blockColor(this.colors().timelineThumb)
        .accessibilityText(this.text($r('app.string.vesper_position'))).accessibilityDescription(this.timeText())
        .id(this.binding.surfaceIdentity + '-timeline')
        .onFocus((): void => { this.focusChanged(true); }).onBlur((): void => { this.focusChanged(false); })
        .onChange((value: number, mode: SliderChangeMode): void => {
          this.dragging = mode !== SliderChangeMode.End && mode !== SliderChangeMode.Click; this.seekPosition = value; this.hideTimer.clear();
          if (!this.dragging && this.canSeek()) { this.binding.report(this.binding.controller.seek(Math.round(value))); this.scheduleHide(); }
        })
    }.height(this.compact() ? 44 : 28).accessibilityGroup(false)
  }
  @Builder
  timeLabel() {
    Text(this.timeText()).fontSize(12).fontColor(this.colors().foreground).fontFeature('"tnum" 1')
      .maxLines(1).textOverflow({ overflow: TextOverflow.Ellipsis }).constraintSize({ maxWidth: '100%' }).flexShrink(0)
      .id(this.binding.surfaceIdentity + '-time')
  }
  @Builder
  transport() {
    Column() {
      if (!this.compact() || this.layout().splitTime) {
        this.timeLabel()
        if (!this.compact() && this.canSeek()) { this.timeline() }
      }
      if (this.compact()) {
        Row({ space: this.metrics().buttonSpacing }) {
          this.playButton(VesperStageButtonVariant.Compact)
          if (this.canSeek()) { Row() { this.timeline() }.layoutWeight(1) }
          if (!this.layout().splitTime) { this.timeLabel() }
          if (!this.canSeek()) { Blank() }
          this.fullscreenButton()
        }.width('100%')
      } else {
        Row() {
          Row({ space: this.metrics().buttonSpacing }) {
            this.playButton(VesperStageButtonVariant.Expanded)
            if (this.layout().leading > 0) { Row() { this.expandedLeadingActions() }.width(this.layout().leading) }
          }.flexShrink(0)
          Blank().constraintSize({ minWidth: this.metrics().buttonSpacing })
          Row({ space: this.metrics().buttonSpacing }) {
            if (this.layout().speed) {
              VesperStagePillButton({ actionId: this.binding.surfaceIdentity + '-speed', label: $r('app.string.vesper_speed'), value: this.rateText(), skin: this.currentSkin(), reducedMotion: this.reducedMotion,
                onAction: (): void => { this.openSettings('speed'); }, onFocusChange: (value: boolean): void => { this.focusChanged(value); } })
            }
            if (this.layout().quality) {
              VesperStagePillButton({ actionId: this.binding.surfaceIdentity + '-quality', label: $r('app.string.vesper_quality'), value: this.qualityText(), skin: this.currentSkin(), reducedMotion: this.reducedMotion,
                onAction: (): void => { this.openSettings('video'); }, onFocusChange: (value: boolean): void => { this.focusChanged(value); } })
            }
            this.fullscreenButton()
          }.flexShrink(0)
        }.width('100%')
      }
    }.width('100%').alignItems(HorizontalAlign.Start)
      .id(this.binding.surfaceIdentity + '-transport')
      .onAreaChange((before: Area, after: Area): void => { this.bottomHeight = Number(after.height); })
  }
  @Builder
  failureAction() {
    if (this.failure().retryable) {
      Button($r('app.string.vesper_retry')).constraintSize({ minHeight: 44 }).fontColor(this.colors().foreground)
        .backgroundColor(this.colors().hudBackground).onClick((): void => { this.onRetry(); })
    } else if (this.onChangeSource !== undefined) {
      Button($r('app.string.vesper_change_source')).constraintSize({ minHeight: 44 }).fontColor(this.colors().foreground)
        .backgroundColor(this.colors().hudBackground).onClick((): void => { this.onChangeSource?.(); })
    }
  }
  @Builder
  failureCard() {
    Column({ space: 12 }) {
      Text(feedbackResource(this.failure())).fontSize(15).fontColor(this.colors().foreground).textAlign(TextAlign.Center).maxLines(4)
      this.failureAction()
    }.width('100%').padding(12).backgroundColor(this.colors().hudBackground).borderRadius(this.metrics().hudBorderRadius)
  }
  @Builder
  controls() {
    Column() {
      Row({ space: this.metrics().buttonSpacing }) {
        if (this.isFullscreen || this.onNavigateBack !== undefined) {
          this.action({ role: VesperStageIconRole.NavigateBack, label: this.isFullscreen ? $r('app.string.vesper_exit_fullscreen') : $r('app.string.vesper_back'),
            variant: VesperStageButtonVariant.Navigation, action: (): void => { if (this.isFullscreen) { this.onFullscreenChange(false); } else { this.onNavigateBack?.(); } } })
        }
        Column({ space: 3 }) {
          Text(this.title).fontSize(16).fontWeight(FontWeight.Medium).fontColor(this.colors().foreground)
            .maxLines(1).textOverflow({ overflow: TextOverflow.Ellipsis }).width('100%').accessibilityText(this.title)
          if (this.layout().badge) {
            Text(this.binding.snapshot.timeline === 'live' ? $r('app.string.vesper_live') : this.binding.snapshot.timeline === 'vod' ? $r('app.string.vesper_on_demand') : $r('app.string.vesper_timeline_unknown'))
              .fontSize(12).fontColor(this.colors().secondaryForeground).maxLines(1).textOverflow({ overflow: TextOverflow.Ellipsis }).width('100%')
          }
        }.layoutWeight(1).alignItems(HorizontalAlign.Start)
        if (this.binding.snapshot.buffering || ['initializing', 'preparing'].includes(this.binding.snapshot.state)) {
          LoadingProgress().width(22).height(22).color(this.colors().foreground).accessibilityText(this.text($r('app.string.vesper_buffering')))
        }
        this.action({ role: VesperStageIconRole.More, label: $r('app.string.vesper_more'), action: (): void => { this.openSettings(); } })
      }.width('100%').id(this.binding.surfaceIdentity + '-header')
        .onAreaChange((before: Area, after: Area): void => { this.headerHeight = Number(after.height); })
      this.touchArea('100%', 'auto')
      if (this.hasError() && this.stageHeight >= 220) { this.failureCard() }
      else if (this.layout().primary) {
        Row({ space: this.metrics().buttonSpacing }) {
          this.touchArea('auto', this.metrics().primary.hitTarget)
          this.action({ role: VesperStageIconRole.SkipBack, label: $r('app.string.vesper_back_ten'), variant: VesperStageButtonVariant.Standard, isEnabled: this.canSeek(), action: (): void => { this.skip(-10000); } })
          this.playButton(VesperStageButtonVariant.Primary)
          this.action({ role: VesperStageIconRole.SkipForward, label: $r('app.string.vesper_forward_ten'), variant: VesperStageButtonVariant.Standard, isEnabled: this.canSeek(), action: (): void => { this.skip(10000); } })
          this.touchArea('auto', this.metrics().primary.hitTarget)
        }.width('100%')
      }
      this.touchArea('100%', 'auto')
      if (!this.hasError() && this.binding.feedback !== undefined && this.binding.feedback.category !== 'cancelled') {
        Text(feedbackResource(this.binding.feedback)).fontSize(12).fontColor(this.colors().error).maxLines(2).width('100%')
      }
      if (this.hasError()) {
        if (this.stageHeight < 220) {
          Row({ space: 8 }) {
            Text(feedbackResource(this.failure())).fontSize(13).fontColor(this.colors().foreground).maxLines(2).layoutWeight(1)
            this.failureAction()
          }.width('100%').padding(8).backgroundColor(this.colors().hudBackground).borderRadius(this.metrics().hudBorderRadius)
        }
      } else { this.transport() }
    }.width('100%').height('100%').padding({ left: this.safeArea.left + this.metrics().horizontalInset,
      right: this.safeArea.right + this.metrics().horizontalInset, top: this.safeArea.top + this.metrics().topInset, bottom: this.safeArea.bottom + 8 })
  }
  @Builder
  settingsPanel() {
    StageSettings({ binding: this.binding, initialPane: this.currentPane() ?? 'menu', environment: this.environment,
      skin: this.currentSkin(), subtitleStyle: this.captions(),
      onSubtitleStyleChange: (style: VesperSubtitleStyle): void => {
        if (this.subtitleStyle === undefined) { this.localSubtitleStyle = style; }
        this.onSubtitleStyleChange?.(style);
      },
      pipAvailable: this.pictureInPicture !== undefined || this.onPictureInPicture !== undefined,
      onPaneChange: (pane: string): void => { this.updatePresentation(true, false, pane); },
      onLock: (): void => { this.cancelInteraction(); this.updatePresentation(false, true); },
      onClose: (): void => { this.closeSettings(); }, onPiP: (): void => { this.startPictureInPicture(); } })
  }
  private captionGeometry(): StageCaptionGeometry {
    return stageCaptionGeometry(this.stageWidth, this.stageHeight, this.binding.snapshot.width, this.binding.snapshot.height, this.safeArea,
      this.visible() && !this.isLocked() && this.interactionEnabled ? this.bottomHeight + 8 : 0, this.captions().bottomInset,
      this.visible() && !this.isLocked() && this.interactionEnabled ? this.headerHeight + this.metrics().topInset : 0);
  }
  private captionLines(): number {
    const lineHeight = this.getUIContext().px2vp(Number(this.getUIContext().getMeasureUtils().measureTextSize({
      textContent: 'Ag国', fontSize: this.metrics().captionSize * this.captions().fontScale + 'fp' }).height));
    return Math.max(0, Math.min(this.captions().maxLines, Math.floor((this.captionGeometry().maxHeight - 8) / Math.max(1, lineHeight))));
  }
  build() {
    Stack({ alignContent: Alignment.Bottom }) {
      VesperPlayerSurface({ binding: this.binding })
      Stack() { this.contentOverlay() }.width('100%').height('100%').hitTestBehavior(HitTestMode.None).accessibilityLevel('no')
      this.touchArea('100%', '100%')
      Text(this.binding.snapshot.subtitle).fontColor((this.subtitleStyle ?? this.localSubtitleStyle)?.foreground ?? this.colors().foreground)
        .fontSize(this.metrics().captionSize * this.captions().fontScale).textAlign(TextAlign.Center).maxLines(Math.max(1, this.captionLines()))
        .textOverflow({ overflow: TextOverflow.Ellipsis }).constraintSize({ maxHeight: this.captionGeometry().maxHeight })
        .textShadow({ radius: this.captions().edgeRadius, color: this.captions().edgeColor })
        .backgroundColor((this.subtitleStyle ?? this.localSubtitleStyle)?.background ?? this.colors().captionBackground).padding(4)
        .width(this.captionGeometry().width).margin({ bottom: this.captionGeometry().bottom })
        .visibility(this.binding.snapshot.subtitle.length > 0 && this.captionLines() > 0 && !this.settingsOpen() && !this.hasError() && this.interactionEnabled ? Visibility.Visible : Visibility.Hidden)
        .hitTestBehavior(HitTestMode.None).accessibilityLevel('no')
      Column() {
        Row() {}.height('38%').width('100%')
          .linearGradient({ angle: 180, colors: [[this.colors().scrim, 0], [Color.Transparent, 1]] }).opacity(0.72)
        Blank()
        Row() {}.height('38%').width('100%')
          .linearGradient({ angle: 180, colors: [[Color.Transparent, 0], [this.colors().scrim, 1]] }).opacity(0.82)
      }.width('100%').height('100%').opacity(this.visible() && !this.isLocked() ? 1 : 0)
        .animation({ duration: this.reducedMotion ? 0 : 180, curve: Curve.EaseOut }).hitTestBehavior(HitTestMode.None)
      Stack() { this.controls() }.width('100%').height('100%')
        .opacity(this.visible() && !this.isLocked() && this.interactionEnabled ? 1 : 0)
        .animation({ duration: this.reducedMotion ? 0 : 180, curve: Curve.EaseOut })
        .hitTestBehavior(this.visible() && !this.isLocked() && this.interactionEnabled ? HitTestMode.Default : HitTestMode.None)
        .accessibilityLevel(this.visible() && !this.isLocked() && this.interactionEnabled ? 'auto' : 'no-hide-descendants')
      if (this.isLocked() && this.interactionEnabled) {
        Row() {
          this.action({ role: VesperStageIconRole.Unlock, label: $r('app.string.vesper_unlock'),
            action: (): void => { this.updatePresentation(true, false); } })
        }.width('100%').height('100%').padding({ left: this.safeArea.left + this.metrics().horizontalInset }).alignItems(VerticalAlign.Center)
      }
      if (this.hudKind.length > 0 && this.interactionEnabled) {
        Column({ space: 8 }) {
          VesperStageIcon({ role: this.hudKind === 'volume' ? VesperStageIconRole.Volume : this.hudKind === 'brightness' ? VesperStageIconRole.Brightness : this.hudKind === 'speed' ? VesperStageIconRole.Speed : VesperStageIconRole.SkipForward,
            skin: this.currentSkin(), iconSize: this.metrics().hudIconSize, color: this.colors().hudForeground })
          Text(this.hudKind === 'seek' ? stageClock(this.hudValue) + ' / ' + stageClock(this.binding.snapshot.durationMs) :
            this.hudKind === 'speed' ? this.text($r('app.string.vesper_hold_speed')) :
            this.text(this.hudKind === 'volume' ? $r('app.string.vesper_volume') : $r('app.string.vesper_brightness')) + ' ' + Math.round(this.hudValue * 100) + '%')
            .fontSize(14).fontColor(this.colors().hudForeground)
          if (this.hudKind === 'volume' || this.hudKind === 'brightness') {
            Progress({ value: this.hudValue * 100, total: 100, type: ProgressType.Linear }).width(96).color(this.colors().accent)
              .backgroundColor(this.colors().timelineInactive).style({ strokeWidth: this.metrics().timelineTrackHeight })
          }
        }.padding(16).borderRadius(this.metrics().hudBorderRadius).backgroundColor(this.colors().hudBackground)
          .position({ x: this.stageWidth / 2 - 88, y: this.hudKind === 'speed' ? this.safeArea.top + 64 : this.stageHeight / 2 - 52 })
          .width(176).hitTestBehavior(HitTestMode.None).accessibilityLevel('no')
      }
    }.width('100%').height('100%').backgroundColor(this.colors().background)
      .bindSheet(this.settingsOpen(), this.settingsPanel(), { height: this.sheetHeight(),
        width: this.stageWidth >= 600 ? 380 : undefined, preferType: this.stageWidth >= 600 ? SheetType.CENTER : SheetType.BOTTOM,
        dragBar: this.stageWidth < 600, showClose: true,
        backgroundColor: this.currentSkin().settingsStyle === 'native' ? $r('sys.color.ohos_id_color_dialog_bg') : this.colors().settingsBackground,
        onWillDismiss: (action: DismissSheetAction): void => {
          if (this.currentPane() !== 'menu' && action.reason === DismissReason.PRESS_BACK) {
            this.updatePresentation(true, false, this.currentPane() === 'captionStyle' ? 'subtitle' : 'menu');
          }
          else { this.closeSettings(); action.dismiss(); }
        }, onDisappear: (): void => {
          if (this.settingsOpen()) { this.closeSettings(); }
          if (!this.disappeared && this.settingsTrigger.length > 0) { this.getUIContext().getFocusController().requestFocus(this.settingsTrigger); }
        } })
      .onAreaChange((before: Area, after: Area): void => {
        if (this.stageWidth !== Number(after.width) || this.stageHeight !== Number(after.height)) { this.cancelInteraction(); this.showControls(); }
        this.stageLeft = Number(after.globalPosition.x); this.stageWidth = Number(after.width); this.stageHeight = Number(after.height);
      })
  }
}
