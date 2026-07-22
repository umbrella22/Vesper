import Foundation
internal import VesperPlayerKitBridgeShim

/// Immediate (synchronous) subtitle selection failures thrown by
/// `setSubtitleTrackSelection`. These surface through the iOS Flutter
/// plugin's `handleSessionCommand` catch as `FlutterError`s, so the Dart
/// `Future<void>` actually fails instead of completing successfully.
///
/// `localizedDescription` carries the matching `subtitle_*` code as a
/// prefix so the plugin's `errorMap(from:)` can decode a structured code
/// without a new Swift error type bleeding into the public wire shape.
public enum VesperSubtitleSelectionError: Error, LocalizedError {
    /// The AV legible media selection group is missing for a non-auto
    /// selection request.
    case platformTrackUnavailable(trackId: String?)
    /// `.track(id)` was issued for an id not present in the current catalog.
    case trackNotFound(trackId: String)
    /// `.auto` was issued but no candidate option exists.
    case autoCandidateUnavailable
    /// `.track(id)` was issued but AVPlayer did not converge on the
    /// requested option after `item.select`. Surfaced as a throw rather
    /// than a state-channel report because the caller asked for a specific
    /// id and the platform refused.
    case selectionDidNotConverge(trackId: String?)

    public var errorDescription: String? {
        switch self {
        case let .platformTrackUnavailable(trackId):
            let suffix = trackId.map { " trackId=\($0)" } ?? ""
            return "subtitle_platform_track_unavailable: no legible media selection group\(suffix)"
        case let .trackNotFound(trackId):
            return "subtitle_track_not_found: trackId=\(trackId) is not in the current catalog"
        case .autoCandidateUnavailable:
            return "subtitle_auto_candidate_unavailable: no subtitle candidate for auto selection"
        case let .selectionDidNotConverge(trackId):
            let suffix = trackId.map { " trackId=\($0)" } ?? ""
            return "subtitle_selection_failed: AVPlayer did not converge on the requested option\(suffix)"
        }
    }
}

/// Subtitle lifecycle status shared between iOS host and Flutter.
///
/// The `unknown` case is reserved for forward compatibility so a future native
/// addition does not corrupt the event stream.
public enum VesperSubtitleStatus: String, Equatable {
    case unavailable
    case loading
    case ready
    case failed
    case unknown
}

/// Phase where a subtitle failure originated.
public enum VesperSubtitleErrorPhase: String, Equatable {
    case manifest
    case resource
    case discovery
    case identity
    case selection
    case unknown
}

/// Structured subtitle error carried alongside the subtitle state.
public struct VesperSubtitleError: Equatable {
    public let code: String
    public let phase: VesperSubtitleErrorPhase
    public let trackId: String?
    public let retriable: Bool
    public let message: String

    public init(
        code: String,
        phase: VesperSubtitleErrorPhase,
        trackId: String?,
        retriable: Bool,
        message: String
    ) {
        self.code = code
        self.phase = phase
        self.trackId = trackId
        self.retriable = retriable
        self.message = message
    }
}

/// Snapshot of subtitle catalog lifecycle exposed to Flutter. The status
/// transitions follow the cross-platform subtitle contract:
///
/// - `unavailable`: manifest has no subtitles.
/// - `loading`: manifest declared subtitles but the AV legible group or
///   option discovery has not finished.
/// - `ready`: at least one subtitle has a unique native mapping and
///   `selectableTrackCount > 0`.
/// - `failed`: manifest declared subtitles but parsing, resource loading,
///   identity mapping, or platform discovery failed. The advertised count
///   is preserved so the host UI can distinguish "subtitles broken" from
///   "no subtitles".
public struct VesperSubtitleState: Equatable {
    public let status: VesperSubtitleStatus
    public let advertisedTrackCount: Int
    public let selectableTrackCount: Int
    public let error: VesperSubtitleError?

