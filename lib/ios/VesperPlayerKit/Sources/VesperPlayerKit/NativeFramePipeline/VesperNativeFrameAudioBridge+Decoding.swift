@preconcurrency import AVFoundation
import CoreAudio
import Foundation
@_implementationOnly import VesperPlayerKitBridgeShim

extension VesperNativeFrameAudioOutput {
    nonisolated static func preflightAudioFormat(asset: AVURLAsset) async throws -> AVAudioFormat {
        let tracks = try await audioTracks(for: asset)
        guard let track = tracks.first else {
            throw VesperNativeFrameAudioOutputError.noAudioTrack
        }
        let output = AVAssetReaderTrackOutput(track: track, outputSettings: pcmOutputSettings())
        output.alwaysCopiesSampleData = false
        let reader = try AVAssetReader(asset: asset)
        guard reader.canAdd(output) else {
            throw VesperNativeFrameAudioOutputError.readerOutputRejected
        }
        reader.add(output)
        guard reader.startReading() else {
            throw reader.error ?? VesperNativeFrameAudioOutputError.readerStartFailed
        }
        defer {
            reader.cancelReading()
        }
        while let sampleBuffer = output.copyNextSampleBuffer() {
            if let format = pcmAudioFormat(from: sampleBuffer) {
                return format
            }
        }
        if reader.status == .failed {
            throw reader.error ?? VesperNativeFrameAudioOutputError.readerFailed
        }
        throw VesperNativeFrameAudioOutputError.readerProducedNoAudio
    }

