@preconcurrency import AVFoundation
import CoreAudio
import Foundation
internal import VesperPlayerKitBridgeShim

@MainActor
protocol VesperNativeFrameAudioOutputing: AnyObject {
    var onStateChanged: ((VesperNativeFrameAudioBridgeState) -> Void)? { get set }
    var currentPositionMs: Int64? { get }

    func prepare(
        source: VesperPlayerSource,
        hasAudioTrack: Bool
    ) async -> VesperNativeFrameAudioBridgeState
    func play(rate: Float)
    func pause()
    func stop()
    func seek(toMs positionMs: Int64)
    func setPlaybackRate(_ rate: Float)
    func close()
}

@MainActor
protocol VesperNativeFramePresenting: AnyObject {
    func setNativeFramePresentationEnabled(_ enabled: Bool)
    func presentNativeFrame(pixelBuffer: CVPixelBuffer) async -> Bool
}

extension PlayerSurfaceView: VesperNativeFramePresenting {
    func presentNativeFrame(pixelBuffer: CVPixelBuffer) async -> Bool {
        await withCheckedContinuation { continuation in
            presentNativeFrame(pixelBuffer: pixelBuffer) { succeeded in
                continuation.resume(returning: succeeded)
            }
        }
    }
}
