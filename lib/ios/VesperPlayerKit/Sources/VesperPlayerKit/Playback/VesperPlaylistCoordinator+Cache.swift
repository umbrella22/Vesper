import Foundation

struct PlaylistResolvedCachePolicy {
    let enabled: Bool
    let memoryCapacity: Int
    let diskCapacity: Int

    static let disabled = PlaylistResolvedCachePolicy(
        enabled: false,
        memoryCapacity: 0,
        diskCapacity: 0
    )
}

final class VesperPlaylistSharedUrlCacheCoordinator {
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

func playlistResolvedCachePolicy(_ resolvedPolicy: VesperCachePolicy) -> PlaylistResolvedCachePolicy {
    let maxMemoryBytes = resolvedPolicy.maxMemoryBytes ?? 0
    let maxDiskBytes = resolvedPolicy.maxDiskBytes ?? 0

    return PlaylistResolvedCachePolicy(
        enabled: max(maxMemoryBytes, maxDiskBytes) > 0,
        memoryCapacity: playlistClampToInt(maxMemoryBytes),
        diskCapacity: playlistClampToInt(maxDiskBytes)
    )
}

func playlistClampToInt(_ value: Int64) -> Int {
    guard value > 0 else {
        return 0
    }
    return Int(min(value, Int64(Int.max)))
}
