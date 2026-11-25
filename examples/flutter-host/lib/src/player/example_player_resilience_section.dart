part of 'example_player_sections.dart';

class ExampleResilienceSection extends StatelessWidget {
  const ExampleResilienceSection({
    super.key,
    required this.palette,
    required this.activePolicy,
    required this.selectedProfile,
    required this.onApplyProfile,
  });

  final ExampleHostPalette palette;
  final VesperPlaybackResiliencePolicy activePolicy;
  final ExampleResilienceProfile selectedProfile;
  final Future<void> Function(ExampleResilienceProfile profile) onApplyProfile;

  @override
  Widget build(BuildContext context) {
    final activeProfile =
        ExampleResilienceProfileLabels.fromPolicy(activePolicy) ??
        selectedProfile;
    final policy = activePolicy;
    final strings = ExampleHostStrings.of(context);
    return ExampleSectionShell(
      palette: palette,
      title: strings[ExampleHostText.resilienceTitle],
      subtitle: strings[ExampleHostText.resilienceSubtitle],
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: <Widget>[
          Wrap(
            spacing: 10,
            runSpacing: 10,
            children: ExampleResilienceProfile.values
                .map((profile) {
                  return ChoiceChip(
                    label: Text(profile.title),
                    selected: profile == activeProfile,
                    onSelected: profile == activeProfile
                        ? null
                        : (_) => onApplyProfile(profile),
                  );
                })
                .toList(growable: false),
          ),
          const SizedBox(height: 14),
          Text(
            activeProfile.subtitle(strings),
            style: Theme.of(
              context,
            ).textTheme.bodyMedium?.copyWith(color: palette.body, height: 1.45),
          ),
          const SizedBox(height: 18),
          ExampleFactRow(
            label: 'buffering',
            value:
                '${policy.buffering.preset.name} · ${bufferWindowLabel(policy.buffering)}',
          ),
          ExampleFactRow(
            label: 'retry',
            value: strings
                .format(ExampleHostText.resilienceRetryValue, <String, Object?>{
                  'count': policy.retry.maxAttempts ?? '-',
                  'backoff': policy.retry.backoff.name,
                }),
          ),
          ExampleFactRow(
            label: 'cache',
            value:
                '${policy.cache.preset.name} · memory ${formatBytes(policy.cache.maxMemoryBytes)} / disk ${formatBytes(policy.cache.maxDiskBytes)}',
          ),
        ],
      ),
    );
  }
}
