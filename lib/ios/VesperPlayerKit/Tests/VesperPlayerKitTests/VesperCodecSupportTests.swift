import CoreMedia
import XCTest

@testable import VesperPlayerKit

final class VesperCodecSupportTests: XCTestCase {
    func testCodecNameNormalizationRecognizesCommonH264Aliases() {
        XCTAssertEqual(VesperHardwareDecodeCandidateCodec(codecName: "H264"), .h264)
        XCTAssertEqual(VesperHardwareDecodeCandidateCodec(codecName: "avc"), .h264)
        XCTAssertEqual(VesperHardwareDecodeCandidateCodec(codecName: "avc1"), .h264)
        XCTAssertEqual(VesperHardwareDecodeCandidateCodec(codecName: "avc1.4D401E"), .h264)
    }

    func testCodecNameNormalizationRecognizesCommonHevcAliases() {
        XCTAssertEqual(VesperHardwareDecodeCandidateCodec(codecName: "HEVC"), .hevc)
        XCTAssertEqual(VesperHardwareDecodeCandidateCodec(codecName: "h265"), .hevc)
        XCTAssertEqual(VesperHardwareDecodeCandidateCodec(codecName: "hvc1"), .hevc)
        XCTAssertEqual(VesperHardwareDecodeCandidateCodec(codecName: "hev1"), .hevc)
        XCTAssertEqual(VesperHardwareDecodeCandidateCodec(codecName: "hvc1.1.6.L93.B0"), .hevc)
        XCTAssertEqual(VesperHardwareDecodeCandidateCodec(codecName: "dvh1.05.06"), .hevc)
        XCTAssertEqual(VesperHardwareDecodeCandidateCodec(codecName: "dvhe.08.07"), .hevc)
    }

    func testCodecNameNormalizationRecognizesModernCodecAliases() {
        XCTAssertEqual(VesperHardwareDecodeCandidateCodec(codecName: "av01.0.05M.08"), .av1)
        XCTAssertEqual(VesperHardwareDecodeCandidateCodec(codecName: "video/av01"), .av1)
        XCTAssertEqual(VesperHardwareDecodeCandidateCodec(codecName: "vvc1.1.L123"), .vvc)
        XCTAssertEqual(VesperHardwareDecodeCandidateCodec(codecName: "h266"), .vvc)
    }

    func testUnknownCodecReturnsNoHardwareSupport() {
        XCTAssertEqual(VesperHardwareDecodeCandidateCodec(codecName: "vp9"), .unknown)
        XCTAssertFalse(VesperCodecSupport.hardwareDecodeSupported(for: "vp9"))
    }

    func testPlaybackCapabilityProbeReportsRemoteNativeFrameAsNetworkFallback() {
        let result = VesperPlaybackCapabilityProbe.probe(
            VesperPlaybackCapabilityProbeRequest(
                source: .hls(url: URL(string: "https://example.com/live.m3u8")!),
                codec: "hvc1.1.6.L93.B0",
                requiresNativeFrame: true,
                nativeFramePipelineConfiguration: VesperNativeFramePipelineConfiguration(
                    decoderPluginLibraryPaths: ["/tmp/libdecoder_videotoolbox.dylib"]
                )
            )
        )

        XCTAssertEqual(result.codecFamily, .hevc)
        XCTAssertEqual(result.status, .fallbackRequired)
        XCTAssertTrue(result.missingCapabilities.contains("hostManagedNetworkProbeNotImplemented"))
    }

    func testPlaybackCapabilityProbeRequiresSourceNormalizerMetadataForHdrNativeFrame() {
        let result = VesperPlaybackCapabilityProbe.probe(
            VesperPlaybackCapabilityProbeRequest(
                source: .localFile(
                    url: URL(fileURLWithPath: "/tmp/local-hdr.mov"),
                    label: "local-hdr.mov"
                ),
                codec: "dvh1.05.06",
                nativeFramePipelineConfiguration: VesperNativeFramePipelineConfiguration(
                    mode: .requireNativeFrame,
                    decoderPluginLibraryPaths: ["/tmp/libdecoder_videotoolbox.dylib"]
                )
            )
        )

        XCTAssertEqual(result.codecFamily, .hevc)
        XCTAssertEqual(result.status, .fallbackRequired)
        XCTAssertTrue(result.missingCapabilities.contains("hdrProgrammableProcessingNotSupported"))
        XCTAssertEqual(result.recommendedPlaybackPath, .systemPlayer)
        XCTAssertEqual(result.hdrKind, .dolbyVision)
        XCTAssertEqual(result.outputFormat, .surfaceOpaque)
        XCTAssertEqual(result.diagnostics["playbackPathPolicy"], "hdrSystemPlaybackOnly")
        XCTAssertEqual(
            result.diagnostics["recommendedPlaybackPathReason"], "hdrNativeFrameUnsupported")
    }

