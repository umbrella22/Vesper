import 'dart:ui' as ui;

import 'package:flutter/services.dart';
import 'package:vesper_player_platform_interface/vesper_player_platform_interface.dart';

class MethodChannelVesperPlayerAndroid extends VesperPlayerPlatform {
  MethodChannelVesperPlayerAndroid() {
    VesperPlayerPlatform.instance = this;
  }

  static const MethodChannel _methodChannel = MethodChannel(
    'io.github.ikaros.vesper_player',
  );
  static const EventChannel _eventChannel = EventChannel(
    'io.github.ikaros.vesper_player/events',
  );
  static const EventChannel _downloadEventChannel = EventChannel(
    'io.github.ikaros.vesper_player/download_events',
  );

  late final Stream<VesperPlayerEvent> _events = _eventChannel
      .receiveBroadcastStream()
      .where((dynamic event) => event is Map)
      .map((dynamic event) => Map<Object?, Object?>.from(event as Map))
      .map(VesperPlayerEvent.fromMap)
      .asBroadcastStream();

  late final Stream<VesperDownloadManagerEvent> _downloadEvents =
      _downloadEventChannel
          .receiveBroadcastStream()
          .where((dynamic event) => event is Map)
          .map((dynamic event) => Map<Object?, Object?>.from(event as Map))
          .map(VesperDownloadManagerEvent.fromMap)
          .asBroadcastStream();

  final Map<String, VesperDownloadStaleResourcePlanRecoveryCallback>
      _downloadRecoveryHandlers =
      <String, VesperDownloadStaleResourcePlanRecoveryCallback>{};
  bool _methodCallHandlerRegistered = false;

  @override
  Future<VesperPlatformCreateResult> createPlayer({
    VesperPlayerSource? initialSource,
    VesperPlayerRenderSurfaceKind renderSurfaceKind =
        VesperPlayerRenderSurfaceKind.auto,
    VesperPlaybackResiliencePolicy resiliencePolicy =
        const VesperPlaybackResiliencePolicy(),
    VesperTrackPreferencePolicy trackPreferencePolicy =
        const VesperTrackPreferencePolicy(),
    VesperPreloadBudgetPolicy preloadBudgetPolicy =
        const VesperPreloadBudgetPolicy(),
    bool keepScreenOnDuringPlayback = true,
    VesperBenchmarkConfiguration benchmarkConfiguration =
        const VesperBenchmarkConfiguration.disabled(),
    VesperSourceNormalizerConfiguration sourceNormalizerConfiguration =
        const VesperSourceNormalizerConfiguration(),
    VesperFrameProcessorConfiguration frameProcessorConfiguration =
        const VesperFrameProcessorConfiguration(),
  }) async {
    final trackPreferenceMap = trackPreferencePolicy.toMap();
    final preloadBudgetMap = preloadBudgetPolicy.toMap();
    final result =
        await _invokeMethod<Object?>('createPlayer', <String, Object?>{
      'initialSource': initialSource?.toMap(),
      'renderSurfaceKind': renderSurfaceKind.name,
      'resiliencePolicy': resiliencePolicy.toMap(),
      if (trackPreferenceMap.isNotEmpty)
        'trackPreferencePolicy': trackPreferenceMap,
      if (preloadBudgetMap.isNotEmpty) 'preloadBudgetPolicy': preloadBudgetMap,
      if (!keepScreenOnDuringPlayback)
        'keepScreenOnDuringPlayback': keepScreenOnDuringPlayback,
      if (benchmarkConfiguration.hasOverrides)
        'benchmarkConfiguration': benchmarkConfiguration.toMap(),
      if (sourceNormalizerConfiguration.hasOverrides)
        'sourceNormalizer': sourceNormalizerConfiguration.toMap(),
      if (frameProcessorConfiguration.hasOverrides)
        'frameProcessor': frameProcessorConfiguration.toMap(),
    });
    final decoded = result is Map
        ? Map<Object?, Object?>.from(result)
        : <Object?, Object?>{};
    return VesperPlatformCreateResult.fromMap(decoded);
  }

