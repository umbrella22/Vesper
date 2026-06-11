@preconcurrency import AVFoundation
import CoreAudio
import Foundation
import VesperPlayerKitBridgeShim
extension VesperNativeFramePipelineSession {
    func mergeStatus(from object: [String: Any]) {
        updateDuration(from: object["durationMillis"] as? NSNumber)
        updateCounters(from: object["counters"] as? [String: Any])
        if let value = object["seekable"] as? Bool {
            seekable = value
        } else if let value = object["seekable"] as? NSNumber {
            seekable = value.boolValue
        }
        if let value = object["hasAudioTrack"] as? Bool {
            hasAudioTrack = value
        } else if let value = object["hasAudioTrack"] as? NSNumber {
            hasAudioTrack = value.boolValue
        }
        if let value = object["selectedVideoStreamIndex"] as? NSNumber {
            selectedVideoStreamIndex = value.intValue
        } else if let value = object["selectedVideoStreamIndex"] as? Int {
            selectedVideoStreamIndex = value
        }
        if let value = object["selectedVideoMediaKind"] as? String, !value.isEmpty {
            selectedVideoMediaKind = value
        }
        if let value = object["videoOutputFormat"] as? String, !value.isEmpty {
            videoOutputFormat = value
        }
        if let value = object["videoTransfer"] as? String, !value.isEmpty {
            videoTransfer = value
        }
        if let value = object["videoBitDepth"] as? NSNumber {
            videoBitDepth = value.intValue
        } else if let value = object["videoBitDepth"] as? Int {
            videoBitDepth = value
        } else if let value = object["videoBitDepth"] as? String, let parsed = Int(value) {
            videoBitDepth = parsed
        }
        if let value = object["hdrKind"] as? String, !value.isEmpty {
            hdrKind = value
        }
        if let value = object["dolbyVisionMode"] as? String, !value.isEmpty {
            dolbyVisionMode = value
        }
        if let value = object["audioStreamIndex"] as? NSNumber {
            audioStreamIndex = value.intValue
        } else if let value = object["audioStreamIndex"] as? Int {
            audioStreamIndex = value
        }
        if let value = object["audioMediaKind"] as? String, !value.isEmpty {
            audioMediaKind = value
        }
        if let value = object["clockSource"] as? String, !value.isEmpty {
            clockSource = value
        }
        if let audioBridgeState {
            applyAudioBridgeStateValues(audioBridgeState)
        }
    }

    func applyAudioBridgeStateValues(_ state: VesperNativeFrameAudioBridgeState) {
        hasAudioTrack = state.hasAudioTrack
        audioDecoderKind = state.decoderKind
        audioOutputKind = state.outputKind
        audioPipelineKind = state.pipelineKind
        audioRateControlKind = state.rateControlKind
        clockSource = state.clockSource
        audioOutputIssue = state.issue
    }

    func updateCounters(from countersObject: [String: Any]?) {
        guard let countersObject else { return }
        counters = VesperNativeFramePipelineCounters(
            processedFrames: (countersObject["processedFrames"] as? NSNumber)?.intValue
                ?? (countersObject["processed_frames"] as? NSNumber)?.intValue
                ?? counters.processedFrames,
            presentedFrames: (countersObject["presentedFrames"] as? NSNumber)?.intValue
                ?? (countersObject["presented_frames"] as? NSNumber)?.intValue
                ?? counters.presentedFrames,
            deadlineMisses: (countersObject["deadlineMisses"] as? NSNumber)?.intValue
                ?? (countersObject["deadline_misses"] as? NSNumber)?.intValue
                ?? counters.deadlineMisses,
            backpressureCount: (countersObject["backpressureCount"] as? NSNumber)?.intValue
                ?? (countersObject["backpressure_count"] as? NSNumber)?.intValue
                ?? counters.backpressureCount,
            lateDropped: (countersObject["lateDropped"] as? NSNumber)?.intValue
                ?? (countersObject["late_dropped"] as? NSNumber)?.intValue
                ?? counters.lateDropped,
            skippedAudioPackets: (countersObject["skippedAudioPackets"] as? NSNumber)?.intValue
                ?? (countersObject["skipped_audio_packets"] as? NSNumber)?.intValue
                ?? counters.skippedAudioPackets,
            skippedVideoPackets: (countersObject["skippedVideoPackets"] as? NSNumber)?.intValue
                ?? (countersObject["skipped_video_packets"] as? NSNumber)?.intValue
                ?? counters.skippedVideoPackets,
            skippedOtherPackets: (countersObject["skippedOtherPackets"] as? NSNumber)?.intValue
                ?? (countersObject["skipped_other_packets"] as? NSNumber)?.intValue
                ?? counters.skippedOtherPackets
        )
    }

    func updateDuration(from durationMillis: NSNumber?) {
        guard let durationMillis else { return }
        let value = durationMillis.int64Value
        if value > 0 {
            durationMs = value
        }
    }
}
