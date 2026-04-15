import Combine
import Foundation
import VesperPlayerKitBridgeShim

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

@MainActor
public final class VesperPlaylistCoordinator: ObservableObject {
    @Published public private(set) var snapshot: VesperPlaylistSnapshot

    private let configuration: VesperPlaylistConfiguration
    private let cachePolicyToken = UUID()
    private var sessionHandle: UInt64 = 0
    private var queue: [VesperPlaylistQueueItem] = []
    private var viewportHints: [VesperPlaylistViewportHint] = []
    private var resiliencePolicy: VesperPlaybackResiliencePolicy
    private var warmupTasks: [UInt64: Task<Void, Never>] = [:]

    public init(
        configuration: VesperPlaylistConfiguration = VesperPlaylistConfiguration(),
        preloadBudgetPolicy: VesperPreloadBudgetPolicy = VesperPreloadBudgetPolicy(),
        resiliencePolicy: VesperPlaybackResiliencePolicy = VesperPlaybackResiliencePolicy()
    ) {
        self.configuration = configuration
        self.resiliencePolicy = resiliencePolicy
        snapshot = VesperPlaylistSnapshot(
            playlistId: configuration.playlistId,
            queue: [],
            activeItem: nil,
            neighborWindow: configuration.neighborWindow,
            preloadWindow: configuration.preloadWindow,
            switchPolicy: configuration.switchPolicy
        )

        var runtimeConfig = configuration.toRuntimeBridgePayload()
        let resolvedBudget = preloadBudgetPolicy.resolvedForRuntime()
        var runtimeBudget = VesperRuntimeResolvedPreloadBudgetPolicy(
            max_concurrent_tasks: UInt32(max(resolvedBudget.maxConcurrentTasks ?? 0, 0)),
            max_memory_bytes: max(resolvedBudget.maxMemoryBytes ?? 0, 0),
            max_disk_bytes: max(resolvedBudget.maxDiskBytes ?? 0, 0),
            warmup_window_ms: UInt64(max(resolvedBudget.warmupWindowMs ?? 0, 0))
        )
        var handle: UInt64 = 0
        let created = withUnsafePointer(to: &runtimeConfig) { configPointer in
            withUnsafePointer(to: &runtimeBudget) { budgetPointer in
                withUnsafeMutablePointer(to: &handle) { handlePointer in
                    vesper_runtime_playlist_session_create(
                        configPointer,
                        budgetPointer,
                        handlePointer
                    )
                }
            }
        }
        freePlaylistCString(runtimeConfig.playlist_id)
        guard created, handle != 0 else {
            preconditionFailure("native playlist session handle must not be zero")
        }
        sessionHandle = handle
    }

    deinit {
        if sessionHandle != 0 {
            vesper_runtime_playlist_session_dispose(sessionHandle)
        }
    }

    public func dispose() {
        cancelAllWarmups()
        VesperPlaylistSharedUrlCacheCoordinator.shared.remove(token: cachePolicyToken)
        if sessionHandle != 0 {
            vesper_runtime_playlist_session_dispose(sessionHandle)
            sessionHandle = 0
        }
    }

    public func setResiliencePolicy(_ policy: VesperPlaybackResiliencePolicy) {
        resiliencePolicy = policy
    }

    public func replaceQueue(_ queue: [VesperPlaylistQueueItem]) {
        self.queue = queue
        viewportHints = viewportHints.filter { hint in
            queue.contains(where: { $0.itemId == hint.itemId })
        }

        var runtimeQueue = queue.map { $0.toRuntimeBridgePayload() }
        let replaced = runtimeQueue.withUnsafeMutableBufferPointer { buffer in
            vesper_runtime_playlist_session_replace_queue(
                sessionHandle,
                buffer.baseAddress,
                UInt(buffer.count)
            )
        }
        freeRuntimeQueueItems(&runtimeQueue)
        guard replaced else { return }

        refreshSnapshot()
        drainAndApplyPreloadCommands()
    }

