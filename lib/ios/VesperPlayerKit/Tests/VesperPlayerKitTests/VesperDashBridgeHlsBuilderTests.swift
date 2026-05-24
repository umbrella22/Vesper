import XCTest
@testable import VesperPlayerKit

final class VesperDashBridgeHlsBuilderTests: XCTestCase {
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
