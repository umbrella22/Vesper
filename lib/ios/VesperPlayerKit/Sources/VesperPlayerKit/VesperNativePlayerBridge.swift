@preconcurrency import AVFoundation
import Foundation
import UIKit
import VesperPlayerKitBridgeShim

@MainActor
final class VesperNativePlayerBridge: ObservableObject, ObservablePlayerBridge {
    let backend: PlayerBridgeBackend = .rustNativeStub
    private static let dashStartupAbrPeakBitRate = 800_000.0
    private static let dashStartupAbrMaxWidth = 1280
    private static let dashStartupAbrMaxHeight = 720

    @Published private(set) var publishedUiState: PlayerHostUiState
    @Published private(set) var publishedTrackCatalog: VesperTrackCatalog
    @Published private(set) var publishedTrackSelection: VesperTrackSelectionSnapshot
    @Published private(set) var publishedEffectiveVideoTrackId: String?
    @Published private(set) var publishedVideoVariantObservation: VesperVideoVariantObservation?
    @Published private(set) var publishedFixedTrackStatus: VesperFixedTrackStatus?
    @Published private(set) var publishedResiliencePolicy: VesperPlaybackResiliencePolicy
    @Published private(set) var publishedLastError: VesperPlayerError?

    private var currentSource: VesperPlayerSource?
    private var player: AVPlayer?
    private var currentDashSession: VesperDashSession?
    private var dashResourceLoaderDelegate: VesperDashResourceLoaderDelegate?
    private weak var surfaceHost: PlayerSurfaceView?
    private var timeObserverToken: Any?
    private var endObserver: NSObjectProtocol?
    private var playbackStalledObserver: NSObjectProtocol?
    private var pendingAutoPlay = false
    private var playbackEpoch: UInt64 = 0
    private var timeControlObservation: NSKeyValueObservation?
    private var itemStatusObservation: NSKeyValueObservation?
    private var itemBufferEmptyObservation: NSKeyValueObservation?
    private var itemLikelyToKeepUpObservation: NSKeyValueObservation?
    private var desiredPlaybackRate: Float = 1.0
    private var isSeekingToStartAfterStop = false
    private var pendingPlayAfterStopSeek = false
    private var pendingPlaybackStart = false
    private var audioGroup: AVMediaSelectionGroup?
    private var subtitleGroup: AVMediaSelectionGroup?
    private var videoVariantPinsByTrackId: [String: LoadedVideoVariantPin] = [:]
    private var desiredVideoVariantPin: LoadedVideoVariantPin?
    private var dashStartupAbrLimitPin: LoadedVideoVariantPin?
    private var audioOptionsByTrackId: [String: AVMediaSelectionOption] = [:]
    private var subtitleOptionsByTrackId: [String: AVMediaSelectionOption] = [:]
    private var currentResiliencePolicy: VesperPlaybackResiliencePolicy
    private let trackPreferencePolicy: VesperTrackPreferencePolicy
    private var resolvedTrackPreferencePolicy: VesperTrackPreferencePolicy
    private var hasAppliedDefaultTrackPreferences = false
    private var pendingResilienceRestore: PendingResilienceRestore?
    private var retryTask: Task<Void, Never>?
    private var retryAttemptCount = 0
    private let cachePolicyToken = UUID()
    private let preloadCoordinator: VesperNativePreloadCoordinator
    private let benchmarkRecorder: VesperBenchmarkRecorder
    private var fixedTrackConvergenceState: FixedTrackConvergenceState?
    private var fixedTrackIssueActive = false

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

    private func recordBenchmark(
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
        benchmarkConfiguration: VesperBenchmarkConfiguration = .disabled
    ) {
        currentSource = initialSource
        currentResiliencePolicy = resiliencePolicy
        self.trackPreferencePolicy = trackPreferencePolicy
        resolvedTrackPreferencePolicy = trackPreferencePolicy.resolvedForRuntime()
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
    }

    func initialize() {
        clearLastError()
        recordBenchmark("initialize_start")
        guard let currentSource else {
            recordBenchmark("initialize_without_source")
            updateState {
                PlayerHostUiState(
                    title: $0.title,
                    subtitle: VesperPlayerI18n.selectSourcePrompt,
                    sourceLabel: VesperPlayerI18n.noSourceSelected,
                    playbackState: .ready,
                    playbackRate: $0.playbackRate,
                    isBuffering: false,
                    isInterrupted: $0.isInterrupted,
                    timeline: TimelineUiState(
                        kind: .vod,
                        isSeekable: true,
                        seekableRange: SeekableRangeUi(startMs: 0, endMs: 0),
                        liveEdgeMs: nil,
                        positionMs: 0,
                        durationMs: nil
                    )
                )
            }
            return
        }
        let shouldAutoPlay = pendingAutoPlay || player == nil
        iosHostLog(
            "initialize source=\(currentSource.uri) label=\(currentSource.label) kind=\(currentSource.kind.rawValue) protocol=\(currentSource.protocol.rawValue) autoPlay=\(shouldAutoPlay)"
        )
        do {
            configureAudioSessionIfNeeded()
            try loadCurrentSource()
            pendingAutoPlay = false
            if shouldAutoPlay {
                iosHostLog("auto-playing source=\(currentSource.uri)")
                startPlayback()
            }
            refreshPlaybackState()
            recordBenchmark("initialize_completed")
        } catch {
            pendingAutoPlay = false
            iosHostLog("initialize failed: \(error.localizedDescription)")
            recordBenchmark(
                "initialize_failed",
                attributes: ["error": error.localizedDescription]
            )
            handlePlaybackFailure(error: error, fallbackMessage: error.localizedDescription)
        }
    }

    func dispose() {
        clearLastError()
        recordBenchmark("dispose_command")
        iosHostLog("dispose")
        cancelPendingRetry(resetAttempts: true)
        pendingResilienceRestore = nil
        currentSource = nil
        hasAppliedDefaultTrackPreferences = false
        pendingAutoPlay = false
        tearDownActivePlayback()
        benchmarkRecorder.dispose()
    }

    func refresh() {
        refreshPlaybackState()
    }

    func selectSource(_ source: VesperPlayerSource) {
        clearLastError()
        recordBenchmark(
            "select_source_start",
            attributes: ["targetProtocol": source.protocol.rawValue]
        )
        iosHostLog(
            "selectSource source=\(source.uri) label=\(source.label) kind=\(source.kind.rawValue) protocol=\(source.protocol.rawValue)"
        )
        currentSource = source
        cancelPendingRetry(resetAttempts: true)
        pendingResilienceRestore = nil
        pendingAutoPlay = true
        tearDownActivePlayback()
        updateState {
            PlayerHostUiState(
                title: $0.title,
                subtitle: sourceSubtitle(for: source),
                sourceLabel: source.label,
                playbackState: .ready,
                playbackRate: $0.playbackRate,
                isBuffering: false,
                isInterrupted: $0.isInterrupted,
                timeline: TimelineUiState(
                    kind: .vod,
                    isSeekable: true,
                    seekableRange: SeekableRangeUi(startMs: 0, endMs: 0),
                    liveEdgeMs: nil,
                    positionMs: 0,
                    durationMs: nil
                )
            )
        }
        initialize()
    }

    private func tearDownActivePlayback() {
        _ = advancePlaybackEpoch()
        preloadCoordinator.cancelAll()
        VesperSharedUrlCacheCoordinator.shared.remove(token: cachePolicyToken)
        pendingPlaybackStart = false
        pendingPlayAfterStopSeek = false
        isSeekingToStartAfterStop = false
        removeObservers()
        player?.pause()
        surfaceHost?.attach(player: nil)
        player = nil
        currentDashSession = nil
        dashResourceLoaderDelegate = nil
        resetTrackState()
    }

    func attachSurfaceHost(_ host: UIView) {
        guard let host = host as? PlayerSurfaceView else {
            return
        }
        recordBenchmark("attach_surface_host")
        if surfaceHost !== host {
            iosHostLog("attachSurfaceHost")
        }
        if surfaceHost !== host {
            surfaceHost?.onReadyForDisplay = nil
        }
        surfaceHost = host
        host.onReadyForDisplay = { [weak self] in
            Task { @MainActor in
                iosHostLog("surfaceReadyForDisplay")
                self?.recordBenchmark("ready_for_display")
                self?.recordBenchmark("first_frame_rendered")
                self?.releaseDashStartupAbrLimitIfNeeded(reason: "readyForDisplay", item: nil)
                self?.attemptPendingPlaybackStart(reason: "surfaceReadyForDisplay")
            }
        }
        host.attach(player: player)
        attemptPendingPlaybackStart(reason: "attachSurfaceHost")
    }

    func detachSurfaceHost() {
        iosHostLog("detachSurfaceHost")
        recordBenchmark("detach_surface_host")
        surfaceHost?.onReadyForDisplay = nil
        surfaceHost?.attach(player: nil)
        surfaceHost = nil
    }

    func play() {
        clearLastError()
        recordBenchmark("play_command")
        if player == nil {
            pendingAutoPlay = true
            initialize()
            return
        }

        if isSeekingToStartAfterStop {
            iosHostLog("play deferred until stop seek completes")
            pendingPlayAfterStopSeek = true
            return
        }

        iosHostLog("play")
        startPlayback()
        refreshPlaybackState()
    }

    private func startPlayback() {
        guard let player else { return }
        recordBenchmark("start_playback_attempt")
        if publishedUiState.playbackState == .finished {
            player.seek(to: .zero)
        }

        if let deferralReason = playbackStartDeferralReason(player) {
            pendingPlaybackStart = true
            recordBenchmark(
                "start_playback_deferred",
                attributes: ["reason": deferralReason]
            )
            iosHostLog("deferring playback until \(deferralReason)")
            return
        }

        pendingPlaybackStart = false
        let rate = desiredPlaybackRate
        applyDefaultPlaybackRate(rate, to: player)
        iosHostLog("startPlayback rate=\(rate)")
        recordBenchmark("start_playback_applied", attributes: ["rate": "\(rate)"])
        player.playImmediately(atRate: rate)
    }

    func pause() {
        clearLastError()
        recordBenchmark("pause_command")
        iosHostLog("pause")
        player?.pause()
        refreshPlaybackState()
    }

    func togglePause() {
        switch publishedUiState.playbackState {
        case .playing:
            pause()
        case .ready, .paused, .finished:
            play()
        }
    }

    func stop() {
        clearLastError()
        recordBenchmark("stop_command")
        iosHostLog("stop")
        pendingPlayAfterStopSeek = false
        isSeekingToStartAfterStop = true
        let playbackEpoch = currentPlaybackEpoch()
        player?.pause()
        player?.seek(to: .zero, toleranceBefore: .zero, toleranceAfter: .zero) { [weak self] _ in
            guard let self else { return }
            Task { @MainActor in
                self.handleStopSeekCompletion(playbackEpoch: playbackEpoch)
            }
        }
        updateState {
            PlayerHostUiState(
                title: $0.title,
                subtitle: $0.subtitle,
                sourceLabel: $0.sourceLabel,
                playbackState: .ready,
                playbackRate: $0.playbackRate,
                isBuffering: false,
                isInterrupted: $0.isInterrupted,
                timeline: currentTimelineState(positionMs: 0)
            )
        }
    }

    func seek(by deltaMs: Int64) {
        clearLastError()
        iosHostLog("seek(by:) deltaMs=\(deltaMs)")
        let timeline = publishedUiState.timeline
        let target = timeline.clampedPosition(timeline.positionMs + deltaMs)
        seekToPosition(target)
    }

    func seek(toRatio ratio: Double) {
        clearLastError()
        iosHostLog("seek(toRatio:) ratio=\(ratio)")
        let timeline = publishedUiState.timeline
        let target = timeline.position(forRatio: ratio)
        seekToPosition(target)
    }

    func seekToLiveEdge() {
        clearLastError()
        let timeline = publishedUiState.timeline
        guard let target = timeline.goLivePositionMs else {
            return
        }
        iosHostLog("seekToLiveEdge targetMs=\(target)")
        seekToPosition(target)
    }

    func setPlaybackRate(_ rate: Float) {
        clearLastError()
        let clampedRate = min(max(rate, 0.5), 3.0)
        iosHostLog("setPlaybackRate rate=\(clampedRate)")
        desiredPlaybackRate = clampedRate
        if let player {
            applyDefaultPlaybackRate(clampedRate, to: player)
        }
        if publishedUiState.playbackState == .playing {
            player?.playImmediately(atRate: clampedRate)
        }
        updateState {
            PlayerHostUiState(
                title: $0.title,
                subtitle: $0.subtitle,
                sourceLabel: $0.sourceLabel,
                playbackState: $0.playbackState,
                playbackRate: clampedRate,
                isBuffering: $0.isBuffering,
                isInterrupted: $0.isInterrupted,
                timeline: $0.timeline
            )
        }
        refreshPlaybackState()
    }

