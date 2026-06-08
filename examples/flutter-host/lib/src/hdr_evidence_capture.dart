import 'dart:async';
import 'dart:convert';
import 'dart:io';

import 'package:vesper_player/vesper_player.dart';

const String _deviceSchema = 'vesper-hdr-dv-device-v1';
const String _sourceSchema = 'vesper-hdr-dv-source-metadata-v1';
const String _probeHostSchema = 'vesper-hdr-dv-probe-host-v1';
const String _probeFlutterSchema = 'vesper-hdr-dv-probe-flutter-v1';
const String _runtimeWarningSchema = 'vesper-hdr-dv-runtime-warning-v1';
const String _runtimeErrorSchema = 'vesper-hdr-dv-runtime-error-v1';
const String _typedEvidenceSchema = 'vesper-hdr-dv-typed-evidence-v1';

const String exampleHdrEvidenceNetworkControlUrl =
    'https://127.0.0.1:9/vesper-hdr-network-control.mp4';

const Map<String, String> _androidRows = <String, String>{
  'HDR10-HEVC-MAIN10-2160P60-PQ': 'A1',
  'HDR10-HEVC-MAIN10-1080P120-PQ': 'A2',
  'HLG-HEVC-2160P60': 'A3',
  'DV-P5-SINGLE-LAYER': 'A4',
  'DV-P8-COMPATIBLE-BL': 'A5',
  'HEVC-SDR-CONTROL': 'A6',
  'NETWORK-FAILURE-CONTROL': 'A7',
};

const Map<String, String> _iosRows = <String, String>{
  'HDR10-HEVC-MAIN10-2160P60-PQ': 'I1',
  'HDR10-HEVC-MAIN10-1080P120-PQ': 'I2',
  'HLG-HEVC-2160P60': 'I3',
  'DV-P5-SINGLE-LAYER': 'I4',
  'DV-P8-COMPATIBLE-BL': 'I5',
  'HEVC-SDR-CONTROL': 'I6',
  'NETWORK-FAILURE-CONTROL': 'I7',
  'PERMISSION-FAILURE-CONTROL': 'I8',
};

final class ExampleHdrEvidenceSamplePreset {
  const ExampleHdrEvidenceSamplePreset({
    required this.sampleId,
    required this.label,
    required this.expectedAxis,
    required this.sourceMetadata,
  });

  final String sampleId;
  final String label;
  final String expectedAxis;
  final Map<String, Object?> sourceMetadata;
}

const List<ExampleHdrEvidenceSamplePreset> exampleHdrEvidenceP0Presets =
    <ExampleHdrEvidenceSamplePreset>[
      ExampleHdrEvidenceSamplePreset(
        sampleId: 'HDR10-HEVC-MAIN10-2160P60-PQ',
        label: 'HDR10 4K60 PQ',
        expectedAxis: 'display',
        sourceMetadata: <String, Object?>{
          'container': 'TBD',
          'codec': 'hvc1',
          'sampleMimeType': 'video/hevc',
          'width': 3840,
          'height': 2160,
          'frameRate': 60.0,
          'bitDepth': 10,
          'hdrKind': 'hdr10',
          'colorPrimaries': 'BT.2020',
          'transferFunction': 'SMPTE_ST_2084_PQ',
          'yCbCrMatrix': 'BT.2020_NCL',
          'controlPurpose': 'none',
        },
      ),
      ExampleHdrEvidenceSamplePreset(
        sampleId: 'HEVC-SDR-CONTROL',
        label: 'HEVC SDR control',
        expectedAxis: 'none',
        sourceMetadata: <String, Object?>{
          'container': 'TBD',
          'codec': 'hvc1',
          'sampleMimeType': 'video/hevc',
          'width': 1920,
          'height': 1080,
          'frameRate': 30.0,
          'bitDepth': 8,
          'hdrKind': 'none',
          'colorPrimaries': 'BT.709',
          'transferFunction': 'BT.709',
          'yCbCrMatrix': 'BT.709',
          'controlPurpose': 'hevcSdrFalsePositive',
        },
      ),
      ExampleHdrEvidenceSamplePreset(
        sampleId: 'NETWORK-FAILURE-CONTROL',
        label: 'Network failure control',
        expectedAxis: 'network',
        sourceMetadata: <String, Object?>{
          'sourceKind': 'progressive',
          'container': 'mp4',
          'codec': 'none',
          'sampleMimeType': 'video/mp4',
          'hdrKind': 'none',
          'sourceUri': exampleHdrEvidenceNetworkControlUrl,
          'manifestKind': 'none',
          'controlPurpose': 'networkFailure',
        },
      ),
    ];

