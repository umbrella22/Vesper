import XCTest
@testable import VesperPlayerHostDemo
import VesperPlayerKit

final class ExampleTimelineRegressionTests: XCTestCase {
    func testLiveDvrAcceptanceSourceIsHlsAndQueueable() {
        let source = iosLiveDvrAcceptanceSource()
        XCTAssertEqual(source.uri, IOS_LIVE_DVR_ACCEPTANCE_URL)
        XCTAssertEqual(source.protocol, .hls)

        let queue = examplePlaylistQueue(playlistItemIds: [IOS_LIVE_DVR_PLAYLIST_ITEM_ID])
        XCTAssertEqual(queue.map { $0.itemId }, [IOS_LIVE_DVR_PLAYLIST_ITEM_ID])
        XCTAssertEqual(queue.first?.source.uri, IOS_LIVE_DVR_ACCEPTANCE_URL)
    }

    func testLiveProtocolsUseProgressiveHdrEvidenceClassification() {
        for sourceProtocol in [
            VesperPlayerSourceProtocol.rtmp,
            .rtsp,
            .flv,
        ] {
            let source = VesperPlayerSource(
                uri: "https://example.invalid/live",
                label: sourceProtocol.rawValue,
                kind: .remote,
                protocol: sourceProtocol
            )

            XCTAssertEqual(
                exampleHdrEvidenceSourceKind(source),
                "progressive",
                sourceProtocol.rawValue
            )
            XCTAssertEqual(
                exampleHdrEvidenceManifestKind(source),
                "none",
                sourceProtocol.rawValue
            )
        }
    }

    func testDolbyAcceptanceUrlsFollowOnlineDeliveryKitPatterns() {
        XCTAssertEqual(
            exampleDolbyAcceptanceUrl(
                profile: .p5,
                fps: 25,
                protocol: .hls,
                drmKind: .clear
            ),
            "https://ott.dolby.com/OnDelKits/Dolby_Vision_Online_Delivery_Kit/v1/test_signals/clear/P5_25/master.m3u8"
        )
        XCTAssertEqual(
            exampleDolbyAcceptanceUrl(
                profile: .p81,
                fps: 60,
                protocol: .dash,
                drmKind: .clear
            ),
            "https://ott.dolby.com/OnDelKits/Dolby_Vision_Online_Delivery_Kit/v1/test_signals/clear/P8_1_60/dash.mpd"
        )
        XCTAssertEqual(
            exampleDolbyAcceptanceUrl(
                profile: .p84,
                fps: 30,
                protocol: .hls,
                drmKind: .fairPlay
            ),
            "https://ott.dolby.com/OnDelKits/Dolby_Vision_Online_Delivery_Kit/v1/test_signals/cbcs/P8_4_30/master.m3u8"
        )
    }

    func testDolbyAcceptanceEnablesOnlyClearHlsForIosDirectPlayback() {
        let catalog = buildExampleDolbyAcceptanceCatalog(fairPlayConfiguration: nil)
        let playable = catalog.filter(\.isPlayable)
        XCTAssertFalse(playable.isEmpty)
        XCTAssertTrue(playable.allSatisfy { $0.drmKind == .clear })
        XCTAssertTrue(playable.allSatisfy { $0.sourceProtocol == .hls })
        XCTAssertTrue(playable.allSatisfy { $0.source.drmConfiguration == nil })
    }

