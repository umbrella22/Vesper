import 'dart:async';
import 'dart:io';
import 'dart:typed_data';

import 'package:flutter_test/flutter_test.dart';
import 'package:integration_test/integration_test.dart';
import 'package:vesper_player/vesper_player.dart';

import 'support/subtitle_media_fixture.dart';

void main() {
  final binding = IntegrationTestWidgetsFlutterBinding.ensureInitialized();

  testWidgets(
    'source replacement preserves pause, seek completion, and obsolete isolation',
    (WidgetTester tester) async {
      await tester.runAsync(() async {
        final directory = await Directory.systemTemp.createTemp(
          'vesper-command-contract-',
        );
        final fixtureBytes = decodeTinyAacFixture();
        final initialFile = await _writeFixture(
          directory,
          'initial.m4a',
          fixtureBytes,
        );
        final currentFile = await _writeFixture(
          directory,
          'current.m4a',
          fixtureBytes,
        );
        final server = await _DelayedMediaServer.start(<String, Uint8List>{
          '/loading.m4a': fixtureBytes,
          '/obsolete.m4a': fixtureBytes,
        });
        VesperPlayerController? controller;

        try {
          controller = await VesperPlayerController.create(
            initialSource: _localSource(initialFile, 'Initial source'),
          );
          await controller.initialize().timeout(const Duration(seconds: 10));
          await controller.pause().timeout(const Duration(seconds: 5));
          await _waitForSnapshot(
            controller,
            (snapshot) =>
                snapshot.sourceLabel == 'Initial source' &&
                (snapshot.timeline.durationMs ?? 0) > 0,
          );

          final loadingSelection = controller.selectSource(
            _remoteSource(server.uri('/loading.m4a'), 'Loading source'),
          );
          await server.waitForRequest('/loading.m4a');
          await controller.pause().timeout(const Duration(seconds: 5));
          server.release('/loading.m4a');
          await loadingSelection.timeout(const Duration(seconds: 10));

          final loadingSettled = await _waitForSnapshot(
            controller,
            (snapshot) =>
                snapshot.sourceLabel == 'Loading source' &&
                !snapshot.isBuffering &&
                (snapshot.timeline.durationMs ?? 0) > 0,
          );
          expect(loadingSettled.playbackState, VesperPlaybackState.paused);
          expect(loadingSettled.lastError, isNull);

          final obsoleteFailure = _playerCommandFailure(
            controller.selectSource(
              _remoteSource(server.uri('/obsolete.m4a'), 'Obsolete source'),
            ),
          );
          await server.waitForRequest('/obsolete.m4a');

          await Future.wait<void>(<Future<void>>[
            controller.selectSource(
              _localSource(currentFile, 'Current source'),
            ),
            controller.pause(),
          ]).timeout(const Duration(seconds: 10));
          final obsoleteError = await obsoleteFailure.timeout(
            const Duration(seconds: 5),
          );
          expect(obsoleteError.isObsolete, isTrue);
          expect(obsoleteError.details['commandId'], isNotNull);
          expect(obsoleteError.details['sourceEpoch'], isNotNull);
          server.release('/obsolete.m4a');

          final currentSettled = await _waitForSnapshot(
            controller,
            (snapshot) =>
                snapshot.sourceLabel == 'Current source' &&
                snapshot.playbackState != VesperPlaybackState.playing &&
                !snapshot.isBuffering &&
                (snapshot.timeline.durationMs ?? 0) > 0,
          );
          expect(currentSettled.lastError, isNull);

          await controller.seekToRatio(0.5).timeout(const Duration(seconds: 5));
          final seekSettled = await _waitForSnapshot(controller, (snapshot) {
            final durationMs = snapshot.timeline.durationMs ?? 0;
            if (durationMs <= 0) return false;
            final ratio = snapshot.timeline.positionMs / durationMs;
            return snapshot.sourceLabel == 'Current source' &&
                ratio >= 0.35 &&
                ratio <= 0.65;
          });
          final positionAfterSeek = seekSettled.timeline.positionMs;
          await Future<void>.delayed(const Duration(milliseconds: 400));
          await controller.refresh().timeout(const Duration(seconds: 5));
          final finalSnapshot = controller.snapshot;

          expect(finalSnapshot.sourceLabel, 'Current source');
          expect(
            finalSnapshot.playbackState,
            isNot(VesperPlaybackState.playing),
          );
          expect(finalSnapshot.lastError, isNull);
          expect(
            (finalSnapshot.timeline.positionMs - positionAfterSeek).abs(),
            lessThanOrEqualTo(200),
          );

          binding.reportData = <String, dynamic>{
            'evidenceName': 'command-contract',
            'platform': Platform.operatingSystem,
            'loadingPause': _snapshotEvidence(loadingSettled),
            'obsoleteCommand': <String, Object?>{
              'code': obsoleteError.codeRawValue ?? obsoleteError.code.name,
              'category':
                  obsoleteError.categoryRawValue ?? obsoleteError.category.name,
              'details': obsoleteError.details,
            },
            'seekCompletion': _snapshotEvidence(finalSnapshot),
          };
        } finally {
          await controller?.dispose();
          await server.close();
          if (await directory.exists()) {
            await directory.delete(recursive: true);
          }
        }
      });
    },
  );
}

