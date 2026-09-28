import 'package:flutter_test/flutter_test.dart';
import 'package:vesper_player_platform_interface/vesper_player_platform_interface.dart';

void main() {
  test('older hosts omit observations and snapshot copies retain or clear them',
      () {
    final old = VesperPlayerSnapshot.fromMap({'title': 'old host'});
    expect(old.playbackDiagnostics, isNull);
    const observation = VesperFirstFrameObservation(
      playbackEpoch: 2,
      elapsedSinceLoadStartMs: 125,
      kind: VesperFirstFrameObservationKind.media3RenderedFirstFrame,
    );
    final value = old.copyWith(
        playbackDiagnostics: const VesperPlaybackDiagnosticsSnapshot(
      playbackEpoch: 2,
      firstFrame: observation,
    ));
    expect(value.copyWith(title: 'changed').playbackDiagnostics,
        same(value.playbackDiagnostics));
    final decoded = VesperPlayerSnapshot.fromMap(value.toMap());
    expect(
        decoded.playbackDiagnostics!.firstFrame!.elapsedSinceLoadStartMs, 125);
    expect(value.copyWith(clearPlaybackDiagnostics: true).playbackDiagnostics,
        isNull);
  });

  test('native first-frame and recoverable audio warning decode independently',
      () {
    final event = VesperPlayerEvent.fromMap({
      'playerId': 'p',
      'type': 'firstFrame',
      'observation': {
        'playbackEpoch': 3,
        'elapsedSinceLoadStartMs': 125,
        'mediaPositionMs': 90000,
        'kind': 'avPlayerLayerReadyForDisplay'
      },
    }) as VesperPlayerFirstFrameEvent;
    expect(event.observation.mediaPositionMs, 90000);
    final warning = VesperRuntimeWarning.fromMap({
      'domain': 'audio',
      'audio': {
        'playbackEpoch': 3,
        'audio': {
          'evidence': 'runtimeFormat',
          'codec': 'ec-3',
          'lastIssue': {
            'kind': 'sinkError',
            'elapsedSinceLoadStartMs': 800,
            'platformCode': 'write'
          },
        }
      },
    });
    expect(warning.domain, VesperRuntimeWarningDomain.audio);
    final forwarded = VesperRuntimeWarning.fromMap(warning.toMap()).audio!;
    expect(forwarded.playbackEpoch, 3);
    expect(forwarded.diagnostics.codec, 'ec-3');
    expect(forwarded.diagnostics.lastIssue!.kind,
        VesperAudioDiagnosticIssueKind.sinkError);
  });

  test('missing or lossy duration cannot become a zero-millisecond first frame',
      () {
    for (final invalid in [null, -1, 2.5, double.nan, '100']) {
      expect(
          () => VesperFirstFrameObservation.fromMap({
                'playbackEpoch': 1,
                'elapsedSinceLoadStartMs': invalid,
                'kind': 'media3RenderedFirstFrame',
              }),
          throwsFormatException);
    }
  });

  test('error audio context does not change generic failure attribution', () {
    final error = VesperPlayerError.fromMap({
      'message': 'failed',
      'code': 'backendFailure',
      'category': 'playback',
      'details': {
        'playbackDiagnostics': {
          'playbackEpoch': 8,
          'audio': {'codec': 'ec-3'}
        }
      },
    });
    expect(error.category, VesperPlayerErrorCategory.playback);
    expect(error.playbackDiagnostics!.audio.codec, 'ec-3');
    expect(error.playbackDiagnostics!.playbackEpoch, 8);
  });
  test('unknown native evidence survives forwarding without becoming success',
      () {
    final value = VesperPlaybackDiagnosticsSnapshot.fromMap({
      'playbackEpoch': 4,
      'audio': {
        'evidence': 'futureAudioObserver',
        'lastIssue': {
          'kind': 'futureAudioIssue',
          'elapsedSinceLoadStartMs': 920,
        },
      },
      'firstFrame': {
        'playbackEpoch': 4,
        'kind': 'futureVideoObserver',
        'elapsedSinceLoadStartMs': 120,
        'mediaPositionMs': 90000,
      },
    });
    expect(value.audio.evidence, VesperAudioDiagnosticEvidence.unknown);
    expect(value.audio.decoderName, isNull);
    expect(value.audio.channels, isNull);
    expect(value.audio.lastIssue!.kind, VesperAudioDiagnosticIssueKind.unknown);
    expect(value.firstFrame!.kind, VesperFirstFrameObservationKind.unknown);
    expect(value.firstFrame!.elapsedSinceLoadStartMs, 120);
    expect(value.firstFrame!.mediaPositionMs, 90000);
    final forwarded = VesperPlaybackDiagnosticsSnapshot.fromMap(value.toMap());
    expect(forwarded.audio.evidenceRawValue, 'futureAudioObserver');
    expect(forwarded.audio.lastIssue!.kindRawValue, 'futureAudioIssue');
    expect(forwarded.firstFrame!.kindRawValue, 'futureVideoObserver');
  });

  test('an attempt without observations carries no inferred output evidence',
      () {
    final value =
        VesperPlaybackDiagnosticsSnapshot.fromMap({'playbackEpoch': 2});
    expect(value.firstFrame, isNull);
    expect(value.audio.evidence, VesperAudioDiagnosticEvidence.unknown);
    expect(value.audio.trackId, isNull);
    expect(value.audio.decoderName, isNull);
    expect(value.audio.lastIssue, isNull);
    expect(value.toMap()['firstFrame'], isNull);
  });
}
