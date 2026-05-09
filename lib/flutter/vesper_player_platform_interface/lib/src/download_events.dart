import 'download_models.dart';
import 'models.dart';

sealed class VesperDownloadManagerEvent {
  const VesperDownloadManagerEvent({required this.downloadId});

  factory VesperDownloadManagerEvent.fromMap(Map<Object?, Object?> map) {
    final normalized = vesperDecodeMap(map);
    final type = normalized['type'] as String?;
    final downloadId = normalized['downloadId'] as String? ?? '';

    switch (type) {
      case 'initialSnapshot':
        return VesperDownloadInitialSnapshotEvent(
          downloadId: downloadId,
          snapshot: VesperDownloadSnapshot.fromMap(
            vesperDecodeMap(normalized['snapshot']),
          ),
        );
      case 'downloadError':
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
      case 'taskCreated':
        return VesperDownloadTaskCreatedEvent(
          downloadId: downloadId,
          task: VesperDownloadTaskSnapshot.fromMap(
            vesperDecodeMap(normalized['task']),
          ),
        );
      case 'taskUpdated':
        final rawTask = normalized['task'];
        final rawPatch = normalized['patch'];
        return VesperDownloadTaskUpdatedEvent(
          downloadId: downloadId,
          task: rawTask == null
              ? null
              : VesperDownloadTaskSnapshot.fromMap(vesperDecodeMap(rawTask)),
          patch: rawPatch == null
              ? null
              : VesperDownloadTaskStatePatch.fromMap(
                  vesperDecodeMap(rawPatch),
                ),
          progressPatch: normalized['progressPatch'] == null
              ? null
              : VesperDownloadTaskProgressPatch.fromMap(
                  vesperDecodeMap(normalized['progressPatch']),
                ),
        );
      case 'taskRemoved':
        return VesperDownloadTaskRemovedEvent(
          downloadId: downloadId,
          taskId: (normalized['taskId'] as num?)?.toInt() ?? 0,
        );
      case 'disposed':
        return VesperDownloadDisposedEvent(downloadId: downloadId);
      default:
        throw FormatException(
            'Unknown download event type: ${type ?? '<missing>'}');
    }
  }

  final String downloadId;
}

final class VesperDownloadInitialSnapshotEvent
    extends VesperDownloadManagerEvent {
  const VesperDownloadInitialSnapshotEvent({
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

final class VesperDownloadTaskCreatedEvent extends VesperDownloadManagerEvent {
  const VesperDownloadTaskCreatedEvent({
    required super.downloadId,
    required this.task,
  });

  final VesperDownloadTaskSnapshot task;
}

final class VesperDownloadTaskUpdatedEvent extends VesperDownloadManagerEvent {
  const VesperDownloadTaskUpdatedEvent({
    required super.downloadId,
    this.task,
    this.patch,
    this.progressPatch,
  });

  final VesperDownloadTaskSnapshot? task;
  final VesperDownloadTaskStatePatch? patch;
  final VesperDownloadTaskProgressPatch? progressPatch;
}

final class VesperDownloadTaskRemovedEvent extends VesperDownloadManagerEvent {
  const VesperDownloadTaskRemovedEvent({
    required super.downloadId,
    required this.taskId,
  });

  final int taskId;
}
