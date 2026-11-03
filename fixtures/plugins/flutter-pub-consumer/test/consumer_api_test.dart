import 'package:flutter_test/flutter_test.dart';
import 'package:vesper_player/vesper_player.dart';

void main() {
  test('published package exposes the controller and source API', () {
    final source = VesperPlayerSource.hls(
      uri: 'https://devstreaming-cdn.apple.com/videos/streaming/examples/img_bipbop_adv_example_ts/master.m3u8',
      label: 'Hosted pub consumer',
    );

    expect(source.protocol, VesperPlayerSourceProtocol.hls);
    expect(source.label, 'Hosted pub consumer');
    expect(VesperPlayerController.create, isA<Function>());
  });
}
