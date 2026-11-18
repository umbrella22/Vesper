import { VesperPlayerController, VesperSourceSession } from '@vesper/player-kit';

async function until(ready: () => boolean): Promise<void> {
  const start = Date.now(); while (!ready()) {
    if (Date.now() - start > 20000) { throw new Error('Live/subtitle observation deadline'); }
    await new Promise<void>((resolve) => { setTimeout(resolve, 20); });
  }
}
export async function runLiveChecks(surface: string): Promise<string> {
  const sources = new VesperSourceSession(); const controller = new VesperPlayerController(); controller.attachSurface(surface);
  const results: string[] = []; let stage = 'live';
  try {
    const live = sources.register({ kind: 'remote', protocol: 'hls', uri: 'http://127.0.0.1:18765/live/media.m3u8', label: 'Rolling live', headers: { 'X-Vesper-Probe': 'h00' } });
    try { await controller.activate(live, { timeoutMs: 20000 }); await until(() => controller.snapshot().firstFrame && controller.snapshot().timeline === 'live'); }
    finally { live.close(); }
    let rejected = false; try { await controller.setPlaybackRate(1.5); } catch (_) { rejected = true; }
    if (!rejected) { throw new Error('Live numeric playback rate was accepted'); }
    await new Promise<void>((resolve) => { setTimeout(resolve, 8000); });
    if (controller.snapshot().state !== 'playing' || controller.snapshot().error !== undefined) { throw new Error('Rolling live did not remain in playing state'); }
    if (controller.capabilities().runtimeApi >= 26) {
      stage = 'live-ranges'; const ranges = await controller.timeRanges();
      results.push(JSON.stringify({ liveRanges: ranges }));
      stage = 'live-default-position';
      let unsupported = false; try { await controller.seekToDefaultPosition(); } catch (error) { unsupported = `${error}`.includes('resource-loader route'); }
      if (!unsupported || controller.snapshot().error !== undefined) { throw new Error('Unverified live default-position route must reject without poisoning playback'); }
      results.push(JSON.stringify({ liveDefaultPosition: 'unsupported_resource_loader', nativeFailureEvidence: 5400102 }));
    }
    results.push(JSON.stringify({ live: controller.snapshot() })); await controller.stop();
    stage = 'dash-audio-video-seek-control';
    const control = sources.register({ kind: 'remote', protocol: 'dash', uri: 'http://127.0.0.1:18765/dash/manifest.mpd', label: 'DASH A/V control', headers: { 'X-Vesper-Probe': 'h00' } });
    try {
      await controller.activate(control, { timeoutMs: 20000 });
      await until(() => controller.snapshot().firstFrame && controller.snapshot().positionMs >= 100);
      await controller.seek(9000); await controller.seek(500);
      results.push(JSON.stringify({ dashControl: controller.snapshot(), events: controller.drainEvents() }));
      await controller.stop();
    } finally { control.close(); }
    stage = 'dash-external-webvtt-control';
    const external = sources.register({ kind: 'remote', protocol: 'dash', uri: 'http://127.0.0.1:18765/dash/manifest.mpd', label: 'DASH external captions', headers: { 'X-Vesper-Probe': 'h00' },
      externalSubtitles: [{ id: 'captions-en', uri: 'http://127.0.0.1:18765/dash/captions.vtt', mimeType: 'text/vtt', label: 'English', headers: { 'X-Vesper-Probe': 'h00' } }] });
    try {
      await controller.activate(external, { timeoutMs: 20000, playWhenReady: false });
      const track = controller.snapshot().tracks.find((item) => item.kind === 'subtitle');
      if (track === undefined) { throw new Error('External WebVTT track is missing'); }
      await controller.selectTrack({ kind: 'subtitle', mode: 'track', trackId: track.id }); await controller.play();
      await until(() => controller.snapshot().subtitle.includes('Vesper') && controller.snapshot().positionMs >= 100);
      await controller.seek(9000); await controller.seek(500);
      results.push(JSON.stringify({ dashExternalControl: controller.snapshot(), events: controller.drainEvents() }));
      await controller.stop();
    } finally { external.close(); }
    stage = 'dash-webvtt';
    const subtitles = sources.register({ kind: 'remote', protocol: 'dash', uri: 'http://127.0.0.1:18765/dash/wvtt.mpd', label: 'DASH captions', headers: { 'X-Vesper-Probe': 'h00' } });
    try {
      await controller.activate(subtitles, { timeoutMs: 20000, playWhenReady: false });
      const track = controller.snapshot().tracks.find((item) => item.kind === 'subtitle');
      if (track === undefined) { throw new Error('DASH WebVTT track is missing'); }
      await controller.selectTrack({ kind: 'subtitle', mode: 'track', trackId: track.id }); await controller.play();
      await until(() => controller.snapshot().subtitle.includes('Vesper')); results.push(JSON.stringify({ dashSubtitle: controller.snapshot() }));
      stage = 'dash-seek-out'; controller.drainEvents();
      await controller.seek(9000); results.push(JSON.stringify({ seekOut: controller.drainEvents() }));
      if (controller.snapshot().subtitle.length > 0) { throw new Error('DASH subtitle survived a seek outside its cue'); }
      stage = 'dash-seek-back';
      await controller.seek(500); results.push(JSON.stringify({ seekBack: controller.drainEvents() }));
      await until(() => controller.snapshot().subtitle.includes('Vesper'));
      await controller.selectTrack({ kind: 'subtitle', mode: 'off' });
      if (controller.snapshot().subtitle.length > 0) { throw new Error('DASH subtitle off retained text'); }
    } finally { subtitles.close(); }
  } catch (error) { throw new Error(JSON.stringify({ stage: stage, error: `${error}`, observations: results, events: controller.drainEvents(), playback: controller.snapshot() })); }
  finally { await controller.dispose(); sources.close(); }
  return JSON.stringify(results);
}
