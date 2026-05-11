import 'dart:async';

import 'package:flutter/foundation.dart';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:vesper_player/vesper_player.dart';

enum VesperExternalProxyPolicy { auto, always, never }

enum VesperExternalPlaybackResultStatus {
  success,
  unavailable,
  unsupported,
  failed,
}

enum VesperExternalPlaybackSessionEventKind {
  routeConnected,
  routeDisconnected,
  loaded,
  playing,
  paused,
  stopped,
  suspended,
  error,
}

final class VesperExternalPlaybackRoute {
  const VesperExternalPlaybackRoute({
    required this.routeId,
    required this.name,
    required this.kind,
    this.manufacturer,
    this.modelName,
    this.active = false,
    this.available = true,
  });

  factory VesperExternalPlaybackRoute.fromMap(Map<Object?, Object?> map) {
    return VesperExternalPlaybackRoute(
      routeId: map['routeId'] as String? ?? '',
      name: map['name'] as String? ?? '',
      kind: _decodeEnum(
        VesperExternalPlaybackRouteKind.values,
        map['kind'],
        VesperExternalPlaybackRouteKind.none,
      ),
      manufacturer: map['manufacturer'] as String?,
      modelName: map['modelName'] as String?,
      active: _decodeBool(map, 'active'),
      available: _decodeBool(map, 'available', fallback: true),
    );
  }

  final String routeId;
  final String name;
  final VesperExternalPlaybackRouteKind kind;
  final String? manufacturer;
  final String? modelName;
  final bool active;
  final bool available;

  Map<String, Object?> toMap() {
    return <String, Object?>{
      'routeId': routeId,
      'name': name,
      'kind': kind.name,
      'manufacturer': manufacturer,
      'modelName': modelName,
      'active': active,
      'available': available,
    };
  }
}

final class VesperExternalPlaybackMediaItem {
  const VesperExternalPlaybackMediaItem({
    required this.sources,
    required this.metadata,
    this.proxyPolicy = VesperExternalProxyPolicy.auto,
  });

  factory VesperExternalPlaybackMediaItem.fromMap(Map<Object?, Object?> map) {
    final rawSources = map['sources'];
    return VesperExternalPlaybackMediaItem(
      sources: rawSources is Iterable
          ? rawSources
              .map(_rawMap)
              .whereType<Map<Object?, Object?>>()
              .map(VesperPlayerSource.fromMap)
              .toList(growable: false)
          : const <VesperPlayerSource>[],
      metadata: VesperSystemPlaybackMetadata.fromMap(
        _rawMap(map['metadata']) ?? const <Object?, Object?>{},
      ),
      proxyPolicy: _decodeEnum(
        VesperExternalProxyPolicy.values,
        map['proxyPolicy'],
        VesperExternalProxyPolicy.auto,
      ),
    );
  }

  final List<VesperPlayerSource> sources;
  final VesperSystemPlaybackMetadata metadata;
  final VesperExternalProxyPolicy proxyPolicy;

  Map<String, Object?> toMap() {
    return <String, Object?>{
      'sources':
          sources.map((source) => source.toMap()).toList(growable: false),
      'metadata': metadata.toMap(),
      'proxyPolicy': proxyPolicy.name,
    };
  }
}

final class VesperExternalPlaybackResult {
  const VesperExternalPlaybackResult({
    required this.status,
    this.message,
    this.routeId,
    this.relayEnabled = false,
  });

  factory VesperExternalPlaybackResult.fromMap(Map<Object?, Object?> map) {
    return VesperExternalPlaybackResult(
      status: _decodeEnum(
        VesperExternalPlaybackResultStatus.values,
        map['status'],
        VesperExternalPlaybackResultStatus.failed,
      ),
      message: map['message'] as String?,
      routeId: map['routeId'] as String?,
      relayEnabled: _decodeBool(map, 'relayEnabled'),
    );
  }

  final VesperExternalPlaybackResultStatus status;
  final String? message;
  final String? routeId;
  final bool relayEnabled;

  bool get isSuccess => status == VesperExternalPlaybackResultStatus.success;
}

final class VesperExternalPlaybackSessionEvent {
  const VesperExternalPlaybackSessionEvent({
    required this.kind,
    this.routeId,
    this.routeName,
    this.message,
    this.positionMs,
  });

  factory VesperExternalPlaybackSessionEvent.fromMap(
    Map<Object?, Object?> map,
  ) {
    return VesperExternalPlaybackSessionEvent(
      kind: _decodeEnum(
        VesperExternalPlaybackSessionEventKind.values,
        map['kind'],
        VesperExternalPlaybackSessionEventKind.error,
      ),
      routeId: map['routeId'] as String?,
      routeName: map['routeName'] as String?,
      message: map['message'] as String?,
      positionMs: (map['positionMs'] as num?)?.toInt(),
    );
  }

