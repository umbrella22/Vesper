import 'dart:async';

import 'package:flutter_test/flutter_test.dart';
import 'package:vesper_player/vesper_player.dart';

void main() {
  TestWidgetsFlutterBinding.ensureInitialized();

  late VesperPlayerPlatform previousPlatform;
  late _TimelineTestPlatform platform;

  setUp(() {
    previousPlatform = VesperPlayerPlatform.instance;
    platform = _TimelineTestPlatform();
    VesperPlayerPlatform.instance = platform;
  });

  tearDown(() async {
    await platform.close();
    VesperPlayerPlatform.instance = previousPlatform;
  });

  testWidgets('timeline samples patch only the timeline', (tester) async {
    final initial = _playingSnapshot(
      timeline: const VesperTimeline(
        kind: VesperTimelineKind.vod,
        isSeekable: true,
        positionMs: 100,
        durationMs: 10_000,
      ),
    );
    platform.initialSnapshot = initial;
    platform.sampleResult = Future<VesperTimeline?>.value(
      const VesperTimeline(
        kind: VesperTimelineKind.vod,
        isSeekable: true,
        positionMs: 250,
        durationMs: 10_000,
      ),
    );

    final controller = await VesperPlayerController.create();
    addTearDown(controller.dispose);

    await tester.pump(const Duration(seconds: 1));
    await tester.pump();

    final expected = initial.toMap();
    expected['timeline'] = platform.sampleTimelineValue.toMap();
    expect(controller.snapshot.toMap(), expected);
    expect(platform.sampleCalls, 1);
    unawaited(controller.dispose());
  });

  testWidgets('an in-flight sample cannot overwrite a newer command',
      (tester) async {
    final initial = _playingSnapshot(
      timeline: const VesperTimeline(
        kind: VesperTimelineKind.vod,
        isSeekable: true,
        positionMs: 100,
        durationMs: 10_000,
      ),
    );
    final pending = Completer<VesperTimeline?>();
    platform.initialSnapshot = initial;
    platform.sampleResult = pending.future;

    final controller = await VesperPlayerController.create();
    addTearDown(controller.dispose);

    await tester.pump(const Duration(seconds: 1));
    expect(platform.sampleCalls, 1);

    await controller.seekBy(1_000);
    pending.complete(
      const VesperTimeline(
        kind: VesperTimelineKind.vod,
        isSeekable: true,
        positionMs: 900,
        durationMs: 10_000,
      ),
    );
    await tester.pump();

    expect(controller.snapshot.timeline.positionMs, 100);
    unawaited(controller.dispose());
  });

  testWidgets('snapshots during a sample do not create an overlapping sample',
      (tester) async {
    final initial = _playingSnapshot(
      timeline: const VesperTimeline(
        kind: VesperTimelineKind.vod,
        isSeekable: true,
        positionMs: 100,
        durationMs: 10_000,
      ),
    );
    final pending = Completer<VesperTimeline?>();
    platform.initialSnapshot = initial;
    platform.sampleResult = pending.future;

    final controller = await VesperPlayerController.create();
    addTearDown(controller.dispose);

    await tester.pump(const Duration(seconds: 1));
    expect(platform.sampleCalls, 1);

    platform.emit(
      VesperPlayerSnapshotEvent(
        playerId: _TimelineTestPlatform.playerId,
        snapshot: initial.copyWith(
          timeline: const VesperTimeline(
            kind: VesperTimelineKind.vod,
            isSeekable: true,
            positionMs: 200,
            durationMs: 10_000,
          ),
        ),
      ),
    );
    platform.emit(
      VesperPlayerSnapshotEvent(
        playerId: _TimelineTestPlatform.playerId,
        snapshot: initial.copyWith(
          timeline: const VesperTimeline(
            kind: VesperTimelineKind.vod,
            isSeekable: true,
            positionMs: 300,
            durationMs: 10_000,
          ),
        ),
      ),
    );
    await tester.pump();

    expect(platform.sampleCalls, 1);
    pending.complete(null);
    await tester.pump();
    unawaited(controller.dispose());
  });

  testWidgets('pause cancels polling after an in-flight sample completes',
      (tester) async {
    final pending = Completer<VesperTimeline?>();
    platform.initialSnapshot = _playingSnapshot();
    platform.sampleResult = pending.future;

    final controller = await VesperPlayerController.create();
    addTearDown(controller.dispose);

    await tester.pump(const Duration(seconds: 1));
    expect(platform.sampleCalls, 1);

    platform.emit(
      VesperPlayerSnapshotEvent(
        playerId: _TimelineTestPlatform.playerId,
        snapshot: platform.initialSnapshot.copyWith(
          playbackState: VesperPlaybackState.paused,
        ),
      ),
    );
    await tester.pump();
    pending.complete(
      const VesperTimeline(
        kind: VesperTimelineKind.vod,
        isSeekable: true,
        positionMs: 900,
        durationMs: 10_000,
      ),
    );
    await tester.pump();
    await tester.pump(const Duration(seconds: 2));

    expect(platform.sampleCalls, 1);
    expect(controller.snapshot.playbackState, VesperPlaybackState.paused);
  });

  testWidgets('sampling failures use bounded 1, 2, 4, 8 second backoff',
      (tester) async {
    platform.initialSnapshot = _playingSnapshot();
    platform.sampleHandler = () async {
      platform.sampleCalls += 1;
      throw StateError('sample unavailable');
    };

    final controller = await VesperPlayerController.create();
    addTearDown(controller.dispose);

    await tester.pump(const Duration(seconds: 1));
    await tester.pump();
    expect(platform.sampleCalls, 1);
    expect(controller.snapshot.lastError, isNull);

    await tester.pump(const Duration(seconds: 1));
    expect(platform.sampleCalls, 1);
    await tester.pump(const Duration(seconds: 1));
    await tester.pump();
    expect(platform.sampleCalls, 2);

    await tester.pump(const Duration(seconds: 4));
    await tester.pump();
    expect(platform.sampleCalls, 3);

    await tester.pump(const Duration(seconds: 8));
    await tester.pump();
    expect(platform.sampleCalls, 4);
    unawaited(controller.dispose());
  });

  testWidgets('an error snapshot stops progress sampling', (tester) async {
    platform.initialSnapshot = _playingSnapshot();
    final controller = await VesperPlayerController.create();
    addTearDown(controller.dispose);

    platform.emit(
      VesperPlayerErrorEvent(
        playerId: _TimelineTestPlatform.playerId,
        error: const VesperPlayerError(
          message: 'network failed',
          code: VesperPlayerErrorCode.backendFailure,
          category: VesperPlayerErrorCategory.network,
          retriable: true,
        ),
      ),
    );
    await tester.pump();
    await tester.pump(const Duration(seconds: 2));

    expect(controller.snapshot.lastError?.category,
        VesperPlayerErrorCategory.network);
    expect(platform.sampleCalls, 0);
  });

  testWidgets('an error event owns lastError even when its snapshot omits it',
      (tester) async {
    platform.initialSnapshot = _playingSnapshot();
    final controller = await VesperPlayerController.create();
    addTearDown(controller.dispose);

    final error = const VesperPlayerError(
      message: 'network failed',
      code: VesperPlayerErrorCode.backendFailure,
      category: VesperPlayerErrorCategory.network,
      retriable: true,
    );
    platform.emit(
      VesperPlayerErrorEvent(
        playerId: _TimelineTestPlatform.playerId,
        error: error,
        snapshot: platform.initialSnapshot,
      ),
    );
    await tester.pump();
    await tester.pump(const Duration(seconds: 2));

    expect(controller.snapshot.lastError, error);
    expect(platform.sampleCalls, 0);
  });
}

