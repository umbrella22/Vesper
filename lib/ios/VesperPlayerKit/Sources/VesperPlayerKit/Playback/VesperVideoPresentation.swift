import Combine
import CoreGraphics

/// Display dimensions after native rotation and pixel aspect correction.
public struct VesperVideoPresentation: Equatable {
    public let displayWidth: Double
    public let displayHeight: Double
    public var displayAspectRatio: Double { displayWidth / displayHeight }

    init?(size: CGSize) {
        guard size.width.isFinite, size.height.isFinite, size.width > 0, size.height > 0 else { return nil }
        displayWidth = Double(size.width)
        displayHeight = Double(size.height)
    }
}

/// Geometry in local points for one attached playback view.
public struct VesperVideoSurfaceGeometry: Equatable {
    public let width: Double
    public let height: Double
    public let contentRect: CGRect
}

@MainActor
final class VesperVideoPresentationState {
    @Published private(set) var value: VesperVideoPresentation?

    func update(_ value: VesperVideoPresentation?) {
        if self.value != value { self.value = value }
    }
}
