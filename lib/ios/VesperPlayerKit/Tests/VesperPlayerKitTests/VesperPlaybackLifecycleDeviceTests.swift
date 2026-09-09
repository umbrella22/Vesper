@preconcurrency import AVFoundation
import UIKit
import XCTest
@testable import VesperPlayerKit

final class VesperPlaybackLifecycleDeviceTests: XCTestCase {
    private let hlsVodURL = URL(
        string: "https://devstreaming-cdn.apple.com/videos/streaming/examples/img_bipbop_adv_example_ts/master.m3u8"
    )!
    private let dashVodURL = URL(
        string: "https://dash.akamaized.net/envivio/EnvivioDash3/manifest.mpd"
    )!
    private let hlsLiveDvrURL = URL(
        string: "https://demo.unified-streaming.com/k8s/live/scte35.isml/.m3u8"
    )!

    @MainActor
    func test720pAVPlayerLifecycleOnPhysicalDevice() async throws {
        try await runLifecycleScenario(
            fixtureName: "device-720p-h264-aac",
            replacementFixtureName: "device-1080p-h264-aac"
        )
    }

    @MainActor
    func test1080pAVPlayerLifecycleOnPhysicalDevice() async throws {
        try await runLifecycleScenario(
            fixtureName: "device-1080p-h264-aac",
            replacementFixtureName: "device-720p-h264-aac"
        )
    }

    @MainActor
    func testHlsVodNetworkPlaybackOnPhysicalDevice() async throws {
        try await runNetworkVodScenario(
            source: .hls(url: hlsVodURL, label: "network-hls-vod")
        )
    }

    @MainActor
    func testDashVodNetworkPlaybackOnPhysicalDevice() async throws {
        try await runNetworkVodScenario(
            source: .dash(url: dashVodURL, label: "network-dash-vod")
        )
    }