VesperPlayerSnapshot _playingSnapshot({VesperTimeline? timeline}) {
  final initial = const VesperPlayerSnapshot.initial();
  return initial
      .copyWith(
        playbackState: VesperPlaybackState.playing,
      )
      .copyWith(timeline: timeline ?? const VesperTimeline.initial());
}

final class _TimelineTestPlatform extends VesperPlayerPlatform {
  static const playerId = 'timeline-test-player';

  final StreamController<VesperPlayerEvent> _events =
      StreamController<VesperPlayerEvent>.broadcast();
  VesperPlayerSnapshot initialSnapshot = const VesperPlayerSnapshot.initial();
  Future<VesperTimeline?>? sampleResult;
  Future<VesperTimeline?> Function()? sampleHandler;
  int sampleCalls = 0;

  VesperTimeline get sampleTimelineValue => const VesperTimeline(
        kind: VesperTimelineKind.vod,
        isSeekable: true,
        positionMs: 250,
        durationMs: 10_000,
      );

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
  }) async {
    return VesperPlatformCreateResult(
      playerId: _TimelineTestPlatform.playerId,
      snapshot: initialSnapshot,
    );
  }

  @override
  Stream<VesperPlayerEvent> eventsFor(String playerId) => _events.stream;

  @override
  Future<void> dispose(String playerId) async {}

  @override
  Future<void> seekBy(String playerId, int deltaMs) async {}

  @override
  Future<VesperTimeline?> sampleTimeline(String playerId) {
    if (sampleHandler != null) {
      return sampleHandler!();
    }
    sampleCalls += 1;
    return sampleResult ?? Future<VesperTimeline?>.value(sampleTimelineValue);
  }

  void emit(VesperPlayerEvent event) => _events.add(event);

  Future<void> close() => _events.close();

  @override
  dynamic noSuchMethod(Invocation invocation) => super.noSuchMethod(invocation);
}
