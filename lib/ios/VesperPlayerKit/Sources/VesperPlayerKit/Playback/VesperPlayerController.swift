import AVFoundation
import Combine
import Foundation
import UIKit

@MainActor
internal protocol VesperPlaybackSequenceAttachment: AnyObject {
    func onControllerDisposed(_ controller: VesperPlayerController)
}

private struct VesperSubtitleBridgeSnapshot {
    let trackSelection: VesperTrackSelectionSnapshot
    let requestedSelection: VesperTrackSelection
    let confirmedSelection: VesperTrackSelection
    let effectiveTrackId: String?
    let state: VesperSubtitleState
    let lastError: VesperPlayerError?
}

@MainActor
public final class VesperPlayerController: ObservableObject {
    public let backend: PlayerBridgeBackend

    @Published private(set) var publishedUiState: PlayerHostUiState
    @Published private(set) var publishedTrackCatalog: VesperTrackCatalog
    @Published private(set) var publishedTrackSelection: VesperTrackSelectionSnapshot
    @Published private(set) var publishedRequestedSubtitleSelection: VesperTrackSelection
    @Published private(set) var publishedConfirmedSubtitleSelection: VesperTrackSelection
    @Published private(set) var publishedEffectiveVideoTrackId: String?
    @Published private(set) var publishedVideoVariantObservation: VesperVideoVariantObservation?
    @Published private(set) var publishedFixedTrackStatus: VesperFixedTrackStatus?
    @Published private(set) var publishedResiliencePolicy: VesperPlaybackResiliencePolicy
    @Published private(set) var publishedLastError: VesperPlayerError?
    /// Subtitle lifecycle state for the active source. Mirrors the bridge's
    /// `publishedSubtitleState` so Flutter can observe subtitle
    /// loading/ready/failed transitions independently of the generic
    /// `lastError` channel.
    @Published private(set) var publishedSubtitleState: VesperSubtitleState
    @Published private(set) var publishedEffectiveSubtitleTrackId: String?
    /// Current subtitle styling (font scale, visibility). Hosts observe this
    /// to drive a subtitle overlay; it does not flow through the player bridge
    /// because it only affects rendering.
    @Published private(set) var publishedSubtitleStyle: VesperSubtitleStyle

    public var uiState: PlayerHostUiState {
        publishedUiState
    }

    /// The latest media track catalog reported by the active source.
    public var trackCatalog: VesperTrackCatalog {
        publishedTrackCatalog
    }

    /// The currently applied track-selection intent for the active source.
    public var trackSelection: VesperTrackSelectionSnapshot {
        publishedTrackSelection
    }

    public var requestedSubtitleSelection: VesperTrackSelection {
        publishedRequestedSubtitleSelection
    }

    public var confirmedSubtitleSelection: VesperTrackSelection {
        publishedConfirmedSubtitleSelection
    }

    /// The best-effort video variant currently rendered by the backend.
    ///
    /// On iOS this is inferred from the current HLS variant ladder, playback
    /// access logs, and presentation size. It may be `nil` until the player has
    /// enough runtime information to identify a matching variant.
    public var effectiveVideoTrackId: String? {
        publishedEffectiveVideoTrackId
    }

    /// The raw runtime video-variant evidence currently observed by the host.
    ///
    /// On iOS this is derived from AVPlayer access logs plus presentation size.
    /// The value may be `nil` until playback produces enough runtime evidence.
    public var videoVariantObservation: VesperVideoVariantObservation? {
        publishedVideoVariantObservation
    }

    /// The latest best-effort status for the active `fixedTrack` ABR request.
    ///
    /// This value is `nil` when no fixed-track request is active. On iOS the
    /// status is derived from the current HLS variant ladder plus playback
    /// runtime evidence, so `.pending` means the host is still waiting for
    /// enough evidence to identify the active variant.
    public var fixedTrackStatus: VesperFixedTrackStatus? {
        publishedFixedTrackStatus
    }