  @override
  Stream<VesperPlayerEvent> eventsFor(String playerId) {
    return _events.where((event) => event.playerId == playerId);
  }

  @override
  Future<void> initialize(String playerId) {
    return _invokeVoid('initialize', <String, Object?>{'playerId': playerId});
  }

  @override
  Future<void> dispose(String playerId) {
    return _invokeVoid('disposePlayer', <String, Object?>{
      'playerId': playerId,
    });
  }

  @override
  Future<void> refreshPlayer(String playerId) {
    return _invokeVoid('refreshPlayer', <String, Object?>{
      'playerId': playerId,
    });
  }

  @override
  Future<void> selectSource(String playerId, VesperPlayerSource source) {
    return _invokeVoid('selectSource', <String, Object?>{
      'playerId': playerId,
      'source': source.toMap(),
    });
  }

  @override
  Future<void> play(String playerId) {
    return _invokeVoid('play', <String, Object?>{'playerId': playerId});
  }

  @override
  Future<void> pause(String playerId) {
    return _invokeVoid('pause', <String, Object?>{'playerId': playerId});
  }

  @override
  Future<void> togglePause(String playerId) {
    return _invokeVoid('togglePause', <String, Object?>{'playerId': playerId});
  }

  @override
  Future<void> stop(String playerId) {
    return _invokeVoid('stop', <String, Object?>{'playerId': playerId});
  }

  @override
  Future<void> seekBy(String playerId, int deltaMs) {
    return _invokeVoid('seekBy', <String, Object?>{
      'playerId': playerId,
      'deltaMs': deltaMs,
    });
  }

  @override
  Future<void> seekToRatio(String playerId, double ratio) {
    return _invokeVoid('seekToRatio', <String, Object?>{
      'playerId': playerId,
      'ratio': ratio,
    });
  }

  @override
  Future<void> seekToLiveEdge(String playerId) {
    return _invokeVoid('seekToLiveEdge', <String, Object?>{
      'playerId': playerId,
    });
  }

  @override
  Future<void> setPlaybackRate(String playerId, double rate) {
    return _invokeVoid('setPlaybackRate', <String, Object?>{
      'playerId': playerId,
      'rate': rate,
    });
  }

  @override
  Future<void> setVideoTrackSelection(
    String playerId,
    VesperTrackSelection selection,
  ) {
    return _invokeVoid('setVideoTrackSelection', <String, Object?>{
      'playerId': playerId,
      'selection': selection.toMap(),
    });
  }

  @override
  Future<void> setAudioTrackSelection(
    String playerId,
    VesperTrackSelection selection,
  ) {
    return _invokeVoid('setAudioTrackSelection', <String, Object?>{
      'playerId': playerId,
      'selection': selection.toMap(),
    });
  }

  @override
  Future<void> setSubtitleTrackSelection(
    String playerId,
    VesperTrackSelection selection,
  ) {
    return _invokeVoid('setSubtitleTrackSelection', <String, Object?>{
      'playerId': playerId,
      'selection': selection.toMap(),
    });
  }

  @override
  Future<void> setAbrPolicy(String playerId, VesperAbrPolicy policy) {
    return _invokeVoid('setAbrPolicy', <String, Object?>{
      'playerId': playerId,
      'policy': policy.toMap(),
    });
  }

  @override
  Future<void> setResiliencePolicy(
    String playerId,
    VesperPlaybackResiliencePolicy policy,
  ) {
    return _invokeVoid('setResiliencePolicy', <String, Object?>{
      'playerId': playerId,
      'policy': policy.toMap(),
    });
  }

  @override
  Future<void> setKeepScreenOnDuringPlayback(
    String playerId,
    bool enabled,
  ) {
    return _invokeVoid('setKeepScreenOnDuringPlayback', <String, Object?>{
      'playerId': playerId,
      'enabled': enabled,
    });
  }

  @override
  Future<void> updateViewport(String playerId, VesperPlayerViewport viewport) {
    final viewportHint = _deriveViewportHint(viewport);
    return _invokeVoid('updateViewport', <String, Object?>{
      'playerId': playerId,
      'viewport': viewport.toMap(),
      'viewportHint': viewportHint.toMap(),
    });
  }

