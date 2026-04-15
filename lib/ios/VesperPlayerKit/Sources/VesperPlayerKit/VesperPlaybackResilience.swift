import Foundation
import VesperPlayerKitBridgeShim

public enum VesperBufferingPreset: String {
    case `default`
    case balanced
    case streaming
    case resilient
    case lowLatency
}

private struct VesperBufferingPresetDefaults {
    let minBufferMs: Int64
    let maxBufferMs: Int64
    let bufferForPlaybackMs: Int64
    let bufferForPlaybackAfterRebufferMs: Int64
}

public struct VesperBufferingPolicy: Equatable {
    public let preset: VesperBufferingPreset
    private let rawMinBufferMs: Int64?
    private let rawMaxBufferMs: Int64?
    private let rawBufferForPlaybackMs: Int64?
    private let rawBufferForPlaybackAfterRebufferMs: Int64?

    public var minBufferMs: Int64? {
        rawMinBufferMs ?? Self.defaults(for: preset)?.minBufferMs
    }

    public var maxBufferMs: Int64? {
        rawMaxBufferMs ?? Self.defaults(for: preset)?.maxBufferMs
    }

    public var bufferForPlaybackMs: Int64? {
        rawBufferForPlaybackMs ?? Self.defaults(for: preset)?.bufferForPlaybackMs
    }

    public var bufferForPlaybackAfterRebufferMs: Int64? {
        rawBufferForPlaybackAfterRebufferMs
            ?? Self.defaults(for: preset)?.bufferForPlaybackAfterRebufferMs
    }

    public init(
        preset: VesperBufferingPreset = .default,
        minBufferMs: Int64? = nil,
        maxBufferMs: Int64? = nil,
        bufferForPlaybackMs: Int64? = nil,
        bufferForPlaybackAfterRebufferMs: Int64? = nil
    ) {
        self.preset = preset
        rawMinBufferMs = minBufferMs
        rawMaxBufferMs = maxBufferMs
        rawBufferForPlaybackMs = bufferForPlaybackMs
        rawBufferForPlaybackAfterRebufferMs = bufferForPlaybackAfterRebufferMs
    }

    public static func == (lhs: VesperBufferingPolicy, rhs: VesperBufferingPolicy) -> Bool {
        lhs.preset == rhs.preset
            && lhs.minBufferMs == rhs.minBufferMs
            && lhs.maxBufferMs == rhs.maxBufferMs
            && lhs.bufferForPlaybackMs == rhs.bufferForPlaybackMs
            && lhs.bufferForPlaybackAfterRebufferMs == rhs.bufferForPlaybackAfterRebufferMs
    }

    public static func balanced() -> VesperBufferingPolicy {
        VesperBufferingPolicy(preset: .balanced)
    }

    public static func streaming() -> VesperBufferingPolicy {
        VesperBufferingPolicy(preset: .streaming)
    }

    public static func resilient() -> VesperBufferingPolicy {
        VesperBufferingPolicy(preset: .resilient)
    }

    public static func lowLatency() -> VesperBufferingPolicy {
        VesperBufferingPolicy(preset: .lowLatency)
    }

    func toRuntimeBridgePayload() -> VesperRuntimeBufferingPolicy {
        VesperRuntimeBufferingPolicy(
            preset_ordinal: preset.runtimeBridgeOrdinal,
            has_min_buffer_ms: rawMinBufferMs != nil,
            min_buffer_ms: rawMinBufferMs ?? 0,
            has_max_buffer_ms: rawMaxBufferMs != nil,
            max_buffer_ms: rawMaxBufferMs ?? 0,
            has_buffer_for_playback_ms: rawBufferForPlaybackMs != nil,
            buffer_for_playback_ms: rawBufferForPlaybackMs ?? 0,
            has_buffer_for_rebuffer_ms: rawBufferForPlaybackAfterRebufferMs != nil,
            buffer_for_rebuffer_ms: rawBufferForPlaybackAfterRebufferMs ?? 0
        )
    }

    private static func defaults(for preset: VesperBufferingPreset) -> VesperBufferingPresetDefaults? {
        switch preset {
        case .default:
            nil
        case .balanced:
            VesperBufferingPresetDefaults(
                minBufferMs: 10_000,
                maxBufferMs: 30_000,
                bufferForPlaybackMs: 1_000,
                bufferForPlaybackAfterRebufferMs: 2_000
            )
        case .streaming:
            VesperBufferingPresetDefaults(
                minBufferMs: 12_000,
                maxBufferMs: 36_000,
                bufferForPlaybackMs: 1_200,
                bufferForPlaybackAfterRebufferMs: 2_500
            )
        case .resilient:
            VesperBufferingPresetDefaults(
                minBufferMs: 20_000,
                maxBufferMs: 50_000,
                bufferForPlaybackMs: 1_500,
                bufferForPlaybackAfterRebufferMs: 3_000
            )
        case .lowLatency:
            VesperBufferingPresetDefaults(
                minBufferMs: 4_000,
                maxBufferMs: 12_000,
                bufferForPlaybackMs: 500,
                bufferForPlaybackAfterRebufferMs: 1_000
            )
        }
    }
}

