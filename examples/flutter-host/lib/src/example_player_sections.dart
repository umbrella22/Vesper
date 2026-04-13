import 'package:flutter/material.dart';
import 'package:vesper_player/vesper_player.dart';

import 'example_player_helpers.dart';
import 'example_player_models.dart';

class ExamplePlayerHeader extends StatelessWidget {
  const ExamplePlayerHeader({
    super.key,
    required this.sourceLabel,
    required this.subtitle,
    required this.palette,
  });

  final String sourceLabel;
  final String subtitle;
  final ExampleHostPalette palette;

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: <Widget>[
        Text(
          'Vesper',
          style: theme.textTheme.headlineMedium?.copyWith(
            color: palette.title,
            fontWeight: FontWeight.w900,
            letterSpacing: -1.2,
          ),
        ),
        const SizedBox(height: 8),
        Text(
          sourceLabel,
          style: theme.textTheme.titleSmall?.copyWith(
            color: palette.title,
            fontWeight: FontWeight.w600,
          ),
          maxLines: 1,
          overflow: TextOverflow.ellipsis,
        ),
        const SizedBox(height: 6),
        Text(
          subtitle,
          style: theme.textTheme.bodyMedium?.copyWith(
            color: palette.body,
            height: 1.45,
          ),
          maxLines: 2,
          overflow: TextOverflow.ellipsis,
        ),
      ],
    );
  }
}

class ExampleSourceSection extends StatelessWidget {
  const ExampleSourceSection({
    super.key,
    required this.palette,
    required this.themeMode,
    required this.remoteUrlController,
    required this.localFilesEnabled,
    required this.dashEnabled,
    required this.onThemeModeChange,
    required this.onPickVideo,
    required this.onUseHlsDemo,
    required this.onUseDashDemo,
    required this.onOpenRemote,
    this.dashUnavailableMessage,
  });

  final ExampleHostPalette palette;
  final ExampleThemeMode themeMode;
  final TextEditingController remoteUrlController;
  final bool localFilesEnabled;
  final bool dashEnabled;
  final ValueChanged<ExampleThemeMode> onThemeModeChange;
  final VoidCallback onPickVideo;
  final VoidCallback onUseHlsDemo;
  final VoidCallback onUseDashDemo;
  final VoidCallback onOpenRemote;
  final String? dashUnavailableMessage;

  @override
  Widget build(BuildContext context) {
    return Container(
      width: double.infinity,
      padding: const EdgeInsets.all(18),
      decoration: BoxDecoration(
        color: palette.sectionBackground,
        borderRadius: BorderRadius.circular(24),
        border: Border.all(color: palette.sectionStroke),
      ),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: <Widget>[
          Text(
            '媒体源',
            style: Theme.of(context).textTheme.titleMedium?.copyWith(
              color: palette.title,
              fontWeight: FontWeight.bold,
            ),
          ),
          const SizedBox(height: 14),
          Text(
            '使用这些演示操作在本地文件、HLS、DASH 和自定义远程 URL 之间切换。',
            style: Theme.of(
              context,
            ).textTheme.bodySmall?.copyWith(color: palette.body),
          ),
          const SizedBox(height: 14),
          SingleChildScrollView(
            scrollDirection: Axis.horizontal,
            child: Row(
              children: <Widget>[
                OutlinedButton(
                  onPressed: localFilesEnabled ? onPickVideo : null,
                  child: const Text('选择视频'),
                ),
                const SizedBox(width: 10),
                OutlinedButton(
                  onPressed: onUseHlsDemo,
                  child: const Text('HLS 演示'),
                ),
                const SizedBox(width: 10),
                OutlinedButton(
                  onPressed: dashEnabled ? onUseDashDemo : null,
                  child: const Text('DASH 演示'),
                ),
              ],
            ),
          ),
          if (dashUnavailableMessage != null) ...<Widget>[
            const SizedBox(height: 10),
            Text(
              dashUnavailableMessage!,
              style: Theme.of(
                context,
              ).textTheme.bodySmall?.copyWith(color: palette.body),
            ),
          ],
          const SizedBox(height: 14),
          TextField(
            controller: remoteUrlController,
            keyboardType: TextInputType.url,
            maxLines: 1,
            decoration: const InputDecoration(labelText: '远程流 URL'),
          ),
          const SizedBox(height: 14),
          Column(
            crossAxisAlignment: CrossAxisAlignment.start,
            children: <Widget>[
              Text(
                '主题',
                style: Theme.of(context).textTheme.labelLarge?.copyWith(
                  color: palette.title,
                  fontWeight: FontWeight.w600,
                ),
              ),
              const SizedBox(height: 10),
              SingleChildScrollView(
                scrollDirection: Axis.horizontal,
                child: Row(
                  children: <Widget>[
                    ExampleThemeModeChip(
                      icon: Icons.brightness_auto_rounded,
                      label: ExampleThemeMode.system.title,
                      selected: themeMode == ExampleThemeMode.system,
                      palette: palette,
                      onTap: () => onThemeModeChange(ExampleThemeMode.system),
                    ),
                    const SizedBox(width: 10),
                    ExampleThemeModeChip(
                      icon: Icons.light_mode_rounded,
                      label: ExampleThemeMode.light.title,
                      selected: themeMode == ExampleThemeMode.light,
                      palette: palette,
                      onTap: () => onThemeModeChange(ExampleThemeMode.light),
                    ),
                    const SizedBox(width: 10),
                    ExampleThemeModeChip(
                      icon: Icons.dark_mode_rounded,
                      label: ExampleThemeMode.dark.title,
                      selected: themeMode == ExampleThemeMode.dark,
                      palette: palette,
                      onTap: () => onThemeModeChange(ExampleThemeMode.dark),
                    ),
                  ],
                ),
              ),
            ],
          ),
          const SizedBox(height: 14),
          FilledButton(
            onPressed: onOpenRemote,
            style: FilledButton.styleFrom(
              backgroundColor: palette.primaryAction,
              foregroundColor: Colors.white,
            ),
            child: const Text('打开远程 URL'),
          ),
        ],
      ),
    );
  }
}