  @override
  Future<void> clearViewport(String playerId) {
    return _invokeVoid('clearViewport', <String, Object?>{
      'playerId': playerId,
    });
  }

  @override
  Future<void> configureSystemPlayback(
    String playerId,
    VesperSystemPlaybackConfiguration configuration,
  ) {
    return _invokeVoid('configureSystemPlayback', <String, Object?>{
      'playerId': playerId,
      'configuration': configuration.toMap(),
    });
  }

  @override
  Future<void> updateSystemPlaybackMetadata(
    String playerId,
    VesperSystemPlaybackMetadata metadata,
  ) {
    return _invokeVoid('updateSystemPlaybackMetadata', <String, Object?>{
      'playerId': playerId,
      'metadata': metadata.toMap(),
    });
  }

  @override
  Future<void> clearSystemPlayback(String playerId) {
    return _invokeVoid('clearSystemPlayback', <String, Object?>{
      'playerId': playerId,
    });
  }

  @override
  Future<VesperSystemPlaybackPermissionStatus>
      requestSystemPlaybackPermissions() async {
    final result = await _invokeMethod<Object?>(
      'requestSystemPlaybackPermissions',
    );
    return _decodePermissionStatus(result);
  }

  @override
  Future<VesperSystemPlaybackPermissionStatus>
      getSystemPlaybackPermissionStatus() async {
    final result = await _invokeMethod<Object?>(
      'getSystemPlaybackPermissionStatus',
    );
    return _decodePermissionStatus(result);
  }

  @override
  Future<VesperPlatformDownloadCreateResult> createDownloadManager({
    VesperDownloadConfiguration configuration =
        const VesperDownloadConfiguration(),
    VesperDownloadStaleResourcePlanRecoveryCallback? staleResourceRecovery,
  }) async {
    final result = await _invokeMethod<Object?>(
      'createDownloadManager',
      <String, Object?>{
        'configuration': configuration.toMap(),
        'hasStaleResourceRecovery': staleResourceRecovery != null,
      },
    );
    final decoded = result is Map
        ? Map<Object?, Object?>.from(result)
        : <Object?, Object?>{};
    final createResult = VesperPlatformDownloadCreateResult.fromMap(decoded);
    if (staleResourceRecovery != null && createResult.downloadId.isNotEmpty) {
      _downloadRecoveryHandlers[createResult.downloadId] =
          staleResourceRecovery;
    }
    return createResult;
  }

  @override
  Stream<VesperDownloadManagerEvent> downloadEventsFor(String downloadId) {
    return _downloadEvents.where((event) => event.downloadId == downloadId);
  }

  @override
  Future<void> refreshDownloadManager(String downloadId) {
    return _invokeVoid('refreshDownloadManager', <String, Object?>{
      'downloadId': downloadId,
    });
  }

  @override
  Future<void> disposeDownloadManager(String downloadId) {
    _downloadRecoveryHandlers.remove(downloadId);
    return _invokeVoid('disposeDownloadManager', <String, Object?>{
      'downloadId': downloadId,
    });
  }

  @override
  Future<int?> createDownloadTask(
    String downloadId, {
    required String assetId,
    required VesperDownloadSource source,
    VesperDownloadProfile profile = const VesperDownloadProfile(),
    VesperDownloadAssetIndex assetIndex = const VesperDownloadAssetIndex(),
  }) async {
    final result = await _invokeMethod<Object?>(
      'createDownloadTask',
      <String, Object?>{
        'downloadId': downloadId,
        'assetId': assetId,
        'source': source.toMap(),
        'profile': profile.toMap(),
        'assetIndex': assetIndex.toMap(),
      },
    );
    return result is int ? result : null;
  }

  @override
  Future<bool> startDownloadTask(String downloadId, int taskId) async {
    final result = await _invokeMethod<Object?>(
      'startDownloadTask',
      <String, Object?>{'downloadId': downloadId, 'taskId': taskId},
    );
    return result == true;
  }

  @override
  Future<bool> pauseDownloadTask(String downloadId, int taskId) async {
    final result = await _invokeMethod<Object?>(
      'pauseDownloadTask',
      <String, Object?>{'downloadId': downloadId, 'taskId': taskId},
    );
    return result == true;
  }