    nonisolated static func audioTracks(for asset: AVURLAsset) async throws -> [AVAssetTrack] {
        if #available(iOS 16.0, *) {
            return try await asset.loadTracks(withMediaType: .audio)
        } else {
            return legacyAudioTracks(for: asset)
        }
    }

    @available(iOS, introduced: 4.0, deprecated: 16.0)
    nonisolated static func legacyAudioTracks(for asset: AVURLAsset) -> [AVAssetTrack] {
        asset.tracks(withMediaType: .audio)
    }

    nonisolated static func streamPcmBuffers(
        asset: AVURLAsset,
        startMs: Int64,
        onBuffer: (AVAudioPCMBuffer) async throws -> Void
    ) async throws {
        let tracks = try await asset.loadTracks(withMediaType: .audio)
        guard let track = tracks.first else {
            throw VesperNativeFrameAudioOutputError.noAudioTrack
        }
        let reader = try AVAssetReader(asset: asset)
        let output = AVAssetReaderTrackOutput(track: track, outputSettings: pcmOutputSettings())
        output.alwaysCopiesSampleData = false
        guard reader.canAdd(output) else {
            throw VesperNativeFrameAudioOutputError.readerOutputRejected
        }
        reader.add(output)
        if startMs > 0 {
            let start = CMTime(value: CMTimeValue(startMs), timescale: 1_000)
            reader.timeRange = CMTimeRange(start: start, duration: .positiveInfinity)
        }
        guard reader.startReading() else {
            throw reader.error ?? VesperNativeFrameAudioOutputError.readerStartFailed
        }
        var producedAudio = false
        while !Task.isCancelled, let sampleBuffer = output.copyNextSampleBuffer() {
            if let pcmBuffer = pcmBuffer(from: sampleBuffer) {
                producedAudio = true
                try await onBuffer(pcmBuffer)
            }
        }
        if Task.isCancelled {
            reader.cancelReading()
            throw CancellationError()
        }
        if reader.status == .failed {
            throw reader.error ?? VesperNativeFrameAudioOutputError.readerFailed
        }
        guard producedAudio else {
            throw VesperNativeFrameAudioOutputError.readerProducedNoAudio
        }
    }

    nonisolated static func pcmOutputSettings() -> [String: Any] {
        [
            AVFormatIDKey: kAudioFormatLinearPCM,
            AVLinearPCMBitDepthKey: 32,
            AVLinearPCMIsFloatKey: true,
            AVLinearPCMIsNonInterleaved: true,
            AVLinearPCMIsBigEndianKey: false,
        ]
    }

    nonisolated static func pcmAudioFormat(from sampleBuffer: CMSampleBuffer) -> AVAudioFormat? {
        guard let formatDescription = CMSampleBufferGetFormatDescription(sampleBuffer) else {
            return nil
        }
        guard let streamDescription = CMAudioFormatDescriptionGetStreamBasicDescription(formatDescription)?
            .pointee
        else {
            return nil
        }
        let channelCount = AVAudioChannelCount(streamDescription.mChannelsPerFrame)
        guard streamDescription.mSampleRate > 0, channelCount > 0 else {
            return nil
        }
        return AVAudioFormat(
            commonFormat: .pcmFormatFloat32,
            sampleRate: streamDescription.mSampleRate,
            channels: channelCount,
            interleaved: false
        )
    }

    nonisolated static func pcmBuffer(from sampleBuffer: CMSampleBuffer) -> AVAudioPCMBuffer? {
        guard let audioFormat = pcmAudioFormat(from: sampleBuffer) else { return nil }
        let channelCount = audioFormat.channelCount
        let frameCount = AVAudioFrameCount(CMSampleBufferGetNumSamples(sampleBuffer))
        guard frameCount > 0 else {
            return nil
        }
        guard let buffer = AVAudioPCMBuffer(pcmFormat: audioFormat, frameCapacity: frameCount) else {
            return nil
        }
        buffer.frameLength = frameCount
        var blockBuffer: CMBlockBuffer?
        var bufferListSize = 0
        let sizeStatus = CMSampleBufferGetAudioBufferListWithRetainedBlockBuffer(
            sampleBuffer,
            bufferListSizeNeededOut: &bufferListSize,
            bufferListOut: nil,
            bufferListSize: 0,
            blockBufferAllocator: kCFAllocatorDefault,
            blockBufferMemoryAllocator: kCFAllocatorDefault,
            flags: 0,
            blockBufferOut: nil
        )
        guard sizeStatus == noErr, bufferListSize > 0 else {
            return nil
        }
        let rawBufferList = UnsafeMutableRawPointer.allocate(
            byteCount: bufferListSize,
            alignment: MemoryLayout<AudioBufferList>.alignment
        )
        defer {
            rawBufferList.deallocate()
        }
        let audioBufferList = rawBufferList.bindMemory(to: AudioBufferList.self, capacity: 1)
        let status = CMSampleBufferGetAudioBufferListWithRetainedBlockBuffer(
            sampleBuffer,
            bufferListSizeNeededOut: nil,
            bufferListOut: audioBufferList,
            bufferListSize: bufferListSize,
            blockBufferAllocator: kCFAllocatorDefault,
            blockBufferMemoryAllocator: kCFAllocatorDefault,
            flags: 0,
            blockBufferOut: &blockBuffer
        )
        guard status == noErr else { return nil }
        let sourceBuffers = UnsafeMutableAudioBufferListPointer(audioBufferList)
        let targetBuffers = UnsafeMutableAudioBufferListPointer(buffer.mutableAudioBufferList)
        let channelCountInt = Int(channelCount)
        guard targetBuffers.count == channelCountInt else {
            return nil
        }
        for channelIndex in 0..<channelCountInt {
            guard let targetData = targetBuffers[channelIndex].mData else {
                return nil
            }
            memset(targetData, 0, Int(targetBuffers[channelIndex].mDataByteSize))
        }
        if sourceBuffers.count == channelCountInt {
            for channelIndex in 0..<channelCountInt {
                guard
                    let sourceData = sourceBuffers[channelIndex].mData,
                    let targetData = targetBuffers[channelIndex].mData
                else {
                    return nil
                }
                memcpy(
                    targetData,
                    sourceData,
                    min(
                        Int(sourceBuffers[channelIndex].mDataByteSize),
                        Int(targetBuffers[channelIndex].mDataByteSize)
                    )
                )
            }
            return buffer
        }
        guard sourceBuffers.count == 1,
              let sourceData = sourceBuffers.first?.mData else {
            return nil
        }
        let sourceSamples = sourceData.assumingMemoryBound(to: Float.self)
        let sourceFrameCount = min(
            Int(frameCount),
            Int(sourceBuffers[0].mDataByteSize) / (channelCountInt * MemoryLayout<Float>.size)
        )
        for channelIndex in 0..<channelCountInt {
            guard let targetData = targetBuffers[channelIndex].mData else {
                return nil
            }
            let targetSamples = targetData.assumingMemoryBound(to: Float.self)
            for frameIndex in 0..<sourceFrameCount {
                targetSamples[frameIndex] = sourceSamples[frameIndex * channelCountInt + channelIndex]
            }
        }
        return buffer
    }
}

private final class VesperNativeFrameAudioTrackLoadResultBox: @unchecked Sendable {
    private let lock = NSLock()
    private var result: Result<[AVAssetTrack], Error>?

    func store(_ result: Result<[AVAssetTrack], Error>) {
        lock.lock()
        self.result = result
        lock.unlock()
    }

    func takeResult() -> Result<[AVAssetTrack], Error>? {
        lock.lock()
        defer { lock.unlock() }
        let value = result
        result = nil
        return value
    }
}
