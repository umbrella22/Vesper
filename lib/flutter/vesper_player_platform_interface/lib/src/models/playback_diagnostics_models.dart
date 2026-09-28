part of '../models.dart';

/// Platform evidence, not a guarantee that the application's UI is visible.
enum VesperFirstFrameObservationKind {
  media3RenderedFirstFrame,
  avPlayerLayerReadyForDisplay,
  unknown,
}

/// The first native video observation in one source loading attempt.
///
/// [elapsedSinceLoadStartMs] uses one native monotonic clock and includes native
/// source preparation. It excludes application source resolution. The media
/// position is independent of that duration, including when resuming a video.
final class VesperFirstFrameObservation {
  const VesperFirstFrameObservation({
    required this.playbackEpoch,
    required this.elapsedSinceLoadStartMs,
    required this.kind,
    this.mediaPositionMs,
    this.kindRawValue,
  });

  factory VesperFirstFrameObservation.fromMap(Map<Object?, Object?> map) {
    final rawKind = map['kind'] as String?;
    return VesperFirstFrameObservation(
      playbackEpoch:
          _requireDiagnosticInteger(map, 'playbackEpoch', minimum: 1),
      elapsedSinceLoadStartMs:
          _requireDiagnosticInteger(map, 'elapsedSinceLoadStartMs'),
      mediaPositionMs: _decodeInt(map, 'mediaPositionMs'),
      kind: _decodeEnum(VesperFirstFrameObservationKind.values, rawKind,
          VesperFirstFrameObservationKind.unknown),
      kindRawValue: rawKind,
    );
  }

  final int playbackEpoch;
  final int elapsedSinceLoadStartMs;
  final int? mediaPositionMs;
  final VesperFirstFrameObservationKind kind;
  final String? kindRawValue;

  Map<String, Object?> toMap() => <String, Object?>{
        'playbackEpoch': playbackEpoch,
        'elapsedSinceLoadStartMs': elapsedSinceLoadStartMs,
        'kind': kindRawValue ?? kind.name,
        if (mediaPositionMs != null) 'mediaPositionMs': mediaPositionMs,
      };
}

/// Evidence for the selected audio input; this does not observe speaker output.
enum VesperAudioDiagnosticEvidence {
  runtimeFormat,
  selectedMediaOption,
  manifestMetadata,
  unknown,
}

enum VesperAudioDiagnosticIssueKind { decoderError, sinkError, unknown }

/// A platform audio callback, which may be recoverable by the native player.
final class VesperAudioDiagnosticIssue {
  const VesperAudioDiagnosticIssue({
    required this.kind,
    required this.elapsedSinceLoadStartMs,
    this.platformCode,
    this.message,
    this.kindRawValue,
  });

  factory VesperAudioDiagnosticIssue.fromMap(Map<Object?, Object?> map) {
    final rawKind = map['kind'] as String?;
    return VesperAudioDiagnosticIssue(
      kind: _decodeEnum(VesperAudioDiagnosticIssueKind.values, rawKind,
          VesperAudioDiagnosticIssueKind.unknown),
      elapsedSinceLoadStartMs:
          _requireDiagnosticInteger(map, 'elapsedSinceLoadStartMs'),
      platformCode: map['platformCode'] as String?,
      message: map['message'] as String?,
      kindRawValue: rawKind,
    );
  }

  final VesperAudioDiagnosticIssueKind kind;
  final int elapsedSinceLoadStartMs;
  final String? platformCode;
  final String? message;
  final String? kindRawValue;

  Map<String, Object?> toMap() => <String, Object?>{
        'kind': kindRawValue ?? kind.name,
        'elapsedSinceLoadStartMs': elapsedSinceLoadStartMs,
        if (platformCode != null) 'platformCode': platformCode,
        if (message != null) 'message': message,
      };
}

/// Current audio input and the latest audio issue in this attempt.
///
/// Unknown fields remain null. In particular, AVPlayer does not expose its
/// decoder identity, and neither format metadata nor an advancing playback
/// clock confirms that audio samples reached the output device.
final class VesperAudioPlaybackDiagnostics {
  const VesperAudioPlaybackDiagnostics({
    this.trackId,
    this.formatId,
    this.codec,
    this.sampleMimeType,
    this.decoderName,
    this.channels,
    this.sampleRate,
    this.evidence = VesperAudioDiagnosticEvidence.unknown,
    this.evidenceRawValue,
    this.lastIssue,
  });

