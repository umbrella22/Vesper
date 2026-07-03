import 'package:vesper_player/vesper_player.dart';

import '../hdr_evidence/hdr_evidence_capture.dart';

const String exampleDolbyAcceptanceWidevineLicenseUri =
    'https://widevine-dash.ezdrm.com/proxy?pX=E8A6EE';

const List<int> exampleDolbyAcceptanceFpsValues = <int>[24, 30, 50, 120];

enum ExampleDolbyAcceptanceProfile { p5, p81, p84 }

extension ExampleDolbyAcceptanceProfileLabels on ExampleDolbyAcceptanceProfile {
  String get pathSegment {
    return switch (this) {
      ExampleDolbyAcceptanceProfile.p5 => 'p5',
      ExampleDolbyAcceptanceProfile.p81 => 'p81',
      ExampleDolbyAcceptanceProfile.p84 => 'p84',
    };
  }

  String get title {
    return switch (this) {
      ExampleDolbyAcceptanceProfile.p5 => 'P5',
      ExampleDolbyAcceptanceProfile.p81 => 'P8.1',
      ExampleDolbyAcceptanceProfile.p84 => 'P8.4',
    };
  }

  String get sampleIdSegment {
    return switch (this) {
      ExampleDolbyAcceptanceProfile.p5 => 'P5',
      ExampleDolbyAcceptanceProfile.p81 => 'P81',
      ExampleDolbyAcceptanceProfile.p84 => 'P84',
    };
  }

  int get dolbyVisionProfile {
    return switch (this) {
      ExampleDolbyAcceptanceProfile.p5 => 5,
      ExampleDolbyAcceptanceProfile.p81 ||
      ExampleDolbyAcceptanceProfile.p84 => 8,
    };
  }

  String get profileFamily {
    return switch (this) {
      ExampleDolbyAcceptanceProfile.p5 => 'profile5',
      ExampleDolbyAcceptanceProfile.p81 => 'profile8.1',
      ExampleDolbyAcceptanceProfile.p84 => 'profile8.4',
    };
  }

  String get fallbackTarget {
    return switch (this) {
      ExampleDolbyAcceptanceProfile.p5 => 'none',
      ExampleDolbyAcceptanceProfile.p81 => 'hdr10',
      ExampleDolbyAcceptanceProfile.p84 => 'hlg',
    };
  }

  String get transferFunction {
    return switch (this) {
      ExampleDolbyAcceptanceProfile.p84 => 'ARIB_STD_B67_HLG',
      ExampleDolbyAcceptanceProfile.p5 ||
      ExampleDolbyAcceptanceProfile.p81 => 'SMPTE_ST_2084_PQ',
    };
  }
}

enum ExampleDolbyAcceptanceDrmKind { clear, widevine, fairPlayPending }

extension ExampleDolbyAcceptanceDrmKindLabels on ExampleDolbyAcceptanceDrmKind {
  String get title {
    return switch (this) {
      ExampleDolbyAcceptanceDrmKind.clear => 'Clear',
      ExampleDolbyAcceptanceDrmKind.widevine => 'Widevine',
      ExampleDolbyAcceptanceDrmKind.fairPlayPending => 'FairPlay pending',
    };
  }

  String get sampleIdSegment {
    return switch (this) {
      ExampleDolbyAcceptanceDrmKind.clear => 'CLEAR',
      ExampleDolbyAcceptanceDrmKind.widevine => 'WIDEVINE',
      ExampleDolbyAcceptanceDrmKind.fairPlayPending => 'FAIRPLAY-PENDING',
    };
  }

  String get metadataValue {
    return switch (this) {
      ExampleDolbyAcceptanceDrmKind.clear => 'none',
      ExampleDolbyAcceptanceDrmKind.widevine => 'widevine',
      ExampleDolbyAcceptanceDrmKind.fairPlayPending => 'fairPlayPending',
    };
  }
}

