import 'dart:async';

import 'package:flutter/foundation.dart';
import 'package:flutter/services.dart';
import 'package:material_ui/material_ui.dart';
import 'package:vesper_player/vesper_player.dart';

part 'external_playback_route_buttons.dart';
part 'external_playback_decode_helpers.dart';
part 'external_playback_channels.dart';

class VesperExternalPlaybackController {
  static const String castRouteId = 'cast:active';

  VesperExternalPlaybackController({
    MethodChannel? methodChannel,
    EventChannel? routesEventChannel,
    EventChannel? sessionEventChannel,
  })  : _methodChannel = methodChannel ?? _defaultMethodChannel,
        _routesEventChannel = routesEventChannel ?? _defaultRoutesEventChannel,
        _sessionEventChannel =
            sessionEventChannel ?? _defaultSessionEventChannel,
        _usesDefaultRoutesEventChannel = routesEventChannel == null,
        _usesDefaultSessionEventChannel = sessionEventChannel == null;

  final MethodChannel _methodChannel;
  final EventChannel _routesEventChannel;
  final EventChannel _sessionEventChannel;
  final bool _usesDefaultRoutesEventChannel;
  final bool _usesDefaultSessionEventChannel;

  Stream<List<VesperExternalPlaybackRoute>>? _nativeRoutes;
  StreamSubscription<List<VesperExternalPlaybackRoute>>?
      _instanceRoutesEventSubscription;
  StreamSubscription<VesperExternalPlaybackSessionEvent>?
      _instanceEventsSubscription;
  Stream<VesperExternalPlaybackSessionEvent>? _events;
  List<VesperExternalPlaybackRoute>? _latestRoutes;
  bool _disposed = false;

  static Stream<List<VesperExternalPlaybackRoute>>? _sharedNativeRoutes;
  static Stream<VesperExternalPlaybackSessionEvent>? _sharedEvents;
  static List<VesperExternalPlaybackRoute>? _sharedLatestRoutes;

  Stream<List<VesperExternalPlaybackRoute>> get routes {
    _ensureActive();
    final nativeRoutes = _usesDefaultRoutesEventChannel
        ? _sharedRoutesStream()
        : _instanceRoutesStream();
    return Stream<List<VesperExternalPlaybackRoute>>.multi((controller) {
      final latestRoutes =
          _usesDefaultRoutesEventChannel ? _sharedLatestRoutes : _latestRoutes;
      if (latestRoutes != null) {
        controller.add(latestRoutes);
      }
      final subscription = nativeRoutes.listen(
        controller.add,
        onError: controller.addError,
        onDone: controller.close,
      );
      controller.onCancel = subscription.cancel;
    });
  }

  Stream<VesperExternalPlaybackSessionEvent> get events {
    _ensureActive();
    if (_usesDefaultSessionEventChannel) {
      return _sharedEventsStream();
    }
    if (_events != null) {
      return _events!;
    }
    final controller = StreamController<VesperExternalPlaybackSessionEvent>.broadcast(
      onCancel: () {
        _instanceEventsSubscription?.cancel();
        _instanceEventsSubscription = null;
        _events = null;
      },
    );
    _instanceEventsSubscription = _sessionEventChannel
        .receiveBroadcastStream()
        .where((event) => event is Map)
        .map(
          (event) => VesperExternalPlaybackSessionEvent.fromMap(
            Map<Object?, Object?>.from(event as Map),
          ),
        )
        .listen(
          controller.add,
          onError: controller.addError,
          onDone: controller.close,
        );
    _events = controller.stream;
    return _events!;
  }

  Stream<List<VesperExternalPlaybackRoute>> _instanceRoutesStream() {
    if (_nativeRoutes != null) {
      return _nativeRoutes!;
    }
    final controller =
        StreamController<List<VesperExternalPlaybackRoute>>.broadcast(
      onCancel: () {
        _instanceRoutesEventSubscription?.cancel();
        _instanceRoutesEventSubscription = null;
        _latestRoutes = null;
        _nativeRoutes = null;
      },
    );
    _instanceRoutesEventSubscription = _routesEventChannel
        .receiveBroadcastStream()
        .map(_decodeRoutes)
        .listen(
      (routes) {
        _latestRoutes = routes;
        controller.add(routes);
      },
      onError: controller.addError,
      onDone: controller.close,
    );
    _nativeRoutes = controller.stream;
    return _nativeRoutes!;
  }

  static Stream<List<VesperExternalPlaybackRoute>> _sharedRoutesStream() {
    return _sharedNativeRoutes ??= _defaultRoutesEventChannel
        .receiveBroadcastStream()
        .map(_decodeRoutes)
        .map((routes) {
      _sharedLatestRoutes = routes;
      return routes;
    }).asBroadcastStream(
      onCancel: (subscription) {
        _sharedLatestRoutes = null;
        _sharedNativeRoutes = null;
        unawaited(subscription.cancel());
      },
    );
  }

