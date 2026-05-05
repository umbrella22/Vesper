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
    TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger
        .setMockMethodCallHandler(channel, null);
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
