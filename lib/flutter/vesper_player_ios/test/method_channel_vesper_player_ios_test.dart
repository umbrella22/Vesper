import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:vesper_player_ios/src/method_channel_vesper_player_ios.dart';
import 'package:vesper_player_platform_interface/vesper_player_platform_interface.dart';

void main() {
  TestWidgetsFlutterBinding.ensureInitialized();

  const channel = MethodChannel('io.github.ikaros.vesper_player');
  final calls = <MethodCall>[];

  setUp(() {
    calls.clear();
    channel.setMethodCallHandler(null);
    TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger
        .setMockMethodCallHandler(channel, (call) async {
      calls.add(call);
      if (call.method == 'createPlayer') {
        return <String, Object?>{'playerId': 'ios-player'};
      }
      return null;
    });
  });

  tearDown(() {
    channel.setMethodCallHandler(null);
    TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger
        .setMockMethodCallHandler(channel, null);
  });

  test('native method handler registers lazily before first platform call',
      () async {
    final platform = MethodChannelVesperPlayerIos();
    final source = VesperDownloadSource.fromSource(
      source: VesperPlayerSource.hls(
        uri: 'https://example.com/archive.m3u8',
        label: 'Archive',
      ),
    );
    final task = VesperDownloadTaskSnapshot(
      taskId: 7,
      assetId: 'asset-7',
      source: source,
      profile: const VesperDownloadProfile(),
      state: VesperDownloadState.failed,
      progress: const VesperDownloadProgressSnapshot(receivedBytes: 128),
      assetIndex: const VesperDownloadAssetIndex(
        contentFormat: VesperDownloadContentFormat.hlsSegments,
      ),
    );
    const staleResource = VesperDownloadStaleResource(
      taskId: 7,
      resourceId: 'manifest',
      uri: 'https://example.com/archive.m3u8',
      statusCode: 404,
      message: 'Manifest no longer exists.',
    );
    final recoveredPlan = VesperDownloadRecoveredTaskPlan(
      source: source,
      profile: const VesperDownloadProfile(),
      assetIndex: const VesperDownloadAssetIndex(
        contentFormat: VesperDownloadContentFormat.hlsSegments,
      ),
    );

    final beforeFirstPlatformCall = await _invokeNativeMethodCall(
      MethodCall('recoverDownloadTaskPlan', <String, Object?>{
        'downloadId': 'downloads',
        'task': task.toMap(),
        'staleResource': staleResource.toMap(),
      }),
    );

    expect(beforeFirstPlatformCall, isNull);

    TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger
        .setMockMethodCallHandler(channel, (call) async {
      calls.add(call);
      if (call.method == 'createDownloadManager') {
        return <String, Object?>{'downloadId': 'downloads'};
      }
      return null;
    });

    await platform.createDownloadManager(
      staleResourceRecovery: (receivedTask, receivedStaleResource) {
        expect(receivedTask.taskId, task.taskId);
        expect(receivedTask.assetId, task.assetId);
        expect(receivedStaleResource.resourceId, staleResource.resourceId);
        expect(receivedStaleResource.statusCode, staleResource.statusCode);
        return recoveredPlan;
      },
    );

    final recovered = await _invokeNativeMethodCall(
      MethodCall('recoverDownloadTaskPlan', <String, Object?>{
        'downloadId': 'downloads',
        'task': task.toMap(),
        'staleResource': staleResource.toMap(),
      }),
    );

    expect(calls.single.method, 'createDownloadManager');
    expect(Map<Object?, Object?>.from(recovered as Map), recoveredPlan.toMap());
  });

  test('createPlayer forwards sparse defaults payloads', () async {
    final platform = MethodChannelVesperPlayerIos();
    final source = VesperPlayerSource.hls(
      uri: 'https://example.com/live.m3u8',
      label: 'Live',
      drmConfiguration: const VesperPlayerDrmConfiguration(
        keySystem: 'fairPlay',
        licenseUri: 'https://license.example.com/fairplay',
        licenseHeaders: <String, String>{'Authorization': 'Bearer token'},
        fairPlayCertificateUri: 'https://license.example.com/fairplay.cer',
      ),
    );
    const policy = VesperPlaybackResiliencePolicy.resilient();
    const trackPreferencePolicy = VesperTrackPreferencePolicy(
      preferredAudioLanguage: 'ja',
      selectSubtitlesByDefault: true,
      subtitleSelection: VesperTrackSelection.track('subtitle:ja'),
    );
    const preloadBudgetPolicy = VesperPreloadBudgetPolicy(
      maxConcurrentTasks: 2,
      warmupWindowMs: 30000,
    );

    final result = await platform.createPlayer(
      initialSource: source,
      resiliencePolicy: policy,
      trackPreferencePolicy: trackPreferencePolicy,
      preloadBudgetPolicy: preloadBudgetPolicy,
    );

    expect(result.playerId, 'ios-player');
    expect(calls, hasLength(1));
    expect(calls.single.method, 'createPlayer');
    expect(
      Map<Object?, Object?>.from(calls.single.arguments as Map),
      <Object?, Object?>{
        'initialSource': source.toMap(),
        'renderSurfaceKind': VesperPlayerRenderSurfaceKind.auto.name,
        'resiliencePolicy': policy.toMap(),
        'trackPreferencePolicy': trackPreferencePolicy.toMap(),
        'preloadBudgetPolicy': preloadBudgetPolicy.toMap(),
      },
    );
  });

  test('createPlayer forwards benchmark configuration when provided', () async {
    final platform = MethodChannelVesperPlayerIos();
    const benchmarkConfiguration = VesperBenchmarkConfiguration(
      enabled: true,
      maxBufferedEvents: 1024,
      includeRawEvents: true,
      consoleLogging: true,
      pluginLibraryPaths: <String>['/tmp/libvesper_sink.dylib'],
    );

    await platform.createPlayer(
      benchmarkConfiguration: benchmarkConfiguration,
    );

    expect(calls, hasLength(1));
    expect(calls.single.method, 'createPlayer');
    expect(
      Map<Object?, Object?>.from(calls.single.arguments as Map),
      <Object?, Object?>{
        'initialSource': null,
        'renderSurfaceKind': VesperPlayerRenderSurfaceKind.auto.name,
        'resiliencePolicy': const VesperPlaybackResiliencePolicy().toMap(),
        'benchmarkConfiguration': benchmarkConfiguration.toMap(),
      },
    );
  });

  test('createPlayer forwards mobile plugin configurations when provided',
      () async {
    final platform = MethodChannelVesperPlayerIos();
    const sourceNormalizerConfiguration = VesperSourceNormalizerConfiguration(
      mode: VesperSourceNormalizerMode.preflightOnly,
      pluginLibraryPaths: <String>[
        '/Frameworks/SourceNormalizer.framework/SourceNormalizer'
      ],
      runtimeProfile: 'generic-fallback',
    );
    const frameProcessorConfiguration = VesperFrameProcessorConfiguration(
      mode: VesperFrameProcessorMode.diagnosticsOnly,
      pluginLibraryPaths: <String>[
        '/Frameworks/FrameProcessor.framework/FrameProcessor'
      ],
    );
    const nativeFramePipelineConfiguration =
        VesperNativeFramePipelineConfiguration(
      mode: VesperNativeFramePipelineMode.requireNativeFrame,
      decoderPluginLibraryPaths: <String>[
        '/Frameworks/VideoToolboxDecoder.framework/VideoToolboxDecoder'
      ],
      frameProcessorPluginLibraryPaths: <String>[
        '/Frameworks/FrameProcessor.framework/FrameProcessor'
      ],
      maxInFlightFrames: 2,
    );

    await platform.createPlayer(
      sourceNormalizerConfiguration: sourceNormalizerConfiguration,
      frameProcessorConfiguration: frameProcessorConfiguration,
      nativeFramePipelineConfiguration: nativeFramePipelineConfiguration,
    );

    expect(calls, hasLength(1));
    expect(
      Map<Object?, Object?>.from(calls.single.arguments as Map),
      containsPair('sourceNormalizer', sourceNormalizerConfiguration.toMap()),
    );
    expect(
      Map<Object?, Object?>.from(calls.single.arguments as Map),
      containsPair('frameProcessor', frameProcessorConfiguration.toMap()),
    );
    expect(
      Map<Object?, Object?>.from(calls.single.arguments as Map),
      containsPair(
        'nativeFramePipeline',
        nativeFramePipelineConfiguration.toMap(),
      ),
    );
  });

  test('probePlaybackCapability forwards request and decodes result', () async {
    TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger
        .setMockMethodCallHandler(channel, (call) async {
      calls.add(call);
      return <String, Object?>{
        'status': 'fallbackRequired',
        'codecFamily': 'hevc',
        'systemPlaybackSupported': true,
        'hardwareDecodeSupported': true,
        'sdkManagedNativeFrameSupported': false,
        'recommendedPlaybackPath': 'systemPlayer',
        'outputFormat': 'surfaceOpaque',
        'hdrKind': 'dolbyVision',
        'dolbyVisionMode': 'unsupported',
        'confidence': 'sourceMetadata',
        'missingCapabilities': <String>[
          'hdrProgrammableProcessingNotSupported'
        ],
        'hdrMetadata': <String, Object?>{
          'hdrKind': 'dolbyVision',
          'dolbyVisionMode': 'unsupported',
          'probe': 'formatDescription',
          'codec': 'dvh1',
          'colorPrimaries': 'ITU_R_2020',
          'transferFunction': 'SMPTE_ST_2084_PQ',
          'yCbCrMatrix': 'ITU_R_2020',
          'masteringDisplayColorVolumePresent': true,
          'masteringDisplayPrimary0': <String, Object?>{
            'x': 0.38970,
            'y': 0.17204,
          },
          'masteringDisplayWhitePoint': <String, Object?>{
            'x': 0.20000,
            'y': 0.20000,
          },
          'masteringDisplayMaxLuminanceNits': 1000.0,
          'masteringDisplayMinLuminanceNits': 0.0001,
          'maxContentLightLevelNits': 1000,
          'maxFrameAverageLightLevelNits': 400,
          'dolbyVisionProfile': 5,
          'dolbyVisionLevel': 6,
          'dolbyVisionCompatibility': 'noCompatibleBaseLayer',
          'dolbyVisionProfileFamily': 'profile5SingleLayer',
          'dolbyVisionBaseLayer': 'none',
          'dolbyVisionFallbackTarget': 'dolbyVisionSystemPlayer',
        },
        'diagnostics': <String, Object?>{
          'probeVersion': '1',
          'playbackPathPolicy': 'hdrSystemPlaybackOnly',
          'recommendedPlaybackPathReason': 'hdrNativeFrameUnsupported',
        },
      };
    });
    final platform = MethodChannelVesperPlayerIos();
    const source = VesperPlayerSource(
      uri: 'file:///tmp/hdr.mov',
      label: 'hdr.mov',
      kind: VesperPlayerSourceKind.local,
      protocol: VesperPlayerSourceProtocol.file,
    );
    const request = VesperPlaybackCapabilityProbeRequest(
      source: source,
      codec: 'dvh1.05.06',
      width: 3840,
      height: 2160,
      frameRate: 59.94,
      nativeFramePipelineConfiguration: VesperNativeFramePipelineConfiguration(
        mode: VesperNativeFramePipelineMode.preferNativeFrame,
        decoderPluginLibraryPaths: <String>[
          '/Frameworks/VideoToolboxDecoder.framework/VideoToolboxDecoder'
        ],
      ),
    );

    final result = await platform.probePlaybackCapability(request);

    expect(calls.single.method, 'probePlaybackCapability');
    expect(
      Map<Object?, Object?>.from(calls.single.arguments as Map),
      request.toMap(),
    );
    expect(result.status, VesperPlaybackCapabilityProbeStatus.fallbackRequired);
    expect(result.codecFamily, VesperPlaybackCodecFamily.hevc);
    expect(
      result.recommendedPlaybackPath,
      VesperRecommendedPlaybackPath.systemPlayer,
    );
    expect(result.outputFormat,
        VesperPlaybackCapabilityOutputFormat.surfaceOpaque);
    expect(result.hdrKind, VesperPlaybackCapabilityHdrKind.dolbyVision);
    expect(
      result.dolbyVisionMode,
      VesperPlaybackCapabilityDolbyVisionMode.unsupported,
    );
    expect(result.hdrMetadata?.probe, 'formatDescription');
    expect(result.hdrMetadata?.codec, 'dvh1');
    expect(result.hdrMetadata?.colorPrimaries, 'ITU_R_2020');
    expect(result.hdrMetadata?.transferFunction, 'SMPTE_ST_2084_PQ');
    expect(result.hdrMetadata?.yCbCrMatrix, 'ITU_R_2020');
    expect(result.hdrMetadata?.masteringDisplayColorVolumePresent, isTrue);
    expect(result.hdrMetadata?.masteringDisplayPrimary0?.x, 0.38970);
    expect(result.hdrMetadata?.masteringDisplayWhitePoint?.y, 0.20000);
    expect(result.hdrMetadata?.masteringDisplayMaxLuminanceNits, 1000.0);
    expect(result.hdrMetadata?.masteringDisplayMinLuminanceNits, 0.0001);
    expect(result.hdrMetadata?.maxContentLightLevelNits, 1000);
    expect(result.hdrMetadata?.maxFrameAverageLightLevelNits, 400);
    expect(result.hdrMetadata?.dolbyVisionProfile, 5);
    expect(
        result.hdrMetadata?.dolbyVisionCompatibility, 'noCompatibleBaseLayer');
    expect(result.hdrMetadata?.dolbyVisionProfileFamily, 'profile5SingleLayer');
    expect(result.hdrMetadata?.dolbyVisionBaseLayer, 'none');
    expect(result.hdrMetadata?.dolbyVisionFallbackTarget,
        'dolbyVisionSystemPlayer');
  });

  test('createPlayer accepts explicit render surface kind', () async {
    final platform = MethodChannelVesperPlayerIos();

    await platform.createPlayer(
      renderSurfaceKind: VesperPlayerRenderSurfaceKind.surfaceView,
    );

    expect(calls, hasLength(1));
    expect(calls.single.method, 'createPlayer');
    expect(
      Map<Object?, Object?>.from(calls.single.arguments as Map),
      <Object?, Object?>{
        'initialSource': null,
        'renderSurfaceKind': VesperPlayerRenderSurfaceKind.surfaceView.name,
        'resiliencePolicy': const VesperPlaybackResiliencePolicy().toMap(),
      },
    );
  });

  test('createPlayer forwards disabled keep-screen-on policy', () async {
    final platform = MethodChannelVesperPlayerIos();

    await platform.createPlayer(keepScreenOnDuringPlayback: false);

    expect(calls, hasLength(1));
    expect(calls.single.method, 'createPlayer');
    expect(
      Map<Object?, Object?>.from(calls.single.arguments as Map),
      <Object?, Object?>{
        'initialSource': null,
        'renderSurfaceKind': VesperPlayerRenderSurfaceKind.auto.name,
        'resiliencePolicy': const VesperPlaybackResiliencePolicy().toMap(),
        'keepScreenOnDuringPlayback': false,
      },
    );
  });

  test('setKeepScreenOnDuringPlayback forwards player id and flag', () async {
    final platform = MethodChannelVesperPlayerIos();

    await platform.setKeepScreenOnDuringPlayback('ios-player', false);

    expect(calls, hasLength(1));
    expect(calls.single.method, 'setKeepScreenOnDuringPlayback');
    expect(
      Map<Object?, Object?>.from(calls.single.arguments as Map),
      <Object?, Object?>{
        'playerId': 'ios-player',
        'enabled': false,
      },
    );
  });

  test(
    'setResiliencePolicy preserves explicit unlimited retry override',
    () async {
      final platform = MethodChannelVesperPlayerIos();
      const policy = VesperPlaybackResiliencePolicy(
        buffering: VesperBufferingPolicy.streaming(),
        retry: VesperRetryPolicy(maxAttempts: null),
        cache: VesperCachePolicy.streaming(),
      );

      await platform.setResiliencePolicy('ios-player', policy);

      expect(calls, hasLength(1));
      expect(calls.single.method, 'setResiliencePolicy');
      expect(
        Map<Object?, Object?>.from(calls.single.arguments as Map),
        <Object?, Object?>{'playerId': 'ios-player', 'policy': policy.toMap()},
      );
    },
  );

  test('refreshPlayer forwards player id', () async {
    final platform = MethodChannelVesperPlayerIos();

    await platform.refreshPlayer('ios-player');

    expect(calls, hasLength(1));
    expect(calls.single.method, 'refreshPlayer');
    expect(
      Map<Object?, Object?>.from(calls.single.arguments as Map),
      <Object?, Object?>{'playerId': 'ios-player'},
    );
  });

  test('typed unsupported platform error maps to unsupported exception',
      () async {
    TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger
        .setMockMethodCallHandler(channel, (_) async {
      throw PlatformException(
        code: 'vesper_operation_failed',
        message: 'unsupported operation',
        details: <String, Object?>{
          'code': 'unsupported',
          'category': 'capability',
          'retriable': false,
          'message': 'unsupported operation',
          'details': <String, Object?>{
            'reason': 'drmUnsupportedRoute',
            'route': 'dash',
            'keySystem': 'fairPlay',
          },
        },
      );
    });
    final platform = MethodChannelVesperPlayerIos();

    await expectLater(
      platform.refreshPlayer('ios-player'),
      throwsA(
        isA<VesperUnsupportedError>()
            .having(
              (error) => error.platformCode,
              'platformCode',
              'vesper_operation_failed',
            )
            .having(
              (error) => error.platformDetails['code'],
              'details.code',
              'unsupported',
            )
            .having(
              (error) => (error.platformDetails['details'] as Map?)?['reason'],
              'details.details.reason',
              'drmUnsupportedRoute',
            ),
      ),
    );
  });

  test('non-capability unsupported platform error is not remapped', () async {
    TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger
        .setMockMethodCallHandler(channel, (_) async {
      throw PlatformException(
        code: 'vesper_operation_failed',
        message: 'legacy unsupported',
        details: <String, Object?>{
          'code': 'unsupported',
          'category': 'platform',
          'message': 'unsupported platform failure',
        },
      );
    });
    final platform = MethodChannelVesperPlayerIos();

    expect(
      () => platform.refreshPlayer('ios-player'),
      throwsA(isA<PlatformException>()),
    );
  });

  test('download DRM unsupported platform error maps to unsupported exception',
      () async {
    TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger
        .setMockMethodCallHandler(channel, (_) async {
      throw PlatformException(
        code: 'vesper_download_operation_failed',
        message: 'DRM is not supported on the download playback route.',
        details: <String, Object?>{
          'code': 'unsupported',
          'category': 'capability',
          'retriable': false,
          'message': 'DRM is not supported on the download playback route.',
          'details': <String, Object?>{
            'reason': 'drmUnsupportedRoute',
            'route': 'download',
            'keySystem': 'fairPlay',
          },
        },
      );
    });
    final platform = MethodChannelVesperPlayerIos();

    await expectLater(
      platform.createDownloadTask(
        'downloads',
        assetId: 'movie',
        source: VesperDownloadSource.fromSource(
          source: VesperPlayerSource.hls(
            uri: 'https://example.com/movie.m3u8',
            drmConfiguration: const VesperPlayerDrmConfiguration(
              keySystem: 'fairPlay',
              licenseUri: 'https://license.example.com/fairplay',
            ),
          ),
        ),
      ),
      throwsA(
        isA<VesperUnsupportedError>().having(
          (error) => (error.platformDetails['details'] as Map?)?['route'],
          'details.details.route',
          'download',
        ),
      ),
    );
  });

  test('snapshot decodes native HDR failure evidence details', () async {
    const eventChannel = EventChannel('io.github.ikaros.vesper_player/events');
    final platform = MethodChannelVesperPlayerIos();
    final events = <VesperPlayerEvent>[];

    final subscription = platform.eventsFor('ios-player').listen(events.add);
    addTearDown(subscription.cancel);
    await TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger
        .handlePlatformMessage(
      eventChannel.name,
      const StandardMethodCodec().encodeSuccessEnvelope(<String, Object?>{
        'playerId': 'ios-player',
        'type': 'snapshot',
        'snapshot': <String, Object?>{
          'title': 'Demo',
          'subtitle': 'Decoder failed',
          'sourceLabel': 'HDR',
          'playbackState': 'ready',
          'playbackRate': 1.0,
          'isBuffering': false,
          'isInterrupted': false,
          'hasVideoSurface': false,
          'timeline': const VesperTimeline.initial().toMap(),
          'lastError': <String, Object?>{
            'message': 'decoder unavailable',
            'code': 'decodeFailure',
            'category': 'decode',
            'retriable': false,
            'details': <String, Object?>{
              'likelyHdrCapabilityIssue': 'true',
              'hdrKind': 'dolbyVision',
              'recommendedPlaybackPath': 'systemPlayer',
              'confidence': 'sourceMetadata',
              'capabilityFailureCause': 'decoderNotFound',
              'assetVideoTrackCount': '1',
              'assetVideoCodec': 'hvc1',
              'assetVideoWidth': '3840',
              'assetVideoHeight': '2160',
              'assetVideoFrameRate': '59.94',
              'assetVideoEstimatedDataRate': '25000000',
              'assetVideoTransferFunction': 'SMPTE_ST_2084_PQ',
              'dolbyVisionProfile': '8',
              'dolbyVisionCompatibility': 'profile8Hdr10BaseLayer',
              'dolbyVisionProfileFamily': 'profile8SingleLayerCompatible',
              'dolbyVisionBaseLayer': 'hdr10BaseLayer',
              'dolbyVisionFallbackTarget': 'hdr10BaseLayerSystemPlayer',
              'dolbyVisionBaseLayerEvidence': 'assetVideoTransferFunction',
              'dolbyVisionBaseLayerTransferFunction': 'SMPTE_ST_2084_PQ',
              'hdrMetadata': <String, Object?>{
                'hdrKind': 'dolbyVision',
                'dolbyVisionMode': 'compatibleBaseLayer',
                'transferFunction': 'SMPTE_ST_2084_PQ',
                'dolbyVisionProfile': 8,
                'dolbyVisionCompatibility': 'profile8Hdr10BaseLayer',
                'dolbyVisionProfileFamily': 'profile8SingleLayerCompatible',
                'dolbyVisionBaseLayer': 'hdr10BaseLayer',
                'dolbyVisionFallbackTarget': 'hdr10BaseLayerSystemPlayer',
                'dolbyVisionBaseLayerEvidence': 'assetVideoTransferFunction',
                'dolbyVisionBaseLayerTransferFunction': 'SMPTE_ST_2084_PQ',
              },
            },
          },
        },
      }),
      (_) {},
    );

    expect(events.single, isA<VesperPlayerSnapshotEvent>());
    final snapshot = (events.single as VesperPlayerSnapshotEvent).snapshot;
    expect(snapshot.lastError?.details['likelyHdrCapabilityIssue'], 'true');
    expect(snapshot.lastError?.details['hdrKind'], 'dolbyVision');
    expect(snapshot.lastError?.details['hdrMetadata'],
        isA<Map<Object?, Object?>>());
    expect(
      snapshot.lastError?.details['recommendedPlaybackPath'],
      'systemPlayer',
    );
    expect(snapshot.lastError?.details['assetVideoWidth'], '3840');
    expect(snapshot.lastError?.details['assetVideoFrameRate'], '59.94');
    expect(snapshot.lastError?.details['dolbyVisionProfile'], '8');
    final evidence = snapshot.lastError?.hdrCapabilityEvidence;
    expect(evidence?.likelyHdrCapabilityIssue, isTrue);
    expect(evidence?.hdrKind, VesperPlaybackCapabilityHdrKind.dolbyVision);
    expect(
      evidence?.recommendedPlaybackPath,
      VesperRecommendedPlaybackPath.systemPlayer,
    );
    expect(evidence?.confidence, 'sourceMetadata');
    expect(evidence?.capabilityFailureCause, 'decoderNotFound');
    expect(evidence?.hdrMetadata?.dolbyVisionProfile, 8);
    expect(
      evidence?.hdrMetadata?.dolbyVisionMode,
      VesperPlaybackCapabilityDolbyVisionMode.compatibleBaseLayer,
    );
    expect(
      evidence?.hdrMetadata?.dolbyVisionCompatibility,
      'profile8Hdr10BaseLayer',
    );
    expect(
      evidence?.hdrMetadata?.dolbyVisionProfileFamily,
      'profile8SingleLayerCompatible',
    );
    expect(evidence?.hdrMetadata?.dolbyVisionBaseLayerEvidence,
        'assetVideoTransferFunction');
    expect(evidence?.hdrMetadata?.dolbyVisionBaseLayerTransferFunction,
        'SMPTE_ST_2084_PQ');
    expect(evidence?.diagnostics['assetVideoTrackCount'], '1');
    expect(evidence?.diagnostics['assetVideoCodec'], 'hvc1');
    expect(evidence?.diagnostics['assetVideoWidth'], '3840');
    expect(evidence?.diagnostics['assetVideoHeight'], '2160');
    expect(evidence?.diagnostics['assetVideoFrameRate'], '59.94');
    expect(evidence?.diagnostics['assetVideoEstimatedDataRate'], '25000000');
  });

  test('event stream decodes FairPlay terminal error and snapshot details',
      () async {
    const eventChannel = EventChannel('io.github.ikaros.vesper_player/events');
    final platform = MethodChannelVesperPlayerIos();
    final events = <VesperPlayerEvent>[];
    final terminalError = <String, Object?>{
      'message': 'FairPlay license request failed.',
      'code': 'backendFailure',
      'category': 'network',
      'retriable': true,
      'details': <String, Object?>{
        'reason': 'fairPlayLicenseRequestFailed',
        'keySystem': 'fairPlay',
        'route': 'direct',
        'licenseUriHost': 'license.example.com',
        'certificateUriHost': 'cert.example.com',
        'httpStatusCode': '503',
        'attemptsExhausted': true,
        'maxAttempts': 3,
      },
    };

    final subscription = platform.eventsFor('ios-player').listen(events.add);
    addTearDown(subscription.cancel);
    await TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger
        .handlePlatformMessage(
      eventChannel.name,
      const StandardMethodCodec().encodeSuccessEnvelope(<String, Object?>{
        'playerId': 'ios-player',
        'type': 'error',
        'error': terminalError,
        'snapshot': <String, Object?>{
          'title': 'Demo',
          'subtitle': 'FairPlay failed',
          'sourceLabel': 'P8.1 FairPlay',
          'playbackState': 'paused',
          'playbackRate': 1.0,
          'isBuffering': false,
          'isInterrupted': false,
          'hasVideoSurface': true,
          'timeline': const VesperTimeline.initial().toMap(),
          'lastError': terminalError,
        },
      }),
      (_) {},
    );

    expect(events.single, isA<VesperPlayerErrorEvent>());
    final event = events.single as VesperPlayerErrorEvent;
    expect(event.error.category, VesperPlayerErrorCategory.network);
    expect(event.error.retriable, isTrue);
    expect(event.error.details['keySystem'], 'fairPlay');
    expect(event.error.details['licenseUriHost'], 'license.example.com');
    expect(event.error.details['certificateUriHost'], 'cert.example.com');
    expect(event.error.details['attemptsExhausted'], isTrue);
    expect(event.snapshot?.playbackState, VesperPlaybackState.paused);
    expect(event.snapshot?.isBuffering, isFalse);
    expect(
      event.snapshot?.lastError?.details['reason'],
      'fairPlayLicenseRequestFailed',
    );
    expect(event.snapshot?.lastError?.details['maxAttempts'], 3);
  });

  test('download output helpers forward payloads', () async {
    TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger
        .setMockMethodCallHandler(channel, (call) async {
      calls.add(call);
      if (call.method == 'saveDownloadTask') {
        return null;
      }
      return null;
    });
    final platform = MethodChannelVesperPlayerIos();

    await platform.shareDownloadTask(
      'downloads',
      42,
      fileName: 'movie.mp4',
      mimeType: 'video/mp4',
    );
    final savedUri = await platform.saveDownloadTask(
      'downloads',
      42,
      fileName: 'movie.mp4',
      collection: VesperDownloadPublicCollection.movies,
    );

    expect(savedUri, isNull);
    expect(calls.map((call) => call.method), <String>[
      'shareDownloadTask',
      'saveDownloadTask',
    ]);
    expect(
      Map<Object?, Object?>.from(calls[0].arguments as Map),
      <Object?, Object?>{
        'downloadId': 'downloads',
        'taskId': 42,
        'fileName': 'movie.mp4',
        'mimeType': 'video/mp4',
      },
    );
    expect(
      Map<Object?, Object?>.from(calls[1].arguments as Map),
      <Object?, Object?>{
        'downloadId': 'downloads',
        'taskId': 42,
        'fileName': 'movie.mp4',
        'collection': VesperDownloadPublicCollection.movies.name,
      },
    );
  });

  test('updateViewport forwards derived shared hint payload', () async {
    final platform = MethodChannelVesperPlayerIos();
    const viewport = VesperPlayerViewport(
      left: 24,
      top: 48,
      width: 180,
      height: 120,
    );

    await platform.updateViewport('ios-player', viewport);

    expect(calls, hasLength(1));
    expect(calls.single.method, 'updateViewport');
    expect(
      Map<Object?, Object?>.from(calls.single.arguments as Map),
      <Object?, Object?>{
        'playerId': 'ios-player',
        'viewport': viewport.toMap(),
        'viewportHint': const VesperViewportHint(
          kind: VesperViewportHintKind.visible,
          visibleFraction: 1,
        ).toMap(),
      },
    );
  });

  test('system playback calls forward payloads', () async {
    final platform = MethodChannelVesperPlayerIos();
    const metadata = VesperSystemPlaybackMetadata(
      title: 'Episode',
      artist: 'Vesper',
      contentUri: 'https://example.com/video.m3u8',
      durationMs: 120000,
    );
    const configuration = VesperSystemPlaybackConfiguration(
      metadata: metadata,
    );

    await platform.configureSystemPlayback('ios-player', configuration);
    await platform.updateSystemPlaybackMetadata('ios-player', metadata);
    await platform.clearSystemPlayback('ios-player');

    expect(calls.map((call) => call.method), <String>[
      'configureSystemPlayback',
      'updateSystemPlaybackMetadata',
      'clearSystemPlayback',
    ]);
    expect(
      Map<Object?, Object?>.from(calls[0].arguments as Map),
      <Object?, Object?>{
        'playerId': 'ios-player',
        'configuration': configuration.toMap(),
      },
    );
    expect(
      Map<Object?, Object?>.from(calls[1].arguments as Map),
      <Object?, Object?>{
        'playerId': 'ios-player',
        'metadata': metadata.toMap(),
      },
    );
    expect(
      Map<Object?, Object?>.from(calls[2].arguments as Map),
      <Object?, Object?>{'playerId': 'ios-player'},
    );
  });

  test('picture-in-picture calls forward payloads', () async {
    TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger
        .setMockMethodCallHandler(channel, (call) async {
      calls.add(call);
      if (call.method == 'isPictureInPictureAvailable') {
        return <String, Object?>{
          'isAvailable': false,
          'isActive': false,
          'source': 'system',
          'error': const VesperPictureInPictureError(
            code:
                VesperPictureInPictureErrorCode.pictureInPictureDisabledByHost,
          ).toMap(),
        };
      }
      return null;
    });
    final platform = MethodChannelVesperPlayerIos();
    const configuration = VesperPictureInPictureConfiguration(
      autoEnter: true,
      preferredAspectRatio: 4 / 3,
    );

    final availability = await platform.isPictureInPictureAvailable(
      'ios-player',
    );
    await platform.setPictureInPictureConfiguration(
      'ios-player',
      configuration,
    );
    await platform.requestPictureInPicture(
      'ios-player',
      configuration: configuration,
    );
    await platform.requestPictureInPicture('ios-player');
    await platform.exitPictureInPicture('ios-player');

    expect(availability.isAvailable, isFalse);
    expect(
      availability.error?.code,
      VesperPictureInPictureErrorCode.pictureInPictureDisabledByHost,
    );
    expect(calls.map((call) => call.method), <String>[
      'isPictureInPictureAvailable',
      'setPictureInPictureConfiguration',
      'requestPictureInPicture',
      'requestPictureInPicture',
      'exitPictureInPicture',
    ]);
    expect(
      Map<Object?, Object?>.from(calls[1].arguments as Map),
      <Object?, Object?>{
        'playerId': 'ios-player',
        'configuration': configuration.toMap(),
      },
    );
    expect(
      Map<Object?, Object?>.from(calls[2].arguments as Map),
      <Object?, Object?>{
        'playerId': 'ios-player',
        'configuration': configuration.toMap(),
      },
    );
    expect(
      Map<Object?, Object?>.from(calls[3].arguments as Map),
      <Object?, Object?>{'playerId': 'ios-player'},
    );
    expect(
      Map<Object?, Object?>.from(calls[4].arguments as Map),
      <Object?, Object?>{'playerId': 'ios-player'},
    );
  });

  test('requestSystemPlaybackPermissions decodes notRequired status', () async {
    TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger
        .setMockMethodCallHandler(channel, (call) async {
      calls.add(call);
      return 'notRequired';
    });
    final platform = MethodChannelVesperPlayerIos();

    final status = await platform.requestSystemPlaybackPermissions();

    expect(status, VesperSystemPlaybackPermissionStatus.notRequired);
    expect(calls.single.method, 'requestSystemPlaybackPermissions');
  });

  test('getSystemPlaybackPermissionStatus decodes notRequired status',
      () async {
    TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger
        .setMockMethodCallHandler(channel, (call) async {
      calls.add(call);
      return 'notRequired';
    });
    final platform = MethodChannelVesperPlayerIos();

    final status = await platform.getSystemPlaybackPermissionStatus();

    expect(status, VesperSystemPlaybackPermissionStatus.notRequired);
    expect(calls.single.method, 'getSystemPlaybackPermissionStatus');
  });
}

Future<Object?> _invokeNativeMethodCall(MethodCall call) async {
  const codec = StandardMethodCodec();
  final response = await TestDefaultBinaryMessengerBinding
      .instance.defaultBinaryMessenger
      .handlePlatformMessage(
    'io.github.ikaros.vesper_player',
    codec.encodeMethodCall(call),
    null,
  );
  return response == null ? null : codec.decodeEnvelope(response);
}
