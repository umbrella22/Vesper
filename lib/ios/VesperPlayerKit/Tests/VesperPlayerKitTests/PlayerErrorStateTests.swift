import AVFoundation
import XCTest
@testable import VesperPlayerKit

@MainActor
final class PlayerErrorStateTests: XCTestCase {
    func testNativeBridgeReportsUnsupportedVideoTrackSelection() {
        let bridge = VesperNativePlayerBridge()
        let missingTrackId = "video:missing"

        bridge.setVideoTrackSelection(.track(missingTrackId))

        XCTAssertEqual(bridge.lastError?.code, .unsupported)
        XCTAssertEqual(bridge.lastError?.category, .capability)
        XCTAssertEqual(bridge.lastError?.retriable, false)
        XCTAssertEqual(
            bridge.lastError?.message,
            "setVideoTrackSelection is not implemented on iOS AVPlayer (mode=track, trackId=\(missingTrackId))"
        )
    }

    func testNativeBridgeReportsUnsupportedFixedTrackAbrWithoutCurrentCatalog() {
        let bridge = VesperNativePlayerBridge()
        let missingTrackId = "video:missing"

        bridge.setAbrPolicy(.fixedTrack(missingTrackId))

        XCTAssertEqual(bridge.lastError?.code, .unsupported)
        XCTAssertEqual(bridge.lastError?.category, .capability)
        XCTAssertEqual(bridge.lastError?.retriable, false)
        XCTAssertEqual(
            bridge.lastError?.message,
            "setAbrPolicy fixedTrack requires a video variant from the current iOS track catalog (trackId=\(missingTrackId))"
        )
    }

    func testNativeBridgeReportsUnsupportedSingleAxisConstrainedAbrWithoutCurrentCatalog() {
        let bridge = VesperNativePlayerBridge()

        bridge.setAbrPolicy(.constrained(maxHeight: 720))

        XCTAssertEqual(bridge.lastError?.code, .unsupported)
        XCTAssertEqual(bridge.lastError?.category, .capability)
        XCTAssertEqual(bridge.lastError?.retriable, false)
        XCTAssertEqual(
            bridge.lastError?.message,
            "setAbrPolicy constrained mode requires a loaded iOS video variant catalog to infer a single-axis maxWidth/maxHeight limit"
        )
    }

    func testMislabeledLocalHlsFileDoesNotExposeAdaptiveVideoWithoutLoadedVariants() async throws {
        let tempUrl = FileManager.default.temporaryDirectory
            .appendingPathComponent(UUID().uuidString)
            .appendingPathExtension("mp4")
        FileManager.default.createFile(atPath: tempUrl.path, contents: Data(), attributes: nil)
        defer { try? FileManager.default.removeItem(at: tempUrl) }

        let source = VesperPlayerSource(
            uri: tempUrl.absoluteString,
            label: "Mislabelled HLS",
            kind: .local,
            protocol: .hls
        )
        let bridge = VesperNativePlayerBridge(initialSource: source)

        bridge.initialize()
        try await settleTrackCatalogRefresh()

        XCTAssertFalse(bridge.trackCatalog.adaptiveVideo)
        XCTAssertTrue(bridge.trackCatalog.videoTracks.isEmpty)
    }

    func testSelectingDashSourceClearsPreviousTrackStateAndUsesDashBridge() throws {
        let tempUrl = FileManager.default.temporaryDirectory
            .appendingPathComponent(UUID().uuidString)
            .appendingPathExtension("mp4")
        FileManager.default.createFile(atPath: tempUrl.path, contents: Data(), attributes: nil)
        defer { try? FileManager.default.removeItem(at: tempUrl) }

        let bridge = VesperNativePlayerBridge(initialSource: .localFile(url: tempUrl, label: "Local"))
        let surface = PlayerSurfaceView(frame: .zero)
        bridge.attachSurfaceHost(surface)
        bridge.initialize()

        XCTAssertNotNil(attachedPlayer(in: surface))

        bridge.selectSource(
            .dash(
                url: URL(string: "https://example.com/playlist.mpd")!,
                label: "DASH"
            )
        )

        XCTAssertNotNil(attachedPlayer(in: surface))
        XCTAssertEqual(bridge.trackCatalog, .empty)
        XCTAssertEqual(bridge.trackSelection, VesperTrackSelectionSnapshot())
        XCTAssertNil(bridge.effectiveVideoTrackId)
        XCTAssertNil(bridge.videoVariantObservation)
        XCTAssertNil(bridge.fixedTrackStatus)
        XCTAssertNil(bridge.lastError)
        XCTAssertEqual(bridge.uiState.sourceLabel, "DASH")
        XCTAssertEqual(bridge.uiState.subtitle, VesperPlayerI18n.nativeRemoteSourceSubtitle("dash"))
    }

