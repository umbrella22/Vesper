import AVKit
import SwiftUI
import VesperPlayerKit

@MainActor
public struct VesperAirPlayRouteButton: UIViewRepresentable {
    @ObservedObject private var controller: VesperPlayerController
    private let prioritizesVideoDevices: Bool
    private let tintColor: UIColor?
    private let activeTintColor: UIColor?

    public init(
        controller: VesperPlayerController,
        prioritizesVideoDevices: Bool = true,
        tintColor: UIColor? = nil,
        activeTintColor: UIColor? = nil
    ) {
        self.controller = controller
        self.prioritizesVideoDevices = prioritizesVideoDevices
        self.tintColor = tintColor
        self.activeTintColor = activeTintColor
    }

    public func makeUIView(context: Context) -> AVRoutePickerView {
        let view = AVRoutePickerView(frame: .zero)
        configure(view)
        return view
    }

    public func updateUIView(_ uiView: AVRoutePickerView, context: Context) {
        configure(uiView)
    }

    private func configure(_ view: AVRoutePickerView) {
        view.backgroundColor = .clear
        view.prioritizesVideoDevices = prioritizesVideoDevices
        _ = controller.routePickerPlayer
        if let tintColor {
            view.tintColor = tintColor
        }
        if let activeTintColor {
            view.activeTintColor = activeTintColor
        }
    }
}
