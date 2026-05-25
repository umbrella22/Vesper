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

    await platform.createPlayer(
      sourceNormalizerConfiguration: sourceNormalizerConfiguration,
      frameProcessorConfiguration: frameProcessorConfiguration,
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