    @MainActor
    func testHlsLiveDvrNetworkPlaybackOnPhysicalDevice() async throws {
#if targetEnvironment(simulator)
        throw XCTSkip("Physical iOS device required for HLS Live-DVR acceptance")
#else
        try requireDeviceTestOptIn()
        let source = VesperPlayerSource.hls(url: hlsLiveDvrURL, label: "network-hls-live-dvr")
        let bridge = VesperNativePlayerBridge(
            initialSource: source,
            resiliencePolicy: .streaming(),
            systemPlayerVolume: 0.0,
            systemPlayerIsMuted: true
        )
        let controller = VesperPlayerController(bridge, keepScreenOnDuringPlayback: false)
        var surface = PlayerSurfaceView(frame: CGRect(x: 0, y: 0, width: 390, height: 220))
        let window = try makeWindow(containing: surface)
        var evidence: [PlaybackLifecycleEvidence] = []
        var failureMessage: String?

        defer {
            attachDiagnostics(
                fixtureName: source.label,
                evidence: evidence,
                failureMessage: failureMessage
            )
            controller.pause()
            controller.detachSurfaceHost()
            controller.dispose()
            surface.detachBridgeIfNeeded()
            window.isHidden = true
        }

        controller.attachSurfaceHost(surface)
        controller.initialize()

        do {
            try await requireCondition(
                "Network HLS did not publish a playable Live-DVR window",
                timeout: .seconds(45)
            ) {
                controller.refresh()
                surface.layoutIfNeeded()
                let timeline = controller.uiState.timeline
                guard let range = timeline.seekableRange else { return false }
                return surface.pictureInPicturePlayerLayer?.isReadyForDisplay == true
                    && timeline.kind == .liveDvr
                    && timeline.isSeekable
                    && range.endMs - range.startMs >= 30_000
                    && controller.uiState.playbackState == .playing
            }
            try requireHealthyPlayback(controller: controller, surface: surface)
            evidence.append(snapshot("live-dvr-first-frame", controller: controller, surface: surface))

            let initialPosition = playerPositionMs(surface)
            try await requireCondition("Live-DVR timeline did not advance", timeout: .seconds(12)) {
                controller.refresh()
                return playerPositionMs(surface) >= initialPosition + 4_000
            }
            evidence.append(snapshot("live-dvr-progress", controller: controller, surface: surface))

            controller.pause()
            try await requireCondition("Live-DVR did not pause", timeout: .seconds(5)) {
                controller.refresh()
                return controller.uiState.playbackState == .paused
            }
            let pausedPosition = playerPositionMs(surface)
            try await Task.sleep(for: .seconds(1.5))
            controller.refresh()
            guard abs(playerPositionMs(surface) - pausedPosition) <= 400 else {
                throw PlaybackLifecycleFailure.assertion("Live-DVR position advanced while paused")
            }
            evidence.append(snapshot("live-dvr-pause-stable", controller: controller, surface: surface))

            controller.seek(by: -15_000)
            try await requireCondition("Live-DVR rewind did not move behind the live edge", timeout: .seconds(12)) {
                controller.refresh()
                return (controller.uiState.timeline.liveOffsetMs ?? 0) >= 10_000
            }
            evidence.append(snapshot("live-dvr-rewind", controller: controller, surface: surface))

            controller.play()
            controller.seekToLiveEdge()
            try await requireCondition("Live-DVR did not return to the live edge", timeout: .seconds(15)) {
                controller.refresh()
                return controller.uiState.playbackState == .playing
                    && controller.uiState.timeline.isAtLiveEdge(toleranceMs: 5_000)
            }
            evidence.append(snapshot("live-dvr-go-live", controller: controller, surface: surface))

            let beforeReattach = playerPositionMs(surface)
            controller.detachSurfaceHost()
            surface.removeFromSuperview()
            surface.detachBridgeIfNeeded()
            let replacementSurface = PlayerSurfaceView(frame: window.bounds)
            window.rootViewController?.view.addSubview(replacementSurface)
            replacementSurface.frame = window.bounds
            replacementSurface.layoutIfNeeded()
            surface = replacementSurface
            controller.attachSurfaceHost(surface)
            try await requireCondition(
                "Live-DVR did not recover after Surface recreation",
                timeout: .seconds(12)
            ) {
                controller.refresh()
                return surface.pictureInPicturePlayerLayer?.isReadyForDisplay == true
                    && playerPositionMs(surface) >= beforeReattach + 1_000
                    && controller.lastError == nil
            }
            try requireHealthyPlayback(controller: controller, surface: surface)
            evidence.append(snapshot("live-dvr-surface-reattach", controller: controller, surface: surface))
        } catch {
            failureMessage = String(describing: error)
            evidence.append(snapshot("failure", controller: controller, surface: surface))
            throw error
        }
#endif
    }