    func setVideoTrackSelection(_ selection: VesperTrackSelection) {
        let trackIdText = selection.trackId ?? "nil"
        reportCommandError(
            category: .unsupported,
            message:
                "setVideoTrackSelection is not implemented on iOS AVPlayer (mode=\(selection.mode.rawValue), trackId=\(trackIdText))"
        )
    }

    func setAudioTrackSelection(_ selection: VesperTrackSelection) {
        clearLastError()
        let trackIdText = selection.trackId ?? "nil"
        iosHostLog(
            "setAudioTrackSelection mode=\(selection.mode.rawValue) trackId=\(trackIdText)"
        )
        guard let item = player?.currentItem else {
            iosHostLog("setAudioTrackSelection ignored: no current item")
            return
        }

        guard let group = audioGroup else {
            iosHostLog("setAudioTrackSelection ignored: no audible media selection group")
            return
        }

        applyTrackSelection(
            selection,
            kind: .audio,
            group: group,
            optionsByTrackId: audioOptionsByTrackId,
            item: item
        )
    }

    func setSubtitleTrackSelection(_ selection: VesperTrackSelection) {
        clearLastError()
        let trackIdText = selection.trackId ?? "nil"
        iosHostLog(
            "setSubtitleTrackSelection mode=\(selection.mode.rawValue) trackId=\(trackIdText)"
        )
        guard let item = player?.currentItem else {
            iosHostLog("setSubtitleTrackSelection ignored: no current item")
            return
        }

        guard let group = subtitleGroup else {
            iosHostLog("setSubtitleTrackSelection ignored: no legible media selection group")
            return
        }

        applyTrackSelection(
            selection,
            kind: .subtitle,
            group: group,
            optionsByTrackId: subtitleOptionsByTrackId,
            item: item
        )
    }

    func setAbrPolicy(_ policy: VesperAbrPolicy) {
        applyAbrPolicy(
            policy,
            origin: .manual,
            clearLastReportedError: true
        )
    }

    private func applyAbrPolicy(
        _ policy: VesperAbrPolicy,
        origin: AbrPolicyOrigin,
        clearLastReportedError: Bool
    ) {
        if clearLastReportedError {
            clearLastError()
        }
        let trackIdText = policy.trackId ?? "nil"
        let maxBitRateText = policy.maxBitRate.map(String.init) ?? "nil"
        let maxWidthText = policy.maxWidth.map(String.init) ?? "nil"
        let maxHeightText = policy.maxHeight.map(String.init) ?? "nil"
        iosHostLog(
            "setAbrPolicy mode=\(policy.mode.rawValue) trackId=\(trackIdText) maxBitRate=\(maxBitRateText) maxWidth=\(maxWidthText) maxHeight=\(maxHeightText)"
        )
        let hasResolutionLimit = policy.maxWidth != nil || policy.maxHeight != nil
        let resolvedVideoVariantPin: LoadedVideoVariantPin?
        var resolvedFixedTrackId: String?
        switch policy.mode {
        case .constrained:
            guard policy.maxBitRate != nil || hasResolutionLimit else {
                reportCommandError(
                    category: .unsupported,
                    message:
                        "setAbrPolicy constrained mode requires maxBitRate or maxWidth/maxHeight on iOS"
                )
                return
            }
            if
                hasResolutionLimit,
                let resolvedPin = resolvedConstrainedVideoVariantPin(for: policy)
            {
                resolvedVideoVariantPin = resolvedPin
            } else if hasResolutionLimit {
                reportCommandError(
                    category: .unsupported,
                    message:
                        "setAbrPolicy constrained mode requires a loaded iOS video variant catalog to infer a single-axis maxWidth/maxHeight limit"
                )
                return
            } else {
                resolvedVideoVariantPin = LoadedVideoVariantPin(
                    peakBitRate: policy.maxBitRate.map(Double.init),
                    maxWidth: nil,
                    maxHeight: nil
                )
            }
        case .fixedTrack:
            guard let trackId = policy.trackId, !trackId.isEmpty else {
                reportCommandError(
                    category: .input,
                    message: "setAbrPolicy fixedTrack requires a non-empty trackId on iOS"
                )
                return
            }
            guard let resolvedFixedTrack = resolvedFixedVideoVariantTrack(for: trackId) else {
                reportCommandError(
                    category: .unsupported,
                    message:
                        "setAbrPolicy fixedTrack requires a video variant from the current iOS track catalog (trackId=\(trackId))"
                )
                return
            }
            guard resolvedFixedTrack.pin.hasAnyLimit else {
                reportCommandError(
                    category: .unsupported,
                    message:
                        "setAbrPolicy fixedTrack could not derive bitrate or resolution limits for trackId=\(resolvedFixedTrack.track.id) on iOS"
                )
                return
            }
            resolvedFixedTrackId = resolvedFixedTrack.track.id
            resolvedVideoVariantPin = resolvedFixedTrack.pin
        case .auto:
            resolvedVideoVariantPin = nil
            break
        }

        guard let item = player?.currentItem else {
            iosHostLog("setAbrPolicy ignored: no current item")
            return
        }

        switch policy.mode {
        case .auto:
            fixedTrackConvergenceState = nil
            applyVideoVariantPin(nil, to: item)
            updateTrackSelection { current in
                VesperTrackSelectionSnapshot(
                    video: .auto(),
                    audio: current.audio,
                    subtitle: current.subtitle,
                    abrPolicy: .auto()
                )
            }
        case .constrained:
            fixedTrackConvergenceState = nil
            applyVideoVariantPin(resolvedVideoVariantPin, to: item)
            updateTrackSelection { current in
                VesperTrackSelectionSnapshot(
                    video: .auto(),
                    audio: current.audio,
                    subtitle: current.subtitle,
                    abrPolicy: .constrained(
                        maxBitRate: policy.maxBitRate,
                        maxWidth: policy.maxWidth,
                        maxHeight: policy.maxHeight
                    )
                )
            }
        case .fixedTrack:
            guard let resolvedFixedTrackId, let resolvedVideoVariantPin else {
                return
            }
            fixedTrackConvergenceState = FixedTrackConvergenceState(
                requestedTrackId: resolvedFixedTrackId,
                origin: origin
            )
            applyVideoVariantPin(resolvedVideoVariantPin, to: item)
            updateTrackSelection { current in
                VesperTrackSelectionSnapshot(
                    // iOS fixedTrack 这里只是按 variant 做 best-effort 约束，不等于精确视频轨选择。
                    video: .auto(),
                    audio: current.audio,
                    subtitle: current.subtitle,
                    abrPolicy: .fixedTrack(resolvedFixedTrackId)
                )
            }
        }
    }

    func setResiliencePolicy(_ policy: VesperPlaybackResiliencePolicy) {
        clearLastError()
        if currentResiliencePolicy == policy {
            return
        }

        currentResiliencePolicy = policy
        publishedResiliencePolicy = policy
        guard let currentSource else {
            return
        }

        iosHostLog(
            "apply resilience policy buffering=\(policy.buffering.preset.rawValue) retry=\(policy.retry.backoff.rawValue) cache=\(policy.cache.preset.rawValue)"
        )
        cancelPendingRetry(resetAttempts: true)

        guard player != nil else {
            return
        }

        pendingResilienceRestore = PendingResilienceRestore(
            sourceUri: currentSource.uri,
            state: PreservedPlaybackState.capture(
                uiState: publishedUiState,
                trackSelection: publishedTrackSelection
            )
        )

        updateState {
            PlayerHostUiState(
                title: $0.title,
                subtitle: $0.subtitle,
                sourceLabel: $0.sourceLabel,
                playbackState: $0.playbackState,
                playbackRate: $0.playbackRate,
                isBuffering: true,
                isInterrupted: $0.isInterrupted,
                timeline: $0.timeline
            )
        }
        initialize()
    }

    func drainBenchmarkEvents() -> [VesperBenchmarkEvent] {
        benchmarkRecorder.drainEvents()
    }

    func benchmarkSummary() -> VesperBenchmarkSummary {
        benchmarkRecorder.summary()
    }

