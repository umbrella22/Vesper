import { common } from '@kit.AbilityKit';
import { VesperPlayerController, VesperSourceSession, VesperSystemSession, VesperPlayerError } from '@vesper/player-kit';
import { VesperPlayerBinding, VesperPictureInPicture } from '@vesper/player-kit-arkui';
import { VesperPlayerStage, VesperPlayerStageSkin, VesperStageColors, VesperPlayerStageIcons,
  VesperStageIconConfiguration, VesperStageIconRole, VesperStagePresentation, VesperStageIconButton, VesperSubtitleStyle } from '@vesper/player-kit-arkui-ui';

@Builder
function fixtureIcon(value: VesperStageIconConfiguration) {
  Stack() {
    Rect().width(value.size).height(value.size).radius(4).fill('#8B5CF6')
    SymbolGlyph($r('sys.symbol.ellipsis_circle')).fontSize(value.size * 0.8 + 'vp').fontColor([value.color])
  }.width(value.size).height(value.size)
}

@Entry
@Component
struct Index {
  @State binding: VesperPlayerBinding = new VesperPlayerBinding(new VesperPlayerController());
  @State secondary: VesperPlayerBinding = new VesperPlayerBinding(new VesperPlayerController());
  @State result: string = 'Ready';
  @State layout: string = 'standard';
  @State fullscreen: boolean = false;
  @State skin: VesperPlayerStageSkin | undefined = undefined;
  @State running: boolean = false;
  @State presentation: VesperStagePresentation = { controlsVisible: true, locked: false };
  @State secondarySkin: VesperPlayerStageSkin = new VesperPlayerStageSkin();
  @State slotActions: number = 0;
  @State captions: VesperSubtitleStyle = new VesperSubtitleStyle();
  @State fontScale: number = 1;
  private sources: VesperSourceSession = new VesperSourceSession();
  private pip?: VesperPictureInPicture;
  private system?: VesperSystemSession;
  private async setFontScale(scale: number): Promise<void> {
    if (this.running) { return; } this.running = true;
    const before = this.binding.controller.snapshot(); const surface = this.binding.surfaceId();
    try {
      const context = this.getUIContext().getHostContext() as common.UIAbilityContext;
      context.getApplicationContext().setFontSizeScale(scale);
      await new Promise<void>((resolve) => { setTimeout(resolve, 400); });
      this.fontScale = context.config.fontSizeScale ?? 1;
      const after = this.binding.controller.snapshot();
      if (Math.abs(this.fontScale - scale) > 0.01 || before.playbackEpoch !== after.playbackEpoch || surface !== this.binding.surfaceId()) {
        throw new Error('Font scaling must apply without replacing playback or its Surface');
      }
      this.result = JSON.stringify({ status: 'PASS', scope: 'native-font-scale', requested: scale, actual: this.fontScale,
        playbackEpoch: after.playbackEpoch, surface: surface });
    } catch (error) { this.result = JSON.stringify({ status: 'FAIL', error: `${error}` }); }
    finally { this.running = false; }
  }
  private async showCaptions(): Promise<void> {
    if (this.running) { return; } this.running = true;
    try {
      const source = this.sources.register({ kind: 'remote', protocol: 'progressive', uri: 'http://127.0.0.1:18765/probe.mp4', label: 'Caption fixture',
        headers: { 'X-Vesper-Probe': 'h00' }, externalSubtitles: [{ id: 'caption-geometry', uri: 'http://127.0.0.1:18765/caption-long.vtt', mimeType: 'text/vtt', label: 'English and Chinese', headers: { 'X-Vesper-Probe': 'h00' } }] });
      try { await this.binding.controller.activate(source, { playWhenReady: false }); } finally { source.close(); }
      const track = this.binding.controller.snapshot().tracks.find((item) => item.kind === 'subtitle');
      if (track === undefined) { throw new Error('The caption track is missing'); }
      await this.binding.controller.selectTrack({ kind: 'subtitle', mode: 'track', trackId: track.id });
      await this.binding.controller.seek(1000); this.captions = new VesperSubtitleStyle({ fontScale: 1.5, maxLines: 2 });
      this.result = JSON.stringify({ status: 'PASS', scope: 'caption-display-fixture', playback: this.binding.controller.snapshot() });
    } catch (error) { this.result = JSON.stringify({ status: 'FAIL', error: `${error}` }); }
    finally { this.running = false; }
  }
  @Builder fixtureOverlay() { Rect().width(24).height(3).fill('#80FFFFFF').position({ x: 12, y: 108 }) }
  @Builder leading() {
    VesperStageIconButton({ role: VesperStageIconRole.SkipForward, label: 'Host action',
      onAction: (): void => { this.slotActions++; } })
  }
  private async runCustomization(): Promise<void> {
    if (this.running) { return; } this.running = true;
    try {
      for (const binding of [this.binding, this.secondary]) {
        const source = this.sources.register({ kind: 'remote', protocol: 'progressive', uri: 'http://127.0.0.1:18765/probe.mp4',
          label: 'Customization fixture', headers: { 'X-Vesper-Probe': 'h00' } });
        try { await binding.controller.activate(source, { playWhenReady: false, timeoutMs: 15000 }); } finally { source.close(); }
      }
      const before = this.binding.controller.snapshot(); const other = this.secondary.controller.snapshot();
      const firstSurface = this.binding.surfaceId(); const secondSurface = this.secondary.surfaceId();
      if (!firstSurface || !secondSurface || firstSurface === secondSurface || this.binding.surfaceIdentity === this.secondary.surfaceIdentity) {
        throw new Error('Independent Stages require independent native Surfaces and component identities');
      }
      const icons = new VesperPlayerStageIcons(); icons.play = $r('sys.symbol.arrow_clockwise');
      const colors = new VesperStageColors(); colors.accent = '#8B5CF6';
      this.skin = new VesperPlayerStageSkin(colors, undefined, { icons: icons,
        iconContent: wrapBuilder(fixtureIcon), customIconRoles: [VesperStageIconRole.More] });
      this.layout = 'expanded'; this.presentation = { controlsVisible: true, locked: false, settingsPane: 'speed' };
      await new Promise<void>((resolve) => { setTimeout(resolve, 400); });
      this.presentation = { controlsVisible: true, locked: false };
      this.layout = 'compact';
      await new Promise<void>((resolve) => { setTimeout(resolve, 400); });
      if (this.secondarySkin.colors.accent === this.skin.colors.accent) { throw new Error('A skin escaped its Stage'); }
      this.skin = undefined;
      await new Promise<void>((resolve) => { setTimeout(resolve, 300); });
      const after = this.binding.controller.snapshot(); const otherAfter = this.secondary.controller.snapshot();
      if (after.playbackEpoch !== before.playbackEpoch || after.sourceId !== before.sourceId || this.binding.surfaceId() !== firstSurface ||
        otherAfter.playbackEpoch !== other.playbackEpoch || otherAfter.sourceId !== other.sourceId || this.secondary.surfaceId() !== secondSurface) {
        throw new Error('Presentation customization changed playback ownership');
      }
      this.result = JSON.stringify({ status: 'PASS', scope: 'har-only-customization', partialIconOverride: true, wrappedBuilder: true,
        independentSkins: true, controlledAndUncontrolled: true, defaultRestored: true, firstSurface: firstSurface, secondSurface: secondSurface,
        sourceId: after.sourceId, playbackEpoch: after.playbackEpoch, secondaryEpoch: otherAfter.playbackEpoch });
    } catch (error) { this.result = JSON.stringify({ status: 'FAIL', error: `${error}` }); }
    finally { this.running = false; }
    console.info(`VESPER_HARMONY_CUSTOMIZATION ${this.result}`);
  }
  private async runRejectedControlCheck(): Promise<void> {
    if (this.running) { return; } this.running = true;
    try {
      const source = this.sources.register({ kind: 'remote', protocol: 'progressive', uri: 'http://127.0.0.1:18765/probe.mp4', label: 'Rejected control fixture',
        headers: { 'X-Vesper-Probe': 'h00' } });
      try { await this.binding.controller.activate(source, { playWhenReady: false, timeoutMs: 15000 }); } finally { source.close(); }
      const before = this.binding.controller.snapshot(); const surface = this.binding.surfaceId();
      if (before.availableBitrates.length !== 0) { throw new Error('The regression requires a finite MP4 without native bitrates'); }
      const command = this.binding.controller.selectTrack({ kind: 'video', mode: 'auto' });
      this.binding.report(command);
      let code: string = '';
      try { await command; } catch (error) { code = (error as VesperPlayerError).code; }
      const after = this.binding.controller.snapshot();
      if (code !== 'unsupported' || this.binding.error.length === 0 || after.error !== undefined || after.state === 'error' ||
        before.playbackEpoch !== after.playbackEpoch || surface !== this.binding.surfaceId()) {
        throw new Error('A rejected control must report its error and retain healthy playback ownership');
      }
      this.result = JSON.stringify({ status: 'PASS', unsupportedControl: code, commandErrorVisible: true,
        playbackHealthy: true, playbackEpoch: after.playbackEpoch, surface: surface });
    } catch (error) { this.result = JSON.stringify({ status: 'FAIL', error: `${error}` }); }
    finally { this.running = false; }
  }
  private async run(): Promise<void> {
    if (this.running) { return; } this.running = true; this.result = 'Running UI integration'; let stage = 'activate';
    try {
      const context = this.getUIContext().getHostContext() as common.UIAbilityContext;
      const source = this.sources.register({ kind: 'remote', protocol: 'hls', uri: 'http://127.0.0.1:18765/hls/master.m3u8',
        label: 'Stage fixture', headers: { 'X-Vesper-Probe': 'h00' } });
      try { await this.binding.controller.activate(source, { timeoutMs: 15000 }); } finally { source.close(); }
      const epoch = this.binding.snapshot.playbackEpoch; const surface = this.binding.surfaceId();
      this.layout = 'compact'; this.fullscreen = true;
      const colors = new VesperStageColors(); colors.buttonBackground = '#087f72'; this.skin = new VesperPlayerStageSkin(colors);
      await new Promise<void>((resolve) => { setTimeout(resolve, 300); });
      if (this.binding.surfaceId() !== surface || this.binding.snapshot.playbackEpoch !== epoch) { throw new Error('Layout or skin change recreated playback or Surface'); }
      this.skin = undefined;
      await new Promise<void>((resolve) => { setTimeout(resolve, 300); });
      if (this.binding.surfaceId() !== surface || this.binding.snapshot.playbackEpoch !== epoch) { throw new Error('Default skin restoration recreated playback or Surface'); }
      await this.binding.controller.seek(3000); await this.binding.controller.setPlaybackRate(1.5);
      stage = 'system'; this.system = await VesperSystemSession.create(context, this.binding.controller,
        { bundleName: 'dev.vesper.harmony.uiconsumer', abilityName: 'EntryAbility', title: 'Vesper UI fixture', backgroundAudio: true });
      stage = 'pip-create'; this.pip = await VesperPictureInPicture.create(context, this.binding);
      await this.pip.setAutoStart(true); await this.pip.setAutoStart(false);
      stage = 'pip-start'; await this.pip.start();
      const started = this.pip.snapshot(); await new Promise<void>((resolve) => { setTimeout(resolve, 300); });
      stage = 'pip-stop'; await this.pip.stop();
      if (this.binding.snapshot.playbackEpoch !== epoch || this.binding.snapshot.retainedNativePlayers !== 1) { throw new Error('PiP replaced or duplicated native playback'); }
      const playing = this.binding.controller.snapshot();
      stage = 'owned-dispose'; await this.binding.controller.dispose();
      if (this.system.snapshot().state !== 'closed' || this.pip.snapshot().state !== 'closed' || this.binding.controller.snapshot().retainedNativePlayers !== 0) { throw new Error('Owned system/PiP cleanup did not finish'); }
      this.result = JSON.stringify({ status: 'PASS', skinChangeRetainedPlayback: true, playing: playing, pipStarted: started, pipClosed: this.pip.snapshot(), system: this.system.snapshot() });
    } catch (error) { this.result = JSON.stringify({ status: 'FAIL', stage: stage, error: `${error}`, playback: this.binding.controller.snapshot(), pip: this.pip?.snapshot() }); }
    finally { this.running = false; }
    console.info(`VESPER_HARMONY_UI ${this.result}`);
  }
  private async runPiPSizeChecks(): Promise<void> {
    if (this.running) { return; } this.running = true; this.result = 'Running PiP size checks';
    const observations: string[] = []; let stage = 'landscape';
    try {
      const context = this.getUIContext().getHostContext() as common.UIAbilityContext;
      const surface = this.binding.surfaceId();
      for (const file of ['probe.mp4', 'portrait.mp4', 'probe.mp4']) {
        stage = file;
        const source = this.sources.register({ kind: 'remote', protocol: 'progressive',
          uri: 'http://127.0.0.1:18765/' + file, label: 'PiP ' + file });
        try { await this.binding.controller.activate(source); } finally { source.close(); }
        const deadline = Date.now() + 15000;
        while (!this.binding.controller.snapshot().firstFrame || this.binding.controller.snapshot().positionMs < 200) {
          if (Date.now() >= deadline) { throw new Error('PiP source did not render before its observation deadline'); }
          await new Promise<void>((resolve) => { setTimeout(resolve, 50); });
        }
        await this.binding.controller.pause();
        if (this.pip === undefined) {
          this.pip = await VesperPictureInPicture.create(context, this.binding); await this.pip.start();
        }
        await new Promise<void>((resolve) => { setTimeout(resolve, 300); });
        const playback = this.binding.controller.snapshot(); const pip = this.pip.snapshot();
        if (!playback.firstFrame || playback.state !== 'paused' || surface !== this.binding.surfaceId() || pip.contentWidth !== playback.width || pip.contentHeight !== playback.height
          || pip.contentEpoch !== playback.playbackEpoch || pip.sizeOrigin !== 'media' || pip.sizeState !== 'applied'
          || file === 'portrait.mp4' && playback.width >= playback.height || file === 'probe.mp4' && playback.width <= playback.height) {
          throw new Error('PiP did not apply the current media dimensions on its original Surface');
        }
        observations.push(JSON.stringify({ file: file, playback: playback, pip: pip, surface: surface }));
        this.result = 'PiP size observation: ' + file;
        await new Promise<void>((resolve) => { setTimeout(resolve, 6000); });
      }
      await this.pip?.close(); this.pip = undefined;
      this.result = JSON.stringify({ status: 'PASS', scope: 'native-pip-landscape-portrait-source-change', observations: observations });
    } catch (error) { this.result = JSON.stringify({ status: 'FAIL', stage: stage, error: `${error}`, observations: observations,
      playback: this.binding.controller.snapshot(), pip: this.pip?.snapshot() }); }
    finally { this.running = false; }
  }
  aboutToDisappear(): void { this.sources.close(); this.binding.report(this.binding.close(true)); this.secondary.report(this.secondary.close(true)); }
  build() {
    Column({ space: 12 }) {
      Text('Vesper ArkUI HAR consumer').fontSize(24).maxLines(1).textOverflow({ overflow: TextOverflow.Ellipsis })
      VesperPlayerStage({ binding: this.binding, controlLayout: this.layout, isFullscreen: this.fullscreen, skin: this.skin,
        presentation: this.presentation, onPresentationChange: (value: VesperStagePresentation): void => { this.presentation = value; },
        subtitleStyle: this.captions, onSubtitleStyleChange: (value: VesperSubtitleStyle): void => { this.captions = value; },
        expandedLeadingActions: this.leading, expandedLeadingWidth: 48, contentOverlay: this.fixtureOverlay,
        title: 'Controlled Stage', onFullscreenChange: (value: boolean): void => { this.fullscreen = value; } }).height(220)
      VesperPlayerStage({ binding: this.secondary, controlLayout: 'compact', skin: this.secondarySkin, title: 'Independent uncontrolled Stage' }).height(180)
      Scroll() {
        Column({ space: 12 }) {
          Button('Run PiP size checks').enabled(!this.running).onClick(() => { this.runPiPSizeChecks(); })
          Button('Run customization checks').enabled(!this.running).onClick(() => { this.runCustomization(); })
          Button('Show caption fixture').enabled(!this.running).onClick(() => { this.showCaptions(); })
          Button('Run UI checks').enabled(!this.running).onClick(() => { this.run(); })
          Button('Run rejected control check').enabled(!this.running).onClick(() => { this.runRejectedControlCheck(); })
          Text('Actual font scale: ' + this.fontScale).fontSize(14)
          ForEach([1, 1.3, 2], (scale: number) => {
            Button('Font ' + scale).enabled(!this.running).onClick(() => { this.setFontScale(scale); })
          })
          Text(this.result).fontSize(12).width('100%')
        }.width('100%')
      }.layoutWeight(1).width('100%')
    }.width('100%').height('100%').padding(16)
  }
}
