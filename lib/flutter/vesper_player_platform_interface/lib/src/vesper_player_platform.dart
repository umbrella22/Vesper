import 'events.dart';
import 'models.dart';
import 'package:plugin_platform_interface/plugin_platform_interface.dart';

final class VesperPlatformCreateResult {
  const VesperPlatformCreateResult({
    required this.playerId,
    required this.snapshot,
  });

  factory VesperPlatformCreateResult.fromMap(Map<Object?, Object?> map) {
    final rawSnapshot = vesperDecodeMap(map['snapshot']);
    return VesperPlatformCreateResult(
      playerId: map['playerId'] as String? ?? '',
      snapshot: rawSnapshot.isNotEmpty
          ? VesperPlayerSnapshot.fromMap(rawSnapshot)
          : const VesperPlayerSnapshot.initial(),
    );
  }

  final String playerId;
  final VesperPlayerSnapshot snapshot;
}

class VesperUnsupportedError extends UnsupportedError {
  VesperUnsupportedError([String? message])
    : super(message ?? 'Vesper player is not supported on this platform.');
}

abstract class VesperPlayerPlatform extends PlatformInterface {
  VesperPlayerPlatform() : super(token: _token);

  static final Object _token = Object();

  static VesperPlayerPlatform _instance = _UnsupportedVesperPlayerPlatform();

  static VesperPlayerPlatform get instance => _instance;

  static set instance(VesperPlayerPlatform instance) {
    PlatformInterface.verifyToken(instance, _token);
    _instance = instance;
  }

  Future<VesperPlatformCreateResult> createPlayer({
    VesperPlayerSource? initialSource,
    VesperPlaybackResiliencePolicy resiliencePolicy =
        const VesperPlaybackResiliencePolicy(),
  });

  Stream<VesperPlayerEvent> eventsFor(String playerId);

  Future<void> initialize(String playerId);

  Future<void> dispose(String playerId);

  Future<void> selectSource(String playerId, VesperPlayerSource source);

  Future<void> play(String playerId);

  Future<void> pause(String playerId);

  Future<void> togglePause(String playerId);

  Future<void> stop(String playerId);

  Future<void> seekBy(String playerId, int deltaMs);

  Future<void> seekToRatio(String playerId, double ratio);

  Future<void> seekToLiveEdge(String playerId);

  Future<void> setPlaybackRate(String playerId, double rate);

  Future<void> setVideoTrackSelection(
    String playerId,
    VesperTrackSelection selection,
  );

  Future<void> setAudioTrackSelection(
    String playerId,
    VesperTrackSelection selection,
  );

  Future<void> setSubtitleTrackSelection(
    String playerId,
    VesperTrackSelection selection,
  );

  Future<void> setAbrPolicy(String playerId, VesperAbrPolicy policy);

  Future<void> setResiliencePolicy(
    String playerId,
    VesperPlaybackResiliencePolicy policy,
  );

  Future<void> updateViewport(String playerId, VesperPlayerViewport viewport);

  Future<void> clearViewport(String playerId);
}

final class _UnsupportedVesperPlayerPlatform extends VesperPlayerPlatform {
  @override
  Future<VesperPlatformCreateResult> createPlayer({
    VesperPlayerSource? initialSource,
    VesperPlaybackResiliencePolicy resiliencePolicy =
        const VesperPlaybackResiliencePolicy(),
  }) async {
    throw VesperUnsupportedError();
  }

  @override
  Stream<VesperPlayerEvent> eventsFor(String playerId) {
    return const Stream<VesperPlayerEvent>.empty();
  }

  @override
  Future<void> initialize(String playerId) async =>
      throw VesperUnsupportedError();

  @override
  Future<void> dispose(String playerId) async => throw VesperUnsupportedError();

  @override
  Future<void> selectSource(String playerId, VesperPlayerSource source) async =>
      throw VesperUnsupportedError();

  @override
  Future<void> play(String playerId) async => throw VesperUnsupportedError();

  @override
  Future<void> pause(String playerId) async => throw VesperUnsupportedError();

  @override
  Future<void> togglePause(String playerId) async =>
      throw VesperUnsupportedError();

  @override
  Future<void> stop(String playerId) async => throw VesperUnsupportedError();

  @override
  Future<void> seekBy(String playerId, int deltaMs) async =>
      throw VesperUnsupportedError();

  @override
  Future<void> seekToRatio(String playerId, double ratio) async =>
      throw VesperUnsupportedError();

  @override
  Future<void> seekToLiveEdge(String playerId) async =>
      throw VesperUnsupportedError();

  @override
  Future<void> setPlaybackRate(String playerId, double rate) async =>
      throw VesperUnsupportedError();

  @override
  Future<void> setVideoTrackSelection(
    String playerId,
    VesperTrackSelection selection,
  ) async => throw VesperUnsupportedError();

  @override
  Future<void> setAudioTrackSelection(
    String playerId,
    VesperTrackSelection selection,
  ) async => throw VesperUnsupportedError();

  @override
  Future<void> setSubtitleTrackSelection(
    String playerId,
    VesperTrackSelection selection,
  ) async => throw VesperUnsupportedError();

  @override
  Future<void> setAbrPolicy(String playerId, VesperAbrPolicy policy) async =>
      throw VesperUnsupportedError();

  @override
  Future<void> setResiliencePolicy(
    String playerId,
    VesperPlaybackResiliencePolicy policy,
  ) async => throw VesperUnsupportedError();

  @override
  Future<void> updateViewport(
    String playerId,
    VesperPlayerViewport viewport,
  ) async => throw VesperUnsupportedError();

  @override
  Future<void> clearViewport(String playerId) async =>
      throw VesperUnsupportedError();
}
