@preconcurrency import AVFoundation
import Foundation
@_implementationOnly import VesperPlayerKitBridgeShim

extension VesperDashSession {
    func selectedPlayableRepresentations(
        manifest: VesperDashManifest,
        variantPolicy: VesperDashMasterPlaylistVariantPolicy
    ) throws -> VesperDashSelectedPlayableResponse {
        if let cached = selectedPlayableByPolicy[variantPolicy] {
            return cached
        }
        let videoDecodeCapabilities = try videoDecodeCapabilities(for: manifest)
        let selected = try VesperDashHlsBuilder.selectedPlayableRepresentations(
            manifest: manifest,
            variantPolicy: variantPolicy,
            videoDecodeCapabilities: videoDecodeCapabilities
        )
        let response = VesperDashSelectedPlayableResponse(
            audio: selected.audio,
            video: selected.video,
            subtitles: selected.subtitles
        )
        selectedPlayableByPolicy[variantPolicy] = response
        if variantPolicy == .all {
            playableByRenditionId = Dictionary(
                uniqueKeysWithValues: (response.audio + response.video + response.subtitles).map {
                    ($0.renditionId, $0)
                }
            )
        }
        return response
    }

    func videoDecodeCapabilities(
        for manifest: VesperDashManifest
    ) throws -> [VesperDashVideoDecodeCapability] {
        if let cached = videoDecodeCapabilitiesCache {
            return cached
        }
        let selected = try VesperDashHlsBuilder.selectedPlayableRepresentations(
            manifest: manifest,
            variantPolicy: .all,
            videoDecodeCapabilities: nil
        )
        let capabilities = selected.video.map(videoDecodeCapability)
        videoDecodeCapabilitiesCache = capabilities
        return capabilities
    }

    func videoDecodeCapability(
        for playable: VesperDashPlayableRepresentation
    ) -> VesperDashVideoDecodeCapability {
        videoDecodeCapabilityProvider(playable)
    }

    nonisolated static func defaultVideoDecodeCapability(
        for playable: VesperDashPlayableRepresentation
    ) -> VesperDashVideoDecodeCapability {
        let candidate = VesperHardwareDecodeCandidateCodec(codecName: playable.representation.codecs)
        let hardwareDecodeSupported = VesperCodecSupport.hardwareDecodeSupported(
            for: playable.representation.codecs
        )
        return VesperDashVideoDecodeCapability(
            renditionId: playable.renditionId,
            codecFamily: candidate.dashCodecFamily,
            hardwareDecodeSupported: hardwareDecodeSupported,
            decoderName: hardwareDecodeSupported ? "VideoToolbox" : nil
        )
    }
}
