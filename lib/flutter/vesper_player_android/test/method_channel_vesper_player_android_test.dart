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