    func testDolbyWidevineRemainsPendingAndFairPlayRequiresLocalConfigOnIos() {
        let catalog = buildExampleDolbyAcceptanceCatalog(fairPlayConfiguration: nil)
        let widevine = catalog.first {
            $0.profile == .p5 &&
                $0.fps == 25 &&
                $0.sourceProtocol == .dash &&
                $0.drmKind == .widevinePending
        }
        let fairPlay = catalog.first {
            $0.profile == .p81 &&
                $0.fps == 30 &&
                $0.sourceProtocol == .hls &&
                $0.drmKind == .fairPlay
        }

        XCTAssertEqual(widevine?.isPlayable, false)
        XCTAssertEqual(fairPlay?.isPlayable, false)
        XCTAssertEqual(widevine?.source.uri, "https://ott.dolby.com/OnDelKits/Dolby_Vision_Online_Delivery_Kit/v1/test_signals/cenc/P5_25/dash.mpd")
        XCTAssertEqual(fairPlay?.source.uri, "https://ott.dolby.com/OnDelKits/Dolby_Vision_Online_Delivery_Kit/v1/test_signals/cbcs/P8_1_30/master.m3u8")
        XCTAssertNil(fairPlay?.source.drmConfiguration)
        XCTAssertEqual(fairPlay?.notes.first, "FairPlay config required.")
    }

    func testDolbyFairPlayLocalConfigEnablesHlsPresetsWithDrmConfig() {
        let config = ExampleFairPlayLocalConfiguration(
            licenseUri: "https://license.example.com/fps",
            certificateUri: "https://license.example.com/fps.cer",
            certificateBase64: nil,
            licenseHeaders: ["Authorization": "Bearer fake", "X-Asset": "demo"]
        )
        let catalog = buildExampleDolbyAcceptanceCatalog(fairPlayConfiguration: config)
        let fairPlay = catalog.first {
            $0.profile == .p5 &&
                $0.fps == 25 &&
                $0.sourceProtocol == .hls &&
                $0.drmKind == .fairPlay
        }

        XCTAssertEqual(fairPlay?.isPlayable, true)
        XCTAssertEqual(fairPlay?.source.uri, "https://ott.dolby.com/OnDelKits/Dolby_Vision_Online_Delivery_Kit/v1/test_signals/cbcs/P5_25/master.m3u8")
        XCTAssertEqual(fairPlay?.source.drmConfiguration?.keySystem, "fairPlay")
        XCTAssertEqual(fairPlay?.source.drmConfiguration?.licenseUri, "https://license.example.com/fps")
        XCTAssertEqual(fairPlay?.source.drmConfiguration?.fairPlayCertificateUri, "https://license.example.com/fps.cer")
        XCTAssertEqual(fairPlay?.source.drmConfiguration?.licenseHeaders.count, 2)
        XCTAssertEqual(fairPlay?.notes.first?.contains("license host: license.example.com"), true)
        XCTAssertEqual(fairPlay?.notes.first?.contains("header count: 2"), true)
        XCTAssertEqual(fairPlay?.notes.first?.contains("Bearer fake"), false)
    }

    func testDolbyCatalogFilterMatchesDrmProfileAndFps() {
        let catalog = buildExampleDolbyAcceptanceCatalog(fairPlayConfiguration: nil)
        let filtered = filterDolbyAcceptancePresets(
            catalog,
            drmKind: .clear,
            profile: .p81,
            fps: 60
        )

        XCTAssertEqual(filtered.count, 2)
        XCTAssertTrue(filtered.allSatisfy { $0.drmKind == .clear })
        XCTAssertTrue(filtered.allSatisfy { $0.profile == .p81 })
        XCTAssertTrue(filtered.allSatisfy { $0.fps == 60 })
    }

    func testDolbyPlaylistItemIdResolvesPlayablePresetIntoQueueItem() {
        let preset = buildExampleDolbyAcceptanceCatalog(fairPlayConfiguration: nil).first {
            $0.drmKind == .clear &&
                $0.sourceProtocol == .hls &&
                $0.profile == .p5 &&
                $0.fps == 25
        }!
        let itemId = dolbyPlaylistItemId(preset.id)

        XCTAssertEqual(dolbyPresetIdFromPlaylistItemId(itemId), preset.id)
        let queue = examplePlaylistQueue(playlistItemIds: [itemId])
        XCTAssertEqual(queue.map(\.itemId), [itemId])
        XCTAssertEqual(queue.first?.source.uri, preset.source.uri)
    }