    private func loadCurrentSource() throws {
        guard let currentSource else {
            throw NSError(
                domain: "io.github.ikaros.vesper.host.ios",
                code: -1,
                userInfo: [NSLocalizedDescriptionKey: VesperPlayerI18n.noSourceSelected]
            )
        }

        recordBenchmark("source_load_start")
        let url = try resolvedUrl(for: currentSource)
        iosHostLog("loadCurrentSource url=\(url.absoluteString)")
        let resolvedResiliencePolicy = currentResiliencePolicy.resolvedForRuntimeSource(currentSource)
        resolvedTrackPreferencePolicy = trackPreferencePolicy.resolvedForRuntime()
        let cachePolicy = resolvedCachePolicy(resolvedResiliencePolicy.cache)
        VesperSharedUrlCacheCoordinator.shared.apply(
            policy: cachePolicy,
            token: cachePolicyToken
        )
        preloadCoordinator.configure(cachePolicy: cachePolicy)
        preloadCoordinator.warmCurrentSource(source: currentSource, url: url)
        let item = makePlayerItem(for: currentSource, url: url)
        let bufferingPolicy = resolvedBufferingPolicy(resolvedResiliencePolicy.buffering)
        item.preferredForwardBufferDuration = bufferingPolicy.preferredForwardBufferDuration
        let player = AVPlayer(playerItem: item)
        player.automaticallyWaitsToMinimizeStalling =
            bufferingPolicy.automaticallyWaitsToMinimizeStalling
        applyDefaultPlaybackRate(desiredPlaybackRate, to: player)

        let playbackEpoch = advancePlaybackEpoch()
        removeObservers()
        pendingPlaybackStart = false
        hasAppliedDefaultTrackPreferences = false
        resetTrackState()
        applyDashStartupAbrLimitIfNeeded(for: currentSource, to: item)
        self.player = player
        surfaceHost?.attach(player: player)
        installObservers(for: player, item: item, playbackEpoch: playbackEpoch)
        recordBenchmark("source_load_configured")

        updateState {
            PlayerHostUiState(
                title: $0.title,
                subtitle: sourceSubtitle(for: currentSource),
                sourceLabel: currentSource.label,
                playbackState: .ready,
                playbackRate: $0.playbackRate,
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
        }
    }

    private func makePlayerItem(for source: VesperPlayerSource, url: URL) -> AVPlayerItem {
        guard source.protocol == .dash else {
            currentDashSession = nil
            dashResourceLoaderDelegate = nil
            guard !source.headers.isEmpty else {
                return AVPlayerItem(url: url)
            }
            let asset = AVURLAsset(
                url: url,
                options: [vesperAVURLAssetHTTPHeaderFieldsKey: source.headers]
            )
            return AVPlayerItem(asset: asset)
        }

        let dashBenchmarkEventRecorder: VesperDashSession.BenchmarkEventRecorder?
        if benchmarkRecorder.isEnabled {
            dashBenchmarkEventRecorder = { [weak self] eventName, attributes in
                self?.recordBenchmark(eventName, attributes: attributes)
            }
        } else {
            dashBenchmarkEventRecorder = nil
        }
        let session = VesperDashSession(
            sourceURL: url,
            headers: source.headers,
            benchmarkEventRecorder: dashBenchmarkEventRecorder
        )
        let loaderDelegate = VesperDashResourceLoaderDelegate(session: session)
        let asset = source.headers.isEmpty
            ? AVURLAsset(url: session.masterPlaylistURL)
            : AVURLAsset(
                url: session.masterPlaylistURL,
                options: [vesperAVURLAssetHTTPHeaderFieldsKey: source.headers]
            )
        asset.resourceLoader.setDelegate(
            loaderDelegate,
            queue: loaderDelegate.resourceLoadingQueue
        )
        currentDashSession = session
        dashResourceLoaderDelegate = loaderDelegate
        iosHostLog("configured DASH bridge master=\(session.masterPlaylistURL.absoluteString)")
        recordBenchmark("dash_bridge_configured")
        return AVPlayerItem(asset: asset)
    }

    private func seekToPosition(_ positionMs: Int64) {
        let playbackEpoch = currentPlaybackEpoch()
        let time = CMTime(milliseconds: positionMs)
        recordBenchmark("seek_start", attributes: ["positionMs": "\(positionMs)"])
        player?.seek(to: time) { [weak self] _ in
            guard let self else { return }
            Task { @MainActor in
                self.handleSeekCompletion(positionMs: positionMs, playbackEpoch: playbackEpoch)
            }
        }
    }

    private func installObservers(for player: AVPlayer, item: AVPlayerItem, playbackEpoch: UInt64) {
        timeObserverToken = player.addPeriodicTimeObserver(
            forInterval: CMTime(seconds: 0.25, preferredTimescale: 600),
            queue: .main
        ) { [weak self] _ in
            guard let self else { return }
            Task { @MainActor in
                guard self.isPlaybackEpochCurrent(playbackEpoch) else {
                    iosHostLog("ignored stale time observer playbackEpoch=\(playbackEpoch)")
                    return
                }
                self.refreshPlaybackState()
            }
        }

        timeControlObservation = player.observe(\.timeControlStatus, options: [.initial, .new]) { [weak self] player, _ in
            let reason = player.reasonForWaitingToPlay?.rawValue ?? "nil"
            iosHostLog(
                "timeControlStatus=\(timeControlStatusName(player.timeControlStatus)) reason=\(reason) rate=\(player.rate)"
            )
            Task { @MainActor in
                guard let self, self.isPlaybackEpochCurrent(playbackEpoch) else {
                    return
                }
                self.recordBenchmark(
                    "time_control_status_changed",
                    attributes: [
                        "status": timeControlStatusName(player.timeControlStatus),
                        "reason": reason,
                    ]
                )
            }
        }

        itemStatusObservation = item.observe(\.status, options: [.initial, .new]) { [weak self] item, _ in
            let errorMessage = item.error?.localizedDescription ?? "nil"
            iosHostLog("itemStatus=\(itemStatusName(item.status)) error=\(errorMessage)")
            guard let self else { return }
            Task { @MainActor in
                guard self.isPlaybackEpochCurrent(playbackEpoch) else {
                    iosHostLog("ignored stale item status playbackEpoch=\(playbackEpoch)")
                    return
                }
                self.recordBenchmark(
                    "player_item_status_changed",
                    attributes: [
                        "status": itemStatusName(item.status),
                        "error": errorMessage,
                    ]
                )
                switch item.status {
                case .readyToPlay:
                    self.recordBenchmark("player_item_ready")
                    self.cancelPendingRetry(resetAttempts: true)
                    self.refreshTrackCatalogAndSelection(for: item)
                    self.applyPendingResilienceRestore(ifNeededFor: item, phase: .coreState)
                    self.attemptPendingPlaybackStart(reason: "itemReadyToPlay")
                    self.refreshPlaybackState()
                case .failed:
                    self.pendingPlaybackStart = false
                    logPlayerItemErrorLog(item)
                    self.handlePlaybackFailure(
                        error: item.error,
                        fallbackMessage: errorMessage
                    )
                case .unknown:
                    break
                @unknown default:
                    break
                }
            }
        }

        itemBufferEmptyObservation = item.observe(\.isPlaybackBufferEmpty, options: [.initial, .new]) { [weak self] item, _ in
            iosHostLog("itemBufferEmpty=\(item.isPlaybackBufferEmpty)")
            Task { @MainActor in
                guard let self, self.isPlaybackEpochCurrent(playbackEpoch) else {
                    return
                }
                self.recordBenchmark(
                    "buffer_empty_changed",
                    attributes: ["empty": "\(item.isPlaybackBufferEmpty)"]
                )
            }
        }

        itemLikelyToKeepUpObservation = item.observe(\.isPlaybackLikelyToKeepUp, options: [.initial, .new]) {
            [weak self] item, _
            in
            iosHostLog("itemLikelyToKeepUp=\(item.isPlaybackLikelyToKeepUp)")
            guard let self else { return }
            if item.isPlaybackLikelyToKeepUp {
                Task { @MainActor in
                    guard self.isPlaybackEpochCurrent(playbackEpoch) else {
                        iosHostLog("ignored stale likelyToKeepUp playbackEpoch=\(playbackEpoch)")
                        return
                    }
                    self.recordBenchmark(
                        "likely_to_keep_up_changed",
                        attributes: ["likely": "\(item.isPlaybackLikelyToKeepUp)"]
                    )
                    self.releaseDashStartupAbrLimitIfNeeded(
                        reason: "likelyToKeepUp",
                        item: item
                    )
                    self.attemptPendingPlaybackStart(reason: "itemLikelyToKeepUp")
                }
            } else {
                Task { @MainActor in
                    guard self.isPlaybackEpochCurrent(playbackEpoch) else {
                        return
                    }
                    self.recordBenchmark(
                        "likely_to_keep_up_changed",
                        attributes: ["likely": "\(item.isPlaybackLikelyToKeepUp)"]
                    )
                }
            }
        }

        endObserver = NotificationCenter.default.addObserver(
            forName: .AVPlayerItemDidPlayToEndTime,
            object: player.currentItem,
            queue: .main
        ) { [weak self] _ in
            guard let self else { return }
            Task { @MainActor in
                guard self.isPlaybackEpochCurrent(playbackEpoch) else {
                    iosHostLog("ignored stale ended observer playbackEpoch=\(playbackEpoch)")
                    return
                }
                self.handlePlaybackEnded()
            }
        }

        playbackStalledObserver = NotificationCenter.default.addObserver(
            forName: .AVPlayerItemPlaybackStalled,
            object: player.currentItem,
            queue: .main
        ) { [weak self] _ in
            guard let self else { return }
            Task { @MainActor in
                guard self.isPlaybackEpochCurrent(playbackEpoch) else {
                    return
                }
                self.recordBenchmark("playback_stalled")
            }
        }

        refreshTrackCatalogAndSelection(for: item)
    }

    private func removeObservers() {
        if let token = timeObserverToken, let player {
            player.removeTimeObserver(token)
        }
        timeObserverToken = nil
        timeControlObservation = nil
        itemStatusObservation = nil
        itemBufferEmptyObservation = nil
        itemLikelyToKeepUpObservation = nil

        if let endObserver {
            NotificationCenter.default.removeObserver(endObserver)
        }
        endObserver = nil
        if let playbackStalledObserver {
            NotificationCenter.default.removeObserver(playbackStalledObserver)
        }
        playbackStalledObserver = nil
    }

    private func advancePlaybackEpoch() -> UInt64 {
        playbackEpoch &+= 1
        return playbackEpoch
    }

    private func currentPlaybackEpoch() -> UInt64 {
        playbackEpoch
    }

    func playbackEpochSnapshot() -> UInt64 {
        playbackEpoch
    }

    func stopSeekStateSnapshot() -> StopSeekStateSnapshot {
        StopSeekStateSnapshot(
            isSeekingToStartAfterStop: isSeekingToStartAfterStop,
            pendingPlayAfterStopSeek: pendingPlayAfterStopSeek
        )
    }

    private func isPlaybackEpochCurrent(_ capturedPlaybackEpoch: UInt64) -> Bool {
        capturedPlaybackEpoch == playbackEpoch
    }

    func handleSeekCompletion(positionMs: Int64, playbackEpoch: UInt64) {
        guard isPlaybackEpochCurrent(playbackEpoch) else {
            iosHostLog(
                "ignored stale seek completion playbackEpoch=\(playbackEpoch) current=\(self.playbackEpoch) positionMs=\(positionMs)"
            )
            return
        }
        recordBenchmark("seek_completed", attributes: ["positionMs": "\(positionMs)"])
        updateTimelinePosition(positionMs)
        refreshPlaybackState()
    }

    func handleStopSeekCompletion(playbackEpoch: UInt64) {
        guard isPlaybackEpochCurrent(playbackEpoch) else {
            iosHostLog(
                "ignored stale stop seek completion playbackEpoch=\(playbackEpoch) current=\(self.playbackEpoch)"
            )
            return
        }
        iosHostLog("stop seek completed")
        recordBenchmark("stop_seek_completed")
        isSeekingToStartAfterStop = false
        updateTimelinePosition(0)
        if pendingPlayAfterStopSeek {
            pendingPlayAfterStopSeek = false
            iosHostLog("resuming deferred play after stop seek")
            startPlayback()
        }
        refreshPlaybackState()
    }

    private func handlePlaybackEnded() {
        recordBenchmark("playback_ended")
        let durationMs = currentDurationMs() ?? publishedUiState.timeline.durationMs ?? 0
        updateState {
            PlayerHostUiState(
                title: $0.title,
                subtitle: $0.subtitle,
                sourceLabel: $0.sourceLabel,
                playbackState: .finished,
                playbackRate: $0.playbackRate,
                isBuffering: false,
                isInterrupted: $0.isInterrupted,
                timeline: currentTimelineState(positionMs: durationMs)
            )
        }
    }

    private func refreshPlaybackState() {
        guard let player else {
            return
        }

        let durationMs = currentDurationMs()
        let positionMs = player.currentTime().milliseconds
        let buffering = player.timeControlStatus == .waitingToPlayAtSpecifiedRate
        let playbackState = derivePlaybackState(
            currentState: publishedUiState.playbackState,
            player: player,
            durationMs: durationMs,
            positionMs: positionMs
        )

        updateState {
            PlayerHostUiState(
                title: $0.title,
                subtitle: $0.subtitle,
                sourceLabel: $0.sourceLabel,
                playbackState: playbackState,
                playbackRate: $0.playbackRate,
                isBuffering: buffering,
                isInterrupted: false,
                timeline: currentTimelineState(positionMs: positionMs)
            )
        }
        refreshEffectiveVideoTrackObservation(for: player.currentItem)
    }

    private func updateTimelinePosition(_ positionMs: Int64) {
        updateState {
            PlayerHostUiState(
                title: $0.title,
                subtitle: $0.subtitle,
                sourceLabel: $0.sourceLabel,
                playbackState: $0.playbackState,
                playbackRate: $0.playbackRate,
                isBuffering: $0.isBuffering,
                isInterrupted: $0.isInterrupted,
                timeline: currentTimelineState(positionMs: positionMs)
            )
        }
    }

    private func currentTimelineState(positionMs explicitPositionMs: Int64? = nil) -> TimelineUiState {
        let durationMs = currentDurationMs()
        let item = player?.currentItem
        let seekableRange = currentSeekableRange(item: item, durationMs: durationMs)
        let kind = currentTimelineKind(durationMs: durationMs, seekableRange: seekableRange)
        let seekableRangeStartMs = seekableRange?.startMs ?? 0
        let seekableRangeEndMs = seekableRange?.endMs ?? 0
        let hasSeekableWindow = seekableRangeEndMs > seekableRangeStartMs

        let liveEdgeMs: Int64?
        switch kind {
        case .vod:
            liveEdgeMs = nil
        case .live, .liveDvr:
            liveEdgeMs = seekableRange?.endMs
        }

        let isSeekable: Bool
        switch kind {
        case .vod, .liveDvr:
            isSeekable = hasSeekableWindow
        case .live:
            isSeekable = false
        }

        let currentPositionMs = player?.currentTime().milliseconds
        let rawPositionMs = explicitPositionMs ?? currentPositionMs ?? publishedUiState.timeline.positionMs
        let clampedPositionMs: Int64
        if let seekableRange, seekableRange.endMs >= seekableRange.startMs {
            clampedPositionMs = min(max(rawPositionMs, seekableRange.startMs), seekableRange.endMs)
        } else {
            clampedPositionMs = max(rawPositionMs, 0)
        }

        let uiDurationMs: Int64?
        switch kind {
        case .vod:
            uiDurationMs = durationMs
        case .live:
            uiDurationMs = nil
        case .liveDvr:
            uiDurationMs = seekableRange.map { max($0.endMs - $0.startMs, 0) }
        }

        return TimelineUiState(
            kind: kind,
            isSeekable: isSeekable,
            seekableRange: isSeekable ? seekableRange : nil,
            liveEdgeMs: liveEdgeMs,
            positionMs: clampedPositionMs,
            durationMs: uiDurationMs
        )
    }

    private func currentTimelineKind(
        durationMs: Int64?,
        seekableRange: SeekableRangeUi?
    ) -> TimelineKindUi {
        if let durationMs, durationMs > 0 {
            return .vod
        }

        guard currentSource?.kind == .remote, currentSource?.protocol == .hls else {
            return .vod
        }

        if let seekableRange, seekableRange.endMs > seekableRange.startMs {
            return .liveDvr
        }

        return .live
    }

    private func currentSeekableRange(
        item: AVPlayerItem?,
        durationMs: Int64?
    ) -> SeekableRangeUi? {
        if let item {
            let ranges = item.seekableTimeRanges.compactMap { value -> SeekableRangeUi? in
                let timeRange = value.timeRangeValue
                guard
                    let startMs = timeRange.start.finiteMilliseconds,
                    let endMs = CMTimeAdd(timeRange.start, timeRange.duration).finiteMilliseconds,
                    endMs >= startMs
                else {
                    return nil
                }
                return SeekableRangeUi(startMs: startMs, endMs: endMs)
            }
            if let widestRange = ranges.max(by: { ($0.endMs - $0.startMs) < ($1.endMs - $1.startMs) }) {
                return widestRange
            }
        }

        return normalizedSeekableRange(durationMs: durationMs)
    }

    private func currentDurationMs() -> Int64? {
        player?.currentItem?.duration.finiteMilliseconds
    }

    private func resetTrackState() {
        audioGroup = nil
        subtitleGroup = nil
        videoVariantPinsByTrackId = [:]
        desiredVideoVariantPin = nil
        dashStartupAbrLimitPin = nil
        audioOptionsByTrackId = [:]
        subtitleOptionsByTrackId = [:]
        hasAppliedDefaultTrackPreferences = false
        fixedTrackConvergenceState = nil
        publishedTrackCatalog = .empty
        publishedTrackSelection = VesperTrackSelectionSnapshot()
        publishedEffectiveVideoTrackId = nil
        publishedVideoVariantObservation = nil
        publishedFixedTrackStatus = nil
    }

    private func updateTrackSelection(
        _ transform: (VesperTrackSelectionSnapshot) -> VesperTrackSelectionSnapshot
    ) {
        publishedTrackSelection = transform(publishedTrackSelection)
        refreshEffectiveVideoTrackObservation(for: player?.currentItem)
    }

    private func resolvedConstrainedVideoVariantPin(
        for policy: VesperAbrPolicy
    ) -> LoadedVideoVariantPin? {
        let resolvedResolution = resolveConstrainedMaximumVideoResolution(
            maxWidth: policy.maxWidth,
            maxHeight: policy.maxHeight,
            tracks: publishedTrackCatalog.videoTracks
        )
        if (policy.maxWidth != nil || policy.maxHeight != nil) && resolvedResolution == nil {
            return nil
        }

        return LoadedVideoVariantPin(
            peakBitRate: policy.maxBitRate.map(Double.init),
            maxWidth: resolvedResolution?.width,
            maxHeight: resolvedResolution?.height
        )
    }

    private func resolvedFixedVideoVariantTrack(
        for requestedTrackId: String
    ) -> (track: VesperMediaTrack, pin: LoadedVideoVariantPin)? {
        let videoTracks = publishedTrackCatalog.videoTracks
        guard !videoTracks.isEmpty else {
            return nil
        }

        if
            let exactTrack = videoTracks.first(where: { $0.id == requestedTrackId }),
            let exactPin = videoVariantPinsByTrackId[requestedTrackId]
        {
            return (track: exactTrack, pin: exactPin)
        }

        guard
            let resolvedTrackId = resolveRequestedVideoVariantTrackId(
                requestedTrackId,
                tracks: videoTracks
            ),
            let resolvedTrack = videoTracks.first(where: { $0.id == resolvedTrackId }),
            let resolvedPin = videoVariantPinsByTrackId[resolvedTrackId]
        else {
            return nil
        }

        iosHostLog(
            "remapped fixedTrack request trackId=\(requestedTrackId) resolvedTrackId=\(resolvedTrackId)"
        )
        return (track: resolvedTrack, pin: resolvedPin)
    }

    private func applyTrackSelection(
        _ selection: VesperTrackSelection,
        kind: VesperMediaTrackKind,
        group: AVMediaSelectionGroup,
        optionsByTrackId: [String: AVMediaSelectionOption],
        item: AVPlayerItem
    ) {
        let optionToSelect: AVMediaSelectionOption?
        switch selection.mode {
        case .auto:
            optionToSelect = group.defaultOption ?? item.currentMediaSelection.selectedMediaOption(in: group)
        case .disabled:
            optionToSelect = nil
        case .track:
            guard let trackId = selection.trackId, let option = optionsByTrackId[trackId] else {
                let trackIdText = selection.trackId ?? "nil"
                iosHostLog(
                    "set\(kind.rawValue.capitalized)TrackSelection ignored: trackId=\(trackIdText) is not present in the current catalog"
                )
                return
            }
            optionToSelect = option
        }

        item.select(optionToSelect, in: group)
        updateTrackSelection { current in
            switch kind {
            case .video:
                VesperTrackSelectionSnapshot(
                    video: selection,
                    audio: current.audio,
                    subtitle: current.subtitle,
                    abrPolicy: current.abrPolicy
                )
            case .audio:
                VesperTrackSelectionSnapshot(
                    video: current.video,
                    audio: selection,
                    subtitle: current.subtitle,
                    abrPolicy: current.abrPolicy
                )
            case .subtitle:
                VesperTrackSelectionSnapshot(
                    video: current.video,
                    audio: current.audio,
                    subtitle: selection,
                    abrPolicy: current.abrPolicy
                )
            }
        }
    }

    private func applyDefaultTrackPreferencesIfNeeded(for item: AVPlayerItem) {
        guard !hasAppliedDefaultTrackPreferences else {
            return
        }

        hasAppliedDefaultTrackPreferences = true
        applyDefaultAudioTrackPreferenceIfPossible(item: item)
        applyDefaultSubtitleTrackPreferenceIfPossible(item: item)
        applyAbrPolicy(
            resolvedTrackPreferencePolicy.abrPolicy,
            origin: .defaultPolicy,
            clearLastReportedError: false
        )
    }

    private func applyDefaultAudioTrackPreferenceIfPossible(item: AVPlayerItem) {
        guard let group = audioGroup else {
            return
        }

        let policy = resolvedTrackPreferencePolicy
        switch policy.audioSelection.mode {
        case .disabled:
            item.select(nil, in: group)
            updateTrackSelection { current in
                VesperTrackSelectionSnapshot(
                    video: current.video,
                    audio: .disabled(),
                    subtitle: current.subtitle,
                    abrPolicy: current.abrPolicy
                )
            }
        case .track:
            applyTrackSelection(
                policy.audioSelection,
                kind: .audio,
                group: group,
                optionsByTrackId: audioOptionsByTrackId,
                item: item
            )
        case .auto:
            if
                let match = matchingMediaOption(
                    language: policy.preferredAudioLanguage,
                    optionsByTrackId: audioOptionsByTrackId
                )
            {
                item.select(match.option, in: group)
            } else {
                item.selectMediaOptionAutomatically(in: group)
            }
            updateTrackSelection { current in
                VesperTrackSelectionSnapshot(
                    video: current.video,
                    audio: .auto(),
                    subtitle: current.subtitle,
                    abrPolicy: current.abrPolicy
                )
            }
        }
    }

    private func applyDefaultSubtitleTrackPreferenceIfPossible(item: AVPlayerItem) {
        guard let group = subtitleGroup else {
            return
        }

        let policy = resolvedTrackPreferencePolicy
        switch policy.subtitleSelection.mode {
        case .disabled:
            item.select(nil, in: group)
            updateTrackSelection { current in
                VesperTrackSelectionSnapshot(
                    video: current.video,
                    audio: current.audio,
                    subtitle: .disabled(),
                    abrPolicy: current.abrPolicy
                )
            }
        case .track:
            applyTrackSelection(
                policy.subtitleSelection,
                kind: .subtitle,
                group: group,
                optionsByTrackId: subtitleOptionsByTrackId,
                item: item
            )
        case .auto:
            let option =
                matchingMediaOption(
                    language: policy.preferredSubtitleLanguage,
                    optionsByTrackId: subtitleOptionsByTrackId
                )?.option
                ?? (policy.selectUndeterminedSubtitleLanguage
                    ? firstUndeterminedMediaOption(optionsByTrackId: subtitleOptionsByTrackId)
                    : nil)
                ?? (policy.selectSubtitlesByDefault ? group.defaultOption : nil)
            item.select(option, in: group)
            updateTrackSelection { current in
                VesperTrackSelectionSnapshot(
                    video: current.video,
                    audio: current.audio,
                    subtitle: option == nil ? .disabled() : .auto(),
                    abrPolicy: current.abrPolicy
                )
            }
        }
    }

    private func matchingMediaOption(
        language: String?,
        optionsByTrackId: [String: AVMediaSelectionOption]
    ) -> (trackId: String, option: AVMediaSelectionOption)? {
        guard let normalizedLanguage = normalizedLanguageIdentifier(language) else {
            return nil
        }

        return optionsByTrackId.first { _, option in
            let candidates = [
                option.extendedLanguageTag,
                option.locale?.identifier,
            ]
            return candidates.contains { candidate in
                guard let normalizedCandidate = normalizedLanguageIdentifier(candidate) else {
                    return false
                }
                return normalizedCandidate == normalizedLanguage ||
                    normalizedCandidate.hasPrefix(normalizedLanguage + "-") ||
                    normalizedLanguage.hasPrefix(normalizedCandidate + "-")
            }
        }.map { (trackId: $0.key, option: $0.value) }
    }

    private func firstUndeterminedMediaOption(
        optionsByTrackId: [String: AVMediaSelectionOption]
    ) -> AVMediaSelectionOption? {
        optionsByTrackId.values.first { option in
            normalizedLanguageIdentifier(option.extendedLanguageTag) == nil &&
                normalizedLanguageIdentifier(option.locale?.identifier) == nil
        }
    }

    private func normalizedLanguageIdentifier(_ value: String?) -> String? {
        guard let value else {
            return nil
        }

        let normalized = value.trimmingCharacters(in: .whitespacesAndNewlines)
            .replacingOccurrences(of: "_", with: "-")
            .lowercased()
        guard !normalized.isEmpty, normalized != "und" else {
            return nil
        }
        return normalized
    }

    private func refreshEffectiveVideoTrackObservation(for item: AVPlayerItem?) {
        let now = Date()
        let videoVariantObservation = resolvedVideoVariantObservation(for: item)
        if publishedVideoVariantObservation != videoVariantObservation {
            publishedVideoVariantObservation = videoVariantObservation
        }
        let resolvedTrackId = resolvedEffectiveVideoTrackId(
            for: item,
            observation: videoVariantObservation
        )
        if publishedEffectiveVideoTrackId != resolvedTrackId {
            publishedEffectiveVideoTrackId = resolvedTrackId
        }
        let rawFixedTrackStatus = resolveFixedTrackStatus(
            abrPolicy: publishedTrackSelection.abrPolicy,
            effectiveVideoTrackId: resolvedTrackId,
            tracks: publishedTrackCatalog.videoTracks
        )
        handleFixedTrackConvergenceUpdate(
            status: rawFixedTrackStatus,
            effectiveVideoTrackId: resolvedTrackId,
            observation: videoVariantObservation,
            now: now
        )
        let resolvedPublishedStatus =
            publishedTrackSelection.abrPolicy.mode == .fixedTrack
            ? resolvePublishableFixedTrackStatus(
                rawStatus: rawFixedTrackStatus,
                lockedElapsed: fixedTrackConvergenceState?.lockedStartedAt.map {
                    now.timeIntervalSince($0)
                },
                hasPersistentMismatch: fixedTrackConvergenceState?
                    .hasHandledPersistentMismatch == true
            )
            : nil
        if publishedFixedTrackStatus != resolvedPublishedStatus {
            publishedFixedTrackStatus = resolvedPublishedStatus
        }
    }

    private func resolvedEffectiveVideoTrackId(
        for item: AVPlayerItem?,
        observation: VesperVideoVariantObservation?
    ) -> String? {
        guard item != nil else {
            return nil
        }

        let videoTracks = publishedTrackCatalog.videoTracks
        guard !videoTracks.isEmpty else {
            return nil
        }

        let effectiveBitRate = observation?.bitRate.map(Double.init)
        let effectivePresentationSize = resolvedPresentationSize(for: observation)
        guard effectiveBitRate != nil || effectivePresentationSize != nil else {
            return nil
        }

        let requestedTrackId =
            publishedTrackSelection.abrPolicy.mode == .fixedTrack
            ? publishedTrackSelection.abrPolicy.trackId
            : nil

        return videoTracks.min { lhs, rhs in
            let lhsScore = effectiveVideoTrackScore(
                lhs,
                bitRate: effectiveBitRate,
                presentationSize: effectivePresentationSize,
                requestedTrackId: requestedTrackId
            )
            let rhsScore = effectiveVideoTrackScore(
                rhs,
                bitRate: effectiveBitRate,
                presentationSize: effectivePresentationSize,
                requestedTrackId: requestedTrackId
            )
            if lhsScore != rhsScore {
                return lhsScore < rhsScore
            }
            return comparePreferredEffectiveVideoTrack(lhs, over: rhs)
        }?.id
    }

    private func resolvedVideoVariantObservation(
        for item: AVPlayerItem?
    ) -> VesperVideoVariantObservation? {
        guard let item else {
            return nil
        }
        return resolveVideoVariantObservation(
            bitRate: resolvedEffectiveVideoBitRate(for: item),
            presentationSize: resolvedEffectivePresentationSize(for: item)
        )
    }

    private func resolvedEffectiveVideoBitRate(for item: AVPlayerItem) -> Double? {
        guard let event = item.accessLog()?.events.last else {
            return nil
        }

        if event.indicatedBitrate.isFinite, event.indicatedBitrate > 0 {
            return event.indicatedBitrate
        }
        if event.observedBitrate.isFinite, event.observedBitrate > 0 {
            return event.observedBitrate
        }
        return nil
    }

    private func resolvedEffectivePresentationSize(for item: AVPlayerItem) -> CGSize? {
        let size = item.presentationSize
        guard size.width.isFinite, size.height.isFinite, size.width > 0, size.height > 0 else {
            return nil
        }
        return size
    }

    private func resolvedPresentationSize(
        for observation: VesperVideoVariantObservation?
    ) -> CGSize? {
        guard
            let width = observation?.width,
            let height = observation?.height,
            width > 0,
            height > 0
        else {
            return nil
        }
        return CGSize(width: width, height: height)
    }

    private func effectiveVideoTrackScore(
        _ track: VesperMediaTrack,
        bitRate: Double?,
        presentationSize: CGSize?,
        requestedTrackId: String?
    ) -> (Int, Int64, Int) {
        let sizeDistance = effectiveVideoTrackSizeDistance(track, presentationSize: presentationSize)
        let bitRateDistance = effectiveVideoTrackBitRateDistance(track, bitRate: bitRate)
        let requestedTrackPenalty: Int
        if let requestedTrackId {
            requestedTrackPenalty = requestedTrackId == track.id ? 0 : 1
        } else {
            requestedTrackPenalty = 0
        }
        return (sizeDistance, bitRateDistance, requestedTrackPenalty)
    }

    private func effectiveVideoTrackSizeDistance(
        _ track: VesperMediaTrack,
        presentationSize: CGSize?
    ) -> Int {
        guard let presentationSize else {
            return 0
        }
        guard let width = track.width, let height = track.height else {
            return Int.max / 4
        }

        let currentMaxEdge = Int(max(presentationSize.width, presentationSize.height).rounded())
        let currentMinEdge = Int(min(presentationSize.width, presentationSize.height).rounded())
        let trackMaxEdge = max(width, height)
        let trackMinEdge = min(width, height)
        return abs(trackMaxEdge - currentMaxEdge) + abs(trackMinEdge - currentMinEdge)
    }

    private func effectiveVideoTrackBitRateDistance(
        _ track: VesperMediaTrack,
        bitRate: Double?
    ) -> Int64 {
        guard let bitRate else {
            return 0
        }
        guard let trackBitRate = track.bitRate else {
            return Int64.max / 4
        }
        return Int64(abs(Double(trackBitRate) - bitRate).rounded())
    }

    private func comparePreferredEffectiveVideoTrack(
        _ lhs: VesperMediaTrack,
        over rhs: VesperMediaTrack
    ) -> Bool {
        let lhsBitRate = lhs.bitRate ?? -1
        let rhsBitRate = rhs.bitRate ?? -1
        if lhsBitRate != rhsBitRate {
            return lhsBitRate > rhsBitRate
        }

        let lhsMaxEdge = max(lhs.width ?? 0, lhs.height ?? 0)
        let rhsMaxEdge = max(rhs.width ?? 0, rhs.height ?? 0)
        if lhsMaxEdge != rhsMaxEdge {
            return lhsMaxEdge > rhsMaxEdge
        }

        let lhsMinEdge = min(lhs.width ?? 0, lhs.height ?? 0)
        let rhsMinEdge = min(rhs.width ?? 0, rhs.height ?? 0)
        if lhsMinEdge != rhsMinEdge {
            return lhsMinEdge > rhsMinEdge
        }

        let lhsFrameRate = Int((lhs.frameRate ?? 0).rounded())
        let rhsFrameRate = Int((rhs.frameRate ?? 0).rounded())
        if lhsFrameRate != rhsFrameRate {
            return lhsFrameRate > rhsFrameRate
        }

        return (lhs.label ?? lhs.id) <= (rhs.label ?? rhs.id)
    }

    private func refreshTrackCatalogAndSelection(for item: AVPlayerItem) {
        Task { [weak self, weak item] in
            guard let self, let item else { return }
            let trackState = await self.loadTrackCatalogState(for: item)
            guard self.player?.currentItem === item else { return }
            self.audioGroup = trackState.audioGroup
            self.subtitleGroup = trackState.subtitleGroup
            self.videoVariantPinsByTrackId = trackState.videoVariantPinsByTrackId
            self.audioOptionsByTrackId = trackState.audioOptionsByTrackId
            self.subtitleOptionsByTrackId = trackState.subtitleOptionsByTrackId
            self.publishedTrackCatalog = trackState.catalog
            self.applyDefaultTrackPreferencesIfNeeded(for: item)
            self.applyPendingResilienceRestore(ifNeededFor: item, phase: .trackSelection)
            self.refreshEffectiveVideoTrackObservation(for: item)
        }
    }

    private func loadTrackCatalogState(for item: AVPlayerItem) async -> LoadedTrackCatalogState {
        let asset = item.asset
        let audibleGroup = await loadMediaSelectionGroup(for: .audible, asset: asset)
        let legibleGroup = await loadMediaSelectionGroup(for: .legible, asset: asset)
        let dashManifestCatalog = await loadDashManifestTrackCatalogSnapshot()
        let videoVariantState: LoadedVideoVariantState
        if let dashManifestCatalog {
            videoVariantState = LoadedVideoVariantState(
                tracks: dashManifestCatalog.videoTracks,
                pinsByTrackId: dashManifestCatalog.videoVariantPinsByTrackId
            )
        } else {
            videoVariantState = await loadVideoVariantState(for: asset)
        }

        var tracks = videoVariantState.tracks
        var audioOptionsByTrackId: [String: AVMediaSelectionOption] = [:]
        var subtitleOptionsByTrackId: [String: AVMediaSelectionOption] = [:]

        if let audibleGroup {
            for (index, option) in audibleGroup.options.enumerated() {
                let trackId = "audio:\(index)"
                let dashAudioMetadata = dashManifestCatalog?.audioMetadata(at: index)
                audioOptionsByTrackId[trackId] = option
                tracks.append(
                    VesperMediaTrack(
                        id: trackId,
                        kind: .audio,
                        label: option.displayName.isEmpty
                            ? dashAudioMetadata?.label
                            : option.displayName,
                        language: option.extendedLanguageTag ?? option.locale?.identifier
                            ?? dashAudioMetadata?.language,
                        codec: dashAudioMetadata?.codec,
                        bitRate: dashAudioMetadata?.bitRate,
                        width: nil,
                        height: nil,
                        frameRate: nil,
                        channels: dashAudioMetadata?.channels,
                        sampleRate: dashAudioMetadata?.sampleRate,
                        isDefault: audibleGroup.defaultOption == option,
                        isForced: false
                    )
                )
            }
        } else if let dashManifestCatalog {
            tracks.append(contentsOf: dashManifestCatalog.audioTracks)
        }

        if let legibleGroup {
            for (index, option) in legibleGroup.options.enumerated() {
                let trackId = "subtitle:\(index)"
                let dashSubtitleMetadata = dashManifestCatalog?.subtitleMetadata(at: index)
                subtitleOptionsByTrackId[trackId] = option
                tracks.append(
                    VesperMediaTrack(
                        id: trackId,
                        kind: .subtitle,
                        label: option.displayName.isEmpty
                            ? dashSubtitleMetadata?.label
                            : option.displayName,
                        language: option.extendedLanguageTag ?? option.locale?.identifier
                            ?? dashSubtitleMetadata?.language,
                        codec: dashSubtitleMetadata?.codec,
                        bitRate: dashSubtitleMetadata?.bitRate,
                        width: nil,
                        height: nil,
                        frameRate: nil,
                        channels: dashSubtitleMetadata?.channels,
                        sampleRate: dashSubtitleMetadata?.sampleRate,
                        isDefault: legibleGroup.defaultOption == option,
                        isForced: option.hasMediaCharacteristic(.containsOnlyForcedSubtitles)
                    )
                )
            }
        } else if let dashManifestCatalog {
            tracks.append(contentsOf: dashManifestCatalog.subtitleTracks)
        }

        return LoadedTrackCatalogState(
            catalog: VesperTrackCatalog(
                tracks: tracks,
                adaptiveVideo: dashManifestCatalog?.adaptiveVideo
                    ?? (videoVariantState.tracks.count > 1),
                adaptiveAudio: dashManifestCatalog?.adaptiveAudio ?? false
            ),
            audioGroup: audibleGroup,
            subtitleGroup: legibleGroup,
            videoVariantPinsByTrackId: videoVariantState.pinsByTrackId,
            audioOptionsByTrackId: audioOptionsByTrackId,
            subtitleOptionsByTrackId: subtitleOptionsByTrackId
        )
    }

    private func loadDashManifestTrackCatalogSnapshot() async -> VesperDashManifestTrackCatalogSnapshot? {
        guard currentSource?.protocol == .dash, let currentDashSession else {
            return nil
        }
        return try? await currentDashSession.manifestTrackCatalogSnapshot()
    }

    private func loadVideoVariantState(for asset: AVAsset) async -> LoadedVideoVariantState {
        guard sourceSupportsVideoVariantCatalog(currentSource) else {
            return .empty
        }
        guard #available(iOS 15.0, *) else {
            return .empty
        }
        guard let urlAsset = asset as? AVURLAsset else {
            return .empty
        }

        let variants = (try? await urlAsset.load(.variants)) ?? []
        guard !variants.isEmpty else {
            return .empty
        }

        let groupedVariants = Dictionary(
            grouping: variants.compactMap(LoadedVideoVariantDescriptor.init)
        ) { descriptor in
            descriptor.deduplicationKey
        }
        let deduplicatedVariants = groupedVariants.values.compactMap { descriptors in
            descriptors.max(by: { left, right in
                LoadedVideoVariantDescriptor.preferredOrdering(
                    left,
                    over: right
                ) == right
            })
        }
        .sorted { left, right in
            if left == right {
                return false
            }
            return LoadedVideoVariantDescriptor.preferredOrdering(left, over: right) == left
        }

        var tracks: [VesperMediaTrack] = []
        var pinsByTrackId: [String: LoadedVideoVariantPin] = [:]
        tracks.reserveCapacity(deduplicatedVariants.count)
        pinsByTrackId.reserveCapacity(deduplicatedVariants.count)

        for (index, descriptor) in deduplicatedVariants.enumerated() {
            let trackId = descriptor.stableTrackId
            tracks.append(
                VesperMediaTrack(
                    id: trackId,
                    kind: .video,
                    label: descriptor.trackLabel,
                    language: nil,
                    codec: descriptor.codec,
                    bitRate: descriptor.peakBitRate,
                    width: descriptor.width,
                    height: descriptor.height,
                    frameRate: descriptor.frameRate,
                    channels: nil,
                    sampleRate: nil,
                    isDefault: index == 0,
                    isForced: false
                )
            )
            pinsByTrackId[trackId] = LoadedVideoVariantPin(
                peakBitRate: descriptor.peakBitRate.map(Double.init),
                maxWidth: descriptor.width,
                maxHeight: descriptor.height
            )
        }

        return LoadedVideoVariantState(
            tracks: tracks,
            pinsByTrackId: pinsByTrackId
        )
    }

