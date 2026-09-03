import 'dart:async';
import 'dart:ui';

import 'package:flutter_test/flutter_test.dart';
import 'package:vesper_player/vesper_player.dart';

void main() {
  TestWidgetsFlutterBinding.ensureInitialized();

  late VesperPlayerPlatform previousPlatform;
  late _DiagnosticsTestPlatform platform;
  VesperPlayerController? controller;

  setUp(() async {
    previousPlatform = VesperPlayerPlatform.instance;
    platform = _DiagnosticsTestPlatform();
    VesperPlayerPlatform.instance = platform;
    controller = await VesperPlayerController.create();
  });

  tearDown(() async {
    await controller?.dispose();
    await platform.close();
    VesperPlayerPlatform.instance = previousPlatform;
  });

  test(
      'batches FrameTiming samples at 120 and keeps capture-time overlay state',
      () async {
    final session = await controller!.startPerformanceDiagnostics();
    _reportTimings(<FrameTiming>[_timing(buildUs: 2, rasterUs: 7)]);
    await session.updateOverlayState(
      const VesperPerformanceOverlayState(
        active: true,
        loadedBasicItemCount: 42,
      ),
    );
    _reportTimings(
      List<FrameTiming>.generate(
        129,
        (_) => _timing(buildUs: 11, rasterUs: 3),
      ),
    );

    await session.snapshot();

    expect(
        platform.submittedBatches.map((batch) => batch.length), <int>[120, 10]);
    final samples = platform.submittedBatches.expand((batch) => batch).toList();
    expect(samples.first.loadNs, 7000);
    expect(samples.first.overlayState.active, isFalse);
    expect(samples[1].loadNs, 11000);
    expect(samples[1].overlayState.active, isTrue);
    expect(samples[1].overlayState.loadedBasicItemCount, 42);
    await session.stop();
  });

  test('stop removes the timing callback and caches the final report',
      () async {
    final session = await controller!.startPerformanceDiagnostics();
    _reportTimings(<FrameTiming>[_timing(buildUs: 1, rasterUs: 2)]);

    final first = await session.stop();
    final second = await session.stop();
    final submittedBeforeLateTiming = platform.submittedBatches.length;
    _reportTimings(<FrameTiming>[_timing(buildUs: 5, rasterUs: 6)]);
    await Future<void>.delayed(Duration.zero);

    expect(identical(first, second), isTrue);
    expect(platform.stopCount, 1);
    expect(platform.submittedBatches.length, submittedBeforeLateTiming);
  });

  test('controller dispose stops diagnostics before disposing the player',
      () async {
    await controller!.startPerformanceDiagnostics();

    await controller!.dispose();

    expect(platform.operations, <String>['start', 'stop', 'dispose']);
  });

  test('dispose during start stops the late session and removes frame timing',
      () async {
    platform.startGate = Completer<void>();
    final start = controller!.startPerformanceDiagnostics();
    await Future<void>.delayed(Duration.zero);

    await controller!.dispose();
    platform.startGate!.complete();

    await expectLater(
      start,
      throwsA(
        isA<VesperPerformanceDiagnosticsException>().having(
          (error) => error.code,
          'code',
          'controllerDisposed',
        ),
      ),
    );
    _reportTimings(<FrameTiming>[_timing(buildUs: 4, rasterUs: 8)]);
    await Future<void>.delayed(Duration.zero);

    expect(platform.operations, <String>['start', 'dispose', 'stop']);
    expect(platform.submittedBatches, isEmpty);
  });

  test('invalid configuration uses the stable error before platform startup',
      () async {
    await expectLater(
      controller!.startPerformanceDiagnostics(
        configuration: const VesperPerformanceDiagnosticsConfiguration(
          maxRawEvents: 2049,
        ),
      ),
      throwsA(
        isA<VesperPerformanceDiagnosticsException>().having(
          (error) => error.code,
          'code',
          'invalidConfiguration',
        ),
      ),
    );

    expect(platform.operations, isEmpty);
  });

  test('invalid overlay state uses the stable protocol error locally',
      () async {
    final session = await controller!.startPerformanceDiagnostics();

    await expectLater(
      session.updateOverlayState(
        const VesperPerformanceOverlayState(
          active: true,
          loadedAdvancedItemCount: -1,
        ),
      ),
      throwsA(
        isA<VesperPerformanceDiagnosticsException>().having(
          (error) => error.code,
          'code',
          'protocolViolation',
        ),
      ),
    );
    await session.stop();
  });

  test('a frame submission failure still stops native resources exactly once',
      () async {
    platform.failSubmissions = true;
    final session = await controller!.startPerformanceDiagnostics();
    _reportTimings(<FrameTiming>[_timing(buildUs: 4, rasterUs: 8)]);

    await expectLater(session.stop(), throwsStateError);
    await expectLater(session.stop(), throwsStateError);

    expect(platform.stopCount, 1);
  });

  test('marker validation rejects lossy or non-finite wire values locally',
      () async {
    final session = await controller!.startPerformanceDiagnostics();

    expect(
      () => session.recordMarker('contains space'),
      throwsA(
        isA<VesperPerformanceDiagnosticsException>().having(
          (error) => error.code,
          'code',
          'protocolViolation',
        ),
      ),
    );
    expect(
      () => session.recordMarker('valid_marker', value: double.nan),
      throwsA(isA<VesperPerformanceDiagnosticsException>()),
    );
    await session.stop();
  });

  test('bounds queued frame batches and reports locally dropped samples',
      () async {
    platform.submissionGate = Completer<void>();
    final session = await controller!.startPerformanceDiagnostics();

    _reportTimings(
      List<FrameTiming>.generate(
        600,
        (_) => _timing(buildUs: 4, rasterUs: 8),
      ),
    );
    await Future<void>.delayed(Duration.zero);
    platform.submissionGate!.complete();

    final report = await session.snapshot();

    expect(platform.submittedBatches.length, 4);
    expect(platform.submittedBatches.every((batch) => batch.length == 120),
        isTrue);
    expect(report.droppedEvents, 120);
    await session.stop();
  });
}

