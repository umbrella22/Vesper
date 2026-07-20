@preconcurrency import AVFoundation
import XCTest
@testable import VesperPlayerKit

final class VesperSubtitleDeviceAcceptanceTests: XCTestCase {
    func testDashWebVttPublishesLegibleGroupOnPhysicalDevice() async throws {
#if targetEnvironment(simulator)
        throw XCTSkip("Physical iOS device required for AVPlayer subtitle acceptance")
#else
        let directory = FileManager.default.temporaryDirectory
            .appendingPathComponent(UUID().uuidString, isDirectory: true)
        try FileManager.default.createDirectory(
            at: directory,
            withIntermediateDirectories: true
        )
        defer { try? FileManager.default.removeItem(at: directory) }

        try Data(deviceWebVttMpd.utf8).write(
            to: directory.appendingPathComponent("manifest.mpd")
        )
        let segment = Data(
            "WEBVTT\n\n00:00:00.000 --> 00:00:02.000\ndevice subtitle proof\n".utf8
        )
        for number in 1...3 {
            try segment.write(
                to: directory.appendingPathComponent("sub-\(number).vtt")
            )
        }

        let session = VesperDashSession(sourceURL: directory.appendingPathComponent("manifest.mpd"))
        let loaderDelegate = VesperDashResourceLoaderDelegate(session: session)
        let asset = AVURLAsset(url: session.masterPlaylistURL)
        asset.resourceLoader.setDelegate(
            loaderDelegate,
            queue: loaderDelegate.resourceLoadingQueue
        )

        let group = try await loadLegibleGroupWithTimeout(from: asset)
        XCTAssertFalse(
            group.options.isEmpty,
            "AVPlayer must publish a legible option for the DASH WebVTT rendition"
        )
#endif
    }

    @MainActor
    func testDashWebVttCueIsDeliveredAfterSeekOnPhysicalDevice() async throws {
#if targetEnvironment(simulator)
        throw XCTSkip("Physical iOS device required for AVPlayer subtitle acceptance")
#else
        let directory = FileManager.default.temporaryDirectory
            .appendingPathComponent(UUID().uuidString, isDirectory: true)
        try FileManager.default.createDirectory(
            at: directory,
            withIntermediateDirectories: true
        )
        defer { try? FileManager.default.removeItem(at: directory) }

        try Data(deviceWebVttMpd.utf8).write(
            to: directory.appendingPathComponent("manifest.mpd")
        )
        let segment = Data(
            "WEBVTT\n\n00:00:00.000 --> 00:00:02.000\ndevice subtitle proof\n".utf8
        )
        for number in 1...3 {
            try segment.write(
                to: directory.appendingPathComponent("sub-\(number).vtt")
            )
        }

        let session = VesperDashSession(
            sourceURL: directory.appendingPathComponent("manifest.mpd")
        )
        let loaderDelegate = VesperDashResourceLoaderDelegate(session: session)
        let subtitleAsset = AVURLAsset(
            url: session.segmentURL(
                for: "sub-en",
                segment: .media(0),
                fileExtension: "vtt"
            )
        )
        subtitleAsset.resourceLoader.setDelegate(
            loaderDelegate,
            queue: loaderDelegate.resourceLoadingQueue
        )

        var subtitleTracks = try await subtitleAsset.loadTracks(withMediaType: .text)
        if subtitleTracks.isEmpty {
            subtitleTracks = try await subtitleAsset.loadTracks(withMediaType: .subtitle)
        }
        let subtitleTrack = try XCTUnwrap(
            subtitleTracks.first,
            "AVFoundation must expose the vesper-dash WebVTT resource as a legible track"
        )
        let mediaURL = try XCTUnwrap(
            Bundle(for: Self.self).url(
                forResource: "tiny-h264-aac",
                withExtension: "m4v"
            )
        )
        let mediaAsset = AVURLAsset(url: mediaURL)
        let composition = AVMutableComposition()
        for mediaType in [AVMediaType.video, .audio] {
            guard let sourceTrack = try await mediaAsset.loadTracks(withMediaType: mediaType).first else {
                continue
            }
            let targetTrack = try XCTUnwrap(
                composition.addMutableTrack(
                    withMediaType: mediaType,
                    preferredTrackID: kCMPersistentTrackID_Invalid
                )
            )
            try targetTrack.insertTimeRange(
                try await sourceTrack.load(.timeRange),
                of: sourceTrack,
                at: .zero
            )
        }
        let compositionSubtitleTrack = try XCTUnwrap(
            composition.addMutableTrack(
                withMediaType: subtitleTrack.mediaType,
                preferredTrackID: kCMPersistentTrackID_Invalid
            )
        )
        try compositionSubtitleTrack.insertTimeRange(
            try await subtitleTrack.load(.timeRange),
            of: subtitleTrack,
            at: .zero
        )

        let cueDelivered = expectation(description: "WebVTT cue delivered after seek")
        var didDeliverExpectedCue = false
        let probe = DeviceLegibleOutputProbe { strings in
            if !didDeliverExpectedCue,
               strings.contains(where: { $0.string.contains("device subtitle proof") }) {
                didDeliverExpectedCue = true
                cueDelivered.fulfill()
            }
        }
        let item = AVPlayerItem(asset: composition)
        let output = AVPlayerItemLegibleOutput()
        output.suppressesPlayerRendering = true
        output.setDelegate(probe, queue: .main)
        item.add(output)
        let player = AVPlayer(playerItem: item)
        defer {
            player.pause()
            item.remove(output)
        }

        let seekCompleted = await withCheckedContinuation { continuation in
            player.seek(
                to: CMTime(seconds: 1, preferredTimescale: 600),
                toleranceBefore: .zero,
                toleranceAfter: .zero
            ) { completed in
                continuation.resume(returning: completed)
            }
        }
        XCTAssertTrue(seekCompleted, "AVPlayer must complete the seek into the active cue interval")
        player.play()
        await fulfillment(of: [cueDelivered], timeout: 10)
#endif
    }

