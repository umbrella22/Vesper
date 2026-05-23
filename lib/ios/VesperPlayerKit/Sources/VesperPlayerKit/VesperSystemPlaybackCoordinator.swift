import AVFoundation
import Foundation
import MediaPlayer
import UIKit

@MainActor
public final class VesperSystemPlaybackCoordinator {
    private weak var controller: VesperPlayerController?
    private var configuration: VesperSystemPlaybackConfiguration?
    private var metadata: VesperSystemPlaybackMetadata?
    private var commandTargets: [(MPRemoteCommand, Any)] = []
    private var artworkTask: Task<Void, Never>?
    private var artworkImage: UIImage?
    private var artworkUri: String?
    private var interruptionObserver: NSObjectProtocol?
    private var routeChangeObserver: NSObjectProtocol?
    private var wasPlayingBeforeInterruption = false
    private var audioSessionActive = false

    public init(controller: VesperPlayerController) {
        self.controller = controller
        registerAudioSessionObservers()
    }

    deinit {
        artworkTask?.cancel()
        if let interruptionObserver {
            NotificationCenter.default.removeObserver(interruptionObserver)
        }
        if let routeChangeObserver {
            NotificationCenter.default.removeObserver(routeChangeObserver)
        }
    }

    public func configure(_ configuration: VesperSystemPlaybackConfiguration) {
        self.configuration = configuration
        if let metadata = configuration.metadata {
            self.metadata = metadata
            refreshArtworkIfNeeded(for: metadata.artworkUri)
        }

        guard configuration.enabled else {
            clear()
            return
        }

        if configuration.backgroundMode == .continueAudio {
            activatePlaybackAudioSession()
        }

        if configuration.showSystemControls {
            registerRemoteCommands(configuration: configuration)
        } else {
            unregisterRemoteCommands()
        }
        updateNowPlayingInfo()
    }

    public func updateMetadata(_ metadata: VesperSystemPlaybackMetadata) {
        self.metadata = metadata
        refreshArtworkIfNeeded(for: metadata.artworkUri)
        updateNowPlayingInfo()
    }

    public func updatePlaybackState(_ uiState: PlayerHostUiState) {
        guard configuration?.enabled == true else { return }
        updateNowPlayingInfo(uiState: uiState)
    }

    public func clear() {
        configuration = nil
        metadata = nil
        artworkTask?.cancel()
        artworkTask = nil
        artworkImage = nil
        artworkUri = nil
        unregisterRemoteCommands()
        MPNowPlayingInfoCenter.default().nowPlayingInfo = nil
        deactivatePlaybackAudioSessionIfNeeded()
    }

    private func activatePlaybackAudioSession() {
        guard !audioSessionActive else {
            return
        }
        if VesperSharedAudioSession.activate(owner: self) {
            audioSessionActive = true
        }
    }

    private func deactivatePlaybackAudioSessionIfNeeded() {
        guard audioSessionActive else {
            return
        }
        VesperSharedAudioSession.deactivate(owner: self)
        audioSessionActive = false
    }

    private func registerAudioSessionObservers() {
        let center = NotificationCenter.default
        let session = AVAudioSession.sharedInstance()
        interruptionObserver = center.addObserver(
            forName: AVAudioSession.interruptionNotification,
            object: session,
            queue: .main
        ) { [weak self] notification in
            Task { @MainActor in
                self?.handleInterruption(notification)
            }
        }
        routeChangeObserver = center.addObserver(
            forName: AVAudioSession.routeChangeNotification,
            object: session,
            queue: .main
        ) { [weak self] notification in
            Task { @MainActor in
                self?.handleRouteChange(notification)
            }
        }
    }

    private func handleInterruption(_ notification: Notification) {
        guard
            let rawType = notification.userInfo?[AVAudioSessionInterruptionTypeKey] as? UInt,
            let type = AVAudioSession.InterruptionType(rawValue: rawType)
        else {
            return
        }

        switch type {
        case .began:
            wasPlayingBeforeInterruption = controller?.uiState.playbackState == .playing
            controller?.setAudioSessionInterrupted(true)
            controller?.pause()
        case .ended:
            controller?.setAudioSessionInterrupted(false)
            let rawOptions =
                notification.userInfo?[AVAudioSessionInterruptionOptionKey] as? UInt ?? 0
            let options = AVAudioSession.InterruptionOptions(rawValue: rawOptions)
            if wasPlayingBeforeInterruption && options.contains(.shouldResume) {
                controller?.play()
            }
            wasPlayingBeforeInterruption = false
        @unknown default:
            break
        }
    }

