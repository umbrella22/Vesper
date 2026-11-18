import { common } from '@kit.AbilityKit';
import { fileIo as fs } from '@kit.CoreFileKit';
import { VesperDownloadManager, VesperDownloadSnapshot, VesperSourceSession, VesperPlayerController,
  VesperPlayerSource, VesperPlayerSnapshot } from '@vesper/player-kit';

interface OfflineResult { name: string; task: VesperDownloadSnapshot; playback: VesperPlayerSnapshot; }

async function firstFrame(controller: VesperPlayerController): Promise<void> {
  const started = Date.now();
  while (!controller.snapshot().firstFrame || controller.snapshot().positionMs <= 0) {
    if (Date.now() - started > 10000) { throw new Error('Offline first-frame deadline'); }
    await new Promise<void>((resolve) => { setTimeout(resolve, 20); });
  }
}

/** Downloads, closes the manager, verifies its checkpoint, and plays without network access. */
export async function runDownloadChecks(context: common.Context, surface: string): Promise<OfflineResult[]> {
  const root = `${context.filesDir}/vesper-downloads`;
  if (await fs.access(root)) { await fs.rmdir(root); }
  let manager = await VesperDownloadManager.open(context);
  const session = new VesperSourceSession(); const controller = new VesperPlayerController(); controller.attachSurface(surface);
  const results: OfflineResult[] = []; const ids: string[] = [];
  let stage = 'create';
  try {
    const paths: string[] = ['probe.mp4', 'hls/master.m3u8', 'dash/manifest.mpd', 'template/manifest.mpd', 'timeline/manifest.mpd'];
    const protocols: string[] = ['progressive', 'hls', 'dash', 'dash', 'dash'];
    for (let index = 0; index < paths.length; index++) {
      const source: VesperPlayerSource = { kind: 'remote', protocol: protocols[index], label: `offline-${protocols[index]}`,
        uri: `http://127.0.0.1:18765/${paths[index]}`, headers: { 'X-Vesper-Probe': 'h00' } };
      const handle = session.register(source);
      try {
        stage = `create-${protocols[index]}`;
        const created = await manager.create(handle, `offline-${protocols[index]}-${index}`); ids.push(created.taskId);
        manager.start(created.taskId);
        if (index === 0) {
          await manager.pause(created.taskId); await manager.wait(created.taskId);
          manager.resume(created.taskId);
        }
        stage = `wait-${protocols[index]}`; const completed = await manager.wait(created.taskId);
        if (completed.status !== 'Completed') { throw new Error(`Download did not complete: ${JSON.stringify(completed)}`); }
      } finally { handle.close(); }
    }
    stage = 'reopen'; await manager.close(); manager = await VesperDownloadManager.open(context);
    const checkpoint = await fs.readText(`${root}/checkpoint.json`);
    if (checkpoint.includes('http:') || checkpoint.includes('X-Vesper-Probe') || checkpoint.includes('h00')) { throw new Error('Checkpoint retained network access context'); }
    for (const id of ids) {
      stage = `offline-source-${id}`;
      const source = await manager.offlineSource(id); const handle = session.register(source);
      try {
        stage = `activate-${id}`; await controller.activate(handle, { timeoutMs: 10000 });
        stage = `frame-${id}`; await firstFrame(controller);
        stage = `seek-${id}`; await controller.pause(); await controller.seek(500); await controller.setPlaybackRate(1.25);
        let retained = false;
        try { await manager.remove(id); } catch (error) { retained = `${error}`.includes('lease'); }
        if (!retained) { throw new Error('Offline playback did not retain its asset'); }
        const task = manager.snapshots().find((item: VesperDownloadSnapshot) => item.taskId === id);
        if (task === undefined) { throw new Error('Restored task missing'); }
        results.push({ name: source.label, task: task, playback: controller.snapshot() });
        await controller.stop();
      } finally { handle.close(); }
    }
    for (const id of ids) { await manager.remove(id); }
  } catch (error) { throw new Error(JSON.stringify({ stage: stage, error: `${error}`, playback: controller.snapshot(), tasks: manager.snapshots(), inventory: manager.inventory() })); }
  finally { await controller.dispose(); await manager.close(); session.close(); }
  return results;
}