    @MainActor
    private func runNetworkVodScenario(source: VesperPlayerSource) async throws {
#if targetEnvironment(simulator)
        throw XCTSkip("Physical iOS device required for network playback acceptance")
#else
        try requireDeviceTestOptIn()
        let bridge = VesperNativePlayerBridge(
            initialSource: source,
            resiliencePolicy: .streaming(),
            systemPlayerVolume: 0.0,
            systemPlayerIsMuted: true
        )
        let controller = VesperPlayerController(bridge, keepScreenOnDuringPlayback: false)
        let surface = PlayerSurfaceView(frame: CGRect(x: 0, y: 0, width: 390, height: 220))
        let window = try makeWindow(containing: surface)
        var evidence: [PlaybackLifecycleEvidence] = []
        var failureMessage: String?

        defer {
            attachDiagnostics(
                fixtureName: source.label,
                evidence: evidence,
                failureMessage: failureMessage
            )
            controller.pause()
            controller.detachSurfaceHost()
            controller.dispose()
            surface.detachBridgeIfNeeded()
            window.isHidden = true
        }

        controller.attachSurfaceHost(surface)
        controller.initialize()

        do {
            try await requireCondition(
                "\(source.label) did not render its first network frame",
                timeout: .seconds(45)
            ) {
                controller.refresh()
                surface.layoutIfNeeded()
                return surface.pictureInPicturePlayerLayer?.isReadyForDisplay == true
                    && controller.uiState.timeline.kind == .vod
                    && controller.uiState.playbackState == .playing
            }
            try requireHealthyPlayback(controller: controller, surface: surface)
            evidence.append(snapshot("network-first-frame", controller: controller, surface: surface))

            let startPosition = playerPositionMs(surface)
            try await requireCondition("\(source.label) timeline did not advance", timeout: .seconds(15)) {
                controller.refresh()
                return playerPositionMs(surface) >= startPosition + 5_000
                    && controller.uiState.timeline.positionMs >= startPosition + 4_000
            }
            let durationMs = try await requireDuration(controller: controller, surface: surface)
            guard durationMs >= 20_000 else {
                throw PlaybackLifecycleFailure.assertion(
                    "\(source.label) published an implausible VOD duration of \(durationMs)ms"
                )
            }
            evidence.append(snapshot("network-progress", controller: controller, surface: surface))

            controller.pause()
            try await requireCondition("\(source.label) did not pause", timeout: .seconds(5)) {
                controller.refresh()
                return controller.uiState.playbackState == .paused
            }
            let pausedPosition = playerPositionMs(surface)
            try await Task.sleep(for: .seconds(1.5))
            controller.refresh()
            guard abs(playerPositionMs(surface) - pausedPosition) <= 400 else {
                throw PlaybackLifecycleFailure.assertion("\(source.label) advanced while paused")
            }
            evidence.append(snapshot("network-pause-stable", controller: controller, surface: surface))

            let seekTargetMs = Int64(Double(durationMs) * 0.55)
            controller.seek(toRatio: 0.55)
            try await requireCondition("\(source.label) seek did not converge", timeout: .seconds(15)) {
                controller.refresh()
                return abs(playerPositionMs(surface) - seekTargetMs) <= 2_500
                    && abs(controller.uiState.timeline.positionMs - seekTargetMs) <= 3_000
            }
            controller.play()
            let resumedPosition = playerPositionMs(surface)
            try await requireCondition("\(source.label) did not resume after seek", timeout: .seconds(12)) {
                controller.refresh()
                return playerPositionMs(surface) >= resumedPosition + 1_500
                    && controller.lastError == nil
            }
            try requireHealthyPlayback(controller: controller, surface: surface)
            evidence.append(snapshot("network-seek-resume", controller: controller, surface: surface))
        } catch {
            failureMessage = String(describing: error)
            evidence.append(snapshot("failure", controller: controller, surface: surface))
            throw error
        }
#endif
    }

