import 'package:flutter/services.dart';

import 'models.dart';

/// A structured failure from a subtitle selection transaction.
///
/// Native plugins may report these failures through a `PlatformException`
/// before a snapshot carrying the same failure is emitted. The transaction
/// identifiers are optional because older host kits do not include them.
final class VesperSubtitleException implements Exception {
  const VesperSubtitleException({
    required this.code,
    required this.phase,
    required this.retriable,
    required this.message,
    this.trackId,
    this.commandId,
    this.sourceEpoch,
    this.phaseRawValue,
  });

  /// Decodes a native [PlatformException] and its nested `details` map.
  factory VesperSubtitleException.fromPlatformException(
    PlatformException error,
  ) {
    final root = _subtitleExceptionMap(error.details);
    final nested = _subtitleExceptionMap(root['details']);
    final payload = <Object?, Object?>{...root, ...nested};
    final rawCode = _firstString(<Object?>[
      payload['subtitleCode'],
      payload['code'],
      error.code,
    ]);
    final rawPhase = _firstString(<Object?>[
      payload['subtitlePhase'],
      payload['phase'],
    ]);
    final message = _firstString(<Object?>[
          payload['message'],
          error.message,
          error.code,
        ]) ??
        'Subtitle operation failed.';
    return VesperSubtitleException(
      code: rawCode ?? 'subtitle_unknown',
      phase: VesperSubtitleErrorPhase.fromWire(rawPhase),
      retriable: _decodeExceptionBool(payload['retriable']),
      message: message,
      trackId: _firstString(<Object?>[payload['trackId']]),
      commandId: _decodeExceptionInt(payload['commandId']),
      sourceEpoch: _decodeExceptionInt(payload['sourceEpoch']),
      phaseRawValue: rawPhase,
    );
  }

  /// Returns `null` for platform failures that are not subtitle failures.
  static VesperSubtitleException? tryFromPlatformException(
    PlatformException error,
  ) {
    final root = _subtitleExceptionMap(error.details);
    final nested = _subtitleExceptionMap(root['details']);
    final isSubtitle =
        error.code == 'vesper_subtitle_error' ||
        root['domain'] == 'subtitle' ||
        nested['domain'] == 'subtitle';
    return isSubtitle
        ? VesperSubtitleException.fromPlatformException(error)
        : null;
  }

  final String code;
  final VesperSubtitleErrorPhase phase;
  final bool retriable;
  final String message;
  final String? trackId;
  final int? commandId;
  final int? sourceEpoch;
  final String? phaseRawValue;

  @override
  String toString() {
    final suffix = trackId == null ? '' : ' trackId=$trackId';
    return 'VesperSubtitleException($code, phase=${phase.name}, '
        'retriable=$retriable$suffix): $message';
  }
}

Map<Object?, Object?> _subtitleExceptionMap(Object? value) {
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

bool _decodeExceptionBool(Object? value) => value is bool && value;

int? _decodeExceptionInt(Object? value) {
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
