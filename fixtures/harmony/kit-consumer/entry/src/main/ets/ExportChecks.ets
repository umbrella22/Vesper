import { common } from '@kit.AbilityKit';
import { fileIo as fs } from '@kit.CoreFileKit';
import { VesperDownloadManager, VesperSourceSession } from '@vesper/player-kit';

/** A runner completes one save, cancels a second picker, and dismisses sharing. */
export async function runExportChecks(context: common.UIAbilityContext): Promise<string> {
  const manager = await VesperDownloadManager.open(context); const sources = new VesperSourceSession();
  let managerClosed = false;
  try {
    const source = sources.register({ kind: 'remote', protocol: 'progressive', label: 'Export checks',
      uri: 'http://127.0.0.1:18765/probe.mp4', headers: { 'X-Vesper-Probe': 'h00' } });
    try {
      const task = await manager.create(source, `export-${Date.now()}`); manager.start(task.taskId);
      if ((await manager.wait(task.taskId)).status !== 'Completed') { throw new Error('Export download did not complete'); }
      const artifact = await manager.exportAsset(task.taskId);
      try {
        const bytes = (await fs.stat(artifact.path)).size;
        await manager.remove(task.taskId); await manager.close(); managerClosed = true;
        if ((await fs.stat(artifact.path)).size !== bytes) { throw new Error('Temporary export did not survive source removal and manager close'); }
        const saved = await artifact.save(context);
        if (!saved) { throw new Error('The first picker must save its output'); }
        const cancelled = !await artifact.save(context);
        if (!cancelled) { throw new Error('The second picker must be cancelled'); }
        let shared = 'chooser-shown';
        try { await artifact.share(context); }
        catch (error) { if (`${error}`.includes('unavailable')) { shared = 'unsupported_on_runtime'; } else { throw error as Error; } }
        let retained = false; try { await artifact.release(); } catch (error) { retained = `${error}`.includes('reader retains'); }
        if (shared === 'chooser-shown' && !retained) { throw new Error('Shared output lost its recipient retention lease'); }
        return JSON.stringify({ saved: saved, cancelled: cancelled, sharing: shared, retainedForRecipient: retained, exportedBytes: bytes });
      } catch (error) { try { await artifact.release(); } catch (_) {} throw error as Error; }
    } finally { source.close(); }
  } finally { sources.close(); if (!managerClosed) { await manager.close(); } }
}
