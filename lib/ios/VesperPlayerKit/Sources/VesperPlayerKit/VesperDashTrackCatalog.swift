import Foundation

struct VesperDashManifestTrackCatalogSnapshot: Equatable {
    let videoTracks: [VesperMediaTrack]
    let audioTracks: [VesperMediaTrack]
    let videoVariantPinsByTrackId: [String: LoadedVideoVariantPin]
    let adaptiveVideo: Bool
    let adaptiveAudio: Bool

    init(
        audio: [VesperDashPlayableRepresentation],
        video: [VesperDashPlayableRepresentation]
    ) {
        var pinsByTrackId: [String: LoadedVideoVariantPin] = [:]
        var videoTracks: [VesperMediaTrack] = []
        videoTracks.reserveCapacity(video.count)
        pinsByTrackId.reserveCapacity(video.count)

        for (index, item) in video.enumerated() {
            let track = Self.videoTrack(for: item, index: index)
            videoTracks.append(track)
            pinsByTrackId[track.id] = LoadedVideoVariantPin(
                peakBitRate: item.representation.bandwidth.map(Double.init),
                maxWidth: item.representation.width,
                maxHeight: item.representation.height
            )
        }

        self.videoTracks = videoTracks
        audioTracks = audio.enumerated().map { index, item in
            Self.audioTrack(for: item, index: index)
        }
        videoVariantPinsByTrackId = pinsByTrackId
        adaptiveVideo = video.count > 1
        adaptiveAudio = audio.count > 1
    }

    func audioMetadata(at index: Int) -> VesperMediaTrack? {
        guard audioTracks.indices.contains(index) else {
            return nil
        }
        return audioTracks[index]
    }

    private static func videoTrack(
        for item: VesperDashPlayableRepresentation,
        index: Int
    ) -> VesperMediaTrack {
        let representation = item.representation
        return VesperMediaTrack(
            id: "video:dash:\(item.renditionId)",
            kind: .video,
            label: videoTrackLabel(representation: representation),
            language: item.adaptationSet.language,
            codec: mediaCodec(representation: representation),
            bitRate: int64(representation.bandwidth),
            width: representation.width,
            height: representation.height,
            frameRate: frameRate(representation.frameRate),
            channels: nil,
            sampleRate: int(representation.audioSamplingRate),
            isDefault: index == 0,
            isForced: false
        )
    }

    private static func audioTrack(
        for item: VesperDashPlayableRepresentation,
        index: Int
    ) -> VesperMediaTrack {
        let representation = item.representation
        return VesperMediaTrack(
            id: "audio:dash:\(item.renditionId)",
            kind: .audio,
            label: audioTrackLabel(item: item, index: index),
            language: item.adaptationSet.language,
            codec: mediaCodec(representation: representation),
            bitRate: int64(representation.bandwidth),
            width: nil,
            height: nil,
            frameRate: nil,
            channels: nil,
            sampleRate: int(representation.audioSamplingRate),
            isDefault: index == 0,
            isForced: false
        )
    }

    private static func videoTrackLabel(representation: VesperDashRepresentation) -> String {
        if let height = representation.height {
            return "\(height)p"
        }
        if let width = representation.width, let height = representation.height {
            return "\(width)x\(height)"
        }
        if let bandwidth = representation.bandwidth {
            return "\(bandwidth)"
        }
        return representation.id.isEmpty ? "Video" : representation.id
    }

    private static func audioTrackLabel(
        item: VesperDashPlayableRepresentation,
        index: Int
    ) -> String {
        if let language = item.adaptationSet.language, !language.isEmpty {
            return language
        }
        if let id = item.adaptationSet.id, !id.isEmpty {
            return id
        }
        return item.representation.id.isEmpty ? "audio-\(index + 1)" : item.representation.id
    }

    private static func mediaCodec(representation: VesperDashRepresentation) -> String? {
        if !representation.codecs.isEmpty {
            return representation.codecs
        }
        if !representation.mimeType.isEmpty {
            return representation.mimeType
        }
        return nil
    }

    private static func int64(_ value: UInt64?) -> Int64? {
        guard let value, value <= UInt64(Int64.max) else {
            return nil
        }
        return Int64(value)
    }

    private static func int(_ value: String?) -> Int? {
        guard
            let value,
            let parsed = Int(value.trimmingCharacters(in: .whitespacesAndNewlines)),
            parsed > 0
        else {
            return nil
        }
        return parsed
    }

    private static func frameRate(_ value: String?) -> Double? {
        guard let value else {
            return nil
        }
        let normalized = value.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !normalized.isEmpty else {
            return nil
        }

        if normalized.contains("/") {
            let components = normalized.split(separator: "/", maxSplits: 1)
            guard
                components.count == 2,
                let numerator = Double(components[0]),
                let denominator = Double(components[1]),
                denominator > 0
            else {
                return nil
            }
            let parsed = numerator / denominator
            return parsed.isFinite && parsed > 0 ? parsed : nil
        }

        guard let parsed = Double(normalized), parsed.isFinite, parsed > 0 else {
            return nil
        }
        return parsed
    }
}
