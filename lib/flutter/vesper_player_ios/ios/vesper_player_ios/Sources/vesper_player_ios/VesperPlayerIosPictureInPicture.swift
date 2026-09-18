import AVKit
import Combine
import Foundation
import VesperPlayerKit

struct FlutterPictureInPictureConfiguration {
    let enabled: Bool
    let autoEnter: Bool
    let preferredAspectRatio: Double?

    init(
        enabled: Bool = true,
        autoEnter: Bool = false,
        preferredAspectRatio: Double? = nil
    ) {
        self.enabled = enabled
        self.autoEnter = autoEnter
        self.preferredAspectRatio = preferredAspectRatio
    }
}

struct VesperIosPictureInPictureError: Error {
    let code: String
    let message: String
    let userMessage: String
    let diagnostics: [String: Any]

    init(
        code: String,
        message: String = "Current playback cannot enter Picture in Picture.",
        userMessage: String = "Current playback cannot enter Picture in Picture.",
        diagnostics: [String: Any] = [:]
    ) {
        self.code = code
        self.message = message
        self.userMessage = userMessage
        self.diagnostics = diagnostics
    }

    func toMap() -> [String: Any] {
        [
            "code": code,
            "message": message,
            "userMessage": userMessage,
            "diagnostics": diagnostics,
        ]
    }
}

@MainActor
protocol VesperIosPictureInPictureController: AnyObject {
    var playerLayer: AVPlayerLayer { get }
    var isPictureInPicturePossible: Bool { get }
    var isPictureInPictureActive: Bool { get }
    var canStartPictureInPictureAutomaticallyFromInline: Bool { get set }
    var delegate: AVPictureInPictureControllerDelegate? { get set }
    func startPictureInPicture()
    func stopPictureInPicture()
    func invalidateContentSource()
    func observePossibility(_ changed: @escaping (Bool) -> Void) -> AnyCancellable
}

extension AVPictureInPictureController: VesperIosPictureInPictureController {
    func invalidateContentSource() { contentSource = nil }

    func observePossibility(_ changed: @escaping (Bool) -> Void) -> AnyCancellable {
        publisher(for: \.isPictureInPicturePossible, options: [.initial, .new])
            .receive(on: RunLoop.main)
            .sink(receiveValue: changed)
    }
}

@MainActor
final class VesperIosPictureInPictureCoordinator: NSObject, AVPictureInPictureControllerDelegate {
    private weak var plugin: VesperPlayerIosPlugin?
    private weak var session: PlayerSession?
    private var controller: (any VesperIosPictureInPictureController)?
    private let makeController: (AVPlayerLayer) -> (any VesperIosPictureInPictureController)?
    private var possibleObservation: AnyCancellable?
    private var startGeneration: UInt64 = 0
    private var disabledStopInFlight = false

    init(
        plugin: VesperPlayerIosPlugin,
        session: PlayerSession,
        makeController: @escaping (AVPlayerLayer) -> (any VesperIosPictureInPictureController)? = { layer in
            guard AVPictureInPictureController.isPictureInPictureSupported() else { return nil }
            return AVPictureInPictureController(playerLayer: layer)
        }
    ) {
        self.plugin = plugin
        self.session = session
        self.makeController = makeController
    }

    func configure(with layer: AVPlayerLayer, createIfNeeded: Bool = true) -> Bool {
        guard session?.pictureInPictureConfiguration.enabled == true else {
            disable()
            return false
        }
        // Re-enabling during an outstanding stop must not reuse the retiring controller.
        if disabledStopInFlight { reset() }
        if let current = controller, current.playerLayer === layer {
            let autoEnter = session?.pictureInPictureConfiguration.autoEnter == true
            if current.canStartPictureInPictureAutomaticallyFromInline && !autoEnter
                && !current.isPictureInPictureActive
                && session?.pictureInPictureState != "entering"
                && session?.pictureInPictureState != "exiting" {
                // An already stopped window can remain eligible for background
                // re-entry. Retire it when automatic entry is turned off later.
                reset()
            } else {
                current.canStartPictureInPictureAutomaticallyFromInline = autoEnter
                return true
            }
        }
        reset()
        // A new controller can inherit AVKit's background eligibility from the
        // same player. Manual-only playback creates one only for an explicit request.
        guard createIfNeeded else { return false }
        guard let next = makeController(layer) else {
            return false
        }
        next.canStartPictureInPictureAutomaticallyFromInline =
            session?.pictureInPictureConfiguration.enabled == true &&
            session?.pictureInPictureConfiguration.autoEnter == true
        next.delegate = self
        controller = next
        possibleObservation = nil
        return true
    }

