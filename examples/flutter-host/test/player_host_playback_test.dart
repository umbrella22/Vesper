import 'dart:async';

import 'package:flutter/services.dart';
import 'package:flutter_host/main.dart';
import 'package:flutter_host/src/player/example_player_stage.dart';
import 'package:flutter_host/src/shell/example_host_strings.dart';
import 'package:flutter_host/src/player/example_player_sections.dart';
import 'package:flutter_host/src/player/example_player_models.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';
import 'package:vesper_player/vesper_player.dart';

void main() {
  late _PausedPlayerPlatform platform;
  late VesperPlayerPlatform previous;

  setUp(() {
    previous = VesperPlayerPlatform.instance;
    platform = _PausedPlayerPlatform();
    VesperPlayerPlatform.instance = platform;
  });
  tearDown(() => VesperPlayerPlatform.instance = previous);

  testWidgets('Library Play starts a new source from a paused controller', (
    tester,
  ) async {
    await tester.pumpWidget(const VesperFlutterHostApp());
    await tester.pumpAndSettle();
    expect(platform.playWhenReady, [false]);
    await tester.tap(find.text('Library'));
    await tester.pumpAndSettle();
    await tester.tap(find.byKey(const ValueKey('library-collection-live')));
    await tester.pumpAndSettle();
    await tester.tap(
      find.byKey(
        const ValueKey('library-item-live-$flutterLiveDvrPlaylistItemId'),
      ),
    );
    await tester.pumpAndSettle();
    // Complete cancellation of the old controller's broadcast subscription.
    await tester.runAsync(() => Future<void>.delayed(Duration.zero));
    await tester.pumpAndSettle();
    expect(platform.playWhenReady, [false, true]);
    expect(find.byKey(const ValueKey('shell-open-media-fab')), findsOneWidget);

    // A settings-driven rebuild of this now-paused source keeps it paused.
    await tester.tap(find.text('Lab'));
    await tester.pumpAndSettle();
    final off = find.widgetWithText(ChoiceChip, 'SourceNormalizer off');
    await tester.ensureVisible(off);
    await tester.tap(off);
    await tester.pumpAndSettle();
    expect(platform.playWhenReady, [false, true, false]);
  });

  testWidgets('rejecting a new URL retains the working player and source', (
    tester,
  ) async {
    await tester.pumpWidget(const VesperFlutterHostApp());
    await tester.pumpAndSettle();
    final oldPlayer = tester
        .widget<ExamplePlayerStage>(find.byType(ExamplePlayerStage))
        .controller;
    platform.onNextActivation = () async =>
        throw PlatformException(code: 'rejected');
    await tester.tap(find.byKey(const ValueKey('shell-open-media-fab')));
    await tester.pumpAndSettle();
    await tester.enterText(
      find.byKey(const ValueKey('open-media-url-field')),
      'https://example.com/rejected.mp4',
    );
    await tester.tap(find.byKey(const ValueKey('open-media-url-submit')));
    await tester.pumpAndSettle();
    expect(
      find.text(ExampleHostStrings.en[ExampleHostText.openMediaFailed]),
      findsOneWidget,
    );
    await tester.binding.handlePopRoute();
    await tester.pumpAndSettle();
    expect(
      tester
          .widget<ExamplePlayerStage>(find.byType(ExamplePlayerStage))
          .controller,
      same(oldPlayer),
    );
    await tester.tap(find.text('Lab'));
    await tester.pumpAndSettle();
    expect(
      tester
          .widget<ExampleDiagnosticsSummarySection>(
            find.byType(ExampleDiagnosticsSummarySection),
          )
          .sourceProtocol,
      'hls',
    );
  });

  testWidgets('Library reports native failures and ignores a superseded tap', (
    tester,
  ) async {
    await tester.pumpWidget(const VesperFlutterHostApp());
    await tester.pumpAndSettle();
    await tester.tap(find.text('Library'));
    await tester.pumpAndSettle();
    await tester.tap(find.byKey(const ValueKey('library-collection-live')));
    await tester.pumpAndSettle();
    final live = find.byKey(
      const ValueKey('library-item-live-$flutterLiveDvrPlaylistItemId'),
    );
    platform.onNextActivation = () async =>
        throw PlatformException(code: 'rejected');
    await tester.tap(live);
    await tester.pumpAndSettle();
    final error = find.text(
      ExampleHostStrings.en[ExampleHostText.openMediaFailed],
    );
    expect(error, findsOneWidget);
    await tester.pump(const Duration(seconds: 5));
    await tester.pumpAndSettle();
    final pending = Completer<void>();
    platform.onNextActivation = () => pending.future;
    await tester.tap(live);
    await tester.pump();
    await tester.tap(live);
    await tester.pump();
    pending.complete();
    await tester.runAsync(() => Future<void>.delayed(Duration.zero));
    await tester.pumpAndSettle();
    expect(platform.playWhenReady, [false, true, true, true]);
    expect(find.byType(ExamplePlayerStage), findsOneWidget);
    expect(error, findsNothing);
    expect(tester.takeException(), isNull);
  });

  testWidgets('Cast sheet updates PiP and permission controls while open', (
    tester,
  ) async {
    await tester.pumpWidget(const VesperFlutterHostApp());
    await tester.pumpAndSettle();
    final cast = find.byKey(const ValueKey('quick-action-cast'));
    await tester.ensureVisible(cast);
    await tester.tap(cast);
    await tester.pumpAndSettle();
    final section = find.byType(ExampleSystemPlaybackSection);
    final sheet = ModalRoute.of(tester.element(section));
    final toggle = find.descendant(
      of: section,
      matching: find.byType(SwitchListTile),
    );
    expect(tester.widget<SwitchListTile>(toggle).value, isFalse);
    await tester.tap(toggle);
    await tester.pumpAndSettle();
    expect(tester.widget<SwitchListTile>(toggle).value, isTrue);
    final permission = find.textContaining('notRequired');
    await tester.ensureVisible(permission);
    await tester.tap(permission);
    await tester.pumpAndSettle();
    expect(find.textContaining('granted'), findsOneWidget);
    expect(ModalRoute.of(tester.element(section)), same(sheet));
    await tester.binding.handlePopRoute();
    await tester.pumpAndSettle();
    expect(section, findsNothing);
  });
}

