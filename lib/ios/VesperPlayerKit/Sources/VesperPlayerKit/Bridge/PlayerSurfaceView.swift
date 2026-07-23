import AVFoundation
import CoreImage
import CoreVideo
import Metal
import QuartzCore
import SwiftUI
import UIKit

struct VesperSubtitleOverlayFrameSnapshot: Codable, Equatable {
    let x: Double
    let y: Double
    let width: Double
    let height: Double
}

struct VesperSubtitleOverlaySnapshot: Codable, Equatable {
    let text: String
    let hidden: Bool
    let alpha: Double
    let windowAttached: Bool
    let frame: VesperSubtitleOverlayFrameSnapshot
    let visible: Bool
}

public struct PlayerSurfaceContainer: UIViewRepresentable {
    @ObservedObject public var controller: VesperPlayerController
    private let onSurfaceReady: ((PlayerSurfaceView) -> Void)?
    private let onSurfaceRemoved: ((PlayerSurfaceView) -> Void)?

    public init(
        controller: VesperPlayerController,
        onSurfaceReady: ((PlayerSurfaceView) -> Void)? = nil,
        onSurfaceRemoved: ((PlayerSurfaceView) -> Void)? = nil
    ) {
        self.controller = controller
        self.onSurfaceReady = onSurfaceReady
        self.onSurfaceRemoved = onSurfaceRemoved
    }

    public func makeCoordinator() -> Coordinator {
        Coordinator(onSurfaceRemoved: onSurfaceRemoved)
    }

    public func makeUIView(context: Context) -> PlayerSurfaceView {
        let view = PlayerSurfaceView()
        context.coordinator.attach(controller: controller, view: view)
        onSurfaceReady?(view)
        return view
    }

    public func updateUIView(_ uiView: PlayerSurfaceView, context: Context) {
        guard !context.coordinator.isAttached(controller: controller, view: uiView) else {
            onSurfaceReady?(uiView)
            return
        }
        context.coordinator.attach(controller: controller, view: uiView)
        onSurfaceReady?(uiView)
    }

    public static func dismantleUIView(_ uiView: PlayerSurfaceView, coordinator: Coordinator) {
        coordinator.surfaceRemoved(uiView)
        coordinator.detach(view: uiView)
    }

    public final class Coordinator {
        private weak var attachedController: VesperPlayerController?
        private weak var attachedView: PlayerSurfaceView?
        private let onSurfaceRemoved: ((PlayerSurfaceView) -> Void)?

        init(onSurfaceRemoved: ((PlayerSurfaceView) -> Void)? = nil) {
            self.onSurfaceRemoved = onSurfaceRemoved
        }

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

        @MainActor
        func surfaceRemoved(_ view: PlayerSurfaceView) {
            onSurfaceRemoved?(view)
        }
    }
}

public final class PlayerSurfaceView: UIView {
    static let subtitleOverlayAccessibilityIdentifier =
        "io.github.ikaros.vesper.player.subtitle-overlay"

    private weak var attachedPlayer: AVPlayer?
    private var readyForDisplayObservation: NSKeyValueObservation?
    private let playerLayer = AVPlayerLayer()
    private var metalLayer: CAMetalLayer?
    private var metalDevice: MTLDevice?
    private var metalCommandQueue: MTLCommandQueue?
    private var ciContext: CIContext?
    private let subtitleLabel = UILabel()
    var onReadyForDisplay: (() -> Void)?

    public override init(frame: CGRect) {
        super.init(frame: frame)
        backgroundColor = UIColor.black
        layer.cornerRadius = 24
        layer.masksToBounds = true
        configurePlayerLayer()
        configureSubtitleLabel()
    }

    public required init?(coder: NSCoder) {
        super.init(coder: coder)
        backgroundColor = UIColor.black
        layer.cornerRadius = 24
        layer.masksToBounds = true
        configurePlayerLayer()
        configureSubtitleLabel()
    }

