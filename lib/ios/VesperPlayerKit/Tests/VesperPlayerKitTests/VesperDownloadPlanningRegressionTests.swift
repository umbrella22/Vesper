import Foundation
import XCTest
@testable import VesperPlayerKit

final class VesperDownloadPlanningRegressionTests: XCTestCase {
    func testOverflowingHlsRangeIsRejectedWithoutChangingImplicitOffset() throws {
        var previous: UInt64 = 42
        XCTAssertNil(parseHlsByteRange("2@18446744073709551615", previousRangeEnd: &previous))
        XCTAssertEqual(previous, 42)
        XCTAssertNil(parseHlsByteRange("0@0", previousRangeEnd: &previous))
        XCTAssertNil(parseHlsByteRange("2@invalid", previousRangeEnd: &previous))
        XCTAssertThrowsError(try parseHlsMediaPlaylist(
            playlistUri: "https://example.com/index.m3u8",
            playlistText: "#EXTM3U\n#EXT-X-BYTERANGE:2@18446744073709551615\nmedia.ts\n#EXT-X-ENDLIST\n"
        ))
    }

    func testOrdinaryHlsRangeKeepsExplicitAndImplicitOffsets() {
        var previous: UInt64 = 0
        XCTAssertEqual(parseHlsByteRange("16@32", previousRangeEnd: &previous), VesperDownloadByteRange(offset: 32, length: 16))
        XCTAssertEqual(parseHlsByteRange("4", previousRangeEnd: &previous), VesperDownloadByteRange(offset: 48, length: 4))
        XCTAssertEqual(previous, 52)
    }

    func testUnrepresentableDashCountAndNumberingAreRejected() throws {
        XCTAssertThrowsError(try plannedDashSegmentCount(durationSeconds: 18_446_744_073_709_551_616, segmentSeconds: 1, startNumber: 1))
        XCTAssertThrowsError(try plannedDashSegmentCount(durationSeconds: .infinity, segmentSeconds: 1, startNumber: 1))
        XCTAssertThrowsError(try plannedDashSegmentCount(durationSeconds: 2, segmentSeconds: 1, startNumber: .max))
        XCTAssertEqual(try plannedDashSegmentCount(durationSeconds: 5.25, segmentSeconds: 2, startNumber: 7), 3)
        XCTAssertNil(parseIso8601DurationSeconds("PT" + String(repeating: "9", count: 400) + "S"))
    }

