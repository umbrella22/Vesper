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
    @Published var publishedEffectiveVideoTrackId: String?
    @Published var publishedVideoVariantObservation: VesperVideoVariantObservation?
    @Published var publishedFixedTrackStatus: VesperFixedTrackStatus?
    @Published var publishedResiliencePolicy: VesperPlaybackResiliencePolicy
    @Published var publishedLastError: VesperPlayerError?
    @Published var publishedSubtitleState: VesperSubtitleState = .empty

    var currentSource: VesperPlayerSource?
    var player: AVPlayer?
    let subtitleOverlayRenderer = VesperSubtitleOverlayRenderer()
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
    var currentResiliencePolicy: VesperPlaybackResiliencePolicy
    let trackPreferencePolicy: VesperTrackPreferencePolicy
    var resolvedTrackPreferencePolicy: VesperTrackPreferencePolicy
    var hasAppliedDefaultTrackPreferences = false
    var pendingResilienceRestore: PendingResilienceRestore?
    var retryTask: Task<Void, Never>?
    var stopSeekTimeoutTask: Task<Void, Never>?
    var sourceLoadTask: Task<Void, Never>?
    var sourceLoadEpoch: UInt64 = 0
    var retryAttemptCount = 0
    let cachePolicyToken = UUID()
    let preloadCoordinator: VesperNativePreloadCoordinator
    let benchmarkRecorder: VesperBenchmarkRecorder
    let sourceNormalizerConfiguration: VesperSourceNormalizerConfiguration
    let frameProcessorConfiguration: VesperFrameProcessorConfiguration
    let nativeFramePipelineConfiguration: VesperNativeFramePipelineConfiguration
    var currentPluginDiagnostics: [[String: Any]]
    let nativeFramePipelineCoordinator: VesperNativeFramePipelineCoordinator
    var nativeFramePipelineFallbackIssue: VesperNativeFramePipelineIssue?
    var currentHdrFailureEvidence: VesperNativeHdrFailureEvidence?
    var fixedTrackConvergenceState: FixedTrackConvergenceState?
    var fixedTrackIssueActive = false
    var audioSessionActive = false

    var uiState: PlayerHostUiState {
        publishedUiState
    }

    var trackCatalog: VesperTrackCatalog {
        publishedTrackCatalog
    }

    var trackSelection: VesperTrackSelectionSnapshot {
        publishedTrackSelection
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

    func recordBenchmark(
        _ eventName: String,
        attributes: [String: String] = [:]
    ) {
        benchmarkRecorder.record(
            eventName,
            sourceProtocol: currentSource?.protocol,
            attributes: attributes
        )
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
        nativeFramePipelineCoordinator: VesperNativeFramePipelineCoordinator? = nil
    ) {
        currentSource = initialSource
        currentResiliencePolicy = resiliencePolicy
        self.trackPreferencePolicy = trackPreferencePolicy
        resolvedTrackPreferencePolicy = trackPreferencePolicy.resolvedForRuntime()
        self.sourceNormalizerConfiguration = sourceNormalizerConfiguration
        self.frameProcessorConfiguration = frameProcessorConfiguration
        self.nativeFramePipelineConfiguration = nativeFramePipelineConfiguration
        self.nativeFramePipelineCoordinator = nativeFramePipelineCoordinator ?? VesperNativeFramePipelineCoordinator()
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
        currentPluginDiagnostics = nativeFramePipelineDiagnostics()
    }
}
