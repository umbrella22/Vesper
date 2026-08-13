import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:flutter_host/src/hdr_evidence/hdr_evidence_capture_output.dart';

void main() {
  TestWidgetsFlutterBinding.ensureInitialized();

  const channel = MethodChannel(
    'io.github.umbrella22.vesper.example.flutter_host/media_picker',
  );

  tearDown(() {
    TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger
        .setMockMethodCallHandler(channel, null);
  });

  test('uses native HDR evidence output root', () async {
    TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger
        .setMockMethodCallHandler(channel, (call) async {
          expect(call.method, 'hdrEvidenceOutputRoot');
          return '/tmp/vesper-hdr-evidence-root';
        });

    final directory = await ExampleHdrEvidenceCaptureOutput.defaultOutputRoot();

    expect(directory.path, '/tmp/vesper-hdr-evidence-root');
  });

  test('falls back to a temporary root when native helper is absent', () async {
    TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger
        .setMockMethodCallHandler(channel, (call) async {
          throw MissingPluginException('not registered');
        });

    final directory = await ExampleHdrEvidenceCaptureOutput.defaultOutputRoot();
    addTearDown(() async {
      if (directory.existsSync()) {
        await directory.delete(recursive: true);
      }
    });

    expect(directory.existsSync(), isTrue);
    expect(directory.path, contains('vesper-hdr-evidence-'));
  });

  test('uses native HDR evidence device sheet', () async {
    TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger
        .setMockMethodCallHandler(channel, (call) async {
          expect(call.method, 'hdrEvidenceDevice');
          return <Object?, Object?>{
            'android': <Object?, Object?>{
              'manufacturer': 'Google',
              'model': 'Pixel fixture',
              'apiLevel': 35,
              'displayHdrTypes': <Object?>['HDR10'],
              'decoderCandidates': <Object?, Object?>{
                'hevc': <Object?>['c2.fixture.hevc.decoder'],
              },
            },
          };
        });

    final device = await ExampleHdrEvidenceCaptureOutput.deviceEvidence();
    final android = device['android'] as Map<String, Object?>;
    final decoderCandidates =
        android['decoderCandidates'] as Map<String, Object?>;

    expect(android['manufacturer'], 'Google');
    expect(android['displayHdrTypes'], <String>['HDR10']);
    expect(decoderCandidates['hevc'], <String>['c2.fixture.hevc.decoder']);
  });

  test(
    'falls back to an empty device sheet when native helper is absent',
    () async {
      TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger
          .setMockMethodCallHandler(channel, (call) async {
            throw MissingPluginException('not registered');
          });

      final device = await ExampleHdrEvidenceCaptureOutput.deviceEvidence();

      expect(device, isEmpty);
    },
  );
}
