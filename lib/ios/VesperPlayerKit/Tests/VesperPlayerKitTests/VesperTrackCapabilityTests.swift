import XCTest
@testable import VesperPlayerKit

@MainActor
final class VesperTrackCapabilityTests: XCTestCase {
    func testCatalogPublicationAddsUnknownSystemPlayerContextAndMonotonicRevision() {
        let bridge = VesperNativePlayerBridge()
        bridge.currentSource = VesperPlayerSource.hls(
            url: URL(string: "https://example.com/video.m3u8")!,
            label: "Fixture"
        )
        bridge.subtitleSourceEpoch = 1
        let track = VesperMediaTrack(
            id: "video:720p",
            kind: .video,
            width: 1280,
            height: 720
        )

        bridge.publishTrackCatalog(
            VesperTrackCatalog(tracks: [track], adaptiveVideo: true),
            playbackPath: "systemPlayer"
        )
        let first = bridge.trackCatalog
        bridge.publishTrackCatalog(
            VesperTrackCatalog(tracks: [track], adaptiveVideo: true),
            playbackPath: "systemPlayer"
        )

        XCTAssertEqual(first.catalogRevision, bridge.trackCatalog.catalogRevision)
        XCTAssertEqual(first.playbackPath, "systemPlayer")
        XCTAssertEqual(first.videoTracks.first?.support.status, .unknown)
        XCTAssertEqual(first.videoTracks.first?.support.reason, .platformUnknown)
        XCTAssertEqual(first.videoTracks.first?.support.source, .runtimeTrackCatalog)
        XCTAssertEqual(first.videoTracks.first?.support.playbackPath, "systemPlayer")

        bridge.publishTrackCatalog(
            VesperTrackCatalog(tracks: [track], adaptiveVideo: true),
            playbackPath: "sdkManagedNativeFrame"
        )
        XCTAssertGreaterThan(bridge.trackCatalog.catalogRevision, first.catalogRevision)
        XCTAssertEqual(bridge.trackCatalog.playbackPath, "sdkManagedNativeFrame")
        XCTAssertEqual(
            bridge.trackCatalog.videoTracks.first?.support.playbackPath,
            "sdkManagedNativeFrame"
        )
    }

    func testFixedTrackRejectsStaleCatalogBeforeChangingSelectionState() {
        let bridge = VesperNativePlayerBridge()
        bridge.currentSource = VesperPlayerSource.hls(
            url: URL(string: "https://example.com/video.m3u8")!,
            label: "Fixture"
        )
        bridge.subtitleSourceEpoch = 1
        let track = VesperMediaTrack(
            id: "video:720p",
            kind: .video,
            width: 1280,
            height: 720
        )
        bridge.publishTrackCatalog(
            VesperTrackCatalog(tracks: [track]),
            playbackPath: "systemPlayer"
        )
        bridge.videoVariantPinsByTrackId[track.id] = LoadedVideoVariantPin(
            peakBitRate: 1_000_000,
            maxWidth: 1280,
            maxHeight: 720
        )
        let before = bridge.publishedTrackSelection

        XCTAssertThrowsError(
            try bridge.setAbrPolicy(
                .fixedTrack(track.id),
                expectedCatalogRevision: bridge.trackCatalog.catalogRevision - 1
        )
        ) { error in
            let rejection = error as? VesperFixedTrackSelectionError
            XCTAssertEqual(rejection?.code, "staleCatalog")
            XCTAssertEqual(rejection?.trackId, track.id)
        }
        XCTAssertEqual(bridge.publishedTrackSelection, before)
        XCTAssertNil(bridge.desiredVideoVariantPin)
    }

    func testFixedTrackCapabilityRejectionsPreserveEvidence() {
        let support = VesperTrackSupport(
            status: .exceedsCapabilities,
            reason: .formatExceedsCapabilities,
            source: .runtimeTrackCatalog,
            formatSupportRawValue: "exceedsCapabilities"
        )
        let track = VesperMediaTrack(
            id: "video:4k",
            kind: .video,
            width: 3840,
            height: 2160,
            support: support
        )
        let bridge = VesperNativePlayerBridge()
        bridge.currentSource = VesperPlayerSource.hls(
            url: URL(string: "https://example.com/video.m3u8")!,
            label: "Fixture"
        )
        bridge.subtitleSourceEpoch = 1
        bridge.publishTrackCatalog(
            VesperTrackCatalog(tracks: [track]),
            playbackPath: "systemPlayer"
        )
        bridge.videoVariantPinsByTrackId[track.id] = LoadedVideoVariantPin(
            peakBitRate: 8_000_000,
            maxWidth: 3840,
            maxHeight: 2160
        )
        let before = bridge.publishedTrackSelection

        XCTAssertThrowsError(
            try bridge.setAbrPolicy(
                .fixedTrack(track.id),
                expectedCatalogRevision: bridge.trackCatalog.catalogRevision
            )
        ) { error in
            let rejection = error as? VesperFixedTrackSelectionError
            XCTAssertEqual(rejection?.code, "trackExceedsCapabilities")
            XCTAssertEqual(rejection?.details["reason"], "formatExceedsCapabilities")
            XCTAssertEqual(rejection?.details["formatSupportRawValue"], "exceedsCapabilities")
        }
        XCTAssertEqual(bridge.publishedTrackSelection, before)
    }
}
