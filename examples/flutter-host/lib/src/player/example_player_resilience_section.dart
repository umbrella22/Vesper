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
    return ExampleSectionShell(
      palette: palette,
      title: '恢复策略',
      subtitle:
          '这里演示 resilience policy 的 Flutter API。切换 profile 时会直接下发到播放器，并尽量保留当前媒体与播放进度。',
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
            activeProfile.subtitle,
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
            value:
                '${policy.retry.maxAttempts ?? '-'} 次 · ${policy.retry.backoff.name}',
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
