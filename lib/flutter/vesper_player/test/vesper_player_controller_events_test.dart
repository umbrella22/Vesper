import 'dart:async';

import 'package:flutter_test/flutter_test.dart';
import 'package:vesper_player/vesper_player.dart';

void main() {
  TestWidgetsFlutterBinding.ensureInitialized();

  late VesperPlayerPlatform previousPlatform;
  late _EventTestPlatform platform;
  VesperPlayerController? controller;

  setUp(() {
    previousPlatform = VesperPlayerPlatform.instance;
    platform = _EventTestPlatform();
    VesperPlayerPlatform.instance = platform;
  });

  tearDown(() async {
    await controller?.dispose();
    await platform.close();
    VesperPlayerPlatform.instance = previousPlatform;
  });

  test('forwards pipeline EventHook reports once without changing snapshot',
      () async {
    controller = await VesperPlayerController.create();
    final snapshotBefore = controller!.snapshot;
    final event = _reportsEvent('playback.ready');
    final received = <VesperPlayerPipelineEventHookReportsEvent>[];
    final subscription = controller!.events.listen((event) {
      if (event is VesperPlayerPipelineEventHookReportsEvent) {
        received.add(event);
      }
    });
    addTearDown(subscription.cancel);

    platform.emit(event);
    await Future<void>.delayed(Duration.zero);

    expect(received, hasLength(1));
    expect(identical(received.single, event), isTrue);
    expect(identical(received.single.reports, event.reports), isTrue);
    expect(identical(controller!.snapshot, snapshotBefore), isTrue);
  });

  test('keeps final dispose reports and ignores events after disposal',
      () async {
    final finalEvent = _reportsEvent('playback.dispose');
    final lateEvent = _reportsEvent('playback.late');
    platform.eventOnDispose = finalEvent;
    controller = await VesperPlayerController.create();
    final received = <VesperPlayerPipelineEventHookReportsEvent>[];
    final subscription = controller!.events.listen((event) {
      if (event is VesperPlayerPipelineEventHookReportsEvent) {
        received.add(event);
      }
    });

    await controller!.dispose();
    platform.emit(lateEvent);
    await Future<void>.delayed(Duration.zero);

    expect(received, hasLength(1));
    expect(identical(received.single, finalEvent), isTrue);
    await subscription.cancel();
  });
}

VesperPlayerPipelineEventHookReportsEvent _reportsEvent(String eventName) {
  return VesperPlayerPipelineEventHookReportsEvent(
    playerId: _EventTestPlatform.playerId,
    reports: VesperPipelineEventHookReportBatch(
      reports: <VesperPipelineEventHookReport>[
        VesperPipelineEventHookReport(
          pluginId: 'dev.vesper.hook',
          capabilityInstanceId: 'dev.vesper.hook.playback',
          transport: VesperPluginTransport.native,
          runId: 'run-1',
          sessionId: 'session-1',
          eventName: eventName,
          result: const VesperPipelineEventHookResult(
            status: VesperPipelineEventHookResultStatus.accepted,
            outcome: VesperPipelineEventHookOutcome(accepted: true),
          ),
        ),
      ],
    ),
  );
}

final class _EventTestPlatform extends VesperPlayerPlatform {
  static const String playerId = 'event-test-player';

  final StreamController<VesperPlayerEvent> _events =
      StreamController<VesperPlayerEvent>.broadcast(sync: true);
  VesperPlayerEvent? eventOnDispose;

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
    return const VesperPlatformCreateResult(
      playerId: playerId,
      snapshot: VesperPlayerSnapshot.initial(),
    );
  }

  @override
  Stream<VesperPlayerEvent> eventsFor(String playerId) => _events.stream;

  @override
  Future<void> dispose(String playerId) async {
    final event = eventOnDispose;
    if (event != null) {
      _events.add(event);
    }
  }

  void emit(VesperPlayerEvent event) {
    _events.add(event);
  }

  Future<void> close() => _events.close();

  @override
  dynamic noSuchMethod(Invocation invocation) => super.noSuchMethod(invocation);
}
