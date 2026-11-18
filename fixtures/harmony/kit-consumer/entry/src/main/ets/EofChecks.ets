import { media } from '@kit.MediaKit';
import { VesperPlayerController, VesperSourceSession } from '@vesper/player-kit';
import { prepareAndPlayNative } from './NativePlayerChecks';

async function until(ready: () => boolean, timeoutMs: number = 15000): Promise<void> {
  const started = Date.now();
  while (!ready()) {
    if (Date.now() - started > timeoutMs) { throw new Error('EOF observation deadline'); }
    await new Promise<void>((resolve) => { setTimeout(resolve, 50); });
  }
}

/** Full 600-second original rendition is retained; these controls seek to its final 15 seconds. */
export async function runLocalAppleEofChecks(surface: string): Promise<string[]> {
  const results: string[] = [];
  const uri = 'http://127.0.0.1:18765/apple/master.m3u8';
  const nativeStarted = Date.now();
  const player = await media.createAVPlayer(); let eos = false; let seekDone = false; let nativeError = '';
  player.on('endOfStream', (): void => { eos = true; });
  player.on('seekDone', (): void => { seekDone = true; });
  player.on('error', (error): void => { nativeError = `${error.code}:${error.message}`; });
  try {
    player.url = uri; await until(() => player.state === 'initialized'); player.surfaceId = surface;
    await prepareAndPlayNative(player); await until(() => player.currentTime > 0);
    const duration = player.duration; player.seek(Math.max(0, duration - 15000), media.SeekMode.SEEK_CLOSEST); await until(() => seekDone);
    for (let index = 0; index < 45; index++) {
      if (player.state === 'completed' || eos || nativeError.length > 0) { break; }
      await new Promise<void>((resolve) => { setTimeout(resolve, 1000); });
    }
    results.push(JSON.stringify({ phase: 'native-full-material-tail-seek', started: nativeStarted, ended: Date.now(), durationMs: duration, positionMs: player.currentTime,
      state: player.state, endOfStream: eos, nativeError: nativeError, tracks: await player.getTrackDescription() }));
  } catch (error) { results.push(JSON.stringify({ phase: 'native-full-material-tail-seek', started: nativeStarted, ended: Date.now(), error: `${error}`, nativeError: nativeError })); }
  finally { await player.release(); }
  const sdkStarted = Date.now();
  const sources = new VesperSourceSession(); const controller = new VesperPlayerController(); controller.attachSurface(surface);
  const handle = sources.register({ kind: 'remote', protocol: 'hls', uri: uri, label: 'Local full Apple rendition' });
  try {
    await controller.activate(handle, { timeoutMs: 20000 }); await until(() => controller.snapshot().firstFrame);
    await controller.seek(Math.max(0, (controller.snapshot().durationMs ?? 0) - 15000));
    for (let index = 0; index < 45; index++) {
      const snapshot = controller.snapshot(); if (snapshot.state === 'completed' || snapshot.error !== undefined) { break; }
      await controller.refreshDiagnostics(); await new Promise<void>((resolve) => { setTimeout(resolve, 1000); });
    }
    results.push(JSON.stringify({ phase: 'sdk-full-material-tail-seek', started: sdkStarted, ended: Date.now(), report: controller.diagnosticReport({ scenario: 'tail-completion', fixtureId: 'apple-bipbop-adv-ts', runtimeKind: 'simulator' }) }));
  } catch (error) { results.push(JSON.stringify({ phase: 'sdk-full-material-tail-seek', started: sdkStarted, ended: Date.now(), error: `${error}` })); }
  finally { handle.close(); await controller.dispose(); sources.close(); }
  return results;
}
