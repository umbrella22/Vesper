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
    _updateState(() {
      _pendingDownloadTasks = _pendingDownloadTasks
          .where((task) => task.requestId != assetId)
          .toList(growable: false);
      _downloadMessage = error != null
          ? '准备下载任务失败：$error'
          : taskId == null
          ? '创建下载任务失败。'
          : null;
      if (_downloadMessage != null) {
        _appendHostLog(
          severity: ExampleHostLogSeverity.error,
          title: '下载任务创建失败',
          detail: _downloadMessage,
        );
      }
    });
  }

  Future<void> _createRemoteDownloadTask(VesperDownloadManager manager) async {
    final uri = _downloadUrlController.text.trim();
    if (uri.isEmpty) {
      _updateState(() {
        _downloadMessage = '请输入下载 URL。';
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
      VesperDownloadState.completed || VesperDownloadState.removed => true,
    };
    if (!mounted || succeeded) {
      return;
    }
    _showMessage('下载任务操作失败。');
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
    final completedPath = task.assetIndex.completedPath?.trim();
    if (completedPath == null || completedPath.isEmpty) {
      _showMessage('找不到已完成任务的输出文件。');
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
      _showMessage('MP4 合成库未安装。');
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
      _showMessage('已转存到系统相册。');
    } on MissingPluginException {
      if (mounted) {
        _showMessage('当前宿主暂未接入相册导出能力。');
      }
    } on PlatformException catch (error) {
      if (mounted) {
        _showMessage(error.message ?? '转存到系统相册失败。');
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
