import { media } from '@kit.MediaKit';
import { VesperPlayerController, VesperSourceSession, VesperBufferingPresets, queryVideoDecoderCapability } from '@vesper/player-kit';
import { runNativeDvrChecks } from './DvrChecks';
import { prepareAndPlayNative } from './NativePlayerChecks';

async function until(ready: () => boolean, timeoutMs: number = 15000): Promise<void> {
  const start = Date.now();
  while (!ready()) {
    if (Date.now() - start > timeoutMs) { throw new Error('Capability observation deadline'); }
    await new Promise<void>((resolve) => { setTimeout(resolve, 50); });
  }
}
interface NativeObservation {
  stage: string; currentTime: number; duration: number; position?: number; ptsUs?: number;
  loaded?: media.Range[]; seekable?: media.Range[]; error?: string; seekDone?: number; nativeError?: string;
}

/** Observes native DASH audio switching without SDK source or resource routing. */
export async function runNativeDashAudioChecks(surface: string): Promise<string[]> {
  const results: string[] = [];
  for (const seekFirst of [false, true]) {
    const player = await media.createAVPlayer();
    let rendered = false; let seekDone = -1; let nativeError = ''; let selectedEvent = -1;
    const events: string[] = [];
    player.on('startRenderFrame', (): void => { rendered = true; });
    player.on('seekDone', (position: number): void => { seekDone = position; });
    player.on('error', (error): void => { nativeError = `${error.code}:${error.message}`; });
    player.on('trackChange', (index: number, selected: boolean): void => {
      if (selected) { selectedEvent = index; }
      if (events.length < 32) { events.push(`${index}:${selected}`); }
    });
    try {
      player.url = 'http://127.0.0.1:18765/template/manifest.mpd';
      await until(() => player.state === 'initialized'); player.surfaceId = surface;
      await prepareAndPlayNative(player); await until(() => rendered);
      if (seekFirst) {
        player.seek(4000, media.SeekMode.SEEK_CLOSEST); await until(() => seekDone >= 0);
        player.setPlaybackRate(1.25);
      }
      const before = await player.getSelectedTracks();
      const tracks = await player.getTrackDescription();
      const alternate = tracks.find((track) => track['track_type'] === 0 && !before.includes(track['track_index'] as number));
      if (alternate === undefined) { throw new Error('Native alternate audio track is missing'); }
      const target = alternate['track_index'] as number;
      let returned = false; let selectionError = '';
      player.selectTrack(target).then(() => { returned = true; }).catch((error: Error) => { returned = true; selectionError = `${error}`; });
      try { await until(() => returned && (selectedEvent === target || selectionError.length > 0), 7000); }
      catch (error) { selectionError = `${error}`; }
      const after = await player.getSelectedTracks();
      results.push(JSON.stringify({ observation: 'native-dash-audio', seekFirst: seekFirst, tracks: tracks,
        target: target, before: before, after: after, returned: returned, selectionError: selectionError,
        selectionConfirmed: selectionError.length === 0 && after.includes(target), events: events,
        currentTime: player.currentTime, state: player.state, nativeError: nativeError }));
    } finally { await player.release(); }
  }
  return results;
}

/** Records direct-native live coordinates before enabling any SDK DVR contract. */
export async function runCapabilityChecks(surface: string): Promise<string[]> {
  const results: string[] = [];
  results.push(JSON.stringify({ hardware: queryVideoDecoderCapability({ mimeType: 'video/avc', width: 320, height: 180 }) }));
  const controller = new VesperPlayerController(); controller.attachSurface(surface);
  const sources = new VesperSourceSession();
  const presets = [VesperBufferingPresets.fastStart(), VesperBufferingPresets.balanced(), VesperBufferingPresets.resilient()];
  for (let index = 0; index < presets.length; index++) {
    const source = sources.register({ kind: 'remote', protocol: 'progressive', label: 'Buffer observation', uri: 'http://127.0.0.1:18765/probe.mp4' });
    try {
      await controller.setPlaybackPolicy({ buffering: presets[index] });
      await controller.activate(source, { timeoutMs: 15000 }); await until(() => controller.snapshot().firstFrame);
      const submitted = controller.snapshot().nativeBuffering?.submitted;
      if (JSON.stringify(submitted) !== JSON.stringify(presets[index])) { throw new Error('Buffer preset was not submitted at activation'); }
      await controller.setPlaybackPolicy({ buffering: {} });
      if (JSON.stringify(controller.snapshot().nativeBuffering?.submitted) !== JSON.stringify(submitted)) { throw new Error('Future buffer policy changed an active native player'); }
      await new Promise<void>((resolve) => { setTimeout(resolve, 1500); });
      results.push(JSON.stringify({ bufferPreset: index, playback: controller.snapshot(), diagnostics: await controller.refreshDiagnostics() }));
      await controller.stop();
    } finally { source.close(); }
  }
  const liveUri = `http://127.0.0.1:18765/dvr/media.m3u8?session=${Date.now()}`;
  if (controller.capabilities().runtimeApi >= 26) { results.push(...await runNativeDvrChecks(surface)); }
  const handle = sources.register({ kind: 'remote', protocol: 'hls', label: 'Controlled DVR', uri: liveUri });
  try {
    await controller.activate(handle, { timeoutMs: 20000 }); await until(() => controller.snapshot().firstFrame);
    results.push(JSON.stringify({ sdkFirst: controller.snapshot(), diagnostics: await controller.refreshDiagnostics() }));
    await new Promise<void>((resolve) => { setTimeout(resolve, 5000); });
    if (controller.capabilities().runtimeApi >= 26) { results.push(JSON.stringify({ sdkRanges: await controller.timeRanges() })); }
    results.push(JSON.stringify({ sdkAfter: controller.snapshot(), diagnostics: await controller.refreshDiagnostics() }));
    await controller.pause(); await controller.play();
  } finally { handle.close(); await controller.dispose(); sources.close(); }
  return results;
}
