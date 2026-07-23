import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:flutter_host/src/device/example_subtitle_overlay_evidence.dart';

void main() {
  TestWidgetsFlutterBinding.ensureInitialized();

  const channel = MethodChannel(
    'io.github.ikaros.vesper.example.flutter_host/device_controls',
  );

  tearDown(() {
    TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger
        .setMockMethodCallHandler(channel, null);
  });

  test(
    'decodes the native subtitle overlay snapshot and PNG evidence',
    () async {
      final calls = <MethodCall>[];
      TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger
          .setMockMethodCallHandler(channel, (call) async {
            calls.add(call);
            final snapshot = <Object?, Object?>{
              'text': 'Subtitle B',
              'hidden': false,
              'alpha': 1.0,
              'windowAttached': true,
              'frame': <Object?, Object?>{
                'x': 24,
                'y': 120,
                'width': 272,
                'height': 28,
              },
              'visible': true,
            };
            if (call.method == 'subtitleOverlaySnapshot') {
              return snapshot;
            }
            return <Object?, Object?>{
              'snapshot': snapshot,
              'png': Uint8List.fromList(<int>[0x89, 0x50, 0x4e, 0x47]),
            };
          });

      final snapshot = await ExampleSubtitleOverlayEvidenceChannel.snapshot(
        'player-1',
      );
      final evidence = await ExampleSubtitleOverlayEvidenceChannel.capture(
        'player-1',
      );

      expect(snapshot.text, 'Subtitle B');
      expect(snapshot.visible, isTrue);
      expect(snapshot.frame.width, 272);
      expect(evidence.snapshot.toJson(), snapshot.toJson());
      expect(evidence.png, <int>[0x89, 0x50, 0x4e, 0x47]);
      expect(calls.map((call) => call.method), <String>[
        'subtitleOverlaySnapshot',
        'captureSubtitleOverlayEvidence',
      ]);
      expect(calls.first.arguments, <String, Object?>{'playerId': 'player-1'});
    },
  );
}