    public var resiliencePolicy: VesperPlaybackResiliencePolicy {
        publishedResiliencePolicy
    }

    public var lastError: VesperPlayerError? {
        publishedLastError
    }

    /// Subtitle lifecycle state for the active source. Exposed so Flutter
    /// can render loading / ready / failed states without coupling to the
    /// generic `lastError` channel.
    public var subtitleState: VesperSubtitleState {
        publishedSubtitleState
    }

    /// The native subtitle track currently confirmed as effective. `nil`
    /// means subtitles are disabled or no platform track has converged yet.
    public var effectiveSubtitleTrackId: String? {
        publishedEffectiveSubtitleTrackId
    }

    /// Flutter wire shape for `subtitleState`. Keys are stable across
    /// iOS/Android and must match the Dart enum names in
    /// `subtitle_state_models.dart`.
    public func subtitleStateWireMap() -> [String: Any] {
        let state = publishedSubtitleState
        var map: [String: Any] = [
            "catalogState": state.catalogStateRawValue ?? state.catalogState.rawValue,
            "selectionState": state.selectionStateRawValue ?? state.selectionState.rawValue,
            "advertisedTrackCount": state.advertisedTrackCount,
            "selectableTrackCount": state.selectableTrackCount,
            "status": state.status.rawValue,
        ]
        map["catalogError"] = subtitleErrorWire(state.catalogError)
        map["selectionError"] = subtitleErrorWire(state.selectionError)
        // Compatibility aliases for pre-0.4 clients.
        map["error"] = subtitleErrorWire(state.error)
        return map
    }

    private func subtitleErrorWire(_ error: VesperSubtitleError?) -> Any {
        guard let error else { return NSNull() }
        var map: [String: Any] = [
            "code": error.code,
            "phase": error.phaseRawValue ?? error.phase.rawValue,
            "trackId": error.trackId ?? NSNull(),
            "retriable": error.retriable,
            "message": error.message,
        ]
        if let commandId = error.commandId { map["commandId"] = commandId }
        if let sourceEpoch = error.sourceEpoch { map["sourceEpoch"] = sourceEpoch }
        return map
    }

    public var subtitleStyle: VesperSubtitleStyle {
        publishedSubtitleStyle
    }

    public private(set) var pluginDiagnostics: [[String: Any]]

