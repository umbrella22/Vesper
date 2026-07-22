import Foundation
internal import VesperPlayerKitBridgeShim

/// Typed subtitle selection failure produced by the iOS backend.
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
    /// AVPlayer did not converge before the bounded confirmation deadline.
    case selectionTimedOut(trackId: String?)
    /// The selection was cancelled because the source or player item changed.
    case selectionCancelled(trackId: String?)
    /// The active source changed while the command was pending.
    case sourceChanged(trackId: String?)
    /// A newer selection superseded this pending command.
    case selectionSuperseded(trackId: String?)
    /// The current catalog cannot route selections because discovery or
    /// identity validation failed. The raw catalog code is preserved.
    case catalogUnavailable(
        code: String,
        trackId: String?,
        phase: VesperSubtitleErrorPhase,
        phaseRawValue: String?,
        message: String,
        retriable: Bool
    )

    public var errorDescription: String? {
        switch self {
        case let .platformTrackUnavailable(trackId):
            let suffix = trackId.map { " trackId=\($0)" } ?? ""
            return "No legible media selection group is available.\(suffix)"
        case let .trackNotFound(trackId):
            return "Subtitle trackId=\(trackId) is not in the current catalog."
        case .autoCandidateUnavailable:
            return "No subtitle candidate is available for automatic selection."
        case let .selectionDidNotConverge(trackId):
            let suffix = trackId.map { " trackId=\($0)" } ?? ""
            return "AVPlayer did not converge on the requested subtitle option.\(suffix)"
        case let .selectionTimedOut(trackId):
            let suffix = trackId.map { " trackId=\($0)" } ?? ""
            return "AVPlayer did not converge before the subtitle confirmation deadline.\(suffix)"
        case let .selectionCancelled(trackId):
            let suffix = trackId.map { " trackId=\($0)" } ?? ""
            return "The subtitle selection was cancelled.\(suffix)"
        case let .sourceChanged(trackId):
            let suffix = trackId.map { " trackId=\($0)" } ?? ""
            return "The active source changed while applying the subtitle selection.\(suffix)"
        case let .selectionSuperseded(trackId):
            let suffix = trackId.map { " trackId=\($0)" } ?? ""
            return "A newer subtitle selection replaced this command.\(suffix)"
        case let .catalogUnavailable(_, _, _, _, message, _):
            return message
        }
    }
}

extension VesperSubtitleSelectionError {
    var subtitleCode: String {
        switch self {
        case .platformTrackUnavailable: return "subtitle_platform_track_unavailable"
        case .trackNotFound: return "subtitle_track_not_found"
        case .autoCandidateUnavailable: return "subtitle_auto_candidate_unavailable"
        case .selectionDidNotConverge: return "subtitle_selection_mismatch"
        case .selectionTimedOut: return "subtitle_selection_timeout"
        case .selectionCancelled: return "subtitle_selection_cancelled"
        case .sourceChanged: return "subtitle_source_changed"
        case .selectionSuperseded: return "subtitle_selection_superseded"
        case let .catalogUnavailable(code, _, _, _, _, _): return code
        }
    }

    var subtitleTrackId: String? {
        switch self {
        case let .platformTrackUnavailable(trackId),
             let .selectionDidNotConverge(trackId),
             let .selectionTimedOut(trackId),
             let .selectionCancelled(trackId),
             let .sourceChanged(trackId),
             let .selectionSuperseded(trackId):
            return trackId
        case let .catalogUnavailable(_, trackId, _, _, _, _):
            return trackId
        case let .trackNotFound(trackId): return trackId
        case .autoCandidateUnavailable: return nil
        }
    }

    var subtitleRetriable: Bool {
        switch self {
        case .selectionTimedOut, .selectionCancelled, .sourceChanged, .selectionSuperseded:
            return true
        case let .catalogUnavailable(_, _, _, _, _, retriable):
            return retriable
        default: return false
        }
    }

    var subtitlePhase: String {
        switch self {
        case let .catalogUnavailable(_, _, phase, phaseRawValue, _, _):
            return phaseRawValue ?? phase.rawValue
        default:
            return VesperSubtitleErrorPhase.selection.rawValue
        }
    }
}

/// Transaction-scoped selection error exposed by the async native API.
public struct VesperSubtitleSelectionCommandError: Error, LocalizedError {
    public let failure: VesperSubtitleSelectionError
    public let commandId: UInt64
    public let sourceEpoch: UInt64

    public init(
        failure: VesperSubtitleSelectionError,
        commandId: UInt64,
        sourceEpoch: UInt64
    ) {
        self.failure = failure
        self.commandId = commandId
        self.sourceEpoch = sourceEpoch
    }

