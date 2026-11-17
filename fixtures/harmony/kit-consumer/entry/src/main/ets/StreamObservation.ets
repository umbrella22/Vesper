import { common } from '@kit.AbilityKit';
import { fileIo as fs } from '@kit.CoreFileKit';
import { VesperPlayerController, VesperSourceSession } from '@vesper/player-kit';

interface StreamCase { name: string; protocol: string; url: string; deadlineMs: number; }

/** Bounded, passive observation of the original public HLS URL, followed by a local control. */
export async function runStreamObservation(context: common.UIAbilityContext, surface: string): Promise<string> {
  const sources = new VesperSourceSession(); const controller = new VesperPlayerController(context); controller.attachSurface(surface);
  const path = context.filesDir + '/public-stream-observation.ndjson';
  const output = await fs.open(path, fs.OpenMode.CREATE | fs.OpenMode.TRUNC | fs.OpenMode.WRITE_ONLY);
  const results: string[] = [];
  try {
    const cases: StreamCase[] = [{ name: 'apple-hls', protocol: 'hls', url: 'https://devstreaming-cdn.apple.com/videos/streaming/examples/img_bipbop_adv_example_ts/master.m3u8', deadlineMs: 720000 },
      { name: 'local-hls-control', protocol: 'hls', url: 'http://127.0.0.1:18765/hls/master.m3u8', deadlineMs: 45000 }];
    for (const test of cases) {
      const source = sources.register({ kind: 'remote', protocol: test.protocol, uri: test.url, label: test.name });
      const started = Date.now(); let lastProgress = started; let position = 0; let terminal = 'deadline'; let samples = 0;
      let selectionAttempted = false; let requestedBitrate: number | undefined;
      try {
        await fs.write(output.fd, JSON.stringify({ test: test.name, url: test.url, started: started, api: controller.capabilities().runtimeApi }) + '\n');
        try { await controller.activate(source, { timeoutMs: 30000 }); }
        catch (error) { terminal = 'activationFailed'; await fs.write(output.fd, JSON.stringify({ test: test.name, error: `${error}` }) + '\n'); }
        if (terminal !== 'activationFailed') {
          while (samples < 400 && Date.now() - started < test.deadlineMs) {
            const snapshot = controller.snapshot();
            await fs.write(output.fd, JSON.stringify({ test: test.name, at: Date.now(), snapshot: snapshot, events: controller.drainEvents() }) + '\n');
            samples++;
            if (snapshot.positionMs > position + 100) { position = snapshot.positionMs; lastProgress = Date.now(); }
            if (snapshot.state === 'completed') { terminal = 'naturalCompletion'; break; }
            if (snapshot.error !== undefined) { terminal = 'playbackError'; break; }
            if (Date.now() - lastProgress > 30000) { terminal = 'noProgress30s'; break; }
            if (test.name === 'apple-hls' && !selectionAttempted && Date.now() - started >= 15000) {
              selectionAttempted = true;
              if (snapshot.availableBitrates.length >= 2) {
                requestedBitrate = Math.min(...snapshot.availableBitrates.filter((value) => value > 0));
                try { await controller.selectTrack({ kind: 'video', mode: 'bitrate', bitrate: requestedBitrate }); }
                catch (error) { await fs.write(output.fd, JSON.stringify({ test: test.name, selectionError: `${error}` }) + '\n'); }
                await fs.write(output.fd, JSON.stringify({ test: test.name, at: Date.now(), action: 'fixed-bitrate', requestedBitrate: requestedBitrate,
                  snapshot: controller.snapshot(), events: controller.drainEvents() }) + '\n');
              }
            }
            await new Promise<void>((resolve) => { setTimeout(resolve, 2000); });
          }
        }
        const final = controller.snapshot();
        let ranges: string = 'API 26 required';
        if (controller.capabilities().runtimeApi >= 26) {
          try { ranges = JSON.stringify(await controller.timeRanges()); } catch (error) { ranges = `${error}`; }
        }
        results.push(JSON.stringify({ test: test.name, terminal: terminal, elapsedMs: Date.now() - started, samples: samples,
          final: final, ranges: ranges, rate: final.playbackRate, requestedBitrate: requestedBitrate }));
      } finally { source.close(); await controller.stop().catch(() => {}); }
    }
    return JSON.stringify({ observations: results, details: path, scope: '1x-original-public-hls-fixed-bitrate-and-local-natural-completion' });
  } finally { await fs.close(output); await controller.dispose(); sources.close(); }
}
