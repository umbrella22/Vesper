import SwiftUI

/// Top-level tabs of the example host shell, in display order.
enum ExampleHostTab: String, CaseIterable, Identifiable, Hashable {
    case play
    case library
    case downloads
    case lab

    var id: String { rawValue }

    var title: String {
        switch self {
        case .play: return ExampleI18n.tabPlay
        case .library: return ExampleI18n.tabLibrary
        case .downloads: return ExampleI18n.tabDownloads
        case .lab: return ExampleI18n.tabLab
        }
    }

    var systemImage: String {
        switch self {
        case .play: return "play.rectangle.fill"
        case .library: return "square.stack.fill"
        case .downloads: return "arrow.down.circle.fill"
        case .lab: return "flask.fill"
        }
    }
}

/// Host-owned sheets. Track and speed selection stay on `ExamplePlayerSheet`.
enum ExampleShellSheet: String, Identifiable {
    case openMedia
    case settings
    case newDownload

    var id: String { rawValue }
}

/// Library collections that group the example-owned sample sources.
enum ExampleLibraryCollection: String, CaseIterable, Identifiable, Hashable {
    case vod
    case adaptive
    case dolby
    case live

    var id: String { rawValue }

    var title: String { ExampleI18n.libraryCollectionTitle(self) }
    var summary: String { ExampleI18n.libraryCollectionSummary(self) }

    var systemImage: String {
        switch self {
        case .vod: return "film"
        case .adaptive: return "waveform.path.ecg"
        case .dolby: return "sun.max"
        case .live: return "dot.radiowaves.left.and.right"
        }
    }

    /// Dolby collections render the acceptance catalog with its filters.
    var isDolbyAcceptance: Bool { self == .dolby }

    /// Playlist item ids that a plain collection can start directly.
    var playlistItemIds: [String] {
        switch self {
        case .vod: return [IOS_HLS_PLAYLIST_ITEM_ID]
        case .adaptive: return [IOS_HLS_PLAYLIST_ITEM_ID, IOS_DASH_PLAYLIST_ITEM_ID]
        case .dolby: return []
        case .live: return [IOS_LIVE_DVR_PLAYLIST_ITEM_ID]
        }
    }
}

struct ExampleLibraryItem: Identifiable, Equatable {
    let id: String
    let title: String
    let subtitle: String
}

func exampleLibraryItem(playlistItemId: String) -> ExampleLibraryItem? {
    switch playlistItemId {
    case IOS_HLS_PLAYLIST_ITEM_ID:
        return ExampleLibraryItem(id: playlistItemId, title: ExampleI18n.hlsDemoLabel, subtitle: "HLS · VOD")
    case IOS_DASH_PLAYLIST_ITEM_ID:
        return ExampleLibraryItem(id: playlistItemId, title: ExampleI18n.dashDemoLabel, subtitle: "DASH · VOD")
    case IOS_LIVE_DVR_PLAYLIST_ITEM_ID:
        return ExampleLibraryItem(
            id: playlistItemId,
            title: ExampleI18n.liveDvrAcceptanceLabel,
            subtitle: "HLS · Live DVR"
        )
    default:
        return nil
    }
}

func exampleLibraryItems(for collection: ExampleLibraryCollection) -> [ExampleLibraryItem] {
    collection.playlistItemIds.compactMap(exampleLibraryItem(playlistItemId:))
}

func exampleLibraryItemCount(
    _ collection: ExampleLibraryCollection,
    dolbyPresets: [ExampleDolbyAcceptancePreset]
) -> Int {
    collection.isDolbyAcceptance ? dolbyPresets.count : exampleLibraryItems(for: collection).count
}

/// Quick samples offered by the Open Media sheet, in display order.
func exampleOpenMediaQuickSamples() -> [ExampleLibraryItem] {
    [IOS_HLS_PLAYLIST_ITEM_ID, IOS_DASH_PLAYLIST_ITEM_ID, IOS_LIVE_DVR_PLAYLIST_ITEM_ID]
        .compactMap(exampleLibraryItem(playlistItemId:))
}

func exampleShellSheetDetents(_ sheet: ExampleShellSheet) -> Set<PresentationDetent> {
    switch sheet {
    case .openMedia, .newDownload, .settings:
        return [.medium, .large]
    }
}

/// The playback-tools root and speed list are short, so they stay at medium.
func examplePlayerSheetDetents(_ sheet: ExamplePlayerSheet) -> Set<PresentationDetent> {
    switch sheet {
    case .menu, .speed:
        return [.medium]
    case .quality, .audio, .subtitle:
        return [.medium, .large]
    }
}

/// The mini player only appears away from the Play tab, while not fullscreen.
func exampleShowsMiniPlayer(
    selectedTab: ExampleHostTab,
    hasActiveSource: Bool,
    isFullscreen: Bool
) -> Bool {
    selectedTab != .play && hasActiveSource && !isFullscreen
}
