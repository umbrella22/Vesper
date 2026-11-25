import { common, ConfigurationConstant } from '@kit.AbilityKit';
import { window, SymbolGlyphModifier } from '@kit.ArkUI';
import { VesperNativeWindowAdapter, VesperWindowPolicy, playbackFeedback } from '@vesper/player-kit-arkui';
import { VesperPlayerStage, VesperPlayerStageSkin, VesperStageColors, VesperStageEnvironment, VesperStageInsets, VesperStagePresentation } from '@vesper/player-kit-arkui-ui';
import { ExampleModel } from '../model/ExampleModel';
import { ExampleFullscreen } from '../model/ExampleFullscreen';
import { examplePalette, ExamplePalette } from '../model/ExampleSupport';
import { isWideLayout, parentStagePane, shellBackAction, sidebarWidth, tabAt, tabIndex } from '../model/ExampleShell';
import { PlaybackPanel } from '../components/PlaybackPanel';
import { DownloadsPanel } from '../components/DownloadsPanel';
import { DiagnosticsPanel } from '../components/DiagnosticsPanel';
import { CollectionPanel, LibraryPanel } from '../components/LibraryPanel';
import { SettingsSheet } from '../components/SettingsSheet';

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
  @Provide('vesper.example.routes') routes: NavPathStack = new NavPathStack();
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
  private wide(width: number): boolean { return isWideLayout(width, this.fontScale); }
  private sidebar(): number { return sidebarWidth(this.viewportWidth); }
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
    const action = shellBackAction({ sheet: this.appearance ? 'settings' : '', routes: this.routes.size(),
      settingsPane: pane, fullscreen: this.fullscreen, tab: this.model.tab });
    if (action === 'closeSheet') { this.appearance = false; return true; }
    if (action === 'stagePane' && pane !== undefined) {
      this.stagePresentation = { controlsVisible: true, locked: false, settingsPane: parentStagePane(pane) }; return true;
    }
    if (action === 'exitFullscreen') { this.setFullscreen(false); return true; }
    if (action === 'popRoute') { this.routes.pop(); return true; }
    if (action === 'homeTab') { this.model.tab = 'play'; return true; }
    return false;
  }
  @Builder settingsSheet() {
    SettingsSheet({ model: this.model, palette: this.palette(this.model.theme, this.colorMode), reducedMotion: this.reducedMotion,
      onReducedMotionChange: (value: boolean): void => { this.reducedMotion = value; } })
  }
  @Builder tabBar(vertical: boolean) {
    Tabs({ barPosition: vertical ? BarPosition.Start : BarPosition.End, index: tabIndex(this.model.tab) }) {
      TabContent() { Column() {} }.tabBar(BottomTabBarStyle.of({ normal: new SymbolGlyphModifier($r('sys.symbol.play_fill')) }, this.text($r('app.string.example_play'))))
      TabContent() { Column() {} }.tabBar(BottomTabBarStyle.of({ normal: new SymbolGlyphModifier($r('sys.symbol.film')) }, this.text($r('app.string.example_library'))))
      TabContent() { Column() {} }.tabBar(BottomTabBarStyle.of({ normal: new SymbolGlyphModifier($r('sys.symbol.download')) }, this.text($r('app.string.example_downloads'))))
      TabContent() { Column() {} }.tabBar(BottomTabBarStyle.of({ normal: new SymbolGlyphModifier($r('sys.symbol.waveform')) }, this.text($r('app.string.example_lab'))))
    }.vertical(vertical).barWidth(vertical ? 96 : '100%').barHeight(vertical ? '100%' : 64)
      .width(vertical ? 96 : '100%').height(vertical ? '100%' : 64).scrollable(false).animationDuration(0)
      .onChange((index: number): void => { this.model.tab = tabAt(index); })
      .visibility(this.fullscreen ? Visibility.None : Visibility.Visible).backgroundColor(this.palette(this.model.theme, this.colorMode).surface)
  }
  @Builder playPage() {
    Scroll(this.pageScroller) {
      Column({ space: this.fullscreen ? 0 : 18 }) {
        Column({ space: 6 }) {
          Text(this.model.nowPlayingTitle).fontSize(20).fontWeight(FontWeight.Medium).fontColor(this.palette(this.model.theme, this.colorMode).title).width('100%').maxLines(2)
          Text(this.model.current()?.detail ?? this.text($r('app.string.example_description_empty'))).fontSize(13).lineHeight(19)
            .fontColor(this.palette(this.model.theme, this.colorMode).secondary).maxLines(2).width('100%')
        }.alignItems(HorizontalAlign.Start).width('100%').visibility(this.fullscreen ? Visibility.None : Visibility.Visible)
        Flex({ direction: this.wide(this.viewportWidth) && !this.fullscreen ? FlexDirection.Row : FlexDirection.Column }) {
          // Scrolling and fullscreen retain this single Stage and native Surface.
          Column() {
            VesperPlayerStage({ binding: this.model.binding, title: this.model.nowPlayingTitle,
              controlLayout: this.fullscreen ? this.model.fullscreenLayout : this.model.layout,
              interactionEnabled: this.model.tab === 'play' && this.routes.size() === 0 && !this.model.pipActive && this.foreground,
              isFullscreen: this.fullscreen, skin: this.model.accented ? this.accentSkin : undefined,
              presentation: this.stagePresentation, onPresentationChange: (value: VesperStagePresentation): void => { this.stagePresentation = value; },
              reducedMotion: this.reducedMotion, availableHeight: this.viewportHeight, environment: this.environment, safeArea: this.insets,
              onPictureInPicture: (): void => { this.model.pictureInPicture(); },
              onFullscreenChange: (value: boolean): void => { this.setFullscreen(value); },
              onChangeSource: (): void => { if (this.fullscreen) { this.setFullscreen(false); } else { this.model.pickVideo(); } },
              onRetry: (): void => { this.model.retry(); } })
              .width('100%').height(this.playerHeight()).clip(true)
          }.width(this.fullscreen ? '100%' : this.wide(this.viewportWidth) ? this.playerWidth() : '100%')
            .height(this.playerHeight()).flexShrink(0).backgroundColor($r('app.color.stage'))
            .borderRadius(this.fullscreen ? 0 : 16).clip(true)
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
  }
  @Builder routeMap(name: string, param: Object) {
    if (name === 'collection') {
      NavDestination() {
        Scroll() {
          CollectionPanel({ model: this.model, palette: this.palette(this.model.theme, this.colorMode), collection: String(param),
            onPlayed: (): void => { this.routes.clear(); } })
        }.width('100%').height('100%').align(Alignment.Top).scrollBar(BarState.Auto)
      }.title(this.model.text('example_collection_' + String(param))).backgroundColor(this.palette(this.model.theme, this.colorMode).top)
    }
  }
  @Builder statusRow() {
    if (!this.fullscreen && (this.model.message.length > 0 || this.windowError.length > 0 || this.model.busy)) {
      Row({ space: 8 }) {
        if (this.model.busy) { LoadingProgress().width(18).height(18).color(this.palette(this.model.theme, this.colorMode).secondary) }
        Text(this.windowError || this.model.message || this.text($r('app.string.example_ready'))).fontSize(12).maxLines(3)
          .fontColor(this.model.messageFailed || this.windowError.length > 0 ? this.palette(this.model.theme, this.colorMode).danger : this.palette(this.model.theme, this.colorMode).secondary).layoutWeight(1)
      }.padding({ left: 20, right: 20, top: 8, bottom: 8 }).width('100%')
    }
  }
  private title(): string {
    if (this.model.tab === 'play') { return 'Vesper'; }
    return this.text(this.model.tab === 'library' ? $r('app.string.example_library') : this.model.tab === 'downloads' ? $r('app.string.example_downloads') : $r('app.string.example_lab'));
  }
  build() {
    Navigation(this.routes) {
      Row() {
        if (this.wide(this.viewportWidth) && !this.fullscreen) { this.tabBar(true) }
        Column() {
          Stack({ alignContent: Alignment.TopStart }) {
            this.playPage()
            if (this.model.tab === 'library') {
              Scroll() { LibraryPanel({ model: this.model, palette: this.palette(this.model.theme, this.colorMode),
                onOpenCollection: (collection: string): void => { this.routes.pushPath({ name: 'collection', param: collection }); } }) }
                .width('100%').height('100%').align(Alignment.Top).scrollBar(BarState.Auto)
            }
            if (this.model.tab === 'lab') {
              Scroll() { DiagnosticsPanel({ model: this.model, palette: this.palette(this.model.theme, this.colorMode) }) }.width('100%').height('100%').scrollBar(BarState.Auto)
            }
            if (this.model.tab === 'downloads') {
              Scroll() { DownloadsPanel({ model: this.model, palette: this.palette(this.model.theme, this.colorMode) }) }.width('100%').height('100%').align(Alignment.Top).scrollBar(BarState.Auto)
            }
          }.width('100%').layoutWeight(1)
          this.statusRow()
          if (!this.wide(this.viewportWidth)) { this.tabBar(false) }
        }.layoutWeight(1).height('100%')
      }.width('100%').height('100%')
        .backgroundColor(this.fullscreen ? $r('app.color.stage') : this.palette(this.model.theme, this.colorMode).top)
        .onAreaChange((before: Area, after: Area): void => { this.viewportWidth = Number(after.width); this.viewportHeight = Number(after.height); })
    }.title(this.title()).titleMode(NavigationTitleMode.Mini).mode(NavigationMode.Stack).hideBackButton(true).hideTitleBar(this.fullscreen)
      .menus(this.fullscreen ? [] : [{ value: this.text($r('app.string.example_settings')), symbolIcon: new SymbolGlyphModifier($r('sys.symbol.gearshape')),
        action: (): void => { this.appearance = true; } }])
      .navDestination(this.routeMap)
      .bindSheet($$this.appearance, this.settingsSheet(), { height: SheetSize.MEDIUM, title: { title: this.text($r('app.string.example_settings')) },
        dragBar: true, showClose: true, backgroundColor: this.palette(this.model.theme, this.colorMode).surface })
  }
}
