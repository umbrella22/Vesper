import 'package:flutter_test/flutter_test.dart';
import 'package:vesper_player_platform_interface/vesper_player_platform_interface.dart';

void main() {
  test('player snapshot event decodes embedded host lastError', () {
    final event = VesperPlayerEvent.fromMap(<Object?, Object?>{
      'playerId': 'ios-player',
      'type': 'snapshot',
      'snapshot': <Object?, Object?>{
        'title': 'Demo',
        'subtitle': 'Unsupported',
        'sourceLabel': 'feed://demo',
        'playbackState': 'ready',
        'playbackRate': 1.0,
        'isBuffering': false,
        'isInterrupted': false,
        'hasVideoSurface': false,
        'timeline': const VesperTimeline.initial().toMap(),
        'fixedTrackStatus': 'pending',
        'lastError': <Object?, Object?>{
          'message':
              'setAbrPolicy fixedTrack is not implemented on iOS AVPlayer',
          'category': 'unsupported',
          'retriable': false,
        },
      },
    });

    expect(event, isA<VesperPlayerSnapshotEvent>());
    final snapshotEvent = event as VesperPlayerSnapshotEvent;
    expect(snapshotEvent.playerId, 'ios-player');
    expect(
      snapshotEvent.snapshot.lastError?.category,
      VesperPlayerErrorCategory.unsupported,
    );
    expect(
      snapshotEvent.snapshot.lastError?.message,
      'setAbrPolicy fixedTrack is not implemented on iOS AVPlayer',
    );
    expect(
      snapshotEvent.snapshot.fixedTrackStatus,
      VesperFixedTrackStatus.pending,
    );
  });
}
