import { VesperTrack, VesperTrackSelection, VesperPlayerError } from './Types';
import { languageComparisonKey, languageFallbackMatches } from './Language';

export interface VesperTrackPreferences { audioLanguages?: string[]; subtitleLanguages?: string[]; subtitleMode?: string; }
export interface VesperTrackPreferenceSelection { status: string; reason?: string; selection?: VesperTrackSelection; }
export interface VesperTrackPreferenceResult {
  playbackEpoch?: string; execution: string; audio: VesperTrackPreferenceSelection; subtitle: VesperTrackPreferenceSelection;
}

/** Language tags are retained verbatim; case folding is only used when comparing tags. */
export function validateTrackPreferences(value: VesperTrackPreferences): VesperTrackPreferences {
  for (const languages of [value.audioLanguages ?? [], value.subtitleLanguages ?? []]) {
    if (languages.length > 8 || languages.some((tag: string): boolean => typeof tag !== 'string' || tag.length > 64 ||
      !/^[A-Za-z]{1,8}(-[A-Za-z0-9]{1,8})*$/.test(tag))) {
      throw new VesperPlayerError('invalid_argument', 'Track preferences accept up to eight language tags of at most 64 characters');
    }
  }
  const mode = value.subtitleMode ?? 'system';
  if (!['system', 'off', 'preferred'].includes(mode)) { throw new VesperPlayerError('unsupported', 'Unknown subtitle preference mode'); }
  return { audioLanguages: (value.audioLanguages ?? []).slice(), subtitleLanguages: (value.subtitleLanguages ?? []).slice(), subtitleMode: mode };
}
function languageSelection(tracks: VesperTrack[], kind: string, languages: string[]): VesperTrackPreferenceSelection {
  if (languages.length === 0) { return { status: 'unchanged', reason: 'systemDefault' }; }
  const catalog = tracks.filter((track: VesperTrack): boolean => track.kind === kind && track.selectionSupported !== false &&
    track.language.length > 0 && track.language.toLowerCase() !== 'und');
  for (const language of languages) {
    const exact = catalog.find((track: VesperTrack): boolean => track.language.toLowerCase() === language.toLowerCase());
    if (exact !== undefined) { return { status: 'requested', selection: { kind: kind, mode: 'track', trackId: exact.id } }; }
    const key = languageComparisonKey(language);
    const alias = catalog.find((track: VesperTrack): boolean => languageComparisonKey(track.language) === key);
    if (alias !== undefined) { return { status: 'requested', selection: { kind: kind, mode: 'track', trackId: alias.id } }; }
    const fallback = catalog.find((track: VesperTrack): boolean => languageFallbackMatches(track.language, language));
    if (fallback !== undefined) { return { status: 'requested', selection: { kind: kind, mode: 'track', trackId: fallback.id } }; }
  }
  return { status: 'unchanged', reason: 'noMatchingLanguage' };
}
/** One explicit selection against this catalog; later catalog changes never overwrite the user's choice. */
export function resolveTrackPreferences(tracks: VesperTrack[], preferences: VesperTrackPreferences, epoch?: string): VesperTrackPreferenceResult {
  const value = validateTrackPreferences(preferences);
  return { playbackEpoch: epoch, execution: 'onceAfterCatalog', audio: languageSelection(tracks, 'audio', value.audioLanguages ?? []),
    subtitle: value.subtitleMode === 'off' ? { status: 'requested', selection: { kind: 'subtitle', mode: 'off' } } :
      value.subtitleMode === 'preferred' ? languageSelection(tracks, 'subtitle', value.subtitleLanguages ?? []) : { status: 'unchanged', reason: 'systemDefault' } };
}

/** System mode prefers an ordinary default sidecar, then forced text matching the selected audio. */
export function resolveExternalSubtitleDefault(tracks: VesperTrack[]): VesperTrackPreferenceSelection {
  const external = tracks.filter((track: VesperTrack): boolean => track.kind === 'subtitle' && track.external === true && track.selectionSupported !== false);
  const ordinary = external.find((track: VesperTrack): boolean => track.isDefault === true && track.isForced !== true);
  if (ordinary !== undefined) { return { status: 'requested', reason: 'externalDefault', selection: { kind: 'subtitle', mode: 'track', trackId: ordinary.id } }; }
  const audio = tracks.find((track: VesperTrack): boolean => track.kind === 'audio' && track.selected);
  const language = audio?.language ?? '';
  const forced = external.find((track: VesperTrack): boolean => track.isForced === true && language.length > 0 && language !== 'und' &&
    (languageComparisonKey(track.language) === languageComparisonKey(language) || languageFallbackMatches(track.language, language)));
  return forced === undefined ? { status: 'unchanged', reason: 'systemDefault' } :
    { status: 'requested', reason: 'forcedAudioLanguage', selection: { kind: 'subtitle', mode: 'track', trackId: forced.id } };
}