    func testPlaybackCapabilityProbeRoutesDolbyVisionPreferNativeFrameToSystemPlayback() {
        let result = VesperPlaybackCapabilityProbe.probe(
            VesperPlaybackCapabilityProbeRequest(
                source: .localFile(
                    url: URL(fileURLWithPath: "/tmp/local-dv.mov"),
                    label: "local-dv.mov"
                ),
                codec: "dvh1.05.06",
                sourceNormalizerConfiguration: VesperSourceNormalizerConfiguration(
                    mode: .preferNormalized,
                    pluginLibraryPaths: ["/tmp/libnormalizer.dylib"]
                ),
                nativeFramePipelineConfiguration: VesperNativeFramePipelineConfiguration(
                    mode: .preferNativeFrame,
                    decoderPluginLibraryPaths: ["/tmp/libdecoder_videotoolbox.dylib"]
                )
            )
        )

        XCTAssertEqual(result.status, .fallbackRequired)
        XCTAssertEqual(result.hdrKind, .dolbyVision)
        XCTAssertEqual(result.dolbyVisionMode, .unsupported)
        XCTAssertEqual(result.diagnostics["dolbyVisionProfile"], "5")
        XCTAssertEqual(result.diagnostics["dolbyVisionLevel"], "6")
        XCTAssertEqual(result.diagnostics["dolbyVisionCompatibility"], "noCompatibleBaseLayer")
        XCTAssertEqual(result.recommendedPlaybackPath, .systemPlayer)
        XCTAssertTrue(result.missingCapabilities.contains("hdrProgrammableProcessingNotSupported"))
        XCTAssertEqual(result.diagnostics["playbackPathPolicy"], "hdrSystemPlaybackOnly")
        XCTAssertEqual(
            result.diagnostics["recommendedPlaybackPathReason"], "hdrNativeFrameUnsupported")
    }

    func testPlaybackCapabilityProbeReportsDolbyVisionProfile8CompatibleBaseLayerCandidate() {
        let result = VesperPlaybackCapabilityProbe.probe(
            VesperPlaybackCapabilityProbeRequest(
                source: .localFile(
                    url: URL(fileURLWithPath: "/tmp/local-dv-profile8.mov"),
                    label: "local-dv-profile8.mov"
                ),
                codec: "dvhe.08.07",
                nativeFramePipelineConfiguration: VesperNativeFramePipelineConfiguration(
                    mode: .preferNativeFrame,
                    decoderPluginLibraryPaths: ["/tmp/libdecoder_videotoolbox.dylib"]
                )
            )
        )

        XCTAssertEqual(result.hdrKind, .dolbyVision)
        XCTAssertEqual(result.dolbyVisionMode, .compatibleBaseLayer)
        XCTAssertEqual(result.diagnostics["dolbyVisionProfile"], "8")
        XCTAssertEqual(result.diagnostics["dolbyVisionLevel"], "7")
        XCTAssertEqual(
            result.diagnostics["dolbyVisionCompatibility"],
            "compatibleBaseLayerCandidate"
        )
        XCTAssertEqual(result.diagnostics["dolbyVisionProfileFamily"], "profile8SingleLayerCompatible")
        XCTAssertEqual(result.diagnostics["dolbyVisionBaseLayer"], "compatibleBaseLayerUnknown")
        XCTAssertEqual(result.diagnostics["dolbyVisionFallbackTarget"], "compatibleBaseLayerSystemPlayer")
        XCTAssertEqual(result.recommendedPlaybackPath, .systemPlayer)
        XCTAssertEqual(result.hdrMetadata?.hdrKind, .dolbyVision)
        XCTAssertEqual(result.hdrMetadata?.dolbyVisionMode, .compatibleBaseLayer)
        XCTAssertEqual(result.hdrMetadata?.dolbyVisionCodec, "dvhe.08.07")
        XCTAssertEqual(result.hdrMetadata?.dolbyVisionProfile, 8)
        XCTAssertEqual(result.hdrMetadata?.dolbyVisionLevel, 7)
        XCTAssertEqual(result.hdrMetadata?.dolbyVisionCompatibility, "compatibleBaseLayerCandidate")
        XCTAssertEqual(result.hdrMetadata?.dolbyVisionProfileFamily, "profile8SingleLayerCompatible")
        XCTAssertEqual(result.hdrMetadata?.dolbyVisionBaseLayer, "compatibleBaseLayerUnknown")
        XCTAssertEqual(result.hdrMetadata?.dolbyVisionFallbackTarget, "compatibleBaseLayerSystemPlayer")

        let wireHdrMetadata = result.wireMap["hdrMetadata"] as? [String: Any]
        XCTAssertEqual(wireHdrMetadata?["hdrKind"] as? String, "dolbyVision")
        XCTAssertEqual(wireHdrMetadata?["dolbyVisionMode"] as? String, "compatibleBaseLayer")
        XCTAssertEqual(wireHdrMetadata?["dolbyVisionCodec"] as? String, "dvhe.08.07")
        XCTAssertEqual(wireHdrMetadata?["dolbyVisionProfile"] as? Int, 8)
        XCTAssertEqual(wireHdrMetadata?["dolbyVisionLevel"] as? Int, 7)
        XCTAssertEqual(
            wireHdrMetadata?["dolbyVisionCompatibility"] as? String,
            "compatibleBaseLayerCandidate"
        )
    }