    public override func layoutSubviews() {
        super.layoutSubviews()
        playerLayer.frame = bounds
        metalLayer?.frame = bounds
        let horizontalInset: CGFloat = 24
        let bottomInset: CGFloat = 32
        let maximumWidth = max(bounds.width - horizontalInset * 2, 0)
        let fittingSize = subtitleLabel.sizeThatFits(
            CGSize(width: maximumWidth, height: bounds.height)
        )
        subtitleLabel.frame = CGRect(
            x: horizontalInset,
            y: max(bounds.height - bottomInset - fittingSize.height, 0),
            width: maximumWidth,
            height: fittingSize.height
        )
        bringSubviewToFront(subtitleLabel)
        if let metalLayer {
            let scale = window?.screen.scale ?? UIScreen.main.scale
            metalLayer.drawableSize = CGSize(
                width: bounds.width * scale,
                height: bounds.height * scale
            )
        }
    }

    func updateSubtitleOverlay(text: String, style: VesperSubtitleStyle) {
        subtitleLabel.text = text
        subtitleLabel.font = UIFont.systemFont(ofSize: 18 * CGFloat(style.fontScale), weight: .semibold)
        subtitleLabel.isHidden = !style.visible || text.isEmpty
        setNeedsLayout()
    }

    var subtitleOverlaySnapshot: VesperSubtitleOverlaySnapshot {
        let text = subtitleLabel.text ?? ""
        let frame = subtitleLabel.frame
        let windowAttached = subtitleLabel.window != nil
        return VesperSubtitleOverlaySnapshot(
            text: text,
            hidden: subtitleLabel.isHidden,
            alpha: Double(subtitleLabel.alpha),
            windowAttached: windowAttached,
            frame: VesperSubtitleOverlayFrameSnapshot(
                x: Double(frame.origin.x),
                y: Double(frame.origin.y),
                width: Double(frame.width),
                height: Double(frame.height)
            ),
            visible: !text.isEmpty
                && !subtitleLabel.isHidden
                && subtitleLabel.alpha > 0
                && windowAttached
                && frame.width > 0
                && frame.height > 0
        )
    }

    private func configureSubtitleLabel() {
        subtitleLabel.backgroundColor = UIColor.black.withAlphaComponent(0.55)
        subtitleLabel.textColor = .white
        subtitleLabel.textAlignment = .center
        subtitleLabel.numberOfLines = 0
        subtitleLabel.layer.cornerRadius = 6
        subtitleLabel.layer.masksToBounds = true
        subtitleLabel.accessibilityIdentifier = Self.subtitleOverlayAccessibilityIdentifier
        subtitleLabel.isAccessibilityElement = false
        subtitleLabel.isUserInteractionEnabled = false
        subtitleLabel.isHidden = true
        addSubview(subtitleLabel)
    }

    var isReadyForDisplay: Bool {
        playerLayer.isReadyForDisplay
    }

    /// Returns the AVPlayerLayer that can be handed to system Picture in Picture.
    public var pictureInPicturePlayerLayer: AVPlayerLayer? {
        guard !playerLayer.isHidden, playerLayer.player != nil else {
            return nil
        }
        return playerLayer
    }

    /// Indicates whether the SDK-managed native-frame presenter owns the surface.
    public var isNativeFramePresentationActive: Bool {
        metalLayer?.isHidden == false || playerLayer.isHidden
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
                bringSubviewToFront(subtitleLabel)
            }
            metalLayer?.isHidden = false
        } else {
            metalLayer?.isHidden = true
            playerLayer.isHidden = false
        }
    }

    func presentNativeFrame(pixelBuffer: CVPixelBuffer, completion: @escaping (Bool) -> Void) {
        guard
            let metalLayer,
            let commandQueue = metalCommandQueue,
            let ciContext,
            let drawable = metalLayer.nextDrawable()
        else {
            completion(false)
            return
        }

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