    func testPendingDolbyPresetCannotBeQueued() {
        let catalog = buildExampleDolbyAcceptanceCatalog(fairPlayConfiguration: nil)
        let widevine = catalog.first { $0.drmKind == .widevinePending }!
        let fairPlay = catalog.first { $0.drmKind == .fairPlay }!

        XCTAssertFalse(canQueueDolbyAcceptancePreset(widevine))
        XCTAssertFalse(canQueueDolbyAcceptancePreset(fairPlay))
        XCTAssertTrue(
            examplePlaylistQueue(
                playlistItemIds: [
                    dolbyPlaylistItemId(widevine.id),
                    dolbyPlaylistItemId(fairPlay.id),
                ]
            ).isEmpty
        )
    }

    func testConfiguredFairPlayPresetCanBeQueued() {
        let config = ExampleFairPlayLocalConfiguration(
            licenseUri: "https://license.example.com/fps",
            certificateUri: "https://license.example.com/fps.cer",
            certificateBase64: nil,
            licenseHeaders: [:]
        )
        let fairPlay = buildExampleDolbyAcceptanceCatalog(fairPlayConfiguration: config).first {
            $0.drmKind == .fairPlay &&
                $0.sourceProtocol == .hls
        }!

        XCTAssertTrue(canQueueDolbyAcceptancePreset(fairPlay))
    }

    func testPlayableDolbyPresetsUseDirectNativePluginConfiguration() {
        let config = ExampleFairPlayLocalConfiguration(
            licenseUri: "https://license.example.com/fps",
            certificateUri: "https://license.example.com/fps.cer",
            certificateBase64: nil,
            licenseHeaders: [:]
        )
        let catalog = buildExampleDolbyAcceptanceCatalog(fairPlayConfiguration: config)
        let playable = catalog.filter(\.isPlayable)

        XCTAssertFalse(playable.isEmpty)
        XCTAssertEqual(Set(playable.map(\.profile)), Set(ExampleDolbyAcceptanceProfile.allCases))
        XCTAssertEqual(Set(playable.map(\.drmKind)), [.clear, .fairPlay])
        XCTAssertTrue(playable.allSatisfy { $0.sourceProtocol == .hls })

        for preset in playable {
            let pluginConfiguration = makeExamplePlaybackPluginConfiguration(
                sourceNormalizerSetting: .requireNormalized,
                nativeFramePipelineSetting: .requireNativeFrame,
                sourceNormalizerPluginReferences: [
                    VesperBundledPluginReferences.sourceNormalizerFfmpeg
                ],
                decoderPluginReferences: [VesperBundledPluginReferences.decoderVideoToolbox],
                frameProcessorPluginReferences: [
                    VesperBundledPluginReferences.frameProcessorDiagnostic
                ],
                directNativePlaybackRequired: true
            )

            XCTAssertEqual(pluginConfiguration.sourceNormalizerConfiguration.mode, .disabled, preset.id)
            XCTAssertTrue(pluginConfiguration.sourceNormalizerConfiguration.pluginReferences.isEmpty, preset.id)
            XCTAssertEqual(pluginConfiguration.frameProcessorConfiguration.mode, .disabled, preset.id)
            XCTAssertTrue(pluginConfiguration.frameProcessorConfiguration.pluginReferences.isEmpty, preset.id)
            XCTAssertEqual(pluginConfiguration.nativeFramePipelineConfiguration.mode, .disabled, preset.id)
            XCTAssertTrue(pluginConfiguration.nativeFramePipelineConfiguration.decoderPluginReferences.isEmpty, preset.id)
            XCTAssertTrue(pluginConfiguration.nativeFramePipelineConfiguration.frameProcessorPluginReferences.isEmpty, preset.id)
            XCTAssertNil(pluginConfiguration.nativeFramePipelineConfiguration.maxInFlightFrames, preset.id)
        }
    }