    func testAssetProbeMergePreservesDolbyVisionKindAndRefinesProfile8BaseLayer() {
        let baseResult = VesperPlaybackCapabilityProbe.probe(
            VesperPlaybackCapabilityProbeRequest(
                source: .localFile(
                    url: URL(fileURLWithPath: "/tmp/local-dv-profile8.mov"),
                    label: "local-dv-profile8.mov"
                ),
                codec: "dvhe.08.07",
                nativeFramePipelineConfiguration: VesperNativeFramePipelineConfiguration(
                    mode: .preferNativeFrame,
                    decoderPluginLibraryPaths: ["/tmp/libdecoder_videotoolbox.dylib"]
                )
            )
        )

        XCTAssertEqual(baseResult.dolbyVisionMode, .compatibleBaseLayer)

        let result = VesperPlaybackCapabilityProbe.withAssetProbeResult(
            baseResult,
            assetProbeResult: VesperPlaybackCapabilityAssetProbeResult(
                isPlayable: true,
                videoTrackCount: 1,
                metadataHdrKind: .hdr10,
                diagnostics: [
                    "assetProbe": "iosAVAsset",
                    "assetVideoTransferFunction": "SMPTE_ST_2084_PQ",
                ]
            )
        )

        XCTAssertEqual(result.hdrKind, .dolbyVision)
        XCTAssertEqual(result.dolbyVisionMode, .compatibleBaseLayer)
        XCTAssertEqual(result.diagnostics["dolbyVisionCompatibility"], "profile8Hdr10BaseLayer")
        XCTAssertEqual(result.diagnostics["dolbyVisionBaseLayer"], "hdr10BaseLayer")
        XCTAssertEqual(result.diagnostics["dolbyVisionFallbackTarget"], "hdr10BaseLayerSystemPlayer")
        XCTAssertEqual(result.diagnostics["dolbyVisionBaseLayerEvidence"], "assetVideoTransferFunction")
        XCTAssertEqual(result.diagnostics["dolbyVisionBaseLayerTransferFunction"], "SMPTE_ST_2084_PQ")
        XCTAssertEqual(result.hdrMetadata?.dolbyVisionCompatibility, "profile8Hdr10BaseLayer")
        XCTAssertEqual(result.hdrMetadata?.dolbyVisionBaseLayer, "hdr10BaseLayer")
        XCTAssertEqual(result.hdrMetadata?.dolbyVisionFallbackTarget, "hdr10BaseLayerSystemPlayer")
        XCTAssertEqual(result.hdrMetadata?.dolbyVisionBaseLayerEvidence, "assetVideoTransferFunction")
        XCTAssertEqual(result.hdrMetadata?.dolbyVisionBaseLayerTransferFunction, "SMPTE_ST_2084_PQ")
    }

    func testDolbyVisionProfile8AssetPqMetadataRefinesBaseLayer() {
        let baseResult = VesperPlaybackCapabilityProbe.probe(
            VesperPlaybackCapabilityProbeRequest(
                source: .remoteUrl(
                    URL(string: "https://example.com/dv-profile8.mov")!,
                    protocol: .progressive
                ),
                codec: "dvhe.08.07"
            )
        )

        XCTAssertEqual(baseResult.diagnostics["dolbyVisionCompatibility"], "compatibleBaseLayerCandidate")
        XCTAssertEqual(baseResult.diagnostics["dolbyVisionBaseLayer"], "compatibleBaseLayerUnknown")

        let result = VesperPlaybackCapabilityProbe.withAssetProbeResult(
            baseResult,
            assetProbeResult: VesperPlaybackCapabilityAssetProbeResult(
                isPlayable: true,
                videoTrackCount: 1,
                metadataHdrKind: .dolbyVision,
                diagnostics: [
                    "assetProbe": "iosAVAsset",
                    "assetVideoTransferFunction": "SMPTE_ST_2084_PQ",
                ]
            )
        )

        XCTAssertEqual(result.hdrKind, .dolbyVision)
        XCTAssertEqual(result.confidence, .sourceMetadata)
        XCTAssertEqual(result.diagnostics["dolbyVisionCompatibility"], "profile8Hdr10BaseLayer")
        XCTAssertEqual(result.diagnostics["dolbyVisionBaseLayer"], "hdr10BaseLayer")
        XCTAssertEqual(result.diagnostics["dolbyVisionFallbackTarget"], "hdr10BaseLayerSystemPlayer")
        XCTAssertEqual(result.diagnostics["dolbyVisionBaseLayerEvidence"], "assetVideoTransferFunction")
        XCTAssertEqual(result.diagnostics["dolbyVisionBaseLayerTransferFunction"], "SMPTE_ST_2084_PQ")
        XCTAssertEqual(result.hdrMetadata?.dolbyVisionCompatibility, "profile8Hdr10BaseLayer")
        XCTAssertEqual(result.hdrMetadata?.dolbyVisionBaseLayer, "hdr10BaseLayer")
        XCTAssertEqual(result.hdrMetadata?.dolbyVisionFallbackTarget, "hdr10BaseLayerSystemPlayer")
        XCTAssertEqual(result.hdrMetadata?.dolbyVisionBaseLayerEvidence, "assetVideoTransferFunction")
        XCTAssertEqual(result.hdrMetadata?.dolbyVisionBaseLayerTransferFunction, "SMPTE_ST_2084_PQ")
    }

