import Darwin
import UIKit
import XCTest
@testable import VesperPlayerHostDemo
@testable import VesperPlayerKit

@MainActor
final class VesperOptionalPluginDeviceAcceptanceTests: XCTestCase {
    func testBundledPluginEntriesAndCheckedLoaders() throws {
        let paths = try bundledPluginPaths()
        for path in paths.all {
            try assertPluginEntryLoads(at: path)
        }

        let source = try localSmokeSource()
        let diagnostics = VesperMobilePluginDiagnosticsProbe.run(
            source: source,
            sourceNormalizer: VesperSourceNormalizerConfiguration(
                mode: .diagnosticsOnly,
                pluginLibraryPaths: [paths.sourceNormalizer]
            ),
            frameProcessor: VesperFrameProcessorConfiguration(
                mode: .diagnosticsOnly,
                pluginLibraryPaths: [paths.frameProcessor]
            )
        )
        XCTAssertTrue(
            diagnostics.contains { diagnostic in
                diagnostic["path"] as? String == paths.sourceNormalizer &&
                    diagnostic["status"] as? String == "sourceNormalizerSupported"
            },
            "The bundled SourceNormalizer must pass the checked mobile plugin loader."
        )
        XCTAssertTrue(
            diagnostics.contains { diagnostic in
                diagnostic["path"] as? String == paths.frameProcessor &&
                    diagnostic["status"] as? String == "frameProcessorSupported"
            },
            "The bundled FrameProcessor must pass the checked mobile plugin loader."
        )

        let baseDirectory = FileManager.default.temporaryDirectory
            .appendingPathComponent("vesper-remux-device-\(UUID().uuidString)", isDirectory: true)
        let manager = VesperDownloadManager(
            configuration: VesperDownloadConfiguration(
                autoStart: false,
                runPostProcessorsOnCompletion: false,
                restoreTasksOnStartup: false,
                baseDirectory: baseDirectory,
                pluginLibraryPaths: [paths.remux]
            )
        )
        defer {
            manager.dispose()
            try? FileManager.default.removeItem(at: baseDirectory)
        }
        let taskId = try manager.createTask(
            assetId: "optional-plugin-device-remux",
            source: VesperDownloadSource(source: source)
        )
        XCTAssertNotNil(
            taskId,
            "The bundled Remux plugin must pass the checked download-plugin loader."
        )
    }

