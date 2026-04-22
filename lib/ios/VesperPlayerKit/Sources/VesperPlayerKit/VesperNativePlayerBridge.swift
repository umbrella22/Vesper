@preconcurrency import AVFoundation
import Foundation
import UIKit
import VesperPlayerKitBridgeShim

@MainActor
final class VesperNativePlayerBridge: ObservableObject, ObservablePlayerBridge {
    let backend: PlayerBridgeBackend = .rustNativeStub

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
    private weak var surfaceHost: PlayerSurfaceView?
    private var timeObserverToken: Any?
    private var endObserver: NSObjectProtocol?
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

    init(
        initialSource: VesperPlayerSource? = nil,
        resiliencePolicy: VesperPlaybackResiliencePolicy = VesperPlaybackResiliencePolicy(),
        trackPreferencePolicy: VesperTrackPreferencePolicy = VesperTrackPreferencePolicy(),
        preloadBudgetPolicy: VesperPreloadBudgetPolicy = VesperPreloadBudgetPolicy()
    ) {
        currentSource = initialSource
        currentResiliencePolicy = resiliencePolicy
        self.trackPreferencePolicy = trackPreferencePolicy
        resolvedTrackPreferencePolicy = trackPreferencePolicy.resolvedForRuntime()
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
        guard let currentSource else {
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
        } catch {
            pendingAutoPlay = false
            iosHostLog("initialize failed: \(error.localizedDescription)")
            handlePlaybackFailure(error: error, fallbackMessage: error.localizedDescription)
        }
    }

    func dispose() {
        clearLastError()
        iosHostLog("dispose")
        cancelPendingRetry(resetAttempts: true)
        pendingResilienceRestore = nil
        currentSource = nil
        hasAppliedDefaultTrackPreferences = false
        pendingAutoPlay = false
        tearDownActivePlayback()
    }

    func selectSource(_ source: VesperPlayerSource) {
        clearLastError()
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
        resetTrackState()
    }

