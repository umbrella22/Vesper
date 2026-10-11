import 'package:flutter_test/flutter_test.dart';
import 'package:vesper_player_platform_interface/vesper_player_platform_interface.dart';

Map<String, Object?> _asset() => <String, Object?>{
      'contentFormat': 'dashSegments',
      'resources': <Object?>[
        {'resourceId': 'init:001', 'uri': 'https://example.com/init.mp4'}
      ],
      'segments': <Object?>[
        {'segmentId': 'seg:001', 'uri': 'https://example.com/1.m4s'}
      ],
      'streams': <Object?>[
        {
          'streamId': 'video:001',
          'kind': 'video',
          'resourceIds': ['init:001'],
          'segmentIds': ['seg:001'],
          'metadata': {'bandwidth': '001000'}
        }
      ],
    };

void main() {
  test(
      'recovered plan requires all three maps while snapshot defaults remain optional',
      () {
    final valid = <String, Object?>{
      'source': <String, Object?>{},
      'profile': <String, Object?>{},
      'assetIndex': <String, Object?>{}
    };
    for (final field in ['source', 'profile', 'assetIndex']) {
      for (final missing in [false, true]) {
        final payload = Map<String, Object?>.from(valid);
        if (missing) {
          payload.remove(field);
        } else {
          payload[field] = null;
        }
        expect(
            () => VesperDownloadRecoveredTaskPlan.fromMap(payload),
            throwsA(isA<FormatException>()
                .having((e) => e.message, 'field', '$field must be a map')));
      }
    }
    expect(
        VesperDownloadTaskSnapshot.fromMap({}).assetIndex.resources, isEmpty);
    expect(
        VesperDownloadTaskSnapshot.fromMap(
                {'assetIndex': null, 'profile': null, 'source': null})
            .assetIndex
            .resources,
        isEmpty);
    expect(VesperDownloadRecoveredTaskPlan.fromMap(valid).assetIndex.resources,
        isEmpty);
  });

  test('graph field-name keys cannot be stringified into valid identities', () {
    final decoders = <String, Object Function(Map<Object?, Object?>)>{
      'asset': VesperDownloadAssetIndex.fromMap,
      'resource': VesperDownloadResourceRecord.fromMap,
      'segment': VesperDownloadSegmentRecord.fromMap,
      'stream': VesperDownloadAssetStream.fromMap,
      'profile': VesperDownloadProfile.fromMap,
      'byteRange': VesperDownloadByteRange.fromMap,
      'source': VesperDownloadSource.fromMap,
      'task': VesperDownloadTaskSnapshot.fromMap,
      'snapshot': VesperDownloadSnapshot.fromMap,
      'plan': VesperDownloadRecoveredTaskPlan.fromMap,
    };
    for (final decode in decoders.values) {
      expect(() => decode({_StringifiedKey('resources'): <Object?>[]}),
          throwsFormatException);
    }
    expect(
        () => VesperDownloadAssetIndex.fromMap({
              'resources': [
                {
                  _StringifiedKey('resourceId'): 'init:001',
                  'uri': 'https://example.com/init'
                }
              ]
            }),
        throwsFormatException);
    expect(
        () => VesperDownloadRecoveredTaskPlan.fromMap({
              'source': <String, Object?>{},
              'profile': <String, Object?>{},
              'assetIndex': {_StringifiedKey('resources'): <Object?>[]}
            }),
        throwsFormatException);
    expect(
        () => VesperDownloadSource.fromMap({
              'source': {
                _StringifiedKey('uri'): 'https://example.com/video.mp4'
              }
            }),
        throwsFormatException);
  });

  test(
      'byte ranges reject explicit non-integers without changing null defaults',
      () {
    for (final field in ['offset', 'length']) {
      for (final invalid in <Object?>[
        '128',
        1.5,
        false,
        <Object?>[],
        <String, Object?>{}
      ]) {
        expect(() => VesperDownloadByteRange.fromMap({field: invalid}),
            throwsFormatException);
        expect(
            () => VesperDownloadResourceRecord.fromMap({
                  'resourceId': 'init:001',
                  'uri': 'https://example.com/init',
                  'byteRange': {field: invalid}
                }),
            throwsFormatException);
        expect(
            () => VesperDownloadSegmentRecord.fromMap({
                  'segmentId': 'seg:001',
                  'uri': 'https://example.com/1',
                  'byteRange': {field: invalid}
                }),
            throwsFormatException);
      }
    }
    expect(VesperDownloadByteRange.fromMap({}).toMap(),
        {'offset': 0, 'length': 0});
    expect(
        VesperDownloadByteRange.fromMap({'offset': null, 'length': null})
            .toMap(),
        {'offset': 0, 'length': 0});
    expect(
        VesperDownloadByteRange.fromMap({'offset': 128, 'length': 1024})
            .toMap(),
        {'offset': 128, 'length': 1024});
    expect(VesperDownloadByteRange.fromMap({'offset': -1, 'length': 0}).toMap(),
        {'offset': -1, 'length': 0});
  });

  test(
      'download profiles preserve future output formats but reject wrong wire types',
      () {
    final profile =
        VesperDownloadProfile.fromMap({'targetOutputFormat': 'futureFormat'});
    expect(profile.targetOutputFormat, isNull);
    expect(profile.targetOutputFormatRawValue, 'futureFormat');
    expect(profile.toMap()['targetOutputFormat'], 'futureFormat');
    for (final invalid in <Object?>[
      7,
      false,
      <Object?>[],
      <String, Object?>{}
    ]) {
      expect(
          () => VesperDownloadProfile.fromMap({'targetOutputFormat': invalid}),
          throwsFormatException);
    }
  });

  test(
      'asset graph round-trip preserves records, order and exact string references',
      () {
    final wire = VesperDownloadAssetIndex.fromMap(_asset()).toMap();
    expect(VesperDownloadAssetIndex.fromMap(wire).toMap(), wire);
    final stream = (wire['streams']! as List).single as Map;
    expect(stream['resourceIds'], ['init:001']);
    expect(stream['segmentIds'], ['seg:001']);
    final profile = VesperDownloadProfile.fromMap({
      'selectedTrackIds': ['video:001', '  audio:ja  ', '']
    });
    expect(
        profile.toMap()['selectedTrackIds'], ['video:001', '  audio:ja  ', '']);
  });

  for (final field in ['resources', 'segments', 'streams']) {
    test(
        '$field rejects malformed containers and entries rather than dropping records',
        () {
      for (final invalid in <Object?>[false, 7, '[]', <String, Object?>{}]) {
        expect(
            () => VesperDownloadAssetIndex.fromMap(_asset()..[field] = invalid),
            throwsFormatException);
      }
      for (final invalid in <Object?>[null, false, 7, 'record']) {
        final map = _asset();
        final records = map[field]! as List;
        map[field] = [records.single, invalid];
        expect(
            () => VesperDownloadAssetIndex.fromMap(map), throwsFormatException);
      }
      expect(VesperDownloadAssetIndex.fromMap({field: null}).toMap()[field],
          isEmpty);
    });
  }

  for (final field in ['resourceIds', 'segmentIds']) {
    test('$field rejects mixed graph references instead of shrinking the graph',
        () {
      for (final invalid in <Object?>['id', 7, false, <String, Object?>{}]) {
        expect(
            () => VesperDownloadAssetStream.fromMap(
                {'streamId': 'video:001', field: invalid}),
            throwsFormatException);
      }
      for (final invalid in <Object?>[7, false, null, <String, Object?>{}]) {
        expect(
            () => VesperDownloadAssetStream.fromMap({
                  'streamId': 'video:001',
                  field: ['exact:001', invalid]
                }),
            throwsFormatException);
      }
      expect(
          VesperDownloadAssetStream.fromMap(
              {'streamId': 'video:001', field: null}).toMap()[field],
          isEmpty);
    });
  }

  test('selected track identities reject lossy conversions and malformed lists',
      () {
    for (final invalid in <Object?>[7, true, null, <String, Object?>{}]) {
      expect(
          () => VesperDownloadProfile.fromMap({
                'selectedTrackIds': ['video:001', invalid]
              }),
          throwsFormatException);
    }
    for (final invalid in <Object?>[7, 'video:001', <String, Object?>{}]) {
      expect(() => VesperDownloadProfile.fromMap({'selectedTrackIds': invalid}),
          throwsFormatException);
    }
    expect(
        VesperDownloadProfile.fromMap({'selectedTrackIds': null})
            .selectedTrackIds,
        isEmpty);
  });

  test('graph record identity fields reject missing and wrongly typed strings',
      () {
    for (final invalid in <Object?>[null, 7, false]) {
      expect(
          () => VesperDownloadResourceRecord.fromMap(
              {'resourceId': invalid, 'uri': 'https://example.com/init'}),
          throwsFormatException);
      expect(
          () => VesperDownloadSegmentRecord.fromMap(
              {'segmentId': invalid, 'uri': 'https://example.com/1'}),
          throwsFormatException);
      expect(() => VesperDownloadAssetStream.fromMap({'streamId': invalid}),
          throwsFormatException);
    }
  });

  test(
      'stream metadata does not stringify or collide wrongly typed keys and values',
      () {
    for (final invalid in <Object?>[
      'metadata',
      {7: 'value'},
      {'bandwidth': 7},
      {'codec': null}
    ]) {
      expect(
          () => VesperDownloadAssetStream.fromMap(
              {'streamId': 'video:001', 'metadata': invalid}),
          throwsFormatException);
    }
  });

  test('task snapshot list rejects malformed entries instead of dropping tasks',
      () {
    for (final invalid in <Object?>['tasks', 7, false, <String, Object?>{}]) {
      expect(() => VesperDownloadSnapshot.fromMap({'tasks': invalid}),
          throwsFormatException);
    }
    for (final invalid in <Object?>[null, 7, false, 'task']) {
      expect(
          () => VesperDownloadSnapshot.fromMap({
                'tasks': [invalid]
              }),
          throwsFormatException);
    }
    expect(VesperDownloadSnapshot.fromMap({}).tasks, isEmpty);
    expect(VesperDownloadSnapshot.fromMap({'tasks': null}).tasks, isEmpty);
  });

  test(
      'task and recovered-plan round-trips retain graph and future snapshot state',
      () {
    final source = VesperDownloadSource.fromSource(
      source: VesperPlayerSource.remote(
          uri: 'https://example.com/video.mp4', label: 'Video'),
    );
    final plan = VesperDownloadRecoveredTaskPlan(
      source: source,
      profile: const VesperDownloadProfile(selectedTrackIds: ['video:001']),
      assetIndex: VesperDownloadAssetIndex.fromMap(_asset()),
    );
    expect(VesperDownloadRecoveredTaskPlan.fromMap(plan.toMap()).toMap(),
        plan.toMap());
    final task = VesperDownloadTaskSnapshot(
      taskId: 7,
      assetId: 'asset:001',
      source: source,
      profile: plan.profile,
      state: VesperDownloadState.unknown,
      stateRawValue: 'futureRetrying',
      progress: const VesperDownloadProgressSnapshot(),
      assetIndex: plan.assetIndex,
    );
    final wire = {
      'tasks': [task.toMap(), task.toMap()..['taskId'] = 8]
    };
    final snapshot = VesperDownloadSnapshot.fromMap(wire);
    expect(snapshot.tasks.map((task) => task.taskId), [7, 8]);
    expect(snapshot.toMap(), wire);
  });

  test('download source containers and source URI reject wrongly typed values',
      () {
    for (final invalid in <Object?>[7, false, 'source', <Object?>[]]) {
      expect(() => VesperDownloadSource.fromMap({'source': invalid}),
          throwsFormatException);
    }
    for (final invalid in <Object?>[
      7,
      false,
      <Object?>[],
      <String, Object?>{}
    ]) {
      expect(
          () => VesperDownloadSource.fromMap({
                'source': {'uri': invalid}
              }),
          throwsFormatException);
    }
  });

  test('recovered task graph rejects explicit malformed nested payloads', () {
    for (final field in ['source', 'profile', 'assetIndex']) {
      for (final invalid in <Object?>['payload', 7, false, <Object?>[]]) {
        expect(
            () => VesperDownloadRecoveredTaskPlan.fromMap({
                  'source': <String, Object?>{},
                  'profile': <String, Object?>{},
                  'assetIndex': <String, Object?>{},
                  field: invalid,
                }),
            throwsA(isA<FormatException>().having(
                (error) => error.message, 'field', '$field must be a map')));
        expect(() => VesperDownloadTaskSnapshot.fromMap({field: invalid}),
            throwsFormatException);
      }
    }
  });
}

final class _StringifiedKey {
  const _StringifiedKey(this.value);
  final String value;
  @override
  String toString() => value;
}
