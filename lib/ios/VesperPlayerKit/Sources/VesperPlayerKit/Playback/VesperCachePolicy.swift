import Foundation
internal import VesperPlayerKitBridgeShim
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
