@preconcurrency import AVFoundation
import CoreAudio
import Foundation
internal import VesperPlayerKitBridgeShim

struct VesperNativeFrameAudioBridgeState: Equatable {
    let hasAudioTrack: Bool
    let decoderKind: String
    let outputKind: String
    let pipelineKind: String
    let rateControlKind: String
    let clockSource: String
    let issue: String?

    static func resolved(
        hasAudioTrack: Bool,
        bridgePrepared: Bool,
        unavailableReason: String? = nil
    ) -> VesperNativeFrameAudioBridgeState {
        if bridgePrepared {
            return VesperNativeFrameAudioBridgeState(
                hasAudioTrack: true,
                decoderKind: "swiftNativeAudioBridge",
                outputKind: "swiftNativeAudioBridge",
                pipelineKind: "swiftNativeAudioBridgeV1",
                rateControlKind: "swiftNativeAudioBridgeTimePitch",
                clockSource: "swiftNativeAudioBridge",
                issue: nil
            )
        }
        if hasAudioTrack {
            return VesperNativeFrameAudioBridgeState(
                hasAudioTrack: true,
                decoderKind: "unavailable",
                outputKind: "unavailable",
                pipelineKind: "swiftNativeAudioBridgeV1",
                rateControlKind: "unavailable",
                clockSource: "video",
                issue: unavailableReason ?? "Swift native audio bridge is unavailable."
            )
        }
        return VesperNativeFrameAudioBridgeState(
            hasAudioTrack: false,
            decoderKind: "none",
            outputKind: "none",
            pipelineKind: "none",
            rateControlKind: "none",
            clockSource: "video",
            issue: nil
        )
    }
}

struct VesperNativeFramePipelineFrame: @unchecked Sendable {
    let frameHandle: UInt64
    let pixelBufferAddress: UInt
    let pixelBuffer: CVPixelBuffer
    let presentationTimeUs: Int64
    let durationUs: Int64?
    let width: Int
    let height: Int
    let leaseGeneration: UInt64
}

/// Result of polling the SDK pipeline for the next frame. `endOfStream` is a
/// terminal signal distinct from `pending` (decoder still draining) so the
/// display loop can stop polling and report end-of-playback.
enum VesperNativeFramePipelineAdvanceOutcome {
    case frame(VesperNativeFramePipelineFrame)
    case pending
    case endOfStream
}

struct VesperNativeFramePipelineTimeline: Equatable {
    let positionMs: Int64
    let durationMs: Int64?
}

enum VesperNativeFramePipelineRouteDecision: Equatable {
    case systemPlayer
    case fallback(VesperNativeFramePipelineIssue)
    case fail(VesperNativeFramePipelineIssue)
    case waitForSurface(VesperNativeFramePipelineIssue)
    case nativeFrame
}

struct VesperNativeFramePipelineCounters: Equatable {
    var processedFrames = 0
    var presentedFrames = 0
    var deadlineMisses = 0
    var backpressureCount = 0
    var lateDropped = 0
    var skippedAudioPackets = 0
    var skippedVideoPackets = 0
    var skippedOtherPackets = 0
}

struct VesperNativeFramePipelineStartupError: LocalizedError, Equatable {
    let issue: VesperNativeFramePipelineIssue

    var message: String {
        issue.message
    }

    var errorDescription: String? {
        message
    }
}

struct VesperNativeFramePipelineIssue: Equatable {
    enum Kind: String {
        case missingSurface
        case missingSourceNormalizerPacketPlugin
        case missingVideoToolboxDecoderPlugin
        case unsupportedSource
        case unsupportedCodec
        case hdrProgrammableProcessingNotSupported
        case sessionNotPrepared
        case sessionClosed
        case nativeAudioBridgeUnavailable
        case startupFailure
    }

    let kind: Kind
    let message: String

    static func classifyStartupFailure(_ message: String) -> VesperNativeFramePipelineIssue {
        if let parsed = parseWireIssue(message) {
            return parsed
        }
        let normalized = message.lowercased()
        if normalized.contains("playersurfaceview") || normalized.contains("surface view") {
            return VesperNativeFramePipelineIssue(kind: .missingSurface, message: message)
        }
        if normalized.contains("sourcenormalizer packet-stream plugin path") ||
            normalized.contains("sourcenormalizer packet plugin") ||
            normalized.contains("source normalizer packet plugin") ||
            normalized.contains("failed to open plugin library")
        {
            return VesperNativeFramePipelineIssue(
                kind: .missingSourceNormalizerPacketPlugin,
                message: message
            )
        }
        if normalized.contains("videotoolbox decoder plugin path") ||
            normalized.contains("is not a native-frame decoder plugin") ||
            normalized.contains("failed to load native-frame decoder plugin")
        {
            return VesperNativeFramePipelineIssue(
                kind: .missingVideoToolboxDecoderPlugin,
                message: message
            )
        }
        if normalized.contains("unsupported source") ||
            normalized.contains("does not handle hls") ||
            normalized.contains("does not handle dash") ||
            normalized.contains("system playback remains the supported route")
        {
            return VesperNativeFramePipelineIssue(kind: .unsupportedSource, message: message)
        }
        if normalized.contains("hdrprogrammableprocessingnotsupported") ||
            normalized.contains("hdr programmable") ||
            normalized.contains("sdk-managed native-frame processing is sdr-only")
        {
            return VesperNativeFramePipelineIssue(
                kind: .hdrProgrammableProcessingNotSupported,
                message: message
            )
        }
        if normalized.contains("unsupported codec") ||
            normalized.contains("does not support") ||
            normalized.contains("first pass only supports") ||
            normalized.contains("decoder not found") ||
            normalized.contains("failed to inspect video stream")
        {
            return VesperNativeFramePipelineIssue(kind: .unsupportedCodec, message: message)
        }
        if normalized.contains("already closed") {
            return VesperNativeFramePipelineIssue(kind: .sessionClosed, message: message)
        }
        if normalized.contains("not prepared") {
            return VesperNativeFramePipelineIssue(kind: .sessionNotPrepared, message: message)
        }
        if normalized.contains("swift native audio bridge") ||
            normalized.contains("native audio bridge") ||
            normalized.contains("audio bridge")
        {
            return VesperNativeFramePipelineIssue(
                kind: .nativeAudioBridgeUnavailable,
                message: message
            )
        }
        return VesperNativeFramePipelineIssue(kind: .startupFailure, message: message)
    }

    private static func parseWireIssue(_ message: String) -> VesperNativeFramePipelineIssue? {
        let prefix = "nativeFrameIssueKind="
        guard message.hasPrefix(prefix),
              let separator = message.firstIndex(of: ";") else {
            return nil
        }
        let kindStart = message.index(message.startIndex, offsetBy: prefix.count)
        let rawKind = String(message[kindStart..<separator])
        let detailsStart = message.index(after: separator)
        let details = message[detailsStart...].trimmingCharacters(in: .whitespacesAndNewlines)
        guard let kind = Kind(rawValue: rawKind) else {
            return VesperNativeFramePipelineIssue(kind: .startupFailure, message: details)
        }
        return VesperNativeFramePipelineIssue(kind: kind, message: details)
    }
}
