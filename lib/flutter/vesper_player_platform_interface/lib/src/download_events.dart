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
      case 'downloadResync':
        final droppedEvents = normalized['droppedEvents'];
        if (droppedEvents is! int || droppedEvents < 0) {
          throw const FormatException(
            'downloadResync droppedEvents must be a non-negative integer.',
          );
        }
        return VesperDownloadResyncEvent(
          downloadId: downloadId,
          snapshot: _decodeAuthoritativeDownloadSnapshot(
            normalized['snapshot'],
          ),
          droppedEvents: droppedEvents,
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
        return VesperDownloadUnknownEvent(
          downloadId: downloadId,
          type: type ?? '<missing>',
          payload: vesperDecodeMap(normalized),
        );
    }
  }

  final String downloadId;
}

VesperDownloadSnapshot _decodeAuthoritativeDownloadSnapshot(Object? raw) {
  if (raw is! Map) {
    throw const FormatException(
      'downloadResync snapshot must be a map.',
    );
  }
  final snapshot = vesperDecodeMap(raw);
  final rawTasks = snapshot['tasks'];
  if (rawTasks is! List) {
    throw const FormatException(
      'downloadResync snapshot.tasks must be a list.',
    );
  }

  final tasks = <VesperDownloadTaskSnapshot>[];
  for (var index = 0; index < rawTasks.length; index += 1) {
    final rawTask = rawTasks[index];
    if (rawTask is! Map) {
      throw FormatException(
        'downloadResync snapshot.tasks[$index] must be a map.',
      );
    }
    final task = vesperDecodeMap(rawTask);
    _validateAuthoritativeDownloadTask(task, index);
    tasks.add(VesperDownloadTaskSnapshot.fromMap(task));
  }

  return VesperDownloadSnapshot(
    tasks: List<VesperDownloadTaskSnapshot>.unmodifiable(tasks),
  );
}

void _validateAuthoritativeDownloadTask(
  Map<String, Object?> task,
  int index,
) {
  final taskId = task['taskId'];
  if (taskId is! int || taskId <= 0) {
    throw FormatException(
      'downloadResync snapshot.tasks[$index].taskId must be a positive integer.',
    );
  }
  _requireNonEmptyString(task, 'assetId', 'snapshot.tasks[$index]');
  final source = _requireMap(task, 'source', 'snapshot.tasks[$index]');
  final playerSource = _requireMap(
    source,
    'source',
    'snapshot.tasks[$index].source',
  );
  _requireNonEmptyString(
    playerSource,
    'uri',
    'snapshot.tasks[$index].source.source',
  );
  _requireMap(task, 'profile', 'snapshot.tasks[$index]');
  _requireNonEmptyString(task, 'state', 'snapshot.tasks[$index]');
  _requireMap(task, 'progress', 'snapshot.tasks[$index]');
  _requireMap(task, 'assetIndex', 'snapshot.tasks[$index]');
  final error = task['error'];
  if (error != null && error is! Map) {
    throw FormatException(
      'downloadResync snapshot.tasks[$index].error must be a map or null.',
    );
  }
}

Map<String, Object?> _requireMap(
  Map<String, Object?> map,
  String key,
  String context,
) {
  final value = map[key];
  if (value is! Map) {
    throw FormatException('downloadResync $context.$key must be a map.');
  }
  return vesperDecodeMap(value);
}

String _requireNonEmptyString(
  Map<String, Object?> map,
  String key,
  String context,
) {
  final value = map[key];
  if (value is! String || value.isEmpty) {
    throw FormatException(
      'downloadResync $context.$key must be a non-empty string.',
    );
  }
  return value;
}

final class VesperDownloadInitialSnapshotEvent
    extends VesperDownloadManagerEvent {
  const VesperDownloadInitialSnapshotEvent({
    required super.downloadId,
    required this.snapshot,
  });

  final VesperDownloadSnapshot snapshot;
}

final class VesperDownloadResyncEvent extends VesperDownloadManagerEvent {
  const VesperDownloadResyncEvent({
    required super.downloadId,
    required this.snapshot,
    required this.droppedEvents,
  });

  final VesperDownloadSnapshot snapshot;
  final int droppedEvents;
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

final class VesperDownloadUnknownEvent extends VesperDownloadManagerEvent {
  const VesperDownloadUnknownEvent({
    required super.downloadId,
    required this.type,
    this.payload = const <String, Object?>{},
  });

  final String type;
  final Map<String, Object?> payload;
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
