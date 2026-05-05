import XCTest
@testable import VesperPlayerKit

final class VesperDashBridgeTests: XCTestCase {
    func testDashSourcePreservesRequestHeaders() {
        let source = VesperPlayerSource.dash(
            url: URL(string: "https://example.com/master.mpd")!,
            label: "DASH",
            headers: [
                "Referer": "https://www.bilibili.com/",
                "User-Agent": "VesperTest",
            ]
        )

        XCTAssertEqual(source.headers["Referer"], "https://www.bilibili.com/")
        XCTAssertEqual(source.headers["User-Agent"], "VesperTest")
    }

    func testManifestParserReadsStaticSegmentBaseVod() throws {
        let manifest = try VesperDashManifestParser.parse(
            data: Data(sampleMpd.utf8),
            manifestURL: URL(string: "https://origin.example.com/path/master.mpd")!
        )

        XCTAssertEqual(manifest.type, .static)
        XCTAssertEqual(manifest.durationMs, 90_500)
        XCTAssertEqual(manifest.minBufferTimeMs, 1_500)
        XCTAssertEqual(manifest.periods.count, 1)
        let video = manifest.periods[0].adaptationSets[0]
        XCTAssertEqual(video.kind, .video)
        XCTAssertEqual(video.representations[0].baseURL, "https://cdn.example.com/root/video/seg.m4s")
        XCTAssertEqual(video.representations[0].segmentBase?.initialization, try VesperDashByteRange(start: 0, end: 999))
        XCTAssertEqual(video.representations[0].segmentBase?.indexRange, try VesperDashByteRange(start: 1_000, end: 1_199))

        let audio = manifest.periods[0].adaptationSets[1]
        XCTAssertEqual(audio.kind, .audio)
        XCTAssertEqual(audio.language, "ja")
        XCTAssertEqual(audio.representations[0].baseURL, "https://cdn.example.com/audio/main.m4s")
    }

    func testManifestParserReadsStaticSegmentTemplateVod() throws {
        let manifest = try VesperDashManifestParser.parse(
            data: Data(sampleSegmentTemplateMpd.utf8),
            manifestURL: URL(string: "https://dash.akamaized.net/envivio/EnvivioDash3/manifest.mpd")!
        )

        XCTAssertEqual(manifest.durationMs, 193_680)
        let video = manifest.periods[0].adaptationSets[0]
        XCTAssertEqual(video.kind, .video)
        XCTAssertEqual(video.representations[0].baseURL, "https://dash.akamaized.net/envivio/EnvivioDash3/manifest.mpd")
        XCTAssertEqual(
            video.representations[0].segmentTemplate,
            VesperDashSegmentTemplate(
                timescale: 90_000,
                duration: 179_704,
                startNumber: 1,
                presentationTimeOffset: 0,
                initialization: "$RepresentationID$-Header.m4s",
                media: "$RepresentationID$-270146-i-$Number$.m4s",
                timeline: []
            )
        )
        XCTAssertNil(video.representations[0].segmentBase)

        let audio = manifest.periods[0].adaptationSets[1]
        XCTAssertEqual(audio.kind, .audio)
        XCTAssertEqual(audio.representations[0].segmentTemplate?.media, "$RepresentationID$-270146-i-$Number$.m4s")
    }

    func testManifestParserReadsDynamicMpdTiming() throws {
        let manifest = try VesperDashManifestParser.parse(
            data: Data(
                #"<MPD type="dynamic" minimumUpdatePeriod="PT2S" timeShiftBufferDepth="PT30S"><Period /></MPD>"#
                    .utf8
            ),
            manifestURL: URL(string: "https://example.com/live.mpd")!
        )

        XCTAssertEqual(manifest.type, .dynamic)
        XCTAssertEqual(manifest.minimumUpdatePeriodMs, 2_000)
        XCTAssertEqual(manifest.timeShiftBufferDepthMs, 30_000)
    }

