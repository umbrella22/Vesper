import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:vesper_player_platform_interface/method_channel_platform_base.dart';
import 'package:vesper_player_platform_interface/vesper_player_platform_interface.dart';

void main() {
  TestWidgetsFlutterBinding.ensureInitialized();
  const channel = MethodChannel('source-contract-test');
  final platform = _SourceChannel(channel);
  final calls = <MethodCall>[];
  final reference = const VesperSourceReference(
      sessionId: 's', sourceId: 'h', expiresAtEpochMs: 9000);
  final messenger =
      TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger;

  setUp(() {
    calls.clear();
    messenger.setMockMethodCallHandler(channel, (call) async {
      calls.add(call);
      return switch (call.method) {
        'createSourceSession' => <String, Object?>{'sessionId': 's'},
        'registerSource' => reference.toMap(),
        'activateSource' => <String, Object?>{
            ...reference.toMap(),
            'activationId': 'a',
            'playbackEpoch': 12
          },
        'preloadSource' ||
        'sourcePreloadSnapshot' ||
        'awaitSourcePreload' =>
          <String, Object?>{
            ...reference.toMap(),
            'taskId': 't',
            'status': 'completed',
            'goal': 'dashSegmentBaseStartup',
            'reuse': 'playbackReusable',
            'actualBytes': 90,
            'cacheHit': false,
          },
        _ => null,
      };
    });
  });
  tearDown(() => messenger.setMockMethodCallHandler(channel, null));

  test(
      'source channels preserve identity, options and native activation correlation',
      () async {
    final id = await platform
        .createSourceSession(const VesperSourceSessionConfiguration());
    final handle = await platform.registerSource(
        id,
        VesperPlayerSource.localDash(
            uri: 'file:///fixture.mpd',
            headers: const {'Referer': 'https://app.test'}),
        expiresAtEpochMs: 9000);
    final task = await platform.preloadSource(
        handle, const VesperSourcePreloadOptions(maximumBytes: 4096));
    expect(task.actualBytes, 90);
    final activation = await platform.activateSource(
        'player',
        handle,
        const VesperSourceActivationOptions(
            playWhenReady: false,
            startPosition: Duration(seconds: 3),
            playbackRate: 1.5));
    expect(activation.playbackEpoch, 12);
    expect(activation.source, handle);
    expect((calls[2].arguments as Map)['options'],
        containsPair('maximumBytes', 4096));
    final payload = calls.last.arguments as Map;
    expect(payload['playerId'], 'player');
    expect(payload['sessionId'], 's');
    expect(payload['sourceId'], 'h');
    expect(payload.containsKey('uri'), isFalse);
    expect(payload['options'], containsPair('startPositionMs', 3000));
    expect(payload['options'], containsPair('playWhenReady', false));
    expect(payload['options'], containsPair('playbackRate', 1.5));
    await platform.awaitSourcePreload(id, task.taskId);
    await platform.sourcePreloadSnapshot(id, task.taskId);
    await platform.cancelSourcePreload(id, task.taskId);
    await platform.releaseSource(handle);
    await platform.invalidateSourceSession(id);
    await platform.disposeSourceSession(id);
  });

  test('activation refuses to manufacture missing native playback evidence',
      () async {
    messenger.setMockMethodCallHandler(
        channel,
        (_) async => <String, Object?>{
              ...reference.toMap(),
              'activationId': 'a',
            });
    await expectLater(
        platform.activateSource(
            'player', reference, const VesperSourceActivationOptions()),
        throwsFormatException);
  });
}

final class _SourceChannel extends VesperMethodChannelPlatformBase {
  _SourceChannel(MethodChannel channel)
      : super(
            methodChannel: channel,
            eventChannel: const EventChannel('source-test-events'),
            downloadEventChannel: const EventChannel('source-test-downloads'),
            sequenceEventChannel: const EventChannel('source-test-sequences'));
}
