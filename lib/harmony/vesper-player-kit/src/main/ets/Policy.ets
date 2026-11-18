import { VesperPlayerError } from './Types';
import { media } from '@kit.MediaKit';
import { VesperBufferObservation } from './Diagnostics';

export interface VesperBufferingPolicy {
  minBufferMs?: number; maxBufferMs?: number;
  bufferForPlaybackMs?: number; bufferForPlaybackAfterRebufferMs?: number;
  preferredBufferDurationMs?: number; preferredBufferDurationForPlayingMs?: number;
}
export interface VesperNativeBufferingPreferences { preferredBufferDurationMs?: number; preferredBufferDurationForPlayingMs?: number; }
export interface VesperNativeBufferingState {
  playbackEpoch: string; requested: VesperNativeBufferingPreferences; submitted?: VesperNativeBufferingPreferences;
  status: string; reason: string;
  observed?: VesperBufferObservation;
}
/** Presets submit native preferences at the next activation; they are not waterline guarantees. */
export class VesperBufferingPresets {
  static fastStart(): VesperBufferingPolicy { return { preferredBufferDurationMs: 2000, preferredBufferDurationForPlayingMs: 500 }; }
  static balanced(): VesperBufferingPolicy { return { preferredBufferDurationMs: 5000, preferredBufferDurationForPlayingMs: 1000 }; }
  static resilient(): VesperBufferingPolicy { return { preferredBufferDurationMs: 15000, preferredBufferDurationForPlayingMs: 3000 }; }
}
export interface VesperRetryPolicy { maxAttempts?: number; }
export interface VesperPlaybackPolicy { buffering?: VesperBufferingPolicy; retry?: VesperRetryPolicy; decoder?: string; }
export interface VesperPolicyParameterResult {
  parameter: string; requested: number | string; effective?: number | string; status: string; reason?: string;
}
export interface VesperPlaybackPolicyResult { parameters: VesperPolicyParameterResult[]; }
export interface ResolvedPolicy { result: VesperPlaybackPolicyResult; attempts: number; buffering: VesperNativeBufferingPreferences; }

/** Unsupported native tuning remains visible; transport retries have a separate execution scope. */
export function resolvePolicy(policy: VesperPlaybackPolicy, currentAttempts: number,
  currentBuffering: VesperNativeBufferingPreferences = {}): ResolvedPolicy {
  const parameters: VesperPolicyParameterResult[] = []; let attempts = currentAttempts;
  let preferences: VesperNativeBufferingPreferences = { preferredBufferDurationMs: currentBuffering.preferredBufferDurationMs,
    preferredBufferDurationForPlayingMs: currentBuffering.preferredBufferDurationForPlayingMs };
  if (Object.keys(policy).some((key: string) => !['buffering', 'retry', 'decoder'].includes(key))) {
    throw new VesperPlayerError('invalid_argument', 'Unknown playback policy parameter');
  }
  if (policy.buffering !== undefined) {
    const buffering = policy.buffering as Record<string, number>;
    for (const key of Object.keys(buffering)) {
      if (!['minBufferMs', 'maxBufferMs', 'bufferForPlaybackMs', 'bufferForPlaybackAfterRebufferMs',
        'preferredBufferDurationMs', 'preferredBufferDurationForPlayingMs'].includes(key)
        || !Number.isSafeInteger(buffering[key]) || buffering[key] < 0) {
        throw new VesperPlayerError('invalid_argument', 'Buffer durations require nonnegative integer milliseconds');
      }
      const native = ['preferredBufferDurationMs', 'preferredBufferDurationForPlayingMs'].includes(key);
      if (native && (buffering[key] > 20000 || key === 'preferredBufferDurationMs' && (buffering[key] < 1000 || buffering[key] % 1000 !== 0))) {
        throw new VesperPlayerError('invalid_argument', 'Native buffer duration requires whole seconds from 1–20; playback threshold accepts 0–20000 milliseconds');
      }
      parameters.push({ parameter: `buffering.${key}`, requested: buffering[key], status: native ? 'deferred' : 'unsupported',
        reason: native ? 'Submitted at the next MediaSource activation; native thresholds have no effective-value readback' :
          'AVPlayer does not expose independent minimum/maximum and initial/rebuffer thresholds; use the native duration preferences' });
    }
    preferences = { preferredBufferDurationMs: policy.buffering.preferredBufferDurationMs,
      preferredBufferDurationForPlayingMs: policy.buffering.preferredBufferDurationForPlayingMs };
    if (preferences.preferredBufferDurationMs !== undefined && preferences.preferredBufferDurationForPlayingMs !== undefined
      && preferences.preferredBufferDurationForPlayingMs > preferences.preferredBufferDurationMs) {
      throw new VesperPlayerError('invalid_argument', 'Playback buffer preference exceeds preferred buffer duration');
    }
  }
  if (policy.retry !== undefined) {
    if (Object.keys(policy.retry).some((key: string) => key !== 'maxAttempts')) { throw new VesperPlayerError('invalid_argument', 'Unknown retry policy parameter'); }
    const requested = policy.retry.maxAttempts;
    if (requested !== undefined) {
      if (!Number.isSafeInteger(requested) || requested < 1) { throw new VesperPlayerError('invalid_argument', 'Retry maxAttempts must be a positive integer'); }
      if (requested <= 3) {
        attempts = requested;
        parameters.push({ parameter: 'retry.maxAttempts', requested: requested, effective: requested, status: 'applied',
          reason: 'Applies to future controller resource requests; native AVPlayer reconnection is system-controlled' });
      } else { parameters.push({ parameter: 'retry.maxAttempts', requested: requested, effective: attempts, status: 'unsupported', reason: 'Controller transport supports at most three attempts per request' }); }
    }
  }
  if (policy.decoder !== undefined) {
    if (typeof policy.decoder !== 'string' || policy.decoder.length > 128) { throw new VesperPlayerError('invalid_argument', 'Decoder policy must be a bounded name'); }
    parameters.push({ parameter: 'decoder', requested: policy.decoder, effective: 'system',
      status: policy.decoder === 'system' ? 'applied' : 'unsupported',
      reason: 'AVPlayer selects the system decoder; hardware/software enforcement and decoder identity are unavailable' });
  }
  return { result: { parameters: parameters }, attempts: attempts, buffering: preferences };
}

/** Duration uses native integer seconds; the playback threshold uses fractional seconds. */
export function bufferingStrategy(preferences: VesperNativeBufferingPreferences, remoteHls: boolean): media.PlaybackStrategy | undefined {
  const playing = preferences.preferredBufferDurationForPlayingMs;
  const duration = preferences.preferredBufferDurationMs ?? (remoteHls ? Math.max(2000, Math.ceil((playing ?? 0) / 1000) * 1000) : undefined);
  if (duration === undefined && playing === undefined) { return undefined; }
  const strategy: media.PlaybackStrategy = {};
  if (duration !== undefined) { strategy.preferredBufferDuration = duration / 1000; }
  if (playing !== undefined) { strategy.preferredBufferDurationForPlaying = playing / 1000; }
  return strategy;
}