    func testManifestParserRejectsDrmAndSegmentList() {
        XCTAssertThrowsError(
            try VesperDashManifestParser.parse(
                data: Data(#"<MPD type="static"><Period><AdaptationSet><ContentProtection /></AdaptationSet></Period></MPD>"#.utf8),
                manifestURL: URL(string: "https://example.com/drm.mpd")!
            )
        ) { error in
            guard case VesperDashBridgeError.unsupportedManifest = error else {
                XCTFail("unexpected error \(error)")
                return
            }
        }

        XCTAssertThrowsError(
            try VesperDashManifestParser.parse(
                data: Data(#"<MPD type="static"><Period><AdaptationSet><SegmentList /></AdaptationSet></Period></MPD>"#.utf8),
                manifestURL: URL(string: "https://example.com/segment-list.mpd")!
            )
        ) { error in
            guard case VesperDashBridgeError.unsupportedManifest = error else {
                XCTFail("unexpected error \(error)")
                return
            }
        }
    }

    func testSidxParserReadsVersionZeroBox() throws {
        var data = mp4Box(type: "ftyp", payload: Data([0, 0, 0, 0]))
        data.append(mp4Box(type: "sidx", payload: sidxPayloadV0()))

        let sidx = try VesperDashSidxParser.parse(data: data)

        XCTAssertEqual(sidx.timescale, 1_000)
        XCTAssertEqual(sidx.earliestPresentationTime, 500)
        XCTAssertEqual(sidx.firstOffset, 10)
        XCTAssertEqual(sidx.references.count, 2)
        XCTAssertEqual(sidx.references[0].referencedSize, 100)
        XCTAssertEqual(sidx.references[0].subsegmentDuration, 2_000)
        XCTAssertTrue(sidx.references[0].startsWithSap)
        XCTAssertEqual(sidx.references[1].referencedSize, 150)
    }

    func testMp4BoxFilterRemovesTopLevelSidxBox() throws {
        var data = mp4Box(type: "styp", payload: Data([0x01]))
        data.append(mp4Box(type: "sidx", payload: Data([0x02, 0x03])))
        data.append(mp4Box(type: "moof", payload: Data([0x04])))

        var expected = mp4Box(type: "styp", payload: Data([0x01]))
        expected.append(mp4Box(type: "moof", payload: Data([0x04])))

        XCTAssertEqual(
            try VesperDashMp4BoxFilter.removingTopLevelSidxBoxes(from: data),
            expected
        )
    }

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

    func testSegmentTemplateMediaPlaylistUsesLoopbackSegmentUrls() async throws {
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
        XCTAssertTrue(playlist.contains("/dash/"))
        XCTAssertTrue(playlist.contains("/v4_258/0.m4s"))
        XCTAssertFalse(playlist.contains("v4_258-270146-i-1.m4s"))
        XCTAssertFalse(playlist.contains("data:video/mp4;base64,"))

        let mediaURLText = try XCTUnwrap(
            firstMatch(#"http://127\.0\.0\.1:[0-9]+/dash/[^[:space:]]+/v4_258/0\.m4s"#, in: playlist)
        )
        let (loadedMediaData, _) = try await URLSession.shared.data(from: try XCTUnwrap(URL(string: mediaURLText)))

        // Loopback returns the fMP4 bytes verbatim, including sidx, instead of
        // stripping sequential sidx boxes.
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

    func testConcurrentSegmentTemplateMediaPlaylistsShareLoopbackServer() async throws {
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

        let ports = Set(try playlists.map { try firstLoopbackPort(in: $0) })
        XCTAssertEqual(ports.count, 1)
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

    func testLargeSegmentTemplateLoopbackStreamsTemporaryFileAndSkipsCache() async throws {
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
            firstMatch(#"http://127\.0\.0\.1:[0-9]+/dash/[^[:space:]]+/v1_257/0\.m4s"#, in: playlist)
        )
        var request = URLRequest(url: try XCTUnwrap(URL(string: mediaURLText)))
        request.setValue("bytes=0-15", forHTTPHeaderField: "Range")

        let (data, response) = try await URLSession.shared.data(for: request)
        let httpResponse = try XCTUnwrap(response as? HTTPURLResponse)

        XCTAssertEqual(httpResponse.statusCode, 206)
        XCTAssertEqual(data, Data((0..<16).map(UInt8.init)))

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

    func testHlsBuilderCreatesMasterAndMediaPlaylists() throws {
        let manifest = try VesperDashManifestParser.parse(
            data: Data(sampleMpd.utf8),
            manifestURL: URL(string: "https://origin.example.com/path/master.mpd")!
        )
        let master = try VesperDashHlsBuilder.buildMasterPlaylist(
            manifest: manifest,
            mediaURL: { "vesper-dash://media/session/\($0).m3u8" }
        )

        XCTAssertTrue(master.contains("#EXT-X-MEDIA:TYPE=AUDIO,GROUP-ID=\"audio\""))
        XCTAssertTrue(master.contains("BANDWIDTH=1856000"))
        XCTAssertTrue(master.contains("AVERAGE-BANDWIDTH=928000"))
        XCTAssertTrue(master.contains("CODECS=\"avc1.64001f,mp4a.40.2\""))
        XCTAssertTrue(master.contains("vesper-dash://media/session/v1.m3u8"))

        let video = manifest.periods[0].adaptationSets[0].representations[0]
        let segmentBase = try XCTUnwrap(video.segmentBase)
        let segments = try VesperDashHlsBuilder.mediaSegments(
            segmentBase: segmentBase,
            sidx: VesperDashSidxBox(
                timescale: 1_000,
                earliestPresentationTime: 0,
                firstOffset: 10,
                references: [
                    VesperDashSidxReference(
                        referenceType: 0,
                        referencedSize: 100,
                        subsegmentDuration: 2_000,
                        startsWithSap: true,
                        sapType: 1,
                        sapDeltaTime: 0
                    ),
                    VesperDashSidxReference(
                        referenceType: 0,
                        referencedSize: 150,
                        subsegmentDuration: 3_500,
                        startsWithSap: true,
                        sapType: 1,
                        sapDeltaTime: 0
                    ),
                ]
            )
        )
        let media = try VesperDashHlsBuilder.buildMediaPlaylist(
            initializationURI: "vesper-dash://segment/session/v1/init.mp4",
            segments: segments,
            segmentURI: { "vesper-dash://segment/session/v1/\($0).m4s" }
        )

        XCTAssertTrue(media.contains("#EXT-X-MAP:URI=\"vesper-dash://segment/session/v1/init.mp4\""))
        XCTAssertTrue(media.contains("#EXT-X-MEDIA-SEQUENCE:1"))
        XCTAssertTrue(media.contains("vesper-dash://segment/session/v1/0.m4s"))
        XCTAssertTrue(media.contains("vesper-dash://segment/session/v1/1.m4s"))
        XCTAssertEqual(segments[0].range, try VesperDashByteRange(start: 1210, end: 1309))
        XCTAssertEqual(segments[1].range, try VesperDashByteRange(start: 1310, end: 1459))
        XCTAssertTrue(media.hasSuffix("#EXT-X-ENDLIST\n"))

        let externalMedia = try VesperDashHlsBuilder.buildExternalMediaPlaylist(
            map: VesperDashHlsMap(
                uri: video.baseURL,
                byteRange: segmentBase.initialization
            ),
            segments: segments.map {
                VesperDashHlsSegment(duration: $0.duration, uri: video.baseURL, byteRange: $0.range)
            }
        )
        XCTAssertTrue(externalMedia.contains("#EXT-X-MEDIA-SEQUENCE:1"))
        XCTAssertTrue(externalMedia.contains("#EXT-X-MAP:URI=\"https://cdn.example.com/root/video/seg.m4s\",BYTERANGE=\"1000@0\""))
        XCTAssertTrue(externalMedia.contains("#EXT-X-BYTERANGE:100@1210\nhttps://cdn.example.com/root/video/seg.m4s"))
        XCTAssertTrue(externalMedia.contains("#EXT-X-BYTERANGE:150@1310\nhttps://cdn.example.com/root/video/seg.m4s"))
    }

    func testHlsBuilderCreatesSegmentTemplateMediaPlaylist() throws {
        let manifest = try VesperDashManifestParser.parse(
            data: Data(sampleSegmentTemplateMpd.utf8),
            manifestURL: URL(string: "https://dash.akamaized.net/envivio/EnvivioDash3/manifest.mpd")!
        )
        let master = try VesperDashHlsBuilder.buildMasterPlaylist(
            manifest: manifest,
            mediaURL: { "vesper-dash://media/session/\($0).m3u8" }
        )

        XCTAssertTrue(master.contains("BANDWIDTH=2661600"))
        XCTAssertTrue(master.contains("AVERAGE-BANDWIDTH=1330800"))
        XCTAssertTrue(master.contains("CODECS=\"avc1.4D401E,mp4a.40.2\""))
        XCTAssertTrue(master.contains("vesper-dash://media/session/v1_257.m3u8"))

        let video = manifest.periods[0].adaptationSets[0].representations[0]
        let template = try XCTUnwrap(video.segmentTemplate)
        let segments = try VesperDashHlsBuilder.templateSegments(
            durationMs: manifest.durationMs,
            segmentTemplate: template
        )
        XCTAssertEqual(segments.count, 97)
        XCTAssertEqual(segments[0].number, 1)
        XCTAssertEqual(segments[96].number, 97)
        XCTAssertNil(segments[0].time)
        XCTAssertEqual(segments[0].duration, 2.0, accuracy: 0.000_001)
        XCTAssertEqual(segments[96].duration, 1.68, accuracy: 0.000_001)

        let media = try VesperDashHlsBuilder.buildMediaPlaylist(
            initializationURI: "vesper-dash://segment/session/v1_257/init.mp4",
            segments: segments,
            segmentURI: { "vesper-dash://segment/session/v1_257/\($0).m4s" }
        )
        XCTAssertTrue(media.contains("#EXT-X-MAP:URI=\"vesper-dash://segment/session/v1_257/init.mp4\""))
        XCTAssertTrue(media.contains("#EXTINF:2.000,"))
        XCTAssertTrue(media.contains("#EXTINF:1.680,"))
        XCTAssertTrue(media.contains("vesper-dash://segment/session/v1_257/0.m4s"))
        XCTAssertTrue(media.hasSuffix("#EXT-X-ENDLIST\n"))

        let externalMedia = try VesperDashHlsBuilder.buildExternalMediaPlaylist(
            map: VesperDashHlsMap(
                uri: "https://dash.akamaized.net/envivio/EnvivioDash3/v1_257-Header.m4s",
                byteRange: nil
            ),
            segments: [
                VesperDashHlsSegment(
                    duration: segments[0].duration,
                    uri: "https://dash.akamaized.net/envivio/EnvivioDash3/v1_257-270146-i-1.m4s",
                    byteRange: nil
                ),
            ]
        )
        XCTAssertTrue(externalMedia.contains("#EXT-X-MEDIA-SEQUENCE:1"))
        XCTAssertTrue(externalMedia.contains("#EXT-X-MAP:URI=\"https://dash.akamaized.net/envivio/EnvivioDash3/v1_257-Header.m4s\""))
        XCTAssertTrue(externalMedia.contains("https://dash.akamaized.net/envivio/EnvivioDash3/v1_257-270146-i-1.m4s"))
        XCTAssertFalse(externalMedia.contains("#EXT-X-BYTERANGE"))
    }

    func testMasterPlaylistCanUseSingleStartupVariant() throws {
        let manifest = try VesperDashManifestParser.parse(
            data: Data(sampleMultiVideoSegmentTemplateMpd.utf8),
            manifestURL: URL(string: "https://dash.akamaized.net/envivio/EnvivioDash3/manifest.mpd")!
        )
        let selected = try VesperDashHlsBuilder.selectedPlayableRepresentations(
            manifest: manifest,
            variantPolicy: .startupSingleVariant
        )
        let master = try VesperDashHlsBuilder.buildMasterPlaylist(
            manifest: manifest,
            variantPolicy: .startupSingleVariant,
            mediaURL: { "vesper-dash://media/session/\($0).m3u8" }
        )

        XCTAssertEqual(selected.video.map(\.renditionId), ["v1_257"])
        XCTAssertEqual(selected.audio.map(\.renditionId), ["v4_258"])
        XCTAssertEqual(countOccurrences(of: "#EXT-X-STREAM-INF", in: master), 1)
        XCTAssertEqual(countOccurrences(of: "#EXT-X-MEDIA:TYPE=AUDIO", in: master), 1)
        XCTAssertTrue(master.contains("vesper-dash://media/session/v1_257.m3u8"))
        XCTAssertTrue(master.contains("vesper-dash://media/session/v4_258.m3u8"))
        XCTAssertFalse(master.contains("vesper-dash://media/session/v2_257.m3u8"))
        XCTAssertFalse(master.contains("vesper-dash://media/session/v7_257.m3u8"))
    }

    func testMasterPlaylistDowngradesUnsupportedAv1ToHardwareHevc() throws {
        let manifest = try VesperDashManifestParser.parse(
            data: Data(sampleMultiCodecSegmentTemplateMpd.utf8),
            manifestURL: URL(string: "https://example.com/manifest.mpd")!
        )
        let capabilities = [
            VesperDashVideoDecodeCapability(
                renditionId: "av1",
                codecFamily: .av1,
                hardwareDecodeSupported: false,
                decoderName: nil
            ),
            VesperDashVideoDecodeCapability(
                renditionId: "hevc",
                codecFamily: .hevc,
                hardwareDecodeSupported: true,
                decoderName: "VideoToolbox"
            ),
            VesperDashVideoDecodeCapability(
                renditionId: "avc",
                codecFamily: .avc,
                hardwareDecodeSupported: true,
                decoderName: "VideoToolbox"
            ),
        ]

        let selected = try VesperDashHlsBuilder.selectedPlayableRepresentations(
            manifest: manifest,
            variantPolicy: .startupSingleVariant,
            videoDecodeCapabilities: capabilities
        )
        let master = try VesperDashHlsBuilder.buildMasterPlaylist(
            manifest: manifest,
            variantPolicy: .all,
            videoDecodeCapabilities: capabilities,
            mediaURL: { "vesper-dash://media/session/\($0).m3u8" }
        )

        XCTAssertEqual(selected.video.map(\.renditionId), ["hevc"])
        XCTAssertFalse(master.contains("vesper-dash://media/session/av1.m3u8"))
        XCTAssertTrue(master.contains("vesper-dash://media/session/hevc.m3u8"))
        XCTAssertTrue(master.contains("vesper-dash://media/session/avc.m3u8"))
    }

    func testMasterPlaylistFailsWhenAllVideoIsSoftwareOnly() throws {
        let manifest = try VesperDashManifestParser.parse(
            data: Data(sampleMultiCodecSegmentTemplateMpd.utf8),
            manifestURL: URL(string: "https://example.com/manifest.mpd")!
        )
        let capabilities = [
            VesperDashVideoDecodeCapability(
                renditionId: "av1",
                codecFamily: .av1,
                hardwareDecodeSupported: false,
                decoderName: nil
            ),
            VesperDashVideoDecodeCapability(
                renditionId: "hevc",
                codecFamily: .hevc,
                hardwareDecodeSupported: false,
                decoderName: nil
            ),
            VesperDashVideoDecodeCapability(
                renditionId: "avc",
                codecFamily: .avc,
                hardwareDecodeSupported: false,
                decoderName: nil
            ),
        ]

        XCTAssertThrowsError(
            try VesperDashHlsBuilder.selectedPlayableRepresentations(
                manifest: manifest,
                variantPolicy: .all,
                videoDecodeCapabilities: capabilities
            )
        ) { error in
            guard case VesperDashBridgeError.unsupportedManifest = error else {
                return XCTFail("Expected unsupportedManifest, got \(error)")
            }
        }
    }

    func testDashManifestTrackCatalogExposesPlayableAudioAndVideoTracks() throws {
        let manifest = try VesperDashManifestParser.parse(
            data: Data(sampleMultiVideoSegmentTemplateMpd.utf8),
            manifestURL: URL(string: "https://dash.akamaized.net/envivio/EnvivioDash3/manifest.mpd")!
        )
        let selected = try VesperDashHlsBuilder.selectedPlayableRepresentations(
            manifest: manifest,
            variantPolicy: .all
        )

        let snapshot = VesperDashManifestTrackCatalogSnapshot(
            audio: selected.audio,
            video: selected.video
        )

        XCTAssertTrue(snapshot.adaptiveVideo)
        XCTAssertFalse(snapshot.adaptiveAudio)
        XCTAssertEqual(
            snapshot.videoTracks.map(\.id),
            [
                "video:dash:v1_257",
                "video:dash:v2_257",
                "video:dash:v7_257",
            ]
        )
        XCTAssertEqual(snapshot.videoTracks[0].bitRate, 1_200_000)
        XCTAssertEqual(snapshot.videoTracks[0].width, 768)
        XCTAssertEqual(snapshot.videoTracks[0].height, 432)
        XCTAssertEqual(snapshot.videoTracks[0].codec, "avc1.4D401E")
        XCTAssertEqual(snapshot.videoTracks[0].frameRate ?? 0, 30_000.0 / 1_001.0, accuracy: 0.001)
        XCTAssertEqual(snapshot.audioTracks.map(\.id), ["audio:dash:v4_258"])
        XCTAssertEqual(snapshot.audioTracks[0].language, "qaa")
        XCTAssertEqual(snapshot.audioTracks[0].codec, "mp4a.40.2")
        XCTAssertEqual(snapshot.audioTracks[0].sampleRate, 48_000)
        XCTAssertEqual(snapshot.videoVariantPinsByTrackId["video:dash:v7_257"]?.maxHeight, 1_080)
    }

    func testDashManifestTrackCatalogMarksSingleVideoAsNonAdaptive() throws {
        let manifest = try VesperDashManifestParser.parse(
            data: Data(sampleSegmentTemplateMpd.utf8),
            manifestURL: URL(string: "https://dash.akamaized.net/envivio/EnvivioDash3/manifest.mpd")!
        )
        let selected = try VesperDashHlsBuilder.selectedPlayableRepresentations(
            manifest: manifest,
            variantPolicy: .all
        )

        let snapshot = VesperDashManifestTrackCatalogSnapshot(
            audio: selected.audio,
            video: selected.video
        )

        XCTAssertFalse(snapshot.adaptiveVideo)
        XCTAssertFalse(snapshot.adaptiveAudio)
        XCTAssertEqual(snapshot.videoTracks.map(\.id), ["video:dash:v1_257"])
    }

    func testDashWebVttSubtitlesReachMasterPlaylistAndTrackCatalog() throws {
        let manifest = try VesperDashManifestParser.parse(
            data: Data(sampleWebVttSubtitleMpd.utf8),
            manifestURL: URL(string: "https://cdn.example.com/vod/manifest.mpd")!
        )
        let selected = try VesperDashHlsBuilder.selectedPlayableRepresentations(
            manifest: manifest,
            variantPolicy: .all
        )

        XCTAssertEqual(selected.subtitles.map(\.renditionId), ["sub-en"])
        let subtitleTemplate = try XCTUnwrap(
            selected.subtitles[0].representation.segmentTemplate
        )
        XCTAssertNil(subtitleTemplate.initialization)

        let master = try VesperDashHlsBuilder.buildMasterPlaylist(
            manifest: manifest,
            mediaURL: { "vesper-dash://media/session/\($0).m3u8" }
        )
        XCTAssertTrue(master.contains("#EXT-X-MEDIA:TYPE=SUBTITLES,GROUP-ID=\"subtitles\""))
        XCTAssertTrue(master.contains("LANGUAGE=\"en\""))
        XCTAssertTrue(master.contains("SUBTITLES=\"subtitles\""))
        XCTAssertTrue(master.contains("vesper-dash://media/session/sub-en.m3u8"))

        let subtitleSegments = try VesperDashHlsBuilder.templateSegments(
            manifestType: manifest.type,
            durationMs: manifest.durationMs,
            segmentTemplate: subtitleTemplate
        )
        let subtitleMedia = try VesperDashHlsBuilder.buildMediaPlaylist(
            initializationURI: nil,
            segments: subtitleSegments,
            segmentURI: { _, segment in
                "https://cdn.example.com/vod/sub-\(segment.number).vtt"
            }
        )
        XCTAssertFalse(subtitleMedia.contains("#EXT-X-MAP"))
        XCTAssertTrue(subtitleMedia.contains("https://cdn.example.com/vod/sub-1.vtt"))
        XCTAssertTrue(subtitleMedia.hasSuffix("#EXT-X-ENDLIST\n"))

        let snapshot = VesperDashManifestTrackCatalogSnapshot(
            audio: selected.audio,
            video: selected.video,
            subtitles: selected.subtitles
        )
        XCTAssertEqual(snapshot.subtitleTracks.map(\.id), ["subtitle:dash:sub-en"])
        XCTAssertEqual(snapshot.subtitleTracks[0].language, "en")
        XCTAssertEqual(snapshot.subtitleTracks[0].codec, "wvtt")
    }

    func testManifestParserReadsSegmentTimelineTemplate() throws {
        let manifest = try VesperDashManifestParser.parse(
            data: Data(sampleSegmentTimelineMpd.utf8),
            manifestURL: URL(string: "https://cdn.example.com/live/vod.mpd")!
        )
        let template = try XCTUnwrap(
            manifest.periods[0].adaptationSets[0].representations[0].segmentTemplate
        )

        XCTAssertNil(template.duration)
        XCTAssertEqual(template.timescale, 1_000)
        XCTAssertEqual(template.startNumber, 7)
        XCTAssertEqual(template.presentationTimeOffset, 5_000)
        XCTAssertEqual(
            template.timeline,
            [
                VesperDashSegmentTimelineEntry(startTime: 5_000, duration: 2_000, repeatCount: 2),
                VesperDashSegmentTimelineEntry(startTime: nil, duration: 1_000, repeatCount: 0),
            ]
        )
    }

    func testHlsBuilderCreatesSegmentTimelineMediaPlaylist() throws {
        let manifest = try VesperDashManifestParser.parse(
            data: Data(sampleSegmentTimelineMpd.utf8),
            manifestURL: URL(string: "https://cdn.example.com/live/vod.mpd")!
        )
        let template = try XCTUnwrap(
            manifest.periods[0].adaptationSets[0].representations[0].segmentTemplate
        )
        let segments = try VesperDashHlsBuilder.templateSegments(
            durationMs: manifest.durationMs,
            segmentTemplate: template
        )

        XCTAssertEqual(
            segments,
            [
                VesperDashTemplateSegment(duration: 2.0, number: 7, time: 5_000),
                VesperDashTemplateSegment(duration: 2.0, number: 8, time: 7_000),
                VesperDashTemplateSegment(duration: 2.0, number: 9, time: 9_000),
                VesperDashTemplateSegment(duration: 1.0, number: 10, time: 11_000),
            ]
        )

        let expanded = try VesperDashTemplateExpander.expand(
            "chunk-$Time%05d$-$Number$.m4s",
            representation: manifest.periods[0].adaptationSets[0].representations[0],
            number: segments[0].number,
            time: segments[0].time
        )
        XCTAssertEqual(expanded, "chunk-05000-7.m4s")
    }

    func testHlsBuilderCreatesLiveSegmentTimelinePlaylist() throws {
        let manifest = try VesperDashManifestParser.parse(
            data: Data(sampleDynamicSegmentTimelineMpd.utf8),
            manifestURL: URL(string: "https://cdn.example.com/live/manifest.mpd")!
        )
        let template = try XCTUnwrap(
            manifest.periods[0].adaptationSets[0].representations[0].segmentTemplate
        )
        let segments = try VesperDashHlsBuilder.templateSegments(
            manifestType: manifest.type,
            durationMs: manifest.durationMs,
            segmentTemplate: template
        )

        XCTAssertEqual(manifest.type, .dynamic)
        XCTAssertEqual(segments.map(\.number), [101, 102, 103])

        let media = try VesperDashHlsBuilder.buildMediaPlaylist(
            initializationURI: "vesper-dash://segment/session/live/init.mp4",
            segments: segments,
            playlistKind: .live,
            mediaSequence: segments.first?.number,
            segmentURI: { _, segment in
                "http://127.0.0.1:1/dash/session/live/\(segment.number).m4s"
            }
        )

        XCTAssertTrue(media.contains("#EXT-X-MEDIA-SEQUENCE:101"))
        XCTAssertTrue(media.contains("http://127.0.0.1:1/dash/session/live/101.m4s"))
        XCTAssertFalse(media.contains("#EXT-X-PLAYLIST-TYPE:VOD"))
        XCTAssertFalse(media.contains("#EXT-X-ENDLIST"))
    }

    func testHlsBuilderRejectsDynamicDurationOnlyTemplate() throws {
        let manifest = try VesperDashManifestParser.parse(
            data: Data(sampleDynamicDurationTemplateMpd.utf8),
            manifestURL: URL(string: "https://cdn.example.com/live/manifest.mpd")!
        )
        let template = try XCTUnwrap(
            manifest.periods[0].adaptationSets[0].representations[0].segmentTemplate
        )

        XCTAssertThrowsError(
            try VesperDashHlsBuilder.templateSegments(
                manifestType: manifest.type,
                durationMs: manifest.durationMs,
                segmentTemplate: template
            )
        ) { error in
            guard case VesperDashBridgeError.unsupportedManifest = error else {
                XCTFail("unexpected error \(error)")
                return
            }
        }
    }

    func testHlsBuilderExpandsOpenEndedSegmentTimeline() throws {
        let manifest = try VesperDashManifestParser.parse(
            data: Data(sampleOpenEndedSegmentTimelineMpd.utf8),
            manifestURL: URL(string: "https://cdn.example.com/vod.mpd")!
        )
        let template = try XCTUnwrap(
            manifest.periods[0].adaptationSets[0].representations[0].segmentTemplate
        )
        let segments = try VesperDashHlsBuilder.templateSegments(
            durationMs: manifest.durationMs,
            segmentTemplate: template
        )

        XCTAssertEqual(
            segments,
            [
                VesperDashTemplateSegment(duration: 2.0, number: 1, time: 0),
                VesperDashTemplateSegment(duration: 2.0, number: 2, time: 2_000),
                VesperDashTemplateSegment(duration: 1.5, number: 3, time: 4_000),
            ]
        )
    }

    func testSegmentTemplateExpandsRepresentationIdNumberAndBandwidth() throws {
        let manifest = try VesperDashManifestParser.parse(
            data: Data(sampleSegmentTemplateMpd.utf8),
            manifestURL: URL(string: "https://dash.akamaized.net/envivio/EnvivioDash3/manifest.mpd")!
        )
        let representation = manifest.periods[0].adaptationSets[0].representations[0]

        XCTAssertEqual(
            try VesperDashTemplateExpander.expand(
                "$RepresentationID$-$Number%05d$-$Bandwidth$.m4s",
                representation: representation,
                number: 12
            ),
            "v1_257-00012-1200000.m4s"
        )
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

    /// Prevents HLS tag concatenation regressions. Swift multiline string
    /// literals previously swallowed a trailing newline and glued
    /// `#EXT-X-PLAYLIST-TYPE:VOD#EXT-X-MAP:URI=...` onto one line, which
    /// AVPlayer silently ignored before surfacing `'frmt'`.
    /// This assertion guarantees that every playlist generated by HlsBuilder
    /// has at most one `#EXT-X-` tag per line.
    func testHlsBuilderNeverGluesTwoTagsOnTheSameLine() throws {
        let manifest = try VesperDashManifestParser.parse(
            data: Data(sampleSegmentTemplateMpd.utf8),
            manifestURL: URL(string: "https://dash.akamaized.net/envivio/EnvivioDash3/manifest.mpd")!
        )
        let master = try VesperDashHlsBuilder.buildMasterPlaylist(
            manifest: manifest,
            mediaURL: { "vesper-dash://media/session/\($0).m3u8" }
        )
        let template = try XCTUnwrap(
            manifest.periods[0].adaptationSets[0].representations[0].segmentTemplate
        )
        let segments = try VesperDashHlsBuilder.templateSegments(
            durationMs: manifest.durationMs,
            segmentTemplate: template
        )
        let media = try VesperDashHlsBuilder.buildMediaPlaylist(
            initializationURI: "vesper-dash://segment/session/v1_257/init.mp4",
            segments: segments,
            segmentURI: { "vesper-dash://segment/session/v1_257/\($0).m4s" }
        )
        let externalMedia = try VesperDashHlsBuilder.buildExternalMediaPlaylist(
            map: VesperDashHlsMap(
                uri: "vesper-dash://segment/session/v1_257/init.mp4",
                byteRange: nil
            ),
            segments: [
                VesperDashHlsSegment(
                    duration: 2.0,
                    uri: "http://127.0.0.1:1/dash/x/v1_257/0.m4s",
                    byteRange: nil
                ),
            ]
        )

        for (label, playlist) in [("master", master), ("media", media), ("externalMedia", externalMedia)] {
            // Playlists must end with \n so callers can safely append more
            // tags or write the text directly to a file.
            XCTAssertTrue(playlist.hasSuffix("\n"), "\(label) playlist is missing a trailing newline")
            for (index, line) in playlist.components(separatedBy: "\n").enumerated() {
                let tagCount = line.components(separatedBy: "#EXT-X-").count - 1
                XCTAssertLessThanOrEqual(
                    tagCount, 1,
                    "\(label) playlist line \(index + 1) contains multiple #EXT-X- tags: \(line)"
                )
            }
        }
    }
}

private func makeTestDashSession(sourceURL: URL) -> VesperDashSession {
    VesperDashSession(
        sourceURL: sourceURL,
        videoDecodeCapabilityProvider: testHardwareVideoDecodeCapabilityProvider
    )
}

private let testHardwareVideoDecodeCapabilityProvider: VesperDashSession.VideoDecodeCapabilityProvider = { playable in
    let candidate = VesperHardwareDecodeCandidateCodec(codecName: playable.representation.codecs)
    let family = candidate.dashCodecFamily
    let supported = family != .unknown
    return VesperDashVideoDecodeCapability(
        renditionId: playable.renditionId,
        codecFamily: family,
        hardwareDecodeSupported: supported,
        decoderName: supported ? "UnitTestHardwareDecoder" : nil
    )
}

private let sampleMpd = #"""
<?xml version="1.0"?>
<MPD type="static" mediaPresentationDuration="PT1M30.5S" minBufferTime="PT1.5S">
  <BaseURL>https://cdn.example.com/root/master.mpd</BaseURL>
  <Period id="p0">
    <AdaptationSet id="v" contentType="video" mimeType="video/mp4">
      <BaseURL>video/</BaseURL>
      <Representation id="v1" bandwidth="800000" codecs="avc1.64001f" width="1280" height="720" frameRate="30000/1001">
        <BaseURL>seg.m4s</BaseURL>
        <SegmentBase indexRange="1000-1199">
          <Initialization range="0-999"/>
        </SegmentBase>
      </Representation>
    </AdaptationSet>
    <AdaptationSet id="a" mimeType="audio/mp4" lang="ja">
      <Representation id="a1" bandwidth="128000" codecs="mp4a.40.2" audioSamplingRate="48000">
        <BaseURL>../audio/main.m4s</BaseURL>
        <SegmentBase indexRange="800-950">
          <Initialization range="0-799"/>
        </SegmentBase>
      </Representation>
    </AdaptationSet>
  </Period>
</MPD>
"""#

private let sampleSegmentTemplateMpd = #"""
<?xml version="1.0" encoding="UTF-8"?>
<MPD type="static" mediaPresentationDuration="PT193.680S" minBufferTime="PT5.000S">
  <Period id="period0">
    <AdaptationSet mimeType="video/mp4" segmentAlignment="true" startWithSAP="1">
      <SegmentTemplate timescale="90000" initialization="$RepresentationID$-Header.m4s" media="$RepresentationID$-270146-i-$Number$.m4s" startNumber="1" duration="179704" presentationTimeOffset="0"/>
      <Representation id="v1_257" bandwidth="1200000" codecs="avc1.4D401E" width="768" height="432" frameRate="30000/1001"/>
    </AdaptationSet>
    <AdaptationSet mimeType="audio/mp4" segmentAlignment="true" startWithSAP="1" lang="qaa">
      <SegmentTemplate timescale="90000" initialization="$RepresentationID$-Header.m4s" media="$RepresentationID$-270146-i-$Number$.m4s" startNumber="1" duration="179704" presentationTimeOffset="0"/>
      <Representation id="v4_258" bandwidth="130800" codecs="mp4a.40.2" audioSamplingRate="48000"/>
    </AdaptationSet>
  </Period>
</MPD>
"""#

private let sampleMultiVideoSegmentTemplateMpd = #"""
<?xml version="1.0" encoding="UTF-8"?>
<MPD type="static" mediaPresentationDuration="PT193.680S" minBufferTime="PT5.000S">
  <Period id="period0">
    <AdaptationSet mimeType="video/mp4" segmentAlignment="true" startWithSAP="1">
      <SegmentTemplate timescale="90000" initialization="$RepresentationID$-Header.m4s" media="$RepresentationID$-270146-i-$Number$.m4s" startNumber="1" duration="179704" presentationTimeOffset="0"/>
      <Representation id="v1_257" bandwidth="1200000" codecs="avc1.4D401E" width="768" height="432" frameRate="30000/1001"/>
      <Representation id="v2_257" bandwidth="1850000" codecs="avc1.4D401E" width="1024" height="576" frameRate="30000/1001"/>
      <Representation id="v7_257" bandwidth="5300000" codecs="avc1.4D401E" width="1920" height="1080" frameRate="30000/1001"/>
    </AdaptationSet>
    <AdaptationSet mimeType="audio/mp4" segmentAlignment="true" startWithSAP="1" lang="qaa">
      <SegmentTemplate timescale="90000" initialization="$RepresentationID$-Header.m4s" media="$RepresentationID$-270146-i-$Number$.m4s" startNumber="1" duration="179704" presentationTimeOffset="0"/>
      <Representation id="v4_258" bandwidth="130800" codecs="mp4a.40.2" audioSamplingRate="48000"/>
    </AdaptationSet>
  </Period>
</MPD>
"""#

private let sampleMultiCodecSegmentTemplateMpd = #"""
<?xml version="1.0" encoding="UTF-8"?>
<MPD type="static" mediaPresentationDuration="PT30S" minBufferTime="PT2S">
  <Period id="period0">
    <AdaptationSet mimeType="video/mp4" segmentAlignment="true" startWithSAP="1">
      <SegmentTemplate timescale="1000" initialization="$RepresentationID$-init.mp4" media="$RepresentationID$-$Number$.m4s" startNumber="1" duration="2000"/>
      <Representation id="av1" bandwidth="760000" codecs="av01.0.05M.08" width="1280" height="720"/>
      <Representation id="hevc" bandwidth="800000" codecs="hvc1.1.6.L93.B0" width="1280" height="720"/>
      <Representation id="avc" bandwidth="800000" codecs="avc1.4D401F" width="1280" height="720"/>
    </AdaptationSet>
    <AdaptationSet mimeType="audio/mp4" segmentAlignment="true" startWithSAP="1" lang="und">
      <SegmentTemplate timescale="1000" initialization="$RepresentationID$-init.mp4" media="$RepresentationID$-$Number$.m4s" startNumber="1" duration="2000"/>
      <Representation id="audio" bandwidth="128000" codecs="mp4a.40.2" audioSamplingRate="48000"/>
    </AdaptationSet>
  </Period>
</MPD>
"""#

private let sampleSegmentTimelineMpd = #"""
<?xml version="1.0" encoding="UTF-8"?>
<MPD type="static" mediaPresentationDuration="PT7S" minBufferTime="PT2S">
  <Period id="period0">
    <AdaptationSet mimeType="video/mp4" segmentAlignment="true">
      <SegmentTemplate timescale="1000" initialization="init-$RepresentationID$.mp4" media="chunk-$Time$.m4s" startNumber="7" presentationTimeOffset="5000">
        <SegmentTimeline>
          <S t="5000" d="2000" r="2"/>
          <S d="1000"/>
        </SegmentTimeline>
      </SegmentTemplate>
      <Representation id="video" bandwidth="800000" codecs="avc1.64001f" width="1280" height="720"/>
    </AdaptationSet>
  </Period>
</MPD>
"""#

private let sampleOpenEndedSegmentTimelineMpd = #"""
<?xml version="1.0" encoding="UTF-8"?>
<MPD type="static" mediaPresentationDuration="PT5.5S" minBufferTime="PT2S">
  <Period id="period0">
    <AdaptationSet mimeType="video/mp4" segmentAlignment="true">
      <SegmentTemplate timescale="1000" initialization="init.mp4" media="chunk-$Time$.m4s">
        <SegmentTimeline>
          <S t="0" d="2000" r="-1"/>
        </SegmentTimeline>
      </SegmentTemplate>
      <Representation id="video" bandwidth="800000" codecs="avc1.64001f" width="1280" height="720"/>
    </AdaptationSet>
  </Period>
</MPD>
"""#

private let sampleDynamicSegmentTimelineMpd = #"""
<?xml version="1.0" encoding="UTF-8"?>
<MPD type="dynamic" minimumUpdatePeriod="PT2S" timeShiftBufferDepth="PT20S" minBufferTime="PT2S">
  <Period id="period0">
    <AdaptationSet mimeType="video/mp4" segmentAlignment="true">
      <SegmentTemplate timescale="1000" initialization="init-$RepresentationID$.mp4" media="chunk-$Time$.m4s" startNumber="101">
        <SegmentTimeline>
          <S t="200000" d="2000" r="2"/>
        </SegmentTimeline>
      </SegmentTemplate>
      <Representation id="live-video" bandwidth="800000" codecs="avc1.64001f" width="1280" height="720"/>
    </AdaptationSet>
  </Period>
</MPD>
"""#

private let sampleDynamicDurationTemplateMpd = #"""
<?xml version="1.0" encoding="UTF-8"?>
<MPD type="dynamic" minimumUpdatePeriod="PT2S" minBufferTime="PT2S">
  <Period id="period0">
    <AdaptationSet mimeType="video/mp4" segmentAlignment="true">
      <SegmentTemplate timescale="1000" initialization="init-$RepresentationID$.mp4" media="chunk-$Number$.m4s" startNumber="101" duration="2000"/>
      <Representation id="live-video" bandwidth="800000" codecs="avc1.64001f" width="1280" height="720"/>
    </AdaptationSet>
  </Period>
</MPD>
"""#

private let sampleWebVttSubtitleMpd = #"""
<?xml version="1.0" encoding="UTF-8"?>
<MPD type="static" mediaPresentationDuration="PT6S" minBufferTime="PT2S">
  <Period id="period0">
    <AdaptationSet mimeType="video/mp4" segmentAlignment="true">
      <SegmentTemplate timescale="1000" initialization="init-$RepresentationID$.mp4" media="video-$Number$.m4s" startNumber="1" duration="2000"/>
      <Representation id="v1" bandwidth="800000" codecs="avc1.64001f" width="1280" height="720"/>
    </AdaptationSet>
    <AdaptationSet id="subs" contentType="text" mimeType="text/vtt" lang="en">
      <SegmentTemplate timescale="1000" media="sub-$Number$.vtt" startNumber="1" duration="2000"/>
      <Representation id="sub-en" bandwidth="1200" codecs="wvtt"/>
    </AdaptationSet>
  </Period>
</MPD>
"""#

private func sidxPayloadV0() -> Data {
    var payload = Data()
    payload.append(contentsOf: [0, 0, 0, 0])
    appendUInt32(1, to: &payload)
    appendUInt32(1_000, to: &payload)
    appendUInt32(500, to: &payload)
    appendUInt32(10, to: &payload)
    appendUInt16(0, to: &payload)
    appendUInt16(2, to: &payload)
    appendReference(size: 100, duration: 2_000, startsWithSap: true, sapType: 1, sapDeltaTime: 0, to: &payload)
    appendReference(size: 150, duration: 3_000, startsWithSap: true, sapType: 2, sapDeltaTime: 5, to: &payload)
    return payload
}

private func appendReference(
    size: UInt32,
    duration: UInt32,
    startsWithSap: Bool,
    sapType: UInt8,
    sapDeltaTime: UInt32,
    to data: inout Data
) {
    appendUInt32(size & 0x7fff_ffff, to: &data)
    appendUInt32(duration, to: &data)
    let sap = (UInt32(startsWithSap ? 1 : 0) << 31)
        | ((UInt32(sapType) & 0x07) << 28)
        | (sapDeltaTime & 0x0fff_ffff)
    appendUInt32(sap, to: &data)
}

private func mp4Box(type: String, payload: Data) -> Data {
    var data = Data()
    appendUInt32(UInt32(payload.count + 8), to: &data)
    data.append(contentsOf: type.utf8)
    data.append(payload)
    return data
}

private func appendUInt16(_ value: UInt16, to data: inout Data) {
    data.append(UInt8((value >> 8) & 0xff))
    data.append(UInt8(value & 0xff))
}

private func appendUInt32(_ value: UInt32, to data: inout Data) {
    data.append(UInt8((value >> 24) & 0xff))
    data.append(UInt8((value >> 16) & 0xff))
    data.append(UInt8((value >> 8) & 0xff))
    data.append(UInt8(value & 0xff))
}

private func countOccurrences(of needle: String, in haystack: String) -> Int {
    haystack.components(separatedBy: needle).count - 1
}

private func firstMatch(_ pattern: String, in text: String) -> String? {
    text.range(of: pattern, options: .regularExpression).map { String(text[$0]) }
}

private func firstLoopbackPort(in playlist: String) throws -> Int {
    let urlText = try XCTUnwrap(
        firstMatch(#"http://127\.0\.0\.1:[0-9]+/dash/[^\s"]+"#, in: playlist)
    )
    let url = try XCTUnwrap(URL(string: urlText))
    return try XCTUnwrap(url.port)
}

private func eventAttributes(
    _ name: String,
    in events: [(name: String, attributes: [String: String])],
    where matches: ([String: String]) -> Bool = { _ in true }
) -> [String: String]? {
    events.first { $0.name == name && matches($0.attributes) }?.attributes
}

private final class CountingDashNetworkClient: VesperDashNetworkClient {
    private let dataByURL: [URL: Data]
    private let delayNanoseconds: UInt64
    private let lock = NSLock()
    private var counts: [String: Int] = [:]

    init(dataByURL: [URL: Data], delayNanoseconds: UInt64 = 0) {
        self.dataByURL = dataByURL
        self.delayNanoseconds = delayNanoseconds
        super.init()
    }

    override func data(
        for url: URL,
        byteRange: VesperDashByteRange? = nil
    ) async throws -> Data {
        if delayNanoseconds > 0 {
            try await Task.sleep(nanoseconds: delayNanoseconds)
        }
        incrementRequestCount(for: url, byteRange: byteRange)
        guard let payload = dataByURL[url] else {
            throw VesperDashBridgeError.network("missing test payload for \(url.absoluteString)")
        }
        guard let byteRange else {
            return payload
        }
        let start = Int(byteRange.start)
        let end = Int(byteRange.end)
        guard start >= 0, end < payload.count, start <= end else {
            throw VesperDashBridgeError.network("test byte range is out of bounds")
        }
        return payload.subdata(in: start..<(end + 1))
    }

    override func download(
        for url: URL,
        byteRange: VesperDashByteRange? = nil,
        to destinationURL: URL
    ) async throws -> UInt64 {
        let payload = try await data(for: url, byteRange: byteRange)
        try FileManager.default.createDirectory(
            at: destinationURL.deletingLastPathComponent(),
            withIntermediateDirectories: true
        )
        try payload.write(to: destinationURL)
        return UInt64(payload.count)
    }

    func requestCount(for url: URL, byteRange: VesperDashByteRange? = nil) -> Int {
        lock.lock()
        defer { lock.unlock() }
        return counts[requestKey(for: url, byteRange: byteRange), default: 0]
    }

    private func incrementRequestCount(for url: URL, byteRange: VesperDashByteRange?) {
        lock.lock()
        defer { lock.unlock() }
        counts[requestKey(for: url, byteRange: byteRange), default: 0] += 1
    }

    private func requestKey(for url: URL, byteRange: VesperDashByteRange?) -> String {
        if let byteRange {
            return "\(url.absoluteString)#\(byteRange.start)-\(byteRange.end)"
        }
        return url.absoluteString
    }
}

private func sampleSegmentBaseMediaData() -> Data {
    var payload = mp4Box(type: "ftyp", payload: Data(repeating: 0, count: 992))
    let sidxBox = mp4Box(type: "sidx", payload: sidxPayloadV0())
    payload.append(sidxBox)
    if payload.count < 1_600 {
        payload.append(Data(repeating: 0x55, count: 1_600 - payload.count))
    }
    return payload
}

private func writeSegmentTemplateFiles(
    directory: URL,
    renditionId: String,
    initData: Data,
    mediaData: Data,
    segmentCount: Int = 97
) throws {
    try initData.write(to: directory.appendingPathComponent("\(renditionId)-Header.m4s"))
    for number in 1...segmentCount {
        try mediaData.write(
            to: directory.appendingPathComponent("\(renditionId)-270146-i-\(number).m4s")
        )
    }
}
