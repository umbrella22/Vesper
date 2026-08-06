@preconcurrency import AVFoundation
import Foundation
import UIKit
internal import VesperPlayerKitBridgeShim

@MainActor
final class VesperNativePlayerBridge: ObservableObject, ObservablePlayerBridge {
    let backend: PlayerBridgeBackend = .rustNativeStub
    static let dashStartupAbrPeakBitRate = 800_000.0
    static let dashStartupAbrMaxWidth = 1280
    static let dashStartupAbrMaxHeight = 720

    @Published var publishedUiState: PlayerHostUiState
    @Published var publishedTrackCatalog: VesperTrackCatalog
    @Published var publishedTrackSelection: VesperTrackSelectionSnapshot
    @Published var publishedRequestedSubtitleSelection: VesperTrackSelection = .disabled()
    @Published var publishedConfirmedSubtitleSelection: VesperTrackSelection = .disabled()
    @Published var publishedEffectiveVideoTrackId: String?
    @Published var publishedVideoVariantObservation: VesperVideoVariantObservation?
    @Published var publishedFixedTrackStatus: VesperFixedTrackStatus?
    @Published var publishedResiliencePolicy: VesperPlaybackResiliencePolicy
    @Published var publishedLastError: VesperPlayerError?
    @Published var publishedSubtitleState: VesperSubtitleState = .empty
    @Published var publishedEffectiveSubtitleTrackId: String?

    var currentSource: VesperPlayerSource?
    var player: AVPlayer?
    let subtitleOverlayRenderer = VesperSubtitleOverlayRenderer()
    var pendingSubtitleOverlayFailure: VesperSubtitleOverlayRenderer.PreparationFailure?
    var currentSubtitleStyle = VesperSubtitleStyle.default
    var currentDashSession: VesperDashSession?
    var dashResourceLoaderDelegate: VesperDashResourceLoaderDelegate?
    var currentSourceNormalizerResource: VesperSourceNormalizerResourceOpenResult?
    var sourceNormalizerResourceSession: VesperSourceNormalizerResourceSession?
    var sourceNormalizerResourceLoaderDelegate: VesperSourceNormalizerResourceLoaderDelegate?
    var fairPlayDrmCoordinator: VesperFairPlayDrmCoordinator?
    var fairPlayDrmCoordinatorId: UUID?
    weak var surfaceHost: PlayerSurfaceView?
    var timeObserverToken: Any?
    var endObserver: NSObjectProtocol?
    var playbackStalledObserver: NSObjectProtocol?
    var didLogLinkedPluginAbiSummary = false
    var pendingAutoPlay = false
    var pendingNativeFrameSurfaceLoad = false
    var pendingNativeFrameSeek: PendingNativeFrameSeek?
    var playbackEpoch: UInt64 = 0
    var firstFrameRenderedPlaybackEpoch: UInt64?
    var readyForDisplayCountByEpoch: [UInt64: Int] = [:]
    var timeControlObservation: NSKeyValueObservation?
    var itemStatusObservation: NSKeyValueObservation?
    var itemBufferEmptyObservation: NSKeyValueObservation?
    var itemLikelyToKeepUpObservation: NSKeyValueObservation?
    var desiredPlaybackRate: Float = 1.0
    var isSeekingToStartAfterStop = false
    var pendingPlayAfterStopSeek = false
    var pendingPlaybackStart = false
    var audioGroup: AVMediaSelectionGroup?
    var subtitleGroup: AVMediaSelectionGroup?
    var videoVariantPinsByTrackId: [String: LoadedVideoVariantPin] = [:]
    var desiredVideoVariantPin: LoadedVideoVariantPin?
    var dashStartupAbrLimitPin: LoadedVideoVariantPin?
    var dashStartupAbrLimitAppliedAtNs: UInt64?
    var audioOptionsByTrackId: [String: AVMediaSelectionOption] = [:]
    var subtitleOptionsByTrackId: [String: AVMediaSelectionOption] = [:]
    /// DASH subtitle renditions that failed resource loading in the current
    /// source epoch. They remain advertised by the manifest but are removed
    /// from the selectable catalog until the source is refreshed.
    var failedSubtitleTrackIds = Set<String>()
    var currentResiliencePolicy: VesperPlaybackResiliencePolicy
    let trackPreferencePolicy: VesperTrackPreferencePolicy
    var resolvedTrackPreferencePolicy: VesperTrackPreferencePolicy
    var hasAppliedDefaultTrackPreferences = false
    var pendingResilienceRestore: PendingResilienceRestore?
    var retryTask: Task<Void, Never>?
    var stopSeekTimeoutTask: Task<Void, Never>?
    var sourceLoadTask: Task<Void, Never>?
    var subtitleOverlayLoadTask: Task<Void, Never>?
    var sourceLoadEpoch: UInt64 = 0
    var subtitleSourceEpoch: UInt64 = 0
    var nextSubtitleCommandId: UInt64 = 0
    var pendingSubtitleSelection: PendingSubtitleSelection?
    var subtitleSelectionTask: Task<Void, Error>?
    let subtitleSelectionWaitPolicy: VesperSubtitleSelectionWaitPolicy
    var trackCatalogLoadGeneration: UInt64 = 0
    var confirmedSubtitleSelection: VesperTrackSelection = .disabled()
    var explicitSubtitleIntentSourceEpoch: UInt64?
    var latestConfirmedExplicitSubtitleSelection: (
        sourceEpoch: UInt64,
        selection: VesperTrackSelection
    )?
    var retryAttemptCount = 0
    let cachePolicyToken = UUID()
    let preloadCoordinator: VesperNativePreloadCoordinator
    let benchmarkRecorder: VesperBenchmarkRecorder
    let sourceNormalizerConfiguration: VesperSourceNormalizerConfiguration
    let frameProcessorConfiguration: VesperFrameProcessorConfiguration
    let nativeFramePipelineConfiguration: VesperNativeFramePipelineConfiguration
    let pipelineEventHookConfiguration: VesperPipelineEventHookConfiguration
    let pipelineEventHookSession: VesperPlaybackEventHookSession?
    let pipelineEventRunId: String
    let pipelineEventSessionId: String
    let pipelineEventResourceIdentity: String
    var currentPluginDiagnostics: [[String: Any]]
    var finalizedPipelineEventHookReports: VesperPipelineEventHookReportBatch? = nil
    /// Set immediately before a periodic AVPlayer update publishes its state.
    /// The Flutter-facing controller consumes this marker to suppress a full
    /// snapshot while native hosts still observe playback and Now Playing
    /// updates.
    var timelineOnlyUpdatePending = false
    let nativeFramePipelineCoordinator: VesperNativeFramePipelineCoordinator
    var nativeFramePipelineFallbackIssue: VesperNativeFramePipelineIssue?
    var currentHdrFailureEvidence: VesperNativeHdrFailureEvidence?
    var fixedTrackConvergenceState: FixedTrackConvergenceState?
    var fixedTrackIssueActive = false
    let audioSessionLease = VesperSharedAudioSessionLease()

