import { runKitChecks } from '../KitChecks';
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
  build() {
    Column({ space: 6 }) {
      Text('Vesper candidate HAR consumer').fontSize(20)
      XComponent({ id: 'kit-video', type: XComponentType.SURFACE, controller: this.surface })
        .width('100%').height(120).onLoad(() => { this.surfaceId = this.surface.getXComponentSurfaceId(); })
      Button('Run kit checks').height(32).fontSize(12).enabled(!this.running).onClick(() => { this.run(); })
      Button('Observe corrected DVR').height(32).fontSize(12).enabled(!this.running).onClick(() => { this.run('dvr'); })
      Button('Observe local Apple EOF').height(32).fontSize(12).enabled(!this.running).onClick(() => { this.run('local-apple-eof'); })
      Button('Observe independent HLS native').height(32).fontSize(12).enabled(!this.running).onClick(() => { this.run('independent-native'); })
      Button('Observe independent HLS SDK').height(32).fontSize(12).enabled(!this.running).onClick(() => { this.run('independent-sdk'); })
      Button('Run capability checks').height(32).fontSize(12).enabled(!this.running).onClick(() => { this.run('capabilities'); })
      Button('Observe native DASH audio').height(32).fontSize(12).enabled(!this.running).onClick(() => { this.run('native-audio'); })
      Button('Run dynamic DASH checks').height(32).fontSize(12).enabled(!this.running).onClick(() => { this.run('dynamic-dash'); })
      Button('Run DASH profile checks').height(32).fontSize(12).enabled(!this.running).onClick(() => { this.run('profiles'); })
      Button('Prepare process downloads').height(32).fontSize(12).enabled(!this.running).onClick(() => { this.run('prepare'); })
      Button('Restore offline downloads').height(32).fontSize(12).enabled(!this.running).onClick(() => { this.run('restore'); })
      Button('Check offline DASH audio').height(32).fontSize(12).enabled(!this.running).onClick(() => { this.run('offline-audio'); })
      Button('Run dynamic DASH baseline').height(32).fontSize(12).enabled(!this.running).onClick(() => { this.run('dynamic-baseline'); })
      Button('Run parity checks').height(32).fontSize(12).enabled(!this.running).onClick(() => { this.run('parity'); })
      Button('Run recovery intent checks').height(32).fontSize(12).enabled(!this.running).onClick(() => { this.run('recovery-intent'); })
      Button('Observe public HLS').height(32).fontSize(12).enabled(!this.running).onClick(() => { this.run('stream-observation'); })
      Button('Run worker cleanup checks').height(32).fontSize(12).enabled(!this.running).onClick(() => { this.run('worker'); })
      Button('Run track system checks').height(32).fontSize(12).enabled(!this.running).onClick(() => { this.run('tracks'); })
      Button('Run sequence cache checks').height(32).fontSize(12).enabled(!this.running).onClick(() => { this.run('sequence'); })
      Button('Run live subtitle checks').height(32).fontSize(12).enabled(!this.running).onClick(() => { this.run('live'); })
      Button('Run final stress checks').height(32).fontSize(12).enabled(!this.running).onClick(() => { this.run('stress'); })
      Button('Run reference checks').height(32).fontSize(12).enabled(!this.running).onClick(() => { this.run('reference'); })
      Button('Prepare interrupted download').height(32).fontSize(12).enabled(!this.running).onClick(() => { this.run('interrupt'); })
      Button('Resume interrupted download').height(32).fontSize(12).enabled(!this.running).onClick(() => { this.run('resume'); })
      Button('Run export chooser checks').height(32).fontSize(12).enabled(!this.running).onClick(() => { this.run('export'); })
      Button('Run playback recovery checks').height(32).fontSize(12).enabled(!this.running).onClick(() => { this.run('recovery'); })
      Text(this.result).fontSize(12).width('100%')
    }.padding(20).width('100%').height('100%')
  }
}
