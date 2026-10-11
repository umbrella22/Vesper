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
    private let onGeometryChanged: ((VesperVideoSurfaceGeometry?) -> Void)?

    public init(
        controller: VesperPlayerController,
        onSurfaceReady: ((PlayerSurfaceView) -> Void)? = nil,
        onSurfaceRemoved: ((PlayerSurfaceView) -> Void)? = nil,
        onGeometryChanged: ((VesperVideoSurfaceGeometry?) -> Void)? = nil
    ) {
        self.controller = controller
        self.onSurfaceReady = onSurfaceReady
        self.onSurfaceRemoved = onSurfaceRemoved
        self.onGeometryChanged = onGeometryChanged
    }

    public func makeCoordinator() -> Coordinator {
        Coordinator(onSurfaceRemoved: onSurfaceRemoved)
    }

    public func makeUIView(context: Context) -> PlayerSurfaceView {
        let view = PlayerSurfaceView()
        context.coordinator.onGeometryChanged = onGeometryChanged
        view.onGeometryChanged = { [weak coordinator = context.coordinator, weak view] geometry in
            guard let view else { return }
            coordinator?.receiveGeometry(geometry, from: view)
        }
        context.coordinator.attach(controller: controller, view: view)
        onSurfaceReady?(view)
        return view
    }

    public func updateUIView(_ uiView: PlayerSurfaceView, context: Context) {
        context.coordinator.onGeometryChanged = onGeometryChanged
        guard !context.coordinator.isRegistered(controller: controller, view: uiView) else {
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

    @MainActor
    public final class Coordinator {
        var onGeometryChanged: ((VesperVideoSurfaceGeometry?) -> Void)?
        private var geometryDelivery = 0

        func receiveGeometry(_ geometry: VesperVideoSurfaceGeometry?, from view: PlayerSurfaceView) {
            geometryDelivery += 1
            let delivery = geometryDelivery
            Task { @MainActor [weak self, weak view] in
                guard let self, let view, self.attachedView === view,
                      self.geometryDelivery == delivery else { return }
                self.onGeometryChanged?(geometry)
            }
        }
        private weak var attachedController: VesperPlayerController?
        private weak var attachedView: PlayerSurfaceView?
        private let onSurfaceRemoved: ((PlayerSurfaceView) -> Void)?

        init(onSurfaceRemoved: ((PlayerSurfaceView) -> Void)? = nil) {
            self.onSurfaceRemoved = onSurfaceRemoved
        }

        @MainActor
        func isRegistered(controller: VesperPlayerController, view: PlayerSurfaceView) -> Bool {
            attachedController === controller && attachedView === view
        }

        @MainActor
        func attach(controller: VesperPlayerController, view: PlayerSurfaceView) {
            if let attachedController,
                let attachedView,
                attachedController !== controller || attachedView !== view
            {
                attachedController.unregisterSurfaceContainer(attachedView)
            }
            controller.registerSurfaceContainer(view)
            attachedController = controller
            attachedView = view
        }

        @MainActor
        func detach(view: PlayerSurfaceView) {
            geometryDelivery += 1
            view.onGeometryChanged = nil
            onGeometryChanged = nil
            if let attachedController {
                attachedController.unregisterSurfaceContainer(view)
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
        "io.github.umbrella22.vesper.player.subtitle-overlay"

    private weak var attachedPlayer: AVPlayer?
    private weak var attachedItem: AVPlayerItem?
    private weak var confirmedReadyPlayer: AVPlayer?
    private weak var confirmedReadyItem: AVPlayerItem?
    private var readyForDisplayObservation: NSKeyValueObservation?
    private var videoRectObservation: NSKeyValueObservation?
    /// The current picture rectangle in this view's local points.
    public private(set) var geometry: VesperVideoSurfaceGeometry?
    public var onGeometryChanged: ((VesperVideoSurfaceGeometry?) -> Void)? {
        didSet { onGeometryChanged?(geometry) }
    }

    private func publishGeometry() {
        let rect = playerLayer.videoRect
        let valid = window != nil && playerLayer.player?.currentItem?.status == .readyToPlay
            && isReadyForDisplay && !isNativeFramePresentationActive
            && bounds.width > 0 && bounds.height > 0 && !rect.isEmpty && !rect.isInfinite && !rect.isNull
        let next = valid ? VesperVideoSurfaceGeometry(
            width: Double(bounds.width), height: Double(bounds.height), contentRect: rect
        ) : nil
        guard next != geometry else { return }
        geometry = next
        onGeometryChanged?(next)
    }
    private var playerLayer = AVPlayerLayer()
    private var bindingGeneration: UInt64 = 0
    private var hasBoundPlayer = false
    private var requiresReadinessReset = false
    private var metalLayer: CAMetalLayer?
    private var metalDevice: MTLDevice?
    private var metalCommandQueue: MTLCommandQueue?
    private var ciContext: CIContext?
    private let subtitleLabel = UILabel()
    var onReadyForDisplay: ((AVPlayer, AVPlayerItem) -> Void)?
    var onOutputPathChanged: (() -> Void)?

    public override init(frame: CGRect) {
        super.init(frame: frame)
        backgroundColor = UIColor.black
        layer.masksToBounds = true
        configurePlayerLayer()
        configureSubtitleLabel()
        configureOutputPathObservation()
    }

    public required init?(coder: NSCoder) {
        super.init(coder: coder)
        backgroundColor = UIColor.black
        layer.masksToBounds = true
        configurePlayerLayer()
        configureSubtitleLabel()
        configureOutputPathObservation()
    }

    // Tests inject one primary layer before any binding. Production retains
    // its original layer so system Picture in Picture references remain valid.
    convenience init(frame: CGRect, playerLayerFactory: () -> AVPlayerLayer) {
        self.init(frame: frame)
        playerLayer.removeFromSuperlayer()
        playerLayer = playerLayerFactory()
        configurePlayerLayer()
    }

    private func configureOutputPathObservation() {
        registerForTraitChanges([UITraitDisplayGamut.self, UITraitDisplayScale.self]) {
            (view: PlayerSurfaceView, _: UITraitCollection) in
            view.onOutputPathChanged?()
        }
        for name in [UIScreen.modeDidChangeNotification, UIScreen.brightnessDidChangeNotification,
                     UIScene.didActivateNotification, UIScene.willDeactivateNotification,
                     UIScene.didDisconnectNotification, AVPlayer.eligibleForHDRPlaybackDidChangeNotification,
                     UIApplication.didBecomeActiveNotification, UIApplication.willResignActiveNotification] {
            NotificationCenter.default.addObserver(self, selector: #selector(outputDisplayChanged), name: name, object: nil)
        }
    }

    @objc private func outputDisplayChanged(_ notification: Notification) {
        guard window != nil else { return }
        if let screen = notification.object as? UIScreen, screen !== window?.screen { return }
        if let scene = notification.object as? UIWindowScene, scene !== window?.windowScene { return }
        onOutputPathChanged?()
    }

    public override func didMoveToWindow() {
        super.didMoveToWindow()
        onOutputPathChanged?()
        publishGeometry()
    }

    public override func layoutSubviews() {
        super.layoutSubviews()
        CATransaction.begin()
        CATransaction.setDisableActions(true)
        playerLayer.frame = bounds
        CATransaction.commit()
        publishGeometry()
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
        !requiresReadinessReset && attachedItem != nil
            && playerLayer.player === attachedPlayer
            && attachedPlayer?.currentItem === attachedItem
            && playerLayer.isReadyForDisplay
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
        let item = player?.currentItem
        if attachedPlayer === player, attachedItem === item, playerLayer.player === player {
            // A bridge can install a callback after this binding became ready.
            deliverReadiness(player: player, item: item, generation: bindingGeneration)
            return
        }
        onOutputPathChanged?()
        bindingGeneration &+= 1
        let generation = bindingGeneration
        readyForDisplayObservation = nil
        videoRectObservation = nil
        // A reused layer can retain its old picture while a replacement loads.
        // Accept initial readiness on the first binding; subsequent bindings
        // need a readiness reset before a true value can describe their item.
        if hasBoundPlayer && playerLayer.isReadyForDisplay {
            requiresReadinessReset = true
        }
        if !playerLayer.isReadyForDisplay { requiresReadinessReset = false }
        // Reattaching the same previously confirmed item can reuse its picture.
        // An unconfirmed replacement never receives this late-attach exception.
        if let player, let item, confirmedReadyPlayer === player, confirmedReadyItem === item {
            requiresReadinessReset = false
        }
        if player != nil { hasBoundPlayer = true }
        attachedPlayer = player
        attachedItem = item
        let readinessObservation = playerLayer.observe(
            \.isReadyForDisplay, options: [.initial, .new]
        ) { [weak self, weak player, weak item] layer, _ in
            guard let self, self.bindingGeneration == generation,
                  self.playerLayer === layer else { return }
            if !layer.isReadyForDisplay { self.requiresReadinessReset = false }
            self.publishGeometry()
            self.deliverReadiness(player: player, item: item, generation: generation)
        }
        guard bindingGeneration == generation else {
            readinessObservation.invalidate()
            return
        }
        readyForDisplayObservation = readinessObservation
        // Observe readiness changes during rebinding, including a synchronous
        // false notification on detach. PiP retains the primary layer identity.
        playerLayer.player = player
        guard bindingGeneration == generation else { return }
        playerLayer.videoGravity = .resizeAspect
        videoRectObservation = playerLayer.observe(\.videoRect, options: [.initial, .new]) { [weak self] _, _ in
            Task { @MainActor [weak self] in
                guard let self, self.bindingGeneration == generation else { return }
                self.publishGeometry()
            }
        }
        publishGeometry()
        deliverReadiness(player: player, item: item, generation: generation)
    }

    private func deliverReadiness(player: AVPlayer?, item: AVPlayerItem?, generation: UInt64) {
        guard bindingGeneration == generation, isReadyForDisplay,
              let player, let item,
              attachedPlayer === player, attachedItem === item,
              player.currentItem === item else { return }
        confirmedReadyPlayer = player
        confirmedReadyItem = item
        onReadyForDisplay?(player, item)
    }

    func attachNativeFramePresenter() {
        onOutputPathChanged?()
        attach(player: nil)
        playerLayer.videoGravity = .resizeAspect
        setNativeFramePresentationEnabled(true)
        publishGeometry()
    }

    public func detachBridgeIfNeeded() {
        clearReadyCallback()
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
        if isNativeFramePresentationActive != enabled { onOutputPathChanged?() }
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
