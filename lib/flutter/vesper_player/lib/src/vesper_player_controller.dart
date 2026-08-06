import 'dart:async';

import 'package:flutter/foundation.dart';
import 'package:vesper_player_platform_interface/vesper_player_platform_interface.dart';

const Duration _progressRefreshInterval = Duration(seconds: 1);
const Duration _maxProgressRefreshBackoff = Duration(seconds: 8);

class VesperPlayerController {
  VesperPlayerController._({
    required this.playerId,
    required VesperPlayerSnapshot initialSnapshot,
    required List<VesperPluginDiagnostic> pluginDiagnostics,
    required VesperPlayerPlatform platform,
  })  : _platform = platform,
        _pluginDiagnostics = List.unmodifiable(pluginDiagnostics),
        snapshotListenable = ValueNotifier<VesperPlayerSnapshot>(
          initialSnapshot,
        ) {
    _snapshotsController.add(initialSnapshot);
    _bindPlatformEvents();
    _syncProgressRefreshTimer(initialSnapshot);
  }

  static Future<VesperPlayerController> create({
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
    final platform = VesperPlayerPlatform.instance;
    final result = await platform.createPlayer(
      initialSource: initialSource,
      renderSurfaceKind: renderSurfaceKind,
      resiliencePolicy: resiliencePolicy,
      trackPreferencePolicy: trackPreferencePolicy,
      preloadBudgetPolicy: preloadBudgetPolicy,
      keepScreenOnDuringPlayback: keepScreenOnDuringPlayback,
      benchmarkConfiguration: benchmarkConfiguration,
      sourceNormalizerConfiguration: sourceNormalizerConfiguration,
      frameProcessorConfiguration: frameProcessorConfiguration,
      nativeFramePipelineConfiguration: nativeFramePipelineConfiguration,
      pipelineEventHookConfiguration: pipelineEventHookConfiguration,
    );
    return VesperPlayerController._(
      playerId: result.playerId,
      initialSnapshot: result.snapshot,
      pluginDiagnostics: result.pluginDiagnostics,
      platform: platform,
    );
  }

  static Future<VesperPlaybackCapabilityProbeResult> probePlaybackCapability(
    VesperPlaybackCapabilityProbeRequest request,
  ) {
    return VesperPlayerPlatform.instance.probePlaybackCapability(request);
  }

  Future<VesperPlaybackCapabilityProbeResult> probeAssociatedPlaybackCapability(
    VesperPlaybackCapabilityProbeRequest request,
  ) {
    _ensureActive();
    return _platform.probePlaybackCapability(request, playerId: playerId);
  }

  final String playerId;
  final VesperPlayerPlatform _platform;
  List<VesperPluginDiagnostic> _pluginDiagnostics;
  final ValueNotifier<VesperPlayerSnapshot> snapshotListenable;
  final StreamController<VesperPlayerEvent> _eventsController =
      StreamController<VesperPlayerEvent>.broadcast();
  final StreamController<VesperPlayerSnapshot> _snapshotsController =
      StreamController<VesperPlayerSnapshot>.broadcast();
  final StreamController<VesperPlayerPictureInPictureEvent>
      _pictureInPictureController =
      StreamController<VesperPlayerPictureInPictureEvent>.broadcast();

  StreamSubscription<VesperPlayerEvent>? _platformSubscription;
  Timer? _progressRefreshTimer;
  bool _refreshInFlight = false;
  bool _disposed = false;
  int _timelineRevision = 0;
  Duration _progressRefreshDelay = _progressRefreshInterval;

  VesperPlayerSnapshot get snapshot => snapshotListenable.value;

  List<VesperPluginDiagnostic> get pluginDiagnostics => _pluginDiagnostics;

  VesperPlayerCapabilities get capabilities => snapshot.capabilities;

  Stream<VesperPlayerEvent> get events => _eventsController.stream;

  Stream<VesperPlayerSnapshot> get snapshots => _snapshotsController.stream;

  Stream<VesperPlayerPictureInPictureEvent> get pictureInPictureEvents =>
      _pictureInPictureController.stream;

  Future<void> initialize() =>
      _runVoidOperation(() => _platform.initialize(playerId));

  Future<void> dispose() async {
    if (_disposed) {
      return;
    }
    _disposed = true;
    _advanceTimelineRevision();
    _progressRefreshTimer?.cancel();
    _progressRefreshTimer = null;

    Object? platformError;
    StackTrace? platformStackTrace;

    try {
      await _platform.dispose(playerId);
    } catch (error, stackTrace) {
      platformError = error;
      platformStackTrace = stackTrace;
    }

    await _guardDisposeCleanup(
      () => _platformSubscription?.cancel(),
      context: 'cancel player event subscription',
    );
    _eventsController.add(VesperPlayerDisposedEvent(playerId: playerId));
    await _guardDisposeCleanup(
      _eventsController.close,
      context: 'close player event stream',
    );
    await _guardDisposeCleanup(
      _snapshotsController.close,
      context: 'close player snapshot stream',
    );
    await _guardDisposeCleanup(
      _pictureInPictureController.close,
      context: 'close player picture-in-picture stream',
    );
    _guardDisposeSyncCleanup(
      snapshotListenable.dispose,
      context: 'dispose player snapshot listenable',
    );

    if (platformError != null) {
      Error.throwWithStackTrace(platformError, platformStackTrace!);
    }
  }

  Future<void> selectSource(VesperPlayerSource source) =>
      _runVoidOperation(() => _platform.selectSource(playerId, source));

  Future<void> refresh() =>
      _runVoidOperation(() => _platform.refreshPlayer(playerId));

  Future<void> play() => _runVoidOperation(() => _platform.play(playerId));

  Future<void> pause() => _runVoidOperation(() => _platform.pause(playerId));

  Future<void> togglePause() =>
      _runVoidOperation(() => _platform.togglePause(playerId));

  Future<void> stop() => _runVoidOperation(() => _platform.stop(playerId));

  Future<void> seekBy(int deltaMs) =>
      _runVoidOperation(() => _platform.seekBy(playerId, deltaMs));

  Future<void> seekToRatio(double ratio) =>
      _runVoidOperation(() => _platform.seekToRatio(playerId, ratio));

  Future<void> seekToLiveEdge() =>
      _runVoidOperation(() => _platform.seekToLiveEdge(playerId));

  Future<void> setPlaybackRate(double rate) =>
      _runVoidOperation(() => _platform.setPlaybackRate(playerId, rate));

  Future<void> setVideoTrackSelection(VesperTrackSelection selection) =>
      _runVoidOperation(
        () => _platform.setVideoTrackSelection(playerId, selection),
      );

  Future<void> setAudioTrackSelection(VesperTrackSelection selection) =>
      _runVoidOperation(
        () => _platform.setAudioTrackSelection(playerId, selection),
      );

  Future<void> setSubtitleTrackSelection(VesperTrackSelection selection) =>
      _runVoidOperation(
        () => _platform.setSubtitleTrackSelection(playerId, selection),
      );

  /// Applies minimal subtitle styling (font scale, visibility).
  ///
  /// Platforms without subtitle styling fail explicitly instead of accepting
  /// the request as a no-op.
  Future<void> setSubtitleStyle(VesperSubtitleStyle style) => _runVoidOperation(
        () => _platform.setSubtitleStyle(playerId, style),
      );

  Future<void> setAbrPolicy(VesperAbrPolicy policy) =>
      _runVoidOperation(() => _platform.setAbrPolicy(playerId, policy));

  Future<void> setPlaybackResiliencePolicy(
    VesperPlaybackResiliencePolicy policy,
  ) =>
      _runVoidOperation(() => _platform.setResiliencePolicy(playerId, policy));

  Future<void> setResiliencePolicy(VesperPlaybackResiliencePolicy policy) =>
      setPlaybackResiliencePolicy(policy);

  Future<void> setKeepScreenOnDuringPlayback(bool enabled) => _runVoidOperation(
        () => _platform.setKeepScreenOnDuringPlayback(playerId, enabled),
      );

  Future<void> updateViewport(VesperPlayerViewport viewport) =>
      _runVoidOperation(() => _platform.updateViewport(playerId, viewport));

  Future<void> clearViewport() =>
      _runVoidOperation(() => _platform.clearViewport(playerId));

  Future<void> configureSystemPlayback(
    VesperSystemPlaybackConfiguration configuration,
  ) =>
      _runVoidOperation(
        () => _platform.configureSystemPlayback(playerId, configuration),
      );

  Future<void> updateSystemPlaybackMetadata(
    VesperSystemPlaybackMetadata metadata,
  ) =>
      _runVoidOperation(
        () => _platform.updateSystemPlaybackMetadata(playerId, metadata),
      );

  Future<void> clearSystemPlayback() =>
      _runVoidOperation(() => _platform.clearSystemPlayback(playerId));

  Future<VesperPictureInPictureAvailability>
      isPictureInPictureAvailable() async {
    _ensureActive();
    return _platform.isPictureInPictureAvailable(playerId);
  }

  Future<void> requestPictureInPicture({
    VesperPictureInPictureConfiguration? configuration,
  }) =>
      _runPictureInPictureOperation(
        () => _platform.requestPictureInPicture(
          playerId,
          configuration: configuration,
        ),
      );

  Future<void> exitPictureInPicture() => _runPictureInPictureOperation(
        () => _platform.exitPictureInPicture(playerId),
      );

  Future<void> setPictureInPictureConfiguration(
    VesperPictureInPictureConfiguration configuration,
  ) =>
      _runVoidOperation(
        () => _platform.setPictureInPictureConfiguration(
          playerId,
          configuration,
        ),
      );

  Future<VesperSystemPlaybackPermissionStatus>
      requestSystemPlaybackPermissions() async {
    _ensureActive();
    try {
      return await _platform.requestSystemPlaybackPermissions();
    } catch (error, stackTrace) {
      _publishSyntheticError(error, stackTrace);
      rethrow;
    }
  }

  Future<VesperSystemPlaybackPermissionStatus>
      getSystemPlaybackPermissionStatus() async {
    _ensureActive();
    try {
      return await _platform.getSystemPlaybackPermissionStatus();
    } catch (error, stackTrace) {
      _publishSyntheticError(error, stackTrace);
      rethrow;
    }
  }

  void _bindPlatformEvents() {
    _platformSubscription = _platform.eventsFor(playerId).listen(
      (event) {
        switch (event) {
          case VesperPlayerSnapshotEvent():
            _applySnapshot(event.snapshot);
          case VesperPlayerErrorEvent():
            _applyPlatformError(event);
          case VesperPlayerWarningEvent():
            _eventsController.add(event);
          case VesperPlayerPictureInPictureEvent():
            _pictureInPictureController.add(event);
            _eventsController.add(event);
          case VesperPlayerDisposedEvent():
            _eventsController.add(event);
          case VesperPlayerUnknownEvent():
            _eventsController.add(event);
        }
      },
      onError: (Object error, StackTrace stackTrace) {
        _publishSyntheticError(error, stackTrace);
      },
    );
  }

  void _applySnapshot(VesperPlayerSnapshot snapshot) {
    if (_disposed) {
      return;
    }
    _advanceTimelineRevision();
    _resetProgressRefreshBackoff();
    _syncPluginDiagnostics(snapshot.pluginDiagnostics);
    snapshotListenable.value = snapshot;
    _snapshotsController.add(snapshot);
    _eventsController.add(
      VesperPlayerSnapshotEvent(playerId: playerId, snapshot: snapshot),
    );
    _syncProgressRefreshTimer(snapshot);
  }

  void _applyPlatformError(VesperPlayerErrorEvent event) {
    if (_disposed) {
      return;
    }
    _advanceTimelineRevision();
    _resetProgressRefreshBackoff();
    final snapshot =
        (event.snapshot ?? this.snapshot).copyWith(lastError: event.error);
    snapshotListenable.value = snapshot;
    _syncPluginDiagnostics(snapshot.pluginDiagnostics);
    _snapshotsController.add(snapshot);
    _eventsController.add(
      VesperPlayerErrorEvent(
        playerId: playerId,
        error: event.error,
        snapshot: snapshot,
      ),
    );
    _syncProgressRefreshTimer(snapshot);
  }

  void _syncPluginDiagnostics(List<VesperPluginDiagnostic> diagnostics) {
    _pluginDiagnostics = List.unmodifiable(diagnostics);
  }

  void _syncProgressRefreshTimer([VesperPlayerSnapshot? nextSnapshot]) {
    final effectiveSnapshot = nextSnapshot ?? snapshot;
    if (_disposed || !_shouldRefreshProgress(effectiveSnapshot)) {
      _progressRefreshTimer?.cancel();
      _progressRefreshTimer = null;
      _resetProgressRefreshBackoff();
      return;
    }

    // The in-flight request owns the next scheduling decision. An
    // authoritative snapshot may arrive while it is pending; do not create a
    // second timer that will only churn against the in-flight guard.
    if (_refreshInFlight || _progressRefreshTimer != null) {
      return;
    }

    _progressRefreshTimer = Timer(_progressRefreshDelay, () {
      _progressRefreshTimer = null;
      unawaited(_refreshTimelineTick());
    });
  }

  Future<void> _refreshTimelineTick() async {
    if (_disposed || _refreshInFlight || !_shouldRefreshProgress(snapshot)) {
      _syncProgressRefreshTimer();
      return;
    }

    _refreshInFlight = true;
    final revision = _timelineRevision;
    try {
      final sampledTimeline = await _platform.sampleTimeline(playerId);
      if (!_disposed && revision == _timelineRevision) {
        _resetProgressRefreshBackoff();
        if (sampledTimeline != null) {
          _publishTimelineSample(sampledTimeline);
        }
      }
    } catch (_) {
      if (!_disposed && revision == _timelineRevision) {
        _increaseProgressRefreshBackoff();
      }
    } finally {
      _refreshInFlight = false;
      _syncProgressRefreshTimer();
    }
  }

  void _publishTimelineSample(VesperTimeline timeline) {
    if (_disposed) {
      return;
    }
    final patchedSnapshot = snapshot.copyWith(timeline: timeline);
    snapshotListenable.value = patchedSnapshot;
    _snapshotsController.add(patchedSnapshot);
    _eventsController.add(
      VesperPlayerSnapshotEvent(
        playerId: playerId,
        snapshot: patchedSnapshot,
      ),
    );
  }

  void _advanceTimelineRevision() {
    _timelineRevision += 1;
  }

  void _resetProgressRefreshBackoff() {
    _progressRefreshDelay = _progressRefreshInterval;
  }

  void _increaseProgressRefreshBackoff() {
    final nextMilliseconds = _progressRefreshDelay.inMilliseconds * 2;
    _progressRefreshDelay = Duration(
      milliseconds: nextMilliseconds > _maxProgressRefreshBackoff.inMilliseconds
          ? _maxProgressRefreshBackoff.inMilliseconds
          : nextMilliseconds,
    );
  }

  Future<void> _runVoidOperation(Future<void> Function() operation) async {
    _ensureActive();
    _advanceTimelineRevision();
    try {
      await operation();
    } catch (error, stackTrace) {
      if (!_isObsoleteSubtitleTransactionError(error)) {
        _publishSyntheticError(error, stackTrace);
      }
      rethrow;
    }
  }

  bool _isObsoleteSubtitleTransactionError(Object error) {
    if (error is! VesperSubtitleException) {
      return false;
    }
    return switch (error.code) {
      'subtitle_selection_cancelled' ||
      'subtitle_source_changed' ||
      'subtitle_selection_superseded' =>
        true,
      _ => false,
    };
  }

  Future<void> _runPictureInPictureOperation(
    Future<void> Function() operation,
  ) async {
    _ensureActive();
    await operation();
  }

  void _publishSyntheticError(Object error, StackTrace stackTrace) {
    if (_disposed || _eventsController.isClosed) {
      return;
    }

    _advanceTimelineRevision();
    _resetProgressRefreshBackoff();

    final vesperError = error is VesperSubtitleException
        ? VesperPlayerError(
            message: error.message,
            code: VesperPlayerErrorCode.invalidState,
            category: VesperPlayerErrorCategory.capability,
            retriable: error.retriable,
            details: <String, Object?>{
              'domain': 'subtitle',
              'code': error.code,
              'phase': error.phase.name,
              'phaseRawValue': error.phaseRawValue,
              'trackId': error.trackId,
              'commandId': error.commandId,
              'sourceEpoch': error.sourceEpoch,
              'retriable': error.retriable,
            },
          )
        : error is VesperUnsupportedError
            ? VesperPlayerError(
                message: error.message?.toString() ??
                    'Vesper player is not supported on this platform.',
                code: VesperPlayerErrorCode.unsupported,
                category: VesperPlayerErrorCategory.capability,
                retriable: false,
                details: _unsupportedErrorDetails(error),
              )
            : VesperPlayerError(
                message: error.toString(),
                code: VesperPlayerErrorCode.backendFailure,
                category: VesperPlayerErrorCategory.platform,
                retriable: false,
              );

    final snapshot = this.snapshot.copyWith(lastError: vesperError);
    snapshotListenable.value = snapshot;
    _snapshotsController.add(snapshot);
    _eventsController.add(
      VesperPlayerErrorEvent(
        playerId: playerId,
        error: vesperError,
        snapshot: snapshot,
      ),
    );
    _syncProgressRefreshTimer(snapshot);
    FlutterError.reportError(
      FlutterErrorDetails(
        exception: error,
        stack: stackTrace,
        library: 'vesper_player',
        context: ErrorDescription('while forwarding a platform operation'),
      ),
    );
  }

  Map<String, Object?> _unsupportedErrorDetails(VesperUnsupportedError error) {
    return <String, Object?>{
      if (error.platformCode != null) 'platformCode': error.platformCode,
      ...error.platformDetails,
    };
  }

  Future<void> _guardDisposeCleanup(
    FutureOr<void> Function() cleanup, {
    required String context,
  }) async {
    try {
      await cleanup();
    } catch (error, stackTrace) {
      FlutterError.reportError(
        FlutterErrorDetails(
          exception: error,
          stack: stackTrace,
          library: 'vesper_player',
          context: ErrorDescription(context),
        ),
      );
    }
  }

  void _guardDisposeSyncCleanup(
    VoidCallback cleanup, {
    required String context,
  }) {
    try {
      cleanup();
    } catch (error, stackTrace) {
      FlutterError.reportError(
        FlutterErrorDetails(
          exception: error,
          stack: stackTrace,
          library: 'vesper_player',
          context: ErrorDescription(context),
        ),
      );
    }
  }

  void _ensureActive() {
    if (_disposed) {
      throw StateError('VesperPlayerController has already been disposed.');
    }
  }
}

bool _shouldRefreshProgress(VesperPlayerSnapshot snapshot) {
  if (snapshot.playbackState == VesperPlaybackState.paused ||
      snapshot.playbackState == VesperPlaybackState.finished) {
    return false;
  }
  return snapshot.playbackState == VesperPlaybackState.playing ||
      snapshot.isBuffering;
}
