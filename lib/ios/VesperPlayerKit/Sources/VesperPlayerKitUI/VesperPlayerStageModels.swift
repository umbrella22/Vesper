import Foundation

public enum VesperPlayerStageSheet: String, Identifiable, Sendable {
    case menu
    case quality
    case audio
    case subtitle
    case speed

    public var id: String { rawValue }
}
