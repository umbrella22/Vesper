import 'dart:async';

import 'package:flutter/foundation.dart';
import 'package:vesper_player_platform_interface/vesper_player_platform_interface.dart';

class VesperDownloadManager {
  VesperDownloadManager._({
    required this.downloadId,
    required VesperDownloadSnapshot initialSnapshot,
    required VesperPlayerPlatform platform,
  })  : _platform = platform,
        snapshotListenable = ValueNotifier<VesperDownloadSnapshot>(
          initialSnapshot,
        ) {
    _replaceTasks(initialSnapshot.tasks);
    _snapshotsController.add(initialSnapshot);
    _bindPlatformEvents();
  }

  static Future<VesperDownloadManager> create({
    VesperDownloadConfiguration configuration =
        const VesperDownloadConfiguration(),
    VesperDownloadStaleResourcePlanRecoveryCallback? staleResourceRecovery,
  }) async {
    final platform = VesperPlayerPlatform.instance;
    final result = await platform.createDownloadManager(
      configuration: configuration,
      staleResourceRecovery: staleResourceRecovery,
    );
    return VesperDownloadManager._(
      downloadId: result.downloadId,
      initialSnapshot: result.snapshot,
      platform: platform,
    );
  }

  final String downloadId;
  final VesperPlayerPlatform _platform;
  final ValueNotifier<VesperDownloadSnapshot> snapshotListenable;
  final StreamController<VesperDownloadManagerEvent> _eventsController =
      StreamController<VesperDownloadManagerEvent>.broadcast();
  final StreamController<VesperDownloadSnapshot> _snapshotsController =
      StreamController<VesperDownloadSnapshot>.broadcast();

  StreamSubscription<VesperDownloadManagerEvent>? _platformSubscription;
  final Map<int, VesperDownloadTaskSnapshot> _tasksById =
      <int, VesperDownloadTaskSnapshot>{};
  bool _disposed = false;

  VesperDownloadSnapshot get snapshot => snapshotListenable.value;

  Stream<VesperDownloadManagerEvent> get events => _eventsController.stream;

  Stream<VesperDownloadSnapshot> get snapshots => _snapshotsController.stream;

  VesperDownloadTaskSnapshot? task(int taskId) {
    return _tasksById[taskId];
  }

  List<VesperDownloadTaskSnapshot> tasksForAsset(String assetId) {
    return snapshot.tasks
        .where((value) => value.assetId == assetId)
        .toList(growable: false);
  }

  Future<void> refresh() {
    _ensureActive();
    return _platform.refreshDownloadManager(downloadId);
  }

  Future<int?> createTask({
    required String assetId,
    required VesperDownloadSource source,
    VesperDownloadProfile profile = const VesperDownloadProfile(),
    VesperDownloadAssetIndex assetIndex = const VesperDownloadAssetIndex(),
  }) {
    _ensureActive();
    return _platform.createDownloadTask(
      downloadId,
      assetId: assetId,
      source: source,
      profile: profile,
      assetIndex: assetIndex,
    );
  }

  Future<bool> startTask(int taskId) {
    _ensureActive();
    return _platform.startDownloadTask(downloadId, taskId);
  }

  Future<bool> pauseTask(int taskId) {
    _ensureActive();
    return _platform.pauseDownloadTask(downloadId, taskId);
  }

  Future<bool> resumeTask(int taskId) {
    _ensureActive();
    return _platform.resumeDownloadTask(downloadId, taskId);
  }

  Future<bool> removeTask(int taskId) {
    _ensureActive();
    return _platform.removeDownloadTask(downloadId, taskId);
  }

  Future<void> exportTaskOutput(int taskId, String outputPath) {
    _ensureActive();
    return _platform.exportDownloadTask(downloadId, taskId, outputPath);
  }

  Future<void> shareTaskOutput(
    int taskId, {
    String? fileName,
    String? mimeType,
  }) {
    _ensureActive();
    return _platform.shareDownloadTask(
      downloadId,
      taskId,
      fileName: fileName,
      mimeType: mimeType,
    );
  }

  Future<String?> saveTaskOutput(
    int taskId, {
    String? fileName,
    VesperDownloadPublicCollection collection =
        VesperDownloadPublicCollection.downloads,
  }) {
    _ensureActive();
    return _platform.saveDownloadTask(
      downloadId,
      taskId,
      fileName: fileName,
      collection: collection,
    );
  }

  Future<void> dispose() async {
    if (_disposed) {
      return;
    }
    _disposed = true;

    Object? platformError;
    StackTrace? platformStackTrace;

    try {
      await _platform.disposeDownloadManager(downloadId);
    } catch (error, stackTrace) {
      platformError = error;
      platformStackTrace = stackTrace;
    }

    await _guardDisposeCleanup(
      () => _platformSubscription?.cancel(),
      context: 'cancel download event subscription',
    );
    _eventsController.add(VesperDownloadDisposedEvent(downloadId: downloadId));
    await _guardDisposeCleanup(
      _eventsController.close,
      context: 'close download event stream',
    );
    await _guardDisposeCleanup(
      _snapshotsController.close,
      context: 'close download snapshot stream',
    );
    _guardDisposeSyncCleanup(
      snapshotListenable.dispose,
      context: 'dispose download snapshot listenable',
    );
    _tasksById.clear();

    if (platformError != null) {
      Error.throwWithStackTrace(platformError, platformStackTrace!);
    }
  }

