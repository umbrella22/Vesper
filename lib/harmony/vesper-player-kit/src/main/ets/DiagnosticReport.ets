import { deviceInfo } from '@kit.BasicServicesKit';
import { VesperPlayerError, VesperPlayerSnapshot } from './Types';
import { VesperTimeRange } from './Platform';
import { VesperPlaybackDiagnostics } from './Diagnostics';
import { VESPER_HARMONY_VERSION, VESPER_HARMONY_SOURCE_SHA256 } from './BuildIdentity';

export interface VesperDiagnosticReportOptions {
  scenario?: string; fixtureId?: string; applicationVersion?: string; applicationBuildIdentity?: string;
  runtimeKind?: string; audibleOutputObservation?: string;
}
export interface VesperDiagnosticRequest {
  requestId: number; atMonotonicMs: number; durationMs: number; status?: number;
  rangeStart?: number; rangeEnd?: number; receivedBytes: number; outcome: string;
}
export interface VesperDiagnosticMoment {
  type: string; atMonotonicMs: number; playbackEpoch?: string; state: string;
  positionMs: number; requestedPositionMs?: number; durationMs?: number; ptsUs?: number;
  ptsSampledAtMonotonicMs?: number;
  buffering: boolean; firstFrame: boolean; selectedTrackIds: number[];
  playbackRate: number; selectionKind?: string; requestedTrackId?: number; selectionOutcome?: string;
  selectionErrorCode?: string;
  loaded: VesperTimeRange[]; seekable: VesperTimeRange[]; errorCode?: string; nativeCode?: number;
}
export interface VesperDiagnosticTrack {
  id: number; kind: string; mimeType: string; language: string; selected: boolean;
  width?: number; height?: number; bitrate?: number;
}
export interface VesperDiagnosticReport {
  schemaVersion: number; sdkVersion: string; sdkSourceSha256: string;
  applicationVersion: string; applicationBuildIdentity: string;
  runtimeApi: number; osFullName: string; displayVersion: string; model: string; deviceType: string;
  runtimeKind: string; runtimeKindEvidence: string; scenario: string; fixtureId: string; protocol: string;
  tracks: VesperDiagnosticTrack[]; latest: VesperDiagnosticMoment; diagnostics?: VesperPlaybackDiagnostics;
  timeline: VesperDiagnosticMoment[]; requests: VesperDiagnosticRequest[];
  droppedMoments: number; droppedRequests: number; omittedTracks: number;
  requestCoverage: string; actualDecoder: string; audibleOutputObservation: string; observationEvidence: string;
}

function publicIdentity(value: string | undefined): string {
  if (value === undefined) { return 'unknown'; }
  if (!/^[a-zA-Z0-9][a-zA-Z0-9._+-]{0,95}$/.test(value)) {
    throw new VesperPlayerError('invalid_argument', 'Diagnostic identifiers require a public version or fixture identifier, without URLs or paths');
  }
  return value;
}
/** Error messages, source labels, URIs, headers and subtitle text never enter reports. */
export function diagnosticCode(value: string | undefined): string | undefined {
  return value === undefined ? undefined : /^[a-zA-Z][a-zA-Z0-9_]{0,63}$/.test(value) ? value : 'redacted';
}
function ranges(values: VesperTimeRange[] | undefined): VesperTimeRange[] {
  return (values ?? []).slice(0, 8).map((range: VesperTimeRange): VesperTimeRange => ({ startMs: range.startMs, endMs: range.endMs }));
}
function moment(type: string, value: VesperPlayerSnapshot, at: number, target?: number): VesperDiagnosticMoment {
  return { type: type, atMonotonicMs: at, playbackEpoch: value.playbackEpoch, state: value.state,
    positionMs: value.positionMs, requestedPositionMs: target, durationMs: value.durationMs,
    ptsUs: value.diagnostics?.presentationTimestampUs, ptsSampledAtMonotonicMs: value.diagnostics?.presentationTimestampUs === undefined ? undefined : value.diagnostics?.sampledAtMonotonicMs,
    buffering: value.buffering, firstFrame: value.firstFrame,
    playbackRate: value.playbackRate, selectionKind: diagnosticCode(value.selection?.requested?.kind),
    requestedTrackId: value.selection?.requested?.trackId, selectionOutcome: diagnosticCode(value.selection?.outcome),
    selectionErrorCode: diagnosticCode(value.selection?.errorCode),
    selectedTrackIds: value.tracks.filter((track): boolean => track.selected).slice(0, 16).map((track): number => track.id),
    loaded: ranges(value.timeRanges?.loaded), seekable: ranges(value.timeRanges?.seekable),
    errorCode: diagnosticCode(value.error?.code), nativeCode: value.error?.nativeCode };
}

