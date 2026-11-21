import { runKitChecks } from '../KitChecks';
import { runRtspChecks } from '../RtspChecks';
import { runTrackSystemChecks } from '../TrackSystemChecks';
import { runParityChecks } from '../ParityChecks';
import { runStreamObservation } from '../StreamObservation';
import { runSequenceCacheChecks, runProcessDownloads, runOfflineDashAudioChecks } from '../IntegrationChecks';
import { runLiveChecks } from '../LiveChecks';
import { runStressChecks } from '../StressChecks';
import { runReferenceChecks } from '../ReferenceChecks';
import { runInterruptedDownload } from '../RestartChecks';
import { runExportChecks } from '../ExportChecks';
import { runWorkerCleanupChecks } from '../WorkerChecks';
import { runProfileChecks, runRecoveryChecks, runRecoveryIntentChecks } from '../GapChecks';
import { common } from '@kit.AbilityKit';
import { runCapabilityChecks, runNativeDashAudioChecks } from '../CapabilityChecks';
import { runDynamicDashChecks } from '../DynamicDashChecks';
import { runIndependentHlsChecks } from '../IndependentHlsChecks';
import { runNativeDvrChecks } from '../DvrChecks';
import { runLocalAppleEofChecks } from '../EofChecks';

@Entry
@Component
struct Index {
  @State result: string = 'Ready';
  @State running: boolean = false;
  private surface: XComponentController = new XComponentController();
  private surfaceId: string = '';
  private async run(mode: string = 'kit'): Promise<void> {
    if (this.running) { return; }
    this.running = true; this.result = 'Running candidate HAR checks';
    try {
      const context = this.getUIContext().getHostContext();
      if (context === undefined) { throw new Error('Ability context is unavailable'); }
      if (mode === 'kit') { this.result = await runKitChecks(context, this.surfaceId); }
      else {
        const result = mode === 'independent-native' || mode === 'independent-sdk' ? await runIndependentHlsChecks(this.surfaceId, mode === 'independent-sdk')
          : mode === 'rtsp' || mode === 'rtsp-off' ? await runRtspChecks(context as common.UIAbilityContext, this.surfaceId, mode === 'rtsp')
          : mode === 'dvr' ? await runNativeDvrChecks(this.surfaceId)
          : mode === 'local-apple-eof' ? await runLocalAppleEofChecks(this.surfaceId)
          : mode === 'dynamic-dash' || mode === 'dynamic-baseline' ? await runDynamicDashChecks(this.surfaceId, mode !== 'dynamic-baseline')
          : mode === 'capabilities' ? await runCapabilityChecks(this.surfaceId)
          : mode === 'native-audio' ? await runNativeDashAudioChecks(this.surfaceId)
          : mode === 'offline-audio' ? await runOfflineDashAudioChecks(context, this.surfaceId)
          : mode === 'parity' ? await runParityChecks(context as common.UIAbilityContext, this.surfaceId)
          : mode === 'stream-observation' ? await runStreamObservation(context as common.UIAbilityContext, this.surfaceId)
          : mode === 'recovery-intent' ? await runRecoveryIntentChecks(this.surfaceId)
          : mode === 'profiles' ? await runProfileChecks(this.surfaceId)
          : mode === 'recovery' ? await runRecoveryChecks(this.surfaceId)
          : mode === 'worker' ? await runWorkerCleanupChecks()
          : mode === 'tracks' ? await runTrackSystemChecks(context as common.UIAbilityContext, this.surfaceId)
          : mode === 'sequence' ? await runSequenceCacheChecks(context, this.surfaceId) : mode === 'live' ? await runLiveChecks(this.surfaceId)
          : mode === 'stress' ? await runStressChecks(context, this.surfaceId)
          : mode === 'reference' ? await runReferenceChecks(this.surfaceId)
          : mode === 'interrupt' || mode === 'resume' ? await runInterruptedDownload(context, mode === 'interrupt')
          : mode === 'export' ? await runExportChecks(context as common.UIAbilityContext)
          : await runProcessDownloads(context, this.surfaceId, mode === 'prepare');
        this.result = JSON.stringify({ status: 'PASS', results: result });
      }
    } catch (error) { this.result = JSON.stringify({ status: 'FAIL', error: `${error}` }); }
    finally { this.running = false; }
    console.info(`VESPER_HARMONY_KIT ${this.result}`);
  }
  @Builder
  private scenarioButton(label: string, mode: string) {
    Button(label).width('49%').height(28).margin({ bottom: 3 }).padding({ left: 4, right: 4 })
      .fontSize(11).enabled(!this.running).onClick(() => { this.run(mode); })
  }
  build() {
    Column({ space: 6 }) {
      Text('Vesper candidate HAR consumer').fontSize(20)
      XComponent({ id: 'kit-video', type: XComponentType.SURFACE, controller: this.surface })
        .width('100%').height(120).onLoad(() => { this.surfaceId = this.surface.getXComponentSurfaceId(); })
      Flex({ wrap: FlexWrap.Wrap, justifyContent: FlexAlign.SpaceBetween }) {
        this.scenarioButton('Run kit checks', 'kit')
        this.scenarioButton('Check RTSP playback', 'rtsp')
        this.scenarioButton('Check RTSP feature off', 'rtsp-off')
        this.scenarioButton('Observe corrected DVR', 'dvr')
        this.scenarioButton('Observe local Apple EOF', 'local-apple-eof')
        this.scenarioButton('Observe independent HLS native', 'independent-native')
        this.scenarioButton('Observe independent HLS SDK', 'independent-sdk')
        this.scenarioButton('Run capability checks', 'capabilities')
        this.scenarioButton('Observe native DASH audio', 'native-audio')
        this.scenarioButton('Run dynamic DASH checks', 'dynamic-dash')
        this.scenarioButton('Run DASH profile checks', 'profiles')
        this.scenarioButton('Prepare process downloads', 'prepare')
        this.scenarioButton('Restore offline downloads', 'restore')
        this.scenarioButton('Check offline DASH audio', 'offline-audio')
        this.scenarioButton('Run dynamic DASH baseline', 'dynamic-baseline')
        this.scenarioButton('Run parity checks', 'parity')
        this.scenarioButton('Run recovery intent checks', 'recovery-intent')
        this.scenarioButton('Observe public HLS', 'stream-observation')
        this.scenarioButton('Run worker cleanup checks', 'worker')
        this.scenarioButton('Run track system checks', 'tracks')
        this.scenarioButton('Run sequence cache checks', 'sequence')
        this.scenarioButton('Run live subtitle checks', 'live')
        this.scenarioButton('Run final stress checks', 'stress')
        this.scenarioButton('Run reference checks', 'reference')
        this.scenarioButton('Prepare interrupted download', 'interrupt')
        this.scenarioButton('Resume interrupted download', 'resume')
        this.scenarioButton('Run export chooser checks', 'export')
        this.scenarioButton('Run playback recovery checks', 'recovery')
      }.width('100%')
      Text(this.result).fontSize(12).width('100%')
    }.padding(20).width('100%').height('100%')
  }
}
