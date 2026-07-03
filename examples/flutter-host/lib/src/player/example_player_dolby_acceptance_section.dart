part of 'example_player_sections.dart';

class ExampleDolbyAcceptanceSection extends StatelessWidget {
  const ExampleDolbyAcceptanceSection({
    super.key,
    required this.palette,
    required this.presets,
    required this.selectedDrmKind,
    required this.selectedProfile,
    required this.selectedFps,
    required this.onDrmKindChanged,
    required this.onProfileChanged,
    required this.onFpsChanged,
    required this.onPresetSelected,
    this.isPresetPlayable,
    this.disabledReasonForPreset,
  });

  final ExampleHostPalette palette;
  final List<ExampleDolbyAcceptancePreset> presets;
  final ExampleDolbyAcceptanceDrmKind selectedDrmKind;
  final ExampleDolbyAcceptanceProfile? selectedProfile;
  final int? selectedFps;
  final ValueChanged<ExampleDolbyAcceptanceDrmKind> onDrmKindChanged;
  final ValueChanged<ExampleDolbyAcceptanceProfile?> onProfileChanged;
  final ValueChanged<int?> onFpsChanged;
  final ValueChanged<ExampleDolbyAcceptancePreset> onPresetSelected;
  final bool Function(ExampleDolbyAcceptancePreset preset)? isPresetPlayable;
  final String? Function(ExampleDolbyAcceptancePreset preset)?
  disabledReasonForPreset;

  List<ExampleDolbyAcceptancePreset> get _filteredPresets {
    return presets
        .where((preset) {
          return preset.drmKind == selectedDrmKind &&
              (selectedProfile == null || preset.profile == selectedProfile) &&
              (selectedFps == null || preset.fps == selectedFps);
        })
        .toList(growable: false);
  }

  @override
  Widget build(BuildContext context) {
    final filteredPresets = _filteredPresets;
    return ExampleSectionShell(
      palette: palette,
      title: 'Dolby 验收',
      subtitle:
          'Dolby Browser Test Kit 公开信号，只走 direct native 播放；不进入下载、预加载、外部投放或插件处理。',
      accent: const Color(0xFF7A4DFF),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: <Widget>[
          _DolbyFilterRow<ExampleDolbyAcceptanceDrmKind>(
            values: ExampleDolbyAcceptanceDrmKind.values,
            selected: selectedDrmKind,
            labelFor: (value) => value.title,
            onSelected: onDrmKindChanged,
          ),
          const SizedBox(height: 10),
          _DolbyNullableFilterRow<ExampleDolbyAcceptanceProfile>(
            allLabel: 'All profiles',
            values: ExampleDolbyAcceptanceProfile.values,
            selected: selectedProfile,
            labelFor: (value) => value.title,
            onSelected: onProfileChanged,
          ),
          const SizedBox(height: 10),
          _DolbyNullableFilterRow<int>(
            allLabel: 'All fps',
            values: exampleDolbyAcceptanceFpsValues,
            selected: selectedFps,
            labelFor: (value) => '${value}fps',
            onSelected: onFpsChanged,
          ),
          const SizedBox(height: 14),
          if (selectedDrmKind == ExampleDolbyAcceptanceDrmKind.fairPlayPending)
            Padding(
              padding: const EdgeInsets.only(bottom: 12),
              child: Text(
                'FairPlay presets are listed for metadata only. Enable them after a certificate URI or base64 certificate is available.',
                style: Theme.of(context).textTheme.bodySmall?.copyWith(
                  color: palette.body,
                  height: 1.45,
                ),
              ),
            ),
          if (filteredPresets.isEmpty)
            Text(
              'No Dolby presets match this filter.',
              style: Theme.of(
                context,
              ).textTheme.bodySmall?.copyWith(color: palette.body),
            )
          else
            ...filteredPresets.map((preset) {
              final playable =
                  isPresetPlayable?.call(preset) ?? preset.isPlayable;
              final disabledReason = playable
                  ? null
                  : disabledReasonForPreset?.call(preset);
              return Padding(
                padding: const EdgeInsets.only(bottom: 10),
                child: ExampleDolbyAcceptancePresetRow(
                  preset: preset,
                  palette: palette,
                  playable: playable,
                  disabledReason: disabledReason,
                  onSelected: playable ? () => onPresetSelected(preset) : null,
                ),
              );
            }),
        ],
      ),
    );
  }
}