    private func handleRouteChange(_ notification: Notification) {
        guard
            let rawReason = notification.userInfo?[AVAudioSessionRouteChangeReasonKey] as? UInt,
            let reason = AVAudioSession.RouteChangeReason(rawValue: rawReason)
        else {
            return
        }

        if reason == .oldDeviceUnavailable {
            controller?.pause()
        }
    }

    private func registerRemoteCommands(configuration: VesperSystemPlaybackConfiguration) {
        unregisterRemoteCommands()

        let commandCenter = MPRemoteCommandCenter.shared()
        addTarget(commandCenter.playCommand) { [weak self] _ in
            self?.controller?.play()
            return .success
        }
        addTarget(commandCenter.pauseCommand) { [weak self] _ in
            self?.controller?.pause()
            return .success
        }
        addTarget(commandCenter.togglePlayPauseCommand) { [weak self] _ in
            self?.controller?.togglePause()
            return .success
        }
        addTarget(commandCenter.stopCommand) { [weak self] _ in
            self?.controller?.stop()
            return .success
        }

        let controls = configuration.controls.normalized(
            showSeekActions: configuration.showSeekActions
        )
        let seekBackOffsetMs = controls.seekOffsetMs(for: .seekBack)
        let seekForwardOffsetMs = controls.seekOffsetMs(for: .seekForward)
        let enablesSeek = configuration.showSeekActions

        commandCenter.changePlaybackPositionCommand.isEnabled = enablesSeek
        commandCenter.skipForwardCommand.isEnabled = enablesSeek && seekForwardOffsetMs != nil
        commandCenter.skipBackwardCommand.isEnabled = enablesSeek && seekBackOffsetMs != nil

        guard enablesSeek else { return }

        if let seekForwardOffsetMs {
            commandCenter.skipForwardCommand.preferredIntervals = [
                NSNumber(value: Double(seekForwardOffsetMs) / 1000.0),
            ]
            addTarget(commandCenter.skipForwardCommand) { [weak self] _ in
                self?.controller?.seek(by: seekForwardOffsetMs)
                return .success
            }
        }
        if let seekBackOffsetMs {
            commandCenter.skipBackwardCommand.preferredIntervals = [
                NSNumber(value: Double(seekBackOffsetMs) / 1000.0),
            ]
            addTarget(commandCenter.skipBackwardCommand) { [weak self] _ in
                self?.controller?.seek(by: -seekBackOffsetMs)
                return .success
            }
        }
        addTarget(commandCenter.changePlaybackPositionCommand) { [weak self] event in
            guard
                let self,
                let positionEvent = event as? MPChangePlaybackPositionCommandEvent,
                let controller = self.controller
            else {
                return .commandFailed
            }
            let targetMs = Int64(positionEvent.positionTime * 1000)
            let deltaMs = targetMs - controller.uiState.timeline.positionMs
            controller.seek(by: deltaMs)
            return .success
        }
    }

    private func addTarget(
        _ command: MPRemoteCommand,
        handler: @escaping @MainActor (MPRemoteCommandEvent) -> MPRemoteCommandHandlerStatus
    ) {
        command.isEnabled = true
        let target = command.addTarget { event in
            if Thread.isMainThread {
                return MainActor.assumeIsolated {
                    handler(event)
                }
            }
            return .commandFailed
        }
        commandTargets.append((command, target))
    }

    private func unregisterRemoteCommands() {
        for (command, target) in commandTargets {
            command.removeTarget(target)
        }
        commandTargets.removeAll()

        let commandCenter = MPRemoteCommandCenter.shared()
        commandCenter.playCommand.isEnabled = false
        commandCenter.pauseCommand.isEnabled = false
        commandCenter.togglePlayPauseCommand.isEnabled = false
        commandCenter.stopCommand.isEnabled = false
        commandCenter.changePlaybackPositionCommand.isEnabled = false
        commandCenter.skipForwardCommand.isEnabled = false
        commandCenter.skipBackwardCommand.isEnabled = false
        commandCenter.skipForwardCommand.preferredIntervals = []
        commandCenter.skipBackwardCommand.preferredIntervals = []
    }

