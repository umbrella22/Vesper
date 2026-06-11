import Foundation
import VesperPlayerKitBridgeShim
public struct VesperPreloadBudgetPolicy: Equatable {
    public let maxConcurrentTasks: Int?
    public let maxMemoryBytes: Int64?
    public let maxDiskBytes: Int64?
    public let warmupWindowMs: Int64?

    public init(
        maxConcurrentTasks: Int? = nil,
        maxMemoryBytes: Int64? = nil,
        maxDiskBytes: Int64? = nil,
        warmupWindowMs: Int64? = nil
    ) {
        self.maxConcurrentTasks = maxConcurrentTasks
        self.maxMemoryBytes = maxMemoryBytes
        self.maxDiskBytes = maxDiskBytes
        self.warmupWindowMs = warmupWindowMs
    }
}

extension VesperPreloadBudgetPolicy {
    func resolvedForRuntime() -> VesperPreloadBudgetPolicy {
        VesperRuntimePreloadBudgetResolver.resolve(self)
    }

    func toRuntimeBridgePayload() -> VesperRuntimePreloadBudgetPolicy {
        VesperRuntimePreloadBudgetPolicy(
            has_max_concurrent_tasks: maxConcurrentTasks != nil,
            max_concurrent_tasks: UInt32(maxConcurrentTasks ?? 0),
            has_max_memory_bytes: maxMemoryBytes != nil,
            max_memory_bytes: maxMemoryBytes ?? 0,
            has_max_disk_bytes: maxDiskBytes != nil,
            max_disk_bytes: maxDiskBytes ?? 0,
            has_warmup_window_ms: warmupWindowMs != nil,
            warmup_window_ms: warmupWindowMs ?? 0
        )
    }
}

private enum VesperRuntimePreloadBudgetResolver {
    static func resolve(_ policy: VesperPreloadBudgetPolicy) -> VesperPreloadBudgetPolicy {
        var payload = policy.toRuntimeBridgePayload()
        var resolved = VesperRuntimeResolvedPreloadBudgetPolicy()
        let didResolve = withUnsafePointer(to: &payload) { payloadPointer in
            withUnsafeMutablePointer(to: &resolved) { resolvedPointer in
                vesper_runtime_resolve_preload_budget(payloadPointer, resolvedPointer)
            }
        }
        guard didResolve else {
            iosHostLog("linked Rust preload budget resolver failed on iOS; using caller policy")
            return policy
        }

        return VesperPreloadBudgetPolicy(
            maxConcurrentTasks: Int(resolved.max_concurrent_tasks),
            maxMemoryBytes: resolved.max_memory_bytes,
            maxDiskBytes: resolved.max_disk_bytes,
            warmupWindowMs: Int64(min(resolved.warmup_window_ms, UInt64(Int64.max)))
        )
    }
}
