import Foundation
import VesperPlayerKitBridgeShim
public enum VesperRetryBackoff: String {
    case fixed
    case linear
    case exponential
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
