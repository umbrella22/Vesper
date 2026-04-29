import 'dart:async';

import 'package:flutter/foundation.dart';
import 'package:vesper_player_platform_interface/vesper_player_platform_interface.dart';

const Duration _progressRefreshInterval = Duration(milliseconds: 250);

class VesperPlayerController {
  VesperPlayerController._({
    required this.playerId,
    required VesperPlayerSnapshot initialSnapshot,
    required VesperPlayerPlatform platform,
  }) : _platform = platform,
       snapshotListenable = ValueNotifier<VesperPlayerSnapshot>(
         initialSnapshot,
       ) {
    _snapshotsController.add(initialSnapshot);
    _bindPlatformEvents();
    _syncProgressRefreshTimer(initialSnapshot);
  }

  static Future<VesperPlayerController> create({
    VesperPlayerSource? initialSource,
    VesperPlaybackResiliencePolicy resiliencePolicy =
        const VesperPlaybackResiliencePolicy(),
    VesperTrackPreferencePolicy trackPreferencePolicy =
        const VesperTrackPreferencePolicy(),
    VesperPreloadBudgetPolicy preloadBudgetPolicy =
        const VesperPreloadBudgetPolicy(),
  }) async {
    final platform = VesperPlayerPlatform.instance;
    final result = await platform.createPlayer(
      initialSource: initialSource,
      resiliencePolicy: resiliencePolicy,
      trackPreferencePolicy: trackPreferencePolicy,
      preloadBudgetPolicy: preloadBudgetPolicy,
    );
    return VesperPlayerController._(
      playerId: result.playerId,
      initialSnapshot: result.snapshot,
      platform: platform,
    );
  }

  final String playerId;
  final VesperPlayerPlatform _platform;
  final ValueNotifier<VesperPlayerSnapshot> snapshotListenable;
  final StreamController<VesperPlayerEvent> _eventsController =
      StreamController<VesperPlayerEvent>.broadcast();
  final StreamController<VesperPlayerSnapshot> _snapshotsController =
      StreamController<VesperPlayerSnapshot>.broadcast();

  StreamSubscription<VesperPlayerEvent>? _platformSubscription;
  Timer? _progressRefreshTimer;
  bool _refreshInFlight = false;
  bool _disposed = false;

  VesperPlayerSnapshot get snapshot => snapshotListenable.value;

  VesperPlayerCapabilities get capabilities => snapshot.capabilities;

  Stream<VesperPlayerEvent> get events => _eventsController.stream;

  Stream<VesperPlayerSnapshot> get snapshots => _snapshotsController.stream;

  Future<void> initialize() =>
      _runVoidOperation(() => _platform.initialize(playerId));

  Future<void> dispose() async {
    if (_disposed) {
      return;
    }
    _disposed = true;
    _progressRefreshTimer?.cancel();
    _progressRefreshTimer = null;

    Object? platformError;
    StackTrace? platformStackTrace;

    try {
      await _platform.dispose(playerId);
    } catch (error, stackTrace) {
      platformError = error;
      platformStackTrace = stackTrace;
    } finally {
      await _platformSubscription?.cancel();
      _eventsController.add(VesperPlayerDisposedEvent(playerId: playerId));
      await _eventsController.close();
      await _snapshotsController.close();
      snapshotListenable.dispose();
    }

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

  Future<void> setAbrPolicy(VesperAbrPolicy policy) =>
      _runVoidOperation(() => _platform.setAbrPolicy(playerId, policy));

  Future<void> setPlaybackResiliencePolicy(
    VesperPlaybackResiliencePolicy policy,
  ) => _runVoidOperation(() => _platform.setResiliencePolicy(playerId, policy));

  Future<void> setResiliencePolicy(VesperPlaybackResiliencePolicy policy) =>
      setPlaybackResiliencePolicy(policy);

  Future<void> updateViewport(VesperPlayerViewport viewport) =>
      _runVoidOperation(() => _platform.updateViewport(playerId, viewport));

  Future<void> clearViewport() =>
      _runVoidOperation(() => _platform.clearViewport(playerId));

  void _bindPlatformEvents() {
    _platformSubscription = _platform
        .eventsFor(playerId)
        .listen(
          (event) {
            switch (event) {
              case VesperPlayerSnapshotEvent():
                _applySnapshot(event.snapshot);
              case VesperPlayerErrorEvent():
                _applyPlatformError(event);
              case VesperPlayerDisposedEvent():
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
    final snapshot =
        event.snapshot ?? this.snapshot.copyWith(lastError: event.error);
    snapshotListenable.value = snapshot;
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

  void _syncProgressRefreshTimer([VesperPlayerSnapshot? nextSnapshot]) {
    final effectiveSnapshot = nextSnapshot ?? snapshot;
    if (_disposed || !_shouldRefreshProgress(effectiveSnapshot)) {
      _progressRefreshTimer?.cancel();
      _progressRefreshTimer = null;
      return;
    }

    if (_progressRefreshTimer != null) {
      return;
    }

    _progressRefreshTimer = Timer.periodic(_progressRefreshInterval, (_) {
      unawaited(_refreshTimelineTick());
    });
  }

  Future<void> _refreshTimelineTick() async {
    if (_disposed || _refreshInFlight || !_shouldRefreshProgress(snapshot)) {
      _syncProgressRefreshTimer();
      return;
    }

    _refreshInFlight = true;
    var refreshFailed = false;
    try {
      await _platform.refreshPlayer(playerId);
    } catch (error, stackTrace) {
      refreshFailed = true;
      _publishSyntheticError(error, stackTrace);
      _progressRefreshTimer?.cancel();
      _progressRefreshTimer = null;
    } finally {
      _refreshInFlight = false;
      if (!refreshFailed) {
        _syncProgressRefreshTimer();
      }
    }
  }

  Future<void> _runVoidOperation(Future<void> Function() operation) async {
    _ensureActive();
    try {
      await operation();
    } catch (error, stackTrace) {
      _publishSyntheticError(error, stackTrace);
      rethrow;
    }
  }

  void _publishSyntheticError(Object error, StackTrace stackTrace) {
    if (_disposed || _eventsController.isClosed) {
      return;
    }

    final vesperError = error is VesperUnsupportedError
        ? VesperPlayerError.unsupported(error.message?.toString())
        : VesperPlayerError(
            message: error.toString(),
            category: VesperPlayerErrorCategory.platform,
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

  void _ensureActive() {
    if (_disposed) {
      throw StateError('VesperPlayerController has already been disposed.');
    }
  }
}

bool _shouldRefreshProgress(VesperPlayerSnapshot snapshot) {
  return snapshot.playbackState == VesperPlaybackState.playing ||
      snapshot.isBuffering;
}
