import 'package:flutter_test/flutter_test.dart';
import 'package:vesper_player_platform_interface/vesper_player_platform_interface.dart';

void main() {
  const fields = <String, bool>{
    'enabled': false,
    'allowHls': true,
    'enableRangeCache': true,
    'allowRemoteDashMediaReferences': false,
    'allowPrivateRemoteDashMediaAddresses': false,
    'debugDiagnostics': false,
  };
  for (final entry in fields.entries) {
    test('external ${entry.key} rejects explicitly malformed booleans', () {
      for (final invalid in <Object?>[
        'true',
        1,
        0.0,
        <Object?>[],
        <String, Object?>{}
      ]) {
        expect(
          () => VesperExternalFormatAdaptationConfig.fromMap(
              {entry.key: invalid}),
          throwsFormatException,
        );
        expect(
          () => VesperExternalPlaybackMediaItem.fromMap({
            'formatAdaptation': {entry.key: invalid},
          }),
          throwsFormatException,
        );
      }
    });
    test('external ${entry.key} preserves defaults and explicit booleans', () {
      for (final input in <Map<Object?, Object?>>[
        {},
        {entry.key: null}
      ]) {
        expect(
            VesperExternalFormatAdaptationConfig.fromMap(input)
                .toMap()[entry.key],
            entry.value);
      }
      for (final value in [false, true]) {
        final config =
            VesperExternalFormatAdaptationConfig.fromMap({entry.key: value});
        final item = VesperExternalPlaybackMediaItem(
          sources: const [],
          metadata: const VesperSystemPlaybackMetadata(title: 'Fixture'),
          formatAdaptation: config,
        );
        final decoded = VesperExternalPlaybackMediaItem.fromMap(item.toMap());
        expect(decoded.formatAdaptation.toMap()[entry.key], value);
        expect(decoded.toMap(), item.toMap());
      }
    });
  }
}