    func testStaleStopSeekCompletionDoesNotOverwriteNewSourceStopSeekState() throws {
        let firstUrl = FileManager.default.temporaryDirectory
            .appendingPathComponent(UUID().uuidString)
            .appendingPathExtension("mp4")
        let secondUrl = FileManager.default.temporaryDirectory
            .appendingPathComponent(UUID().uuidString)
            .appendingPathExtension("mp4")
        FileManager.default.createFile(atPath: firstUrl.path, contents: Data(), attributes: nil)
        FileManager.default.createFile(atPath: secondUrl.path, contents: Data(), attributes: nil)
        defer {
            try? FileManager.default.removeItem(at: firstUrl)
            try? FileManager.default.removeItem(at: secondUrl)
        }

        let bridge = VesperNativePlayerBridge(initialSource: .localFile(url: firstUrl, label: "First"))
        bridge.initialize()
        let staleEpoch = bridge.playbackEpochSnapshot()

        bridge.selectSource(.localFile(url: secondUrl, label: "Second"))
        bridge.stop()
        bridge.play()

        XCTAssertEqual(bridge.uiState.sourceLabel, "Second")
        XCTAssertEqual(
            bridge.stopSeekStateSnapshot(),
            StopSeekStateSnapshot(
                isSeekingToStartAfterStop: true,
                pendingPlayAfterStopSeek: true
            )
        )

        bridge.handleStopSeekCompletion(playbackEpoch: staleEpoch)

        XCTAssertEqual(bridge.uiState.sourceLabel, "Second")
        XCTAssertEqual(
            bridge.stopSeekStateSnapshot(),
            StopSeekStateSnapshot(
                isSeekingToStartAfterStop: true,
                pendingPlayAfterStopSeek: true
            )
        )
        XCTAssertNil(bridge.lastError)
    }

    func testStaleRetryTaskDoesNotReinitializeSameUriAfterPolicyReinit() throws {
        let tempUrl = FileManager.default.temporaryDirectory
            .appendingPathComponent(UUID().uuidString)
            .appendingPathExtension("mp4")
        FileManager.default.createFile(atPath: tempUrl.path, contents: Data(), attributes: nil)
        defer { try? FileManager.default.removeItem(at: tempUrl) }

        let bridge = VesperNativePlayerBridge(initialSource: .localFile(url: tempUrl, label: "Local"))
        bridge.initialize()
        let staleEpoch = bridge.playbackEpochSnapshot()

        bridge.setResiliencePolicy(.resilient())
        let currentEpoch = bridge.playbackEpochSnapshot()
        XCTAssertNotEqual(currentEpoch, staleEpoch)

        bridge.handleScheduledRetryFire(
            expectedUri: tempUrl.absoluteString,
            playbackEpoch: staleEpoch,
            attempt: 1,
            delayMs: 500
        )

        XCTAssertEqual(bridge.playbackEpochSnapshot(), currentEpoch)
        XCTAssertEqual(bridge.uiState.sourceLabel, "Local")
        XCTAssertNil(bridge.lastError)
    }

    func testStaleRetryTaskAfterDisposeDoesNotReinitializeBridge() throws {
        let tempUrl = FileManager.default.temporaryDirectory
            .appendingPathComponent(UUID().uuidString)
            .appendingPathExtension("mp4")
        FileManager.default.createFile(atPath: tempUrl.path, contents: Data(), attributes: nil)
        defer { try? FileManager.default.removeItem(at: tempUrl) }

        let bridge = VesperNativePlayerBridge(initialSource: .localFile(url: tempUrl, label: "Local"))
        bridge.initialize()
        let staleEpoch = bridge.playbackEpochSnapshot()

        bridge.dispose()
        let disposedEpoch = bridge.playbackEpochSnapshot()

        bridge.handleScheduledRetryFire(
            expectedUri: tempUrl.absoluteString,
            playbackEpoch: staleEpoch,
            attempt: 1,
            delayMs: 500
        )

        XCTAssertEqual(bridge.playbackEpochSnapshot(), disposedEpoch)
        XCTAssertEqual(bridge.uiState.sourceLabel, "Local")
        XCTAssertNil(bridge.lastError)
    }