    private func loadMediaSelectionGroup(
        for characteristic: AVMediaCharacteristic,
        asset: AVAsset
    ) async -> AVMediaSelectionGroup? {
        return try? await asset.loadMediaSelectionGroup(for: characteristic)
    }

    private func applyDefaultPlaybackRate(_ rate: Float, to player: AVPlayer) {
        player.defaultRate = rate
    }

    private func applyVideoVariantPin(_ pin: LoadedVideoVariantPin?, to item: AVPlayerItem) {
        desiredVideoVariantPin = pin
        applyEffectiveVideoVariantPin(pin, to: item)
    }

    private func applyDashStartupAbrLimitIfNeeded(
        for source: VesperPlayerSource,
        to item: AVPlayerItem
    ) {
        guard source.protocol == .dash else {
            dashStartupAbrLimitPin = nil
            return
        }

        dashStartupAbrLimitPin = LoadedVideoVariantPin(
            peakBitRate: Self.dashStartupAbrPeakBitRate,
            maxWidth: Self.dashStartupAbrMaxWidth,
            maxHeight: Self.dashStartupAbrMaxHeight
        )
        applyEffectiveVideoVariantPin(desiredVideoVariantPin, to: item)
        recordBenchmark(
            "dash_startup_abr_limit_applied",
            attributes: [
                "maxBitRate": "\(Int(Self.dashStartupAbrPeakBitRate))",
                "maxWidth": "\(Self.dashStartupAbrMaxWidth)",
                "maxHeight": "\(Self.dashStartupAbrMaxHeight)",
            ]
        )
        iosHostLog(
            "dashStartupAbrLimit applied maxBitRate=\(Int(Self.dashStartupAbrPeakBitRate)) maxWidth=\(Self.dashStartupAbrMaxWidth) maxHeight=\(Self.dashStartupAbrMaxHeight)"
        )
    }

