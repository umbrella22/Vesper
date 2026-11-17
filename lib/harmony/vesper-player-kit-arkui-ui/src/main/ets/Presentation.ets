import { VesperPlayerSnapshot, VesperSelectionState, VesperTimeRange, VesperTrack } from '@vesper/player-kit-arkui';
import { VesperStageInsets } from './Gestures';

export interface VesperStagePresentation { controlsVisible: boolean; locked: boolean; settingsPane?: string; }
export interface StageLayout {
  compact: boolean; splitTime: boolean; speed: boolean; quality: boolean; leading: number;
  badge: boolean; primary: boolean;
}
export interface StageLayoutGeometry {
  fullscreenTarget?: number; compactTarget?: number; compactFullscreenTarget?: number;
  buttonHeight?: number; compactButtonHeight?: number; timeHeight?: number; headerHeight?: number;
  primaryHeight?: number; primaryWidth?: number; seekable?: boolean;
}

/** Widths include targets and measured text, so the scrubber never borrows a button's target. */
export function stageLayout(width: number, height: number, layout: string, target: number, spacing: number,
  timeWidth: number, speedWidth: number, qualityWidth: number, leadingWidth: number = 0,
  geometry: StageLayoutGeometry = {}): StageLayout {
  const buttonHeight = geometry.buttonHeight ?? Math.max(target, geometry.fullscreenTarget ?? target);
  const timeHeight = geometry.timeHeight ?? 16;
  const headerHeight = geometry.headerHeight ?? 48;
  const expandedHeight = timeHeight + 28 + buttonHeight;
  // Keep a usable video gesture region between the header and transport.
  const compact = layout === 'compact' || height < headerHeight + expandedHeight + 24;
  const play = compact ? geometry.compactTarget ?? target : target;
  const fullscreen = compact ? geometry.compactFullscreenTarget ?? geometry.fullscreenTarget ?? target : geometry.fullscreenTarget ?? target;
  const scrubberWidth = geometry.seekable === false ? 0 : 56;
  const splitTime = compact && width - play - fullscreen - spacing * 3 - timeWidth < scrubberWidth;
  // The expanded row has two fixed groups separated by one flexible gap.
  let remaining = width - play - fullscreen - spacing;
  const leading = !compact && leadingWidth > 0 && remaining >= leadingWidth + spacing ? leadingWidth : 0;
  if (leading > 0) { remaining -= leading + spacing; }
  const quality = !compact && remaining >= qualityWidth + spacing;
  if (quality) { remaining -= qualityWidth + spacing; }
  const speed = !compact && remaining >= speedWidth + spacing;
  const bottomHeight = compact ? (geometry.compactButtonHeight ?? buttonHeight) + (splitTime ? timeHeight : 0) : expandedHeight;
  const badge = height >= headerHeight + bottomHeight + 24;
  const primary = !compact && layout === 'standard' && width >= (geometry.primaryWidth ?? 192) &&
    height >= headerHeight + expandedHeight + (geometry.primaryHeight ?? 72) + 24;
  return { compact: compact, splitTime: splitTime, speed: speed, quality: quality, leading: leading,
    badge: badge, primary: primary };
}

/** Native buffering percentages and cached durations intentionally do not enter this function. */
export function stageLoadedRanges(snapshot: VesperPlayerSnapshot): VesperTimeRange[] {
  const duration = snapshot.durationMs;
  if (snapshot.timeline !== 'vod' || duration === undefined || !Number.isFinite(duration) || duration <= 0) { return []; }
  return (snapshot.timeRanges?.loaded ?? []).filter((range: VesperTimeRange): boolean =>
    Number.isFinite(range.startMs) && Number.isFinite(range.endMs) && range.startMs >= 0 && range.endMs > range.startMs && range.startMs < duration)
    .map((range: VesperTimeRange): VesperTimeRange => ({ startMs: range.startMs, endMs: Math.min(duration, range.endMs) }));
}

export interface StageQuality { mode: string; status: string; height?: number; bitrate?: number; }
function videoSelection(snapshot: VesperPlayerSnapshot): VesperSelectionState | undefined {
  return snapshot.trackSelections?.video ?? (snapshot.selection.requested?.kind === 'video' ||
    snapshot.selection.effective?.kind === 'video' ? snapshot.selection : undefined);
}
export function stageQuality(snapshot: VesperPlayerSnapshot): StageQuality {
  const state = videoSelection(snapshot);
  const requested = state?.requested;
  const track = requested?.mode === 'track' ? snapshot.tracks.find((value): boolean => value.kind === 'video' && value.id === requested.trackId) : undefined;
  const observed = requested?.mode === 'track' ? track?.height !== undefined && track.height > 0 && track.height === snapshot.height &&
    (track.width === undefined || track.width <= 0 || track.width === snapshot.width) :
    requested?.mode === 'bitrate' ? state?.effectiveBitrate === requested.bitrate : true;
  return { mode: state?.effective?.mode ?? (snapshot.availableBitrates.length > 0 ? 'auto' : 'system'),
    status: state?.outcome === 'failed' ? 'failed' : requested !== undefined && (state?.outcome !== 'confirmed' || !observed) ? 'switching' : 'observed',
    height: snapshot.height > 0 ? snapshot.height : undefined, bitrate: state?.effectiveBitrate ?? snapshot.selection.effectiveBitrate };
}

