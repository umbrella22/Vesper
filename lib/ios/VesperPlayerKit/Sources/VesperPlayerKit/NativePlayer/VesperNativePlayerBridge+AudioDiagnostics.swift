@preconcurrency import AVFoundation

extension VesperNativePlayerBridge {
    func refreshAudioDiagnostics() {
        guard let token = activePlayerObservationToken,
              diagnosticsTracker.isCurrent(token) else { return }
        var audio = VesperAudioPlaybackDiagnostics()
        if let item = player?.currentItem, let group = audioGroup,
           let selected = item.currentMediaSelection.selectedMediaOption(in: group),
           let trackId = audioOptionsByTrackId.first(where: { $0.value == selected })?.key {
            audio.trackId = trackId
            audio.evidence = .selectedMediaOption
            // Codec/rate/channel metadata in the iOS audio catalog comes from
            // DASH. It is not runtime decoder or audio output evidence.
            if currentSource?.protocol == .dash,
               let track = publishedTrackCatalog.audioTracks.first(where: { $0.id == trackId }) {
                audio.codec = track.codec
                audio.channels = track.channels
                audio.sampleRate = track.sampleRate
                if audio.codec != nil || audio.channels != nil || audio.sampleRate != nil {
                    audio.evidence = .manifestMetadata
                }
            }
        }
        diagnosticsTracker.audio(token, value: audio)
    }
}
