import 'dart:async';
import 'dart:io';

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:vesper_player/vesper_player.dart';
import 'package:vesper_player_external_playback/vesper_player_external_playback.dart';
import 'package:vesper_player_ui/vesper_player_ui.dart' as ui;

import 'example_device_controls.dart';
import 'example_download_planner.dart';
import 'example_download_sections.dart';
import 'example_local_media_picker.dart';
import 'example_player_helpers.dart';
import 'example_player_models.dart';
import 'example_player_sections.dart';
import 'example_player_sheet.dart';
import 'example_player_stage.dart';

class PlayerHostPage extends StatefulWidget {
  const PlayerHostPage({
    super.key,
    required this.themeMode,
    required this.onThemeModeChange,
  });

  final ExampleThemeMode themeMode;
  final ValueChanged<ExampleThemeMode> onThemeModeChange;

  @override
  State<PlayerHostPage> createState() => _PlayerHostPageState();
}

class _PlayerHostPageState extends State<PlayerHostPage> {
  late final TextEditingController _remoteUrlController;
  late final TextEditingController _downloadUrlController;
  final ExampleDeviceControls _deviceControls = ExampleDeviceControls();
  final VesperExternalPlaybackController _externalPlaybackController =
      VesperExternalPlaybackController();
  late Future<VesperPlayerController> _controllerFuture;
  Future<VesperDownloadManager>? _downloadManagerFuture;

  VesperPlayerController? _controller;
  VesperDownloadManager? _downloadManager;
  StreamSubscription<VesperDownloadManagerEvent>? _downloadEventsSubscription;
  StreamSubscription<List<VesperExternalPlaybackRoute>>?
  _externalRoutesSubscription;
  StreamSubscription<VesperExternalPlaybackSessionEvent>?
  _externalEventsSubscription;
  ExampleHostTab _selectedTab = ExampleHostTab.player;
  ExampleResilienceProfile _selectedResilienceProfile =
      ExampleResilienceProfile.balanced;
  ExampleSourceNormalizerSetting _sourceNormalizerSetting =
      ExampleSourceNormalizerSetting.preflightOnly;
  bool _isApplyingResilienceProfile = false;
  bool _isRebuildingController = false;
  bool _sheetOpen = false;
  List<String> _playlistItemIds = <String>[flutterHlsPlaylistItemId];
  String? _activePlaylistItemId = flutterHlsPlaylistItemId;
  String? _downloadMessage;
  String? _externalPlaybackMessage;
  bool _externalPlaybackMessageIsDiagnostic = false;
  bool _isDownloadExportPluginInstalled = false;
  List<String> _sourceNormalizerPluginLibraryPaths = const <String>[];
  List<String> _frameProcessorPluginLibraryPaths = const <String>[];
  bool _externalPlaybackPausedLocalPlayback = false;
  VesperSystemPlaybackPermissionStatus _systemPlaybackPermissionStatus =
      VesperSystemPlaybackPermissionStatus.notRequired;
  VesperPlayerSource? _queuedRemoteSource;
  VesperPlayerSource? _queuedLocalSource;
  Set<int> _savingTaskIds = <int>{};
  Map<int, double> _exportProgressByTaskId = <int, double>{};
  List<VesperExternalPlaybackRoute> _externalRoutes =
      <VesperExternalPlaybackRoute>[];
  List<ExamplePendingDownloadTask> _pendingDownloadTasks =
      <ExamplePendingDownloadTask>[];

  @override
  void initState() {
    super.initState();
    _remoteUrlController = TextEditingController(text: flutterHlsDemoUrl);
    _downloadUrlController = TextEditingController(text: flutterHlsDemoUrl);
    if (Platform.isAndroid) {
      _externalRoutesSubscription = _externalPlaybackController.routes.listen(
        _handleExternalRoutes,
      );
      _externalEventsSubscription = _externalPlaybackController.events.listen(
        _handleExternalEvent,
      );
      unawaited(_externalPlaybackController.startDiscovery());
    }
    _controllerFuture = _createController();
  }

