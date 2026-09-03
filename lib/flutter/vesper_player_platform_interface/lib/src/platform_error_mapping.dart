import 'package:flutter/services.dart';

import 'models.dart';
import 'fixed_track_exception.dart';
import 'player_command_exception.dart';
import 'performance_diagnostics_exception.dart';
import 'subtitle_exception.dart';
import 'vesper_player_platform.dart' show VesperUnsupportedError;

Object vesperMapPlatformException(PlatformException error) {
  final rawDetails = error.details;
  if (rawDetails is Map && rawDetails['performanceDiagnosticsCode'] is String) {
    return VesperPerformanceDiagnosticsException.fromPlatformException(error);
  }
  final subtitleError = VesperSubtitleException.tryFromPlatformException(error);
  if (subtitleError != null) {
    return subtitleError;
  }
  final fixedTrackError =
      VesperFixedTrackSelectionException.tryFromPlatformException(error);
  if (fixedTrackError != null) {
    return fixedTrackError;
  }
  final commandError =
      VesperPlayerCommandException.tryFromPlatformException(error);
  if (commandError != null) {
    return commandError;
  }
  final details = error.details;
  final normalized = details is Map
      ? Map<Object?, Object?>.from(details)
      : <Object?, Object?>{};
  if (normalized['code'] == VesperPlayerErrorCode.unsupported.name &&
      normalized['category'] == VesperPlayerErrorCategory.capability.name) {
    return VesperUnsupportedError(
      normalized['message'] as String? ?? error.message,
      error.code,
      vesperToStringKeyedMap(normalized),
    );
  }
  return error;
}

Map<String, Object?> vesperToStringKeyedMap(Map<Object?, Object?> source) {
  return source.map((key, value) => MapEntry(key.toString(), value));
}
