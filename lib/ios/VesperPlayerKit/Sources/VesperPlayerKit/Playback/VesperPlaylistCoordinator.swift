import Combine
import Foundation
@_implementationOnly import VesperPlayerKitBridgeShim

@MainActor
public final class VesperPlaylistCoordinator: ObservableObject {
    @Published public internal(set) var snapshot: VesperPlaylistSnapshot

    let configuration: VesperPlaylistConfiguration
    let cachePolicyToken = UUID()
    var sessionHandle: UInt64 = 0
    var queue: [VesperPlaylistQueueItem] = []
    var viewportHints: [VesperPlaylistViewportHint] = []
    var resiliencePolicy: VesperPlaybackResiliencePolicy
    var warmupTasks: [UInt64: Task<Void, Never>] = [:]

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
            max_concurrent_tasks: encodeRuntimeUInt32(
                resolvedBudget.maxConcurrentTasks,
                field: "maxConcurrentTasks"
            ),
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
            iosHostLog("native playlist session creation failed")
            return
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
}
