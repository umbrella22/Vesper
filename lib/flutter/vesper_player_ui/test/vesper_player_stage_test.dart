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
    Widget? contentOverlay,
    Widget? landscapeControlBarLeading,
    VoidCallback? onNavigateBack,
    String? navigateBackSemanticLabel,
    Widget? topBarPrimaryAction,
    Widget? topBarSecondaryAction,
    VesperPlayerSnapshot? snapshot,
    VesperPlayerStageStrings strings = const VesperPlayerStageStrings(),
    bool keepControlsVisible = false,
    bool pictureInPicturePresentation = false,
    bool isPortrait = true,
    bool insideVerticalScrollView = false,
    ScrollController? scrollController,
  }) async {
    addTearDown(() async {
      await tester.pumpWidget(const SizedBox.shrink());
      await controller.dispose();
    });

    final stage = Center(
      child: SizedBox(
        width: 400,
        height: 240,
        child: VesperPlayerStage(
          controller: controller,
          snapshot: snapshot ?? _playingSnapshot,
          isPortrait: isPortrait,
          deviceControls: deviceControls,
          contentOverlay: contentOverlay,
          landscapeControlBarLeading: landscapeControlBarLeading,
          onNavigateBack: onNavigateBack,
          navigateBackSemanticLabel: navigateBackSemanticLabel,
          topBarPrimaryAction: topBarPrimaryAction,
          topBarSecondaryAction: topBarSecondaryAction,
          keepControlsVisible: keepControlsVisible,
          pictureInPicturePresentation: pictureInPicturePresentation,
          strings: strings,
          onOpenSheet: openedSheets.add,
          onToggleFullscreen: () {
            fullscreenToggleCount += 1;
          },
        ),
      ),
    );

    await tester.pumpWidget(
      MaterialApp(
        home: Scaffold(
          body: insideVerticalScrollView
              ? SingleChildScrollView(
                  controller: scrollController,
                  child: SizedBox(height: 900, child: stage),
                )
              : stage,
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

  testWidgets('navigate back action is optional and uses the host label',
      (tester) async {
    var navigateBackCount = 0;
    final semantics = tester.ensureSemantics();
    try {
      await pumpStage(
        tester,
        onNavigateBack: () {
          navigateBackCount += 1;
        },
        navigateBackSemanticLabel: 'Exit listen mode',
      );

      expect(find.byTooltip('Exit listen mode'), findsOneWidget);
      expect(find.bySemanticsLabel('Exit listen mode'), findsOneWidget);
      await tester.tap(find.byIcon(Icons.arrow_back_rounded));
      expect(navigateBackCount, 1);

      await pumpStage(tester);
      expect(find.byIcon(Icons.arrow_back_rounded), findsNothing);
    } finally {
      semantics.dispose();
    }
  });

  testWidgets('content overlay cannot intercept stage or control input',
      (tester) async {
    var overlayTapCount = 0;
    final semantics = tester.ensureSemantics();
    try {
      await pumpStage(
        tester,
        contentOverlay: Semantics(
          label: 'Host overlay content',
          child: GestureDetector(
            key: const Key('content-overlay'),
            behavior: HitTestBehavior.opaque,
            onTap: () {
              overlayTapCount += 1;
            },
            child: const SizedBox.expand(),
          ),
        ),
      );

      expect(find.byKey(const Key('content-overlay')), findsOneWidget);
      expect(find.bySemanticsLabel('Host overlay content'), findsNothing);
      await tester.tapAt(const Offset(400, 300));
      await tester.pump(const Duration(milliseconds: 400));
      expect(overlayTapCount, 0);

      await tester.tapAt(const Offset(400, 300));
      await tester.pump(const Duration(milliseconds: 400));
      await tester.tap(find.byIcon(Icons.pause_rounded));
      await tester.pump();

      expect(overlayTapCount, 0);
      expect(platform.togglePauseCount, 1);
    } finally {
      semantics.dispose();
    }
  });

  testWidgets('empty landscape slot does not move built-in controls',
      (tester) async {
    await pumpStage(tester, isPortrait: false);
    final speedWithoutSlot =
        tester.getRect(find.byType(VesperStagePillButton).first);

    await pumpStage(
      tester,
      isPortrait: false,
      landscapeControlBarLeading: const SizedBox.shrink(
        key: Key('empty-landscape-slot'),
      ),
    );
    final speedWithEmptySlot =
        tester.getRect(find.byType(VesperStagePillButton).first);

    expect(speedWithEmptySlot, speedWithoutSlot);
  });

  testWidgets('landscape slot accepts fixed and flexible host widths',
      (tester) async {
    await pumpStage(
      tester,
      isPortrait: false,
      landscapeControlBarLeading: const SizedBox(
        key: Key('fixed-landscape-slot'),
        width: 72,
        height: 38,
      ),
    );

    expect(
      tester.getSize(find.byKey(const Key('fixed-landscape-slot'))).width,
      72,
    );
    expect(
      tester.getTopLeft(find.byKey(const Key('fixed-landscape-slot'))).dx,
      greaterThan(tester.getTopLeft(find.byIcon(Icons.pause_rounded)).dx),
    );

    await pumpStage(
      tester,
      isPortrait: false,
      landscapeControlBarLeading: const Expanded(
        child: SizedBox(
          key: Key('flex-landscape-slot'),
          height: 38,
        ),
      ),
    );

    expect(
      tester.getSize(find.byKey(const Key('flex-landscape-slot'))).width,
      greaterThan(72),
    );
  });

  testWidgets('keepControlsVisible restarts auto-hide after release',
      (tester) async {
    bool controlsIgnoreInput() {
      return tester
          .widgetList<IgnorePointer>(
            find.ancestor(
              of: find.byIcon(Icons.pause_rounded),
              matching: find.byType(IgnorePointer),
            ),
          )
          .any((widget) => widget.ignoring);
    }

    await pumpStage(tester, keepControlsVisible: true);
    await tester.pump(const Duration(seconds: 4));
    expect(controlsIgnoreInput(), isFalse);

    await pumpStage(tester);
    await tester.pump(const Duration(milliseconds: 2900));
    expect(controlsIgnoreInput(), isFalse);
    await tester.pump(const Duration(milliseconds: 200));
    expect(controlsIgnoreInput(), isTrue);
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

  testWidgets('snapshot-only updates keep the player view stable',
      (tester) async {
    await pumpStage(tester);
    final viewportUpdatesAfterInitialLayout = platform.viewportUpdateCount;

    await pumpStage(
      tester,
      snapshot: _playingSnapshot.copyWith(
        timeline: const VesperTimeline(
          kind: VesperTimelineKind.vod,
          isSeekable: true,
          seekableRange: null,
          liveEdgeMs: null,
          positionMs: 60000,
          durationMs: 100000,
        ),
      ),
    );

    expect(
      platform.viewportUpdateCount,
      viewportUpdatesAfterInitialLayout,
    );
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

  testWidgets(
      'windowed timeline drag keeps the scrubber gesture and uses its bounds',
      (tester) async {
    await pumpStage(tester);

    final scrubberRect = tester.getRect(find.byType(VesperTimelineScrubber));
    final start = Offset(
      scrubberRect.left + scrubberRect.width * 0.15,
      scrubberRect.center.dy,
    );
    final end = Offset(
      scrubberRect.left + scrubberRect.width * 0.85,
      scrubberRect.center.dy,
    );

    final gesture = await tester.startGesture(start);
    await gesture.moveTo(
      end,
      timeStamp: const Duration(milliseconds: 240),
    );
    await gesture.up();
    await tester.pump();

    expect(platform.seekRatios, isNotEmpty);
    expect(platform.seekRatios, hasLength(1));
    expect(platform.seekRatios.last, closeTo(0.85, 0.08));
  });

  testWidgets(
      'windowed timeline drag is not claimed by the portrait scroll view',
      (tester) async {
    final scrollController = ScrollController();
    addTearDown(scrollController.dispose);
    await pumpStage(
      tester,
      insideVerticalScrollView: true,
      scrollController: scrollController,
    );

    final scrubberRect = tester.getRect(find.byType(VesperTimelineScrubber));
    final gesture = await tester.startGesture(
      Offset(scrubberRect.left + scrubberRect.width * 0.12,
          scrubberRect.center.dy),
    );
    await gesture.moveBy(
      const Offset(8, -24),
      timeStamp: const Duration(milliseconds: 40),
    );
    for (final step in <double>[0.26, 0.43, 0.61, 0.79, 0.88]) {
      await gesture.moveTo(
        Offset(
          scrubberRect.left + scrubberRect.width * step,
          scrubberRect.center.dy + (step < 0.6 ? 3 : -3),
        ),
        timeStamp: Duration(milliseconds: (step * 600).round()),
      );
    }
    await gesture.up();
    await tester.pump();

    expect(scrollController.offset, 0);
    expect(platform.seekRatios, hasLength(1));
    expect(platform.seekRatios.single, closeTo(0.88, 0.08));
  });

  testWidgets('scrubber drag survives sibling width changes', (tester) async {
    final previews = <double>[];
    final commits = <double>[];

    await tester.pumpWidget(
      MaterialApp(
        home: Scaffold(
          body: Center(
            child: SizedBox(
              width: 360,
              child: StatefulBuilder(
                builder: (context, setState) {
                  final expandedSummary = previews.isNotEmpty;
                  return Row(
                    children: <Widget>[
                      Expanded(
                        child: VesperTimelineScrubber(
                          displayedRatio:
                              previews.isEmpty ? 0.1 : previews.last,
                          compact: true,
                          onSeekPreview: (ratio) {
                            previews.add(ratio);
                            setState(() {});
                          },
                          onSeekCommit: commits.add,
                          onSeekCancel: () {},
                        ),
                      ),
                      Text(expandedSummary ? '00:00/03:13' : '0'),
                    ],
                  );
                },
              ),
            ),
          ),
        ),
      ),
    );

    final scrubber = find.byType(VesperTimelineScrubber);
    final initialRect = tester.getRect(scrubber);
    final gesture = await tester.startGesture(Offset(
      initialRect.left + initialRect.width * 0.1,
      initialRect.center.dy,
    ));
    await gesture.moveTo(
      Offset(
          initialRect.left + initialRect.width * 0.35, initialRect.center.dy),
      timeStamp: const Duration(milliseconds: 80),
    );
    await tester.pump();

    final resizedRect = tester.getRect(scrubber);
    expect(resizedRect.width, lessThan(initialRect.width));
    await gesture.moveTo(
      Offset(resizedRect.left + resizedRect.width * 0.8, resizedRect.center.dy),
      timeStamp: const Duration(milliseconds: 160),
    );
    await gesture.up();
    await tester.pump();

    expect(previews, isNotEmpty);
    expect(previews.last, closeTo(0.8, 0.08));
    expect(commits, hasLength(1));
    expect(commits.single, closeTo(0.8, 0.08));
  });

  testWidgets(
      'scrubber commits the pointer-up position when the final move is coalesced',
      (tester) async {
    final previews = <double>[];
    final commits = <double>[];

    await tester.pumpWidget(
      MaterialApp(
        home: Scaffold(
          body: Center(
            child: SizedBox(
              width: 360,
              child: VesperTimelineScrubber(
                displayedRatio: 0.1,
                compact: true,
                onSeekPreview: previews.add,
                onSeekCommit: commits.add,
                onSeekCancel: () {},
              ),
            ),
          ),
        ),
      ),
    );
    await tester.pump();

    final scrubberRect = tester.getRect(find.byType(VesperTimelineScrubber));
    final start = Offset(
      scrubberRect.left + scrubberRect.width * 0.1,
      scrubberRect.center.dy,
    );
    final lastMove = Offset(
      scrubberRect.left + scrubberRect.width * 0.35,
      scrubberRect.center.dy,
    );
    final release = Offset(
      scrubberRect.left + scrubberRect.width * 0.9,
      scrubberRect.center.dy,
    );
    final pointer = TestPointer(41);

    await tester.sendEventToBinding(pointer.down(start));
    await tester.sendEventToBinding(pointer.move(lastMove));
    await tester.sendEventToBinding(
      PointerUpEvent(pointer: pointer.pointer, position: release),
    );
    await tester.pump();

    expect(previews, isNotEmpty);
    expect(commits, hasLength(1));
    expect(previews.last, closeTo(0.9, 0.04));
    expect(commits.single, closeTo(0.9, 0.04));
  });

  testWidgets(
      'picture in picture presentation hides custom chrome and gestures',
      (tester) async {
    await pumpStage(
      tester,
      contentOverlay: const SizedBox(key: Key('picture-overlay')),
      pictureInPicturePresentation: true,
      snapshot: _playingSnapshot.copyWith(isBuffering: true),
    );

    expect(find.text('Sample'), findsNothing);
    expect(find.text('Loading media'), findsNothing);
    expect(find.byIcon(Icons.more_vert_rounded), findsNothing);
    expect(find.byIcon(Icons.pause_rounded), findsNothing);
    expect(find.byType(VesperTimelineScrubber), findsNothing);
    expect(find.byKey(const Key('picture-overlay')), findsNothing);

    await tester.tapAt(const Offset(400, 300));
    await tester.pump(const Duration(milliseconds: 400));
    await tester.tapAt(const Offset(400, 300));
    await tester.pump();

    expect(platform.togglePauseCount, 0);
    expect(openedSheets, isEmpty);
  });
}

final _playingSnapshot = VesperPlayerSnapshot(
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
  var viewportUpdateCount = 0;
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
    VesperNativeFramePipelineConfiguration nativeFramePipelineConfiguration =
        const VesperNativeFramePipelineConfiguration(),
    VesperPipelineEventHookConfiguration pipelineEventHookConfiguration =
        const VesperPipelineEventHookConfiguration(),
  }) async =>
      VesperPlatformCreateResult(
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
    String playerId,
    VesperPlayerViewport viewport,
  ) async {
    viewportUpdateCount += 1;
  }

  @override
  Future<void> clearViewport(String playerId) async {}

  @override
  Future<void> dispose(String playerId) async {}

  @override
  dynamic noSuchMethod(Invocation invocation) => super.noSuchMethod(invocation);
}
