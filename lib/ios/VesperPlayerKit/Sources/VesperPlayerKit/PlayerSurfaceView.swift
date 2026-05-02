import AVFoundation
import SwiftUI
import UIKit

public struct PlayerSurfaceContainer: UIViewRepresentable {
    @ObservedObject public var controller: VesperPlayerController

    public init(controller: VesperPlayerController) {
        self.controller = controller
    }

    public func makeUIView(context: Context) -> PlayerSurfaceView {
        let view = PlayerSurfaceView()
        controller.attachSurfaceHost(view)
        return view
    }

    public func updateUIView(_ uiView: PlayerSurfaceView, context: Context) {
        controller.attachSurfaceHost(uiView)
    }

    public static func dismantleUIView(_ uiView: PlayerSurfaceView, coordinator: ()) {
        uiView.detachBridgeIfNeeded()
    }
}

public final class PlayerSurfaceView: UIView {
    private weak var attachedPlayer: AVPlayer?
    private var readyForDisplayObservation: NSKeyValueObservation?
    var onReadyForDisplay: (() -> Void)?

    public override init(frame: CGRect) {
        super.init(frame: frame)
        backgroundColor = UIColor.black
        layer.cornerRadius = 24
        layer.masksToBounds = true
    }

    public required init?(coder: NSCoder) {
        fatalError("init(coder:) has not been implemented")
    }

    public override func layoutSubviews() {
        super.layoutSubviews()
        playerLayer.frame = bounds
    }

    var isReadyForDisplay: Bool {
        playerLayer.isReadyForDisplay
    }

    func clearReadyCallback() {
        onReadyForDisplay = nil
    }

    func attach(player: AVPlayer?) {
        if attachedPlayer === player, playerLayer.player === player {
            return
        }
        readyForDisplayObservation = nil
        attachedPlayer = player
        playerLayer.player = player
        playerLayer.videoGravity = .resizeAspect
        readyForDisplayObservation = playerLayer.observe(\.isReadyForDisplay, options: [.initial, .new]) {
            [weak self] layer, _
            in
            guard layer.isReadyForDisplay else { return }
            self?.onReadyForDisplay?()
        }
    }

    func detachBridgeIfNeeded() {
        attachedPlayer = nil
        clearReadyCallback()
        readyForDisplayObservation = nil
        attach(player: nil)
    }

    private var playerLayer: AVPlayerLayer {
        if let existing = layer.sublayers?.compactMap({ $0 as? AVPlayerLayer }).first {
            return existing
        }

        let layer = AVPlayerLayer()
        layer.frame = bounds
        layer.videoGravity = .resizeAspect
        self.layer.addSublayer(layer)
        return layer
    }
}
