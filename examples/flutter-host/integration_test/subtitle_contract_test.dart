import 'dart:async';
import 'dart:io';

import 'package:flutter/services.dart';
import 'package:flutter/widgets.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:integration_test/integration_test.dart';
import 'package:vesper_player/vesper_player.dart';

const String _subtitleAId = 'external-a';
const String _subtitleBId = 'external-b';

void main() {
  IntegrationTestWidgetsFlutterBinding.ensureInitialized();

  testWidgets('subtitle contract converges through the native host kit', (
    WidgetTester tester,
  ) async {
    Directory? directory;
    VesperPlayerController? controller;
    late VesperPlayerSource Function(List<String> orderedIds) source;
    try {
      await tester.runAsync(() async {
        directory = await Directory.systemTemp.createTemp(
          'vesper-subtitle-contract-',
        );
        final fixtureDirectory = directory!;
        final media = File('${fixtureDirectory.path}/tiny-aac.m4a');
        final mediaBytes = await rootBundle.load(
          'assets/subtitle_contract/tiny-aac.m4a',
        );
        await media.writeAsBytes(
          mediaBytes.buffer.asUint8List(
            mediaBytes.offsetInBytes,
            mediaBytes.lengthInBytes,
          ),
          flush: true,
        );
        final subtitleA = await _writeWebVtt(
          fixtureDirectory,
          fileName: 'a.vtt',
          cue: 'Subtitle A',
        );
        final subtitleB = await _writeWebVtt(
          fixtureDirectory,
          fileName: 'b.vtt',
          cue: 'Subtitle B',
        );

        source = (List<String> orderedIds) {
          final byId = <String, VesperExternalSubtitleSource>{
            _subtitleAId: VesperExternalSubtitleSource(
              id: _subtitleAId,
              uri: subtitleA.uri.toString(),
              mimeType: VesperExternalSubtitleSource.mimeWebvtt,
              language: 'en',
              label: 'English A',
            ),
            _subtitleBId: VesperExternalSubtitleSource(
              id: _subtitleBId,
              uri: subtitleB.uri.toString(),
              mimeType: VesperExternalSubtitleSource.mimeWebvtt,
              language: 'zh',
              label: 'Chinese B',
              isDefault: true,
            ),
          };
          return VesperPlayerSource.local(
            uri: media.uri.toString(),
            label: 'Subtitle contract fixture',
            externalSubtitles: orderedIds
                .map((String id) => byId[id]!)
                .toList(),
          );
        };

        controller = await VesperPlayerController.create(
          initialSource: source(<String>[_subtitleAId, _subtitleBId]),
        );
      });

      final activeController = controller!;
      await tester.pumpWidget(
        Directionality(
          textDirection: TextDirection.ltr,
          child: SizedBox(
            width: 320,
            height: 180,
            child: VesperPlayerView(controller: activeController),
          ),
        ),
      );
      await tester.pump();
      await tester.runAsync(() async {
        await Future<void>.delayed(const Duration(milliseconds: 250));
      });
      await tester.pump();

      await tester.runAsync(() async {
        await activeController.initialize().timeout(
          const Duration(seconds: 10),
        );
        var snapshot = await _waitForSnapshot(
          activeController,
          (VesperPlayerSnapshot value) =>
              value.subtitleState.catalogState ==
                  VesperSubtitleCatalogState.ready &&
              value.trackCatalog.subtitleTracks
                  .map((VesperMediaTrack track) => track.id)
                  .toSet()
                  .containsAll(<String>{_subtitleAId, _subtitleBId}),
        );
        expect(snapshot.subtitleState.advertisedTrackCount, 2);
        expect(snapshot.subtitleState.selectableTrackCount, 2);

        await activeController
            .setSubtitleTrackSelection(
              const VesperTrackSelection.track(_subtitleBId),
            )
            .timeout(const Duration(seconds: 5));
        snapshot = await _waitForSnapshot(
          activeController,
          (VesperPlayerSnapshot value) =>
              value.trackSelection.confirmedSubtitle.trackId == _subtitleBId &&
              value.subtitleState.selectionState ==
                  VesperSubtitleSelectionState.confirmed,
        );
        expect(snapshot.trackSelection.subtitle.trackId, _subtitleBId);

        final playbackErrors = <Object>[];
        final playbackErrorSubscription = activeController.events.listen((
          event,
        ) {
          if (event is VesperPlayerErrorEvent) {
            playbackErrors.add(event.error);
          }
        });
        try {
          await activeController.play().timeout(const Duration(seconds: 10));
          snapshot = await _waitForSnapshot(
            activeController,
            (VesperPlayerSnapshot value) =>
                value.trackSelection.effectiveSubtitleTrackId == _subtitleBId &&
                (value.playbackState == VesperPlaybackState.playing ||
                    value.timeline.positionMs > 0 ||
                    value.playbackState == VesperPlaybackState.finished ||
                    value.lastError != null),
          );
          snapshot = await _waitForSnapshot(
            activeController,
            (VesperPlayerSnapshot value) =>
                value.playbackState == VesperPlaybackState.finished ||
                value.lastError != null,
          );
          expect(
            playbackErrors,
            isEmpty,
            reason: 'external WebVTT playback emitted a terminal player error',
          );
          expect(
            snapshot.lastError,
            isNull,
            reason: 'external WebVTT playback left a terminal player error',
          );
          expect(
            snapshot.playbackState,
            VesperPlaybackState.finished,
            reason: 'external WebVTT playback did not finish cleanly',
          );
        } finally {
          await playbackErrorSubscription.cancel();
        }

        final confirmedBeforeFailure =
            snapshot.trackSelection.confirmedSubtitle;
        final effectiveBeforeFailure =
            snapshot.trackSelection.effectiveSubtitleTrackId;
        final catalogIdsBeforeFailure = snapshot.trackCatalog.subtitleTracks
            .map((VesperMediaTrack track) => track.id)
            .toList();
        final advertisedBeforeFailure =
            snapshot.subtitleState.advertisedTrackCount;
        final selectableBeforeFailure =
            snapshot.subtitleState.selectableTrackCount;
        Object? invalidError;
        FlutterErrorDetails? reportedInvalidError;
        final previousFlutterErrorHandler = FlutterError.onError;
        FlutterError.onError = (FlutterErrorDetails details) {
          reportedInvalidError = details;
        };
        try {
          try {
            await activeController
                .setSubtitleTrackSelection(
                  const VesperTrackSelection.track('missing-subtitle'),
                )
                .timeout(const Duration(seconds: 5));
          } catch (error) {
            invalidError = error;
          }
        } finally {
          FlutterError.onError = previousFlutterErrorHandler;
        }
        expect(invalidError, isA<VesperSubtitleException>());
        expect(reportedInvalidError?.exception, isA<VesperSubtitleException>());
        expect(
          (invalidError! as VesperSubtitleException).code,
          'subtitle_track_not_found',
        );
        snapshot = await _waitForSnapshot(
          activeController,
          (VesperPlayerSnapshot value) =>
              value.subtitleState.selectionState ==
                  VesperSubtitleSelectionState.failed &&
              value.subtitleState.selectionError?.code ==
                  'subtitle_track_not_found',
        );
        expect(
          snapshot.trackCatalog.subtitleTracks
              .map((VesperMediaTrack track) => track.id)
              .toList(),
          catalogIdsBeforeFailure,
        );
        expect(
          snapshot.subtitleState.advertisedTrackCount,
          advertisedBeforeFailure,
        );
        expect(
          snapshot.subtitleState.selectableTrackCount,
          selectableBeforeFailure,
        );
        expect(
          snapshot.trackSelection.confirmedSubtitle.mode,
          confirmedBeforeFailure.mode,
        );
        expect(
          snapshot.trackSelection.confirmedSubtitle.trackId,
          confirmedBeforeFailure.trackId,
        );
        expect(
          snapshot.trackSelection.effectiveSubtitleTrackId,
          effectiveBeforeFailure,
        );

        await activeController
            .setSubtitleTrackSelection(const VesperTrackSelection.disabled())
            .timeout(const Duration(seconds: 5));
        snapshot = await _waitForSnapshot(
          activeController,
          (VesperPlayerSnapshot value) =>
              value.trackSelection.confirmedSubtitle.mode ==
                  VesperTrackSelectionMode.disabled &&
              value.trackSelection.effectiveSubtitleTrackId == null &&
              value.subtitleState.selectionState ==
                  VesperSubtitleSelectionState.confirmed,
        );
        expect(
          snapshot.trackSelection.subtitle.mode,
          VesperTrackSelectionMode.disabled,
        );

        await activeController
            .setSubtitleTrackSelection(const VesperTrackSelection.auto())
            .timeout(const Duration(seconds: 5));
        snapshot = await _waitForSnapshot(
          activeController,
          (VesperPlayerSnapshot value) =>
              value.trackSelection.confirmedSubtitle.mode ==
                  VesperTrackSelectionMode.auto &&
              value.trackSelection.effectiveSubtitleTrackId == _subtitleBId,
        );
        expect(
          snapshot.trackSelection.subtitle.mode,
          VesperTrackSelectionMode.auto,
        );

        await activeController
            .selectSource(source(<String>[_subtitleBId, _subtitleAId]))
            .timeout(const Duration(seconds: 10));
        snapshot = await _waitForSubtitleIds(activeController, <String>{
          _subtitleAId,
          _subtitleBId,
        });
        expect(
          snapshot.trackCatalog.subtitleTracks
              .map((VesperMediaTrack track) => track.id)
              .toSet(),
          <String>{_subtitleAId, _subtitleBId},
        );

        await activeController
            .selectSource(source(<String>[_subtitleBId]))
            .timeout(const Duration(seconds: 10));
        snapshot = await _waitForSubtitleIds(activeController, <String>{
          _subtitleBId,
        });
        expect(
          snapshot.trackCatalog.subtitleTracks
              .map((VesperMediaTrack track) => track.id)
              .toList(),
          <String>[_subtitleBId],
        );
      });
    } finally {
      await tester.pumpWidget(const SizedBox.shrink());
      await tester.pump();
      await tester.runAsync(() async {
        await controller?.dispose();
        final fixtureDirectory = directory;
        if (fixtureDirectory != null && await fixtureDirectory.exists()) {
          await fixtureDirectory.delete(recursive: true);
        }
      });
    }
  });
}

