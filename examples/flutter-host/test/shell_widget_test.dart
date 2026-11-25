import 'dart:async';

import 'package:flutter/services.dart';
import 'package:material_ui/material_ui.dart';
import 'package:flutter_host/src/shell/example_host_navigation.dart';
import 'package:flutter_host/src/shell/example_host_shell.dart';
import 'package:flutter_host/src/shell/example_host_strings.dart';
import 'package:flutter_host/src/shell/example_open_media_sheet.dart';
import 'package:flutter_host/src/theme/example_player_theme.dart';
import 'package:flutter_test/flutter_test.dart';

/// Minimal host around [ExampleHostShell] that mirrors the page's wiring.
class _Harness extends StatefulWidget {
  const _Harness({this.onOpenUrl, this.strings = ExampleHostStrings.en});

  final Future<String?> Function(String url)? onOpenUrl;
  final ExampleHostStrings strings;

  @override
  State<_Harness> createState() => _HarnessState();
}

class _HarnessState extends State<_Harness> {
  ExampleHostNavigationState nav = const ExampleHostNavigationState();
  bool? lastOpened;

  void update(ExampleHostNavigationState next) => setState(() => nav = next);

  @override
  Widget build(BuildContext context) {
    return MaterialApp(
      theme: buildExampleHostTheme(useDarkTheme: false),
      builder: (context, child) =>
          ExampleHostStringsScope(strings: widget.strings, child: child!),
      home: Builder(
        builder: (context) => ExampleHostShell(
          navigation: nav,
          isFullscreen: nav.isFullscreen(autoFullscreen: false),
          title: 'Vesper',
          body: Center(child: Text('body-${nav.tab.name}')),
          fabExtended: true,
          onSelectTab: (tab) => setState(() => nav = nav.selectTab(tab)),
          onSystemBack: (action) => setState(() => nav = nav.applyBack(action)),
          onOpenSettings: () {},
          onOpenMedia: () async {
            final opened = await showExampleOpenMediaSheet(
              context,
              sheet: ExampleOpenMediaSheet(
                initialUrl: '',
                localFilesEnabled: true,
                dashEnabled: false,
                onPickLocal: () async => '',
                onOpenUrl: widget.onOpenUrl ?? (_) async => null,
                onOpenSample: (_) async => null,
              ),
            );
            setState(() {
              lastOpened = opened;
              if (opened) {
                nav = nav.showPlayer();
              }
            });
          },
        ),
      ),
    );
  }
}

Future<void> _systemBack(WidgetTester tester) async {
  await tester.binding.handlePopRoute();
  await tester.pumpAndSettle();
}

