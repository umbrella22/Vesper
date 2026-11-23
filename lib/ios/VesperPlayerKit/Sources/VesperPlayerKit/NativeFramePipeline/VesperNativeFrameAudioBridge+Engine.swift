@preconcurrency import AVFoundation
import CoreAudio
import Foundation
@_implementationOnly import VesperPlayerKitBridgeShim

extension VesperNativeFrameAudioOutput {
    func rebuildAndStart() {
        guard let asset, let preparedAudioFormat else { return }
        audioDecodeTask?.cancel()
        audioDecodeTask = nil
        playbackGate.cancelPlayback()
        playerNode?.stop()
        engine?.stop()
        playerNode = nil
        timePitch = nil
        self.engine = nil
        scheduledBufferGate = nil
        let playbackGeneration = playbackGate.beginPlayback()
        let processor: VesperNativeFrameAudioProcessor?
        do {
            processor = try processorBinding?.open(format: preparedAudioFormat, epoch: playbackGeneration)
        } catch {
            markBridgeUnavailable(reason: error.localizedDescription)
            return
        }
        let engine = AVAudioEngine()
        let playerNode = AVAudioPlayerNode()
        let timePitch = AVAudioUnitTimePitch()
        playerNode.volume = outputVolume
        timePitch.rate = playbackRate
        engine.attach(playerNode)
        engine.attach(timePitch)
        engine.connect(playerNode, to: timePitch, format: preparedAudioFormat)
        engine.connect(timePitch, to: engine.mainMixerNode, format: preparedAudioFormat)
        do {
            try engine.start()
        } catch {
            iosHostLog("native audio engine start failed: \(error.localizedDescription)")
            markBridgeUnavailable(reason: "Swift native audio bridge engine start failed: \(error.localizedDescription)")
            return
        }
        self.engine = engine
        self.playerNode = playerNode
        self.timePitch = timePitch
        let bufferGate = VesperNativeFrameAudioScheduledBufferGate(maxQueuedBuffers: 12)
        scheduledBufferGate = bufferGate

        audioDecodeTask = Task.detached(priority: .userInitiated) {
            [self, asset, seekPositionMs, playerNode, bufferGate, playbackGeneration, processor] in
            do {
                var reportedAudioProcessor = false
                try await Self.streamPcmBuffers(asset: asset, startMs: seekPositionMs) { pcmBuffer, ptsUs in
                    try Task.checkCancellation()
                    if let processor {
                        try processor.process(pcmBuffer, ptsUs: ptsUs)
                        if !reportedAudioProcessor {
                            iosHostLog("native-frame audio processed PCM epoch=\(playbackGeneration)")
                            reportedAudioProcessor = true
                        }
                    }
                    try bufferGate.waitUntilSlotAvailable()
                    if Task.isCancelled {
                        bufferGate.releaseSlot()
                        throw CancellationError()
                    }
                    let scheduled = await MainActor.run {
                        guard self.playerNode === playerNode,
                              self.playbackGate.isCurrent(playbackGeneration) else {
                            return false
                        }
                        playerNode.scheduleBuffer(
                            pcmBuffer,
                            completionCallbackType: .dataConsumed
                        ) { _ in
                            bufferGate.releaseSlot()
                        }
                        if !playerNode.isPlaying {
                            playerNode.play()
                        }
                        return true
                    }
                    if !scheduled {
                        bufferGate.releaseSlot()
                        throw CancellationError()
                    }
                }
            } catch is CancellationError {
                return
            } catch {
                await MainActor.run {
                    guard self.playbackGate.isCurrent(playbackGeneration) else { return }
                    iosHostLog("native audio decode failed: \(error.localizedDescription)")
                    self.markBridgeUnavailable(
                        reason: "Swift native audio bridge decode failed: \(error.localizedDescription)"
                    )
                }
            }
        }
    }

    func markBridgeUnavailable(reason: String) {
        audioDecodeTask?.cancel()
        audioDecodeTask = nil
        playbackGate.cancelPlayback()
        playerNode?.stop()
        engine?.stop()
        playerNode = nil
        timePitch = nil
        engine = nil
        scheduledBufferGate = nil
        isPrepared = false
        onStateChanged?(
            VesperNativeFrameAudioBridgeState.resolved(
                hasAudioTrack: true,
                bridgePrepared: false,
                unavailableReason: reason
            )
        )
    }
}
