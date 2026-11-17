import { VesperPlayerSnapshot } from './Types';
import { VesperHarmonyCapabilities, VesperTimeRanges } from './Platform';

export interface VesperOperationAvailability { status: string; reason: string; }
export interface VesperPlaybackView {
  sourceId?: string; playbackEpoch?: string; timeline: string;
  seek: VesperOperationAvailability; rate: VesperOperationAvailability;
  videoTracks: VesperOperationAvailability; audioTracks: VesperOperationAvailability;
  subtitles: VesperOperationAvailability; adaptiveBitrate: VesperOperationAvailability;
  timeRanges: VesperOperationAvailability; trackConstraints: VesperOperationAvailability;
  buffering: boolean; nativeBufferingPercent?: number; cachedDurationMs?: number;
  ranges?: VesperTimeRanges;
}

/** Platform availability and source applicability remain separate observations. */
export function resolvePlaybackView(snapshot: VesperPlayerSnapshot, platform: VesperHarmonyCapabilities,
  protocol: string): VesperPlaybackView {
  const ready = !snapshot.disposed && ['prepared', 'playing', 'paused', 'completed'].includes(snapshot.state);
  const available = (supported: boolean, reason: string): VesperOperationAvailability =>
    !ready ? { status: 'temporarilyUnavailable', reason: snapshot.disposed ? 'disposed' : 'playerNotReady' } :
      { status: supported ? 'supported' : 'unsupported', reason: supported ? 'available' : reason };
  const vod = snapshot.timeline === 'vod';
  const catalog = (kind: string): VesperOperationAvailability => available(
    snapshot.tracks.some((track): boolean => track.kind === kind && track.selectionSupported !== false), 'noSelectableTracks');
  return { sourceId: snapshot.sourceId, playbackEpoch: snapshot.playbackEpoch, timeline: snapshot.timeline,
    seek: available(vod && Number.isFinite(snapshot.durationMs) && (snapshot.durationMs ?? 0) > 0, vod ? 'durationUnknown' : 'timelineNotSeekable'),
    rate: available(vod, 'numericRateRequiresVod'), videoTracks: catalog('video'), audioTracks: catalog('audio'),
    subtitles: catalog('subtitle'), adaptiveBitrate: available(snapshot.availableBitrates.length > 0, 'noNativeBitrateCatalog'),
    timeRanges: platform.timeRanges ? available(true, '') : { status: 'unsupported', reason: 'requiresApi26' },
    trackConstraints: !platform.advancedTrackConstraints ? { status: 'unsupported', reason: 'requiresApi26' } :
      available(protocol === 'hls', 'filterApplicabilityUnverified'),
    buffering: snapshot.buffering, nativeBufferingPercent: snapshot.bufferingPercent,
    cachedDurationMs: snapshot.cachedDurationMs, ranges: snapshot.timeRanges };
}
