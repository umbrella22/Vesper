import Foundation

public enum VesperBufferingPreset: String {
    case `default`
    case balanced
    case streaming
    case resilient
    case lowLatency
}

public struct VesperBufferingPolicy: Equatable {
    public let preset: VesperBufferingPreset
    public let minBufferMs: Int64?
    public let maxBufferMs: Int64?
    public let bufferForPlaybackMs: Int64?
    public let bufferForPlaybackAfterRebufferMs: Int64?

    public init(
        preset: VesperBufferingPreset = .default,
        minBufferMs: Int64? = nil,
        maxBufferMs: Int64? = nil,
        bufferForPlaybackMs: Int64? = nil,
        bufferForPlaybackAfterRebufferMs: Int64? = nil
    ) {
        self.preset = preset
        self.minBufferMs = minBufferMs
        self.maxBufferMs = maxBufferMs
        self.bufferForPlaybackMs = bufferForPlaybackMs
        self.bufferForPlaybackAfterRebufferMs = bufferForPlaybackAfterRebufferMs
    }

    public static func balanced() -> VesperBufferingPolicy {
        VesperBufferingPolicy(
            preset: .balanced,
            minBufferMs: 10_000,
            maxBufferMs: 30_000,
            bufferForPlaybackMs: 1_000,
            bufferForPlaybackAfterRebufferMs: 2_000
        )
    }

    public static func streaming() -> VesperBufferingPolicy {
        VesperBufferingPolicy(
            preset: .streaming,
            minBufferMs: 12_000,
            maxBufferMs: 36_000,
            bufferForPlaybackMs: 1_200,
            bufferForPlaybackAfterRebufferMs: 2_500
        )
    }

    public static func resilient() -> VesperBufferingPolicy {
        VesperBufferingPolicy(
            preset: .resilient,
            minBufferMs: 20_000,
            maxBufferMs: 50_000,
            bufferForPlaybackMs: 1_500,
            bufferForPlaybackAfterRebufferMs: 3_000
        )
    }

    public static func lowLatency() -> VesperBufferingPolicy {
        VesperBufferingPolicy(
            preset: .lowLatency,
            minBufferMs: 4_000,
            maxBufferMs: 12_000,
            bufferForPlaybackMs: 500,
            bufferForPlaybackAfterRebufferMs: 1_000
        )
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

public struct VesperRetryPolicy: Equatable {
    public let maxAttempts: Int?
    public let baseDelayMs: UInt64
    public let maxDelayMs: UInt64
    public let backoff: VesperRetryBackoff

    public init(
        maxAttempts: Int? = 3,
        baseDelayMs: UInt64 = 1_000,
        maxDelayMs: UInt64 = 5_000,
        backoff: VesperRetryBackoff = .linear
    ) {
        self.maxAttempts = maxAttempts
        self.baseDelayMs = baseDelayMs
        self.maxDelayMs = maxDelayMs
        self.backoff = backoff
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
}

public struct VesperCachePolicy: Equatable {
    public let preset: VesperCachePreset
    public let maxMemoryBytes: Int64?
    public let maxDiskBytes: Int64?

    public init(
        preset: VesperCachePreset = .default,
        maxMemoryBytes: Int64? = nil,
        maxDiskBytes: Int64? = nil
    ) {
        self.preset = preset
        self.maxMemoryBytes = maxMemoryBytes
        self.maxDiskBytes = maxDiskBytes
    }

    public static func disabled() -> VesperCachePolicy {
        VesperCachePolicy(
            preset: .disabled,
            maxMemoryBytes: 0,
            maxDiskBytes: 0
        )
    }

    public static func streaming() -> VesperCachePolicy {
        VesperCachePolicy(
            preset: .streaming,
            maxMemoryBytes: 8 * 1024 * 1024,
            maxDiskBytes: 128 * 1024 * 1024
        )
    }

    public static func resilient() -> VesperCachePolicy {
        VesperCachePolicy(
            preset: .resilient,
            maxMemoryBytes: 16 * 1024 * 1024,
            maxDiskBytes: 384 * 1024 * 1024
        )
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
