import 'package:material_ui/material_ui.dart';

import '../player/example_player_models.dart';
import 'example_host_strings.dart';

extension ExampleThemeModeText on ExampleThemeMode {
  ExampleHostText get textKey {
    return switch (this) {
      ExampleThemeMode.system => ExampleHostText.themeSystem,
      ExampleThemeMode.light => ExampleHostText.themeLight,
      ExampleThemeMode.dark => ExampleHostText.themeDark,
    };
  }

  IconData get icon {
    return switch (this) {
      ExampleThemeMode.system => Icons.brightness_auto_rounded,
      ExampleThemeMode.light => Icons.light_mode_rounded,
      ExampleThemeMode.dark => Icons.dark_mode_rounded,
    };
  }
}

/// Settings sheet opened from the top-bar gear.
class ExampleSettingsSheet extends StatefulWidget {
  const ExampleSettingsSheet({
    super.key,
    required this.themeMode,
    required this.onThemeModeChange,
  });

  final ExampleThemeMode themeMode;
  final ValueChanged<ExampleThemeMode> onThemeModeChange;

  @override
  State<ExampleSettingsSheet> createState() => _ExampleSettingsSheetState();
}

class _ExampleSettingsSheetState extends State<ExampleSettingsSheet> {
  late ExampleThemeMode _mode = widget.themeMode;

  @override
  Widget build(BuildContext context) {
    final strings = ExampleHostStrings.of(context);
    final theme = Theme.of(context);
    return SingleChildScrollView(
      padding: const EdgeInsets.fromLTRB(24, 0, 24, 28),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        mainAxisSize: MainAxisSize.min,
        children: <Widget>[
          Text(
            strings[ExampleHostText.settings],
            style: theme.textTheme.titleLarge,
          ),
          const SizedBox(height: 20),
          Text(
            strings[ExampleHostText.settingsAppearance],
            style: theme.textTheme.titleSmall,
          ),
          const SizedBox(height: 10),
          SegmentedButton<ExampleThemeMode>(
            key: const ValueKey<String>('settings-theme-mode'),
            showSelectedIcon: false,
            segments: <ButtonSegment<ExampleThemeMode>>[
              for (final mode in ExampleThemeMode.values)
                ButtonSegment<ExampleThemeMode>(
                  value: mode,
                  icon: Icon(mode.icon, size: 18),
                  label: Text(strings[mode.textKey]),
                ),
            ],
            selected: <ExampleThemeMode>{_mode},
            onSelectionChanged: (selection) {
              final next = selection.first;
              setState(() => _mode = next);
              widget.onThemeModeChange(next);
            },
          ),
          const SizedBox(height: 24),
          Text(
            strings[ExampleHostText.settingsAbout],
            style: theme.textTheme.bodySmall?.copyWith(
              color: theme.colorScheme.onSurfaceVariant,
            ),
          ),
        ],
      ),
    );
  }
}

Future<void> showExampleSettingsSheet(
  BuildContext context, {
  required ExampleThemeMode themeMode,
  required ValueChanged<ExampleThemeMode> onThemeModeChange,
}) {
  final strings = ExampleHostStrings.of(context);
  return showModalBottomSheet<void>(
    context: context,
    isScrollControlled: true,
    useSafeArea: true,
    showDragHandle: true,
    builder: (context) => ExampleHostStringsScope(
      strings: strings,
      child: ExampleSettingsSheet(
        themeMode: themeMode,
        onThemeModeChange: onThemeModeChange,
      ),
    ),
  );
}