    private func updateNowPlayingInfo(uiState explicitUiState: PlayerHostUiState? = nil) {
        guard configuration?.enabled == true else { return }
        let uiState = explicitUiState ?? controller?.uiState
        guard let uiState else { return }

        let metadata = metadata
        var info = MPNowPlayingInfoCenter.default().nowPlayingInfo ?? [:]
        info[MPMediaItemPropertyTitle] = metadata?.title.nonEmpty ?? uiState.sourceLabel
        info[MPMediaItemPropertyArtist] = metadata?.artist
        info[MPMediaItemPropertyAlbumTitle] = metadata?.albumTitle

        let durationMs = metadata?.durationMs ?? uiState.timeline.durationMs
        if let durationMs, durationMs > 0 {
            info[MPMediaItemPropertyPlaybackDuration] = Double(durationMs) / 1000.0
        } else {
            info.removeValue(forKey: MPMediaItemPropertyPlaybackDuration)
        }
        info[MPNowPlayingInfoPropertyElapsedPlaybackTime] =
            Double(max(uiState.timeline.positionMs, 0)) / 1000.0
        info[MPNowPlayingInfoPropertyPlaybackRate] =
            uiState.playbackState == .playing ? Double(uiState.playbackRate) : 0.0
        info[MPNowPlayingInfoPropertyIsLiveStream] =
            (metadata?.isLive == true) || uiState.timeline.kind != .vod

        if let artworkImage {
            info[MPMediaItemPropertyArtwork] =
                MPMediaItemArtwork(boundsSize: artworkImage.size) { _ in artworkImage }
        } else {
            info.removeValue(forKey: MPMediaItemPropertyArtwork)
        }

        MPNowPlayingInfoCenter.default().nowPlayingInfo = info
    }

    private func refreshArtworkIfNeeded(for uri: String?) {
        guard artworkUri != uri else { return }
        artworkUri = uri
        artworkImage = nil
        artworkTask?.cancel()
        guard let uri, !uri.isEmpty else { return }

        artworkTask = Task { [weak self] in
            let image = await Self.loadArtwork(uri: uri)
            await MainActor.run {
                guard let self, self.artworkUri == uri else { return }
                self.artworkImage = image
                self.updateNowPlayingInfo()
            }
        }
    }

    private static func loadArtwork(uri: String) async -> UIImage? {
        if let url = URL(string: uri) {
            if url.isFileURL {
                return UIImage(contentsOfFile: url.path)
            }
            if ["http", "https"].contains(url.scheme?.lowercased()) {
                do {
                    let (data, _) = try await URLSession.shared.data(from: url)
                    return UIImage(data: data)
                } catch {
                    return nil
                }
            }
        }
        return UIImage(contentsOfFile: uri)
    }
}

@MainActor
enum VesperSharedAudioSession {
    private static var activeOwners: Set<ObjectIdentifier> = []

    @discardableResult
    static func activate(owner: AnyObject) -> Bool {
        assert(Thread.isMainThread)
        let ownerId = ObjectIdentifier(owner)
        do {
            let session = AVAudioSession.sharedInstance()
            try session.setCategory(.playback, mode: .moviePlayback, options: [])
            try session.setActive(true)
            activeOwners.insert(ownerId)
            return true
        } catch {
            iosHostLog("audio session activation failed: \(error.localizedDescription)")
            return false
        }
    }

    static func deactivate(owner: AnyObject) {
        assert(Thread.isMainThread)
        activeOwners.remove(ObjectIdentifier(owner))
        guard activeOwners.isEmpty else {
            return
        }
        do {
            try AVAudioSession.sharedInstance().setActive(
                false,
                options: .notifyOthersOnDeactivation
            )
        } catch {
            iosHostLog("audio session deactivation failed: \(error.localizedDescription)")
        }
    }
}

private extension String {
    var nonEmpty: String? {
        isEmpty ? nil : self
    }
}
