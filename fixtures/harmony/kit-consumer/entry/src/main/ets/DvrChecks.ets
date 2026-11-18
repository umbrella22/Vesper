import { media } from '@kit.MediaKit';
import { prepareAndPlayNative } from './NativePlayerChecks';

async function until(ready: () => boolean, timeoutMs: number = 12000): Promise<void> {
  const started = Date.now();
  while (!ready()) {
    if (Date.now() - started > timeoutMs) { throw new Error('DVR observation deadline'); }
    await new Promise<void>((resolve) => { setTimeout(resolve, 50); });
  }
}
interface DvrObservation {
  stage: string; startTag: boolean; at: number; state: string; currentTime: number; seekDone: number;
  nativePositionMs?: number; ptsUs?: number; loaded?: media.Range[]; seekable?: media.Range[]; error?: string;
}

/** The requested point must be inside the native window and behind the observed PTS. */
export function dvrRewindTarget(window: media.Range, ptsUs: number): number | undefined {
  if (!Number.isFinite(ptsUs) || !Number.isFinite(window.min) || !Number.isFinite(window.max) || window.max - window.min < 6000) { return undefined; }
  const lower = window.min + 1000;
  const upper = Math.min(window.max - 1000, ptsUs / 1000 - 1000);
  return upper > lower ? Math.round((lower + upper) / 2) : undefined;
}

/** Rewind is proven by PTS and progression before testing the native return-live command. */
export async function runNativeDvrChecks(surface: string): Promise<string[]> {
  const results: string[] = [];
  for (const startTag of [false, true]) {
    const player = await media.createAVPlayer(); let rendered = false; let seekDone = -1; let error = '';
    player.on('startRenderFrame', (): void => { rendered = true; });
    player.on('seekDone', (position: number): void => { seekDone = position; });
    player.on('error', (failure): void => { error = `${failure.code}:${failure.message}`; });
    const capture = async (stage: string): Promise<DvrObservation> => {
      const value: DvrObservation = { stage: stage, startTag: startTag, at: Date.now(), state: player.state,
        currentTime: player.currentTime, seekDone: seekDone, error: error };
      try {
        value.nativePositionMs = player.getPlaybackPosition(); value.ptsUs = player.getCurrentPresentationTimestamp();
        value.loaded = await player.getLoadedTimeRanges(); value.seekable = await player.getSeekableTimeRanges();
      } catch (failure) { value.error = `${failure}`; }
      results.push(JSON.stringify(value)); return value;
    };
    try {
      player.url = `http://127.0.0.1:18765/dvr/media.m3u8?session=dvr-${Date.now()}&start=${startTag ? 1 : 0}`;
      await until(() => player.state === 'initialized'); player.surfaceId = surface;
      await prepareAndPlayNative(player); await until(() => rendered);
      await new Promise<void>((resolve) => { setTimeout(resolve, 5000); });
      const before = await capture('rewind-request');
      const window = before.seekable?.[before.seekable.length - 1];
      if (window === undefined || window.max - window.min < 6000 || before.ptsUs === undefined) {
        results.push(JSON.stringify({ stage: 'dvr-contract', startTag: startTag, outcome: 'insufficient-observable-window', defaultAttempted: false })); continue;
      }
      const target = dvrRewindTarget(window, before.ptsUs);
      if (target === undefined) {
        results.push(JSON.stringify({ stage: 'dvr-contract', startTag: startTag, outcome: 'no-interior-rewind-point', defaultAttempted: false })); continue;
      }
      results.push(JSON.stringify({ stage: 'rewind-target', startTag: startTag, targetMs: target, window: window }));
      seekDone = -1; player.seek(target, media.SeekMode.SEEK_CLOSEST);
      await until(() => seekDone >= 0, 6000);
      await new Promise<void>((resolve) => { setTimeout(resolve, 300); });
      const completed = await capture('rewind-completion');
      await new Promise<void>((resolve) => { setTimeout(resolve, 2500); });
      const progressing = await capture('rewind-progression');
      const rewind = completed.ptsUs !== undefined && Math.abs(completed.ptsUs / 1000 - target) <= 1000
        && completed.ptsUs < before.ptsUs - 500000;
      const progression = completed.ptsUs !== undefined && progressing.ptsUs !== undefined && progressing.ptsUs > completed.ptsUs + 1000000;
      if (!rewind || !progression) {
        results.push(JSON.stringify({ stage: 'dvr-contract', startTag: startTag, rewindConfirmed: rewind,
          progressionConfirmed: progression, targetToleranceMs: 1000, defaultAttempted: false, outcome: 'rewind-unconfirmed' })); continue;
      }
      await capture('return-live-request'); seekDone = -1; player.seekToDefaultPosition();
      await until(() => seekDone >= 0 || error.length > 0, 6000);
      const returned = await capture('return-live-completion');
      await new Promise<void>((resolve) => { setTimeout(resolve, 2500); });
      const final = await capture('return-live-progression');
      const edge = final.seekable?.[final.seekable.length - 1];
      const confirmed = error.length === 0 && returned.ptsUs !== undefined && final.ptsUs !== undefined
        && final.ptsUs > returned.ptsUs + 1000000 && edge !== undefined
        && final.ptsUs / 1000 >= edge.max - 10000 && final.ptsUs / 1000 <= edge.max + 2000;
      results.push(JSON.stringify({ stage: 'dvr-contract', startTag: startTag, rewindConfirmed: true,
        progressionConfirmed: true, defaultAttempted: true, returnLiveConfirmed: confirmed }));
    } catch (failure) { results.push(JSON.stringify({ stage: 'dvr-error', startTag: startTag, error: `${failure}`, nativeError: error })); }
    finally { await player.release(); }
  }
  return results;
}