    @MainActor
    private func runLifecycleScenario(
        fixtureName: String,
        replacementFixtureName: String
    ) async throws {
#if targetEnvironment(simulator)
        throw XCTSkip("Physical iOS device required for AVPlayer lifecycle acceptance")
#else
        try requireDeviceTestOptIn()

        let fixtureURL = try requiredFixture(named: fixtureName)
        let replacementURL = try requiredFixture(named: replacementFixtureName)
        let expectedPresentationSize = presentationSize(for: fixtureName)
        let expectedReplacementPresentationSize = presentationSize(for: replacementFixtureName)
        let bridge = VesperNativePlayerBridge(
            initialSource: .localFile(url: fixtureURL, label: fixtureName),
            systemPlayerVolume: 0.0,
            systemPlayerIsMuted: true
        )
        let controller = VesperPlayerController(bridge, keepScreenOnDuringPlayback: false)
        var surface = PlayerSurfaceView(frame: CGRect(x: 0, y: 0, width: 390, height: 220))
        let window = try makeWindow(containing: surface)
        var evidence: [PlaybackLifecycleEvidence] = []
        var failureMessage: String?

        defer {
            attachDiagnostics(
                fixtureName: fixtureName,
                evidence: evidence,
                failureMessage: failureMessage
            )
            controller.pause()
            controller.detachSurfaceHost()
            controller.dispose()
            surface.detachBridgeIfNeeded()
            window.isHidden = true
        }

        controller.attachSurfaceHost(surface)
        controller.initialize()

        do {
            try await requireCondition(
                "AVPlayerLayer did not become ready for the first decoded frame",
                timeout: .seconds(12)
            ) {
                controller.refresh()
                surface.layoutIfNeeded()
                guard let layer = surface.pictureInPicturePlayerLayer else { return false }
                return layer.isReadyForDisplay
                    && layer.player?.currentItem != nil
                    && !surface.isNativeFramePresentationActive
            }
            try requireHealthyPlayback(
                controller: controller,
                surface: surface,
                expectedPresentationSize: expectedPresentationSize
            )
            evidence.append(snapshot("first-frame", controller: controller, surface: surface))

            let continuousStart = playerPositionMs(surface)
            try await requireCondition(
                "Timeline did not advance continuously by eight seconds",
                timeout: .seconds(14)
            ) {
                controller.refresh()
                return playerPositionMs(surface) >= continuousStart + 8_000
                    && controller.uiState.timeline.positionMs >= continuousStart + 7_000
            }
            try requireHealthyPlayback(controller: controller, surface: surface)
            evidence.append(snapshot("continuous-playback-8s", controller: controller, surface: surface))

            controller.pause()
            try await requireCondition("Controller did not enter paused state", timeout: .seconds(3)) {
                controller.refresh()
                return controller.uiState.playbackState == .paused
            }
            let pausedPosition = playerPositionMs(surface)
            try await Task.sleep(for: .seconds(1.5))
            controller.refresh()
            let pausedDelta = abs(playerPositionMs(surface) - pausedPosition)
            guard pausedDelta <= 350 else {
                throw PlaybackLifecycleFailure.assertion(
                    "Playback moved \(pausedDelta)ms while paused (maximum 350ms)"
                )
            }
            try requireHealthyPlayback(controller: controller, surface: surface)
            evidence.append(snapshot("pause-stable", controller: controller, surface: surface))

            controller.play()
            try await requireCondition("Playback did not resume", timeout: .seconds(7)) {
                controller.refresh()
                return playerPositionMs(surface) >= pausedPosition + 1_200
            }
            try requireHealthyPlayback(controller: controller, surface: surface)
            evidence.append(snapshot("resume", controller: controller, surface: surface))

            let durationMs = try await requireDuration(controller: controller, surface: surface)
            guard durationMs >= 20_000 else {
                throw PlaybackLifecycleFailure.assertion(
                    "Fixture duration was \(durationMs)ms; device lifecycle fixtures must be at least 20 seconds"
                )
            }
            let seekTargetMs = Int64(Double(durationMs) * 0.70)
            controller.seek(toRatio: 0.70)
            try await requireCondition("Seek did not converge near 70%", timeout: .seconds(8)) {
                controller.refresh()
                return abs(playerPositionMs(surface) - seekTargetMs) <= 1_500
                    && abs(controller.uiState.timeline.positionMs - seekTargetMs) <= 2_000
            }
            try requireHealthyPlayback(controller: controller, surface: surface)
            evidence.append(snapshot("seek-70-percent", controller: controller, surface: surface))

            let beforeReattach = playerPositionMs(surface)
            controller.detachSurfaceHost()
            surface.removeFromSuperview()
            surface.detachBridgeIfNeeded()
            let replacementSurface = PlayerSurfaceView(frame: window.bounds)
            window.rootViewController?.view.addSubview(replacementSurface)
            replacementSurface.frame = window.bounds
            replacementSurface.layoutIfNeeded()
            surface = replacementSurface
            controller.attachSurfaceHost(surface)
            try await requireCondition("Recreated AVPlayerLayer did not render and progress", timeout: .seconds(8)) {
                controller.refresh()
                return surface.pictureInPicturePlayerLayer?.isReadyForDisplay == true
                    && playerPositionMs(surface) >= beforeReattach + 1_000
            }
            try requireHealthyPlayback(
                controller: controller,
                surface: surface,
                expectedPresentationSize: expectedPresentationSize
            )
            evidence.append(snapshot("surface-reattach", controller: controller, surface: surface))

            controller.selectSource(.localFile(url: replacementURL, label: replacementFixtureName))
            try await requireCondition("Replacement source did not produce a new first frame", timeout: .seconds(12)) {
                controller.refresh()
                guard
                    controller.uiState.sourceLabel == replacementFixtureName,
                    surface.pictureInPicturePlayerLayer?.isReadyForDisplay == true,
                    let asset = surface.pictureInPicturePlayerLayer?.player?.currentItem?.asset as? AVURLAsset
                else { return false }
                let positionMs = playerPositionMs(surface)
                return asset.url.standardizedFileURL == replacementURL.standardizedFileURL
                    && positionMs >= 500
                    && positionMs <= 3_000
            }
            try requireHealthyPlayback(
                controller: controller,
                surface: surface,
                expectedPresentationSize: expectedReplacementPresentationSize
            )
            evidence.append(snapshot("source-replacement", controller: controller, surface: surface))
        } catch {
            failureMessage = String(describing: error)
            evidence.append(snapshot("failure", controller: controller, surface: surface))
            throw error
        }
#endif
    }