void main() {
  testWidgets('renders four tabs in a bottom bar on phones', (tester) async {
    tester.view.physicalSize = const Size(390, 844);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.reset);
    await tester.pumpWidget(const _Harness(strings: ExampleHostStrings.zh));

    expect(
      find.byKey(const ValueKey<String>('shell-navigation-bar')),
      findsOneWidget,
    );
    for (final label in <String>['播放', '媒体库', '下载', '实验室']) {
      expect(find.text(label), findsOneWidget);
    }
    await tester.tap(find.text('实验室'));
    await tester.pumpAndSettle();
    expect(find.text('body-lab'), findsOneWidget);
    // The open-media FAB is only on the play tab.
    expect(
      find.byKey(const ValueKey<String>('shell-open-media-fab')),
      findsNothing,
    );
  });

  testWidgets('uses a navigation rail at 600dp and wider', (tester) async {
    tester.view.physicalSize = const Size(900, 700);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.reset);
    await tester.pumpWidget(const _Harness());
    expect(
      find.byKey(const ValueKey<String>('shell-navigation-rail')),
      findsOneWidget,
    );
    expect(
      find.byKey(const ValueKey<String>('shell-navigation-bar')),
      findsNothing,
    );
  });

  testWidgets('open media: invalid URL keeps the sheet open', (tester) async {
    await tester.pumpWidget(const _Harness());
    await tester.tap(
      find.byKey(const ValueKey<String>('shell-open-media-fab')),
    );
    await tester.pumpAndSettle();
    await tester.enterText(
      find.byKey(const ValueKey<String>('open-media-url-field')),
      'not a url',
    );
    await tester.tap(
      find.byKey(const ValueKey<String>('open-media-url-submit')),
    );
    await tester.pumpAndSettle();
    expect(
      find.text('The URL must start with http:// or https://.'),
      findsOneWidget,
    );
    expect(find.byType(ExampleOpenMediaSheet), findsOneWidget);
  });

  testWidgets('open media: host error keeps the sheet open', (tester) async {
    await tester.pumpWidget(
      _Harness(onOpenUrl: (_) async => 'Playback failed'),
    );
    await tester.tap(
      find.byKey(const ValueKey<String>('shell-open-media-fab')),
    );
    await tester.pumpAndSettle();
    await tester.enterText(
      find.byKey(const ValueKey<String>('open-media-url-field')),
      'https://example.com/a.m3u8',
    );
    await tester.tap(
      find.byKey(const ValueKey<String>('open-media-url-submit')),
    );
    await tester.pumpAndSettle();
    expect(find.text('Playback failed'), findsOneWidget);
    expect(find.byType(ExampleOpenMediaSheet), findsOneWidget);
  });

  testWidgets('open media: a native failure preserves the URL for retry', (
    tester,
  ) async {
    const url = 'https://example.com/retry.m3u8';
    final attempts = <String>[];
    await tester.pumpWidget(
      _Harness(
        strings: ExampleHostStrings.zh,
        onOpenUrl: (value) async {
          attempts.add(value);
          if (attempts.length == 1) throw PlatformException(code: 'openFailed');
          return null;
        },
      ),
    );
    await tester.tap(
      find.byKey(const ValueKey<String>('shell-open-media-fab')),
    );
    await tester.pumpAndSettle();
    final field = find.byKey(const ValueKey<String>('open-media-url-field'));
    final submit = find.byKey(const ValueKey<String>('open-media-url-submit'));
    await tester.enterText(field, url);
    await tester.tap(submit);
    await tester.pumpAndSettle();
    expect(
      find.text(ExampleHostStrings.zh[ExampleHostText.openMediaFailed]),
      findsOneWidget,
    );
    expect(tester.widget<TextField>(field).enabled, isTrue);
    expect(tester.widget<TextField>(field).controller!.text, url);
    await tester.tap(submit);
    await tester.pumpAndSettle();
    expect(attempts, [url, url]);
    expect(find.byType(ExampleOpenMediaSheet), findsNothing);
  });

  testWidgets('open media: a late success cannot pop the page underneath', (
    tester,
  ) async {
    final pending = Completer<String?>();
    await tester.pumpWidget(_Harness(onOpenUrl: (_) => pending.future));
    final state = tester.state<_HarnessState>(find.byType(_Harness));
    await tester.tap(
      find.byKey(const ValueKey<String>('shell-open-media-fab')),
    );
    await tester.pumpAndSettle();
    await tester.enterText(
      find.byKey(const ValueKey<String>('open-media-url-field')),
      'https://example.com/video.mp4',
    );
    await tester.tap(
      find.byKey(const ValueKey<String>('open-media-url-submit')),
    );
    await tester.pump();
    await tester.binding.handlePopRoute();
    await tester.pump();
    pending.complete(null);
    await tester.pumpAndSettle();
    expect(find.byType(ExampleOpenMediaSheet), findsNothing);
    expect(find.text('body-play'), findsOneWidget);
    expect(state.lastOpened, isFalse);
  });

  testWidgets('open media: success closes and switches to play', (
    tester,
  ) async {
    await tester.pumpWidget(const _Harness());
    final state = tester.state<_HarnessState>(find.byType(_Harness));
    state.update(state.nav.selectTab(ExampleHostTab.play));
    await tester.tap(
      find.byKey(const ValueKey<String>('shell-open-media-fab')),
    );
    await tester.pumpAndSettle();
    await tester.enterText(
      find.byKey(const ValueKey<String>('open-media-url-field')),
      'https://example.com/a.m3u8',
    );
    await tester.tap(
      find.byKey(const ValueKey<String>('open-media-url-submit')),
    );
    await tester.pumpAndSettle();
    expect(find.byType(ExampleOpenMediaSheet), findsNothing);
    expect(state.lastOpened, isTrue);
    expect(state.nav.tab, ExampleHostTab.play);
  });

  testWidgets('back closes sheet, fullscreen, collection, then tab', (
    tester,
  ) async {
    await tester.pumpWidget(const _Harness());
    final state = tester.state<_HarnessState>(find.byType(_Harness));

    // 1. Sheet first.
    await tester.tap(
      find.byKey(const ValueKey<String>('shell-open-media-fab')),
    );
    await tester.pumpAndSettle();
    state.update(state.nav.enterFullscreen());
    await tester.pump();
    await _systemBack(tester);
    expect(find.byType(ExampleOpenMediaSheet), findsNothing);
    expect(state.nav.manualFullscreen, isTrue);

    // 2. Fullscreen.
    await _systemBack(tester);
    expect(state.nav.manualFullscreen, isFalse);

    // 3. Collection detail, then 4. back to the play tab.
    state.update(state.nav.showCollection(ExampleLibraryCollectionId.dolby));
    await tester.pumpAndSettle();
    await _systemBack(tester);
    expect(state.nav.tab, ExampleHostTab.library);
    expect(state.nav.openCollection, isNull);
    await _systemBack(tester);
    expect(state.nav.tab, ExampleHostTab.play);
    expect(
      state.nav.backAction(autoFullscreen: false),
      ExampleBackAction.exitApp,
    );
  });
}
