import 'download_models.dart';
import 'models.dart';

sealed class VesperDownloadManagerEvent {
  const VesperDownloadManagerEvent({required this.downloadId});

  factory VesperDownloadManagerEvent.fromMap(Map<Object?, Object?> map) {
    final normalized = vesperDecodeMap(map);
    final type = normalized['type'] as String? ?? 'snapshot';
    final downloadId = normalized['downloadId'] as String? ?? '';

    switch (type) {
      case 'error':
        final snapshot = VesperDownloadSnapshot.fromMap(
          vesperDecodeMap(normalized['snapshot']),
        );
        final error = VesperDownloadError.fromMap(
          vesperDecodeMap(normalized['error']),
        );
        return VesperDownloadErrorEvent(
          downloadId: downloadId,
          error: error,
          snapshot: snapshot,
        );
      case 'exportProgress':
        return VesperDownloadExportProgressEvent(
          downloadId: downloadId,
          taskId: (normalized['taskId'] as num?)?.toInt() ?? 0,
          ratio: (normalized['ratio'] as num?)?.toDouble() ?? 0,
        );
      case 'disposed':
        return VesperDownloadDisposedEvent(downloadId: downloadId);
      case 'snapshot':
      default:
        return VesperDownloadSnapshotEvent(
          downloadId: downloadId,
          snapshot: VesperDownloadSnapshot.fromMap(
            vesperDecodeMap(normalized['snapshot']),
          ),
        );
    }
  }

  final String downloadId;
}

final class VesperDownloadSnapshotEvent extends VesperDownloadManagerEvent {
  const VesperDownloadSnapshotEvent({
    required super.downloadId,
    required this.snapshot,
  });

  final VesperDownloadSnapshot snapshot;
}

final class VesperDownloadErrorEvent extends VesperDownloadManagerEvent {
  const VesperDownloadErrorEvent({
    required super.downloadId,
    required this.error,
    required this.snapshot,
  });

  final VesperDownloadError error;
  final VesperDownloadSnapshot snapshot;
}

final class VesperDownloadDisposedEvent extends VesperDownloadManagerEvent {
  const VesperDownloadDisposedEvent({required super.downloadId});
}

final class VesperDownloadExportProgressEvent
    extends VesperDownloadManagerEvent {
  const VesperDownloadExportProgressEvent({
    required super.downloadId,
    required this.taskId,
    required this.ratio,
  });

  final int taskId;
  final double ratio;
}