  @override
  void dispose() {
    unawaited(_downloadEventsSubscription?.cancel() ?? Future<void>.value());
    unawaited(_externalRoutesSubscription?.cancel() ?? Future<void>.value());
    unawaited(_externalEventsSubscription?.cancel() ?? Future<void>.value());
    if (Platform.isAndroid) {
      unawaited(_externalPlaybackController.stopDiscovery());
      unawaited(_externalPlaybackController.disconnect());
    }
    final currentController = _controller;
    if (currentController != null) {
      _disposeControllerSilently(currentController);
    }
    unawaited(
      _controllerFuture
          .then((value) {
            if (!identical(value, currentController)) {
              return value.dispose();
            }
          })
          .catchError((_) {}),
    );

    final currentDownloadManager = _downloadManager;
    if (currentDownloadManager != null) {
      _disposeDownloadManagerSilently(currentDownloadManager);
    }
    final downloadManagerFuture = _downloadManagerFuture;
    if (downloadManagerFuture != null) {
      unawaited(
        downloadManagerFuture
            .then((value) {
              if (!identical(value, currentDownloadManager)) {
                return value.dispose();
              }
            })
            .catchError((_) {}),
      );
    }

    unawaited(_restoreSystemPresentation());
    _remoteUrlController.dispose();
    _downloadUrlController.dispose();
    super.dispose();
  }

