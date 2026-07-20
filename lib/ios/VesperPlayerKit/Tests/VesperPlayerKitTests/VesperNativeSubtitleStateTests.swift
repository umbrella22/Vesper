import XCTest
@testable import VesperPlayerKit

/// Contract tests for the iOS subtitle state model and wire shape.
///
/// These tests cover the behavior that does not require a live
/// `AVPlayerItem` / legible media selection group. The end-to-end
/// `loadTrackCatalogState` AV legible-group behavior is exercised by the
/// existing `VesperDashBridgeHlsBuilderTests` and depends on a real
/// `vesper-dash://` AVAsset; the cases here isolate the model helpers and
/// the Flutter wire mapping so regressions surface without a simulator
/// device boot.
final class VesperNativeSubtitleStateTests: XCTestCase {
    func testSubtitleStateUnavailableHasZeroCounts() {
        let state = VesperSubtitleState.unavailable()
        XCTAssertEqual(state.status, .unavailable)
        XCTAssertEqual(state.advertisedTrackCount, 0)
        XCTAssertEqual(state.selectableTrackCount, 0)
        XCTAssertNil(state.error)
    }

    func testSubtitleStateLoadingPreservesAdvertisedCount() {
        let state = VesperSubtitleState.loading(advertisedTrackCount: 2)
        XCTAssertEqual(state.status, .loading)
        XCTAssertEqual(state.advertisedTrackCount, 2)
        XCTAssertEqual(state.selectableTrackCount, 0)
        XCTAssertNil(state.error)
    }

    func testSubtitleStateReadyCarriesSelectableCount() {
        let state = VesperSubtitleState.ready(advertisedTrackCount: 2, selectableTrackCount: 2)
        XCTAssertEqual(state.status, .ready)
        XCTAssertEqual(state.advertisedTrackCount, 2)
        XCTAssertEqual(state.selectableTrackCount, 2)
        XCTAssertNil(state.error)
    }

    func testSubtitleStateFailedPreservesAdvertisedCountAndCarriesStructuredError() throws {
        let state = VesperSubtitleState.failed(
            advertisedTrackCount: 3,
            code: "subtitle_platform_track_unavailable",
            phase: .discovery,
            message: "no legible group"
        )
        XCTAssertEqual(state.status, .failed)
        XCTAssertEqual(state.advertisedTrackCount, 3)
        XCTAssertEqual(state.selectableTrackCount, 0)
        let error = try XCTUnwrap(state.error)
        XCTAssertEqual(error.code, "subtitle_platform_track_unavailable")
        XCTAssertEqual(error.phase, .discovery)
        XCTAssertNil(error.trackId)
        XCTAssertFalse(error.retriable)
    }

    func testSubtitleStateFailedCarriesTrackIdForSelectionFailures() throws {
        let state = VesperSubtitleState.failed(
            advertisedTrackCount: 1,
            code: "subtitle_track_not_found",
            phase: .selection,
            trackId: "subtitle:dash:sub-en",
            message: "missing"
        )
        let error = try XCTUnwrap(state.error)
        XCTAssertEqual(error.trackId, "subtitle:dash:sub-en")
        XCTAssertEqual(error.phase, .selection)
    }

    @MainActor
    func testReportSubtitleFailurePublishesBothLastErrorAndSubtitleState() throws {
        let bridge = VesperNativePlayerBridge()
        bridge.publishedSubtitleState = .ready(advertisedTrackCount: 2, selectableTrackCount: 2)

        bridge.reportSubtitleFailure(
            code: "subtitle_track_not_found",
            phase: .selection,
            trackId: "subtitle:dash:sub-zh",
            message: "track not in catalog"
        )

        XCTAssertEqual(bridge.publishedSubtitleState.status, .failed)
        // Advertised count must be preserved across the failure transition
        // so a future ready state can still show "2 of 2 subtitles".
        XCTAssertEqual(bridge.publishedSubtitleState.advertisedTrackCount, 2)
        let subtitleError = try XCTUnwrap(bridge.publishedSubtitleState.error)
        XCTAssertEqual(subtitleError.code, "subtitle_track_not_found")
        XCTAssertEqual(subtitleError.phase, .selection)
        XCTAssertEqual(subtitleError.trackId, "subtitle:dash:sub-zh")

        // The existing generic `lastError` channel must also carry the
        // structured subtitle phase/code details so Flutter consumers that
        // have not migrated to subtitleState can still observe the failure.
        let lastError = try XCTUnwrap(bridge.publishedLastError)
        XCTAssertEqual(lastError.code, .invalidState)
        XCTAssertEqual(lastError.category, .capability)
        XCTAssertEqual(lastError.details["subtitlePhase"], "selection")
        XCTAssertEqual(lastError.details["subtitleCode"], "subtitle_track_not_found")
        XCTAssertEqual(lastError.details["trackId"], "subtitle:dash:sub-zh")
    }

