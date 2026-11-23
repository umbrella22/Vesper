@preconcurrency import AVFoundation
import CoreAudio
import Foundation
@_implementationOnly import VesperPlayerKitBridgeShim

@MainActor
final class VesperNativeFrameAudioOutput: VesperNativeFrameAudioOutputing, @unchecked Sendable {
    let outputVolume: Float
    let processorReference: VesperPluginReference?
    var processorBinding: VesperNativeFrameAudioProcessorBinding?
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

    init(outputVolume: Float = 1.0, processorReference: VesperPluginReference? = nil) {
        self.outputVolume = min(max(outputVolume, 0.0), 1.0)
        self.processorReference = processorReference
    }

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
        let asset = AVURLAsset(url: url)
        let preparationGeneration = playbackGate.generation
        do {
            if processorReference != nil {
                guard source.drmConfiguration == nil else {
                    throw VesperNativeFrameAudioProcessorError.protectedAsset
                }
                if try await asset.load(.hasProtectedContent) {
                    throw VesperNativeFrameAudioProcessorError.protectedAsset
                }
            }
            let format = try await Self.preflightAudioFormat(asset: asset)
            try Task.checkCancellation()
            guard preparationGeneration == playbackGate.generation else {
                throw CancellationError()
            }
            if let processorReference {
                let binding = try VesperNativeFrameAudioProcessorBinding(reference: processorReference)
                // Verify the plugin's PCM contract during startup, while prefer mode can still fall back.
                _ = try binding.open(format: format, epoch: 1)
                processorBinding = binding
            }
            preparedAudioFormat = format
        } catch {
            return VesperNativeFrameAudioBridgeState.resolved(
                hasAudioTrack: true,
                bridgePrepared: false,
                unavailableReason: "Swift native audio bridge preflight failed: \(error.localizedDescription)"
            )
        }
        sourceURL = url
        self.asset = asset
        isPrepared = true
        seekPositionMs = 0
        return VesperNativeFrameAudioBridgeState.resolved(
            hasAudioTrack: true,
            bridgePrepared: true,
            usesAudioProcessor: processorBinding != nil
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
        processorBinding = nil
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