    func testDolbyAdHocOriginDoesNotAdvancePlaylistOnFinished() {
        XCTAssertFalse(
            shouldAdvancePlaylistOnFinished(
                origin: .dolbyAdHoc(presetId: "DOLBY-DV-P5-25-HLS-CLEAR"),
                activeItemId: IOS_HLS_PLAYLIST_ITEM_ID
            )
        )
        XCTAssertFalse(
            shouldAdvancePlaylistOnFinished(
                origin: .queue(itemId: IOS_DASH_PLAYLIST_ITEM_ID),
                activeItemId: IOS_HLS_PLAYLIST_ITEM_ID
            )
        )
        XCTAssertTrue(
            shouldAdvancePlaylistOnFinished(
                origin: .queue(itemId: IOS_HLS_PLAYLIST_ITEM_ID),
                activeItemId: IOS_HLS_PLAYLIST_ITEM_ID
            )
        )
    }

    func testEventLogIsBoundedNewestFirst() {
        let entries = (0..<83).reduce(into: [ExampleHostLogEntry]()) { values, index in
            values = appendExampleHostLogEntry(
                values,
                entry: ExampleHostLogEntry(
                    id: Int64(index),
                    atMillis: Int64(index),
                    severity: .info,
                    title: "entry-\(index)",
                    detail: nil
                )
            )
        }

        XCTAssertEqual(entries.count, EXAMPLE_HOST_LOG_CAPACITY)
        XCTAssertEqual(entries.first?.id, 82)
        XCTAssertEqual(entries.last?.id, 3)
    }

    func testFairPlayLocalConfigReadsEnvironmentWithoutLeakingSecretsIntoSummary() {
        let config = exampleFairPlayLocalConfiguration(
            environment: [
                EXAMPLE_FAIRPLAY_LICENSE_URI_ENV: " https://license.example.com/fps ",
                EXAMPLE_FAIRPLAY_CERTIFICATE_BASE64_ENV: "ZmFrZS1jZXJ0",
                EXAMPLE_FAIRPLAY_LICENSE_HEADERS_JSON_ENV: """
                {"X-Asset":"demo","Ignored":7}
                """,
                EXAMPLE_FAIRPLAY_AUTHORIZATION_ENV: "Bearer fake",
            ]
        )

        XCTAssertEqual(config?.licenseUri, "https://license.example.com/fps")
        XCTAssertEqual(config?.certificateBase64, "ZmFrZS1jZXJ0")
        XCTAssertNil(config?.certificateUri)
        XCTAssertEqual(config?.licenseHeaders.count, 2)
        XCTAssertEqual(config?.summary.contains("license host: license.example.com"), true)
        XCTAssertEqual(config?.summary.contains("header count: 2"), true)
        XCTAssertEqual(config?.summary.contains("Bearer fake"), false)
    }

    func testDolbyHdrEvidencePresetsPreserveProfileFpsProtocolAndDrmMetadata() {
        let preset = buildExampleDolbyAcceptanceCatalog(fairPlayConfiguration: nil).first {
            $0.profile == .p84 &&
                $0.fps == 60 &&
                $0.sourceProtocol == .hls &&
                $0.drmKind == .clear
        }?.toHdrEvidencePreset()

        XCTAssertEqual(preset?.sourceMetadata["hdrKind"] as? String, "dolbyVision")
        XCTAssertEqual(preset?.sourceMetadata["manifestKind"] as? String, "hls")
        XCTAssertEqual(preset?.sourceMetadata["frameRate"] as? Double, 60.0)
        XCTAssertEqual(preset?.sourceMetadata["drmKind"] as? String, "none")
        XCTAssertEqual(preset?.sourceMetadata["manualGate"] as? String, "requiresDolbyVisionDisplay")
        let dolbyVision = preset?.sourceMetadata["dolbyVision"] as? [String: Any]
        XCTAssertEqual(dolbyVision?["profileFamily"] as? String, "profile8.4")
    }