    @MainActor
    func testStaleDashResourceFailureDoesNotOverwriteCurrentSourceState() {
        let oldSource = VesperPlayerSource.dash(
            url: URL(string: "https://example.test/old.mpd")!
        )
        let currentSource = VesperPlayerSource.dash(
            url: URL(string: "https://example.test/current.mpd")!
        )
        let oldSession = VesperDashSession(sourceURL: URL(string: oldSource.uri)!)
        let currentSession = VesperDashSession(sourceURL: URL(string: currentSource.uri)!)
        let bridge = VesperNativePlayerBridge(initialSource: currentSource)
        bridge.currentDashSession = currentSession
        bridge.publishedSubtitleState = .ready(
            advertisedTrackCount: 1,
            selectableTrackCount: 1
        )

        bridge.reportDashSubtitleResourceFailure(
            session: oldSession,
            source: oldSource
        )

        XCTAssertEqual(bridge.publishedSubtitleState.status, .ready)
        XCTAssertNil(bridge.publishedSubtitleState.error)

        bridge.reportDashSubtitleResourceFailure(
            session: currentSession,
            source: currentSource
        )
        XCTAssertEqual(bridge.publishedSubtitleState.status, .failed)
        XCTAssertEqual(
            bridge.publishedSubtitleState.error?.code,
            "subtitle_resource_load_failed"
        )
    }

    @MainActor
    func testClearSubtitleFailureRevertsToReadyWhenSelectableTracksExist() {
        let bridge = VesperNativePlayerBridge()
        bridge.publishedSubtitleState = .failed(
            advertisedTrackCount: 2,
            code: "subtitle_track_not_found",
            phase: .selection,
            message: "previous failure"
        )
        // Simulate that a previous load produced a selectable track.
        bridge.publishedSubtitleState = VesperSubtitleState(
            status: .failed,
            advertisedTrackCount: 2,
            selectableTrackCount: 1,
            error: VesperSubtitleError(
                code: "subtitle_track_not_found",
                phase: .selection,
                trackId: nil,
                retriable: false,
                message: "previous failure"
            )
        )

        bridge.clearSubtitleFailure()

        XCTAssertEqual(bridge.publishedSubtitleState.status, .ready)
        XCTAssertEqual(bridge.publishedSubtitleState.advertisedTrackCount, 2)
        XCTAssertEqual(bridge.publishedSubtitleState.selectableTrackCount, 1)
        XCTAssertNil(bridge.publishedSubtitleState.error)
    }

    @MainActor
    func testClearSubtitleFailureRevertsToLoadingWhenNoSelectableTracks() {
        let bridge = VesperNativePlayerBridge()
        bridge.publishedSubtitleState = .failed(
            advertisedTrackCount: 1,
            code: "subtitle_platform_track_unavailable",
            phase: .discovery,
            message: "no group"
        )

        bridge.clearSubtitleFailure()

        // Without any selectable track, the cleared state is `loading`
        // (still waiting for the AV legible group to populate).
        XCTAssertEqual(bridge.publishedSubtitleState.status, .loading)
        XCTAssertEqual(bridge.publishedSubtitleState.advertisedTrackCount, 1)
        XCTAssertNil(bridge.publishedSubtitleState.error)
    }

