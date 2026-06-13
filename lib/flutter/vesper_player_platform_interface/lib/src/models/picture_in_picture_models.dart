part of '../models.dart';

enum VesperPictureInPictureStatus {
  inactive,
  entering,
  active,
  exiting,
  failed,
}

enum VesperPictureInPictureErrorCode {
  pictureInPictureNotSupported,
  pictureInPictureDisabledByHost,
  pictureInPictureSystemPlayerUnavailable,
  pictureInPictureSourceUnsupportedBySystemPlayer,
  pictureInPictureNativeFrameRouteCannotHandOff,
  pictureInPictureSurfaceUnavailable,
  pictureInPicturePlatformRequestRejected,
  pictureInPictureUnavailableForCurrentRoute,
}

final class VesperPictureInPictureConfiguration {
  const VesperPictureInPictureConfiguration({
    this.enabled = true,
    this.autoEnter = false,
    this.preferredAspectRatio,
  });

  factory VesperPictureInPictureConfiguration.fromMap(
    Map<Object?, Object?> map,
  ) {
    return VesperPictureInPictureConfiguration(
      enabled: _decodeBoolValue(map['enabled'], fallback: true),
      autoEnter: _decodeBoolValue(map['autoEnter']),
      preferredAspectRatio: _decodeDouble(map, 'preferredAspectRatio'),
    );
  }

  final bool enabled;
  final bool autoEnter;
  final double? preferredAspectRatio;

  Map<String, Object?> toMap() {
    return <String, Object?>{
      'enabled': enabled,
      'autoEnter': autoEnter,
      if (preferredAspectRatio != null)
        'preferredAspectRatio': preferredAspectRatio,
    };
  }
}

final class VesperPictureInPictureError {
  const VesperPictureInPictureError({
    required this.code,
    this.message = 'Current playback cannot enter Picture in Picture.',
    this.userMessage = 'Current playback cannot enter Picture in Picture.',
    this.diagnostics = const <String, Object?>{},
  });

  factory VesperPictureInPictureError.fromMap(Map<Object?, Object?> map) {
    return VesperPictureInPictureError(
      code: _decodeEnum(
        VesperPictureInPictureErrorCode.values,
        map['code'],
        VesperPictureInPictureErrorCode
            .pictureInPictureUnavailableForCurrentRoute,
      ),
      message: map['message'] as String? ??
          'Current playback cannot enter Picture in Picture.',
      userMessage: map['userMessage'] as String? ??
          'Current playback cannot enter Picture in Picture.',
      diagnostics: _decodeObjectMap(map['diagnostics']),
    );
  }

  final VesperPictureInPictureErrorCode code;
  final String message;
  final String userMessage;
  final Map<String, Object?> diagnostics;

  Map<String, Object?> toMap() {
    return <String, Object?>{
      'code': code.name,
      'message': message,
      'userMessage': userMessage,
      if (diagnostics.isNotEmpty) 'diagnostics': diagnostics,
    };
  }
}

final class VesperPictureInPictureAvailability {
  const VesperPictureInPictureAvailability({
    required this.isAvailable,
    this.isActive = false,
    this.canAutoEnter = false,
    this.source = 'system',
    this.error,
    this.diagnostics = const <String, Object?>{},
  });

  factory VesperPictureInPictureAvailability.fromMap(
    Map<Object?, Object?> map,
  ) {
    final error = _rawMap(map['error']);
    return VesperPictureInPictureAvailability(
      isAvailable: _decodeBoolValue(map['isAvailable']),
      isActive: _decodeBoolValue(map['isActive']),
      canAutoEnter: _decodeBoolValue(map['canAutoEnter']),
      source: map['source'] as String? ?? 'system',
      error: error != null ? VesperPictureInPictureError.fromMap(error) : null,
      diagnostics: _decodeObjectMap(map['diagnostics']),
    );
  }

  final bool isAvailable;
  final bool isActive;
  final bool canAutoEnter;
  final String source;
  final VesperPictureInPictureError? error;
  final Map<String, Object?> diagnostics;

  Map<String, Object?> toMap() {
    return <String, Object?>{
      'isAvailable': isAvailable,
      'isActive': isActive,
      'canAutoEnter': canAutoEnter,
      'source': source,
      if (error != null) 'error': error!.toMap(),
      if (diagnostics.isNotEmpty) 'diagnostics': diagnostics,
    };
  }
}

bool _decodeBoolValue(Object? raw, {bool fallback = false}) {
  if (raw is bool) {
    return raw;
  }
  if (raw is String) {
    return raw == 'true';
  }
  if (raw is num) {
    return raw != 0;
  }
  return fallback;
}