    private var bridgeObservation: AnyCancellable?
    private let initializeImpl: () -> Void
    private let initializeAsyncImpl: () async throws -> Void
    private let disposeImpl: () -> Void
    private let refreshImpl: () -> Void
    private let sampleTimelineImpl: () -> TimelineUiState?
    private let selectSourceImpl: (VesperPlayerSource) -> Void
    private let startSourceSelectionImpl: (VesperPlayerSource) -> Task<Void, Error>
    private let selectSourceAsyncImpl: (VesperPlayerSource) async throws -> Void
    private let attachSurfaceHostImpl: (UIView) -> Void
    private let detachSurfaceHostImpl: () -> Void
    private let detachSurfaceHostForHostImpl: (UIView) -> Void
    private let playImpl: () -> Void
    private let pauseImpl: () -> Void
    private let togglePauseImpl: () -> Void
    private let stopImpl: () -> Void
    private let seekByImpl: (Int64) -> Void
    private let seekByAsyncImpl: (Int64) async throws -> Void
    private let seekToRatioImpl: (Double) -> Void
    private let seekToRatioAsyncImpl: (Double) async throws -> Void
    private let seekToLiveEdgeImpl: () -> Void
    private let seekToLiveEdgeAsyncImpl: () async throws -> Void
    private let setPlaybackRateImpl: (Float) -> Void
    private let setVideoTrackSelectionImpl: (VesperTrackSelection) -> Void
    private let setAudioTrackSelectionImpl: (VesperTrackSelection) -> Void
    private let setSubtitleTrackSelectionImpl: (VesperTrackSelection) async throws -> Void
    private let subtitleBridgeSnapshotImpl: () -> VesperSubtitleBridgeSnapshot
    private let setSubtitleStyleImpl: (VesperSubtitleStyle) -> Void
    private let setAbrPolicyImpl: (VesperAbrPolicy, Int64?) throws -> Void
    private let setResiliencePolicyImpl: (VesperPlaybackResiliencePolicy) -> Void
    private let setAudioSessionInterruptedImpl: (Bool) -> Void
    private let drainBenchmarkEventsImpl: () -> [VesperBenchmarkEvent]
    private let drainPipelineEventHookReportsImpl: () -> VesperPipelineEventHookReportBatch
    private let benchmarkSummaryImpl: () -> VesperBenchmarkSummary
    private let awaitBenchmarkSinkShutdownImpl: (TimeInterval) async -> Bool
    private let startPerformanceDiagnosticsImpl: (
        VesperPerformanceDiagnosticsConfiguration,
        VesperPerformanceProbe
    ) async throws -> String
    private let updatePerformanceOverlayStateImpl: (
        String,
        VesperPerformanceOverlayState
    ) throws -> Void
    private let recordPerformanceMarkerImpl: (
        String,
        String,
        Double?,
        Int?,
        Bool?
    ) throws -> Void
    private let submitPerformanceFrameSamplesImpl: (
        String,
        [VesperPerformanceFrameSample]
    ) throws -> Void
    private let performanceDiagnosticsSnapshotImpl: (
        String
    ) async throws -> VesperPerformanceDiagnosticsReport
    private let stopPerformanceDiagnosticsImpl: (
        String
    ) async throws -> VesperPerformanceDiagnosticsReport
    private let routePickerPlayerImpl: () -> AVPlayer?
    private let screenSleepToken = VesperScreenSleepToken()
    private var keepScreenOnDuringPlayback: Bool
    private var pendingTimelineOnlyUpdate = false
    private var systemPlaybackCoordinatorStorage: VesperSystemPlaybackCoordinator?
    private var isDisposed = false
    private weak var sequenceAttachment: VesperPlaybackSequenceAttachment?

    private var systemPlaybackCoordinator: VesperSystemPlaybackCoordinator {
        if let coordinator = systemPlaybackCoordinatorStorage {
            return coordinator
        }
        let coordinator = VesperSystemPlaybackCoordinator(controller: self)
        systemPlaybackCoordinatorStorage = coordinator
        return coordinator
    }

    var systemPlaybackCoordinatorForTesting: VesperSystemPlaybackCoordinator? {
        systemPlaybackCoordinatorStorage
    }

