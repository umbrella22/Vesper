@preconcurrency import AVFoundation
import CoreAudio
import Foundation
import VesperPlayerKitBridgeShim
extension VesperNativeFramePipelineSession {
    func play(rate: Float = 1.0) {
        guard didStart, !isClosed else { return }
        playbackRate = max(rate, 0.01)
        isPlaying = true
        audioOutput.play(rate: playbackRate)
        guard let runtime else { return }
        commandQueue.submit { [runtime, playbackRate] _ in
            await runtime.play(rate: playbackRate)
        }
    }

    func pause() {
        isPlaying = false
        audioOutput.pause()
        guard let runtime else { return }
        commandQueue.submit { [runtime] _ in
            await runtime.pause()
        }
    }

    func stop() {
        isPlaying = false
        audioOutput.stop()
        seek(toMs: 0)
    }

    func flush() {
        guard didStart else { return }
        isPlaying = false
        audioOutput.pause()
        guard let runtime else { return }
        commandQueue.submit { [self, runtime] token in
            let result = await runtime.flush()
            await MainActor.run {
                guard commandQueue.isLatest(token) else { return }
                applyRuntimeCommandResult(result, operation: "flush")
            }
        }
    }

    func setPlaybackRate(_ rate: Float) {
        playbackRate = max(rate, 0.01)
        audioOutput.setPlaybackRate(playbackRate)
        guard let runtime else { return }
        commandQueue.submit { [runtime, playbackRate] _ in
            await runtime.setPlaybackRate(playbackRate)
        }
    }

    func applyAudioBridgeState(_ state: VesperNativeFrameAudioBridgeState) {
        audioBridgeState = state
        applyAudioBridgeStateValues(state)
    }

    @discardableResult
    func seek(
        toMs positionMs: Int64,
        completion: (@MainActor (Bool) -> Void)? = nil
    ) -> Bool {
        guard didStart else { return false }
        guard seekable else {
            iosHostLog("native-frame seek failed: source is not seekable")
            return false
        }
        let targetMs = clampedSeekPositionMs(positionMs)
        let wasPlaying = isPlaying
        isPlaying = false
        audioOutput.pause()
        guard let runtime else { return false }
        commandQueue.submit { [self, runtime] token in
            let result = await runtime.seek(positionMs: targetMs)
            await MainActor.run {
                guard commandQueue.isLatest(token) else {
                    completion?(false)
                    return
                }
                let didApply = applyRuntimeSeekResult(
                    result,
                    targetMs: targetMs,
                    resumePlayback: wasPlaying
                )
                completion?(didApply)
            }
        }
        return true
    }
}
