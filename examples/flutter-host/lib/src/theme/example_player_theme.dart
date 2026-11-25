import 'package:material_ui/material_ui.dart';

import '../player/example_player_models.dart';

/// Material 3 theme built from the Android prototype tokens.
///
/// `primary` is the neutral ink color; the amber accent is applied only to the
/// open-media FAB and the selected navigation indicator.
ThemeData buildExampleHostTheme({required bool useDarkTheme}) {
  final palette = exampleHostPalette(useDarkTheme);
  final brightness = useDarkTheme ? Brightness.dark : Brightness.light;
  final colorScheme = ColorScheme(
    brightness: brightness,
    primary: palette.primaryAction,
    onPrimary: palette.onPrimaryAction,
    primaryContainer: palette.surfaceVariant,
    onPrimaryContainer: palette.title,
    secondary: palette.accentInk,
    onSecondary: palette.onPrimaryAction,
    secondaryContainer: palette.accent,
    onSecondaryContainer: palette.onAccent,
    tertiary: palette.accentInk,
    onTertiary: palette.onPrimaryAction,
    error: palette.error,
    onError: Colors.white,
    surface: palette.sectionBackground,
    onSurface: palette.title,
    onSurfaceVariant: palette.body,
    surfaceContainerLowest: palette.pageBottom,
    surfaceContainerLow: palette.pageBottom,
    surfaceContainer: palette.pageBottom,
    surfaceContainerHigh: palette.sectionBackground,
    surfaceContainerHighest: palette.surfaceVariant,
    outline: palette.outline,
    outlineVariant: palette.sectionStroke,
    inverseSurface: palette.title,
    onInverseSurface: palette.pageBottom,
    inversePrimary: palette.accent,
    shadow: Colors.black,
    scrim: Colors.black,
  );
  final base = ThemeData(
    useMaterial3: true,
    brightness: brightness,
    colorScheme: colorScheme,
  );

  return base.copyWith(
    scaffoldBackgroundColor: palette.pageBottom,
    dividerColor: palette.sectionStroke,
    textTheme: base.textTheme.apply(
      bodyColor: palette.title,
      displayColor: palette.title,
    ),
    appBarTheme: AppBarTheme(
      backgroundColor: palette.pageBottom,
      foregroundColor: palette.title,
      surfaceTintColor: Colors.transparent,
      elevation: 0,
      scrolledUnderElevation: 0,
    ),
    navigationBarTheme: NavigationBarThemeData(
      backgroundColor: palette.pageBottom,
      surfaceTintColor: Colors.transparent,
      indicatorColor: palette.accent,
      iconTheme: WidgetStateProperty.resolveWith(
        (states) => IconThemeData(
          color: states.contains(WidgetState.selected)
              ? palette.onAccent
              : palette.body,
        ),
      ),
    ),
    navigationRailTheme: NavigationRailThemeData(
      backgroundColor: palette.pageBottom,
      indicatorColor: palette.accent,
      selectedIconTheme: IconThemeData(color: palette.onAccent),
      unselectedIconTheme: IconThemeData(color: palette.body),
      selectedLabelTextStyle: TextStyle(
        color: palette.title,
        fontWeight: FontWeight.w600,
      ),
      unselectedLabelTextStyle: TextStyle(color: palette.body),
    ),
    floatingActionButtonTheme: FloatingActionButtonThemeData(
      backgroundColor: palette.accent,
      foregroundColor: palette.onAccent,
      shape: RoundedRectangleBorder(borderRadius: BorderRadius.circular(16)),
    ),
    bottomSheetTheme: BottomSheetThemeData(
      backgroundColor: palette.sectionBackground,
      surfaceTintColor: Colors.transparent,
      showDragHandle: true,
      dragHandleColor: palette.outline,
      shape: const RoundedRectangleBorder(
        borderRadius: BorderRadius.vertical(top: Radius.circular(28)),
      ),
    ),
    snackBarTheme: SnackBarThemeData(
      behavior: SnackBarBehavior.floating,
      backgroundColor: palette.title,
      contentTextStyle: TextStyle(color: palette.pageBottom),
      actionTextColor: palette.accent,
    ),
    cardTheme: CardThemeData(
      color: palette.sectionBackground,
      surfaceTintColor: Colors.transparent,
      elevation: 0,
      margin: EdgeInsets.zero,
      shape: RoundedRectangleBorder(
        borderRadius: BorderRadius.circular(12),
        side: BorderSide(color: palette.sectionStroke),
      ),
    ),
    filledButtonTheme: FilledButtonThemeData(
      style: FilledButton.styleFrom(
        backgroundColor: palette.primaryAction,
        foregroundColor: palette.onPrimaryAction,
        shape: const StadiumBorder(),
      ),
    ),
    outlinedButtonTheme: OutlinedButtonThemeData(
      style: OutlinedButton.styleFrom(
        foregroundColor: palette.title,
        side: BorderSide(color: palette.outline),
        shape: const StadiumBorder(),
      ),
    ),
    textButtonTheme: TextButtonThemeData(
      style: TextButton.styleFrom(
        foregroundColor: palette.title,
        shape: const StadiumBorder(),
      ),
    ),
    inputDecorationTheme: InputDecorationTheme(
      filled: true,
      fillColor: palette.fieldBackground,
      labelStyle: TextStyle(color: palette.body),
      hintStyle: TextStyle(color: palette.body.withValues(alpha: 0.86)),
      border: OutlineInputBorder(
        borderRadius: BorderRadius.circular(12),
        borderSide: BorderSide(color: palette.sectionStroke),
      ),
      enabledBorder: OutlineInputBorder(
        borderRadius: BorderRadius.circular(12),
        borderSide: BorderSide(color: palette.sectionStroke),
      ),
      focusedBorder: OutlineInputBorder(
        borderRadius: BorderRadius.circular(12),
        borderSide: BorderSide(color: palette.title, width: 2),
      ),
    ),
    chipTheme: base.chipTheme.copyWith(
      shape: RoundedRectangleBorder(borderRadius: BorderRadius.circular(8)),
    ),
  );
}
