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

  test('download manager event requires the breaking incremental type', () {
    expect(
      () => VesperDownloadManagerEvent.fromMap(<Object?, Object?>{
        'downloadId': 'downloads',
        'snapshot': const VesperDownloadSnapshot.initial().toMap(),
      }),
      throwsA(isA<FormatException>()),
    );
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
      warningEvent.warning.frameProcessor.kind,
      VesperFrameProcessorWarningKind.deadlineMissed,
    );
    expect(warningEvent.warning.frameProcessor.pluginName, 'fixture-processor');
    expect(warningEvent.warning.frameProcessor.processorIndex, 2);
    expect(warningEvent.warning.frameProcessor.frameId, 7);
    expect(warningEvent.warning.frameProcessor.framePtsUs, 33000);
    expect(
      warningEvent.warning.frameProcessor.policyAction,
      VesperFrameProcessorPolicyAction.bypassOriginalFrame,
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

  test('mobile plugin configurations round-trip through maps', () {
    const sourceNormalizer = VesperSourceNormalizerConfiguration(
      mode: VesperSourceNormalizerMode.requireNormalized,
      pluginLibraryPaths: <String>['/tmp/libplayer_source_normalizer.dylib'],
      runtimeProfile: 'generic-fallback',
    );
    const frameProcessor = VesperFrameProcessorConfiguration(
      mode: VesperFrameProcessorMode.diagnosticsOnly,
      pluginLibraryPaths: <String>['/tmp/libplayer_frame_processor.dylib'],
    );

    expect(
      VesperSourceNormalizerConfiguration.fromMap(sourceNormalizer.toMap())
          .mode,
      VesperSourceNormalizerMode.requireNormalized,
    );
    expect(
      VesperFrameProcessorConfiguration.fromMap(frameProcessor.toMap()).mode,
      VesperFrameProcessorMode.diagnosticsOnly,
    );
  });
}
