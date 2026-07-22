import Foundation
internal import VesperPlayerKitBridgeShim
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
extension VesperPlaybackResiliencePolicy {
    func resolvedForRuntimeSource(_ source: VesperPlayerSource) -> VesperPlaybackResiliencePolicy {
        VesperRuntimeResilienceResolver.resolve(source: source, policy: self)
    }
}
