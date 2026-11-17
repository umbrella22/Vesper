import { media } from '@kit.MediaKit';
import { systemDateTime } from '@kit.BasicServicesKit';
import { VesperPlayerController, VesperSourceSession } from '@vesper/player-kit';

interface Sample { route: string; readyMs: number; firstFrameMs: number; seekDoneMs: number; }
function now(): number { return systemDateTime.getUptime(systemDateTime.TimeType.STARTUP, false); }
async function until(ready: () => boolean): Promise<void> {
  const start = now(); while (!ready()) {
    if (now() - start > 15000) { throw new Error('Reference observation deadline'); }
    await new Promise<void>((resolve) => { setTimeout(resolve, 20); });
  }
}
export async function runReferenceChecks(surface: string): Promise<string> {
  const samples: Sample[] = [];
  for (let repeat = 0; repeat < 3; repeat++) {
    const started = now(); const player = await media.createAVPlayer(); let frame = -1; let seek = false; let failure = '';
    try {
      player.on('error', (error) => { failure = `${error.code}`; });
      player.on('startRenderFrame', () => { frame = now() - started; }); player.on('seekDone', () => { seek = true; });
      const source = media.createMediaSourceWithUrl('http://127.0.0.1:18765/probe.mp4', { 'X-Vesper-Probe': 'h00' });
      await player.setMediaSource(source); await until(() => player.state === 'initialized' || failure.length > 0);
      if (failure.length > 0) { throw new Error(`Reference source error ${failure}`); }
      player.surfaceId = surface; await player.prepare();
      await until(() => player.state === 'prepared' || failure.length > 0);
      if (failure.length > 0) { throw new Error(`Reference prepare error ${failure}`); }
      const ready = now() - started; await player.play();
      await until(() => frame >= 0 || failure.length > 0);
      if (failure.length > 0) { throw new Error(`Reference playback error ${failure}`); }
      const seekStart = now(); player.seek(4000); await until(() => seek || failure.length > 0);
      if (failure.length > 0) { throw new Error(`Reference seek error ${failure}`); }
      samples.push({ route: 'native-direct-http', readyMs: ready, firstFrameMs: frame, seekDoneMs: now() - seekStart });
    } finally { await player.release(); }
    const sources = new VesperSourceSession(); const handle = sources.register({ kind: 'remote', protocol: 'progressive', label: 'Reference',
      uri: 'http://127.0.0.1:18765/probe.mp4', headers: { 'X-Vesper-Probe': 'h00' } });
    const controller = new VesperPlayerController(); controller.attachSurface(surface); const sdkStart = now();
    try {
      await controller.activate(handle, { timeoutMs: 15000 }); const ready = now() - sdkStart;
      await until(() => controller.snapshot().firstFrame); const frame = controller.snapshot().firstFrameMs ?? -1;
      const seekStart = now(); await controller.seek(4000);
      samples.push({ route: 'vesper-resource-loader-http', readyMs: ready, firstFrameMs: frame, seekDoneMs: now() - seekStart });
    } finally { await controller.dispose(); handle.close(); sources.close(); }
  }
  return JSON.stringify({ samples: samples, scope: 'alternating-simulator-smoke-observations',
    limitations: 'Three samples per route, warm process, native-direct versus loader. No p95, visible seek recovery, CPU, power or physical-device performance acceptance.' });
}