    func testGoLiveFallsBackToSeekableEndForLiveDvr() {
        let timeline = TimelineUiState(
            kind: .liveDvr,
            isSeekable: true,
            seekableRange: SeekableRangeUi(startMs: 10_000, endMs: 60_000),
            liveEdgeMs: nil,
            positionMs: 55_000,
            durationMs: 60_000
        )

        XCTAssertEqual(liveButtonState(timeline), .liveBehind(5_000))
        XCTAssertEqual(
            timelineSummaryState(timeline, pendingSeekRatio: nil),
            .window(positionMs: 45_000, endMs: 50_000)
        )
        XCTAssertEqual(compactTimelineSummary(timeline, pendingSeekRatio: nil), "00:45/00:50")
    }

    func testLiveEdgeToleranceKeepsLiveBadgeActive() {
        let timeline = TimelineUiState(
            kind: .live,
            isSeekable: false,
            seekableRange: nil,
            liveEdgeMs: 120_000,
            positionMs: 119_100,
            durationMs: nil
        )

        XCTAssertEqual(liveButtonState(timeline), .live)
        XCTAssertEqual(
            timelineSummaryState(timeline, pendingSeekRatio: nil),
            .liveEdge(120_000)
        )
        XCTAssertEqual(compactTimelineSummary(timeline, pendingSeekRatio: nil), ExampleI18n.live)
    }

    func testPendingRatioIsClampedToSeekableRange() {
        let timeline = TimelineUiState(
            kind: .liveDvr,
            isSeekable: true,
            seekableRange: SeekableRangeUi(startMs: 30_000, endMs: 90_000),
            liveEdgeMs: 90_000,
            positionMs: 48_000,
            durationMs: 90_000
        )

        XCTAssertEqual(displayedTimelinePositionMs(timeline, pendingSeekRatio: 1.4), 90_000)
        XCTAssertEqual(
            timelineSummaryState(timeline, pendingSeekRatio: 1.4),
            .window(positionMs: 60_000, endMs: 60_000)
        )
        XCTAssertEqual(compactTimelineSummary(timeline, pendingSeekRatio: 1.4), "01:00/01:00")
    }

    func testWindowShrinkClampsStalePositionBeforeRendering() {
        let timeline = TimelineUiState(
            kind: .liveDvr,
            isSeekable: true,
            seekableRange: SeekableRangeUi(startMs: 40_000, endMs: 70_000),
            liveEdgeMs: nil,
            positionMs: 82_000,
            durationMs: 120_000
        )

        XCTAssertEqual(displayedTimelinePositionMs(timeline, pendingSeekRatio: nil), 70_000)
        XCTAssertEqual(liveButtonState(timeline), .live)
        XCTAssertEqual(
            timelineSummaryState(timeline, pendingSeekRatio: nil),
            .window(positionMs: 30_000, endMs: 30_000)
        )
    }

    func testQualityHelpersExposeFixedTrackStateAndObservation() {
        let trackCatalog = VesperTrackCatalog(
            tracks: [
                VesperMediaTrack(
                    id: "video:hls:cavc1:b854000:w854:h480:f3000",
                    kind: .video,
                    bitRate: 854_000,
                    width: 854,
                    height: 480,
                    frameRate: 30
                ),
                VesperMediaTrack(
                    id: "video:hls:cavc1:b1500000:w1280:h720:f3000",
                    kind: .video,
                    bitRate: 1_500_000,
                    width: 1280,
                    height: 720,
                    frameRate: 30
                ),
            ],
            adaptiveVideo: true,
            adaptiveAudio: false
        )
        let trackSelection = VesperTrackSelectionSnapshot(
            abrPolicy: .fixedTrack("video:hls:cavc1:b1500000:w1280:h720:f3000")
        )

        XCTAssertEqual(
            currentFixedTrackStatus(
                trackCatalog,
                trackSelection,
                effectiveVideoTrackId: "video:hls:cavc1:b854000:w854:h480:f3000",
                fixedTrackStatus: .fallback
            ),
            .fallback
        )
        XCTAssertEqual(
            qualityOptionBadgeLabel(
                trackId: "video:hls:cavc1:b1500000:w1280:h720:f3000",
                trackCatalog: trackCatalog,
                trackSelection: trackSelection,
                effectiveVideoTrackId: "video:hls:cavc1:b854000:w854:h480:f3000",
                fixedTrackStatus: .fallback
            ),
            ExampleI18n.qualityStatusFallback
        )
        XCTAssertEqual(
            videoVariantObservationSummary(
                VesperVideoVariantObservation(
                    bitRate: 854_000,
                    width: 854,
                    height: 480
                )
            ),
            "854x480 · 854 kbps"
        )
    }

