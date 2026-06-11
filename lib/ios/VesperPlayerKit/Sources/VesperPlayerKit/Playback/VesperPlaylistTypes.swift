import Foundation

public enum VesperPlaylistViewportHintKind: Int {
    case visible = 0
    case nearVisible = 1
    case prefetchOnly = 2
    case hidden = 3
}

public enum VesperPlaylistRepeatMode: Int {
    case off = 0
    case one = 1
    case all = 2
}

public enum VesperPlaylistFailureStrategy: Int {
    case pause = 0
    case skipToNext = 1
}

public struct VesperPlaylistNeighborWindow: Equatable {
    public let previous: Int
    public let next: Int

    public init(previous: Int = 1, next: Int = 1) {
        self.previous = previous
        self.next = next
    }
}

public struct VesperPlaylistPreloadWindow: Equatable {
    public let nearVisible: Int
    public let prefetchOnly: Int

    public init(nearVisible: Int = 2, prefetchOnly: Int = 2) {
        self.nearVisible = nearVisible
        self.prefetchOnly = prefetchOnly
    }
}

public struct VesperPlaylistSwitchPolicy: Equatable {
    public let autoAdvance: Bool
    public let repeatMode: VesperPlaylistRepeatMode
    public let failureStrategy: VesperPlaylistFailureStrategy

    public init(
        autoAdvance: Bool = true,
        repeatMode: VesperPlaylistRepeatMode = .off,
        failureStrategy: VesperPlaylistFailureStrategy = .skipToNext
    ) {
        self.autoAdvance = autoAdvance
        self.repeatMode = repeatMode
        self.failureStrategy = failureStrategy
    }
}

public struct VesperPlaylistConfiguration: Equatable {
    public let playlistId: String
    public let neighborWindow: VesperPlaylistNeighborWindow
    public let preloadWindow: VesperPlaylistPreloadWindow
    public let switchPolicy: VesperPlaylistSwitchPolicy

    public init(
        playlistId: String = "ios-host-playlist",
        neighborWindow: VesperPlaylistNeighborWindow = VesperPlaylistNeighborWindow(),
        preloadWindow: VesperPlaylistPreloadWindow = VesperPlaylistPreloadWindow(),
        switchPolicy: VesperPlaylistSwitchPolicy = VesperPlaylistSwitchPolicy()
    ) {
        self.playlistId = playlistId
        self.neighborWindow = neighborWindow
        self.preloadWindow = preloadWindow
        self.switchPolicy = switchPolicy
    }
}

public struct VesperPlaylistItemPreloadProfile: Equatable {
    public let expectedMemoryBytes: UInt64
    public let expectedDiskBytes: UInt64
    public let ttlMs: UInt64?
    public let warmupWindowMs: UInt64?

    public init(
        expectedMemoryBytes: UInt64 = 0,
        expectedDiskBytes: UInt64 = 0,
        ttlMs: UInt64? = nil,
        warmupWindowMs: UInt64? = nil
    ) {
        self.expectedMemoryBytes = expectedMemoryBytes
        self.expectedDiskBytes = expectedDiskBytes
        self.ttlMs = ttlMs
        self.warmupWindowMs = warmupWindowMs
    }
}

public struct VesperPlaylistQueueItem: Equatable {
    public let itemId: String
    public let source: VesperPlayerSource
    public let preloadProfile: VesperPlaylistItemPreloadProfile

    public init(
        itemId: String,
        source: VesperPlayerSource,
        preloadProfile: VesperPlaylistItemPreloadProfile = VesperPlaylistItemPreloadProfile()
    ) {
        self.itemId = itemId
        self.source = source
        self.preloadProfile = preloadProfile
    }
}

public struct VesperPlaylistViewportHint: Equatable {
    public let itemId: String
    public let kind: VesperPlaylistViewportHintKind
    public let order: UInt32

    public init(
        itemId: String,
        kind: VesperPlaylistViewportHintKind,
        order: UInt32 = 0
    ) {
        self.itemId = itemId
        self.kind = kind
        self.order = order
    }
}

public struct VesperPlaylistActiveItem: Equatable {
    public let itemId: String
    public let index: Int

    public init(itemId: String, index: Int) {
        self.itemId = itemId
        self.index = index
    }
}

public struct VesperPlaylistQueueItemState: Equatable {
    public let item: VesperPlaylistQueueItem
    public let index: Int
    public let viewportHint: VesperPlaylistViewportHintKind
    public let isActive: Bool

    public init(
        item: VesperPlaylistQueueItem,
        index: Int,
        viewportHint: VesperPlaylistViewportHintKind,
        isActive: Bool
    ) {
        self.item = item
        self.index = index
        self.viewportHint = viewportHint
        self.isActive = isActive
    }
}

public struct VesperPlaylistSnapshot: Equatable {
    public let playlistId: String
    public let queue: [VesperPlaylistQueueItemState]
    public let activeItem: VesperPlaylistActiveItem?
    public let neighborWindow: VesperPlaylistNeighborWindow
    public let preloadWindow: VesperPlaylistPreloadWindow
    public let switchPolicy: VesperPlaylistSwitchPolicy

    public init(
        playlistId: String,
        queue: [VesperPlaylistQueueItemState],
        activeItem: VesperPlaylistActiveItem?,
        neighborWindow: VesperPlaylistNeighborWindow,
        preloadWindow: VesperPlaylistPreloadWindow,
        switchPolicy: VesperPlaylistSwitchPolicy
    ) {
        self.playlistId = playlistId
        self.queue = queue
        self.activeItem = activeItem
        self.neighborWindow = neighborWindow
        self.preloadWindow = preloadWindow
        self.switchPolicy = switchPolicy
    }
}
