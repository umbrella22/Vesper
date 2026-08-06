import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:vesper_player/vesper_player.dart';

void main() {
  TestWidgetsFlutterBinding.ensureInitialized();

  late _FakeVesperPlatform platform;
  late VesperPlayerController controller;

  setUp(() async {
    platform = _FakeVesperPlatform();
    VesperPlayerPlatform.instance = platform;
    controller = await VesperPlayerController.create();
  });

  tearDown(() async {
    await controller.dispose();
  });

  testWidgets('scrolling coalesces viewport reports', (tester) async {
    final scrollController = ScrollController();
    addTearDown(scrollController.dispose);

    await tester.pumpWidget(
      MaterialApp(
        home: SizedBox(
          width: 400,
          height: 800,
          child: SingleChildScrollView(
            controller: scrollController,
            child: Column(
              children: <Widget>[
                const SizedBox(height: 120),
                SizedBox(
                  width: 320,
                  height: 180,
                  child: VesperPlayerView(controller: controller),
                ),
                const SizedBox(height: 1600),
              ],
            ),
          ),
        ),
      ),
    );
    await tester.pump();

    final initialReports = platform.viewportUpdates.length;
    expect(initialReports, greaterThan(0));

    await tester.drag(
      find.byType(SingleChildScrollView),
      const Offset(0, -240),
    );
    await tester.pump(const Duration(milliseconds: 250));

    expect(scrollController.offset, greaterThan(0));
    final reportsDuringScroll =
        platform.viewportUpdates.length - initialReports;
    expect(reportsDuringScroll, lessThanOrEqualTo(3));
    expect(platform.viewportUpdates.last.top, lessThan(120));
  });
}

final class _FakeVesperPlatform extends VesperPlayerPlatform {
  final viewportUpdates = <VesperPlayerViewport>[];

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
      playerId: 'view-test-player',
      snapshot: VesperPlayerSnapshot.initial(),
    );
  }

  @override
  Stream<VesperPlayerEvent> eventsFor(String playerId) {
    return const Stream<VesperPlayerEvent>.empty();
  }

  @override
  Future<void> updateViewport(
    String playerId,
    VesperPlayerViewport viewport,
  ) async {
    viewportUpdates.add(viewport);
  }

  @override
  Future<void> clearViewport(String playerId) async {}

  @override
  Future<void> dispose(String playerId) async {}

  @override
  dynamic noSuchMethod(Invocation invocation) => super.noSuchMethod(invocation);
}