    func testDolbyVisionProfile8AssetHlgMetadataRefinesBaseLayer() {
        let baseResult = VesperPlaybackCapabilityProbe.probe(
            VesperPlaybackCapabilityProbeRequest(
                source: .remoteUrl(
                    URL(string: "https://example.com/dv-profile8-hlg.mov")!,
                    protocol: .hls
                ),
                codec: "dvhe.08.07"
            )
        )

        let result = VesperPlaybackCapabilityProbe.withAssetProbeResult(
            baseResult,
            assetProbeResult: VesperPlaybackCapabilityAssetProbeResult(
                isPlayable: true,
                videoTrackCount: 1,
                metadataHdrKind: .dolbyVision,
                diagnostics: [
                    "assetProbe": "iosAVAsset",
                    "assetVideoAlternativeTransferCharacteristics": "ARIB_STD_B67_HLG",
                ]
            )
        )

        XCTAssertEqual(result.diagnostics["dolbyVisionCompatibility"], "profile8HlgBaseLayer")
        XCTAssertEqual(result.diagnostics["dolbyVisionBaseLayer"], "hlgBaseLayer")
        XCTAssertEqual(result.diagnostics["dolbyVisionFallbackTarget"], "hlgBaseLayerSystemPlayer")
        XCTAssertEqual(
            result.diagnostics["dolbyVisionBaseLayerEvidence"],
            "assetVideoAlternativeTransferCharacteristics"
        )
        XCTAssertEqual(
            result.diagnostics["dolbyVisionBaseLayerTransferFunction"],
            "ARIB_STD_B67_HLG"
        )
        XCTAssertEqual(result.hdrMetadata?.dolbyVisionCompatibility, "profile8HlgBaseLayer")
        XCTAssertEqual(result.hdrMetadata?.dolbyVisionBaseLayer, "hlgBaseLayer")
        XCTAssertEqual(
            result.hdrMetadata?.dolbyVisionBaseLayerEvidence,
            "assetVideoAlternativeTransferCharacteristics"
        )
        XCTAssertEqual(
            result.hdrMetadata?.dolbyVisionBaseLayerTransferFunction,
            "ARIB_STD_B67_HLG"
        )
    }

    func testDolbyVisionCodecInfoParsesProfileMatrixConservatively() {
        let profile5 = VesperPlaybackCapabilityProbe.detectDolbyVisionCodecInfo("dvh1.05.06")
        XCTAssertEqual(profile5?.profile, 5)
        XCTAssertEqual(profile5?.dolbyVisionMode, .unsupported)
        XCTAssertEqual(profile5?.diagnostics["dolbyVisionCompatibility"], "noCompatibleBaseLayer")
        XCTAssertEqual(profile5?.diagnostics["dolbyVisionProfileFamily"], "profile5SingleLayer")
        XCTAssertEqual(profile5?.diagnostics["dolbyVisionBaseLayer"], "none")

        let profile7 = VesperPlaybackCapabilityProbe.detectDolbyVisionCodecInfo("dvhe.07.06")
        XCTAssertEqual(profile7?.profile, 7)
        XCTAssertEqual(profile7?.dolbyVisionMode, .compatibleBaseLayer)
        XCTAssertEqual(profile7?.diagnostics["dolbyVisionCompatibility"], "dualLayerBaseLayerCandidate")
        XCTAssertEqual(profile7?.diagnostics["dolbyVisionProfileFamily"], "profile7DualLayer")
        XCTAssertEqual(profile7?.diagnostics["dolbyVisionBaseLayer"], "hdr10BaseLayerCandidate")

        let profile8 = VesperPlaybackCapabilityProbe.detectDolbyVisionCodecInfo(
            "video/dvhe.08.07,mp4a.40.2"
        )
        XCTAssertEqual(profile8?.profile, 8)
        XCTAssertEqual(profile8?.level, 7)
        XCTAssertEqual(profile8?.dolbyVisionMode, .compatibleBaseLayer)
        XCTAssertEqual(profile8?.diagnostics["dolbyVisionProfileFamily"], "profile8SingleLayerCompatible")

        let profile9 = VesperPlaybackCapabilityProbe.detectDolbyVisionCodecInfo("dvh1.09.01")
        XCTAssertEqual(profile9?.profile, 9)
        XCTAssertEqual(profile9?.dolbyVisionMode, .unsupported)
        XCTAssertEqual(profile9?.diagnostics["dolbyVisionCompatibility"], "unknownProfile")
        XCTAssertEqual(profile9?.diagnostics["dolbyVisionProfileFamily"], "profile9ConservativeUnknown")
    }

