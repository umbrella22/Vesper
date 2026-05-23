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
    private let playerLayer = AVPlayerLayer()
    var onReadyForDisplay: (() -> Void)?

    public override init(frame: CGRect) {
        super.init(frame: frame)
        backgroundColor = UIColor.black
        layer.cornerRadius = 24
        layer.masksToBounds = true
        configurePlayerLayer()
    }

    public required init?(coder: NSCoder) {
        super.init(coder: coder)
        backgroundColor = UIColor.black
        layer.cornerRadius = 24
        layer.masksToBounds = true
        configurePlayerLayer()
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

    public func detachBridgeIfNeeded() {
        attachedPlayer = nil
        clearReadyCallback()
        readyForDisplayObservation = nil
        attach(player: nil)
    }

    private func configurePlayerLayer() {
        playerLayer.frame = bounds
        playerLayer.videoGravity = .resizeAspect
        if playerLayer.superlayer == nil {
            layer.addSublayer(playerLayer)
        }
    }
}
