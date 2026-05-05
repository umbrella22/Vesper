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

    public init(controller: VesperPlayerController) {
        self.controller = controller
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
            registerRemoteCommands(showSeekActions: configuration.showSeekActions)
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
        try? AVAudioSession.sharedInstance().setActive(false, options: .notifyOthersOnDeactivation)
    }

    private func activatePlaybackAudioSession() {
        do {
            let session = AVAudioSession.sharedInstance()
            try session.setCategory(
                .playback,
                mode: .moviePlayback,
                policy: .longFormVideo,
                options: []
            )
            try session.setActive(true)
        } catch {
            iosHostLog("system playback audio session failed: \(error.localizedDescription)")
        }
    }

    private func registerRemoteCommands(showSeekActions: Bool) {
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

        commandCenter.changePlaybackPositionCommand.isEnabled = showSeekActions
        commandCenter.skipForwardCommand.isEnabled = showSeekActions
        commandCenter.skipBackwardCommand.isEnabled = showSeekActions

        guard showSeekActions else { return }

        commandCenter.skipForwardCommand.preferredIntervals = [15]
        commandCenter.skipBackwardCommand.preferredIntervals = [15]
        addTarget(commandCenter.skipForwardCommand) { [weak self] event in
            let interval = (event as? MPSkipIntervalCommandEvent)?.interval ?? 15
            self?.controller?.seek(by: Int64(interval * 1000))
            return .success
        }
        addTarget(commandCenter.skipBackwardCommand) { [weak self] event in
            let interval = (event as? MPSkipIntervalCommandEvent)?.interval ?? 15
            self?.controller?.seek(by: -Int64(interval * 1000))
            return .success
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
            Task { @MainActor in
                _ = handler(event)
            }
            return .success
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

private extension String {
    var nonEmpty: String? {
        isEmpty ? nil : self
    }
}
