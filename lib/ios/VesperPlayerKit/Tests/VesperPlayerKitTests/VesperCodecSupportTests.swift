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
        XCTAssertEqual(result.recommendedPlaybackPath, .systemPlayer)
        XCTAssertTrue(result.missingCapabilities.contains("hdrProgrammableProcessingNotSupported"))
        XCTAssertEqual(result.diagnostics["playbackPathPolicy"], "hdrSystemPlaybackOnly")
        XCTAssertEqual(
            result.diagnostics["recommendedPlaybackPathReason"], "hdrNativeFrameUnsupported")
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
}
