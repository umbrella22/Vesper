import { VesperPlayerBinding, VesperTrack, VesperOperationFeedback } from '@vesper/player-kit-arkui';
import { VesperStageEnvironment } from './Gestures';
import { VesperPlayerStageSkin } from './Skin';
import { VesperSubtitleStyle, VesperSubtitleStyleOptions, stageQuality, stageTracks, stageTrackSelected, stageTrackDetail, stageLanguageBase } from './Presentation';
import { stageText, feedbackResource } from './Strings';

interface SettingRow { title: string; detail?: string; selected?: boolean; enabled?: boolean; action: () => void; }

/** Native sheet chrome and branded sheet content use complete, separate color roles. */
@Component
export struct StageSettings {
  @ObjectLink binding: VesperPlayerBinding;
  @Prop @Watch('initialChanged') initialPane: string = 'menu';
  @Prop skin: VesperPlayerStageSkin = new VesperPlayerStageSkin();
  @Prop subtitleStyle: VesperSubtitleStyle = new VesperSubtitleStyle();
  @State pane: string = 'menu';
  @State details: boolean = false;
  onPaneChange?: (pane: string) => void;
  onLock: () => void = (): void => {};
  onClose: () => void = (): void => {};
  onPiP: () => void = (): void => {};
  onSubtitleStyleChange?: (style: VesperSubtitleStyle) => void;
  environment?: VesperStageEnvironment;
  pipAvailable: boolean = false;
  private scroller: Scroller = new Scroller();
  private scrollSelection: boolean = true;
  private rates: number[] = [0.25, 0.5, 0.75, 1, 1.25, 1.5, 1.75, 2, 3, 4];
  private text(resource: Resource, index?: number): string { return stageText(this.getUIContext(), resource, index); }
  private foreground(): ResourceColor { return this.skin.settingsStyle === 'native' ? $r('sys.color.ohos_id_color_text_primary') : this.skin.colors.settingsForeground; }
  private secondary(): ResourceColor { return this.skin.settingsStyle === 'native' ? $r('sys.color.ohos_id_color_text_secondary') : this.skin.colors.settingsSecondaryForeground; }
  private accent(): ResourceColor { return this.skin.settingsStyle === 'native' ? $r('sys.color.ohos_id_color_emphasize') : this.skin.colors.settingsAccent; }
  aboutToAppear(): void { this.pane = this.initialPane; }
  private initialChanged(): void { this.pane = this.initialPane; this.scrollSelection = true; }
  private navigate(pane: string): void {
    this.pane = pane; this.scrollSelection = true; this.details = false; this.onPaneChange?.(pane);
  }
  private title(): Resource {
    return this.pane === 'speed' ? $r('app.string.vesper_speed') : this.pane === 'video' ? $r('app.string.vesper_quality') :
      this.pane === 'audio' ? $r('app.string.vesper_audio') : this.pane === 'subtitle' ? $r('app.string.vesper_subtitles') :
      this.pane === 'captionStyle' ? $r('app.string.vesper_captions_style') : $r('app.string.vesper_more');
  }
  private language(language: string): string {
    switch (stageLanguageBase(language)) {
      case 'en': return this.text($r('app.string.vesper_language_en'));
      case 'zh': return this.text($r('app.string.vesper_language_zh'));
      case 'ja': return this.text($r('app.string.vesper_language_ja'));
      case 'ko': return this.text($r('app.string.vesper_language_ko'));
      case 'fr': return this.text($r('app.string.vesper_language_fr'));
      case 'de': return this.text($r('app.string.vesper_language_de'));
      case 'es': return this.text($r('app.string.vesper_language_es'));
      case 'pt': return this.text($r('app.string.vesper_language_pt'));
      case 'ru': return this.text($r('app.string.vesper_language_ru'));
      case 'it': return this.text($r('app.string.vesper_language_it'));
      case 'ar': return this.text($r('app.string.vesper_language_ar'));
      default: return language;
    }
  }
  private trackTitle(track: VesperTrack): string {
    if (track.kind === 'video' && (track.height ?? 0) > 0) { return track.height + 'p'; }
    if (track.external && track.label) { return track.label; }
    if (track.language.length > 0 && track.language !== 'und') { return this.language(track.language); }
    const index = this.binding.snapshot.tracks.filter((value: VesperTrack): boolean => value.kind === track.kind).findIndex((value: VesperTrack): boolean => value.id === track.id) + 1;
    return this.text(track.kind === 'audio' ? $r('app.string.vesper_audio_track') : track.kind === 'subtitle' ? $r('app.string.vesper_subtitle_track') : $r('app.string.vesper_video_track'), index);
  }
  private summary(kind: string): string {
    const tracks = this.binding.snapshot.tracks.filter((track: VesperTrack): boolean => track.kind === kind && track.selected);
    return tracks.map((track: VesperTrack): string => this.trackTitle(track)).join(', ') ||
      this.text(kind === 'subtitle' ? $r('app.string.vesper_off') : $r('app.string.vesper_system_default'));
  }
  private selected(mode: string, bitrate?: number): boolean {
    if (this.pane === 'video' && mode === 'auto') { return stageQuality(this.binding.snapshot).mode === 'auto'; }
    const state = this.pane === 'video' ? this.binding.snapshot.trackSelections?.video ?? this.binding.snapshot.selection : this.binding.snapshot.selection;
    const effective = state.effective;
    return effective?.kind === this.pane && effective.mode === mode && (bitrate === undefined || effective.bitrate === bitrate);
  }
  private selectionSummary(): string {
    const state = this.pane === 'video' ? this.binding.snapshot.trackSelections?.video ?? this.binding.snapshot.selection :
      this.pane === 'audio' ? this.binding.snapshot.trackSelections?.audio : this.binding.snapshot.trackSelections?.subtitle;
    if (state?.requested?.kind !== this.pane) { return ''; }
    if (state.outcome === 'failed') { return this.text($r('app.string.vesper_selection_failed')); }
    if (this.pane === 'audio' && state.outcome === 'pending' && this.binding.snapshot.state === 'paused') { return this.text($r('app.string.vesper_selection_on_resume')); }
    return this.text(state.outcome !== 'confirmed' || this.pane === 'video' && stageQuality(this.binding.snapshot).status === 'switching' ?
      $r('app.string.vesper_switching') : $r('app.string.vesper_selected'));
  }
  @Builder row($$: SettingRow) {
    Button({ type: ButtonType.Normal }) {
      Row({ space: 12 }) {
        Column({ space: 4 }) {
          Text($$.title).fontSize(16).fontColor(this.foreground()).width('100%')
          if ($$.detail) { Text($$.detail).fontSize(13).fontColor(this.secondary()).width('100%') }
        }.layoutWeight(1).alignItems(HorizontalAlign.Start)
        if ($$.selected) { SymbolGlyph($r('sys.symbol.checkmark')).fontSize(20).fontColor([this.accent()]).accessibilityLevel('no') }
      }.width('100%').padding({ left: 16, right: 16, top: 14, bottom: 14 })
    }.width('100%').constraintSize({ minHeight: 56 }).borderRadius(12).backgroundColor(Color.Transparent)
      .enabled($$.enabled ?? true).focusable(true).accessibilityText($$.title + ($$.detail ? ', ' + $$.detail : ''))
      .accessibilitySelected($$.selected ?? false).onClick((): void => { $$.action(); })
      .onAreaChange((before: Area, after: Area): void => {
        if ($$.selected && this.scrollSelection) { this.scrollSelection = false; this.scroller.scrollTo({ xOffset: 0, yOffset: Math.max(0, Number(after.position.y) - 16) }); }
      })
  }
  @Builder note(text: string) {
    Text(text).fontSize(13).lineHeight(19).fontColor(this.secondary()).width('100%').padding(16)
  }
  @Builder selectionNote() {
    Text(this.selectionSummary()).fontSize(13).lineHeight(19).fontColor(this.secondary()).width('100%').padding(16)
  }
  private subtitleCatalogNote(): string {
    const state = this.binding.snapshot.subtitleState?.catalogState;
    return state === 'loading' ? this.text($r('app.string.vesper_subtitle_loading')) :
      state === 'partial' ? this.text($r('app.string.vesper_subtitle_partial')) :
      state === 'failed' ? this.text($r('app.string.vesper_subtitle_failed')) :
      state === 'empty' ? this.text($r('app.string.vesper_no_tracks')) : '';
  }
  @Builder subtitleCatalogStatus() {
    Text(this.subtitleCatalogNote()).fontSize(13).lineHeight(19).fontColor(this.secondary()).width('100%').padding(16)
  }
  private trackDetail(track: VesperTrack): string {
    const details: string[] = [];
    if (track.isDefault) { details.push(this.text($r('app.string.vesper_subtitle_default'))); }
    if (track.isForced) { details.push(this.text($r('app.string.vesper_subtitle_forced'))); }
    details.push(track.selectionSupported === false ? this.text($r('app.string.vesper_unavailable_track')) : stageTrackDetail(this.binding.snapshot, track));
    return details.filter((value: string): boolean => value.length > 0).join(' · ');
  }
  @Builder menu() {
    this.row({ title: this.text($r('app.string.vesper_speed')), detail: this.binding.snapshot.playbackRate + '×', action: (): void => { this.navigate('speed'); } })
    this.row({ title: this.text($r('app.string.vesper_quality')), detail: stageQuality(this.binding.snapshot).status === 'switching' ?
      this.text($r('app.string.vesper_switching')) : (this.binding.snapshot.height > 0 ? this.binding.snapshot.height + 'p' : this.text($r('app.string.vesper_system_default'))),
      action: (): void => { this.navigate('video'); } })
    this.row({ title: this.text($r('app.string.vesper_audio')), detail: this.summary('audio'), action: (): void => { this.navigate('audio'); } })
    this.row({ title: this.text($r('app.string.vesper_subtitles')), detail: this.summary('subtitle'), action: (): void => { this.navigate('subtitle'); } })
    Row() {
      Text($r('app.string.vesper_mute')).fontSize(16).fontColor(this.foreground()).layoutWeight(1)
      Toggle({ type: ToggleType.Switch, isOn: this.binding.snapshot.muted }).accessibilityText(this.text($r('app.string.vesper_mute')))
        .onChange((value: boolean): void => { this.binding.report(this.binding.controller.setMuted(value)); })
    }.width('100%').constraintSize({ minHeight: 56 }).padding({ left: 16, right: 16 })
    Row({ space: 12 }) {
      SymbolGlyph(this.skin.icons.volume).fontSize(22).fontColor([this.secondary()]).accessibilityLevel('no')
      Slider({ value: this.binding.snapshot.volume, min: 0, max: 1, step: 0.05 }).layoutWeight(1).height(48)
        .selectedColor(this.accent()).accessibilityText(this.text($r('app.string.vesper_volume')))
        .onChange((value: number, mode: SliderChangeMode): void => {
          if (mode === SliderChangeMode.End || mode === SliderChangeMode.Click) { this.binding.report(this.binding.controller.setVolume(value)); }
        })
    }.constraintSize({ minHeight: 56 }).padding({ left: 16, right: 16 }).width('100%')
    if (this.environment?.brightness !== undefined) {
      Row({ space: 12 }) {
        SymbolGlyph(this.skin.icons.brightness).fontSize(22).fontColor([this.secondary()]).accessibilityLevel('no')
        Slider({ value: this.environment?.brightness?.current() ?? 0.5, min: 0, max: 1, step: 0.05 }).layoutWeight(1).height(48)
          .selectedColor(this.accent()).accessibilityText(this.text($r('app.string.vesper_brightness')))
          .onChange((value: number, mode: SliderChangeMode): void => {
            const brightness = this.environment?.brightness;
            if (brightness !== undefined && (mode === SliderChangeMode.End || mode === SliderChangeMode.Click)) { this.binding.report(brightness.set(value)); }
          })
      }.constraintSize({ minHeight: 56 }).padding({ left: 16, right: 16 }).width('100%')
    }
    this.row({ title: this.text($r('app.string.vesper_pip')), enabled: this.pipAvailable,
      detail: this.pipAvailable ? undefined : this.text($r('app.string.vesper_pip_unavailable')), action: (): void => { this.onClose(); this.onPiP(); } })
    this.row({ title: this.text($r('app.string.vesper_lock')), action: (): void => { this.onLock(); } })
  }
  @Builder tracks() {
    if (this.pane === 'subtitle') {
      this.row({ title: this.text($r('app.string.vesper_off')), selected: !this.binding.snapshot.tracks.some((track: VesperTrack): boolean => track.kind === 'subtitle' && track.selected),
        action: (): void => { this.binding.report(this.binding.controller.selectTrack({ kind: 'subtitle', mode: 'off' })); } })
      this.row({ title: this.text($r('app.string.vesper_captions_style')), action: (): void => { this.navigate('captionStyle'); } })
    }
    if (this.pane === 'video') {
      this.row({ title: this.text($r('app.string.vesper_auto')), detail: this.text(this.binding.snapshot.availableBitrates.length > 0 ? $r('app.string.vesper_adapt') : $r('app.string.vesper_no_auto')),
        selected: this.selected('auto'), enabled: this.binding.snapshot.availableBitrates.length > 0,
        action: (): void => { this.binding.report(this.binding.controller.selectTrack({ kind: 'video', mode: 'auto' })); } })
    }
    ForEach(stageTracks(this.binding.snapshot, this.pane), (track: VesperTrack) => {
      this.row({ title: this.trackTitle(track), detail: this.trackDetail(track),
        selected: stageTrackSelected(this.binding.snapshot, track), enabled: track.selectionSupported !== false,
        action: (): void => { this.binding.report(this.binding.controller.selectTrack({ kind: track.kind, mode: 'track', trackId: track.id })); } })
    }, (track: VesperTrack): string => this.binding.snapshot.playbackEpoch + ':' + track.id + ':' + stageTrackSelected(this.binding.snapshot, track) + ':' + track.selectionSupported + ':' + track.bitrate)
    if (this.pane === 'video' && (!this.binding.snapshot.tracks.some((track: VesperTrack): boolean => track.kind === 'video' && track.selectionSupported !== false) ||
      stageQuality(this.binding.snapshot).mode === 'bitrate')) {
      ForEach(this.binding.snapshot.availableBitrates.slice().sort((left: number, right: number): number => right - left), (bitrate: number) => {
        this.row({ title: Math.round(bitrate / 1000) + ' kbps', selected: this.selected('bitrate', bitrate),
          action: (): void => { this.binding.report(this.binding.controller.selectTrack({ kind: 'video', mode: 'bitrate', bitrate: bitrate })); } })
      })
    }
    if (this.pane === 'subtitle' && this.subtitleCatalogNote()) { this.subtitleCatalogStatus() }
    else if (this.pane !== 'subtitle' && !this.binding.snapshot.tracks.some((track: VesperTrack): boolean => track.kind === this.pane)) { this.note(this.text($r('app.string.vesper_no_tracks'))) }
    if (this.selectionSummary()) { this.selectionNote() }
  }
  @Builder captionStylePanel() {
    this.note(this.text($r('app.string.vesper_caption_size')))
    ForEach([0.8, 1, 1.25, 1.5, 2], (scale: number) => {
      this.row({ title: Math.round(scale * 100) + '%', selected: this.subtitleStyle.fontScale === scale, action: (): void => {
        this.changeCaptionStyle({ fontScale: scale });
      } })
    })
    this.note(this.text($r('app.string.vesper_caption_foreground')))
    this.row({ title: this.text($r('app.string.vesper_caption_white')), selected: this.subtitleStyle.foreground === Color.White,
      action: (): void => { this.changeCaptionStyle({ foreground: Color.White }); } })
    this.row({ title: this.text($r('app.string.vesper_caption_yellow')), selected: this.subtitleStyle.foreground === '#FFFF00',
      action: (): void => { this.changeCaptionStyle({ foreground: '#FFFF00' }); } })
    Row() {
      Text($r('app.string.vesper_caption_background')).fontSize(16).fontColor(this.foreground()).layoutWeight(1)
      Toggle({ type: ToggleType.Switch, isOn: this.subtitleStyle.background !== Color.Transparent })
        .accessibilityText(this.text($r('app.string.vesper_caption_background')))
        .onChange((value: boolean): void => { this.changeCaptionStyle({ background: value ? '#A0000000' : Color.Transparent }); })
    }.width('100%').constraintSize({ minHeight: 56 }).padding(16)
    Row() {
      Text($r('app.string.vesper_caption_edge')).fontSize(16).fontColor(this.foreground()).layoutWeight(1)
      Toggle({ type: ToggleType.Switch, isOn: this.subtitleStyle.edgeRadius > 0 })
        .accessibilityText(this.text($r('app.string.vesper_caption_edge')))
        .onChange((value: boolean): void => { this.changeCaptionStyle({ edgeRadius: value ? 2 : 0 }); })
    }.width('100%').constraintSize({ minHeight: 56 }).padding(16)
  }
  private changeCaptionStyle(change: VesperSubtitleStyleOptions): void {
    const current = this.subtitleStyle;
    this.onSubtitleStyleChange?.(new VesperSubtitleStyle({ fontScale: change.fontScale ?? current.fontScale,
      foreground: change.foreground ?? current.foreground, background: change.background ?? current.background,
      edgeColor: change.edgeColor ?? current.edgeColor, edgeRadius: change.edgeRadius ?? current.edgeRadius,
      bottomInset: current.bottomInset, maxLines: current.maxLines }));
  }
  build() {
    Column() {
      Row() {
        if (this.pane !== 'menu') {
          Button({ type: ButtonType.Normal }) { SymbolGlyph(this.skin.icons.navigateBack).fontSize(22).fontColor([this.foreground()]) }
            .width(48).height(48).backgroundColor(Color.Transparent).accessibilityText(this.text($r('app.string.vesper_back')))
            .onClick((): void => { this.navigate(this.pane === 'captionStyle' ? 'subtitle' : 'menu'); })
        }
        Text(this.title()).fontSize(20).fontWeight(FontWeight.Medium).fontColor(this.foreground()).layoutWeight(1).maxLines(2)
      }.width('100%').padding({ left: 16, right: 48, bottom: 8 }).constraintSize({ minHeight: 56 })
      Scroll(this.scroller) {
        Column() {
          if (this.binding.feedback !== undefined && this.binding.feedback.category !== 'cancelled') {
            Text(feedbackResource(this.binding.feedback)).fontSize(13).fontColor(this.foreground()).width('100%').padding(16)
          }
          if (this.pane === 'menu') { this.menu() }
          else if (this.pane === 'speed') {
            ForEach(this.rates, (rate: number) => {
              this.row({ title: rate === 1 ? this.text($r('app.string.vesper_normal')) : rate + '×', selected: this.binding.snapshot.playbackRate === rate,
                enabled: this.binding.controller.playbackView().rate.status === 'supported',
                action: (): void => { this.binding.report(this.binding.controller.setPlaybackRate(rate)); } })
            })
            if (this.binding.snapshot.timeline !== 'vod') { this.note(this.text($r('app.string.vesper_speed_vod'))) }
          } else if (this.pane === 'captionStyle') { this.captionStylePanel() }
          else { this.tracks() }
          if (this.binding.feedback !== undefined && this.binding.feedback.category !== 'cancelled') {
            this.row({ title: this.text($r('app.string.vesper_details')), action: (): void => { this.details = !this.details; } })
            if (this.details) { this.note(this.binding.feedback.code + (this.binding.feedback.nativeCode === undefined ? '' : ' · ' + this.binding.feedback.nativeCode)) }
          }
        }.width('100%').padding({ left: 8, right: 8, bottom: 24 })
      }.width('100%').layoutWeight(1).scrollBar(BarState.Auto).align(Alignment.TopStart)
    }.width('100%').height('100%')
  }
}
