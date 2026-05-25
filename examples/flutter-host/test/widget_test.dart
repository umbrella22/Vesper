import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';

import 'package:flutter_host/main.dart';

void main() {
  testWidgets('shows loading then unsupported error in widget test env', (
    WidgetTester tester,
  ) async {
    const channel = MethodChannel(
      'io.github.ikaros.vesper.example.flutter_host/media_picker',
    );
    TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger
        .setMockMethodCallHandler(channel, (call) async {
          return switch (call.method) {
            'bundledDownloadPluginLibraryPaths' => const <String>[],
            'bundledSourceNormalizerPluginLibraryPaths' => const <String>[],
            'bundledFrameProcessorPluginLibraryPaths' => const <String>[],
            _ => null,
          };
        });
    addTearDown(() {
      TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger
          .setMockMethodCallHandler(channel, null);
    });

    await tester.pumpWidget(const VesperFlutterHostApp());

    expect(find.text('正在初始化 Vesper Flutter Host...'), findsOneWidget);

    for (var i = 0; i < 10; i += 1) {
      await tester.pump(const Duration(milliseconds: 20));
      if (find.text('控制器初始化失败').evaluate().isNotEmpty) {
        break;
      }
    }

    expect(find.text('控制器初始化失败'), findsOneWidget);
  });
}
