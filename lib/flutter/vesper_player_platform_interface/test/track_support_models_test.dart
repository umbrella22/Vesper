import 'package:flutter_test/flutter_test.dart';
import 'package:vesper_player_platform_interface/vesper_player_platform_interface.dart';

void main() {
  test('missing track support keeps conservative compatibility defaults', () {
    final track = VesperMediaTrack.fromMap(const <Object?, Object?>{
      'id': 'video:720p',
      'kind': 'video',
    });

    expect(track.support.status, VesperTrackSupportStatus.unknown);
    expect(track.support.reason, VesperTrackSupportReason.platformUnknown);
    expect(track.support.source, VesperTrackSupportSource.unavailable);
    expect(track.support.reasonRawValue, isNull);
    expect(track.support.sourceRawValue, isNull);
  });

  test('unknown track support reason and source preserve raw wire values', () {
    final support = VesperTrackSupport.fromMap(const <Object?, Object?>{
      'status': 'futureStatus',
      'reason': 'futureReason',
      'source': 'futureSource',
    });

    expect(support.status, VesperTrackSupportStatus.unknown);
    expect(support.statusRawValue, 'futureStatus');
    expect(support.reason, VesperTrackSupportReason.unknown);
    expect(support.reasonRawValue, 'futureReason');
    expect(support.source, VesperTrackSupportSource.unknown);
    expect(support.sourceRawValue, 'futureSource');

    final roundTrip = VesperTrackSupport.fromMap(support.toMap());
    expect(roundTrip.reason, VesperTrackSupportReason.unknown);
    expect(roundTrip.reasonRawValue, 'futureReason');
    expect(roundTrip.source, VesperTrackSupportSource.unknown);
    expect(roundTrip.sourceRawValue, 'futureSource');
  });

  test('track catalog preserves revision and playback path', () {
    final catalog = VesperTrackCatalog.fromMap(const <Object?, Object?>{
      'tracks': <Object?>[
        <Object?, Object?>{
          'id': 'video:720p',
          'kind': 'video',
          'support': <Object?, Object?>{
            'status': 'supported',
            'reason': 'none',
            'source': 'runtimeTrackCatalog',
          },
        },
      ],
      'adaptiveVideo': true,
      'catalogRevision': 12,
      'playbackPath': 'systemPlayer',
    });

    expect(catalog.catalogRevision, 12);
    expect(catalog.playbackPath, 'systemPlayer');
    expect(catalog.videoTracks.single.support.status,
        VesperTrackSupportStatus.supported);

    final roundTrip = VesperTrackCatalog.fromMap(catalog.toMap());
    expect(roundTrip.catalogRevision, 12);
    expect(roundTrip.playbackPath, 'systemPlayer');
    expect(roundTrip.videoTracks.single.id, 'video:720p');
  });
}
