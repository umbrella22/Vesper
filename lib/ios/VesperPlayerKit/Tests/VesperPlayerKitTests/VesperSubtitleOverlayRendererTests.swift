import Foundation
import XCTest
@testable import VesperPlayerKit

@MainActor
final class VesperSubtitleOverlayRendererTests: XCTestCase {
    func testSubRipLoadSelectionAndTimelineRendering() async throws {
        let url = try temporarySubtitle(
            extension: "srt",
            contents: """
            1
            00:00:01,000 --> 00:00:02,500
            Hello <b>Vesper</b>

            2
            00:00:03,000 --> 00:00:04,000
            Second cue
            """
        )
        defer { try? FileManager.default.removeItem(at: url) }
        let renderer = VesperSubtitleOverlayRenderer()

        try await renderer.configure([
            VesperSubtitleSideLoad(uri: url.absoluteString, mimeType: .subrip)
        ])
        XCTAssertTrue(renderer.select(trackId: VesperSubtitleOverlayRenderer.trackId(for: 0)))

        renderer.render(positionMs: 1_500)
        XCTAssertEqual(renderer.renderedTextSnapshot, "Hello Vesper")
        renderer.render(positionMs: 2_750)
        XCTAssertEqual(renderer.renderedTextSnapshot, "")
        renderer.render(positionMs: 3_250)
        XCTAssertEqual(renderer.renderedTextSnapshot, "Second cue")
    }

    func testSsaDialogueIsParsedAndOverrideTagsAreRemoved() async throws {
        let url = try temporarySubtitle(
            extension: "ass",
            contents: """
            [Events]
            Format: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text
            Dialogue: 0,0:00:01.00,0:00:03.00,Default,,0,0,0,,{\\an8}Line one\\NLine two
            """
        )
        defer { try? FileManager.default.removeItem(at: url) }
        let renderer = VesperSubtitleOverlayRenderer()

        try await renderer.configure([
            VesperSubtitleSideLoad(uri: url.absoluteString, mimeType: .ssa)
        ])
        XCTAssertTrue(renderer.select(trackId: VesperSubtitleOverlayRenderer.trackId(for: 0)))
        renderer.render(positionMs: 2_000)

        XCTAssertEqual(renderer.renderedTextSnapshot, "Line one\nLine two")
    }

    func testOversizedSubtitleIsRejected() async throws {
        let url = FileManager.default.temporaryDirectory
            .appendingPathComponent(UUID().uuidString)
            .appendingPathExtension("srt")
        try Data(repeating: 0x41, count: VesperSubtitleOverlayRenderer.maximumSubtitleBytes + 1)
            .write(to: url)
        defer { try? FileManager.default.removeItem(at: url) }
        let renderer = VesperSubtitleOverlayRenderer()

        do {
            try await renderer.configure([
                VesperSubtitleSideLoad(uri: url.absoluteString, mimeType: .subrip)
            ])
            XCTFail("Expected the bounded subtitle reader to reject the file")
        } catch let error as VesperPlayerError {
            XCTAssertEqual(error.details["reason"], "subtitleFileTooLarge")
        }
    }

    private func temporarySubtitle(extension pathExtension: String, contents: String) throws -> URL {
        let url = FileManager.default.temporaryDirectory
            .appendingPathComponent(UUID().uuidString)
            .appendingPathExtension(pathExtension)
        try XCTUnwrap(contents.data(using: .utf8)).write(to: url)
        return url
    }
}
