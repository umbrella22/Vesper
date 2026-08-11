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
    var lastEmittedTerminalError: [String: Any]?
    var viewport: FlutterViewport?
    var viewportHint: FlutterViewportHint = .hidden
    var currentSourceFingerprint: VesperSourceFingerprint?
    var recentHdrProbeEvidence: VesperHdrProbeEvidence?
    var pictureInPictureConfiguration = FlutterPictureInPictureConfiguration()
    var pictureInPictureCoordinator: VesperIosPictureInPictureCoordinator?
    var pictureInPictureState = "inactive"
    var pictureInPictureActive = false

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

struct VesperSourceFingerprint: Equatable {
    let uri: String
    let kind: String
    let sourceProtocol: String

    init(source: VesperPlayerSource) {
        uri = source.uri
        kind = source.kind.rawValue
        sourceProtocol = source.protocol.rawValue
    }
}

struct VesperHdrProbeEvidence: Equatable {
    let sourceFingerprint: VesperSourceFingerprint
    let hdrKind: VesperPlaybackCapabilityHdrKind
    let confidence: VesperPlaybackCapabilityConfidence
    let hdrMetadata: [String: Any]?

    init?(source: VesperPlayerSource?, result: VesperPlaybackCapabilityProbeResult) {
        guard let source,
            result.recommendedPlaybackPath == .systemPlayer,
            result.hdrKind != .none,
            result.hdrKind != .unknown,
            result.confidence == .sourceMetadata || result.confidence == .sessionProbe
        else {
            return nil
        }

        sourceFingerprint = VesperSourceFingerprint(source: source)
        hdrKind = result.hdrKind
        confidence = result.confidence
        hdrMetadata = flutterHdrMetadataMap(from: result)
    }

    static func == (lhs: VesperHdrProbeEvidence, rhs: VesperHdrProbeEvidence) -> Bool {
        lhs.sourceFingerprint == rhs.sourceFingerprint
            && lhs.hdrKind == rhs.hdrKind
            && lhs.confidence == rhs.confidence
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

@MainActor
final class PlaybackSequenceSession {
    let id: String
    let playerId: String
    let sequence: VesperPlaybackSequence
    var observation: AnyCancellable?

    init(id: String, playerId: String, sequence: VesperPlaybackSequence) {
        self.id = id
        self.playerId = playerId
        self.sequence = sequence
    }
}
