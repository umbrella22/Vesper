part of 'player_host_page.dart';

extension _PlayerHostSourceActions on _PlayerHostPageState {
  Future<VesperPlayerController> _createController({
    VesperPlayerSource? source,
    VesperSourceActivationOptions activationOptions =
        const VesperSourceActivationOptions(playWhenReady: false),
    bool preservePlaylistState = false,
  }) async {
    final generation = ++_sourceRequestGeneration;
    bool isCurrent() =>
        mounted && !_isClosing && generation == _sourceRequestGeneration;
    void checkCurrent() {
      if (!isCurrent()) {
        throw StateError('Controller creation was superseded or disposed.');
      }
    }

    VesperPlayerController? nextController;
    try {
      checkCurrent();
      final selectedSource = source ?? flutterHlsDemoSource();
      final directNativePlaybackRequired =
          exampleDolbyAcceptanceSourceRequiresDirectNativePlayback(
            selectedSource,
          );
      final sourceNormalizerConfiguration = _sourceNormalizerConfiguration(
        directNativePlaybackRequired: directNativePlaybackRequired,
      );
      const frameProcessorPluginReferences = <VesperPluginReference>[];
      // Scrolling list stages are composed by Flutter, where TextureView keeps
      // native video geometry inside the platform view. Direct-native sources
      // retain SurfaceView for HDR and high-fidelity playback.
      final renderSurfaceKind = directNativePlaybackRequired
          ? VesperPlayerRenderSurfaceKind.surfaceView
          : VesperPlayerRenderSurfaceKind.textureView;
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
        renderSurfaceKind: renderSurfaceKind,
        resiliencePolicy: _selectedResilienceProfile.policy,
        sourceNormalizerConfiguration: sourceNormalizerConfiguration,
        frameProcessorConfiguration: VesperFrameProcessorConfiguration(
          mode: frameProcessorPluginReferences.isEmpty
              ? VesperFrameProcessorMode.disabled
              : VesperFrameProcessorMode.diagnosticsOnly,
          pluginReferences: frameProcessorPluginReferences,
        ),
      );
      checkCurrent();
      _pendingControllers.add(nextController);
      await activateExampleSource(
        nextController,
        selectedSource,
        options: activationOptions,
        isCurrent: isCurrent,
      );
      checkCurrent();
      await _configureSystemPlayback(
        nextController,
        selectedSource,
        isCurrent: isCurrent,
      );
      checkCurrent();
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
      await _bindPictureInPicture(nextController);
      return nextController;
    } catch (_) {
      if (nextController != null) {
        _disposeControllerSilently(nextController);
      }
      rethrow;
    } finally {
      if (nextController != null) _pendingControllers.remove(nextController);
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
    if (!mounted || _isClosing) return;
    final activeSource = _activePlaybackSource();
    if (activeSource != null &&
        exampleDolbyAcceptanceSourceRequiresDirectNativePlayback(
          activeSource,
        ) &&
        setting != ExampleSourceNormalizerSetting.disabled) {
      final strings = _hostStrings;
      _showMessage(strings[ExampleHostText.dolbyPluginsKeptOff]);
      if (mounted) {
        _updateState(() {
          _sourceNormalizerSetting = ExampleSourceNormalizerSetting.disabled;
          _appendHostLog(
            severity: ExampleHostLogSeverity.warning,
            title: strings[ExampleHostText.logPluginsKeptOff],
            detail: strings[ExampleHostText.logPluginsKeptOffDetail],
          );
        });
      }
      return;
    }
    final previousSnapshot = previousController.snapshot;
    final restorePositionMs = previousSnapshot.timeline.positionMs;
    final shouldResumePlayback =
        previousSnapshot.playbackState == VesperPlaybackState.playing;

    final strings = _hostStrings;
    _updateState(() {
      _sourceNormalizerSetting = setting;
      _isRebuildingController = true;
      _appendHostLog(
        title: strings[ExampleHostText.logPluginModeChanged],
        detail: setting.title(strings),
      );
      _controllerFuture = _createController(
        source: activeSource,
        activationOptions: VesperSourceActivationOptions(
          playWhenReady: shouldResumePlayback,
          startPosition: Duration(milliseconds: restorePositionMs),
          playbackRate: previousSnapshot.playbackRate,
        ),
        preservePlaylistState: true,
      );
    });

    final rebuilding = _controllerFuture;
    try {
      await rebuilding;
    } catch (error) {
      if (mounted) {
        _showMessage(
          strings.format(
            ExampleHostText.normalizerSwitchFailed,
            <String, Object?>{'error': error},
          ),
        );
      }
    } finally {
      if (mounted && identical(_controllerFuture, rebuilding)) {
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
      _appendHostLog(
        title: _hostStrings[ExampleHostText.logResilienceChanged],
        detail: profile.title,
      );
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
    final generation = ++_sourceRequestGeneration;
    bool isCurrent() =>
        mounted &&
        !_isClosing &&
        generation == _sourceRequestGeneration &&
        identical(_controller, controller);
    if (!isCurrent()) return;
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
      final strings = _hostStrings;
      if (wasPluginRoute && mounted) {
        _showMessage(strings[ExampleHostText.dolbyPluginsTurnedOff]);
      }
      _updateState(() {
        _sourceNormalizerSetting = ExampleSourceNormalizerSetting.disabled;
        _appendHostLog(
          title: 'Dolby direct native playback',
          detail: strings[ExampleHostText.logDolbyDirectDetail],
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
    await activateExampleSource(
      controller,
      source,
      options: const VesperSourceActivationOptions(playWhenReady: true),
      isCurrent: isCurrent,
    );
    if (!isCurrent()) return;
    await _configureSystemPlayback(controller, source, isCurrent: isCurrent);
  }

  Future<void> _rebuildControllerForSource(
    VesperPlayerSource source, {
    required bool shouldResumePlayback,
  }) async {
    if (!mounted || _isClosing) return;

    _updateState(() {
      _isRebuildingController = true;
      _controllerFuture = _createController(
        source: source,
        activationOptions: VesperSourceActivationOptions(
          playWhenReady: shouldResumePlayback,
        ),
        preservePlaylistState: true,
      );
    });

    final rebuilding = _controllerFuture;
    try {
      await rebuilding;
    } finally {
      if (mounted && identical(_controllerFuture, rebuilding)) {
        _updateState(() {
          _isRebuildingController = false;
        });
      }
    }
  }

  Future<void> _configureSystemPlayback(
    VesperPlayerController controller,
    VesperPlayerSource source, {
    bool Function()? isCurrent,
  }) async {
    final permissionStatus = await controller
        .getSystemPlaybackPermissionStatus();
    if (isCurrent != null && !isCurrent()) return;
    if (mounted && !_isClosing) {
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