    init<Bridge: ObservablePlayerBridge>(
        _ bridge: Bridge,
        keepScreenOnDuringPlayback: Bool = true
    ) {
        backend = bridge.backend
        self.keepScreenOnDuringPlayback = keepScreenOnDuringPlayback
        publishedUiState = bridge.publishedUiState
        publishedTrackCatalog = bridge.publishedTrackCatalog
        publishedTrackSelection = bridge.publishedTrackSelection
        publishedRequestedSubtitleSelection = bridge.publishedRequestedSubtitleSelection
        publishedConfirmedSubtitleSelection = bridge.publishedConfirmedSubtitleSelection
        publishedSubtitleState = bridge.publishedSubtitleState
        publishedEffectiveSubtitleTrackId = bridge.publishedEffectiveSubtitleTrackId
        publishedEffectiveVideoTrackId = bridge.publishedEffectiveVideoTrackId
        publishedVideoVariantObservation = bridge.publishedVideoVariantObservation
        publishedFixedTrackStatus = bridge.publishedFixedTrackStatus
        publishedResiliencePolicy = bridge.publishedResiliencePolicy
        publishedLastError = bridge.publishedLastError
        publishedSubtitleStyle = .default
        pluginDiagnostics = bridge.pluginDiagnostics
        initializeImpl = bridge.initialize
        initializeAsyncImpl = bridge.initializeAsync
        disposeImpl = bridge.dispose
        refreshImpl = bridge.refresh
        sampleTimelineImpl = bridge.sampleTimeline
        selectSourceImpl = bridge.selectSource
        startSourceSelectionImpl = bridge.startSourceSelection
        selectSourceAsyncImpl = bridge.selectSourceAsync
        attachSurfaceHostImpl = { host in
            bridge.attachSurfaceHost(host)
        }
        detachSurfaceHostImpl = bridge.detachSurfaceHost
        detachSurfaceHostForHostImpl = { host in
            bridge.detachSurfaceHost(host)
        }
        playImpl = bridge.play
        pauseImpl = bridge.pause
        togglePauseImpl = bridge.togglePause
        stopImpl = bridge.stop
        seekByImpl = { deltaMs in
            bridge.seek(by: deltaMs)
        }
        seekByAsyncImpl = { deltaMs in
            try await bridge.seekAsync(by: deltaMs)
        }
        seekToRatioImpl = { ratio in
            bridge.seek(toRatio: ratio)
        }
        seekToRatioAsyncImpl = { ratio in
            try await bridge.seekAsync(toRatio: ratio)
        }
        seekToLiveEdgeImpl = bridge.seekToLiveEdge
        seekToLiveEdgeAsyncImpl = bridge.seekToLiveEdgeAsync
        setPlaybackRateImpl = bridge.setPlaybackRate
        setVideoTrackSelectionImpl = bridge.setVideoTrackSelection
        setAudioTrackSelectionImpl = bridge.setAudioTrackSelection
        setSubtitleTrackSelectionImpl = bridge.setSubtitleTrackSelection
        subtitleBridgeSnapshotImpl = {
            VesperSubtitleBridgeSnapshot(
                trackSelection: bridge.publishedTrackSelection,
                requestedSelection: bridge.publishedRequestedSubtitleSelection,
                confirmedSelection: bridge.publishedConfirmedSubtitleSelection,
                effectiveTrackId: bridge.publishedEffectiveSubtitleTrackId,
                state: bridge.publishedSubtitleState,
                lastError: bridge.publishedLastError
            )
        }
        setSubtitleStyleImpl = bridge.setSubtitleStyle
        setAbrPolicyImpl = { policy, expectedCatalogRevision in
            try bridge.setAbrPolicy(
                policy,
                expectedCatalogRevision: expectedCatalogRevision
            )
        }
        setResiliencePolicyImpl = bridge.setResiliencePolicy
        setAudioSessionInterruptedImpl = bridge.setAudioSessionInterrupted
        drainBenchmarkEventsImpl = bridge.drainBenchmarkEvents
        drainPipelineEventHookReportsImpl = bridge.drainPipelineEventHookReports
        benchmarkSummaryImpl = bridge.benchmarkSummary
        awaitBenchmarkSinkShutdownImpl = bridge.awaitBenchmarkSinkShutdown
        startPerformanceDiagnosticsImpl = bridge.startPerformanceDiagnostics
        updatePerformanceOverlayStateImpl = bridge.updatePerformanceOverlayState
        recordPerformanceMarkerImpl = bridge.recordPerformanceMarker
        submitPerformanceFrameSamplesImpl = bridge.submitPerformanceFrameSamples
        performanceDiagnosticsSnapshotImpl = bridge.performanceDiagnosticsSnapshot
        stopPerformanceDiagnosticsImpl = bridge.stopPerformanceDiagnostics
        routePickerPlayerImpl = { bridge.routePickerPlayer }
        bridgeObservation = bridge.objectWillChange.sink { [weak self] _ in
            guard let self else { return }
            let timelineOnlyUpdate = bridge.consumeTimelineOnlyUpdate()
            Task { @MainActor in
                self.pendingTimelineOnlyUpdate =
                    self.pendingTimelineOnlyUpdate || timelineOnlyUpdate
                self.publishedUiState = bridge.publishedUiState
                if timelineOnlyUpdate {
                    self.systemPlaybackCoordinator.updatePlaybackState(self.publishedUiState)
                    self.updateScreenSleepPolicy()
                    return
                }
                self.publishedTrackCatalog = bridge.publishedTrackCatalog
                self.publishedTrackSelection = bridge.publishedTrackSelection
                self.publishedRequestedSubtitleSelection = bridge.publishedRequestedSubtitleSelection
                self.publishedConfirmedSubtitleSelection = bridge.publishedConfirmedSubtitleSelection
                self.publishedEffectiveVideoTrackId = bridge.publishedEffectiveVideoTrackId
                self.publishedVideoVariantObservation = bridge.publishedVideoVariantObservation
                self.publishedFixedTrackStatus = bridge.publishedFixedTrackStatus
                self.publishedResiliencePolicy = bridge.publishedResiliencePolicy
                self.publishedLastError = bridge.publishedLastError
                self.publishedSubtitleState = bridge.publishedSubtitleState
                self.publishedEffectiveSubtitleTrackId = bridge.publishedEffectiveSubtitleTrackId
                self.pluginDiagnostics = bridge.pluginDiagnostics
                self.systemPlaybackCoordinator.updatePlaybackState(self.publishedUiState)
                self.updateScreenSleepPolicy()
            }
        }
        updateScreenSleepPolicy()
    }