    public func updateViewportHints(_ hints: [VesperPlaylistViewportHint]) {
        viewportHints = hints
            .filter { $0.kind != .hidden }
            .filter { hint in queue.contains(where: { $0.itemId == hint.itemId }) }

        var runtimeHints = viewportHints.map { $0.toRuntimeBridgePayload() }
        let updated = runtimeHints.withUnsafeMutableBufferPointer { buffer in
            vesper_runtime_playlist_session_update_viewport_hints(
                sessionHandle,
                buffer.baseAddress,
                UInt(buffer.count)
            )
        }
        freeRuntimeViewportHints(&runtimeHints)
        guard updated else { return }

        refreshSnapshot()
        drainAndApplyPreloadCommands()
    }

    public func clearViewportHints() {
        viewportHints.removeAll()
        guard vesper_runtime_playlist_session_clear_viewport_hints(sessionHandle) else {
            return
        }
        refreshSnapshot()
        drainAndApplyPreloadCommands()
    }

    public func advanceToNext() {
        guard vesper_runtime_playlist_session_advance_to_next(sessionHandle) else {
            return
        }
        refreshSnapshot()
        drainAndApplyPreloadCommands()
    }

    public func advanceToPrevious() {
        guard vesper_runtime_playlist_session_advance_to_previous(sessionHandle) else {
            return
        }
        refreshSnapshot()
        drainAndApplyPreloadCommands()
    }

    public func handlePlaybackCompleted() {
        guard vesper_runtime_playlist_session_handle_playback_completed(sessionHandle) else {
            return
        }
        refreshSnapshot()
        drainAndApplyPreloadCommands()
    }

    public func handlePlaybackFailed() {
        guard vesper_runtime_playlist_session_handle_playback_failed(sessionHandle) else {
            return
        }
        refreshSnapshot()
        drainAndApplyPreloadCommands()
    }

    private func refreshSnapshot() {
        var runtimeActiveItem = VesperRuntimePlaylistActiveItem(item_id: nil, index: 0)
        let hasActive = withUnsafeMutablePointer(to: &runtimeActiveItem) { pointer in
            vesper_runtime_playlist_session_current_active_item(sessionHandle, pointer)
        }

        let activeItem: VesperPlaylistActiveItem?
        if hasActive, let itemIdPointer = runtimeActiveItem.item_id {
            activeItem = VesperPlaylistActiveItem(
                itemId: String(cString: itemIdPointer),
                index: Int(runtimeActiveItem.index)
            )
        } else {
            activeItem = nil
        }
        vesper_runtime_playlist_active_item_free(&runtimeActiveItem)

        let hintByItemId = Dictionary(
            uniqueKeysWithValues: viewportHints.map { ($0.itemId, $0.kind) }
        )
        let activeItemId = activeItem?.itemId

        snapshot = VesperPlaylistSnapshot(
            playlistId: configuration.playlistId,
            queue: queue.enumerated().map { index, item in
                VesperPlaylistQueueItemState(
                    item: item,
                    index: index,
                    viewportHint: hintByItemId[item.itemId] ?? .hidden,
                    isActive: activeItemId == item.itemId
                )
            },
            activeItem: activeItem,
            neighborWindow: configuration.neighborWindow,
            preloadWindow: configuration.preloadWindow,
            switchPolicy: configuration.switchPolicy
        )
    }

    private func drainAndApplyPreloadCommands() {
        var commands = VesperRuntimePreloadCommandList(commands: nil, len: 0)
        guard vesper_runtime_playlist_session_drain_preload_commands(sessionHandle, &commands) else {
            return
        }

        let runtimeCommands: [PlaylistWarmupCommand]
        if let pointer = commands.commands, commands.len > 0 {
            runtimeCommands = Array(UnsafeBufferPointer(start: pointer, count: Int(commands.len)))
                .compactMap(PlaylistWarmupCommand.init)
        } else {
            runtimeCommands = []
        }
        vesper_runtime_preload_command_list_free(&commands)

        for command in runtimeCommands {
            switch command {
            case let .start(task):
                startWarmup(task)
            case let .cancel(taskId):
                cancelWarmup(taskId: taskId)
            }
        }
    }

