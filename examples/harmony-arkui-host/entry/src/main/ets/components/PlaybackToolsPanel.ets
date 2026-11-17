import { ExampleModel } from '../model/ExampleModel';
import { ExamplePalette, examplePalette, formatBytes } from '../model/ExampleSupport';
import { ExampleAction, ExampleHeading, ExampleValue } from './ExampleControls';

@Component
export struct PlaybackToolsPanel {
  @ObjectLink model: ExampleModel;
  @Prop palette: ExamplePalette = examplePalette(false);
  private available(): boolean { return this.model.ready && !this.model.busy; }
  private text(key: string): string { return this.model.text('example_' + key); }
  private recoveryText(): string {
    const state = this.model.recoveryState;
    return this.text('recovery_' + (['off', 'watching', 'waiting', 'activating', 'awaitingFrame', 'recovered', 'exhausted', 'hostActionRequired', 'sourceRefreshRequired', 'cancelled', 'closed'].includes(state) ? state : 'unknown'));
  }
  private language(): string { return this.text(this.model.preferenceLanguage === 'en' ? 'language_english' : this.model.preferenceLanguage === 'zh' ? 'language_chinese' : 'system'); }
  private preference(): string { return this.text(this.model.subtitlePreference === 'off' ? 'subtitle_off' : this.model.subtitlePreference === 'preferred' ? 'subtitle_preferred' : 'system'); }
  build() {
    Column({ space: 20 }) {
      Column({ space: 12 }) {
        ExampleHeading({ title: this.text('recovery'), detail: this.text('recovery_detail'), palette: this.palette })
        Row({ space: 12 }) {
          Text(this.recoveryText()).fontSize(15).fontColor(this.palette.title).layoutWeight(1)
          Toggle({ type: ToggleType.Switch, isOn: this.model.recoveryEnabled }).enabled(this.available())
            .accessibilityText(this.text('recovery')).onChange((value: boolean): void => { this.model.setRecoveryEnabled(value); })
        }.width('100%').constraintSize({ minHeight: 56 })
        Text(this.text('recovery_note')).fontSize(13).fontColor(this.palette.secondary).width('100%')
      }.padding(16).width('100%').backgroundColor(this.palette.surface).borderRadius(18)
      Column({ space: 12 }) {
        ExampleHeading({ title: this.text('track_preferences'), detail: this.text('track_preferences_detail'), palette: this.palette })
        Text(this.text('audio_language')).fontSize(14).fontColor(this.palette.title).width('100%')
        Select([{ value: this.text('system') }, { value: this.text('language_english') }, { value: this.text('language_chinese') }])
          .selected(['system', 'en', 'zh'].indexOf(this.model.preferenceLanguage)).value(this.language()).enabled(this.available())
          .accessibilityText(this.text('audio_language')).onSelect((index: number): void => { this.model.setTrackPreferences(['system', 'en', 'zh'][index], this.model.subtitlePreference); })
        Text(this.text('subtitle_preference')).fontSize(14).fontColor(this.palette.title).width('100%')
        Select([{ value: this.text('system') }, { value: this.text('subtitle_off') }, { value: this.text('subtitle_preferred') }])
          .selected(['system', 'off', 'preferred'].indexOf(this.model.subtitlePreference)).value(this.preference()).enabled(this.available())
          .accessibilityText(this.text('subtitle_preference')).onSelect((index: number): void => { this.model.setTrackPreferences(this.model.preferenceLanguage, ['system', 'off', 'preferred'][index]); })
        if (this.model.snapshot.trackPreferences !== undefined) {
          Text(this.text('preference_' + (this.model.snapshot.trackPreferences?.audio.status ?? 'unchanged'))).fontSize(13).fontColor(this.palette.secondary).width('100%')
          Text(this.text('preference_' + (this.model.snapshot.trackPreferences?.subtitle.status ?? 'unchanged'))).fontSize(13).fontColor(this.palette.secondary).width('100%')
        }
      }.padding(16).width('100%').backgroundColor(this.palette.surface).borderRadius(18)
      Column({ space: 12 }) {
        ExampleHeading({ title: this.text('policy'), detail: this.text('policy_detail'), palette: this.palette })
        Select([{ value: this.text('policy_system') }, { value: this.text('policy_responsive') }, { value: this.text('policy_resilient') }])
          .selected(['system', 'responsive', 'resilient'].indexOf(this.model.policyProfile)).value(this.text('policy_' + this.model.policyProfile))
          .enabled(this.available()).accessibilityText(this.text('policy'))
          .onSelect((index: number): void => { this.model.applyPolicy(['system', 'responsive', 'resilient'][index]); })
        Text(this.text('policy_note')).fontSize(13).fontColor(this.palette.secondary).width('100%')
      }.padding(16).width('100%').backgroundColor(this.palette.surface).borderRadius(18)
      Column({ space: 12 }) {
        ExampleHeading({ title: this.text('preload'), detail: this.text('preload_detail'), palette: this.palette })
        ExampleValue({ label: this.text('received_reuse'), value: formatBytes(this.model.preloadBytes), palette: this.palette })
        Flex({ wrap: FlexWrap.Wrap }) {
          ExampleAction({ title: this.text('preload_next'), icon: $r('sys.symbol.download'), palette: this.palette,
            isEnabled: this.available() && !['queued', 'running'].includes(this.model.preloadStatus), onAction: (): void => { this.model.preloadNext(); } }).margin({ right: 8, bottom: 8 })
          ExampleAction({ title: this.text('cancel_preload'), icon: $r('sys.symbol.xmark'), palette: this.palette,
            isEnabled: ['queued', 'running'].includes(this.model.preloadStatus), onAction: (): void => { this.model.cancelPreload(); } })
        }.width('100%')
        Text(this.text('preload_note')).fontSize(13).fontColor(this.palette.secondary).width('100%')
      }.padding(16).width('100%').backgroundColor(this.palette.surface).borderRadius(18)
      Column({ space: 12 }) {
        ExampleHeading({ title: this.text('cache'), detail: this.text('cache_detail'), palette: this.palette })
        ExampleValue({ label: this.text('disk_entries'), value: `${this.model.cacheEntries} · ${formatBytes(this.model.cacheBytes)}`, palette: this.palette })
        ExampleValue({ label: this.text('disk_reuse'), value: formatBytes(this.model.cacheHits), palette: this.palette })
        ExampleAction({ title: this.text('cache_clear'), icon: $r('sys.symbol.trash'), palette: this.palette,
          isEnabled: this.available() && !['queued', 'running'].includes(this.model.preloadStatus), onAction: (): void => { this.model.clearCache(); } })
      }.padding(16).width('100%').backgroundColor(this.palette.surface).borderRadius(18)
    }.width('100%').alignItems(HorizontalAlign.Start)
  }
}
