import 'events.dart';
import 'download_events.dart';
import 'download_models.dart';
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

final class VesperPlatformDownloadCreateResult {
  const VesperPlatformDownloadCreateResult({
    required this.downloadId,
    required this.snapshot,
  });

  factory VesperPlatformDownloadCreateResult.fromMap(
      Map<Object?, Object?> map) {
    return VesperPlatformDownloadCreateResult(
      downloadId: map['downloadId'] as String? ?? '',
      snapshot:
          VesperDownloadSnapshot.fromMap(vesperDecodeMap(map['snapshot'])),
    );
  }

  final String downloadId;
  final VesperDownloadSnapshot snapshot;
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
    VesperTrackPreferencePolicy trackPreferencePolicy =
        const VesperTrackPreferencePolicy(),
    VesperPreloadBudgetPolicy preloadBudgetPolicy =
        const VesperPreloadBudgetPolicy(),
  });

  Stream<VesperPlayerEvent> eventsFor(String playerId);

  Future<void> initialize(String playerId);

  Future<void> dispose(String playerId);

  Future<void> refreshPlayer(String playerId) async {
    throw UnimplementedError('refreshPlayer() has not been implemented.');
  }

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

  Future<VesperPlatformDownloadCreateResult> createDownloadManager({
    VesperDownloadConfiguration configuration =
        const VesperDownloadConfiguration(),
  });

  Stream<VesperDownloadManagerEvent> downloadEventsFor(String downloadId);

  Future<void> refreshDownloadManager(String downloadId);

  Future<void> disposeDownloadManager(String downloadId);

  Future<int?> createDownloadTask(
    String downloadId, {
    required String assetId,
    required VesperDownloadSource source,
    VesperDownloadProfile profile = const VesperDownloadProfile(),
    VesperDownloadAssetIndex assetIndex = const VesperDownloadAssetIndex(),
  });

  Future<bool> startDownloadTask(String downloadId, int taskId);

  Future<bool> pauseDownloadTask(String downloadId, int taskId);

  Future<bool> resumeDownloadTask(String downloadId, int taskId);

  Future<bool> removeDownloadTask(String downloadId, int taskId);

  Future<void> exportDownloadTask(
    String downloadId,
    int taskId,
    String outputPath,
  );
}

final class _UnsupportedVesperPlayerPlatform extends VesperPlayerPlatform {
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
  Future<void> refreshPlayer(String playerId) async =>
      throw VesperUnsupportedError();

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
  ) async =>
      throw VesperUnsupportedError();

  @override
  Future<void> setAudioTrackSelection(
    String playerId,
    VesperTrackSelection selection,
  ) async =>
      throw VesperUnsupportedError();

  @override
  Future<void> setSubtitleTrackSelection(
    String playerId,
    VesperTrackSelection selection,
  ) async =>
      throw VesperUnsupportedError();

  @override
  Future<void> setAbrPolicy(String playerId, VesperAbrPolicy policy) async =>
      throw VesperUnsupportedError();

  @override
  Future<void> setResiliencePolicy(
    String playerId,
    VesperPlaybackResiliencePolicy policy,
  ) async =>
      throw VesperUnsupportedError();

  @override
  Future<void> updateViewport(
    String playerId,
    VesperPlayerViewport viewport,
  ) async =>
      throw VesperUnsupportedError();

  @override
  Future<void> clearViewport(String playerId) async =>
      throw VesperUnsupportedError();

  @override
  Future<VesperPlatformDownloadCreateResult> createDownloadManager({
    VesperDownloadConfiguration configuration =
        const VesperDownloadConfiguration(),
  }) async =>
      throw VesperUnsupportedError();

  @override
  Stream<VesperDownloadManagerEvent> downloadEventsFor(String downloadId) {
    return const Stream<VesperDownloadManagerEvent>.empty();
  }

  @override
  Future<void> refreshDownloadManager(String downloadId) async =>
      throw VesperUnsupportedError();

  @override
  Future<void> disposeDownloadManager(String downloadId) async =>
      throw VesperUnsupportedError();

  @override
  Future<int?> createDownloadTask(
    String downloadId, {
    required String assetId,
    required VesperDownloadSource source,
    VesperDownloadProfile profile = const VesperDownloadProfile(),
    VesperDownloadAssetIndex assetIndex = const VesperDownloadAssetIndex(),
  }) async =>
      throw VesperUnsupportedError();

  @override
  Future<bool> startDownloadTask(String downloadId, int taskId) async =>
      throw VesperUnsupportedError();

  @override
  Future<bool> pauseDownloadTask(String downloadId, int taskId) async =>
      throw VesperUnsupportedError();

  @override
  Future<bool> resumeDownloadTask(String downloadId, int taskId) async =>
      throw VesperUnsupportedError();

  @override
  Future<bool> removeDownloadTask(String downloadId, int taskId) async =>
      throw VesperUnsupportedError();

  @override
  Future<void> exportDownloadTask(
    String downloadId,
    int taskId,
    String outputPath,
  ) async =>
      throw VesperUnsupportedError();
}
