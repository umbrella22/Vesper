import Foundation
import XCTest
@testable import VesperPlayerKit

@MainActor
final class VesperSubtitleOverlayRendererTests: XCTestCase {
    func testExternalSubtitleRedirectStripsConfiguredHeadersAcrossOrigin() throws {
        let originalURL = try XCTUnwrap(URL(string: "https://media.example.test/subtitle.vtt"))
        var crossOriginRequest = URLRequest(
            url: try XCTUnwrap(URL(string: "https://cdn.example.test/subtitle.vtt"))
        )
        crossOriginRequest.setValue("subtitle-secret", forHTTPHeaderField: "Authorization")
        crossOriginRequest.setValue("session-value", forHTTPHeaderField: "X-Subtitle-Session")
        crossOriginRequest.setValue("preserved", forHTTPHeaderField: "Accept")

        let stripped = externalSubtitleRedirectRequest(
            originalURL: originalURL,
            headerNames: ["Authorization", "X-Subtitle-Session"],
            request: crossOriginRequest
        )

        XCTAssertNil(stripped.value(forHTTPHeaderField: "Authorization"))
        XCTAssertNil(stripped.value(forHTTPHeaderField: "X-Subtitle-Session"))
        XCTAssertEqual(stripped.value(forHTTPHeaderField: "Accept"), "preserved")

        var sameOriginRequest = URLRequest(
            url: try XCTUnwrap(URL(string: "https://media.example.test:443/redirected.vtt"))
        )
        sameOriginRequest.setValue("subtitle-secret", forHTTPHeaderField: "Authorization")
        let retained = externalSubtitleRedirectRequest(
            originalURL: originalURL,
            headerNames: ["Authorization"],
            request: sameOriginRequest
        )
        XCTAssertEqual(
            retained.value(forHTTPHeaderField: "Authorization"),
            "subtitle-secret"
        )
    }

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
            VesperExternalSubtitleSource(
                id: "srt",
                uri: url.absoluteString,
                mimeType: VesperExternalSubtitleSource.mimeSubrip
            )
        ])
        XCTAssertTrue(renderer.select(trackId: "srt"))

        renderer.render(positionMs: 1_500)
        XCTAssertEqual(renderer.renderedTextSnapshot, "Hello Vesper")
        renderer.render(positionMs: 2_750)
        XCTAssertEqual(renderer.renderedTextSnapshot, "")
        renderer.render(positionMs: 3_250)
        XCTAssertEqual(renderer.renderedTextSnapshot, "Second cue")
    }

    func testWebVttShortTimestampProducesCue() async throws {
        let url = try temporarySubtitle(
            extension: "vtt",
            contents: "WEBVTT\n\n00:01.000 --> 00:02.500\nShort timestamp cue\n"
        )
        defer { try? FileManager.default.removeItem(at: url) }
        let renderer = VesperSubtitleOverlayRenderer()

        try await renderer.configure([
            VesperExternalSubtitleSource(
                id: "webvtt-short",
                uri: url.absoluteString,
                mimeType: VesperExternalSubtitleSource.mimeWebVtt
            )
        ])
        XCTAssertTrue(renderer.select(trackId: "webvtt-short"))

        renderer.render(positionMs: 1_500)
        XCTAssertEqual(renderer.renderedTextSnapshot, "Short timestamp cue")
    }

    func testSurfaceSnapshotRequiresVisibleAttachedNonzeroSubtitleLabel() throws {
        let surface = PlayerSurfaceView(frame: CGRect(x: 0, y: 0, width: 320, height: 180))
        let window = UIWindow(frame: surface.bounds)
        let viewController = UIViewController()
        window.rootViewController = viewController
        viewController.view.addSubview(surface)
        window.makeKeyAndVisible()
        defer { window.isHidden = true }

        surface.updateSubtitleOverlay(text: "Subtitle B", style: .default)
        surface.layoutIfNeeded()

        let snapshot = surface.subtitleOverlaySnapshot
        XCTAssertEqual(snapshot.text, "Subtitle B")
        XCTAssertFalse(snapshot.hidden)
        XCTAssertGreaterThan(snapshot.alpha, 0)
        XCTAssertTrue(snapshot.windowAttached)
        XCTAssertGreaterThan(snapshot.frame.width, 0)
        XCTAssertGreaterThan(snapshot.frame.height, 0)
        XCTAssertTrue(snapshot.visible)

        let label = try XCTUnwrap(
            surface.subviews.compactMap { $0 as? UILabel }.first
        )
        XCTAssertEqual(
            label.accessibilityIdentifier,
            PlayerSurfaceView.subtitleOverlayAccessibilityIdentifier
        )

        surface.updateSubtitleOverlay(text: "", style: .default)
        surface.layoutIfNeeded()
        XCTAssertFalse(surface.subtitleOverlaySnapshot.visible)
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
            VesperExternalSubtitleSource(
                id: "ssa",
                uri: url.absoluteString,
                mimeType: VesperExternalSubtitleSource.mimeSsa
            )
        ])
        XCTAssertTrue(renderer.select(trackId: "ssa"))
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
                VesperExternalSubtitleSource(
                    id: "large",
                    uri: url.absoluteString,
                    mimeType: VesperExternalSubtitleSource.mimeSubrip
                )
            ])
            XCTFail("Expected the bounded subtitle reader to reject the file")
        } catch let error as VesperSubtitleError {
            XCTAssertEqual(error.code, "subtitle_resource_failed")
            XCTAssertEqual(error.phase, .resource)
            XCTAssertEqual(error.trackId, "large")
        }
    }

    func testPrepareDoesNotClearActiveTracksUntilInstall() async throws {
        let validURL = try temporarySubtitle(
            extension: "vtt",
            contents: "WEBVTT\n\n00:00:01.000 --> 00:00:02.000\nActive cue\n"
        )
        defer { try? FileManager.default.removeItem(at: validURL) }
        let renderer = VesperSubtitleOverlayRenderer()

        try await renderer.configure([
            VesperExternalSubtitleSource(
                id: "valid",
                uri: validURL.absoluteString,
                mimeType: VesperExternalSubtitleSource.mimeWebVtt
            )
        ])
        XCTAssertTrue(renderer.select(trackId: "valid"))
        renderer.render(positionMs: 1_500)
        XCTAssertEqual(renderer.renderedTextSnapshot, "Active cue")

        let prepared = try await renderer.prepare([
            VesperExternalSubtitleSource(
                id: "invalid",
                uri: "not a valid uri",
                mimeType: VesperExternalSubtitleSource.mimeWebVtt
            )
        ])
        XCTAssertEqual(prepared.failures.count, 1)
        // Preparing a replacement is side-effect free, so a stale source load
        // cannot clear the currently displayed track before it commits.
        renderer.render(positionMs: 1_500)
        XCTAssertEqual(renderer.renderedTextSnapshot, "Active cue")

        renderer.install(prepared)
        XCTAssertFalse(renderer.hasTracks)
        XCTAssertEqual(renderer.renderedTextSnapshot, "")
    }

    func testPrepareKeepsSuccessfulTracksWhenOneExternalTrackFails() async throws {
        let validURL = try temporarySubtitle(
            extension: "srt",
            contents: "1\n00:00:01,000 --> 00:00:02,000\nGood cue\n"
        )
        defer { try? FileManager.default.removeItem(at: validURL) }
        let renderer = VesperSubtitleOverlayRenderer()

        let prepared = try await renderer.prepare([
            VesperExternalSubtitleSource(
                id: "valid",
                uri: validURL.absoluteString,
                mimeType: VesperExternalSubtitleSource.mimeSubrip
            ),
            VesperExternalSubtitleSource(
                id: "invalid",
                uri: "not a valid uri",
                mimeType: VesperExternalSubtitleSource.mimeSubrip
            ),
        ])

        XCTAssertEqual(prepared.advertisedTrackCount, 2)
        XCTAssertEqual(prepared.cuesByTrackId.count, 1)
        XCTAssertEqual(prepared.failures.map(\.trackId), ["invalid"])
        renderer.install(prepared)
        XCTAssertTrue(renderer.select(trackId: "valid"))
        renderer.render(positionMs: 1_500)
        XCTAssertEqual(renderer.renderedTextSnapshot, "Good cue")
    }

    func testPrepareRejectsNonEmptySubtitleWithNoValidCues() async throws {
        let url = try temporarySubtitle(
            extension: "vtt",
            contents: "WEBVTT\n\nnot a cue\n"
        )
        let source = VesperExternalSubtitleSource(
            id: "malformed",
            uri: url.absoluteString,
            mimeType: VesperExternalSubtitleSource.mimeWebVtt
        )
        let renderer = VesperSubtitleOverlayRenderer()

        let prepared = try await renderer.prepare([source])

        XCTAssertTrue(prepared.cuesByTrackId.isEmpty)
        XCTAssertEqual(prepared.failures.map(\.error.code), ["subtitle_resource_failed"])
        XCTAssertEqual(prepared.failures.first?.trackId, "malformed")
    }

    func testExternalSubtitleIdsRemainStableAcrossReorderAndRemoval() async throws {
        let aURL = try temporarySubtitle(
            extension: "vtt",
            contents: "WEBVTT\n\n00:00:01.000 --> 00:00:02.000\nA\n"
        )
        let bURL = try temporarySubtitle(
            extension: "vtt",
            contents: "WEBVTT\n\n00:00:01.000 --> 00:00:02.000\nB\n"
        )
        defer {
            try? FileManager.default.removeItem(at: aURL)
            try? FileManager.default.removeItem(at: bURL)
        }
        let renderer = VesperSubtitleOverlayRenderer()
        let a = VesperExternalSubtitleSource(
            id: "external-a",
            uri: aURL.absoluteString,
            mimeType: VesperExternalSubtitleSource.mimeWebVtt
        )
        let b = VesperExternalSubtitleSource(
            id: "external-b",
            uri: bURL.absoluteString,
            mimeType: VesperExternalSubtitleSource.mimeWebVtt
        )

        renderer.install(try await renderer.prepare([a, b]))
        XCTAssertEqual(renderer.loadedTrackIds, ["external-a", "external-b"])

        renderer.install(try await renderer.prepare([b, a]))
        XCTAssertEqual(renderer.loadedTrackIds, ["external-b", "external-a"])
        XCTAssertTrue(renderer.select(trackId: "external-b"))

        renderer.install(try await renderer.prepare([b]))
        XCTAssertEqual(renderer.loadedTrackIds, ["external-b"])
        XCTAssertTrue(renderer.select(trackId: "external-b"))
        renderer.render(positionMs: 1_500)
        XCTAssertEqual(renderer.renderedTextSnapshot, "B")
    }

    private func temporarySubtitle(extension pathExtension: String, contents: String) throws -> URL {
        let url = FileManager.default.temporaryDirectory
            .appendingPathComponent(UUID().uuidString)
            .appendingPathExtension(pathExtension)
        try XCTUnwrap(contents.data(using: .utf8)).write(to: url)
        return url
    }
}
