import 'package:flutter_test/flutter_test.dart';
import 'package:vesper_player_platform_interface/vesper_player_platform_interface.dart';

void main() {
  test('source references contain opaque identities and preserve expiration',
      () {
    final reference = VesperSourceReference.fromMap(<String, Object?>{
      'sessionId': 'session-a',
      'sourceId': 'source-b',
      'expiresAtEpochMs': 1000,
      'uri': 'https://media.test/?secret=one',
    });
    expect(reference.toMap(), <String, Object?>{
      'sessionId': 'session-a',
      'sourceId': 'source-b',
      'expiresAtEpochMs': 1000,
    });
    expect(
        reference,
        const VesperSourceReference(
            sessionId: 'session-a', sourceId: 'source-b'));
    expect(
        reference,
        isNot(const VesperSourceReference(
            sessionId: 'another', sourceId: 'source-b')));
  });

  test('unknown native observations retain their wire values', () {
    final snapshot = VesperSourcePreloadSnapshot.fromMap(<String, Object?>{
      'sessionId': 's',
      'sourceId': 'r',
      'taskId': 't',
      'status': 'future-status',
      'goal': 'future-goal',
      'reuse': 'future-reuse',
    });
    expect(snapshot.status, VesperSourcePreloadStatus.unknown);
    expect(snapshot.goal, VesperSourcePreloadGoal.unknown);
    expect(snapshot.reuse, VesperSourcePreloadReuse.unknown);
    expect(snapshot.rawStatus, 'future-status');
    expect(snapshot.rawGoal, 'future-goal');
    expect(snapshot.rawReuse, 'future-reuse');
    expect(snapshot.isTerminal, isFalse);
  });

  test('preload completion and cache reuse capability are independent', () {
    final snapshot = VesperSourcePreloadSnapshot.fromMap(<String, Object?>{
      'sessionId': 's',
      'sourceId': 'r',
      'taskId': 't',
      'status': 'completed',
      'goal': 'progressiveRange',
      'reuse': 'downloadOnly',
      'actualBytes': 64,
      'cacheHit': false,
    });
    expect(snapshot.isTerminal, isTrue);
    expect(snapshot.reuse, VesperSourcePreloadReuse.downloadOnly);
    expect(snapshot.cacheHit, isFalse);
  });

  test('invalid registration identities and counters fail at the boundary', () {
    expect(
        () => VesperSourceReference.fromMap(<String, Object?>{'sourceId': 'r'}),
        throwsFormatException);
    expect(
        () => VesperSourceReference.fromMap(<String, Object?>{
              'sessionId': 's',
              'sourceId': 'r',
              'expiresAtEpochMs': -1,
            }),
        throwsFormatException);
    expect(
        () => VesperSourceReference.fromMap(<String, Object?>{
              'sessionId': 's',
              'sourceId': 'r',
              'expiresAtEpochMs': 1.5,
            }),
        throwsFormatException);
  });

  test('resource and activation limits validate in release code', () {
    expect(
        () => const VesperSourceSessionConfiguration(maxSources: 513).toMap(),
        throwsArgumentError);
    expect(
        () => const VesperSourcePreloadOptions(timeout: Duration.zero).toMap(),
        throwsArgumentError);
    expect(
        () => const VesperSourceActivationOptions(playbackRate: double.nan)
            .toMap(),
        throwsArgumentError);
    expect(
        const VesperSourceActivationOptions(
                playWhenReady: false, startPosition: Duration(seconds: 7))
            .toMap(),
        containsPair('startPositionMs', 7000));
    expect(const VesperSourceSessionConfiguration(maxMemoryBytes: 0).toMap(),
        containsPair('maxMemoryBytes', 0));
  });
}
