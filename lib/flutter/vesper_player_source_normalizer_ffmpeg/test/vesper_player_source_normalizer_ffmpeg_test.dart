import 'package:flutter_test/flutter_test.dart';
import 'package:vesper_player_source_normalizer_ffmpeg/vesper_player_source_normalizer_ffmpeg.dart';

void main() {
  test('marker export is available', () {
    expect(vesperPlayerSourceNormalizerFfmpegAvailable, isTrue);
  });
}