    private func releaseDashStartupAbrLimitIfNeeded(reason: String, item: AVPlayerItem?) {
        guard dashStartupAbrLimitPin != nil else {
            return
        }
        dashStartupAbrLimitPin = nil
        guard let item = item ?? player?.currentItem else {
            return
        }
        applyEffectiveVideoVariantPin(desiredVideoVariantPin, to: item)
        recordBenchmark(
            "dash_startup_abr_limit_released",
            attributes: ["reason": reason]
        )
        iosHostLog("dashStartupAbrLimit released reason=\(reason)")
    }

    private func applyEffectiveVideoVariantPin(
        _ pin: LoadedVideoVariantPin?,
        to item: AVPlayerItem
    ) {
        let effectivePin = combinedVideoVariantPin(pin, dashStartupAbrLimitPin)
        item.preferredPeakBitRate = effectivePin?.peakBitRate ?? 0
        if let maxWidth = effectivePin?.maxWidth, let maxHeight = effectivePin?.maxHeight {
            item.preferredMaximumResolution = CGSize(
                width: CGFloat(maxWidth),
                height: CGFloat(maxHeight)
            )
        } else {
            item.preferredMaximumResolution = .zero
        }
    }

    private func combinedVideoVariantPin(
        _ desiredPin: LoadedVideoVariantPin?,
        _ temporaryPin: LoadedVideoVariantPin?
    ) -> LoadedVideoVariantPin? {
        guard let desiredPin else {
            return temporaryPin
        }
        guard let temporaryPin else {
            return desiredPin
        }
        return LoadedVideoVariantPin(
            peakBitRate: minimumOptional(desiredPin.peakBitRate, temporaryPin.peakBitRate),
            maxWidth: minimumOptional(desiredPin.maxWidth, temporaryPin.maxWidth),
            maxHeight: minimumOptional(desiredPin.maxHeight, temporaryPin.maxHeight)
        )
    }

