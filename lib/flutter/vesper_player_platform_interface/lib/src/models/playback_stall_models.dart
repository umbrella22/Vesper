part of '../models.dart';

/// Native media-clock observation thresholds. Initial startup is excluded.
final class VesperPlaybackStallPolicy {
  const VesperPlaybackStallPolicy(
      {this.enabled = true,
      this.positionThresholdMs = 5000,
      this.bufferingThresholdMs = 15000});
  final bool enabled;
  final int positionThresholdMs;
  final int bufferingThresholdMs;
  Map<String, Object?> toMap() {
    if (positionThresholdMs <= 0 || bufferingThresholdMs <= 0) {
      throw ArgumentError('Playback stall thresholds must be positive.');
    }
    return <String, Object?>{
      'enabled': enabled,
      'positionThresholdMs': positionThresholdMs,
      'bufferingThresholdMs': bufferingThresholdMs
    };
  }
}

enum VesperPlaybackStallKind { positionNotAdvancing, bufferingTimeout, unknown }

/// Historical suspicion of stalled playback, not proof of an audio output failure.
final class VesperPlaybackStallObservation {
  const VesperPlaybackStallObservation(
      {required this.playbackEpoch,
      required this.kind,
      required this.stalledForMs,
      required this.elapsedSinceLoadStartMs,
      required this.mediaPositionMs,
      required this.audio,
      this.kindRawValue});
  factory VesperPlaybackStallObservation.fromMap(Map<Object?, Object?> map) {
    final raw = map['kind'] as String?;
    return VesperPlaybackStallObservation(
      playbackEpoch:
          _requireDiagnosticInteger(map, 'playbackEpoch', minimum: 1),
      kind: _decodeEnum(
          VesperPlaybackStallKind.values, raw, VesperPlaybackStallKind.unknown),
      kindRawValue: raw,
      stalledForMs: _requireDiagnosticInteger(map, 'stalledForMs'),
      elapsedSinceLoadStartMs:
          _requireDiagnosticInteger(map, 'elapsedSinceLoadStartMs'),
      mediaPositionMs: _requireDiagnosticInteger(map, 'mediaPositionMs'),
      audio: VesperAudioPlaybackDiagnostics.fromMap(
          _rawMap(map['audio']) ?? const <Object?, Object?>{}),
    );
  }
  final int playbackEpoch;
  final VesperPlaybackStallKind kind;
  final String? kindRawValue;
  final int stalledForMs;
  final int elapsedSinceLoadStartMs;
  final int mediaPositionMs;
  final VesperAudioPlaybackDiagnostics audio;
  Map<String, Object?> toMap() => <String, Object?>{
        'playbackEpoch': playbackEpoch,
        'kind': kindRawValue ?? kind.name,
        'stalledForMs': stalledForMs,
        'elapsedSinceLoadStartMs': elapsedSinceLoadStartMs,
        'mediaPositionMs': mediaPositionMs,
        'audio': audio.toMap(),
      };
}
