part of '../models.dart';

/// Subtitle lifecycle status shared by iOS and Android host kits.
///
/// The `unknown` variant is reserved for forward compatibility so a future
/// native addition does not corrupt the event stream: when the wire value
/// is not recognized, [status] falls back to [VesperSubtitleStatus.unknown]
/// and [statusRawValue] preserves the original string.
enum VesperSubtitleStatus {
  unavailable,
  loading,
  ready,
  failed,
  unknown;

  static VesperSubtitleStatus fromWire(String? raw) {
    if (raw == null) {
      return VesperSubtitleStatus.unavailable;
    }
    for (final value in VesperSubtitleStatus.values) {
      if (value.name == raw) {
        return value;
      }
    }
    return VesperSubtitleStatus.unknown;
  }
}

/// Phase where a subtitle failure originated.
enum VesperSubtitleErrorPhase {
  manifest,
  resource,
  discovery,
  identity,
  selection,
  unknown;

  static VesperSubtitleErrorPhase fromWire(String? raw) {
    if (raw == null) {
      return VesperSubtitleErrorPhase.unknown;
    }
    for (final value in VesperSubtitleErrorPhase.values) {
      if (value.name == raw) {
        return value;
      }
    }
    return VesperSubtitleErrorPhase.unknown;
  }
}

/// Structured subtitle error carried alongside [VesperSubtitleState].
///
/// The [code] is a stable string (e.g. `subtitle_track_not_found`) defined
/// by the cross-platform subtitle contract. Unknown codes are preserved in
/// [codeRawValue] so a newer native side does not silently lose diagnostic
/// information. The same applies to [phase] / [phaseRawValue].
final class VesperSubtitleError {
  const VesperSubtitleError({
    required this.code,
    required this.phase,
    required this.retriable,
    required this.message,
    this.trackId,
    this.codeRawValue,
    this.phaseRawValue,
  });

  factory VesperSubtitleError.fromMap(Map<Object?, Object?> map) {
    final rawCode = map['code'];
    final rawPhase = map['phase'];
    return VesperSubtitleError(
      code: rawCode is String ? _subtitleErrorCodeFromWire(rawCode) : 'unknown',
      phase: VesperSubtitleErrorPhase.fromWire(
          rawPhase is String ? rawPhase : null),
      retriable: _decodeBool(map, 'retriable'),
      message: map['message'] as String? ?? '',
      trackId: map['trackId'] as String?,
      codeRawValue: rawCode is String ? rawCode : null,
      phaseRawValue: rawPhase is String ? rawPhase : null,
    );
  }

  final String code;
  final VesperSubtitleErrorPhase phase;
  final bool retriable;
  final String message;
  final String? trackId;
  final String? codeRawValue;
  final String? phaseRawValue;

  Map<String, Object?> toMap() {
    return <String, Object?>{
      'code': codeRawValue ?? code,
      'phase': phaseRawValue ?? phase.name,
      'retriable': retriable,
      'message': message,
      if (trackId != null) 'trackId': trackId,
    };
  }
}

/// Maps a wire `code` string into a stable identifier. The subtitle contract
/// defines a fixed error-code set; codes outside that set are preserved as
/// raw strings via [VesperSubtitleError.codeRawValue]. The returned [code]
/// is the raw wire value when it is non-empty, or `'unknown'` when missing,
/// so consumers can compare stable identifiers while still observing
/// forward-compatible additions.
String _subtitleErrorCodeFromWire(String raw) {
  final trimmed = raw.trim();
  if (trimmed.isEmpty) {
    return 'unknown';
  }
  return trimmed;
}

/// Snapshot of subtitle catalog lifecycle exposed by the host kits.
///
/// Fields follow the cross-platform subtitle contract:
///
/// - [status] transitions through `unavailable -> loading -> ready` or
///   `-> failed` depending on whether the AV legible group / Media3 text
///   tracks align with the manifest descriptors.
/// - [advertisedTrackCount] preserves the manifest-declared count even
///   when [status] is [VesperSubtitleStatus.failed], so the host UI can
///   distinguish "subtitles broken" from "no subtitles".
/// - [selectableTrackCount] reflects how many descriptors currently map
///   to a native-selectable option.
///
/// When a field is absent in the wire payload (older host version), the
/// decoder falls back to const defaults via [VesperSubtitleState.empty]
/// so the entire snapshot remains decodable.
final class VesperSubtitleState {
  const VesperSubtitleState({
    this.status = VesperSubtitleStatus.unavailable,
    this.advertisedTrackCount = 0,
    this.selectableTrackCount = 0,
    this.error,
    this.statusRawValue,
  });

  /// Empty / initial state used when the host payload omits the field.
  static const empty = VesperSubtitleState();

  factory VesperSubtitleState.fromMap(Map<Object?, Object?> map) {
    final rawStatus = map['status'];
    final rawError = map['error'];
    return VesperSubtitleState(
      status:
          VesperSubtitleStatus.fromWire(rawStatus is String ? rawStatus : null),
      advertisedTrackCount: _decodeInt(map, 'advertisedTrackCount') ?? 0,
      selectableTrackCount: _decodeInt(map, 'selectableTrackCount') ?? 0,
      error: rawError is Map
          ? VesperSubtitleError.fromMap(Map<Object?, Object?>.from(rawError))
          : null,
      statusRawValue: rawStatus is String ? rawStatus : null,
    );
  }

  final VesperSubtitleStatus status;
  final int advertisedTrackCount;
  final int selectableTrackCount;
  final VesperSubtitleError? error;
  final String? statusRawValue;

  Map<String, Object?> toMap() {
    return <String, Object?>{
      'status': statusRawValue ?? status.name,
      'advertisedTrackCount': advertisedTrackCount,
      'selectableTrackCount': selectableTrackCount,
      'error': error?.toMap(),
    };
  }

  VesperSubtitleState copyWith({
    VesperSubtitleStatus? status,
    int? advertisedTrackCount,
    int? selectableTrackCount,
    Object? error = _sentinel,
    Object? statusRawValue = _sentinel,
  }) {
    return VesperSubtitleState(
      status: status ?? this.status,
      advertisedTrackCount: advertisedTrackCount ?? this.advertisedTrackCount,
      selectableTrackCount: selectableTrackCount ?? this.selectableTrackCount,
      error: identical(error, _sentinel)
          ? this.error
          : error as VesperSubtitleError?,
      statusRawValue: identical(statusRawValue, _sentinel)
          ? (status == null ? this.statusRawValue : null)
          : statusRawValue as String?,
    );
  }
}

const Object _sentinel = Object();