    deinit {
        bridgeObservation?.cancel()
        let systemPlaybackCoordinator = systemPlaybackCoordinatorStorage
        let token = screenSleepToken
        let disposeFn = isDisposed ? nil : disposeImpl
        Task { @MainActor in
            systemPlaybackCoordinator?.clear()
            disposeFn?()
            VesperScreenSleepCoordinator.release(token)
        }
    }

    public func initialize() {
        initializeImpl()
    }

    @_spi(VesperFlutter)
    public func initializeAsync() async throws {
        try await initializeAsyncImpl()
    }

    public func dispose() {
        guard !isDisposed else { return }
        isDisposed = true
        let attachment = sequenceAttachment
        sequenceAttachment = nil
        attachment?.onControllerDisposed(self)
        bridgeObservation?.cancel()
        bridgeObservation = nil
        VesperScreenSleepCoordinator.release(screenSleepToken)
        systemPlaybackCoordinatorStorage?.clear()
        disposeImpl()
    }

    public func refresh() {
        refreshImpl()
    }

    @_spi(VesperFlutter)
    public func sampleTimeline() -> TimelineUiState? {
        sampleTimelineImpl()
    }

    @_spi(VesperFlutter)
    public func consumeTimelineOnlyUpdate() -> Bool {
        let pending = pendingTimelineOnlyUpdate
        pendingTimelineOnlyUpdate = false
        return pending
    }

    public func selectSource(_ source: VesperPlayerSource) {
        guard sequenceAttachment == nil else {
            publishedLastError = VesperPlayerError(
                message: "direct source selection is blocked while a playback sequence is attached",
                code: .invalidState,
                category: .playback,
                retriable: false,
                details: ["code": "sequence_attached_conflict"]
            )
            return
        }
        selectSourceImpl(source)
    }

    /// Checked source-selection entry point for hosts that need typed conflict handling.
    public func selectSourceChecked(_ source: VesperPlayerSource) throws {
        guard sequenceAttachment == nil else {
            throw VesperPlayerError(
                message: "direct source selection is blocked while a playback sequence is attached",
                code: .invalidState,
                category: .playback,
                retriable: false,
                details: ["code": "sequence_attached_conflict"]
            )
        }
        selectSourceImpl(source)
    }

