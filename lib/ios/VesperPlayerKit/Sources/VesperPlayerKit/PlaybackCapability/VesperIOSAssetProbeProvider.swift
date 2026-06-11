import AVFoundation
import Foundation

enum VesperIOSAssetProbeProvider {
    static func probe(
        _ request: VesperPlaybackCapabilityProbeRequest
    ) async -> VesperPlaybackCapabilityAssetProbeResult? {
        guard let source = request.source,
            source.protocol == .file || source.protocol == .progressive || source.protocol == .hls
        else {
            return nil
        }
        guard let url = URL(string: source.uri) else {
            return VesperPlaybackCapabilityAssetProbeResult(
                diagnostics: [
                    "assetProbe": "iosAVAsset",
                    "assetProbeError": "invalidSourceUrl",
                ]
            )
        }

        let asset = AVURLAsset(url: url)
        return await probe(asset)
    }

    static func probe(_ asset: AVAsset) async -> VesperPlaybackCapabilityAssetProbeResult {
        var diagnostics: [String: String] = [
            "assetProbe": "iosAVAsset",
            "assetProbeAvailable": "true",
        ]

        do {
            let isPlayable = try await asset.load(.isPlayable)
            diagnostics["assetPlayable"] = String(isPlayable)

            let videoTracks = try await asset.loadTracks(withMediaType: .video)
            diagnostics["assetVideoTrackCount"] = String(videoTracks.count)
            if let firstVideoTrack = videoTracks.first {
                diagnostics.merge(await videoDiagnostics(for: firstVideoTrack)) { _, new in new }
            }

            return VesperPlaybackCapabilityAssetProbeResult(
                isPlayable: isPlayable,
                videoTrackCount: videoTracks.count,
                metadataHdrKind: VesperPlaybackCapabilityProbe.detectMetadataHdrKind(diagnostics),
                diagnostics: diagnostics
            )
        } catch {
            diagnostics["assetProbeError"] = String(describing: type(of: error))
            diagnostics["assetProbeErrorMessage"] = error.localizedDescription
            return VesperPlaybackCapabilityAssetProbeResult(diagnostics: diagnostics)
        }
    }

    private static func videoDiagnostics(for track: AVAssetTrack) async -> [String: String] {
        var diagnostics: [String: String] = [:]

        if let naturalSize = try? await track.load(.naturalSize) {
            let width = abs(Int(naturalSize.width.rounded()))
            let height = abs(Int(naturalSize.height.rounded()))
            if width > 0 {
                diagnostics["assetVideoWidth"] = String(width)
            }
            if height > 0 {
                diagnostics["assetVideoHeight"] = String(height)
            }
        }

        if let nominalFrameRate = try? await track.load(.nominalFrameRate),
            nominalFrameRate.isFinite,
            nominalFrameRate > 0
        {
            diagnostics["assetVideoFrameRate"] = String(Double(nominalFrameRate))
        }

        if let estimatedDataRate = try? await track.load(.estimatedDataRate),
            estimatedDataRate.isFinite,
            estimatedDataRate > 0
        {
            diagnostics["assetVideoEstimatedDataRate"] = String(Int(estimatedDataRate.rounded()))
        }

        if let formatDescription = (try? await track.load(.formatDescriptions))?.first {
            let mediaSubtype = CMFormatDescriptionGetMediaSubType(formatDescription)
            diagnostics["assetVideoCodec"] = playbackCapabilityFourCharCodeString(mediaSubtype)
            diagnostics.merge(formatDescriptionColorDiagnostics(formatDescription)) { _, new in new }
        }

        return diagnostics
    }

    private static func formatDescriptionColorDiagnostics(
        _ formatDescription: CMFormatDescription
    ) -> [String: String] {
        guard let extensions = CMFormatDescriptionGetExtensions(formatDescription) as? [String: Any]
        else {
            return [:]
        }

        var diagnostics: [String: String] = [:]
        copyExtension(
            kCMFormatDescriptionExtension_ColorPrimaries,
            from: extensions,
            into: &diagnostics,
            diagnosticKey: "assetVideoColorPrimaries"
        )
        copyExtension(
            kCMFormatDescriptionExtension_TransferFunction,
            from: extensions,
            into: &diagnostics,
            diagnosticKey: "assetVideoTransferFunction"
        )
        copyExtension(
            kCMFormatDescriptionExtension_YCbCrMatrix,
            from: extensions,
            into: &diagnostics,
            diagnosticKey: "assetVideoYCbCrMatrix"
        )
        diagnostics.merge(VesperIOSHdrStaticMetadataDiagnostics.diagnostics(from: extensions)) { _, new in
            new
        }
        if diagnostics["assetVideoTransferFunction"] != nil ||
            diagnostics["assetVideoAlternativeTransferCharacteristics"] != nil ||
            diagnostics["assetVideoMasteringDisplayColorVolumePresent"] == "true" ||
            diagnostics["assetVideoContentLightLevelInfoPresent"] == "true"
        {
            diagnostics["assetVideoHdrMetadataProbe"] = "formatDescription"
        }
        return diagnostics
    }

    private static func copyExtension(
        _ key: CFString,
        from extensions: [String: Any],
        into diagnostics: inout [String: String],
        diagnosticKey: String
    ) {
        guard let value = extensions[key as String] else {
            return
        }
        diagnostics[diagnosticKey] = String(describing: value)
    }
}
