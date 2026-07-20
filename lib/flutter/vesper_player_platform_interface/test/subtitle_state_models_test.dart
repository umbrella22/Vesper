import 'package:flutter_test/flutter_test.dart';
import 'package:vesper_player_platform_interface/vesper_player_platform_interface.dart';

/// Subtitle state wire contract tests.
///
/// These tests lock down the iOS/Android wire shape and the forward-
/// compatibility unknown-value preservation policy. The contract must hold
/// across iOS and Android host kits so a Flutter consumer observes the same
/// fields regardless of which platform produced the snapshot.
void main() {
  group('VesperSubtitleState', () {
    test('fromMap uses defaults when fields are absent', () {
      const empty = VesperSubtitleState.empty;
      final decoded = VesperSubtitleState.fromMap(const <Object?, Object?>{});

      expect(decoded.status, VesperSubtitleStatus.unavailable);
      expect(decoded.advertisedTrackCount, 0);
      expect(decoded.selectableTrackCount, 0);
      expect(decoded.error, isNull);
      expect(decoded.statusRawValue, isNull);
      // Round-trips to the same shape as `empty`.
      expect(decoded.toMap(), empty.toMap());
    });

    test('fromMap preserves unknown status raw value', () {
      final decoded = VesperSubtitleState.fromMap(
        const <Object?, Object?>{'status': 'weird-future-status'},
      );

      expect(
        decoded.status,
        VesperSubtitleStatus.unknown,
        reason: 'unknown wire values fall back to the unknown enum variant',
      );
      expect(
        decoded.statusRawValue,
        'weird-future-status',
        reason: 'the raw wire value is preserved for diagnostics',
      );
    });

    test('fromMap decodes ready state with counts and no error', () {
      final decoded = VesperSubtitleState.fromMap(
        const <Object?, Object?>{
          'status': 'ready',
          'advertisedTrackCount': 2,
          'selectableTrackCount': 2,
          'error': null,
        },
      );

      expect(decoded.status, VesperSubtitleStatus.ready);
      expect(decoded.advertisedTrackCount, 2);
      expect(decoded.selectableTrackCount, 2);
      expect(decoded.error, isNull);
    });

    test('fromMap decodes loading status with advertised count', () {
      final decoded = VesperSubtitleState.fromMap(
        const <Object?, Object?>{
          'status': 'loading',
          'advertisedTrackCount': 3,
          'selectableTrackCount': 0,
        },
      );

      expect(decoded.status, VesperSubtitleStatus.loading);
      expect(decoded.advertisedTrackCount, 3);
      expect(decoded.selectableTrackCount, 0);
      expect(decoded.error, isNull);
    });

    test('fromMap handles error without trackId field', () {
      final decoded = VesperSubtitleState.fromMap(
        const <Object?, Object?>{
          'status': 'failed',
          'advertisedTrackCount': 1,
          'selectableTrackCount': 0,
          'error': <Object?, Object?>{
            'code': 'subtitle_platform_track_unavailable',
            'phase': 'discovery',
            'retriable': false,
            'message': 'no legible group',
          },
        },
      );

      expect(decoded.status, VesperSubtitleStatus.failed);
      expect(decoded.error?.trackId, isNull);
      expect(decoded.error?.code, 'subtitle_platform_track_unavailable');
    });

    test('toMap round-trips known and unknown status values', () {
      final known = VesperSubtitleState(
        status: VesperSubtitleStatus.failed,
        advertisedTrackCount: 3,
        selectableTrackCount: 0,
        error: VesperSubtitleError(
          code: 'subtitle_track_not_found',
          phase: VesperSubtitleErrorPhase.selection,
          retriable: false,
          message: 'missing',
          trackId: 'subtitle:dash:sub-en',
        ),
      );
      final roundTrip = VesperSubtitleState.fromMap(_objectMap(known.toMap()));

      expect(roundTrip.status, VesperSubtitleStatus.failed);
      expect(roundTrip.advertisedTrackCount, 3);
      expect(roundTrip.error?.code, 'subtitle_track_not_found');
      expect(roundTrip.error?.phase, VesperSubtitleErrorPhase.selection);
      expect(roundTrip.error?.trackId, 'subtitle:dash:sub-en');

      // Now an unknown wire value round-trips with raw preservation.
      final unknown = VesperSubtitleState.fromMap(
        const <Object?, Object?>{'status': 'future'},
      );
      final unknownRoundTrip =
          VesperSubtitleState.fromMap(_objectMap(unknown.toMap()));
      expect(unknownRoundTrip.status, VesperSubtitleStatus.unknown);
      expect(unknownRoundTrip.statusRawValue, 'future');
    });

    test('copyWith clears an unknown raw value when status changes', () {
      final unknown = VesperSubtitleState.fromMap(
        const <Object?, Object?>{'status': 'future'},
      );

      final ready = unknown.copyWith(status: VesperSubtitleStatus.ready);

      expect(ready.status, VesperSubtitleStatus.ready);
      expect(ready.statusRawValue, isNull);
      expect(ready.toMap()['status'], 'ready');
      expect(
        unknown.copyWith(advertisedTrackCount: 1).statusRawValue,
        'future',
        reason: 'unrelated updates must preserve the forward-compatible value',
      );
    });
  });

  group('VesperSubtitleError', () {
    test('fromMap preserves unknown code and phase raw values', () {
      final decoded = VesperSubtitleError.fromMap(
        const <Object?, Object?>{
          'code': 'subtitle_future_code',
          'phase': 'post_selection',
          'retriable': true,
          'message': 'future failure',
          'trackId': 'subtitle:dash:sub-zh',
        },
      );

      expect(decoded.code, 'subtitle_future_code');
      expect(decoded.codeRawValue, 'subtitle_future_code');
      expect(
        decoded.phase,
        VesperSubtitleErrorPhase.unknown,
        reason: 'unknown phase falls back to unknown',
      );
      expect(decoded.phaseRawValue, 'post_selection');
      expect(decoded.retriable, isTrue);
      expect(decoded.trackId, 'subtitle:dash:sub-zh');
    });

    test('fromMap decodes known code and phase values', () {
      final decoded = VesperSubtitleError.fromMap(
        const <Object?, Object?>{
          'code': 'subtitle_platform_track_unavailable',
          'phase': 'discovery',
          'retriable': false,
          'message': 'no legible group',
        },
      );

      expect(decoded.code, 'subtitle_platform_track_unavailable');
      expect(decoded.phase, VesperSubtitleErrorPhase.discovery);
      expect(decoded.phaseRawValue, 'discovery');
      expect(decoded.retriable, isFalse);
      expect(decoded.trackId, isNull);
    });

    test('toMap round-trips raw values when present', () {
      final error = VesperSubtitleError(
        code: 'unknown',
        phase: VesperSubtitleErrorPhase.unknown,
        retriable: false,
        message: 'future',
        codeRawValue: 'subtitle_future_code',
        phaseRawValue: 'post_selection',
      );
      final roundTrip = VesperSubtitleError.fromMap(_objectMap(error.toMap()));

      expect(roundTrip.codeRawValue, 'subtitle_future_code');
      expect(roundTrip.phaseRawValue, 'post_selection');
      expect(roundTrip.phase, VesperSubtitleErrorPhase.unknown);
    });

    test('toMap prefers raw code over fallback enum name', () {
      final error = VesperSubtitleError(
        code: 'subtitle_track_not_found',
        phase: VesperSubtitleErrorPhase.selection,
        retriable: false,
        message: 'missing',
        codeRawValue: 'subtitle_track_not_found',
      );
      expect(error.toMap()['code'], 'subtitle_track_not_found');
    });

    test('fromMap tolerates missing fields', () {
      final decoded = VesperSubtitleError.fromMap(const <Object?, Object?>{});

      expect(decoded.code, 'unknown');
      expect(decoded.phase, VesperSubtitleErrorPhase.unknown);
      expect(decoded.retriable, isFalse);
      expect(decoded.message, '');
      expect(decoded.trackId, isNull);
    });
  });

  group('VesperPlayerSnapshot subtitleState', () {
    test('snapshot with subtitleState decodes and emits the field', () {
      final snapshot = VesperPlayerSnapshot.initial().copyWith(
        subtitleState: VesperSubtitleState(
          status: VesperSubtitleStatus.ready,
          advertisedTrackCount: 2,
          selectableTrackCount: 2,
        ),
      );

      final roundTrip = VesperPlayerSnapshot.fromMap(
        _objectMap(snapshot.toMap()),
      );

      expect(roundTrip.subtitleState.status, VesperSubtitleStatus.ready);
      expect(roundTrip.subtitleState.advertisedTrackCount, 2);
      expect(roundTrip.subtitleState.selectableTrackCount, 2);
      expect(roundTrip.subtitleState.error, isNull);
    });

    test('snapshot without subtitleState falls back to empty default', () {
      // Simulate an older host payload that omits the field entirely.
      final legacyPayload = VesperPlayerSnapshot.initial().toMap()
        ..remove('subtitleState');

      final decoded = VesperPlayerSnapshot.fromMap(
        _objectMap(legacyPayload),
      );

      expect(decoded.subtitleState.status, VesperSubtitleStatus.unavailable);
      expect(decoded.subtitleState.advertisedTrackCount, 0);
      expect(decoded.subtitleState.selectableTrackCount, 0);
      expect(decoded.subtitleState.error, isNull);
    });

    test('snapshot preserves subtitle state unknown wire values', () {
      final legacyPayload = VesperPlayerSnapshot.initial().toMap()
        ..['subtitleState'] = <String, Object?>{
          'status': 'future-status',
          'advertisedTrackCount': 1,
          'selectableTrackCount': 0,
          'error': <String, Object?>{
            'code': 'subtitle_future_code',
            'phase': 'post_selection',
            'retriable': false,
            'message': 'future',
          },
        };

      final decoded = VesperPlayerSnapshot.fromMap(
        _objectMap(legacyPayload),
      );

      expect(decoded.subtitleState.status, VesperSubtitleStatus.unknown);
      expect(decoded.subtitleState.statusRawValue, 'future-status');
      expect(decoded.subtitleState.error?.codeRawValue, 'subtitle_future_code');
      expect(decoded.subtitleState.error?.phaseRawValue, 'post_selection');
    });
  });
}

/// Converts a `Map<String, Object?>` (the shape `toMap()` produces) into the
/// `Map<Object?, Object?>` shape `fromMap()` consumes. This mirrors how
/// Flutter's standard method codec hands payloads to decoders.
Map<Object?, Object?> _objectMap(Map<String, Object?> input) {
  return Map<Object?, Object?>.from(input);
}