    func testQualityHelpersKeepFixedTrackPendingWhileRuntimeEvidenceSettles() {
        let requestedTrackId = "video:hls:cavc1:b1500000:w1280:h720:f3000"
        let observedTrackId = "video:hls:cavc1:b854000:w854:h480:f3000"
        let trackCatalog = VesperTrackCatalog(
            tracks: [
                VesperMediaTrack(
                    id: observedTrackId,
                    kind: .video,
                    bitRate: 854_000,
                    width: 854,
                    height: 480,
                    frameRate: 30
                ),
                VesperMediaTrack(
                    id: requestedTrackId,
                    kind: .video,
                    bitRate: 1_500_000,
                    width: 1280,
                    height: 720,
                    frameRate: 30
                ),
            ],
            adaptiveVideo: true,
            adaptiveAudio: false
        )
        let trackSelection = VesperTrackSelectionSnapshot(
            abrPolicy: .fixedTrack(requestedTrackId)
        )

        XCTAssertEqual(
            currentFixedTrackStatus(
                trackCatalog,
                trackSelection,
                effectiveVideoTrackId: observedTrackId,
                fixedTrackStatus: .pending
            ),
            .pending
        )
        XCTAssertEqual(
            qualityOptionBadgeLabel(
                trackId: requestedTrackId,
                trackCatalog: trackCatalog,
                trackSelection: trackSelection,
                effectiveVideoTrackId: observedTrackId,
                fixedTrackStatus: .pending
            ),
            ExampleI18n.qualityStatusPending
        )
        XCTAssertEqual(
            qualityAutoRowSubtitle(
                trackCatalog,
                trackSelection,
                effectiveVideoTrackId: observedTrackId,
                fixedTrackStatus: .pending,
                videoVariantObservation: nil
            ),
            ExampleI18n.qualityFixedSubtitlePending("720p")
        )
    }

    func testNativeFrameDiagnosticDetailsExposeClockAudioSeekAndIssue() {
        let summary = nativeFrameDiagnosticDetails([
            "clockSource": "swiftNativeAudioBridge",
            "audioDecoder": "swiftNativeAudioBridge",
            "audioOutput": "swiftNativeAudioBridge",
            "audioPipeline": "swiftNativeAudioBridgeV1",
            "audioRateControl": "swiftNativeAudioBridgeTimePitch",
            "selectedVideoStreamIndex": 0,
            "selectedVideoMediaKind": "video",
            "audioStreamIndex": 1,
            "audioMediaKind": "audio",
            "seekable": true,
            "fallbackTargetRoute": "systemPlayer",
            "issueKind": "missingSurface",
        ])

        XCTAssertEqual(
            summary,
            "clock=swiftNativeAudioBridge · audioDecoder=swiftNativeAudioBridge · audio=swiftNativeAudioBridge · audioPipeline=swiftNativeAudioBridgeV1 · rateControl=swiftNativeAudioBridgeTimePitch · video=video#0 · audioTrack=audio#1 · seekable=true · fallbackTarget=systemPlayer · issue=missingSurface"
        )
    }

