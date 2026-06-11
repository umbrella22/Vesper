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

    public func makeCoordinator() -> Coordinator {
        Coordinator()
    }

    public func makeUIView(context: Context) -> PlayerSurfaceView {
        let view = PlayerSurfaceView()
        context.coordinator.attach(controller: controller, view: view)
        return view
    }

    public func updateUIView(_ uiView: PlayerSurfaceView, context: Context) {
        guard !context.coordinator.isAttached(controller: controller, view: uiView) else {
            return
        }
        context.coordinator.attach(controller: controller, view: uiView)
    }

    public static func dismantleUIView(_ uiView: PlayerSurfaceView, coordinator: Coordinator) {
        coordinator.detach(view: uiView)
    }

    public final class Coordinator {
        private weak var attachedController: VesperPlayerController?
        private weak var attachedView: PlayerSurfaceView?

        @MainActor
        func isAttached(controller: VesperPlayerController, view: PlayerSurfaceView) -> Bool {
            attachedController === controller && attachedView === view
        }

        @MainActor
        func attach(controller: VesperPlayerController, view: PlayerSurfaceView) {
            if let attachedController,
                let attachedView,
                attachedController !== controller || attachedView !== view
            {
                attachedController.detachSurfaceHost(attachedView)
            }
            controller.attachSurfaceHost(view)
            attachedController = controller
            attachedView = view
        }

        @MainActor
        func detach(view: PlayerSurfaceView) {
            if let attachedController {
                attachedController.detachSurfaceHost(view)
            } else {
                view.detachBridgeIfNeeded()
            }
            attachedController = nil
            attachedView = nil
        }
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
        readyForDisplayObservation = playerLayer.observe(
            \.isReadyForDisplay, options: [.initial, .new]
        ) {
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
                layer.pixelFormat = nativeFrameMetalPixelFormat()
                layer.framebufferOnly = false
                layer.wantsExtendedDynamicRangeContent = true
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
        let drawableBounds = CGRect(origin: .zero, size: metalLayer.drawableSize)
        let imageExtent = image.extent
        guard imageExtent.width > 0, imageExtent.height > 0,
            drawableBounds.width > 0, drawableBounds.height > 0
        else {
            retainedPixelBuffer.release()
            completion(false)
            return
        }
        let scale = min(
            drawableBounds.width / imageExtent.width,
            drawableBounds.height / imageExtent.height
        )
        let scaledWidth = imageExtent.width * scale
        let scaledHeight = imageExtent.height * scale
        let offsetX = (drawableBounds.width - scaledWidth) / 2
        let offsetY = (drawableBounds.height - scaledHeight) / 2
        let fittedImage =
            image
            .transformed(
                by: CGAffineTransform(
                    translationX: -imageExtent.origin.x,
                    y: -imageExtent.origin.y
                )
            )
            .transformed(by: CGAffineTransform(scaleX: scale, y: scale))
            .transformed(by: CGAffineTransform(translationX: offsetX, y: offsetY))
        guard let commandBuffer = commandQueue.makeCommandBuffer() else {
            retainedPixelBuffer.release()
            completion(false)
            return
        }
        let clearPass = MTLRenderPassDescriptor()
        clearPass.colorAttachments[0].texture = drawable.texture
        clearPass.colorAttachments[0].loadAction = .clear
        clearPass.colorAttachments[0].storeAction = .store
        clearPass.colorAttachments[0].clearColor = MTLClearColor(
            red: 0, green: 0, blue: 0, alpha: 1)
        guard let clearEncoder = commandBuffer.makeRenderCommandEncoder(descriptor: clearPass)
        else {
            retainedPixelBuffer.release()
            completion(false)
            return
        }
        clearEncoder.endEncoding()
        ciContext.render(
            fittedImage,
            to: drawable.texture,
            commandBuffer: commandBuffer,
            bounds: drawableBounds,
            colorSpace: nativeFrameRenderColorSpace()
        )
        commandBuffer.present(drawable)
        commandBuffer.addCompletedHandler { _ in
            retainedPixelBuffer.release()
        }
        commandBuffer.commit()
        completion(true)
    }

    private func configurePlayerLayer() {
        playerLayer.frame = bounds
        playerLayer.videoGravity = .resizeAspect
        if playerLayer.superlayer == nil {
            layer.addSublayer(playerLayer)
        }
    }

    private func nativeFrameMetalPixelFormat() -> MTLPixelFormat {
        // SDK-managed native-frame presentation is intentionally SDR-only today.
        // HDR and Dolby Vision content should use system playback, where AVPlayer
        // and the system compositor own the extended dynamic range path.
        return .bgra8Unorm
    }

    private func nativeFrameRenderColorSpace() -> CGColorSpace {
        CGColorSpace(name: CGColorSpace.sRGB)
            ?? CGColorSpaceCreateDeviceRGB()
    }
}
