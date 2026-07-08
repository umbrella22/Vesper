import 'models.dart';

sealed class VesperPlayerEvent {
  const VesperPlayerEvent({required this.playerId});

  factory VesperPlayerEvent.fromMap(Map<Object?, Object?> map) {
    final type = map['type'] as String? ?? 'snapshot';
    final playerId = map['playerId'] as String? ?? '';

    switch (type) {
      case 'pictureInPicture':
        final errorMap = vesperDecodeMap(map['error']);
        final rawState = map['state'];
        final state = _decodePictureInPictureStatus(rawState);
        return VesperPlayerPictureInPictureEvent(
          playerId: playerId,
          state: state,
          stateRawValue: rawState is String && rawState != state.name
              ? rawState
              : null,
          isActive: _decodeEventBool(map['isActive']),
          source: map['source'] as String? ?? 'system',
          error: errorMap.isNotEmpty
              ? VesperPictureInPictureError.fromMap(errorMap)
              : null,
          canAutoEnter: _decodeNullableEventBool(map['canAutoEnter']),
          diagnostics: vesperDecodeMap(map['diagnostics']),
        );
      case 'error':
        final rawError = map['error'];
        final errorMap = vesperDecodeMap(rawError);
        final error = errorMap.isNotEmpty
            ? VesperPlayerError.fromMap(errorMap)
            : const VesperPlayerError(
                message: 'Unknown Vesper player error.',
                code: VesperPlayerErrorCode.backendFailure,
                category: VesperPlayerErrorCategory.platform,
                retriable: false,
              );
        final rawSnapshot = map['snapshot'];
        final snapshotMap = vesperDecodeMap(rawSnapshot);
        return VesperPlayerErrorEvent(
          playerId: playerId,
          error: error,
          snapshot: snapshotMap.isNotEmpty
              ? VesperPlayerSnapshot.fromMap(snapshotMap)
              : null,
        );
      case 'disposed':
        return VesperPlayerDisposedEvent(playerId: playerId);
      case 'warning':
        final rawWarning = map['warning'];
        final warningMap = vesperDecodeMap(rawWarning);
        return VesperPlayerWarningEvent(
          playerId: playerId,
          warning: VesperRuntimeWarning.fromMap(warningMap),
        );
      case 'snapshot':
        final rawSnapshot = map['snapshot'];
        final snapshotMap = vesperDecodeMap(rawSnapshot);
        final snapshot = snapshotMap.isNotEmpty
            ? VesperPlayerSnapshot.fromMap(snapshotMap)
            : const VesperPlayerSnapshot.initial();
        return VesperPlayerSnapshotEvent(
          playerId: playerId,
          snapshot: snapshot,
        );
      default:
        return VesperPlayerUnknownEvent(
          playerId: playerId,
          type: type,
          payload: vesperDecodeMap(map),
        );
    }
  }

  final String playerId;
}

final class VesperPlayerSnapshotEvent extends VesperPlayerEvent {
  const VesperPlayerSnapshotEvent({
    required super.playerId,
    required this.snapshot,
  });

  final VesperPlayerSnapshot snapshot;
}

final class VesperPlayerErrorEvent extends VesperPlayerEvent {
  const VesperPlayerErrorEvent({
    required super.playerId,
    required this.error,
    this.snapshot,
  });

  final VesperPlayerError error;
  final VesperPlayerSnapshot? snapshot;
}

final class VesperPlayerWarningEvent extends VesperPlayerEvent {
  const VesperPlayerWarningEvent({
    required super.playerId,
    required this.warning,
  });

  final VesperRuntimeWarning warning;
}

final class VesperPlayerPictureInPictureEvent extends VesperPlayerEvent {
  const VesperPlayerPictureInPictureEvent({
    required super.playerId,
    required this.state,
    required this.isActive,
    this.stateRawValue,
    this.source = 'system',
    this.error,
    this.canAutoEnter,
    this.diagnostics = const <String, Object?>{},
  });

  final VesperPictureInPictureStatus state;
  final String? stateRawValue;
  final bool isActive;
  final String source;
  final VesperPictureInPictureError? error;
  final bool? canAutoEnter;
  final Map<String, Object?> diagnostics;
}

final class VesperPlayerDisposedEvent extends VesperPlayerEvent {
  const VesperPlayerDisposedEvent({required super.playerId});
}

final class VesperPlayerUnknownEvent extends VesperPlayerEvent {
  const VesperPlayerUnknownEvent({
    required super.playerId,
    required this.type,
    this.payload = const <String, Object?>{},
  });

  final String type;
  final Map<String, Object?> payload;
}

VesperPictureInPictureStatus _decodePictureInPictureStatus(Object? raw) {
  if (raw is String) {
    for (final value in VesperPictureInPictureStatus.values) {
      if (value.name == raw) {
        return value;
      }
    }
  }
  return VesperPictureInPictureStatus.inactive;
}

bool _decodeEventBool(Object? raw) => _decodeNullableEventBool(raw) ?? false;

bool? _decodeNullableEventBool(Object? raw) {
  if (raw is bool) {
    return raw;
  }
  if (raw is String) {
    return raw == 'true';
  }
  if (raw is num) {
    return raw != 0;
  }
  return null;
}
