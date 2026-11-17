import { media } from '@kit.MediaKit';
import { VesperPlayerError } from './Types';

export interface VesperVideoResolution { width: number; height: number; }
export interface VesperTrackConstraints {
  maxVideoBitrate?: number; minVideoBitrate?: number;
  maxVideoFrameRate?: number; minVideoFrameRate?: number;
  maxVideoResolution?: VesperVideoResolution; minVideoResolution?: VesperVideoResolution;
  preferredAudioLanguages?: string[]; preferredSubtitleLanguages?: string[];
}
export interface VesperTrackConstraintResult {
  playbackEpoch: string; requested: VesperTrackConstraints; accepted?: VesperTrackConstraints;
  status: string; errorCode?: string;
}

function resolution(value: VesperVideoResolution | undefined, native: boolean): VesperVideoResolution | undefined {
  if (value === undefined) { return undefined; }
  if (value === null || Object.keys(value).some((key: string): boolean => !['width', 'height'].includes(key))
    || !Number.isSafeInteger(value.width) || !Number.isSafeInteger(value.height)
    || value.width < (native ? 0 : 1) || value.height < (native ? 0 : 1)) {
    throw new VesperPlayerError(native ? 'invalid_response' : 'invalid_argument', 'Resolution requires integer width and height');
  }
  return { width: value.width, height: value.height };
}
function languages(value: string[] | undefined, native: boolean): string[] | undefined {
  if (value === undefined) { return undefined; }
  if (!Array.isArray(value) || value.length > 8 || value.some((tag: string): boolean =>
    typeof tag !== 'string' || tag.length > 64 || !/^[A-Za-z]{1,8}(-[A-Za-z0-9]{1,8})*$/.test(tag))) {
    throw new VesperPlayerError(native ? 'invalid_response' : 'invalid_argument', 'Track filters accept up to eight language tags of at most 64 characters');
  }
  return value.slice();
}
function copy(value: VesperTrackConstraints, native: boolean): VesperTrackConstraints {
  const values = [value.maxVideoBitrate, value.minVideoBitrate, value.maxVideoFrameRate, value.minVideoFrameRate];
  if (values.some((item: number | undefined): boolean => item !== undefined && (!Number.isSafeInteger(item) || item < (native ? 0 : 1)))) {
    throw new VesperPlayerError(native ? 'invalid_response' : 'invalid_argument', 'Track constraints require positive integers; native zero means unrestricted');
  }
  return { maxVideoBitrate: value.maxVideoBitrate, minVideoBitrate: value.minVideoBitrate,
    maxVideoFrameRate: value.maxVideoFrameRate, minVideoFrameRate: value.minVideoFrameRate,
    maxVideoResolution: resolution(value.maxVideoResolution, native), minVideoResolution: resolution(value.minVideoResolution, native),
    preferredAudioLanguages: languages(value.preferredAudioLanguages, native), preferredSubtitleLanguages: languages(value.preferredSubtitleLanguages, native) };
}

/** Copy before queuing; reject invalid numbers before any JSON serialization can change them. */
export function validateTrackConstraints(value: VesperTrackConstraints): VesperTrackConstraints {
  const allowed = ['maxVideoBitrate', 'minVideoBitrate', 'maxVideoFrameRate', 'minVideoFrameRate',
    'maxVideoResolution', 'minVideoResolution', 'preferredAudioLanguages', 'preferredSubtitleLanguages'];
  if (Object.keys(value).some((key: string): boolean => !allowed.includes(key))) {
    throw new VesperPlayerError('unsupported', 'Unknown track constraint');
  }
  const result = copy(value, false);
  if (result.minVideoBitrate !== undefined && result.maxVideoBitrate !== undefined && result.minVideoBitrate > result.maxVideoBitrate
    || result.minVideoFrameRate !== undefined && result.maxVideoFrameRate !== undefined && result.minVideoFrameRate > result.maxVideoFrameRate
    || result.minVideoResolution !== undefined && result.maxVideoResolution !== undefined &&
      (result.minVideoResolution.width > result.maxVideoResolution.width || result.minVideoResolution.height > result.maxVideoResolution.height)) {
    throw new VesperPlayerError('invalid_argument', 'Track constraint minimum exceeds maximum');
  }
  return result;
}

/** Native readback describes the configured filter, not the rendered track or resolution. */
export function nativeTrackConstraints(value: media.TrackSelectionFilter): VesperTrackConstraints {
  const projected: VesperTrackConstraints = { maxVideoBitrate: value.maxVideoBitrate, minVideoBitrate: value.minVideoBitrate,
    maxVideoFrameRate: value.maxVideoFrameRate, minVideoFrameRate: value.minVideoFrameRate,
    maxVideoResolution: nativeResolution(value.maxVideoResolution), minVideoResolution: nativeResolution(value.minVideoResolution),
    preferredAudioLanguages: value.preferredAudioLanguages, preferredSubtitleLanguages: value.preferredSubtitleLanguages };
  return copy(projected, true);
}
function nativeResolution(value: media.VideoSize | undefined): VesperVideoResolution | undefined {
  if (value === undefined) { return undefined; }
  if (value.width === undefined || value.height === undefined) { throw new VesperPlayerError('invalid_response', 'Native filter resolution lacks width or height'); }
  return { width: value.width, height: value.height };
}