  Future<VesperPlayerController> _createController({
    VesperPlayerSource? initialSource,
    bool preservePlaylistState = false,
  }) async {
    VesperPlayerController? nextController;
    try {
      final pluginPaths =
          await Future.wait<List<String>>(<Future<List<String>>>[
            ExampleLocalMediaPicker.bundledSourceNormalizerPluginLibraryPaths(),
            ExampleLocalMediaPicker.bundledFrameProcessorPluginLibraryPaths(),
          ]);
      if (mounted) {
        setState(() {
          _sourceNormalizerPluginLibraryPaths = pluginPaths[0];
          _frameProcessorPluginLibraryPaths = pluginPaths[1];
        });
      } else {
        _sourceNormalizerPluginLibraryPaths = pluginPaths[0];
        _frameProcessorPluginLibraryPaths = pluginPaths[1];
      }

      final selectedSource = initialSource ?? flutterHlsDemoSource();
      nextController = await VesperPlayerController.create(
        initialSource: selectedSource,
        renderSurfaceKind: VesperPlayerRenderSurfaceKind.surfaceView,
        resiliencePolicy: _selectedResilienceProfile.policy,
        sourceNormalizerConfiguration: VesperSourceNormalizerConfiguration(
          mode: _sourceNormalizerSetting.mode,
          pluginLibraryPaths: pluginPaths[0],
        ),
        frameProcessorConfiguration: VesperFrameProcessorConfiguration(
          mode: pluginPaths[1].isEmpty
              ? VesperFrameProcessorMode.disabled
              : VesperFrameProcessorMode.diagnosticsOnly,
          pluginLibraryPaths: pluginPaths[1],
        ),
      );
      await nextController.initialize();
      await _configureSystemPlayback(nextController, selectedSource);
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

    setState(() {
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
        setState(() {
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
        setState(() {
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
    setState(() {
      _selectedResilienceProfile = profile;
      _isApplyingResilienceProfile = true;
    });
    try {
      await controller.setResiliencePolicy(profile.policy);
    } catch (_) {
      if (mounted) {
        setState(() {
          _selectedResilienceProfile = previousProfile;
        });
      }
      rethrow;
    } finally {
      if (mounted) {
        setState(() {
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

    setState(() {
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
        setState(() {
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
      setState(() {
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
    setState(() {
      _systemPlaybackPermissionStatus = permissionStatus;
    });
  }

  void _handleExternalRoutes(List<VesperExternalPlaybackRoute> routes) {
    if (!mounted) {
      return;
    }
    setState(() {
      _externalRoutes = routes;
    });
  }

  Future<void> _refreshExternalRoutes() async {
    if (!Platform.isAndroid) {
      return;
    }
    await _externalPlaybackController.startDiscovery();
    if (!mounted) {
      return;
    }
    setState(() {
      _setExternalPlaybackMessage('正在重新扫描 DLNA 设备。', force: true);
    });
  }

  void _handleExternalEvent(VesperExternalPlaybackSessionEvent event) {
    unawaited(_handleExternalEventAsync(event));
  }

  Future<void> _handleExternalEventAsync(
    VesperExternalPlaybackSessionEvent event,
  ) async {
    if (!mounted) {
      return;
    }

    switch (event.kind) {
      case VesperExternalPlaybackSessionEventKind.routeConnected:
        final routeLabel = event.routeName ?? event.routeId ?? '设备';
        setState(() {
          _setExternalPlaybackMessage('外部播放已连接：$routeLabel', force: true);
        });
        if (event.routeId == VesperExternalPlaybackController.castRouteId) {
          await _loadCurrentExternalMedia(routeLabel: routeLabel);
        }
      case VesperExternalPlaybackSessionEventKind.routeDisconnected:
        await _resumeLocalPlaybackFromExternal(event.positionMs);
        if (!mounted) {
          return;
        }
        setState(() {
          _externalPlaybackPausedLocalPlayback = false;
          _setExternalPlaybackMessage('外部播放已断开，本地播放已恢复。', force: true);
        });
      case VesperExternalPlaybackSessionEventKind.loaded:
        setState(() {
          _setExternalPlaybackMessage('外部播放媒体已加载。');
        });
      case VesperExternalPlaybackSessionEventKind.playing:
        setState(() {
          _setExternalPlaybackMessage('外部播放已继续。');
        });
      case VesperExternalPlaybackSessionEventKind.paused:
        setState(() {
          _setExternalPlaybackMessage('外部播放已暂停。');
        });
      case VesperExternalPlaybackSessionEventKind.stopped:
        setState(() {
          _setExternalPlaybackMessage('外部播放已停止。');
        });
      case VesperExternalPlaybackSessionEventKind.suspended:
        setState(() {
          _setExternalPlaybackMessage('外部播放连接已暂停。');
        });
      case VesperExternalPlaybackSessionEventKind.discoveryDiagnostic:
        if (event.details['severity'] != 'info') {
          setState(() {
            _setExternalPlaybackMessage(
              _formatExternalPlaybackDiagnostic(event),
              diagnostic: true,
            );
          });
        }
      case VesperExternalPlaybackSessionEventKind.error:
        setState(() {
          _setExternalPlaybackMessage(
            event.message ?? '外部播放发生错误。',
            diagnostic: true,
          );
        });
    }
  }

  Future<void> _loadExternalRoute(VesperExternalPlaybackRoute route) async {
    if (mounted) {
      setState(() {
        _setExternalPlaybackMessage('正在连接外部播放：${route.name}', force: true);
      });
    }
    final connectResult = await _externalPlaybackController.connect(
      route.routeId,
    );
    if (!connectResult.isSuccess) {
      if (mounted) {
        setState(() {
          _setExternalPlaybackMessage(connectResult.message, diagnostic: true);
        });
      }
      return;
    }
    await _loadCurrentExternalMedia(routeLabel: route.name);
  }

  Future<void> _loadCurrentExternalMedia({required String routeLabel}) async {
    final controller = _controller ?? await _controllerFuture;
    final source = _activePlaylistItemId == null
        ? null
        : _playlistSourceForItem(_activePlaylistItemId!);
    if (source == null) {
      return;
    }
    final wasPlaying =
        controller.snapshot.playbackState == VesperPlaybackState.playing;
    final shouldAutoplay = wasPlaying || _externalPlaybackPausedLocalPlayback;
    final loadResult = await _externalPlaybackController.load(
      VesperExternalPlaybackMediaItem(
        sources: <VesperPlayerSource>[source],
        metadata: _systemPlaybackMetadataForSource(source),
        formatAdaptation: _externalFormatAdaptationForSource(source),
      ),
      startPositionMs: controller.snapshot.timeline.positionMs,
      autoplay: shouldAutoplay,
    );
    if (loadResult.isSuccess && wasPlaying) {
      await controller.pause();
    }
    if (!mounted) {
      return;
    }
    setState(() {
      if (loadResult.isSuccess && shouldAutoplay) {
        _externalPlaybackPausedLocalPlayback = true;
      }
      _setExternalPlaybackMessage(
        loadResult.isSuccess ? '外部播放已加载：$routeLabel' : loadResult.message,
        diagnostic: !loadResult.isSuccess,
      );
    });
  }

  Future<void> _resumeLocalPlaybackFromExternal(int? positionMs) async {
    if (!_externalPlaybackPausedLocalPlayback) {
      return;
    }
    final controller = _controller;
    if (controller == null) {
      return;
    }
    if (positionMs != null) {
      final deltaMs = positionMs - controller.snapshot.timeline.positionMs;
      await controller.seekBy(deltaMs);
    }
    await controller.play();
  }

  void _setExternalPlaybackMessage(
    String? message, {
    bool diagnostic = false,
    bool force = false,
  }) {
    if (!force && !diagnostic && _externalPlaybackMessageIsDiagnostic) {
      return;
    }
    _externalPlaybackMessage = message;
    _externalPlaybackMessageIsDiagnostic = diagnostic;
  }

  String _formatExternalPlaybackDiagnostic(
    VesperExternalPlaybackSessionEvent event,
  ) {
    final labels = <String>[
      if (event.code != null) 'code=${event.code}',
      if (event.details['httpStatus'] != null)
        "http=${event.details['httpStatus']}",
      if (event.details['fallbackFormat'] != null)
        "fallback=${event.details['fallbackFormat']}",
      if (event.details['inputMode'] != null)
        "mode=${event.details['inputMode']}",
    ];
    final suffix = labels.isEmpty ? '' : ' (${labels.join(', ')})';
    return '${event.message ?? '外部播放诊断事件。'}$suffix';
  }

  VesperSystemPlaybackMetadata _systemPlaybackMetadataForSource(
    VesperPlayerSource source,
  ) {
    return VesperSystemPlaybackMetadata(
      title: source.label,
      artist: 'Vesper Player SDK',
      contentUri: source.uri,
    );
  }

  VesperExternalFormatAdaptationConfig _externalFormatAdaptationForSource(
    VesperPlayerSource source,
  ) {
    if (source.protocol == VesperPlayerSourceProtocol.dash) {
      return const VesperExternalFormatAdaptationConfig.dlnaRemux(
        debugDiagnostics: true,
      );
    }
    return const VesperExternalFormatAdaptationConfig.disabled();
  }

  VesperPlayerSource? _playlistSourceForItem(String itemId) {
    return switch (itemId) {
      flutterHlsPlaylistItemId => flutterHlsDemoSource(),
      flutterDashPlaylistItemId => flutterDashDemoSource(),
      flutterLiveDvrPlaylistItemId => flutterLiveDvrAcceptanceSource(),
      flutterLocalPlaylistItemId => _queuedLocalSource,
      flutterRemotePlaylistItemId => _queuedRemoteSource,
      _ => null,
    };
  }

  List<ExamplePlaylistItemViewData> _buildPlaylistItems() {
    final activeIndex = _playlistItemIds.indexOf(_activePlaylistItemId ?? '');
    return _playlistItemIds
        .asMap()
        .entries
        .map((entry) {
          final index = entry.key;
          final itemId = entry.value;
          final source = _playlistSourceForItem(itemId);
          if (source == null) {
            return null;
          }
          final isActive = itemId == _activePlaylistItemId;
          return ExamplePlaylistItemViewData(
            itemId: itemId,
            label: source.label,
            status: playlistItemStatusLabel(
              index: index,
              activeIndex: activeIndex,
            ),
            isActive: isActive,
          );
        })
        .whereType<ExamplePlaylistItemViewData>()
        .toList(growable: false);
  }

  Future<void> _activatePlaylistSource(
    VesperPlayerController controller, {
    required String itemId,
    required VesperPlayerSource source,
    VesperPlayerSource? remoteSource,
    VesperPlayerSource? localSource,
  }) async {
    await _selectSource(controller, source);
    if (!mounted) {
      return;
    }
    setState(() {
      if (remoteSource != null) {
        _queuedRemoteSource = remoteSource;
      }
      if (localSource != null) {
        _queuedLocalSource = localSource;
      }
      _playlistItemIds = enqueuePlaylistItem(_playlistItemIds, itemId);
      _activePlaylistItemId = itemId;
    });
  }

  Future<void> _focusPlaylistItem(
    VesperPlayerController controller,
    String itemId,
  ) async {
    final source = _playlistSourceForItem(itemId);
    if (source == null) {
      return;
    }
    await _selectSource(controller, source);
    if (!mounted) {
      return;
    }
    setState(() {
      _activePlaylistItemId = itemId;
    });
  }

  Future<void> _playCustomUrl(VesperPlayerController controller) async {
    final uri = _remoteUrlController.text.trim();
    if (uri.isEmpty) {
      return;
    }

    final protocol = inferProtocol(uri);
    if (protocol == VesperPlayerSourceProtocol.dash &&
        !controller.capabilities.supportsDash) {
      _showMessage('当前平台宿主暂不支持 DASH 流。');
      return;
    }

    final source = VesperPlayerSource.remote(
      uri: uri,
      label: '自定义远程流',
      protocol: protocol,
    );
    await _activatePlaylistSource(
      controller,
      itemId: flutterRemotePlaylistItemId,
      source: source,
      remoteSource: source,
    );
  }

  Future<void> _pickLocalVideo(VesperPlayerController controller) async {
    try {
      final pickedVideo = await ExampleLocalMediaPicker.pickVideo();
      if (!mounted || pickedVideo == null) {
        return;
      }
      final source = VesperPlayerSource.local(
        uri: pickedVideo.uri,
        label: pickedVideo.label,
      );
      await _activatePlaylistSource(
        controller,
        itemId: flutterLocalPlaylistItemId,
        source: source,
        localSource: source,
      );
      return;
    } on MissingPluginException {
      // Fall back to manual input when the host picker is not wired, which keeps debugging simple.
    } on PlatformException catch (error) {
      if (!mounted || error.code == 'cancelled') {
        return;
      }
    }

    await _promptLocalVideoFallback(controller);
  }

  Future<void> _promptLocalVideoFallback(
    VesperPlayerController controller,
  ) async {
    final localUri = await showDialog<String>(
      context: context,
      builder: (context) {
        final textController = TextEditingController();
        return AlertDialog(
          title: const Text('选择视频'),
          content: TextField(
            controller: textController,
            autofocus: true,
            decoration: const InputDecoration(
              labelText: '本地路径或 URI',
              hintText: 'file:///sdcard/Movies/demo.mp4',
            ),
          ),
          actions: <Widget>[
            TextButton(
              onPressed: () => Navigator.of(context).pop(),
              child: const Text('取消'),
            ),
            FilledButton(
              onPressed: () => Navigator.of(context).pop(textController.text),
              child: const Text('打开'),
            ),
          ],
        );
      },
    );

    if (!mounted || localUri == null || localUri.trim().isEmpty) {
      return;
    }

    final normalizedUri = normalizeLocalUri(localUri);
    final source = VesperPlayerSource.local(
      uri: normalizedUri,
      label: localSourceLabel(normalizedUri),
    );
    await _activatePlaylistSource(
      controller,
      itemId: flutterLocalPlaylistItemId,
      source: source,
      localSource: source,
    );
  }

  Future<void> _openToolSheet(
    VesperPlayerController controller,
    ExamplePlayerSheet initialSheet,
  ) async {
    if (!mounted) {
      return;
    }

    setState(() {
      _sheetOpen = true;
    });

    try {
      await showExampleSelectionSheet(
        context,
        controller: controller,
        initialSheet: initialSheet,
      );
    } finally {
      if (mounted) {
        setState(() {
          _sheetOpen = false;
        });
      }
    }
  }

  Future<void> _toggleFullscreen(Orientation orientation) async {
    if (orientation == Orientation.portrait) {
      await SystemChrome.setPreferredOrientations(const <DeviceOrientation>[
        DeviceOrientation.landscapeLeft,
        DeviceOrientation.landscapeRight,
      ]);
      await SystemChrome.setEnabledSystemUIMode(SystemUiMode.immersiveSticky);
      return;
    }

    await _restoreSystemPresentation();
  }

  Future<void> _restoreSystemPresentation() async {
    await SystemChrome.setPreferredOrientations(const <DeviceOrientation>[]);
    await SystemChrome.setEnabledSystemUIMode(SystemUiMode.edgeToEdge);
  }

  Future<void> _createDownloadTask(
    VesperDownloadManager manager, {
    required String assetIdPrefix,
    required VesperPlayerSource source,
  }) async {
    final assetId = '$assetIdPrefix-${DateTime.now().millisecondsSinceEpoch}';
    setState(() {
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
    setState(() {
      _pendingDownloadTasks = _pendingDownloadTasks
          .where((task) => task.requestId != assetId)
          .toList(growable: false);
      _downloadMessage = error != null
          ? '准备下载任务失败：$error'
          : taskId == null
          ? '创建下载任务失败。'
          : null;
    });
  }

  Future<void> _createRemoteDownloadTask(VesperDownloadManager manager) async {
    final uri = _downloadUrlController.text.trim();
    if (uri.isEmpty) {
      setState(() {
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
    setState(() {
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
        setState(() {
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

  @override
  Widget build(BuildContext context) {
    final mediaQuery = MediaQuery.of(context);
    final immersivePlayer =
        mediaQuery.orientation == Orientation.landscape &&
        _selectedTab == ExampleHostTab.player;
    final useDarkTheme = Theme.of(context).brightness == Brightness.dark;
    final palette = exampleHostPalette(useDarkTheme);

    final body = switch (_selectedTab) {
      ExampleHostTab.player => _buildPlayerFutureContent(
        context,
        immersivePlayer: immersivePlayer,
        palette: palette,
      ),
      ExampleHostTab.downloads => _buildDownloadFutureContent(palette),
    };

    return Scaffold(
      body: DecoratedBox(
        decoration: BoxDecoration(
          gradient: LinearGradient(
            begin: Alignment.topCenter,
            end: Alignment.bottomCenter,
            colors: <Color>[palette.pageTop, palette.pageBottom],
          ),
        ),
        child: immersivePlayer ? body : SafeArea(child: body),
      ),
      bottomNavigationBar: immersivePlayer
          ? null
          : NavigationBar(
              selectedIndex: _selectedTab.index,
              onDestinationSelected: (index) {
                setState(() {
                  _selectedTab = ExampleHostTab.values[index];
                });
              },
              destinations: const <Widget>[
                NavigationDestination(
                  icon: Icon(Icons.video_library_rounded),
                  label: '播放器',
                ),
                NavigationDestination(
                  icon: Icon(Icons.download_rounded),
                  label: '下载',
                ),
              ],
            ),
    );
  }

  Widget _buildPlayerFutureContent(
    BuildContext context, {
    required bool immersivePlayer,
    required ExampleHostPalette palette,
  }) {
    return FutureBuilder<VesperPlayerController>(
      future: _controllerFuture,
      builder: (context, asyncSnapshot) {
        if (asyncSnapshot.hasError && !asyncSnapshot.hasData) {
          return ExampleErrorState(error: asyncSnapshot.error);
        }

        final controller = asyncSnapshot.data ?? _controller;
        if (controller == null) {
          return const ExampleLoadingState();
        }
        final playlistItems = _buildPlaylistItems();

        return ValueListenableBuilder<VesperPlayerSnapshot>(
          valueListenable: controller.snapshotListenable,
          builder: (context, snapshot, _) {
            final content = immersivePlayer
                ? _buildLandscapeLayout(controller, snapshot, asyncSnapshot)
                : _buildPortraitLayout(
                    context,
                    controller,
                    snapshot,
                    playlistItems,
                    palette,
                    asyncSnapshot,
                  );

            return Stack(
              children: <Widget>[
                Positioned.fill(child: content),
                if (_isApplyingResilienceProfile)
                  const Positioned(
                    top: 18,
                    right: 18,
                    child: ExampleBusyPill(label: '正在应用策略'),
                  ),
                if (_isRebuildingController)
                  const Positioned(
                    top: 18,
                    left: 18,
                    child: ExampleBusyPill(label: '正在切换插件'),
                  ),
              ],
            );
          },
        );
      },
    );
  }

  Widget _buildPortraitLayout(
    BuildContext context,
    VesperPlayerController controller,
    VesperPlayerSnapshot snapshot,
    List<ExamplePlaylistItemViewData> playlistItems,
    ExampleHostPalette palette,
    AsyncSnapshot<VesperPlayerController> asyncSnapshot,
  ) {
    final transientError = asyncSnapshot.hasError ? asyncSnapshot.error : null;

    return SingleChildScrollView(
      padding: const EdgeInsets.symmetric(horizontal: 18, vertical: 18),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: <Widget>[
          ExamplePlayerHeader(
            sourceLabel: snapshot.sourceLabel.isEmpty
                ? snapshot.title
                : snapshot.sourceLabel,
            subtitle: snapshot.subtitle,
            palette: palette,
          ),
          if (transientError != null) ...<Widget>[
            const SizedBox(height: 18),
            ExampleInlineControllerError(error: transientError),
          ],
          const SizedBox(height: 18),
          SizedBox(
            width: double.infinity,
            height: 248,
            child: ExamplePlayerStage(
              controller: controller,
              snapshot: snapshot,
              isPortrait: true,
              sheetOpen: _sheetOpen,
              deviceControls: _deviceControls,
              topBarPrimaryAction: _buildStageRouteAction(controller),
              onOpenSheet: (sheet) =>
                  unawaited(_openToolSheet(controller, sheet)),
              onToggleFullscreen: () =>
                  unawaited(_toggleFullscreen(Orientation.portrait)),
            ),
          ),
          const SizedBox(height: 18),
          ExampleSourceSection(
            palette: palette,
            themeMode: widget.themeMode,
            remoteUrlController: _remoteUrlController,
            localFilesEnabled: snapshot.capabilities.supportsLocalFiles,
            dashEnabled: snapshot.capabilities.supportsDash,
            dashUnavailableMessage: snapshot.capabilities.supportsDash
                ? null
                : '当前平台宿主暂不支持 DASH 演示。',
            onThemeModeChange: widget.onThemeModeChange,
            onPickVideo: () => unawaited(_pickLocalVideo(controller)),
            onUseHlsDemo: () => unawaited(
              _activatePlaylistSource(
                controller,
                itemId: flutterHlsPlaylistItemId,
                source: flutterHlsDemoSource(),
              ),
            ),
            onUseDashDemo: () => unawaited(
              _activatePlaylistSource(
                controller,
                itemId: flutterDashPlaylistItemId,
                source: flutterDashDemoSource(),
              ),
            ),
            onUseLiveDvrAcceptance: () => unawaited(
              _activatePlaylistSource(
                controller,
                itemId: flutterLiveDvrPlaylistItemId,
                source: flutterLiveDvrAcceptanceSource(),
              ),
            ),
            onOpenRemote: () => unawaited(_playCustomUrl(controller)),
          ),
          const SizedBox(height: 18),
          ExamplePlaylistSection(
            palette: palette,
            playlistItems: playlistItems,
            onSelectItem: (itemId) =>
                unawaited(_focusPlaylistItem(controller, itemId)),
          ),
          const SizedBox(height: 18),
          ExamplePluginDiagnosticsSection(
            palette: palette,
            sourceNormalizerSetting: _sourceNormalizerSetting,
            sourceNormalizerPluginLibraryPaths:
                _sourceNormalizerPluginLibraryPaths,
            frameProcessorPluginLibraryPaths: _frameProcessorPluginLibraryPaths,
            pluginDiagnostics: controller.pluginDiagnostics,
            onSourceNormalizerSettingChange: (setting) =>
                unawaited(_applySourceNormalizerSetting(setting)),
          ),
          const SizedBox(height: 18),
          ExampleSystemPlaybackSection(
            palette: palette,
            controller: controller,
            permissionStatus: _systemPlaybackPermissionStatus,
            onRefreshExternalRoutes: () => unawaited(_refreshExternalRoutes()),
            externalRoutes: _externalRoutes
                .where(
                  (route) => route.kind == VesperExternalPlaybackRouteKind.dlna,
                )
                .toList(growable: false),
            externalPlaybackMessage: _externalPlaybackMessage,
            onExternalRouteSelected: (route) =>
                unawaited(_loadExternalRoute(route)),
            onRequestPermission: () =>
                unawaited(_requestSystemPlaybackPermissions(controller)),
          ),
          const SizedBox(height: 18),
          ExampleResilienceSection(
            palette: palette,
            activePolicy: snapshot.resiliencePolicy,
            selectedProfile: _selectedResilienceProfile,
            onApplyProfile: _applyResilienceProfile,
          ),
          if (snapshot.lastError != null) ...<Widget>[
            const SizedBox(height: 18),
            ExampleRecentErrorSection(
              palette: palette,
              error: snapshot.lastError!,
            ),
          ],
        ],
      ),
    );
  }

  Widget _buildLandscapeLayout(
    VesperPlayerController controller,
    VesperPlayerSnapshot snapshot,
    AsyncSnapshot<VesperPlayerController> asyncSnapshot,
  ) {
    return Stack(
      children: <Widget>[
        Positioned.fill(
          child: ExamplePlayerStage(
            controller: controller,
            snapshot: snapshot,
            isPortrait: false,
            sheetOpen: _sheetOpen,
            deviceControls: _deviceControls,
            topBarPrimaryAction: _buildStageRouteAction(controller),
            onOpenSheet: (sheet) =>
                unawaited(_openToolSheet(controller, sheet)),
            onToggleFullscreen: () =>
                unawaited(_toggleFullscreen(Orientation.landscape)),
          ),
        ),
        if (asyncSnapshot.hasError)
          Positioned(
            top: 18,
            left: 18,
            right: 96,
            child: ExampleInlineControllerError(error: asyncSnapshot.error),
          ),
      ],
    );
  }

  Widget? _buildStageRouteAction(VesperPlayerController controller) {
    if (Platform.isAndroid) {
      return const VesperExternalRouteIconButton(size: 38);
    }
    if (Platform.isIOS) {
      return ui.VesperAirPlayRouteIconButton(controller: controller, size: 38);
    }
    return null;
  }

  Widget _buildDownloadFutureContent(ExampleHostPalette palette) {
    final downloadManagerFuture = _ensureDownloadManagerFuture();
    return FutureBuilder<VesperDownloadManager>(
      future: downloadManagerFuture,
      builder: (context, asyncSnapshot) {
        if (asyncSnapshot.hasError && !asyncSnapshot.hasData) {
          return ExampleErrorState(error: asyncSnapshot.error);
        }

        final manager = asyncSnapshot.data ?? _downloadManager;
        if (manager == null) {
          return const ExampleLoadingState();
        }

        return ValueListenableBuilder<VesperDownloadSnapshot>(
          valueListenable: manager.snapshotListenable,
          builder: (context, snapshot, _) {
            return SingleChildScrollView(
              padding: const EdgeInsets.symmetric(horizontal: 18, vertical: 18),
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: <Widget>[
                  ExampleDownloadHeader(
                    palette: palette,
                    isDownloadExportPluginInstalled:
                        _isDownloadExportPluginInstalled,
                  ),
                  if (asyncSnapshot.hasError) ...<Widget>[
                    const SizedBox(height: 18),
                    ExampleInlineControllerError(error: asyncSnapshot.error),
                  ],
                  const SizedBox(height: 18),
                  ExampleDownloadCreateSection(
                    palette: palette,
                    remoteUrlController: _downloadUrlController,
                    message: _downloadMessage,
                    onUseHlsDemo: () => unawaited(
                      _createDownloadTask(
                        manager,
                        assetIdPrefix: flutterHlsPlaylistItemId,
                        source: flutterHlsDemoSource(),
                      ),
                    ),
                    onUseDashDemo: () => unawaited(
                      _createDownloadTask(
                        manager,
                        assetIdPrefix: flutterDashPlaylistItemId,
                        source: flutterDashDemoSource(),
                      ),
                    ),
                    onCreateRemote: () =>
                        unawaited(_createRemoteDownloadTask(manager)),
                  ),
                  const SizedBox(height: 18),
                  ExampleDownloadTasksSection(
                    palette: palette,
                    tasks: snapshot.tasks,
                    pendingTasks: _pendingDownloadTasks,
                    isDownloadExportPluginInstalled:
                        _isDownloadExportPluginInstalled,
                    savingTaskIds: _savingTaskIds,
                    exportProgressByTaskId: _exportProgressByTaskId,
                    onPrimaryAction: (task) =>
                        unawaited(_handleDownloadPrimaryAction(manager, task)),
                    onSaveToGallery: (task) =>
                        unawaited(_saveDownloadToGallery(manager, task)),
                    onRemoveTask: (task) =>
                        unawaited(manager.removeTask(task.taskId)),
                  ),
                ],
              ),
            );
          },
        );
      },
    );
  }
}

enum ExampleHostTab { player, downloads }
