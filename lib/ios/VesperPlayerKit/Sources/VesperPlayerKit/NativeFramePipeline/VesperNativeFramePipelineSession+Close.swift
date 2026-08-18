@preconcurrency import AVFoundation
import CoreAudio
import Foundation
@_implementationOnly import VesperPlayerKitBridgeShim
extension VesperNativeFramePipelineSession {
    @discardableResult
    func close(detachPresenter: Bool = true) -> Task<Void, Never> {
        if let nativeFrameCloseCompletion {
            return nativeFrameCloseCompletion
        }
        guard !isClosed else {
            let completed = Task<Void, Never> {}
            nativeFrameCloseCompletion = completed
            return completed
        }
        isClosed = true
        desiredPlaybackActive = false
        isPlaying = false
        pendingSeekGeneration = nil
        let runtime = runtime
        self.runtime = nil
        commandQueue.cancel()
        let startupCompletion: Task<Void, Never>?
        if nativeFrameStartupInProgress {
            startupCompletion = Task { @MainActor [weak self] in
                await self?.waitForNativeFrameStartupCompletion()
            }
        } else {
            startupCompletion = nil
        }
        let closeCompletion = Task { [runtime, startupCompletion] in
            await startupCompletion?.value
            await runtime?.close()
        }
        nativeFrameCloseCompletion = closeCompletion
        audioOutput.close()
        onFramePresented = nil
        if detachPresenter {
            nativeFramePresenter.setNativeFramePresentationEnabled(false)
        }
        onPlaybackFailed = nil
        return closeCompletion
    }
}
