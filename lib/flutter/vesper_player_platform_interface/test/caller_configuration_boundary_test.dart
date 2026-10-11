import 'package:flutter_test/flutter_test.dart';
import 'package:vesper_player_platform_interface/vesper_player_platform_interface.dart';

typedef _Decode = Map<String, Object?> Function(Map<Object?, Object?>);

void main() {
  final choices = <(String, String, List<String>, String, _Decode)>[
    (
      'trackSelection',
      'mode',
      VesperTrackSelectionMode.values.map((v) => v.name).toList(),
      'auto',
      (m) => VesperTrackSelection.fromMap(m).toMap()
    ),
    (
      'ABR',
      'mode',
      VesperAbrMode.values.map((v) => v.name).toList(),
      'auto',
      (m) => VesperAbrPolicy.fromMap(m).toMap()
    ),
    (
      'sourceNormalizer',
      'mode',
      VesperSourceNormalizerMode.values.map((v) => v.name).toList(),
      'disabled',
      (m) => VesperSourceNormalizerConfiguration.fromMap(m).toMap()
    ),
    (
      'nativeFrame',
      'mode',
      VesperNativeFramePipelineMode.values.map((v) => v.name).toList(),
      'disabled',
      (m) => VesperNativeFramePipelineConfiguration.fromMap(m).toMap()
    ),
    (
      'frameProcessor',
      'mode',
      VesperFrameProcessorMode.values.map((v) => v.name).toList(),
      'disabled',
      (m) => VesperFrameProcessorConfiguration.fromMap(m).toMap()
    ),
    (
      'systemPlayback',
      'backgroundMode',
      VesperBackgroundPlaybackMode.values.map((v) => v.name).toList(),
      'continueAudio',
      (m) => VesperSystemPlaybackConfiguration.fromMap(m).toMap()
    ),
    (
      'compactButton',
      'kind',
      VesperSystemPlaybackControlKind.values.map((v) => v.name).toList(),
      'playPause',
      (m) => VesperSystemPlaybackControlButton.fromMap(m).toMap()
    ),
    (
      'buffering',
      'preset',
      VesperBufferingPreset.values.map((v) => v.name).toList(),
      'defaultPreset',
      (m) => VesperBufferingPolicy.fromMap(m).toMap()
    ),
    (
      'cache',
      'preset',
      VesperCachePreset.values.map((v) => v.name).toList(),
      'defaultPreset',
      (m) => VesperCachePolicy.fromMap(m).toMap()
    ),
    (
      'externalProxyPolicy',
      'proxyPolicy',
      VesperExternalProxyPolicy.values.map((v) => v.name).toList(),
      'auto',
      (m) => VesperExternalPlaybackMediaItem.fromMap(m).toMap()
    ),
    (
      'externalPreferredFallback',
      'preferredFallback',
      VesperExternalFallbackFormat.values.map((v) => v.name).toList(),
      'mpegTs',
      (m) => VesperExternalFormatAdaptationConfig.fromMap(m).toMap()
    ),
  ];
  for (final (name, field, values, fallback, decode) in choices) {
    test('$name rejects unknown or wrongly typed explicit choices', () {
      expect(() => decode(<Object?, Object?>{1: values.first}),
          throwsFormatException);
      for (final invalid in <Object?>[
        'futureMode',
        '',
        3,
        true,
        <Object?>[],
        <String, Object?>{}
      ]) {
        expect(() => decode(<Object?, Object?>{field: invalid}),
            throwsFormatException,
            reason: '$name: $invalid');
      }
    });
    test('$name keeps absent/null defaults and round-trips known choices', () {
      expect(decode(<Object?, Object?>{})[field], fallback);
      expect(decode(<Object?, Object?>{field: null})[field], fallback);
      for (final value in values) {
        final wire =
            decode(<Object?, Object?>{field: value, 'trackId': 'video:001'});
        expect(wire[field], value);
        expect(decode(wire), wire);
      }
    });
  }

  test('buffering and cache retain the native default preset alias', () {
    expect(VesperBufferingPolicy.fromMap({'preset': 'default'}).preset,
        VesperBufferingPreset.defaultPreset);
    expect(VesperCachePolicy.fromMap({'preset': 'default'}).preset,
        VesperCachePreset.defaultPreset);
  });

  test(
      'retry distinguishes absent default, explicit unlimited and valid bounds',
      () {
    final absent = VesperRetryPolicy.fromMap({});
    expect(absent.maxAttempts, 3);
    expect(absent.toMap().containsKey('maxAttempts'), isFalse);
    final unlimited =
        VesperRetryPolicy.fromMap({'maxAttempts': null, 'backoff': null});
    expect(unlimited.maxAttempts, isNull);
    expect(unlimited.toMap().containsKey('maxAttempts'), isTrue);
    expect(unlimited.toMap()['maxAttempts'], isNull);
    for (final backoff in VesperRetryBackoff.values) {
      final wire = {
        'maxAttempts': 6,
        'baseDelayMs': 500,
        'maxDelayMs': 8000,
        'backoff': backoff.name
      };
      expect(VesperRetryPolicy.fromMap(wire).toMap(), wire);
    }
  });

  test(
      'retry rejects malformed maxAttempts and backoff without creating defaults',
      () {
    for (final invalid in <Object?>[
      '6',
      1.5,
      true,
      <Object?>[],
      <String, Object?>{}
    ]) {
      expect(() => VesperRetryPolicy.fromMap({'maxAttempts': invalid}),
          throwsFormatException);
      expect(() => VesperRetryPolicy(maxAttempts: invalid).maxAttempts,
          throwsArgumentError);
      expect(() => VesperRetryPolicy(maxAttempts: invalid).toMap(),
          throwsArgumentError);
    }
    for (final invalid in <Object?>[
      'futureBackoff',
      '',
      1,
      true,
      <Object?>[]
    ]) {
      expect(() => VesperRetryPolicy.fromMap({'backoff': invalid}),
          throwsFormatException);
    }
  });

  test('nested caller policies reject explicit non-map configurations', () {
    expect(
        () => VesperPlaybackResiliencePolicy.fromMap({
              'retry': {1: 3}
            }),
        throwsFormatException);
    expect(
        () => VesperSystemPlaybackControls.fromMap({
              'compactButtons': [
                {1: 'playPause'}
              ]
            }),
        throwsFormatException);
    expect(
        () => VesperPlaybackCapabilityProbeRequest.fromMap({
              'sourceNormalizer': {1: 'disabled'}
            }),
        throwsFormatException);
    for (final invalid in <Object?>['auto', 7, false, <Object?>[]]) {
      for (final field in ['buffering', 'retry', 'cache']) {
        expect(() => VesperPlaybackResiliencePolicy.fromMap({field: invalid}),
            throwsFormatException);
      }
      for (final field in [
        'audioSelection',
        'subtitleSelection',
        'abrPolicy'
      ]) {
        expect(() => VesperTrackPreferencePolicy.fromMap({field: invalid}),
            throwsFormatException);
      }
      for (final field in [
        'sourceNormalizer',
        'frameProcessor',
        'nativeFramePipeline'
      ]) {
        expect(
            () =>
                VesperPlaybackCapabilityProbeRequest.fromMap({field: invalid}),
            throwsFormatException);
      }
      expect(
          () =>
              VesperSystemPlaybackConfiguration.fromMap({'controls': invalid}),
          throwsFormatException);
    }
    expect(
        VesperPlaybackResiliencePolicy.fromMap({'retry': null})
            .retry
            .maxAttempts,
        3);
    expect(
        VesperPlaybackCapabilityProbeRequest.fromMap({'sourceNormalizer': null})
            .sourceNormalizerConfiguration
            .mode,
        VesperSourceNormalizerMode.disabled);
  });

  test('compact buttons validate every entry before deliberate normalization',
      () {
    for (final invalid in <Object?>[
      'playPause',
      false,
      7,
      <String, Object?>{}
    ]) {
      expect(
          () =>
              VesperSystemPlaybackControls.fromMap({'compactButtons': invalid}),
          throwsFormatException);
    }
    for (final invalid in <Object?>[null, 'seekBack', 7]) {
      expect(
          () => VesperSystemPlaybackControls.fromMap({
                'compactButtons': [
                  {'kind': 'playPause'},
                  invalid
                ]
              }),
          throwsFormatException);
    }
    expect(
        () => VesperSystemPlaybackControls.fromMap({
              'compactButtons': [
                {'kind': 'playPause'},
                {'kind': 'seekBack'},
                {'kind': 'seekForward'},
                {'kind': 'futureKind'}
              ]
            }),
        throwsFormatException);
    expect(
        VesperSystemPlaybackControls.fromMap({'compactButtons': null}).toMap(),
        const VesperSystemPlaybackControls.videoDefault().toMap());
  });

  test('external adaptation rejects malformed nested maps and keys', () {
    for (final invalid in <Object?>[
      'disabled',
      7,
      true,
      <Object?>[],
      <Object?, Object?>{1: 'hls'},
      <String, Object?>{'preferredFallback': 'futureFormat'},
    ]) {
      expect(
        () => VesperExternalPlaybackMediaItem.fromMap(
            {'formatAdaptation': invalid}),
        throwsFormatException,
      );
    }
  });

  test('external header collections reject bad containers and every bad entry',
      () {
    for (final invalid in <Object?>[
      'User-Agent',
      7,
      true,
      <String, Object?>{'User-Agent': true},
    ]) {
      expect(
        () => VesperExternalFormatAdaptationConfig.fromMap(
            {'remoteDashMediaRequestHeaders': invalid}),
        throwsFormatException,
      );
    }
    for (final invalid in <Object?>[
      null,
      7,
      true,
      <Object?>[],
      <String, Object?>{},
    ]) {
      expect(
        () => VesperExternalFormatAdaptationConfig.fromMap({
          'remoteDashMediaRequestHeaders': ['User-Agent', invalid, 'Accept'],
        }),
        throwsFormatException,
      );
    }
    expect(
      () => VesperExternalFormatAdaptationConfig.fromMap({
        'remoteDashMediaRequestHeaders': <Object?>{'User-Agent', 7},
      }),
      throwsFormatException,
    );
  });

  test('external headers keep defaults only for absent or null collections',
      () {
    const defaults = VesperExternalFormatAdaptationConfig.disabled();
    for (final map in <Map<Object?, Object?>>[
      {},
      {'remoteDashMediaRequestHeaders': null},
    ]) {
      expect(VesperExternalFormatAdaptationConfig.fromMap(map).toMap(),
          defaults.toMap());
    }
    for (final empty in <Iterable<String>>[
      <String>[],
      <String>{},
      <String>[].map((value) => value),
    ]) {
      final config = VesperExternalFormatAdaptationConfig.fromMap(
          {'remoteDashMediaRequestHeaders': empty});
      expect(config.remoteDashMediaRequestHeaders, isEmpty);
      expect(config.hasOverrides, isTrue);
      expect(config.toMap()['remoteDashMediaRequestHeaders'], <String>[]);
      final item = VesperExternalPlaybackMediaItem(
        sources: const [],
        metadata: const VesperSystemPlaybackMetadata(title: 'Fixture'),
        formatAdaptation: config,
      );
      final wire = item.toMap();
      expect(wire.containsKey('formatAdaptation'), isTrue);
      final decoded = VesperExternalPlaybackMediaItem.fromMap(wire);
      expect(decoded.formatAdaptation.remoteDashMediaRequestHeaders, isEmpty);
      expect(decoded.toMap(), wire);
    }
  });

  test('external header iterables preserve strings and serialize as lists', () {
    final names = <String>['', ' ', 'User-Agent', 'user-agent', ' 中文 ', 'é'];
    for (final input in <Iterable<String>>[
      [...names, names.first],
      names.toSet(),
      names.map((value) => value),
    ]) {
      final config = VesperExternalFormatAdaptationConfig.fromMap(
          {'remoteDashMediaRequestHeaders': input});
      expect(config.remoteDashMediaRequestHeaders, names.toSet());
      final wire = config.toMap();
      expect(wire['remoteDashMediaRequestHeaders'], names);
      expect(VesperExternalFormatAdaptationConfig.fromMap(wire).toMap(), wire);
    }
  });

  test('external caller choices round-trip through the media channel map', () {
    for (final proxyPolicy in VesperExternalProxyPolicy.values) {
      for (final fallback in VesperExternalFallbackFormat.values) {
        final item = VesperExternalPlaybackMediaItem(
          sources: [VesperPlayerSource.dash(uri: 'https://fixture.test/a.mpd')],
          metadata: const VesperSystemPlaybackMetadata(title: 'Fixture'),
          proxyPolicy: proxyPolicy,
          formatAdaptation: VesperExternalFormatAdaptationConfig.dlnaRemux(
            preferredFallback: fallback,
            remoteDashMediaRequestHeaders: const <String>{},
          ),
        );
        final wire = item.toMap();
        expect(VesperExternalPlaybackMediaItem.fromMap(wire).toMap(), wire);
      }
    }
  });

  test('external event snapshots retain unknown kind and raw value', () {
    final event =
        VesperExternalPlaybackSessionEvent.fromMap({'kind': 'futureEvent'});
    expect(event.kind, VesperExternalPlaybackSessionEventKind.unknown);
    expect(event.rawKind, 'futureEvent');
  });
}
