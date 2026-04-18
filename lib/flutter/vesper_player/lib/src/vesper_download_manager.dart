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
    _snapshotsController.add(initialSnapshot);
    _bindPlatformEvents();
  }

  static Future<VesperDownloadManager> create({
    VesperDownloadConfiguration configuration =
        const VesperDownloadConfiguration(),
  }) async {
    final platform = VesperPlayerPlatform.instance;
    final result = await platform.createDownloadManager(
      configuration: configuration,
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
  bool _disposed = false;

  VesperDownloadSnapshot get snapshot => snapshotListenable.value;

  Stream<VesperDownloadManagerEvent> get events => _eventsController.stream;

  Stream<VesperDownloadSnapshot> get snapshots => _snapshotsController.stream;

  VesperDownloadTaskSnapshot? task(int taskId) {
    for (final value in snapshot.tasks) {
      if (value.taskId == taskId) {
        return value;
      }
    }
    return null;
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
    } finally {
      await _platformSubscription?.cancel();
      _eventsController
          .add(VesperDownloadDisposedEvent(downloadId: downloadId));
      await _eventsController.close();
      await _snapshotsController.close();
      snapshotListenable.dispose();
    }

    if (platformError != null) {
      Error.throwWithStackTrace(platformError, platformStackTrace!);
    }
  }

  void _bindPlatformEvents() {
    _platformSubscription =
        _platform.downloadEventsFor(downloadId).listen((event) {
      switch (event) {
        case VesperDownloadSnapshotEvent():
          _applySnapshot(event.snapshot);
        case VesperDownloadErrorEvent():
          _applyErrorEvent(event);
        case VesperDownloadExportProgressEvent():
          _eventsController.add(event);
        case VesperDownloadDisposedEvent():
          _eventsController.add(event);
      }
    });
  }

  void _applySnapshot(VesperDownloadSnapshot snapshot) {
    if (_disposed) {
      return;
    }
    snapshotListenable.value = snapshot;
    _snapshotsController.add(snapshot);
    _eventsController.add(
      VesperDownloadSnapshotEvent(
        downloadId: downloadId,
        snapshot: snapshot,
      ),
    );
  }

  void _applyErrorEvent(VesperDownloadErrorEvent event) {
    if (_disposed) {
      return;
    }
    snapshotListenable.value = event.snapshot;
    _snapshotsController.add(event.snapshot);
    _eventsController.add(event);
  }

  void _ensureActive() {
    if (_disposed) {
      throw StateError('VesperDownloadManager has already been disposed.');
    }
  }
}
