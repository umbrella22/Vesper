@preconcurrency import AVFoundation
import CoreAudio
import Foundation
@_implementationOnly import VesperPlayerKitBridgeShim

@MainActor
final class VesperNativeFrameAudioOutput: VesperNativeFrameAudioOutputing, @unchecked Sendable {
    var engine: AVAudioEngine?
    var playerNode: AVAudioPlayerNode?
    var timePitch: AVAudioUnitTimePitch?
    var asset: AVURLAsset?
    var sourceURL: URL?
    var preparedAudioFormat: AVAudioFormat?
    var audioDecodeTask: Task<Void, Never>?
    var scheduledBufferGate: VesperNativeFrameAudioScheduledBufferGate?
    let playbackGate = VesperNativeFrameAudioPlaybackGate()
    var playbackRate: Float = 1.0
    var isPrepared = false
    var seekPositionMs: Int64 = 0
    var onStateChanged: ((VesperNativeFrameAudioBridgeState) -> Void)?

    var currentPositionMs: Int64? {
        guard isPrepared else { return nil }
        guard playerNode?.isPlaying == true else {
            return seekPositionMs
        }
        guard let nodeTime = playerNode?.lastRenderTime,
              let playerTime = playerNode?.playerTime(forNodeTime: nodeTime),
              playerTime.sampleRate > 0
        else {
            return seekPositionMs
        }
        let renderedMs = Int64(
            (Double(playerTime.sampleTime) / playerTime.sampleRate * 1_000.0).rounded(.down)
        )
        return max(seekPositionMs + renderedMs, 0)
    }

    func prepare(
        source: VesperPlayerSource,
        hasAudioTrack: Bool
    ) async -> VesperNativeFrameAudioBridgeState {
        close()
        guard hasAudioTrack else {
            return VesperNativeFrameAudioBridgeState.resolved(
                hasAudioTrack: false,
                bridgePrepared: false
            )
        }
        guard source.kind == .local,
              let url = URL(string: source.uri),
              url.isFileURL
        else {
            return VesperNativeFrameAudioBridgeState.resolved(
                hasAudioTrack: true,
                bridgePrepared: false,
                unavailableReason: "Swift native audio bridge v1 only supports local file sources."
            )
        }
        sourceURL = url
        let asset = AVURLAsset(url: url)
        do {
            preparedAudioFormat = try await Self.preflightAudioFormat(asset: asset)
        } catch {
            return VesperNativeFrameAudioBridgeState.resolved(
                hasAudioTrack: true,
                bridgePrepared: false,
                unavailableReason: "Swift native audio bridge preflight failed: \(error.localizedDescription)"
            )
        }
        self.asset = asset
        isPrepared = true
        seekPositionMs = 0
        return VesperNativeFrameAudioBridgeState.resolved(
            hasAudioTrack: true,
            bridgePrepared: true
        )
    }

    func play(rate: Float) {
        guard isPrepared else { return }
        playbackRate = max(rate, 0.01)
        rebuildAndStart()
    }

    func pause() {
        seekPositionMs = currentPositionMs ?? seekPositionMs
        playbackGate.cancelPlayback()
        playerNode?.pause()
        engine?.pause()
    }

    func stop() {
        audioDecodeTask?.cancel()
        audioDecodeTask = nil
        playbackGate.cancelPlayback()
        playerNode?.stop()
        engine?.stop()
        playerNode = nil
        timePitch = nil
        engine = nil
        scheduledBufferGate = nil
        seekPositionMs = 0
    }

    func seek(toMs positionMs: Int64) {
        seekPositionMs = max(positionMs, 0)
        if playerNode?.isPlaying == true || playbackGate.wantsPlayback {
            rebuildAndStart()
        }
    }

    func setPlaybackRate(_ rate: Float) {
        let positionBeforeRateChange = currentPositionMs
        playbackRate = max(rate, 0.01)
        timePitch?.rate = playbackRate
        if playerNode?.isPlaying == true || playbackGate.wantsPlayback {
            seekPositionMs = positionBeforeRateChange ?? seekPositionMs
            rebuildAndStart()
        }
    }

    func close() {
        stop()
        asset = nil
        sourceURL = nil
        preparedAudioFormat = nil
        isPrepared = false
    }
}

final class VesperNativeFrameAudioScheduledBufferGate: @unchecked Sendable {
    let semaphore: DispatchSemaphore

    init(maxQueuedBuffers: Int) {
        semaphore = DispatchSemaphore(value: max(maxQueuedBuffers, 1))
    }

    func waitUntilSlotAvailable() throws {
        while semaphore.wait(timeout: .now() + 0.05) == .timedOut {
            if Task.isCancelled {
                throw CancellationError()
            }
        }
    }

    func releaseSlot() {
        semaphore.signal()
    }
}

@MainActor
final class VesperNativeFrameAudioPlaybackGate {
    private(set) var generation: UInt64 = 0
    private(set) var wantsPlayback = false

    func beginPlayback() -> UInt64 {
        wantsPlayback = true
        generation = generation &+ 1
        return generation
    }

    func cancelPlayback() {
        wantsPlayback = false
        generation = generation &+ 1
    }

    func isCurrent(_ generation: UInt64) -> Bool {
        wantsPlayback && self.generation == generation
    }
}

enum VesperNativeFrameAudioOutputError: LocalizedError {
    case noAudioTrack
    case readerOutputRejected
    case readerStartFailed
    case readerFailed
    case readerProducedNoAudio

    var errorDescription: String? {
        switch self {
        case .noAudioTrack:
            return "source has no audio track"
        case .readerOutputRejected:
            return "AVAssetReader rejected the native audio output"
        case .readerStartFailed:
            return "AVAssetReader failed to start"
        case .readerFailed:
            return "AVAssetReader failed"
        case .readerProducedNoAudio:
            return "AVAssetReader produced no audio samples"
        }
    }
}