    private func requireDeviceTestOptIn() throws {
        guard ProcessInfo.processInfo.environment["VESPER_IOS_PLAYBACK_DEVICE_TESTS"] == "1" else {
            throw XCTSkip("Set VESPER_IOS_PLAYBACK_DEVICE_TESTS=1 to run physical-device playback acceptance")
        }
    }

    @MainActor
    private func requiredFixture(named name: String) throws -> URL {
        guard let url = Bundle(for: Self.self).url(forResource: name, withExtension: "m4v") else {
            throw PlaybackLifecycleFailure.assertion(
                "Opted-in device test is missing required fixture \(name).m4v"
            )
        }
        return url
    }

    private func presentationSize(for fixtureName: String) -> CGSize {
        fixtureName.contains("1080p")
            ? CGSize(width: 1_920, height: 1_080)
            : CGSize(width: 1_280, height: 720)
    }

    @MainActor
    private func makeWindow(containing surface: PlayerSurfaceView) throws -> UIWindow {
        guard let windowScene = UIApplication.shared.connectedScenes
            .compactMap({ $0 as? UIWindowScene })
            .first
        else {
            throw PlaybackLifecycleFailure.assertion("Device test host has no connected window scene")
        }
        let window = UIWindow(windowScene: windowScene)
        window.frame = surface.bounds
        let viewController = UIViewController()
        viewController.view.frame = window.bounds
        viewController.view.backgroundColor = .black
        window.rootViewController = viewController
        viewController.view.addSubview(surface)
        surface.frame = viewController.view.bounds
        window.makeKeyAndVisible()
        surface.layoutIfNeeded()
        return window
    }

    @MainActor
    private func requireCondition(
        _ message: String,
        timeout: Duration,
        condition: () -> Bool
    ) async throws {
        let clock = ContinuousClock()
        let deadline = clock.now.advanced(by: timeout)
        while clock.now < deadline {
            if condition() { return }
            try await clock.sleep(for: .milliseconds(100))
        }
        guard condition() else {
            throw PlaybackLifecycleFailure.assertion(message)
        }
    }

    @MainActor
    private func requireDuration(
        controller: VesperPlayerController,
        surface: PlayerSurfaceView
    ) async throws -> Int64 {
        var durationMs: Int64 = 0
        try await requireCondition("VOD duration did not become available", timeout: .seconds(5)) {
            controller.refresh()
            let sdkDuration = controller.uiState.timeline.durationMs ?? 0
            let playerDuration = surface.pictureInPicturePlayerLayer?.player?.currentItem?.duration.seconds ?? 0
            if sdkDuration > 0 {
                durationMs = sdkDuration
            } else if playerDuration.isFinite,
                      playerDuration > 0,
                      playerDuration <= Double(Int64.max) / 1_000 {
                durationMs = Int64(playerDuration * 1_000)
            }
            return durationMs > 0
        }
        return durationMs
    }

    @MainActor
    private func requireHealthyPlayback(
        controller: VesperPlayerController,
        surface: PlayerSurfaceView,
        expectedPresentationSize: CGSize? = nil
    ) throws {
        if let error = controller.lastError {
            throw PlaybackLifecycleFailure.assertion(
                "Controller reported an error: \(String(describing: error))"
            )
        }
        if let error = surface.pictureInPicturePlayerLayer?.player?.currentItem?.error {
            throw PlaybackLifecycleFailure.assertion(
                "AVPlayerItem reported an error: \(error.localizedDescription)"
            )
        }
        guard let player = surface.pictureInPicturePlayerLayer?.player else {
            throw PlaybackLifecycleFailure.assertion("AVPlayerLayer has no player")
        }
        guard player.volume == 0.0, player.isMuted else {
            throw PlaybackLifecycleFailure.assertion(
                "Device acceptance must stay silent; volume=\(player.volume), isMuted=\(player.isMuted)"
            )
        }
        if let expectedPresentationSize {
            let actual = player.currentItem?.presentationSize ?? .zero
            guard actual == expectedPresentationSize else {
                throw PlaybackLifecycleFailure.assertion(
                    "Expected presentation size \(expectedPresentationSize), got \(actual)"
                )
            }
        }
    }

