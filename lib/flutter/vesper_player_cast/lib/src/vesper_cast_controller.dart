import 'dart:async';

import 'package:flutter/foundation.dart';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:vesper_player/vesper_player.dart';

enum VesperCastOperationStatus { success, unavailable, unsupported }

enum VesperCastSessionEventKind { started, resumed, ended, suspended }

final class VesperCastConfiguration {
  const VesperCastConfiguration({this.receiverApplicationId});

  final String? receiverApplicationId;

  Map<String, Object?> toMap() {
    return <String, Object?>{'receiverApplicationId': receiverApplicationId};
  }
}

final class VesperCastOperationResult {
  const VesperCastOperationResult({required this.status, this.message});

  factory VesperCastOperationResult.fromMap(Map<Object?, Object?> map) {
    return VesperCastOperationResult(
      status: _decodeEnum(
        VesperCastOperationStatus.values,
        map['status'],
        VesperCastOperationStatus.unavailable,
      ),
      message: map['message'] as String?,
    );
  }

  final VesperCastOperationStatus status;
  final String? message;

  bool get isSuccess => status == VesperCastOperationStatus.success;
}

final class VesperCastSessionEvent {
  const VesperCastSessionEvent({
    required this.kind,
    this.routeName,
    this.positionMs,
  });

  factory VesperCastSessionEvent.fromMap(Map<Object?, Object?> map) {
    return VesperCastSessionEvent(
      kind: _decodeEnum(
        VesperCastSessionEventKind.values,
        map['kind'],
        VesperCastSessionEventKind.suspended,
      ),
      routeName: map['routeName'] as String?,
      positionMs: (map['positionMs'] as num?)?.toInt(),
    );
  }

  final VesperCastSessionEventKind kind;
  final String? routeName;
  final int? positionMs;
}

class VesperCastController {
  VesperCastController({
    MethodChannel? methodChannel,
    EventChannel? eventChannel,
  }) : _methodChannel = methodChannel ?? _defaultMethodChannel,
       _eventChannel = eventChannel ?? _defaultEventChannel;

  final MethodChannel _methodChannel;
  final EventChannel _eventChannel;

  Stream<VesperCastSessionEvent>? _events;

  Stream<VesperCastSessionEvent> get events {
    return _events ??= _eventChannel
        .receiveBroadcastStream()
        .where((event) => event is Map)
        .map(
          (event) => VesperCastSessionEvent.fromMap(
            Map<Object?, Object?>.from(event as Map),
          ),
        );
  }

  Future<bool> isCastSessionAvailable() async {
    final result = await _methodChannel.invokeMethod<Object?>(
      'isCastSessionAvailable',
    );
    return result == true;
  }

  Future<VesperCastOperationResult> loadRemoteSource({
    required VesperPlayerSource source,
    VesperSystemPlaybackMetadata? metadata,
    int startPositionMs = 0,
    bool autoplay = true,
  }) async {
    final result = await _methodChannel
        .invokeMethod<Object?>('loadRemoteMedia', <String, Object?>{
          'source': source.toMap(),
          'metadata': metadata?.toMap(),
          'startPositionMs': startPositionMs,
          'autoplay': autoplay,
        });
    return _decodeOperationResult(result);
  }

  Future<VesperCastOperationResult> loadFromPlayer({
    required VesperPlayerController player,
    required VesperPlayerSource source,
    VesperSystemPlaybackMetadata? metadata,
  }) async {
    final wasPlaying =
        player.snapshot.playbackState == VesperPlaybackState.playing;
    final result = await loadRemoteSource(
      source: source,
      metadata: metadata,
      startPositionMs: player.snapshot.timeline.positionMs,
      autoplay: wasPlaying,
    );
    if (result.isSuccess && wasPlaying) {
      await player.pause();
    }
    return result;
  }

  Future<VesperCastOperationResult> play() => _invokeOperation('play');

  Future<VesperCastOperationResult> pause() => _invokeOperation('pause');

  Future<VesperCastOperationResult> stop() => _invokeOperation('stop');

  Future<VesperCastOperationResult> seekTo(int positionMs) {
    return _invokeOperation('seekTo', <String, Object?>{
      'positionMs': positionMs,
    });
  }

  Future<VesperCastOperationResult> _invokeOperation(
    String method, [
    Map<String, Object?>? arguments,
  ]) async {
    final result = await _methodChannel.invokeMethod<Object?>(
      method,
      arguments,
    );
    return _decodeOperationResult(result);
  }
}

class VesperCastButton extends StatelessWidget {
  const VesperCastButton({super.key, this.size = 40});

  final double size;

  @override
  Widget build(BuildContext context) {
    if (kIsWeb || defaultTargetPlatform != TargetPlatform.android) {
      return SizedBox.square(dimension: size);
    }
    return SizedBox.square(
      dimension: size,
      child: const AndroidView(
        viewType: _castButtonViewType,
        creationParamsCodec: StandardMessageCodec(),
      ),
    );
  }
}

VesperCastOperationResult _decodeOperationResult(Object? result) {
  if (result is Map) {
    return VesperCastOperationResult.fromMap(
      Map<Object?, Object?>.from(result),
    );
  }
  return const VesperCastOperationResult(
    status: VesperCastOperationStatus.unavailable,
    message: 'Cast operation did not return a result.',
  );
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

const MethodChannel _defaultMethodChannel = MethodChannel(
  'io.github.ikaros.vesper_player_cast',
);
const EventChannel _defaultEventChannel = EventChannel(
  'io.github.ikaros.vesper_player_cast/events',
);
const String _castButtonViewType =
    'io.github.ikaros.vesper_player_cast/cast_button';
