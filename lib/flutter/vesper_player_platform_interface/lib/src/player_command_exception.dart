import 'package:flutter/services.dart';

import 'models.dart';

/// A structured failure from a player command that crossed a platform boundary.
///
/// This exception is used for source and playback commands that are not owned
/// by a more specific contract such as subtitle or fixed-track selection.
final class VesperPlayerCommandException implements Exception {
  const VesperPlayerCommandException({
    required this.message,
    required this.code,
    required this.category,
    required this.retriable,
    this.details = const <String, Object?>{},
    this.codeRawValue,
    this.categoryRawValue,
    this.platformCode,
  });

  factory VesperPlayerCommandException.fromPlatformException(
    PlatformException error,
  ) {
    final root = _commandExceptionMap(error.details);
    final nested = _commandExceptionMap(root['details']);
    final payload = <Object?, Object?>{...root, ...nested};
    final rawCode = _firstCommandString(<Object?>[payload['code']]);
    final rawCategory = _firstCommandString(<Object?>[payload['category']]);
    return VesperPlayerCommandException(
      message: _firstCommandString(<Object?>[
            payload['message'],
            error.message,
            rawCode,
          ]) ??
          'Vesper player command failed.',
      code: _commandCodeFromWire(rawCode),
      category: _commandCategoryFromWire(rawCategory),
      retriable: payload['retriable'] is bool && payload['retriable'] == true,
      details: Map<String, Object?>.unmodifiable(
        payload.map((key, value) => MapEntry(key.toString(), value))
          ..remove('message')
          ..remove('code')
          ..remove('category')
          ..remove('retriable')
          ..remove('details'),
      ),
      codeRawValue: rawCode,
      categoryRawValue: rawCategory,
      platformCode: error.code,
    );
  }

  static VesperPlayerCommandException? tryFromPlatformException(
    PlatformException error,
  ) {
    final root = _commandExceptionMap(error.details);
    final nested = _commandExceptionMap(root['details']);
    final hasStructuredError =
        (root['code'] is String && root['category'] is String) ||
            (nested['code'] is String && nested['category'] is String);
    final hasCommandMetadata =
        _firstCommandString(<Object?>[nested['commandReason']]) != null &&
            _isCommandIdentifier(nested['commandId']) &&
            _isCommandIdentifier(nested['sourceEpoch']);
    return hasStructuredError && hasCommandMetadata
        ? VesperPlayerCommandException.fromPlatformException(error)
        : null;
  }

  final String message;
  final VesperPlayerErrorCode code;
  final VesperPlayerErrorCategory category;
  final bool retriable;
  final Map<String, Object?> details;
  final String? codeRawValue;
  final String? categoryRawValue;
  final String? platformCode;

  /// Whether this failure belongs only to an obsolete command generation.
  bool get isObsolete => details['obsolete'] == true;

  @override
  String toString() {
    return 'VesperPlayerCommandException('
        '${codeRawValue ?? code.name}, category='
        '${categoryRawValue ?? category.name}, retriable=$retriable): $message';
  }
}

Map<Object?, Object?> _commandExceptionMap(Object? value) {
  if (value is Map<Object?, Object?>) {
    return value;
  }
  if (value is Map) {
    return Map<Object?, Object?>.from(value);
  }
  return const <Object?, Object?>{};
}

String? _firstCommandString(Iterable<Object?> values) {
  for (final value in values) {
    if (value is String && value.isNotEmpty) {
      return value;
    }
  }
  return null;
}

bool _isCommandIdentifier(Object? value) {
  if (value is int) {
    return value >= 0;
  }
  if (value is num) {
    return value.isFinite && value >= 0 && value == value.truncateToDouble();
  }
  if (value is String) {
    return RegExp(r'^\d+$').hasMatch(value);
  }
  return false;
}

VesperPlayerErrorCode _commandCodeFromWire(String? raw) {
  for (final value in VesperPlayerErrorCode.values) {
    if (value.name == raw) {
      return value;
    }
  }
  return VesperPlayerErrorCode.unknown;
}

VesperPlayerErrorCategory _commandCategoryFromWire(String? raw) {
  for (final value in VesperPlayerErrorCategory.values) {
    if (value.name == raw) {
      return value;
    }
  }
  return VesperPlayerErrorCategory.unknown;
}