    func testPlaybackCapabilityProbeCanUseSessionProbeFeedback() {
        let result = VesperPlaybackCapabilityProbe.probe(
            VesperPlaybackCapabilityProbeRequest(
                source: .localFile(
                    url: URL(fileURLWithPath: "/tmp/local-dv.mov"),
                    label: "local-dv.mov"
                ),
                codec: "dvh1.05.06",
                nativeFramePipelineConfiguration: VesperNativeFramePipelineConfiguration(
                    mode: .preferNativeFrame,
                    decoderPluginLibraryPaths: ["/tmp/libdecoder_videotoolbox.dylib"]
                )
            ),
            sessionProbeProvider: { _ in
                VesperPlaybackCapabilitySessionProbeResult(
                    supportedHdrKinds: [.dolbyVision],
                    diagnostics: ["sessionProbe": "fakeDisplay"]
                )
            }
        )

        XCTAssertEqual(result.confidence, .sessionProbe)
        XCTAssertEqual(result.diagnostics["sessionProbe"], "fakeDisplay")
        XCTAssertFalse(result.missingCapabilities.contains("displayHdrCapability"))
    }

    func testPlaybackCapabilityProbeReportsMissingDisplayHdrCapability() {
        let result = VesperPlaybackCapabilityProbe.probe(
            VesperPlaybackCapabilityProbeRequest(
                source: .localFile(
                    url: URL(fileURLWithPath: "/tmp/local-dv.mov"),
                    label: "local-dv.mov"
                ),
                codec: "dvh1.05.06",
                nativeFramePipelineConfiguration: VesperNativeFramePipelineConfiguration(
                    mode: .preferNativeFrame,
                    decoderPluginLibraryPaths: ["/tmp/libdecoder_videotoolbox.dylib"]
                )
            ),
            sessionProbeProvider: { _ in
                VesperPlaybackCapabilitySessionProbeResult(
                    supportedHdrKinds: [.hdr10],
                    diagnostics: ["sessionProbe": "fakeDisplay"]
                )
            }
        )

        XCTAssertEqual(result.confidence, .sessionProbe)
        XCTAssertTrue(result.missingCapabilities.contains("displayHdrCapability"))
        XCTAssertEqual(result.diagnostics["displayHdrSupported"], "false")
    }

    func testIOSSessionProbeUsesHdrEligibilityForRequestedHdrKind() {
        let result = VesperPlaybackCapabilityProbe.probe(
            VesperPlaybackCapabilityProbeRequest(
                source: .localFile(
                    url: URL(fileURLWithPath: "/tmp/local-dv.mov"),
                    label: "local-dv.mov"
                ),
                codec: "dvh1.05.06",
                width: 3840,
                height: 2160,
                frameRate: 60,
                nativeFramePipelineConfiguration: VesperNativeFramePipelineConfiguration(
                    mode: .preferNativeFrame,
                    decoderPluginLibraryPaths: ["/tmp/libdecoder_videotoolbox.dylib"]
                )
            ),
            sessionProbeProvider: { request in
                VesperIOSSessionProbeProvider.probe(
                    request,
                    environment: VesperIOSSessionProbeEnvironment(
                        displayGamut: .p3,
                        hdrPlaybackEligible: true,
                        maximumFramesPerSecond: 120,
                        nativeWidth: 2796,
                        nativeHeight: 1290
                    )
                )
            }
        )

        XCTAssertEqual(result.confidence, .sessionProbe)
        XCTAssertFalse(result.missingCapabilities.contains("displayHdrCapability"))
        XCTAssertFalse(result.missingCapabilities.contains("displayFrameRate"))
        XCTAssertEqual(result.diagnostics["sessionProbe"], "iosDisplayAndPlayerHdrEligibility")
        XCTAssertEqual(result.diagnostics["avPlayerEligibleForHDRPlayback"], "true")
        XCTAssertEqual(result.diagnostics["displayFrameRateSupported"], "true")
        XCTAssertEqual(result.diagnostics["requestedWidth"], "3840")
        XCTAssertEqual(result.diagnostics["requestedHeight"], "2160")
    }

    func testIOSSessionProbeReportsHdrAndFrameRateGaps() {
        let result = VesperPlaybackCapabilityProbe.probe(
            VesperPlaybackCapabilityProbeRequest(
                source: .localFile(
                    url: URL(fileURLWithPath: "/tmp/local-hdr.mov"),
                    label: "local-hdr.mov"
                ),
                codec: "hdr10",
                width: 3840,
                height: 2160,
                frameRate: 120,
                nativeFramePipelineConfiguration: VesperNativeFramePipelineConfiguration(
                    mode: .preferNativeFrame,
                    decoderPluginLibraryPaths: ["/tmp/libdecoder_videotoolbox.dylib"]
                )
            ),
            sessionProbeProvider: { request in
                VesperIOSSessionProbeProvider.probe(
                    request,
                    environment: VesperIOSSessionProbeEnvironment(
                        displayGamut: .srgb,
                        hdrPlaybackEligible: false,
                        maximumFramesPerSecond: 60,
                        nativeWidth: 1334,
                        nativeHeight: 750
                    )
                )
            }
        )

        XCTAssertEqual(result.confidence, .sessionProbe)
        XCTAssertTrue(result.missingCapabilities.contains("displayHdrCapability"))
        XCTAssertTrue(result.missingCapabilities.contains("displayFrameRate"))
        XCTAssertEqual(result.diagnostics["displayHdrSupported"], "false")
        XCTAssertEqual(result.diagnostics["displayFrameRateSupported"], "false")
        XCTAssertEqual(result.diagnostics["displayMaximumFramesPerSecond"], "60")
        XCTAssertEqual(result.diagnostics["displayGamut"], "srgb")
    }

