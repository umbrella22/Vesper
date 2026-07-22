import Foundation
internal import VesperPlayerKitBridgeShim
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
