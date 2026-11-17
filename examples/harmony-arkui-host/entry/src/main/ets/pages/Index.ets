import { common, ConfigurationConstant } from '@kit.AbilityKit';
import { window, SymbolGlyphModifier } from '@kit.ArkUI';
import { VesperNativeWindowAdapter, VesperWindowPolicy, playbackFeedback } from '@vesper/player-kit-arkui';
import { VesperPlayerStage, VesperPlayerStageSkin, VesperStageColors, VesperStageEnvironment, VesperStageInsets, VesperStagePresentation } from '@vesper/player-kit-arkui-ui';
import { ExampleModel } from '../model/ExampleModel';
import { ExampleFullscreen } from '../model/ExampleFullscreen';
import { examplePalette, ExamplePalette } from '../model/ExampleSupport';
import { PlaybackPanel } from '../components/PlaybackPanel';
import { DownloadsPanel } from '../components/DownloadsPanel';
import { DiagnosticsPanel } from '../components/DiagnosticsPanel';

function blueSkin(): VesperPlayerStageSkin {
  const colors = new VesperStageColors();
  colors.accent = $r('sys.color.ohos_id_color_emphasize');
  colors.timelineStart = '#549FFF'; colors.timelineEnd = '#91C8FF';
  return new VesperPlayerStageSkin(colors);
}