    func testAssetProbeResultCanMarkAssetUnplayableWithoutChangingWireShape() {
        let baseResult = VesperPlaybackCapabilityProbe.probe(
            VesperPlaybackCapabilityProbeRequest(
                source: .localFile(
                    url: URL(fileURLWithPath: "/tmp/local-hdr.mov"),
                    label: "local-hdr.mov"
                ),
                codec: "hdr10"
            )
        )

        let result = VesperPlaybackCapabilityProbe.withAssetProbeResult(
            baseResult,
            assetProbeResult: VesperPlaybackCapabilityAssetProbeResult(
                isPlayable: false,
                videoTrackCount: 1,
                diagnostics: [
                    "assetProbe": "iosAVAsset",
                    "assetPlayable": "false",
                    "assetVideoTrackCount": "1",
                ]
            )
        )

        XCTAssertEqual(result.status, .unsupported)
        XCTAssertFalse(result.systemPlaybackSupported)
        XCTAssertTrue(result.missingCapabilities.contains("assetPlayable"))
        XCTAssertEqual(result.diagnostics["assetProbe"], "iosAVAsset")
        XCTAssertNil(result.wireMap["requiresHdrNativeFrame"])
        XCTAssertNil(result.wireMap["hdrNativeFrameSupported"])
    }

    func testAssetMetadataCanPromoteHevcProbeToHdr10Fallback() {
        let baseResult = VesperPlaybackCapabilityProbe.probe(
            VesperPlaybackCapabilityProbeRequest(
                source: .localFile(
                    url: URL(fileURLWithPath: "/tmp/local-hevc-hdr.mov"),
                    label: "local-hevc-hdr.mov"
                ),
                codec: "hvc1.1.6.L93.B0"
            )
        )

        XCTAssertEqual(baseResult.hdrKind, .none)

        let result = VesperPlaybackCapabilityProbe.withAssetProbeResult(
            baseResult,
            assetProbeResult: VesperPlaybackCapabilityAssetProbeResult(
                isPlayable: true,
                videoTrackCount: 1,
                metadataHdrKind: .hdr10,
                diagnostics: [
                    "assetProbe": "iosAVAsset",
                    "assetVideoTransferFunction": "SMPTE_ST_2084_PQ",
                ]
            )
        )

        XCTAssertEqual(result.status, .fallbackRequired)
        XCTAssertEqual(result.hdrKind, .hdr10)
        XCTAssertEqual(result.recommendedPlaybackPath, .systemPlayer)
        XCTAssertEqual(result.outputFormat, .surfaceOpaque)
        XCTAssertTrue(result.missingCapabilities.contains("hdrProgrammableProcessingNotSupported"))
        XCTAssertEqual(result.diagnostics["hdrKindSource"], "assetMetadata")
        XCTAssertEqual(result.diagnostics["assetVideoMetadataHdrKind"], "hdr10")
        XCTAssertEqual(result.hdrMetadata?.hdrKind, .hdr10)
        XCTAssertEqual(result.hdrMetadata?.probe, "iosAVAsset")
        XCTAssertEqual(result.hdrMetadata?.transferFunction, "SMPTE_ST_2084_PQ")

        let wireHdrMetadata = result.wireMap["hdrMetadata"] as? [String: Any]
        XCTAssertEqual(wireHdrMetadata?["hdrKind"] as? String, "hdr10")
        XCTAssertEqual(wireHdrMetadata?["probe"] as? String, "iosAVAsset")
        XCTAssertEqual(wireHdrMetadata?["transferFunction"] as? String, "SMPTE_ST_2084_PQ")
    }

    func testAssetMetadataPromotesRemoteCodecOnlyConfidenceToSourceMetadata() {
        let baseResult = VesperPlaybackCapabilityProbe.probe(
            VesperPlaybackCapabilityProbeRequest(
                source: .remoteUrl(
                    URL(string: "https://example.com/hdr.mov")!,
                    protocol: .progressive
                ),
                codec: "hvc1.1.6.L93.B0"
            )
        )

        XCTAssertEqual(baseResult.confidence, VesperPlaybackCapabilityConfidence.codecOnly)

        let result = VesperPlaybackCapabilityProbe.withAssetProbeResult(
            baseResult,
            assetProbeResult: VesperPlaybackCapabilityAssetProbeResult(
                isPlayable: true,
                videoTrackCount: 1,
                metadataHdrKind: .hdr10,
                diagnostics: [
                    "assetProbe": "iosAVAsset",
                    "assetVideoTransferFunction": "SMPTE_ST_2084_PQ",
                ]
            )
        )

        XCTAssertEqual(result.hdrKind, VesperPlaybackCapabilityHdrKind.hdr10)
        XCTAssertEqual(result.confidence, VesperPlaybackCapabilityConfidence.sourceMetadata)
        XCTAssertEqual(result.diagnostics["hdrKindSource"], "assetMetadata")
    }

