import 'package:flutter_test/flutter_test.dart';
import 'package:vesper_player_platform_interface/vesper_player_platform_interface.dart';

void main() {
  test('sequence item keeps cache identity separate from signed source', () {
    const cache = VesperPlaybackSequenceCacheIdentity(
      providerNamespace: 'example.provider',
      contentIdentity: 'content-1',
      renditionIdentity: '720p',
      resourceIdentity: 'progressive',
      accessPartition: 'public',
      sourceRevision: 1,
    );
    final item = VesperPlaybackSequenceItem(
      itemId: 'item-1',
      contentIdentity: const VesperPlaybackSequenceContentIdentity(
        providerNamespace: 'example.provider',
        value: 'content-1',
      ),
      source: VesperPlayerSource.remote(
        uri: 'https://cdn.example/video?token=secret',
        headers: const <String, String>{'Authorization': 'Bearer secret'},
      ),
      cacheIdentity: cache,
      sourceRevision: 1,
    );

    expect(item.cacheIdentity?.toMap()['contentIdentity'], 'content-1');
    expect(item.cacheIdentity?.toMap().containsKey('uri'), isFalse);
    expect(item.cacheIdentity?.toMap().containsKey('Authorization'), isFalse);
    expect(item.toMap()['source'], isNotNull);
  });

  test('snapshot decodes pending request envelope and nested item state', () {
    final snapshot = VesperPlaybackSequenceSnapshot.fromMap(
      <Object?, Object?>{
        'sequenceId': 'feed',
        'sessionGeneration': 3,
        'activationEpoch': 7,
        'items': <Object?>[
          <Object?, Object?>{
            'index': 0,
            'isActive': true,
            'item': <Object?, Object?>{
              'itemId': 'item-1',
              'mediaKind': 'vod',
              'sourceState': <Object?, Object?>{
                'state': 'unresolved',
                'sourceRevision': 0,
              },
            },
          },
        ],
        'activeItemId': 'item-1',
        'pendingRequests': <Object?>[
          <Object?, Object?>{
            'request': <Object?, Object?>{
              'type': 'itemsRequested',
              'sequenceId': 'feed',
              'sessionGeneration': 3,
              'requestId': 9,
              'direction': 'next',
              'maxCount': 1,
              'deadlineRemainingMs': 1000,
            },
          },
        ],
        'requestFailures': const <Object?>[],
        'previousEndReached': false,
        'nextEndReached': false,
        'droppedEvents': 0,
        'warmupTasks': <Object?>[
          <Object?, Object?>{
            'taskId': 17,
            'itemId': 'item-1',
            'sourceRevision': 1,
            'warmupGoal': 'progressiveRange',
            'status': 'completed',
            'expectedBytes': 65536,
            'actualBytes': 65536,
            'cacheHit': true,
            'cacheEntries': 2,
            'cacheBytes': 131072,
            'evictedEntries': 1,
          },
        ],
        'warmupStats': <Object?, Object?>{
          'started': 1,
          'completed': 1,
          'cacheHits': 1,
          'expectedBytes': 65536,
          'actualBytes': 65536,
        },
      },
    );

    expect(snapshot.items.single.itemId, 'item-1');
    expect(snapshot.items.single.sourceState, 'unresolved');
    expect(snapshot.pendingRequests.single['request'], isA<Map>());
    expect(snapshot.warmupTasks.single.status, 'completed');
    expect(snapshot.warmupTasks.single.cacheHit, isTrue);
    expect(snapshot.warmupStats.completed, 1);
    expect(snapshot.warmupStats.actualBytes, 65536);
  });

  test('event parser preserves unknown event type', () {
    final event = VesperPlaybackSequenceEvent.fromMap(
      <Object?, Object?>{
        'type': 'event',
        'sequenceId': 'feed',
        'sessionGeneration': 4,
        'eventSequence': 11,
        'event': <Object?, Object?>{
          'type': 'futureEvent',
          'value': 1,
        },
      },
    );

    expect(event, isA<VesperPlaybackSequenceUnknownEvent>());
    expect((event as VesperPlaybackSequenceUnknownEvent).type, 'futureEvent');
  });
}