    func attachSurfaceHost(_ host: UIView) {
        guard let host = host as? PlayerSurfaceView else {
            return
        }
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
                self?.attemptPendingPlaybackStart(reason: "surfaceReadyForDisplay")
            }
        }
        host.attach(player: player)
        attemptPendingPlaybackStart(reason: "attachSurfaceHost")
    }

    func detachSurfaceHost() {
        iosHostLog("detachSurfaceHost")
        surfaceHost?.onReadyForDisplay = nil
        surfaceHost?.attach(player: nil)
        surfaceHost = nil
    }

    func play() {
        clearLastError()
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
        if publishedUiState.playbackState == .finished {
            player.seek(to: .zero)
        }

        if let deferralReason = playbackStartDeferralReason(player) {
            pendingPlaybackStart = true
            iosHostLog("deferring playback until \(deferralReason)")
            return
        }

        pendingPlaybackStart = false
        let rate = desiredPlaybackRate
        applyDefaultPlaybackRate(rate, to: player)
        iosHostLog("startPlayback rate=\(rate)")
        player.playImmediately(atRate: rate)
    }

    func pause() {
        clearLastError()
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
                        "setAbrPolicy constrained mode requires a loaded HLS variant catalog to infer a single-axis maxWidth/maxHeight limit on iOS"
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

    private func loadCurrentSource() throws {
        guard let currentSource else {
            throw NSError(
                domain: "io.github.ikaros.vesper.host.ios",
                code: -1,
                userInfo: [NSLocalizedDescriptionKey: VesperPlayerI18n.noSourceSelected]
            )
        }

        if currentSource.kind == .remote, currentSource.protocol == .dash {
            throw NSError(
                domain: "io.github.ikaros.vesper.host.ios",
                code: -3,
                userInfo: [NSLocalizedDescriptionKey: VesperPlayerI18n.dashUnsupportedOnIos]
            )
        }

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
        let item = AVPlayerItem(url: url)
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
        self.player = player
        surfaceHost?.attach(player: player)
        installObservers(for: player, item: item, playbackEpoch: playbackEpoch)

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

    private func seekToPosition(_ positionMs: Int64) {
        let playbackEpoch = currentPlaybackEpoch()
        let time = CMTime(milliseconds: positionMs)
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

        timeControlObservation = player.observe(\.timeControlStatus, options: [.initial, .new]) { player, _ in
            let reason = player.reasonForWaitingToPlay?.rawValue ?? "nil"
            iosHostLog(
                "timeControlStatus=\(timeControlStatusName(player.timeControlStatus)) reason=\(reason) rate=\(player.rate)"
            )
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
                switch item.status {
                case .readyToPlay:
                    self.cancelPendingRetry(resetAttempts: true)
                    self.refreshTrackCatalogAndSelection(for: item)
                    self.applyPendingResilienceRestore(ifNeededFor: item, phase: .coreState)
                    self.attemptPendingPlaybackStart(reason: "itemReadyToPlay")
                    self.refreshPlaybackState()
                case .failed:
                    self.pendingPlaybackStart = false
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

        itemBufferEmptyObservation = item.observe(\.isPlaybackBufferEmpty, options: [.initial, .new]) { item, _ in
            iosHostLog("itemBufferEmpty=\(item.isPlaybackBufferEmpty)")
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
                    self.attemptPendingPlaybackStart(reason: "itemLikelyToKeepUp")
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
        let resolvedStatus = resolveFixedTrackStatus(
            abrPolicy: publishedTrackSelection.abrPolicy,
            effectiveVideoTrackId: resolvedTrackId,
            tracks: publishedTrackCatalog.videoTracks
        )
        if publishedFixedTrackStatus != resolvedStatus {
            publishedFixedTrackStatus = resolvedStatus
        }
        handleFixedTrackConvergenceUpdate(
            status: resolvedStatus,
            effectiveVideoTrackId: resolvedTrackId,
            observation: videoVariantObservation
        )
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
        let videoVariantState = await loadVideoVariantState(for: asset)

        var tracks = videoVariantState.tracks
        var audioOptionsByTrackId: [String: AVMediaSelectionOption] = [:]
        var subtitleOptionsByTrackId: [String: AVMediaSelectionOption] = [:]

        if let audibleGroup {
            for (index, option) in audibleGroup.options.enumerated() {
                let trackId = "audio:\(index)"
                audioOptionsByTrackId[trackId] = option
                tracks.append(
                    VesperMediaTrack(
                        id: trackId,
                        kind: .audio,
                        label: option.displayName,
                        language: option.extendedLanguageTag ?? option.locale?.identifier,
                        codec: nil,
                        bitRate: nil,
                        width: nil,
                        height: nil,
                        frameRate: nil,
                        channels: nil,
                        sampleRate: nil,
                        isDefault: audibleGroup.defaultOption == option,
                        isForced: false
                    )
                )
            }
        }

        if let legibleGroup {
            for (index, option) in legibleGroup.options.enumerated() {
                let trackId = "subtitle:\(index)"
                subtitleOptionsByTrackId[trackId] = option
                tracks.append(
                    VesperMediaTrack(
                        id: trackId,
                        kind: .subtitle,
                        label: option.displayName,
                        language: option.extendedLanguageTag ?? option.locale?.identifier,
                        codec: nil,
                        bitRate: nil,
                        width: nil,
                        height: nil,
                        frameRate: nil,
                        channels: nil,
                        sampleRate: nil,
                        isDefault: legibleGroup.defaultOption == option,
                        isForced: option.hasMediaCharacteristic(.containsOnlyForcedSubtitles)
                    )
                )
            }
        }

        return LoadedTrackCatalogState(
            catalog: VesperTrackCatalog(
                tracks: tracks,
                adaptiveVideo: currentSource?.kind == .remote && currentSource?.protocol == .hls,
                adaptiveAudio: false
            ),
            audioGroup: audibleGroup,
            subtitleGroup: legibleGroup,
            videoVariantPinsByTrackId: videoVariantState.pinsByTrackId,
            audioOptionsByTrackId: audioOptionsByTrackId,
            subtitleOptionsByTrackId: subtitleOptionsByTrackId
        )
    }

    private func loadVideoVariantState(for asset: AVAsset) async -> LoadedVideoVariantState {
        guard currentSource?.kind == .remote, currentSource?.protocol == .hls else {
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
        item.preferredPeakBitRate = pin?.peakBitRate ?? 0
        if let maxWidth = pin?.maxWidth, let maxHeight = pin?.maxHeight {
            item.preferredMaximumResolution = CGSize(
                width: CGFloat(maxWidth),
                height: CGFloat(maxHeight)
            )
        } else {
            item.preferredMaximumResolution = .zero
        }
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
        if item.status != .readyToPlay {
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
        observation: VesperVideoVariantObservation?
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
                fixedTrackConvergenceState = convergenceState
            } else {
                fixedTrackConvergenceState = nil
            }
            if fixedTrackIssueActive {
                clearLastError()
            }
        case .pending:
            if var convergenceState {
                convergenceState.resetMismatch()
                fixedTrackConvergenceState = convergenceState
            } else {
                fixedTrackConvergenceState = nil
            }
        case .fallback:
            guard var convergenceState else {
                return
            }
            let mismatchSignature = FixedTrackMismatchSignature(
                effectiveVideoTrackId: effectiveVideoTrackId,
                observation: observation
            )
            let now = Date()
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
    var mismatchSignature: FixedTrackMismatchSignature?
    var mismatchStartedAt: Date?
    var hasHandledPersistentMismatch = false

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

private struct LoadedTrackCatalogState {
    let catalog: VesperTrackCatalog
    let audioGroup: AVMediaSelectionGroup?
    let subtitleGroup: AVMediaSelectionGroup?
    let videoVariantPinsByTrackId: [String: LoadedVideoVariantPin]
    let audioOptionsByTrackId: [String: AVMediaSelectionOption]
    let subtitleOptionsByTrackId: [String: AVMediaSelectionOption]
}

private struct LoadedVideoVariantState {
    let tracks: [VesperMediaTrack]
    let pinsByTrackId: [String: LoadedVideoVariantPin]

    static let empty = LoadedVideoVariantState(
        tracks: [],
        pinsByTrackId: [:]
    )
}

struct ResolvedMaximumVideoResolution: Equatable {
    let width: Int
    let height: Int
}

func resolveConstrainedMaximumVideoResolution(
    maxWidth: Int?,
    maxHeight: Int?,
    tracks: [VesperMediaTrack]
) -> ResolvedMaximumVideoResolution? {
    switch (maxWidth, maxHeight) {
    case let (width?, height?):
        guard width > 0, height > 0 else {
            return nil
        }
        return ResolvedMaximumVideoResolution(width: width, height: height)
    case let (width?, nil):
        guard width > 0 else {
            return nil
        }
        guard
            let reference = resolvedMaximumVideoResolutionReference(
                requestedWidth: width,
                requestedHeight: nil,
                tracks: tracks
            )
        else {
            return nil
        }
        let height = max(
            Int((Double(reference.height) / Double(reference.width) * Double(width)).rounded()),
            1
        )
        return ResolvedMaximumVideoResolution(width: width, height: height)
    case let (nil, height?):
        guard height > 0 else {
            return nil
        }
        guard
            let reference = resolvedMaximumVideoResolutionReference(
                requestedWidth: nil,
                requestedHeight: height,
                tracks: tracks
            )
        else {
            return nil
        }
        let width = max(
            Int((Double(reference.width) / Double(reference.height) * Double(height)).rounded()),
            1
        )
        return ResolvedMaximumVideoResolution(width: width, height: height)
    case (nil, nil):
        return nil
    }
}

private func resolvedMaximumVideoResolutionReference(
    requestedWidth: Int?,
    requestedHeight: Int?,
    tracks: [VesperMediaTrack]
) -> ResolvedMaximumVideoResolution? {
    let candidates = tracks.compactMap { track -> ResolvedMaximumVideoResolution? in
        guard
            let width = track.width,
            let height = track.height,
            width > 0,
            height > 0
        else {
            return nil
        }
        return ResolvedMaximumVideoResolution(width: width, height: height)
    }
    guard !candidates.isEmpty else {
        return nil
    }

    return candidates.min { lhs, rhs in
        let lhsScore = resolvedMaximumVideoResolutionReferenceScore(
            lhs,
            requestedWidth: requestedWidth,
            requestedHeight: requestedHeight
        )
        let rhsScore = resolvedMaximumVideoResolutionReferenceScore(
            rhs,
            requestedWidth: requestedWidth,
            requestedHeight: requestedHeight
        )
        if lhsScore != rhsScore {
            return lhsScore < rhsScore
        }
        return lhs.width > rhs.width
    }
}

private func resolvedMaximumVideoResolutionReferenceScore(
    _ candidate: ResolvedMaximumVideoResolution,
    requestedWidth: Int?,
    requestedHeight: Int?
) -> (Int, Int, Int, Int, Int) {
    let primaryDistance: Int
    let secondaryDistance: Int
    if let requestedHeight {
        primaryDistance = abs(candidate.height - requestedHeight)
        secondaryDistance = requestedWidth.map { abs(candidate.width - $0) } ?? 0
    } else if let requestedWidth {
        primaryDistance = abs(candidate.width - requestedWidth)
        secondaryDistance = requestedHeight.map { abs(candidate.height - $0) } ?? 0
    } else {
        primaryDistance = 0
        secondaryDistance = 0
    }

    let exceedPenalty =
        (requestedWidth.map { candidate.width > $0 ? 1 : 0 } ?? 0) +
        (requestedHeight.map { candidate.height > $0 ? 1 : 0 } ?? 0)

    return (
        primaryDistance,
        secondaryDistance,
        exceedPenalty,
        Int.max - candidate.width,
        Int.max - candidate.height
    )
}

private struct LoadedVideoVariantPin {
    let peakBitRate: Double?
    let maxWidth: Int?
    let maxHeight: Int?

    var hasAnyLimit: Bool {
        peakBitRate != nil || (maxWidth != nil && maxHeight != nil)
    }
}

@available(iOS 15.0, *)
private struct LoadedVideoVariantDescriptor: Equatable {
    let codec: String?
    let peakBitRate: Int64?
    let width: Int?
    let height: Int?
    let frameRate: Double?

    init?(_ variant: AVAssetVariant) {
        guard let videoAttributes = variant.videoAttributes else {
            return nil
        }

        let presentationSize = videoAttributes.presentationSize
        let width = LoadedVideoVariantDescriptor.intOrNil(presentationSize.width)
        let height = LoadedVideoVariantDescriptor.intOrNil(presentationSize.height)
        let peakBitRate = variant.peakBitRate.flatMap(
            LoadedVideoVariantDescriptor.bitRateOrNil
        )
        let frameRate = videoAttributes.nominalFrameRate.flatMap(
            LoadedVideoVariantDescriptor.doubleOrNil
        )
        let codec = videoAttributes.codecTypes.first.map { value in
            fourCharCodeString(value)
        }

        guard peakBitRate != nil || (width != nil && height != nil) else {
            return nil
        }

        self.codec = codec
        self.peakBitRate = peakBitRate
        self.width = width
        self.height = height
        self.frameRate = frameRate
    }

    var deduplicationKey: LoadedVideoVariantDeduplicationKey {
        LoadedVideoVariantDeduplicationKey(
            codec: codec,
            peakBitRate: peakBitRate,
            width: width,
            height: height,
            frameRate: frameRate.map { Int(($0 * 100).rounded()) }
        )
    }

    var stableTrackId: String {
        stableVideoVariantTrackId(
            codec: codec,
            peakBitRate: peakBitRate,
            width: width,
            height: height,
            frameRate: frameRate
        )
    }

    var trackLabel: String {
        if let height {
            return "\(height)p"
        }
        if let width, let height {
            return "\(width)x\(height)"
        }
        if let peakBitRate {
            return "\(peakBitRate)"
        }
        return "Video"
    }

    private static func intOrNil(_ value: CGFloat) -> Int? {
        guard value.isFinite, value > 0 else {
            return nil
        }
        return Int(value.rounded())
    }

    private static func bitRateOrNil(_ value: Double) -> Int64? {
        guard value.isFinite, value > 0 else {
            return nil
        }
        return Int64(value.rounded())
    }

    private static func doubleOrNil(_ value: Double) -> Double? {
        guard value.isFinite, value > 0 else {
            return nil
        }
        return value
    }

    static func preferredOrdering(
        _ lhs: LoadedVideoVariantDescriptor,
        over rhs: LoadedVideoVariantDescriptor
    ) -> LoadedVideoVariantDescriptor {
        let lhsBitRate = lhs.peakBitRate ?? -1
        let rhsBitRate = rhs.peakBitRate ?? -1
        if lhsBitRate != rhsBitRate {
            return lhsBitRate > rhsBitRate ? lhs : rhs
        }

        let lhsMaxEdge = max(lhs.width ?? 0, lhs.height ?? 0)
        let rhsMaxEdge = max(rhs.width ?? 0, rhs.height ?? 0)
        if lhsMaxEdge != rhsMaxEdge {
            return lhsMaxEdge > rhsMaxEdge ? lhs : rhs
        }

        let lhsMinEdge = min(lhs.width ?? 0, lhs.height ?? 0)
        let rhsMinEdge = min(rhs.width ?? 0, rhs.height ?? 0)
        if lhsMinEdge != rhsMinEdge {
            return lhsMinEdge > rhsMinEdge ? lhs : rhs
        }

        let lhsFrameRate = Int((lhs.frameRate ?? 0).rounded())
        let rhsFrameRate = Int((rhs.frameRate ?? 0).rounded())
        if lhsFrameRate != rhsFrameRate {
            return lhsFrameRate > rhsFrameRate ? lhs : rhs
        }

        return lhs.trackLabel <= rhs.trackLabel ? lhs : rhs
    }
}

private struct LoadedVideoVariantDeduplicationKey: Hashable {
    let codec: String?
    let peakBitRate: Int64?
    let width: Int?
    let height: Int?
    let frameRate: Int?
}

func abrPolicyRequiresLoadedVideoVariantCatalog(_ policy: VesperAbrPolicy) -> Bool {
    switch policy.mode {
    case .fixedTrack:
        return true
    case .constrained:
        let hasWidthLimit = policy.maxWidth != nil
        let hasHeightLimit = policy.maxHeight != nil
        return hasWidthLimit != hasHeightLimit
    case .auto:
        return false
    }
}

func resolveFixedTrackStatus(
    abrPolicy: VesperAbrPolicy,
    effectiveVideoTrackId: String?,
    tracks: [VesperMediaTrack]
) -> VesperFixedTrackStatus? {
    guard
        abrPolicy.mode == .fixedTrack,
        let requestedTrackId = abrPolicy.trackId,
        !requestedTrackId.isEmpty
    else {
        return nil
    }

    guard tracks.contains(where: { $0.id == requestedTrackId }) else {
        return .pending
    }

    guard let effectiveVideoTrackId else {
        return .pending
    }

    if effectiveVideoTrackId == requestedTrackId {
        return .locked
    }

    return .fallback
}

func resolveFixedTrackRecoveryPolicy(
    requestedTrackId: String,
    tracks: [VesperMediaTrack]
) -> VesperAbrPolicy {
    guard let requestedTrack = tracks.first(where: { $0.id == requestedTrackId }) else {
        return .auto()
    }

    let hasResolutionLimit = requestedTrack.width != nil && requestedTrack.height != nil
    let hasBitRateLimit = requestedTrack.bitRate != nil
    guard hasResolutionLimit || hasBitRateLimit else {
        return .auto()
    }

    return .constrained(
        maxBitRate: requestedTrack.bitRate,
        maxWidth: hasResolutionLimit ? requestedTrack.width : nil,
        maxHeight: hasResolutionLimit ? requestedTrack.height : nil
    )
}

func shouldEscalatePersistentFixedTrackFallback(
    status: VesperFixedTrackStatus?,
    observation: VesperVideoVariantObservation?,
    playbackState: PlaybackStateUi,
    isBuffering: Bool,
    elapsed: TimeInterval
) -> Bool {
    guard status == .fallback else {
        return false
    }
    guard observation != nil else {
        return false
    }
    guard playbackState == .playing, !isBuffering else {
        return false
    }
    return elapsed >= 2.0
}

func resolveVideoVariantObservation(
    bitRate: Double?,
    presentationSize: CGSize?
) -> VesperVideoVariantObservation? {
    let normalizedBitRate: Int64?
    if let bitRate, bitRate.isFinite, bitRate > 0 {
        normalizedBitRate = Int64(bitRate.rounded())
    } else {
        normalizedBitRate = nil
    }

    let normalizedWidth: Int?
    let normalizedHeight: Int?
    if
        let presentationSize,
        presentationSize.width.isFinite,
        presentationSize.height.isFinite,
        presentationSize.width > 0,
        presentationSize.height > 0
    {
        normalizedWidth = Int(presentationSize.width.rounded())
        normalizedHeight = Int(presentationSize.height.rounded())
    } else {
        normalizedWidth = nil
        normalizedHeight = nil
    }

    guard normalizedBitRate != nil || (normalizedWidth != nil && normalizedHeight != nil) else {
        return nil
    }

    return VesperVideoVariantObservation(
        bitRate: normalizedBitRate,
        width: normalizedWidth,
        height: normalizedHeight
    )
}

func stableVideoVariantTrackId(
    codec: String?,
    peakBitRate: Int64?,
    width: Int?,
    height: Int?,
    frameRate: Double?
) -> String {
    let frameRateBucket = frameRate.flatMap { value -> Int? in
        guard value.isFinite, value > 0 else {
            return nil
        }
        return Int((value * 100).rounded())
    }

    let components = [
        "c\(sanitizedStableVideoVariantTrackIdComponent(codec))",
        "b\(peakBitRate.map(String.init) ?? "na")",
        "w\(width.map(String.init) ?? "na")",
        "h\(height.map(String.init) ?? "na")",
        "f\(frameRateBucket.map(String.init) ?? "na")",
    ]
    return "video:hls:" + components.joined(separator: ":")
}

func resolveRequestedVideoVariantTrackId(
    _ requestedTrackId: String,
    tracks: [VesperMediaTrack]
) -> String? {
    guard !requestedTrackId.isEmpty else {
        return nil
    }

    if tracks.contains(where: { $0.id == requestedTrackId }) {
        return requestedTrackId
    }

    guard
        let requestedFingerprint = StableVideoVariantFingerprint(trackId: requestedTrackId),
        requestedFingerprint.hasComparableFields
    else {
        return nil
    }

    return tracks
        .filter { $0.kind == .video }
        .min { lhs, rhs in
            let lhsScore = requestedVideoVariantTrackScore(lhs, requested: requestedFingerprint)
            let rhsScore = requestedVideoVariantTrackScore(rhs, requested: requestedFingerprint)
            if lhsScore != rhsScore {
                return lhsScore < rhsScore
            }
            return preferredVideoVariantTrack(lhs, over: rhs).id == lhs.id
        }?
        .id
}

private func sanitizedStableVideoVariantTrackIdComponent(_ value: String?) -> String {
    let rawValue = value?.lowercased() ?? "na"
    let sanitizedScalars = rawValue.unicodeScalars.map { scalar -> UnicodeScalar in
        if CharacterSet.alphanumerics.contains(scalar) {
            return scalar
        }
        return "_"
    }
    let sanitized = String(String.UnicodeScalarView(sanitizedScalars))
        .replacingOccurrences(of: "_+", with: "_", options: .regularExpression)
        .trimmingCharacters(in: CharacterSet(charactersIn: "_"))
    return sanitized.isEmpty ? "na" : sanitized
}

private struct StableVideoVariantFingerprint {
    let codecComponent: String?
    let peakBitRate: Int64?
    let width: Int?
    let height: Int?
    let frameRateBucket: Int?

    init?(trackId: String) {
        let components = trackId.split(separator: ":")
        guard components.count >= 7, components[0] == "video", components[1] == "hls" else {
            return nil
        }

        var codecComponent: String?
        var peakBitRate: Int64?
        var width: Int?
        var height: Int?
        var frameRateBucket: Int?

        for component in components.dropFirst(2) {
            guard let prefix = component.first else {
                continue
            }
            let rawValue = String(component.dropFirst())
            switch prefix {
            case "c":
                codecComponent = rawValue == "na" ? nil : rawValue
            case "b":
                peakBitRate = rawValue == "na" ? nil : Int64(rawValue)
            case "w":
                width = rawValue == "na" ? nil : Int(rawValue)
            case "h":
                height = rawValue == "na" ? nil : Int(rawValue)
            case "f":
                frameRateBucket = rawValue == "na" ? nil : Int(rawValue)
            default:
                continue
            }
        }

        self.codecComponent = codecComponent
        self.peakBitRate = peakBitRate
        self.width = width
        self.height = height
        self.frameRateBucket = frameRateBucket
    }

    init(track: VesperMediaTrack) {
        codecComponent = track.codec.map(sanitizedStableVideoVariantTrackIdComponent)
        peakBitRate = track.bitRate
        width = track.width
        height = track.height
        frameRateBucket = track.frameRate.flatMap { value in
            guard value.isFinite, value > 0 else {
                return nil
            }
            return Int((Double(value) * 100).rounded())
        }
    }

    var hasComparableFields: Bool {
        codecComponent != nil ||
            peakBitRate != nil ||
            width != nil ||
            height != nil ||
            frameRateBucket != nil
    }
}

private struct RequestedVideoVariantTrackScore: Comparable {
    let codecPenalty: Int
    let sizeMissingPenalty: Int
    let sizeDistance: Int
    let bitRateMissingPenalty: Int
    let bitRateDistance: Int64
    let frameRateMissingPenalty: Int
    let frameRateDistance: Int64
    let inverseWidth: Int
    let inverseHeight: Int
    let inverseBitRate: Int
    let trackId: String

    static func < (
        lhs: RequestedVideoVariantTrackScore,
        rhs: RequestedVideoVariantTrackScore
    ) -> Bool {
        if lhs.codecPenalty != rhs.codecPenalty {
            return lhs.codecPenalty < rhs.codecPenalty
        }
        if lhs.sizeMissingPenalty != rhs.sizeMissingPenalty {
            return lhs.sizeMissingPenalty < rhs.sizeMissingPenalty
        }
        if lhs.sizeDistance != rhs.sizeDistance {
            return lhs.sizeDistance < rhs.sizeDistance
        }
        if lhs.bitRateMissingPenalty != rhs.bitRateMissingPenalty {
            return lhs.bitRateMissingPenalty < rhs.bitRateMissingPenalty
        }
        if lhs.bitRateDistance != rhs.bitRateDistance {
            return lhs.bitRateDistance < rhs.bitRateDistance
        }
        if lhs.frameRateMissingPenalty != rhs.frameRateMissingPenalty {
            return lhs.frameRateMissingPenalty < rhs.frameRateMissingPenalty
        }
        if lhs.frameRateDistance != rhs.frameRateDistance {
            return lhs.frameRateDistance < rhs.frameRateDistance
        }
        if lhs.inverseWidth != rhs.inverseWidth {
            return lhs.inverseWidth < rhs.inverseWidth
        }
        if lhs.inverseHeight != rhs.inverseHeight {
            return lhs.inverseHeight < rhs.inverseHeight
        }
        if lhs.inverseBitRate != rhs.inverseBitRate {
            return lhs.inverseBitRate < rhs.inverseBitRate
        }
        return lhs.trackId < rhs.trackId
    }
}

private func requestedVideoVariantTrackScore(
    _ track: VesperMediaTrack,
    requested: StableVideoVariantFingerprint
) -> RequestedVideoVariantTrackScore {
    let candidate = StableVideoVariantFingerprint(track: track)
    let codecPenalty = requestedCodecPenalty(
        requested.codecComponent,
        candidate.codecComponent
    )
    let widthDistance = requestedVariantDistance(requested.width, candidate.width)
    let heightDistance = requestedVariantDistance(requested.height, candidate.height)
    let bitRateDistance = requestedVariantDistance(requested.peakBitRate, candidate.peakBitRate)
    let frameRateDistance = requestedVariantDistance(
        requested.frameRateBucket,
        candidate.frameRateBucket
    )

    return RequestedVideoVariantTrackScore(
        codecPenalty: codecPenalty,
        sizeMissingPenalty: widthDistance.missingPenalty + heightDistance.missingPenalty,
        sizeDistance: widthDistance.distance + heightDistance.distance,
        bitRateMissingPenalty: bitRateDistance.missingPenalty,
        bitRateDistance: bitRateDistance.distance,
        frameRateMissingPenalty: frameRateDistance.missingPenalty,
        frameRateDistance: Int64(frameRateDistance.distance),
        inverseWidth: Int.max - (track.width ?? 0),
        inverseHeight: Int.max - (track.height ?? 0),
        inverseBitRate: Int.max - Int(clamping: track.bitRate ?? 0),
        trackId: track.id
    )
}

private func requestedCodecPenalty(_ requested: String?, _ candidate: String?) -> Int {
    guard let requested else {
        return 0
    }
    guard let candidate else {
        return 1
    }
    return requested == candidate ? 0 : 3
}

private func requestedVariantDistance(
    _ requested: Int?,
    _ candidate: Int?
) -> (missingPenalty: Int, distance: Int) {
    guard let requested else {
        return (0, 0)
    }
    guard let candidate else {
        return (1, Int.max / 4)
    }
    return (0, abs(candidate - requested))
}

private func requestedVariantDistance(
    _ requested: Int64?,
    _ candidate: Int64?
) -> (missingPenalty: Int, distance: Int64) {
    guard let requested else {
        return (0, 0)
    }
    guard let candidate else {
        return (1, Int64.max / 4)
    }
    return (0, abs(candidate - requested))
}

private func preferredVideoVariantTrack(
    _ lhs: VesperMediaTrack,
    over rhs: VesperMediaTrack
) -> VesperMediaTrack {
    let lhsBitRate = lhs.bitRate ?? -1
    let rhsBitRate = rhs.bitRate ?? -1
    if lhsBitRate != rhsBitRate {
        return lhsBitRate > rhsBitRate ? lhs : rhs
    }

    let lhsMaxEdge = max(lhs.width ?? 0, lhs.height ?? 0)
    let rhsMaxEdge = max(rhs.width ?? 0, rhs.height ?? 0)
    if lhsMaxEdge != rhsMaxEdge {
        return lhsMaxEdge > rhsMaxEdge ? lhs : rhs
    }

    let lhsMinEdge = min(lhs.width ?? 0, lhs.height ?? 0)
    let rhsMinEdge = min(rhs.width ?? 0, rhs.height ?? 0)
    if lhsMinEdge != rhsMinEdge {
        return lhsMinEdge > rhsMinEdge ? lhs : rhs
    }

    let lhsFrameRate = Int((lhs.frameRate ?? 0).rounded())
    let rhsFrameRate = Int((rhs.frameRate ?? 0).rounded())
    if lhsFrameRate != rhsFrameRate {
        return lhsFrameRate > rhsFrameRate ? lhs : rhs
    }

    return (lhs.label ?? lhs.id) <= (rhs.label ?? rhs.id) ? lhs : rhs
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
                warmupTask = Task.detached(priority: .utility) {
                    await Self.executeWarmup(handle: self.sessionHandle, task: task, url: url)
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
        url: URL
    ) async {
        let warmupBytes = max(Int64(task.expected_memory_bytes), 1)
        var request = URLRequest(url: url)
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

private func fourCharCodeString(_ value: UInt32) -> String {
    let scalarValues = [
        UInt8((value >> 24) & 0xFF),
        UInt8((value >> 16) & 0xFF),
        UInt8((value >> 8) & 0xFF),
        UInt8(value & 0xFF),
    ]
    let printable = scalarValues.allSatisfy { (0x20 ... 0x7E).contains($0) }
    guard printable else {
        return String(format: "0x%08X", value)
    }
    return String(bytes: scalarValues, encoding: .ascii) ?? String(format: "0x%08X", value)
}
