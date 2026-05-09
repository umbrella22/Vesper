import 'package:flutter_test/flutter_test.dart';
import 'package:vesper_player_platform_interface/vesper_player_platform_interface.dart';

void main() {
  test('download task update event decodes prepared task', () {
    final event = VesperDownloadManagerEvent.fromMap(<Object?, Object?>{
      'downloadId': 'downloads',
      'type': 'taskUpdated',
      'task': <Object?, Object?>{
        'taskId': 11,
        'assetId': 'asset-hls',
        'source': VesperDownloadSource.fromSource(
          source: VesperPlayerSource.hls(
            uri: 'https://example.com/master.m3u8',
            label: 'HLS demo',
          ),
          manifestUri: 'https://example.com/master.m3u8',
        ).toMap(),
        'profile': const VesperDownloadProfile(
          targetOutputFormat: VesperDownloadOutputFormat.mp4,
        ).toMap(),
        'state': 'preparing',
        'progress': const VesperDownloadProgressSnapshot(
          totalBytes: 1024,
          totalSegments: 2,
        ).toMap(),
        'assetIndex': const VesperDownloadAssetIndex(
          contentFormat: VesperDownloadContentFormat.hlsSegments,
          totalSizeBytes: 1024,
          segments: <VesperDownloadSegmentRecord>[
            VesperDownloadSegmentRecord(
              segmentId: 'seg-1',
              uri: 'https://example.com/seg-1.ts',
              relativePath: 'seg-1.ts',
              sequence: 1,
              sizeBytes: 1024,
            ),
          ],
        ).toMap(),
      },
    });

    expect(event, isA<VesperDownloadTaskUpdatedEvent>());
    final updateEvent = event as VesperDownloadTaskUpdatedEvent;
    expect(updateEvent.downloadId, 'downloads');
    expect(updateEvent.task?.taskId, 11);
    expect(updateEvent.task?.assetIndex.totalSizeBytes, 1024);
    expect(
      updateEvent.task?.profile.targetOutputFormat,
      VesperDownloadOutputFormat.mp4,
    );
  });

  test('download manager event requires the breaking incremental type', () {
    expect(
      () => VesperDownloadManagerEvent.fromMap(<Object?, Object?>{
        'downloadId': 'downloads',
        'snapshot': const VesperDownloadSnapshot.initial().toMap(),
      }),
      throwsA(isA<FormatException>()),
    );
  });

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
