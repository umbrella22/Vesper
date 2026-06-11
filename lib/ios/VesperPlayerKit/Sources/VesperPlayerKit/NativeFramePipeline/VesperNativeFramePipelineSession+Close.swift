@preconcurrency import AVFoundation
import CoreAudio
import Foundation
import VesperPlayerKitBridgeShim
extension VesperNativeFramePipelineSession {
    func close() {
        guard !isClosed else { return }
        isClosed = true
        isPlaying = false
        let runtime = runtime
        self.runtime = nil
        commandQueue.cancel()
        commandQueue.submit { [runtime] _ in
            await runtime?.close()
        }
        audioOutput.close()
        onFramePresented = nil
        nativeFramePresenter.setNativeFramePresentationEnabled(false)
        onPlaybackFailed = nil
    }
}