/** Controller-local history: 128 moments, one position sample per second, no background timer. */
export class DiagnosticHistory {
  private moments: VesperDiagnosticMoment[] = [];
  private dropped: number = 0;
  private lastPositionAt: number = -1000;
  record(type: string, value: VesperPlayerSnapshot, at: number, target?: number): void {
    if (type === 'position') {
      if (at - this.lastPositionAt < 1000) { return; }
      this.lastPositionAt = at;
    }
    if (this.moments.length >= 128) { this.moments.shift(); this.dropped++; }
    this.moments.push(moment(type, value, at, target));
  }
  report(value: VesperPlayerSnapshot, protocol: string, at: number, requests: VesperDiagnosticRequest[],
    droppedRequests: number, options: VesperDiagnosticReportOptions = {}): VesperDiagnosticReport {
    const kind = options.runtimeKind ?? 'unknown';
    const audible = options.audibleOutputObservation ?? 'not-assessed';
    if (!['unknown', 'simulator', 'device'].includes(kind) || !['not-assessed', 'audible', 'silent', 'intermittent'].includes(audible)) {
      throw new VesperPlayerError('invalid_argument', 'Unknown diagnostic environment or human observation');
    }
    const report: VesperDiagnosticReport = {
      schemaVersion: 1, sdkVersion: VESPER_HARMONY_VERSION, sdkSourceSha256: VESPER_HARMONY_SOURCE_SHA256,
      applicationVersion: publicIdentity(options.applicationVersion), applicationBuildIdentity: publicIdentity(options.applicationBuildIdentity),
      runtimeApi: deviceInfo.sdkApiVersion, osFullName: deviceInfo.osFullName, displayVersion: deviceInfo.displayVersion,
      model: deviceInfo.productModel, deviceType: deviceInfo.deviceType, runtimeKind: kind,
      runtimeKindEvidence: kind === 'unknown' ? 'unavailable' : 'host-reported',
      scenario: publicIdentity(options.scenario), fixtureId: publicIdentity(options.fixtureId),
      protocol: ['file', 'progressive', 'hls', 'dash'].includes(protocol) ? protocol : 'unknown',
      tracks: value.tracks.slice(0, 64).map((track): VesperDiagnosticTrack => ({ id: track.id,
        kind: ['audio', 'video', 'subtitle'].includes(track.kind) ? track.kind : 'unknown',
        mimeType: /^(audio|video|text|application)\/[a-zA-Z0-9.+-]{1,80}$/.test(track.mimeType) ? track.mimeType : 'unknown',
        language: /^[a-zA-Z]{2,8}(-[a-zA-Z0-9]{1,8})*$/.test(track.language) && track.language.length <= 64 ? track.language : 'unknown',
        selected: track.selected, width: track.width, height: track.height, bitrate: track.bitrate })),
      latest: moment('snapshot', value, at), diagnostics: value.diagnostics === undefined ? undefined : {
        playbackEpoch: value.diagnostics.playbackEpoch, sampledAtMonotonicMs: value.diagnostics.sampledAtMonotonicMs,
        buffer: { status: value.diagnostics.buffer.status, durationMs: value.diagnostics.buffer.durationMs, errorCode: diagnosticCode(value.diagnostics.buffer.errorCode) },
        metrics: { status: value.diagnostics.metrics.status, errorCode: diagnosticCode(value.diagnostics.metrics.errorCode),
          prepareDurationMs: value.diagnostics.metrics.prepareDurationMs, resourceConnectionDurationMs: value.diagnostics.metrics.resourceConnectionDurationMs,
          firstFrameDecapsulationDurationMs: value.diagnostics.metrics.firstFrameDecapsulationDurationMs,
          playingDurationMs: value.diagnostics.metrics.playingDurationMs, stallCount: value.diagnostics.metrics.stallCount,
          stallDurationMs: value.diagnostics.metrics.stallDurationMs, nativeDownloadRequests: value.diagnostics.metrics.nativeDownloadRequests,
          nativeDownloadDurationMs: value.diagnostics.metrics.nativeDownloadDurationMs, nativeDownloadBytes: value.diagnostics.metrics.nativeDownloadBytes,
          unrecognizedKeys: value.diagnostics.metrics.unrecognizedKeys.map((key: string): string => diagnosticCode(key) ?? 'redacted') },
        nativeDownloadScope: value.diagnostics.nativeDownloadScope, nativeDownloadRateBitsPerSecond: value.diagnostics.nativeDownloadRateBitsPerSecond,
        nativeAverageDownloadRateBitsPerSecond: value.diagnostics.nativeAverageDownloadRateBitsPerSecond,
        upstreamRequestsSinceControllerCreation: value.diagnostics.upstreamRequestsSinceControllerCreation,
        upstreamBytesSinceControllerCreation: value.diagnostics.upstreamBytesSinceControllerCreation,
        decoderExecutionEvidence: 'unknown', nativePositionMs: value.diagnostics.nativePositionMs,
        presentationTimestampUs: value.diagnostics.presentationTimestampUs, positionStatus: value.diagnostics.positionStatus,
        presentationTimestampStatus: value.diagnostics.presentationTimestampStatus, timeRangesStatus: value.diagnostics.timeRangesStatus },
      timeline: this.moments.filter((entry: VesperDiagnosticMoment): boolean => entry.playbackEpoch === value.playbackEpoch),
      requests: requests, droppedMoments: this.dropped, droppedRequests: droppedRequests,
      omittedTracks: Math.max(0, value.tracks.length - 64), requestCoverage: 'controller-transport-only; AVPlayer-internal requests unavailable',
      actualDecoder: 'unknown', audibleOutputObservation: audible,
      observationEvidence: audible === 'not-assessed' ? 'unavailable' : 'human-reported'
    };
    // All variable arrays have field-specific limits; detach the returned report from retained history.
    const serialized = JSON.stringify(report);
    if (serialized.length > 128 * 1024) { throw new VesperPlayerError('capacity', 'Diagnostic report exceeds its 512 KiB UTF-8 export budget'); }
    return JSON.parse(serialized) as VesperDiagnosticReport;
  }
}
