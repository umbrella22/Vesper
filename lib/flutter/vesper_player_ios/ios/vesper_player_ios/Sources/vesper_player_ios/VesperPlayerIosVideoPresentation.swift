import Flutter
import VesperPlayerKit

func flutterVideoPresentationMap(_ value: VesperVideoPresentation?) -> Any {
    guard let value else { return NSNull() }
    return ["displayWidth": value.displayWidth, "displayHeight": value.displayHeight]
}

func flutterVideoGeometryMap(_ value: VesperVideoSurfaceGeometry?) -> Any? {
    guard let value else { return nil }
    return ["width": value.width, "height": value.height, "contentRect": [
        "left": Double(value.contentRect.minX), "top": Double(value.contentRect.minY),
        "width": Double(value.contentRect.width), "height": Double(value.contentRect.height),
    ]]
}

@MainActor
final class VideoGeometryStream: NSObject, @preconcurrency FlutterStreamHandler {
    private let channel: FlutterEventChannel
    private weak var host: PlayerSurfaceView?
    private var sink: FlutterEventSink?
    private var closed = false

    init(messenger: FlutterBinaryMessenger, viewId: Int64, host: PlayerSurfaceView) {
        channel = FlutterEventChannel(
            name: "io.github.umbrella22.vesper_player/views/\(viewId)/geometry", binaryMessenger: messenger)
        self.host = host
        super.init()
        channel.setStreamHandler(self)
        host.onGeometryChanged = { [weak self] geometry in
            self?.sink?(flutterVideoGeometryMap(geometry))
        }
    }

    func onListen(withArguments arguments: Any?, eventSink events: @escaping FlutterEventSink) -> FlutterError? {
        sink = events
        if closed {
            events(FlutterEndOfEventStream)
            return nil
        }
        events(flutterVideoGeometryMap(host?.geometry))
        return nil
    }

    func onCancel(withArguments arguments: Any?) -> FlutterError? {
        sink = nil
        if closed { channel.setStreamHandler(nil) }
        return nil
    }

    func close() {
        guard !closed else { return }
        closed = true
        host?.onGeometryChanged = nil
        let activeSink = sink
        sink = nil
        if let activeSink {
            // A native view may be released before Dart cancels its stream.
            // Retain the handler until cancellation acknowledges this end event.
            activeSink(FlutterEndOfEventStream)
        } else {
            channel.setStreamHandler(nil)
        }
    }
}
