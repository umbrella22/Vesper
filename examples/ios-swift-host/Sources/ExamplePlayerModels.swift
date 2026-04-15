import Foundation
import SwiftUI
import VesperPlayerKit

enum ExamplePlayerSheet: String, Identifiable {
    case menu
    case quality
    case audio
    case subtitle
    case speed

    var id: String { rawValue }
}

enum ExampleThemeMode: String, CaseIterable, Identifiable {
    case system
    case light
    case dark

    var id: String { rawValue }

    var preferredColorScheme: ColorScheme? {
        switch self {
        case .system:
            nil
        case .light:
            .light
        case .dark:
            .dark
        }
    }

    var title: String {
        switch self {
        case .system:
            ExampleI18n.themeSystem
        case .light:
            ExampleI18n.themeLight
        case .dark:
            ExampleI18n.themeDark
        }
    }

    var systemImage: String {
        switch self {
        case .system:
            "circle.lefthalf.filled"
        case .light:
            "sun.max.fill"
        case .dark:
            "moon.fill"
        }
    }
}

enum ExampleResilienceProfile: String, CaseIterable, Identifiable {
    case balanced
    case streaming
    case resilient
    case lowLatency

    var id: String { rawValue }

    var title: String {
        switch self {
        case .balanced:
            ExampleI18n.resilienceBalanced
        case .streaming:
            ExampleI18n.resilienceStreaming
        case .resilient:
            ExampleI18n.resilienceResilient
        case .lowLatency:
            ExampleI18n.resilienceLowLatency
        }
    }

    var subtitle: String {
        switch self {
        case .balanced:
            ExampleI18n.resilienceBalancedSubtitle
        case .streaming:
            ExampleI18n.resilienceStreamingSubtitle
        case .resilient:
            ExampleI18n.resilienceResilientSubtitle
        case .lowLatency:
            ExampleI18n.resilienceLowLatencySubtitle
        }
    }

    var policy: VesperPlaybackResiliencePolicy {
        switch self {
        case .balanced:
            .balanced()
        case .streaming:
            .streaming()
        case .resilient:
            .resilient()
        case .lowLatency:
            .lowLatency()
        }
    }
}

struct ExampleHostPalette {
    let pageTop: Color
    let pageBottom: Color
    let sectionBackground: Color
    let sectionStroke: Color
    let title: Color
    let body: Color
    let fieldBackground: Color
    let fieldText: Color
    let primaryAction: Color
}

func exampleHostPalette(useDarkTheme: Bool) -> ExampleHostPalette {
    if useDarkTheme {
        ExampleHostPalette(
            pageTop: Color(red: 0.047, green: 0.063, blue: 0.098),
            pageBottom: Color(red: 0.023, green: 0.027, blue: 0.043),
            sectionBackground: .white.opacity(0.04),
            sectionStroke: .white.opacity(0.06),
            title: .white,
            body: .white.opacity(0.62),
            fieldBackground: .white.opacity(0.06),
            fieldText: .white,
            primaryAction: Color(red: 0.165, green: 0.545, blue: 1.0)
        )
    } else {
        ExampleHostPalette(
            pageTop: Color(red: 0.972, green: 0.949, blue: 0.918),
            pageBottom: Color(red: 0.949, green: 0.957, blue: 0.976),
            sectionBackground: .white.opacity(0.88),
            sectionStroke: Color.black.opacity(0.06),
            title: Color(red: 0.063, green: 0.082, blue: 0.129),
            body: Color(red: 0.361, green: 0.400, blue: 0.478),
            fieldBackground: Color(red: 0.965, green: 0.969, blue: 0.980),
            fieldText: Color(red: 0.063, green: 0.082, blue: 0.129),
            primaryAction: Color(red: 0.145, green: 0.427, blue: 1.0)
        )
    }
}

struct AbrPreset: Identifiable {
    let id: String
    let title: String
    let subtitle: String
    let policy: VesperAbrPolicy
}

