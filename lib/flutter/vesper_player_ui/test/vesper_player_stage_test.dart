import 'package:flutter/foundation.dart';
import 'package:material_ui/material_ui.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:vesper_player/vesper_player.dart';
import 'package:vesper_player_ui/vesper_player_ui.dart';

void main() {
  TestWidgetsFlutterBinding.ensureInitialized();

  late _FakeVesperPlayerPlatform platform;
  late VesperPlayerController controller;
  late _FakeDeviceControls deviceControls;
  final openedSheets = <VesperPlayerStageSheet>[];
  var fullscreenToggleCount = 0;

  setUp(() async {
    platform = _FakeVesperPlayerPlatform();
    VesperPlayerPlatform.instance = platform;
    controller = await VesperPlayerController.create();
    deviceControls = _FakeDeviceControls();
    openedSheets.clear();
    fullscreenToggleCount = 0;
  });

  Future<void> pumpStage(
    WidgetTester tester, {
    Widget? topBarPrimaryAction,
    Widget? topBarSecondaryAction,
    VesperPlayerSnapshot snapshot = _playingSnapshot,
    VesperPlayerStageStrings strings = const VesperPlayerStageStrings(),
  }) async {
    addTearDown(() async {
      await tester.pumpWidget(const SizedBox.shrink());
      await controller.dispose();
    });

    await tester.pumpWidget(
      MaterialApp(
        home: Scaffold(
          body: Center(
            child: SizedBox(
              width: 400,
              height: 240,
              child: VesperPlayerStage(
                controller: controller,
                snapshot: snapshot,
                isPortrait: true,
                deviceControls: deviceControls,
                topBarPrimaryAction: topBarPrimaryAction,
                topBarSecondaryAction: topBarSecondaryAction,
                strings: strings,
                onOpenSheet: openedSheets.add,
                onToggleFullscreen: () {
                  fullscreenToggleCount += 1;
                },
              ),
            ),
          ),
        ),
      ),
    );
    await tester.pump();
  }

  testWidgets(
      'empty stage taps still reach gestures while controls are visible',
      (tester) async {
    await pumpStage(tester);

    await tester.tapAt(const Offset(400, 300));
    await tester.pump(const Duration(milliseconds: 400));
    await tester.tap(find.byIcon(Icons.more_vert_rounded), warnIfMissed: false);

    expect(openedSheets, isEmpty);

    await tester.tapAt(const Offset(400, 300));
    await tester.pump(const Duration(milliseconds: 400));
    await tester.tap(find.byIcon(Icons.more_vert_rounded));

    expect(openedSheets, <VesperPlayerStageSheet>[
      VesperPlayerStageSheet.menu,
    ]);
  });

  testWidgets('top bar action slots render primary left of secondary',
      (tester) async {
    const primaryKey = Key('stage-primary-action');
    const secondaryKey = Key('stage-secondary-action');

    await pumpStage(
      tester,
      topBarPrimaryAction: const SizedBox.square(
        key: primaryKey,
        dimension: 38,
      ),
      topBarSecondaryAction: const SizedBox.square(
        key: secondaryKey,
        dimension: 38,
      ),
    );

    expect(find.byKey(primaryKey), findsOneWidget);
    expect(find.byKey(secondaryKey), findsOneWidget);
    expect(find.byIcon(Icons.more_vert_rounded), findsNothing);
    expect(
      tester.getTopLeft(find.byKey(primaryKey)).dx,
      lessThan(tester.getTopLeft(find.byKey(secondaryKey)).dx),
    );
  });

  testWidgets('default menu action renders when secondary slot is absent',
      (tester) async {
    await pumpStage(tester);

    expect(find.byIcon(Icons.more_vert_rounded), findsOneWidget);
  });

  testWidgets('stage uses supplied visible strings', (tester) async {
    await pumpStage(
      tester,
      snapshot: _playingSnapshot.copyWith(isBuffering: true),
      strings: const VesperPlayerStageStrings(
        buffering: 'Loading media',
        vodTimelineBadge: 'On-demand asset',
      ),
    );

    expect(find.text('Loading media'), findsOneWidget);
    expect(find.text('On-demand asset'), findsOneWidget);
  });

  testWidgets('empty left-side vertical drags drive brightness controls',
      (tester) async {
    await pumpStage(tester);

    await tester.dragFrom(const Offset(280, 300), const Offset(0, -80));
    await tester.pump();
    await tester.pump(const Duration(milliseconds: 400));

    expect(deviceControls.brightnessSets, isNotEmpty);
    expect(deviceControls.brightnessSets.last, greaterThan(0.5));
  });

  testWidgets('brightness at 100 percent does not block the next stage drag',
      (tester) async {
    deviceControls.setBrightnessResult = 1.0;
    await pumpStage(tester);

    await tester.dragFrom(const Offset(280, 300), const Offset(0, -120));
    await tester.pump();
    final firstSetCount = deviceControls.brightnessSets.length;

    await tester.dragFrom(const Offset(280, 300), const Offset(0, -40));
    await tester.pump();
    await tester.pump(const Duration(milliseconds: 400));

    expect(firstSetCount, greaterThan(0));
    expect(deviceControls.brightnessSets.length, greaterThan(firstSetCount));
  });

  testWidgets('visible timeline and buttons remain clickable', (tester) async {
    await pumpStage(tester);

    await tester.tap(find.byIcon(Icons.pause_rounded).first);
    await tester.pump();
    expect(platform.togglePauseCount, 1);

    await tester.tap(find.byType(VesperTimelineScrubber));
    await tester.pump();
    expect(platform.seekRatios, isNotEmpty);

    await tester.tap(find.byIcon(Icons.fullscreen_rounded));
    await tester.pump();
    expect(fullscreenToggleCount, 1);
  });
}

