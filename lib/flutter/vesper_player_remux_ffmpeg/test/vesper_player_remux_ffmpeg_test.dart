import 'package:flutter_test/flutter_test.dart';
import 'package:vesper_player_remux_ffmpeg/vesper_player_remux_ffmpeg.dart';

void main() {
  test('reports the optional native dependency marker', () {
    expect(vesperPlayerRemuxFfmpegAvailable, isTrue);
  });
}