public enum VesperRetryBackoff: String {
    case fixed
    case linear
    case exponential
}

public enum VesperCachePreset: String {
    case `default`
    case disabled
    case streaming
    case resilient
}

private struct VesperCachePresetDefaults {
    let maxMemoryBytes: Int64
    let maxDiskBytes: Int64
}

public struct VesperRetryPolicy: Equatable {
    private let usesDefaultMaxAttempts: Bool
    private let rawMaxAttempts: Int?
    private let rawBaseDelayMs: UInt64?
    private let rawMaxDelayMs: UInt64?
    private let rawBackoff: VesperRetryBackoff?

    public var maxAttempts: Int? {
        usesDefaultMaxAttempts ? 3 : rawMaxAttempts
    }

    public var baseDelayMs: UInt64 {
        rawBaseDelayMs ?? 1_000
    }

    public var maxDelayMs: UInt64 {
        rawMaxDelayMs ?? 5_000
    }

    public var backoff: VesperRetryBackoff {
        rawBackoff ?? .linear
    }

    public init(
        maxAttempts: Int? = 3,
        baseDelayMs: UInt64? = nil,
        maxDelayMs: UInt64? = nil,
        backoff: VesperRetryBackoff? = nil
    ) {
        usesDefaultMaxAttempts = maxAttempts == 3
        rawMaxAttempts = maxAttempts == 3 ? nil : maxAttempts
        rawBaseDelayMs = baseDelayMs
        rawMaxDelayMs = maxDelayMs
        rawBackoff = backoff
    }

    public static func aggressive() -> VesperRetryPolicy {
        VesperRetryPolicy(
            maxAttempts: 2,
            baseDelayMs: 500,
            maxDelayMs: 2_000,
            backoff: .fixed
        )
    }

    public static func resilient() -> VesperRetryPolicy {
        VesperRetryPolicy(
            maxAttempts: 6,
            baseDelayMs: 1_000,
            maxDelayMs: 8_000,
            backoff: .exponential
        )
    }

    func toRuntimeBridgePayload() -> VesperRuntimeRetryPolicy {
        VesperRuntimeRetryPolicy(
            uses_default_max_attempts: usesDefaultMaxAttempts,
            has_max_attempts: rawMaxAttempts != nil,
            max_attempts: Int32(rawMaxAttempts ?? 0),
            has_base_delay_ms: rawBaseDelayMs != nil,
            base_delay_ms: rawBaseDelayMs ?? 0,
            has_max_delay_ms: rawMaxDelayMs != nil,
            max_delay_ms: rawMaxDelayMs ?? 0,
            has_backoff: rawBackoff != nil,
            backoff_ordinal: (rawBackoff ?? .linear).runtimeBridgeOrdinal
        )
    }
}

public struct VesperCachePolicy: Equatable {
    public let preset: VesperCachePreset
    private let rawMaxMemoryBytes: Int64?
    private let rawMaxDiskBytes: Int64?

    public var maxMemoryBytes: Int64? {
        rawMaxMemoryBytes ?? Self.defaults(for: preset)?.maxMemoryBytes
    }

    public var maxDiskBytes: Int64? {
        rawMaxDiskBytes ?? Self.defaults(for: preset)?.maxDiskBytes
    }

    public init(
        preset: VesperCachePreset = .default,
        maxMemoryBytes: Int64? = nil,
        maxDiskBytes: Int64? = nil
    ) {
        self.preset = preset
        rawMaxMemoryBytes = maxMemoryBytes
        rawMaxDiskBytes = maxDiskBytes
    }

    public static func == (lhs: VesperCachePolicy, rhs: VesperCachePolicy) -> Bool {
        lhs.preset == rhs.preset
            && lhs.maxMemoryBytes == rhs.maxMemoryBytes
            && lhs.maxDiskBytes == rhs.maxDiskBytes
    }

    public static func disabled() -> VesperCachePolicy {
        VesperCachePolicy(preset: .disabled)
    }

    public static func streaming() -> VesperCachePolicy {
        VesperCachePolicy(preset: .streaming)
    }

    public static func resilient() -> VesperCachePolicy {
        VesperCachePolicy(preset: .resilient)
    }