    private func resolvedUrl(for source: VesperPlayerSource) throws -> URL {
        guard let url = URL(string: source.uri) else {
            throw NSError(
                domain: "io.github.ikaros.vesper.host.ios",
                code: -2,
                userInfo: [NSLocalizedDescriptionKey: VesperPlayerI18n.invalidMediaUrl]
            )
        }
        return url
    }

    private func sourceSubtitle(for source: VesperPlayerSource) -> String {
        switch source.kind {
        case .local:
            return VesperPlayerI18n.nativeLocalSourceSubtitle()
        case .remote:
            return VesperPlayerI18n.nativeRemoteSourceSubtitle(source.protocol.rawValue)
        }
    }

    private func cancelPendingRetry(resetAttempts: Bool) {
        retryTask?.cancel()
        retryTask = nil
        if resetAttempts {
            retryAttemptCount = 0
        }
    }

    private func clearLastError() {
        publishedLastError = nil
        fixedTrackIssueActive = false
    }

    private func reportCommandError(category: VesperPlayerErrorCategory, message: String) {
        iosHostLog("commandError category=\(category.rawValue) message=\(message)")
        fixedTrackIssueActive = false
        publishedLastError = VesperPlayerError(
            message: message,
            category: category,
            retriable: false
        )
    }

    private func handlePlaybackFailure(error: Error?, fallbackMessage: String) {
        let resolvedError = classifyPlaybackFailure(error, fallbackMessage: fallbackMessage)
        iosHostLog(
            "playbackFailure category=\(resolvedError.category.rawValue) retriable=\(resolvedError.retriable) message=\(resolvedError.message)"
        )
        recordBenchmark(
            "playback_error",
            attributes: [
                "category": resolvedError.category.rawValue,
                "retriable": "\(resolvedError.retriable)",
            ]
        )
        fixedTrackIssueActive = false
        publishedLastError = resolvedError.toPlayerError()

        if scheduleRetryIfPossible(for: resolvedError) {
            return
        }

        updateErrorState(message: resolvedError.message)
    }

    private func updateErrorState(message: String) {
        updateState {
            PlayerHostUiState(
                title: $0.title,
                subtitle: VesperPlayerI18n.nativeBridgeError(message),
                sourceLabel: $0.sourceLabel,
                playbackState: .ready,
                playbackRate: $0.playbackRate,
                isBuffering: false,
                isInterrupted: $0.isInterrupted,
                timeline: $0.timeline
            )
        }
    }

    private func scheduleRetryIfPossible(for error: ResolvedBridgeError) -> Bool {
        guard error.retriable, let currentSource, currentSource.kind == .remote else {
            return false
        }

        let retryPolicy = currentResiliencePolicy.resolvedForRuntimeSource(currentSource).retry
        let nextAttempt = retryAttemptCount + 1
        if let maxAttempts = retryPolicy.maxAttempts, nextAttempt > maxAttempts {
            return false
        }

        let delayMs = retryDelayMs(forAttempt: nextAttempt, retryPolicy: retryPolicy)
        retryAttemptCount = nextAttempt
        pendingAutoPlay = true
        pendingPlaybackStart = false
        retryTask?.cancel()

        updateState {
            PlayerHostUiState(
                title: $0.title,
                subtitle: VesperPlayerI18n.retryScheduled(delay: formattedRetryDelay(delayMs), message: error.message),
                sourceLabel: $0.sourceLabel,
                playbackState: .ready,
                playbackRate: $0.playbackRate,
                isBuffering: false,
                isInterrupted: $0.isInterrupted,
                timeline: $0.timeline
            )
        }

        let expectedUri = currentSource.uri
        let expectedPlaybackEpoch = currentPlaybackEpoch()
        retryTask = Task { [weak self] in
            try? await Task.sleep(nanoseconds: delayMs * 1_000_000)
            guard !Task.isCancelled else { return }
            await MainActor.run {
                self?.handleScheduledRetryFire(
                    expectedUri: expectedUri,
                    playbackEpoch: expectedPlaybackEpoch,
                    attempt: nextAttempt,
                    delayMs: delayMs
                )
            }
        }
        return true
    }

    func handleScheduledRetryFire(
        expectedUri: String,
        playbackEpoch: UInt64,
        attempt: Int,
        delayMs: UInt64
    ) {
        guard currentSource?.uri == expectedUri else {
            iosHostLog(
                "ignored stale retry task sourceUri=\(expectedUri) currentSource=\(currentSource?.uri ?? "nil") attempt=\(attempt)"
            )
            return
        }
        guard isPlaybackEpochCurrent(playbackEpoch) else {
            iosHostLog(
                "ignored stale retry task playbackEpoch=\(playbackEpoch) current=\(self.playbackEpoch) attempt=\(attempt)"
            )
            return
        }
        iosHostLog("retrying playback attempt=\(attempt) delayMs=\(delayMs)")
        initialize()
    }

    private func retryDelayMs(forAttempt attempt: Int, retryPolicy: VesperRetryPolicy) -> UInt64 {
        let policy = retryPolicy
        let multiplier: Double
        switch policy.backoff {
        case .fixed:
            multiplier = 1
        case .linear:
            multiplier = Double(attempt)
        case .exponential:
            multiplier = pow(2, Double(max(attempt - 1, 0)))
        }

        let computedDelay = Double(policy.baseDelayMs) * multiplier
        return min(UInt64(computedDelay.rounded()), policy.maxDelayMs)
    }

    private func classifyPlaybackFailure(
        _ error: Error?,
        fallbackMessage: String
    ) -> ResolvedBridgeError {
        guard let error else {
            return ResolvedBridgeError(
                category: .platform,
                retriable: false,
                message: fallbackMessage
            )
        }

        let nsError = error as NSError
        if nsError.domain == "io.github.ikaros.vesper.host.ios", nsError.code == -3 {
            return ResolvedBridgeError(
                category: .unsupported,
                retriable: false,
                message: nsError.localizedDescription
            )
        }
        if nsError.domain == NSURLErrorDomain {
            switch nsError.code {
            case NSURLErrorTimedOut,
                NSURLErrorCannotFindHost,
                NSURLErrorCannotConnectToHost,
                NSURLErrorNetworkConnectionLost,
                NSURLErrorDNSLookupFailed,
                NSURLErrorNotConnectedToInternet:
                return ResolvedBridgeError(
                    category: .network,
                    retriable: true,
                    message: nsError.localizedDescription
                )
            case NSURLErrorFileDoesNotExist,
                NSURLErrorBadURL,
                NSURLErrorUnsupportedURL:
                return ResolvedBridgeError(
                    category: .source,
                    retriable: false,
                    message: nsError.localizedDescription
                )
            case NSURLErrorNoPermissionsToReadFile:
                return ResolvedBridgeError(
                    category: .capability,
                    retriable: false,
                    message: nsError.localizedDescription
                )
            default:
                break
            }
        }

        if nsError.domain == AVFoundationErrorDomain || nsError.domain == AVError.errorDomain {
            switch AVError.Code(rawValue: nsError.code) {
            case .decoderNotFound, .decoderTemporarilyUnavailable:
                return ResolvedBridgeError(
                    category: .decode,
                    retriable: false,
                    message: nsError.localizedDescription
                )
            case .fileFormatNotRecognized:
                return ResolvedBridgeError(
                    category: .capability,
                    retriable: false,
                    message: nsError.localizedDescription
                )
            case .contentIsUnavailable, .mediaServicesWereReset:
                return ResolvedBridgeError(
                    category: .platform,
                    retriable: false,
                    message: nsError.localizedDescription
                )
            default:
                break
            }
        }

        return ResolvedBridgeError(
            category: .platform,
            retriable: false,
            message: nsError.localizedDescription
        )
    }

    private func resolvedBufferingPolicy(_ resolvedPolicy: VesperBufferingPolicy) -> ResolvedBufferingPolicy {
        let effectiveMs =
            resolvedPolicy.maxBufferMs
            ?? resolvedPolicy.minBufferMs
            ?? resolvedPolicy.bufferForPlaybackAfterRebufferMs
            ?? resolvedPolicy.bufferForPlaybackMs
            ?? 0

        let automaticallyWaits = switch resolvedPolicy.preset {
        case .lowLatency:
            false
        default:
            true
        }

        return ResolvedBufferingPolicy(
            preferredForwardBufferDuration: TimeInterval(effectiveMs) / 1000.0,
            automaticallyWaitsToMinimizeStalling: automaticallyWaits
        )
    }

    private func resolvedCachePolicy(_ resolvedPolicy: VesperCachePolicy) -> ResolvedCachePolicy {
        let maxMemoryBytes = resolvedPolicy.maxMemoryBytes ?? 0
        let maxDiskBytes = resolvedPolicy.maxDiskBytes ?? 0

        return ResolvedCachePolicy(
            enabled: max(maxMemoryBytes, maxDiskBytes) > 0,
            memoryCapacity: clampToInt(maxMemoryBytes),
            diskCapacity: clampToInt(maxDiskBytes)
        )
    }

    private func formattedRetryDelay(_ delayMs: UInt64) -> String {
        let seconds = Double(delayMs) / 1000.0
        if seconds >= 10 || seconds.rounded() == seconds {
            return VesperPlayerI18n.retryDelaySecondsInt(Int(seconds.rounded()))
        }
        return VesperPlayerI18n.retryDelaySecondsDecimal(seconds)
    }

    private func configureAudioSessionIfNeeded() {
        do {
            let session = AVAudioSession.sharedInstance()
            try session.setCategory(.playback, mode: .moviePlayback, options: [])
            try session.setActive(true)
            iosHostLog("audio session configured")
        } catch {
            iosHostLog("audio session configuration failed: \(error.localizedDescription)")
        }
    }

    private func updateState(_ transform: (PlayerHostUiState) -> PlayerHostUiState) {
        publishedUiState = transform(publishedUiState)
    }

    private func applyPendingResilienceRestore(
        ifNeededFor item: AVPlayerItem,
        phase: PendingResilienceRestorePhase
    ) {
        guard
            var pendingResilienceRestore,
            currentSource?.uri == pendingResilienceRestore.sourceUri,
            player?.currentItem === item
        else {
            return
        }

        switch phase {
        case .coreState:
            if pendingResilienceRestore.needsCoreStateRestore {
                restoreCorePlaybackState(pendingResilienceRestore.state)
                pendingResilienceRestore.needsCoreStateRestore = false
            }
        case .trackSelection:
            if pendingResilienceRestore.needsTrackSelectionRestore {
                pendingResilienceRestore.needsTrackSelectionRestore =
                    restoreTrackSelectionsIfNeeded(pendingResilienceRestore.state, item: item)
            }
        }

        if
            !pendingResilienceRestore.needsCoreStateRestore &&
                !pendingResilienceRestore.needsTrackSelectionRestore
        {
            self.pendingResilienceRestore = nil
            return
        }

        self.pendingResilienceRestore = pendingResilienceRestore
    }