    func testPluginDiagnosticCountersExposeNativeFramePacketSkips() {
        let counters = pluginDiagnosticCounters([
            "processedFrames": 3,
            "presentedFrames": 2,
            "deadlineMisses": 1,
            "backpressureCount": 4,
            "lateDropped": 5,
            "skippedAudioPackets": 6,
            "skippedVideoPackets": 7,
            "skippedOtherPackets": 8,
        ])

        XCTAssertEqual(
            counters,
            "processed 3 · presented 2 · deadline 1 · backpressure 4 · late 5 · skipAudio 6 · skipVideo 7 · skipOther 8"
        )
    }

    func testPluginLabCopyFramesNativeFrameAsExplicitHardwareFirstRoute() {
        let englishBundle = Bundle.main
            .path(forResource: "en", ofType: "lproj")
            .flatMap(Bundle.init(path:))
        XCTAssertNotNil(englishBundle)
        let subtitle = englishBundle?.localizedString(
            forKey: "example.plugins.subtitle",
            value: nil,
            table: "Localizable"
        ) ?? ""
        let preferSubtitle = englishBundle?.localizedString(
            forKey: "example.plugins.native_frame.prefer_subtitle",
            value: nil,
            table: "Localizable"
        ) ?? ""
        let requireSubtitle = englishBundle?.localizedString(
            forKey: "example.plugins.native_frame.require_subtitle",
            value: nil,
            table: "Localizable"
        ) ?? ""
        let localizedSubtitle = ExampleI18n.pluginDiagnosticsSubtitle
        let localizedPreferSubtitle = ExampleNativeFramePipelineSetting.preferNativeFrame.subtitle
        let localizedRequireSubtitle = ExampleNativeFramePipelineSetting.requireNativeFrame.subtitle

        XCTAssertTrue(subtitle.contains("Default playback remains AVPlayer"))
        XCTAssertTrue(subtitle.contains("explicit"))
        XCTAssertTrue(subtitle.contains("hardware-first"))
        XCTAssertTrue(subtitle.contains("SourceNormalizer packets"))
        XCTAssertTrue(subtitle.contains("VideoToolbox"))
        XCTAssertTrue(subtitle.contains("Metal presentation"))
        XCTAssertFalse(subtitle.localizedCaseInsensitiveContains("soft decode"))
        XCTAssertFalse(subtitle.localizedCaseInsensitiveContains("software decoder"))

        XCTAssertTrue(preferSubtitle.contains("VideoToolbox hardware decode"))
        XCTAssertTrue(preferSubtitle.contains("fall back to AVPlayer"))
        XCTAssertTrue(requireSubtitle.contains("Strict diagnostic mode"))
        XCTAssertTrue(requireSubtitle.contains("downgrade to Prefer"))
        XCTAssertTrue(requireSubtitle.contains("fall back to AVPlayer"))

        XCTAssertTrue(localizedSubtitle.contains("AVPlayer"))
        XCTAssertTrue(localizedSubtitle.contains("SourceNormalizer"))
        XCTAssertTrue(localizedSubtitle.contains("VideoToolbox"))
        XCTAssertTrue(localizedSubtitle.contains("Metal"))
        XCTAssertFalse(localizedSubtitle.localizedCaseInsensitiveContains("soft decode"))
        XCTAssertFalse(localizedSubtitle.localizedCaseInsensitiveContains("software decoder"))
        XCTAssertTrue(localizedPreferSubtitle.contains("VideoToolbox"))
        XCTAssertTrue(localizedPreferSubtitle.contains("AVPlayer"))
        XCTAssertTrue(localizedRequireSubtitle.contains("native frame"))
        XCTAssertTrue(localizedRequireSubtitle.contains("Prefer"))
        XCTAssertTrue(localizedRequireSubtitle.contains("AVPlayer"))
    }
}
