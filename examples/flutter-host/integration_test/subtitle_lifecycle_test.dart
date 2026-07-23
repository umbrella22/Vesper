import 'dart:async';
import 'dart:io';

import 'package:flutter/foundation.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:integration_test/integration_test.dart';
import 'package:vesper_player/vesper_player.dart';

import 'support/subtitle_media_fixture.dart';

void main() {
  final binding = IntegrationTestWidgetsFlutterBinding.ensureInitialized();
  final scenarios = <String, Object?>{};

  void publishEvidence() {
    binding.reportData = <String, dynamic>{
      'evidenceName': 'subtitle-lifecycle',
      'scenarios': scenarios,
    };
  }

  testWidgets('late WebVTT completion cannot commit after the real timeout', (
    WidgetTester tester,
  ) async {
    await tester.runAsync(() async {
      final server = await _DelayedWebVttServer.start(<String, String>{
        '/timeout.vtt': _webVtt('Late timeout subtitle'),
      });
      final fixture = await _SubtitleLifecycleFixture.create();
      VesperPlayerController? controller;
      try {
        controller = await VesperPlayerController.create(
          initialSource: fixture.source(<VesperExternalSubtitleSource>[
            fixture.remoteTrack(
              id: 'timeout-track',
              uri: server.uri('/timeout.vtt'),
            ),
          ]),
        );
        await controller.initialize().timeout(const Duration(seconds: 10));
        await server.waitForRequest('/timeout.vtt');

        FlutterErrorDetails? reportedError;
        final previousErrorHandler = FlutterError.onError;
        FlutterError.onError = (FlutterErrorDetails details) {
          reportedError = details;
        };
        final stopwatch = Stopwatch()..start();
        late VesperSubtitleException error;
        try {
          error = await _subtitleFailure(
            controller.setSubtitleTrackSelection(
              const VesperTrackSelection.track('timeout-track'),
            ),
          );
        } finally {
          stopwatch.stop();
          FlutterError.onError = previousErrorHandler;
        }

        expect(error.code, 'subtitle_selection_timeout');
        expect(error.commandId, isNotNull);
        expect(error.commandId!, greaterThan(0));
        expect(error.sourceEpoch, 0);
        expect(
          stopwatch.elapsed,
          greaterThanOrEqualTo(const Duration(milliseconds: 2800)),
        );
        expect(stopwatch.elapsed, lessThan(const Duration(seconds: 8)));
        expect(reportedError?.exception, isA<VesperSubtitleException>());

        server.release('/timeout.vtt');
        final snapshot = await _waitForSnapshot(
          controller,
          (value) =>
              value.subtitleState.catalogState ==
                  VesperSubtitleCatalogState.ready &&
              value.trackCatalog.subtitleTracks.any(
                (track) => track.id == 'timeout-track',
              ),
        );
        await Future<void>.delayed(const Duration(milliseconds: 300));
        final settled = controller.snapshot;
        expect(
          snapshot.trackSelection.confirmedSubtitle.mode,
          VesperTrackSelectionMode.disabled,
        );
        expect(
          settled.trackSelection.confirmedSubtitle.mode,
          VesperTrackSelectionMode.disabled,
        );
        expect(settled.trackSelection.effectiveSubtitleTrackId, isNull);
        expect(
          settled.subtitleState.selectionError?.code,
          'subtitle_selection_timeout',
        );
        expect(
          settled.subtitleState.selectionError?.commandId,
          error.commandId,
        );
        expect(
          settled.subtitleState.selectionError?.sourceEpoch,
          error.sourceEpoch,
        );

        scenarios['timeout'] = <String, Object?>{
          'error': _exceptionEvidence(error),
          'elapsedMs': stopwatch.elapsedMilliseconds,
          'settled': _snapshotEvidence(settled),
        };
        publishEvidence();
      } finally {
        await controller?.dispose();
        await fixture.dispose();
        await server.close();
      }
    });
  });

  testWidgets('source switch owns state after a pending subtitle command', (
    WidgetTester tester,
  ) async {
    await tester.runAsync(() async {
      final server = await _DelayedWebVttServer.start(<String, String>{
        '/source-a.vtt': _webVtt('Obsolete source A'),
      });
      final fixture = await _SubtitleLifecycleFixture.create();
      VesperPlayerController? controller;
      try {
        controller = await VesperPlayerController.create(
          initialSource: fixture.source(<VesperExternalSubtitleSource>[
            fixture.remoteTrack(
              id: 'source-a-track',
              uri: server.uri('/source-a.vtt'),
            ),
          ]),
        );
        await controller.initialize().timeout(const Duration(seconds: 10));
        await server.waitForRequest('/source-a.vtt');

        final oldFailure = _subtitleFailure(
          controller.setSubtitleTrackSelection(
            const VesperTrackSelection.track('source-a-track'),
          ),
        );
        await _waitForSnapshot(
          controller,
          (value) =>
              value.trackSelection.subtitle.trackId == 'source-a-track' &&
              value.subtitleState.selectionState ==
                  VesperSubtitleSelectionState.applying,
        );

        final sourceBTrack = await fixture.localTrack(
          id: 'source-b-track',
          fileName: 'source-b.vtt',
          text: 'Current source B',
        );
        await controller
            .selectSource(
              fixture.source(<VesperExternalSubtitleSource>[sourceBTrack]),
            )
            .timeout(const Duration(seconds: 10));
        final error = await oldFailure;
        expect(error.code, 'subtitle_source_changed');
        expect(error.commandId, isNotNull);
        expect(error.commandId!, greaterThan(0));
        expect(error.sourceEpoch, 0);

        await _waitForSnapshot(
          controller,
          (value) =>
              value.subtitleState.catalogState ==
                  VesperSubtitleCatalogState.ready &&
              value.trackCatalog.subtitleTracks.any(
                (track) => track.id == 'source-b-track',
              ),
        );
        await controller
            .setSubtitleTrackSelection(
              const VesperTrackSelection.track('source-b-track'),
            )
            .timeout(const Duration(seconds: 5));
        server.release('/source-a.vtt');
        await Future<void>.delayed(const Duration(milliseconds: 500));

        final settled = controller.snapshot;
        expect(
          settled.trackSelection.confirmedSubtitle.trackId,
          'source-b-track',
        );
        expect(
          settled.trackSelection.effectiveSubtitleTrackId,
          'source-b-track',
        );
        expect(
          settled.subtitleState.selectionState,
          VesperSubtitleSelectionState.confirmed,
        );
        expect(settled.subtitleState.selectionError, isNull);
        expect(settled.lastError, isNull);

        scenarios['sourceChange'] = <String, Object?>{
          'error': _exceptionEvidence(error),
          'settled': _snapshotEvidence(settled),
        };
        publishEvidence();
      } finally {
        await controller?.dispose();
        await fixture.dispose();
        await server.close();
      }
    });
  });

  testWidgets('new subtitle command supersedes A and B alone commits', (
    WidgetTester tester,
  ) async {
    await tester.runAsync(() async {
      final server = await _DelayedWebVttServer.start(<String, String>{
        '/supersede-a.vtt': _webVtt('Subtitle A'),
      });
      final fixture = await _SubtitleLifecycleFixture.create();
      VesperPlayerController? controller;
      try {
        final trackB = await fixture.localTrack(
          id: 'supersede-b',
          fileName: 'supersede-b.vtt',
          text: 'Subtitle B',
        );
        controller = await VesperPlayerController.create(
          initialSource: fixture.source(<VesperExternalSubtitleSource>[
            fixture.remoteTrack(
              id: 'supersede-a',
              uri: server.uri('/supersede-a.vtt'),
            ),
            trackB,
          ]),
        );
        await controller.initialize().timeout(const Duration(seconds: 10));
        await server.waitForRequest('/supersede-a.vtt');

        final firstFailure = _subtitleFailure(
          controller.setSubtitleTrackSelection(
            const VesperTrackSelection.track('supersede-a'),
          ),
        );
        await _waitForSnapshot(
          controller,
          (value) =>
              value.trackSelection.subtitle.trackId == 'supersede-a' &&
              value.subtitleState.selectionState ==
                  VesperSubtitleSelectionState.applying,
        );
        final secondSelection = controller.setSubtitleTrackSelection(
          const VesperTrackSelection.track('supersede-b'),
        );

        final error = await firstFailure;
        server.release('/supersede-a.vtt');
        await secondSelection.timeout(const Duration(seconds: 10));
        await Future<void>.delayed(const Duration(milliseconds: 300));

        expect(error.code, 'subtitle_selection_superseded');
        expect(error.commandId, isNotNull);
        expect(error.commandId!, greaterThan(0));
        expect(error.sourceEpoch, 0);

        final settled = controller.snapshot;
        expect(settled.trackSelection.subtitle.trackId, 'supersede-b');
        expect(settled.trackSelection.confirmedSubtitle.trackId, 'supersede-b');
        expect(settled.trackSelection.effectiveSubtitleTrackId, 'supersede-b');
        expect(
          settled.subtitleState.selectionState,
          VesperSubtitleSelectionState.confirmed,
        );
        expect(settled.subtitleState.selectionError, isNull);
        expect(settled.lastError, isNull);

        scenarios['supersede'] = <String, Object?>{
          'error': _exceptionEvidence(error),
          'settled': _snapshotEvidence(settled),
        };
        publishEvidence();
      } finally {
        await controller?.dispose();
        await fixture.dispose();
        await server.close();
      }
    });
  });
}

