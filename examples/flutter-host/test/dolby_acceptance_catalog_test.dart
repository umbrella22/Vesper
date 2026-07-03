import 'package:flutter_test/flutter_test.dart';
import 'package:flutter_host/src/player/example_dolby_acceptance_catalog.dart';
import 'package:vesper_player/vesper_player.dart';

void main() {
  test('clear presets use Dolby Browser Test Kit DASH and HLS URLs', () {
    final dash = exampleDolbyAcceptancePresetById('DOLBY-DV-P5-24-DASH-CLEAR');
    final hls = exampleDolbyAcceptancePresetById('DOLBY-DV-P81-30-HLS-CLEAR');

    expect(
      dash?.source.uri,
      'https://ott.dolby.com/browser_test_kit/clear/p5/24/dash.mpd',
    );
    expect(dash?.source.protocol, VesperPlayerSourceProtocol.dash);
    expect(dash?.source.drmConfiguration, isNull);
    expect(
      hls?.source.uri,
      'https://ott.dolby.com/browser_test_kit/clear/p81/30/master.m3u8',
    );
    expect(hls?.source.protocol, VesperPlayerSourceProtocol.hls);
    expect(hls?.source.drmConfiguration, isNull);
  });

  test('widevine presets are DASH direct sources with DRM configuration', () {
    final widevine = exampleDolbyAcceptancePresetById(
      'DOLBY-DV-P84-50-DASH-WIDEVINE',
    );

    expect(widevine, isNotNull);
    expect(widevine?.protocol, VesperPlayerSourceProtocol.dash);
    expect(
      widevine?.source.uri,
      'https://ott.dolby.com/browser_test_kit/cenc/p84/50/dash.mpd',
    );
    expect(widevine?.source.drmConfiguration?.keySystem, 'widevine');
    expect(
      widevine?.source.drmConfiguration?.licenseUri,
      exampleDolbyAcceptanceWidevineLicenseUri,
    );
    expect(widevine?.source.drmConfiguration?.licenseHeaders, isEmpty);
    expect(widevine?.isPlayable, isTrue);
  });

  test('fairplay presets remain pending and disabled', () {
    final fairPlay = exampleDolbyAcceptancePresetById(
      'DOLBY-DV-P5-24-HLS-FAIRPLAY-PENDING',
    );

    expect(fairPlay, isNotNull);
    expect(
      fairPlay?.source.uri,
      'https://ott.dolby.com/browser_test_kit/cbcs/p5/24/master.m3u8',
    );
    expect(fairPlay?.source.protocol, VesperPlayerSourceProtocol.hls);
    expect(fairPlay?.source.drmConfiguration, isNull);
    expect(fairPlay?.enabled, isFalse);
    expect(fairPlay?.isPlayable, isFalse);
  });

  test(
    'metadata preserves profile, fps, drm, expected HDR, and manual gate',
    () {
      final preset = exampleDolbyAcceptancePresetById(
        'DOLBY-DV-P81-120-DASH-WIDEVINE',
      )!;
      final evidence = preset.toHdrEvidencePreset();
      final metadata = evidence.sourceMetadata;
      final dolbyVision = metadata['dolbyVision'] as Map<String, Object?>;

      expect(evidence.sampleId, preset.id);
      expect(metadata['hdrKind'], 'dolbyVision');
      expect(metadata['frameRate'], 120.0);
      expect(metadata['drmKind'], 'widevine');
      expect(metadata['manualGate'], 'requiresDolbyVisionDisplay');
      expect(dolbyVision['profile'], 8);
      expect(dolbyVision['profileFamily'], 'profile8.1');
    },
  );

  test('catalog covers selected profile, fps, protocol, and DRM matrix', () {
    expect(exampleDolbyAcceptanceCatalog, hasLength(48));
    for (final profile in ExampleDolbyAcceptanceProfile.values) {
      for (final fps in exampleDolbyAcceptanceFpsValues) {
        expect(
          exampleDolbyAcceptanceCatalog.where(
            (preset) => preset.profile == profile && preset.fps == fps,
          ),
          hasLength(4),
        );
      }
    }
  });
}
