import XCTest
@testable import VesperPlayerKit

final class VesperDashBridgeSessionTests: XCTestCase {
    func testSegmentTemplateRedirectWritesLocalMediaFileVerbatim() async throws {
        let directory = FileManager.default.temporaryDirectory
            .appendingPathComponent(UUID().uuidString, isDirectory: true)
        try FileManager.default.createDirectory(
            at: directory,
            withIntermediateDirectories: true
        )
        defer { try? FileManager.default.removeItem(at: directory) }

        let manifestURL = directory.appendingPathComponent("manifest.mpd")
        try Data(sampleSegmentTemplateMpd.utf8).write(to: manifestURL)

        var initData = mp4Box(type: "ftyp", payload: Data([0x01]))
        initData.append(mp4Box(type: "moov", payload: Data([0x02])))
        try initData.write(to: directory.appendingPathComponent("v1_257-Header.m4s"))

        var mediaData = mp4Box(type: "styp", payload: Data([0x03]))
        mediaData.append(mp4Box(type: "sidx", payload: Data([0x04])))
        mediaData.append(mp4Box(type: "moof", payload: Data([0x05])))
        try mediaData.write(to: directory.appendingPathComponent("v1_257-270146-i-1.m4s"))

        let session = makeTestDashSession(sourceURL: manifestURL)
        let initRedirectURL = try await session.segmentRedirectURL(
            renditionId: "v1_257",
            segment: .initialization
        )
        let mediaRedirectURL = try await session.segmentRedirectURL(
            renditionId: "v1_257",
            segment: .media(0)
        )

        XCTAssertTrue(initRedirectURL.isFileURL)
        XCTAssertTrue(mediaRedirectURL.isFileURL)
        XCTAssertEqual(try Data(contentsOf: initRedirectURL), initData)
        // Preserve the original fMP4 bytes, including sidx, so
        // tfhd.base_data_offset stays aligned.
        XCTAssertEqual(try Data(contentsOf: mediaRedirectURL), mediaData)
    }