  factory VesperAudioPlaybackDiagnostics.fromMap(Map<Object?, Object?> map) {
    final rawEvidence = map['evidence'] as String?;
    final issue = _rawMap(map['lastIssue']);
    return VesperAudioPlaybackDiagnostics(
      trackId: map['trackId'] as String?,
      formatId: map['formatId'] as String?,
      codec: map['codec'] as String?,
      sampleMimeType: map['sampleMimeType'] as String?,
      decoderName: map['decoderName'] as String?,
      channels: _decodeInt(map, 'channels'),
      sampleRate: _decodeInt(map, 'sampleRate'),
      evidence: _decodeEnum(VesperAudioDiagnosticEvidence.values, rawEvidence,
          VesperAudioDiagnosticEvidence.unknown),
      evidenceRawValue: rawEvidence,
      lastIssue:
          issue == null ? null : VesperAudioDiagnosticIssue.fromMap(issue),
    );
  }

  final String? trackId;
  final String? formatId;
  final String? codec;
  final String? sampleMimeType;
  final String? decoderName;
  final int? channels;
  final int? sampleRate;
  final VesperAudioDiagnosticEvidence evidence;
  final String? evidenceRawValue;
  final VesperAudioDiagnosticIssue? lastIssue;

  Map<String, Object?> toMap() => <String, Object?>{
        'evidence': evidenceRawValue ?? evidence.name,
        if (trackId != null) 'trackId': trackId,
        if (formatId != null) 'formatId': formatId,
        if (codec != null) 'codec': codec,
        if (sampleMimeType != null) 'sampleMimeType': sampleMimeType,
        if (decoderName != null) 'decoderName': decoderName,
        if (channels != null) 'channels': channels,
        if (sampleRate != null) 'sampleRate': sampleRate,
        if (lastIssue != null) 'lastIssue': lastIssue!.toMap(),
      };
}

/// Retained observations for one native source loading attempt.
///
/// Epochs are local to a controller and advance even when reloading the same
/// source. Zero means no attempt. A missing snapshot means the host does not
/// implement these observations; it is not evidence of successful playback.
final class VesperPlaybackDiagnosticsSnapshot {
  const VesperPlaybackDiagnosticsSnapshot({
    required this.playbackEpoch,
    this.audio = const VesperAudioPlaybackDiagnostics(),
    this.firstFrame,
  });

  factory VesperPlaybackDiagnosticsSnapshot.fromMap(Map<Object?, Object?> map) {
    final firstFrame = _rawMap(map['firstFrame']);
    return VesperPlaybackDiagnosticsSnapshot(
      playbackEpoch: _requireDiagnosticInteger(map, 'playbackEpoch'),
      audio: VesperAudioPlaybackDiagnostics.fromMap(
          _rawMap(map['audio']) ?? const <Object?, Object?>{}),
      firstFrame: firstFrame == null
          ? null
          : VesperFirstFrameObservation.fromMap(firstFrame),
    );
  }

  final int playbackEpoch;
  final VesperAudioPlaybackDiagnostics audio;
  final VesperFirstFrameObservation? firstFrame;

  Map<String, Object?> toMap() => <String, Object?>{
        'playbackEpoch': playbackEpoch,
        'audio': audio.toMap(),
        if (firstFrame != null) 'firstFrame': firstFrame!.toMap(),
      };
}

/// Captured audio evidence from a possibly recoverable native audio callback.
final class VesperAudioRuntimeWarning {
  const VesperAudioRuntimeWarning(
      {required this.playbackEpoch, required this.diagnostics});
  factory VesperAudioRuntimeWarning.fromMap(Map<Object?, Object?> map) =>
      VesperAudioRuntimeWarning(
        playbackEpoch:
            _requireDiagnosticInteger(map, 'playbackEpoch', minimum: 1),
        diagnostics: VesperAudioPlaybackDiagnostics.fromMap(
            _rawMap(map['audio']) ?? const <Object?, Object?>{}),
      );
  final int playbackEpoch;
  final VesperAudioPlaybackDiagnostics diagnostics;
  Map<String, Object?> toMap() => <String, Object?>{
        'playbackEpoch': playbackEpoch,
        'audio': diagnostics.toMap(),
      };
}

int _requireDiagnosticInteger(Map<Object?, Object?> map, String key,
    {int minimum = 0}) {
  final value = map[key];
  if (value is! int || value < minimum) {
    throw FormatException(
        'Playback diagnostics $key must be an integer >= $minimum.', value);
  }
  return value;
}
