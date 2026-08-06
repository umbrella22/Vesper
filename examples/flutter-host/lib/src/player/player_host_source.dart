part of 'player_host_page.dart';

extension _PlayerHostSourceActions on _PlayerHostPageState {
  Future<VesperPlayerController> _createController({
    VesperPlayerSource? initialSource,
    bool preservePlaylistState = false,
  }) async {
    VesperPlayerController? nextController;
    try {
      final selectedSource = initialSource ?? flutterHlsDemoSource();
      final directNativePlaybackRequired =
          exampleDolbyAcceptanceSourceRequiresDirectNativePlayback(
            selectedSource,
          );
      final sourceNormalizerConfiguration = _sourceNormalizerConfiguration(
        directNativePlaybackRequired: directNativePlaybackRequired,
      );
      final frameProcessorPluginReferences = directNativePlaybackRequired
          ? const <VesperPluginReference>[]
          : <VesperPluginReference>[
              VesperBundledPluginReferences.frameProcessorDiagnostic,
            ];
      if (mounted) {
        _updateState(() {
          _sourceNormalizerPluginReferences =
              sourceNormalizerConfiguration.pluginReferences;
          _frameProcessorPluginReferences = frameProcessorPluginReferences;
        });
      } else {
        _sourceNormalizerPluginReferences =
            sourceNormalizerConfiguration.pluginReferences;
        _frameProcessorPluginReferences = frameProcessorPluginReferences;
      }

      nextController = await VesperPlayerController.create(
        initialSource: selectedSource,
        renderSurfaceKind: VesperPlayerRenderSurfaceKind.surfaceView,
        resiliencePolicy: _selectedResilienceProfile.policy,
        sourceNormalizerConfiguration: sourceNormalizerConfiguration,
        frameProcessorConfiguration: VesperFrameProcessorConfiguration(
          mode: frameProcessorPluginReferences.isEmpty
              ? VesperFrameProcessorMode.disabled
              : VesperFrameProcessorMode.diagnosticsOnly,
          pluginReferences: frameProcessorPluginReferences,
        ),
      );
      await nextController.initialize();
      await _configureSystemPlayback(nextController, selectedSource);
      await _bindPictureInPicture(nextController);
      if (!preservePlaylistState) {
        _playlistItemIds = <String>[flutterHlsPlaylistItemId];
        _activePlaylistItemId = flutterHlsPlaylistItemId;
        _activeDirectSource = selectedSource;
        _playbackOrigin = const ExampleQueuePlaybackOrigin(
          flutterHlsPlaylistItemId,
        );
      }

      final previous = _controller;
      _controller = nextController;
      _observeControllerSnapshot(nextController);
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

  VesperSourceNormalizerConfiguration _sourceNormalizerConfiguration({
    bool directNativePlaybackRequired = false,
  }) {
    if (directNativePlaybackRequired) {
      return const VesperSourceNormalizerConfiguration();
    }
    switch (_sourceNormalizerSetting.mode) {
      case VesperSourceNormalizerMode.preferNormalized:
        return VesperSourceNormalizerConfiguration.preferBundled();
      case VesperSourceNormalizerMode.requireNormalized:
        return VesperSourceNormalizerConfiguration.requireBundled();
      case VesperSourceNormalizerMode.disabled:
      case VesperSourceNormalizerMode.diagnosticsOnly:
      case VesperSourceNormalizerMode.preflightOnly:
        return VesperSourceNormalizerConfiguration(
          mode: _sourceNormalizerSetting.mode,
          pluginReferences:
              _sourceNormalizerSetting.mode ==
                  VesperSourceNormalizerMode.disabled
              ? const <VesperPluginReference>[]
              : <VesperPluginReference>[
                  VesperBundledPluginReferences.sourceNormalizerFfmpeg,
                ],
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
    final activeSource = _activePlaybackSource();
    if (activeSource != null &&
        exampleDolbyAcceptanceSourceRequiresDirectNativePlayback(
          activeSource,
        ) &&
        setting != ExampleSourceNormalizerSetting.disabled) {
      _showMessage('Dolby Vision 验收流需要 direct native playback，已保持插件关闭。');
      if (mounted) {
        _updateState(() {
          _sourceNormalizerSetting = ExampleSourceNormalizerSetting.disabled;
          _appendHostLog(
            severity: ExampleHostLogSeverity.warning,
            title: '插件模式保持关闭',
            detail:
                'Dolby Vision 验收流不经过 SourceNormalizer、FFmpeg 或 FrameProcessor。',
          );
        });
      }
      return;
    }
    final previousSnapshot = previousController.snapshot;
    final restorePositionMs = previousSnapshot.timeline.positionMs;
    final shouldResumePlayback =
        previousSnapshot.playbackState == VesperPlaybackState.playing;

    _updateState(() {
      _sourceNormalizerSetting = setting;
      _isRebuildingController = true;
      _appendHostLog(title: '插件模式已切换', detail: setting.title);
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
    late final VesperDownloadManager manager;
    try {
      manager = await VesperDownloadManager.create(
        configuration: VesperDownloadConfiguration(
          runPostProcessorsOnCompletion: false,
          postDownloadPluginReferences: <VesperPluginReference>[
            VesperBundledPluginReferences.remuxFfmpeg,
          ],
        ),
      );
      _isDownloadExportPluginInstalled = true;
    } catch (_) {
      manager = await VesperDownloadManager.create(
        configuration: const VesperDownloadConfiguration(
          runPostProcessorsOnCompletion: false,
        ),
      );
      _isDownloadExportPluginInstalled = false;
    }
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
      case VesperDownloadResyncEvent():
      case VesperDownloadTaskCreatedEvent():
      case VesperDownloadTaskUpdatedEvent():
      case VesperDownloadTaskRemovedEvent():
      case VesperDownloadErrorEvent():
      case VesperDownloadDisposedEvent():
      case VesperDownloadUnknownEvent():
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
      _appendHostLog(title: '插件模式已切换', detail: profile.title);
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
    VesperPlayerSource source, {
    ExamplePlaybackOrigin? origin,
  }) async {
    final directNativePlaybackRequired =
        exampleDolbyAcceptanceSourceRequiresDirectNativePlayback(source);
    _activeDirectSource = source;
    _playbackOrigin = origin;
    if (source.kind == VesperPlayerSourceKind.remote) {
      _remoteUrlController.text = source.uri;
    }
    if (directNativePlaybackRequired) {
      final wasPluginRoute =
          _sourceNormalizerSetting != ExampleSourceNormalizerSetting.disabled ||
          _frameProcessorPluginReferences.isNotEmpty;
      if (wasPluginRoute && mounted) {
        _showMessage('Dolby Vision 验收已切回 direct native playback，插件选择已关闭。');
      }
      _updateState(() {
        _sourceNormalizerSetting = ExampleSourceNormalizerSetting.disabled;
        _appendHostLog(
          title: 'Dolby direct native playback',
          detail: 'SourceNormalizer、FFmpeg 与 FrameProcessor 已关闭。',
        );
      });
      await _rebuildControllerForSource(
        source,
        shouldResumePlayback:
            controller.snapshot.playbackState == VesperPlaybackState.playing,
      );
      return;
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