    @_spi(VesperFlutter)
    public func startSourceSelection(
        _ source: VesperPlayerSource
    ) throws -> Task<Void, Error> {
        guard sequenceAttachment == nil else {
            throw VesperPlayerError(
                message: "direct source selection is blocked while a playback sequence is attached",
                code: .invalidState,
                category: .playback,
                retriable: false,
                details: ["code": "sequence_attached_conflict"]
            )
        }
        return startSourceSelectionImpl(source)
    }

    @_spi(VesperFlutter)
    public func selectSourceAsync(_ source: VesperPlayerSource) async throws {
        guard sequenceAttachment == nil else {
            throw VesperPlayerError(
                message: "direct source selection is blocked while a playback sequence is attached",
                code: .invalidState,
                category: .playback,
                retriable: false,
                details: ["code": "sequence_attached_conflict"]
            )
        }
        try await selectSourceAsyncImpl(source)
    }

    internal func attachPlaybackSequence(_ attachment: VesperPlaybackSequenceAttachment) throws {
        guard !isDisposed else {
            throw VesperPlayerError(
                message: "player controller has been disposed",
                code: .invalidState,
                category: .playback,
                retriable: false,
                details: ["code": "controller_disposed"]
            )
        }
        guard sequenceAttachment == nil else {
            throw VesperPlayerError(
                message: "player controller already has a playback sequence",
                code: .invalidState,
                category: .playback,
                retriable: false,
                details: ["code": "already_attached"]
            )
        }
        sequenceAttachment = attachment
    }

    internal func detachPlaybackSequence(_ attachment: VesperPlaybackSequenceAttachment) {
        if sequenceAttachment === attachment {
            sequenceAttachment = nil
        }
    }

    internal func activateSequenceSource(
        _ attachment: VesperPlaybackSequenceAttachment,
        source: VesperPlayerSource
    ) throws {
        guard sequenceAttachment === attachment else {
            throw VesperPlayerError(
                message: "sequence no longer owns the player controller",
                code: .invalidState,
                category: .playback,
                retriable: false,
                details: ["code": "sequence_attached_conflict"]
            )
        }
        selectSourceImpl(source)
    }

    public func attachSurfaceHost(_ host: UIView) {
        attachSurfaceHostImpl(host)
    }

    public func detachSurfaceHost() {
        detachSurfaceHostImpl()
    }

    func detachSurfaceHost(_ host: UIView) {
        detachSurfaceHostForHostImpl(host)
    }

    public func play() {
        playImpl()
    }

    public func pause() {
        pauseImpl()
    }

    public func togglePause() {
        togglePauseImpl()
    }

    public func stop() {
        stopImpl()
    }

    public func seek(by deltaMs: Int64) {
        seekByImpl(deltaMs)
    }

    @_spi(VesperFlutter)
    public func seekAsync(by deltaMs: Int64) async throws {
        try await seekByAsyncImpl(deltaMs)
    }

    public func seek(toRatio ratio: Double) {
        seekToRatioImpl(ratio)
    }

    @_spi(VesperFlutter)
    public func seekAsync(toRatio ratio: Double) async throws {
        try await seekToRatioAsyncImpl(ratio)
    }

    public func seekToLiveEdge() {
        seekToLiveEdgeImpl()
    }

    @_spi(VesperFlutter)
    public func seekToLiveEdgeAsync() async throws {
        try await seekToLiveEdgeAsyncImpl()
    }

    public func setPlaybackRate(_ rate: Float) {
        setPlaybackRateImpl(rate)
    }

    public func setVideoTrackSelection(_ selection: VesperTrackSelection) {
        setVideoTrackSelectionImpl(selection)
    }

    public func setAudioTrackSelection(_ selection: VesperTrackSelection) {
        setAudioTrackSelectionImpl(selection)
    }

