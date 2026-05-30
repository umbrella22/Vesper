import AVFoundation
import CoreImage
import CoreVideo
import Metal
import QuartzCore
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
    private var metalLayer: CAMetalLayer?
    private var metalDevice: MTLDevice?
    private var metalCommandQueue: MTLCommandQueue?
    private var ciContext: CIContext?
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
        metalLayer?.frame = bounds
        if let metalLayer {
            let scale = window?.screen.scale ?? UIScreen.main.scale
            metalLayer.drawableSize = CGSize(
                width: bounds.width * scale,
                height: bounds.height * scale
            )
        }
    }

    var isReadyForDisplay: Bool {
        playerLayer.isReadyForDisplay
    }

    func clearReadyCallback() {
        onReadyForDisplay = nil
    }

    func attach(player: AVPlayer?) {
        setNativeFramePresentationEnabled(false)
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

    func attachNativeFramePresenter() {
        readyForDisplayObservation = nil
        attachedPlayer = nil
        playerLayer.player = nil
        playerLayer.videoGravity = .resizeAspect
        setNativeFramePresentationEnabled(true)
    }

    public func detachBridgeIfNeeded() {
        attachedPlayer = nil
        clearReadyCallback()
        readyForDisplayObservation = nil
        setNativeFramePresentationEnabled(false)
        attach(player: nil)
    }

    var supportsNativeFrameMetalPresentation: Bool {
        MTLCreateSystemDefaultDevice() != nil
    }

    var nativeFrameMetalLayerHandle: UInt {
        guard let metalLayer else { return 0 }
        return UInt(bitPattern: Unmanaged.passUnretained(metalLayer).toOpaque())
    }

    func setNativeFramePresentationEnabled(_ enabled: Bool) {
        if enabled {
            guard let device = MTLCreateSystemDefaultDevice() else {
                return
            }
            if metalDevice == nil {
                metalDevice = device
                metalCommandQueue = device.makeCommandQueue()
                ciContext = CIContext(mtlDevice: device)
            }
            playerLayer.isHidden = true
            if metalLayer == nil {
                let layer = CAMetalLayer()
                layer.device = device
                layer.pixelFormat = .bgra8Unorm
                layer.framebufferOnly = false
                layer.contentsScale = window?.screen.scale ?? UIScreen.main.scale
                layer.contentsGravity = .resizeAspect
                layer.frame = bounds
                self.layer.addSublayer(layer)
                metalLayer = layer
            }
            metalLayer?.isHidden = false
        } else {
            metalLayer?.isHidden = true
            playerLayer.isHidden = false
        }
    }

    func presentNativeFrame(
        pixelBufferAddress: UInt,
        completion: @escaping (Bool) -> Void
    ) {
        guard
            let metalLayer,
            let commandQueue = metalCommandQueue,
            let ciContext,
            let drawable = metalLayer.nextDrawable(),
            pixelBufferAddress != 0
        else {
            completion(false)
            return
        }

        guard let pointer = UnsafeRawPointer(bitPattern: pixelBufferAddress) else {
            completion(false)
            return
        }
        let pixelBuffer = Unmanaged<CVPixelBuffer>.fromOpaque(pointer).takeUnretainedValue()
        let retainedPixelBuffer = Unmanaged.passRetained(pixelBuffer)
        let image = CIImage(cvPixelBuffer: pixelBuffer)
        let targetRect = AVMakeRect(
            aspectRatio: image.extent.size,
            insideRect: CGRect(origin: .zero, size: metalLayer.drawableSize)
        )
        guard let commandBuffer = commandQueue.makeCommandBuffer() else {
            retainedPixelBuffer.release()
            completion(false)
            return
        }
        ciContext.render(
            image,
            to: drawable.texture,
            commandBuffer: commandBuffer,
            bounds: image.extent,
            colorSpace: CGColorSpaceCreateDeviceRGB()
        )
        // TODO(native-frame v2): honor aspect-fit. `targetRect` computes the
        // aspect-fitted destination, but `ciContext.render` above draws the full
        // pixel buffer across the drawable, so non-source-sized layers stretch the
        // image. Follow-up: render into `targetRect` (letterbox/pillarbox the
        // remainder) or drive layout from the frame's display aspect ratio so the
        // Metal layer matches the source. Tracked for the v2 presenter pass.
        _ = targetRect
        commandBuffer.present(drawable)
        commandBuffer.addCompletedHandler { _ in
            retainedPixelBuffer.release()
            completion(true)
        }
        commandBuffer.commit()
    }

    private func configurePlayerLayer() {
        playerLayer.frame = bounds
        playerLayer.videoGravity = .resizeAspect
        if playerLayer.superlayer == nil {
            layer.addSublayer(playerLayer)
        }
    }
}