  static Stream<VesperExternalPlaybackSessionEvent> _sharedEventsStream() {
    return _sharedEvents ??= _defaultSessionEventChannel
        .receiveBroadcastStream()
        .where((event) => event is Map)
        .map(
          (event) => VesperExternalPlaybackSessionEvent.fromMap(
            Map<Object?, Object?>.from(event as Map),
          ),
        )
        .asBroadcastStream(
      onCancel: (subscription) {
        _sharedEvents = null;
        unawaited(subscription.cancel());
      },
    );
  }

  Future<void> startDiscovery() {
    _ensureActive();
    return _methodChannel.invokeMethod<void>('startDiscovery');
  }

  Future<void> stopDiscovery() {
    _ensureActive();
    return _methodChannel.invokeMethod<void>('stopDiscovery');
  }

  Future<VesperExternalPlaybackResult> showRoutePicker({
    Brightness? brightness,
  }) {
    _ensureActive();
    return _invokeResult(
      'showRoutePicker',
      <String, Object?>{
        if (brightness != null) 'brightness': brightness.name,
      },
    );
  }

  Future<VesperExternalPlaybackResult> connect(String routeId) {
    _ensureActive();
    return _invokeResult('connect', <String, Object?>{'routeId': routeId});
  }

  Future<VesperExternalPlaybackResult> load(
    VesperExternalPlaybackMediaItem item, {
    int startPositionMs = 0,
    bool autoplay = true,
  }) {
    _ensureActive();
    return _invokeResult('load', <String, Object?>{
      'item': item.toMap(),
      'startPositionMs': startPositionMs,
      'autoplay': autoplay,
    });
  }

  Future<VesperExternalPlaybackResult> loadFromPlayer({
    required VesperPlayerController player,
    required VesperPlayerSource source,
    VesperSystemPlaybackMetadata? metadata,
    VesperExternalProxyPolicy proxyPolicy = VesperExternalProxyPolicy.auto,
    VesperExternalFormatAdaptationConfig formatAdaptation =
        const VesperExternalFormatAdaptationConfig.disabled(),
  }) async {
    _ensureActive();
    final wasPlaying =
        player.snapshot.playbackState == VesperPlaybackState.playing;
    final result = await load(
      VesperExternalPlaybackMediaItem(
        sources: <VesperPlayerSource>[source],
        metadata: metadata ??
            VesperSystemPlaybackMetadata(
              title: source.label,
              contentUri: source.uri,
            ),
        proxyPolicy: proxyPolicy,
        formatAdaptation: formatAdaptation,
      ),
      startPositionMs: player.snapshot.timeline.positionMs,
      autoplay: wasPlaying,
    );
    if (result.isSuccess && wasPlaying) {
      await player.pause();
    }
    return result;
  }

  Future<VesperExternalPlaybackResult> play() {
    _ensureActive();
    return _invokeResult('play');
  }

  Future<VesperExternalPlaybackResult> pause() {
    _ensureActive();
    return _invokeResult('pause');
  }

  Future<VesperExternalPlaybackResult> stop() {
    _ensureActive();
    return _invokeResult('stop');
  }

  Future<VesperExternalPlaybackResult> seekTo(int positionMs) {
    _ensureActive();
    return _invokeResult('seekTo', <String, Object?>{
      'positionMs': positionMs,
    });
  }

  Future<VesperExternalPlaybackResult> disconnect() {
    _ensureActive();
    return _invokeResult('disconnect');
  }

  void dispose() {
    if (_disposed) {
      return;
    }
    _disposed = true;
    if (_usesDefaultRoutesEventChannel) {
      _sharedLatestRoutes = null;
    } else {
      _latestRoutes = null;
      _instanceRoutesEventSubscription?.cancel();
      _instanceRoutesEventSubscription = null;
      _nativeRoutes?.drain<List<VesperExternalPlaybackRoute>>();
      _nativeRoutes = null;
    }
    if (!_usesDefaultSessionEventChannel) {
      _instanceEventsSubscription?.cancel();
      _instanceEventsSubscription = null;
      _events = null;
    }
  }

  Future<VesperExternalPlaybackResult> _invokeResult(
    String method, [
    Map<String, Object?>? arguments,
  ]) async {
    final result = await _methodChannel.invokeMethod<Object?>(
      method,
      arguments,
    );
    if (result is Map) {
      return VesperExternalPlaybackResult.fromMap(
        Map<Object?, Object?>.from(result),
      );
    }
    return const VesperExternalPlaybackResult(
      status: VesperExternalPlaybackResultStatus.failed,
      message: 'External playback operation did not return a result.',
    );
  }

  void _ensureActive() {
    if (_disposed) {
      throw StateError(
        'VesperExternalPlaybackController has already been disposed.',
      );
    }
  }
}