Future<File> _writeWebVtt(
  Directory directory, {
  required String fileName,
  required String cue,
}) async {
  final file = File('${directory.path}/$fileName');
  await file.writeAsString(
    'WEBVTT\n\n00:00:00.000 --> 00:00:05.000\n$cue\n',
    flush: true,
  );
  return file;
}

Future<VesperPlayerSnapshot> _waitForSubtitleIds(
  VesperPlayerController controller,
  Set<String> expectedIds,
) {
  return _waitForSnapshot(
    controller,
    (VesperPlayerSnapshot value) =>
        value.subtitleState.catalogState == VesperSubtitleCatalogState.ready &&
        value.trackCatalog.subtitleTracks
            .map((VesperMediaTrack track) => track.id)
            .toSet()
            .difference(expectedIds)
            .isEmpty &&
        expectedIds
            .difference(
              value.trackCatalog.subtitleTracks
                  .map((VesperMediaTrack track) => track.id)
                  .toSet(),
            )
            .isEmpty,
  );
}

Future<VesperPlayerSnapshot> _waitForSnapshot(
  VesperPlayerController controller,
  bool Function(VesperPlayerSnapshot snapshot) predicate, {
  Duration timeout = const Duration(seconds: 10),
}) async {
  final deadline = DateTime.now().add(timeout);
  while (DateTime.now().isBefore(deadline)) {
    final snapshot = controller.snapshot;
    if (predicate(snapshot)) {
      return snapshot;
    }
    await Future<void>.delayed(const Duration(milliseconds: 50));
  }
  final snapshot = controller.snapshot;
  throw TimeoutException(
    'subtitle snapshot did not converge: '
    'playback=${snapshot.playbackState.name} '
    'positionMs=${snapshot.timeline.positionMs} '
    'durationMs=${snapshot.timeline.durationMs} '
    'buffering=${snapshot.isBuffering} '
    'lastError=${snapshot.lastError?.code.name} '
    'catalog=${snapshot.subtitleState.catalogState.name} '
    'selection=${snapshot.subtitleState.selectionState.name} '
    'advertised=${snapshot.subtitleState.advertisedTrackCount} '
    'selectable=${snapshot.subtitleState.selectableTrackCount} '
    'tracks=${snapshot.trackCatalog.subtitleTracks.map((track) => track.id).toList()}',
    timeout,
  );
}
