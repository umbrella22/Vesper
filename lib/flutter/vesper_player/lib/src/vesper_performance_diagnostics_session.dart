import 'dart:async';
import 'dart:math' as math;
import 'dart:ui';

import 'package:flutter/scheduler.dart';
import 'package:vesper_player_platform_interface/vesper_player_platform_interface.dart';

import 'vesper_player_controller.dart';

const int _maximumFrameBatchSize = 120;
const int _maximumQueuedFrameBatches = 4;
const Duration _maximumFrameBatchDelay = Duration(milliseconds: 500);

final class VesperPerformanceDiagnosticsSession {
  VesperPerformanceDiagnosticsSession._({
    required VesperPlayerController controller,
    required this.runId,
  })  : _platform = controller.platformForSequence,
        _playerId = controller.playerId {
    SchedulerBinding.instance.addTimingsCallback(_onFrameTimings);
  }

  static Future<VesperPerformanceDiagnosticsSession> start(
    VesperPlayerController controller, {
    VesperPerformanceDiagnosticsConfiguration configuration =
        const VesperPerformanceDiagnosticsConfiguration(),
  }) async {
    if (configuration.maxRawEvents < 0 || configuration.maxRawEvents > 2048) {
      throw const VesperPerformanceDiagnosticsException(
        code: 'invalidConfiguration',
        message: 'maxRawEvents must be between 0 and 2048.',
      );
    }
    final runId = await controller.platformForSequence
        .startPerformanceDiagnostics(controller.playerId, configuration);
    try {
      return VesperPerformanceDiagnosticsSession._(
        controller: controller,
        runId: runId,
      );
    } catch (_) {
      await controller.platformForSequence.stopPerformanceDiagnostics(
        controller.playerId,
        runId,
      );
      rethrow;
    }
  }

  final VesperPlayerPlatform _platform;
  final String _playerId;
  final String runId;
  final List<VesperPerformanceFrameSample> _pendingSamples =
      <VesperPerformanceFrameSample>[];
  VesperPerformanceOverlayState _overlayState =
      const VesperPerformanceOverlayState(active: false);
  Timer? _batchTimer;
  Future<void> _flushTail = Future<void>.value();
  int _queuedFrameBatches = 0;
  int _locallyDroppedSamples = 0;
  Object? _firstFlushError;
  StackTrace? _firstFlushStackTrace;
  Future<VesperPerformanceDiagnosticsReport>? _finalReportFuture;
  bool _stopping = false;

  bool get isStopped => _finalReportFuture != null;

  Future<void> updateOverlayState(VesperPerformanceOverlayState state) async {
    _ensureActive();
    _validateOverlayState(state);
    _overlayState = state;
    await _platform.updatePerformanceOverlayState(_playerId, runId, state);
  }

  Future<void> recordMarker(
    String name, {
    double? value,
    int? sequenceIndex,
    bool? expectedOverlayActive,
  }) {
    _ensureActive();
    if (!_isValidMarkerName(name) || (value != null && !value.isFinite)) {
      throw const VesperPerformanceDiagnosticsException(
        code: 'protocolViolation',
        message: 'The performance marker does not satisfy the wire contract.',
      );
    }
    return _platform.recordPerformanceMarker(
      _playerId,
      runId,
      name,
      value: value,
      sequenceIndex: sequenceIndex,
      expectedOverlayActive: expectedOverlayActive,
    );
  }

  Future<VesperPerformanceDiagnosticsReport> snapshot() async {
    _ensureActive();
    _flushPendingSamples();
    await _flushTail;
    _throwPendingFlushError();
    final report =
        await _platform.performanceDiagnosticsSnapshot(_playerId, runId);
    return _withLocalDrops(report);
  }

  Future<VesperPerformanceDiagnosticsReport> stop() {
    final existing = _finalReportFuture;
    if (existing != null) return existing;
    final completion = _stopOnce();
    _finalReportFuture = completion;
    return completion;
  }

  Future<VesperPerformanceDiagnosticsReport> _stopOnce() async {
    _stopping = true;
    SchedulerBinding.instance.removeTimingsCallback(_onFrameTimings);
    _batchTimer?.cancel();
    _batchTimer = null;
    _flushPendingSamples();
    await _flushTail;
    final flushError = _firstFlushError;
    final flushStackTrace = _firstFlushStackTrace;
    final report = await _platform.stopPerformanceDiagnostics(_playerId, runId);
    if (flushError != null) {
      Error.throwWithStackTrace(
          flushError, flushStackTrace ?? StackTrace.current);
    }
    return _withLocalDrops(report);
  }

