import CoreGraphics
import Foundation
import XCTest
@testable import VesperPlayerKit

final class VesperAbrStabilityTests: XCTestCase {
    func testFixedTrackAbrRestoreWaitsForLoadedVariantCatalog() {
        XCTAssertTrue(abrPolicyRequiresLoadedVideoVariantCatalog(.fixedTrack("video:any")))
        XCTAssertTrue(abrPolicyRequiresLoadedVideoVariantCatalog(.constrained(maxWidth: 1280)))
        XCTAssertTrue(abrPolicyRequiresLoadedVideoVariantCatalog(.constrained(maxHeight: 720)))
        XCTAssertFalse(abrPolicyRequiresLoadedVideoVariantCatalog(.auto()))
        XCTAssertFalse(
            abrPolicyRequiresLoadedVideoVariantCatalog(
                .constrained(maxBitRate: 2_500_000)
            )
        )
        XCTAssertFalse(
            abrPolicyRequiresLoadedVideoVariantCatalog(
                .constrained(maxBitRate: 2_500_000, maxWidth: 1920, maxHeight: 1080)
            )
        )
    }

    func testStableVideoVariantTrackIdUsesVariantFingerprint() {
        let first = stableVideoVariantTrackId(
            codec: "avc1.640028",
            peakBitRate: 2_500_000,
            width: 1920,
            height: 1080,
            frameRate: 29.97
        )
        let equivalent = stableVideoVariantTrackId(
            codec: "avc1.640028",
            peakBitRate: 2_500_000,
            width: 1920,
            height: 1080,
            frameRate: 29.969
        )
        let differentBitRate = stableVideoVariantTrackId(
            codec: "avc1.640028",
            peakBitRate: 4_000_000,
            width: 1920,
            height: 1080,
            frameRate: 29.97
        )

        XCTAssertEqual(first, equivalent)
        XCTAssertNotEqual(first, differentBitRate)
        XCTAssertEqual(first, "video:hls:cavc1_640028:b2500000:w1920:h1080:f2997")
    }

    func testResolveRequestedVideoVariantTrackIdPrefersExactTrackId() {
        let exactTrackId = "video:hls:cavc1:b2500000:w1920:h1080:f3000"
        let resolved = resolveRequestedVideoVariantTrackId(
            exactTrackId,
            tracks: sampleVideoTracks
        )

        XCTAssertEqual(resolved, exactTrackId)
    }

    func testResolveRequestedVideoVariantTrackIdRemapsNearEquivalentVariant() {
        let requestedTrackId = stableVideoVariantTrackId(
            codec: "avc1.640028",
            peakBitRate: 2_500_000,
            width: 1920,
            height: 1080,
            frameRate: 29.97
        )
        let remappedTrackId = stableVideoVariantTrackId(
            codec: "avc1.640028",
            peakBitRate: 2_400_000,
            width: 1920,
            height: 1080,
            frameRate: 30.0
        )
        let resolved = resolveRequestedVideoVariantTrackId(
            requestedTrackId,
            tracks: [
                VesperMediaTrack(
                    id: "video:hls:cavc1_640028:b1500000:w1280:h720:f3000",
                    kind: .video,
                    codec: "avc1.640028",
                    bitRate: 1_500_000,
                    width: 1280,
                    height: 720,
                    frameRate: 30
                ),
                VesperMediaTrack(
                    id: remappedTrackId,
                    kind: .video,
                    codec: "avc1.640028",
                    bitRate: 2_400_000,
                    width: 1920,
                    height: 1080,
                    frameRate: 30
                ),
            ]
        )

        XCTAssertEqual(resolved, remappedTrackId)
    }

    func testResolveRequestedVideoVariantTrackIdRejectsLegacyUnparseableTrackId() {
        XCTAssertNil(
            resolveRequestedVideoVariantTrackId(
                "video:legacy:0",
                tracks: sampleVideoTracks
            )
        )
    }

    func testResolveFixedTrackStatusReturnsPendingWithoutObservedVariant() {
        XCTAssertEqual(
            resolveFixedTrackStatus(
                abrPolicy: .fixedTrack("video:hls:cavc1:b1500000:w1280:h720:f3000"),
                effectiveVideoTrackId: nil,
                tracks: sampleVideoTracks
            ),
            .pending
        )
    }

    func testResolveFixedTrackStatusReturnsLockedWhenObservedVariantMatches() {
        XCTAssertEqual(
            resolveFixedTrackStatus(
                abrPolicy: .fixedTrack("video:hls:cavc1:b1500000:w1280:h720:f3000"),
                effectiveVideoTrackId: "video:hls:cavc1:b1500000:w1280:h720:f3000",
                tracks: sampleVideoTracks
            ),
            .locked
        )
    }