    func testNativeBridgeAddsHdrFailureEvidenceToDecodeErrors() {
        let source = VesperPlayerSource.localFile(
            url: URL(fileURLWithPath: "/tmp/local-dv-profile8.mov"),
            label: "DV Profile 8"
        )
        let bridge = VesperNativePlayerBridge(initialSource: source)
        let probeResult = VesperPlaybackCapabilityProbe.probe(
            VesperPlaybackCapabilityProbeRequest(
                source: source,
                codec: "dvhe.08.07"
            )
        )

        bridge.updateCurrentHdrFailureEvidence(probeResult, source: source)
        bridge.handlePlaybackFailureForTesting(
            error: NSError(
                domain: AVFoundationErrorDomain,
                code: AVError.Code.decoderNotFound.rawValue,
                userInfo: [NSLocalizedDescriptionKey: "decoder unavailable"]
            ),
            fallbackMessage: "decoder unavailable",
            itemStatusDetails: playerItemStatusDetailsForTesting(.failed)
        )

        XCTAssertEqual(bridge.lastError?.category, .decode)
        XCTAssertEqual(bridge.lastError?.details["likelyHdrCapabilityIssue"], "true")
        XCTAssertEqual(bridge.lastError?.details["capabilityFailureCause"], "decoderNotFound")
        XCTAssertEqual(bridge.lastError?.details["hdrKind"], "dolbyVision")
        XCTAssertEqual(bridge.lastError?.details["recommendedPlaybackPath"], "systemPlayer")
        XCTAssertEqual(
            bridge.lastError?.details["iosRuntimeEvidenceSource"],
            "hostKitHdrRuntimeFailureEvidence"
        )
        XCTAssertEqual(
            bridge.lastError?.details["avPlayerItemStatusEvidenceSource"],
            "avPlayerItemStatus"
        )
        XCTAssertEqual(bridge.lastError?.details["avPlayerItemStatus"], "failed")
        XCTAssertEqual(bridge.lastError?.details["iosRuntimeFailureCategory"], "decode")
        XCTAssertEqual(bridge.lastError?.details["iosRuntimeFailureRetriable"], "false")
        XCTAssertEqual(bridge.lastError?.details["iosRuntimeFailureCode"], "decodeFailure")
        XCTAssertEqual(bridge.lastError?.details["dolbyVisionProfile"], "8")
        XCTAssertEqual(
            bridge.lastError?.details["dolbyVisionCompatibility"],
            "compatibleBaseLayerCandidate"
        )
        XCTAssertEqual(
            bridge.lastError?.details["dolbyVisionProfileFamily"],
            "profile8SingleLayerCompatible"
        )
        XCTAssertEqual(
            bridge.lastError?.details["dolbyVisionBaseLayer"],
            "compatibleBaseLayerUnknown"
        )
        XCTAssertEqual(
            bridge.lastError?.details["dolbyVisionFallbackTarget"],
            "compatibleBaseLayerSystemPlayer"
        )
        XCTAssertEqual(bridge.lastError?.details["dolbyVisionCodec"], "dvhe.08.07")
    }

    func testNativeBridgeAddsPlayerItemErrorLogDetailsToHdrDecodeErrors() {
        let source = VesperPlayerSource.localFile(
            url: URL(fileURLWithPath: "/tmp/local-dv-profile8.mov"),
            label: "DV Profile 8"
        )
        let bridge = VesperNativePlayerBridge(initialSource: source)
        let probeResult = VesperPlaybackCapabilityProbe.probe(
            VesperPlaybackCapabilityProbeRequest(
                source: source,
                codec: "dvhe.08.07"
            )
        )

        bridge.updateCurrentHdrFailureEvidence(probeResult, source: source)
        bridge.handlePlaybackFailureForTesting(
            error: NSError(
                domain: AVFoundationErrorDomain,
                code: AVError.Code.decoderNotFound.rawValue,
                userInfo: [NSLocalizedDescriptionKey: "decoder unavailable"]
            ),
            fallbackMessage: "decoder unavailable",
            itemErrorLogDetails: playerItemErrorLogDetailsForTesting(
                eventCount: 2,
                uri: "https://media.example.invalid/video/profile8.m3u8",
                serverAddress: "203.0.113.10",
                playbackSessionID: "session-42",
                errorStatusCode: -12906,
                errorDomain: "CoreMediaErrorDomain",
                errorComment: "format description reports unsupported Dolby Vision profile"
            )
        )

        XCTAssertEqual(bridge.lastError?.details["likelyHdrCapabilityIssue"], "true")
        XCTAssertEqual(
            bridge.lastError?.details["avPlayerItemErrorLogEvidenceSource"],
            "avPlayerItemErrorLog"
        )
        XCTAssertEqual(bridge.lastError?.details["avPlayerItemErrorLogEventCount"], "2")
        XCTAssertEqual(bridge.lastError?.details["avPlayerItemErrorStatusCode"], "-12906")
        XCTAssertEqual(bridge.lastError?.details["avPlayerItemErrorDomain"], "CoreMediaErrorDomain")
        XCTAssertEqual(
            bridge.lastError?.details["avPlayerItemErrorComment"],
            "format description reports unsupported Dolby Vision profile"
        )
        XCTAssertEqual(bridge.lastError?.details["avPlayerItemErrorPlaybackSessionID"], "session-42")
    }

