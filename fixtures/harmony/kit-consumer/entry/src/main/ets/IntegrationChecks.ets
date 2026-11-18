import { common } from '@kit.AbilityKit';
import { fileIo as fs } from '@kit.CoreFileKit';
import { VesperPlayerController, VesperSourceSession, VesperPlaybackSequence, VesperDiskCache, VesperDownloadManager, VesperPlayerError } from '@vesper/player-kit';

async function until(ready: () => boolean): Promise<void> {
  const started = Date.now(); while (!ready()) {
    if (Date.now() - started > 15000) { throw new Error('Integration observation deadline'); }
    await new Promise<void>((resolve) => { setTimeout(resolve, 20); });
  }
}
export async function runSequenceCacheChecks(context: common.Context, surface: string): Promise<string> {
  const disk = await VesperDiskCache.open(context, { partition: 'consumer-fixture', maximumBytes: 8 * 1024 * 1024 }); await disk.clear();
  const sources = new VesperSourceSession({ memoryBudgetBytes: 0, diskCache: disk });
  const controller = new VesperPlayerController(); controller.attachSurface(surface);
  const results: string[] = []; let stage = 'cache-cold';
  const one = sources.register({ kind: 'remote', protocol: 'progressive', label: 'One', uri: 'http://127.0.0.1:18765/probe.mp4', headers: { 'X-Vesper-Probe': 'h00' } });
  const two = sources.register({ kind: 'remote', protocol: 'progressive', label: 'Two', uri: 'http://127.0.0.1:18765/probe.mp4', headers: { 'X-Vesper-Probe': 'h00' } });
  const sequence = new VesperPlaybackSequence({ sequenceId: 'consumer-sequence' });
  try {
    await controller.activate(one, { timeoutMs: 15000 }); await until(() => controller.snapshot().firstFrame && JSON.parse(disk.inventory()).residentBytes > 65536);
    results.push(JSON.stringify({ cold: controller.snapshot(), disk: disk.inventory() })); await controller.stop();
    stage = 'cache-warm'; await controller.activate(one, { timeoutMs: 15000 }); await until(() => controller.snapshot().firstFrame);
    if (controller.snapshot().cacheBytes < 1) { throw new Error('Native player did not consume disk bytes'); }
    results.push(JSON.stringify({ warm: controller.snapshot(), disk: disk.inventory() })); await controller.stop();
    stage = 'cache-registration-isolation'; await controller.activate(two, { timeoutMs: 15000 }); await until(() => controller.snapshot().firstFrame);
    if (controller.snapshot().cacheBytes !== 0) { throw new Error('Separate registration hit another access partition'); } await controller.stop();
    stage = 'cache-clear'; await disk.clear(); await controller.activate(one, { timeoutMs: 15000 }); await until(() => controller.snapshot().firstFrame);
    if (controller.snapshot().cacheBytes !== 0) { throw new Error('Cleared disk bytes were reused'); } await controller.stop();
    stage = 'sequence';
    sequence.replace([{ itemId: 'one', providerNamespace: 'fixture', contentIdentity: 'one', mediaKind: 'vod', source: one },
      { itemId: 'two', providerNamespace: 'fixture', contentIdentity: 'two', mediaKind: 'vod', source: two }], 'one');
    one.close(); two.close(); sequence.attach(controller);
    const before = controller.snapshot().playbackEpoch; await sequence.activate({ timeoutMs: 15000 });
    const epoch = controller.snapshot().playbackEpoch; if (epoch === before) { throw new Error('Sequence did not activate'); }
    sequence.next(); if (controller.snapshot().playbackEpoch !== epoch) { throw new Error('Navigation implicitly activated'); }
    await sequence.activate({ timeoutMs: 15000 }); await until(() => controller.snapshot().firstFrame);
    if (controller.snapshot().playbackEpoch === epoch) { throw new Error('Sequence epoch did not advance'); }
    results.push(JSON.stringify({ sequence: sequence.snapshot(), playback: controller.snapshot() })); sequence.close(); await controller.dispose();
    if (sources.snapshot().leases !== 0 || controller.snapshot().retainedNativePlayers !== 0) { throw new Error('Sequence retained native/source resources'); }
  } catch (error) { throw new Error(JSON.stringify({ stage: stage, error: `${error}`, playback: controller.snapshot(), disk: disk.inventory() })); }
  finally { sequence.close(); await controller.dispose(); one.close(); two.close(); sources.close(); await disk.close(); }
  return JSON.stringify(results);
}

