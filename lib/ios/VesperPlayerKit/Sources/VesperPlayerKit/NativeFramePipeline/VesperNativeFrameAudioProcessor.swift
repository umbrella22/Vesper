@preconcurrency import AVFoundation
import Foundation
@_implementationOnly import VesperPlayerKitBridgeShim

enum VesperNativeFrameAudioProcessorError: LocalizedError {
    case protectedAsset
    case unsupportedFormat
    case negotiationFailed
    case processingFailed

    var errorDescription: String? {
        switch self {
        case .protectedAsset:
            "Protected audio cannot use a native-frame audio processor."
        case .unsupportedFormat:
            "The native-frame audio processor requires planar float PCM with a supported channel count and sample rate."
        case .negotiationFailed:
            "The bundled audio processor could not open a fixed-block session with unchanged PCM format and zero latency."
        case .processingFailed:
            "The bundled audio processor failed to process a PCM block."
        }
    }
}

@MainActor
final class VesperNativeFrameAudioProcessorBinding {
    private let registry: VesperEmbeddedPluginRegistry
    private let json: String

    init(reference: VesperPluginReference) throws {
        let registry = try VesperEmbeddedPluginRegistry.create(references: [reference])
        json = try encodeVesperPluginBindingsJSON([reference], registryHandle: registry.handle)
        self.registry = registry
    }

    func open(format: AVAudioFormat, epoch: UInt64) throws -> VesperNativeFrameAudioProcessor {
        // The binding holds the registry alive until Rust has retained the plugin.
        try withExtendedLifetime(registry) {
            try VesperNativeFrameAudioProcessor(json: json, format: format, epoch: epoch)
        }
    }
}

/// Created on the main actor, then used exclusively by one decoding task.
final class VesperNativeFrameAudioProcessor: @unchecked Sendable {
    // Matches the iOS FFI's maximum scratch block. The plugin can negotiate a smaller block.
    private static let maxBlockFrames = 1024
    private let handle: UInt64
    private let channels: Int
    private let sampleRate: UInt32
    private var scratch: [Float]

    init(json: String, format: AVAudioFormat, epoch: UInt64) throws {
        guard !format.isInterleaved, format.commonFormat == .pcmFormatFloat32,
              (1...8).contains(format.channelCount),
              let channels = UInt16(exactly: format.channelCount),
              let sampleRate = UInt32(exactly: format.sampleRate), sampleRate != 0
        else {
            throw VesperNativeFrameAudioProcessorError.unsupportedFormat
        }
        let bytes = json.utf8CString
        let handle = bytes.withUnsafeBufferPointer { buffer in
            vesper_ios_native_frame_audio_open(buffer.baseAddress, buffer.count - 1, sampleRate, channels, epoch)
        }
        guard handle != 0 else {
            throw VesperNativeFrameAudioProcessorError.negotiationFailed
        }
        self.handle = handle
        self.sampleRate = sampleRate
        self.channels = Int(channels)
        scratch = [Float](repeating: 0, count: Self.maxBlockFrames * Int(channels))
    }

    deinit {
        vesper_ios_native_frame_audio_close(handle)
    }

    func process(_ buffer: AVAudioPCMBuffer, ptsUs: Int64) throws {
        guard !buffer.format.isInterleaved,
              buffer.format.channelCount == channels,
              buffer.format.sampleRate == Double(sampleRate),
              let planes = buffer.floatChannelData
        else {
            throw VesperNativeFrameAudioProcessorError.unsupportedFormat
        }
        var offset = 0
        while offset < Int(buffer.frameLength) {
            try Task.checkCancellation()
            let count = min(Self.maxBlockFrames, Int(buffer.frameLength) - offset)
            let deltaUs = Int64(offset) * 1_000_000 / Int64(sampleRate)
            let blockTime = ptsUs.addingReportingOverflow(deltaUs)
            let blockPts = ptsUs == .min || blockTime.overflow ? Int64.min : blockTime.partialValue
            // AVAudioEngine consumes planar PCM. Adapt to the ABI's interleaved PCM in
            // reusable task-owned scratch, then write the result back before scheduling.
            let processed = scratch.withUnsafeMutableBufferPointer { samples in
                for frame in 0..<count {
                    for channel in 0..<channels {
                        samples[frame * channels + channel] = planes[channel][offset + frame]
                    }
                }
                guard vesper_ios_native_frame_audio_process(handle, samples.baseAddress, count * channels, blockPts) else {
                    return false
                }
                for frame in 0..<count {
                    for channel in 0..<channels {
                        planes[channel][offset + frame] = samples[frame * channels + channel]
                    }
                }
                return true
            }
            guard processed else {
                throw VesperNativeFrameAudioProcessorError.processingFailed
            }
            offset += count
        }
    }
}