    func start() {
        guard let controller else { return }
        cancelPendingStart()
        let generation = startGeneration
        possibleObservation = controller.observePossibility { [weak self, weak controller] possible in
            guard possible else { return }
            Task { @MainActor [weak self, weak controller] in
                guard let self, let controller,
                      self.startGeneration == generation,
                      self.controller === controller,
                      controller.isPictureInPicturePossible,
                      self.session?.pictureInPictureConfiguration.enabled == true else { return }
                self.cancelPendingStart()
                controller.startPictureInPicture()
            }
        }
    }

    private func cancelPendingStart() {
        startGeneration &+= 1
        possibleObservation = nil
    }

    func stop() {
        let wasActive = isActive
        cancelPendingStart()
        controller?.stopPictureInPicture()
        if !wasActive { publishInactive() }
    }

    private func disable() {
        cancelPendingStart()
        controller?.canStartPictureInPictureAutomaticallyFromInline = false
        guard !disabledStopInFlight else { return }
        if isActive {
            // Keep the delegate until AVKit confirms that the window has closed.
            disabledStopInFlight = true
            stop()
        } else {
            reset()
        }
    }

    func reset() {
        cancelPendingStart()
        disabledStopInFlight = false
        let previous = controller
        controller = nil
        previous?.canStartPictureInPictureAutomaticallyFromInline = false
        previous?.delegate = nil
        if previous?.isPictureInPictureActive == true {
            previous?.stopPictureInPicture()
        }
        // Turning off automatic inline entry alone can leave AVKit eligible to
        // restart a previously active window on the next background transition.
        previous?.invalidateContentSource()
        publishInactive()
    }

    private func publishInactive() {
        guard let plugin, let session,
              session.pictureInPictureState == "entering"
                || session.pictureInPictureState == "exiting"
                || session.pictureInPictureActive else { return }
        session.pictureInPictureState = "inactive"
        session.pictureInPictureActive = false
        plugin.emitPictureInPictureEvent(for: session)
    }

    var isActive: Bool {
        controller?.isPictureInPictureActive == true
    }

    func pictureInPictureControllerWillStartPictureInPicture(
        _ pictureInPictureController: AVPictureInPictureController
    ) {
        guard controller === pictureInPictureController, let plugin, let session else { return }
        session.pictureInPictureState = "entering"
        session.pictureInPictureActive = false
        plugin.emitPictureInPictureEvent(for: session)
    }

    func pictureInPictureControllerDidStartPictureInPicture(
        _ pictureInPictureController: AVPictureInPictureController
    ) {
        guard controller === pictureInPictureController, let plugin, let session else { return }
        session.pictureInPictureState = "active"
        session.pictureInPictureActive = true
        plugin.emitPictureInPictureEvent(for: session)
    }

    func pictureInPictureController(
        _ pictureInPictureController: AVPictureInPictureController,
        failedToStartPictureInPictureWithError error: Error
    ) {
        guard controller === pictureInPictureController, let plugin, let session else { return }
        cancelPendingStart()
        let pipError = VesperIosPictureInPictureError(
            code: "pictureInPicturePlatformRequestRejected",
            message: error.localizedDescription,
            diagnostics: ["exception": String(describing: type(of: error))]
        )
        session.pictureInPictureState = "failed"
        session.pictureInPictureActive = false
        plugin.emitPictureInPictureEvent(for: session, error: pipError)
    }

    func pictureInPictureControllerWillStopPictureInPicture(
        _ pictureInPictureController: AVPictureInPictureController
    ) {
        guard controller === pictureInPictureController, let plugin, let session else { return }
        session.pictureInPictureState = "exiting"
        plugin.emitPictureInPictureEvent(for: session)
    }

    func pictureInPictureControllerDidStopPictureInPicture(
        _ pictureInPictureController: AVPictureInPictureController
    ) {
        guard controller === pictureInPictureController, let plugin, let session else { return }
        cancelPendingStart()
        if disabledStopInFlight || session.pictureInPictureConfiguration.autoEnter == false {
            reset()
            return
        }
        session.pictureInPictureState = "inactive"
        session.pictureInPictureActive = false
        plugin.emitPictureInPictureEvent(for: session)
    }

    func pictureInPictureController(
        _ pictureInPictureController: AVPictureInPictureController,
        restoreUserInterfaceForPictureInPictureStopWithCompletionHandler
            completionHandler: @escaping (Bool) -> Void
    ) {
        completionHandler(controller === pictureInPictureController)
    }
}

extension Dictionary where Key == String, Value == Any {
    func toPictureInPictureConfiguration() -> FlutterPictureInPictureConfiguration {
        FlutterPictureInPictureConfiguration(
            enabled: self["enabled"] as? Bool ?? true,
            autoEnter: self["autoEnter"] as? Bool ?? false,
            preferredAspectRatio: (self["preferredAspectRatio"] as? NSNumber)?.doubleValue
        )
    }
}
