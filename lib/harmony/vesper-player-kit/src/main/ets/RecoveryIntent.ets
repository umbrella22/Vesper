import { VesperPlayerSnapshot, VesperTrack, VesperTrackSelection } from './Types';
import { VesperTrackConstraints } from './TrackConstraints';
import { languageComparisonKey, languageFallbackMatches } from './Language';

interface TrackIdentity {
  stableId?: string; language: string; mimeType: string; width?: number; height?: number; bitrate?: number;
}
export interface RecoveryTrackIntent { kind: string; mode: string; bitrate?: number; track?: TrackIdentity; }
export interface RecoveryIntent { constraints?: VesperTrackConstraints; tracks: RecoveryTrackIntent[]; }
export interface RecoveryTrackMatch { selection?: VesperTrackSelection; reason?: string; }

/** Capture semantic choices, never an epoch-local native track index. */
export function captureRecoveryIntent(snapshot: VesperPlayerSnapshot): RecoveryIntent {
  const intent: RecoveryIntent = { tracks: [] };
  const filter = snapshot.trackConstraints;
  if (filter !== undefined && filter.status !== 'failed') {
    intent.constraints = JSON.parse(JSON.stringify(filter.requested)) as VesperTrackConstraints;
  }
  for (const kind of ['video', 'audio', 'subtitle']) {
    const state = kind === 'video' ? snapshot.trackSelections?.video : kind === 'audio' ? snapshot.trackSelections?.audio : snapshot.trackSelections?.subtitle;
    const choice = state?.outcome === 'pending' ? state.requested : state?.effective;
    if (choice === undefined) { continue; }
    const item: RecoveryTrackIntent = { kind: kind, mode: choice.mode, bitrate: choice.bitrate };
    if (choice.mode === 'track') {
      const track = snapshot.tracks.find((track: VesperTrack): boolean => track.kind === kind && track.id === choice.trackId);
      if (track !== undefined) {
        item.track = { stableId: track.stableId, language: track.language, mimeType: track.mimeType,
          width: track.width, height: track.height, bitrate: track.bitrate };
      }
    }
    intent.tracks.push(item);
  }
  return intent;
}

/** Ambiguous or missing identities remain observable instead of selecting an arbitrary track. */
export function matchRecoveryTrack(intent: RecoveryTrackIntent, catalog: VesperTrack[]): RecoveryTrackMatch {
  if (intent.mode !== 'track') { return { selection: { kind: intent.kind, mode: intent.mode, bitrate: intent.bitrate } }; }
  const identity = intent.track;
  if (identity === undefined) { return { reason: 'missingOriginalTrack' }; }
  const candidates = catalog.filter((track: VesperTrack): boolean => {
    if (track.kind !== intent.kind || track.selectionSupported === false) { return false; }
    if (identity.stableId !== undefined) { return track.stableId === identity.stableId; }
    const language = languageComparisonKey(track.language) === languageComparisonKey(identity.language) ||
      languageFallbackMatches(track.language, identity.language);
    return language && track.mimeType === identity.mimeType && track.width === identity.width &&
      track.height === identity.height && track.bitrate === identity.bitrate;
  });
  if (candidates.length !== 1) { return { reason: candidates.length === 0 ? 'noMatchingTrack' : 'ambiguousTrack' }; }
  return { selection: { kind: intent.kind, mode: 'track', trackId: candidates[0].id } };
}
