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
    @Published private(set) var publishedLastError: VesperPlayerError?

    private var currentSource: VesperPlayerSource?
    private var player: AVPlayer?
    private weak var surfaceHost: PlayerSurfaceView?
    private var timeObserverToken: Any?
    private var endObserver: NSObjectProtocol?
    private var pendingAutoPlay = false
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
    private var audioOptionsByTrackId: [String: AVMediaSelectionOption] = [:]
    private var subtitleOptionsByTrackId: [String: AVMediaSelectionOption] = [:]
    private var resiliencePolicy: VesperPlaybackResiliencePolicy
    private let trackPreferencePolicy: VesperTrackPreferencePolicy
    private var resolvedTrackPreferencePolicy: VesperTrackPreferencePolicy
    private var hasAppliedDefaultTrackPreferences = false
    private var pendingResilienceRestore: PendingResilienceRestore?
    private var retryTask: Task<Void, Never>?
    private var retryAttemptCount = 0
    private let cachePolicyToken = UUID()
    private let preloadCoordinator: VesperNativePreloadCoordinator

    var uiState: PlayerHostUiState {
        publishedUiState
    }

    var trackCatalog: VesperTrackCatalog {
        publishedTrackCatalog
    }

    var trackSelection: VesperTrackSelectionSnapshot {
        publishedTrackSelection
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
        self.resiliencePolicy = resiliencePolicy
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
        preloadCoordinator.cancelAll()
        VesperSharedUrlCacheCoordinator.shared.remove(token: cachePolicyToken)
        pendingResilienceRestore = nil
        hasAppliedDefaultTrackPreferences = false
        pendingAutoPlay = false
        pendingPlayAfterStopSeek = false
        isSeekingToStartAfterStop = false
        removeObservers()
        player?.pause()
        surfaceHost?.attach(player: nil)
        player = nil
        resetTrackState()
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
        player?.pause()
        player?.seek(to: .zero, toleranceBefore: .zero, toleranceAfter: .zero) { [weak self] _ in
            guard let self else { return }
            Task { @MainActor in
                iosHostLog("stop seek completed")
                self.isSeekingToStartAfterStop = false
                self.updateTimelinePosition(0)
                if self.pendingPlayAfterStopSeek {
                    self.pendingPlayAfterStopSeek = false
                    iosHostLog("resuming deferred play after stop seek")
                    self.startPlayback()
                }
                self.refreshPlaybackState()
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
        clearLastError()
        let trackIdText = policy.trackId ?? "nil"
        let maxBitRateText = policy.maxBitRate.map(String.init) ?? "nil"
        let maxWidthText = policy.maxWidth.map(String.init) ?? "nil"
        let maxHeightText = policy.maxHeight.map(String.init) ?? "nil"
        iosHostLog(
            "setAbrPolicy mode=\(policy.mode.rawValue) trackId=\(trackIdText) maxBitRate=\(maxBitRateText) maxWidth=\(maxWidthText) maxHeight=\(maxHeightText)"
        )
        let hasResolutionLimit = policy.maxWidth != nil || policy.maxHeight != nil
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
            if hasResolutionLimit && (policy.maxWidth == nil || policy.maxHeight == nil) {
                reportCommandError(
                    category: .unsupported,
                    message:
                        "setAbrPolicy constrained mode requires both maxWidth and maxHeight for AVPlayer resolution limits"
                )
                return
            }
        case .fixedTrack:
            reportCommandError(
                category: .unsupported,
                message: "setAbrPolicy fixedTrack is not implemented on iOS AVPlayer"
            )
            return
        case .auto:
            break
        }

        guard let item = player?.currentItem else {
            iosHostLog("setAbrPolicy ignored: no current item")
            return
        }

        switch policy.mode {
        case .auto:
            item.preferredPeakBitRate = 0
            item.preferredMaximumResolution = .zero
            updateTrackSelection { current in
                VesperTrackSelectionSnapshot(
                    video: current.video,
                    audio: current.audio,
                    subtitle: current.subtitle,
                    abrPolicy: .auto()
                )
            }
        case .constrained:
            item.preferredPeakBitRate = policy.maxBitRate.map(Double.init) ?? 0
            if let maxWidth = policy.maxWidth, let maxHeight = policy.maxHeight {
                item.preferredMaximumResolution = CGSize(
                    width: CGFloat(maxWidth),
                    height: CGFloat(maxHeight)
                )
            } else {
                item.preferredMaximumResolution = .zero
            }
            updateTrackSelection { current in
                VesperTrackSelectionSnapshot(
                    video: current.video,
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
            return
        }
    }

    func setResiliencePolicy(_ policy: VesperPlaybackResiliencePolicy) {
        clearLastError()
        if resiliencePolicy == policy {
            return
        }

        resiliencePolicy = policy
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
        let resolvedResiliencePolicy = resiliencePolicy.resolvedForRuntimeSource(currentSource)
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

        removeObservers()
        pendingPlaybackStart = false
        hasAppliedDefaultTrackPreferences = false
        resetTrackState()
        self.player = player
        surfaceHost?.attach(player: player)
        installObservers(for: player, item: item)

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
        let time = CMTime(milliseconds: positionMs)
        player?.seek(to: time) { [weak self] _ in
            guard let self else { return }
            Task { @MainActor in
                self.updateTimelinePosition(positionMs)
                self.refreshPlaybackState()
            }
        }
    }

    private func installObservers(for player: AVPlayer, item: AVPlayerItem) {
        timeObserverToken = player.addPeriodicTimeObserver(
            forInterval: CMTime(seconds: 0.25, preferredTimescale: 600),
            queue: .main
        ) { [weak self] _ in
            guard let self else { return }
            Task { @MainActor in
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
        audioOptionsByTrackId = [:]
        subtitleOptionsByTrackId = [:]
        hasAppliedDefaultTrackPreferences = false
        publishedTrackCatalog = .empty
        publishedTrackSelection = VesperTrackSelectionSnapshot()
    }

    private func updateTrackSelection(
        _ transform: (VesperTrackSelectionSnapshot) -> VesperTrackSelectionSnapshot
    ) {
        publishedTrackSelection = transform(publishedTrackSelection)
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
        setAbrPolicy(resolvedTrackPreferencePolicy.abrPolicy)
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

    private func refreshTrackCatalogAndSelection(for item: AVPlayerItem) {
        Task { [weak self, weak item] in
            guard let self, let item else { return }
            let trackState = await self.loadTrackCatalogState(for: item)
            guard self.player?.currentItem === item else { return }
            self.audioGroup = trackState.audioGroup
            self.subtitleGroup = trackState.subtitleGroup
            self.audioOptionsByTrackId = trackState.audioOptionsByTrackId
            self.subtitleOptionsByTrackId = trackState.subtitleOptionsByTrackId
            self.publishedTrackCatalog = trackState.catalog
            self.applyDefaultTrackPreferencesIfNeeded(for: item)
            self.applyPendingResilienceRestore(ifNeededFor: item, phase: .trackSelection)
        }
    }

    private func loadTrackCatalogState(for item: AVPlayerItem) async -> LoadedTrackCatalogState {
        let asset = item.asset
        let audibleGroup = await loadMediaSelectionGroup(for: .audible, asset: asset)
        let legibleGroup = await loadMediaSelectionGroup(for: .legible, asset: asset)

        var tracks: [VesperMediaTrack] = []
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
            audioOptionsByTrackId: audioOptionsByTrackId,
            subtitleOptionsByTrackId: subtitleOptionsByTrackId
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
    }

    private func reportCommandError(category: VesperPlayerErrorCategory, message: String) {
        iosHostLog("commandError category=\(category.rawValue) message=\(message)")
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

        let retryPolicy = resiliencePolicy.resolvedForRuntimeSource(currentSource).retry
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
        retryTask = Task { [weak self] in
            try? await Task.sleep(nanoseconds: delayMs * 1_000_000)
            guard !Task.isCancelled else { return }
            await MainActor.run {
                guard
                    let self,
                    self.currentSource?.uri == expectedUri
                else {
                    return
                }
                iosHostLog("retrying playback attempt=\(nextAttempt) delayMs=\(delayMs)")
                self.initialize()
            }
        }
        return true
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

        setAbrPolicy(state.abrPolicy)

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
    let audioOptionsByTrackId: [String: AVMediaSelectionOption]
    let subtitleOptionsByTrackId: [String: AVMediaSelectionOption]
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
