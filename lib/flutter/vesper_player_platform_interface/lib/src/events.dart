import 'models.dart';

sealed class VesperPlayerEvent {
  const VesperPlayerEvent({required this.playerId});

  factory VesperPlayerEvent.fromMap(Map<Object?, Object?> map) {
    final type = map['type'] as String? ?? 'snapshot';
    final playerId = map['playerId'] as String? ?? '';

    switch (type) {
      case 'error':
        final rawError = map['error'];
        final errorMap = vesperDecodeMap(rawError);
        final error = errorMap.isNotEmpty
            ? VesperPlayerError.fromMap(errorMap)
            : const VesperPlayerError(message: 'Unknown Vesper player error.');
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
      case 'snapshot':
      default:
        final rawSnapshot = map['snapshot'];
        final snapshotMap = vesperDecodeMap(rawSnapshot);
        final snapshot = snapshotMap.isNotEmpty
            ? VesperPlayerSnapshot.fromMap(snapshotMap)
            : const VesperPlayerSnapshot.initial();
        return VesperPlayerSnapshotEvent(
          playerId: playerId,
          snapshot: snapshot,
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

final class VesperPlayerDisposedEvent extends VesperPlayerEvent {
  const VesperPlayerDisposedEvent({required super.playerId});
}
