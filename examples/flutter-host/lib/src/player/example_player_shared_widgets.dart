part of 'example_player_sections.dart';

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

class ExampleDiagnosticsSummarySection extends StatelessWidget {
  const ExampleDiagnosticsSummarySection({
    super.key,
    required this.palette,
    required this.sourceLabel,
    required this.sourceProtocol,
    required this.routeLabel,
    required this.playbackOrigin,
    required this.sourceNormalizerSetting,
  });

  final ExampleHostPalette palette;
  final String sourceLabel;
  final String sourceProtocol;
  final String routeLabel;
  final ExamplePlaybackOrigin? playbackOrigin;
  final ExampleSourceNormalizerSetting sourceNormalizerSetting;

  @override
  Widget build(BuildContext context) {
    return ExampleSectionShell(
      palette: palette,
      title: 'Session 概览',
      subtitle: '当前播放路线、source、队列来源和宿主侧诊断模式。',
      child: Column(
        children: <Widget>[
          ExampleFactRow(label: 'Source', value: sourceLabel),
          ExampleFactRow(label: 'Protocol', value: sourceProtocol),
          ExampleFactRow(label: 'Route', value: routeLabel),
          ExampleFactRow(label: 'Origin', value: _playbackOriginLabel()),
          ExampleFactRow(
            label: 'SourceNormalizer',
            value: sourceNormalizerSetting.title,
          ),
        ],
      ),
    );
  }

  String _playbackOriginLabel() {
    final origin = playbackOrigin;
    return switch (origin) {
      ExampleQueuePlaybackOrigin() => '队列：${origin.itemId}',
      ExampleDolbyAdHocPlaybackOrigin() => 'Dolby 临时播放：${origin.presetId}',
      null => '无',
    };
  }
}

class ExampleEventLogSection extends StatelessWidget {
  const ExampleEventLogSection({
    super.key,
    required this.palette,
    required this.entries,
  });

  final ExampleHostPalette palette;
  final List<ExampleHostLogEntry> entries;

  @override
  Widget build(BuildContext context) {
    return ExampleSectionShell(
      palette: palette,
      title: '事件日志',
      subtitle: '只记录这个示例 app 的宿主操作事件；不读取 Logcat 或 native log。',
      child: entries.isEmpty
          ? Text(
              '还没有宿主事件。',
              style: Theme.of(
                context,
              ).textTheme.bodySmall?.copyWith(color: palette.body),
            )
          : SizedBox(
              height: 320,
              child: ListView.builder(
                itemCount: entries.length,
                itemBuilder: (context, index) {
                  final entry = entries[index];
                  return Padding(
                    key: ValueKey<int>(entry.id),
                    padding: const EdgeInsets.only(bottom: 10),
                    child: _ExampleEventLogRow(entry: entry, palette: palette),
                  );
                },
              ),
            ),
    );
  }
}

class _ExampleEventLogRow extends StatelessWidget {
  const _ExampleEventLogRow({required this.entry, required this.palette});

  final ExampleHostLogEntry entry;
  final ExampleHostPalette palette;

  @override
  Widget build(BuildContext context) {
    final at = DateTime.fromMillisecondsSinceEpoch(entry.atMillis);
    final timeLabel =
        '${at.hour.toString().padLeft(2, '0')}:'
        '${at.minute.toString().padLeft(2, '0')}:'
        '${at.second.toString().padLeft(2, '0')}';
    return Container(
      width: double.infinity,
      padding: const EdgeInsets.symmetric(horizontal: 14, vertical: 11),
      decoration: BoxDecoration(
        color: palette.fieldBackground,
        borderRadius: BorderRadius.circular(16),
      ),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: <Widget>[
          Row(
            children: <Widget>[
              SizedBox(
                width: 72,
                child: Text(
                  timeLabel,
                  style: Theme.of(context).textTheme.labelMedium?.copyWith(
                    color: palette.body,
                    fontFeatures: const <FontFeature>[
                      FontFeature.tabularFigures(),
                    ],
                  ),
                ),
              ),
              Text(
                _severityLabel(entry.severity),
                style: Theme.of(context).textTheme.labelMedium?.copyWith(
                  color: _severityColor(entry.severity),
                  fontWeight: FontWeight.w700,
                ),
              ),
              const SizedBox(width: 8),
              Expanded(
                child: Text(
                  entry.title,
                  maxLines: 1,
                  overflow: TextOverflow.ellipsis,
                  style: Theme.of(context).textTheme.bodyMedium?.copyWith(
                    color: palette.title,
                    fontWeight: FontWeight.w700,
                  ),
                ),
              ),
            ],
          ),
          if (entry.detail?.isNotEmpty == true) ...<Widget>[
            const SizedBox(height: 4),
            Text(
              entry.detail!,
              maxLines: 3,
              overflow: TextOverflow.ellipsis,
              style: Theme.of(
                context,
              ).textTheme.bodySmall?.copyWith(color: palette.body),
            ),
          ],
        ],
      ),
    );
  }

  String _severityLabel(ExampleHostLogSeverity severity) {
    return switch (severity) {
      ExampleHostLogSeverity.info => 'INFO',
      ExampleHostLogSeverity.warning => 'WARN',
      ExampleHostLogSeverity.error => 'ERROR',
    };
  }

  Color _severityColor(ExampleHostLogSeverity severity) {
    return switch (severity) {
      ExampleHostLogSeverity.info => palette.primaryAction,
      ExampleHostLogSeverity.warning => const Color(0xFFC17414),
      ExampleHostLogSeverity.error => const Color(0xFFC13C36),
    };
  }
}