    public init(
        status: VesperSubtitleStatus,
        advertisedTrackCount: Int,
        selectableTrackCount: Int,
        error: VesperSubtitleError?
    ) {
        self.status = status
        self.advertisedTrackCount = advertisedTrackCount
        self.selectableTrackCount = selectableTrackCount
        self.error = error
    }

    public static let empty = VesperSubtitleState(
        status: .unavailable,
        advertisedTrackCount: 0,
        selectableTrackCount: 0,
        error: nil
    )

    public static func unavailable() -> VesperSubtitleState {
        VesperSubtitleState(
            status: .unavailable,
            advertisedTrackCount: 0,
            selectableTrackCount: 0,
            error: nil
        )
    }

    public static func loading(advertisedTrackCount: Int) -> VesperSubtitleState {
        VesperSubtitleState(
            status: .loading,
            advertisedTrackCount: advertisedTrackCount,
            selectableTrackCount: 0,
            error: nil
        )
    }

    public static func ready(advertisedTrackCount: Int, selectableTrackCount: Int) -> VesperSubtitleState {
        VesperSubtitleState(
            status: .ready,
            advertisedTrackCount: advertisedTrackCount,
            selectableTrackCount: selectableTrackCount,
            error: nil
        )
    }

    public static func failed(
        advertisedTrackCount: Int,
        code: String,
        phase: VesperSubtitleErrorPhase,
        trackId: String? = nil,
        retriable: Bool = false,
        message: String
    ) -> VesperSubtitleState {
        VesperSubtitleState(
            status: .failed,
            advertisedTrackCount: advertisedTrackCount,
            selectableTrackCount: 0,
            error: VesperSubtitleError(
                code: code,
                phase: phase,
                trackId: trackId,
                retriable: retriable,
                message: message
            )
        )
    }
}

extension VesperNativePlayerBridge {
    /// Records a subtitle failure into both `publishedLastError` (so the
    /// existing `lastError` Flutter channel surfaces it) and
    /// `publishedSubtitleState` (so the new subtitle state channel surfaces
    /// the structured phase/code). Native-stage selection races appear in
    /// both channels.
    func reportSubtitleFailure(
        code: String,
        phase: VesperSubtitleErrorPhase,
        trackId: String? = nil,
        retriable: Bool = false,
        message: String,
        bridgeErrorCode: VesperPlayerErrorCode = .invalidState,
        bridgeErrorCategory: VesperPlayerErrorCategory = .capability
    ) {
        let advertised = publishedSubtitleState.advertisedTrackCount
        publishedSubtitleState = .failed(
            advertisedTrackCount: advertised,
            code: code,
            phase: phase,
            trackId: trackId,
            retriable: retriable,
            message: message
        )
        var details: [String: String] = [
            "subtitlePhase": phase.rawValue,
            "subtitleCode": code,
        ]
        if let trackId {
            details["trackId"] = trackId
        }
        publishedLastError = VesperPlayerError(
            message: message,
            code: bridgeErrorCode,
            category: bridgeErrorCategory,
            retriable: retriable,
            details: details
        )
        fixedTrackIssueActive = false
        iosHostLog("subtitleFailure phase=\(phase.rawValue) code=\(code)")
    }

    /// Clears the subtitle error channel without changing the
    /// `advertisedTrackCount`. Used at the start of a selection command so
    /// a previous failure does not linger while the new command is in
    /// flight. After clearing, the status reverts to `.ready` if a
    /// selectable track count is still known, otherwise to `.loading`
    /// (waiting for the next `refreshTrackCatalogAndSelection` to confirm
    /// the catalog state).
    func clearSubtitleFailure() {
        if publishedSubtitleState.error != nil {
            publishedSubtitleState = VesperSubtitleState(
                status: publishedSubtitleState.selectableTrackCount > 0 ? .ready : .loading,
                advertisedTrackCount: publishedSubtitleState.advertisedTrackCount,
                selectableTrackCount: publishedSubtitleState.selectableTrackCount,
                error: nil
            )
        }
    }
}
