import 'dart:async';

import 'package:vesper_player_platform_interface/vesper_player_platform_interface.dart';

/// Owns accepted sources and bounded preloading independently of a player.
final class VesperSourceSession {
  VesperSourceSession._(this.sessionId, this.configuration, this._platform);

  static Future<VesperSourceSession> create({
    VesperSourceSessionConfiguration configuration =
        const VesperSourceSessionConfiguration(),
  }) async {
    configuration.toMap();
    final platform = VesperPlayerPlatform.instance;
    final id = await platform.createSourceSession(configuration);
    return VesperSourceSession._(id, configuration, platform);
  }

  final String sessionId;
  final VesperSourceSessionConfiguration configuration;
  final VesperPlayerPlatform _platform;
  final Set<VesperSourceHandle> _handles = <VesperSourceHandle>{};
  int _pendingRegistrations = 0;
  bool _closed = false;
  bool _invalidated = false;
  Future<void>? _closeFuture;

  bool get isClosed => _closed;

  /// Registers a new immutable source identity, including when its URL repeats.
  Future<VesperSourceHandle> register(
    VesperPlayerSource source, {
    int? expiresAtEpochMs,
  }) async {
    _ensureOpen();
    if (expiresAtEpochMs != null && expiresAtEpochMs < 0) {
      throw ArgumentError.value(expiresAtEpochMs, 'expiresAtEpochMs');
    }
    if (_handles.length + _pendingRegistrations >= configuration.maxSources) {
      throw StateError('Source session registration limit reached.');
    }
    _pendingRegistrations++;
    try {
      final reference = await _platform.registerSource(sessionId, source,
          expiresAtEpochMs: expiresAtEpochMs);
      if (reference.sessionId != sessionId) {
        throw const FormatException('Registration returned a foreign session.');
      }
      if (_closed) {
        await _platform.releaseSource(reference);
        throw StateError('Source session closed while registering a source.');
      }
      final handle = VesperSourceHandle._(this, reference);
      _handles.add(handle);
      return handle;
    } finally {
      _pendingRegistrations--;
    }
  }

  /// Revokes access held by playback leases and closes this session.
  /// Call this instead of [dispose] when the access context is being revoked.
  Future<void> invalidate() {
    if (_invalidated) return _closeFuture!;
    _ensureOpen();
    _invalidated = true;
    return _close(invalidate: true);
  }

  /// Releases registrations and cancels preloads. Acquired playback leases live
  /// until their player or sequence releases them.
  Future<void> dispose() => _closeFuture ?? _close(invalidate: false);

  Future<void> _close({required bool invalidate}) {
    _closed = true;
    for (final handle in _handles) {
      handle._closed = true;
    }
    _handles.clear();
    return _closeFuture = invalidate
        ? _platform.invalidateSourceSession(sessionId)
        : _platform.disposeSourceSession(sessionId);
  }

  void _ensureOpen() {
    if (_closed) throw StateError('VesperSourceSession has been closed.');
  }
}

/// An opaque reference to one accepted source and its native cache scope.
final class VesperSourceHandle extends VesperSourceReference {
  VesperSourceHandle._(this._session, VesperSourceReference reference)
      : super(
          sessionId: reference.sessionId,
          sourceId: reference.sourceId,
          expiresAtEpochMs: reference.expiresAtEpochMs,
        );

  final VesperSourceSession _session;
  bool _closed = false;
  Future<void>? _closeFuture;
  Future<VesperSourcePreloadTask>? _startingPreload;
  VesperSourcePreloadTask? _preload;

  bool get isClosed => _closed || _session.isClosed;

  /// Active requests for this handle share a task. The first options apply;
  /// cancellation by any caller cancels the shared task.
  Future<VesperSourcePreloadTask> preload({
    VesperSourcePreloadOptions options = const VesperSourcePreloadOptions(),
  }) {
    ensureAvailable();
    options.toMap();
    final starting = _startingPreload;
    if (starting != null) return starting;
    final current = _preload;
    if (current != null && !current.snapshot.isTerminal) {
      return Future<VesperSourcePreloadTask>.value(current);
    }
    return _startingPreload = _startPreload(options);
  }

  Future<VesperSourcePreloadTask> _startPreload(
      VesperSourcePreloadOptions options) async {
    try {
      final initial = await _session._platform.preloadSource(this, options);
      if (initial.source != this) {
        throw const FormatException('Preload returned a foreign source.');
      }
      if (isClosed) {
        await _session._platform.cancelSourcePreload(sessionId, initial.taskId);
        throw StateError('Source closed while starting a preload.');
      }
      final task = VesperSourcePreloadTask._(_session._platform, initial);
      _preload = task;
      return task;
    } finally {
      _startingPreload = null;
    }
  }

  /// Prevents new acquisitions. Existing playback and sequence leases survive.
  Future<void> dispose() {
    if (_closeFuture != null) return _closeFuture!;
    if (isClosed) return Future<void>.value();
    _closed = true;
    _session._handles.remove(this);
    return _closeFuture = _session._platform.releaseSource(this);
  }

  /// Validates local lifetime before a native acquisition.
  void ensureAvailable() {
    if (isClosed) throw StateError('VesperSourceHandle has been closed.');
  }
}

/// A retained observation and completion of one independent native preload.
final class VesperSourcePreloadTask {
  VesperSourcePreloadTask._(this._platform, this._snapshot) {
    completion = _snapshot.isTerminal
        ? Future<VesperSourcePreloadSnapshot>.value(_snapshot)
        : _wait();
    // A caller may use only the retained snapshot. Keep transport errors on the
    // completion future without producing an unhandled asynchronous error.
    unawaited(
        completion.then<void>((_) {}, onError: (Object _, StackTrace __) {}));
  }

  final VesperPlayerPlatform _platform;
  VesperSourcePreloadSnapshot _snapshot;
  late final Future<VesperSourcePreloadSnapshot> completion;
  VesperSourcePreloadSnapshot get snapshot => _snapshot;
  String get taskId => _snapshot.taskId;

  Future<VesperSourcePreloadSnapshot> _wait() async {
    try {
      return _accept(await _platform.awaitSourcePreload(
          _snapshot.source.sessionId, taskId));
    } catch (_) {
      _accept(VesperSourcePreloadSnapshot(
        taskId: taskId,
        source: _snapshot.source,
        rawStatus: 'failed',
        rawGoal: _snapshot.rawGoal,
        rawReuse: _snapshot.rawReuse,
        actualBytes: _snapshot.actualBytes,
        reasonCode: 'platform_error',
      ));
      rethrow;
    }
  }

  /// Refreshes once; the SDK does not create a polling timer.
  Future<VesperSourcePreloadSnapshot> refresh() async {
    if (_snapshot.isTerminal) return _snapshot;
    return _accept(await _platform.sourcePreloadSnapshot(
        _snapshot.source.sessionId, taskId));
  }

  Future<void> cancel() async {
    if (_snapshot.isTerminal) return;
    await _platform.cancelSourcePreload(_snapshot.source.sessionId, taskId);
  }

  VesperSourcePreloadSnapshot _accept(VesperSourcePreloadSnapshot value) {
    if (value.taskId != taskId || value.source != _snapshot.source) {
      throw const FormatException('Preload observation changed task identity.');
    }
    if (!_snapshot.isTerminal) _snapshot = value;
    return _snapshot;
  }
}
