import 'dart:convert';
import 'dart:io';

import 'package:flutter_test/flutter_test.dart';
import 'package:vesper_player_platform_interface/vesper_player_platform_interface.dart';

/// Subtitle state wire contract tests.
///
/// These tests lock down the iOS/Android wire shape and the forward-
/// compatibility unknown-value preservation policy. The contract must hold
/// across iOS and Android host kits so a Flutter consumer observes the same
/// fields regardless of which platform produced the snapshot.
void main() {
  test('shared subtitle contract fixtures decode canonical fields', () {
    final statePayload = _contractMap('subtitle_state.json');
    final state = VesperSubtitleState.fromMap(statePayload);
    final errorPayload = _contractMap('subtitle_error.json');

    expect(state.catalogState, VesperSubtitleCatalogState.ready);
    expect(state.selectionState, VesperSubtitleSelectionState.failed);
    expect(state.advertisedTrackCount, 3);
    expect(state.selectableTrackCount, 2);
    expect(state.catalogError?.code, 'subtitle_resource_failed');
    expect(state.selectionError?.trackId, 'opaque-track-7');
    expect(state.selectionError?.commandId, 42);
    expect(state.selectionError?.sourceEpoch, 9);

    final error = VesperSubtitleError.fromMap(errorPayload);
    expect(error.code, 'subtitle_selection_timeout');
    expect(error.phase, VesperSubtitleErrorPhase.selection);
    expect(error.trackId, 'opaque-track-7');
  });

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

    test('canonical catalog and selection fields round-trip with errors', () {
      final state = VesperSubtitleState(
        catalogState: VesperSubtitleCatalogState.ready,
        selectionState: VesperSubtitleSelectionState.confirmed,
        advertisedTrackCount: 3,
        selectableTrackCount: 2,
        catalogError: const VesperSubtitleError(
          code: 'subtitle_catalog_warning',
          phase: VesperSubtitleErrorPhase.discovery,
          retriable: true,
          message: 'one descriptor is unavailable',
        ),
        selectionError: const VesperSubtitleError(
          code: 'subtitle_selection_failed',
          phase: VesperSubtitleErrorPhase.selection,
          retriable: false,
          message: 'selection did not converge',
          commandId: 7,
          sourceEpoch: 4,
        ),
      );

      final map = state.toMap();
      expect(map['catalogState'], 'ready');
      expect(map['selectionState'], 'confirmed');
      expect(map['catalogError'], isA<Map>());
      expect(map['selectionError'], isA<Map>());
      expect(map['status'], 'ready', reason: 'legacy alias remains emitted');

      final decoded = VesperSubtitleState.fromMap(_objectMap(map));
      expect(decoded.catalogState, VesperSubtitleCatalogState.ready);
      expect(decoded.selectionState, VesperSubtitleSelectionState.confirmed);
      expect(decoded.catalogError?.phase, VesperSubtitleErrorPhase.discovery);
      expect(decoded.selectionError?.commandId, 7);
      expect(decoded.selectionError?.sourceEpoch, 4);
    });

    test('canonical fields override conflicting legacy constructor aliases',
        () {
      const canonicalError = VesperSubtitleError(
        code: 'subtitle_catalog_warning',
        phase: VesperSubtitleErrorPhase.discovery,
        retriable: true,
        message: 'canonical',
      );
      const legacyError = VesperSubtitleError(
        code: 'subtitle_legacy_failure',
        phase: VesperSubtitleErrorPhase.selection,
        retriable: false,
        message: 'legacy',
      );
      const state = VesperSubtitleState(
        catalogState: VesperSubtitleCatalogState.ready,
        selectionState: VesperSubtitleSelectionState.confirmed,
        catalogError: canonicalError,
        status: VesperSubtitleStatus.failed,
        error: legacyError,
      );

      expect(state.status, VesperSubtitleStatus.ready);
      expect(state.error, same(canonicalError));
      expect(state.toMap()['status'], 'ready');
      expect(
        (state.toMap()['error'] as Map<String, Object?>)['code'],
        'subtitle_catalog_warning',
      );
    });

    test('canonical unknown states preserve raw values', () {
      final decoded = VesperSubtitleState.fromMap(
        const <Object?, Object?>{
          'catalogState': 'future_catalog',
          'selectionState': 'future_selection',
        },
      );

      expect(decoded.catalogState, VesperSubtitleCatalogState.unknown);
      expect(decoded.selectionState, VesperSubtitleSelectionState.unknown);
      expect(decoded.catalogStateRawValue, 'future_catalog');
      expect(decoded.selectionStateRawValue, 'future_selection');
      expect(decoded.toMap()['catalogState'], 'future_catalog');
      expect(decoded.toMap()['selectionState'], 'future_selection');
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

    test('snapshot round-trips requested, confirmed, and effective subtitles',
        () {
      final snapshot = VesperPlayerSnapshot.initial().copyWith(
        requestedSubtitleSelection:
            const VesperTrackSelection.track('subtitle:en'),
        confirmedSubtitleSelection:
            const VesperTrackSelection.track('subtitle:en'),
        effectiveSubtitleTrackId: 'subtitle:en',
      );

      final decoded = VesperPlayerSnapshot.fromMap(
        _objectMap(snapshot.toMap()),
      );

      expect(decoded.requestedSubtitleSelection.trackId, 'subtitle:en');
      expect(decoded.confirmedSubtitleSelection.trackId, 'subtitle:en');
      expect(decoded.effectiveSubtitleTrackId, 'subtitle:en');
      expect(decoded.trackSelection.subtitle.trackId, 'subtitle:en');
    });

    test('snapshot derives requested and confirmed subtitles from legacy map',
        () {
      final legacyTrackSelection = <String, Object?>{
        ...VesperPlayerSnapshot.initial().trackSelection.toMap(),
        'subtitle': const VesperTrackSelection.track('subtitle:legacy').toMap(),
      }
        ..remove('confirmedSubtitle')
        ..remove('effectiveSubtitleTrackId');
      final legacy = VesperPlayerSnapshot.initial().toMap()
        ..remove('requestedSubtitleSelection')
        ..remove('confirmedSubtitleSelection')
        ..remove('effectiveSubtitleTrackId')
        ..['trackSelection'] = legacyTrackSelection;

      final decoded = VesperPlayerSnapshot.fromMap(_objectMap(legacy));
      expect(decoded.requestedSubtitleSelection.trackId, 'subtitle:legacy');
      expect(decoded.confirmedSubtitleSelection.trackId, 'subtitle:legacy');
      expect(decoded.effectiveSubtitleTrackId, isNull);
    });

    test('snapshot constructor preserves nested confirmed selection', () {
      final snapshot = VesperPlayerSnapshot(
        title: 'Vesper',
        subtitle: '',
        sourceLabel: '',
        playbackState: VesperPlaybackState.ready,
        playbackRate: 1,
        isBuffering: false,
        isInterrupted: false,
        hasVideoSurface: false,
        timeline: const VesperTimeline.initial(),
        trackSelection: const VesperTrackSelectionSnapshot(
          subtitle: VesperTrackSelection.track('requested'),
          confirmedSubtitle: VesperTrackSelection.track('confirmed'),
          effectiveSubtitleTrackId: 'confirmed',
        ),
      );

      expect(snapshot.requestedSubtitleSelection.trackId, 'requested');
      expect(snapshot.confirmedSubtitleSelection.trackId, 'confirmed');
      expect(snapshot.trackSelection.confirmedSubtitle.trackId, 'confirmed');
      expect(snapshot.effectiveSubtitleTrackId, 'confirmed');
    });

    test('requested subtitle remains the canonical legacy snapshot alias', () {
      final snapshot = VesperPlayerSnapshot.initial().copyWith(
        requestedSubtitleSelection:
            const VesperTrackSelection.track('subtitle:requested'),
      );
      expect(snapshot.trackSelection.subtitle.trackId, 'subtitle:requested');

      final legacyUpdate = snapshot.copyWith(
        trackSelection: const VesperTrackSelectionSnapshot(
          subtitle: VesperTrackSelection.track('subtitle:legacy-update'),
        ),
      );
      expect(
        legacyUpdate.requestedSubtitleSelection.trackId,
        'subtitle:legacy-update',
      );
      expect(
        legacyUpdate.trackSelection.subtitle.trackId,
        'subtitle:legacy-update',
      );
      expect(
        legacyUpdate.toMap()['trackSelection'],
        legacyUpdate.trackSelection.toMap(),
      );
    });

    test('clearing effective subtitle clears nested and top-level aliases', () {
      final snapshot = VesperPlayerSnapshot.initial().copyWith(
        trackSelection: const VesperTrackSelectionSnapshot(
          confirmedSubtitle: VesperTrackSelection.track('confirmed'),
          effectiveSubtitleTrackId: 'confirmed',
        ),
      );

      final cleared = snapshot.copyWith(clearEffectiveSubtitleTrackId: true);
      expect(cleared.effectiveSubtitleTrackId, isNull);
      expect(cleared.trackSelection.effectiveSubtitleTrackId, isNull);
      expect(cleared.toMap()['effectiveSubtitleTrackId'], isNull);
      expect(
        (cleared.toMap()['trackSelection']
            as Map<String, Object?>)['effectiveSubtitleTrackId'],
        isNull,
      );
    });

    test('nested canonical subtitle fields win conflicting top-level aliases',
        () {
      final payload = VesperPlayerSnapshot.initial().toMap()
        ..['trackSelection'] = const VesperTrackSelectionSnapshot(
          subtitle: VesperTrackSelection.track('nested-requested'),
          confirmedSubtitle: VesperTrackSelection.track('nested-confirmed'),
          effectiveSubtitleTrackId: 'nested-confirmed',
        ).toMap()
        ..['requestedSubtitleSelection'] =
            const VesperTrackSelection.track('top-requested').toMap()
        ..['confirmedSubtitleSelection'] =
            const VesperTrackSelection.track('top-confirmed').toMap()
        ..['effectiveSubtitleTrackId'] = 'top-confirmed';

      final decoded = VesperPlayerSnapshot.fromMap(_objectMap(payload));
      expect(decoded.requestedSubtitleSelection.trackId, 'nested-requested');
      expect(decoded.confirmedSubtitleSelection.trackId, 'nested-confirmed');
      expect(decoded.effectiveSubtitleTrackId, 'nested-confirmed');
    });

    test('constructor nested fields win conflicting top-level aliases', () {
      final snapshot = VesperPlayerSnapshot(
        title: 'Vesper',
        subtitle: '',
        sourceLabel: '',
        playbackState: VesperPlaybackState.ready,
        playbackRate: 1,
        isBuffering: false,
        isInterrupted: false,
        hasVideoSurface: false,
        timeline: const VesperTimeline.initial(),
        trackSelection: const VesperTrackSelectionSnapshot(
          subtitle: VesperTrackSelection.track('nested-requested'),
          confirmedSubtitle: VesperTrackSelection.track('nested-confirmed'),
          effectiveSubtitleTrackId: 'nested-confirmed',
        ),
        requestedSubtitleSelection:
            const VesperTrackSelection.track('top-requested'),
        confirmedSubtitleSelection:
            const VesperTrackSelection.track('top-confirmed'),
        effectiveSubtitleTrackId: 'top-confirmed',
      );

      expect(snapshot.requestedSubtitleSelection.trackId, 'nested-requested');
      expect(snapshot.confirmedSubtitleSelection.trackId, 'nested-confirmed');
      expect(snapshot.effectiveSubtitleTrackId, 'nested-confirmed');
      expect(snapshot.toMap()['requestedSubtitleSelection'],
          snapshot.trackSelection.subtitle.toMap());
    });
  });
}

/// Converts a `Map<String, Object?>` (the shape `toMap()` produces) into the
/// `Map<Object?, Object?>` shape `fromMap()` consumes. This mirrors how
/// Flutter's standard method codec hands payloads to decoders.
Map<Object?, Object?> _objectMap(Map<String, Object?> input) {
  return Map<Object?, Object?>.from(input);
}

Map<Object?, Object?> _contractMap(String name) {
  final decoded = jsonDecode(
    File('../../../fixtures/contracts/$name').readAsStringSync(),
  );
  return Map<Object?, Object?>.from(decoded as Map);
}
