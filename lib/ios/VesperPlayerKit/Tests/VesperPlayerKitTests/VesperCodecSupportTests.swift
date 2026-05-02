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
}
