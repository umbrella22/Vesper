// Opt-in screenshot fallback for when the native app cannot be built:
//   flutter test test/shell_screenshots_test.dart --dart-define=VESPER_SHOTS=true
// Writes PNGs to /tmp/vesper-flutter-impl/shots/. Controller-backed widgets
// (Stage, live diagnostics) are replaced by static stand-ins.
import 'dart:io';
import 'dart:ui' as ui;

import 'package:material_ui/material_ui.dart';
import 'package:flutter/rendering.dart';
import 'package:flutter/services.dart';
import 'package:flutter_host/src/download/example_download_sections.dart';
import 'package:flutter_host/src/player/example_dolby_acceptance_catalog.dart';
import 'package:flutter_host/src/player/example_player_models.dart';
import 'package:flutter_host/src/player/example_player_sections.dart';
import 'package:flutter_host/src/shell/example_host_navigation.dart';
import 'package:flutter_host/src/shell/example_host_shell.dart';
import 'package:flutter_host/src/shell/example_host_strings.dart';
import 'package:flutter_host/src/shell/example_library_catalog.dart';
import 'package:flutter_host/src/shell/example_library_pages.dart';
import 'package:flutter_host/src/shell/example_open_media_sheet.dart';
import 'package:flutter_host/src/shell/example_play_widgets.dart';
import 'package:flutter_host/src/shell/example_settings_sheet.dart';
import 'package:flutter_host/src/theme/example_player_theme.dart';
import 'package:flutter_test/flutter_test.dart';

const bool _enabled = bool.fromEnvironment('VESPER_SHOTS');
const String _outDir = '/tmp/vesper-flutter-impl/shots';
final GlobalKey _boundary = GlobalKey();

Future<void> _loadFonts() async {
  final root = File(
    Platform.resolvedExecutable,
  ).parent.parent.parent.parent.parent.parent.path;
  final fonts = '$root/bin/cache/artifacts/material_fonts';
  Future<void> load(String family, List<String> files) async {
    final loader = FontLoader(family);
    for (final file in files) {
      final bytes = File('$fonts/$file').readAsBytesSync();
      loader.addFont(Future.value(ByteData.view(bytes.buffer)));
    }
    await loader.load();
  }

  await load('Roboto', <String>[
    'Roboto-Regular.ttf',
    'Roboto-Medium.ttf',
    'Roboto-Bold.ttf',
  ]);
  await load('MaterialIcons', <String>['MaterialIcons-Regular.otf']);
}

Widget _app(bool dark, Widget home) {
  return RepaintBoundary(
    key: _boundary,
    child: MaterialApp(
      debugShowCheckedModeBanner: false,
      theme: buildExampleHostTheme(useDarkTheme: dark),
      darkTheme: buildExampleHostTheme(useDarkTheme: dark),
      builder: (context, child) => ExampleHostStringsScope(
        strings: ExampleHostStrings.en,
        child: child!,
      ),
      home: home,
    ),
  );
}

Widget _shell(ExampleHostNavigationState nav, String title, Widget body) {
  return ExampleHostShell(
    navigation: nav,
    isFullscreen: false,
    title: title,
    body: body,
    fabExtended: true,
    onSelectTab: (_) {},
    onSystemBack: (_) {},
    onOpenSettings: () {},
    onOpenMedia: () {},
    onTitleBack: nav.isCollectionVisible ? () {} : null,
  );
}

Widget _playBody(ExampleHostPalette palette) {
  return ListView(
    padding: const EdgeInsets.fromLTRB(18, 8, 18, 104),
    children: <Widget>[
      Container(
        height: 220,
        decoration: BoxDecoration(
          color: palette.stage,
          borderRadius: BorderRadius.circular(16),
        ),
        alignment: Alignment.center,
        child: Icon(
          Icons.play_arrow_rounded,
          size: 56,
          color: palette.stageForeground,
        ),
      ),
      const SizedBox(height: 16),
      const ExampleNowPlayingCard(
        title: 'HLS demo',
        subtitle: 'Apple BipBop, adaptive bitrate',
      ),
      const SizedBox(height: 12),
      ExampleQuickActionsRow(
        actions: <ExampleQuickAction>[
          ExampleQuickAction(
            id: 'pip',
            icon: Icons.picture_in_picture_alt_rounded,
            label: 'Picture in Picture',
            onTap: () {},
          ),
          ExampleQuickAction(
            id: 'cast',
            icon: Icons.cast_rounded,
            label: 'Cast',
            onTap: () {},
          ),
          ExampleQuickAction(
            id: 'tracks',
            icon: Icons.subtitles_outlined,
            label: 'Audio & subtitles',
            onTap: () {},
          ),
        ],
      ),
      const SizedBox(height: 20),
      ExampleQueuePanel(
        palette: palette,
        playlistItems: const <ExamplePlaylistItemViewData>[
          ExamplePlaylistItemViewData(
            itemId: 'hls',
            label: 'HLS demo',
            status: 'Playing',
            isActive: true,
          ),
          ExamplePlaylistItemViewData(
            itemId: 'dash',
            label: 'DASH demo',
            status: 'Up next',
            isActive: false,
          ),
        ],
        onSelectItem: (_) {},
        onManageQueue: () {},
      ),
    ],
  );
}

