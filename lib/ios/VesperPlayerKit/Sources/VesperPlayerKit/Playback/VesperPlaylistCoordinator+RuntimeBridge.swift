import Foundation
internal import VesperPlayerKitBridgeShim

func duplicatePlaylistCString(_ value: String) -> UnsafePointer<CChar>? {
    let duplicated = strdup(value)
    guard let duplicated else {
        return nil
    }
    return UnsafePointer(duplicated)
}

func freePlaylistCString(_ pointer: UnsafePointer<CChar>?) {
    guard let pointer else {
        return
    }
    free(UnsafeMutableRawPointer(mutating: pointer))
}

func freeRuntimeQueueItems(_ items: inout [VesperRuntimePlaylistQueueItem]) {
    for item in items {
        freePlaylistCString(item.item_id)
        freePlaylistCString(item.source_uri)
    }
    items.removeAll(keepingCapacity: false)
}

func freeRuntimeViewportHints(_ hints: inout [VesperRuntimePlaylistViewportHint]) {
    for hint in hints {
        freePlaylistCString(hint.item_id)
    }
    hints.removeAll(keepingCapacity: false)
}

extension VesperPlaylistConfiguration {
    func toRuntimeBridgePayload() -> VesperRuntimePlaylistConfig {
        VesperRuntimePlaylistConfig(
            playlist_id: duplicatePlaylistCString(playlistId),
            neighbor_previous: UInt32(max(neighborWindow.previous, 0)),
            neighbor_next: UInt32(max(neighborWindow.next, 0)),
            preload_near_visible: UInt32(max(preloadWindow.nearVisible, 0)),
            preload_prefetch_only: UInt32(max(preloadWindow.prefetchOnly, 0)),
            auto_advance: switchPolicy.autoAdvance,
            repeat_mode: VesperRuntimePlaylistRepeatMode(rawValue: switchPolicy.repeatMode.rawValue)
                ?? VesperRuntimePlaylistRepeatModeOff,
            failure_strategy: VesperRuntimePlaylistFailureStrategy(
                rawValue: switchPolicy.failureStrategy.rawValue
            ) ?? VesperRuntimePlaylistFailureStrategySkipToNext
        )
    }
}

extension VesperPlaylistQueueItem {
    func toRuntimeBridgePayload() -> VesperRuntimePlaylistQueueItem {
        VesperRuntimePlaylistQueueItem(
            item_id: duplicatePlaylistCString(itemId),
            source_uri: duplicatePlaylistCString(source.uri),
            expected_memory_bytes: preloadProfile.expectedMemoryBytes,
            expected_disk_bytes: preloadProfile.expectedDiskBytes,
            has_ttl_ms: preloadProfile.ttlMs != nil,
            ttl_ms: preloadProfile.ttlMs ?? 0,
            has_warmup_window_ms: preloadProfile.warmupWindowMs != nil,
            warmup_window_ms: preloadProfile.warmupWindowMs ?? 0
        )
    }
}

extension VesperPlaylistViewportHint {
    func toRuntimeBridgePayload() -> VesperRuntimePlaylistViewportHint {
        VesperRuntimePlaylistViewportHint(
            item_id: duplicatePlaylistCString(itemId),
            kind: VesperRuntimePlaylistViewportHintKind(rawValue: kind.rawValue)
                ?? VesperRuntimePlaylistViewportHintKindHidden,
            order: order
        )
    }
}

extension VesperRuntimePreloadCommandKind {
    static var playlistStart: VesperRuntimePreloadCommandKind {
        VesperRuntimePreloadCommandKindStart
    }

    static var playlistCancel: VesperRuntimePreloadCommandKind {
        VesperRuntimePreloadCommandKindCancel
    }
}

extension VesperRuntimePlaylistRepeatMode {
    init?(rawValue: Int) {
        switch rawValue {
        case 0: self = VesperRuntimePlaylistRepeatModeOff
        case 1: self = VesperRuntimePlaylistRepeatModeOne
        case 2: self = VesperRuntimePlaylistRepeatModeAll
        default: return nil
        }
    }
}

extension VesperRuntimePlaylistFailureStrategy {
    init?(rawValue: Int) {
        switch rawValue {
        case 0: self = VesperRuntimePlaylistFailureStrategyPause
        case 1: self = VesperRuntimePlaylistFailureStrategySkipToNext
        default: return nil
        }
    }
}

extension VesperRuntimePlaylistViewportHintKind {
    init?(rawValue: Int) {
        switch rawValue {
        case 0: self = VesperRuntimePlaylistViewportHintKindVisible
        case 1: self = VesperRuntimePlaylistViewportHintKindNearVisible
        case 2: self = VesperRuntimePlaylistViewportHintKindPrefetchOnly
        case 3: self = VesperRuntimePlaylistViewportHintKindHidden
        default: return nil
        }
    }
}