    var uiState: PlayerHostUiState {
        publishedUiState
    }

    var trackCatalog: VesperTrackCatalog {
        publishedTrackCatalog
    }

    var trackSelection: VesperTrackSelectionSnapshot {
        publishedTrackSelection
    }

    var requestedSubtitleSelection: VesperTrackSelection {
        publishedRequestedSubtitleSelection
    }

    var effectiveVideoTrackId: String? {
        publishedEffectiveVideoTrackId
    }

    var videoVariantObservation: VesperVideoVariantObservation? {
        publishedVideoVariantObservation
    }

    var fixedTrackStatus: VesperFixedTrackStatus? {
        publishedFixedTrackStatus
    }

    var resiliencePolicy: VesperPlaybackResiliencePolicy {
        publishedResiliencePolicy
    }

    var lastError: VesperPlayerError? {
        publishedLastError
    }

    var pluginDiagnostics: [[String: Any]] {
        currentPluginDiagnostics
    }

    var routePickerPlayer: AVPlayer? {
        player
    }

    func sampleTimeline() -> TimelineUiState? {
        VesperPlaybackTrace.interval("VesperRefresh#sampleTimeline") {
            if let nativeSession = nativeFramePipelineCoordinator.activeSession {
                return nativeFrameTimelineState(
                    positionMs: publishedUiState.timeline.positionMs,
                    durationMs: nativeSession.durationMs ?? publishedUiState.timeline.durationMs
                )
            }
            guard player != nil else {
                return nil
            }
            return currentTimelineState()
        }
    }

    func consumeTimelineOnlyUpdate() -> Bool {
        let pending = timelineOnlyUpdatePending
        timelineOnlyUpdatePending = false
        return pending
    }

    func recordBenchmark(
        _ eventName: String,
        attributes: [String: String] = [:]
    ) {
        benchmarkRecorder.record(
            eventName,
            sourceProtocol: currentSource?.protocol,
            attributes: attributes
        )
        _ = pipelineEventHookSession?.submit(
            runId: pipelineEventRunId,
            sessionId: pipelineEventSessionId,
            protocolName: currentSource?.protocol.rawValue,
            eventName: eventName,
            timestampNs: DispatchTime.now().uptimeNanoseconds,
            resourceIdentity: pipelineEventResourceIdentity,
            attributes: sanitizedPipelineEventAttributes(attributes)
        )
    }

    func drainPipelineEventHookReports() -> VesperPipelineEventHookReportBatch {
        if let finalized = finalizedPipelineEventHookReports {
            finalizedPipelineEventHookReports = nil
            return finalized
        }
        return pipelineEventHookSession?.drainReports() ?? VesperPipelineEventHookReportBatch()
    }

