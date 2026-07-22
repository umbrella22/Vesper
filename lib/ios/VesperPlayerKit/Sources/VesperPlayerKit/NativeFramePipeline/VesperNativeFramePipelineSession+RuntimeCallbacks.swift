@preconcurrency import AVFoundation
import CoreAudio
import Foundation
internal import VesperPlayerKitBridgeShim
extension VesperNativeFramePipelineSession {
    func runtimePresent(frame: VesperNativeFramePipelineFrame) async -> Bool {
        guard !isClosed else { return false }
        return await nativeFramePresenter.presentNativeFrame(pixelBuffer: frame.pixelBuffer)
    }

    func runtimeTimeline(framePresentationTimeUs presentationTimeUs: Int64) -> VesperNativeFramePipelineTimeline {
        VesperNativeFramePipelineTimeline(
            positionMs: timelinePositionMs(framePresentationTimeUs: presentationTimeUs),
            durationMs: durationMs
        )
    }

    func runtimeDidPresentFrame(_ timeline: VesperNativeFramePipelineTimeline) {
        guard !isClosed, isPlaying else { return }
        onFramePresented?(timeline)
    }

    func runtimeMergeStatus(_ object: [String: Any]) {
        guard !isClosed else { return }
        mergeStatus(from: object)
    }
}