    func testBundledNativeFramePluginsSeekSynchronizeAndReplayLocalMp4OnPhysicalDevice() async throws {
#if targetEnvironment(simulator)
        throw XCTSkip("Physical iOS device required for optional plugin playback acceptance.")
#else
        let paths = try bundledPluginPaths()
        let source = try localSmokeSource()
        let surfaceView = PlayerSurfaceView(frame: CGRect(x: 0, y: 0, width: 320, height: 180))
        guard surfaceView.supportsNativeFrameMetalPresentation else {
            throw XCTSkip("Metal native-frame presentation is unavailable on this device.")
        }

        let window = UIWindow(frame: surfaceView.bounds)
        let viewController = UIViewController()
        viewController.view.frame = window.bounds
        viewController.view.backgroundColor = .black
        window.rootViewController = viewController
        window.makeKeyAndVisible()
        viewController.view.addSubview(surfaceView)
        surfaceView.frame = viewController.view.bounds
        surfaceView.layoutIfNeeded()
        surfaceView.attachNativeFramePresenter()

        let session = VesperNativeFramePipelineSession(
            source: source,
            configuration: VesperNativeFramePipelineConfiguration(
                mode: .requireNativeFrame,
                decoderPluginLibraryPaths: [paths.decoder],
                frameProcessorPluginLibraryPaths: [paths.frameProcessor],
                maxInFlightFrames: 1
            ),
            sourceNormalizer: VesperSourceNormalizerConfiguration(
                mode: .preflightOnly,
                pluginLibraryPaths: [paths.sourceNormalizer]
            ),
            surfaceHost: surfaceView
        )
        var timelines: [VesperNativeFramePipelineTimeline] = []
        var playbackEndedCount = 0
        var playbackFailures: [VesperNativeFramePipelineIssue] = []
        session.onFramePresented = { timelines.append($0) }
        session.onPlaybackEnded = { playbackEndedCount += 1 }
        session.onPlaybackFailed = { playbackFailures.append($0) }
        defer {
            session.close()
            surfaceView.detachBridgeIfNeeded()
            window.isHidden = true
        }

        switch await session.start() {
        case .success:
            break
        case .failure(let error):
            XCTFail("The bundled native-frame plugin chain failed to start: \(error.localizedDescription)")
            return
        }

        XCTAssertTrue(session.play())
        let presented = await waitForPluginPlayback(timeout: 10) {
            session.counters.presentedFrames > 0 &&
                session.counters.processedFrames > 0
        }

        XCTAssertTrue(
            presented,
            "SourceNormalizer, VideoToolbox Decoder, and FrameProcessor must produce and present a frame."
        )
        XCTAssertEqual(session.route, "sdkManagedNativeFrame")
        XCTAssertEqual(session.participation, "participated")
        XCTAssertGreaterThan(session.counters.presentedFrames, 0)
        XCTAssertGreaterThan(session.counters.processedFrames, 0)
        XCTAssertEqual(session.clockSource, "swiftNativeAudioBridge")
        XCTAssertTrue(session.hasAudioTrack)
        XCTAssertTrue(session.seekable)
        let durationMs = try XCTUnwrap(session.durationMs)
        XCTAssertGreaterThan(durationMs, 1_250)

        var firstSeekApplied: Bool?
        var latestSeekApplied: Bool?
        let firstSeekTargetMs: Int64 = 250
        let latestSeekTargetMs = min(Int64(1_000), durationMs - 250)
        let presentedBeforeSeek = session.counters.presentedFrames
        XCTAssertTrue(session.seek(toMs: firstSeekTargetMs) { firstSeekApplied = $0 })
        session.setPlaybackRate(1.25)
        session.pause()
        XCTAssertTrue(session.play(rate: 1.25))
        XCTAssertTrue(session.seek(toMs: latestSeekTargetMs) { latestSeekApplied = $0 })

        let latestSeekResumed = await waitForPluginPlayback(timeout: 10) {
            firstSeekApplied != nil &&
                latestSeekApplied == true &&
                session.isPlaying &&
                session.counters.presentedFrames > presentedBeforeSeek
        }
        XCTAssertTrue(
            latestSeekResumed,
            "The latest seek must resume presentation after interleaved rate, pause, and play commands."
        )
        XCTAssertEqual(firstSeekApplied, false)
        XCTAssertEqual(latestSeekApplied, true)
        XCTAssertEqual(session.playbackRate, 1.25)
        XCTAssertTrue(timelines.contains { $0.positionMs == latestSeekTargetMs })

        let audioClockAdvanced = await waitForPluginPlayback(timeout: 5) {
            session.timelinePositionMs(framePresentationTimeUs: 0) >= latestSeekTargetMs + 100
        }
        XCTAssertTrue(
            audioClockAdvanced,
            "The physical Swift native audio clock must advance from the latest seek target."
        )

        let reachedEnd = await waitForPluginPlayback(timeout: 10) {
            playbackEndedCount == 1 && session.hasReachedEnd && !session.isPlaying
        }
        XCTAssertTrue(reachedEnd, "The two-second fixture must reach native-frame end-of-stream.")
        let presentedAtEnd = session.counters.presentedFrames
        let replayTimelineStart = timelines.count

        XCTAssertTrue(session.play(rate: 1.0))
        let replayed = await waitForPluginPlayback(timeout: 10) {
            !session.hasReachedEnd &&
                session.isPlaying &&
                session.counters.presentedFrames > presentedAtEnd
        }
        session.pause()

        XCTAssertTrue(replayed, "A seekable source must rewind and present again after end-of-stream.")
        XCTAssertTrue(
            timelines.dropFirst(replayTimelineStart).contains { $0.positionMs == 0 },
            "Replay must publish the successful rewind-to-zero timeline."
        )
        XCTAssertEqual(playbackEndedCount, 1)
        XCTAssertTrue(playbackFailures.isEmpty, playbackFailures.map(\.message).joined(separator: " | "))
#endif
    }