    /// EventHook attributes are a separate, redacted contract from benchmark
    /// diagnostics. Never forward free-form paths, URLs, or error text.
    private func sanitizedPipelineEventAttributes(
        _ attributes: [String: String]
    ) -> [String: String] {
        let forbiddenKeys = Set(["error", "url", "sourceUri", "path", "resourcePath"])
        return attributes
            .filter { key, _ in !forbiddenKeys.contains(key) }
            .sorted { $0.key < $1.key }
            .prefix(32)
            .reduce(into: [String: String]()) { result, item in
                result[item.key] = String(item.value.prefix(256))
            }
    }

    init(
        initialSource: VesperPlayerSource? = nil,
        resiliencePolicy: VesperPlaybackResiliencePolicy = VesperPlaybackResiliencePolicy(),
        trackPreferencePolicy: VesperTrackPreferencePolicy = VesperTrackPreferencePolicy(),
        preloadBudgetPolicy: VesperPreloadBudgetPolicy = VesperPreloadBudgetPolicy(),
        benchmarkConfiguration: VesperBenchmarkConfiguration = .disabled,
        sourceNormalizerConfiguration: VesperSourceNormalizerConfiguration =
            VesperSourceNormalizerConfiguration(),
        frameProcessorConfiguration: VesperFrameProcessorConfiguration =
            VesperFrameProcessorConfiguration(),
        nativeFramePipelineConfiguration: VesperNativeFramePipelineConfiguration =
            VesperNativeFramePipelineConfiguration(),
        pipelineEventHookConfiguration: VesperPipelineEventHookConfiguration =
            VesperPipelineEventHookConfiguration(),
        nativeFramePipelineCoordinator: VesperNativeFramePipelineCoordinator? = nil,
        subtitleSelectionWaitPolicy: VesperSubtitleSelectionWaitPolicy = .production
    ) {
        currentSource = initialSource
        currentResiliencePolicy = resiliencePolicy
        self.trackPreferencePolicy = trackPreferencePolicy
        resolvedTrackPreferencePolicy = trackPreferencePolicy.resolvedForRuntime()
        self.sourceNormalizerConfiguration = sourceNormalizerConfiguration
        self.frameProcessorConfiguration = frameProcessorConfiguration
        self.nativeFramePipelineConfiguration = nativeFramePipelineConfiguration
        self.pipelineEventHookConfiguration = pipelineEventHookConfiguration
        let pipelineEventIdentity = UUID().uuidString.lowercased()
        pipelineEventRunId = "playback-run:\(pipelineEventIdentity)"
        pipelineEventSessionId = "playback-session:\(pipelineEventIdentity)"
        pipelineEventResourceIdentity = "playback-session:\(pipelineEventIdentity)"
        if pipelineEventHookConfiguration.isDisabled {
            pipelineEventHookSession = nil
        } else {
            do {
                pipelineEventHookSession = try VesperPlaybackEventHookSession(
                    configuration: pipelineEventHookConfiguration
                )
            } catch {
                pipelineEventHookSession = nil
                iosHostLog("playback EventHook session create failed: \(error.localizedDescription)")
            }
        }
        self.nativeFramePipelineCoordinator = nativeFramePipelineCoordinator ?? VesperNativeFramePipelineCoordinator()
        self.subtitleSelectionWaitPolicy = subtitleSelectionWaitPolicy
        currentPluginDiagnostics = []
        benchmarkRecorder = VesperBenchmarkRecorder(configuration: benchmarkConfiguration)
        preloadCoordinator = VesperNativePreloadCoordinator(
            budgetPolicy: preloadBudgetPolicy.resolvedForRuntime()
        )
        publishedUiState = PlayerHostUiState(
            title: VesperPlayerI18n.playerTitle,
            subtitle: VesperPlayerI18n.nativeBridgeReady,
            sourceLabel: initialSource?.label ?? VesperPlayerI18n.noSourceSelected,
            playbackState: .ready,
            playbackRate: 1.0,
            isBuffering: false,
            isInterrupted: false,
            timeline: TimelineUiState(
                kind: .vod,
                isSeekable: true,
                seekableRange: SeekableRangeUi(startMs: 0, endMs: 0),
                liveEdgeMs: nil,
                positionMs: 0,
                durationMs: nil
            )
        )
        publishedTrackCatalog = .empty
        publishedTrackSelection = VesperTrackSelectionSnapshot()
        publishedEffectiveVideoTrackId = nil
        publishedVideoVariantObservation = nil
        publishedFixedTrackStatus = nil
        publishedResiliencePolicy = resiliencePolicy
        publishedLastError = nil
        publishedSubtitleState = .empty
        publishedEffectiveSubtitleTrackId = nil
        pendingSubtitleOverlayFailure = nil
        currentPluginDiagnostics = nativeFramePipelineDiagnostics()
    }
}