    func testAssetMetadataDoesNotDowngradeSessionProbeConfidence() {
        let baseResult = VesperPlaybackCapabilityProbe.probe(
            VesperPlaybackCapabilityProbeRequest(
                source: .remoteUrl(
                    URL(string: "https://example.com/hdr.mov")!,
                    protocol: .progressive
                ),
                codec: "dvh1.05.06"
            ),
            sessionProbeProvider: { _ in
                VesperPlaybackCapabilitySessionProbeResult(
                    supportedHdrKinds: [.dolbyVision],
                    diagnostics: ["sessionProbe": "fakeDisplay"]
                )
            }
        )

        XCTAssertEqual(baseResult.confidence, VesperPlaybackCapabilityConfidence.sessionProbe)

        let result = VesperPlaybackCapabilityProbe.withAssetProbeResult(
            baseResult,
            assetProbeResult: VesperPlaybackCapabilityAssetProbeResult(
                isPlayable: true,
                videoTrackCount: 1,
                metadataHdrKind: .hdr10,
                diagnostics: [
                    "assetProbe": "iosAVAsset",
                    "assetVideoTransferFunction": "SMPTE_ST_2084_PQ",
                ]
            )
        )

        XCTAssertEqual(result.hdrKind, VesperPlaybackCapabilityHdrKind.dolbyVision)
        XCTAssertEqual(result.confidence, VesperPlaybackCapabilityConfidence.sessionProbe)
        XCTAssertEqual(result.diagnostics["sessionProbe"], "fakeDisplay")
    }

    func testHdrMetadataModelParsesDiagnosticsIntoTypedFields() {
        let metadata = VesperPlaybackCapabilityProbe.buildHdrMetadata(
            hdrKind: .hdr10,
            dolbyVisionMode: .none,
            diagnostics: [
                "runtimeFormatHdrMetadataProbe": "media3FormatColorInfo",
                "runtimeFormatSampleMimeType": "video/hevc",
                "runtimeFormatColorSpace": "bt2020",
                "runtimeFormatColorRange": "limited",
                "runtimeFormatColorTransfer": "st2084",
                "runtimeFormatLumaBitDepth": "10",
                "runtimeFormatChromaBitDepth": "10",
                "runtimeFormatHdrStaticInfoPresent": "true",
                "runtimeFormatHdrStaticInfoByteLength": "25",
                "runtimeFormatMaxContentLightLevelNits": "1000",
                "runtimeFormatMaxFrameAverageLightLevelNits": "400",
                "assetVideoMasteringDisplayPrimary0": "0.38970,0.17204",
                "assetVideoMasteringDisplayMaxLuminanceNits": "1000.0",
                "assetVideoMasteringDisplayMinLuminanceNits": "0.0001",
            ]
        )

        XCTAssertEqual(metadata?.hdrKind, .hdr10)
        XCTAssertEqual(metadata?.probe, "media3FormatColorInfo")
        XCTAssertEqual(metadata?.sampleMimeType, "video/hevc")
        XCTAssertEqual(metadata?.colorSpace, "bt2020")
        XCTAssertEqual(metadata?.colorRange, "limited")
        XCTAssertEqual(metadata?.transferFunction, "st2084")
        XCTAssertEqual(metadata?.lumaBitDepth, 10)
        XCTAssertEqual(metadata?.chromaBitDepth, 10)
        XCTAssertEqual(metadata?.hdrStaticInfoPresent, true)
        XCTAssertEqual(metadata?.hdrStaticInfoByteLength, 25)
        XCTAssertEqual(metadata?.maxContentLightLevelNits, 1000)
        XCTAssertEqual(metadata?.maxFrameAverageLightLevelNits, 400)
        XCTAssertEqual(metadata?.masteringDisplayPrimary0?.x ?? 0, 0.38970, accuracy: 0.00001)
        XCTAssertEqual(metadata?.masteringDisplayPrimary0?.y ?? 0, 0.17204, accuracy: 0.00001)
        XCTAssertEqual(metadata?.masteringDisplayMaxLuminanceNits ?? 0, 1000.0, accuracy: 0.00001)
        XCTAssertEqual(metadata?.masteringDisplayMinLuminanceNits ?? 0, 0.0001, accuracy: 0.00001)
    }

    func testDetectMetadataHdrKindRecognizesHlgTransferFunction() {
        let hdrKind = VesperPlaybackCapabilityProbe.detectMetadataHdrKind([
            "assetVideoTransferFunction": "ARIB_STD_B67_HLG"
        ])

        XCTAssertEqual(hdrKind, .hlg)
    }