    public var errorDescription: String? { failure.errorDescription }
    public var code: String { failure.subtitleCode }
    public var trackId: String? { failure.subtitleTrackId }
    public var retriable: Bool { failure.subtitleRetriable }
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
public struct VesperSubtitleError: Error, LocalizedError, Equatable {
    public let code: String
    public let phase: VesperSubtitleErrorPhase
    public let trackId: String?
    public let retriable: Bool
    public let message: String
    public let commandId: UInt64?
    public let sourceEpoch: UInt64?
    public let phaseRawValue: String?

    public init(
        code: String,
        phase: VesperSubtitleErrorPhase,
        trackId: String?,
        retriable: Bool,
        message: String,
        commandId: UInt64? = nil,
        sourceEpoch: UInt64? = nil,
        phaseRawValue: String? = nil
    ) {
        self.code = code
        self.phase = phase
        self.trackId = trackId
        self.retriable = retriable
        self.message = message
        self.commandId = commandId
        self.sourceEpoch = sourceEpoch
        self.phaseRawValue = phaseRawValue
    }

    public var errorDescription: String? { message }
}

/// Canonical subtitle catalog lifecycle shared with Flutter and Android.
public enum VesperSubtitleCatalogState: String, Equatable {
    case unavailable
    case loading
    case ready
    case failed
    case unknown
}

/// Canonical subtitle selection transaction lifecycle.
public enum VesperSubtitleSelectionState: String, Equatable {
    case idle
    case applying
    case confirmed
    case failed
    case unknown
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
    public let catalogState: VesperSubtitleCatalogState
    public let selectionState: VesperSubtitleSelectionState
    public let advertisedTrackCount: Int
    public let selectableTrackCount: Int
    public let catalogError: VesperSubtitleError?
    public let selectionError: VesperSubtitleError?
    public let catalogStateRawValue: String?
    public let selectionStateRawValue: String?

    /// Compatibility alias for the pre-0.4 catalog status.
    public var status: VesperSubtitleStatus {
        switch catalogState {
        case .unavailable: return .unavailable
        case .loading: return .loading
        case .ready: return .ready
        case .failed: return .failed
        case .unknown: return .unknown
        }
    }

    /// Compatibility alias. Selection failures take precedence.
    public var error: VesperSubtitleError? {
        selectionError ?? catalogError
    }

    public init(
        catalogState: VesperSubtitleCatalogState,
        selectionState: VesperSubtitleSelectionState = .idle,
        advertisedTrackCount: Int,
        selectableTrackCount: Int,
        catalogError: VesperSubtitleError? = nil,
        selectionError: VesperSubtitleError? = nil,
        catalogStateRawValue: String? = nil,
        selectionStateRawValue: String? = nil
    ) {
        self.catalogState = catalogState
        self.selectionState = selectionState
        self.advertisedTrackCount = advertisedTrackCount
        self.selectableTrackCount = selectableTrackCount
        self.catalogError = catalogError
        self.selectionError = selectionError
        self.catalogStateRawValue = catalogStateRawValue
        self.selectionStateRawValue = selectionStateRawValue
    }

    /// Compatibility initializer for older native callers.
    public init(
        status: VesperSubtitleStatus,
        advertisedTrackCount: Int,
        selectableTrackCount: Int,
        error: VesperSubtitleError?
    ) {
        let catalogState: VesperSubtitleCatalogState
        switch status {
        case .unavailable: catalogState = .unavailable
        case .loading: catalogState = .loading
        case .ready: catalogState = .ready
        case .failed: catalogState = .failed
        case .unknown: catalogState = .unknown
        }
        self.init(
            catalogState: catalogState,
            selectionState: error?.phase == .selection ? .failed : .idle,
            advertisedTrackCount: advertisedTrackCount,
            selectableTrackCount: selectableTrackCount,
            catalogError: error?.phase == .selection ? nil : error,
            selectionError: error?.phase == .selection ? error : nil
        )
    }

    public static let empty = VesperSubtitleState(
        catalogState: .unavailable,
        selectionState: .idle,
        advertisedTrackCount: 0,
        selectableTrackCount: 0,
        catalogError: nil,
        selectionError: nil
    )

    public static func unavailable() -> VesperSubtitleState {
        VesperSubtitleState(
            catalogState: .unavailable,
            selectionState: .idle,
            advertisedTrackCount: 0,
            selectableTrackCount: 0,
            catalogError: nil,
            selectionError: nil
        )
    }