    func testBundledVideoToolboxPluginReordersBFramesAndFlushesSeekOnPhysicalDevice() async throws {
#if targetEnvironment(simulator)
        throw XCTSkip("Physical iOS device required for VideoToolbox B-frame acceptance.")
#else
        let paths = try bundledPluginPaths()
        let source = try localBFrameSmokeSource()
        let surfaceView = PlayerSurfaceView(frame: CGRect(x: 0, y: 0, width: 320, height: 180))
        guard surfaceView.supportsNativeFrameMetalPresentation else {
            throw XCTSkip("Metal native-frame presentation is unavailable on this device.")
        }

        let window = UIWindow(frame: surfaceView.bounds)
        let viewController = UIViewController()
        viewController.view.frame = window.bounds
        viewController.view.backgroundColor = .black
        window.rootViewController = viewController
        window.makeKeyAndVisible()
        viewController.view.addSubview(surfaceView)
        surfaceView.frame = viewController.view.bounds
        surfaceView.layoutIfNeeded()
        surfaceView.attachNativeFramePresenter()

        let session = VesperNativeFramePipelineSession(
            source: source,
            configuration: VesperNativeFramePipelineConfiguration(
                mode: .requireNativeFrame,
                decoderPluginLibraryPaths: [paths.decoder],
                maxInFlightFrames: 1
            ),
            sourceNormalizer: VesperSourceNormalizerConfiguration(
                mode: .preflightOnly,
                pluginLibraryPaths: [paths.sourceNormalizer]
            ),
            surfaceHost: surfaceView
        )
        var timelines: [VesperNativeFramePipelineTimeline] = []
        var playbackEndedCount = 0
        var playbackFailures: [VesperNativeFramePipelineIssue] = []
        session.onFramePresented = { timelines.append($0) }
        session.onPlaybackEnded = { playbackEndedCount += 1 }
        session.onPlaybackFailed = { playbackFailures.append($0) }
        defer {
            session.close()
            surfaceView.detachBridgeIfNeeded()
            window.isHidden = true
        }

        switch await session.start() {
        case .success:
            break
        case .failure(let error):
            XCTFail("The bundled VideoToolbox B-frame session failed to start: \(error.localizedDescription)")
            return
        }

        XCTAssertFalse(session.hasAudioTrack)
        XCTAssertEqual(session.clockSource, "video")
        XCTAssertTrue(session.seekable)
        XCTAssertTrue(session.play())
        let presentedBeforeSeek = await waitForPluginPlayback(timeout: 10) {
            session.counters.presentedFrames >= 3
        }
        XCTAssertTrue(presentedBeforeSeek, "The B-frame fixture must present before seek.")

        let preSeekPositions = timelines.map(\.positionMs)
        assertNondecreasing(preSeekPositions, message: "VideoToolbox emitted out-of-order PTS before seek.")

        let seekTargetMs: Int64 = 1_000
        let postSeekTimelineStart = timelines.count
        let presentedFrameCountBeforeSeek = session.counters.presentedFrames
        var seekApplied: Bool?
        XCTAssertTrue(session.seek(toMs: seekTargetMs) { seekApplied = $0 })
        let seekResumed = await waitForPluginPlayback(timeout: 10) {
            seekApplied == true &&
                session.counters.presentedFrames > presentedFrameCountBeforeSeek
        }
        XCTAssertTrue(seekResumed, "The B-frame fixture must resume after seek flush.")

        let reachedEnd = await waitForPluginPlayback(timeout: 10) {
            playbackEndedCount == 1 && session.hasReachedEnd && !session.isPlaying
        }
        XCTAssertTrue(reachedEnd, "The B-frame fixture must drain delayed frames to EOS.")

        let postSeekPositions = timelines.dropFirst(postSeekTimelineStart).map(\.positionMs)
        XCTAssertFalse(postSeekPositions.isEmpty)
        XCTAssertTrue(
            postSeekPositions.allSatisfy { $0 >= seekTargetMs },
            "Seek flush must not emit stale pre-seek PTS: \(postSeekPositions)"
        )
        assertNondecreasing(
            postSeekPositions,
            message: "VideoToolbox emitted out-of-order PTS after seek."
        )
        XCTAssertEqual(session.counters.lateDropped, 0)
        XCTAssertGreaterThan(session.counters.presentedFrames, 6)
        XCTAssertTrue(playbackFailures.isEmpty, playbackFailures.map(\.message).joined(separator: " | "))
#endif
    }

