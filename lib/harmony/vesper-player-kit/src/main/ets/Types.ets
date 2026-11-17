import { VesperPlaybackPolicyResult, VesperNativeBufferingState } from './Policy';
import { VesperTimeRanges } from './Platform';
import { VesperTrackPreferenceResult } from './TrackPreferences';
import { VesperTrackConstraintResult } from './TrackConstraints';
import { VesperPlaybackDiagnostics } from './Diagnostics';

interface ErrorDetails { name: string; message: string; code: string; nativeCode?: number; }
export class VesperPlayerError extends Error {
  readonly code: string;
  readonly nativeCode?: number;
  constructor(code: string, message: string, nativeCode?: number) {
    super(message); this.name = 'VesperPlayerError'; this.code = code; this.nativeCode = nativeCode;
  }
  toJSON(): ErrorDetails { return { name: this.name, message: this.message, code: this.code, nativeCode: this.nativeCode }; }
}

export interface VesperExternalSubtitleSource {
  id: string;
  uri: string;
  label: string;
  mimeType: string;
  language?: string;
  headers?: Record<string, string>;
  isDefault?: boolean;
  isForced?: boolean;
}

export interface VesperPlayerSource {
  uri: string;
  label: string;
  kind: string;
  protocol: string;
  headers?: Record<string, string>;
  drmConfiguration?: Object;
  externalSubtitles?: VesperExternalSubtitleSource[];
}

export interface VesperSourceActivationOptions {
  playWhenReady?: boolean;
  startPositionMs?: number;
  playbackRate?: number;
  timeoutMs?: number;
}

export interface VesperSourceActivationResult {
  activationId: string;
  sessionId: string;
  sourceId: string;
  playbackEpoch: string;
}

export interface VesperTrack {
  id: number;
  stableId?: string;
  label?: string;
  external?: boolean;
  isDefault?: boolean;
  isForced?: boolean;
  kind: string;
  nativeType: number;
  mimeType: string;
  language: string;
  selected: boolean;
  width?: number;
  height?: number;
  bitrate?: number;
  selectionSupported?: boolean;
  selectionReason?: string;
}

export interface VesperTrackSelection {
  kind: string;
  mode: string;
  trackId?: number;
  bitrate?: number;
}

export interface VesperSelectionState {
  requested?: VesperTrackSelection;
  confirmedTrackIds: number[];
  effectiveBitrate?: number;
  outcome?: string;
  effective?: VesperTrackSelection;
  errorCode?: string;
}
export interface VesperTrackSelectionStates { audio?: VesperSelectionState; video?: VesperSelectionState; subtitle?: VesperSelectionState; }
export interface VesperSubtitleError { stage: string; code: string; sourceId?: string; }
export interface VesperExternalSubtitleState { id: string; state: string; error?: VesperSubtitleError; }
export interface VesperSubtitleState {
  playbackEpoch?: string; catalogState: string; selectionState: string;
  selectedTrackId?: number; selectedStableId?: string;
  catalogError?: VesperSubtitleError; selectionError?: VesperSubtitleError;
  externalTracks: VesperExternalSubtitleState[];
}

export interface VesperPlayerSnapshot {
  disposed: boolean;
  state: string;
  sourceId?: string;
  sessionId?: string;
  activationId?: string;
  playbackEpoch?: string;
  positionMs: number;
  durationMs?: number;
  timeline: string;
  playbackRate: number;
  volume: number;
  muted: boolean;
  playWhenReady: boolean;
  intentRevision?: number;
  firstFrame: boolean;
  firstFrameMs?: number;
  buffering: boolean;
  bufferingPercent?: number;
  cachedDurationMs?: number;
  timeRanges?: VesperTimeRanges;
  lastBufferingUpdate?: VesperBufferingUpdate;
  width: number;
  height: number;
  subtitle: string;
  subtitleState?: VesperSubtitleState;
  tracks: VesperTrack[];
  selection: VesperSelectionState;
  trackSelections?: VesperTrackSelectionStates;
  trackPreferences?: VesperTrackPreferenceResult;
  trackConstraints?: VesperTrackConstraintResult;
  availableBitrates: number[];
  cacheBytes: number;
  loaderBytes: number;
  activeResources: number;
  resourceLoaderInfo?: string;
  releasedPlayers: number;
  retainedNativePlayers: number;
  listenerFailures: number;
  droppedEvents: number;
  audioOutputEvidence: string;
  interruption?: string;
  audioRouteReason?: number;
  error?: VesperPlayerError;
  policy?: VesperPlaybackPolicyResult;
  nativeBuffering?: VesperNativeBufferingState;
  diagnostics?: VesperPlaybackDiagnostics;
}

export interface VesperBufferingUpdate { nativeType: number; value: number; }

export interface VesperPlayerEvent {
  type: string;
  atMonotonicMs: number;
  playbackEpoch?: string;
  nativeState?: string;
  positionMs?: number;
  error?: VesperPlayerError;
}