class ExampleDolbyAcceptancePresetRow extends StatelessWidget {
  const ExampleDolbyAcceptancePresetRow({
    super.key,
    required this.preset,
    required this.palette,
    required this.playable,
    required this.disabledReason,
    required this.onSelected,
  });

  final ExampleDolbyAcceptancePreset preset;
  final ExampleHostPalette palette;
  final bool playable;
  final String? disabledReason;
  final VoidCallback? onSelected;

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    final enabled = onSelected != null;
    final details = <String>[
      preset.profile.title,
      '${preset.fps}fps',
      preset.protocolLabel,
      preset.drmKind.title,
      preset.manualGate,
    ].join(' · ');
    return OutlinedButton(
      onPressed: onSelected,
      style: OutlinedButton.styleFrom(
        alignment: Alignment.centerLeft,
        padding: const EdgeInsets.symmetric(horizontal: 14, vertical: 12),
        foregroundColor: enabled ? palette.title : palette.body,
      ),
      child: SizedBox(
        width: double.infinity,
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: <Widget>[
            Row(
              children: <Widget>[
                Icon(
                  playable
                      ? Icons.play_circle_outline_rounded
                      : Icons.lock_clock_rounded,
                  size: 18,
                ),
                const SizedBox(width: 8),
                Expanded(
                  child: Text(
                    preset.label,
                    maxLines: 1,
                    overflow: TextOverflow.ellipsis,
                    style: theme.textTheme.bodyMedium?.copyWith(
                      fontWeight: FontWeight.w700,
                    ),
                  ),
                ),
              ],
            ),
            const SizedBox(height: 6),
            Text(
              details,
              maxLines: 2,
              overflow: TextOverflow.ellipsis,
              style: theme.textTheme.bodySmall?.copyWith(
                color: palette.body,
                height: 1.35,
              ),
            ),
            if (disabledReason != null || preset.notes.isNotEmpty) ...<Widget>[
              const SizedBox(height: 4),
              Text(
                disabledReason ?? preset.notes.first,
                maxLines: 2,
                overflow: TextOverflow.ellipsis,
                style: theme.textTheme.bodySmall?.copyWith(
                  color: palette.body,
                  height: 1.35,
                ),
              ),
            ],
          ],
        ),
      ),
    );
  }
}

class _DolbyFilterRow<T> extends StatelessWidget {
  const _DolbyFilterRow({
    required this.values,
    required this.selected,
    required this.labelFor,
    required this.onSelected,
  });

  final List<T> values;
  final T selected;
  final String Function(T value) labelFor;
  final ValueChanged<T> onSelected;

  @override
  Widget build(BuildContext context) {
    return SingleChildScrollView(
      scrollDirection: Axis.horizontal,
      child: Row(
        children: values
            .map(
              (value) => Padding(
                padding: const EdgeInsets.only(right: 10),
                child: ChoiceChip(
                  label: Text(labelFor(value)),
                  selected: value == selected,
                  onSelected: value == selected
                      ? null
                      : (_) => onSelected(value),
                ),
              ),
            )
            .toList(growable: false),
      ),
    );
  }
}

class _DolbyNullableFilterRow<T> extends StatelessWidget {
  const _DolbyNullableFilterRow({
    required this.allLabel,
    required this.values,
    required this.selected,
    required this.labelFor,
    required this.onSelected,
  });

  final String allLabel;
  final List<T> values;
  final T? selected;
  final String Function(T value) labelFor;
  final ValueChanged<T?> onSelected;

  @override
  Widget build(BuildContext context) {
    return SingleChildScrollView(
      scrollDirection: Axis.horizontal,
      child: Row(
        children: <Widget>[
          Padding(
            padding: const EdgeInsets.only(right: 10),
            child: ChoiceChip(
              label: Text(allLabel),
              selected: selected == null,
              onSelected: selected == null ? null : (_) => onSelected(null),
            ),
          ),
          ...values.map(
            (value) => Padding(
              padding: const EdgeInsets.only(right: 10),
              child: ChoiceChip(
                label: Text(labelFor(value)),
                selected: value == selected,
                onSelected: value == selected ? null : (_) => onSelected(value),
              ),
            ),
          ),
        ],
      ),
    );
  }
}
