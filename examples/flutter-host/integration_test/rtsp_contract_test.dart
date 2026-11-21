import 'dart:io';

import 'package:flutter/widgets.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:integration_test/integration_test.dart';
import 'package:vesper_player/vesper_player.dart';

void main() {
  IntegrationTestWidgetsFlutterBinding.ensureInitialized();
  const uri = String.fromEnvironment('VESPER_RTSP_TEST_URI');
  const enabled = bool.fromEnvironment(
    'VESPER_RTSP_EXPECT_BUNDLED',
    defaultValue: true,
  );

  const expectAndroidAudio = bool.fromEnvironment('VESPER_RTSP_EXPECT_AUDIO');
  const requiredProgressMs = int.fromEnvironment(
    'VESPER_RTSP_TEST_PROGRESS_MS',
    defaultValue: 1500,
  );

  testWidgets('RTSP reaches the optional native host route', (tester) async {
    final controller = (await tester.runAsync(
      () => VesperPlayerController.create(
        initialSource: VesperPlayerSource.rtsp(uri: uri, label: 'RTSP fixture'),
        keepScreenOnDuringPlayback: false,
      ),
    ))!;
    try {
      await tester.pumpWidget(
        Directionality(
          textDirection: TextDirection.ltr,
          child: SizedBox.expand(
            child: VesperPlayerView(controller: controller),
          ),
        ),
      );
      await tester.runAsync(() async {
        if (!enabled) {
          const reason = 'rtsp_bridge_not_bundled';
          final previousErrorHandler = FlutterError.onError;
          FlutterError.onError = (details) {
            if (_failureReason(details.exception) == reason) {
              return;
            }
            previousErrorHandler?.call(details);
          };
          try {
            await expectLater(
              controller.initialize().timeout(const Duration(seconds: 12)),
              throwsA(isA<Object>().having(_failureReason, 'reason', reason)),
            );
          } finally {
            FlutterError.onError = previousErrorHandler;
          }
          await controller.refresh().timeout(const Duration(seconds: 2));
          expect(controller.snapshot.lastError?.details['reason'], reason);
          return;
        }
        await controller.initialize().timeout(const Duration(seconds: 32));
        await controller.play().timeout(const Duration(seconds: 5));
      });
      if (!enabled) return;
      expect(requiredProgressMs, greaterThan(0));
      final deadline = Duration(milliseconds: 26000 + requiredProgressMs);
      final watch = Stopwatch()..start();
      int? firstPosition;
      var advanced = false;
      while (watch.elapsed < deadline) {
        await tester.pump(const Duration(milliseconds: 100));
        await tester.runAsync(() async {
          await Future<void>.delayed(const Duration(milliseconds: 100));
          await controller.refresh().timeout(const Duration(seconds: 2));
        });
        final snapshot = controller.snapshot;
        expect(snapshot.lastError, isNull);
        if (snapshot.playbackDiagnostics?.firstFrame != null &&
            snapshot.playbackState == VesperPlaybackState.playing) {
          firstPosition ??= snapshot.timeline.positionMs;
          if (snapshot.timeline.positionMs >=
              firstPosition + requiredProgressMs) {
            advanced = true;
            break;
          }
        }
      }
      expect(
        advanced,
        isTrue,
        reason:
            'The native first frame and advancing position must both be observed.',
      );
      if (Platform.isAndroid && expectAndroidAudio) {
        final audio = controller.snapshot.playbackDiagnostics?.audio;
        expect(audio?.sampleRate, 48000);
        expect(audio?.channels, 1);
        expect(audio?.lastIssue, isNull);
      }
      expect(controller.snapshot.sourceLabel, 'RTSP fixture');
      expect(controller.snapshot.timeline.durationMs, isNull);
    } finally {
      await tester.pumpWidget(const SizedBox.shrink());
      await tester.runAsync(controller.dispose);
    }
  }, skip: uri.isEmpty || !(Platform.isAndroid || Platform.isIOS));
}

Object? _failureReason(Object error) => switch (error) {
  VesperPlayerCommandException(:final details) => details['reason'],
  VesperUnsupportedError(
    platformDetails: {'details': {'reason': final reason}},
  ) =>
    reason,
  _ => null,
};