let IOS_HLS_DEMO_URL =
    "https://devstreaming-cdn.apple.com/videos/streaming/examples/img_bipbop_adv_example_ts/master.m3u8"
let IOS_HLS_PLAYLIST_ITEM_ID = "hls-demo"
let IOS_REMOTE_PLAYLIST_ITEM_ID = "custom-remote"
let IOS_LOCAL_PLAYLIST_ITEM_ID = "local-file"

func iosHlsDemoSource() -> VesperPlayerSource {
    VesperPlayerSource.hls(
        url: URL(string: IOS_HLS_DEMO_URL)!,
        label: ExampleI18n.hlsDemoLabel
    )
}

func examplePlaylistQueue(
    playlistItemIds: [String],
    remoteSource: VesperPlayerSource? = nil,
    localSource: VesperPlayerSource? = nil
) -> [VesperPlaylistQueueItem] {
    playlistItemIds.compactMap { itemId in
        switch itemId {
        case IOS_HLS_PLAYLIST_ITEM_ID:
            return VesperPlaylistQueueItem(
                itemId: IOS_HLS_PLAYLIST_ITEM_ID,
                source: iosHlsDemoSource(),
                preloadProfile: VesperPlaylistItemPreloadProfile(
                    expectedMemoryBytes: 256 * 1024,
                    expectedDiskBytes: 512 * 1024,
                    warmupWindowMs: 30_000
                )
            )

        case IOS_LOCAL_PLAYLIST_ITEM_ID:
            guard let localSource else { return nil }
            return VesperPlaylistQueueItem(
                itemId: IOS_LOCAL_PLAYLIST_ITEM_ID,
                source: localSource,
                preloadProfile: VesperPlaylistItemPreloadProfile(
                    expectedMemoryBytes: 128 * 1024
                )
            )

        case IOS_REMOTE_PLAYLIST_ITEM_ID:
            guard let remoteSource else { return nil }
            return VesperPlaylistQueueItem(
                itemId: IOS_REMOTE_PLAYLIST_ITEM_ID,
                source: remoteSource,
                preloadProfile: VesperPlaylistItemPreloadProfile(
                    expectedMemoryBytes: 256 * 1024,
                    expectedDiskBytes: 512 * 1024,
                    warmupWindowMs: 30_000
                )
            )

        default:
            return nil
        }
    }
}

func enqueuePlaylistItem(
    _ playlistItemIds: [String],
    itemId: String
) -> [String] {
    playlistItemIds.filter { existingItemId in
        existingItemId != itemId
    } + [itemId]
}

func examplePlaylistViewportHints(
    queue: [VesperPlaylistQueueItem],
    focusedItemId: String?
) -> [VesperPlaylistViewportHint] {
    guard !queue.isEmpty else {
        return []
    }

    let focusIndex = focusedItemId
        .flatMap { itemId in
            queue.firstIndex(where: { $0.itemId == itemId })
        } ?? 0

    var hints = [
        VesperPlaylistViewportHint(
            itemId: queue[focusIndex].itemId,
            kind: .visible,
            order: 0
        )
    ]

    let remainingIndexes = queue.indices
        .filter { $0 != focusIndex }
        .sorted {
            let leftDistance = abs($0 - focusIndex)
            let rightDistance = abs($1 - focusIndex)
            if leftDistance == rightDistance {
                return $0 < $1
            }
            return leftDistance < rightDistance
        }

    for (offset, index) in remainingIndexes.enumerated() {
        let distance = abs(index - focusIndex)
        hints.append(
            VesperPlaylistViewportHint(
                itemId: queue[index].itemId,
                kind: distance == 1 ? .nearVisible : .prefetchOnly,
                order: UInt32(offset + 1)
            )
        )
    }

    return hints
}

func examplePlaylistSwitchPolicy() -> VesperPlaylistSwitchPolicy {
    VesperPlaylistSwitchPolicy(
        autoAdvance: true,
        repeatMode: .off,
        failureStrategy: .skipToNext
    )
}