  final VesperExternalPlaybackSessionEventKind kind;
  final String? routeId;
  final String? routeName;
  final String? message;
  final int? positionMs;
}

class VesperExternalPlaybackController {
  VesperExternalPlaybackController({
    MethodChannel? methodChannel,
    EventChannel? routesEventChannel,
    EventChannel? sessionEventChannel,
  })  : _methodChannel = methodChannel ?? _defaultMethodChannel,
        _routesEventChannel = routesEventChannel ?? _defaultRoutesEventChannel,
        _sessionEventChannel =
            sessionEventChannel ?? _defaultSessionEventChannel;

  final MethodChannel _methodChannel;
  final EventChannel _routesEventChannel;
  final EventChannel _sessionEventChannel;

  Stream<List<VesperExternalPlaybackRoute>>? _routes;
  Stream<VesperExternalPlaybackSessionEvent>? _events;

  Stream<List<VesperExternalPlaybackRoute>> get routes {
    return _routes ??=
        _routesEventChannel.receiveBroadcastStream().map((event) {
      if (event is! Iterable) {
        return const <VesperExternalPlaybackRoute>[];
      }
      return event
          .map(_rawMap)
          .whereType<Map<Object?, Object?>>()
          .map(VesperExternalPlaybackRoute.fromMap)
          .toList(growable: false);
    });
  }

  Stream<VesperExternalPlaybackSessionEvent> get events {
    return _events ??= _sessionEventChannel
        .receiveBroadcastStream()
        .where((event) => event is Map)
        .map(
          (event) => VesperExternalPlaybackSessionEvent.fromMap(
            Map<Object?, Object?>.from(event as Map),
          ),
        );
  }

  Future<void> startDiscovery() => _methodChannel.invokeMethod<void>(
        'startDiscovery',
      );

  Future<void> stopDiscovery() => _methodChannel.invokeMethod<void>(
        'stopDiscovery',
      );

  Future<VesperExternalPlaybackResult> connect(String routeId) {
    return _invokeResult('connect', <String, Object?>{'routeId': routeId});
  }

  Future<VesperExternalPlaybackResult> load(
    VesperExternalPlaybackMediaItem item, {
    int startPositionMs = 0,
    bool autoplay = true,
  }) {
    return _invokeResult('load', <String, Object?>{
      'item': item.toMap(),
      'startPositionMs': startPositionMs,
      'autoplay': autoplay,
    });
  }

  Future<VesperExternalPlaybackResult> play() => _invokeResult('play');

  Future<VesperExternalPlaybackResult> pause() => _invokeResult('pause');

  Future<VesperExternalPlaybackResult> stop() => _invokeResult('stop');

  Future<VesperExternalPlaybackResult> seekTo(int positionMs) {
    return _invokeResult('seekTo', <String, Object?>{
      'positionMs': positionMs,
    });
  }

  Future<VesperExternalPlaybackResult> disconnect() =>
      _invokeResult('disconnect');

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
}

class VesperExternalRouteButton extends StatelessWidget {
  const VesperExternalRouteButton({super.key, this.size = 40});

  final double size;

  @override
  Widget build(BuildContext context) {
    if (kIsWeb || defaultTargetPlatform != TargetPlatform.android) {
      return SizedBox.square(dimension: size);
    }
    return SizedBox.square(
      dimension: size,
      child: const AndroidView(
        viewType: _routeButtonViewType,
        creationParamsCodec: StandardMessageCodec(),
      ),
    );
  }
}

T _decodeEnum<T extends Enum>(List<T> values, Object? value, T fallback) {
  final name = value as String?;
  if (name == null) {
    return fallback;
  }
  for (final entry in values) {
    if (entry.name == name) {
      return entry;
    }
  }
  return fallback;
}

bool _decodeBool(
  Map<Object?, Object?> map,
  String key, {
  bool fallback = false,
}) {
  final raw = map[key];
  return raw is bool ? raw : fallback;
}

Map<Object?, Object?>? _rawMap(Object? raw) {
  if (raw is Map<Object?, Object?>) {
    return raw;
  }
  if (raw is Map) {
    return Map<Object?, Object?>.from(raw);
  }
  return null;
}

const MethodChannel _defaultMethodChannel = MethodChannel(
  'io.github.ikaros.vesper_player_external_playback',
);
const EventChannel _defaultRoutesEventChannel = EventChannel(
  'io.github.ikaros.vesper_player_external_playback/routes',
);
const EventChannel _defaultSessionEventChannel = EventChannel(
  'io.github.ikaros.vesper_player_external_playback/events',
);
const String _routeButtonViewType =
    'io.github.ikaros.vesper_player_external_playback/route_button';