final class ExampleHdrEvidenceBundle {
  const ExampleHdrEvidenceBundle({
    required this.sampleId,
    required this.deviceId,
    required this.platform,
    required this.captureDate,
    required this.sdkCommit,
    required this.sourceMetadata,
    required this.device,
    required this.flutterProbe,
    this.hostProbe,
    this.playbackOutcome = 'notRun',
    this.runtimeWarning,
    this.runtimeError,
    this.expectedAxis = 'inconclusive',
    this.axisSupportedByEvidence,
    this.missingEvidence = const <String>[],
    this.matchesHostProbe,
    this.matchesHostEvidence,
    this.probeMismatches = const <String>[],
    this.evidenceMismatches = const <String>[],
    this.platformLog = '',
    this.notes,
  });

  final String sampleId;
  final String deviceId;
  final String platform;
  final String captureDate;
  final String sdkCommit;
  final Map<String, Object?> sourceMetadata;
  final Map<String, Object?> device;
  final VesperPlaybackCapabilityProbeResult flutterProbe;
  final VesperPlaybackCapabilityProbeResult? hostProbe;
  final String playbackOutcome;
  final VesperCapabilityWarning? runtimeWarning;
  final VesperPlayerError? runtimeError;
  final String expectedAxis;
  final bool? axisSupportedByEvidence;
  final List<String> missingEvidence;
  final bool? matchesHostProbe;
  final bool? matchesHostEvidence;
  final List<String> probeMismatches;
  final List<String> evidenceMismatches;
  final String platformLog;
  final String? notes;

  Map<String, Object?> deviceJson() {
    return _mergeMaps(<String, Object?>{
      'schema': _deviceSchema,
      'deviceId': deviceId,
      'platform': platform,
      'captureDate': captureDate,
      'sdkCommit': sdkCommit,
      'hostApp': <String, Object?>{
        'name': 'flutter-host',
        'version': 'debug',
        'displayPath': platform == 'ios' ? 'AVPlayer' : 'SurfaceView',
      },
      'android': <String, Object?>{
        'manufacturer': 'TBD',
        'model': 'TBD',
        'apiLevel': 'TBD',
        'buildFingerprint': 'TBD',
        'displayHdrTypes': const <String>[],
        'displayRefreshRate': null,
        'displayModes': const <String>[],
        'media3Version': 'TBD',
        'decoderCandidates': <String, Object?>{
          'hevc': const <String>[],
          'dolbyVision': const <String>[],
        },
      },
      'ios': <String, Object?>{
        'model': 'TBD',
        'iosVersion': 'TBD',
        'avPlayerEligibleForHdrPlayback': null,
        'displayGamut': 'TBD',
        'nativeDisplaySize': <String, Object?>{'width': null, 'height': null},
        'maximumFramesPerSecond': null,
      },
      'knownCaveats': const <String>[],
    }, device);
  }