    private func bundledPluginPaths() throws -> BundledPluginPaths {
        try BundledPluginPaths(
            remux: XCTUnwrap(bundledDownloadPluginLibraryPaths().first),
            sourceNormalizer: XCTUnwrap(bundledSourceNormalizerPluginLibraryPaths().first),
            decoder: XCTUnwrap(bundledDecoderPluginLibraryPaths().first),
            frameProcessor: XCTUnwrap(bundledFrameProcessorPluginLibraryPaths().first)
        )
    }

    private func localSmokeSource() throws -> VesperPlayerSource {
        let mediaURL = try XCTUnwrap(
            Bundle(for: Self.self).url(
                forResource: "tiny-h264-aac",
                withExtension: "m4v"
            )
        )
        return try VesperPlayerSource(
            uri: mediaURL.absoluteString,
            label: "Optional Plugin Device Smoke",
            kind: .local,
            protocol: .file
        )
    }

    private func localBFrameSmokeSource() throws -> VesperPlayerSource {
        let mediaURL = try XCTUnwrap(
            Bundle(for: Self.self).url(
                forResource: "tiny-h264-bframes",
                withExtension: "m4v"
            )
        )
        return try VesperPlayerSource(
            uri: mediaURL.absoluteString,
            label: "Optional Plugin B-Frame Smoke",
            kind: .local,
            protocol: .file
        )
    }

    private func assertNondecreasing(_ positions: [Int64], message: String) {
        for (current, next) in zip(positions, positions.dropFirst()) {
            XCTAssertLessThanOrEqual(current, next, message)
        }
    }

    private func assertPluginEntryLoads(at path: String) throws {
        dlerror()
        guard let handle = dlopen(path, RTLD_NOW | RTLD_LOCAL) else {
            throw OptionalPluginAcceptanceError.dynamicLoader(
                path: path,
                message: dynamicLoaderMessage()
            )
        }
        defer { dlclose(handle) }

        dlerror()
        guard let symbol = dlsym(handle, "vesper_plugin_entry") else {
            throw OptionalPluginAcceptanceError.missingEntry(
                path: path,
                message: dynamicLoaderMessage()
            )
        }
        typealias PluginEntry = @convention(c) () -> UnsafeRawPointer?
        let entry = unsafeBitCast(symbol, to: PluginEntry.self)
        XCTAssertNotNil(entry(), "The plugin descriptor must not be null: \(path)")
    }

    private func dynamicLoaderMessage() -> String {
        guard let message = dlerror() else {
            return "unknown dynamic loader error"
        }
        return String(cString: message)
    }

    private func waitForPluginPlayback(
        timeout: TimeInterval,
        condition: @escaping () -> Bool
    ) async -> Bool {
        let deadline = Date().addingTimeInterval(timeout)
        while Date() < deadline {
            if condition() {
                return true
            }
            try? await Task.sleep(for: .milliseconds(50))
        }
        return condition()
    }
}

private struct BundledPluginPaths {
    let remux: String
    let sourceNormalizer: String
    let decoder: String
    let frameProcessor: String

    var all: [String] {
        [remux, sourceNormalizer, decoder, frameProcessor]
    }
}

private enum OptionalPluginAcceptanceError: LocalizedError {
    case dynamicLoader(path: String, message: String)
    case missingEntry(path: String, message: String)

    var errorDescription: String? {
        switch self {
        case .dynamicLoader(let path, let message):
            "Failed to load optional plugin at \(path): \(message)"
        case .missingEntry(let path, let message):
            "Optional plugin at \(path) is missing vesper_plugin_entry: \(message)"
        }
    }
}
