import { VesperPlayerController, VesperSourceSession, VesperPlaybackRecovery, VesperPlayerError } from '@vesper/player-kit';
import { rcp } from '@kit.RemoteCommunicationKit';

async function until(ready: () => boolean): Promise<void> {
  const deadline = Date.now() + 40000;
  while (!ready()) {
    if (Date.now() >= deadline) { throw new Error('Gap contract observation deadline'); }
    await new Promise<void>((resolve) => { setTimeout(resolve, 50); });
  }
}

/** Exercises the original MPDs through the production resource-loader route. */
export async function runProfileChecks(surface: string): Promise<string> {
  const sources = new VesperSourceSession(); const controller = new VesperPlayerController(); controller.attachSurface(surface);
  const results: string[] = []; let stage = '';
  try {
    for (const profile of ['template', 'timeline']) {
      stage = profile;
      const source = sources.register({ kind: 'remote', protocol: 'dash', label: 'Static ' + profile,
        uri: 'http://127.0.0.1:18765/' + profile + '/manifest.mpd', headers: { 'X-Vesper-Probe': 'h00' },
        externalSubtitles: [{ id: 'caption-geometry', uri: 'http://127.0.0.1:18765/caption-long.vtt', mimeType: 'text/vtt', label: 'Caption geometry', headers: { 'X-Vesper-Probe': 'h00' } }] });
      try {
        await controller.activate(source, { timeoutMs: 20000, playWhenReady: false });
        const track = controller.snapshot().tracks.find((item) => item.kind === 'subtitle');
        if (track === undefined) { throw new Error('The caption catalog is missing'); }
        await controller.selectTrack({ kind: 'subtitle', mode: 'track', trackId: track.id });
        await controller.play();
        await until(() => controller.snapshot().firstFrame && controller.snapshot().positionMs >= 200 && controller.snapshot().subtitle.length > 0);
        const rendered = controller.snapshot();
        await controller.seek(9000);
        if (controller.snapshot().subtitle.length > 0) { throw new Error('An expired cue survived seek'); }
        await controller.seek(1000);
        await until(() => controller.snapshot().subtitle.length > 0);
        await controller.selectTrack({ kind: 'subtitle', mode: 'off' });
        if (controller.snapshot().subtitle.length > 0) { throw new Error('Subtitle Off retained a cue'); }
        results.push(JSON.stringify({ profile: profile, rendered: rendered, final: controller.snapshot() }));
      } finally { source.close(); await controller.stop(); }
    }
    return JSON.stringify(results);
  } catch (error) { throw new Error(JSON.stringify({ stage: stage, error: `${error}`, observations: results, playback: controller.snapshot() })); }
  finally { await controller.dispose(); sources.close(); }
}

/** A failed resource request must recover through a new activation and a real rendered frame. */
export async function runRecoveryChecks(surface: string): Promise<string> {
  const sources = new VesperSourceSession(); const controller = new VesperPlayerController(); controller.attachSurface(surface);
  const source = sources.register({ kind: 'remote', protocol: 'progressive', label: 'Transient 503',
    uri: 'http://127.0.0.1:18765/recover-once/probe.mp4?case=' + Date.now(), headers: { 'X-Vesper-Probe': 'h00' } });
  await controller.setPlaybackPolicy({ retry: { maxAttempts: 1 } });
  const recovery = new VesperPlaybackRecovery(controller, (options, cancellation) => controller.activate(source, options, cancellation),
    { maxAttempts: 2, deadlineMs: 30000, backoffMs: 200 });
  let initialCode = '';
  try {
    try { await controller.activate(source, { timeoutMs: 15000 }); } catch (error) { initialCode = (error as VesperPlayerError).code; }
    await until(() => ['recovered', 'exhausted', 'hostActionRequired', 'sourceRefreshRequired'].includes(recovery.snapshot().state));
    const recovered = recovery.snapshot(); const playback = controller.snapshot();
    if (recovered.state !== 'recovered' || recovered.attempts < 1 || recovered.playbackEpoch === recovered.originalEpoch ||
      !playback.firstFrame || playback.state !== 'playing' || playback.error !== undefined) {
      throw new Error(JSON.stringify({ initialCode: initialCode, recovery: recovered, playback: playback }));
    }
    await until(() => controller.snapshot().positionMs >= 200);
    return JSON.stringify({ initialCode: initialCode, recovery: recovered, playback: controller.snapshot(),
      scope: 'initial-resource-failure-and-rendered-recovery', midStreamPositionRestore: 'host-contract-only' });
  } finally { recovery.close(); await controller.dispose(); source.close(); sources.close(); }
}