  Map<String, Object?> sourceMetadataJson() {
    return _mergeMaps(<String, Object?>{
      'schema': _sourceSchema,
      'sampleId': sampleId,
      'sourceKind': 'TBD',
      'sourceUri': 'TBD',
      'container': 'TBD',
      'manifestKind': 'none',
      'codec': 'TBD',
      'sampleMimeType': 'TBD',
      'width': null,
      'height': null,
      'frameRate': null,
      'bitDepth': null,
      'hdrKind': flutterProbe.hdrKind.name,
      'colorPrimaries': 'TBD',
      'transferFunction': 'TBD',
      'yCbCrMatrix': 'TBD',
      'maxContentLightLevelNits': null,
      'maxFrameAverageLightLevelNits': null,
      'masteringDisplay': <String, Object?>{
        'present': null,
        'primary0': null,
        'primary1': null,
        'primary2': null,
        'whitePoint': null,
        'maxLuminanceNits': null,
        'minLuminanceNits': null,
      },
      'dolbyVision': <String, Object?>{
        'codec': flutterProbe.hdrMetadata?.dolbyVisionCodec,
        'profile': flutterProbe.hdrMetadata?.dolbyVisionProfile,
        'level': flutterProbe.hdrMetadata?.dolbyVisionLevel,
        'compatibility': flutterProbe.hdrMetadata?.dolbyVisionCompatibility,
        'profileFamily': flutterProbe.hdrMetadata?.dolbyVisionProfileFamily,
        'baseLayer': flutterProbe.hdrMetadata?.dolbyVisionBaseLayer,
        'fallbackTarget': flutterProbe.hdrMetadata?.dolbyVisionFallbackTarget,
        'baseLayerEvidence':
            flutterProbe.hdrMetadata?.dolbyVisionBaseLayerEvidence,
        'baseLayerTransferFunction':
            flutterProbe.hdrMetadata?.dolbyVisionBaseLayerTransferFunction,
        'containerEvidence': null,
      },
      'controlPurpose': 'none',
      'metadataTool': <String, Object?>{
        'name': 'TBD',
        'version': 'TBD',
        'command': 'TBD',
      },
      'notes': const <String>[],
    }, sourceMetadata);
  }

  Map<String, Object?> probeHostJson() {
    final probe = hostProbe ?? flutterProbe;
    return <String, Object?>{
      'schema': _probeHostSchema,
      'sampleId': sampleId,
      'deviceId': deviceId,
      'platform': platform,
      'captureDate': captureDate,
      'request': <String, Object?>{
        'codec': sourceMetadata['codec'],
        'width': sourceMetadata['width'],
        'height': sourceMetadata['height'],
        'frameRate': sourceMetadata['frameRate'],
        'sourceUri': sourceMetadata['sourceUri'],
      },
      'result': _probeResult(probe),
      'diagnostics': _groupProbeDiagnostics(probe.diagnostics),
      'raw': <String, Object?>{
        'capturedVia': hostProbe == null
            ? 'flutterHostProbePlaybackCapability'
            : 'hostProbe',
        'result': probe.toMap(),
      },
    };
  }

  Map<String, Object?> probeFlutterJson() {
    return <String, Object?>{
      'schema': _probeFlutterSchema,
      'sampleId': sampleId,
      'deviceId': deviceId,
      'captureDate': captureDate,
      'result': _probeResult(flutterProbe),
      'diagnostics': flutterProbe.diagnostics,
      'matchesHostProbe': matchesHostProbe,
      'mismatches': probeMismatches,
    };
  }

  Map<String, Object?> runtimeWarningJson() {
    return <String, Object?>{
      'schema': _runtimeWarningSchema,
      'sampleId': sampleId,
      'deviceId': deviceId,
      'captureDate': captureDate,
      'observed': runtimeWarning != null,
      'warning': _warningResult(runtimeWarning),
      'expected': <String, Object?>{
        'shouldObserveWarning': null,
        'expectedReason': null,
        'expectedCapabilityFailureCause': null,
        'expectedCapabilityFailureAxis': null,
      },
      'notes': const <String>[],
    };
  }

  Map<String, Object?> runtimeErrorJson() {
    final error = runtimeError;
    final details = error?.details ?? const <String, Object?>{};
    return <String, Object?>{
      'schema': _runtimeErrorSchema,
      'sampleId': sampleId,
      'deviceId': deviceId,
      'captureDate': captureDate,
      'playbackOutcome': playbackOutcome,
      'error': <String, Object?>{
        'observed': error != null,
        'code': error?.code.name,
        'category': error?.category.name,
        'message': error?.message,
        'retriable': error?.retriable,
        'details': details,
      },
      'android': _androidRuntime(details),
      'ios': _iosRuntime(details),
      'expectedAxis': expectedAxis,
      'axisSupportedByEvidence': axisSupportedByEvidence,
      'missingEvidence': missingEvidence,
    };
  }

