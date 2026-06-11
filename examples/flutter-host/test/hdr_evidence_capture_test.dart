import 'dart:convert';
import 'dart:io';

import 'package:flutter_test/flutter_test.dart';
import 'package:flutter_host/src/hdr_evidence/hdr_evidence_capture.dart';
import 'package:vesper_player/vesper_player.dart';

void main() {
  test('P0 presets expose HDR10, HEVC SDR, and network controls', () {
    expect(
      exampleHdrEvidenceP0Presets.map((preset) => preset.sampleId),
      containsAll(<String>[
        'HDR10-HEVC-MAIN10-2160P60-PQ',
        'HEVC-SDR-CONTROL',
        'NETWORK-FAILURE-CONTROL',
      ]),
    );
    final hdr10 = exampleHdrEvidenceP0Presets.firstWhere(
      (preset) => preset.sampleId == 'HDR10-HEVC-MAIN10-2160P60-PQ',
    );
    expect(hdr10.expectedAxis, 'display');
    expect(hdr10.sourceMetadata['hdrKind'], 'hdr10');
    expect(hdr10.sourceMetadata['width'], 3840);

    final hevcSdr = exampleHdrEvidenceP0Presets.firstWhere(
      (preset) => preset.sampleId == 'HEVC-SDR-CONTROL',
    );
    expect(hevcSdr.expectedAxis, 'none');
    expect(hevcSdr.sourceMetadata['hdrKind'], 'none');
    expect(hevcSdr.sourceMetadata['controlPurpose'], 'hevcSdrFalsePositive');

    final network = exampleHdrEvidenceP0Presets.firstWhere(
      (preset) => preset.sampleId == 'NETWORK-FAILURE-CONTROL',
    );
    expect(network.expectedAxis, 'network');
    expect(network.sourceMetadata['controlPurpose'], 'networkFailure');
    expect(
      network.sourceMetadata['sourceUri'],
      exampleHdrEvidenceNetworkControlUrl,
    );
    expect(network.sourceMetadata['hdrKind'], 'none');
    expect(network.sourceMetadata['codec'], 'none');
  });

  test(
    'writes HDR evidence bundle files from observed Flutter evidence',
    () async {
      final requestedOutputRoot =
          Platform.environment['VESPER_HDR_EVIDENCE_TEST_OUTPUT'];
      final outputRoot = requestedOutputRoot == null
          ? await Directory.systemTemp.createTemp('vesper-hdr-evidence-test-')
          : Directory(requestedOutputRoot);
      if (requestedOutputRoot == null) {
        addTearDown(() async {
          if (outputRoot.existsSync()) {
            await outputRoot.delete(recursive: true);
          }
        });
      } else {
        await outputRoot.create(recursive: true);
      }

      const probe = VesperPlaybackCapabilityProbeResult(
        status: VesperPlaybackCapabilityProbeStatus.fallbackRequired,
        codecFamily: VesperPlaybackCodecFamily.hevc,
        systemPlaybackSupported: true,
        hardwareDecodeSupported: true,
        sdkManagedNativeFrameSupported: false,
        recommendedPlaybackPath: VesperRecommendedPlaybackPath.systemPlayer,
        outputFormat: VesperPlaybackCapabilityOutputFormat.surfaceOpaque,
        hdrKind: VesperPlaybackCapabilityHdrKind.dolbyVision,
        dolbyVisionMode:
            VesperPlaybackCapabilityDolbyVisionMode.compatibleBaseLayer,
        confidence: VesperPlaybackCapabilityConfidence.sessionProbe,
        hdrMetadata: VesperHdrMetadata(
          hdrKind: VesperPlaybackCapabilityHdrKind.dolbyVision,
          dolbyVisionMode:
              VesperPlaybackCapabilityDolbyVisionMode.compatibleBaseLayer,
          codec: 'dvhe.08.07',
          sampleMimeType: 'video/dolby-vision',
          transferFunction: 'SMPTE_ST_2084_PQ',
          dolbyVisionCodec: 'dvhe',
          dolbyVisionProfile: 8,
          dolbyVisionLevel: 7,
          dolbyVisionCompatibility: 'profile8Hdr10BaseLayer',
          dolbyVisionProfileFamily: 'profile8SingleLayerCompatible',
          dolbyVisionBaseLayer: 'hdr10BaseLayer',
          dolbyVisionFallbackTarget: 'hdr10BaseLayerSystemPlayer',
          dolbyVisionBaseLayerEvidence: 'assetVideoTransferFunction',
          dolbyVisionBaseLayerTransferFunction: 'SMPTE_ST_2084_PQ',
        ),
        missingCapabilities: <String>['hdrProgrammableProcessingNotSupported'],
        diagnostics: <String, Object?>{
          'displayHdrSupported': true,
          'codecFormatSupported': true,
          'assetVideoCodec': 'dvhe.08.07',
          'dolbyVisionProfile': 8,
        },
      );
      const warning = VesperCapabilityWarning(
        reason: VesperCapabilityWarningReason.hdrNativeFrameUnsupported,
        recommendedPlaybackPath: VesperRecommendedPlaybackPath.systemPlayer,
        hdrKind: VesperPlaybackCapabilityHdrKind.dolbyVision,
        likelyHdrCapabilityIssue: true,
        confidence: 'sessionProbe',
        errorCode: 'ERROR_CODE_DECODER_INIT_FAILED',
        capabilityFailureCause: 'decoderInit',
        capabilityFailureAxis: 'decoder',
        hdrMetadata: VesperHdrMetadata(
          hdrKind: VesperPlaybackCapabilityHdrKind.dolbyVision,
          dolbyVisionProfile: 8,
        ),
        diagnostics: <String, Object?>{
          'runtimeSessionProbeStatus': 'fallbackRequired',
        },
        message: 'HDR/Dolby Vision uses system playback.',
      );
      final error = VesperPlayerError.fromMap(<Object?, Object?>{
        'message': 'decoder unavailable',
        'code': 'decodeFailure',
        'category': 'decode',
        'retriable': false,
        'details': <Object?, Object?>{
          'likelyHdrCapabilityIssue': true,
          'hdrKind': 'dolbyVision',
          'recommendedPlaybackPath': 'systemPlayer',
          'confidence': 'sessionProbe',
          'errorCode': 'ERROR_CODE_DECODER_INIT_FAILED',
          'capabilityFailureCause': 'decoderInit',
          'capabilityFailureAxis': 'decoder',
          'runtimeSessionProbeStatus': 'fallbackRequired',
          'runtimeSessionProbeRecommendedPlaybackPath': 'systemPlayer',
          'runtimeSessionProbeConfidence': 'sessionProbe',
          'runtimeSessionProbeHdrKind': 'dolbyVision',
          'runtimeSessionProbeCodecFormatSupported': true,
          'runtimeSessionProbeDisplayHdrSupported': true,
          'playbackFailureCauseClass': 'MediaCodecRenderer',
          'playbackFailureRendererName': 'MediaCodecVideoRenderer',
          'hdrMetadata': <Object?, Object?>{
            'hdrKind': 'dolbyVision',
            'dolbyVisionProfile': 8,
          },
        },
      });

      final bundle = ExampleHdrEvidenceBundle(
        sampleId: 'DV-P8-COMPATIBLE-BL',
        deviceId: 'android-pixel-fixture',
        platform: 'android',
        captureDate: '2026-06-07',
        sdkCommit: 'fixture',
        sourceMetadata: const <String, Object?>{
          'sourceKind': 'file',
          'sourceUri': 'file:///tmp/dv-p8.mp4',
          'container': 'mp4',
          'codec': 'dvhe.08.07',
          'sampleMimeType': 'video/dolby-vision',
          'width': 3840,
          'height': 2160,
          'frameRate': 60.0,
          'bitDepth': 10,
          'hdrKind': 'dolbyVision',
          'transferFunction': 'SMPTE_ST_2084_PQ',
        },
        device: const <String, Object?>{
          'android': <String, Object?>{
            'manufacturer': 'Google',
            'model': 'Pixel fixture',
            'apiLevel': 35,
            'buildFingerprint': 'fixture/fingerprint',
            'displayHdrTypes': <String>['HDR10'],
            'displayRefreshRate': 120.0,
            'displayModes': <String>['3840x2160@120'],
            'media3Version': 'fixture',
            'decoderCandidates': <String, Object?>{
              'hevc': <String>['c2.fixture.hevc.decoder'],
              'dolbyVision': <String>['c2.fixture.dv.decoder'],
            },
          },
        },
        flutterProbe: probe,
        playbackOutcome: 'failure',
        runtimeWarning: warning,
        runtimeError: error,
        expectedAxis: 'decoder',
        axisSupportedByEvidence: true,
        matchesHostProbe: true,
        matchesHostEvidence: true,
        platformLog: 'Media3 decoder fixture log',
      );

      final directory = await ExampleHdrEvidenceBundleWriter(
        outputRoot: outputRoot,
      ).write(bundle);

      expect(File('${directory.path}/device.json').existsSync(), isTrue);
      expect(
        File('${directory.path}/source-metadata.json').existsSync(),
        isTrue,
      );
      expect(File('${directory.path}/probe-host.json').existsSync(), isTrue);
      expect(File('${directory.path}/probe-flutter.json').existsSync(), isTrue);
      expect(
        File('${directory.path}/runtime-warning.json').existsSync(),
        isTrue,
      );
      expect(File('${directory.path}/runtime-error.json').existsSync(), isTrue);
      expect(
        File('${directory.path}/typed-evidence.json').existsSync(),
        isTrue,
      );
      expect(File('${directory.path}/platform-log.txt').existsSync(), isTrue);
      expect(File('${directory.path}/notes.md').existsSync(), isTrue);

      final probeFlutter = _readJson(directory, 'probe-flutter.json');
      expect(probeFlutter['schema'], 'vesper-hdr-dv-probe-flutter-v1');
      expect(
        probeFlutter['result'],
        containsPair('recommendedPlaybackPath', 'systemPlayer'),
      );
      expect(
        probeFlutter['result'],
        containsPair('confidence', 'sessionProbe'),
      );
      expect(probeFlutter['result'], containsPair('hdrKind', 'dolbyVision'));

      final runtimeError = _readJson(directory, 'runtime-error.json');
      expect(runtimeError['playbackOutcome'], 'failure');
      expect(runtimeError['expectedAxis'], 'decoder');
      expect(runtimeError['axisSupportedByEvidence'], isTrue);
      expect(
        runtimeError['android'],
        containsPair('runtimeSessionProbeStatus', 'fallbackRequired'),
      );
      expect(
        runtimeError['android'],
        containsPair('capabilityFailureAxis', 'decoder'),
      );

      final typedEvidence = _readJson(directory, 'typed-evidence.json');
      final flutter = typedEvidence['flutter'] as Map<String, Object?>;
      final hdrEvidence =
          flutter['vesperHdrCapabilityEvidence'] as Map<String, Object?>;
      expect(hdrEvidence['present'], isTrue);
      expect(hdrEvidence['recommendedPlaybackPath'], 'systemPlayer');
      expect(hdrEvidence['capabilityFailureCause'], 'decoderInit');
      final capabilityWarning =
          flutter['vesperCapabilityWarning'] as Map<String, Object?>;
      expect(capabilityWarning['present'], isTrue);
      expect(capabilityWarning['reason'], 'hdrNativeFrameUnsupported');
    },
  );

  test('deep merges native device evidence into device.json', () {
    const probe = VesperPlaybackCapabilityProbeResult(
      status: VesperPlaybackCapabilityProbeStatus.supported,
      codecFamily: VesperPlaybackCodecFamily.hevc,
      systemPlaybackSupported: true,
      hardwareDecodeSupported: true,
      sdkManagedNativeFrameSupported: false,
      recommendedPlaybackPath:
          VesperRecommendedPlaybackPath.nativeFramePipeline,
      outputFormat: VesperPlaybackCapabilityOutputFormat.nv12,
      hdrKind: VesperPlaybackCapabilityHdrKind.none,
      dolbyVisionMode: VesperPlaybackCapabilityDolbyVisionMode.none,
      confidence: VesperPlaybackCapabilityConfidence.codecOnly,
    );
    final bundle = ExampleHdrEvidenceBundle(
      sampleId: 'HEVC-SDR-CONTROL',
      deviceId: 'android-device-sheet',
      platform: 'android',
      captureDate: '2026-06-08',
      sdkCommit: 'fixture',
      sourceMetadata: const <String, Object?>{
        'sourceUri': 'file:///tmp/hevc-sdr.mp4',
        'hdrKind': 'none',
      },
      device: const <String, Object?>{
        'android': <String, Object?>{
          'manufacturer': 'Google',
          'model': 'Pixel fixture',
          'apiLevel': 35,
          'buildFingerprint': 'fixture/fingerprint',
          'displayHdrTypes': <String>['HDR10'],
          'displayRefreshRate': 120.0,
          'displayModes': <String>['1080x2400@120.00'],
          'media3Version': '1.9.3',
          'decoderCandidates': <String, Object?>{
            'hevc': <String>['c2.fixture.hevc.decoder'],
          },
        },
        'knownCaveats': <String>['fixture caveat'],
      },
      flutterProbe: probe,
    );

    final device = bundle.deviceJson();
    final android = device['android'] as Map<String, Object?>;
    final decoderCandidates =
        android['decoderCandidates'] as Map<String, Object?>;

    expect(android['manufacturer'], 'Google');
    expect(android['displayRefreshRate'], 120.0);
    expect(decoderCandidates['hevc'], <String>['c2.fixture.hevc.decoder']);
    expect(decoderCandidates['dolbyVision'], isEmpty);
    expect(device['knownCaveats'], <String>['fixture caveat']);
  });
}

Map<String, Object?> _readJson(Directory directory, String fileName) {
  final raw = File('${directory.path}/$fileName').readAsStringSync();
  return (jsonDecode(raw) as Map<Object?, Object?>).cast<String, Object?>();
}