/** Arm a single real segment failure only after nondefault playback choices exist. */
export async function runRecoveryIntentChecks(surface: string): Promise<string> {
  const sources = new VesperSourceSession(); const controller = new VesperPlayerController(); controller.attachSurface(surface);
  const faultCase = Date.now().toString();
  const source = sources.register({ kind: 'remote', protocol: 'hls', label: 'Midstream recovery',
    uri: 'http://127.0.0.1:18765/fault-once/' + faultCase + '/abr/master.m3u8' });
  let recovery: VesperPlaybackRecovery | undefined; let stage = 'activate';
  try {
    await controller.setPlaybackPolicy({ retry: { maxAttempts: 1 } });
    await controller.activate(source, { timeoutMs: 15000 });
    await until(() => controller.snapshot().firstFrame && controller.snapshot().availableBitrates.length >= 2);
    stage = 'nondefault-choices';
    await controller.setTrackConstraints({ maxVideoBitrate: 1200000 });
    const bitrate = Math.min(...controller.snapshot().availableBitrates.filter((value) => value > 0));
    await controller.selectTrack({ kind: 'video', mode: 'bitrate', bitrate: bitrate });
    await controller.selectTrack({ kind: 'subtitle', mode: 'off' });
    await controller.setPlaybackRate(1.5); await controller.setVolume(0.6); await controller.setMuted(true);
    await until(() => controller.snapshot().positionMs >= 3000);
    const before = controller.snapshot();
    recovery = new VesperPlaybackRecovery(controller, (options, cancellation) => controller.activate(source, options, cancellation),
      { maxAttempts: 2, deadlineMs: 30000, backoffMs: 200 });
    stage = 'arm-segment-fault';
    const session = rcp.createSession();
    try {
      const request = new rcp.Request('http://127.0.0.1:18765/fault-control/' + faultCase + '/arm');
      request.configuration = { transfer: { timeout: { connectMs: 5000, transferMs: 5000 } } };
      const response = await session.fetch(request);
      if (response.statusCode !== 200) { throw new Error('Could not arm fixture fault'); }
    } finally { session.close(); }
    stage = 'recovery-and-restoration';
    await until(() => controller.snapshot().state === 'completed' ||
      ['recovered', 'recoveredPartial', 'exhausted', 'hostActionRequired', 'sourceRefreshRequired'].includes(recovery?.snapshot().state ?? ''));
    const state = recovery.snapshot(); const after = controller.snapshot();
    if (state.attempts === 0 && after.state === 'completed' && after.error === undefined &&
      after.playbackEpoch === before.playbackEpoch && after.resourceLoaderInfo?.includes('http_503') &&
      after.trackSelections?.video?.effective?.bitrate === bitrate && after.trackSelections?.subtitle?.effective?.mode === 'off') {
      return JSON.stringify({ faultCase: faultCase, before: before, after: after, recovery: state,
        scope: 'native-resource-failure-retried-by-media-kit', sdkRestartReached: false,
        sdkChoiceRestoration: 'not-exercised-native-player-retained-the-epoch' });
    }
    if (state.state !== 'recovered' || state.attempts < 1 || after.playbackEpoch === before.playbackEpoch ||
      after.trackSelections?.video?.effective?.bitrate !== bitrate || after.trackSelections?.subtitle?.effective?.mode !== 'off'
      || !after.firstFrame || after.state !== 'playing' || after.playbackRate !== 1.5 || !after.muted || after.volume !== 0.6
      || after.trackConstraints?.requested?.maxVideoBitrate !== 1200000) {
      throw new Error(JSON.stringify({ before: before, after: after, recovery: state }));
    }
    return JSON.stringify({ faultCase: faultCase, before: before, after: after, recovery: state,
      scope: 'native-midstream-fault-bitrate-off-filter-position-rate-volume', nativeAudioRematching: 'host-contract-only' });
  } catch (error) { throw new Error(JSON.stringify({ stage: stage, error: `${error}`, playback: controller.snapshot(), recovery: recovery?.snapshot(), faultCase: faultCase })); }
  finally { recovery?.close(); await controller.dispose(); source.close(); sources.close(); }
}
