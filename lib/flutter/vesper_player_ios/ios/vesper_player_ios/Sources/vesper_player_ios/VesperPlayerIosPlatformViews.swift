import AVKit
import Flutter
import UIKit
import VesperPlayerKit

final class PlayerViewFactory: NSObject, FlutterPlatformViewFactory {
    private weak var plugin: VesperPlayerIosPlugin?

    init(plugin: VesperPlayerIosPlugin) {
        self.plugin = plugin
    }

    func createArgsCodec() -> FlutterMessageCodec & NSObjectProtocol {
        FlutterStandardMessageCodec.sharedInstance()
    }

    func create(
        withFrame frame: CGRect,
        viewIdentifier viewId: Int64,
        arguments args: Any?
    ) -> FlutterPlatformView {
        let arguments = args as? [String: Any] ?? [:]
        let playerId = arguments["playerId"] as? String
        let hostView = PlayerSurfaceView(frame: frame)
        hostView.isUserInteractionEnabled = false

        if let playerId {
            Task { @MainActor [weak plugin, weak hostView] in
                guard let plugin, let hostView else { return }
                plugin.bindSessionHost(playerId: playerId, host: hostView)
            }
        }

        return PlayerPlatformView(hostView: hostView) { [weak plugin, weak hostView] in
            guard let playerId else { return }
            Task { @MainActor in
                guard let plugin, let hostView else { return }
                plugin.unbindSessionHost(playerId: playerId, host: hostView)
            }
        }
    }
}

final class AirPlayRouteButtonFactory: NSObject, FlutterPlatformViewFactory {
    private weak var plugin: VesperPlayerIosPlugin?

    init(plugin: VesperPlayerIosPlugin) {
        self.plugin = plugin
    }

    func createArgsCodec() -> FlutterMessageCodec & NSObjectProtocol {
        FlutterStandardMessageCodec.sharedInstance()
    }

    func create(
        withFrame frame: CGRect,
        viewIdentifier viewId: Int64,
        arguments args: Any?
    ) -> FlutterPlatformView {
        let arguments = args as? [String: Any] ?? [:]
        let view = AVRoutePickerView(frame: frame)
        view.backgroundColor = .clear
        view.prioritizesVideoDevices = arguments["prioritizesVideoDevices"] as? Bool ?? true
        if let tintColor = (arguments["tintColor"] as? NSNumber)?.uint32Value {
            view.tintColor = UIColor(argb: tintColor)
        }
        if let activeTintColor = (arguments["activeTintColor"] as? NSNumber)?.uint32Value {
            view.activeTintColor = UIColor(argb: activeTintColor)
        }

        let routeView = AirPlayRoutePlatformView(routePickerView: view)
        if let playerId = arguments["playerId"] as? String {
            Task { @MainActor [weak plugin, weak routeView] in
                guard let plugin, let routeView, let session = plugin.sessions[playerId] else { return }
                routeView.bind(controller: session.controller)
            }
        }
        return routeView
    }
}

final class AirPlayRoutePlatformView: NSObject, FlutterPlatformView {
    private let routePickerView: AVRoutePickerView

    init(routePickerView: AVRoutePickerView) {
        self.routePickerView = routePickerView
    }

    @MainActor
    func bind(controller: VesperPlayerController) {
        _ = controller.routePickerPlayer
    }

    func view() -> UIView {
        routePickerView
    }

    func dispose() {}
}

final class PlayerPlatformView: NSObject, FlutterPlatformView {
    private let hostView: PlayerSurfaceView
    private let onDispose: () -> Void

    init(hostView: PlayerSurfaceView, onDispose: @escaping () -> Void) {
        self.hostView = hostView
        self.onDispose = onDispose
    }

    func view() -> UIView {
        hostView
    }

    func dispose() {
        onDispose()
    }
}

