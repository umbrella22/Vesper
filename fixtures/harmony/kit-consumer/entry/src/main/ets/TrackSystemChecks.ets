import { common } from '@kit.AbilityKit';
import { fileIo as fs } from '@kit.CoreFileKit';
import { VesperPlayerController, VesperSourceSession, VesperPlayerSource, VesperSystemSession, VesperTrackConstraints } from '@vesper/player-kit';

async function until(ready: () => boolean, timeoutMs: number = 10000): Promise<void> {
  const start = Date.now();
  while (!ready()) { if (Date.now() - start > timeoutMs) { throw new Error('Track/system observation deadline'); } await new Promise<void>((resolve) => { setTimeout(resolve, 20); }); }
}

export async function runTrackSystemChecks(context: common.UIAbilityContext, surface: string): Promise<string> {
  const sources = new VesperSourceSession(); const controller = new VesperPlayerController(context); controller.attachSurface(surface);
  const results: string[] = []; let system: VesperSystemSession | undefined;
  const captions = ['1\n00:00:00,100 --> 00:00:08,000\nVesper SRT subtitle\n\n', 'WEBVTT\n\n00:00.100 --> 00:08.000\nVesper WebVTT subtitle\n\n'];
  let stage = 'subtitle';
  try {
    const local = `${context.filesDir}/subtitle-main.mp4`;
    const content = await context.resourceManager.getRawFileContent('probe.mp4');
    const video = await fs.open(local, fs.OpenMode.CREATE | fs.OpenMode.TRUNC | fs.OpenMode.WRITE_ONLY);
    try { await fs.write(video.fd, content.buffer); } finally { await fs.close(video); }
    for (let index = 0; index < captions.length; index++) {
      const path = `${context.filesDir}/caption.${index === 0 ? 'srt' : 'vtt'}`;
      const output = await fs.open(path, fs.OpenMode.CREATE | fs.OpenMode.TRUNC | fs.OpenMode.WRITE_ONLY);
      try { await fs.write(output.fd, captions[index]); } finally { await fs.close(output); }
      const source: VesperPlayerSource = { kind: 'local', protocol: 'file', label: 'Subtitles', uri: local,
        externalSubtitles: [{ id: 'captions-en', uri: path, mimeType: index === 0 ? 'application/x-subrip' : 'text/vtt', label: 'English' }] };
      const handle = sources.register(source);
      try {
        stage = `subtitle-activate-${index}`; await controller.activate(handle, { timeoutMs: 10000 });
        stage = `subtitle-track-${index}`;
        await until(() => controller.snapshot().tracks.some((track) => track.kind === 'subtitle'));
        const track = controller.snapshot().tracks.find((track) => track.kind === 'subtitle');
        if (track === undefined) { throw new Error('Subtitle track missing'); }
        await controller.selectTrack({ kind: 'subtitle', mode: 'track', trackId: track.id });
        stage = `subtitle-text-${index}`; await until(() => controller.snapshot().subtitle.includes('Vesper'));
        await controller.selectTrack({ kind: 'subtitle', mode: 'off' });
        if (controller.snapshot().subtitle !== '') { throw new Error('Disabled subtitle retained stale text'); }
        results.push(JSON.stringify({ subtitle: index === 0 ? 'srt' : 'webvtt', snapshot: controller.snapshot() }));
      } finally { handle.close(); }
    }
    stage = 'multiple-audio-tracks';
    const multiple = sources.register({ kind: 'remote', protocol: 'progressive', label: 'Audio selection',
      uri: 'http://127.0.0.1:18765/tracks.mp4', headers: { 'X-Vesper-Probe': 'h00' } });
    try {
      await controller.activate(multiple, { timeoutMs: 15000 }); await controller.pause();
      const tracks = controller.snapshot().tracks.filter((track) => track.kind === 'audio');
      const alternate = tracks.find((track) => !track.selected);
      const current = tracks.find((track) => track.selected);
      if (tracks.length < 2 || alternate === undefined || current === undefined) { throw new Error('Multiple native audio tracks were not exposed'); }
      const paused = controller.snapshot();
      await controller.selectTrack({ kind: 'audio', mode: 'track', trackId: alternate.id });
      const pending = controller.snapshot();
      if (pending.state !== 'paused' || pending.positionMs !== paused.positionMs || pending.playbackEpoch !== paused.playbackEpoch
        || pending.trackSelections?.audio?.outcome !== 'pending' || !pending.tracks.some((track) => track.id === current.id && track.selected)
        || pending.tracks.some((track) => track.id === alternate.id && track.selected)) { throw new Error('Paused audio intent changed playback or effective selection'); }
      await controller.selectTrack({ kind: 'audio', mode: 'track', trackId: current.id });
      const cancelled = controller.snapshot();
      if (cancelled.state !== 'paused' || cancelled.trackSelections?.audio?.outcome !== 'confirmed'
        || cancelled.trackSelections?.audio?.effective?.trackId !== current.id) { throw new Error('Selecting the current audio did not cancel the deferred intent'); }
      await controller.selectTrack({ kind: 'audio', mode: 'track', trackId: alternate.id });
      await controller.play();
      await until(() => controller.snapshot().trackSelections?.audio?.outcome === 'confirmed');
      await controller.pause();
      const confirmed = controller.snapshot();
      const selected = confirmed.tracks.filter((track) => track.kind === 'audio' && track.selected);
      if (confirmed.playbackEpoch !== paused.playbackEpoch || selected.length !== 1 || selected[0].id !== alternate.id
        || confirmed.trackSelections?.audio?.effective?.trackId !== alternate.id) { throw new Error('Resumed audio selection was not uniquely confirmed'); }
      results.push(JSON.stringify({ pausedAudio: pending, cancelledAudio: cancelled, multipleAudio: confirmed }));
    } finally { multiple.close(); }
    stage = 'hls-bitrate-selection';
    const bufferPolicy = await controller.setPlaybackPolicy({ buffering: { preferredBufferDurationMs: 2000, preferredBufferDurationForPlayingMs: 500 } });
    if (bufferPolicy.parameters.some((item) => item.status !== 'deferred' || item.effective !== undefined)) { throw new Error('Native buffer preference was reported as immediately effective'); }
    const adaptive = sources.register({ kind: 'remote', protocol: 'hls', label: 'Two HLS variants',
      uri: 'http://127.0.0.1:18765/abr/master.m3u8', headers: { 'X-Vesper-Probe': 'h00' } });
    try {
      await controller.activate(adaptive, { timeoutMs: 15000 });
      const nativeBuffering = controller.snapshot().nativeBuffering;
      if (nativeBuffering?.status !== 'accepted' || nativeBuffering.submitted?.preferredBufferDurationMs !== 2000
        || nativeBuffering.submitted?.preferredBufferDurationForPlayingMs !== 500) { throw new Error('MediaSource buffer preferences were not submitted at activation'); }
      const deferredBuffer = await controller.setPlaybackPolicy({ buffering: { preferredBufferDurationMs: 5000, preferredBufferDurationForPlayingMs: 1000 } });
      if (JSON.stringify(controller.snapshot().nativeBuffering) !== JSON.stringify(nativeBuffering)) { throw new Error('Deferred policy changed active buffer diagnostics'); }
      results.push(JSON.stringify({ nativeBuffering: nativeBuffering, nextActivationBuffering: deferredBuffer }));
      await until(() => controller.snapshot().availableBitrates.filter((value) => value > 0).length >= 2);
      stage = 'hls-resolution-filter';
      const before = controller.snapshot();
      const requested: VesperTrackConstraints = { minVideoResolution: { width: 160, height: 90 }, maxVideoResolution: { width: 160, height: 90 },
        preferredAudioLanguages: ['en'], preferredSubtitleLanguages: [] };
      const accepted = await controller.setTrackConstraints(requested);
      if (accepted.maxVideoResolution?.width !== 160 || accepted.maxVideoResolution.height !== 90
        || accepted.minVideoResolution?.width !== 160 || accepted.minVideoResolution.height !== 90
        || accepted.preferredAudioLanguages?.[0] !== 'en') { throw new Error('Native resolution/language filter readback differs from request'); }
      await until(() => controller.snapshot().width === 160 && controller.snapshot().height === 90, 45000);
      results.push(JSON.stringify({ resolutionFilter: 'accepted', requested: requested, accepted: accepted, before: before, observed: controller.snapshot() }));
      await controller.setTrackConstraints({});
      stage = 'hls-bitrate-selection';
      const bitrate = Math.min(...controller.snapshot().availableBitrates.filter((value) => value > 0));
      await controller.selectTrack({ kind: 'video', mode: 'bitrate', bitrate: bitrate });
      if (controller.snapshot().selection.effectiveBitrate !== bitrate) { throw new Error('Fixed native bitrate was not confirmed'); }
      await until(() => controller.snapshot().width === 160 && controller.snapshot().height === 90, 45000);
      const fixed = controller.snapshot();
      await controller.selectTrack({ kind: 'video', mode: 'auto' });
      results.push(JSON.stringify({ fixedBitrate: fixed, restoredAuto: controller.snapshot() }));
    } finally { adaptive.close(); }
    const systemSource = sources.register({ kind: 'remote', protocol: 'progressive', label: 'System playback',
      uri: 'http://127.0.0.1:18765/probe.mp4', headers: { 'X-Vesper-Probe': 'h00' } });
    try { await controller.activate(systemSource, { timeoutMs: 10000 }); } finally { systemSource.close(); }
    if (controller.snapshot().nativeBuffering?.submitted?.preferredBufferDurationMs !== 5000
      || controller.snapshot().trackConstraints !== undefined) { throw new Error('New activation did not apply deferred buffering or clear epoch-local track filters'); }
    stage = 'native-options';
    const policy = await controller.setPlaybackPolicy({ buffering: { minBufferMs: 1000 }, retry: { maxAttempts: 2 }, decoder: 'hardwareOnly' });
    if (policy.parameters.filter((item) => item.status === 'unsupported').length !== 2) { throw new Error('Policy unsupported results were lost'); }
    results.push(JSON.stringify({ policy: policy }));
    const capabilities = controller.capabilities();
    const effective = await controller.setTrackConstraints({ maxVideoBitrate: 2000000 });
    const ranges = await controller.timeRanges();
    await controller.seekToDefaultPosition();
    await controller.setTrackConstraints({});
    results.push(JSON.stringify({ capabilities: capabilities, effective: effective, ranges: ranges }));
    stage = 'system-create';
    system = await VesperSystemSession.create(context, controller, { bundleName: 'dev.vesper.harmony.kitconsumer',
      abilityName: 'EntryAbility', title: 'Vesper integration fixture', backgroundAudio: true });
    stage = 'system-pause'; await system.sendCommand('pause'); await until(() => controller.snapshot().state === 'paused');
    stage = 'system-play'; await system.sendCommand('play'); await until(() => controller.snapshot().state === 'playing');
    await until(() => system?.snapshot().background === 'active' || system?.snapshot().background === 'failed');
    if (system.snapshot().background !== 'active') { throw new Error(`Background service failed: ${JSON.stringify(system.snapshot())}`); }
    results.push(JSON.stringify({ system: system.snapshot() }));
    stage = 'system-close'; await system.close(); results.push(JSON.stringify({ systemClosed: system.snapshot() }));
  } catch (error) { throw new Error(JSON.stringify({ stage: stage, error: `${error}`, playback: controller.snapshot(), system: system?.snapshot() })); }
  finally { await system?.close(); await controller.dispose(); sources.close(); }
  return JSON.stringify(results);
}
