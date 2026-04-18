import XCTest
@testable import VesperPlayerKit

final class VesperCodecSupportTests: XCTestCase {
    func testCodecNameNormalizationRecognizesCommonH264Aliases() {
        XCTAssertEqual(VesperHardwareDecodeCandidateCodec(codecName: "H264"), .h264)
        XCTAssertEqual(VesperHardwareDecodeCandidateCodec(codecName: "avc"), .h264)
        XCTAssertEqual(VesperHardwareDecodeCandidateCodec(codecName: "avc1"), .h264)
    }

    func testCodecNameNormalizationRecognizesCommonHevcAliases() {
        XCTAssertEqual(VesperHardwareDecodeCandidateCodec(codecName: "HEVC"), .hevc)
        XCTAssertEqual(VesperHardwareDecodeCandidateCodec(codecName: "h265"), .hevc)
        XCTAssertEqual(VesperHardwareDecodeCandidateCodec(codecName: "hvc1"), .hevc)
        XCTAssertEqual(VesperHardwareDecodeCandidateCodec(codecName: "hev1"), .hevc)
    }

    func testUnknownCodecReturnsNoHardwareSupport() {
        XCTAssertEqual(VesperHardwareDecodeCandidateCodec(codecName: "vp9"), .unknown)
        XCTAssertFalse(VesperCodecSupport.hardwareDecodeSupported(for: "vp9"))
    }
}