Widget _labBody(ExampleHostPalette palette) {
  return ListView(
    padding: const EdgeInsets.all(18),
    children: <Widget>[
      ExampleDiagnosticsSummarySection(
        palette: palette,
        sourceLabel: 'HLS demo',
        sourceProtocol: 'hls',
        routeLabel: 'direct native',
        playbackOrigin: const ExampleQueuePlaybackOrigin('hls'),
        sourceNormalizerSetting: ExampleSourceNormalizerSetting.preflightOnly,
      ),
      const SizedBox(height: 18),
      ExampleEventLogSection(
        palette: palette,
        entries: const <ExampleHostLogEntry>[],
      ),
    ],
  );
}

Future<void> _shot(WidgetTester tester, String name) async {
  await tester.pumpAndSettle();
  await tester.runAsync(() async {
    final boundary =
        _boundary.currentContext!.findRenderObject()! as RenderRepaintBoundary;
    final image = await boundary.toImage(pixelRatio: 2);
    final bytes = await image.toByteData(format: ui.ImageByteFormat.png);
    File('$_outDir/$name.png').writeAsBytesSync(bytes!.buffer.asUint8List());
  });
}

void main() {
  setUpAll(() async {
    Directory(_outDir).createSync(recursive: true);
    await _loadFonts();
  });

  for (final dark in <bool>[false, true]) {
    final mode = dark ? 'dark' : 'light';
    final palette = exampleHostPalette(dark);

    testWidgets('screenshots $mode', (tester) async {
      tester.view.physicalSize = const Size(780, 1688);
      tester.view.devicePixelRatio = 2;
      addTearDown(tester.view.reset);
      const nav = ExampleHostNavigationState();

      await tester.pumpWidget(
        _app(dark, _shell(nav, 'Vesper', _playBody(palette))),
      );
      await _shot(tester, 'play-$mode');

      await tester.pumpWidget(
        _app(
          dark,
          _shell(
            nav.selectTab(ExampleHostTab.library),
            'Library',
            ExampleLibraryHome(onOpenCollection: (_) {}),
          ),
        ),
      );
      await _shot(tester, 'library-$mode');

      final dolby = exampleLibraryCollectionById(
        ExampleLibraryCollectionId.dolby,
      );
      await tester.pumpWidget(
        _app(
          dark,
          _shell(
            nav.showCollection(ExampleLibraryCollectionId.dolby),
            'HDR & Dolby',
            ExampleLibraryCollectionDetail(
              collection: dolby,
              dashEnabled: true,
              onPlayItem: (_) {},
              dolbyCatalog: ExampleDolbyCatalogPanel(
                palette: palette,
                presets: exampleDolbyAcceptanceCatalog,
                selectedDrmKind: ExampleDolbyAcceptanceDrmKind.clear,
                selectedProfile: null,
                selectedFps: null,
                onDrmKindChanged: (_) {},
                onProfileChanged: (_) {},
                onFpsChanged: (_) {},
                onPresetPlayNow: (_) {},
                onPresetAddToQueue: (_) {},
              ),
            ),
          ),
        ),
      );
      await _shot(tester, 'library-dolby-$mode');

      await tester.pumpWidget(
        _app(
          dark,
          _shell(
            nav.selectTab(ExampleHostTab.downloads),
            'Downloads',
            ListView(
              padding: const EdgeInsets.all(18),
              children: <Widget>[
                ExampleDownloadTasksSection(
                  palette: palette,
                  tasks: const [],
                  pendingTasks: const [],
                  isDownloadExportPluginInstalled: false,
                  savingTaskIds: const <int>{},
                  exportProgressByTaskId: const <int, double>{},
                  onPrimaryAction: (_) {},
                  onSaveToGallery: (_) {},
                  onRemoveTask: (_) {},
                ),
              ],
            ),
          ),
        ),
      );
      await _shot(tester, 'downloads-$mode');

      await tester.pumpWidget(
        _app(
          dark,
          _shell(nav.selectTab(ExampleHostTab.lab), 'Lab', _labBody(palette)),
        ),
      );
      await _shot(tester, 'lab-$mode');

      await tester.pumpWidget(
        _app(
          dark,
          Builder(
            builder: (context) {
              return _shell(nav, 'Vesper', _playBody(palette));
            },
          ),
        ),
      );
      final shellContext = tester.element(find.byType(ExampleHostShell));
      showExampleOpenMediaSheet(
        shellContext,
        sheet: ExampleOpenMediaSheet(
          initialUrl: 'https://example.com/live.m3u8',
          localFilesEnabled: true,
          dashEnabled: true,
          onPickLocal: () async => '',
          onOpenUrl: (_) async => null,
          onOpenSample: (_) async => null,
        ),
      );
      await _shot(tester, 'open-media-$mode');
      Navigator.of(shellContext).pop();
      await tester.pumpAndSettle();

      showExampleSettingsSheet(
        shellContext,
        themeMode: dark ? ExampleThemeMode.dark : ExampleThemeMode.light,
        onThemeModeChange: (_) {},
      );
      await _shot(tester, 'settings-$mode');
    }, skip: !_enabled);
  }
}