const _playingSnapshot = VesperPlayerSnapshot(
  title: 'Sample',
  subtitle: '',
  sourceLabel: '',
  playbackState: VesperPlaybackState.playing,
  playbackRate: 1,
  isBuffering: false,
  isInterrupted: false,
  hasVideoSurface: true,
  timeline: VesperTimeline(
    kind: VesperTimelineKind.vod,
    isSeekable: true,
    seekableRange: null,
    liveEdgeMs: null,
    positionMs: 50000,
    durationMs: 100000,
  ),
);

final class _FakeDeviceControls implements VesperPlayerDeviceControls {
  final brightnessSets = <double>[];
  double currentBrightness = 0.5;
  double? setBrightnessResult;

  @override
  Future<double?> currentBrightnessRatio() => SynchronousFuture<double?>(
        currentBrightness,
      );

  @override
  Future<double?> setBrightnessRatio(double ratio) {
    brightnessSets.add(ratio);
    return SynchronousFuture<double?>(setBrightnessResult ?? ratio);
  }

  @override
  Future<double?> currentVolumeRatio() => SynchronousFuture<double?>(0.5);

  @override
  Future<double?> setVolumeRatio(double ratio) => SynchronousFuture<double?>(
        ratio,
      );
}

final class _FakeVesperPlayerPlatform extends VesperPlayerPlatform {
  var togglePauseCount = 0;
  final seekRatios = <double>[];

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
  }) async =>
      const VesperPlatformCreateResult(
        playerId: 'stage-test-player',
        snapshot: _playingSnapshot,
      );

  @override
  Stream<VesperPlayerEvent> eventsFor(String playerId) {
    return const Stream<VesperPlayerEvent>.empty();
  }

  @override
  Future<void> togglePause(String playerId) async {
    togglePauseCount += 1;
  }

  @override
  Future<void> seekToRatio(String playerId, double ratio) async {
    seekRatios.add(ratio);
  }

  @override
  Future<void> updateViewport(
      String playerId, VesperPlayerViewport viewport) async {}

  @override
  Future<void> clearViewport(String playerId) async {}

  @override
  Future<void> dispose(String playerId) async {}

  @override
  dynamic noSuchMethod(Invocation invocation) => super.noSuchMethod(invocation);
}