    @MainActor
    private func playerPositionMs(_ surface: PlayerSurfaceView) -> Int64 {
        guard let seconds = surface.pictureInPicturePlayerLayer?.player?.currentTime().seconds,
              seconds.isFinite else { return 0 }
        return Int64(seconds * 1_000)
    }

    @MainActor
    private func snapshot(
        _ checkpoint: String,
        controller: VesperPlayerController,
        surface: PlayerSurfaceView
    ) -> PlaybackLifecycleEvidence {
        let layer = surface.pictureInPicturePlayerLayer
        let presentationSize = layer?.player?.currentItem?.presentationSize ?? .zero
        return PlaybackLifecycleEvidence(
            checkpoint: checkpoint,
            sourceLabel: controller.uiState.sourceLabel,
            playbackState: controller.uiState.playbackState.rawValue,
            isBuffering: controller.uiState.isBuffering,
            isInterrupted: controller.uiState.isInterrupted,
            sdkPositionMs: controller.uiState.timeline.positionMs,
            sdkDurationMs: controller.uiState.timeline.durationMs,
            playerPositionMs: playerPositionMs(surface),
            playerTimeControlStatus: layer?.player?.timeControlStatus.rawValue,
            presentationWidth: Double(presentationSize.width),
            presentationHeight: Double(presentationSize.height),
            playerLayerReadyForDisplay: layer?.isReadyForDisplay ?? false,
            playerLayerHasPlayer: layer?.player != nil,
            playerVolume: layer?.player?.volume,
            playerIsMuted: layer?.player?.isMuted,
            nativeFramePresentationActive: surface.isNativeFramePresentationActive,
            controllerError: controller.lastError.map { String(describing: $0) },
            playerItemError: layer?.player?.currentItem?.error?.localizedDescription
        )
    }

    @MainActor
    private func attachDiagnostics(
        fixtureName: String,
        evidence: [PlaybackLifecycleEvidence],
        failureMessage: String?
    ) {
        let report = PlaybackLifecycleReport(
            fixture: fixtureName,
            device: UIDevice.current.model,
            systemVersion: UIDevice.current.systemVersion,
            failure: failureMessage,
            checkpoints: evidence
        )
        if let data = try? JSONEncoder().encode(report) {
            let attachment = XCTAttachment(data: data, uniformTypeIdentifier: "public.json")
            attachment.name = "\(fixtureName)-playback-lifecycle.json"
            attachment.lifetime = .keepAlways
            add(attachment)
        }
    }
}

private struct PlaybackLifecycleReport: Codable {
    let fixture: String
    let device: String
    let systemVersion: String
    let failure: String?
    let checkpoints: [PlaybackLifecycleEvidence]
}

private struct PlaybackLifecycleEvidence: Codable {
    let checkpoint: String
    let sourceLabel: String
    let playbackState: String
    let isBuffering: Bool
    let isInterrupted: Bool
    let sdkPositionMs: Int64
    let sdkDurationMs: Int64?
    let playerPositionMs: Int64
    let playerTimeControlStatus: Int?
    let presentationWidth: Double
    let presentationHeight: Double
    let playerLayerReadyForDisplay: Bool
    let playerLayerHasPlayer: Bool
    let playerVolume: Float?
    let playerIsMuted: Bool?
    let nativeFramePresentationActive: Bool
    let controllerError: String?
    let playerItemError: String?
}

private enum PlaybackLifecycleFailure: LocalizedError {
    case assertion(String)

    var errorDescription: String? {
        switch self {
        case .assertion(let message): message
        }
    }
}