final class ExampleDolbyAcceptancePreset {
  const ExampleDolbyAcceptancePreset({
    required this.id,
    required this.label,
    required this.profile,
    required this.fps,
    required this.protocol,
    required this.drmKind,
    required this.source,
    required this.expectedHdrKind,
    required this.manualGate,
    this.notes = const <String>[],
    this.enabled = true,
  });

  final String id;
  final String label;
  final ExampleDolbyAcceptanceProfile profile;
  final int fps;
  final VesperPlayerSourceProtocol protocol;
  final ExampleDolbyAcceptanceDrmKind drmKind;
  final VesperPlayerSource source;
  final String expectedHdrKind;
  final String manualGate;
  final List<String> notes;
  final bool enabled;

  bool get isDrm => drmKind != ExampleDolbyAcceptanceDrmKind.clear;

  bool get isPlayable =>
      enabled && drmKind != ExampleDolbyAcceptanceDrmKind.fairPlayPending;

  String get protocolLabel {
    return switch (protocol) {
      VesperPlayerSourceProtocol.dash => 'DASH',
      VesperPlayerSourceProtocol.hls => 'HLS',
      _ => protocol.name,
    };
  }

  ExampleHdrEvidenceSamplePreset toHdrEvidencePreset() {
    return ExampleHdrEvidenceSamplePreset(
      sampleId: id,
      label: label,
      expectedAxis: 'display',
      sourceMetadata: <String, Object?>{
        'sourceUri': source.uri,
        'sourceKind': 'remote',
        'container': protocolLabel.toLowerCase(),
        'manifestKind': protocolLabel.toLowerCase(),
        'codec': 'dolby-vision',
        'sampleMimeType': 'video/dolby-vision',
        'width': null,
        'height': null,
        'frameRate': fps.toDouble(),
        'bitDepth': 10,
        'hdrKind': expectedHdrKind,
        'colorPrimaries': 'BT.2020',
        'transferFunction': profile.transferFunction,
        'yCbCrMatrix': 'BT.2020_NCL',
        'drmKind': drmKind.metadataValue,
        'manualGate': manualGate,
        'controlPurpose': 'dolbyVisionAcceptance',
        'dolbyVision': <String, Object?>{
          'profile': profile.dolbyVisionProfile,
          'profileFamily': profile.profileFamily,
          'baseLayer': 'hevc-main10',
          'fallbackTarget': profile.fallbackTarget,
          'containerEvidence': 'dolby-browser-test-kit',
        },
        'metadataTool': <String, Object?>{
          'name': 'Dolby Browser Test Kit',
          'version': 'public',
          'command': 'catalog-url',
        },
        'notes': <String>[
          'Dolby Browser Test Kit public URL; media is not bundled.',
          ...notes,
        ],
      },
    );
  }
}

String exampleDolbyAcceptanceUrl({
  required ExampleDolbyAcceptanceProfile profile,
  required int fps,
  required VesperPlayerSourceProtocol protocol,
  required ExampleDolbyAcceptanceDrmKind drmKind,
}) {
  final protocolFile = switch (protocol) {
    VesperPlayerSourceProtocol.dash => 'dash.mpd',
    VesperPlayerSourceProtocol.hls => 'master.m3u8',
    _ => throw ArgumentError.value(protocol, 'protocol', 'DASH or HLS only'),
  };
  final pathKind = switch (drmKind) {
    ExampleDolbyAcceptanceDrmKind.clear => 'clear',
    ExampleDolbyAcceptanceDrmKind.widevine => 'cenc',
    ExampleDolbyAcceptanceDrmKind.fairPlayPending => 'cbcs',
  };
  return 'https://ott.dolby.com/browser_test_kit/$pathKind/'
      '${profile.pathSegment}/$fps/$protocolFile';
}