  Map<String, Object?> typedEvidenceJson() {
    final hdrEvidence = runtimeError?.hdrCapabilityEvidence;
    return <String, Object?>{
      'schema': _typedEvidenceSchema,
      'sampleId': sampleId,
      'deviceId': deviceId,
      'captureDate': captureDate,
      'flutter': <String, Object?>{
        'vesperHdrCapabilityEvidence': <String, Object?>{
          'present': hdrEvidence != null,
          'likelyHdrCapabilityIssue':
              hdrEvidence?.likelyHdrCapabilityIssue ?? false,
          'hdrKind': hdrEvidence?.hdrKind.name ?? flutterProbe.hdrKind.name,
          'recommendedPlaybackPath':
              hdrEvidence?.recommendedPlaybackPath.name ??
              flutterProbe.recommendedPlaybackPath.name,
          'confidence': hdrEvidence?.confidence,
          'errorCode': hdrEvidence?.errorCode,
          'capabilityFailureCause': hdrEvidence?.capabilityFailureCause,
          'capabilityFailureAxis': hdrEvidence?.capabilityFailureAxis,
          'hdrMetadata':
              hdrEvidence?.hdrMetadata?.toMap() ??
              flutterProbe.hdrMetadata?.toMap() ??
              const <String, Object?>{},
          'diagnostics': hdrEvidence?.diagnostics ?? const <String, Object?>{},
        },
        'vesperCapabilityWarning': <String, Object?>{
          'present': runtimeWarning != null,
          ..._warningResult(runtimeWarning),
        },
      },
      'matchesHostEvidence': matchesHostEvidence,
      'mismatches': evidenceMismatches,
    };
  }

  String platformLogText() {
    return <String>[
      'schema: vesper-hdr-dv-platform-log-v1',
      'sampleId: $sampleId',
      'deviceId: $deviceId',
      'captureDate: $captureDate',
      '',
      platformLog.isEmpty
          ? 'No bounded platform log was attached.'
          : platformLog,
      '',
    ].join('\n');
  }

  String notesMarkdown({String? bundlePath}) {
    final rowId = platform == 'ios'
        ? _iosRows[sampleId] ?? 'TBD'
        : _androidRows[sampleId] ?? 'TBD';
    return notes ??
        '''
# HDR / Dolby Vision Evidence Notes

- Bundle path: `${bundlePath ?? 'TBD'}`
- Matrix row: `$rowId`
- Sample ID: `$sampleId`
- Device ID: `$deviceId`
- Capture date: `$captureDate`
- SDK commit: `$sdkCommit`
- Host app: `flutter-host`

## Expected Axis

`$expectedAxis`

## Outcome

Playback outcome:

- `$playbackOutcome`

Observed route:

- `${flutterProbe.recommendedPlaybackPath.name}`

Observed confidence:

- `${flutterProbe.confidence.name}`

## Evidence Summary

Source metadata:

- Captured in `source-metadata.json`.

Device capability:

- Captured in `device.json`.

Probe result:

- Flutter probe captured in `probe-flutter.json`; host probe captured in `probe-host.json`.

Runtime result:

- Warning observed: `${runtimeWarning != null}`
- Error observed: `${runtimeError != null}`

Typed Flutter evidence:

- Captured in `typed-evidence.json`.

## Axis Decision

Status:

- `Open`

Decision:

- Review validator output and update the real-device matrix manually.

Missing or contradictory evidence:

- ${missingEvidence.isEmpty ? 'TBD' : missingEvidence.join('; ')}
''';
  }
}

final class ExampleHdrEvidenceCaptureRecorder {
  ExampleHdrEvidenceCaptureRecorder({
    required this.controller,
    required this.sampleId,
    required this.deviceId,
    required this.platform,
    required this.captureDate,
    required this.sdkCommit,
    required this.source,
    this.sourceMetadata = const <String, Object?>{},
    this.device = const <String, Object?>{},
    this.expectedAxis = 'inconclusive',
    this.captureWindow = const Duration(seconds: 10),
    this.platformLog = '',
  });

  final VesperPlayerController controller;
  final String sampleId;
  final String deviceId;
  final String platform;
  final String captureDate;
  final String sdkCommit;
  final VesperPlayerSource source;
  final Map<String, Object?> sourceMetadata;
  final Map<String, Object?> device;
  final String expectedAxis;
  final Duration captureWindow;
  final String platformLog;

