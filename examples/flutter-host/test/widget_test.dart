import 'package:flutter/services.dart';
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:flutter_host/src/player/example_dolby_acceptance_catalog.dart';
import 'package:flutter_host/src/player/example_player_models.dart';
import 'package:flutter_host/src/player/example_player_sections.dart';
import 'package:flutter_host/src/hdr_evidence/hdr_evidence_capture.dart';
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
        .setMockMethodCallHandler(channel, (_) async => null);
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
              sourceNormalizerPluginReferences: const <VesperPluginReference>[],
              frameProcessorPluginReferences: const <VesperPluginReference>[],
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
              sourceNormalizerPluginReferences: const <VesperPluginReference>[],
              frameProcessorPluginReferences: const <VesperPluginReference>[],
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

  testWidgets('Dolby acceptance section exposes clear and Widevine presets', (
    WidgetTester tester,
  ) async {
    ExampleDolbyAcceptancePreset? selected;

    await tester.pumpWidget(
      MaterialApp(
        home: Scaffold(
          body: SingleChildScrollView(
            child: ExampleDolbyCatalogPanel(
              palette: exampleHostPalette(false),
              presets: exampleDolbyAcceptanceCatalog,
              selectedDrmKind: ExampleDolbyAcceptanceDrmKind.clear,
              selectedProfile: ExampleDolbyAcceptanceProfile.p5,
              selectedFps: 24,
              onDrmKindChanged: (_) {},
              onProfileChanged: (_) {},
              onFpsChanged: (_) {},
              onPresetPlayNow: (preset) {
                selected = preset;
              },
              onPresetAddToQueue: (_) {},
            ),
          ),
        ),
      ),
    );

    expect(find.text('Dolby 验收'), findsOneWidget);
    expect(find.text('P5 24fps DASH Clear'), findsOneWidget);
    expect(find.text('P5 24fps HLS Clear'), findsOneWidget);

    await tester.tap(find.widgetWithText(FilledButton, '立即播放').first);
    expect(selected?.id, 'DOLBY-DV-P5-24-DASH-CLEAR');

    await tester.pumpWidget(
      MaterialApp(
        home: Scaffold(
          body: SingleChildScrollView(
            child: ExampleDolbyCatalogPanel(
              palette: exampleHostPalette(false),
              presets: exampleDolbyAcceptanceCatalog,
              selectedDrmKind: ExampleDolbyAcceptanceDrmKind.widevine,
              selectedProfile: ExampleDolbyAcceptanceProfile.p81,
              selectedFps: 50,
              onDrmKindChanged: (_) {},
              onProfileChanged: (_) {},
              onFpsChanged: (_) {},
              onPresetPlayNow: (preset) {
                selected = preset;
              },
              onPresetAddToQueue: (_) {},
            ),
          ),
        ),
      ),
    );

    expect(find.text('P8.1 50fps DASH Widevine'), findsOneWidget);
    expect(
      find.textContaining('Widevine DASH direct native route'),
      findsOneWidget,
    );
    expect(find.widgetWithText(OutlinedButton, '创建下载任务'), findsNothing);
    expect(find.widgetWithText(OutlinedButton, '外部投放'), findsNothing);
  });

  testWidgets(
    'Dolby acceptance section can disable host-incompatible presets',
    (WidgetTester tester) async {
      ExampleDolbyAcceptancePreset? selected;

      await tester.pumpWidget(
        MaterialApp(
          home: Scaffold(
            body: SingleChildScrollView(
              child: ExampleDolbyCatalogPanel(
                palette: exampleHostPalette(false),
                presets: exampleDolbyAcceptanceCatalog,
                selectedDrmKind: ExampleDolbyAcceptanceDrmKind.clear,
                selectedProfile: ExampleDolbyAcceptanceProfile.p5,
                selectedFps: 24,
                isPresetPlayable: (preset) =>
                    preset.protocol != VesperPlayerSourceProtocol.dash,
                disabledReasonForPreset: (_) =>
                    'iOS Dolby acceptance uses HLS direct playback.',
                onDrmKindChanged: (_) {},
                onProfileChanged: (_) {},
                onFpsChanged: (_) {},
                onPresetPlayNow: (preset) {
                  selected = preset;
                },
                onPresetAddToQueue: (_) {},
              ),
            ),
          ),
        ),
      );

      expect(
        find.text('iOS Dolby acceptance uses HLS direct playback.'),
        findsOneWidget,
      );
      final dashButton = tester.widget<FilledButton>(
        find.widgetWithText(FilledButton, '立即播放').first,
      );
      expect(dashButton.onPressed, isNull);

      await tester.tap(find.widgetWithText(FilledButton, '立即播放').first);
      expect(selected, isNull);
    },
  );

  testWidgets('Dolby FairPlay pending presets are disabled', (
    WidgetTester tester,
  ) async {
    var selected = false;

    await tester.pumpWidget(
      MaterialApp(
        home: Scaffold(
          body: SingleChildScrollView(
            child: ExampleDolbyCatalogPanel(
              palette: exampleHostPalette(false),
              presets: exampleDolbyAcceptanceCatalog,
              selectedDrmKind: ExampleDolbyAcceptanceDrmKind.fairPlayPending,
              selectedProfile: ExampleDolbyAcceptanceProfile.p5,
              selectedFps: 24,
              onDrmKindChanged: (_) {},
              onProfileChanged: (_) {},
              onFpsChanged: (_) {},
              onPresetPlayNow: (_) {
                selected = true;
              },
              onPresetAddToQueue: (_) {},
            ),
          ),
        ),
      ),
    );

    expect(find.text('P5 24fps HLS FairPlay pending'), findsOneWidget);
    expect(find.textContaining('certificate URI or base64'), findsOneWidget);

    final button = tester.widget<FilledButton>(
      find.widgetWithText(FilledButton, '立即播放').first,
    );
    expect(button.onPressed, isNull);

    await tester.tap(find.widgetWithText(FilledButton, '立即播放').first);
    expect(selected, isFalse);
  });

  testWidgets(
    'recent error section explains Dolby P5 device capability failure',
    (WidgetTester tester) async {
      await tester.pumpWidget(
        MaterialApp(
          home: Scaffold(
            body: ExampleRecentErrorSection(
              palette: exampleHostPalette(false),
              error: const VesperPlayerError(
                message: 'Decoder init failed.',
                code: VesperPlayerErrorCode.decodeFailure,
                category: VesperPlayerErrorCategory.decode,
                retriable: false,
                details: <String, Object?>{
                  'dolbyVisionProfile': 5,
                  'codec': 'dvhe.05.06',
                  'decoderName': 'pending',
                },
              ),
            ),
          ),
        ),
      );

      expect(
        find.text('当前设备不支持这个 Dolby Vision P5 / Widevine 播放组合。'),
        findsOneWidget,
      );
      expect(find.text('codec：dvhe.05.06'), findsOneWidget);
    },
  );

  testWidgets(
    'recent error section explains exhausted Widevine retry failure',
    (WidgetTester tester) async {
      await tester.pumpWidget(
        MaterialApp(
          home: Scaffold(
            body: ExampleRecentErrorSection(
              palette: exampleHostPalette(false),
              error: const VesperPlayerError(
                message: 'Widevine license failed.',
                code: VesperPlayerErrorCode.backendFailure,
                category: VesperPlayerErrorCategory.network,
                retriable: true,
                details: <String, Object?>{
                  'keySystem': 'widevine',
                  'licenseUriHost': 'license.example.com',
                  'attemptsExhausted': true,
                  'maxAttempts': 3,
                  'errorCodeName': 'ERROR_CODE_DRM_LICENSE_ACQUISITION_FAILED',
                },
              ),
            ),
          ),
        ),
      );

      expect(
        find.text('Widevine license 或 provisioning 请求失败，已重试 3 次。'),
        findsOneWidget,
      );
      expect(find.text('license host：license.example.com'), findsOneWidget);
      expect(
        find.text('错误码：ERROR_CODE_DRM_LICENSE_ACQUISITION_FAILED'),
        findsOneWidget,
      );
    },
  );

  testWidgets(
    'recent error section explains exhausted FairPlay retry failure',
    (WidgetTester tester) async {
      await tester.pumpWidget(
        MaterialApp(
          home: Scaffold(
            body: ExampleRecentErrorSection(
              palette: exampleHostPalette(false),
              error: const VesperPlayerError(
                message: 'FairPlay license failed.',
                code: VesperPlayerErrorCode.backendFailure,
                category: VesperPlayerErrorCategory.network,
                retriable: true,
                details: <String, Object?>{
                  'keySystem': 'fairPlay',
                  'licenseUriHost': 'license.example.com',
                  'certificateUriHost': 'cert.example.com',
                  'attemptsExhausted': true,
                  'maxAttempts': 3,
                  'httpStatusCode': '503',
                },
              ),
            ),
          ),
        ),
      );

      expect(
        find.text('FairPlay license 或 certificate 请求失败，已重试 3 次。'),
        findsOneWidget,
      );
      expect(find.text('license host：license.example.com'), findsOneWidget);
      expect(find.text('certificate host：cert.example.com'), findsOneWidget);
      expect(find.text('HTTP status：503'), findsOneWidget);
    },
  );
}