    func testDetectMetadataHdrKindRecognizesDolbyVisionCodec() {
        let hdrKind = VesperPlaybackCapabilityProbe.detectMetadataHdrKind([
            "assetVideoCodec": "dvh1"
        ])

        XCTAssertEqual(hdrKind, .dolbyVision)
    }

    func testDetectMetadataHdrKindDoesNotTreatGenericSmpteTransferAsHdr10() {
        let hdrKind = VesperPlaybackCapabilityProbe.detectMetadataHdrKind([
            "assetVideoTransferFunction": "SMPTE_240M_1995"
        ])

        XCTAssertNil(hdrKind)
    }

    func testHdrStaticMetadataDiagnosticsExposeMasteringAndLightLevelInfo() {
        let masteringDisplayColorVolume = Data([
            0x4C, 0x1D,
            0x21, 0x9A,
            0x00, 0x00,
            0x21, 0x9A,
            0x21, 0x9A,
            0x00, 0x00,
            0x27, 0x10,
            0x27, 0x10,
            0x00, 0x0F, 0x42, 0x40,
            0x00, 0x00, 0x00, 0x01,
        ])
        let contentLightLevelInfo = Data([
            0x03, 0xE8,
            0x01, 0x90,
        ])
        let diagnostics = VesperIOSHdrStaticMetadataDiagnostics.diagnostics(from: [
            kCMFormatDescriptionExtension_AlternativeTransferCharacteristics as String:
                kCMFormatDescriptionTransferFunction_ITU_R_2100_HLG,
            kCMFormatDescriptionExtension_MasteringDisplayColorVolume as String:
                masteringDisplayColorVolume,
            kCMFormatDescriptionExtension_ContentLightLevelInfo as String:
                contentLightLevelInfo,
        ])

        XCTAssertEqual(
            diagnostics["assetVideoAlternativeTransferCharacteristics"],
            String(describing: kCMFormatDescriptionTransferFunction_ITU_R_2100_HLG)
        )
        XCTAssertEqual(diagnostics["assetVideoMasteringDisplayColorVolumePresent"], "true")
        XCTAssertEqual(diagnostics["assetVideoMasteringDisplayColorVolumeByteLength"], "24")
        XCTAssertEqual(diagnostics["assetVideoMasteringDisplayPrimary0"], "0.38970,0.17204")
        XCTAssertEqual(diagnostics["assetVideoMasteringDisplayPrimary1"], "0.00000,0.17204")
        XCTAssertEqual(diagnostics["assetVideoMasteringDisplayPrimary2"], "0.17204,0.00000")
        XCTAssertEqual(diagnostics["assetVideoMasteringDisplayWhitePoint"], "0.20000,0.20000")
        XCTAssertEqual(diagnostics["assetVideoMasteringDisplayMaxLuminanceNits"], "1000000")
        XCTAssertEqual(diagnostics["assetVideoMasteringDisplayMinLuminanceNits"], "0.0001")
        XCTAssertEqual(diagnostics["assetVideoContentLightLevelInfoPresent"], "true")
        XCTAssertEqual(diagnostics["assetVideoContentLightLevelInfoByteLength"], "4")
        XCTAssertEqual(diagnostics["assetVideoMaxContentLightLevelNits"], "1000")
        XCTAssertEqual(diagnostics["assetVideoMaxFrameAverageLightLevelNits"], "400")
    }

    func testHdrStaticMetadataDiagnosticsReportShortPayloads() {
        let diagnostics = VesperIOSHdrStaticMetadataDiagnostics.diagnostics(from: [
            kCMFormatDescriptionExtension_MasteringDisplayColorVolume as String:
                Data([0x00, 0x01]),
            kCMFormatDescriptionExtension_ContentLightLevelInfo as String:
                Data([0x00, 0x01]),
        ])

        XCTAssertEqual(diagnostics["assetVideoMasteringDisplayColorVolumePresent"], "true")
        XCTAssertEqual(diagnostics["assetVideoMasteringDisplayColorVolumeByteLength"], "2")
        XCTAssertEqual(diagnostics["assetVideoMasteringDisplayColorVolumeParseError"], "tooShort")
        XCTAssertEqual(diagnostics["assetVideoContentLightLevelInfoPresent"], "true")
        XCTAssertEqual(diagnostics["assetVideoContentLightLevelInfoByteLength"], "2")
        XCTAssertEqual(diagnostics["assetVideoContentLightLevelInfoParseError"], "tooShort")
    }

    func testAssetProbeReportsLoadFailureDiagnostics() async {
        let result = await VesperIOSAssetProbeProvider.probe(
            VesperPlaybackCapabilityProbeRequest(
                source: VesperPlayerSource(
                    uri: " ",
                    label: "invalid",
                    kind: .local,
                    protocol: .file
                ),
                codec: "hdr10"
            )
        )

        XCTAssertEqual(result?.diagnostics["assetProbe"], "iosAVAsset")
        XCTAssertNotNil(result?.diagnostics["assetProbeError"])
    }
}