class ExampleResilienceSection extends StatelessWidget {
  const ExampleResilienceSection({
    super.key,
    required this.palette,
    required this.selectedProfile,
    required this.onApplyProfile,
  });

  final ExampleHostPalette palette;
  final ExampleResilienceProfile selectedProfile;
  final Future<void> Function(ExampleResilienceProfile profile) onApplyProfile;

  @override
  Widget build(BuildContext context) {
    final policy = selectedProfile.policy;
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
                    selected: profile == selectedProfile,
                    onSelected: profile == selectedProfile
                        ? null
                        : (_) => onApplyProfile(profile),
                  );
                })
                .toList(growable: false),
          ),
          const SizedBox(height: 14),
          Text(
            selectedProfile.subtitle,
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

class ExampleRecentErrorSection extends StatelessWidget {
  const ExampleRecentErrorSection({
    super.key,
    required this.palette,
    required this.error,
  });

  final ExampleHostPalette palette;
  final VesperPlayerError error;

  @override
  Widget build(BuildContext context) {
    return ExampleSectionShell(
      palette: palette,
      title: '最近错误',
      subtitle: '平台层错误会同时进入 snapshot 和 event stream。',
      accent: const Color(0xFFC13C36),
      child: Text(
        error.message,
        style: const TextStyle(color: Color(0xFF7F231F), height: 1.45),
      ),
    );
  }
}

class ExampleSectionShell extends StatelessWidget {
  const ExampleSectionShell({
    super.key,
    required this.palette,
    required this.title,
    required this.subtitle,
    required this.child,
    this.accent = const Color(0xFF172033),
  });

  final ExampleHostPalette palette;
  final String title;
  final String subtitle;
  final Widget child;
  final Color accent;

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    return Container(
      padding: const EdgeInsets.all(18),
      decoration: BoxDecoration(
        color: palette.sectionBackground,
        borderRadius: BorderRadius.circular(24),
        border: Border.all(color: palette.sectionStroke),
      ),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: <Widget>[
          Text(
            title,
            style: theme.textTheme.titleMedium?.copyWith(
              color: palette.title,
              fontWeight: FontWeight.w700,
            ),
          ),
          const SizedBox(height: 8),
          Text(
            subtitle,
            style: theme.textTheme.bodySmall?.copyWith(
              color: palette.body,
              height: 1.45,
            ),
          ),
          const SizedBox(height: 14),
          Container(
            width: 42,
            height: 4,
            decoration: BoxDecoration(
              color: accent,
              borderRadius: BorderRadius.circular(999),
            ),
          ),
          const SizedBox(height: 16),
          child,
        ],
      ),
    );
  }
}

class ExampleThemeModeChip extends StatelessWidget {
  const ExampleThemeModeChip({
    super.key,
    required this.icon,
    required this.label,
    required this.selected,
    required this.palette,
    required this.onTap,
  });

  final IconData icon;
  final String label;
  final bool selected;
  final ExampleHostPalette palette;
  final VoidCallback onTap;