    func toRuntimeBridgePayload() -> VesperRuntimeCachePolicy {
        VesperRuntimeCachePolicy(
            preset_ordinal: preset.runtimeBridgeOrdinal,
            has_max_memory_bytes: rawMaxMemoryBytes != nil,
            max_memory_bytes: rawMaxMemoryBytes ?? 0,
            has_max_disk_bytes: rawMaxDiskBytes != nil,
            max_disk_bytes: rawMaxDiskBytes ?? 0
        )
    }

    private static func defaults(for preset: VesperCachePreset) -> VesperCachePresetDefaults? {
        switch preset {
        case .default:
            nil
        case .disabled:
            VesperCachePresetDefaults(
                maxMemoryBytes: 0,
                maxDiskBytes: 0
            )
        case .streaming:
            VesperCachePresetDefaults(
                maxMemoryBytes: 8 * 1024 * 1024,
                maxDiskBytes: 128 * 1024 * 1024
            )
        case .resilient:
            VesperCachePresetDefaults(
                maxMemoryBytes: 16 * 1024 * 1024,
                maxDiskBytes: 384 * 1024 * 1024
            )
        }
    }
}

public struct VesperPlaybackResiliencePolicy: Equatable {
    public let buffering: VesperBufferingPolicy
    public let retry: VesperRetryPolicy
    public let cache: VesperCachePolicy

    public init(
        buffering: VesperBufferingPolicy = VesperBufferingPolicy(),
        retry: VesperRetryPolicy = VesperRetryPolicy(),
        cache: VesperCachePolicy = VesperCachePolicy()
    ) {
        self.buffering = buffering
        self.retry = retry
        self.cache = cache
    }

    public static func balanced() -> VesperPlaybackResiliencePolicy {
        VesperPlaybackResiliencePolicy(
            buffering: .balanced(),
            retry: VesperRetryPolicy(),
            cache: .streaming()
        )
    }

    public static func streaming() -> VesperPlaybackResiliencePolicy {
        VesperPlaybackResiliencePolicy(
            buffering: .streaming(),
            retry: VesperRetryPolicy(),
            cache: .streaming()
        )
    }

    public static func resilient() -> VesperPlaybackResiliencePolicy {
        VesperPlaybackResiliencePolicy(
            buffering: .resilient(),
            retry: .resilient(),
            cache: .resilient()
        )
    }

    public static func lowLatency() -> VesperPlaybackResiliencePolicy {
        VesperPlaybackResiliencePolicy(
            buffering: .lowLatency(),
            retry: .aggressive(),
            cache: .disabled()
        )
    }
}

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
            preconditionFailure(
                "VesperPlayerKit requires the linked Rust preload budget resolver on iOS."
            )
        }

        return VesperPreloadBudgetPolicy(
            maxConcurrentTasks: Int(resolved.max_concurrent_tasks),
            maxMemoryBytes: resolved.max_memory_bytes,
            maxDiskBytes: resolved.max_disk_bytes,
            warmupWindowMs: Int64(min(resolved.warmup_window_ms, UInt64(Int64.max)))
        )
    }
}

private extension VesperPlayerSourceKind {
    var runtimeBridgeOrdinal: Int32 {
        switch self {
        case .local:
            0
        case .remote:
            1
        }
    }
}

private extension VesperPlayerSourceProtocol {
    var runtimeBridgeOrdinal: Int32 {
        switch self {
        case .unknown:
            0
        case .file:
            1
        case .content:
            2
        case .progressive:
            3
        case .hls:
            4
        case .dash:
            5
        }
    }
}

private extension VesperBufferingPreset {
    var runtimeBridgeOrdinal: Int32 {
        switch self {
        case .default:
            0
        case .balanced:
            1
        case .streaming:
            2
        case .resilient:
            3
        case .lowLatency:
            4
        }
    }

    init(runtimeBridgeOrdinal: Int32) {
        switch runtimeBridgeOrdinal {
        case 1:
            self = .balanced
        case 2:
            self = .streaming
        case 3:
            self = .resilient
        case 4:
            self = .lowLatency
        default:
            self = .default
        }
    }
}

private extension VesperRetryBackoff {
    var runtimeBridgeOrdinal: Int32 {
        switch self {
        case .fixed:
            0
        case .linear:
            1
        case .exponential:
            2
        }
    }

    init(runtimeBridgeOrdinal: Int32) {
        switch runtimeBridgeOrdinal {
        case 0:
            self = .fixed
        case 2:
            self = .exponential
        default:
            self = .linear
        }
    }
}