  Future<ExampleHdrEvidenceBundle> capture() async {
    VesperCapabilityWarning? capabilityWarning;
    VesperPlayerError? playbackError;
    final subscription = controller.events.listen((event) {
      switch (event) {
        case VesperPlayerWarningEvent():
          final warning = event.warning.capability;
          if (warning != null) {
            capabilityWarning = warning;
          }
        case VesperPlayerErrorEvent():
          playbackError = event.error;
        case VesperPlayerSnapshotEvent():
        case VesperPlayerDisposedEvent():
          break;
      }
    });

    late final VesperPlaybackCapabilityProbeResult flutterProbe;
    try {
      flutterProbe = await VesperPlayerController.probePlaybackCapability(
        VesperPlaybackCapabilityProbeRequest(
          source: source,
          codec: sourceMetadata['codec'] as String?,
          width: _intValue(sourceMetadata['width']),
          height: _intValue(sourceMetadata['height']),
          frameRate: _doubleValue(sourceMetadata['frameRate']),
        ),
      );
      await controller.selectSource(source);
      await controller.play();
      await Future<void>.delayed(captureWindow);
    } catch (error) {
      if (error is VesperPlayerError) {
        playbackError = error;
      } else {
        rethrow;
      }
    } finally {
      await subscription.cancel();
    }

    final effectiveSourceMetadata = <String, Object?>{
      'sourceUri': source.uri,
      'sourceKind': _sourceKindFor(source),
      'manifestKind': _manifestKindFor(source),
      ...sourceMetadata,
    };

    return ExampleHdrEvidenceBundle(
      sampleId: sampleId,
      deviceId: deviceId,
      platform: platform,
      captureDate: captureDate,
      sdkCommit: sdkCommit,
      sourceMetadata: effectiveSourceMetadata,
      device: device,
      flutterProbe: flutterProbe,
      playbackOutcome: _playbackOutcome(playbackError, capabilityWarning),
      runtimeWarning: capabilityWarning,
      runtimeError: playbackError,
      expectedAxis: expectedAxis,
      matchesHostProbe: null,
      matchesHostEvidence: null,
      platformLog: platformLog,
    );
  }
}

final class ExampleHdrEvidenceBundleWriter {
  const ExampleHdrEvidenceBundleWriter({required this.outputRoot});

  final Directory outputRoot;

  Future<Directory> write(
    ExampleHdrEvidenceBundle bundle, {
    bool overwrite = false,
  }) async {
    final directory = Directory(
      '${outputRoot.path}/${bundle.captureDate}/${bundle.deviceId}/${bundle.sampleId}',
    );
    if (directory.existsSync()) {
      if (!overwrite) {
        throw StateError('Evidence bundle already exists: ${directory.path}');
      }
    } else {
      await directory.create(recursive: true);
    }

    await _writeJson(
      File('${directory.path}/device.json'),
      bundle.deviceJson(),
    );
    await _writeJson(
      File('${directory.path}/source-metadata.json'),
      bundle.sourceMetadataJson(),
    );
    await _writeJson(
      File('${directory.path}/probe-host.json'),
      bundle.probeHostJson(),
    );
    await _writeJson(
      File('${directory.path}/probe-flutter.json'),
      bundle.probeFlutterJson(),
    );
    await _writeJson(
      File('${directory.path}/runtime-warning.json'),
      bundle.runtimeWarningJson(),
    );
    await _writeJson(
      File('${directory.path}/runtime-error.json'),
      bundle.runtimeErrorJson(),
    );
    await _writeJson(
      File('${directory.path}/typed-evidence.json'),
      bundle.typedEvidenceJson(),
    );
    await File(
      '${directory.path}/platform-log.txt',
    ).writeAsString(bundle.platformLogText());
    await File(
      '${directory.path}/notes.md',
    ).writeAsString(bundle.notesMarkdown(bundlePath: directory.path));
    return directory;
  }

  Future<void> _writeJson(File file, Map<String, Object?> data) {
    const encoder = JsonEncoder.withIndent('  ');
    return file.writeAsString('${encoder.convert(_jsonValue(data))}\n');
  }
}

Map<String, Object?> _probeResult(VesperPlaybackCapabilityProbeResult probe) {
  return <String, Object?>{
    'status': probe.status.name,
    'recommendedPlaybackPath': probe.recommendedPlaybackPath.name,
    'confidence': probe.confidence.name,
    'hdrKind': probe.hdrKind.name,
    'missingCapabilities': probe.missingCapabilities,
    'hdrMetadata': probe.hdrMetadata?.toMap() ?? const <String, Object?>{},
  };
}

