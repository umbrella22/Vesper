part of 'player_host_page.dart';

extension _PlayerHostSourceActions on _PlayerHostPageState {
  Future<VesperPlayerController> _createController({
    VesperPlayerSource? source,
    VesperSourceActivationOptions activationOptions =
        const VesperSourceActivationOptions(playWhenReady: false),
    bool preservePlaylistState = false,
    ExamplePlaybackOrigin? origin,
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
      final previous = _controller;
      if (!preservePlaylistState) {
        _playlistItemIds = <String>[flutterHlsPlaylistItemId];
        _activePlaylistItemId = flutterHlsPlaylistItemId;
        origin = const ExampleQueuePlaybackOrigin(flutterHlsPlaylistItemId);
      }
      if (origin != null) {
        _recordActiveSource(selectedSource, origin);
        if (directNativePlaybackRequired) {
          if (_sourceNormalizerSetting !=
                  ExampleSourceNormalizerSetting.disabled ||
              _frameProcessorPluginReferences.isNotEmpty) {
            _showMessage(_hostStrings[ExampleHostText.dolbyPluginsTurnedOff]);
          }
          _sourceNormalizerSetting = ExampleSourceNormalizerSetting.disabled;
          _appendHostLog(
            title: 'Dolby direct native playback',
            detail: _hostStrings[ExampleHostText.logDolbyDirectDetail],
          );
        }
      }
      _sourceNormalizerPluginReferences =
          sourceNormalizerConfiguration.pluginReferences;
      _frameProcessorPluginReferences = frameProcessorPluginReferences;
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
    final previousSetting = _sourceNormalizerSetting;
    _updateState(() {
      _sourceNormalizerSetting = setting;
      _appendHostLog(
        title: strings[ExampleHostText.logPluginModeChanged],
        detail: setting.title(strings),
      );
    });
    final rebuilding = _rebuildControllerForSource(
      activeSource,
      VesperSourceActivationOptions(
        playWhenReady: shouldResumePlayback,
        startPosition: Duration(milliseconds: restorePositionMs),
        playbackRate: previousSnapshot.playbackRate,
      ),
    );
    final generation = _sourceRequestGeneration;
    try {
      await rebuilding;
    } catch (error) {
      if (mounted && generation == _sourceRequestGeneration) {
        _updateState(() => _sourceNormalizerSetting = previousSetting);
        _showMessage(
          strings.format(
            ExampleHostText.normalizerSwitchFailed,
            <String, Object?>{'error': error},
          ),
        );
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
    required ExamplePlaybackOrigin origin,
  }) async {
    if (!mounted || _isClosing || !identical(_controller, controller)) return;
    final directNativePlaybackRequired =
        exampleDolbyAcceptanceSourceRequiresDirectNativePlayback(source);
    const activation = VesperSourceActivationOptions(playWhenReady: true);
    if (directNativePlaybackRequired ||
        (_sourceNormalizerSetting != ExampleSourceNormalizerSetting.disabled &&
            _sourceNormalizerSetting !=
                ExampleSourceNormalizerSetting.diagnosticsOnly)) {
      await _rebuildControllerForSource(source, activation, origin: origin);
      return;
    }
    final generation = ++_sourceRequestGeneration;
    bool isCurrent() =>
        mounted &&
        !_isClosing &&
        generation == _sourceRequestGeneration &&
        identical(_controller, controller);
    await activateExampleSource(
      controller,
      source,
      options: activation,
      isCurrent: isCurrent,
    );
    if (!isCurrent()) return;
    _recordActiveSource(source, origin);
    await _configureSystemPlayback(controller, source, isCurrent: isCurrent);
  }

  void _recordActiveSource(
    VesperPlayerSource source,
    ExamplePlaybackOrigin origin,
  ) {
    _activeDirectSource = source;
    _playbackOrigin = origin;
    if (source.kind == VesperPlayerSourceKind.remote) {
      _remoteUrlController.text = source.uri;
    }
  }

  Future<void> _rebuildControllerForSource(
    VesperPlayerSource? source,
    VesperSourceActivationOptions activationOptions, {
    ExamplePlaybackOrigin? origin,
  }) async {
    if (!mounted || _isClosing) return;
    final retained = _controller;
    _updateState(() {
      _isRebuildingController = true;
      _controllerFuture = _createController(
        source: source,
        activationOptions: activationOptions,
        preservePlaylistState: true,
        origin: origin,
      );
    });

    final rebuilding = _controllerFuture;
    var succeeded = false;
    try {
      await rebuilding;
      if (!identical(_controllerFuture, rebuilding)) {
        throw StateError('Controller replacement was superseded.');
      }
      succeeded = true;
    } finally {
      if (mounted && identical(_controllerFuture, rebuilding)) {
        _updateState(() {
          _isRebuildingController = false;
          if (!succeeded &&
              retained != null &&
              identical(_controller, retained)) {
            _controllerFuture = Future.value(retained);
          }
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
