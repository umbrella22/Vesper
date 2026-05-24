import Flutter
import Foundation

final class DownloadEventStreamHandler: NSObject, FlutterStreamHandler {
    private weak var plugin: VesperPlayerIosPlugin?

    init(plugin: VesperPlayerIosPlugin) {
        self.plugin = plugin
    }

    func onListen(withArguments arguments: Any?, eventSink events: @escaping FlutterEventSink) -> FlutterError? {
        Task { @MainActor [weak plugin] in
            guard let plugin else { return }
            plugin.downloadEventSink = events
            plugin.downloadSessions.values.forEach {
                plugin.emitDownloadSnapshot(for: $0)
                plugin.emitDownloadRuntimeEvents(for: $0)
            }
        }
        return nil
    }

    func onCancel(withArguments arguments: Any?) -> FlutterError? {
        Task { @MainActor [weak plugin] in
            plugin?.downloadEventSink = nil
        }
        return nil
    }
}