    func testNativeBridgeAddsHdrFailureCauseToFormatCapabilityErrors() {
        let source = VesperPlayerSource.localFile(
            url: URL(fileURLWithPath: "/tmp/local-dv-profile8.mov"),
            label: "DV Profile 8"
        )
        let bridge = VesperNativePlayerBridge(initialSource: source)
        let probeResult = VesperPlaybackCapabilityProbe.probe(
            VesperPlaybackCapabilityProbeRequest(
                source: source,
                codec: "dvhe.08.07"
            )
        )

        bridge.updateCurrentHdrFailureEvidence(probeResult, source: source)
        bridge.handlePlaybackFailureForTesting(
            error: NSError(
                domain: AVFoundationErrorDomain,
                code: AVError.Code.fileFormatNotRecognized.rawValue,
                userInfo: [NSLocalizedDescriptionKey: "file format not recognized"]
            ),
            fallbackMessage: "file format not recognized"
        )

        XCTAssertEqual(bridge.lastError?.category, .capability)
        XCTAssertEqual(bridge.lastError?.details["likelyHdrCapabilityIssue"], "true")
        XCTAssertEqual(bridge.lastError?.details["capabilityFailureCause"], "fileFormatNotRecognized")
        XCTAssertEqual(bridge.lastError?.details["hdrKind"], "dolbyVision")
        XCTAssertEqual(bridge.lastError?.details["recommendedPlaybackPath"], "systemPlayer")
    }

    func testNativeBridgeAddsHdrFailureCauseToTemporaryDecoderErrors() {
        let source = VesperPlayerSource.localFile(
            url: URL(fileURLWithPath: "/tmp/local-dv-profile8.mov"),
            label: "DV Profile 8"
        )
        let bridge = VesperNativePlayerBridge(initialSource: source)
        let probeResult = VesperPlaybackCapabilityProbe.probe(
            VesperPlaybackCapabilityProbeRequest(
                source: source,
                codec: "dvhe.08.07"
            )
        )

        bridge.updateCurrentHdrFailureEvidence(probeResult, source: source)
        bridge.handlePlaybackFailureForTesting(
            error: NSError(
                domain: AVFoundationErrorDomain,
                code: AVError.Code.decoderTemporarilyUnavailable.rawValue,
                userInfo: [NSLocalizedDescriptionKey: "decoder temporarily unavailable"]
            ),
            fallbackMessage: "decoder temporarily unavailable"
        )

        XCTAssertEqual(bridge.lastError?.category, .decode)
        XCTAssertEqual(bridge.lastError?.details["likelyHdrCapabilityIssue"], "true")
        XCTAssertEqual(bridge.lastError?.details["capabilityFailureCause"], "decoderTemporarilyUnavailable")
        XCTAssertEqual(bridge.lastError?.details["hdrKind"], "dolbyVision")
        XCTAssertEqual(bridge.lastError?.details["recommendedPlaybackPath"], "systemPlayer")
    }

    func testPlayerItemErrorLogDetailsTruncateLongValues() throws {
        let longComment = String(repeating: "x", count: 300)

        let details = playerItemErrorLogDetailsForTesting(
            eventCount: 1,
            uri: nil,
            serverAddress: nil,
            playbackSessionID: nil,
            errorStatusCode: 500,
            errorDomain: "CoreMediaErrorDomain",
            errorComment: longComment
        )

        let comment = try XCTUnwrap(details["avPlayerItemErrorComment"])
        XCTAssertEqual(comment.count, 256)
        XCTAssertTrue(comment.hasSuffix("..."))
    }

