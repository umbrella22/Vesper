import { media } from '@kit.MediaKit';

export interface VesperBufferObservation { status: string; durationMs?: number; errorCode?: string; }
export interface VesperNativePlaybackMetrics {
  status: string; errorCode?: string; prepareDurationMs?: number; resourceConnectionDurationMs?: number;
  firstFrameDecapsulationDurationMs?: number; playingDurationMs?: number; stallCount?: number; stallDurationMs?: number;
  nativeDownloadRequests?: number; nativeDownloadDurationMs?: number; nativeDownloadBytes?: number;
  unrecognizedKeys: string[];
}
export interface VesperPlaybackDiagnostics {
  playbackEpoch: string; sampledAtMonotonicMs: number; buffer: VesperBufferObservation;
  metrics: VesperNativePlaybackMetrics; nativeDownloadScope: string;
  nativeDownloadRateBitsPerSecond?: number; nativeAverageDownloadRateBitsPerSecond?: number;
  upstreamRequestsSinceControllerCreation: number; upstreamBytesSinceControllerCreation: number;
  decoderExecutionEvidence: string;
  nativePositionMs?: number; presentationTimestampUs?: number; positionStatus?: string; presentationTimestampStatus?: string; timeRangesStatus?: string;
}

function nonnegative(values: Record<string, Object>, key: string): number | undefined {
  const value = values[key]; return typeof value === 'number' && Number.isFinite(value) && value >= 0 ? value : undefined;
}

/** PlaybackInfo buffer duration is seconds; native download rates are bits/s. */
export function playbackInfoObservation(info: media.PlaybackInfo, result: VesperPlaybackDiagnostics): void {
  const seconds = nonnegative(info, 'buffer_duration');
  result.buffer = seconds === undefined ? { status: 'unavailable' } : { status: 'observed', durationMs: seconds * 1000 };
  result.nativeDownloadRateBitsPerSecond = nonnegative(info, 'download_rate');
  result.nativeAverageDownloadRateBitsPerSecond = nonnegative(info, 'average_download_rate');
}

/** Unknown metric names remain visible without persisting arbitrary native values. */
export function playbackMetricsObservation(metrics: media.PlaybackMetrics): VesperNativePlaybackMetrics {
  const values = metrics as Record<string, Object>;
  const known: string[] = ['prepare_duration', 'resource_connection_duration', 'first_frame_decapsulation_duration',
    'total_playback_time', 'loading_requests_count', 'total_loading_time', 'total_loading_bytes',
    'stalling_count', 'total_stalling_time', 'lip_async_count', 'total_lip_async_time'];
  return { status: 'observed', prepareDurationMs: nonnegative(values, 'prepare_duration'),
    resourceConnectionDurationMs: nonnegative(values, 'resource_connection_duration'),
    firstFrameDecapsulationDurationMs: nonnegative(values, 'first_frame_decapsulation_duration'),
    playingDurationMs: nonnegative(values, 'total_playback_time'), stallCount: nonnegative(values, 'stalling_count'),
    stallDurationMs: nonnegative(values, 'total_stalling_time'), nativeDownloadRequests: nonnegative(values, 'loading_requests_count'),
    nativeDownloadDurationMs: nonnegative(values, 'total_loading_time'), nativeDownloadBytes: nonnegative(values, 'total_loading_bytes'),
    unrecognizedKeys: Object.keys(values).filter((key: string): boolean => !known.includes(key)).slice(0, 32) };
}
