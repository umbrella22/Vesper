@preconcurrency import AVFoundation
import CoreAudio
import Foundation
@_implementationOnly import VesperPlayerKitBridgeShim
extension VesperNativeFramePipelineSession {
    @discardableResult
    func play(rate: Float = 1.0) -> Bool {
        guard didStart, !isClosed else { return false }
        playbackRate = max(rate, 0.01)
        desiredPlaybackActive = true
        if hasReachedEnd {
            guard seekable else {
                desiredPlaybackActive = false
                isPlaying = false
                let issue = VesperNativeFramePipelineIssue(
                    kind: .unsupportedOperation,
                    message: "nativeFrameIssueKind=unsupportedOperation; end-of-stream replay requires a seekable source."
                )
                iosHostLog("native-frame replay failed: \(issue.message)")
                onPlaybackFailed?(issue)
                return false
            }
            return seek(toMs: 0) { [weak self] didApply in
                guard let self, !didApply, !self.isClosed else { return }
                self.desiredPlaybackActive = false
                self.isPlaying = false
                self.audioOutput.pause()
                self.onPlaybackFailed?(
                    VesperNativeFramePipelineIssue(
                        kind: .unsupportedOperation,
                        message: "nativeFrameIssueKind=unsupportedOperation; failed to rewind the native-frame source for replay."
                    )
                )
            }
        }
        if pendingSeekGeneration != nil {
            return true
        }
        isPlaying = true
        audioOutput.play(rate: playbackRate)
        guard let runtime else { return false }
        commandQueue.submit { [runtime, playbackRate] _ in
            await runtime.play(rate: playbackRate)
        }
        return true
    }

    func pause() {
        desiredPlaybackActive = false
        isPlaying = false
        audioOutput.pause()
        if pendingSeekGeneration != nil {
            return
        }
        guard let runtime else { return }
        commandQueue.submit { [runtime] _ in
            await runtime.pause()
        }
    }

    func stop() {
        desiredPlaybackActive = false
        isPlaying = false
        audioOutput.stop()
        guard let runtime else { return }
        commandQueue.submit { [runtime] _ in
            await runtime.pause()
        }
        guard seekable else {
            iosHostLog("native-frame stop paused an unseekable source without rewinding")
            return
        }
        _ = seek(toMs: 0)
    }

    func flush() {
        guard didStart else { return }
        desiredPlaybackActive = false
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
        seekGeneration &+= 1
        if seekGeneration == 0 {
            seekGeneration = 1
        }
        let submittedSeekGeneration = seekGeneration
        pendingSeekGeneration = submittedSeekGeneration
        isPlaying = false
        audioOutput.pause()
        guard let runtime else {
            pendingSeekGeneration = nil
            return false
        }
        let submittedToken = commandQueue.submit(
            policy: .replacingPending("seek"),
            onDropped: { [weak self] in
                if self?.pendingSeekGeneration == submittedSeekGeneration {
                    self?.pendingSeekGeneration = nil
                }
                completion?(false)
            }
        ) { [self, runtime] _ in
            let result = await runtime.seek(positionMs: targetMs)
            await MainActor.run {
                guard !isClosed,
                      seekGeneration == submittedSeekGeneration,
                      pendingSeekGeneration == submittedSeekGeneration else {
                    completion?(false)
                    return
                }
                pendingSeekGeneration = nil
                let didApply = applyRuntimeSeekResult(
                    result,
                    targetMs: targetMs
                )
                completion?(didApply)
            }
        }
        if submittedToken == nil, pendingSeekGeneration == submittedSeekGeneration {
            pendingSeekGeneration = nil
        }
        return submittedToken != nil
    }
}
