import 'events.dart';
import 'download_events.dart';
import 'download_models.dart';
import 'models.dart';
import 'sequence_models.dart';
import 'performance_diagnostics_exception.dart';
import 'package:plugin_platform_interface/plugin_platform_interface.dart';

final class VesperPlatformCreateResult {
  const VesperPlatformCreateResult({
    required this.playerId,
    required this.snapshot,
    this.pluginDiagnostics = const <VesperPluginDiagnostic>[],
  });

  factory VesperPlatformCreateResult.fromMap(Map<Object?, Object?> map) {
    final rawSnapshot = vesperDecodeMap(map['snapshot']);
    final rawPluginDiagnostics = map['pluginDiagnostics'];
    return VesperPlatformCreateResult(
      playerId: map['playerId'] as String? ?? '',
      snapshot: rawSnapshot.isNotEmpty
          ? VesperPlayerSnapshot.fromMap(rawSnapshot)
          : const VesperPlayerSnapshot.initial(),
      pluginDiagnostics: rawPluginDiagnostics is Iterable
          ? rawPluginDiagnostics
              .map((item) => vesperDecodeMap(item))
              .map(VesperPluginDiagnostic.fromMap)
              .toList(growable: false)
          : const <VesperPluginDiagnostic>[],
    );
  }

  final String playerId;
  final VesperPlayerSnapshot snapshot;
  final List<VesperPluginDiagnostic> pluginDiagnostics;
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
  VesperUnsupportedError([
    String? message,
    this.platformCode,
    this.platformDetails = const <String, Object?>{},
  ]) : super(message ?? 'Vesper player is not supported on this platform.');

