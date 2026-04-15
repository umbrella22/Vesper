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

  late final Stream<VesperPlayerEvent> _events = _eventChannel
      .receiveBroadcastStream()
      .where((dynamic event) => event is Map)
      .map((dynamic event) => Map<Object?, Object?>.from(event as Map))
      .map(VesperPlayerEvent.fromMap)
      .asBroadcastStream();

  @override
  Future<VesperPlatformCreateResult> createPlayer({
    VesperPlayerSource? initialSource,
    VesperPlaybackResiliencePolicy resiliencePolicy =
        const VesperPlaybackResiliencePolicy(),
    VesperTrackPreferencePolicy trackPreferencePolicy =
        const VesperTrackPreferencePolicy(),
    VesperPreloadBudgetPolicy preloadBudgetPolicy =
        const VesperPreloadBudgetPolicy(),
  }) async {
    final trackPreferenceMap = trackPreferencePolicy.toMap();
    final preloadBudgetMap = preloadBudgetPolicy.toMap();
    final result = await _methodChannel
        .invokeMethod<Object?>('createPlayer', <String, Object?>{
      'initialSource': initialSource?.toMap(),
      'resiliencePolicy': resiliencePolicy.toMap(),
      if (trackPreferenceMap.isNotEmpty)
        'trackPreferencePolicy': trackPreferenceMap,
      if (preloadBudgetMap.isNotEmpty) 'preloadBudgetPolicy': preloadBudgetMap,
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

  Future<void> _invokeVoid(String method, [Object? arguments]) async {
    await _methodChannel.invokeMethod<void>(method, arguments);
  }
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
