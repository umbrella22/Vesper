part of 'example_player_sections.dart';

class ExamplePlaylistSection extends StatelessWidget {
  const ExamplePlaylistSection({
    super.key,
    required this.palette,
    required this.playlistItems,
    required this.onSelectItem,
  });

  final ExampleHostPalette palette;
  final List<ExamplePlaylistItemViewData> playlistItems;
  final ValueChanged<String> onSelectItem;

  @override
  Widget build(BuildContext context) {
    return ExampleSectionShell(
      palette: palette,
      title: '播放列表',
      subtitle: '点击演示流、本地视频或自定义远程 URL 后，媒体源会按加入顺序出现在这里。',
      child: playlistItems.isEmpty
          ? Text(
              '播放列表里还没有媒体源。',
              style: Theme.of(
                context,
              ).textTheme.bodySmall?.copyWith(color: palette.body),
            )
          : Column(
              children: playlistItems
                  .map(
                    (item) => Padding(
                      padding: const EdgeInsets.only(bottom: 10),
                      child: _ExamplePlaylistRow(
                        item: item,
                        palette: palette,
                        onTap: () => onSelectItem(item.itemId),
                      ),
                    ),
                  )
                  .toList(growable: false),
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

class _ExamplePlaylistRow extends StatelessWidget {
  const _ExamplePlaylistRow({
    required this.item,
    required this.palette,
    required this.onTap,
  });

  final ExamplePlaylistItemViewData item;
  final ExampleHostPalette palette;
  final VoidCallback onTap;

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    return TextButton(
      onPressed: onTap,
      style: TextButton.styleFrom(
        padding: const EdgeInsets.symmetric(horizontal: 16, vertical: 12),
        backgroundColor: item.isActive
            ? palette.primaryAction
            : palette.fieldBackground,
        foregroundColor: item.isActive ? Colors.white : palette.title,
        shape: RoundedRectangleBorder(
          borderRadius: BorderRadius.circular(18),
          side: BorderSide(
            color: item.isActive ? Colors.transparent : palette.sectionStroke,
          ),
        ),
      ),
      child: SizedBox(
        width: double.infinity,
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: <Widget>[
            Text(
              item.label,
              maxLines: 1,
              overflow: TextOverflow.ellipsis,
              style: theme.textTheme.bodyLarge?.copyWith(
                fontWeight: FontWeight.w600,
                color: item.isActive ? Colors.white : palette.title,
              ),
            ),
            const SizedBox(height: 4),
            Text(
              item.status,
              maxLines: 1,
              overflow: TextOverflow.ellipsis,
              style: theme.textTheme.labelMedium?.copyWith(
                color: item.isActive
                    ? Colors.white.withValues(alpha: 0.88)
                    : palette.body,
              ),
            ),
          ],
        ),
      ),
    );
  }
}