private extension VesperCachePreset {
    var runtimeBridgeOrdinal: Int32 {
        switch self {
        case .default:
            0
        case .disabled:
            1
        case .streaming:
            2
        case .resilient:
            3
        }
    }

    init(runtimeBridgeOrdinal: Int32) {
        switch runtimeBridgeOrdinal {
        case 1:
            self = .disabled
        case 2:
            self = .streaming
        case 3:
            self = .resilient
        default:
            self = .default
        }
    }
}

private enum VesperRuntimeResilienceResolver {
    private static var loggedRuntime = false

    static func resolve(
        source: VesperPlayerSource,
        policy: VesperPlaybackResiliencePolicy
    ) -> VesperPlaybackResiliencePolicy {
        let resolved = resolveWithRuntime(source: source, policy: policy)
        logRuntimeUsageIfNeeded(source: source)
        return resolved
    }

    private static func resolveWithRuntime(
        source: VesperPlayerSource,
        policy: VesperPlaybackResiliencePolicy
    ) -> VesperPlaybackResiliencePolicy {
        var buffering = policy.buffering.toRuntimeBridgePayload()
        var retry = policy.retry.toRuntimeBridgePayload()
        var cache = policy.cache.toRuntimeBridgePayload()
        var resolved = VesperRuntimeResolvedResiliencePolicy(
            buffering: VesperRuntimeBufferingPolicy(),
            retry: VesperRuntimeRetryPolicy(),
            cache: VesperRuntimeCachePolicy()
        )

        let didResolve = withUnsafePointer(to: &buffering) { bufferingPointer in
            withUnsafePointer(to: &retry) { retryPointer in
                withUnsafePointer(to: &cache) { cachePointer in
                    withUnsafeMutablePointer(to: &resolved) { resolvedPointer in
                        vesper_runtime_resolve_resilience_policy(
                            source.kind.runtimeBridgeOrdinal,
                            source.protocol.runtimeBridgeOrdinal,
                            bufferingPointer,
                            retryPointer,
                            cachePointer,
                            resolvedPointer
                        )
                    }
                }
            }
        }
        guard didResolve else {
            preconditionFailure(
                "VesperPlayerKit requires the linked Rust defaults resolver on iOS."
            )
        }

        return VesperPlaybackResiliencePolicy(
            buffering: VesperBufferingPolicy(
                preset: VesperBufferingPreset(
                    runtimeBridgeOrdinal: resolved.buffering.preset_ordinal
                ),
                minBufferMs: resolved.buffering.has_min_buffer_ms
                    ? resolved.buffering.min_buffer_ms
                    : nil,
                maxBufferMs: resolved.buffering.has_max_buffer_ms
                    ? resolved.buffering.max_buffer_ms
                    : nil,
                bufferForPlaybackMs: resolved.buffering.has_buffer_for_playback_ms
                    ? resolved.buffering.buffer_for_playback_ms
                    : nil,
                bufferForPlaybackAfterRebufferMs:
                    resolved.buffering.has_buffer_for_rebuffer_ms
                    ? resolved.buffering.buffer_for_rebuffer_ms
                    : nil
            ),
            retry: VesperRetryPolicy(
                maxAttempts: resolved.retry.has_max_attempts
                    ? Int(resolved.retry.max_attempts)
                    : nil,
                baseDelayMs: resolved.retry.has_base_delay_ms
                    ? resolved.retry.base_delay_ms
                    : nil,
                maxDelayMs: resolved.retry.has_max_delay_ms
                    ? resolved.retry.max_delay_ms
                    : nil,
                backoff: resolved.retry.has_backoff
                    ? VesperRetryBackoff(
                        runtimeBridgeOrdinal: resolved.retry.backoff_ordinal
                    )
                    : nil
            ),
            cache: VesperCachePolicy(
                preset: VesperCachePreset(runtimeBridgeOrdinal: resolved.cache.preset_ordinal),
                maxMemoryBytes: resolved.cache.has_max_memory_bytes
                    ? resolved.cache.max_memory_bytes
                    : nil,
                maxDiskBytes: resolved.cache.has_max_disk_bytes
                    ? resolved.cache.max_disk_bytes
                    : nil
            )
        )
    }

    private static func logRuntimeUsageIfNeeded(source: VesperPlayerSource) {
        guard !loggedRuntime else { return }
        loggedRuntime = true
        iosHostLog(
            "runtime defaults resolver active for source=\(source.uri)"
        )
    }
}

extension VesperPlaybackResiliencePolicy {
    func resolvedForRuntimeSource(_ source: VesperPlayerSource) -> VesperPlaybackResiliencePolicy {
        VesperRuntimeResilienceResolver.resolve(source: source, policy: self)
    }
}