    private func restoreCorePlaybackState(_ state: PreservedPlaybackState) {
        if state.seekToLiveEdge, publishedUiState.timeline.kind == .liveDvr {
            seekToLiveEdge()
        } else if state.restorePosition {
            seekToPosition(max(state.positionMs, 0))
        }

        if abs(state.playbackRate - 1.0) > 0.001 {
            setPlaybackRate(state.playbackRate)
        }

        if !abrPolicyRequiresLoadedVideoVariantCatalog(state.abrPolicy) {
            applyAbrPolicy(
                state.abrPolicy,
                origin: .resilienceRestore,
                clearLastReportedError: false
            )
        }

        if state.shouldResumePlayback {
            play()
        } else if state.playbackState == .paused {
            pause()
        }
    }

    private func restoreTrackSelectionsIfNeeded(
        _ state: PreservedPlaybackState,
        item: AVPlayerItem
    ) -> Bool {
        if state.audioSelection.mode != .auto {
            if let group = audioGroup {
                applyTrackSelection(
                    state.audioSelection,
                    kind: .audio,
                    group: group,
                    optionsByTrackId: audioOptionsByTrackId,
                    item: item
                )
            }
        }

        if state.subtitleSelection.mode != .auto {
            if let group = subtitleGroup {
                applyTrackSelection(
                    state.subtitleSelection,
                    kind: .subtitle,
                    group: group,
                    optionsByTrackId: subtitleOptionsByTrackId,
                    item: item
                )
            }
        }

        if abrPolicyRequiresLoadedVideoVariantCatalog(state.abrPolicy) {
            applyAbrPolicy(
                state.abrPolicy,
                origin: .resilienceRestore,
                clearLastReportedError: false
            )
        }

        return false
    }

    private func canStartPlayback(_ player: AVPlayer) -> Bool {
        playbackStartDeferralReason(player) == nil
    }

    private func playbackStartDeferralReason(_ player: AVPlayer) -> String? {
        guard let item = player.currentItem else {
            return "player item is attached"
        }
        switch item.status {
        case .readyToPlay:
            break
        case .failed:
            return "current item recovers from failure"
        case .unknown:
            if currentSource?.protocol != .dash {
                return "current item becomes ready"
            }
        @unknown default:
            return "current item becomes ready"
        }
        if currentSource?.kind == .local, let surfaceHost, !surfaceHost.isReadyForDisplay {
            return "first video frame is ready for display"
        }
        return nil
    }

    private func attemptPendingPlaybackStart(reason: String) {
        guard pendingPlaybackStart else {
            return
        }
        guard let player, canStartPlayback(player) else {
            return
        }
        iosHostLog("resuming deferred playback reason=\(reason)")
        startPlayback()
    }

    private func handleFixedTrackConvergenceUpdate(
        status: VesperFixedTrackStatus?,
        effectiveVideoTrackId: String?,
        observation: VesperVideoVariantObservation?,
        now: Date
    ) {
        let abrPolicy = publishedTrackSelection.abrPolicy
        guard
            abrPolicy.mode == .fixedTrack,
            let requestedTrackId = abrPolicy.trackId,
            !requestedTrackId.isEmpty
        else {
            fixedTrackConvergenceState = nil
            if fixedTrackIssueActive {
                clearLastError()
            }
            return
        }

        var convergenceState = fixedTrackConvergenceState
        if convergenceState?.requestedTrackId != requestedTrackId {
            convergenceState = FixedTrackConvergenceState(
                requestedTrackId: requestedTrackId,
                origin: convergenceState?.origin ?? .manual
            )
        }

        switch status {
        case .locked:
            if var convergenceState {
                convergenceState.resetMismatch()
                if convergenceState.lockedStartedAt == nil {
                    convergenceState.lockedStartedAt = now
                }
                fixedTrackConvergenceState = convergenceState
            } else {
                fixedTrackConvergenceState = nil
            }
            if fixedTrackIssueActive {
                clearLastError()
            }
        case .pending:
            if var convergenceState {
                convergenceState.resetLocked()
                convergenceState.resetMismatch()
                fixedTrackConvergenceState = convergenceState
            } else {
                fixedTrackConvergenceState = nil
            }
        case .fallback:
            guard var convergenceState else {
                return
            }
            convergenceState.resetLocked()
            let mismatchSignature = FixedTrackMismatchSignature(
                effectiveVideoTrackId: effectiveVideoTrackId,
                observation: observation
            )
            if convergenceState.mismatchSignature != mismatchSignature {
                convergenceState.mismatchSignature = mismatchSignature
                convergenceState.mismatchStartedAt = now
                convergenceState.hasHandledPersistentMismatch = false
                fixedTrackConvergenceState = convergenceState
                return
            }
            guard let mismatchStartedAt = convergenceState.mismatchStartedAt else {
                convergenceState.mismatchStartedAt = now
                fixedTrackConvergenceState = convergenceState
                return
            }
            let mismatchDuration = now.timeIntervalSince(mismatchStartedAt)
            guard
                !convergenceState.hasHandledPersistentMismatch,
                shouldEscalatePersistentFixedTrackFallback(
                    status: status,
                    observation: observation,
                    playbackState: publishedUiState.playbackState,
                    isBuffering: publishedUiState.isBuffering,
                    elapsed: mismatchDuration
                )
            else {
                fixedTrackConvergenceState = convergenceState
                return
            }

            convergenceState.hasHandledPersistentMismatch = true
            fixedTrackConvergenceState = convergenceState
            reportPersistentFixedTrackMismatch(
                requestedTrackId: requestedTrackId,
                effectiveVideoTrackId: effectiveVideoTrackId,
                observation: observation,
                origin: convergenceState.origin
            )
        case nil:
            if var convergenceState {
                convergenceState.resetLocked()
                convergenceState.resetMismatch()
                fixedTrackConvergenceState = convergenceState
            } else {
                fixedTrackConvergenceState = nil
            }
        }
    }

    private func reportPersistentFixedTrackMismatch(
        requestedTrackId: String,
        effectiveVideoTrackId: String?,
        observation: VesperVideoVariantObservation?,
        origin: AbrPolicyOrigin
    ) {
        let requestedTrack = publishedTrackCatalog.videoTracks.first { track in
            track.id == requestedTrackId
        }
        let observedTrack = effectiveVideoTrackId.flatMap { effectiveVideoTrackId in
            publishedTrackCatalog.videoTracks.first { track in
                track.id == effectiveVideoTrackId
            }
        }
        let observedDescription = observedVariantDescription(
            observedTrack: observedTrack,
            observation: observation
        )
        let requestedDescription = requestedTrackDescription(
            requestedTrack: requestedTrack,
            fallbackTrackId: requestedTrackId
        )

        let message: String
        switch origin {
        case .resilienceRestore:
            let recoveryPolicy = resolveFixedTrackRecoveryPolicy(
                requestedTrackId: requestedTrackId,
                tracks: publishedTrackCatalog.videoTracks
            )
            applyAbrPolicy(
                recoveryPolicy,
                origin: .recoveredFallback,
                clearLastReportedError: false
            )
            switch recoveryPolicy.mode {
            case .constrained:
                message = VesperPlayerI18n.fixedTrackRestoreFallbackConstrained(
                    requested: requestedDescription,
                    fallback: abrPolicyDescription(recoveryPolicy),
                    observed: observedDescription
                )
            case .auto, .fixedTrack:
                message = VesperPlayerI18n.fixedTrackRestoreFallbackAuto(
                    requested: requestedDescription,
                    observed: observedDescription
                )
            }
        case .manual, .defaultPolicy, .recoveredFallback:
            message = VesperPlayerI18n.fixedTrackMismatch(
                requested: requestedDescription,
                observed: observedDescription
            )
        }

        iosHostLog(
            "fixedTrackMismatch requested=\(requestedTrackId) effective=\(effectiveVideoTrackId ?? "nil") origin=\(origin.rawValue) message=\(message)"
        )
        fixedTrackIssueActive = true
        publishedLastError = VesperPlayerError(
            message: message,
            category: .playback,
            retriable: false
        )
    }

    private func requestedTrackDescription(
        requestedTrack: VesperMediaTrack?,
        fallbackTrackId: String
    ) -> String {
        if let label = requestedTrack?.label, !label.isEmpty {
            return label
        }
        if let requestedTrack {
            return trackObservationDescription(requestedTrack)
        }
        return fallbackTrackId
    }

    private func observedVariantDescription(
        observedTrack: VesperMediaTrack?,
        observation: VesperVideoVariantObservation?
    ) -> String {
        if let observedTrack {
            if let observationDescription = observationDescription(observation) {
                return "\(trackObservationDescription(observedTrack)) (\(observationDescription))"
            }
            return trackObservationDescription(observedTrack)
        }
        return observationDescription(observation) ?? "an unknown adaptive variant"
    }

    private func trackObservationDescription(_ track: VesperMediaTrack) -> String {
        if let label = track.label, !label.isEmpty {
            return label
        }

        var components: [String] = []
        if let width = track.width, let height = track.height {
            components.append("\(width)x\(height)")
        }
        if let bitRate = track.bitRate {
            components.append(formattedBitRate(bitRate))
        }
        if !components.isEmpty {
            return components.joined(separator: " · ")
        }
        return track.id
    }

    private func observationDescription(_ observation: VesperVideoVariantObservation?) -> String? {
        guard let observation else {
            return nil
        }

        var components: [String] = []
        if let width = observation.width, let height = observation.height {
            components.append("\(width)x\(height)")
        }
        if let bitRate = observation.bitRate {
            components.append(formattedBitRate(bitRate))
        }
        return components.isEmpty ? nil : components.joined(separator: " · ")
    }

    private func formattedBitRate(_ bitRate: Int64) -> String {
        let bitRateDouble = Double(bitRate)
        if bitRateDouble >= 1_000_000 {
            let value = (bitRateDouble / 100_000).rounded() / 10
            return String(format: "%.1f Mbps", locale: Locale.current, value)
        }
        if bitRateDouble >= 1_000 {
            let value = (bitRateDouble / 100).rounded() / 10
            return String(format: "%.1f Kbps", locale: Locale.current, value)
        }
        return "\(bitRate) bps"
    }

    private func abrPolicyDescription(_ policy: VesperAbrPolicy) -> String {
        switch policy.mode {
        case .constrained:
            var components: [String] = []
            if let maxHeight = policy.maxHeight {
                components.append("\(maxHeight)p")
            } else if let maxWidth = policy.maxWidth {
                components.append("\(maxWidth)w")
            }
            if let maxBitRate = policy.maxBitRate {
                components.append(formattedBitRate(maxBitRate))
            }
            return components.isEmpty ? "automatic ABR" : components.joined(separator: " · ")
        case .auto:
            return "automatic ABR"
        case .fixedTrack:
            return policy.trackId ?? "fixed track"
        }
    }
}

private enum AbrPolicyOrigin: String {
    case manual
    case defaultPolicy
    case resilienceRestore
    case recoveredFallback
}

private struct FixedTrackConvergenceState {
    let requestedTrackId: String
    let origin: AbrPolicyOrigin
    var lockedStartedAt: Date?
    var mismatchSignature: FixedTrackMismatchSignature?
    var mismatchStartedAt: Date?
    var hasHandledPersistentMismatch = false

    mutating func resetLocked() {
        lockedStartedAt = nil
    }

    mutating func resetMismatch() {
        mismatchSignature = nil
        mismatchStartedAt = nil
        hasHandledPersistentMismatch = false
    }
}

private struct FixedTrackMismatchSignature: Equatable {
    let effectiveVideoTrackId: String?
    let bitRate: Int64?
    let width: Int?
    let height: Int?

    init(
        effectiveVideoTrackId: String?,
        observation: VesperVideoVariantObservation?
    ) {
        self.effectiveVideoTrackId = effectiveVideoTrackId
        bitRate = observation?.bitRate
        width = observation?.width
        height = observation?.height
    }
}

private struct ResolvedBridgeError {
    let category: VesperPlayerErrorCategory
    let retriable: Bool
    let message: String

    func toPlayerError() -> VesperPlayerError {
        VesperPlayerError(
            message: message,
            category: category,
            retriable: retriable
        )
    }
}

private struct ResolvedBufferingPolicy {
    let preferredForwardBufferDuration: TimeInterval
    let automaticallyWaitsToMinimizeStalling: Bool
}

private struct ResolvedCachePolicy {
    let enabled: Bool
    let memoryCapacity: Int
    let diskCapacity: Int

    static let disabled = ResolvedCachePolicy(
        enabled: false,
        memoryCapacity: 0,
        diskCapacity: 0
    )
}

private enum PendingResilienceRestorePhase {
    case coreState
    case trackSelection
}

private struct PendingResilienceRestore {
    let sourceUri: String
    let state: PreservedPlaybackState
    var needsCoreStateRestore = true
    var needsTrackSelectionRestore = true
}

struct StopSeekStateSnapshot: Equatable {
    let isSeekingToStartAfterStop: Bool
    let pendingPlayAfterStopSeek: Bool
}