    public static func loading(advertisedTrackCount: Int) -> VesperSubtitleState {
        VesperSubtitleState(
            catalogState: .loading,
            selectionState: .idle,
            advertisedTrackCount: advertisedTrackCount,
            selectableTrackCount: 0,
            catalogError: nil,
            selectionError: nil
        )
    }

    public static func ready(advertisedTrackCount: Int, selectableTrackCount: Int) -> VesperSubtitleState {
        VesperSubtitleState(
            catalogState: .ready,
            selectionState: .idle,
            advertisedTrackCount: advertisedTrackCount,
            selectableTrackCount: selectableTrackCount,
            catalogError: nil,
            selectionError: nil
        )
    }

    public static func failed(
        advertisedTrackCount: Int,
        code: String,
        phase: VesperSubtitleErrorPhase,
        trackId: String? = nil,
        retriable: Bool = false,
        message: String,
        selectableTrackCount: Int = 0,
        phaseRawValue: String? = nil
    ) -> VesperSubtitleState {
        let error = VesperSubtitleError(
            code: code,
            phase: phase,
            trackId: trackId,
            retriable: retriable,
            message: message,
            phaseRawValue: phaseRawValue
        )
        if phase == .selection {
            return VesperSubtitleState(
                catalogState: .ready,
                selectionState: .failed,
                advertisedTrackCount: advertisedTrackCount,
                selectableTrackCount: selectableTrackCount,
                catalogError: nil,
                selectionError: error
            )
        }
        return VesperSubtitleState(
            catalogState: .failed,
            selectionState: .idle,
            advertisedTrackCount: advertisedTrackCount,
            selectableTrackCount: selectableTrackCount,
            catalogError: error,
            selectionError: nil
        )
    }

    func replacingCatalog(with catalog: VesperSubtitleState) -> VesperSubtitleState {
        VesperSubtitleState(
            catalogState: catalog.catalogState,
            selectionState: selectionState,
            advertisedTrackCount: catalog.advertisedTrackCount,
            selectableTrackCount: catalog.selectableTrackCount,
            catalogError: catalog.catalogError,
            selectionError: selectionError,
            catalogStateRawValue: catalog.catalogStateRawValue,
            selectionStateRawValue: selectionStateRawValue
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
        bridgeErrorCategory: VesperPlayerErrorCategory = .capability,
        commandId: UInt64? = nil,
        sourceEpoch: UInt64? = nil
    ) {
        let advertised = publishedSubtitleState.advertisedTrackCount
        let error = VesperSubtitleError(
            code: code,
            phase: phase,
            trackId: trackId,
            retriable: retriable,
            message: message,
            commandId: commandId,
            sourceEpoch: sourceEpoch
        )
        if phase == .selection {
            publishedSubtitleState = VesperSubtitleState(
                catalogState: publishedSubtitleState.catalogState,
                selectionState: .failed,
                advertisedTrackCount: advertised,
                selectableTrackCount: publishedSubtitleState.selectableTrackCount,
                catalogError: publishedSubtitleState.catalogError,
                selectionError: error
            )
        } else if publishedSubtitleState.selectableTrackCount > 0 {
            publishedSubtitleState = VesperSubtitleState(
                catalogState: .ready,
                selectionState: publishedSubtitleState.selectionState,
                advertisedTrackCount: advertised,
                selectableTrackCount: publishedSubtitleState.selectableTrackCount,
                catalogError: error,
                selectionError: publishedSubtitleState.selectionError
            )
        } else {
            publishedSubtitleState = VesperSubtitleState(
                catalogState: .failed,
                selectionState: publishedSubtitleState.selectionState,
                advertisedTrackCount: advertised,
                selectableTrackCount: publishedSubtitleState.selectableTrackCount,
                catalogError: error,
                selectionError: publishedSubtitleState.selectionError
            )
        }
        var details: [String: String] = [
            "domain": "subtitle",
            "phase": phase.rawValue,
            "code": code,
            "retriable": retriable ? "true" : "false",
            "message": message,
        ]
        if let trackId {
            details["trackId"] = trackId
        }
        if let commandId {
            details["commandId"] = String(commandId)
        }
        if let sourceEpoch {
            details["sourceEpoch"] = String(sourceEpoch)
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
        if publishedSubtitleState.selectionError != nil {
            publishedSubtitleState = VesperSubtitleState(
                catalogState: publishedSubtitleState.catalogState,
                selectionState: .idle,
                advertisedTrackCount: publishedSubtitleState.advertisedTrackCount,
                selectableTrackCount: publishedSubtitleState.selectableTrackCount,
                catalogError: publishedSubtitleState.catalogError,
                selectionError: nil
            )
        }
    }
}
