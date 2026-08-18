@preconcurrency import AVFoundation
import CoreAudio
import Foundation
@_implementationOnly import VesperPlayerKitBridgeShim
extension VesperNativeFramePipelineSession {
    func rebindSurfaceHost(_ nextSurfaceHost: PlayerSurfaceView) {
        guard !isClosed else { return }
        if surfaceHost === nextSurfaceHost {
            nextSurfaceHost.setNativeFramePresentationEnabled(true)
            return
        }

        surfaceHost.setNativeFramePresentationEnabled(false)
        surfaceHost = nextSurfaceHost
        if usesSurfaceHostPresenter {
            nativeFramePresenter = nextSurfaceHost
        }
        nextSurfaceHost.setNativeFramePresentationEnabled(true)
    }
}
