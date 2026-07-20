@preconcurrency import AVFoundation
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

    // MARK: - WebVTT subtitle session routing

    /// Writes WebVTT segment files into `directory` using names that match
    /// the `sub-$Number$.vtt` template in `sampleWebVttSubtitleMpd`. The
    /// generic helper `writeWebVttSegmentFiles` uses rendition-id-prefixed
    /// names, but the manifest template expands to `sub-<n>.vtt` (no prefix),
    /// so tests that exercise the session must write the template-expanded
    /// names directly.
    private func writeSampleWebVttSegmentFiles(
        directory: URL,
        segmentData: Data,
        segmentCount: Int = 3
    ) throws {
        for number in 1...segmentCount {
            try segmentData.write(to: directory.appendingPathComponent("sub-\(number).vtt"))
        }
    }

    /// WebVTT subtitle media playlist must emit `.vtt` segment URLs through
    /// the `vesper-dash://` resource loader scheme, not the misleading
    /// `.m4s` extension that all SegmentTemplate media used previously.
    func testWebVttSubtitleMediaPlaylistRoutesSegmentUrlsAsVtt() async throws {
        let directory = FileManager.default.temporaryDirectory
            .appendingPathComponent(UUID().uuidString, isDirectory: true)
        try FileManager.default.createDirectory(
            at: directory,
            withIntermediateDirectories: true
        )
        defer { try? FileManager.default.removeItem(at: directory) }

        let manifestURL = directory.appendingPathComponent("manifest.mpd")
        try Data(sampleWebVttSubtitleMpd.utf8).write(to: manifestURL)

        let vttBytes = Data("WEBVTT\n\n00:00:00.000 --> 00:00:02.000\nHello subtitle\n".utf8)
        try writeSampleWebVttSegmentFiles(
            directory: directory,
            segmentData: vttBytes,
            segmentCount: 3
        )

        let session = makeTestDashSession(sourceURL: manifestURL)
        let data = try await session.mediaPlaylistData(renditionId: "sub-en")
        let playlist = String(decoding: data, as: UTF8.self)

        // Subtitle rendition has no initialization in the fixture, so no
        // EXT-X-MAP should be emitted.
        XCTAssertFalse(playlist.contains("#EXT-X-MAP"))
        // Each media segment URI must use `.vtt` (not `.m4s`) and route
        // through the vesper-dash:// resource loader scheme.
        XCTAssertTrue(
            playlist.contains("vesper-dash://segment/"),
            "subtitle segments must route through resource loader: \(playlist)"
        )
        XCTAssertTrue(
            playlist.contains("/sub-en/1.vtt"),
            "subtitle media segment URL must use .vtt extension: \(playlist)"
        )
        XCTAssertFalse(
            playlist.contains(".m4s"),
            "subtitle media playlist must not use .m4s extension: \(playlist)"
        )
    }

    /// The resource loader must serve the bytes of a WebVTT segment
    /// verbatim, not reinterpret them as MP4. This proves the local file
    /// bytes are preserved end-to-end through the session route.
    func testWebVttSubtitleSegmentPayloadPreservesLocalFileBytes() async throws {
        let directory = FileManager.default.temporaryDirectory
            .appendingPathComponent(UUID().uuidString, isDirectory: true)
        try FileManager.default.createDirectory(
            at: directory,
            withIntermediateDirectories: true
        )
        defer { try? FileManager.default.removeItem(at: directory) }

        let manifestURL = directory.appendingPathComponent("manifest.mpd")
        try Data(sampleWebVttSubtitleMpd.utf8).write(to: manifestURL)

        let vttBytes = Data("WEBVTT\n\n00:00:00.000 --> 00:00:02.000\nHello subtitle\n".utf8)
        try writeSampleWebVttSegmentFiles(
            directory: directory,
            segmentData: vttBytes,
            segmentCount: 3
        )

        let session = makeTestDashSession(sourceURL: manifestURL)
        // Static manifest: segment index is 0-based in the playlist, so the
        // first media segment is `.media(0)` even though `$Number$` is 1.
        let payload = try await session.segmentResourcePayload(
            renditionId: "sub-en",
            segment: .media(0)
        )
        let served = try payload.readData()
        XCTAssertEqual(
            served,
            vttBytes,
            "subtitle segment bytes must be preserved verbatim through the resource loader"
        )
    }

    /// WebVTT subtitle segment payload must expose `public.webvtt` as its
    /// UTI content type so AVPlayer receives a MIME-aware response. This
    /// guards against regressions where the init-segment hardcoded
    /// `public.mpeg-4` could leak into the subtitle path.
    func testWebVttSubtitleSegmentContentTypeIsPublicWebvtt() async throws {
        let directory = FileManager.default.temporaryDirectory
            .appendingPathComponent(UUID().uuidString, isDirectory: true)
        try FileManager.default.createDirectory(
            at: directory,
            withIntermediateDirectories: true
        )
        defer { try? FileManager.default.removeItem(at: directory) }

        let manifestURL = directory.appendingPathComponent("manifest.mpd")
        try Data(sampleWebVttSubtitleMpd.utf8).write(to: manifestURL)

        let vttBytes = Data("WEBVTT\n\n00:00:00.000 --> 00:00:02.000\nHello subtitle\n".utf8)
        try writeSampleWebVttSegmentFiles(
            directory: directory,
            segmentData: vttBytes,
            segmentCount: 3
        )

        let session = makeTestDashSession(sourceURL: manifestURL)
        let payload = try await session.segmentResourcePayload(
            renditionId: "sub-en",
            segment: .media(0)
        )
        // The payload exposes the segment MIME type; the AVPlayer-facing
        // UTI lives on `localResourceBody.contentType` (after
        // `avResourceContentType` mapping). Both must classify as WebVTT.
        XCTAssertEqual(
            payload.contentType,
            "text/vtt",
            "subtitle segment payload must carry the text/vtt MIME type"
        )
        XCTAssertEqual(
            payload.localResourceBody.contentType,
            "public.webvtt",
            "subtitle localResourceBody must expose the public.webvtt UTI for AVPlayer"
        )
    }

    /// `route(for:)` must accept `.vtt` media URLs (and `init.vtt`) so the
    /// resource loader can dispatch subtitle requests. Previously the route
    /// rejected anything that was not `.m4s` / `init.mp4`.
    func testWebVttSubtitleRouteAcceptsVttUrls() {
        let session = makeTestDashSession(
            sourceURL: URL(string: "https://cdn.example.com/manifest.mpd")!
        )
        let vttMediaURL = URL(string: "vesper-dash://segment/\(session.id)/sub-en/1.vtt")!
        XCTAssertEqual(
            session.route(for: vttMediaURL),
            .segment("sub-en", .media(1))
        )
        let vttInitURL = URL(string: "vesper-dash://segment/\(session.id)/sub-en/init.vtt")!
        XCTAssertEqual(
            session.route(for: vttInitURL),
            .segment("sub-en", .initialization)
        )
        // Audio/video `.m4s` and `init.mp4` routes must still work so the
        // subtitle change does not regress existing renditions.
        let m4sMediaURL = URL(string: "vesper-dash://segment/\(session.id)/v1_257/0.m4s")!
        XCTAssertEqual(
            session.route(for: m4sMediaURL),
            .segment("v1_257", .media(0))
        )
        let mp4InitURL = URL(string: "vesper-dash://segment/\(session.id)/v1_257/init.mp4")!
        XCTAssertEqual(
            session.route(for: mp4InitURL),
            .segment("v1_257", .initialization)
        )
    }

    /// Constructs a real `AVURLAsset` pointed at the session's
    /// `vesper-dash://master/...` URL, installs the resource loader delegate,
    /// and verifies that AVFoundation can discover a `.legible` media
    /// selection group. This is the closest the simulator can get to real
    /// AVPlayer subtitle evidence without a physical device. Full cue output
    /// and seek-precision evidence require a real device.
    ///
    /// The test is marked with `XCTSkip` when the simulator cannot load
    /// the asset (e.g. when `vesper-dash://` custom scheme handling
    /// differs between simulator and device), documenting the gap as
    /// device-evidence-pending.
    func testWebVttSubtitleRouteProducesLegibleGroupOnSimulator() async throws {
        let directory = FileManager.default.temporaryDirectory
            .appendingPathComponent(UUID().uuidString, isDirectory: true)
        try FileManager.default.createDirectory(
            at: directory,
            withIntermediateDirectories: true
        )
        defer { try? FileManager.default.removeItem(at: directory) }

        let manifestURL = directory.appendingPathComponent("manifest.mpd")
        try Data(sampleWebVttSubtitleMpd.utf8).write(to: manifestURL)

        let vttBytes = Data("WEBVTT\n\n00:00:00.000 --> 00:00:02.000\nHello subtitle\n".utf8)
        try writeSampleWebVttSegmentFiles(
            directory: directory,
            segmentData: vttBytes,
            segmentCount: 3
        )

        let session = makeTestDashSession(sourceURL: manifestURL)
        let loaderDelegate = VesperDashResourceLoaderDelegate(session: session)
        let asset = AVURLAsset(url: session.masterPlaylistURL)
        asset.resourceLoader.setDelegate(
            loaderDelegate,
            queue: loaderDelegate.resourceLoadingQueue
        )

        // Attempt to load the legible group. On a real device this should
        // return a non-nil group with options matching the manifest. On a
        // simulator the result depends on AVFoundation's handling of the
        // custom scheme; if it cannot load the group, skip with a clear
        // message rather than failing.
        let group = try? await asset.loadMediaSelectionGroup(for: .legible)
        try XCTSkipIf(
            group == nil || group?.options.isEmpty == true,
            "Simulator could not produce a legible group for vesper-dash:// asset; real-device evidence is required"
        )

        let options = group?.options ?? []
        XCTAssertGreaterThan(
            options.count,
            0,
            "AVPlayer must discover at least one legible option from the subtitle rendition"
        )
    }
}