    func testPlayerItemErrorLogDetailsIncludeBoundedRecentEventList() throws {
        let details = playerItemErrorLogDetailsForTesting(
            eventCount: 6,
            events: (1...6).map {
                [
                    "uri": "https://media.example.invalid/\($0).m4s",
                    "serverAddress": "203.0.113.\($0)",
                    "playbackSessionID": "session-\($0)",
                    "errorStatusCode": -12900 - $0,
                    "errorDomain": "CoreMediaErrorDomain",
                    "errorComment": "event-\($0)",
                ]
            }
        )

        XCTAssertEqual(details["avPlayerItemErrorLogEventCount"], "6")
        XCTAssertEqual(details["avPlayerItemErrorLogRecentEventCount"], "5")
        XCTAssertEqual(details["avPlayerItemErrorStatusCode"], "-12906")
        XCTAssertEqual(details["avPlayerItemErrorComment"], "event-6")

        let eventSummary = try XCTUnwrap(details["avPlayerItemErrorLogEvents"])
        let data = try XCTUnwrap(eventSummary.data(using: .utf8))
        let events = try XCTUnwrap(
            JSONSerialization.jsonObject(with: data) as? [[String: Any]]
        )
        XCTAssertEqual(events.count, 5)
        XCTAssertEqual(events.first?["errorComment"] as? String, "event-2")
        XCTAssertEqual(events.last?["errorComment"] as? String, "event-6")
    }

    func testNativeBridgeAddsAssetVideoCombinationEvidenceToHdrDecodeErrors() {
        let source = VesperPlayerSource.localFile(
            url: URL(fileURLWithPath: "/tmp/local-hdr-4k60.mov"),
            label: "HDR 4K60"
        )
        let bridge = VesperNativePlayerBridge(initialSource: source)
        let probeResult = VesperPlaybackCapabilityProbe.withAssetProbeResult(
            VesperPlaybackCapabilityProbe.probe(
                VesperPlaybackCapabilityProbeRequest(source: source)
            ),
            assetProbeResult: VesperPlaybackCapabilityAssetProbeResult(
                isPlayable: true,
                videoTrackCount: 1,
                metadataHdrKind: .hdr10,
                diagnostics: [
                    "assetProbe": "iosAVAsset",
                    "assetVideoTrackCount": "1",
                    "assetVideoCodec": "hvc1",
                    "assetVideoWidth": "3840",
                    "assetVideoHeight": "2160",
                    "assetVideoFrameRate": "59.94",
                    "assetVideoEstimatedDataRate": "25000000",
                    "assetVideoTransferFunction": "SMPTE_ST_2084_PQ",
                ]
            )
        )

        bridge.updateCurrentHdrFailureEvidence(probeResult, source: source)
        bridge.handlePlaybackFailureForTesting(
            error: NSError(
                domain: AVFoundationErrorDomain,
                code: AVError.Code.decoderNotFound.rawValue,
                userInfo: [NSLocalizedDescriptionKey: "decoder unavailable"]
            ),
            fallbackMessage: "decoder unavailable"
        )

        XCTAssertEqual(bridge.lastError?.details["likelyHdrCapabilityIssue"], "true")
        XCTAssertEqual(bridge.lastError?.details["assetVideoTrackCount"], "1")
        XCTAssertEqual(bridge.lastError?.details["assetVideoCodec"], "hvc1")
        XCTAssertEqual(bridge.lastError?.details["assetVideoWidth"], "3840")
        XCTAssertEqual(bridge.lastError?.details["assetVideoHeight"], "2160")
        XCTAssertEqual(bridge.lastError?.details["assetVideoFrameRate"], "59.94")
        XCTAssertEqual(bridge.lastError?.details["assetVideoEstimatedDataRate"], "25000000")
    }