    public func setSubtitleTrackSelection(_ selection: VesperTrackSelection) async throws {
        do {
            try await setSubtitleTrackSelectionImpl(selection)
            synchronizeSubtitleStateFromBridge()
        } catch {
            synchronizeSubtitleStateFromBridge()
            throw error
        }
    }

    private func synchronizeSubtitleStateFromBridge() {
        let snapshot = subtitleBridgeSnapshotImpl()
        publishedTrackSelection = snapshot.trackSelection
        publishedRequestedSubtitleSelection = snapshot.requestedSelection
        publishedConfirmedSubtitleSelection = snapshot.confirmedSelection
        publishedEffectiveSubtitleTrackId = snapshot.effectiveTrackId
        publishedSubtitleState = snapshot.state
        publishedLastError = snapshot.lastError
    }

    /// Updates subtitle styling (font scale, visibility). Hosts observing
    /// `subtitleStyle` should apply the new value to their subtitle overlay.
    public func setSubtitleStyle(_ style: VesperSubtitleStyle) {
        guard style.fontScale.isFinite, (0.5...3.0).contains(style.fontScale) else {
            return
        }
        setSubtitleStyleImpl(style)
        publishedSubtitleStyle = style
    }

    /// Applies adaptive bitrate behavior for the active source.
    ///
    /// On iOS, `fixedTrack` maps to best-effort HLS variant pinning. Single-axis
    /// constrained resolution requests also wait for the current HLS variant
    /// catalog before the missing dimension can be inferred.
    public func setAbrPolicy(_ policy: VesperAbrPolicy) {
        try? setAbrPolicy(policy, expectedCatalogRevision: nil)
    }

    /// Applies an ABR policy with an optional catalog revision precondition.
    /// A fixed-track rejection is thrown before AVPlayer selection state is
    /// modified.
    public func setAbrPolicy(
        _ policy: VesperAbrPolicy,
        expectedCatalogRevision: Int64?
    ) throws {
        try setAbrPolicyImpl(policy, expectedCatalogRevision)
    }

    public func setResiliencePolicy(_ policy: VesperPlaybackResiliencePolicy) {
        setResiliencePolicyImpl(policy)
    }

    func setAudioSessionInterrupted(_ interrupted: Bool) {
        setAudioSessionInterruptedImpl(interrupted)
    }

    public func setKeepScreenOnDuringPlayback(_ enabled: Bool) {
        keepScreenOnDuringPlayback = enabled
        updateScreenSleepPolicy()
    }

    public func configureSystemPlayback(_ configuration: VesperSystemPlaybackConfiguration) {
        systemPlaybackCoordinator.configure(configuration)
    }

    public func updateSystemPlaybackMetadata(_ metadata: VesperSystemPlaybackMetadata) {
        systemPlaybackCoordinator.updateMetadata(metadata)
    }

    public func clearSystemPlayback() {
        systemPlaybackCoordinator.clear()
    }

    public var routePickerPlayer: AVPlayer? {
        routePickerPlayerImpl()
    }

    public static func requestSystemPlaybackPermissions() -> VesperSystemPlaybackPermissionStatus {
        .notRequired
    }

    public static func getSystemPlaybackPermissionStatus() -> VesperSystemPlaybackPermissionStatus {
        .notRequired
    }

    public func drainBenchmarkEvents() -> [VesperBenchmarkEvent] {
        drainBenchmarkEventsImpl()
    }

    /// Drains structured reports produced by the configured playback EventHook plugins.
    public func drainPipelineEventHookReports() -> VesperPipelineEventHookReportBatch {
        drainPipelineEventHookReportsImpl()
    }

    public func benchmarkSummary() -> VesperBenchmarkSummary {
        benchmarkSummaryImpl()
    }

    public func startPerformanceDiagnostics(
        configuration: VesperPerformanceDiagnosticsConfiguration =
            VesperPerformanceDiagnosticsConfiguration()
    ) async throws -> VesperPerformanceDiagnosticsSession {
        try await startPerformanceDiagnostics(
            probe: .iosDisplayLink,
            configuration: configuration
        )
    }

