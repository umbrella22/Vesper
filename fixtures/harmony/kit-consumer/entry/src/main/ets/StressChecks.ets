import { common } from '@kit.AbilityKit';
import { fileIo as fs } from '@kit.CoreFileKit';
import { VesperPlayerController, VesperSourceSession, VesperCancellationToken } from '@vesper/player-kit';

/** One final batch exercises physical release and resource convergence. */
export async function runStressChecks(context: common.Context, surface: string): Promise<string> {
  const local = `${context.filesDir}/stress.mp4`; const bytes = await context.resourceManager.getRawFileContent('probe.mp4');
  const output = await fs.open(local, fs.OpenMode.CREATE | fs.OpenMode.TRUNC | fs.OpenMode.WRITE_ONLY);
  try { await fs.write(output.fd, bytes.buffer); } finally { await fs.close(output); }
  const sources = new VesperSourceSession(); const source = sources.register({ kind: 'local', protocol: 'file', uri: local, label: 'Stress' });
  const observations: number[] = [];
  try {
    for (let iteration = 0; iteration < 100; iteration++) {
      const controller = new VesperPlayerController(); controller.attachSurface(surface);
      try {
        const token = new VesperCancellationToken(); const cancelled = controller.activate(source, { timeoutMs: 10000 }, token);
        token.cancel(); let rejected = false;
        try { await cancelled; } catch (error) { rejected = `${error}`.includes('cancel'); }
        if (!rejected) { throw new Error(`Stress cancellation did not settle at ${iteration}`); }
        await controller.stop();
        await controller.activate(source, { timeoutMs: 10000 }); const started = Date.now();
        while (!controller.snapshot().firstFrame) {
          if (Date.now() - started > 10000) { throw new Error(`Stress first-frame deadline at ${iteration}`); }
          await new Promise<void>((resolve) => { setTimeout(resolve, 20); });
        }
        observations.push(controller.snapshot().firstFrameMs ?? -1);
        await controller.seek(250); await controller.seek(750); await controller.pause(); await controller.play();
      } finally { await controller.dispose(); }
      const released = controller.snapshot();
      if (released.retainedNativePlayers !== 0 || released.activeResources !== 0 || sources.snapshot().leases !== 0) {
        throw new Error(`Stress retained resources at ${iteration}: ${JSON.stringify(released)}`);
      }
    }
  } finally { source.close(); sources.close(); }
  return JSON.stringify({ iterations: observations.length, firstFrameMs: observations, source: sources.snapshot(),
    limitations: 'Simulator resource convergence; no physical decoder, audible output, power or thermal claim' });
}
