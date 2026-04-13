import 'package:flutter/material.dart';

import 'example_player_models.dart';

ThemeData buildExampleHostTheme({required bool useDarkTheme}) {
  final palette = exampleHostPalette(useDarkTheme);
  final base = ThemeData(
    useMaterial3: true,
    brightness: useDarkTheme ? Brightness.dark : Brightness.light,
  );

  final colorScheme = useDarkTheme
      ? ColorScheme.dark(
          primary: palette.primaryAction,
          onPrimary: Colors.white,
          secondary: const Color(0xFF4BC0B3),
          onSecondary: Colors.white,
          surface: palette.sectionBackground,
          onSurface: palette.title,
          outline: palette.sectionStroke,
          error: const Color(0xFFFF7066),
          onError: Colors.white,
        )
      : ColorScheme.light(
          primary: palette.primaryAction,
          onPrimary: Colors.white,
          secondary: const Color(0xFF145A63),
          onSecondary: Colors.white,
          surface: palette.sectionBackground,
          onSurface: palette.title,
          outline: palette.sectionStroke,
          error: const Color(0xFFC13C36),
          onError: Colors.white,
        );

  return base.copyWith(
    colorScheme: colorScheme,
    scaffoldBackgroundColor: palette.pageBottom,
    dividerColor: palette.sectionStroke,
    textTheme: base.textTheme.apply(
      bodyColor: palette.title,
      displayColor: palette.title,
    ),
    filledButtonTheme: FilledButtonThemeData(
      style: FilledButton.styleFrom(
        shape: RoundedRectangleBorder(borderRadius: BorderRadius.circular(16)),
      ),
    ),
    outlinedButtonTheme: OutlinedButtonThemeData(
      style: OutlinedButton.styleFrom(
        side: BorderSide(color: palette.sectionStroke),
        shape: RoundedRectangleBorder(borderRadius: BorderRadius.circular(16)),
      ),
    ),
    textButtonTheme: TextButtonThemeData(
      style: TextButton.styleFrom(
        shape: RoundedRectangleBorder(borderRadius: BorderRadius.circular(999)),
      ),
    ),
    inputDecorationTheme: InputDecorationTheme(
      filled: true,
      fillColor: palette.fieldBackground,
      labelStyle: TextStyle(color: palette.body),
      hintStyle: TextStyle(color: palette.body.withValues(alpha: 0.86)),
      border: OutlineInputBorder(
        borderRadius: BorderRadius.circular(18),
        borderSide: BorderSide(color: palette.sectionStroke),
      ),
      enabledBorder: OutlineInputBorder(
        borderRadius: BorderRadius.circular(18),
        borderSide: BorderSide(color: palette.sectionStroke),
      ),
      focusedBorder: OutlineInputBorder(
        borderRadius: BorderRadius.circular(18),
        borderSide: BorderSide(color: palette.primaryAction),
      ),
    ),
    chipTheme: base.chipTheme.copyWith(
      shape: RoundedRectangleBorder(borderRadius: BorderRadius.circular(999)),
    ),
  );
}
