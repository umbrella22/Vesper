import 'package:flutter/material.dart';
import 'package:vesper_player/vesper_player.dart';
import 'package:vesper_player_external_playback/vesper_player_external_playback.dart';
import 'package:vesper_player_ui/vesper_player_ui.dart' as ui;

import 'example_player_helpers.dart';
import 'example_player_models.dart';
import '../hdr_evidence/hdr_evidence_capture.dart';

part 'example_player_resilience_section.dart';
part 'example_player_plugin_sections.dart';
part 'example_player_system_section.dart';
part 'example_player_playlist_section.dart';
part 'example_player_shared_widgets.dart';

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
    required this.onUseLiveDvrAcceptance,
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
  final VoidCallback onUseLiveDvrAcceptance;
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
                  onPressed: onUseLiveDvrAcceptance,
                  child: const Text('Live DVR 验收'),
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
