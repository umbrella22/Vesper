import { ExampleModel } from '../model/ExampleModel';
import { ExampleLogEntry, examplePalette, ExamplePalette, formatBytes, formatTime } from '../model/ExampleSupport';
import { ExampleAction, ExampleIconAction, ExampleHeading, ExampleValue } from './ExampleControls';
import { PlaybackToolsPanel } from './PlaybackToolsPanel';

@Component
export struct DiagnosticsPanel {
  @ObjectLink model: ExampleModel;
  @Prop palette: ExamplePalette = examplePalette(false);
  private text(key: string): string { return this.model.text('example_diag_' + key); }
  build() {
    Column({ space: 24 }) {
      Row({ space: 12 }) {
        ExampleHeading({ title: this.text('player_status'), detail: this.text('player_detail'), palette: this.palette }).layoutWeight(1)
        ExampleIconAction({ title: this.text('copy_report'), icon: $r('sys.symbol.doc_text_on_doc_text'), palette: this.palette, isEnabled: this.model.ready && !this.model.busy,
          onAction: (): void => { this.model.copyDiagnostics(); } })
      }.width('100%')
      Column({ space: 12 }) {
        ExampleHeading({ title: this.text('feedback'), detail: this.text('feedback_detail'), palette: this.palette })
        Select([{ value: this.text('scenario_progress') }, { value: this.text('scenario_audio') },
          { value: this.text('scenario_tail') }, { value: this.text('scenario_dvr') }])
          .selected(['progress', 'alternate-audio', 'tail-completion', 'live-dvr'].indexOf(this.model.diagnosticScenario))
          .value(this.text('scenario_' + ['progress', 'audio', 'tail', 'dvr'][['progress', 'alternate-audio', 'tail-completion', 'live-dvr'].indexOf(this.model.diagnosticScenario)]))
          .enabled(!this.model.busy).accessibilityText(this.text('scenario'))
          .onSelect((index: number): void => { this.model.diagnosticScenario = ['progress', 'alternate-audio', 'tail-completion', 'live-dvr'][index]; this.model.diagnosticResult = undefined; })
        Text(this.text('environment')).fontSize(14).fontColor(this.palette.title)
        Select([{ value: this.text('unknown') }, { value: this.text('device') }, { value: this.text('simulator') }])
          .selected(['unknown', 'device', 'simulator'].indexOf(this.model.diagnosticRuntimeKind)).value(this.text(this.model.diagnosticRuntimeKind))
          .onSelect((index: number): void => { this.model.diagnosticRuntimeKind = ['unknown', 'device', 'simulator'][index]; })
        Text(this.text('audio_observation')).fontSize(14).fontColor(this.palette.title)
        Select([{ value: this.text('not_assessed') }, { value: this.text('audible') }, { value: this.text('silent') }, { value: this.text('intermittent') }])
          .selected(['not-assessed', 'audible', 'silent', 'intermittent'].indexOf(this.model.diagnosticAudioObservation)).value(this.text(this.model.diagnosticAudioObservation.replace('-', '_')))
          .onSelect((index: number): void => { this.model.diagnosticAudioObservation = ['not-assessed', 'audible', 'silent', 'intermittent'][index]; })
        Flex({ wrap: FlexWrap.Wrap }) {
          ExampleAction({ title: this.text('run_scenario'), icon: $r('sys.symbol.play_fill'), palette: this.palette,
            isEnabled: this.model.ready && !this.model.busy, onAction: (): void => { this.model.runDiagnostics(); } }).margin({ right: 8, bottom: 8 })
          ExampleAction({ title: this.text('save_report'), icon: $r('sys.symbol.download'), palette: this.palette,
            isEnabled: this.model.ready && !this.model.busy, onAction: (): void => { this.model.saveDiagnostics(); } })
        }.width('100%')
        if (this.model.diagnosticResult !== undefined) {
          Text(`${this.model.diagnosticResult?.outcome} ${this.model.diagnosticResult?.errorCode ?? ''}`).fontSize(13).fontColor(this.palette.secondary).width('100%')
        }
      }.padding(16).width('100%').backgroundColor(this.palette.surface).borderRadius(24).alignItems(HorizontalAlign.Start)
      Column({ space: 4 }) {
        Text(this.model.snapshot.state).fontSize(32).fontWeight(FontWeight.Bold).fontColor(this.palette.title).width('100%')
        ExampleValue({ label: this.text('position_duration'), value: `${formatTime(this.model.snapshot.positionMs)} / ${formatTime(this.model.snapshot.durationMs)}`, palette: this.palette })
        ExampleValue({ label: this.text('timeline'), value: this.model.snapshot.timeline, palette: this.palette })
        ExampleValue({ label: this.text('video_dimensions'), value: `${this.model.snapshot.width} × ${this.model.snapshot.height}`, palette: this.palette })
        ExampleValue({ label: this.text('first_frame'), value: this.model.snapshot.firstFrame ? `Rendered · ${this.model.snapshot.firstFrameMs ?? '?'} ms` : 'Waiting', palette: this.palette })
        ExampleValue({ label: this.text('playback_rate'), value: `${this.model.snapshot.playbackRate}×`, palette: this.palette })
        ExampleValue({ label: this.text('volume'), value: this.model.snapshot.muted ? 'Muted' : `${Math.round(this.model.snapshot.volume * 100)}%`, palette: this.palette })
        ExampleValue({ label: this.text('playback_epoch'), value: this.model.snapshot.playbackEpoch ?? 'None', palette: this.palette })
        ExampleValue({ label: this.text('surface'), value: this.model.binding.surfaceId() || 'Not attached', palette: this.palette })
        ExampleValue({ label: this.text('cache_loader'), value: `${formatBytes(this.model.snapshot.cacheBytes)} / ${formatBytes(this.model.snapshot.loaderBytes)}`, palette: this.palette })
        ExampleValue({ label: this.text('active_resources'), value: `${this.model.snapshot.activeResources}`, palette: this.palette })
        ExampleValue({ label: this.text('retained_players'), value: `${this.model.snapshot.retainedNativePlayers}`, palette: this.palette })
        ExampleValue({ label: this.text('dropped_events'), value: `${this.model.snapshot.droppedEvents}`, palette: this.palette })
        if (this.model.snapshot.error !== undefined) { ExampleValue({ label: this.text('error_code'), value: this.model.snapshot.error?.code ?? '', palette: this.palette }) }
      }.padding(16).width('100%').backgroundColor(this.palette.surface).borderRadius(24)
      Column({ space: 4 }) {
        ExampleHeading({ title: this.text('selection_presentation'), palette: this.palette })
        ExampleValue({ label: this.text('requested_selection'), value: JSON.stringify(this.model.snapshot.selection.requested ?? {}), palette: this.palette })
        ExampleValue({ label: this.text('effective_selection'), value: JSON.stringify(this.model.snapshot.selection.effective ?? {}), palette: this.palette })
        ExampleValue({ label: this.text('selection_outcome'), value: `${this.model.snapshot.selection.outcome ?? 'none'} ${this.model.snapshot.selection.errorCode ?? ''}`, palette: this.palette })
        ExampleValue({ label: this.text('confirmed_tracks'), value: this.model.snapshot.selection.confirmedTrackIds.join(', ') || 'None', palette: this.palette })
        ExampleValue({ label: this.text('per_kind_selection'), value: JSON.stringify(this.model.snapshot.trackSelections ?? {}), palette: this.palette })
        ExampleValue({ label: this.text('track_preferences'), value: JSON.stringify(this.model.snapshot.trackPreferences ?? {}), palette: this.palette })
        ExampleValue({ label: this.text('track_filter'), value: JSON.stringify(this.model.snapshot.trackConstraints ?? {}), palette: this.palette })
        ExampleValue({ label: this.text('time_ranges'), value: JSON.stringify(this.model.snapshot.timeRanges ?? {}), palette: this.palette })
        ExampleValue({ label: this.text('policy_parameters'), value: JSON.stringify(this.model.snapshot.policy ?? {}), palette: this.palette })
        ExampleValue({ label: this.text('native_buffering'), value: JSON.stringify(this.model.snapshot.nativeBuffering ?? {}), palette: this.palette })
        ExampleValue({ label: this.text('native_observations'), value: JSON.stringify(this.model.snapshot.diagnostics ?? {}), palette: this.palette })
        ExampleValue({ label: this.text('window_policy'), value: JSON.stringify(this.model.binding.windowFeedback ?? {}), palette: this.palette })
        ExampleValue({ label: this.text('system_session'), value: this.model.systemState, palette: this.palette })
        ExampleValue({ label: this.text('pip'), value: this.model.pipState, palette: this.palette })
      }.padding(16).width('100%').backgroundColor(this.palette.surface).borderRadius(24)
      Column({ space: 4 }) {
        ExampleHeading({ title: this.text('platform'), palette: this.palette })
        ExampleValue({ label: this.text('runtime_api'), value: `${this.model.binding.controller.capabilities().runtimeApi}`, palette: this.palette })
        ExampleValue({ label: this.text('native_player'), value: 'AVPlayer', palette: this.palette })
        ExampleValue({ label: this.text('track_constraints'), value: 'Available', palette: this.palette })
        ExampleValue({ label: this.text('decoder_guarantee'), value: this.model.binding.controller.capabilities().decoderGuarantee, palette: this.palette })
        ExampleValue({ label: this.text('decoder_preflight'), value: JSON.stringify(this.model.decoderCapability ?? {}), palette: this.palette })
        ExampleValue({ label: this.text('hdr_evidence'), value: this.model.binding.controller.capabilities().hdrEvidence, palette: this.palette })
        ExampleValue({ label: this.text('audio_evidence'), value: this.model.snapshot.audioOutputEvidence, palette: this.palette })
      }.padding(16).width('100%').backgroundColor(this.palette.surface).borderRadius(24)
      Column({ space: 12 }) {
        Row() {
          ExampleHeading({ title: this.text('event_history'), detail: this.text('history_detail'), palette: this.palette }).layoutWeight(1)
          ExampleIconAction({ title: this.text('clear_history'), icon: $r('sys.symbol.trash'), palette: this.palette, onAction: (): void => { this.model.clearLogs(); } })
        }.width('100%')
        ForEach(this.model.logs, (entry: ExampleLogEntry) => {
          Row({ space: 12 }) {
            Text(entry.time).fontFamily('monospace').fontSize(11).fontColor(this.palette.secondary)
            Column({ space: 3 }) {
              Text(entry.title).fontSize(14).fontWeight(FontWeight.Medium).fontColor(entry.failed ? this.palette.danger : this.palette.title).width('100%')
              Text(entry.detail).fontSize(12).fontColor(this.palette.secondary).width('100%')
            }.layoutWeight(1).alignItems(HorizontalAlign.Start)
          }.width('100%').padding({ top: 8, bottom: 8 })
        }, (entry: ExampleLogEntry) => entry.id.toString())
      }.width('100%')
      PlaybackToolsPanel({ model: this.model, palette: this.palette })
    }.padding(20).constraintSize({ maxWidth: 860 }).width('100%').alignItems(HorizontalAlign.Start)
  }
}
