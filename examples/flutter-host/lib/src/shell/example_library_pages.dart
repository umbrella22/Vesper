import 'package:material_ui/material_ui.dart';

import 'example_host_navigation.dart';
import 'example_host_strings.dart';
import 'example_library_catalog.dart';

/// Library home: one card per scenario collection.
class ExampleLibraryHome extends StatelessWidget {
  const ExampleLibraryHome({
    super.key,
    required this.onOpenCollection,
    this.bottomPadding = 16,
  });

  final ValueChanged<ExampleLibraryCollectionId> onOpenCollection;
  final double bottomPadding;

  @override
  Widget build(BuildContext context) {
    final strings = ExampleHostStrings.of(context);
    final theme = Theme.of(context);
    return ListView(
      key: const ValueKey<String>('library-home'),
      padding: EdgeInsets.fromLTRB(16, 8, 16, bottomPadding),
      children: <Widget>[
        Text(
          strings[ExampleHostText.librarySubtitle],
          style: theme.textTheme.bodyMedium?.copyWith(
            color: theme.colorScheme.onSurfaceVariant,
          ),
        ),
        const SizedBox(height: 16),
        Text(
          strings[ExampleHostText.libraryCollections],
          style: theme.textTheme.titleMedium,
        ),
        const SizedBox(height: 8),
        for (final collection in exampleLibraryCollections)
          Padding(
            padding: const EdgeInsets.only(bottom: 12),
            child: _ExampleCollectionCard(
              collection: collection,
              onTap: () => onOpenCollection(collection.id),
            ),
          ),
      ],
    );
  }
}

class _ExampleCollectionCard extends StatelessWidget {
  const _ExampleCollectionCard({required this.collection, required this.onTap});

  final ExampleLibraryCollection collection;
  final VoidCallback onTap;

  @override
  Widget build(BuildContext context) {
    final strings = ExampleHostStrings.of(context);
    final theme = Theme.of(context);
    return Card(
      key: ValueKey<String>('library-collection-${collection.id.name}'),
      clipBehavior: Clip.antiAlias,
      child: InkWell(
        onTap: onTap,
        child: Padding(
          padding: const EdgeInsets.all(16),
          child: Row(
            children: <Widget>[
              Container(
                width: 56,
                height: 56,
                decoration: BoxDecoration(
                  borderRadius: BorderRadius.circular(12),
                  gradient: const LinearGradient(
                    begin: Alignment.topCenter,
                    end: Alignment.bottomCenter,
                    colors: <Color>[Color(0xFF171B33), Color(0xFF0B0D16)],
                  ),
                ),
                child: Icon(
                  _collectionIcon(collection.id),
                  color: const Color(0xFFF4F2EC),
                ),
              ),
              const SizedBox(width: 16),
              Expanded(
                child: Column(
                  crossAxisAlignment: CrossAxisAlignment.start,
                  children: <Widget>[
                    Text(
                      strings[collection.titleKey],
                      style: theme.textTheme.titleMedium?.copyWith(
                        fontWeight: FontWeight.w600,
                      ),
                    ),
                    const SizedBox(height: 2),
                    Text(
                      strings[collection.summaryKey],
                      style: theme.textTheme.bodySmall?.copyWith(
                        color: theme.colorScheme.onSurfaceVariant,
                      ),
                    ),
                    const SizedBox(height: 4),
                    Text(
                      strings.itemCount(collection.itemCount),
                      style: theme.textTheme.labelSmall?.copyWith(
                        color: theme.colorScheme.onSurfaceVariant,
                        fontFeatures: const <FontFeature>[
                          FontFeature.tabularFigures(),
                        ],
                      ),
                    ),
                  ],
                ),
              ),
              Icon(
                Icons.chevron_right_rounded,
                color: theme.colorScheme.onSurfaceVariant,
              ),
            ],
          ),
        ),
      ),
    );
  }
}

/// Collection detail page. Plain collections list their items; the HDR &
/// Dolby collection embeds [dolbyCatalog] (the acceptance checklist).
class ExampleLibraryCollectionDetail extends StatelessWidget {
  const ExampleLibraryCollectionDetail({
    super.key,
    required this.collection,
    required this.dashEnabled,
    required this.onPlayItem,
    this.dolbyCatalog,
    this.bottomPadding = 16,
  });

  final ExampleLibraryCollection collection;
  final bool dashEnabled;
  final ValueChanged<ExampleLibraryItem> onPlayItem;
  final Widget? dolbyCatalog;
  final double bottomPadding;

  @override
  Widget build(BuildContext context) {
    final strings = ExampleHostStrings.of(context);
    final theme = Theme.of(context);
    return ListView(
      key: ValueKey<String>('library-detail-${collection.id.name}'),
      padding: EdgeInsets.fromLTRB(16, 8, 16, bottomPadding),
      children: <Widget>[
        Text(
          strings[collection.summaryKey],
          style: theme.textTheme.bodyMedium?.copyWith(
            color: theme.colorScheme.onSurfaceVariant,
          ),
        ),
        const SizedBox(height: 4),
        Text(
          strings.itemCount(collection.itemCount),
          style: theme.textTheme.labelSmall?.copyWith(
            color: theme.colorScheme.onSurfaceVariant,
          ),
        ),
        const SizedBox(height: 12),
        if (collection.usesDolbyCatalog && dolbyCatalog != null)
          dolbyCatalog!
        else
          Card(
            child: Column(
              children: <Widget>[
                for (final item in collection.items)
                  ListTile(
                    key: ValueKey<String>(
                      'library-item-${collection.id.name}-'
                      '${item.playlistItemId}',
                    ),
                    enabled: !item.requiresDash || dashEnabled,
                    leading: const Icon(Icons.play_circle_outline_rounded),
                    title: Text(strings[item.titleKey]),
                    subtitle: Text(
                      item.requiresDash && !dashEnabled
                          ? strings[ExampleHostText.dashUnavailable]
                          : strings[item.subtitleKey],
                    ),
                    trailing: const Icon(Icons.playlist_add_rounded),
                    onTap: () => onPlayItem(item),
                  ),
              ],
            ),
          ),
      ],
    );
  }
}

IconData _collectionIcon(ExampleLibraryCollectionId id) {
  return switch (id) {
    ExampleLibraryCollectionId.vod => Icons.movie_outlined,
    ExampleLibraryCollectionId.live => Icons.sensors_rounded,
    ExampleLibraryCollectionId.adaptive => Icons.equalizer_rounded,
    ExampleLibraryCollectionId.dolby => Icons.hdr_on_rounded,
  };
}