  void _bindPlatformEvents() {
    _platformSubscription =
        _platform.downloadEventsFor(downloadId).listen((event) {
      switch (event) {
        case VesperDownloadInitialSnapshotEvent():
          _applySnapshot(event.snapshot, forwardEvent: event);
        case VesperDownloadErrorEvent():
          _applyErrorEvent(event);
        case VesperDownloadExportProgressEvent():
          _eventsController.add(event);
        case VesperDownloadTaskCreatedEvent():
          _applyTaskCreatedEvent(event);
        case VesperDownloadTaskUpdatedEvent():
          _applyTaskUpdatedEvent(event);
        case VesperDownloadTaskRemovedEvent():
          _applyTaskRemovedEvent(event);
        case VesperDownloadDisposedEvent():
          _eventsController.add(event);
      }
    });
  }

  void _applySnapshot(
    VesperDownloadSnapshot snapshot, {
    required VesperDownloadManagerEvent forwardEvent,
  }) {
    if (_disposed) {
      return;
    }
    _replaceTasks(snapshot.tasks);
    snapshotListenable.value = snapshot;
    _snapshotsController.add(snapshot);
    _eventsController.add(forwardEvent);
  }

  void _applyErrorEvent(VesperDownloadErrorEvent event) {
    if (_disposed) {
      return;
    }
    _replaceTasks(event.snapshot.tasks);
    snapshotListenable.value = event.snapshot;
    _snapshotsController.add(event.snapshot);
    _eventsController.add(event);
  }

  void _applyTaskCreatedEvent(VesperDownloadTaskCreatedEvent event) {
    if (_disposed) {
      return;
    }
    _tasksById[event.task.taskId] = event.task;
    _publishTaskMap(event);
  }

  void _applyTaskUpdatedEvent(VesperDownloadTaskUpdatedEvent event) {
    if (_disposed) {
      return;
    }
    final task = event.task;
    if (task != null) {
      _tasksById[task.taskId] = task;
    }
    final patch = event.patch;
    if (patch != null) {
      final existing = _tasksById[patch.taskId];
      if (existing != null) {
        _tasksById[patch.taskId] = existing.copyWith(
          state: patch.state,
          progress: patch.progress,
          assetIndex: _assetIndexWithCompletedPath(
            existing.assetIndex,
            patch.completedPath,
          ),
          error: patch.error,
        );
      }
    }
    final progressPatch = event.progressPatch;
    if (progressPatch != null) {
      final existing = _tasksById[progressPatch.taskId];
      if (existing != null) {
        _tasksById[progressPatch.taskId] = existing.copyWith(
          progress: progressPatch.progress,
        );
      }
    }
    _publishTaskMap(event);
  }

  void _applyTaskRemovedEvent(VesperDownloadTaskRemovedEvent event) {
    if (_disposed) {
      return;
    }
    _tasksById.remove(event.taskId);
    _publishTaskMap(event);
  }

  void _publishTaskMap(VesperDownloadManagerEvent event) {
    final updatedSnapshot = VesperDownloadSnapshot(
      tasks: List<VesperDownloadTaskSnapshot>.unmodifiable(
        _tasksById.values,
      ),
    );
    snapshotListenable.value = updatedSnapshot;
    _snapshotsController.add(updatedSnapshot);
    _eventsController.add(event);
  }

  void _replaceTasks(List<VesperDownloadTaskSnapshot> tasks) {
    _tasksById
      ..clear()
      ..addEntries(
          tasks.map((task) => MapEntry<int, VesperDownloadTaskSnapshot>(
                task.taskId,
                task,
              )));
  }

  VesperDownloadAssetIndex _assetIndexWithCompletedPath(
    VesperDownloadAssetIndex assetIndex,
    String? completedPath,
  ) {
    return VesperDownloadAssetIndex(
      contentFormat: assetIndex.contentFormat,
      version: assetIndex.version,
      etag: assetIndex.etag,
      checksum: assetIndex.checksum,
      totalSizeBytes: assetIndex.totalSizeBytes,
      resources: assetIndex.resources,
      segments: assetIndex.segments,
      completedPath: completedPath ?? assetIndex.completedPath,
    );
  }

  void _ensureActive() {
    if (_disposed) {
      throw StateError('VesperDownloadManager has already been disposed.');
    }
  }

  Future<void> _guardDisposeCleanup(
    FutureOr<void> Function() cleanup, {
    required String context,
  }) async {
    try {
      await cleanup();
    } catch (error, stackTrace) {
      FlutterError.reportError(
        FlutterErrorDetails(
          exception: error,
          stack: stackTrace,
          library: 'vesper_player',
          context: ErrorDescription(context),
        ),
      );
    }
  }

  void _guardDisposeSyncCleanup(
    VoidCallback cleanup, {
    required String context,
  }) {
    try {
      cleanup();
    } catch (error, stackTrace) {
      FlutterError.reportError(
        FlutterErrorDetails(
          exception: error,
          stack: stackTrace,
          library: 'vesper_player',
          context: ErrorDescription(context),
        ),
      );
    }
  }
}
