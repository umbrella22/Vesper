import { deviceInfo } from '@kit.BasicServicesKit';
import { media } from '@kit.MediaKit';
import { VesperPlayerError } from './Types';

export interface VesperTimeRange { startMs: number; endMs: number; }
export interface VesperTimeRanges { loaded: VesperTimeRange[]; seekable: VesperTimeRange[]; }
export { VesperTrackConstraints, VesperVideoResolution, VesperTrackConstraintResult } from './TrackConstraints';
export interface VesperHarmonyCapabilities {
  runtimeApi: number; nativePlayer: boolean; advancedTrackConstraints: boolean; timeRanges: boolean;
  decoderGuarantee: string; hdrEvidence: string; audioOutputEvidence: string;
}

/** Higher API calls stay behind runtime checks and are never executed on API 20. */
export class Platform {
  static capabilities(): VesperHarmonyCapabilities {
    const api = deviceInfo.sdkApiVersion;
    return { runtimeApi: api, nativePlayer: true, advancedTrackConstraints: api >= 26, timeRanges: api >= 26,
      decoderGuarantee: 'unknown', hdrEvidence: 'unknown', audioOutputEvidence: 'unknown' };
  }
  static advanced(): void {
    if (deviceInfo.sdkApiVersion < 26) { throw new VesperPlayerError('unsupported', 'This operation requires HarmonyOS API 26'); }
  }
  static ranges(values: media.Range[]): VesperTimeRange[] {
    if (values.length > 256) { throw new VesperPlayerError('capacity', 'Native time-range count exceeds 256'); }
    return values.map((range: media.Range): VesperTimeRange => {
      if (!Number.isFinite(range.min) || !Number.isFinite(range.max) || range.min < 0 || range.max < range.min) {
        throw new VesperPlayerError('invalid_response', 'Native time range is invalid');
      }
      return { startMs: range.min, endMs: range.max };
    });
  }
}
