import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:vesper_player/vesper_player.dart';
import 'package:vesper_player_cast/vesper_player_cast.dart';

void main() {
  TestWidgetsFlutterBinding.ensureInitialized();

  const channel = MethodChannel('io.github.ikaros.vesper_player_cast_test');
  final calls = <MethodCall>[];

  tearDown(() {
    TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger
        .setMockMethodCallHandler(channel, null);
    calls.clear();
  });

  test('loadRemoteSource serializes source and metadata', () async {
    TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger
        .setMockMethodCallHandler(channel, (call) async {
          calls.add(call);
          return <String, Object?>{'status': 'success'};
        });
    final controller = VesperCastController(methodChannel: channel);
    final source = VesperPlayerSource.hls(
      uri: 'https://example.com/video.m3u8',
      label: 'HLS',
    );
    const metadata = VesperSystemPlaybackMetadata(title: 'Episode');

    final result = await controller.loadRemoteSource(
      source: source,
      metadata: metadata,
      startPositionMs: 12000,
    );

    expect(result.status, VesperCastOperationStatus.success);
    expect(calls.single.method, 'loadRemoteMedia');
    expect(
      Map<Object?, Object?>.from(calls.single.arguments as Map),
      <Object?, Object?>{
        'source': source.toMap(),
        'metadata': metadata.toMap(),
        'startPositionMs': 12000,
        'autoplay': true,
      },
    );
  });

  test('operation result decodes unsupported status', () async {
    TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger
        .setMockMethodCallHandler(channel, (call) async {
          calls.add(call);
          return <String, Object?>{
            'status': 'unsupported',
            'message': 'local source',
          };
        });
    final controller = VesperCastController(methodChannel: channel);

    final result = await controller.stop();

    expect(result.status, VesperCastOperationStatus.unsupported);
    expect(result.message, 'local source');
    expect(calls.single.method, 'stop');
  });
}