Future<VesperSubtitleException> _subtitleFailure(Future<void> operation) async {
  try {
    await operation;
  } catch (error) {
    expect(error, isA<VesperSubtitleException>());
    return error as VesperSubtitleException;
  }
  fail('Expected a subtitle command failure.');
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
    'subtitle lifecycle snapshot did not converge: '
    '${_snapshotEvidence(snapshot)}',
    timeout,
  );
}

Map<String, Object?> _exceptionEvidence(VesperSubtitleException error) {
  return <String, Object?>{
    'code': error.code,
    'phase': error.phaseRawValue ?? error.phase.name,
    'trackId': error.trackId,
    'retriable': error.retriable,
    'message': error.message,
    'commandId': error.commandId,
    'sourceEpoch': error.sourceEpoch,
  };
}

Map<String, Object?> _snapshotEvidence(VesperPlayerSnapshot snapshot) {
  return <String, Object?>{
    'requestedMode': snapshot.trackSelection.subtitle.mode.name,
    'requestedTrackId': snapshot.trackSelection.subtitle.trackId,
    'confirmedMode': snapshot.trackSelection.confirmedSubtitle.mode.name,
    'confirmedTrackId': snapshot.trackSelection.confirmedSubtitle.trackId,
    'effectiveTrackId': snapshot.trackSelection.effectiveSubtitleTrackId,
    'catalogState': snapshot.subtitleState.catalogState.name,
    'selectionState': snapshot.subtitleState.selectionState.name,
    'selectionError': snapshot.subtitleState.selectionError?.toMap(),
    'lastError': snapshot.lastError?.code.name,
  };
}