Map<String, Object?> _warningResult(VesperCapabilityWarning? warning) {
  if (warning == null) {
    return const <String, Object?>{
      'reason': null,
      'recommendedPlaybackPath': null,
      'hdrKind': null,
      'likelyHdrCapabilityIssue': false,
      'confidence': null,
      'errorCode': null,
      'capabilityFailureCause': null,
      'capabilityFailureAxis': null,
      'hdrMetadata': <String, Object?>{},
      'diagnostics': <String, Object?>{},
      'message': null,
    };
  }
  return <String, Object?>{
    'reason': warning.reason.name,
    'recommendedPlaybackPath': warning.recommendedPlaybackPath.name,
    'hdrKind': warning.hdrKind.name,
    'likelyHdrCapabilityIssue': warning.likelyHdrCapabilityIssue,
    'confidence': warning.confidence,
    'errorCode': warning.errorCode,
    'capabilityFailureCause': warning.capabilityFailureCause,
    'capabilityFailureAxis': warning.capabilityFailureAxis,
    'hdrMetadata': warning.hdrMetadata?.toMap() ?? const <String, Object?>{},
    'diagnostics': warning.diagnostics,
    'message': warning.message,
  };
}

Map<String, Object?> _groupProbeDiagnostics(Map<String, Object?> diagnostics) {
  return <String, Object?>{
    'display': _matchingDiagnostics(diagnostics, <String>[
      'display',
      'avPlayer',
      'requestedFrameRate',
    ]),
    'codecFormat': _matchingDiagnostics(diagnostics, <String>['codecFormat']),
    'asset': _matchingDiagnostics(diagnostics, <String>['asset']),
    'dolbyVision': _matchingDiagnostics(diagnostics, <String>['dolbyVision']),
    'other': _otherDiagnostics(diagnostics, <String>[
      'display',
      'avPlayer',
      'requestedFrameRate',
      'codecFormat',
      'asset',
      'dolbyVision',
    ]),
  };
}

Map<String, Object?> _matchingDiagnostics(
  Map<String, Object?> diagnostics,
  List<String> needles,
) {
  return Map<String, Object?>.fromEntries(
    diagnostics.entries.where(
      (entry) => needles.any((needle) => entry.key.startsWith(needle)),
    ),
  );
}

Map<String, Object?> _otherDiagnostics(
  Map<String, Object?> diagnostics,
  List<String> groupedNeedles,
) {
  return Map<String, Object?>.fromEntries(
    diagnostics.entries.where(
      (entry) => !groupedNeedles.any((needle) => entry.key.startsWith(needle)),
    ),
  );
}

