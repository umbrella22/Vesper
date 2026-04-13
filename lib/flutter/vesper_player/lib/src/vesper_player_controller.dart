import 'dart:async';

import 'package:flutter/foundation.dart';
import 'package:vesper_player_platform_interface/vesper_player_platform_interface.dart';

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
  }

  static Future<VesperPlayerController> create({
    VesperPlayerSource? initialSource,
    VesperPlaybackResiliencePolicy resiliencePolicy =
        const VesperPlaybackResiliencePolicy(),
  }) async {
    final platform = VesperPlayerPlatform.instance;
    final result = await platform.createPlayer(
      initialSource: initialSource,
      resiliencePolicy: resiliencePolicy,
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