List<ExampleDolbyAcceptancePreset> buildExampleDolbyAcceptanceCatalog() {
  return <ExampleDolbyAcceptancePreset>[
    for (final profile in ExampleDolbyAcceptanceProfile.values)
      for (final fps
          in exampleDolbyAcceptanceFpsValues) ...<ExampleDolbyAcceptancePreset>[
        _buildDolbyPreset(
          profile: profile,
          fps: fps,
          protocol: VesperPlayerSourceProtocol.dash,
          drmKind: ExampleDolbyAcceptanceDrmKind.clear,
        ),
        _buildDolbyPreset(
          profile: profile,
          fps: fps,
          protocol: VesperPlayerSourceProtocol.hls,
          drmKind: ExampleDolbyAcceptanceDrmKind.clear,
        ),
        _buildDolbyPreset(
          profile: profile,
          fps: fps,
          protocol: VesperPlayerSourceProtocol.dash,
          drmKind: ExampleDolbyAcceptanceDrmKind.widevine,
        ),
        _buildDolbyPreset(
          profile: profile,
          fps: fps,
          protocol: VesperPlayerSourceProtocol.hls,
          drmKind: ExampleDolbyAcceptanceDrmKind.fairPlayPending,
          enabled: false,
        ),
      ],
  ];
}

final List<ExampleDolbyAcceptancePreset> exampleDolbyAcceptanceCatalog =
    buildExampleDolbyAcceptanceCatalog();

ExampleDolbyAcceptancePreset? exampleDolbyAcceptancePresetById(String id) {
  for (final preset in exampleDolbyAcceptanceCatalog) {
    if (preset.id == id) {
      return preset;
    }
  }
  return null;
}

List<ExampleHdrEvidenceSamplePreset>
exampleDolbyAcceptanceHdrEvidencePresets() {
  return exampleDolbyAcceptanceCatalog
      .where((preset) => preset.isPlayable)
      .map((preset) => preset.toHdrEvidencePreset())
      .toList(growable: false);
}

ExampleDolbyAcceptancePreset _buildDolbyPreset({
  required ExampleDolbyAcceptanceProfile profile,
  required int fps,
  required VesperPlayerSourceProtocol protocol,
  required ExampleDolbyAcceptanceDrmKind drmKind,
  bool enabled = true,
}) {
  final protocolSegment = switch (protocol) {
    VesperPlayerSourceProtocol.dash => 'DASH',
    VesperPlayerSourceProtocol.hls => 'HLS',
    _ => throw ArgumentError.value(protocol, 'protocol', 'DASH or HLS only'),
  };
  final id =
      'DOLBY-DV-${profile.sampleIdSegment}-$fps-'
      '$protocolSegment-${drmKind.sampleIdSegment}';
  final label = '${profile.title} ${fps}fps $protocolSegment ${drmKind.title}';
  final uri = exampleDolbyAcceptanceUrl(
    profile: profile,
    fps: fps,
    protocol: protocol,
    drmKind: drmKind,
  );
  final source = VesperPlayerSource(
    uri: uri,
    label: label,
    kind: VesperPlayerSourceKind.remote,
    protocol: protocol,
    drmConfiguration: drmKind == ExampleDolbyAcceptanceDrmKind.widevine
        ? const VesperPlayerDrmConfiguration(
            keySystem: 'widevine',
            licenseUri: exampleDolbyAcceptanceWidevineLicenseUri,
          )
        : null,
  );
  final notes = <String>[
    if (drmKind == ExampleDolbyAcceptanceDrmKind.widevine)
      'Widevine DASH direct native route only.',
    if (drmKind == ExampleDolbyAcceptanceDrmKind.fairPlayPending)
      'FairPlay certificate URI/base64 is not available yet; preset is disabled.',
    if (fps == 50) 'Dolby 50fps signal covers the 60-ish validation bucket.',
    'MP4 zip assets remain manual local-file material and are not bundled.',
  ];
  return ExampleDolbyAcceptancePreset(
    id: id,
    label: label,
    profile: profile,
    fps: fps,
    protocol: protocol,
    drmKind: drmKind,
    source: source,
    expectedHdrKind: 'dolbyVision',
    manualGate: 'requiresDolbyVisionDisplay',
    notes: notes,
    enabled: enabled,
  );
}
