import 'package:flutter/services.dart';

/// Stable rejection codes for an explicit fixed-video-track command.
enum VesperFixedTrackSelectionErrorCode {
  trackUnavailable,
  trackExceedsCapabilities,
  trackUnsupported,
  staleCatalog,
  unknown,
}

/// A structured fixed-track rejection emitted by a native playback host.
///
/// [codeRawValue] is retained when a newer host introduces a code that this
/// package does not know yet. A missing expected revision is represented as
/// `null`; it is never synthesized from the current catalog.
final class VesperFixedTrackSelectionException extends UnsupportedError {
  VesperFixedTrackSelectionException({
    required this.codeRawValue,
    required this.trackId,
    required this.expectedCatalogRevision,
    required this.actualCatalogRevision,
    required String message,
    this.platformCode,
    this.platformDetails = const <String, Object?>{},
  })  : code = _codeFromWire(codeRawValue),
        super(message);

  factory VesperFixedTrackSelectionException.fromPlatformException(
    PlatformException error,
  ) {
    final root = _fixedTrackMap(error.details);
    final nested = _fixedTrackMap(root['details']);
    final payload = <Object?, Object?>{...root, ...nested};
    final rawCode = _firstString(<Object?>[
          payload['fixedTrackCode'],
          payload['code'],
          error.code,
        ]) ??
        'unknown';
    final message = _firstString(<Object?>[
          payload['message'],
          error.message,
          rawCode,
        ]) ??
        'Fixed-track selection failed.';
    return VesperFixedTrackSelectionException(
      codeRawValue: rawCode,
      trackId: _firstString(<Object?>[payload['trackId']]),
      expectedCatalogRevision:
          _decodeFixedTrackRevision(payload['expectedCatalogRevision']),
      actualCatalogRevision:
          _decodeFixedTrackRevision(payload['actualCatalogRevision']),
      message: message,
      platformCode: error.code,
      platformDetails: _toStringKeyedMap(payload),
    );
  }

  static VesperFixedTrackSelectionException? tryFromPlatformException(
    PlatformException error,
  ) {
    final root = _fixedTrackMap(error.details);
    final nested = _fixedTrackMap(root['details']);
    return root['domain'] == 'fixedTrack' ||
            nested['domain'] == 'fixedTrack' ||
            error.code == 'vesper_fixed_track_error'
        ? VesperFixedTrackSelectionException.fromPlatformException(error)
        : null;
  }

  final VesperFixedTrackSelectionErrorCode code;
  final String codeRawValue;
  final String? trackId;
  final int? expectedCatalogRevision;
  final int? actualCatalogRevision;
  final String? platformCode;
  final Map<String, Object?> platformDetails;

  @override
  String toString() {
    final suffix = trackId == null ? '' : ' trackId=$trackId';
    return 'VesperFixedTrackSelectionException($codeRawValue$suffix): $message';
  }
}

VesperFixedTrackSelectionErrorCode _codeFromWire(String raw) {
  for (final value in VesperFixedTrackSelectionErrorCode.values) {
    if (value.name == raw) {
      return value;
    }
  }
  return VesperFixedTrackSelectionErrorCode.unknown;
}

Map<Object?, Object?> _fixedTrackMap(Object? value) {
  if (value is Map<Object?, Object?>) {
    return value;
  }
  if (value is Map) {
    return Map<Object?, Object?>.from(value);
  }
  return const <Object?, Object?>{};
}

String? _firstString(Iterable<Object?> values) {
  for (final value in values) {
    if (value is String && value.isNotEmpty) {
      return value;
    }
  }
  return null;
}

int? _decodeFixedTrackRevision(Object? value) {
  if (value is int) {
    return value;
  }
  if (value is num && value.isFinite) {
    return value.toInt();
  }
  if (value is String) {
    return int.tryParse(value);
  }
  return null;
}

Map<String, Object?> _toStringKeyedMap(Map<Object?, Object?> source) {
  return source.map((key, value) => MapEntry(key.toString(), value));
}