private struct PreservedPlaybackState {
    let positionMs: Int64
    let restorePosition: Bool
    let seekToLiveEdge: Bool
    let playbackRate: Float
    let playbackState: PlaybackStateUi
    let shouldResumePlayback: Bool
    let audioSelection: VesperTrackSelection
    let subtitleSelection: VesperTrackSelection
    let abrPolicy: VesperAbrPolicy

    static func capture(
        uiState: PlayerHostUiState,
        trackSelection: VesperTrackSelectionSnapshot
    ) -> PreservedPlaybackState {
        let seekToLiveEdge =
            uiState.timeline.kind == .liveDvr &&
                uiState.timeline.isAtLiveEdge()
        return PreservedPlaybackState(
            positionMs: uiState.timeline.positionMs,
            restorePosition: uiState.timeline.isSeekable || uiState.timeline.durationMs != nil,
            seekToLiveEdge: seekToLiveEdge,
            playbackRate: uiState.playbackRate,
            playbackState: uiState.playbackState,
            shouldResumePlayback: uiState.playbackState == .playing,
            audioSelection: trackSelection.audio,
            subtitleSelection: trackSelection.subtitle,
            abrPolicy: trackSelection.abrPolicy
        )
    }
}

private func derivePlaybackState(
    currentState: PlaybackStateUi,
    player: AVPlayer,
    durationMs: Int64?,
    positionMs: Int64,
) -> PlaybackStateUi {
    if currentState == .finished {
        return .finished
    }

    if player.rate > 0 || player.timeControlStatus == .playing {
        return .playing
    }

    if let durationMs, durationMs > 0, positionMs >= durationMs {
        return .finished
    }

    if positionMs > 0 {
        return .paused
    }

    return .ready
}

private func normalizedSeekableRange(durationMs: Int64?) -> SeekableRangeUi {
    SeekableRangeUi(startMs: 0, endMs: max(durationMs ?? 0, 0))
}

func iosHostLog(_ message: String) {
    print("[VesperPlayerIOSHost] \(message)")
}

private func clampToInt(_ value: Int64) -> Int {
    guard value > 0 else {
        return 0
    }
    return Int(min(value, Int64(Int.max)))
}

private final class VesperSharedUrlCacheCoordinator {
    static let shared = VesperSharedUrlCacheCoordinator()

    private let lock = NSLock()
    private var baselineMemoryCapacity: Int?
    private var baselineDiskCapacity: Int?
    private var activePolicies: [UUID: ResolvedCachePolicy] = [:]

    func apply(policy: ResolvedCachePolicy, token: UUID) {
        lock.lock()
        defer { lock.unlock() }

        captureBaselineIfNeeded()
        activePolicies[token] = policy
        reconfigureSharedCache()
    }

    func remove(token: UUID) {
        lock.lock()
        defer { lock.unlock() }

        captureBaselineIfNeeded()
        activePolicies.removeValue(forKey: token)
        reconfigureSharedCache()
    }

    private func captureBaselineIfNeeded() {
        if baselineMemoryCapacity == nil {
            baselineMemoryCapacity = URLCache.shared.memoryCapacity
        }
        if baselineDiskCapacity == nil {
            baselineDiskCapacity = URLCache.shared.diskCapacity
        }
    }

    private func reconfigureSharedCache() {
        let baselineMemoryCapacity = baselineMemoryCapacity ?? URLCache.shared.memoryCapacity
        let baselineDiskCapacity = baselineDiskCapacity ?? URLCache.shared.diskCapacity
        let enabledPolicies = activePolicies.values.filter(\.enabled)
        let requestedMemoryCapacity = enabledPolicies.map(\.memoryCapacity).max() ?? 0
        let requestedDiskCapacity = enabledPolicies.map(\.diskCapacity).max() ?? 0

        let targetMemoryCapacity = max(baselineMemoryCapacity, requestedMemoryCapacity)
        let targetDiskCapacity = max(baselineDiskCapacity, requestedDiskCapacity)

        if URLCache.shared.memoryCapacity != targetMemoryCapacity {
            URLCache.shared.memoryCapacity = targetMemoryCapacity
        }
        if URLCache.shared.diskCapacity != targetDiskCapacity {
            URLCache.shared.diskCapacity = targetDiskCapacity
        }

        iosHostLog(
            "urlCache memoryCapacity=\(targetMemoryCapacity) diskCapacity=\(targetDiskCapacity)"
        )
    }
}

private final class VesperNativePreloadCoordinator {
    private let budgetPolicy: VesperPreloadBudgetPolicy
    private var cachePolicy: ResolvedCachePolicy = .disabled
    private var warmupTask: Task<Void, Never>?
    private var sessionHandle: UInt64 = 0

    init(budgetPolicy: VesperPreloadBudgetPolicy) {
        self.budgetPolicy = budgetPolicy
        sessionHandle = createPreloadSession(budgetPolicy)
    }

    func configure(cachePolicy: ResolvedCachePolicy) {
        self.cachePolicy = cachePolicy
    }

    func warmCurrentSource(source: VesperPlayerSource, url: URL) {
        cancelWarmupOnly()
        guard max(cachePolicy.memoryCapacity, cachePolicy.diskCapacity) > 0 else {
            return
        }

        let candidate = runtimePreloadCandidate(source: source)
        guard planPreloadCandidates(handle: sessionHandle, candidates: [candidate]) else {
            return
        }

        let commands = drainPreloadCommands(handle: sessionHandle)
        for command in commands {
            switch command.kind {
            case .start:
                let task = command.task
                let headers = source.headers
                warmupTask = Task.detached(priority: .utility) {
                    await Self.executeWarmup(
                        handle: self.sessionHandle,
                        task: task,
                        url: url,
                        headers: headers
                    )
                }
            case .cancel:
                warmupTask?.cancel()
            default:
                continue
            }
        }
    }

    func cancelAll() {
        cancelWarmupOnly()
        if sessionHandle != 0 {
            vesper_runtime_preload_session_dispose(sessionHandle)
            sessionHandle = 0
        }
    }

    private func cancelWarmupOnly() {
        warmupTask?.cancel()
        warmupTask = nil
    }

    private func runtimePreloadCandidate(source: VesperPlayerSource) -> VesperRuntimePreloadCandidate {
        VesperRuntimePreloadCandidate(
            source_uri: duplicateCString(source.uri),
            scope_kind: VesperRuntimePreloadScopeKindApp,
            scope_id: nil,
            candidate_kind: VesperRuntimePreloadCandidateKindCurrent,
            selection_hint: VesperRuntimePreloadSelectionHintCurrentItem,
            priority: VesperRuntimePreloadPriorityCritical,
            expected_memory_bytes: UInt64(max(budgetPolicy.maxMemoryBytes ?? 32 * 1024, 0)),
            expected_disk_bytes: UInt64(max(budgetPolicy.maxDiskBytes ?? 0, 0)),
            has_ttl_ms: true,
            ttl_ms: UInt64(max(budgetPolicy.warmupWindowMs ?? 30_000, 0)),
            has_warmup_window_ms: true,
            warmup_window_ms: UInt64(max(budgetPolicy.warmupWindowMs ?? 30_000, 0))
        )
    }

    private static func executeWarmup(
        handle: UInt64,
        task: VesperRuntimePreloadTask,
        url: URL,
        headers: [String: String]
    ) async {
        let warmupBytes = max(Int64(task.expected_memory_bytes), 1)
        var request = URLRequest(url: url)
        applyHttpHeaders(headers, to: &request)
        request.cachePolicy = .returnCacheDataElseLoad
        request.timeoutInterval = TimeInterval(max(Int64(task.warmup_window_ms), 1_000)) / 1000.0
        request.setValue("bytes=0-\(max(warmupBytes - 1, 0))", forHTTPHeaderField: "Range")

        do {
            let (_, response) = try await URLSession.shared.data(for: request)
            if let httpResponse = response as? HTTPURLResponse {
                iosHostLog(
                    "preload warmup completed status=\(httpResponse.statusCode) url=\(url.absoluteString)"
                )
            }
            _ = vesper_runtime_preload_session_complete(handle, task.task_id)
        } catch {
            iosHostLog("preload warmup failed: \(error.localizedDescription)")
            error.localizedDescription.withCString { message in
                _ = vesper_runtime_preload_session_fail(
                    handle,
                    task.task_id,
                    3,
                    7,
                    false,
                    message
                )
            }
        }
    }
}

private func createPreloadSession(_ budgetPolicy: VesperPreloadBudgetPolicy) -> UInt64 {
    var resolved = VesperRuntimeResolvedPreloadBudgetPolicy(
        max_concurrent_tasks: UInt32(max(budgetPolicy.maxConcurrentTasks ?? 0, 0)),
        max_memory_bytes: budgetPolicy.maxMemoryBytes ?? 0,
        max_disk_bytes: budgetPolicy.maxDiskBytes ?? 0,
        warmup_window_ms: UInt64(max(budgetPolicy.warmupWindowMs ?? 0, 0))
    )
    var handle: UInt64 = 0
    let created = withUnsafePointer(to: &resolved) { resolvedPointer in
        withUnsafeMutablePointer(to: &handle) { handlePointer in
            vesper_runtime_preload_session_create(resolvedPointer, handlePointer)
        }
    }
    return created ? handle : 0
}

private func planPreloadCandidates(
    handle: UInt64,
    candidates: [VesperRuntimePreloadCandidate]
) -> Bool {
    guard !candidates.isEmpty else { return true }
    var mutableCandidates = candidates
    let planned = mutableCandidates.withUnsafeMutableBufferPointer { buffer in
        vesper_runtime_preload_session_plan(handle, buffer.baseAddress, UInt(buffer.count))
    }
    for candidate in mutableCandidates {
        if let sourceUri = candidate.source_uri {
            free(UnsafeMutablePointer(mutating: sourceUri))
        }
    }
    return planned
}

private func drainPreloadCommands(handle: UInt64) -> [VesperRuntimePreloadCommand] {
    var commands = VesperRuntimePreloadCommandList(commands: nil, len: 0)
    guard vesper_runtime_preload_session_drain_commands(handle, &commands),
          let commandPointer = commands.commands,
          commands.len > 0
    else {
        return []
    }

    let result = Array(UnsafeBufferPointer(start: commandPointer, count: Int(commands.len)))
    vesper_runtime_preload_command_list_free(&commands)
    return result
}

private func duplicateCString(_ value: String) -> UnsafePointer<CChar>? {
    let duplicated = strdup(value)
    guard let duplicated else {
        return nil
    }
    return UnsafePointer(duplicated)
}

private extension VesperRuntimePreloadCommandKind {
    static var start: VesperRuntimePreloadCommandKind {
        VesperRuntimePreloadCommandKindStart
    }

    static var cancel: VesperRuntimePreloadCommandKind {
        VesperRuntimePreloadCommandKindCancel
    }
}

private func minimumOptional<T: Comparable>(_ lhs: T?, _ rhs: T?) -> T? {
    switch (lhs, rhs) {
    case let (lhs?, rhs?):
        return min(lhs, rhs)
    case let (lhs?, nil):
        return lhs
    case let (nil, rhs?):
        return rhs
    case (nil, nil):
        return nil
    }
}

private func clampToInt64(_ value: Int64) -> Int64 {
    max(value, 0)
}

private func timeControlStatusName(_ status: AVPlayer.TimeControlStatus) -> String {
    switch status {
    case .paused:
        return "paused"
    case .waitingToPlayAtSpecifiedRate:
        return "waiting"
    case .playing:
        return "playing"
    @unknown default:
        return "unknown"
    }
}

private func itemStatusName(_ status: AVPlayerItem.Status) -> String {
    switch status {
    case .unknown:
        return "unknown"
    case .readyToPlay:
        return "readyToPlay"
    case .failed:
        return "failed"
    @unknown default:
        return "unknown"
    }
}

private func logPlayerItemErrorLog(_ item: AVPlayerItem) {
    guard let events = item.errorLog()?.events, !events.isEmpty else {
        return
    }
    for event in events.suffix(5) {
        iosHostLog(
            "itemErrorLog uri=\(event.uri ?? "nil") status=\(event.errorStatusCode) domain=\(event.errorDomain) comment=\(event.errorComment ?? "nil")"
        )
    }
}

private extension CMTime {
    init(milliseconds: Int64) {
        self = CMTime(seconds: Double(milliseconds) / 1000.0, preferredTimescale: 600)
    }

    var milliseconds: Int64 {
        guard isValid, isNumeric, seconds.isFinite else {
            return 0
        }
        return max(Int64(seconds * 1000.0), 0)
    }

    var finiteMilliseconds: Int64? {
        guard isValid, isNumeric, seconds.isFinite else {
            return nil
        }
        return max(Int64(seconds * 1000.0), 0)
    }
}
