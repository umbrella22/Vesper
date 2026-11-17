import { VesperPlayerController, VesperPlayerError } from '@vesper/player-kit';

export interface ExampleDiagnosticResult { scenario: string; outcome: string; errorCode?: string; }

/** Presets operate on the selected video and never substitute an unrelated fixture. */
export async function runDiagnosticScenario(controller: VesperPlayerController, scenario: string,
  check: () => void): Promise<ExampleDiagnosticResult> {
  if (!['progress', 'alternate-audio', 'tail-completion', 'live-dvr'].includes(scenario)) {
    throw new VesperPlayerError('invalid_argument', 'Unknown diagnostic scenario');
  }
  try {
    check(); await controller.play(); check(); await controller.refreshDiagnostics(); check();
    const before = controller.snapshot(); const epoch = before.playbackEpoch;
    if (scenario === 'alternate-audio') {
      const target = before.tracks.find((track): boolean => track.kind === 'audio' && !track.selected);
      if (target === undefined) { throw new VesperPlayerError('unsupported', 'The selected video has no alternate audio track'); }
      await controller.selectTrack({ kind: 'audio', mode: 'track', trackId: target.id }); check();
    } else if (scenario === 'tail-completion') {
      if (before.timeline !== 'vod' || before.durationMs === undefined) { throw new VesperPlayerError('unsupported', 'Tail testing requires a finite VOD duration'); }
      await controller.seek(Math.max(0, before.durationMs - 15000)); check();
    } else if (scenario === 'live-dvr') {
      // The public SDK gate remains authoritative until the native DVR contract is verified.
      const ranges = await controller.timeRanges(); check();
      const window = ranges.seekable[ranges.seekable.length - 1];
      if (before.timeline !== 'live' || window === undefined || window.endMs - window.startMs < 10000) {
        throw new VesperPlayerError('unsupported', 'The selected live source has no sufficiently wide DVR window');
      }
      await controller.seek(Math.round((window.startMs + window.endMs) / 2)); check();
      throw new VesperPlayerError('unsupported', 'Returning live requires verified rewind PTS and continuous progression');
    }
    for (let sample = 0; sample < (scenario === 'tail-completion' ? 35 : 4); sample++) {
      await new Promise<void>((resolve) => { setTimeout(resolve, 1000); }); check();
      const current = controller.snapshot();
      if (current.playbackEpoch !== epoch) { throw new VesperPlayerError('stale_epoch', 'Diagnostic playback was replaced'); }
      if (current.error !== undefined) { throw current.error; }
      if (current.state === 'completed') { return { scenario: scenario, outcome: 'completed' }; }
      await controller.refreshDiagnostics(); check();
    }
    const after = controller.snapshot();
    return { scenario: scenario, outcome: scenario === 'tail-completion' ? 'completion-unconfirmed' :
      scenario === 'alternate-audio' ? 'selection-confirmed-audio-unobserved' :
      after.positionMs > before.positionMs + 1000 ? 'progress-confirmed-audio-unobserved' : 'progress-unconfirmed' };
  } catch (error) {
    check(); const failure = error as VesperPlayerError;
    const code = /^[a-zA-Z][a-zA-Z0-9_]{0,63}$/.test(failure.code ?? '') ? failure.code : 'unknown';
    return { scenario: scenario, outcome: code === 'unsupported' ? 'unsupported' : 'failed', errorCode: code };
  }
}