  void _onFrameTimings(List<FrameTiming> timings) {
    if (_stopping || timings.isEmpty) return;
    final budgetNs = _currentFrameBudgetNs();
    for (final timing in timings) {
      final effectiveLoad = math.max(
        timing.buildDuration.inMicroseconds,
        timing.rasterDuration.inMicroseconds,
      );
      _pendingSamples.add(
        VesperPerformanceFrameSample(
          loadNs: effectiveLoad * 1000,
          budgetNs: budgetNs,
          overlayState: _overlayState,
        ),
      );
      if (_pendingSamples.length >= _maximumFrameBatchSize) {
        _flushPendingSamples();
      }
    }
    if (_pendingSamples.isNotEmpty && _batchTimer == null) {
      _batchTimer = Timer(_maximumFrameBatchDelay, _flushPendingSamples);
    }
  }

  void _flushPendingSamples() {
    _batchTimer?.cancel();
    _batchTimer = null;
    if (_pendingSamples.isEmpty) return;
    final samples = List<VesperPerformanceFrameSample>.of(_pendingSamples);
    _pendingSamples.clear();
    for (var offset = 0;
        offset < samples.length;
        offset += _maximumFrameBatchSize) {
      final end = math.min(offset + _maximumFrameBatchSize, samples.length);
      final batch = samples.sublist(offset, end);
      if (_queuedFrameBatches >= _maximumQueuedFrameBatches) {
        _locallyDroppedSamples += batch.length;
        continue;
      }
      _queuedFrameBatches += 1;
      _flushTail = _flushTail.then((_) async {
        try {
          await _platform.submitPerformanceFrameSamples(
            _playerId,
            runId,
            batch,
          );
        } catch (error, stackTrace) {
          _firstFlushError ??= error;
          _firstFlushStackTrace ??= stackTrace;
        } finally {
          _queuedFrameBatches -= 1;
        }
      });
    }
  }

  int _currentFrameBudgetNs() {
    final views = PlatformDispatcher.instance.views;
    final refreshRate = views.isEmpty ? 60.0 : views.first.display.refreshRate;
    final effectiveRate =
        refreshRate.isFinite && refreshRate > 0 ? refreshRate : 60.0;
    return (1000000000 / effectiveRate).round().clamp(1, 1000000000);
  }

  void _ensureActive() {
    if (_stopping || _finalReportFuture != null) {
      throw const VesperPerformanceDiagnosticsException(
        code: 'controllerDisposed',
        message: 'The performance diagnostics session has stopped.',
      );
    }
  }

  void _throwPendingFlushError() {
    final error = _firstFlushError;
    if (error == null) return;
    Error.throwWithStackTrace(
      error,
      _firstFlushStackTrace ?? StackTrace.current,
    );
  }

  VesperPerformanceDiagnosticsReport _withLocalDrops(
    VesperPerformanceDiagnosticsReport report,
  ) {
    if (_locallyDroppedSamples == 0) return report;
    return VesperPerformanceDiagnosticsReport(
      schemaVersion: report.schemaVersion,
      runId: report.runId,
      sessionId: report.sessionId,
      platform: report.platform,
      probe: report.probe,
      durationNs: report.durationNs,
      frameBudgetNs: report.frameBudgetNs,
      cohorts: report.cohorts,
      playback: report.playback,
      diagnosis: report.diagnosis,
      acceptedEvents: report.acceptedEvents,
      droppedEvents: report.droppedEvents + _locallyDroppedSamples,
      rawEventsDropped: report.rawEventsDropped,
      diagnostics: report.diagnostics,
      rawEvents: report.rawEvents,
      extensions: report.extensions,
    );
  }
}

bool _isValidMarkerName(String name) {
  if (name.isEmpty || name.length > 64) return false;
  final units = name.codeUnits;
  if (units.any((unit) => unit > 0x7f)) return false;
  bool isLetter(int unit) =>
      (unit >= 0x41 && unit <= 0x5a) || (unit >= 0x61 && unit <= 0x7a);
  bool isDigit(int unit) => unit >= 0x30 && unit <= 0x39;
  if (!isLetter(units.first) && units.first != 0x5f) return false;
  return units.every(
    (unit) =>
        isLetter(unit) ||
        isDigit(unit) ||
        unit == 0x5f ||
        unit == 0x2e ||
        unit == 0x2d,
  );
}

void _validateOverlayState(VesperPerformanceOverlayState state) {
  final sampleClass = state.sampleClass.rawValue;
  if ((state.loadedBasicItemCount ?? 0) < 0 ||
      (state.loadedAdvancedItemCount ?? 0) < 0 ||
      (sampleClass != VesperPerformanceSampleClass.steady.rawValue &&
          sampleClass != VesperPerformanceSampleClass.transition.rawValue &&
          sampleClass != VesperPerformanceSampleClass.excluded.rawValue)) {
    throw const VesperPerformanceDiagnosticsException(
      code: 'protocolViolation',
      message:
          'The performance overlay state does not satisfy the wire contract.',
    );
  }
}
