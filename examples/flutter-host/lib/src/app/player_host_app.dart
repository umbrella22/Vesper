import 'package:material_ui/material_ui.dart';
import 'package:vesper_player_ui/vesper_player_ui.dart' as ui;

import '../player/example_player_models.dart';
import '../player/player_host_page.dart';
import '../shell/example_host_strings.dart';
import '../theme/example_player_theme.dart';

class VesperFlutterHostApp extends StatefulWidget {
  const VesperFlutterHostApp({super.key, this.localeOverride});

  /// Forces the host and Stage copy language (used by tests). When null the
  /// device locale is used.
  final Locale? localeOverride;

  @override
  State<VesperFlutterHostApp> createState() => _VesperFlutterHostAppState();
}

class _VesperFlutterHostAppState extends State<VesperFlutterHostApp>
    with WidgetsBindingObserver {
  ExampleThemeMode _themeMode = ExampleThemeMode.system;

  @override
  void initState() {
    super.initState();
    WidgetsBinding.instance.addObserver(this);
  }

  @override
  void dispose() {
    WidgetsBinding.instance.removeObserver(this);
    super.dispose();
  }

  @override
  void didChangeLocales(List<Locale>? locales) {
    setState(() {});
  }

  @override
  Widget build(BuildContext context) {
    // The app does not depend on flutter_localizations, so Material keeps its
    // built-in English localizations and `Localizations.localeOf` stays en.
    // Host copy and the SDK's own Stage copy follow the device locale through
    // their scopes instead.
    final locale =
        widget.localeOverride ??
        WidgetsBinding.instance.platformDispatcher.locale;
    return MaterialApp(
      debugShowCheckedModeBanner: false,
      title: 'Vesper Flutter Host',
      theme: buildExampleHostTheme(useDarkTheme: false),
      darkTheme: buildExampleHostTheme(useDarkTheme: true),
      themeMode: switch (_themeMode) {
        ExampleThemeMode.system => ThemeMode.system,
        ExampleThemeMode.light => ThemeMode.light,
        ExampleThemeMode.dark => ThemeMode.dark,
      },
      builder: (context, child) => ExampleHostStringsScope(
        strings: ExampleHostStrings.forLocale(locale),
        child: ui.VesperPlayerStageStringsScope(
          strings: ui.VesperPlayerStageStrings.forLocale(locale),
          child: child ?? const SizedBox.shrink(),
        ),
      ),
      home: PlayerHostPage(
        themeMode: _themeMode,
        onThemeModeChange: (mode) {
          setState(() {
            _themeMode = mode;
          });
        },
      ),
    );
  }
}
