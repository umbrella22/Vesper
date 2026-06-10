import 'package:flutter/services.dart';
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:flutter_host/src/example_player_models.dart';
import 'package:flutter_host/src/example_player_sections.dart';
import 'package:flutter_host/src/hdr_evidence_capture.dart';
import 'package:vesper_player/vesper_player.dart';

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
    expect(find.text('HDR10 4K60 PQ'), findsNothing);

    for (var i = 0; i < 10; i += 1) {
      await tester.pump(const Duration(milliseconds: 20));
      if (find.text('控制器初始化失败').evaluate().isNotEmpty) {
        break;
      }
    }

    expect(find.text('控制器初始化失败'), findsOneWidget);
  });

  testWidgets('plugin diagnostics section exposes HDR P0 presets', (
    WidgetTester tester,
  ) async {
    await tester.pumpWidget(
      MaterialApp(
        home: Scaffold(
          body: SingleChildScrollView(
            child: ExamplePluginDiagnosticsSection(
              palette: exampleHostPalette(false),
              sourceNormalizerSetting: ExampleSourceNormalizerSetting.disabled,
              sourceNormalizerPluginLibraryPaths: const <String>[],
              frameProcessorPluginLibraryPaths: const <String>[],
              pluginDiagnostics: const <VesperPluginDiagnostic>[],
              isCapturingHdrEvidence: false,
              hdrEvidenceActiveSourceAvailable: true,
              hdrEvidencePresets: exampleHdrEvidenceP0Presets,
              selectedHdrEvidencePreset: exampleHdrEvidenceP0Presets[1],
              onSourceNormalizerSettingChange: (_) {},
              onHdrEvidencePresetChange: (_) {},
              onCaptureHdrEvidence: () {},
            ),
          ),
        ),
      ),
    );

    expect(find.text('HDR10 4K60 PQ'), findsOneWidget);
    expect(find.text('HEVC SDR control'), findsOneWidget);
    expect(find.text('Network failure control'), findsOneWidget);
    expect(find.text('采集 HDR evidence'), findsOneWidget);
  });

  testWidgets('HDR evidence capture button waits for an active source', (
    WidgetTester tester,
  ) async {
    await tester.pumpWidget(
      MaterialApp(
        home: Scaffold(
          body: SingleChildScrollView(
            child: ExamplePluginDiagnosticsSection(
              palette: exampleHostPalette(false),
              sourceNormalizerSetting: ExampleSourceNormalizerSetting.disabled,
              sourceNormalizerPluginLibraryPaths: const <String>[],
              frameProcessorPluginLibraryPaths: const <String>[],
              pluginDiagnostics: const <VesperPluginDiagnostic>[],
              isCapturingHdrEvidence: false,
              hdrEvidenceActiveSourceAvailable: false,
              hdrEvidencePresets: exampleHdrEvidenceP0Presets,
              selectedHdrEvidencePreset: exampleHdrEvidenceP0Presets.first,
              onSourceNormalizerSettingChange: (_) {},
              onHdrEvidencePresetChange: (_) {},
              onCaptureHdrEvidence: () {},
            ),
          ),
        ),
      ),
    );

    final button = tester.widget<OutlinedButton>(
      find.widgetWithText(OutlinedButton, '采集 HDR evidence'),
    );
    expect(button.onPressed, isNull);
    expect(
      find.text(
        'Select a local file or remote URL before capturing this preset.',
      ),
      findsOneWidget,
    );
  });
}
