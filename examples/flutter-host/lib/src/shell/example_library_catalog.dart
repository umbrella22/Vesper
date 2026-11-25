import '../player/example_dolby_acceptance_catalog.dart';
import '../player/example_player_models.dart';
import 'example_host_strings.dart';

/// One built-in library entry that maps onto a playlist item id.
final class ExampleLibraryItem {
  const ExampleLibraryItem({
    required this.playlistItemId,
    required this.titleKey,
    required this.subtitleKey,
    this.requiresDash = false,
  });

  final String playlistItemId;
  final ExampleHostText titleKey;
  final ExampleHostText subtitleKey;
  final bool requiresDash;
}

/// A scenario collection shown on the library home page.
///
/// The HDR & Dolby collection renders the Dolby acceptance catalog instead of
/// [items], so its count comes from that catalog.
final class ExampleLibraryCollection {
  const ExampleLibraryCollection({
    required this.id,
    required this.titleKey,
    required this.summaryKey,
    this.items = const <ExampleLibraryItem>[],
  });

  final ExampleLibraryCollectionId id;
  final ExampleHostText titleKey;
  final ExampleHostText summaryKey;
  final List<ExampleLibraryItem> items;

  bool get usesDolbyCatalog => id == ExampleLibraryCollectionId.dolby;

  int get itemCount =>
      usesDolbyCatalog ? exampleDolbyAcceptanceCatalog.length : items.length;
}

const ExampleLibraryItem _hlsItem = ExampleLibraryItem(
  playlistItemId: flutterHlsPlaylistItemId,
  titleKey: ExampleHostText.sampleHlsTitle,
  subtitleKey: ExampleHostText.sampleHlsSubtitle,
);

const ExampleLibraryItem _dashItem = ExampleLibraryItem(
  playlistItemId: flutterDashPlaylistItemId,
  titleKey: ExampleHostText.sampleDashTitle,
  subtitleKey: ExampleHostText.sampleDashSubtitle,
  requiresDash: true,
);

const ExampleLibraryItem _liveDvrItem = ExampleLibraryItem(
  playlistItemId: flutterLiveDvrPlaylistItemId,
  titleKey: ExampleHostText.sampleLiveDvrTitle,
  subtitleKey: ExampleHostText.sampleLiveDvrSubtitle,
);

/// Quick samples offered by the open-media sheet, in display order.
const List<ExampleLibraryItem> exampleOpenMediaSamples = <ExampleLibraryItem>[
  _hlsItem,
  _dashItem,
  _liveDvrItem,
];

const List<ExampleLibraryCollection> exampleLibraryCollections =
    <ExampleLibraryCollection>[
      ExampleLibraryCollection(
        id: ExampleLibraryCollectionId.vod,
        titleKey: ExampleHostText.collectionVodTitle,
        summaryKey: ExampleHostText.collectionVodSummary,
        items: <ExampleLibraryItem>[_hlsItem],
      ),
      ExampleLibraryCollection(
        id: ExampleLibraryCollectionId.live,
        titleKey: ExampleHostText.collectionLiveTitle,
        summaryKey: ExampleHostText.collectionLiveSummary,
        items: <ExampleLibraryItem>[_liveDvrItem],
      ),
      ExampleLibraryCollection(
        id: ExampleLibraryCollectionId.adaptive,
        titleKey: ExampleHostText.collectionAdaptiveTitle,
        summaryKey: ExampleHostText.collectionAdaptiveSummary,
        items: <ExampleLibraryItem>[_hlsItem, _dashItem],
      ),
      ExampleLibraryCollection(
        id: ExampleLibraryCollectionId.dolby,
        titleKey: ExampleHostText.collectionDolbyTitle,
        summaryKey: ExampleHostText.collectionDolbySummary,
      ),
    ];

ExampleLibraryCollection exampleLibraryCollectionById(
  ExampleLibraryCollectionId id,
) {
  return exampleLibraryCollections.firstWhere(
    (collection) => collection.id == id,
  );
}
