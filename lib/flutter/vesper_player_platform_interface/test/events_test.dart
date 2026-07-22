import 'dart:convert';
import 'dart:io';

import 'package:flutter_test/flutter_test.dart';
import 'package:vesper_player_platform_interface/vesper_player_platform_interface.dart';

void main() {
  test('shared plugin diagnostics contract decodes capability union', () {
    final decoded = jsonDecode(
        File('../../../fixtures/contracts/plugin_diagnostics.json')
            .readAsStringSync()) as List<dynamic>;
    final diagnostics = decoded
        .map((value) => VesperPluginDiagnostic.fromMap(
            Map<Object?, Object?>.from(value as Map)))
        .toList(growable: false);

    expect(diagnostics, hasLength(3));
    expect(
        diagnostics[0].status, VesperPluginDiagnosticStatus.decoderSupported);
    expect(
        diagnostics[0].participation, VesperPluginParticipation.participated);
    expect(diagnostics[0].capability?.kind, VesperPluginCapabilityKind.decoder);
    expect(diagnostics[0].capability?.decoder?.codecs.single.codec, 'h264');
    expect(diagnostics[0].capability?.decoder?.supportsGpuHandles, isTrue);
    expect(diagnostics[0].extra['details'], isA<Map>());
    expect(diagnostics[1].status,
        VesperPluginDiagnosticStatus.frameProcessorSupported);
    expect(diagnostics[1].capability?.kind,
        VesperPluginCapabilityKind.frameProcessor);
    expect(diagnostics[1].capability?.frameProcessor?.maxInFlightFrames, 4);
    expect(diagnostics[1].participation, VesperPluginParticipation.available);
    expect(diagnostics[2].status,
        VesperPluginDiagnosticStatus.sourceNormalizerSupported);
    expect(diagnostics[2].participation, VesperPluginParticipation.bypassed);
    expect(diagnostics[2].capability?.kind,
        VesperPluginCapabilityKind.sourceNormalizer);
    expect(
      diagnostics[2]
          .capability
          ?.sourceNormalizer
          ?.supportedRuntimeProfiles
          .single,
      'generic-fallback',
    );
    expect(
      diagnostics[2].capability?.sourceNormalizer?.supportedOutputRoutes.single,
      'packetStream',
    );
    expect(
        diagnostics[2].capability?.sourceNormalizer?.requiresNetwork, isFalse);
    final sourceNormalizerDetails =
        Map<Object?, Object?>.from(diagnostics[2].extra['details'] as Map);
    expect(sourceNormalizerDetails['selectedVideoStreamIndex'], '0');
    expect(sourceNormalizerDetails['audioStreamIndex'], '1');
    expect(sourceNormalizerDetails['route'], 'sdkManagedNativeFrame');
  });

  test('native frame pipeline diagnostic keeps unknown plugin kind extras', () {
    final diagnostic = VesperPluginDiagnostic.fromMap(<Object?, Object?>{
      'path': '/tmp/libdecoder.dylib:/tmp/libframe.dylib',
      'pluginName': 'vesper-mobile-native-frame-pipeline',
      'pluginKind': 'native_frame_pipeline',
      'status': 'loaded',
      'message': 'explicit native frame pipeline requested',
      'participation': 'selected',
      'pipelineProfile': 'VideoToolboxCvPixelBuffer',
    });

    expect(diagnostic.pluginKind, 'native_frame_pipeline');
    expect(diagnostic.status, VesperPluginDiagnosticStatus.loaded);
    expect(diagnostic.participation, VesperPluginParticipation.selected);
    expect(diagnostic.extra['pipelineProfile'], 'VideoToolboxCvPixelBuffer');
  });

  test('plugin diagnostic decodes fallback participation', () {
    final diagnostic = VesperPluginDiagnostic.fromMap(<Object?, Object?>{
      'path': '/tmp/player-decoder-videotoolbox.dylib',
      'pluginName': 'player-decoder-videotoolbox',
      'pluginKind': 'decoder',
      'status': 'decoderSupported',
      'participation': 'fallback',
      'message': 'native-frame runtime fell back to softwareDecoder route',
    });

    expect(diagnostic.participation, VesperPluginParticipation.fallback);
    expect(diagnostic.message, contains('softwareDecoder'));
  });

  test('plugin diagnostic preserves unknown wire values', () {
    final diagnostic = VesperPluginDiagnostic.fromMap(<Object?, Object?>{
      'path': '/tmp/player-future-plugin.dylib',
      'pluginName': 'player-future-plugin',
      'pluginKind': 'future',
      'status': 'futureStatus',
      'participation': 'futureParticipation',
      'capability': <Object?, Object?>{
        'kind': 'futureCapability',
        'futureCapability': <Object?, Object?>{
          'feature': 'packetHints',
        },
      },
    });

    expect(diagnostic.status, VesperPluginDiagnosticStatus.unsupportedKind);
    expect(diagnostic.statusRawValue, 'futureStatus');
    expect(diagnostic.participation, VesperPluginParticipation.unknown);
    expect(diagnostic.participationRawValue, 'futureParticipation');
    expect(diagnostic.capability?.kind, VesperPluginCapabilityKind.unknown);
    expect(diagnostic.capability?.rawKind, 'futureCapability');
    expect(diagnostic.capability?.decoder, isNull);
    expect(diagnostic.capability?.frameProcessor, isNull);
    expect(diagnostic.capability?.sourceNormalizer, isNull);

    final encoded = diagnostic.toMap();
    expect(encoded['status'], 'futureStatus');
    expect(encoded['participation'], 'futureParticipation');
    final capability = Map<Object?, Object?>.from(encoded['capability'] as Map);
    expect(capability['kind'], 'futureCapability');
    expect(
      Map<Object?, Object?>.from(
          capability['futureCapability'] as Map)['feature'],
      'packetHints',
    );
  });

  test('download task update event decodes prepared task', () {
    final event = VesperDownloadManagerEvent.fromMap(<Object?, Object?>{
      'downloadId': 'downloads',
      'type': 'taskUpdated',
      'task': <Object?, Object?>{
        'taskId': 11,
        'assetId': 'asset-hls',
        'source': VesperDownloadSource.fromSource(
          source: VesperPlayerSource.hls(
            uri: 'https://example.com/master.m3u8',
            label: 'HLS demo',
          ),
          manifestUri: 'https://example.com/master.m3u8',
        ).toMap(),
        'profile': const VesperDownloadProfile(
          targetOutputFormat: VesperDownloadOutputFormat.mp4,
        ).toMap(),
        'state': 'preparing',
        'progress': const VesperDownloadProgressSnapshot(
          totalBytes: 1024,
          totalSegments: 2,
        ).toMap(),
        'assetIndex': const VesperDownloadAssetIndex(
          contentFormat: VesperDownloadContentFormat.hlsSegments,
          totalSizeBytes: 1024,
          segments: <VesperDownloadSegmentRecord>[
            VesperDownloadSegmentRecord(
              segmentId: 'seg-1',
              uri: 'https://example.com/seg-1.ts',
              relativePath: 'seg-1.ts',
              sequence: 1,
              sizeBytes: 1024,
            ),
          ],
        ).toMap(),
      },
    });

    expect(event, isA<VesperDownloadTaskUpdatedEvent>());
    final updateEvent = event as VesperDownloadTaskUpdatedEvent;
    expect(updateEvent.downloadId, 'downloads');
    expect(updateEvent.task?.taskId, 11);
    expect(updateEvent.task?.assetIndex.totalSizeBytes, 1024);
    expect(
      updateEvent.task?.profile.targetOutputFormat,
      VesperDownloadOutputFormat.mp4,
    );
  });

  test('download manager event preserves unknown payloads', () {
    final event = VesperDownloadManagerEvent.fromMap(<Object?, Object?>{
      'downloadId': 'downloads',
      'snapshot': const VesperDownloadSnapshot.initial().toMap(),
    });

    expect(event, isA<VesperDownloadUnknownEvent>());
    final unknown = event as VesperDownloadUnknownEvent;
    expect(unknown.downloadId, 'downloads');
    expect(unknown.type, '<missing>');
    expect(unknown.payload['snapshot'], isA<Map>());
  });

  test('player snapshot event decodes embedded host lastError', () {
    final event = VesperPlayerEvent.fromMap(<Object?, Object?>{
      'playerId': 'ios-player',
      'type': 'snapshot',
      'snapshot': <Object?, Object?>{
        'title': 'Demo',
        'subtitle': 'Unsupported',
        'sourceLabel': 'feed://demo',
        'playbackState': 'ready',
        'playbackRate': 1.0,
        'isBuffering': false,
        'isInterrupted': false,
        'hasVideoSurface': false,
        'timeline': const VesperTimeline.initial().toMap(),
        'fixedTrackStatus': 'pending',
        'lastError': <Object?, Object?>{
          'message':
              'setAbrPolicy fixedTrack is not implemented on iOS AVPlayer',
          'code': 'unsupported',
          'category': 'capability',
          'retriable': false,
        },
      },
    });

    expect(event, isA<VesperPlayerSnapshotEvent>());
    final snapshotEvent = event as VesperPlayerSnapshotEvent;
    expect(snapshotEvent.playerId, 'ios-player');
    expect(
      snapshotEvent.snapshot.lastError?.code,
      VesperPlayerErrorCode.unsupported,
    );
    expect(snapshotEvent.snapshot.lastError?.category,
        VesperPlayerErrorCategory.capability);
    expect(
      snapshotEvent.snapshot.lastError?.message,
      'setAbrPolicy fixedTrack is not implemented on iOS AVPlayer',
    );
    expect(
      snapshotEvent.snapshot.fixedTrackStatus,
      VesperFixedTrackStatus.pending,
    );
  });

  test('player error event decodes nested subtitle error details', () {
    final subtitleDetails = <Object?, Object?>{
      'domain': 'subtitle',
      'code': 'subtitle_selection_timeout',
      'phase': 'selection',
      'trackId': 'external-en',
      'retriable': true,
      'commandId': 42,
      'sourceEpoch': 9,
      'message': 'confirmation timed out',
    };
    final eventError = <Object?, Object?>{
      'message': 'confirmation timed out',
      'code': 'backendFailure',
      'category': 'platform',
      'retriable': true,
      'details': subtitleDetails,
    };
    final event = VesperPlayerEvent.fromMap(<Object?, Object?>{
      'playerId': 'android-player',
      'type': 'error',
      'error': eventError,
      'snapshot': <Object?, Object?>{
        'title': 'Demo',
        'subtitle': 'Subtitle',
        'sourceLabel': 'https://example.com/video.mp4',
        'playbackState': 'ready',
        'playbackRate': 1.0,
        'isBuffering': false,
        'isInterrupted': false,
        'hasVideoSurface': false,
        'timeline': const VesperTimeline.initial().toMap(),
        'lastError': eventError,
      },
    });

    final errorEvent = event as VesperPlayerErrorEvent;
    expect(errorEvent.error.code, VesperPlayerErrorCode.backendFailure);
    expect(errorEvent.error.category, VesperPlayerErrorCategory.platform);
    expect(errorEvent.error.details['domain'], 'subtitle');
    expect(errorEvent.error.details['code'], 'subtitle_selection_timeout');
    expect(errorEvent.error.details['phase'], 'selection');
    expect(errorEvent.error.details['trackId'], 'external-en');
    expect(errorEvent.error.details['commandId'], 42);
    expect(errorEvent.error.details['sourceEpoch'], 9);
    final snapshotError = errorEvent.snapshot?.lastError;
    expect(snapshotError?.code, VesperPlayerErrorCode.backendFailure);
    expect(snapshotError?.category, VesperPlayerErrorCategory.platform);
    expect(snapshotError?.details['domain'], 'subtitle');
    expect(snapshotError?.details['code'], 'subtitle_selection_timeout');
  });

  test('player warning event decodes frame processor payload', () {
    final event = VesperPlayerEvent.fromMap(<Object?, Object?>{
      'playerId': 'macos-player',
      'type': 'warning',
      'warning': <Object?, Object?>{
        'domain': 'frameProcessor',
        'frameProcessor': <Object?, Object?>{
          'kind': 'deadlineMissed',
          'pluginName': 'fixture-processor',
          'processorIndex': 2,
          'frameId': 7,
          'framePtsUs': 33000,
          'inputHandleKind': 'CvPixelBuffer',
          'outputHandleKind': 'CvPixelBuffer',
          'processTimeUs': 50000,
          'deadlineOverrunUs': 34000,
          'policyAction': 'bypassOriginalFrame',
          'message': 'processor output missed frame deadline',
        },
      },
    });

    expect(event, isA<VesperPlayerWarningEvent>());
    final warningEvent = event as VesperPlayerWarningEvent;
    expect(warningEvent.playerId, 'macos-player');
    expect(
        warningEvent.warning.domain, VesperRuntimeWarningDomain.frameProcessor);
    expect(
      warningEvent.warning.frameProcessor?.kind,
      VesperFrameProcessorWarningKind.deadlineMissed,
    );
    expect(
      warningEvent.warning.frameProcessor?.pluginName,
      'fixture-processor',
    );
    expect(warningEvent.warning.frameProcessor?.processorIndex, 2);
    expect(warningEvent.warning.frameProcessor?.frameId, 7);
    expect(warningEvent.warning.frameProcessor?.framePtsUs, 33000);
    expect(
      warningEvent.warning.frameProcessor?.policyAction,
      VesperFrameProcessorPolicyAction.bypassOriginalFrame,
    );
  });

  test('player warning event preserves unknown frame processor wire values',
      () {
    final event = VesperPlayerEvent.fromMap(<Object?, Object?>{
      'playerId': 'macos-player',
      'type': 'warning',
      'warning': <Object?, Object?>{
        'domain': 'frameProcessor',
        'frameProcessor': <Object?, Object?>{
          'kind': 'futureFrameWarning',
          'pluginName': 'future-processor',
          'processorIndex': 3,
          'policyAction': 'futureAction',
          'message': 'future warning payload',
        },
      },
    });

    final warningEvent = event as VesperPlayerWarningEvent;
    final frameProcessor = warningEvent.warning.frameProcessor!;
    expect(frameProcessor.kind, VesperFrameProcessorWarningKind.unsupported);
    expect(frameProcessor.policyAction,
        VesperFrameProcessorPolicyAction.continuePlayback);
    expect(frameProcessor.kindRawValue, 'futureFrameWarning');
    expect(frameProcessor.policyActionRawValue, 'futureAction');
    expect(frameProcessor.toMap()['kind'], 'futureFrameWarning');
    expect(frameProcessor.toMap()['policyAction'], 'futureAction');
  });

  test('player warning event preserves unknown runtime warning payloads', () {
    final event = VesperPlayerEvent.fromMap(<Object?, Object?>{
      'playerId': 'ios-player',
      'type': 'warning',
      'warning': <Object?, Object?>{
        'domain': 'sourceNormalizer',
        'sourceNormalizer': <Object?, Object?>{
          'status': 'futureStatus',
          'cached': true,
        },
      },
    });

    final warningEvent = event as VesperPlayerWarningEvent;
    expect(warningEvent.warning.domain, VesperRuntimeWarningDomain.unknown);
    expect(warningEvent.warning.domainRawValue, 'sourceNormalizer');
    expect(warningEvent.warning.frameProcessor, isNull);
    expect(warningEvent.warning.capability, isNull);
    expect(warningEvent.warning.rawPayload['sourceNormalizer'], isA<Map>());
    final encoded = warningEvent.warning.toMap();
    expect(encoded['domain'], 'sourceNormalizer');
    expect(
      (encoded['sourceNormalizer'] as Map<Object?, Object?>)['status'],
      'futureStatus',
    );
  });

  test('player warning event decodes capability HDR fallback payload', () {
    final event = VesperPlayerEvent.fromMap(<Object?, Object?>{
      'playerId': 'ios-player',
      'type': 'warning',
      'warning': <Object?, Object?>{
        'domain': 'capability',
        'capability': <Object?, Object?>{
          'reason': 'hdrNativeFrameUnsupported',
          'recommendedPlaybackPath': 'systemPlayer',
          'hdrKind': 'dolbyVision',
          'likelyHdrCapabilityIssue': true,
          'confidence': 'sourceMetadata',
          'errorCode': 'ERROR_CODE_DECODER_INIT_FAILED',
          'capabilityFailureCause': 'decoderInit',
          'capabilityFailureAxis': 'decoder',
          'appProbeStatus': 'fallbackRequired',
          'appProbeRecommendedPlaybackPath': 'systemPlayer',
          'appProbeConfidence': 'sessionProbe',
          'appProbeHdrKind': 'dolbyVision',
          'appProbeDolbyVisionMode': 'compatibleBaseLayer',
          'appProbeMissingCapabilities':
              'hdrProgrammableProcessingNotSupported,displayHdrCapability',
          'appProbeSourceUri': 'https://example.com/movie-dv.mp4',
          'appProbeSourceProtocol': 'progressive',
          'appProbeSourceMatchesRuntime': true,
          'appProbeRuntimeRecommendedPathMatches': true,
          'appProbeRuntimeHdrKindMatches': true,
          'appProbeRuntimeDolbyVisionModeMatches': true,
          'appProbeRuntimeSystemPlayerRecommendationConfirmed': true,
          'appProbeDisplayHdrSupported': 'false',
          'appProbeDisplayFrameRateSupported': 'true',
          'appProbeCodecFormatSupported': 'false',
          'appProbeCodecFormatMissingCapability': 'codecProfileLevel',
          'appProbeCodecFormatSampleMimeType': 'video/dolby-vision',
          'appProbeCodecFormatCodecs': 'dvhe.08.07',
          'appProbeCodecFormatWidth': '3840',
          'appProbeCodecFormatHeight': '2160',
          'appProbeCodecFormatFrameRate': '60.0',
          'runtimeFormatHdrMetadataProbe': 'media3FormatColorInfo',
          'runtimeFormatColorSpace': 'bt2020',
          'runtimeFormatColorRange': 'limited',
          'runtimeFormatColorTransfer': 'st2084',
          'runtimeFormatHdrStaticInfoPresent': true,
          'runtimeFormatMaxContentLightLevelNits': 1000,
          'runtimeFormatMaxFrameAverageLightLevelNits': 400,
          'message': 'HDR/Dolby Vision uses system playback.',
        },
      },
    });

    expect(event, isA<VesperPlayerWarningEvent>());
    final warningEvent = event as VesperPlayerWarningEvent;
    expect(warningEvent.warning.domain, VesperRuntimeWarningDomain.capability);
    expect(
      warningEvent.warning.capability?.reason,
      VesperCapabilityWarningReason.hdrNativeFrameUnsupported,
    );
    expect(
      warningEvent.warning.capability?.recommendedPlaybackPath,
      VesperRecommendedPlaybackPath.systemPlayer,
    );
    expect(
      warningEvent.warning.capability?.hdrKind,
      VesperPlaybackCapabilityHdrKind.dolbyVision,
    );
    expect(
      warningEvent.warning.capability?.likelyHdrCapabilityIssue,
      isTrue,
    );
    expect(
      warningEvent.warning.capability?.confidence,
      'sourceMetadata',
    );
    expect(
      warningEvent.warning.capability?.errorCode,
      'ERROR_CODE_DECODER_INIT_FAILED',
    );
    expect(
      warningEvent.warning.capability?.capabilityFailureCause,
      'decoderInit',
    );
    expect(
      warningEvent.warning.capability?.capabilityFailureAxis,
      'decoder',
    );
    final appProbe = warningEvent.warning.capability?.appProbeConvergence;
    expect(
        appProbe?.status, VesperPlaybackCapabilityProbeStatus.fallbackRequired);
    expect(
      appProbe?.recommendedPlaybackPath,
      VesperRecommendedPlaybackPath.systemPlayer,
    );
    expect(
        appProbe?.confidence, VesperPlaybackCapabilityConfidence.sessionProbe);
    expect(appProbe?.hdrKind, VesperPlaybackCapabilityHdrKind.dolbyVision);
    expect(
      appProbe?.dolbyVisionMode,
      VesperPlaybackCapabilityDolbyVisionMode.compatibleBaseLayer,
    );
    expect(
      appProbe?.missingCapabilities,
      <String>[
        'hdrProgrammableProcessingNotSupported',
        'displayHdrCapability',
      ],
    );
    expect(appProbe?.sourceUri, 'https://example.com/movie-dv.mp4');
    expect(appProbe?.sourceProtocol, VesperPlayerSourceProtocol.progressive);
    expect(appProbe?.sourceMatchesRuntime, isTrue);
    expect(appProbe?.runtimeRecommendedPathMatches, isTrue);
    expect(appProbe?.runtimeHdrKindMatches, isTrue);
    expect(appProbe?.runtimeDolbyVisionModeMatches, isTrue);
    expect(appProbe?.runtimeSystemPlayerRecommendationConfirmed, isTrue);
    expect(appProbe?.displayHdrSupported, isFalse);
    expect(appProbe?.displayFrameRateSupported, isTrue);
    expect(appProbe?.codecFormatSupported, isFalse);
    expect(appProbe?.codecFormatMissingCapability, 'codecProfileLevel');
    expect(appProbe?.codecFormatSampleMimeType, 'video/dolby-vision');
    expect(appProbe?.codecFormatCodecs, 'dvhe.08.07');
    expect(appProbe?.codecFormatWidth, 3840);
    expect(appProbe?.codecFormatHeight, 2160);
    expect(appProbe?.codecFormatFrameRate, 60.0);
    expect(
      warningEvent.warning.capability?.diagnostics
          .containsKey('appProbeStatus'),
      isFalse,
    );
    expect(
      warningEvent.warning.capability?.diagnostics
          .containsKey('appProbeRuntimeHdrKindMatches'),
      isFalse,
    );
    expect(
      warningEvent.warning.capability?.diagnostics
          .containsKey('capabilityFailureCause'),
      isFalse,
    );
    expect(
      warningEvent.warning.capability?.diagnostics
          .containsKey('capabilityFailureAxis'),
      isFalse,
    );
    expect(
      warningEvent
          .warning.capability?.diagnostics['runtimeFormatHdrMetadataProbe'],
      'media3FormatColorInfo',
    );
    expect(
      warningEvent
          .warning.capability?.diagnostics['runtimeFormatColorTransfer'],
      'st2084',
    );
    expect(
      warningEvent.warning.capability
          ?.diagnostics['runtimeFormatMaxContentLightLevelNits'],
      1000,
    );
    expect(
      warningEvent.warning.capability?.hdrMetadata?.probe,
      'media3FormatColorInfo',
    );
    expect(
      warningEvent.warning.capability?.hdrMetadata?.colorSpace,
      'bt2020',
    );
    expect(
      warningEvent.warning.capability?.hdrMetadata?.colorRange,
      'limited',
    );
    expect(
      warningEvent.warning.capability?.hdrMetadata?.transferFunction,
      'st2084',
    );
    expect(
      warningEvent.warning.capability?.hdrMetadata?.hdrStaticInfoPresent,
      isTrue,
    );
    expect(
      warningEvent.warning.capability?.hdrMetadata?.maxContentLightLevelNits,
      1000,
    );
    expect(
      warningEvent
          .warning.capability?.hdrMetadata?.maxFrameAverageLightLevelNits,
      400,
    );
    expect(
      warningEvent.warning.capability
          ?.toMap()['runtimeFormatMaxFrameAverageLightLevelNits'],
      400,
    );
    expect(
      (warningEvent.warning.capability?.toMap()['hdrMetadata']
          as Map<Object?, Object?>)['transferFunction'],
      'st2084',
    );
    expect(
      warningEvent.warning.capability?.toMap()['appProbeStatus'],
      'fallbackRequired',
    );
    expect(
      warningEvent.warning.capability?.toMap()['appProbeMissingCapabilities'],
      <String>[
        'hdrProgrammableProcessingNotSupported',
        'displayHdrCapability',
      ],
    );
  });

  test('capability warning preserves unknown wire values', () {
    final event = VesperPlayerEvent.fromMap(<Object?, Object?>{
      'playerId': 'ios-player',
      'type': 'warning',
      'warning': <Object?, Object?>{
        'domain': 'capability',
        'capability': <Object?, Object?>{
          'reason': 'futureReason',
          'recommendedPlaybackPath': 'futurePath',
          'hdrKind': 'futureHdr',
          'appProbeStatus': 'futureStatus',
          'appProbeRecommendedPlaybackPath': 'futureAppPath',
          'appProbeConfidence': 'futureConfidence',
          'appProbeHdrKind': 'futureAppHdr',
          'appProbeDolbyVisionMode': 'futureDolbyVision',
          'appProbeSourceProtocol': 'futureProtocol',
        },
      },
    });

    final warningEvent = event as VesperPlayerWarningEvent;
    final capability = warningEvent.warning.capability!;
    expect(
      capability.reason,
      VesperCapabilityWarningReason.hdrNativeFrameUnsupported,
    );
    expect(
      capability.recommendedPlaybackPath,
      VesperRecommendedPlaybackPath.systemPlayer,
    );
    expect(capability.hdrKind, VesperPlaybackCapabilityHdrKind.unknown);
    expect(capability.reasonRawValue, 'futureReason');
    expect(capability.recommendedPlaybackPathRawValue, 'futurePath');
    expect(capability.hdrKindRawValue, 'futureHdr');
    expect(capability.toMap()['reason'], 'futureReason');
    expect(capability.toMap()['recommendedPlaybackPath'], 'futurePath');
    expect(capability.toMap()['hdrKind'], 'futureHdr');

    final appProbe = capability.appProbeConvergence!;
    expect(appProbe.status, VesperPlaybackCapabilityProbeStatus.unknown);
    expect(
      appProbe.recommendedPlaybackPath,
      VesperRecommendedPlaybackPath.systemPlayer,
    );
    expect(appProbe.confidence, VesperPlaybackCapabilityConfidence.codecOnly);
    expect(appProbe.hdrKind, VesperPlaybackCapabilityHdrKind.unknown);
    expect(
      appProbe.dolbyVisionMode,
      VesperPlaybackCapabilityDolbyVisionMode.none,
    );
    expect(appProbe.sourceProtocol, isNull);
    expect(appProbe.toMap()['appProbeStatus'], 'futureStatus');
    expect(
      appProbe.toMap()['appProbeRecommendedPlaybackPath'],
      'futureAppPath',
    );
    expect(appProbe.toMap()['appProbeConfidence'], 'futureConfidence');
    expect(appProbe.toMap()['appProbeHdrKind'], 'futureAppHdr');
    expect(
      appProbe.toMap()['appProbeDolbyVisionMode'],
      'futureDolbyVision',
    );
    expect(appProbe.toMap()['appProbeSourceProtocol'], 'futureProtocol');
  });

  test('player warning event decodes iOS runtime HDR capability hint', () {
    final event = VesperPlayerEvent.fromMap(<Object?, Object?>{
      'playerId': 'ios-player',
      'type': 'warning',
      'warning': <Object?, Object?>{
        'domain': 'capability',
        'capability': <Object?, Object?>{
          'reason': 'hdrNativeFrameUnsupported',
          'recommendedPlaybackPath': 'systemPlayer',
          'hdrKind': 'hdr10',
          'likelyHdrCapabilityIssue': true,
          'confidence': 'sessionProbe',
          'errorCode': 'decodeFailure',
          'capabilityFailureCause': 'decoderNotFound',
          'assetVideoTrackCount': '1',
          'assetVideoCodec': 'hvc1',
          'assetVideoWidth': '3840',
          'assetVideoHeight': '2160',
          'assetVideoFrameRate': '59.94',
          'assetVideoEstimatedDataRate': '25000000',
          'avPlayerItemErrorLogEventCount': '1',
          'avPlayerItemErrorStatusCode': '-11828',
          'message':
              'Playback failed after an HDR/Dolby Vision capability probe.',
        },
      },
    });

    expect(event, isA<VesperPlayerWarningEvent>());
    final warningEvent = event as VesperPlayerWarningEvent;
    expect(
      warningEvent.warning.capability?.hdrKind,
      VesperPlaybackCapabilityHdrKind.hdr10,
    );
    expect(
      warningEvent.warning.capability?.likelyHdrCapabilityIssue,
      isTrue,
    );
    expect(
      warningEvent.warning.capability?.confidence,
      'sessionProbe',
    );
    expect(warningEvent.warning.capability?.errorCode, 'decodeFailure');
    expect(
      warningEvent.warning.capability?.capabilityFailureCause,
      'decoderNotFound',
    );
    expect(
      warningEvent.warning.capability?.diagnostics['assetVideoTrackCount'],
      '1',
    );
    expect(
      warningEvent.warning.capability?.diagnostics['assetVideoCodec'],
      'hvc1',
    );
    expect(
      warningEvent.warning.capability?.diagnostics['assetVideoWidth'],
      '3840',
    );
    expect(
      warningEvent.warning.capability?.diagnostics['assetVideoHeight'],
      '2160',
    );
    expect(
      warningEvent.warning.capability?.diagnostics['assetVideoFrameRate'],
      '59.94',
    );
    expect(
      warningEvent
          .warning.capability?.diagnostics['assetVideoEstimatedDataRate'],
      '25000000',
    );
    expect(
      warningEvent
          .warning.capability?.diagnostics['avPlayerItemErrorLogEventCount'],
      '1',
    );
    expect(
      warningEvent
          .warning.capability?.diagnostics['avPlayerItemErrorStatusCode'],
      '-11828',
    );
  });

  test('platform create result decodes plugin diagnostics', () {
    final result = VesperPlatformCreateResult.fromMap(<Object?, Object?>{
      'playerId': 'macos-player',
      'snapshot': const VesperPlayerSnapshot.initial().toMap(),
      'pluginDiagnostics': <Object?>[
        <Object?, Object?>{
          'path': '/tmp/player-decoder-fixture.dylib',
          'pluginName': 'fixture-decoder',
          'pluginKind': 'decoder',
          'status': 'decoderSupported',
          'participation': 'selected',
          'message': 'fixture decoder loaded',
          'capability': <Object?, Object?>{
            'kind': 'decoder',
            'decoder': <Object?, Object?>{
              'codecs': <Object?>[
                <Object?, Object?>{
                  'mediaKind': 'Video',
                  'codec': 'h264',
                },
              ],
              'legacyCodecs': <String>['Video:h264'],
              'supportsNativeFrameOutput': true,
              'supportsHardwareDecode': true,
              'supportsGpuHandles': true,
              'supportsFlush': true,
              'supportsDrain': true,
              'maxSessions': 1,
            },
          },
        },
        <Object?, Object?>{
          'path': '/tmp/player-frame-processor-fixture.dylib',
          'pluginName': 'fixture-processor',
          'pluginKind': 'frame_processor',
          'status': 'frameProcessorSupported',
          'capability': <Object?, Object?>{
            'kind': 'frameProcessor',
            'frameProcessor': <Object?, Object?>{
              'acceptedInputHandleKinds': <String>['CvPixelBuffer'],
              'outputHandleKinds': <String>['CvPixelBuffer'],
              'acceptedInputPipelineProfiles': <String>[
                'video_toolbox_cv_pixel_buffer',
              ],
              'outputPipelineProfiles': <String>[
                'video_toolbox_cv_pixel_buffer',
              ],
              'supportsVideoFrames': true,
              'supportsInPlacePassthrough': true,
              'preservesDimensions': true,
              'preservesColorMetadata': true,
              'preservesHdrMetadata': true,
              'supportsFlush': true,
              'maxSessions': 2,
              'maxInFlightFrames': 4,
            },
          },
        },
        <Object?, Object?>{
          'path': '/tmp/player-source-normalizer-fixture.dylib',
          'pluginName': 'fixture-source-normalizer',
          'pluginKind': 'source_normalizer',
          'status': 'sourceNormalizerSupported',
          'participation': 'bypassed',
          'message': 'fixture source normalizer preflight completed',
          'capability': <Object?, Object?>{
            'kind': 'sourceNormalizer',
            'sourceNormalizer': <Object?, Object?>{
              'supportedRuntimeProfiles': <String>['generic-fallback'],
              'supportedOutputRoutes': <String>['packetStream'],
              'maxLevel': 'packet_repair',
              'mediaKinds': <String>['video'],
              'codecs': <String>['h264'],
              'bitstreamFormats': <String>['annex_b'],
              'supportsSeek': true,
              'supportsFlush': true,
              'supportsGrowingResources': false,
              'supportsRangeReads': false,
              'supportsCancel': false,
              'contentTypes': <String>[],
              'requiredLibraries': <String>['avformat'],
              'requiredDemuxers': <String>['mov'],
              'requiredMuxers': <String>['mp4'],
              'requiredProtocols': <String>['file'],
              'requiredParsers': <String>['h264'],
              'requiredBitstreamFilters': <String>['h264_mp4toannexb'],
              'requiredTls': 'secure-transport',
              'requiresNetwork': false,
              'maxSessions': 1,
            },
          },
        },
      ],
    });

    expect(result.pluginDiagnostics, hasLength(3));
    final decoder = result.pluginDiagnostics.first;
    expect(decoder.status, VesperPluginDiagnosticStatus.decoderSupported);
    expect(decoder.participation, VesperPluginParticipation.selected);
    expect(decoder.capability?.kind, VesperPluginCapabilityKind.decoder);
    expect(decoder.capability?.decoder?.codecs.single.codec, 'h264');
    expect(decoder.capability?.decoder?.legacyCodecs.single, 'Video:h264');
    expect(decoder.capability?.decoder?.supportsNativeFrameOutput, isTrue);
    expect(decoder.capability?.decoder?.maxSessions, 1);

    final frameProcessor = result.pluginDiagnostics[1];
    expect(
      frameProcessor.status,
      VesperPluginDiagnosticStatus.frameProcessorSupported,
    );
    expect(
      frameProcessor.capability?.kind,
      VesperPluginCapabilityKind.frameProcessor,
    );
    expect(
      frameProcessor
          .capability?.frameProcessor?.acceptedInputHandleKinds.single,
      'CvPixelBuffer',
    );
    expect(
      frameProcessor
          .capability?.frameProcessor?.acceptedInputPipelineProfiles.single,
      'video_toolbox_cv_pixel_buffer',
    );
    expect(
      frameProcessor.capability?.frameProcessor?.maxInFlightFrames,
      4,
    );
    expect(
      frameProcessor.capability?.toMap()['kind'],
      VesperPluginCapabilityKind.frameProcessor.name,
    );

    final sourceNormalizer = result.pluginDiagnostics[2];
    expect(
      sourceNormalizer.status,
      VesperPluginDiagnosticStatus.sourceNormalizerSupported,
    );
    expect(sourceNormalizer.participation, VesperPluginParticipation.bypassed);
    expect(
      sourceNormalizer.capability?.kind,
      VesperPluginCapabilityKind.sourceNormalizer,
    );
    expect(
      sourceNormalizer
          .capability?.sourceNormalizer?.supportedRuntimeProfiles.single,
      'generic-fallback',
    );
    expect(
      sourceNormalizer
          .capability?.sourceNormalizer?.supportedOutputRoutes.single,
      'packetStream',
    );
    expect(sourceNormalizer.capability?.sourceNormalizer?.requiresNetwork,
        isFalse);
  });

  test(
      'platform create result preserves source normalizer bypass diagnostic extras',
      () {
    final result = VesperPlatformCreateResult.fromMap(<Object?, Object?>{
      'playerId': 'android-player',
      'snapshot': const VesperPlayerSnapshot.initial().toMap(),
      'pluginDiagnostics': <Object?>[
        <Object?, Object?>{
          'path': '/data/local/tmp/libsource_normalizer.so',
          'pluginKind': 'source_normalizer',
          'status': 'sourceNormalizerUnsupported',
          'participation': 'bypassed',
          'message':
              'HdrResourceMetadataNotPreserved: source normalizer fMP4 resource route cannot currently guarantee HDR/Dolby Vision metadata preservation for system playback',
          'route': 'native',
          'fallbackReason': 'sourceNormalizerResourceBypassedForHdr',
        },
      ],
    });

    final diagnostic = result.pluginDiagnostics.single;
    expect(
      diagnostic.status,
      VesperPluginDiagnosticStatus.sourceNormalizerUnsupported,
    );
    expect(diagnostic.participation, VesperPluginParticipation.bypassed);
    expect(diagnostic.message, contains('HdrResourceMetadataNotPreserved'));
    expect(diagnostic.extra['route'], 'native');
    expect(
      diagnostic.extra['fallbackReason'],
      'sourceNormalizerResourceBypassedForHdr',
    );
  });

  test('mobile plugin configurations round-trip through maps', () {
    const sourceNormalizer = VesperSourceNormalizerConfiguration(
      mode: VesperSourceNormalizerMode.requireNormalized,
      pluginLibraryPaths: <String>['/tmp/libvesper_source_normalizer.dylib'],
      runtimeProfile: 'generic-fallback',
    );
    const preferBundled = VesperSourceNormalizerConfiguration.preferBundled();
    const requireBundled = VesperSourceNormalizerConfiguration.requireBundled(
      runtimeProfile: 'generic-fallback',
    );
    const frameProcessor = VesperFrameProcessorConfiguration(
      mode: VesperFrameProcessorMode.diagnosticsOnly,
      pluginLibraryPaths: <String>['/tmp/libvesper_frame_processor.dylib'],
    );
    const nativeFramePipeline = VesperNativeFramePipelineConfiguration(
      mode: VesperNativeFramePipelineMode.preferNativeFrame,
      decoderPluginLibraryPaths: <String>['/tmp/libvesper_decoder.dylib'],
      frameProcessorPluginLibraryPaths: <String>[
        '/tmp/libvesper_frame_processor.dylib'
      ],
      maxInFlightFrames: 2,
    );

    expect(
      VesperSourceNormalizerConfiguration.fromMap(sourceNormalizer.toMap())
          .mode,
      VesperSourceNormalizerMode.requireNormalized,
    );
    expect(preferBundled.toMap(), <String, Object?>{
      'mode': 'preferNormalized',
      'pluginLibraryPaths': <String>[],
    });
    expect(requireBundled.toMap(), <String, Object?>{
      'mode': 'requireNormalized',
      'pluginLibraryPaths': <String>[],
      'runtimeProfile': 'generic-fallback',
    });
    expect(
      VesperFrameProcessorConfiguration.fromMap(frameProcessor.toMap()).mode,
      VesperFrameProcessorMode.diagnosticsOnly,
    );
    expect(
      VesperNativeFramePipelineConfiguration.fromMap(
        nativeFramePipeline.toMap(),
      ).mode,
      VesperNativeFramePipelineMode.preferNativeFrame,
    );
    expect(
      VesperNativeFramePipelineConfiguration.fromMap(
        nativeFramePipeline.toMap(),
      ).maxInFlightFrames,
      2,
    );
  });

  test('picture-in-picture event decodes structured failure', () {
    final event = VesperPlayerEvent.fromMap(<Object?, Object?>{
      'playerId': 'android-player',
      'type': 'pictureInPicture',
      'state': 'failed',
      'isActive': false,
      'source': 'system',
      'canAutoEnter': true,
      'error': <Object?, Object?>{
        'code': 'pictureInPictureNativeFrameRouteCannotHandOff',
        'message': 'Native-frame route cannot hand off to system player.',
        'userMessage': 'Current playback cannot enter Picture in Picture.',
        'diagnostics': <Object?, Object?>{
          'route': 'nativeFramePipeline',
        },
      },
      'diagnostics': <Object?, Object?>{
        'platform': 'android',
      },
    });

    expect(event, isA<VesperPlayerPictureInPictureEvent>());
    final pip = event as VesperPlayerPictureInPictureEvent;
    expect(pip.playerId, 'android-player');
    expect(pip.state, VesperPictureInPictureStatus.failed);
    expect(pip.isActive, isFalse);
    expect(pip.canAutoEnter, isTrue);
    expect(
      pip.error?.code,
      VesperPictureInPictureErrorCode
          .pictureInPictureNativeFrameRouteCannotHandOff,
    );
    expect(
      pip.error?.userMessage,
      'Current playback cannot enter Picture in Picture.',
    );
    expect(pip.error?.diagnostics['route'], 'nativeFramePipeline');
    expect(pip.diagnostics['platform'], 'android');
  });

  test('picture-in-picture event preserves unknown state raw value', () {
    final event = VesperPlayerEvent.fromMap(<Object?, Object?>{
      'playerId': 'ios-player',
      'type': 'pictureInPicture',
      'state': 'suspendedBySystem',
      'isActive': true,
    });

    expect(event, isA<VesperPlayerPictureInPictureEvent>());
    final pip = event as VesperPlayerPictureInPictureEvent;
    expect(pip.state, VesperPictureInPictureStatus.inactive);
    expect(pip.stateRawValue, 'suspendedBySystem');
    expect(pip.isActive, isTrue);
  });

  test('unknown player event does not decode as snapshot', () {
    final event = VesperPlayerEvent.fromMap(<Object?, Object?>{
      'playerId': 'ios-player',
      'type': 'futureEvent',
      'value': 7,
    });

    expect(event, isA<VesperPlayerUnknownEvent>());
    final unknown = event as VesperPlayerUnknownEvent;
    expect(unknown.playerId, 'ios-player');
    expect(unknown.type, 'futureEvent');
    expect(unknown.payload['value'], 7);
  });
}