/** A native adaptive rendition is observed output, not an explicit fixed selection. */
export function stageTrackSelected(snapshot: VesperPlayerSnapshot, track: VesperTrack): boolean {
  if (track.kind !== 'video') { return track.selected; }
  const mode = stageQuality(snapshot).mode;
  if (mode === 'track') { return videoSelection(snapshot)?.effective?.trackId === track.id; }
  return mode === 'system' && track.selected;
}

export function stageTracks(snapshot: VesperPlayerSnapshot, kind: string): VesperTrack[] {
  const tracks = snapshot.tracks.filter((track: VesperTrack): boolean => track.kind === kind);
  if (kind === 'video') {
    tracks.sort((left: VesperTrack, right: VesperTrack): number =>
      (right.height ?? 0) - (left.height ?? 0) || (right.bitrate ?? 0) - (left.bitrate ?? 0) || left.id - right.id);
  }
  return tracks;
}

export function stageTrackDetail(snapshot: VesperPlayerSnapshot, track: VesperTrack): string {
  const parts: string[] = [];
  if (track.width && track.height) { parts.push(track.width + ' × ' + track.height); }
  const codec = track.mimeType === 'video/avc' ? 'H.264' : track.mimeType === 'video/hevc' ? 'HEVC' :
    track.mimeType === 'video/av01' ? 'AV1' : track.mimeType === 'audio/mp4a-latm' ? 'AAC' : '';
  if (codec) { parts.push(codec); }
  if (track.kind === 'video' && snapshot.tracks.some((other: VesperTrack): boolean =>
    other.kind === 'video' && other.id !== track.id && other.height === track.height) && (track.bitrate ?? 0) > 0) {
    parts.push(Math.round((track.bitrate ?? 0) / 1000) + ' kbps');
  }
  return parts.join(' · ');
}

/** Normalize display keys while retaining the native language tag in the catalog. */
export function stageLanguageBase(language: string): string {
  const base = language.toLowerCase().split('-')[0];
  switch (base) {
    case 'eng': return 'en';
    case 'zho': case 'chi': return 'zh';
    case 'jpn': return 'ja';
    case 'kor': return 'ko';
    case 'fra': case 'fre': return 'fr';
    case 'deu': case 'ger': return 'de';
    case 'spa': return 'es';
    case 'por': return 'pt';
    case 'rus': return 'ru';
    case 'ita': return 'it';
    case 'ara': return 'ar';
    default: return base;
  }
}

export interface VesperSubtitleStyleOptions {
  fontScale?: number; foreground?: ResourceColor; background?: ResourceColor; edgeColor?: ResourceColor;
  edgeRadius?: number; bottomInset?: number; maxLines?: number;
}
export class VesperSubtitleStyle {
  readonly fontScale: number;
  readonly foreground: ResourceColor;
  readonly background: ResourceColor;
  readonly edgeColor: ResourceColor;
  readonly edgeRadius: number;
  readonly bottomInset: number;
  readonly maxLines: number;
  constructor(options: VesperSubtitleStyleOptions = {}) {
    this.fontScale = options.fontScale ?? 1; this.foreground = options.foreground ?? Color.White;
    this.background = options.background ?? '#A0000000'; this.edgeColor = options.edgeColor ?? Color.Black;
    this.edgeRadius = options.edgeRadius ?? 2; this.bottomInset = options.bottomInset ?? 16;
    this.maxLines = options.maxLines ?? 3;
    if (!Number.isFinite(this.fontScale) || this.fontScale < 0.5 || this.fontScale > 3 ||
      !Number.isFinite(this.edgeRadius) || this.edgeRadius < 0 || !Number.isFinite(this.bottomInset) || this.bottomInset < 0 ||
      !Number.isInteger(this.maxLines) || this.maxLines < 1 || this.maxLines > 8) {
      throw new Error('Caption style requires scale 0.5–3, nonnegative geometry and 1–8 display lines');
    }
  }
}
export interface StageCaptionGeometry { width: number; bottom: number; maxHeight: number; }
export function stageCaptionGeometry(width: number, height: number, videoWidth: number, videoHeight: number,
  safe: VesperStageInsets, controlsHeight: number, inset: number, headerHeight: number = 0): StageCaptionGeometry {
  const availableWidth = Math.max(0, width - safe.left - safe.right);
  const availableHeight = Math.max(0, height - safe.top - safe.bottom);
  const known = videoWidth > 0 && videoHeight > 0;
  const scale = known ? Math.min(availableWidth / videoWidth, availableHeight / videoHeight) : 1;
  const contentWidth = known ? videoWidth * scale : availableWidth;
  const contentHeight = known ? videoHeight * scale : availableHeight;
  const letterbox = (availableHeight - contentHeight) / 2;
  const bottom = Math.max(safe.bottom + letterbox + inset, safe.bottom + controlsHeight + inset);
  const top = safe.top + Math.max(letterbox, headerHeight) + inset;
  return { width: Math.max(0, contentWidth - inset * 2), bottom: bottom, maxHeight: Math.max(0, height - bottom - top) };
}