    private func startWarmup(_ task: PlaylistWarmupTask) {
        cancelWarmup(taskId: task.taskId)
        guard let source = sourceForWarmup(uri: task.sourceUri) else {
            return
        }

        let resolvedResiliencePolicy = resiliencePolicy.resolvedForRuntimeSource(source.source)
        let cachePolicy = playlistResolvedCachePolicy(resolvedResiliencePolicy.cache)
        VesperPlaylistSharedUrlCacheCoordinator.shared.apply(
            policy: cachePolicy,
            token: cachePolicyToken
        )

        let handle = sessionHandle
        warmupTasks[task.taskId] = Task.detached(priority: .utility) {
            var request = URLRequest(url: source.url)
            request.cachePolicy = .returnCacheDataElseLoad
            let clampedWarmupWindowMs = Int64(min(task.warmupWindowMs, UInt64(Int64.max)))
            request.timeoutInterval = TimeInterval(max(clampedWarmupWindowMs, 1_000)) / 1000.0
            let clampedExpectedMemoryBytes = Int64(min(task.expectedMemoryBytes, UInt64(Int64.max)))
            let warmupBytes = max(clampedExpectedMemoryBytes, 1)
            request.setValue("bytes=0-\(max(warmupBytes - 1, 0))", forHTTPHeaderField: "Range")

            do {
                _ = try await URLSession.shared.data(for: request)
                _ = vesper_runtime_playlist_session_complete_preload_task(handle, task.taskId)
            } catch {
                error.localizedDescription.withCString { message in
                    _ = vesper_runtime_playlist_session_fail_preload_task(
                        handle,
                        task.taskId,
                        3,
                        7,
                        false,
                        message
                    )
                }
            }

            _ = await MainActor.run {
                self.warmupTasks.removeValue(forKey: task.taskId)
            }
        }
    }

    private func cancelWarmup(taskId: UInt64) {
        warmupTasks.removeValue(forKey: taskId)?.cancel()
    }

    private func cancelAllWarmups() {
        let tasks = warmupTasks.values
        warmupTasks.removeAll()
        tasks.forEach { $0.cancel() }
    }

    private func sourceForWarmup(uri: String) -> PlaylistWarmupSource? {
        if let source = queue.first(where: { $0.source.uri == uri })?.source,
           let url = URL(string: source.uri)
        {
            return PlaylistWarmupSource(source: source, url: url)
        }

        guard let url = URL(string: uri) else {
            return nil
        }
        if url.isFileURL {
            return PlaylistWarmupSource(source: .localFile(url: url), url: url)
        }
        return PlaylistWarmupSource(source: .remoteUrl(url), url: url)
    }
}

private struct PlaylistWarmupSource {
    let source: VesperPlayerSource
    let url: URL
}

private struct PlaylistWarmupTask {
    let taskId: UInt64
    let sourceUri: String
    let expectedMemoryBytes: UInt64
    let warmupWindowMs: UInt64
}

private enum PlaylistWarmupCommand {
    case start(PlaylistWarmupTask)
    case cancel(UInt64)

    init?(_ command: VesperRuntimePreloadCommand) {
        switch command.kind {
        case .start:
            guard let sourceUri = command.task.source_uri else {
                return nil
            }
            self = .start(
                PlaylistWarmupTask(
                    taskId: command.task.task_id,
                    sourceUri: String(cString: sourceUri),
                    expectedMemoryBytes: command.task.expected_memory_bytes,
                    warmupWindowMs: command.task.warmup_window_ms
                )
            )
        case .cancel:
            self = .cancel(command.task_id)
        default:
            return nil
        }
    }
}

private struct PlaylistResolvedCachePolicy {
    let enabled: Bool
    let memoryCapacity: Int
    let diskCapacity: Int

    static let disabled = PlaylistResolvedCachePolicy(
        enabled: false,
        memoryCapacity: 0,
        diskCapacity: 0
    )
}

private final class VesperPlaylistSharedUrlCacheCoordinator {
    static let shared = VesperPlaylistSharedUrlCacheCoordinator()

    private let lock = NSLock()
    private var baselineMemoryCapacity: Int?
    private var baselineDiskCapacity: Int?
    private var activePolicies: [UUID: PlaylistResolvedCachePolicy] = [:]

    func apply(policy: PlaylistResolvedCachePolicy, token: UUID) {
        lock.lock()
        defer { lock.unlock() }

        captureBaselineIfNeeded()
        activePolicies[token] = policy
        reconfigureSharedCache()
    }

    func remove(token: UUID) {
        lock.lock()
        defer { lock.unlock() }

        captureBaselineIfNeeded()
        activePolicies.removeValue(forKey: token)
        reconfigureSharedCache()
    }

