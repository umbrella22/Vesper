import { VesperPlayerController, VesperSourceSession, VesperPlayerError } from '@vesper/player-kit';
import { rcp } from '@kit.RemoteCommunicationKit';

interface LiveInventory { refreshes: number; refreshFailures: number; retainedResources: number; activePins: number; mediaSequences: string[]; }
interface ServerInventory { provider?: string; failures: number; }
async function until(ready: () => boolean, milliseconds: number = 20000): Promise<void> {
  const deadline = Date.now() + milliseconds;
  while (!ready()) {
    if (Date.now() >= deadline) { throw new Error('Dynamic DASH observation deadline'); }
    await new Promise<void>((resolve) => { setTimeout(resolve, 50); });
  }
}
async function arm(faultCase: string, resource: string): Promise<void> {
  const session = rcp.createSession();
  try {
    const request = new rcp.Request('http://127.0.0.1:18765/fault-control/' + faultCase + '/arm?resource=' + resource);
    request.configuration = { transfer: { timeout: { connectMs: 5000, transferMs: 5000 } } };
    if ((await session.fetch(request)).statusCode !== 200) { throw new Error('Could not arm dynamic DASH fault'); }
  } finally { session.close(); }
}

/** Proves advancing native playback and new segment requests across moving MPD windows. */
export async function runDynamicDashChecks(surface: string, faults: boolean = true): Promise<string> {
  const sources = new VesperSourceSession(); const controller = new VesperPlayerController(); controller.attachSurface(surface);
  const faultCase = Date.now().toString();
  const source = sources.register({ kind: 'remote', protocol: 'dash', label: 'Restricted dynamic DASH',
    uri: 'http://127.0.0.1:18765/fault-once/' + faultCase + '/dash-live/manifest.mpd?session=' + faultCase });
  const replacement = sources.register({ kind: 'remote', protocol: 'progressive', label: 'Replacement', uri: 'http://127.0.0.1:18765/probe.mp4' });
  let stage = 'activate'; const observations: string[] = [];
  const inventory = (): ServerInventory => JSON.parse(controller.snapshot().resourceLoaderInfo ?? '{"failures":0}') as ServerInventory;
  const live = (): LiveInventory => JSON.parse(inventory().provider ?? '{"refreshes":0,"refreshFailures":0}') as LiveInventory;
  try {
    await controller.setPlaybackPolicy({ retry: { maxAttempts: 1 } });
    await controller.activate(source, { timeoutMs: 25000 });
    await until(() => controller.snapshot().firstFrame && controller.snapshot().positionMs > 200);
    const first = controller.snapshot();
    if (first.timeline !== 'live' || !first.tracks.some((track) => track.kind === 'audio') || !first.tracks.some((track) => track.kind === 'video')) {
      throw new Error('Dynamic DASH must expose native audio/video and a live timeline');
    }
    observations.push(JSON.stringify({ first: first, bridge: live() }));
    if (faults) {
      stage = 'manifest-refresh-failure'; await until(() => live().refreshes >= 3); await arm(faultCase, 'mpd');
      await until(() => live().refreshFailures >= 1);
      observations.push(JSON.stringify({ afterManifestFailure: controller.snapshot(), bridge: live() }));
      stage = 'segment-failure'; await arm(faultCase, 'm4s'); await until(() => inventory().failures >= 1);
    }
    stage = 'continued-playback';
    await until(() => controller.snapshot().positionMs >= first.positionMs + 30000 && live().refreshes >= 10, 50000);
    const advancing = controller.snapshot();
    if (advancing.error !== undefined || advancing.playbackEpoch !== first.playbackEpoch || live().retainedResources > 1044) {
      throw new Error('Live playback did not retain a coherent bounded activation');
    }
    observations.push(JSON.stringify({ advancing: advancing, bridge: live() }));
    stage = 'source-change'; const changedAt = Date.now();
    await controller.activate(replacement, { timeoutMs: 15000 }); await until(() => controller.snapshot().firstFrame && controller.snapshot().positionMs > 200);
    if (controller.snapshot().playbackEpoch === first.playbackEpoch || controller.snapshot().retainedNativePlayers !== 1) {
      throw new Error('Source change retained the previous native player');
    }
    const replaced = controller.snapshot(); await controller.stop();
    if (sources.snapshot().leases !== 0 || controller.snapshot().retainedNativePlayers !== 0) { throw new Error('Dynamic DASH cleanup retained resources'); }
    return JSON.stringify({ faultCase: faultCase, injectedFaults: faults, changedAt: changedAt, observations: observations,
      replacement: replaced, final: controller.snapshot(), sources: sources.snapshot(), dvr: 'unsupported' });
  } catch (error) {
    if (stage === 'activate' && error instanceof VesperPlayerError && error.code === 'unsupported') {
      await controller.stop();
      await until(() => controller.snapshot().retainedNativePlayers === 0 && sources.snapshot().leases === 0, 10000);
      const rejected = controller.snapshot();
      await controller.activate(replacement, { timeoutMs: 15000 }); await until(() => controller.snapshot().firstFrame);
      return JSON.stringify({ profileStatus: 'unsupported', reason: error.message, expectedContract: 'explicit-rejection',
        dynamicPlaybackAccepted: false, rejected: rejected, replacement: controller.snapshot() });
    }
    throw new Error(JSON.stringify({ stage: stage, error: `${error}`, observations: observations, playback: controller.snapshot(), bridge: live() }));
  }
  finally { source.close(); replacement.close(); await controller.dispose(); sources.close(); }
}
