@preconcurrency import AVFoundation
import CoreAudio
import Foundation
import VesperPlayerKitBridgeShim
extension VesperNativeFramePipelineSession {
    func applyRuntimeCommandResult(
        _ result: VesperNativeFramePipelineRuntime.CommandResult,
        operation: String
    ) {
        guard !isClosed else { return }
        switch result {
        case .success(let object):
            mergeStatus(from: object)
        case .failure(let error):
            iosHostLog("native-frame \(operation) failed: \(error.message)")
        case .ignored:
            break
        }
    }

    func applyRuntimeSeekResult(
        _ result: VesperNativeFramePipelineRuntime.CommandResult,
        targetMs: Int64,
        resumePlayback: Bool
    ) -> Bool {
        guard !isClosed else { return false }
        switch result {
        case .success(let object):
            mergeStatus(from: object)
            audioOutput.seek(toMs: targetMs)
            onFramePresented?(
                VesperNativeFramePipelineTimeline(
                    positionMs: targetMs,
                    durationMs: durationMs
                )
            )
            if resumePlayback {
                isPlaying = true
                audioOutput.play(rate: playbackRate)
                if let runtime {
                    commandQueue.submit { [runtime, playbackRate] _ in
                        await runtime.play(rate: playbackRate)
                    }
                }
            }
            return true
        case .failure(let error):
            iosHostLog("native-frame seek failed: \(error.message)")
            if resumePlayback {
                isPlaying = true
                audioOutput.play(rate: playbackRate)
                if let runtime {
                    commandQueue.submit { [runtime, playbackRate] _ in
                        await runtime.play(rate: playbackRate)
                    }
                }
            }
            return false
        case .ignored:
            return false
        }
    }

    func clampedSeekPositionMs(_ positionMs: Int64) -> Int64 {
        let lowerBounded = max(positionMs, 0)
        guard let durationMs, durationMs > 0 else {
            return lowerBounded
        }
        return min(lowerBounded, durationMs)
    }

    func timelinePositionMs(framePresentationTimeUs presentationTimeUs: Int64) -> Int64 {
        let videoPositionMs = max(presentationTimeUs / 1_000, 0)
        guard clockSource == "swiftNativeAudioBridge",
              let audioPositionMs = audioOutput.currentPositionMs else {
            return videoPositionMs
        }
        return max(audioPositionMs, 0)
    }

    /// Stops the display loop and reports end-of-playback once the SDK pipeline
    /// drains. A seek clears the Rust-side EOF state and bumps the frame lease, so
    /// `isPlaying` resumes the loop and a later EOF reports again.
    func runtimeDidReachEndOfStream() {
        isPlaying = false
        audioOutput.pause()
        if let durationMs {
            onFramePresented?(
                VesperNativeFramePipelineTimeline(
                    positionMs: durationMs,
                    durationMs: durationMs
                )
            )
        }
        onPlaybackEnded?()
    }

    func failPlaybackForAudioBridge(reason: String) {
        guard !isClosed else { return }
        isPlaying = false
        audioOutput.pause()
        let runtime = runtime
        commandQueue.submit { [runtime] _ in
            await runtime?.pause()
        }
        onPlaybackFailed?(
            VesperNativeFramePipelineIssue(
                kind: .nativeAudioBridgeUnavailable,
                message: reason
            )
        )
    }
}
