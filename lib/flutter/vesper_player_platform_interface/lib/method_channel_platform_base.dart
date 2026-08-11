import 'dart:ui' as ui;

import 'package:flutter/services.dart';

import 'src/download_events.dart';
import 'src/download_models.dart';
import 'src/events.dart';
import 'src/models.dart';
import 'src/platform_error_mapping.dart';
import 'src/sequence_models.dart';
import 'src/vesper_player_platform.dart';

const Duration _vesperDownloadRecoveryTimeout = Duration(seconds: 30);

abstract class VesperMethodChannelPlatformBase extends VesperPlayerPlatform {
  VesperMethodChannelPlatformBase({
    required this.methodChannel,
    required this.eventChannel,
    required this.downloadEventChannel,
    required this.sequenceEventChannel,
  });

  final MethodChannel methodChannel;
  final EventChannel eventChannel;
  final EventChannel downloadEventChannel;
  final EventChannel sequenceEventChannel;

  late final Stream<VesperPlayerEvent> _events = eventChannel
      .receiveBroadcastStream()
      .where((dynamic event) => event is Map)
      .map((dynamic event) => Map<Object?, Object?>.from(event as Map))
      .map(VesperPlayerEvent.fromMap)
      .asBroadcastStream();

  late final Stream<VesperDownloadManagerEvent> _downloadEvents =
      downloadEventChannel
          .receiveBroadcastStream()
          .where((dynamic event) => event is Map)
          .map((dynamic event) => Map<Object?, Object?>.from(event as Map))
          .map(VesperDownloadManagerEvent.fromMap)
          .asBroadcastStream();