String _webVtt(String text) {
  return 'WEBVTT\n\n00:00:00.000 --> 00:01:00.000\n$text\n';
}

final class _SubtitleLifecycleFixture {
  _SubtitleLifecycleFixture(this.directory, this.media);

  final Directory directory;
  final File media;

  static Future<_SubtitleLifecycleFixture> create() async {
    final directory = await Directory.systemTemp.createTemp(
      'vesper-subtitle-lifecycle-',
    );
    final media = await writeTinyAacFixture(directory);
    return _SubtitleLifecycleFixture(directory, media);
  }

  VesperPlayerSource source(List<VesperExternalSubtitleSource> subtitles) {
    return VesperPlayerSource.local(
      uri: media.uri.toString(),
      label: 'Subtitle lifecycle fixture',
      externalSubtitles: subtitles,
    );
  }

  VesperExternalSubtitleSource remoteTrack({
    required String id,
    required Uri uri,
  }) {
    return VesperExternalSubtitleSource(
      id: id,
      uri: uri.toString(),
      mimeType: VesperExternalSubtitleSource.mimeWebvtt,
      language: 'en',
      label: id,
    );
  }

  Future<VesperExternalSubtitleSource> localTrack({
    required String id,
    required String fileName,
    required String text,
  }) async {
    final file = File('${directory.path}/$fileName');
    await file.writeAsString(_webVtt(text), flush: true);
    return VesperExternalSubtitleSource(
      id: id,
      uri: file.uri.toString(),
      mimeType: VesperExternalSubtitleSource.mimeWebvtt,
      language: 'en',
      label: id,
    );
  }

  Future<void> dispose() async {
    if (await directory.exists()) {
      await directory.delete(recursive: true);
    }
  }
}

final class _DelayedWebVttServer {
  _DelayedWebVttServer(this._server, this._responses) {
    _subscription = _server.listen(_handleRequest);
  }

  final HttpServer _server;
  final Map<String, _DelayedWebVttResponse> _responses;
  late final StreamSubscription<HttpRequest> _subscription;

  static Future<_DelayedWebVttServer> start(
    Map<String, String> responses,
  ) async {
    final server = await HttpServer.bind(InternetAddress.loopbackIPv4, 0);
    return _DelayedWebVttServer(server, <String, _DelayedWebVttResponse>{
      for (final entry in responses.entries)
        entry.key: _DelayedWebVttResponse(entry.value),
    });
  }

  Uri uri(String path) => Uri.parse('http://127.0.0.1:${_server.port}$path');

  Future<void> waitForRequest(
    String path, {
    Duration timeout = const Duration(seconds: 10),
  }) {
    return _responses[path]!.requested.future.timeout(timeout);
  }

  void release(String path) {
    final response = _responses[path]!;
    if (!response.released.isCompleted) {
      response.released.complete();
    }
  }

  Future<void> close() async {
    for (final response in _responses.values) {
      if (!response.released.isCompleted) {
        response.released.complete();
      }
    }
    await _server.close(force: true);
    await _subscription.cancel();
  }

  Future<void> _handleRequest(HttpRequest request) async {
    final delayed = _responses[request.uri.path];
    if (delayed == null) {
      request.response.statusCode = HttpStatus.notFound;
      await request.response.close();
      return;
    }
    if (!delayed.requested.isCompleted) {
      delayed.requested.complete();
    }
    await delayed.released.future;
    try {
      request.response.statusCode = HttpStatus.ok;
      request.response.headers.contentType = ContentType(
        'text',
        'vtt',
        charset: 'utf-8',
      );
      request.response.write(delayed.body);
      await request.response.close();
    } on HttpException {
      // Source changes may cancel the client request before the response is released.
    } on SocketException {
      // Source changes may close the loopback connection before the response is released.
    }
  }
}

final class _DelayedWebVttResponse {
  _DelayedWebVttResponse(this.body);

  final String body;
  final Completer<void> requested = Completer<void>();
  final Completer<void> released = Completer<void>();
}
