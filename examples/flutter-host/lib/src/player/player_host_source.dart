part of 'player_host_page.dart';

extension _PlayerHostSourceActions on _PlayerHostPageState {
  Future<VesperPlayerController> _createController({
    VesperPlayerSource? initialSource,
    bool preservePlaylistState = false,
  }) async {
    VesperPlayerController? nextController;
    try {
      final frameProcessorPluginPaths =
          await ExampleLocalMediaPicker.bundledFrameProcessorPluginLibraryPaths();
      if (mounted) {
        _updateState(() {
          _sourceNormalizerPluginLibraryPaths = const <String>[];
          _frameProcessorPluginLibraryPaths = frameProcessorPluginPaths;
        });
      } else {
        _sourceNormalizerPluginLibraryPaths = const <String>[];
        _frameProcessorPluginLibraryPaths = frameProcessorPluginPaths;
      }

      final selectedSource = initialSource ?? flutterHlsDemoSource();
      nextController = await VesperPlayerController.create(
        initialSource: selectedSource,
        renderSurfaceKind: VesperPlayerRenderSurfaceKind.surfaceView,
        resiliencePolicy: _selectedResilienceProfile.policy,
        sourceNormalizerConfiguration: _sourceNormalizerConfiguration(),
        frameProcessorConfiguration: VesperFrameProcessorConfiguration(
          mode: frameProcessorPluginPaths.isEmpty
              ? VesperFrameProcessorMode.disabled
              : VesperFrameProcessorMode.diagnosticsOnly,
          pluginLibraryPaths: frameProcessorPluginPaths,
        ),
      );
      await nextController.initialize();
      await _configureSystemPlayback(nextController, selectedSource);
      await _bindPictureInPicture(nextController);
      if (!preservePlaylistState) {
        _playlistItemIds = <String>[flutterHlsPlaylistItemId];
        _activePlaylistItemId = flutterHlsPlaylistItemId;
      }

      final previous = _controller;
      _controller = nextController;
      if (previous != null && !identical(previous, nextController)) {
        _disposeControllerSilently(previous);
      }
      return nextController;
    } catch (_) {
      if (nextController != null) {
        _disposeControllerSilently(nextController);
      }
      rethrow;
    }
  }

  VesperSourceNormalizerConfiguration _sourceNormalizerConfiguration() {
    switch (_sourceNormalizerSetting.mode) {
      case VesperSourceNormalizerMode.preferNormalized:
        return const VesperSourceNormalizerConfiguration.preferBundled();
      case VesperSourceNormalizerMode.requireNormalized:
        return const VesperSourceNormalizerConfiguration.requireBundled();
      case VesperSourceNormalizerMode.disabled:
      case VesperSourceNormalizerMode.diagnosticsOnly:
      case VesperSourceNormalizerMode.preflightOnly:
        return VesperSourceNormalizerConfiguration(
          mode: _sourceNormalizerSetting.mode,
        );
    }
  }

  Future<void> _applySourceNormalizerSetting(
    ExampleSourceNormalizerSetting setting,
  ) async {
    if (setting == _sourceNormalizerSetting || _isRebuildingController) {
      return;
    }

    final previousController = _controller ?? await _controllerFuture;
    final activeSource = _activePlaylistItemId == null
        ? null
        : _playlistSourceForItem(_activePlaylistItemId!);
    final previousSnapshot = previousController.snapshot;
    final restorePositionMs = previousSnapshot.timeline.positionMs;
    final shouldResumePlayback =
        previousSnapshot.playbackState == VesperPlaybackState.playing;

    _updateState(() {
      _sourceNormalizerSetting = setting;
      _isRebuildingController = true;
      _controllerFuture = _createController(
        initialSource: activeSource,
        preservePlaylistState: true,
      );
    });

    try {
      final nextController = await _controllerFuture;
      if (restorePositionMs > 0) {
        await nextController.seekBy(restorePositionMs);
      }
      if (shouldResumePlayback) {
        await nextController.play();
      }
    } catch (error) {
      if (mounted) {
        _showMessage('SourceNormalizer 配置切换失败：$error');
      }
    } finally {
      if (mounted) {
        _updateState(() {
          _isRebuildingController = false;
        });
      }
    }
  }

