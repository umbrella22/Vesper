import { media } from '@kit.MediaKit';
import { VesperPlayerController, VesperSourceSession } from '@vesper/player-kit';
import { prepareAndPlayNative } from './NativePlayerChecks';

async function until(ready: () => boolean, timeoutMs: number = 15000): Promise<void> {
  const started = Date.now();
  while (!ready()) {
    if (Date.now() - started > timeoutMs) { throw new Error('Independent HLS observation deadline'); }
    await new Promise<void>((resolve) => { setTimeout(resolve, 50); });
  }
}
interface HlsSample { elapsedMs: number; positionMs: number; state: string; eventPositionMs?: number; ptsUs?: number; selected?: number[]; error?: string; }

/** The origin uses FFmpeg HLS output; neither phase calls Vesper's DASH playlist generator. */
export async function runIndependentHlsChecks(surface: string, sdk: boolean): Promise<string[]> {
  const results: string[] = [];
  for (const profile of ['ts', 'fmp4-muxed', 'fmp4-separate']) {
    for (const live of [false, true]) {
      const started = Date.now(); const samples: HlsSample[] = [];
      const uri = `http://127.0.0.1:18765/independent/${profile}/${live ? 'live' : 'master'}.m3u8?session=${sdk ? 'sdk' : 'native'}-${profile}-${started}`;
      if (sdk) {
        const controller = new VesperPlayerController(); controller.attachSurface(surface); const sources = new VesperSourceSession();
        const handle = sources.register({ kind: 'remote', protocol: 'hls', uri: uri, label: 'Independent HLS' });
        try {
          await controller.activate(handle, { timeoutMs: 15000 }); await until(() => controller.snapshot().firstFrame);
          for (let index = 0; index < 6; index++) {
            await controller.refreshDiagnostics(); const state = controller.snapshot();
            samples.push({ elapsedMs: Date.now() - started, positionMs: state.positionMs, state: state.state,
              ptsUs: state.diagnostics?.presentationTimestampUs, selected: state.selection.confirmedTrackIds, error: state.error?.code });
            await new Promise<void>((resolve) => { setTimeout(resolve, 3000); });
          }
          results.push(JSON.stringify({ phase: 'sdk', profile: profile, live: live, started: started, ended: Date.now(),
            samples: samples, report: controller.diagnosticReport({ scenario: 'independent-hls', fixtureId: profile, runtimeKind: 'simulator' }),
            audibleOutput: 'unobserved' }));
        } catch (error) { results.push(JSON.stringify({ phase: 'sdk', profile: profile, live: live, samples: samples, error: `${error}` })); }
        finally { handle.close(); await controller.dispose(); sources.close(); }
      } else {
        const player = await media.createAVPlayer(); let rendered = false; let failure = ''; let eventPosition: number | undefined = undefined;
        player.on('startRenderFrame', (): void => { rendered = true; });
        player.on('error', (error): void => { failure = `${error.code}:${error.message}`; });
        player.on('timeUpdate', (position: number): void => { eventPosition = position; });
        try {
          player.url = uri; await until(() => player.state === 'initialized'); player.surfaceId = surface;
          await prepareAndPlayNative(player); await until(() => rendered);
          for (let index = 0; index < 6; index++) {
            const sample: HlsSample = { elapsedMs: Date.now() - started, positionMs: player.currentTime, state: player.state,
              eventPositionMs: eventPosition, selected: await player.getSelectedTracks(), error: failure };
            try { sample.ptsUs = player.getCurrentPresentationTimestamp(); } catch (_) {}
            samples.push(sample); await new Promise<void>((resolve) => { setTimeout(resolve, 3000); });
          }
          results.push(JSON.stringify({ phase: 'native', profile: profile, live: live, started: started, ended: Date.now(),
            tracks: await player.getTrackDescription(), samples: samples, audibleOutput: 'unobserved' }));
        } catch (error) { results.push(JSON.stringify({ phase: 'native', profile: profile, live: live, samples: samples, error: `${error}`, nativeError: failure })); }
        finally { await player.release(); }
      }
    }
  }
  return results;
}