Map<String, Object?> _androidRuntime(Map<String, Object?> details) {
  return <String, Object?>{
    'playbackExceptionErrorCode': details['errorCode'],
    'capabilityFailureCause': details['capabilityFailureCause'],
    'capabilityFailureAxis': details['capabilityFailureAxis'],
    'playbackFailureCauseClass': details['playbackFailureCauseClass'],
    'playbackFailureCauseMessage': details['playbackFailureCauseMessage'],
    'playbackFailureRootCauseClass': details['playbackFailureRootCauseClass'],
    'playbackFailureRootCauseMessage':
        details['playbackFailureRootCauseMessage'],
    'rendererName': details['playbackFailureRendererName'],
    'rendererIndex': details['playbackFailureRendererIndex'],
    'rendererFormatSupport': details['playbackFailureRendererFormatSupport'],
    'rendererFormatSampleMimeType':
        details['playbackFailureRendererFormatSampleMimeType'],
    'rendererFormatCodecs': details['playbackFailureRendererFormatCodecs'],
    'rendererFormatWidth': details['playbackFailureRendererFormatWidth'],
    'rendererFormatHeight': details['playbackFailureRendererFormatHeight'],
    'rendererFormatFrameRate':
        details['playbackFailureRendererFormatFrameRate'],
    'rendererFormatSupported':
        details['playbackFailureRendererFormatSupported'],
    'rendererFormatMimeMatchesRuntime':
        details['playbackFailureRendererFormatMimeMatchesRuntime'],
    'rendererFormatCodecsMatchRuntime':
        details['playbackFailureRendererFormatCodecsMatchRuntime'],
    'rendererFormatSizeMatchesRuntime':
        details['playbackFailureRendererFormatSizeMatchesRuntime'],
    'rendererFormatFrameRateMatchesRuntime':
        details['playbackFailureRendererFormatFrameRateMatchesRuntime'],
    'runtimeSessionProbeStatus': details['runtimeSessionProbeStatus'],
    'runtimeSessionProbeRecommendedPlaybackPath':
        details['runtimeSessionProbeRecommendedPlaybackPath'],
    'runtimeSessionProbeConfidence': details['runtimeSessionProbeConfidence'],
    'runtimeSessionProbeHdrKind': details['runtimeSessionProbeHdrKind'],
    'runtimeSessionProbeDolbyVisionMode':
        details['runtimeSessionProbeDolbyVisionMode'],
    'runtimeSessionProbeMissingCapabilities':
        details['runtimeSessionProbeMissingCapabilities'],
    'runtimeSessionProbeCodecFormatSupported':
        details['runtimeSessionProbeCodecFormatSupported'],
    'runtimeSessionProbeCodecFormatMissingCapability':
        details['runtimeSessionProbeCodecFormatMissingCapability'],
    'runtimeSessionProbeCodecFormatSampleMimeType':
        details['runtimeSessionProbeCodecFormatSampleMimeType'],
    'runtimeSessionProbeCodecFormatCodecs':
        details['runtimeSessionProbeCodecFormatCodecs'],
    'runtimeSessionProbeCodecFormatWidth':
        details['runtimeSessionProbeCodecFormatWidth'],
    'runtimeSessionProbeCodecFormatHeight':
        details['runtimeSessionProbeCodecFormatHeight'],
    'runtimeSessionProbeCodecFormatFrameRate':
        details['runtimeSessionProbeCodecFormatFrameRate'],
    'runtimeSessionProbeDisplayHdrSupported':
        details['runtimeSessionProbeDisplayHdrSupported'],
    'runtimeSessionProbeDisplayFrameRateSupported':
        details['runtimeSessionProbeDisplayFrameRateSupported'],
    'runtimeSessionProbeCodecFormatMimeMatchesRuntime':
        details['runtimeSessionProbeCodecFormatMimeMatchesRuntime'],
    'runtimeSessionProbeCodecFormatCodecsMatchRuntime':
        details['runtimeSessionProbeCodecFormatCodecsMatchRuntime'],
    'runtimeSessionProbeCodecFormatSizeMatchesRuntime':
        details['runtimeSessionProbeCodecFormatSizeMatchesRuntime'],
    'runtimeSessionProbeCodecFormatFrameRateMatchesRuntime':
        details['runtimeSessionProbeCodecFormatFrameRateMatchesRuntime'],
    'rawPayloadKeys': <String, Object?>{
      'playbackFailureRendererFormatSupported':
          details['playbackFailureRendererFormatSupported'],
      'playbackFailureRendererFormatMimeMatchesRuntime':
          details['playbackFailureRendererFormatMimeMatchesRuntime'],
      'playbackFailureRendererFormatCodecsMatchRuntime':
          details['playbackFailureRendererFormatCodecsMatchRuntime'],
      'playbackFailureRendererFormatSizeMatchesRuntime':
          details['playbackFailureRendererFormatSizeMatchesRuntime'],
      'playbackFailureRendererFormatFrameRateMatchesRuntime':
          details['playbackFailureRendererFormatFrameRateMatchesRuntime'],
    },
  };
}