  Future<VesperDownloadManager> _createDownloadManager() async {
    final pluginLibraryPaths =
        await ExampleLocalMediaPicker.bundledDownloadPluginLibraryPaths();
    _isDownloadExportPluginInstalled = pluginLibraryPaths.isNotEmpty;
    final manager = await VesperDownloadManager.create(
      configuration: VesperDownloadConfiguration(
        runPostProcessorsOnCompletion: false,
        pluginLibraryPaths: pluginLibraryPaths,
      ),
    );
    await (_downloadEventsSubscription?.cancel() ?? Future<void>.value());
    _downloadEventsSubscription = manager.events.listen(_handleDownloadEvent);
    final previous = _downloadManager;
    _downloadManager = manager;
    if (previous != null && !identical(previous, manager)) {
      _disposeDownloadManagerSilently(previous);
    }
    return manager;
  }

  void _handleDownloadEvent(VesperDownloadManagerEvent event) {
    if (!mounted) {
      return;
    }
    switch (event) {
      case VesperDownloadExportProgressEvent():
        _updateState(() {
          _exportProgressByTaskId[event.taskId] = event.ratio
              .clamp(0, 1)
              .toDouble();
        });
      case VesperDownloadInitialSnapshotEvent():
      case VesperDownloadTaskCreatedEvent():
      case VesperDownloadTaskUpdatedEvent():
      case VesperDownloadTaskRemovedEvent():
      case VesperDownloadErrorEvent():
      case VesperDownloadDisposedEvent():
        break;
    }
  }

  Future<VesperDownloadManager> _ensureDownloadManagerFuture() {
    final existingFuture = _downloadManagerFuture;
    if (existingFuture != null) {
      return existingFuture;
    }
    final future = _createDownloadManager();
    _downloadManagerFuture = future;
    return future;
  }

  Future<void> _applyResilienceProfile(ExampleResilienceProfile profile) async {
    if (profile == _selectedResilienceProfile) {
      return;
    }
    final controller = _controller ?? await _controllerFuture;
    final previousProfile = _selectedResilienceProfile;
    _updateState(() {
      _selectedResilienceProfile = profile;
      _isApplyingResilienceProfile = true;
    });
    try {
      await controller.setResiliencePolicy(profile.policy);
    } catch (_) {
      if (mounted) {
        _updateState(() {
          _selectedResilienceProfile = previousProfile;
        });
      }
      rethrow;
    } finally {
      if (mounted) {
        _updateState(() {
          _isApplyingResilienceProfile = false;
        });
      }
    }
  }

  Future<void> _selectSource(
    VesperPlayerController controller,
    VesperPlayerSource source,
  ) async {
    if (source.kind == VesperPlayerSourceKind.remote) {
      _remoteUrlController.text = source.uri;
    }
    if (_sourceNormalizerSetting != ExampleSourceNormalizerSetting.disabled &&
        _sourceNormalizerSetting !=
            ExampleSourceNormalizerSetting.diagnosticsOnly) {
      await _rebuildControllerForSource(
        source,
        shouldResumePlayback:
            controller.snapshot.playbackState == VesperPlaybackState.playing,
      );
      return;
    }
    await controller.selectSource(source);
    await _configureSystemPlayback(controller, source);
  }

  Future<void> _rebuildControllerForSource(
    VesperPlayerSource source, {
    required bool shouldResumePlayback,
  }) async {
    if (_isRebuildingController) {
      return;
    }

    _updateState(() {
      _isRebuildingController = true;
      _controllerFuture = _createController(
        initialSource: source,
        preservePlaylistState: true,
      );
    });

    try {
      final nextController = await _controllerFuture;
      if (shouldResumePlayback) {
        await nextController.play();
      }
    } finally {
      if (mounted) {
        _updateState(() {
          _isRebuildingController = false;
        });
      }
    }
  }

  Future<void> _configureSystemPlayback(
    VesperPlayerController controller,
    VesperPlayerSource source,
  ) async {
    final permissionStatus = await controller
        .getSystemPlaybackPermissionStatus();
    if (mounted) {
      _updateState(() {
        _systemPlaybackPermissionStatus = permissionStatus;
      });
    }
    await controller.configureSystemPlayback(
      VesperSystemPlaybackConfiguration(
        metadata: _systemPlaybackMetadataForSource(source),
        controls: const VesperSystemPlaybackControls.videoDefault(),
      ),
    );
  }

  Future<void> _requestSystemPlaybackPermissions(
    VesperPlayerController controller,
  ) async {
    final permissionStatus = await controller
        .requestSystemPlaybackPermissions();
    if (!mounted) {
      return;
    }
    _updateState(() {
      _systemPlaybackPermissionStatus = permissionStatus;
    });
  }
}