    func testSegmentTemplateMediaPlaylistUsesResourceLoaderSegmentUrls() async throws {
        let directory = FileManager.default.temporaryDirectory
            .appendingPathComponent(UUID().uuidString, isDirectory: true)
        try FileManager.default.createDirectory(
            at: directory,
            withIntermediateDirectories: true
        )
        defer { try? FileManager.default.removeItem(at: directory) }

        let manifestURL = directory.appendingPathComponent("manifest.mpd")
        try Data(sampleSegmentTemplateMpd.utf8).write(to: manifestURL)

        var initData = mp4Box(type: "ftyp", payload: Data([0x01]))
        initData.append(mp4Box(type: "moov", payload: Data([0x02])))

        var mediaData = mp4Box(type: "styp", payload: Data([0x03]))
        mediaData.append(mp4Box(type: "sidx", payload: Data([0x04])))
        mediaData.append(mp4Box(type: "moof", payload: Data([0x05])))
        try writeSegmentTemplateFiles(
            directory: directory,
            renditionId: "v4_258",
            initData: initData,
            mediaData: mediaData
        )

        let session = makeTestDashSession(sourceURL: manifestURL)
        let data = try await session.mediaPlaylistData(renditionId: "v4_258")
        let playlist = String(decoding: data, as: UTF8.self)

        XCTAssertTrue(playlist.contains("#EXT-X-MAP:URI=\"vesper-dash://segment/"))
        XCTAssertTrue(playlist.contains("/v4_258/init.mp4\""))
        XCTAssertFalse(playlist.contains("http://127.0.0.1:"))
        XCTAssertTrue(playlist.contains("/v4_258/0.m4s"))
        XCTAssertFalse(playlist.contains("v4_258-270146-i-1.m4s"))
        XCTAssertFalse(playlist.contains("data:video/mp4;base64,"))

        let mediaURLText = try XCTUnwrap(
            firstMatch(#"vesper-dash://segment/[^"]+/v4_258/0\.m4s"#, in: playlist)
        )
        XCTAssertEqual(
            session.route(for: try XCTUnwrap(URL(string: mediaURLText))),
            .segment("v4_258", .media(0))
        )
        let loadedMediaData = try await session.segmentData(renditionId: "v4_258", segment: .media(0))

        // Resource loader segment delivery preserves the fMP4 bytes verbatim,
        // including sidx, instead of stripping sequential sidx boxes.
        XCTAssertEqual(loadedMediaData, mediaData)
    }

    @MainActor
    func testDashBenchmarkRecordsPlaylistAndSegmentRequests() async throws {
        let directory = FileManager.default.temporaryDirectory
            .appendingPathComponent(UUID().uuidString, isDirectory: true)
        try FileManager.default.createDirectory(
            at: directory,
            withIntermediateDirectories: true
        )
        defer { try? FileManager.default.removeItem(at: directory) }

        let manifestURL = directory.appendingPathComponent("manifest.mpd")
        try Data(sampleSegmentTemplateMpd.utf8).write(to: manifestURL)

        var initData = mp4Box(type: "ftyp", payload: Data([0x01]))
        initData.append(mp4Box(type: "moov", payload: Data([0x02])))

        var mediaData = mp4Box(type: "styp", payload: Data([0x03]))
        mediaData.append(mp4Box(type: "sidx", payload: Data([0x04])))
        mediaData.append(mp4Box(type: "moof", payload: Data([0x05])))
        try writeSegmentTemplateFiles(
            directory: directory,
            renditionId: "v4_258",
            initData: initData,
            mediaData: mediaData
        )

        var events: [(name: String, attributes: [String: String])] = []
        let session = VesperDashSession(
            sourceURL: manifestURL,
            videoDecodeCapabilityProvider: testHardwareVideoDecodeCapabilityProvider,
            benchmarkEventRecorder: { name, attributes in
                events.append((name, attributes))
            }
        )

        _ = try await session.masterPlaylistData()
        _ = try await session.mediaPlaylistData(renditionId: "v4_258")
        _ = try await session.segmentData(renditionId: "v4_258", segment: .initialization)
        _ = try await session.segmentData(renditionId: "v4_258", segment: .media(0))

        XCTAssertTrue(events.contains { $0.name == "dash_master_playlist_request_start" })
        XCTAssertEqual(
            eventAttributes("dash_master_playlist_request_end", in: events)?["cacheHit"],
            "false"
        )

        let mediaPlaylistEnd = try XCTUnwrap(
            eventAttributes("dash_media_playlist_request_end", in: events) {
                $0["renditionId"] == "v4_258"
            }
        )
        XCTAssertEqual(mediaPlaylistEnd["renditionId"], "v4_258")
        XCTAssertNotNil(mediaPlaylistEnd["cacheHit"])

        let initSegmentEnd = try XCTUnwrap(
            eventAttributes("dash_init_segment_request_end", in: events) {
                $0["renditionId"] == "v4_258"
                    && $0["requestOrigin"] == "resourceLoader"
            }
        )
        XCTAssertEqual(initSegmentEnd["renditionId"], "v4_258")
        XCTAssertEqual(initSegmentEnd["segmentKind"], "initialization")
        XCTAssertEqual(initSegmentEnd["bytes"], "\(initData.count)")
        XCTAssertEqual(initSegmentEnd["requestOrigin"], "resourceLoader")

        let mediaSegmentEnd = try XCTUnwrap(
            eventAttributes("dash_media_segment_request_end", in: events) {
                $0["renditionId"] == "v4_258"
                    && $0["requestOrigin"] == "resourceLoader"
            }
        )
        XCTAssertEqual(mediaSegmentEnd["renditionId"], "v4_258")
        XCTAssertEqual(mediaSegmentEnd["index"], "0")
        XCTAssertEqual(mediaSegmentEnd["bytes"], "\(mediaData.count)")
        XCTAssertEqual(mediaSegmentEnd["segmentType"], "template")
        XCTAssertNotNil(mediaSegmentEnd["cacheHit"])
    }

    func testConcurrentSegmentTemplateMediaPlaylistsUseResourceLoaderSegmentUrls() async throws {
        let directory = FileManager.default.temporaryDirectory
            .appendingPathComponent(UUID().uuidString, isDirectory: true)
        try FileManager.default.createDirectory(
            at: directory,
            withIntermediateDirectories: true
        )
        defer { try? FileManager.default.removeItem(at: directory) }

        let manifestURL = directory.appendingPathComponent("manifest.mpd")
        try Data(sampleSegmentTemplateMpd.utf8).write(to: manifestURL)

        var initData = mp4Box(type: "ftyp", payload: Data([0x01]))
        initData.append(mp4Box(type: "moov", payload: Data([0x02])))
        var mediaData = mp4Box(type: "styp", payload: Data([0x03]))
        mediaData.append(mp4Box(type: "sidx", payload: Data([0x04])))
        mediaData.append(mp4Box(type: "moof", payload: Data([0x05])))
        try writeSegmentTemplateFiles(
            directory: directory,
            renditionId: "v4_258",
            initData: initData,
            mediaData: mediaData
        )
        try writeSegmentTemplateFiles(
            directory: directory,
            renditionId: "v1_257",
            initData: initData,
            mediaData: mediaData
        )

        let session = makeTestDashSession(sourceURL: manifestURL)
        let renditionIds = [
            "v4_258",
            "v1_257",
            "v4_258",
            "v1_257",
            "v4_258",
            "v1_257",
        ]
        let playlists = try await withThrowingTaskGroup(of: String.self, returning: [String].self) { group in
            for renditionId in renditionIds {
                group.addTask {
                    String(
                        decoding: try await session.mediaPlaylistData(renditionId: renditionId),
                        as: UTF8.self
                    )
                }
            }

            var output: [String] = []
            for try await playlist in group {
                output.append(playlist)
            }
            return output
        }

        let sessionIds = Set(try playlists.map { try firstResourceLoaderSegmentSessionId(in: $0) })
        XCTAssertEqual(sessionIds, [session.id])
        XCTAssertTrue(playlists.allSatisfy { !$0.contains("http://127.0.0.1:") })
    }

    @MainActor
    func testConcurrentMediaPlaylistRequestsReuseInFlightManifestAndSidx() async throws {
        let manifestURL = URL(string: "https://origin.example.com/path/master.mpd")!
        let mediaURL = URL(string: "https://cdn.example.com/root/video/seg.m4s")!
        let indexRange = try VesperDashByteRange(start: 1_000, end: 1_199)
        let networkClient = CountingDashNetworkClient(
            dataByURL: [
                manifestURL: Data(sampleMpd.utf8),
                mediaURL: sampleSegmentBaseMediaData(),
            ],
            delayNanoseconds: 100_000_000
        )
        var events: [(name: String, attributes: [String: String])] = []
        let session = VesperDashSession(
            sourceURL: manifestURL,
            networkClient: networkClient,
            videoDecodeCapabilityProvider: testHardwareVideoDecodeCapabilityProvider,
            benchmarkEventRecorder: { name, attributes in
                events.append((name, attributes))
            }
        )

        async let first = session.mediaPlaylistData(renditionId: "v1")
        async let second = session.mediaPlaylistData(renditionId: "v1")
        _ = try await (first, second)

        XCTAssertEqual(networkClient.requestCount(for: manifestURL), 1)
        XCTAssertEqual(networkClient.requestCount(for: mediaURL, byteRange: indexRange), 1)
        XCTAssertTrue(
            events.contains {
                $0.name == "dash_media_playlist_request_end"
                    && $0.attributes["renditionId"] == "v1"
                    && $0.attributes["coalesced"] == "true"
            }
        )
    }

    func testDashSessionMasterPlaylistExposesAllVideoVariantsForAbr() async throws {
        let directory = FileManager.default.temporaryDirectory
            .appendingPathComponent(UUID().uuidString, isDirectory: true)
        try FileManager.default.createDirectory(
            at: directory,
            withIntermediateDirectories: true
        )
        defer { try? FileManager.default.removeItem(at: directory) }

        let manifestURL = directory.appendingPathComponent("manifest.mpd")
        try Data(sampleMultiVideoSegmentTemplateMpd.utf8).write(to: manifestURL)

        let session = makeTestDashSession(sourceURL: manifestURL)
        let playlist = String(
            decoding: try await session.masterPlaylistData(),
            as: UTF8.self
        )

        XCTAssertEqual(countOccurrences(of: "#EXT-X-STREAM-INF", in: playlist), 3)
        XCTAssertTrue(playlist.contains("vesper-dash://media/\(session.id)/v1_257.m3u8"))
        XCTAssertTrue(playlist.contains("vesper-dash://media/\(session.id)/v2_257.m3u8"))
        XCTAssertTrue(playlist.contains("vesper-dash://media/\(session.id)/v7_257.m3u8"))
        XCTAssertTrue(playlist.contains("vesper-dash://media/\(session.id)/v4_258.m3u8"))
    }

    func testSegmentTemplateCachePrunesOldMediaFiles() async throws {
        let directory = FileManager.default.temporaryDirectory
            .appendingPathComponent(UUID().uuidString, isDirectory: true)
        try FileManager.default.createDirectory(
            at: directory,
            withIntermediateDirectories: true
        )
        defer { try? FileManager.default.removeItem(at: directory) }

        let requestedMediaCount = VesperDashSession.segmentCacheMaxEntryCount + 12
        let manifest = sampleSegmentTemplateMpd.replacingOccurrences(
            of: #"mediaPresentationDuration="PT193.680S""#,
            with: #"mediaPresentationDuration="PT360S""#
        )
        let manifestURL = directory.appendingPathComponent("manifest.mpd")
        try Data(manifest.utf8).write(to: manifestURL)

        let mediaData = mp4Box(type: "styp", payload: Data([0x03, 0x04]))
        try writeSegmentTemplateFiles(
            directory: directory,
            renditionId: "v1_257",
            initData: mp4Box(type: "ftyp", payload: Data([0x01])),
            mediaData: mediaData,
            segmentCount: requestedMediaCount
        )

        let session = makeTestDashSession(sourceURL: manifestURL)
        for index in 0..<requestedMediaCount {
            _ = try await session.segmentRedirectURL(
                renditionId: "v1_257",
                segment: .media(index)
            )
        }

        let cachedMediaFiles = try FileManager.default.contentsOfDirectory(
            at: session.segmentCacheDirectory,
            includingPropertiesForKeys: nil
        )
        .filter { $0.pathExtension == "m4s" }

        XCTAssertLessThanOrEqual(
            cachedMediaFiles.count,
            VesperDashSession.segmentCacheMaxEntryCount
        )
    }

    func testLargeSegmentTemplateResourceLoaderUsesTemporaryFileAndSkipsCache() async throws {
        let directory = FileManager.default.temporaryDirectory
            .appendingPathComponent(UUID().uuidString, isDirectory: true)
        try FileManager.default.createDirectory(
            at: directory,
            withIntermediateDirectories: true
        )
        defer { try? FileManager.default.removeItem(at: directory) }

        let manifestURL = directory.appendingPathComponent("manifest.mpd")
        try Data(sampleSegmentTemplateMpd.utf8).write(to: manifestURL)

        let mediaURL = directory.appendingPathComponent("v1_257-270146-i-1.m4s")
        FileManager.default.createFile(atPath: mediaURL.path, contents: nil)
        let handle = try FileHandle(forWritingTo: mediaURL)
        try handle.truncate(atOffset: VesperDashSession.segmentCacheMaxSingleMediaBytes + 4_096)
        try handle.seek(toOffset: 0)
        handle.write(Data((0..<16).map(UInt8.init)))
        try handle.close()

        let session = makeTestDashSession(sourceURL: manifestURL)
        let playlist = String(
            decoding: try await session.mediaPlaylistData(renditionId: "v1_257"),
            as: UTF8.self
        )
        let mediaURLText = try XCTUnwrap(
            firstMatch(#"vesper-dash://segment/[^"]+/v1_257/0\.m4s"#, in: playlist)
        )
        XCTAssertEqual(
            session.route(for: try XCTUnwrap(URL(string: mediaURLText))),
            .segment("v1_257", .media(0))
        )
        let payload = try await session.segmentResourcePayload(renditionId: "v1_257", segment: .media(0))
        guard case let .file(url, offset, size, removeAfterServing, _) = payload else {
            XCTFail("large media segment should be delivered from a temporary file")
            return
        }
        XCTAssertTrue(removeAfterServing)
        XCTAssertEqual(size, VesperDashSession.segmentCacheMaxSingleMediaBytes + 4_096)
        let readHandle = try FileHandle(forReadingFrom: url)
        defer { try? readHandle.close() }
        try readHandle.seek(toOffset: offset)
        XCTAssertEqual(try readHandle.read(upToCount: 16), Data((0..<16).map(UInt8.init)))
        payload.cleanupIfTemporary()

        let cachedFiles = try FileManager.default.contentsOfDirectory(
            at: session.segmentCacheDirectory,
            includingPropertiesForKeys: nil
        )
        XCTAssertTrue(cachedFiles.filter { $0.pathExtension == "m4s" }.isEmpty)
        XCTAssertTrue(cachedFiles.filter { $0.lastPathComponent.hasPrefix("tmp-") }.isEmpty)
    }

    func testSegmentBaseMediaPlaylistUsesSessionCache() async throws {
        let directory = FileManager.default.temporaryDirectory
            .appendingPathComponent(UUID().uuidString, isDirectory: true)
        try FileManager.default.createDirectory(
            at: directory,
            withIntermediateDirectories: true
        )
        defer { try? FileManager.default.removeItem(at: directory) }

        let mediaURL = directory.appendingPathComponent("media.m4s")
        var mediaData = Data([0x01, 0x02, 0x03, 0x04])
        mediaData.append(mp4Box(type: "sidx", payload: sidxPayloadV0()))
        try mediaData.write(to: mediaURL)

        let manifestURL = directory.appendingPathComponent("manifest.mpd")
        let manifest = #"""
        <?xml version="1.0"?>
        <MPD type="static" mediaPresentationDuration="PT12S">
          <Period id="p0">
            <AdaptationSet id="v" contentType="video" mimeType="video/mp4">
              <Representation id="v1" bandwidth="800000" codecs="avc1.64001f" width="1280" height="720">
                <BaseURL>media.m4s</BaseURL>
                <SegmentBase indexRange="4-59">
                  <Initialization range="0-3"/>
                </SegmentBase>
              </Representation>
            </AdaptationSet>
          </Period>
        </MPD>
        """#
        try Data(manifest.utf8).write(to: manifestURL)

        let session = makeTestDashSession(sourceURL: manifestURL)
        let firstPlaylist = try await session.mediaPlaylistData(renditionId: "v1")

        try FileManager.default.removeItem(at: mediaURL)
        let secondPlaylist = try await session.mediaPlaylistData(renditionId: "v1")

        XCTAssertEqual(secondPlaylist, firstPlaylist)
    }

    func testDashSessionRoutesMasterAndMediaUrls() {
        let session = VesperDashSession(sourceURL: URL(string: "https://example.com/master.mpd")!)

        XCTAssertEqual(session.route(for: session.masterPlaylistURL), .master)
        XCTAssertEqual(session.route(for: session.mediaPlaylistURL(for: "video/main")), .media("video/main"))
        XCTAssertEqual(
            session.route(for: session.segmentURL(for: "video/main", segment: .initialization)),
            .segment("video/main", .initialization)
        )
        XCTAssertEqual(
            session.route(for: session.segmentURL(for: "video/main", segment: .media(12))),
            .segment("video/main", .media(12))
        )
        XCTAssertNil(session.route(for: URL(string: "https://example.com/master.mpd")!))
    }
}