  final String? platformCode;
  final Map<String, Object?> platformDetails;
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
  });

  Future<VesperPlaybackCapabilityProbeResult> probePlaybackCapability(
      VesperPlaybackCapabilityProbeRequest request,
      {String? playerId}) async {
    return const VesperPlaybackCapabilityProbeResult(
      status: VesperPlaybackCapabilityProbeStatus.unknown,
      codecFamily: VesperPlaybackCodecFamily.unknown,
      systemPlaybackSupported: false,
      hardwareDecodeSupported: false,
      sdkManagedNativeFrameSupported: false,
      recommendedPlaybackPath: VesperRecommendedPlaybackPath.systemPlayer,
      outputFormat: VesperPlaybackCapabilityOutputFormat.unknown,
      hdrKind: VesperPlaybackCapabilityHdrKind.none,
      dolbyVisionMode: VesperPlaybackCapabilityDolbyVisionMode.none,
      confidence: VesperPlaybackCapabilityConfidence.codecOnly,
      missingCapabilities: <String>['platformProbeNotImplemented'],
      diagnostics: <String, Object?>{
        'reason': 'platformProbeNotImplemented',
      },
    );
  }

  Stream<VesperPlayerEvent> eventsFor(String playerId);

  Future<void> initialize(String playerId);

  Future<void> dispose(String playerId);

  Future<String> startPerformanceDiagnostics(
    String playerId,
    VesperPerformanceDiagnosticsConfiguration configuration,
  ) async {
    throw const VesperPerformanceDiagnosticsException(
      code: 'artifactUnavailable',
      message: 'Performance diagnostics are unavailable on this platform.',
    );
  }

  Future<void> updatePerformanceOverlayState(
    String playerId,
    String runId,
    VesperPerformanceOverlayState state,
  ) async {
    throw const VesperPerformanceDiagnosticsException(
      code: 'controllerDisposed',
      message: 'No performance diagnostics session is active.',
    );
  }

  Future<void> recordPerformanceMarker(
    String playerId,
    String runId,
    String name, {
    double? value,
    int? sequenceIndex,
    bool? expectedOverlayActive,
  }) async {
    throw const VesperPerformanceDiagnosticsException(
      code: 'controllerDisposed',
      message: 'No performance diagnostics session is active.',
    );
  }

  Future<void> submitPerformanceFrameSamples(
    String playerId,
    String runId,
    List<VesperPerformanceFrameSample> samples,
  ) async {
    throw const VesperPerformanceDiagnosticsException(
      code: 'controllerDisposed',
      message: 'No performance diagnostics session is active.',
    );
  }

  Future<VesperPerformanceDiagnosticsReport> performanceDiagnosticsSnapshot(
    String playerId,
    String runId,
  ) async {
    throw const VesperPerformanceDiagnosticsException(
      code: 'controllerDisposed',
      message: 'No performance diagnostics session is active.',
    );
  }

  Future<VesperPerformanceDiagnosticsReport> stopPerformanceDiagnostics(
    String playerId,
    String runId,
  ) async {
    throw const VesperPerformanceDiagnosticsException(
      code: 'controllerDisposed',
      message: 'No performance diagnostics session is active.',
    );
  }

  Future<void> refreshPlayer(String playerId) async {
    throw UnimplementedError('refreshPlayer() has not been implemented.');
  }

  /// Samples the current playback timeline without requiring a full snapshot.
  ///
  /// Older or third-party platform implementations retain compatibility by
  /// performing a full refresh and returning `null`.
  Future<VesperTimeline?> sampleTimeline(String playerId) async {
    await refreshPlayer(playerId);
    return null;
  }

  Future<void> selectSource(String playerId, VesperPlayerSource source);

  /// Creates the bounded native sequence session attached to one controller.
  /// Platform implementations must keep source bytes and headers on the host.
  Future<VesperPlaybackSequenceSnapshot> createPlaybackSequence(
    String playerId,
    VesperPlaybackSequenceConfiguration configuration,
  ) async {
    throw VesperUnsupportedError(
      'Playback sequences are not supported by this platform.',
      'sequenceUnsupported',
    );
  }

  Stream<VesperPlaybackSequenceEvent> playbackSequenceEventsFor(
    String sequenceId,
  ) =>
      const Stream<VesperPlaybackSequenceEvent>.empty();

  Future<Map<String, Object?>> executePlaybackSequenceCommand(
    String sequenceId,
    Map<String, Object?> command,
  ) async {
    throw VesperUnsupportedError(
      'Playback sequences are not supported by this platform.',
      'sequenceUnsupported',
    );
  }

  Future<VesperPlaybackSequenceSnapshot> playbackSequenceSnapshot(
    String sequenceId,
  ) async {
    throw VesperUnsupportedError(
      'Playback sequences are not supported by this platform.',
      'sequenceUnsupported',
    );
  }

  Future<void> disposePlaybackSequence(String sequenceId) async {
    throw VesperUnsupportedError(
      'Playback sequences are not supported by this platform.',
      'sequenceUnsupported',
    );
  }

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

  /// Applies minimal subtitle styling (font scale, visibility).
  ///
  /// Platforms without subtitle styling must fail explicitly instead of
  /// silently accepting the request.
  Future<void> setSubtitleStyle(
    String playerId,
    VesperSubtitleStyle style,
  ) async =>
      throw VesperUnsupportedError('Subtitle styling is not supported.');

  Future<void> setAbrPolicy(
    String playerId,
    VesperAbrPolicy policy, {
    int? expectedCatalogRevision,
  });

  Future<void> setResiliencePolicy(
    String playerId,
    VesperPlaybackResiliencePolicy policy,
  );

  Future<void> setKeepScreenOnDuringPlayback(
    String playerId,
    bool enabled,
  ) async {
    throw UnimplementedError(
      'setKeepScreenOnDuringPlayback() has not been implemented.',
    );
  }

  Future<void> updateViewport(String playerId, VesperPlayerViewport viewport);

  Future<void> clearViewport(String playerId);

  Future<void> configureSystemPlayback(
    String playerId,
    VesperSystemPlaybackConfiguration configuration,
  ) async {
    throw UnimplementedError(
      'configureSystemPlayback() has not been implemented.',
    );
  }

  Future<void> updateSystemPlaybackMetadata(
    String playerId,
    VesperSystemPlaybackMetadata metadata,
  ) async {
    throw UnimplementedError(
      'updateSystemPlaybackMetadata() has not been implemented.',
    );
  }

  Future<void> clearSystemPlayback(String playerId) async {
    throw UnimplementedError('clearSystemPlayback() has not been implemented.');
  }

  Future<VesperPictureInPictureAvailability> isPictureInPictureAvailable(
    String playerId,
  ) async {
    return const VesperPictureInPictureAvailability(
      isAvailable: false,
      error: VesperPictureInPictureError(
        code: VesperPictureInPictureErrorCode.pictureInPictureNotSupported,
      ),
    );
  }

  Future<void> requestPictureInPicture(
    String playerId, {
    VesperPictureInPictureConfiguration? configuration,
  }) async {
    throw UnimplementedError(
      'requestPictureInPicture() has not been implemented.',
    );
  }

  Future<void> exitPictureInPicture(String playerId) async {
    throw UnimplementedError(
      'exitPictureInPicture() has not been implemented.',
    );
  }

  Future<void> setPictureInPictureConfiguration(
    String playerId,
    VesperPictureInPictureConfiguration configuration,
  ) async {
    throw UnimplementedError(
      'setPictureInPictureConfiguration() has not been implemented.',
    );
  }

  Future<VesperSystemPlaybackPermissionStatus>
      requestSystemPlaybackPermissions() async {
    return VesperSystemPlaybackPermissionStatus.notRequired;
  }

  Future<VesperSystemPlaybackPermissionStatus>
      getSystemPlaybackPermissionStatus() async {
    return VesperSystemPlaybackPermissionStatus.notRequired;
  }

  Future<VesperPlatformDownloadCreateResult> createDownloadManager({
    VesperDownloadConfiguration configuration =
        const VesperDownloadConfiguration(),
    VesperDownloadStaleResourcePlanRecoveryCallback? staleResourceRecovery,
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

  Future<void> shareDownloadTask(
    String downloadId,
    int taskId, {
    String? fileName,
    String? mimeType,
  });

  Future<String?> saveDownloadTask(
    String downloadId,
    int taskId, {
    String? fileName,
    VesperDownloadPublicCollection collection =
        VesperDownloadPublicCollection.downloads,
  });
}

final class _UnsupportedVesperPlayerPlatform extends VesperPlayerPlatform {
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
    throw VesperUnsupportedError();
  }

  @override
  Future<VesperPlaybackCapabilityProbeResult> probePlaybackCapability(
          VesperPlaybackCapabilityProbeRequest request,
          {String? playerId}) async =>
      throw VesperUnsupportedError();

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
  Future<VesperTimeline?> sampleTimeline(String playerId) async =>
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
  Future<void> setAbrPolicy(
    String playerId,
    VesperAbrPolicy policy, {
    int? expectedCatalogRevision,
  }) async =>
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
  Future<void> configureSystemPlayback(
    String playerId,
    VesperSystemPlaybackConfiguration configuration,
  ) async =>
      throw VesperUnsupportedError();

  @override
  Future<void> updateSystemPlaybackMetadata(
    String playerId,
    VesperSystemPlaybackMetadata metadata,
  ) async =>
      throw VesperUnsupportedError();

  @override
  Future<void> clearSystemPlayback(String playerId) async =>
      throw VesperUnsupportedError();

  @override
  Future<VesperSystemPlaybackPermissionStatus>
      requestSystemPlaybackPermissions() async =>
          throw VesperUnsupportedError();

  @override
  Future<VesperSystemPlaybackPermissionStatus>
      getSystemPlaybackPermissionStatus() async =>
          throw VesperUnsupportedError();

  @override
  Future<VesperPlatformDownloadCreateResult> createDownloadManager({
    VesperDownloadConfiguration configuration =
        const VesperDownloadConfiguration(),
    VesperDownloadStaleResourcePlanRecoveryCallback? staleResourceRecovery,
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

  @override
  Future<void> shareDownloadTask(
    String downloadId,
    int taskId, {
    String? fileName,
    String? mimeType,
  }) async =>
      throw VesperUnsupportedError();

  @override
  Future<String?> saveDownloadTask(
    String downloadId,
    int taskId, {
    String? fileName,
    VesperDownloadPublicCollection collection =
        VesperDownloadPublicCollection.downloads,
  }) async =>
      throw VesperUnsupportedError();
}