VesperPlayerSource _localSource(File file, String label) {
  return VesperPlayerSource.local(uri: file.uri.toString(), label: label);
}

VesperPlayerSource _remoteSource(Uri uri, String label) {
  return VesperPlayerSource.remote(
    uri: uri.toString(),
    label: label,
    protocol: VesperPlayerSourceProtocol.progressive,
  );
}

Future<File> _writeFixture(
  Directory directory,
  String name,
  Uint8List bytes,
) async {
  final file = File('${directory.path}/$name');
  await file.writeAsBytes(bytes, flush: true);
  return file;
}

Future<VesperPlayerCommandException> _playerCommandFailure(
  Future<void> command,
) async {
  try {
    await command;
    throw AssertionError('Expected the player command to become obsolete.');
  } on VesperPlayerCommandException catch (error) {
    return error;
  }
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
    'player command snapshot did not converge: ${_snapshotEvidence(snapshot)}',
    timeout,
  );
}

Map<String, Object?> _snapshotEvidence(VesperPlayerSnapshot snapshot) {
  return <String, Object?>{
    'sourceLabel': snapshot.sourceLabel,
    'playbackState': snapshot.playbackState.name,
    'positionMs': snapshot.timeline.positionMs,
    'durationMs': snapshot.timeline.durationMs,
    'isBuffering': snapshot.isBuffering,
    'lastError': snapshot.lastError?.code.name,
  };
}

final class _DelayedMediaServer {
  _DelayedMediaServer(this._server, this._responses) {
    _subscription = _server.listen(_handleRequest);
  }

  final HttpServer _server;
  final Map<String, _DelayedMediaResponse> _responses;
  late final StreamSubscription<HttpRequest> _subscription;

  static Future<_DelayedMediaServer> start(
    Map<String, Uint8List> responses,
  ) async {
    final server = await HttpServer.bind(InternetAddress.loopbackIPv4, 0);
    return _DelayedMediaServer(server, <String, _DelayedMediaResponse>{
      for (final entry in responses.entries)
        entry.key: _DelayedMediaResponse(entry.value),
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
      _writeMediaResponse(request, delayed.body);
      await request.response.close();
    } on HttpException {
      // Superseding the source may close the loopback request before release.
    } on SocketException {
      // Superseding the source may close the loopback request before release.
    }
  }
}

void _writeMediaResponse(HttpRequest request, Uint8List body) {
  final response = request.response;
  response.headers.contentType = ContentType('audio', 'mp4');
  response.headers.set(HttpHeaders.acceptRangesHeader, 'bytes');
  final range = _parseByteRange(
    request.headers.value(HttpHeaders.rangeHeader),
    body.lengthInBytes,
  );
  if (range == null) {
    response.statusCode = HttpStatus.ok;
    response.contentLength = body.lengthInBytes;
    if (request.method != 'HEAD') response.add(body);
    return;
  }

  response.statusCode = HttpStatus.partialContent;
  response.headers.set(
    HttpHeaders.contentRangeHeader,
    'bytes ${range.start}-${range.end}/${body.lengthInBytes}',
  );
  response.contentLength = range.end - range.start + 1;
  if (request.method != 'HEAD') {
    response.add(body.sublist(range.start, range.end + 1));
  }
}

_ByteRange? _parseByteRange(String? value, int length) {
  if (value == null) return null;
  final match = RegExp(r'^bytes=(\d+)-(\d*)$').firstMatch(value.trim());
  if (match == null) return null;
  final start = int.parse(match.group(1)!);
  if (start >= length) return null;
  final requestedEnd = match.group(2)!.isEmpty
      ? length - 1
      : int.parse(match.group(2)!);
  return _ByteRange(start, requestedEnd.clamp(start, length - 1));
}

final class _ByteRange {
  const _ByteRange(this.start, this.end);

  final int start;
  final int end;
}

final class _DelayedMediaResponse {
  _DelayedMediaResponse(this.body);

  final Uint8List body;
  final Completer<void> requested = Completer<void>();
  final Completer<void> released = Completer<void>();
}
