import XCTest
@testable import VesperPlayerKit

final class VesperDashBridgeParserTests: XCTestCase {
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

    func testDashNetworkClientRejectsInsecureHTTPBeforeATS() async {
        let client = VesperDashNetworkClient()

        do {
            _ = try await client.data(for: URL(string: "http://cdn.example.com/master.mpd")!)
            XCTFail("insecure DASH HTTP request should fail")
        } catch {
            XCTAssertTrue(error.localizedDescription.contains("App Transport Security"))
            XCTAssertTrue(error.localizedDescription.contains("http://cdn.example.com/master.mpd"))
        }
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

    func testSidxParserMapsInvalidMp4Errors() {
        let truncated = Data([0, 0, 0, 16, 0x73, 0x69, 0x64, 0x78])

        XCTAssertThrowsError(try VesperDashSidxParser.parse(data: truncated)) { error in
            guard case VesperDashBridgeError.invalidMp4 = error else {
                XCTFail("unexpected error \(error)")
                return
            }
        }
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
}
