import Foundation
internal import VesperPlayerKitBridgeShim
extension VesperPlayerSourceKind {
    var runtimeBridgeOrdinal: Int32 {
        switch self {
        case .local:
            0
        case .remote:
            1
        }
    }
}

extension VesperPlayerSourceProtocol {
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
        case .rtmp:
            6
        case .rtsp:
            7
        case .flv:
            8
        }
    }
}

extension VesperBufferingPreset {
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

extension VesperRetryBackoff {
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

extension VesperCachePreset {
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

enum VesperRuntimeResilienceResolver {
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
            iosHostLog("linked Rust defaults resolver failed on iOS; using caller resilience policy")
            return policy
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
            "runtime defaults resolver active for source=\(diagnosticURLDescription(source.uri))"
        )
    }
}
