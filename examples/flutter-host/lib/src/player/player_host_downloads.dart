part of 'player_host_page.dart';

extension _PlayerHostDownloadActions on _PlayerHostPageState {
  Future<void> _createDownloadTask(
    VesperDownloadManager manager, {
    required String assetIdPrefix,
    required VesperPlayerSource source,
  }) async {
    final assetId = '$assetIdPrefix-${DateTime.now().millisecondsSinceEpoch}';
    _updateState(() {
      _downloadMessage = null;
      _pendingDownloadTasks = <ExamplePendingDownloadTask>[
        ..._pendingDownloadTasks,
        ExamplePendingDownloadTask(
          requestId: assetId,
          assetId: assetId,
          label: exampleDraftDownloadLabelFromSource(source),
          sourceUri: source.uri,
        ),
      ];
    });

    int? taskId;
    Object? error;
    try {
      final preparedTask = await prepareExampleDownloadTask(
        assetId: assetId,
        source: source,
      );
      taskId = await manager.createTask(
        assetId: assetId,
        source: preparedTask.source,
        profile: preparedTask.profile,
        assetIndex: preparedTask.assetIndex,
      );
    } catch (caughtError) {
      error = caughtError;
    }
    if (!mounted) {
      return;
    }
    final strings = _hostStrings;
    _updateState(() {
      _pendingDownloadTasks = _pendingDownloadTasks
          .where((task) => task.requestId != assetId)
          .toList(growable: false);
      _downloadMessage = error != null
          ? strings.format(
              ExampleHostText.downloadPrepareFailed,
              <String, Object?>{'error': error},
            )
          : taskId == null
          ? strings[ExampleHostText.downloadCreateFailed]
          : null;
      if (_downloadMessage != null) {
        _appendHostLog(
          severity: ExampleHostLogSeverity.error,
          title: strings[ExampleHostText.logDownloadCreateFailed],
          detail: _downloadMessage,
        );
      }
    });
  }

  Future<void> _createRemoteDownloadTask(VesperDownloadManager manager) async {
    final uri = _downloadUrlController.text.trim();
    if (uri.isEmpty) {
      _updateState(() {
        _downloadMessage = _hostStrings[ExampleHostText.downloadUrlEmpty];
      });
      return;
    }

    final source = VesperPlayerSource.remote(
      uri: uri,
      label: exampleDraftDownloadLabelFromUri(uri),
      protocol: inferProtocol(uri),
    );
    await _createDownloadTask(
      manager,
      assetIdPrefix: flutterRemotePlaylistItemId,
      source: source,
    );
  }

  Future<void> _handleDownloadPrimaryAction(
    VesperDownloadManager manager,
    VesperDownloadTaskSnapshot task,
  ) async {
    final succeeded = switch (task.state) {
      VesperDownloadState.queued ||
      VesperDownloadState.failed => await manager.startTask(task.taskId),
      VesperDownloadState.preparing ||
      VesperDownloadState.downloading => await manager.pauseTask(task.taskId),
      VesperDownloadState.paused => await manager.resumeTask(task.taskId),
      VesperDownloadState.completed ||
      VesperDownloadState.removed ||
      VesperDownloadState.unknown => true,
    };
    if (!mounted || succeeded) {
      return;
    }
    _showMessage(_hostStrings[ExampleHostText.downloadActionFailed]);
  }

  Future<File> _createDownloadExportFile(
    VesperDownloadTaskSnapshot task,
  ) async {
    final exportDirectory = Directory(
      '${Directory.systemTemp.path}/vesper-exported-videos',
    );
    if (!await exportDirectory.exists()) {
      await exportDirectory.create(recursive: true);
    }
    final trimmedAssetId = task.assetId.trim();
    final safeStem =
        (trimmedAssetId.isEmpty ? 'download-${task.taskId}' : trimmedAssetId)
            .replaceAll(RegExp(r'[^A-Za-z0-9._-]'), '_');
    return File('${exportDirectory.path}/$safeStem.mp4');
  }

  Future<void> _saveDownloadToGallery(
    VesperDownloadManager manager,
    VesperDownloadTaskSnapshot task,
  ) async {
    final strings = _hostStrings;
    final completedPath = task.assetIndex.completedPath?.trim();
    if (completedPath == null || completedPath.isEmpty) {
      _showMessage(strings[ExampleHostText.downloadOutputMissing]);
      return;
    }
    if (_savingTaskIds.contains(task.taskId)) {
      return;
    }

    final needsExport =
        task.source.contentFormat == VesperDownloadContentFormat.hlsSegments ||
        task.source.contentFormat == VesperDownloadContentFormat.dashSegments ||
        task.source.contentFormat == VesperDownloadContentFormat.flvSegments;
    if (needsExport && !_isDownloadExportPluginInstalled) {
      _showMessage(strings[ExampleHostText.downloadExportMissing]);
      return;
    }
    _updateState(() {
      _savingTaskIds = <int>{..._savingTaskIds, task.taskId};
      if (needsExport) {
        _exportProgressByTaskId = <int, double>{
          ..._exportProgressByTaskId,
          task.taskId: 0,
        };
      }
    });

    File? exportFile;
    try {
      final gallerySourcePath = await (() async {
        if (!needsExport) {
          return completedPath;
        }
        exportFile = await _createDownloadExportFile(task);
        if (await exportFile!.exists()) {
          await exportFile!.delete();
        }
        await manager.exportTaskOutput(task.taskId, exportFile!.path);
        return exportFile!.path;
      })();
      await ExampleLocalMediaPicker.saveVideoToGallery(gallerySourcePath);
      if (!mounted) {
        return;
      }
      _showMessage(strings[ExampleHostText.downloadSavedToGallery]);
    } on MissingPluginException {
      if (mounted) {
        _showMessage(strings[ExampleHostText.downloadGalleryUnsupported]);
      }
    } on PlatformException catch (error) {
      if (mounted) {
        _showMessage(
          error.message ?? strings[ExampleHostText.downloadGalleryFailed],
        );
      }
    } finally {
      if (exportFile != null && await exportFile!.exists()) {
        await exportFile!.delete();
      }
      if (mounted) {
        _updateState(() {
          _savingTaskIds = <int>{
            ..._savingTaskIds.where((taskId) => taskId != task.taskId),
          };
          _exportProgressByTaskId = <int, double>{..._exportProgressByTaskId}
            ..remove(task.taskId);
        });
      }
    }
  }

  void _showMessage(String message) {
    final messenger = ScaffoldMessenger.maybeOf(context);
    if (messenger == null) {
      return;
    }
    messenger
      ..hideCurrentSnackBar()
      ..showSnackBar(SnackBar(content: Text(message)));
  }

  void _disposeControllerSilently(VesperPlayerController controller) {
    unawaited(controller.dispose().catchError((_) {}));
  }

  void _disposeDownloadManagerSilently(VesperDownloadManager manager) {
    unawaited(manager.dispose().catchError((_) {}));
  }
}
