@preconcurrency import AVFoundation
import Foundation

extension VesperNativePlayerBridge {
    func installPlaybackStallObserver(player: AVPlayer, item: AVPlayerItem) {
        playbackStallTask?.cancel()
        guard let token = activePlayerObservationToken else { return }
        // Media-time observers stop firing when media time stalls. Use wall time.
        playbackStallTask = Task { @MainActor [weak self, weak player, weak item] in
            while !Task.isCancelled {
                do { try await Task.sleep(for: .seconds(1)) } catch { return }
                guard !Task.isCancelled, let self, let player, let item,
                      self.player === player, player.currentItem === item,
                      self.diagnosticsTracker.isCurrent(token) else { return }
                let time = player.currentTime()
                let position = time.isNumeric ? time.milliseconds : nil
                let eligible = player.timeControlStatus != .paused &&
                    item.status == .readyToPlay && item.error == nil &&
                    self.publishedLastError == nil && !self.uiState.isInterrupted &&
                    self.activeSeekCommand == nil && self.activeLegacySeekId == nil && !self.isSeekingToStartAfterStop
                self.diagnosticsTracker.sampleStall(token, positionMs: position, eligible: eligible,
                    buffering: player.timeControlStatus == .waitingToPlayAtSpecifiedRate)
            }
        }
    }
}
