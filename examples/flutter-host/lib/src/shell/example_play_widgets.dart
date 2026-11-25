import 'package:material_ui/material_ui.dart';

import 'example_host_strings.dart';

/// Compact "now playing" card under the stage.
class ExampleNowPlayingCard extends StatelessWidget {
  const ExampleNowPlayingCard({
    super.key,
    required this.title,
    required this.subtitle,
  });

  final String title;
  final String subtitle;

  @override
  Widget build(BuildContext context) {
    final strings = ExampleHostStrings.of(context);
    final theme = Theme.of(context);
    final muted = theme.colorScheme.onSurfaceVariant;
    return Card(
      key: const ValueKey<String>('play-now-playing'),
      child: Padding(
        padding: const EdgeInsets.all(16),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: <Widget>[
            Text(
              strings[ExampleHostText.nowPlaying],
              style: theme.textTheme.labelMedium?.copyWith(color: muted),
            ),
            const SizedBox(height: 6),
            Text(
              title.isEmpty ? strings[ExampleHostText.nothingPlaying] : title,
              maxLines: 1,
              overflow: TextOverflow.ellipsis,
              style: theme.textTheme.titleMedium?.copyWith(
                fontWeight: FontWeight.w600,
              ),
            ),
            if (subtitle.isNotEmpty) ...<Widget>[
              const SizedBox(height: 4),
              Text(
                subtitle,
                maxLines: 2,
                overflow: TextOverflow.ellipsis,
                style: theme.textTheme.bodySmall?.copyWith(color: muted),
              ),
            ],
          ],
        ),
      ),
    );
  }
}

/// One quick action tile. A `null` [onTap] renders it disabled.
final class ExampleQuickAction {
  const ExampleQuickAction({
    required this.id,
    required this.icon,
    required this.label,
    required this.onTap,
  });

  final String id;
  final IconData icon;
  final String label;
  final VoidCallback? onTap;
}

/// Row of quick actions below the now-playing card. Only actions the host or
/// SDK supports on the current platform should be passed in.
class ExampleQuickActionsRow extends StatelessWidget {
  const ExampleQuickActionsRow({super.key, required this.actions});

  final List<ExampleQuickAction> actions;

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    final scheme = theme.colorScheme;
    return Row(
      children: <Widget>[
        for (var index = 0; index < actions.length; index += 1) ...<Widget>[
          if (index > 0) const SizedBox(width: 8),
          Expanded(
            child: Material(
              color: scheme.surfaceContainerHighest,
              borderRadius: BorderRadius.circular(12),
              clipBehavior: Clip.antiAlias,
              child: InkWell(
                key: ValueKey<String>('quick-action-${actions[index].id}'),
                onTap: actions[index].onTap,
                child: ConstrainedBox(
                  constraints: const BoxConstraints(minHeight: 80),
                  child: Padding(
                    padding: const EdgeInsets.symmetric(
                      horizontal: 4,
                      vertical: 8,
                    ),
                    child: Column(
                      mainAxisAlignment: MainAxisAlignment.center,
                      children: <Widget>[
                        Icon(
                          actions[index].icon,
                          color: actions[index].onTap == null
                              ? scheme.onSurface.withValues(alpha: 0.38)
                              : scheme.onSurface,
                        ),
                        const SizedBox(height: 6),
                        Text(
                          actions[index].label,
                          textAlign: TextAlign.center,
                          maxLines: 2,
                          style: theme.textTheme.labelMedium?.copyWith(
                            color: actions[index].onTap == null
                                ? scheme.onSurface.withValues(alpha: 0.38)
                                : scheme.onSurface,
                          ),
                        ),
                      ],
                    ),
                  ),
                ),
              ),
            ),
          ),
        ],
      ],
    );
  }
}

/// Section header used by the shell pages (title + optional trailing action).
class ExampleShellSectionHeader extends StatelessWidget {
  const ExampleShellSectionHeader({
    super.key,
    required this.title,
    this.trailing,
  });

  final String title;
  final Widget? trailing;

  @override
  Widget build(BuildContext context) {
    return ConstrainedBox(
      constraints: const BoxConstraints(minHeight: 40),
      child: Row(
        children: <Widget>[
          Expanded(
            child: Text(
              title,
              style: Theme.of(
                context,
              ).textTheme.titleMedium?.copyWith(fontWeight: FontWeight.w600),
            ),
          ),
          ?trailing,
        ],
      ),
    );
  }
}