  @override
  Future<bool> resumeDownloadTask(String downloadId, int taskId) async {
    final result = await _invokeMethod<Object?>(
      'resumeDownloadTask',
      <String, Object?>{'downloadId': downloadId, 'taskId': taskId},
    );
    return result == true;
  }

  @override
  Future<bool> removeDownloadTask(String downloadId, int taskId) async {
    final result = await _invokeMethod<Object?>(
      'removeDownloadTask',
      <String, Object?>{'downloadId': downloadId, 'taskId': taskId},
    );
    return result == true;
  }

  @override
  Future<void> exportDownloadTask(
    String downloadId,
    int taskId,
    String outputPath,
  ) {
    return _invokeVoid('exportDownloadTask', <String, Object?>{
      'downloadId': downloadId,
      'taskId': taskId,
      'outputPath': outputPath,
    });
  }

  @override
  Future<void> shareDownloadTask(
    String downloadId,
    int taskId, {
    String? fileName,
    String? mimeType,
  }) {
    return _invokeVoid('shareDownloadTask', <String, Object?>{
      'downloadId': downloadId,
      'taskId': taskId,
      'fileName': fileName,
      'mimeType': mimeType,
    });
  }

  @override
  Future<String?> saveDownloadTask(
    String downloadId,
    int taskId, {
    String? fileName,
    VesperDownloadPublicCollection collection =
        VesperDownloadPublicCollection.downloads,
  }) {
    return _invokeMethod<String>(
      'saveDownloadTask',
      <String, Object?>{
        'downloadId': downloadId,
        'taskId': taskId,
        'fileName': fileName,
        'collection': collection.name,
      },
    );
  }

  Future<void> _invokeVoid(String method, [Object? arguments]) async {
    await _invokeMethod<void>(method, arguments);
  }

  Future<T?> _invokeMethod<T>(String method, [Object? arguments]) async {
    _ensureMethodCallHandlerRegistered();
    try {
      return await _methodChannel.invokeMethod<T>(method, arguments);
    } on PlatformException catch (error) {
      throw vesperMapPlatformException(error);
    }
  }

  void _ensureMethodCallHandlerRegistered() {
    if (_methodCallHandlerRegistered) {
      return;
    }
    _methodChannel.setMethodCallHandler(_handleMethodCall);
    _methodCallHandlerRegistered = true;
  }

  Future<Object?> _handleMethodCall(MethodCall call) async {
    if (call.method != 'recoverDownloadTaskPlan') {
      throw MissingPluginException();
    }
    final arguments = call.arguments is Map
        ? Map<Object?, Object?>.from(call.arguments as Map)
        : <Object?, Object?>{};
    final downloadId = arguments['downloadId'] as String? ?? '';
    final handler = _downloadRecoveryHandlers[downloadId];
    if (handler == null) {
      return null;
    }
    final plan = await handler(
      VesperDownloadTaskSnapshot.fromMap(vesperDecodeMap(arguments['task'])),
      VesperDownloadStaleResource.fromMap(
        vesperDecodeMap(arguments['staleResource']),
      ),
    );
    return plan?.toMap();
  }
}

VesperSystemPlaybackPermissionStatus _decodePermissionStatus(Object? raw) {
  if (raw is String) {
    for (final value in VesperSystemPlaybackPermissionStatus.values) {
      if (value.name == raw) {
        return value;
      }
    }
  }
  return VesperSystemPlaybackPermissionStatus.denied;
}

VesperViewportHint _deriveViewportHint(VesperPlayerViewport viewport) {
  final view = ui.PlatformDispatcher.instance.implicitView ??
      (ui.PlatformDispatcher.instance.views.isNotEmpty
          ? ui.PlatformDispatcher.instance.views.first
          : null);
  if (view == null || view.devicePixelRatio <= 0) {
    return const VesperViewportHint.hidden();
  }

  return viewport.classifyHint(
    surfaceWidth: view.physicalSize.width / view.devicePixelRatio,
    surfaceHeight: view.physicalSize.height / view.devicePixelRatio,
  );
}