    private func captureBaselineIfNeeded() {
        if baselineMemoryCapacity == nil {
            baselineMemoryCapacity = URLCache.shared.memoryCapacity
        }
        if baselineDiskCapacity == nil {
            baselineDiskCapacity = URLCache.shared.diskCapacity
        }
    }

    private func reconfigureSharedCache() {
        let baselineMemoryCapacity = baselineMemoryCapacity ?? URLCache.shared.memoryCapacity
        let baselineDiskCapacity = baselineDiskCapacity ?? URLCache.shared.diskCapacity
        let enabledPolicies = activePolicies.values.filter(\.enabled)
        let requestedMemoryCapacity = enabledPolicies.map(\.memoryCapacity).max() ?? 0
        let requestedDiskCapacity = enabledPolicies.map(\.diskCapacity).max() ?? 0

        URLCache.shared.memoryCapacity = max(baselineMemoryCapacity, requestedMemoryCapacity)
        URLCache.shared.diskCapacity = max(baselineDiskCapacity, requestedDiskCapacity)
    }
}

private func playlistResolvedCachePolicy(_ resolvedPolicy: VesperCachePolicy) -> PlaylistResolvedCachePolicy {
    let maxMemoryBytes = resolvedPolicy.maxMemoryBytes ?? 0
    let maxDiskBytes = resolvedPolicy.maxDiskBytes ?? 0

    return PlaylistResolvedCachePolicy(
        enabled: max(maxMemoryBytes, maxDiskBytes) > 0,
        memoryCapacity: playlistClampToInt(maxMemoryBytes),
        diskCapacity: playlistClampToInt(maxDiskBytes)
    )
}

private func playlistClampToInt(_ value: Int64) -> Int {
    guard value > 0 else {
        return 0
    }
    return Int(min(value, Int64(Int.max)))
}

private func duplicatePlaylistCString(_ value: String) -> UnsafePointer<CChar>? {
    let duplicated = strdup(value)
    guard let duplicated else {
        return nil
    }
    return UnsafePointer(duplicated)
}

private func freePlaylistCString(_ pointer: UnsafePointer<CChar>?) {
    guard let pointer else {
        return
    }
    free(UnsafeMutableRawPointer(mutating: pointer))
}

private func freeRuntimeQueueItems(_ items: inout [VesperRuntimePlaylistQueueItem]) {
    for item in items {
        freePlaylistCString(item.item_id)
        freePlaylistCString(item.source_uri)
    }
    items.removeAll(keepingCapacity: false)
}

private func freeRuntimeViewportHints(_ hints: inout [VesperRuntimePlaylistViewportHint]) {
    for hint in hints {
        freePlaylistCString(hint.item_id)
    }
    hints.removeAll(keepingCapacity: false)
}

private extension VesperPlaylistConfiguration {
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

private extension VesperPlaylistQueueItem {
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

private extension VesperPlaylistViewportHint {
    func toRuntimeBridgePayload() -> VesperRuntimePlaylistViewportHint {
        VesperRuntimePlaylistViewportHint(
            item_id: duplicatePlaylistCString(itemId),
            kind: VesperRuntimePlaylistViewportHintKind(rawValue: kind.rawValue)
                ?? VesperRuntimePlaylistViewportHintKindHidden,
            order: order
        )
    }
}

private extension VesperRuntimePreloadCommandKind {
    static var start: VesperRuntimePreloadCommandKind {
        VesperRuntimePreloadCommandKindStart
    }

    static var cancel: VesperRuntimePreloadCommandKind {
        VesperRuntimePreloadCommandKindCancel
    }
}

private extension VesperRuntimePlaylistRepeatMode {
    init?(rawValue: Int) {
        switch rawValue {
        case 0: self = VesperRuntimePlaylistRepeatModeOff
        case 1: self = VesperRuntimePlaylistRepeatModeOne
        case 2: self = VesperRuntimePlaylistRepeatModeAll
        default: return nil
        }
    }
}

private extension VesperRuntimePlaylistFailureStrategy {
    init?(rawValue: Int) {
        switch rawValue {
        case 0: self = VesperRuntimePlaylistFailureStrategyPause
        case 1: self = VesperRuntimePlaylistFailureStrategySkipToNext
        default: return nil
        }
    }
}

private extension VesperRuntimePlaylistViewportHintKind {
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
