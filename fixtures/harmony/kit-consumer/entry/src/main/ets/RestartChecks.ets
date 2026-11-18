import { common } from '@kit.AbilityKit';
import { fileIo as fs } from '@kit.CoreFileKit';
import { VesperDownloadManager, VesperSourceSession } from '@vesper/player-kit';

let interruptedManager: VesperDownloadManager | undefined;
interface RestartIdentity { taskId: string; }
interface RestartFile { partialBytes?: number; partialHashes?: string[]; }
interface RestartRecord { task: RestartIdentity; files: RestartFile[]; }
interface RestartCheckpoint { tasks: RestartRecord[]; }

/** The runner force-stops after a durable prefix exists, with the next request still active. */
export async function runInterruptedDownload(context: common.Context, prepare: boolean): Promise<string> {
  const manager = await VesperDownloadManager.open(context); const sources = new VesperSourceSession();
  try {
    if (prepare) {
      for (const task of manager.snapshots()) { await manager.remove(task.taskId); }
      const source = sources.register({ kind: 'remote', protocol: 'progressive', label: 'Interrupted transfer',
        uri: 'http://127.0.0.1:18765/slow/probe.mp4', headers: { 'X-Vesper-Probe': 'h00' } });
      try {
        const task = await manager.create(source, 'interrupted-transfer'); manager.start(task.taskId);
        const root = `${context.filesDir}/vesper-downloads/asset-${task.taskId}`;
        const started = Date.now();
        while (Date.now() - started < 15000) {
          const names = await fs.listFile(root, { recursion: true, listNum: 512 });
          for (const name of names.filter((item: string) => item.endsWith('.part'))) {
            const size = (await fs.stat(`${root}/${name}`)).size;
            const checkpoint = JSON.parse(await fs.readText(`${context.filesDir}/vesper-downloads/checkpoint.json`)) as RestartCheckpoint;
            const committed = checkpoint.tasks.find((item: RestartRecord) => item.task.taskId === task.taskId)?.files
              .some((file: RestartFile) => (file.partialBytes ?? 0) >= 65536 && (file.partialHashes?.length ?? 0) > 0) ?? false;
            const current = manager.snapshots().find((item) => item.taskId === task.taskId);
            if (size >= 65536 && committed && current?.status !== 'Completed') {
              interruptedManager = manager;
              return JSON.stringify({ phase: 'transfer-in-progress', task: current, partialBytes: size, durablePrefix: true, inventory: manager.inventory() });
            }
          }
          await new Promise<void>((resolve) => { setTimeout(resolve, 50); });
        }
        throw new Error('Partial download did not reach the force-stop checkpoint');
      } finally { source.close(); }
    }
    const task = manager.snapshots().find((item) => item.assetId === 'interrupted-transfer');
    if (task === undefined || task.status !== 'Paused' || task.reason !== 'source_refresh_required') { throw new Error(`Interrupted restore was not paused: ${JSON.stringify(manager.snapshots())}`); }
    let rejected = false; try { manager.resume(task.taskId); } catch (error) { rejected = `${error}`.includes('Refresh the source'); }
    if (!rejected) { throw new Error('Interrupted restore reused nonpersistent access context'); }
    const source = sources.register({ kind: 'remote', protocol: 'progressive', label: 'Refreshed transfer',
      uri: 'http://127.0.0.1:18765/slow/probe.mp4', headers: { 'X-Vesper-Probe': 'h00' } });
    try {
      await manager.refreshSource(task.taskId, source);
      const refreshed = manager.snapshots().find((item) => item.taskId === task.taskId);
      if (refreshed?.resumeState !== 'version-check-required' || Number(refreshed.reusableBytes ?? '0') < 65536) {
        throw new Error('Refreshed source did not retain a committed resume prefix: ' + JSON.stringify(refreshed));
      }
      manager.resume(task.taskId);
      const done = await manager.wait(task.taskId);
      if (done.status !== 'Completed') { throw new Error(`Refreshed transfer failed: ${JSON.stringify(done)}`); }
      await manager.offlineSource(task.taskId);
      return JSON.stringify({ phase: 'resumed-after-force-stop', restored: task, refreshed: refreshed, completed: done });
    } finally { source.close(); }
  } finally { sources.close(); if (interruptedManager !== manager) { await manager.close(); } }
}
