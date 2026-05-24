import Combine
import Foundation
import VesperPlayerKit

final class PlayerSession {
    let id: String
    let controller: VesperPlayerController
    let benchmarkConsoleLogging: Bool
    var hostView: PlayerSurfaceView?
    var pendingHostDetachTask: Task<Void, Never>?
    var hostDetachGeneration: UInt64 = 0
    var observation: AnyCancellable?
    var lastError: [String: Any]?
    var viewport: FlutterViewport?
    var viewportHint: FlutterViewportHint = .hidden

    init(
        id: String,
        controller: VesperPlayerController,
        benchmarkConsoleLogging: Bool = false
    ) {
        self.id = id
        self.controller = controller
        self.benchmarkConsoleLogging = benchmarkConsoleLogging
    }

    func cancelPendingHostDetach() {
        pendingHostDetachTask?.cancel()
        pendingHostDetachTask = nil
    }

    @discardableResult
    func advanceHostDetachGeneration() -> UInt64 {
        hostDetachGeneration &+= 1
        return hostDetachGeneration
    }
}

struct BenchmarkConsolePayload: Encodable {
    let playerId: String
    let events: [VesperBenchmarkEvent]
    let summary: VesperBenchmarkSummary
}

final class DownloadSession {
    let id: String
    let manager: VesperDownloadManager
    var observation: AnyCancellable?
    var lastError: [String: Any]?

    init(id: String, manager: VesperDownloadManager) {
        self.id = id
        self.manager = manager
    }
}

struct FlutterViewport {
    let left: Double
    let top: Double
    let width: Double
    let height: Double

    func toMap() -> [String: Any] {
        [
            "left": left,
            "top": top,
            "width": width,
            "height": height,
        ]
    }
}

struct FlutterViewportHint {
    let kind: String
    let visibleFraction: Double

    static let hidden = FlutterViewportHint(kind: "hidden", visibleFraction: 0)

    func toMap() -> [String: Any] {
        [
            "kind": kind,
            "visibleFraction": visibleFraction,
        ]
    }
}