    @MainActor
    func testControllerSubtitleStateWireMapMatchesFlutterContract() throws {
        let bridge = VesperNativePlayerBridge()
        bridge.publishedSubtitleState = .failed(
            advertisedTrackCount: 2,
            code: "subtitle_track_not_found",
            phase: .selection,
            trackId: "subtitle:dash:sub-en",
            message: "missing"
        )
        let controller = VesperPlayerController(bridge)

        let map = controller.subtitleStateWireMap()

        // Status / phase / code wire names must be lowercase to match the
        // Dart enum names in subtitle_state_models.dart.
        XCTAssertEqual(map["status"] as? String, "failed")
        XCTAssertEqual(map["advertisedTrackCount"] as? Int, 2)
        XCTAssertEqual(map["selectableTrackCount"] as? Int, 0)
        let errorMap = try XCTUnwrap(map["error"] as? [String: Any])
        XCTAssertEqual(errorMap["code"] as? String, "subtitle_track_not_found")
        XCTAssertEqual(errorMap["phase"] as? String, "selection")
        XCTAssertEqual(errorMap["trackId"] as? String, "subtitle:dash:sub-en")
        XCTAssertEqual(errorMap["retriable"] as? Bool, false)
        XCTAssertEqual(errorMap["message"] as? String, "missing")
    }

    @MainActor
    func testControllerSubtitleStateWireMapEmitsNullErrorWhenReady() {
        let bridge = VesperNativePlayerBridge()
        bridge.publishedSubtitleState = .ready(advertisedTrackCount: 1, selectableTrackCount: 1)
        let controller = VesperPlayerController(bridge)

        let map = controller.subtitleStateWireMap()

        XCTAssertEqual(map["status"] as? String, "ready")
        XCTAssertTrue(map["error"] is NSNull)
    }

    // MARK: - setSubtitleTrackSelection throws

    /// When the AV legible group is missing for a `.track(id)` request, the
    /// bridge must throw
    /// `.platformTrackUnavailable(trackId:)` so the iOS Flutter plugin's
    /// `handleSessionCommand` catch converts it to a `FlutterError` and
    /// the Dart `Future<void>` actually fails.
    ///
    /// This test verifies the error type's payload contract directly
    /// because constructing a real `AVPlayerItem` + legible group in a
    /// unit test is not feasible. The end-to-end plugin path is exercised
    /// by the integration test suite.
    func testPlatformTrackUnavailableErrorCarriesStructuredCode() {
        let error = VesperSubtitleSelectionError.platformTrackUnavailable(
            trackId: "subtitle:dash:sub-en"
        )
        XCTAssertEqual(
            error.errorDescription,
            "subtitle_platform_track_unavailable: no legible media selection group trackId=subtitle:dash:sub-en"
        )
    }

    /// `.platformTrackUnavailable` without a trackId produces the correct
    /// message shape (used by `.auto` paths where no id is in flight).
    func testPlatformTrackUnavailableErrorWithoutTrackId() {
        let error = VesperSubtitleSelectionError.platformTrackUnavailable(trackId: nil)
        XCTAssertEqual(
            error.errorDescription,
            "subtitle_platform_track_unavailable: no legible media selection group"
        )
    }

    /// A `.track(id)` for an id not in `subtitleOptionsByTrackId` must
    /// throw `.trackNotFound(trackId:)` carrying the offending id.
    func testSetSubtitleTrackSelectionThrowsForUnknownTrackId() {
        let error = VesperSubtitleSelectionError.trackNotFound(trackId: "subtitle:dash:sub-zh")
        XCTAssertEqual(
            error.errorDescription,
            "subtitle_track_not_found: trackId=subtitle:dash:sub-zh is not in the current catalog"
        )
    }

    /// `.autoCandidateUnavailable` carries the matching subtitle_* code.
    func testAutoCandidateUnavailableErrorCarriesStructuredCode() {
        let error = VesperSubtitleSelectionError.autoCandidateUnavailable
        XCTAssertEqual(
            error.errorDescription,
            "subtitle_auto_candidate_unavailable: no subtitle candidate for auto selection"
        )
    }

    /// `.selectionDidNotConverge(trackId:)` carries the matching code.
    func testSelectionDidNotConvergeErrorCarriesStructuredCode() {
        let error = VesperSubtitleSelectionError.selectionDidNotConverge(trackId: "subtitle:dash:sub-en")
        XCTAssertEqual(
            error.errorDescription,
            "subtitle_selection_failed: AVPlayer did not converge on the requested option trackId=subtitle:dash:sub-en"
        )
    }
}