    func testResolveFixedTrackStatusReturnsFallbackWhenObservedVariantDiffers() {
        XCTAssertEqual(
            resolveFixedTrackStatus(
                abrPolicy: .fixedTrack("video:hls:cavc1:b1500000:w1280:h720:f3000"),
                effectiveVideoTrackId: "video:hls:cavc1:b854000:w854:h480:f3000",
                tracks: sampleVideoTracks
            ),
            .fallback
        )
    }

    func testResolveVideoVariantObservationUsesBitRateAndPresentationSize() {
        let observation = resolveVideoVariantObservation(
            bitRate: 1_500_000,
            presentationSize: CGSize(width: 1280, height: 720)
        )

        XCTAssertEqual(
            observation,
            VesperVideoVariantObservation(
                bitRate: 1_500_000,
                width: 1280,
                height: 720
            )
        )
    }

    func testResolveVideoVariantObservationReturnsNilWithoutRuntimeEvidence() {
        XCTAssertNil(
            resolveVideoVariantObservation(
                bitRate: nil,
                presentationSize: nil
            )
        )
    }

    func testResolveFixedTrackRecoveryPolicyUsesRequestedTrackLimits() {
        let policy = resolveFixedTrackRecoveryPolicy(
            requestedTrackId: "video:hls:cavc1:b1500000:w1280:h720:f3000",
            tracks: sampleVideoTracks
        )

        XCTAssertEqual(
            policy,
            .constrained(
                maxBitRate: 1_500_000,
                maxWidth: 1280,
                maxHeight: 720
            )
        )
    }

    func testResolveFixedTrackRecoveryPolicyFallsBackToAutoWithoutMatchingTrack() {
        XCTAssertEqual(
            resolveFixedTrackRecoveryPolicy(
                requestedTrackId: "video:hls:missing",
                tracks: sampleVideoTracks
            ),
            .auto()
        )
    }

    func testShouldEscalatePersistentFixedTrackFallbackRequiresStablePlaybackEvidence() {
        XCTAssertFalse(
            shouldEscalatePersistentFixedTrackFallback(
                status: .fallback,
                observation: VesperVideoVariantObservation(
                    bitRate: 854_000,
                    width: 854,
                    height: 480
                ),
                playbackState: .ready,
                isBuffering: false,
                elapsed: 3.0
            )
        )
        XCTAssertFalse(
            shouldEscalatePersistentFixedTrackFallback(
                status: .fallback,
                observation: VesperVideoVariantObservation(
                    bitRate: 854_000,
                    width: 854,
                    height: 480
                ),
                playbackState: .playing,
                isBuffering: true,
                elapsed: 3.0
            )
        )
        XCTAssertFalse(
            shouldEscalatePersistentFixedTrackFallback(
                status: .fallback,
                observation: VesperVideoVariantObservation(
                    bitRate: 854_000,
                    width: 854,
                    height: 480
                ),
                playbackState: .playing,
                isBuffering: false,
                elapsed: 1.0
            )
        )
        XCTAssertTrue(
            shouldEscalatePersistentFixedTrackFallback(
                status: .fallback,
                observation: VesperVideoVariantObservation(
                    bitRate: 854_000,
                    width: 854,
                    height: 480
                ),
                playbackState: .playing,
                isBuffering: false,
                elapsed: 2.5
            )
        )
    }

    func testConstrainedMaximumResolutionInfersWidthFromHeight() {
        let resolved = resolveConstrainedMaximumVideoResolution(
            maxWidth: nil,
            maxHeight: 720,
            tracks: sampleVideoTracks
        )

        XCTAssertEqual(
            resolved,
            ResolvedMaximumVideoResolution(width: 1280, height: 720)
        )
    }

    func testConstrainedMaximumResolutionInfersHeightFromWidth() {
        let resolved = resolveConstrainedMaximumVideoResolution(
            maxWidth: 640,
            maxHeight: nil,
            tracks: sampleVideoTracks
        )

        XCTAssertEqual(
            resolved,
            ResolvedMaximumVideoResolution(width: 640, height: 360)
        )
    }

    func testConstrainedMaximumResolutionRequiresCatalogForSingleAxisConstraint() {
        XCTAssertNil(
            resolveConstrainedMaximumVideoResolution(
                maxWidth: nil,
                maxHeight: 720,
                tracks: []
            )
        )
    }

    func testConstrainedMaximumResolutionRejectsNonPositiveAxisValues() {
        XCTAssertNil(
            resolveConstrainedMaximumVideoResolution(
                maxWidth: 0,
                maxHeight: 720,
                tracks: sampleVideoTracks
            )
        )
        XCTAssertNil(
            resolveConstrainedMaximumVideoResolution(
                maxWidth: 1280,
                maxHeight: 0,
                tracks: sampleVideoTracks
            )
        )
    }
}

private let sampleVideoTracks: [VesperMediaTrack] = [
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
    VesperMediaTrack(
        id: "video:hls:cavc1:b2500000:w1920:h1080:f3000",
        kind: .video,
        bitRate: 2_500_000,
        width: 1920,
        height: 1080,
        frameRate: 30
    ),
]