Map<String, Object?> _iosRuntime(Map<String, Object?> details) {
  return <String, Object?>{
    'avErrorCode': details['avErrorCode'],
    'nsErrorDomain': details['nsErrorDomain'],
    'nsErrorCode': details['nsErrorCode'],
    'iosRuntimeEvidenceSource': details['iosRuntimeEvidenceSource'],
    'iosRuntimeFailureCategory': details['iosRuntimeFailureCategory'],
    'iosRuntimeFailureRetriable': details['iosRuntimeFailureRetriable'],
    'iosRuntimeFailureCode': details['iosRuntimeFailureCode'],
    'capabilityFailureCause': details['capabilityFailureCause'],
    'missingCapabilities': details['missingCapabilities'],
    'sessionProbe': details['sessionProbe'],
    'displayHdrProbeAvailable': details['displayHdrProbeAvailable'],
    'displayHdrSupported': details['displayHdrSupported'],
    'displayGamut': details['displayGamut'],
    'avPlayerEligibleForHDRPlayback': details['avPlayerEligibleForHDRPlayback'],
    'hdrKindSupportBasis': details['hdrKindSupportBasis'],
    'displayFrameRateSupported': details['displayFrameRateSupported'],
    'displayMaximumFramesPerSecond': details['displayMaximumFramesPerSecond'],
    'displayNativeWidth': details['displayNativeWidth'],
    'displayNativeHeight': details['displayNativeHeight'],
    'requestedWidth': details['requestedWidth'],
    'requestedHeight': details['requestedHeight'],
    'requestedFrameRate': details['requestedFrameRate'],
    'avPlayerItemStatusEvidenceSource':
        details['avPlayerItemStatusEvidenceSource'],
    'avPlayerItemStatus': details['avPlayerItemStatus'],
    'avPlayerItemErrorLogEvidenceSource':
        details['avPlayerItemErrorLogEvidenceSource'],
    'avPlayerItemErrorLogEventCount': details['avPlayerItemErrorLogEventCount'],
    'avPlayerItemErrorLogRecentEventCount':
        details['avPlayerItemErrorLogRecentEventCount'],
    'avPlayerItemErrorLogEvents': details['avPlayerItemErrorLogEvents'],
    'avPlayerItemErrorStatusCode': details['avPlayerItemErrorStatusCode'],
    'avPlayerItemErrorDomain': details['avPlayerItemErrorDomain'],
    'avPlayerItemErrorComment': details['avPlayerItemErrorComment'],
  };
}

Object? _jsonValue(Object? value) {
  if (value == null || value is num || value is bool || value is String) {
    return value;
  }
  if (value is Iterable) {
    return value.map(_jsonValue).toList(growable: false);
  }
  if (value is Map) {
    return <String, Object?>{
      for (final entry in value.entries)
        entry.key.toString(): _jsonValue(entry.value),
    };
  }
  return value.toString();
}

Map<String, Object?> _mergeMaps(
  Map<String, Object?> defaults,
  Map<String, Object?> overrides,
) {
  final result = Map<String, Object?>.of(defaults);
  for (final entry in overrides.entries) {
    final base = result[entry.key];
    final override = entry.value;
    if (base is Map<String, Object?> && override is Map) {
      result[entry.key] = _mergeMaps(base, <String, Object?>{
        for (final overrideEntry in override.entries)
          overrideEntry.key.toString(): overrideEntry.value,
      });
    } else {
      result[entry.key] = override;
    }
  }
  return result;
}

int? _intValue(Object? value) {
  if (value is int) {
    return value;
  }
  if (value is num) {
    return value.toInt();
  }
  if (value is String) {
    return int.tryParse(value);
  }
  return null;
}

double? _doubleValue(Object? value) {
  if (value is double) {
    return value;
  }
  if (value is num) {
    return value.toDouble();
  }
  if (value is String) {
    return double.tryParse(value);
  }
  return null;
}

String _playbackOutcome(
  VesperPlayerError? error,
  VesperCapabilityWarning? warning,
) {
  if (error != null) {
    return 'failure';
  }
  if (warning?.recommendedPlaybackPath ==
      VesperRecommendedPlaybackPath.systemPlayer) {
    return 'fallback';
  }
  return 'success';
}

String _sourceKindFor(VesperPlayerSource source) {
  return switch (source.protocol) {
    VesperPlayerSourceProtocol.file ||
    VesperPlayerSourceProtocol.content => 'file',
    VesperPlayerSourceProtocol.hls => 'hls',
    VesperPlayerSourceProtocol.dash => 'progressive',
    VesperPlayerSourceProtocol.progressive => 'progressive',
    VesperPlayerSourceProtocol.unknown =>
      source.kind == VesperPlayerSourceKind.local ? 'file' : 'progressive',
  };
}

String _manifestKindFor(VesperPlayerSource source) {
  return switch (source.protocol) {
    VesperPlayerSourceProtocol.hls => 'hls',
    VesperPlayerSourceProtocol.dash => 'dash',
    _ => 'none',
  };
}
