import 'package:flutter_test/flutter_test.dart';
import 'package:flutter_host/src/player/example_dolby_acceptance_catalog.dart';
import 'package:flutter_host/src/player/example_player_models.dart';
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

  test('host platform routing keeps iOS Dolby acceptance on HLS direct', () {
    final dashClear = exampleDolbyAcceptancePresetById(
      'DOLBY-DV-P5-24-DASH-CLEAR',
    )!;
    final hlsClear = exampleDolbyAcceptancePresetById(
      'DOLBY-DV-P5-24-HLS-CLEAR',
    )!;
    final widevine = exampleDolbyAcceptancePresetById(
      'DOLBY-DV-P84-50-DASH-WIDEVINE',
    )!;

    expect(
      exampleDolbyAcceptancePresetIsPlayableOnHost(
        dashClear,
        isAndroid: false,
        isIOS: true,
      ),
      isFalse,
    );
    expect(
      exampleDolbyAcceptancePresetUnavailableReasonOnHost(
        dashClear,
        isAndroid: false,
        isIOS: true,
      ),
      contains('HLS direct'),
    );
    expect(
      exampleDolbyAcceptancePresetIsPlayableOnHost(
        hlsClear,
        isAndroid: false,
        isIOS: true,
      ),
      isTrue,
    );
    expect(
      exampleDolbyAcceptancePresetUnavailableReasonOnHost(
        widevine,
        isAndroid: false,
        isIOS: true,
      ),
      contains('Android-only'),
    );
    expect(
      exampleDolbyAcceptancePresetIsPlayableOnHost(
        widevine,
        isAndroid: true,
        isIOS: false,
      ),
      isTrue,
    );
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

  test('filter returns presets for selected drm, profile, and fps', () {
    final filtered = filterDolbyAcceptancePresets(
      presets: exampleDolbyAcceptanceCatalog,
      drmKind: ExampleDolbyAcceptanceDrmKind.clear,
      profile: ExampleDolbyAcceptanceProfile.p81,
      fps: 50,
    );

    expect(filtered, hasLength(2));
    expect(
      filtered.every(
        (preset) => preset.drmKind == ExampleDolbyAcceptanceDrmKind.clear,
      ),
      isTrue,
    );
    expect(
      filtered.every(
        (preset) => preset.profile == ExampleDolbyAcceptanceProfile.p81,
      ),
      isTrue,
    );
    expect(filtered.every((preset) => preset.fps == 50), isTrue);
  });

  test('dolby queue ids use explicit prefix and resolve preset id', () {
    final preset = exampleDolbyAcceptancePresetById(
      'DOLBY-DV-P5-24-HLS-CLEAR',
    )!;
    final itemId = flutterDolbyPlaylistItemId(preset.id);

    expect(itemId, 'dolby-${preset.id}');
    expect(flutterDolbyPresetIdFromPlaylistItemId(itemId), preset.id);
    expect(flutterDolbyPresetIdFromPlaylistItemId(preset.id), isNull);
  });

  test('host queueability follows platform playable rules', () {
    final widevine = exampleDolbyAcceptancePresetById(
      'DOLBY-DV-P84-50-DASH-WIDEVINE',
    )!;
    final fairPlayPending = exampleDolbyAcceptancePresetById(
      'DOLBY-DV-P5-24-HLS-FAIRPLAY-PENDING',
    )!;
    final hlsClear = exampleDolbyAcceptancePresetById(
      'DOLBY-DV-P5-24-HLS-CLEAR',
    )!;

    expect(
      canQueueDolbyAcceptancePresetOnHost(
        widevine,
        isAndroid: true,
        isIOS: false,
      ),
      isTrue,
    );
    expect(
      canQueueDolbyAcceptancePresetOnHost(
        widevine,
        isAndroid: false,
        isIOS: true,
      ),
      isFalse,
    );
    expect(
      canQueueDolbyAcceptancePresetOnHost(
        fairPlayPending,
        isAndroid: true,
        isIOS: false,
      ),
      isFalse,
    );
    expect(
      canQueueDolbyAcceptancePresetOnHost(
        hlsClear,
        isAndroid: false,
        isIOS: true,
      ),
      isTrue,
    );
  });

  test('Dolby Browser Test Kit sources require direct native playback', () {
    final dolby = exampleDolbyAcceptancePresetById('DOLBY-DV-P5-24-HLS-CLEAR')!;

    expect(exampleDolbyAcceptancePresetForSource(dolby.source), same(dolby));
    expect(
      exampleDolbyAcceptanceSourceRequiresDirectNativePlayback(dolby.source),
      isTrue,
    );
    expect(
      exampleDolbyAcceptanceSourceRequiresDirectNativePlayback(
        flutterHlsDemoSource(),
      ),
      isFalse,
    );
  });

  test('all Dolby acceptance profiles and DRM variants require direct native playback', () {
    final profileSet = <ExampleDolbyAcceptanceProfile>{};
    final drmSet = <ExampleDolbyAcceptanceDrmKind>{};
    final protocolSet = <VesperPlayerSourceProtocol>{};

    for (final preset in exampleDolbyAcceptanceCatalog) {
      profileSet.add(preset.profile);
      drmSet.add(preset.drmKind);
      protocolSet.add(preset.protocol);
      expect(
        exampleDolbyAcceptancePresetForSource(preset.source),
        same(preset),
        reason: preset.id,
      );
      expect(
        exampleDolbyAcceptanceSourceRequiresDirectNativePlayback(
          preset.source,
        ),
        isTrue,
        reason: preset.id,
      );
    }

    expect(profileSet, ExampleDolbyAcceptanceProfile.values.toSet());
    expect(drmSet, ExampleDolbyAcceptanceDrmKind.values.toSet());
    expect(
      protocolSet,
      <VesperPlayerSourceProtocol>{
        VesperPlayerSourceProtocol.dash,
        VesperPlayerSourceProtocol.hls,
      },
    );
  });
}
