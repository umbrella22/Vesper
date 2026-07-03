import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:vesper_player_android/src/method_channel_vesper_player_android.dart';
import 'package:vesper_player_platform_interface/vesper_player_platform_interface.dart';

void main() {
  TestWidgetsFlutterBinding.ensureInitialized();

  const channel = MethodChannel('io.github.ikaros.vesper_player');
  final calls = <MethodCall>[];

  setUp(() {
    calls.clear();
    TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger
        .setMockMethodCallHandler(channel, (call) async {
      calls.add(call);
      if (call.method == 'createPlayer') {
        return <String, Object?>{'playerId': 'android-player'};
      }
      return null;
    });
  });

  tearDown(() {
    TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger
        .setMockMethodCallHandler(channel, null);
  });

  test('createPlayer forwards sparse defaults payloads', () async {
    final platform = MethodChannelVesperPlayerAndroid();
    final source = VesperPlayerSource.hls(
      uri: 'https://example.com/live.m3u8',
      label: 'Live',
      drmConfiguration: const VesperPlayerDrmConfiguration(
        keySystem: 'widevine',
        licenseUri: 'https://license.example.com/widevine',
        licenseHeaders: <String, String>{'Authorization': 'Bearer token'},
        multiSession: true,
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

    expect(result.playerId, 'android-player');
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
    final platform = MethodChannelVesperPlayerAndroid();
    const benchmarkConfiguration = VesperBenchmarkConfiguration(
      enabled: true,
      maxBufferedEvents: 1024,
      includeRawEvents: true,
      consoleLogging: true,
      pluginLibraryPaths: <String>['/data/local/tmp/libvesper_sink.so'],
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
    final platform = MethodChannelVesperPlayerAndroid();
    const sourceNormalizerConfiguration = VesperSourceNormalizerConfiguration(
      mode: VesperSourceNormalizerMode.preflightOnly,
      pluginLibraryPaths: <String>['/data/local/tmp/libsource.so'],
      runtimeProfile: 'generic-fallback',
    );
    const frameProcessorConfiguration = VesperFrameProcessorConfiguration(
      mode: VesperFrameProcessorMode.diagnosticsOnly,
      pluginLibraryPaths: <String>['/data/local/tmp/libframe.so'],
    );
    const nativeFramePipelineConfiguration =
        VesperNativeFramePipelineConfiguration(
      mode: VesperNativeFramePipelineMode.preferNativeFrame,
      decoderPluginLibraryPaths: <String>['/data/local/tmp/libdecoder.so'],
      frameProcessorPluginLibraryPaths: <String>['/data/local/tmp/libframe.so'],
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

  test('createPlayer decodes source normalizer HDR bypass diagnostics',
      () async {
    TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger
        .setMockMethodCallHandler(channel, (call) async {
      calls.add(call);
      return <String, Object?>{
        'playerId': 'android-player',
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
      };
    });
    final platform = MethodChannelVesperPlayerAndroid();

    final result = await platform.createPlayer();

    expect(calls.single.method, 'createPlayer');
    expect(result.pluginDiagnostics, hasLength(1));
    final diagnostic = result.pluginDiagnostics.single;
    expect(
      diagnostic.status,
      VesperPluginDiagnosticStatus.sourceNormalizerUnsupported,
    );
    expect(diagnostic.participation, VesperPluginParticipation.bypassed);
    expect(
      diagnostic.message,
      contains('HdrResourceMetadataNotPreserved'),
    );
    expect(diagnostic.extra['route'], 'native');
    expect(
      diagnostic.extra['fallbackReason'],
      'sourceNormalizerResourceBypassedForHdr',
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
          'probe': 'media3FormatColorInfo',
          'codec': 'dvh1.05.06',
          'sampleMimeType': 'video/dolby-vision',
          'colorSpace': 'bt2020',
          'colorRange': 'limited',
          'transferFunction': 'st2084',
          'lumaBitDepth': 10,
          'chromaBitDepth': 10,
          'hdrStaticInfoPresent': true,
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
    final platform = MethodChannelVesperPlayerAndroid();
    const source = VesperPlayerSource(
      uri: 'file:///tmp/hdr.mp4',
      label: 'hdr.mp4',
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
        mode: VesperNativeFramePipelineMode.requireNativeFrame,
        decoderPluginLibraryPaths: <String>['/tmp/libmediacodec.so'],
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
    expect(
      result.outputFormat,
      VesperPlaybackCapabilityOutputFormat.surfaceOpaque,
    );
    expect(result.hdrKind, VesperPlaybackCapabilityHdrKind.dolbyVision);
    expect(
      result.dolbyVisionMode,
      VesperPlaybackCapabilityDolbyVisionMode.unsupported,
    );
    expect(result.hdrMetadata?.probe, 'media3FormatColorInfo');
    expect(result.hdrMetadata?.sampleMimeType, 'video/dolby-vision');
    expect(result.hdrMetadata?.colorSpace, 'bt2020');
    expect(result.hdrMetadata?.transferFunction, 'st2084');
    expect(result.hdrMetadata?.lumaBitDepth, 10);
    expect(result.hdrMetadata?.hdrStaticInfoPresent, isTrue);
    expect(result.hdrMetadata?.maxContentLightLevelNits, 1000);
    expect(result.hdrMetadata?.maxFrameAverageLightLevelNits, 400);
    expect(result.hdrMetadata?.dolbyVisionProfile, 5);
    expect(
        result.hdrMetadata?.dolbyVisionCompatibility, 'noCompatibleBaseLayer');
    expect(result.hdrMetadata?.dolbyVisionProfileFamily, 'profile5SingleLayer');
    expect(result.hdrMetadata?.dolbyVisionBaseLayer, 'none');
    expect(result.hdrMetadata?.dolbyVisionFallbackTarget,
        'dolbyVisionSystemPlayer');
    expect(
      result.missingCapabilities,
      <String>['hdrProgrammableProcessingNotSupported'],
    );
  });

  test('snapshot decodes native HDR failure evidence details', () async {
    const eventChannel = EventChannel('io.github.ikaros.vesper_player/events');
    final platform = MethodChannelVesperPlayerAndroid();
    final events = <VesperPlayerEvent>[];

    final subscription =
        platform.eventsFor('android-player').listen(events.add);
    addTearDown(subscription.cancel);
    await TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger
        .handlePlatformMessage(
      eventChannel.name,
      const StandardMethodCodec().encodeSuccessEnvelope(<String, Object?>{
        'playerId': 'android-player',
        'type': 'snapshot',
        'snapshot': <String, Object?>{
          'title': 'Demo',
          'subtitle': 'Decoder failed',
          'sourceLabel': 'HDR',
          'playbackState': 'ready',
          'playbackRate': 1.0,
          'isBuffering': false,
          'isInterrupted': false,
          'hasVideoSurface': true,
          'timeline': const VesperTimeline.initial().toMap(),
          'lastError': <String, Object?>{
            'message': 'decoder init failed',
            'code': 'decodeFailure',
            'category': 'decode',
            'retriable': false,
            'details': <String, Object?>{
              'likelyHdrCapabilityIssue': true,
              'hdrKind': 'dolbyVision',
              'recommendedPlaybackPath': 'systemPlayer',
              'confidence': 'sessionProbe',
              'errorCode': 'ERROR_CODE_DECODER_INIT_FAILED',
              'capabilityFailureCause': 'decoderInit',
              'capabilityFailureAxis': 'decoder',
              'hdrMetadata': <String, Object?>{
                'hdrKind': 'dolbyVision',
                'dolbyVisionMode': 'compatibleBaseLayer',
                'probe': 'media3FormatColorInfo',
                'sampleMimeType': 'video/dolby-vision',
                'colorSpace': 'bt2020',
                'transferFunction': 'st2084',
                'dolbyVisionProfile': 8,
                'dolbyVisionCompatibility': 'profile8Hdr10BaseLayer',
                'dolbyVisionProfileFamily': 'profile8SingleLayerCompatible',
                'dolbyVisionBaseLayer': 'hdr10BaseLayer',
                'dolbyVisionFallbackTarget': 'hdr10BaseLayerSystemPlayer',
                'dolbyVisionBaseLayerEvidence': 'runtimeFormatColorTransfer',
                'dolbyVisionBaseLayerTransferFunction': 'st2084',
              },
            },
          },
        },
      }),
      (_) {},
    );

    expect(events.single, isA<VesperPlayerSnapshotEvent>());
    final snapshot = (events.single as VesperPlayerSnapshotEvent).snapshot;
    expect(
      snapshot.lastError?.details['capabilityFailureCause'],
      'decoderInit',
    );
    final evidence = snapshot.lastError?.hdrCapabilityEvidence;
    expect(evidence?.likelyHdrCapabilityIssue, isTrue);
    expect(evidence?.hdrKind, VesperPlaybackCapabilityHdrKind.dolbyVision);
    expect(evidence?.confidence, 'sessionProbe');
    expect(evidence?.errorCode, 'ERROR_CODE_DECODER_INIT_FAILED');
    expect(evidence?.capabilityFailureCause, 'decoderInit');
    expect(evidence?.capabilityFailureAxis, 'decoder');
    expect(evidence?.hdrMetadata?.probe, 'media3FormatColorInfo');
    expect(evidence?.hdrMetadata?.sampleMimeType, 'video/dolby-vision');
    expect(evidence?.hdrMetadata?.dolbyVisionProfile, 8);
    expect(
      evidence?.hdrMetadata?.dolbyVisionCompatibility,
      'profile8Hdr10BaseLayer',
    );
    expect(evidence?.hdrMetadata?.dolbyVisionBaseLayerEvidence,
        'runtimeFormatColorTransfer');
    expect(
        evidence?.hdrMetadata?.dolbyVisionBaseLayerTransferFunction, 'st2084');
  });

  test('error event carries terminal Widevine snapshot lastError details',
      () async {
    const eventChannel = EventChannel('io.github.ikaros.vesper_player/events');
    final platform = MethodChannelVesperPlayerAndroid();
    final events = <VesperPlayerEvent>[];

    final subscription =
        platform.eventsFor('android-player').listen(events.add);
    addTearDown(subscription.cancel);
    await TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger
        .handlePlatformMessage(
      eventChannel.name,
      const StandardMethodCodec().encodeSuccessEnvelope(<String, Object?>{
        'playerId': 'android-player',
        'type': 'error',
        'error': <String, Object?>{
          'message': 'Widevine license failed',
          'code': 'backendFailure',
          'category': 'network',
          'retriable': true,
          'details': <String, Object?>{
            'reason': 'drmLicenseAcquisitionFailed',
            'keySystem': 'widevine',
            'licenseUriHost': 'license.example.com',
            'attemptsExhausted': true,
            'maxAttempts': 3,
            'errorCodeName': 'ERROR_CODE_DRM_LICENSE_ACQUISITION_FAILED',
          },
        },
        'snapshot': <String, Object?>{
          'title': 'Demo',
          'subtitle': 'Widevine license failed',
          'sourceLabel': 'P8.1 Widevine',
          'playbackState': 'paused',
          'playbackRate': 1.0,
          'isBuffering': false,
          'isInterrupted': false,
          'hasVideoSurface': true,
          'timeline': const VesperTimeline.initial().toMap(),
          'lastError': <String, Object?>{
            'message': 'Widevine license failed',
            'code': 'backendFailure',
            'category': 'network',
            'retriable': true,
            'details': <String, Object?>{
              'reason': 'drmLicenseAcquisitionFailed',
              'keySystem': 'widevine',
              'licenseUriHost': 'license.example.com',
              'attemptsExhausted': true,
              'maxAttempts': 3,
            },
          },
        },
      }),
      (_) {},
    );

    expect(events.single, isA<VesperPlayerErrorEvent>());
    final event = events.single as VesperPlayerErrorEvent;
    expect(event.error.category, VesperPlayerErrorCategory.network);
    expect(event.error.retriable, isTrue);
    expect(event.error.details['attemptsExhausted'], isTrue);
    expect(event.error.details['maxAttempts'], 3);
    expect(event.snapshot?.playbackState, VesperPlaybackState.paused);
    expect(event.snapshot?.isBuffering, isFalse);
    expect(event.snapshot?.lastError?.details['keySystem'], 'widevine');
  });

  test('createPlayer forwards explicit texture render surface kind', () async {
    final platform = MethodChannelVesperPlayerAndroid();

    await platform.createPlayer(
      renderSurfaceKind: VesperPlayerRenderSurfaceKind.textureView,
    );

    expect(calls, hasLength(1));
    expect(calls.single.method, 'createPlayer');
    expect(
      Map<Object?, Object?>.from(calls.single.arguments as Map),
      <Object?, Object?>{
        'initialSource': null,
        'renderSurfaceKind': VesperPlayerRenderSurfaceKind.textureView.name,
        'resiliencePolicy': const VesperPlaybackResiliencePolicy().toMap(),
      },
    );
  });

  test('createPlayer forwards explicit surface render surface kind', () async {
    final platform = MethodChannelVesperPlayerAndroid();

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

  test('createPlayer does not add a public surface lifecycle policy', () async {
    final platform = MethodChannelVesperPlayerAndroid();

    await platform.createPlayer();

    final arguments = Map<Object?, Object?>.from(calls.single.arguments as Map);
    expect(arguments, containsPair('renderSurfaceKind', 'auto'));
    expect(arguments, isNot(contains('surfacePolicy')));
    expect(arguments, isNot(contains('viewLifecycleMode')));
  });

  test('createPlayer forwards disabled keep-screen-on policy', () async {
    final platform = MethodChannelVesperPlayerAndroid();

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
    final platform = MethodChannelVesperPlayerAndroid();

    await platform.setKeepScreenOnDuringPlayback('android-player', false);

    expect(calls, hasLength(1));
    expect(calls.single.method, 'setKeepScreenOnDuringPlayback');
    expect(
      Map<Object?, Object?>.from(calls.single.arguments as Map),
      <Object?, Object?>{
        'playerId': 'android-player',
        'enabled': false,
      },
    );
  });

  test(
    'setResiliencePolicy preserves explicit unlimited retry override',
    () async {
      final platform = MethodChannelVesperPlayerAndroid();
      const policy = VesperPlaybackResiliencePolicy(
        buffering: VesperBufferingPolicy.streaming(),
        retry: VesperRetryPolicy(maxAttempts: null),
        cache: VesperCachePolicy.streaming(),
      );

      await platform.setResiliencePolicy('android-player', policy);

      expect(calls, hasLength(1));
      expect(calls.single.method, 'setResiliencePolicy');
      expect(
        Map<Object?, Object?>.from(calls.single.arguments as Map),
        <Object?, Object?>{
          'playerId': 'android-player',
          'policy': policy.toMap(),
        },
      );
    },
  );

  test('refreshPlayer forwards player id', () async {
    final platform = MethodChannelVesperPlayerAndroid();

    await platform.refreshPlayer('android-player');

    expect(calls, hasLength(1));
    expect(calls.single.method, 'refreshPlayer');
    expect(
      Map<Object?, Object?>.from(calls.single.arguments as Map),
      <Object?, Object?>{'playerId': 'android-player'},
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
            'route': 'sourceNormalizer',
            'keySystem': 'widevine',
          },
        },
      );
    });
    final platform = MethodChannelVesperPlayerAndroid();

    await expectLater(
      platform.refreshPlayer('android-player'),
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
    final platform = MethodChannelVesperPlayerAndroid();

    expect(
      () => platform.refreshPlayer('android-player'),
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
            'keySystem': 'widevine',
          },
        },
      );
    });
    final platform = MethodChannelVesperPlayerAndroid();

    await expectLater(
      platform.createDownloadTask(
        'downloads',
        assetId: 'movie',
        source: VesperDownloadSource.fromSource(
          source: VesperPlayerSource.hls(
            uri: 'https://example.com/movie.m3u8',
            drmConfiguration: const VesperPlayerDrmConfiguration(
              keySystem: 'widevine',
              licenseUri: 'https://license.example.com/widevine',
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

  test('download output helpers forward payloads', () async {
    TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger
        .setMockMethodCallHandler(channel, (call) async {
      calls.add(call);
      if (call.method == 'saveDownloadTask') {
        return 'content://downloads/movie.mp4';
      }
      return null;
    });
    final platform = MethodChannelVesperPlayerAndroid();

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

    expect(savedUri, 'content://downloads/movie.mp4');
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
    final platform = MethodChannelVesperPlayerAndroid();
    const viewport = VesperPlayerViewport(
      left: 24,
      top: 48,
      width: 180,
      height: 120,
    );

    await platform.updateViewport('android-player', viewport);

    expect(calls, hasLength(1));
    expect(calls.single.method, 'updateViewport');
    expect(
      Map<Object?, Object?>.from(calls.single.arguments as Map),
      <Object?, Object?>{
        'playerId': 'android-player',
        'viewport': viewport.toMap(),
        'viewportHint': const VesperViewportHint(
          kind: VesperViewportHintKind.visible,
          visibleFraction: 1,
        ).toMap(),
      },
    );
  });

  test('system playback calls forward payloads', () async {
    final platform = MethodChannelVesperPlayerAndroid();
    const metadata = VesperSystemPlaybackMetadata(
      title: 'Episode',
      artist: 'Vesper',
      contentUri: 'https://example.com/video.m3u8',
      durationMs: 120000,
    );
    const configuration = VesperSystemPlaybackConfiguration(
      metadata: metadata,
    );

    await platform.configureSystemPlayback('android-player', configuration);
    await platform.updateSystemPlaybackMetadata('android-player', metadata);
    await platform.clearSystemPlayback('android-player');

    expect(calls.map((call) => call.method), <String>[
      'configureSystemPlayback',
      'updateSystemPlaybackMetadata',
      'clearSystemPlayback',
    ]);
    expect(
      Map<Object?, Object?>.from(calls[0].arguments as Map),
      <Object?, Object?>{
        'playerId': 'android-player',
        'configuration': configuration.toMap(),
      },
    );
    expect(
      Map<Object?, Object?>.from(calls[1].arguments as Map),
      <Object?, Object?>{
        'playerId': 'android-player',
        'metadata': metadata.toMap(),
      },
    );
    expect(
      Map<Object?, Object?>.from(calls[2].arguments as Map),
      <Object?, Object?>{'playerId': 'android-player'},
    );
  });

  test('picture-in-picture calls forward payloads', () async {
    TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger
        .setMockMethodCallHandler(channel, (call) async {
      calls.add(call);
      if (call.method == 'isPictureInPictureAvailable') {
        return <String, Object?>{
          'isAvailable': true,
          'isActive': false,
          'canAutoEnter': false,
          'source': 'system',
        };
      }
      return null;
    });
    final platform = MethodChannelVesperPlayerAndroid();
    const configuration = VesperPictureInPictureConfiguration(
      autoEnter: true,
      preferredAspectRatio: 16 / 9,
    );

    final availability =
        await platform.isPictureInPictureAvailable('android-player');
    await platform.setPictureInPictureConfiguration(
      'android-player',
      configuration,
    );
    await platform.requestPictureInPicture(
      'android-player',
      configuration: configuration,
    );
    await platform.requestPictureInPicture('android-player');
    await platform.exitPictureInPicture('android-player');

    expect(availability.isAvailable, isTrue);
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
        'playerId': 'android-player',
        'configuration': configuration.toMap(),
      },
    );
    expect(
      Map<Object?, Object?>.from(calls[2].arguments as Map),
      <Object?, Object?>{
        'playerId': 'android-player',
        'configuration': configuration.toMap(),
      },
    );
    expect(
      Map<Object?, Object?>.from(calls[3].arguments as Map),
      <Object?, Object?>{'playerId': 'android-player'},
    );
    expect(
      Map<Object?, Object?>.from(calls[4].arguments as Map),
      <Object?, Object?>{'playerId': 'android-player'},
    );
  });

  test('requestSystemPlaybackPermissions decodes platform status', () async {
    TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger
        .setMockMethodCallHandler(channel, (call) async {
      calls.add(call);
      return 'granted';
    });
    final platform = MethodChannelVesperPlayerAndroid();

    final status = await platform.requestSystemPlaybackPermissions();

    expect(status, VesperSystemPlaybackPermissionStatus.granted);
    expect(calls.single.method, 'requestSystemPlaybackPermissions');
  });

  test('getSystemPlaybackPermissionStatus decodes platform status', () async {
    TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger
        .setMockMethodCallHandler(channel, (call) async {
      calls.add(call);
      return 'denied';
    });
    final platform = MethodChannelVesperPlayerAndroid();

    final status = await platform.getSystemPlaybackPermissionStatus();

    expect(status, VesperSystemPlaybackPermissionStatus.denied);
    expect(calls.single.method, 'getSystemPlaybackPermissionStatus');
  });
}
