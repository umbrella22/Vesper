import 'package:flutter_test/flutter_test.dart';
import 'package:vesper_player_platform_interface/vesper_player_platform_interface.dart';

void main() {
  test('legacy and unavailable platform snapshots leave output unconfirmed',
      () {
    for (final output in <Object?>[
      null,
      <String, Object?>{},
      <String, Object?>{
        'state': 'unknown',
        'reason': 'outputObservationUnavailable',
      },
    ]) {
      final snapshot = VesperPlayerSnapshot.fromMap(<Object?, Object?>{
        ...const VesperPlayerSnapshot.initial().toMap(),
        'hdrOutput': output,
      });
      expect(snapshot.hdrOutputState, VesperHdrOutputState.unknown);
    }
    expect(const VesperPlayerSnapshot.initial().hdrOutput, isNull);
  });

  test(
      'future output states and formats remain unknown and round-trip raw values',
      () {
    final output = VesperHdrOutputSnapshot.fromMap(<Object?, Object?>{
      'state': 'futureState',
      'format': 'futureFormat',
      'reason': 'futureReason',
    });
    expect(output.state, VesperHdrOutputState.unknown);
    expect(output.format, VesperHdrOutputFormat.unknown);
    expect(output.toMap()['state'], 'futureState');
    expect(output.toMap()['format'], 'futureFormat');
    expect(output.reason, 'futureReason');
  });

  test(
      'snapshot events preserve output evidence and its independent generation',
      () {
    final event = VesperPlayerEvent.fromMap(<Object?, Object?>{
      'playerId': 'player-1',
      'type': 'snapshot',
      'snapshot': <String, Object?>{
        ...const VesperPlayerSnapshot.initial().toMap(),
        'hdrOutput': const VesperHdrOutputSnapshot(
          state: VesperHdrOutputState.hdr,
          format: VesperHdrOutputFormat.hlg,
          playerId: 'player-1',
          sourceRevision: 2,
          outputGeneration: 7,
          effectiveVideoTrackId: 'video:a',
          catalogRevision: 3,
          displayId: 'display:0',
          evidence: 'testDisplayObservation',
        ).toMap(),
      },
    }) as VesperPlayerSnapshotEvent;
    final output = event.snapshot.hdrOutput!;
    expect(output.state, VesperHdrOutputState.hdr);
    expect(output.format, VesperHdrOutputFormat.hlg);
    expect(output.playerId, event.playerId);
    expect(output.sourceRevision, 2);
    expect(output.outputGeneration, 7);
    expect(output.effectiveVideoTrackId, 'video:a');
    expect(output.catalogRevision, 3);
    expect(output.displayId, 'display:0');
    expect(output.evidence, 'testDisplayObservation');
    expect(
        VesperPlayerSnapshot.fromMap(event.snapshot.toMap()).hdrOutput!.toMap(),
        output.toMap());
  });

  test('confirmed HDR can have an unknown output format', () {
    final output = VesperHdrOutputSnapshot.fromMap(<Object?, Object?>{
      'state': 'hdr',
      'playerId': 'player-1',
      'sourceRevision': 1,
      'outputGeneration': 1,
      'evidence': 'testDisplayObservation',
    });
    expect(output.state, VesperHdrOutputState.hdr);
    expect(output.format, VesperHdrOutputFormat.unknown);
  });

  for (final state in <String>['hdr', 'sdr']) {
    test('incomplete $state evidence stays unknown after round-trip', () {
      final complete = <Object?, Object?>{
        'state': state,
        'format': 'hdr10',
        'playerId': 'player-1',
        'sourceRevision': 1,
        'outputGeneration': 1,
        'evidence': 'testDisplayObservation',
      };
      final invalid = <Map<Object?, Object?>>[
        <Object?, Object?>{'state': state},
        for (final field in <String>[
          'playerId',
          'sourceRevision',
          'outputGeneration',
          'evidence'
        ])
          Map<Object?, Object?>.of(complete)..remove(field),
        <Object?, Object?>{...complete, 'playerId': ' '},
        <Object?, Object?>{...complete, 'evidence': ' '},
        <Object?, Object?>{...complete, 'sourceRevision': -1},
        <Object?, Object?>{...complete, 'outputGeneration': -1},
        <Object?, Object?>{...complete, 'outputGeneration': '1'},
      ];
      for (final map in invalid) {
        final output = VesperHdrOutputSnapshot.fromMap(map);
        expect(output.state, VesperHdrOutputState.unknown);
        expect(output.format, VesperHdrOutputFormat.unknown);
        expect(output.evidence, isNull);
        expect(output.reason, 'incompleteOutputEvidence');
        expect(output.stateRawValue, state);
        expect(output.toMap()['state'], 'unknown');
        final roundTrip = VesperHdrOutputSnapshot.fromMap(output.toMap());
        expect(roundTrip.state, VesperHdrOutputState.unknown);
        expect(roundTrip.stateRawValue, state);
      }
    });
  }

  test('complete evidence preserves valid identities and zero generations', () {
    final output = VesperHdrOutputSnapshot.fromMap(<Object?, Object?>{
      'state': 'sdr',
      'playerId': ' player-1 ',
      'sourceRevision': 0,
      'outputGeneration': 0,
      'evidence': ' testDisplayObservation ',
    });
    expect(output.state, VesperHdrOutputState.sdr);
    expect(output.playerId, ' player-1 ');
    expect(output.evidence, ' testDisplayObservation ');
    expect(VesperHdrOutputSnapshot.fromMap(output.toMap()).state,
        VesperHdrOutputState.sdr);
  });

  test('copyWith preserves evidence unless explicitly replaced or cleared', () {
    final snapshot = const VesperPlayerSnapshot.initial().copyWith(
      hdrOutput: const VesperHdrOutputSnapshot(
        state: VesperHdrOutputState.sdr,
        playerId: 'player-1',
        sourceRevision: 1,
        outputGeneration: 1,
        evidence: 'testSdrObservation',
      ),
    );
    expect(
        snapshot.copyWith(playbackRate: 2).hdrOutput, same(snapshot.hdrOutput));
    expect(snapshot.copyWith(clearHdrOutput: true).hdrOutputState,
        VesperHdrOutputState.unknown);
    final invalidated = snapshot.copyWith(
      hdrOutput: const VesperHdrOutputSnapshot(
        playerId: 'player-1',
        sourceRevision: 1,
        outputGeneration: 2,
        reason: 'displayPathChanged',
      ),
    );
    expect(invalidated.hdrOutputState, VesperHdrOutputState.unknown);
    expect(invalidated.hdrOutput!.evidence, isNull);
  });

  test('HDR capability and pixel-format hints never populate output state', () {
    final snapshot = VesperPlayerSnapshot.fromMap(<Object?, Object?>{
      'hdrKind': 'hdr10',
      'outputFormat': 'p010',
      'confidence': 'sessionProbe',
      'displayHdrSupported': true,
      'hdrMetadata': <String, Object?>{'hdrKind': 'hdr10'},
    });
    expect(snapshot.hdrOutput, isNull);
    expect(snapshot.hdrOutputState, VesperHdrOutputState.unknown);
  });
}