/** Split entrypoints let an external runner terminate the actual app process between them. */
export async function runProcessDownloads(context: common.Context, surface: string, prepare: boolean): Promise<string> {
  const manager = await VesperDownloadManager.open(context); const sources = new VesperSourceSession();
  const controller = new VesperPlayerController(); controller.attachSurface(surface); const results: string[] = [];
  let stage = prepare ? 'prepare' : 'offline-catalog'; let activeTitle = '';
  try {
    if (prepare) {
      for (const snapshot of manager.snapshots()) { await manager.remove(snapshot.taskId); }
      for (const route of ['probe.mp4', 'hls/master.m3u8', 'dash/manifest.mpd', 'template/manifest.mpd', 'timeline/manifest.mpd', 'long-template/manifest.mpd']) {
        activeTitle = route; stage = 'plan';
        const handle = sources.register({ kind: 'remote', protocol: route.endsWith('mpd') ? 'dash' : route.endsWith('m3u8') ? 'hls' : 'progressive',
          uri: `http://127.0.0.1:18765/${route}`, label: route, headers: { 'X-Vesper-Probe': 'h00' } });
        try {
          const task = await manager.create(handle, `process-${results.length}`); manager.start(task.taskId);
          stage = 'download-wait';
          let done = task;
          const windows = route.startsWith('long-template/') ? 6 : 1;
          for (let window = 0; window < windows; window++) {
            try { done = await manager.wait(task.taskId); break; }
            catch (error) { if (!(error instanceof VesperPlayerError) || error.code !== 'timeout' || window === windows - 1) { throw error as Error; } }
          }
          if (done.status !== 'Completed') { throw new Error(`Download did not complete: ${JSON.stringify(done)}`); }
          if (route.startsWith('long-template/')) { results.push(JSON.stringify({ task: done, profile: 'two-hour-2403-files' })); continue; }
          stage = 'export';
          const exported = await manager.exportAsset(task.taskId);
          const size = (await fs.stat(exported.path)).size;
          if (size < 1 || route !== 'probe.mp4' && !exported.path.endsWith('.zip')) { throw new Error('Original export format is invalid'); }
          results.push(JSON.stringify({ task: done, exportedBytes: size, exportMimeType: exported.mimeType })); await exported.release();
        } finally { handle.close(); }
      }
    } else {
      const completed = manager.snapshots().filter((task) => task.status === 'Completed');
      if (completed.length !== 6) { throw new Error('Expected six assets from the previous app process'); }
      for (const task of completed) {
        activeTitle = task.title ?? task.taskId; stage = 'offline-source';
        const source = sources.register(await manager.offlineSource(task.taskId));
        try { stage = 'offline-activation'; await controller.activate(source, { timeoutMs: 15000 });
          stage = 'offline-first-frame'; await until(() => controller.snapshot().firstFrame);
          stage = 'offline-seek'; await controller.seek(4000);
          let middleSeekMs: number | undefined; let tailSeekMs: number | undefined;
          if ((controller.snapshot().durationMs ?? 0) > 1000000) {
            const duration = controller.snapshot().durationMs ?? 0;
            stage = 'offline-middle-seek';
            await controller.seek(Math.floor(duration / 2));
            if (Math.abs(controller.snapshot().positionMs - duration / 2) > 1500) { throw new Error('Long offline asset middle seek did not complete'); }
            middleSeekMs = controller.snapshot().positionMs;
            stage = 'offline-tail-seek';
            await controller.seek(duration - 4000);
            tailSeekMs = controller.snapshot().positionMs;
          }
          stage = 'offline-rate'; await controller.setPlaybackRate(1.25);
          stage = 'offline-completion';
          await until(() => controller.snapshot().state === 'completed' || controller.snapshot().error !== undefined);
          if (controller.snapshot().state !== 'completed') { throw new Error('Offline asset failed before its end'); }
          results.push(JSON.stringify({ title: task.title, middleSeekMs: middleSeekMs, tailSeekMs: tailSeekMs, playback: controller.snapshot() })); await controller.stop(); }
        finally { source.close(); }
      }
    }
  } catch (error) { throw new Error(JSON.stringify({ stage: stage, title: activeTitle, error: `${error}`, completed: results, playback: controller.snapshot() })); }
  finally { await controller.dispose(); sources.close(); await manager.close(); }
  return JSON.stringify({ stage: prepare ? 'persisted' : 'restored-offline', results: results });
}

/** Audio selection has its own gate so a failure does not hide long-asset playback results. */
export async function runOfflineDashAudioChecks(context: common.Context, surface: string): Promise<string[]> {
  const manager = await VesperDownloadManager.open(context); const sources = new VesperSourceSession();
  const controller = new VesperPlayerController(); controller.attachSurface(surface);
  const results: string[] = []; let failures = 0;
  try {
    const tasks = manager.snapshots().filter((task) => task.status === 'Completed' &&
      (task.title?.startsWith('template/') || task.title?.startsWith('timeline/')));
    if (tasks.length !== 2) { throw new Error('Expected both persisted DASH audio profiles'); }
    for (const task of tasks) {
      const source = sources.register(await manager.offlineSource(task.taskId));
      try {
        await controller.activate(source, { timeoutMs: 15000 }); await until(() => controller.snapshot().firstFrame);
        await controller.seek(4000);
        const audio = controller.snapshot().tracks.filter((track) => track.kind === 'audio');
        if (audio.length !== 2) { throw new Error('Both downloaded audio tracks must remain available offline'); }
        const alternate = audio.find((track) => !track.selected);
        if (alternate === undefined) { throw new Error('No alternate downloaded audio track'); }
        await controller.selectTrack({ kind: 'audio', mode: 'track', trackId: alternate.id });
        if (!controller.snapshot().selection.confirmedTrackIds.includes(alternate.id)) { throw new Error('Offline audio selection was not confirmed'); }
        results.push(JSON.stringify({ title: task.title, status: 'PASS', playback: controller.snapshot() }));
      } catch (error) {
        failures++; results.push(JSON.stringify({ title: task.title, status: 'FAIL', error: `${error}`, playback: controller.snapshot() }));
      } finally { await controller.stop(); source.close(); }
    }
  } finally { await controller.dispose(); sources.close(); await manager.close(); }
  if (failures > 0) { throw new Error(JSON.stringify({ gate: 'offline-dash-audio-selection', failures: failures, results: results })); }
  return results;
}
