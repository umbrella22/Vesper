import 'package:flutter_test/flutter_test.dart';
import 'package:vesper_player_platform_interface/vesper_player_platform_interface.dart';

void main() {
  test('media display dimensions round trip independently of per-view geometry',
      () {
    const media =
        VesperVideoPresentation(displayWidth: 1080, displayHeight: 1920);
    final snapshot =
        const VesperPlayerSnapshot.initial().copyWith(videoPresentation: media);
    final decoded = VesperPlayerSnapshot.fromMap(snapshot.toMap());
    expect(decoded.videoPresentation!.displayAspectRatio, 9 / 16);
    expect(decoded.videoPresentation!.toMap().containsKey('surface'), isFalse);
    expect(decoded.copyWith(clearVideoPresentation: true).videoPresentation,
        isNull);
    expect(const VesperPlayerSnapshot.initial().videoPresentation, isNull);
  });

  test('content rectangle excludes renderer bars and maps logical coordinates',
      () {
    final geometry = VesperVideoSurfaceGeometry.fromMap(<Object?, Object?>{
      'width': 320,
      'height': 180,
      'contentRect': <String, double>{
        'left': 109.375,
        'top': 0,
        'width': 101.25,
        'height': 180
      },
    });
    expect(geometry.contentRect.contains(160, 90), isTrue);
    expect(geometry.contentRect.contains(20, 90), isFalse);
    expect(geometry.contentRect.contains(210.625, 90), isFalse);
    expect(VesperVideoSurfaceGeometry.fromMap(geometry.toMap()).toMap(),
        geometry.toMap());
  });

  test('missing, nonfinite, and zero native dimensions are rejected', () {
    for (final width in <Object?>[null, 0, -1, double.nan, double.infinity]) {
      expect(
          () => VesperVideoPresentation.fromMap(
              <Object?, Object?>{'displayWidth': width, 'displayHeight': 1920}),
          throwsFormatException);
    }
  });
}