    private func loadLegibleGroupWithTimeout(
        from asset: AVURLAsset
    ) async throws -> AVMediaSelectionGroup {
        try await withThrowingTaskGroup(of: AVMediaSelectionGroup.self) { group in
            group.addTask {
                guard let result = try await asset.loadMediaSelectionGroup(for: .legible) else {
                    throw DeviceAcceptanceError.legibleGroupMissing
                }
                return result
            }
            group.addTask {
                try await Task.sleep(for: .seconds(15))
                throw DeviceAcceptanceError.timedOut
            }
            guard let result = try await group.next() else {
                throw DeviceAcceptanceError.legibleGroupMissing
            }
            group.cancelAll()
            return result
        }
    }
}

private final class DeviceLegibleOutputProbe: NSObject, AVPlayerItemLegibleOutputPushDelegate {
    private let didOutput: ([NSAttributedString]) -> Void

    init(didOutput: @escaping ([NSAttributedString]) -> Void) {
        self.didOutput = didOutput
    }

    func legibleOutput(
        _ output: AVPlayerItemLegibleOutput,
        didOutputAttributedStrings strings: [NSAttributedString],
        nativeSampleBuffers: [Any],
        forItemTime itemTime: CMTime
    ) {
        didOutput(strings)
    }
}

private enum DeviceAcceptanceError: LocalizedError {
    case legibleGroupMissing
    case timedOut

    var errorDescription: String? {
        switch self {
        case .legibleGroupMissing:
            "AVPlayer did not publish a legible media selection group"
        case .timedOut:
            "Timed out waiting for AVPlayer to publish a legible media selection group"
        }
    }
}

private let deviceWebVttMpd = #"""
<?xml version="1.0" encoding="UTF-8"?>
<MPD type="static" mediaPresentationDuration="PT6S" minBufferTime="PT2S">
  <Period id="period0">
    <AdaptationSet mimeType="video/mp4" segmentAlignment="true">
      <SegmentTemplate timescale="1000" initialization="init-$RepresentationID$.mp4" media="video-$Number$.m4s" startNumber="1" duration="2000"/>
      <Representation id="v1" bandwidth="800000" codecs="avc1.64001f" width="1280" height="720"/>
    </AdaptationSet>
    <AdaptationSet id="subs" contentType="text" mimeType="text/vtt" lang="en">
      <Label>English</Label>
      <Role schemeIdUri="urn:mpeg:dash:role:2011" value="main"/>
      <SegmentTemplate timescale="1000" media="sub-$Number$.vtt" startNumber="1" duration="2000"/>
      <Representation id="sub-en" bandwidth="1200" codecs="wvtt"/>
    </AdaptationSet>
  </Period>
</MPD>
"""#
