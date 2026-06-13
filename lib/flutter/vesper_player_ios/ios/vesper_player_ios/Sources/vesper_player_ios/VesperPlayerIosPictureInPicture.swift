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
final class VesperIosPictureInPictureCoordinator: NSObject, AVPictureInPictureControllerDelegate {
    private weak var plugin: VesperPlayerIosPlugin?
    private weak var session: PlayerSession?
    private var controller: AVPictureInPictureController?
    private var possibleObservation: AnyCancellable?

    init(plugin: VesperPlayerIosPlugin, session: PlayerSession) {
        self.plugin = plugin
        self.session = session
    }

    func configure(with layer: AVPlayerLayer) -> Bool {
        if controller?.playerLayer === layer {
            controller?.canStartPictureInPictureAutomaticallyFromInline =
                session?.pictureInPictureConfiguration.enabled == true &&
                session?.pictureInPictureConfiguration.autoEnter == true
            return true
        }
        guard AVPictureInPictureController.isPictureInPictureSupported() else {
            return false
        }
        guard let next = AVPictureInPictureController(playerLayer: layer) else {
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
        if controller.isPictureInPicturePossible {
            controller.startPictureInPicture()
            return
        }
        possibleObservation = controller
            .publisher(for: \.isPictureInPicturePossible, options: [.new])
            .receive(on: RunLoop.main)
            .sink { [weak self, weak controller] possible in
                guard possible else { return }
                Task { @MainActor [weak self, weak controller] in
                    guard let self, let controller else { return }
                    self.possibleObservation = nil
                    controller.startPictureInPicture()
                }
            }
    }

    func stop() {
        possibleObservation = nil
        controller?.stopPictureInPicture()
    }

    var isActive: Bool {
        controller?.isPictureInPictureActive == true
    }

    func pictureInPictureControllerWillStartPictureInPicture(
        _ pictureInPictureController: AVPictureInPictureController
    ) {
        guard let plugin, let session else { return }
        session.pictureInPictureState = "entering"
        session.pictureInPictureActive = false
        plugin.emitPictureInPictureEvent(for: session)
    }

    func pictureInPictureControllerDidStartPictureInPicture(
        _ pictureInPictureController: AVPictureInPictureController
    ) {
        guard let plugin, let session else { return }
        session.pictureInPictureState = "active"
        session.pictureInPictureActive = true
        plugin.emitPictureInPictureEvent(for: session)
    }

    func pictureInPictureController(
        _ pictureInPictureController: AVPictureInPictureController,
        failedToStartPictureInPictureWithError error: Error
    ) {
        guard let plugin, let session else { return }
        possibleObservation = nil
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
        guard let plugin, let session else { return }
        session.pictureInPictureState = "exiting"
        plugin.emitPictureInPictureEvent(for: session)
    }

    func pictureInPictureControllerDidStopPictureInPicture(
        _ pictureInPictureController: AVPictureInPictureController
    ) {
        guard let plugin, let session else { return }
        possibleObservation = nil
        session.pictureInPictureState = "inactive"
        session.pictureInPictureActive = false
        plugin.emitPictureInPictureEvent(for: session)
    }

    func pictureInPictureController(
        _ pictureInPictureController: AVPictureInPictureController,
        restoreUserInterfaceForPictureInPictureStopWithCompletionHandler
            completionHandler: @escaping (Bool) -> Void
    ) {
        completionHandler(true)
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
