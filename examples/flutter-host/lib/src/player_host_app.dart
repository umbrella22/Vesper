import 'package:flutter/material.dart';

import 'example_player_models.dart';
import 'example_player_theme.dart';
import 'player_host_page.dart';

class VesperFlutterHostApp extends StatefulWidget {
  const VesperFlutterHostApp({super.key});

  @override
  State<VesperFlutterHostApp> createState() => _VesperFlutterHostAppState();
}

class _VesperFlutterHostAppState extends State<VesperFlutterHostApp> {
  ExampleThemeMode _themeMode = ExampleThemeMode.system;

  @override
  Widget build(BuildContext context) {
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
