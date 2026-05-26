import AVFoundation
import XCTest
@testable import VesperPlayerKit

final class VesperSourceNormalizerResourceLoaderTests: XCTestCase {
    func testSessionBuildsCustomPlaybackUrlAndMapsPrimaryResource() throws {
        let directory = try makeTemporaryDirectory()
        defer { try? FileManager.default.removeItem(at: directory) }
        let primary = directory.appendingPathComponent("normalized.mp4")
        try Data("media".utf8).write(to: primary)

        let session = try VesperSourceNormalizerResourceSession(
            resource: makeResource(route: "fmp4LocalStream", primary: primary)
        )

        XCTAssertEqual(session.playbackURL.scheme, "vesper-normalized")
        XCTAssertEqual(session.localURL(for: session.playbackURL), primary)
    }

    func testSessionMapsHlsSegmentsWithinSessionDirectory() throws {
        let directory = try makeTemporaryDirectory()
        defer { try? FileManager.default.removeItem(at: directory) }
        let playlist = directory.appendingPathComponent("index.m3u8")
        let segment = directory.appendingPathComponent("segment_00001.m4s")
        try Data("#EXTM3U\n#EXTINF:3,\nsegment_00001.m4s\n".utf8).write(to: playlist)
        try Data("segment".utf8).write(to: segment)

        let session = try VesperSourceNormalizerResourceSession(
            resource: makeResource(route: "hlsShortWindow", primary: playlist)
        )
        let segmentURL = try XCTUnwrap(
            URL(string: session.playbackURL.absoluteString.replacingOccurrences(
                of: "index.m3u8",
                with: "segment_00001.m4s"
            ))
        )

        XCTAssertEqual(session.localURL(for: segmentURL), segment)
        XCTAssertNil(URL(string: "vesper-normalized://session/\(session.id)/../outside.m4s").flatMap(session.localURL))
    }

    func testContentTypeUsesPlaylistAndFmp4Utis() throws {
        let directory = try makeTemporaryDirectory()
        defer { try? FileManager.default.removeItem(at: directory) }
        let playlist = directory.appendingPathComponent("index.m3u8")
        let segment = directory.appendingPathComponent("segment_00001.m4s")
        try Data().write(to: playlist)
        try Data().write(to: segment)
        let session = try VesperSourceNormalizerResourceSession(
            resource: makeResource(route: "hlsShortWindow", primary: playlist)
        )

        XCTAssertEqual(session.contentType(for: playlist), "public.m3u-playlist")
        XCTAssertEqual(session.contentType(for: segment), "public.mpeg-4")
    }

    func testSessionReadPolicyKeepsFourMiBProfileBuffer() throws {
        let directory = try makeTemporaryDirectory()
        defer { try? FileManager.default.removeItem(at: directory) }
        let primary = directory.appendingPathComponent("normalized.mp4")
        try Data("media".utf8).write(to: primary)
        let session = try VesperSourceNormalizerResourceSession(
            resource: makeResource(
                route: "fmp4LocalStream",
                primary: primary,
                sessionReadBufferBytes: 4 * 1024 * 1024
            )
        )

        XCTAssertEqual(session.readPolicy.bufferBytes, 4 * 1024 * 1024)
    }

    func testSessionReadPolicyCapsOversizedProfileBufferAtFourMiB() throws {
        let directory = try makeTemporaryDirectory()
        defer { try? FileManager.default.removeItem(at: directory) }
        let primary = directory.appendingPathComponent("normalized.mp4")
        try Data("media".utf8).write(to: primary)
        let session = try VesperSourceNormalizerResourceSession(
            resource: makeResource(
                route: "fmp4LocalStream",
                primary: primary,
                sessionReadBufferBytes: 16 * 1024 * 1024
            )
        )

        XCTAssertEqual(session.readPolicy.bufferBytes, 4 * 1024 * 1024)
    }

    private func makeTemporaryDirectory() throws -> URL {
        let directory = FileManager.default.temporaryDirectory
            .appendingPathComponent(UUID().uuidString, isDirectory: true)
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        return directory
    }

    private func makeResource(
        route: String,
        primary: URL,
        sessionReadBufferBytes: Int = 4096
    ) -> VesperSourceNormalizerResourceOpenResult {
        VesperSourceNormalizerResourceOpenResult(
            handle: 42,
            outputRoute: route,
            selectedProfile: "test-profile",
            container: route == "hlsShortWindow" ? "hls" : "fmp4",
            primaryResourcePath: primary.path,
            primaryContentType: route == "hlsShortWindow"
                ? "application/vnd.apple.mpegurl"
                : "video/mp4",
            playbackUri: nil,
            resources: [],
            cachePolicy: ["sessionReadBufferBytes": sessionReadBufferBytes],
            diagnostics: []
        )
    }
}