final class _PausedPlayerPlatform extends VesperPlayerPlatform {
  final playWhenReady = <bool>[];
  int playerCount = 0;
  Future<void> Function()? onNextActivation;

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
  }) async => VesperPlatformCreateResult(
    playerId: 'player-${++playerCount}',
    snapshot: const VesperPlayerSnapshot.initial().copyWith(
      playbackState: VesperPlaybackState.paused,
    ),
  );

  @override
  Stream<VesperPlayerEvent> eventsFor(String playerId) => const Stream.empty();
  @override
  Future<String> createSourceSession(
    VesperSourceSessionConfiguration configuration,
  ) async => 'session';
  @override
  Future<VesperSourceReference> registerSource(
    String sessionId,
    VesperPlayerSource source, {
    int? expiresAtEpochMs,
  }) async => VesperSourceReference(sessionId: sessionId, sourceId: source.uri);
  @override
  Future<VesperSourceActivation> activateSource(
    String playerId,
    VesperSourceReference source,
    VesperSourceActivationOptions options,
  ) async {
    playWhenReady.add(options.playWhenReady);
    final before = onNextActivation;
    onNextActivation = null;
    if (before != null) await before();
    return VesperSourceActivation(
      activationId: playerId,
      source: source,
      playbackEpoch: 1,
    );
  }

  @override
  Future<void> disposeSourceSession(String sessionId) async {}
  @override
  Future<void> configureSystemPlayback(
    String playerId,
    VesperSystemPlaybackConfiguration configuration,
  ) async {}
  @override
  Future<void> setPictureInPictureConfiguration(
    String playerId,
    VesperPictureInPictureConfiguration configuration,
  ) async {}
  @override
  Future<VesperSystemPlaybackPermissionStatus>
  requestSystemPlaybackPermissions() async =>
      VesperSystemPlaybackPermissionStatus.granted;
  @override
  Future<void> updateViewport(
    String playerId,
    VesperPlayerViewport viewport,
  ) async {}
  @override
  Future<void> clearViewport(String playerId) async {}
  @override
  Future<void> dispose(String playerId) async {}
  @override
  dynamic noSuchMethod(Invocation invocation) => super.noSuchMethod(invocation);
}