    func testNativeBridgeAddsDisplayEligibilityEvidenceToHdrDecodeErrors() {
        let source = VesperPlayerSource.localFile(
            url: URL(fileURLWithPath: "/tmp/local-hdr-4k120.mov"),
            label: "HDR 4K120"
        )
        let bridge = VesperNativePlayerBridge(initialSource: source)
        let probeResult = VesperPlaybackCapabilityProbe.probe(
            VesperPlaybackCapabilityProbeRequest(
                source: source,
                codec: "hdr10",
                width: 3840,
                height: 2160,
                frameRate: 120
            ),
            sessionProbeProvider: { request in
                VesperIOSSessionProbeProvider.probe(
                    request,
                    environment: VesperIOSSessionProbeEnvironment(
                        displayGamut: .srgb,
                        hdrPlaybackEligible: false,
                        maximumFramesPerSecond: 60,
                        nativeWidth: 1334,
                        nativeHeight: 750
                    )
                )
            }
        )

        bridge.updateCurrentHdrFailureEvidence(probeResult, source: source)
        bridge.handlePlaybackFailureForTesting(
            error: NSError(
                domain: AVFoundationErrorDomain,
                code: AVError.Code.decoderNotFound.rawValue,
                userInfo: [NSLocalizedDescriptionKey: "decoder unavailable"]
            ),
            fallbackMessage: "decoder unavailable"
        )

        XCTAssertEqual(bridge.lastError?.details["likelyHdrCapabilityIssue"], "true")
        XCTAssertEqual(bridge.lastError?.details["hdrKind"], "hdr10")
        XCTAssertEqual(bridge.lastError?.details["confidence"], "sessionProbe")
        XCTAssertEqual(
            bridge.lastError?.details["missingCapabilities"],
            "hdrProgrammableProcessingNotSupported,displayHdrCapability,displayFrameRate"
        )
        XCTAssertEqual(
            bridge.lastError?.details["sessionProbe"],
            "iosDisplayAndPlayerHdrEligibility"
        )
        XCTAssertEqual(bridge.lastError?.details["displayHdrProbeAvailable"], "true")
        XCTAssertEqual(bridge.lastError?.details["displayHdrSupported"], "false")
        XCTAssertEqual(bridge.lastError?.details["displayGamut"], "srgb")
        XCTAssertEqual(bridge.lastError?.details["avPlayerEligibleForHDRPlayback"], "false")
        XCTAssertEqual(bridge.lastError?.details["displayFrameRateSupported"], "false")
        XCTAssertEqual(bridge.lastError?.details["displayMaximumFramesPerSecond"], "60")
        XCTAssertEqual(bridge.lastError?.details["displayNativeWidth"], "1334")
        XCTAssertEqual(bridge.lastError?.details["displayNativeHeight"], "750")
        XCTAssertEqual(bridge.lastError?.details["requestedWidth"], "3840")
        XCTAssertEqual(bridge.lastError?.details["requestedHeight"], "2160")
        XCTAssertEqual(bridge.lastError?.details["requestedFrameRate"], "120.0")
    }

    func testNativeBridgeDoesNotAddHdrEvidenceToNetworkErrors() {
        let source = VesperPlayerSource.localFile(
            url: URL(fileURLWithPath: "/tmp/local-hdr.mov"),
            label: "HDR"
        )
        let bridge = VesperNativePlayerBridge(initialSource: source)
        let probeResult = VesperPlaybackCapabilityProbe.withAssetProbeResult(
            VesperPlaybackCapabilityProbe.probe(
                VesperPlaybackCapabilityProbeRequest(source: source)
            ),
            assetProbeResult: VesperPlaybackCapabilityAssetProbeResult(
                isPlayable: true,
                videoTrackCount: 1,
                metadataHdrKind: .hdr10,
                diagnostics: [
                    "assetProbe": "iosAVAsset",
                    "assetVideoTransferFunction": "SMPTE_ST_2084_PQ",
                ]
            )
        )

        bridge.updateCurrentHdrFailureEvidence(probeResult, source: source)
        bridge.handlePlaybackFailureForTesting(
            error: NSError(
                domain: NSURLErrorDomain,
                code: NSURLErrorNetworkConnectionLost,
                userInfo: [NSLocalizedDescriptionKey: "network lost"]
            ),
            fallbackMessage: "network lost"
        )

        XCTAssertEqual(bridge.lastError?.category, .network)
        XCTAssertNil(bridge.lastError?.details["likelyHdrCapabilityIssue"])
        XCTAssertNil(bridge.lastError?.details["capabilityFailureCause"])
    }
}

@MainActor
private func attachedPlayer(in surface: PlayerSurfaceView) -> AVPlayer? {
    surface.layer.sublayers?
        .compactMap { $0 as? AVPlayerLayer }
        .first?
        .player
}

private func settleTrackCatalogRefresh() async throws {
    for _ in 0..<5 {
        await Task.yield()
    }
    try await Task.sleep(nanoseconds: 100_000_000)
}
