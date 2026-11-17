import { common } from '@kit.AbilityKit';
import { fileIo as fs } from '@kit.CoreFileKit';
import { VesperPlayerController, VesperSourceSession, VesperPlayerSource,
  VesperPlayerSnapshot, VesperCancellationToken } from '@vesper/player-kit';
import { runDownloadChecks } from './DownloadChecks';
import { runTrackSystemChecks } from './TrackSystemChecks';

interface CheckResult { name: string; snapshot: VesperPlayerSnapshot; }

async function until(ready: () => boolean): Promise<void> {
  const started = Date.now();
  while (!ready()) {
    if (Date.now() - started > 10000) { throw new Error('Consumer observation deadline'); }
    await new Promise<void>((resolve) => { setTimeout(resolve, 20); });
  }
}

/** Exercises only the candidate HAR's public facade. */
export async function runKitChecks(context: common.Context, surfaceId: string): Promise<string> {
  const localPath = `${context.filesDir}/consumer.mp4`;
  const content = await context.resourceManager.getRawFileContent('probe.mp4');
  const file = await fs.open(localPath, fs.OpenMode.CREATE | fs.OpenMode.TRUNC | fs.OpenMode.WRITE_ONLY);
  try { await fs.write(file.fd, content.buffer); } finally { await fs.close(file); }
  const sources: VesperPlayerSource[] = [
    { kind: 'local', protocol: 'file', uri: localPath, label: 'local-fd' },
    { kind: 'remote', protocol: 'progressive', uri: 'http://127.0.0.1:18765/probe.mp4', label: 'http-mp4', headers: { 'X-Vesper-Probe': 'h00' } },
    { kind: 'remote', protocol: 'progressive', uri: 'http://127.0.0.1:18765/redirect/probe.mp4', label: 'redirect-mp4', headers: { 'X-Vesper-Probe': 'h00' } },
    { kind: 'remote', protocol: 'hls', uri: 'http://127.0.0.1:18765/hls/master.m3u8', label: 'hls-vod', headers: { 'X-Vesper-Probe': 'h00' } },
    { kind: 'remote', protocol: 'dash', uri: 'http://127.0.0.1:18765/dash/manifest.mpd', label: 'dash-vod', headers: { 'X-Vesper-Probe': 'h00' } }
  ];
  const session = new VesperSourceSession();
  const controller = new VesperPlayerController();
  controller.attachSurface(surfaceId);
  const results: CheckResult[] = [];
  const failures: string[] = [];
  let downloads: string = '';
  let tracksAndSystem: string = '';
  try {
    for (const source of sources) {
      const handle = session.register(source);
      try {
        const preload = source.protocol === 'dash' ? handle.preload({ timeoutMs: 10000 }) : undefined;
        if (preload !== undefined) { await preload.completion; }
        await controller.activate(handle, { timeoutMs: 10000 });
        await until(() => controller.snapshot().firstFrame && controller.snapshot().positionMs > 0);
        await controller.pause(); await controller.seek(500); await controller.setPlaybackRate(1.25);
        await controller.setVolume(0.5); await controller.setMuted(true); await controller.setMuted(false);
        const snapshot = controller.snapshot();
        if (preload !== undefined && (snapshot.cacheBytes < 1 || preload.snapshot().reuseLevel !== 'playbackReusable')) {
          throw new Error('Formal DASH startup cache was not consumed and rendered');
        }
        if (snapshot.tracks.length < 2 || snapshot.selection.confirmedTrackIds.length < 1) { throw new Error('Track evidence missing'); }
        results.push({ name: source.label, snapshot: snapshot });
        await controller.stop();
      } catch (error) { failures.push(`${source.label}: ${error}`); }
      finally { handle.close(); }
    }
    const handle = session.register(sources[1]);
    const cancelled = new VesperCancellationToken(); cancelled.cancel();
    let cancellation = '';
    try { await controller.activate(handle, {}, cancelled); }
    catch (error) { cancellation = `${error}`; }
    if (!cancellation.includes('cancel')) { failures.push('Pre-cancelled activation did not reject'); }
    handle.close();
    try { downloads = JSON.stringify(await runDownloadChecks(context, surfaceId)); }
    catch (error) { failures.push(`downloads: ${error}`); }
    try { tracksAndSystem = await runTrackSystemChecks(context as common.UIAbilityContext, surfaceId); }
    catch (error) { failures.push(`tracks-system: ${error}`); }
  } finally { await controller.dispose(); session.close(); }
  return JSON.stringify({ status: failures.length === 0 ? 'PASS' : 'FAIL',
    scope: 'production-har-public-facade', results: results, downloads: downloads, tracksAndSystem: tracksAndSystem, failures: failures,
    finalController: controller.snapshot(), finalSources: session.snapshot() });
}
