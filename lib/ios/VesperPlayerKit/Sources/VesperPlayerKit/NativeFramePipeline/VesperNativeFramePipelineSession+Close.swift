@preconcurrency import AVFoundation
import CoreAudio
import Foundation
internal import VesperPlayerKitBridgeShim
extension VesperNativeFramePipelineSession {
    func close(detachPresenter: Bool = true) {
        guard !isClosed else { return }
        isClosed = true
        isPlaying = false
        let runtime = runtime
        self.runtime = nil
        commandQueue.cancel()
        Task { [runtime] in
            await runtime?.close()
        }
        audioOutput.close()
        onFramePresented = nil
        if detachPresenter {
            nativeFramePresenter.setNativeFramePresentationEnabled(false)
        }
        onPlaybackFailed = nil
    }
}