@Entry
@Component
struct Index {
  @State model: ExampleModel = new ExampleModel();
  @StorageLink('vesper.example.colorMode') colorMode: number = ConfigurationConstant.ColorMode.COLOR_MODE_LIGHT;
  @StorageLink('vesper.example.fontScale') fontScale: number = 1;
  @StorageLink('vesper.example.foreground') foreground: boolean = true;
  @State fullscreen: boolean = false;
  @State appearance: boolean = false;
  @State viewportWidth: number = 400;
  @State viewportHeight: number = 800;
  @State windowError: string = '';
  @State reducedMotion: boolean = false;
  @State stagePresentation: VesperStagePresentation = { controlsVisible: true, locked: false };
  @State insets: VesperStageInsets = { top: 0, right: 0, bottom: 0, left: 0 };
  private accentSkin: VesperPlayerStageSkin = blueSkin();
  private nativeWindow?: window.Window;
  private windowPolicy?: VesperWindowPolicy;
  private closed: boolean = false;
  private fullscreenPending: boolean = false;
  private fullscreenPolicy?: ExampleFullscreen;
  private pageScroller: Scroller = new Scroller();
  private inlineScrollOffset: number = 0;
  private pendingScrollOffset: number | undefined = undefined;
  private avoidAreaChanged: (event: window.AvoidAreaOptions) => void = (): void => { this.updateInsets(); };
  private environment: VesperStageEnvironment = { brightness: {
    current: (): number => this.windowPolicy?.brightness() ?? 0.5,
    set: (value: number): Promise<void> => this.windowPolicy?.setBrightness(value) ?? Promise.reject(new Error('Window is not ready'))
  } };
  private context(): common.UIAbilityContext { return this.getUIContext().getHostContext() as common.UIAbilityContext; }
  private text(resource: Resource): string { return this.context().resourceManager.getStringSync(resource.id); }
  private palette(theme: string, colorMode: number): ExamplePalette {
    return examplePalette(theme === 'dark' || theme === 'system' && colorMode === ConfigurationConstant.ColorMode.COLOR_MODE_DARK);
  }
  private wide(width: number): boolean { return width >= Math.max(840, 680 * Math.min(1.5, Math.max(1, this.fontScale))); }
  private sidebar(): number { return Math.min(400, Math.max(320, this.viewportWidth * 0.3)); }
  private playerWidth(): number { return Math.max(96, this.viewportWidth - (this.wide(this.viewportWidth) ? this.sidebar() + 40 : 40)); }
  private playerHeight(): number | string { return this.fullscreen ? this.viewportHeight : this.wide(this.viewportWidth) ? '100%' : 248; }
  aboutToAppear(): void {
    this.model.start(this.context());
    window.getLastWindow(this.context()).then(async (nativeWindow: window.Window): Promise<void> => {
      if (this.closed) { return; }
      const policy = new VesperWindowPolicy(new VesperNativeWindowAdapter(nativeWindow));
      try { this.model.binding.attachWindowPolicy(policy); }
      catch (error) { await policy.close(); throw error as Error; }
      this.nativeWindow = nativeWindow; this.windowPolicy = policy;
      this.fullscreenPolicy = new ExampleFullscreen(nativeWindow);
      nativeWindow.on('avoidAreaChange', this.avoidAreaChanged); this.updateInsets();
    }).catch((error: Error): void => {
      if (!this.closed) { this.windowError = this.text($r('app.string.example_window_failed')); this.model.binding.windowFeedback = playbackFeedback(error); }
    });
  }
  private updateInsets(): void {
    const nativeWindow = this.nativeWindow;
    if (nativeWindow === undefined || !this.fullscreen) { this.insets = { top: 0, right: 0, bottom: 0, left: 0 }; return; }
    const system = nativeWindow.getWindowAvoidArea(window.AvoidAreaType.TYPE_SYSTEM);
    const cutout = nativeWindow.getWindowAvoidArea(window.AvoidAreaType.TYPE_CUTOUT);
    const navigation = nativeWindow.getWindowAvoidArea(window.AvoidAreaType.TYPE_NAVIGATION_INDICATOR);
    const ui = this.getUIContext();
    this.insets = { top: ui.px2vp(Math.max(system.topRect.height, cutout.topRect.height)),
      right: ui.px2vp(Math.max(system.rightRect.width, cutout.rightRect.width)),
      bottom: ui.px2vp(Math.max(system.bottomRect.height, cutout.bottomRect.height, navigation.bottomRect.height)),
      left: ui.px2vp(Math.max(system.leftRect.width, cutout.leftRect.width)) };
  }
  private async setFullscreen(value: boolean): Promise<void> {
    const policy = this.fullscreenPolicy;
    if (this.closed || this.fullscreenPending || policy === undefined || value === this.fullscreen) { return; }
    this.fullscreenPending = true;
    try {
      if (value) { this.inlineScrollOffset = this.pageScroller.currentOffset().yOffset; }
      await policy.set(value, this.model.snapshot.width > this.model.snapshot.height);
      if (this.closed) { return; }
      this.pendingScrollOffset = value ? 0 : this.inlineScrollOffset;
      this.fullscreen = value;
      this.stagePresentation = { controlsVisible: true, locked: false }; this.updateInsets(); this.windowError = '';
    } catch (error) {
      if (!this.closed) { this.windowError = this.text($r('app.string.example_window_failed')); this.model.binding.windowFeedback = playbackFeedback(error as Error); }
    } finally { this.fullscreenPending = false; }
  }
  private restoreScroll(): void {
    const offset = this.pendingScrollOffset;
    if (offset === undefined || this.closed) { return; }
    this.pendingScrollOffset = undefined;
    this.pageScroller.scrollTo({ xOffset: 0, yOffset: offset, animation: false });
  }
  aboutToDisappear(): void {
    this.closed = true; this.model.binding.setPresentationVisible(false);
    const nativeWindow = this.nativeWindow; nativeWindow?.off('avoidAreaChange', this.avoidAreaChanged); this.nativeWindow = undefined;
    this.closeModel();
  }
  private closeModel(attempt: number = 0): void {
    const restoration = this.fullscreenPolicy?.close() ?? Promise.resolve();
    restoration.then((): Promise<void> => this.model.close()).catch((error: Error): void => {
      if (attempt < 2) { setTimeout((): void => { this.closeModel(attempt + 1); }, 1000); }
      else { console.error('Example cleanup failed: ' + playbackFeedback(error).code); }
    });
  }
  onBackPress(): boolean {
    const pane = this.stagePresentation.settingsPane;
    if (pane !== undefined) {
      this.stagePresentation = { controlsVisible: true, locked: false,
        settingsPane: pane === 'menu' ? undefined : pane === 'captionStyle' ? 'subtitle' : 'menu' }; return true;
    }
    if (this.fullscreen) { this.setFullscreen(false); return true; }
    if (this.model.tab !== 'play') { this.model.tab = 'play'; return true; }
    return false;
  }
  @Builder appearancePanel() {
    Column({ space: 8 }) {
      Row() {
        Text($r('app.string.example_appearance')).fontSize(16).fontColor(this.palette(this.model.theme, this.colorMode).title).layoutWeight(1)
        Select([{ value: this.text($r('app.string.example_system')) }, { value: this.text($r('app.string.example_light')) }, { value: this.text($r('app.string.example_dark')) }])
          .selected(['system', 'light', 'dark'].indexOf(this.model.theme))
          .value(this.text(this.model.theme === 'dark' ? $r('app.string.example_dark') : this.model.theme === 'light' ? $r('app.string.example_light') : $r('app.string.example_system')))
          .onSelect((index: number): void => { this.model.setTheme(['system', 'light', 'dark'][index]); }).accessibilityText(this.text($r('app.string.example_appearance')))
      }.width('100%').constraintSize({ minHeight: 56 })
      Row() {
        Text($r('app.string.example_inline_controls')).fontSize(16).fontColor(this.palette(this.model.theme, this.colorMode).title).layoutWeight(1)
        Select([{ value: this.text($r('app.string.example_compact')) }, { value: this.text($r('app.string.example_expanded')) }, { value: this.text($r('app.string.example_classic')) }])
          .selected(['compact', 'expanded', 'standard'].indexOf(this.model.layout))
          .value(this.text(this.model.layout === 'standard' ? $r('app.string.example_classic') : this.model.layout === 'expanded' ? $r('app.string.example_expanded') : $r('app.string.example_compact')))
          .onSelect((index: number): void => { this.model.layout = ['compact', 'expanded', 'standard'][index]; })
      }.width('100%').constraintSize({ minHeight: 56 })
      Row() {
        Text($r('app.string.example_fullscreen_controls')).fontSize(16).fontColor(this.palette(this.model.theme, this.colorMode).title).layoutWeight(1)
        Select([{ value: this.text($r('app.string.example_expanded')) }, { value: this.text($r('app.string.example_compact')) }])
          .selected(this.model.fullscreenLayout === 'compact' ? 1 : 0)
          .value(this.text(this.model.fullscreenLayout === 'compact' ? $r('app.string.example_compact') : $r('app.string.example_expanded')))
          .onSelect((index: number): void => { this.model.fullscreenLayout = index === 1 ? 'compact' : 'expanded'; })
      }.width('100%').constraintSize({ minHeight: 56 })
      Row() {
        Text($r('app.string.example_blue_accent')).fontSize(16).fontColor(this.palette(this.model.theme, this.colorMode).title).layoutWeight(1)
        Toggle({ type: ToggleType.Switch, isOn: this.model.accented }).accessibilityText(this.text($r('app.string.example_blue_accent')))
          .onChange((enabled: boolean): void => { this.model.accented = enabled; })
      }.width('100%').constraintSize({ minHeight: 56 })
      Row() {
        Text($r('app.string.example_reduced_motion')).fontSize(16).fontColor(this.palette(this.model.theme, this.colorMode).title).layoutWeight(1)
        Toggle({ type: ToggleType.Switch, isOn: this.reducedMotion }).accessibilityText(this.text($r('app.string.example_reduced_motion')))
          .onChange((enabled: boolean): void => { this.reducedMotion = enabled; })
      }.width('100%').constraintSize({ minHeight: 56 })
      Text($r('app.string.example_appearance_note')).fontSize(13).lineHeight(19).fontColor(this.palette(this.model.theme, this.colorMode).secondary).width('100%')
    }.padding({ left: 24, right: 24, bottom: 24 }).width('100%')
  }
  @Builder tabBar() {
    Tabs({ barPosition: BarPosition.End, index: ['play', 'diagnostics', 'downloads'].indexOf(this.model.tab) }) {
      TabContent() { Column() {} }.tabBar(BottomTabBarStyle.of({ normal: new SymbolGlyphModifier($r('sys.symbol.play_fill')) }, this.text($r('app.string.example_play'))))
      TabContent() { Column() {} }.tabBar(BottomTabBarStyle.of({ normal: new SymbolGlyphModifier($r('sys.symbol.waveform')) }, this.text($r('app.string.example_diagnostics'))))
      TabContent() { Column() {} }.tabBar(BottomTabBarStyle.of({ normal: new SymbolGlyphModifier($r('sys.symbol.download')) }, this.text($r('app.string.example_downloads'))))
    }.height(64).barHeight(64).width('100%').scrollable(false).animationDuration(0)
      .onChange((index: number): void => { this.model.tab = ['play', 'diagnostics', 'downloads'][index]; })
      .visibility(this.fullscreen ? Visibility.None : Visibility.Visible).backgroundColor(this.palette(this.model.theme, this.colorMode).surface)
  }
  @Builder themeSelector() {
    Scroll() {
      Row({ space: 10 }) {
        ForEach(['system', 'light', 'dark'], (mode: string) => {
          Button({ type: ButtonType.Capsule }) {
            Row({ space: 6 }) {
              SymbolGlyph(mode === 'dark' ? $r('sys.symbol.moon') : mode === 'light' ? $r('sys.symbol.sun_max') : $r('sys.symbol.circle_lefthalf_inset_filled'))
                .fontSize(16).fontColor([this.model.theme === mode ? Color.White : this.palette(this.model.theme, this.colorMode).title]).accessibilityLevel('no')
              Text(this.text(mode === 'dark' ? $r('app.string.example_dark') : mode === 'light' ? $r('app.string.example_light') : $r('app.string.example_system')))
                .fontSize(14).maxLines(1)
            }.padding({ left: 12, right: 12 }).height(40)
          }.padding(0).backgroundColor(this.model.theme === mode ? this.palette(this.model.theme, this.colorMode).accent : this.palette(this.model.theme, this.colorMode).surface)
            .fontColor(this.model.theme === mode ? Color.White : this.palette(this.model.theme, this.colorMode).title)
            .accessibilitySelected(this.model.theme === mode).onClick((): void => { this.model.setTheme(mode); })
        }, (mode: string): string => mode)
        Button({ type: ButtonType.Circle }) {
          SymbolGlyph($r('sys.symbol.gearshape')).fontSize(20).fontColor([this.palette(this.model.theme, this.colorMode).title]).accessibilityLevel('no')
        }.width(44).height(44).backgroundColor(this.palette(this.model.theme, this.colorMode).surface)
          .accessibilityText(this.text($r('app.string.example_appearance'))).onClick((): void => { this.appearance = true; })
      }
    }.width('100%').height(48).scrollable(ScrollDirection.Horizontal).scrollBar(BarState.Off)
      .visibility(this.fullscreen ? Visibility.None : Visibility.Visible)
  }
  build() {
    Navigation() {
      Column() {
        Stack({ alignContent: Alignment.TopStart }) {
          Scroll(this.pageScroller) {
            Column({ space: this.fullscreen ? 0 : 18 }) {
              Column({ space: 6 }) {
                Text(this.model.nowPlayingTitle).fontSize(20).fontWeight(FontWeight.Medium).fontColor(this.palette(this.model.theme, this.colorMode).title).width('100%').maxLines(2)
                Text(this.model.current()?.detail ?? this.text($r('app.string.example_description_empty'))).fontSize(13).lineHeight(19)
                  .fontColor(this.palette(this.model.theme, this.colorMode).secondary).maxLines(2).width('100%')
              }.alignItems(HorizontalAlign.Start).width('100%').visibility(this.fullscreen ? Visibility.None : Visibility.Visible)
              this.themeSelector()
              Flex({ direction: this.wide(this.viewportWidth) && !this.fullscreen ? FlexDirection.Row : FlexDirection.Column }) {
                // Scrolling and fullscreen retain this single Stage and native Surface.
                Column() {
                  VesperPlayerStage({ binding: this.model.binding, title: this.model.nowPlayingTitle,
                    controlLayout: this.fullscreen ? this.model.fullscreenLayout : this.model.layout,
                    interactionEnabled: this.model.tab === 'play' && !this.model.pipActive && this.foreground,
                    isFullscreen: this.fullscreen, skin: this.model.accented ? this.accentSkin : undefined,
                    presentation: this.stagePresentation, onPresentationChange: (value: VesperStagePresentation): void => { this.stagePresentation = value; },
                    reducedMotion: this.reducedMotion, availableHeight: this.viewportHeight, environment: this.environment, safeArea: this.insets,
                    onPictureInPicture: (): void => { this.model.pictureInPicture(); },
                    onFullscreenChange: (value: boolean): void => { this.setFullscreen(value); },
                    onChangeSource: (): void => { if (this.fullscreen) { this.setFullscreen(false); } else { this.model.pickVideo(); } },
                    onRetry: (): void => { this.model.retry(); } })
                    .width('100%').height(this.playerHeight()).clip(true)
                }.width(this.fullscreen ? '100%' : this.wide(this.viewportWidth) ? this.playerWidth() : '100%')
                  .height(this.playerHeight()).flexShrink(0)
                Scroll() {
                  PlaybackPanel({ model: this.model, palette: this.palette(this.model.theme, this.colorMode), contentPadding: 0 }).width('100%')
                }.width(this.wide(this.viewportWidth) ? this.sidebar() : '100%')
                  .height(this.wide(this.viewportWidth) && !this.fullscreen ? '100%' : undefined)
                  .enableScrollInteraction(this.wide(this.viewportWidth) && !this.fullscreen)
                  .scrollBar(this.wide(this.viewportWidth) ? BarState.Auto : BarState.Off)
                  .padding({ top: this.wide(this.viewportWidth) ? 0 : 18, left: this.wide(this.viewportWidth) ? 20 : 0 })
                  .visibility(this.fullscreen ? Visibility.None : Visibility.Visible)
              }.width('100%').layoutWeight(this.wide(this.viewportWidth) && !this.fullscreen ? 1 : 0)
            }.width('100%').height(this.wide(this.viewportWidth) && !this.fullscreen ? '100%' : undefined)
              .padding(this.fullscreen ? 0 : 20).onAreaChange((): void => { this.restoreScroll(); })
          }.width('100%').height('100%').scrollBar(this.fullscreen || this.wide(this.viewportWidth) ? BarState.Off : BarState.Auto)
            .enableScrollInteraction(!this.fullscreen && !this.wide(this.viewportWidth))
            .visibility(this.model.tab === 'play' ? Visibility.Visible : Visibility.Hidden)
          if (this.model.tab === 'diagnostics') {
            Scroll() { DiagnosticsPanel({ model: this.model, palette: this.palette(this.model.theme, this.colorMode) }) }.width('100%').height('100%').scrollBar(BarState.Auto)
          }
          if (this.model.tab === 'downloads') {
            Scroll() { DownloadsPanel({ model: this.model, palette: this.palette(this.model.theme, this.colorMode) }) }.width('100%').height('100%').scrollBar(BarState.Auto)
          }
        }.width('100%').layoutWeight(1)
        if (!this.fullscreen && (this.model.message.length > 0 || this.windowError.length > 0 || this.model.busy)) {
          Row({ space: 8 }) {
            if (this.model.busy) { LoadingProgress().width(18).height(18).color(this.palette(this.model.theme, this.colorMode).accent) }
            Text(this.windowError || this.model.message || this.text($r('app.string.example_ready'))).fontSize(12).maxLines(3)
              .fontColor(this.model.messageFailed || this.windowError.length > 0 ? this.palette(this.model.theme, this.colorMode).danger : this.palette(this.model.theme, this.colorMode).secondary).layoutWeight(1)
          }.padding({ left: 20, right: 20, top: 8, bottom: 8 }).width('100%')
        }
        this.tabBar()
      }.width('100%').height('100%').linearGradient({ angle: 180, colors: [[this.palette(this.model.theme, this.colorMode).top, 0], [this.palette(this.model.theme, this.colorMode).bottom, 1]] })
        .onAreaChange((before: Area, after: Area): void => { this.viewportWidth = Number(after.width); this.viewportHeight = Number(after.height); })
    }.title(this.model.tab === 'play' ? 'Vesper' : this.text(this.model.tab === 'downloads' ? $r('app.string.example_downloads') : $r('app.string.example_diagnostics')))
      .titleMode(NavigationTitleMode.Mini).mode(NavigationMode.Stack).hideBackButton(true).hideTitleBar(this.fullscreen)
      .bindSheet($$this.appearance, this.appearancePanel(), { height: SheetSize.FIT_CONTENT, title: { title: this.text($r('app.string.example_settings')) }, dragBar: true, showClose: true })
  }
}