    public func startPerformanceDiagnostics(
        probe: VesperPerformanceProbe,
        configuration: VesperPerformanceDiagnosticsConfiguration =
            VesperPerformanceDiagnosticsConfiguration()
    ) async throws -> VesperPerformanceDiagnosticsSession {
        try ensurePerformanceDiagnosticsControllerActive()
        let runId = try await startPerformanceDiagnosticsImpl(configuration, probe)
        try ensurePerformanceDiagnosticsControllerActive()
        return VesperPerformanceDiagnosticsSession(controller: self, runId: runId)
    }

    func updatePerformanceOverlayState(
        runId: String,
        state: VesperPerformanceOverlayState
    ) throws {
        try ensurePerformanceDiagnosticsControllerActive()
        try updatePerformanceOverlayStateImpl(runId, state)
    }

    func recordPerformanceMarker(
        runId: String,
        name: String,
        value: Double?,
        sequenceIndex: Int?,
        expectedOverlayActive: Bool?
    ) throws {
        try ensurePerformanceDiagnosticsControllerActive()
        try recordPerformanceMarkerImpl(
            runId,
            name,
            value,
            sequenceIndex,
            expectedOverlayActive
        )
    }

    func submitPerformanceFrameSamples(
        runId: String,
        samples: [VesperPerformanceFrameSample]
    ) throws {
        try ensurePerformanceDiagnosticsControllerActive()
        try submitPerformanceFrameSamplesImpl(runId, samples)
    }

    func performanceDiagnosticsSnapshot(
        runId: String
    ) async throws -> VesperPerformanceDiagnosticsReport {
        try ensurePerformanceDiagnosticsControllerActive()
        return try await performanceDiagnosticsSnapshotImpl(runId)
    }

    func stopPerformanceDiagnostics(
        runId: String
    ) async throws -> VesperPerformanceDiagnosticsReport {
        try await stopPerformanceDiagnosticsImpl(runId)
    }

    @_spi(VesperFlutter)
    public func awaitBenchmarkSinkShutdown(timeout: TimeInterval) async -> Bool {
        await awaitBenchmarkSinkShutdownImpl(timeout)
    }

    private func ensurePerformanceDiagnosticsControllerActive() throws {
        guard !isDisposed else {
            throw VesperPerformanceDiagnosticsError(
                code: .controllerDisposed,
                message: "The player controller has been disposed."
            )
        }
    }

    /// Playback rates exposed by the current iOS host surface.
    public static let supportedPlaybackRates: [Float] = [0.5, 1.0, 1.5, 2.0, 3.0]

    private func updateScreenSleepPolicy() {
        VesperScreenSleepCoordinator.setActive(
            keepScreenOnDuringPlayback && publishedUiState.playbackState == .playing,
            for: screenSleepToken
        )
    }
}

private final class VesperScreenSleepToken {}

@MainActor
private enum VesperScreenSleepCoordinator {
    private static var activeTokens: Set<ObjectIdentifier> = []
    private static var previousIdleTimerDisabled: Bool?

    static func setActive(_ active: Bool, for token: VesperScreenSleepToken) {
        let identifier = ObjectIdentifier(token)
        if active {
            let wasEmpty = activeTokens.isEmpty
            activeTokens.insert(identifier)
            if wasEmpty {
                previousIdleTimerDisabled = UIApplication.shared.isIdleTimerDisabled
                UIApplication.shared.isIdleTimerDisabled = true
            }
            return
        }

        activeTokens.remove(identifier)
        guard activeTokens.isEmpty else { return }
        UIApplication.shared.isIdleTimerDisabled = previousIdleTimerDisabled ?? false
        previousIdleTimerDisabled = nil
    }

    static func release(_ token: VesperScreenSleepToken) {
        setActive(false, for: token)
    }
}