    func testMasterWithoutExternalAudioRetainsItsReferencedMediaPlaylist() async throws {
        let directory = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: directory) }
        try Data([1, 2, 3]).write(to: directory.appendingPathComponent("media.ts"))
        let media = "#EXTM3U\n#EXTINF:1,\nmedia.ts\n#EXT-X-ENDLIST\n"
        try Data(media.utf8).write(to: directory.appendingPathComponent("video.m3u8"))
        let index = try await VesperForegroundDownloadExecutor().planHlsMasterAssetIndex(
            manifestUri: directory.appendingPathComponent("master.m3u8").absoluteString,
            manifestText: "#EXTM3U\n#EXT-X-STREAM-INF:BANDWIDTH=1000\nvideo.m3u8\n",
            profile: VesperDownloadProfile(), requestHeaders: [:]
        )
        let root = try XCTUnwrap(index.resources.first { $0.relativePath == "index.m3u8" }?.generatedText)
        XCTAssertTrue(root.contains("video.m3u8"))
        XCTAssertNotNil(index.resources.first { $0.relativePath == "video.m3u8" && $0.generatedText != nil })
    }
    func testHlsMapChangesAndDiscontinuitiesStayAtTheirSegmentBoundaries() throws {
        let parsed = try parseHlsMediaPlaylist(playlistUri: "https://fixture.test/index.m3u8", playlistText: """
        #EXTM3U
        #EXT-X-MAP:URI="first.mp4"
        #EXTINF:1,
        one.m4s
        #EXT-X-DISCONTINUITY
        #EXT-X-MAP:URI="second.mp4"
        #EXTINF:1,
        two.m4s
        #EXT-X-ENDLIST
        """)
        let paths = Dictionary(uniqueKeysWithValues: parsed.maps.enumerated().map {
            (hlsByteRangeKey(uri: $0.element.uri, byteRange: $0.element.byteRange), "init-\($0.offset).mp4")
        })
        let text = rewriteHlsMedia(mediaId: "video", playlist: parsed, localMaps: paths)
        let first = try XCTUnwrap(text.range(of: "#EXT-X-MAP:URI=\"init-0.mp4\""))
        let one = try XCTUnwrap(text.range(of: "video-00001.m4s"))
        let discontinuity = try XCTUnwrap(text.range(of: "#EXT-X-DISCONTINUITY"))
        let second = try XCTUnwrap(text.range(of: "#EXT-X-MAP:URI=\"init-1.mp4\""))
        let two = try XCTUnwrap(text.range(of: "video-00002.m4s"))
        XCTAssertLessThan(first.lowerBound, one.lowerBound)
        XCTAssertLessThan(one.lowerBound, discontinuity.lowerBound)
        XCTAssertLessThan(discontinuity.lowerBound, second.lowerBound)
        XCTAssertLessThan(second.lowerBound, two.lowerBound)
    }

    func testEncryptedHlsIsExplicitlyUnsupportedButMethodNoneIsAccepted() throws {
        for directive in ["#EXT-X-KEY:METHOD=AES-128,URI=\"key.bin\"", "#EXT-X-SESSION-KEY:METHOD=SAMPLE-AES,URI=\"key.bin\""] {
            XCTAssertThrowsError(try validateHlsEncryption("#EXTM3U\n" + directive)) { error in
                guard case VesperForegroundDownloadPreparationError.unsupported = error else {
                    return XCTFail("expected unsupported, got \(error)")
                }
            }
        }
        XCTAssertNoThrow(try validateHlsEncryption("#EXTM3U\n#EXT-X-KEY:METHOD=NONE"))
    }

    func testAudioSelectionUsesVariantGroupAndDoesNotInventExternalAudio() throws {
        let text = """
        #EXTM3U
        #EXT-X-MEDIA:TYPE=AUDIO,GROUP-ID="other",LANGUAGE="en",DEFAULT=YES,URI="wrong.m3u8"
        #EXT-X-MEDIA:TYPE=AUDIO,GROUP-ID="selected",LANGUAGE="zh",URI="right.m3u8"
        #EXT-X-STREAM-INF:BANDWIDTH=1000,AUDIO="selected"
        video.m3u8
        """
        let master = parseHlsMasterPlaylist(manifestUri: "https://fixture.test/master.m3u8", manifestText: text)
        let variant = try XCTUnwrap(master.variants.first)
        XCTAssertEqual(selectHlsAudio(master: master, variant: variant, profile: VesperDownloadProfile(preferredAudioLanguage: "en"))?.uri,
                       "https://fixture.test/right.m3u8")
        XCTAssertNil(selectHlsAudio(master: master, variant: HlsVariant(uri: variant.uri, attributes: [:]), profile: VesperDownloadProfile()))
        let inband = parseHlsMasterPlaylist(manifestUri: "https://fixture.test/master.m3u8", manifestText:
            "#EXTM3U\n#EXT-X-MEDIA:TYPE=AUDIO,GROUP-ID=\"selected\",DEFAULT=YES\n#EXT-X-STREAM-INF:BANDWIDTH=1000,AUDIO=\"selected\"\nvideo.m3u8\n")
        XCTAssertNil(selectHlsAudio(master: inband, variant: inband.variants[0], profile: VesperDownloadProfile())?.uri)
    }

    func testDashBaseUrlIsResolvedOnceAndXmlReferencesDecodeOneLayer() throws {
        let selected = selectDashRepresentations(manifestText: """
        <MPD><BaseURL>media/</BaseURL><Period><AdaptationSet mimeType="video/mp4">
        <Representation id="v&amp;1"><BaseURL>clip.mp4?a=1&amp;b=&#50;&amp;literal=&amp;amp;</BaseURL></Representation>
        </AdaptationSet></Period></MPD>
        """, manifestUri: "https://fixture.test/movie/manifest.mpd", profile: VesperDownloadProfile())
        XCTAssertEqual(selected.first?.id, "v&1")
        XCTAssertEqual(selected.first?.baseUrl, "https://fixture.test/movie/media/clip.mp4?a=1&b=2&literal=&amp;")
        XCTAssertEqual(decodeXmlReferences("&lt;&gt;&apos;&quot;&#x1F642;"), "<>'\"🙂")
        XCTAssertEqual(decodeXmlReferences("&amp;amp;"), "&amp;")
        for invalid in ["&unknown;", "&#0;", "&#xD800;", "&#999999999999;", "unfinished&"] {
            XCTAssertNil(decodeXmlReferences(invalid))
        }
    }

    func testDashPeriodBaseUrlIsInheritedBeforeAdaptationAndRepresentation() {
        let selected = selectDashRepresentations(manifestText: """
        <MPD mediaPresentationDuration="PT6S"><BaseURL>movie/</BaseURL><Period><BaseURL>period/</BaseURL>
        <AdaptationSet mimeType="video/mp4"><BaseURL>video/</BaseURL>
        <SegmentTemplate initialization="init.mp4" media="$Number$.m4s" duration="2" timescale="1"/>
        <Representation id="v"><BaseURL>high/</BaseURL></Representation></AdaptationSet></Period></MPD>
        """, manifestUri: "https://fixture.test/root/manifest.mpd", profile: VesperDownloadProfile())
        XCTAssertEqual(selected.count, 1)
        XCTAssertEqual(selected.first?.baseUri, "https://fixture.test/root/movie/period/video/high/")
        XCTAssertEqual(resolveRemoteReference(baseUri: selected.first!.baseUri, reference: selected.first!.template!.initialization!),
                       "https://fixture.test/root/movie/period/video/high/init.mp4")
    }

    func testDashAbsolutePeriodBaseAndSignedRepresentationKeepTheirMeaning() {
        let selected = selectDashRepresentations(manifestText: """
        <MPD><BaseURL>ignored/</BaseURL><Period><BaseURL>https://cdn.fixture.test/period/</BaseURL>
        <AdaptationSet mimeType="video/mp4"><Representation id="v"><BaseURL>clip.mp4?a=1&amp;b=2</BaseURL>
        </Representation></AdaptationSet></Period></MPD>
        """, manifestUri: "https://fixture.test/root/manifest.mpd", profile: VesperDownloadProfile())
        XCTAssertEqual(selected.first?.baseUri, "https://cdn.fixture.test/period/clip.mp4?a=1&b=2")
        XCTAssertEqual(selected.first?.baseUrl, "https://cdn.fixture.test/period/clip.mp4?a=1&b=2")
    }

    func testDashMissingPeriodBaseAndLegacyNoPeriodKeepPreviousResolution() {
        for period in [false, true] {
            let adaptation = "<AdaptationSet mimeType=\"video/mp4\"><Representation id=\"v\"><BaseURL>clip.mp4</BaseURL></Representation></AdaptationSet>"
            let body = period ? "<Period>\(adaptation)</Period>" : adaptation
            let selected = selectDashRepresentations(manifestText: "<MPD><BaseURL>movie/</BaseURL>\(body)</MPD>",
                manifestUri: "https://fixture.test/root/manifest.mpd", profile: VesperDownloadProfile())
            XCTAssertEqual(selected.first?.baseUri, "https://fixture.test/root/movie/clip.mp4")
        }
    }

    func testContentRangeExtremeLengthAndInvalidTotalAreRejectedWithoutTrapping() throws {
        let url = URL(string: "https://fixture.test/media")!
        XCTAssertNil(parseHttpContentRange("bytes 0-18446744073709551615/*")?.length)
        XCTAssertThrowsError(try validateHTTPPartialContentRange(contentRangeHeader: "bytes 0-18446744073709551615/*",
            contentLengthHeader: nil, requestedStart: 0, requestedEndInclusive: nil, expectedBodyLength: nil,
            expectedTotalSizeBytes: nil, sourceURL: url))
        XCTAssertThrowsError(try validateHTTPPartialContentRange(contentRangeHeader: "bytes 0-0/0",
            contentLengthHeader: "1", requestedStart: 0, requestedEndInclusive: 0, expectedBodyLength: 1,
            expectedTotalSizeBytes: nil, sourceURL: url))
    }

    func testAllAssetIndexRebuildsAndPersistencePreserveStreamIdentity() throws {
        let stream = VesperDownloadAssetStream(streamId: "audio-zh", kind: .audio, language: "zh",
            codec: "mp4a", label: "Main audio", resourceIds: ["init"], segmentIds: ["one"], metadata: ["group": "a1"])
        let index = VesperDownloadAssetIndex(contentFormat: .hlsSegments, streams: [stream])
        let rebuilt = [index.compactedForPersistence(), index.withResources([]), index.withCompletedPath("/offline/index.m3u8")]
        for value in rebuilt {
            XCTAssertEqual(value.streams, [stream])
            let encoded = try JSONEncoder().encode(value)
            XCTAssertEqual(try JSONDecoder().decode(VesperDownloadAssetIndex.self, from: encoded).streams, [stream])
        }
    }

    func testSharedHlsMapIsPresentInEachRewrittenMediaPlaylist() async throws {
        let text = """
        #EXTM3U
        #EXT-X-MAP:URI="shared.mp4",BYTERANGE="4@0"
        #EXTINF:1,
        #EXT-X-BYTERANGE:4@4
        shared.mp4
        #EXT-X-ENDLIST
        """
        let video = try parseHlsMediaPlaylist(playlistUri: "https://fixture.test/video.m3u8", playlistText: text)
        let audio = try parseHlsMediaPlaylist(playlistUri: "https://fixture.test/audio.m3u8", playlistText: text)
        let executor = VesperForegroundDownloadExecutor()
        defer { executor.dispose() }
        let index = try await executor.buildHlsMediaAssetIndex(manifestPath: "index.m3u8",
            mediaPlaylists: [("video", video), ("audio", audio)], rootIsMediaPlaylist: false, requestHeaders: [:])
        let maps = index.resources.filter { $0.uri == "https://fixture.test/shared.mp4" }
        XCTAssertEqual(maps.count, 1)
        let mapPath = try XCTUnwrap(maps.first?.relativePath)
        for playlist in ["video.m3u8", "audio.m3u8"] {
            let body = try XCTUnwrap(index.resources.first { $0.relativePath == playlist }?.generatedText)
            XCTAssertTrue(body.contains("#EXT-X-MAP:URI=\"\(mapPath)\""))
        }
    }

    func testPreparedAndMaterializedAssetIndexPreservesSuppliedStreams() async throws {
        let directory = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        defer { try? FileManager.default.removeItem(at: directory) }
        let executor = VesperForegroundDownloadExecutor(baseDirectory: directory)
        defer { executor.dispose() }
        let stream = VesperDownloadAssetStream(streamId: "host-audio", kind: .audio, language: "ja",
            resourceIds: ["manifest"], metadata: ["role": "main"])
        let index = VesperDownloadAssetIndex(contentFormat: .hlsSegments, resources: [
            VesperDownloadResourceRecord(resourceId: "manifest", uri: "vesper-generated://index.m3u8",
                relativePath: "index.m3u8", generatedText: "#EXTM3U\n#EXT-X-ENDLIST\n")
        ], streams: [stream])
        let prepared = try await executor.completePreparedAssetIndex(contentFormat: .hlsSegments,
            assetIndex: index, requestHeaders: [:])
        XCTAssertEqual(prepared.streams, [stream])
        let materialized = try executor.materializeGeneratedResources(assetId: "streams-test", taskId: nil,
            profile: VesperDownloadProfile(), assetIndex: prepared)
        XCTAssertEqual(materialized.streams, [stream])
    }

}