  @override
  Widget build(BuildContext context) {
    return TextButton.icon(
      onPressed: onTap,
      style: TextButton.styleFrom(
        padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 8),
        foregroundColor: selected ? Colors.white : palette.title,
        backgroundColor: selected
            ? palette.primaryAction
            : Theme.of(context).colorScheme.surface.withValues(alpha: 0.72),
      ),
      icon: Icon(icon, size: 16),
      label: Text(label, maxLines: 1),
    );
  }
}

class ExampleFactRow extends StatelessWidget {
  const ExampleFactRow({super.key, required this.label, required this.value});

  final String label;
  final String value;

  @override
  Widget build(BuildContext context) {
    return Padding(
      padding: const EdgeInsets.symmetric(vertical: 6),
      child: Row(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: <Widget>[
          SizedBox(
            width: 112,
            child: Text(
              label,
              style: Theme.of(
                context,
              ).textTheme.bodyMedium?.copyWith(color: const Color(0xFF5C667A)),
            ),
          ),
          const SizedBox(width: 10),
          Expanded(
            child: Text(
              value,
              style: Theme.of(
                context,
              ).textTheme.bodyMedium?.copyWith(fontWeight: FontWeight.w600),
            ),
          ),
        ],
      ),
    );
  }
}

class ExampleInlineControllerError extends StatelessWidget {
  const ExampleInlineControllerError({super.key, required this.error});

  final Object? error;

  @override
  Widget build(BuildContext context) {
    return Container(
      width: double.infinity,
      padding: const EdgeInsets.symmetric(horizontal: 16, vertical: 14),
      decoration: BoxDecoration(
        color: const Color(0x14C13C36),
        borderRadius: BorderRadius.circular(18),
        border: Border.all(color: const Color(0x33C13C36)),
      ),
      child: Row(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: <Widget>[
          const Icon(Icons.error_outline_rounded, color: Color(0xFFC13C36)),
          const SizedBox(width: 12),
          Expanded(
            child: Text(
              '$error',
              style: Theme.of(context).textTheme.bodyMedium?.copyWith(
                color: const Color(0xFF7F231F),
                height: 1.4,
              ),
            ),
          ),
        ],
      ),
    );
  }
}

class ExampleBusyPill extends StatelessWidget {
  const ExampleBusyPill({super.key, required this.label});

  final String label;

  @override
  Widget build(BuildContext context) {
    return Container(
      padding: const EdgeInsets.symmetric(horizontal: 14, vertical: 10),
      decoration: BoxDecoration(
        color: Colors.white.withValues(alpha: 0.92),
        borderRadius: BorderRadius.circular(999),
        boxShadow: const <BoxShadow>[
          BoxShadow(
            color: Color(0x16000000),
            blurRadius: 20,
            offset: Offset(0, 12),
          ),
        ],
      ),
      child: Row(
        mainAxisSize: MainAxisSize.min,
        children: <Widget>[
          const SizedBox(
            width: 14,
            height: 14,
            child: CircularProgressIndicator(strokeWidth: 2),
          ),
          const SizedBox(width: 10),
          Text(
            label,
            style: Theme.of(
              context,
            ).textTheme.labelLarge?.copyWith(fontWeight: FontWeight.w700),
          ),
        ],
      ),
    );
  }
}

class ExampleLoadingState extends StatelessWidget {
  const ExampleLoadingState({super.key});

  @override
  Widget build(BuildContext context) {
    return const Center(
      child: Column(
        mainAxisSize: MainAxisSize.min,
        children: <Widget>[
          CircularProgressIndicator(),
          SizedBox(height: 18),
          Text('正在初始化 Vesper Flutter Host...'),
        ],
      ),
    );
  }
}

class ExampleErrorState extends StatelessWidget {
  const ExampleErrorState({super.key, required this.error});

  final Object? error;

  @override
  Widget build(BuildContext context) {
    return Center(
      child: Padding(
        padding: const EdgeInsets.all(24),
        child: Column(
          mainAxisSize: MainAxisSize.min,
          children: <Widget>[
            const Icon(
              Icons.error_outline_rounded,
              size: 40,
              color: Color(0xFFC13C36),
            ),
            const SizedBox(height: 16),
            Text(
              '控制器初始化失败',
              style: Theme.of(
                context,
              ).textTheme.titleLarge?.copyWith(fontWeight: FontWeight.w700),
            ),
            const SizedBox(height: 10),
            Text(
              '$error',
              textAlign: TextAlign.center,
              style: Theme.of(
                context,
              ).textTheme.bodyMedium?.copyWith(color: const Color(0xFF7F231F)),
            ),
          ],
        ),
      ),
    );
  }
}