  late final Stream<VesperPlaybackSequenceEvent> _sequenceEvents =
      sequenceEventChannel
          .receiveBroadcastStream()
          .where((dynamic event) => event is Map)
          .map((dynamic event) => Map<Object?, Object?>.from(event as Map))
          .map(VesperPlaybackSequenceEvent.fromMap)
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
    VesperNativeFramePipelineConfiguration nativeFramePipelineConfiguration =
        const VesperNativeFramePipelineConfiguration(),
    VesperPipelineEventHookConfiguration pipelineEventHookConfiguration =
        const VesperPipelineEventHookConfiguration(),
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
      if (nativeFramePipelineConfiguration.hasOverrides)
        'nativeFramePipeline': nativeFramePipelineConfiguration.toMap(),
      if (pipelineEventHookConfiguration.hasOverrides)
        'pipelineEventHook': pipelineEventHookConfiguration.toMap(),
    });
    final decoded = result is Map
        ? Map<Object?, Object?>.from(result)
        : <Object?, Object?>{};
    return VesperPlatformCreateResult.fromMap(decoded);
  }

  @override
  Future<VesperPlaybackCapabilityProbeResult> probePlaybackCapability(
      VesperPlaybackCapabilityProbeRequest request,
      {String? playerId}) async {
    final result = await _invokeMethod<Object?>(
      'probePlaybackCapability',
      <String, Object?>{
        ...request.toMap(),
        if (playerId != null) 'playerId': playerId,
      },
    );
    final decoded = result is Map
        ? Map<Object?, Object?>.from(result)
        : <Object?, Object?>{};
    return VesperPlaybackCapabilityProbeResult.fromMap(decoded);
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
  Future<VesperTimeline?> sampleTimeline(String playerId) async {
    Object? raw;
    try {
      raw = await _invokeMethod<Object?>('sampleTimeline', <String, Object?>{
        'playerId': playerId,
      });
    } on MissingPluginException {
      await refreshPlayer(playerId);
      return null;
    }
    if (raw == null) {
      // A null sample is the compatibility signal for an older or
      // temporarily unavailable native sampler. Keep the full-refresh
      // fallback in this adapter so every platform has the same contract.
      await refreshPlayer(playerId);
      return null;
    }
    if (raw is! Map) {
      throw const FormatException('sampleTimeline returned a non-map value.');
    }
    return VesperTimeline.fromSampleMap(
      Map<Object?, Object?>.from(raw),
    );
  }

  @override
  Future<void> selectSource(String playerId, VesperPlayerSource source) {
    return _invokeVoid('selectSource', <String, Object?>{
      'playerId': playerId,
      'source': source.toMap(),
    });
  }

  @override
  Future<VesperPlaybackSequenceSnapshot> createPlaybackSequence(
    String playerId,
    VesperPlaybackSequenceConfiguration configuration,
  ) async {
    final result = await _invokeMethod<Object?>(
      'createPlaybackSequence',
      <String, Object?>{
        'playerId': playerId,
        'configuration': configuration.toMap(),
      },
    );
    return VesperPlaybackSequenceSnapshot.fromMap(vesperDecodeMap(result));
  }

  @override
  Stream<VesperPlaybackSequenceEvent> playbackSequenceEventsFor(
    String sequenceId,
  ) =>
      _sequenceEvents.where((event) => event.sequenceId == sequenceId);

  @override
  Future<Map<String, Object?>> executePlaybackSequenceCommand(
    String sequenceId,
    Map<String, Object?> command,
  ) async {
    final result = await _invokeMethod<Object?>(
      'executePlaybackSequenceCommand',
      <String, Object?>{
        'sequenceId': sequenceId,
        'command': command,
      },
    );
    return vesperDecodeMap(result);
  }

  @override
  Future<VesperPlaybackSequenceSnapshot> playbackSequenceSnapshot(
    String sequenceId,
  ) async {
    final result = await _invokeMethod<Object?>(
      'playbackSequenceSnapshot',
      <String, Object?>{'sequenceId': sequenceId},
    );
    return VesperPlaybackSequenceSnapshot.fromMap(vesperDecodeMap(result));
  }

  @override
  Future<void> disposePlaybackSequence(String sequenceId) {
    return _invokeVoid('disposePlaybackSequence', <String, Object?>{
      'sequenceId': sequenceId,
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
  Future<void> setSubtitleStyle(
    String playerId,
    VesperSubtitleStyle style,
  ) {
    return _invokeVoid('setSubtitleStyle', <String, Object?>{
      'playerId': playerId,
      'style': style.toMap(),
    });
  }

  @override
  Future<void> setAbrPolicy(
    String playerId,
    VesperAbrPolicy policy, {
    int? expectedCatalogRevision,
  }) {
    return _invokeVoid('setAbrPolicy', <String, Object?>{
      'playerId': playerId,
      'policy': policy.toMap(),
      'expectedCatalogRevision': expectedCatalogRevision,
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
  Future<VesperPictureInPictureAvailability> isPictureInPictureAvailable(
    String playerId,
  ) async {
    final result = await _invokeMethod<Object?>(
      'isPictureInPictureAvailable',
      <String, Object?>{'playerId': playerId},
    );
    final decoded = result is Map
        ? Map<Object?, Object?>.from(result)
        : <Object?, Object?>{};
    return VesperPictureInPictureAvailability.fromMap(decoded);
  }

  @override
  Future<void> requestPictureInPicture(
    String playerId, {
    VesperPictureInPictureConfiguration? configuration,
  }) {
    final arguments = <String, Object?>{
      'playerId': playerId,
      if (configuration != null) 'configuration': configuration.toMap(),
    };
    return _invokeVoid('requestPictureInPicture', arguments);
  }

  @override
  Future<void> exitPictureInPicture(String playerId) {
    return _invokeVoid('exitPictureInPicture', <String, Object?>{
      'playerId': playerId,
    });
  }

  @override
  Future<void> setPictureInPictureConfiguration(
    String playerId,
    VesperPictureInPictureConfiguration configuration,
  ) {
    return _invokeVoid('setPictureInPictureConfiguration', <String, Object?>{
      'playerId': playerId,
      'configuration': configuration.toMap(),
    });
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
      return await methodChannel.invokeMethod<T>(method, arguments);
    } on PlatformException catch (error) {
      throw vesperMapPlatformException(error);
    }
  }

  void _ensureMethodCallHandlerRegistered() {
    if (_methodCallHandlerRegistered) {
      return;
    }
    methodChannel.setMethodCallHandler(_handleMethodCall);
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
    final plan = await Future<VesperDownloadRecoveredTaskPlan?>.sync(
      () => handler(
        VesperDownloadTaskSnapshot.fromMap(vesperDecodeMap(arguments['task'])),
        VesperDownloadStaleResource.fromMap(
          vesperDecodeMap(arguments['staleResource']),
        ),
      ),
    ).timeout(_vesperDownloadRecoveryTimeout, onTimeout: () => null);
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