void _reportTimings(List<FrameTiming> timings) {
  PlatformDispatcher.instance.onReportTimings?.call(timings);
}

FrameTiming _timing({required int buildUs, required int rasterUs}) =>
    FrameTiming(
      vsyncStart: 0,
      buildStart: 1,
      buildFinish: 1 + buildUs,
      rasterStart: 100,
      rasterFinish: 100 + rasterUs,
      rasterFinishWallTime: 100 + rasterUs,
    );

final class _DiagnosticsTestPlatform extends VesperPlayerPlatform {
  static const playerId = 'diagnostics-test-player';

  final StreamController<VesperPlayerEvent> _events =
      StreamController<VesperPlayerEvent>.broadcast();
  final List<List<VesperPerformanceFrameSample>> submittedBatches =
      <List<VesperPerformanceFrameSample>>[];
  final List<String> operations = <String>[];
  bool failSubmissions = false;
  Completer<void>? startGate;
  Completer<void>? submissionGate;
  int stopCount = 0;

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
  }) async =>
      const VesperPlatformCreateResult(
        playerId: playerId,
        snapshot: VesperPlayerSnapshot.initial(),
      );

  @override
  Stream<VesperPlayerEvent> eventsFor(String playerId) => _events.stream;

  @override
  Future<String> startPerformanceDiagnostics(
    String playerId,
    VesperPerformanceDiagnosticsConfiguration configuration,
  ) async {
    operations.add('start');
    await startGate?.future;
    return 'run-1';
  }

  @override
  Future<void> updatePerformanceOverlayState(
    String playerId,
    String runId,
    VesperPerformanceOverlayState state,
  ) async {}

  @override
  Future<void> recordPerformanceMarker(
    String playerId,
    String runId,
    String name, {
    double? value,
    int? sequenceIndex,
    bool? expectedOverlayActive,
  }) async {}

  @override
  Future<void> submitPerformanceFrameSamples(
    String playerId,
    String runId,
    List<VesperPerformanceFrameSample> samples,
  ) async {
    submittedBatches.add(List<VesperPerformanceFrameSample>.of(samples));
    await submissionGate?.future;
    if (failSubmissions) throw StateError('submit failed');
  }

  @override
  Future<VesperPerformanceDiagnosticsReport> performanceDiagnosticsSnapshot(
    String playerId,
    String runId,
  ) async =>
      _report(runId);

  @override
  Future<VesperPerformanceDiagnosticsReport> stopPerformanceDiagnostics(
    String playerId,
    String runId,
  ) async {
    stopCount += 1;
    operations.add('stop');
    return _report(runId);
  }

  @override
  Future<void> dispose(String playerId) async {
    operations.add('dispose');
  }

  Future<void> close() => _events.close();

  @override
  dynamic noSuchMethod(Invocation invocation) => super.noSuchMethod(invocation);
}

VesperPerformanceDiagnosticsReport _report(String runId) =>
    VesperPerformanceDiagnosticsReport.fromMap(<Object?, Object?>{
      'schemaVersion': 1,
      'runId': runId,
      'sessionId': 'session-1',
      'platform': 'test',
      'probe': 'flutterFrameTiming',
      'durationNs': 1,
      'frameBudgetNs': 16666667,
      'cohorts': <String, Object?>{
        for (final name in <String>[
          'overlayInactive',
          'overlayActive',
          'transition',
          'excluded',
        ])
          name: <String, Object?>{
            'sampleCount': 0,
            'jankCount': 0,
            'severeJankCount': 0,
            'jankRatio': 0.0,
            'severeJankRatio': 0.0,
            'minLoadNs': 0,
            'p50LoadNs': 0,
            'p95LoadNs': 0,
            'maxLoadNs': 0,
          },
      },
      'playback': <String, Object?>{
        'activeDurationNs': 0,
        'droppedVideoFrames': 0,
        'bufferingCount': 0,
        'bufferingDurationNs': 0,
        'stallCount': 0,
      },
      'diagnosis': <String, Object?>{
        'kind': 'insufficientEvidence',
        'confidence': 'low',
        'evidenceCodes': <String>['steady_cohorts_below_120'],
      },
      'acceptedEvents': 0,
      'droppedEvents': 0,
      'rawEventsDropped': 0,
      'diagnostics': <Object?>[
        <String, Object?>{
          'code': 'performance.diagnosis',
          'severity': 'warning',
          'message': 'Correlation only.',
          'attributes': <String, String>{
            'kind': 'insufficientEvidence',
            'confidence': 'low',
            'evidenceCodes': 'steady_cohorts_below_120',
          },
        },
      ],
      'rawEvents': <Object?>[],
    });
