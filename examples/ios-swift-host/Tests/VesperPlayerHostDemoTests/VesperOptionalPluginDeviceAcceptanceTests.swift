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

    func testBundledNativeFramePluginsPresentLocalMp4OnPhysicalDevice() async throws {
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

        session.play()
        let presented = await waitForPluginPlayback(timeout: 10) {
            session.counters.presentedFrames > 0 &&
                session.counters.processedFrames > 0
        }
        session.pause()

        XCTAssertTrue(
            presented,
            "SourceNormalizer, VideoToolbox Decoder, and FrameProcessor must produce and present a frame."
        )
        XCTAssertEqual(session.route, "sdkManagedNativeFrame")
        XCTAssertEqual(session.participation, "participated")
        XCTAssertGreaterThan(session.counters.presentedFrames, 0)
        XCTAssertGreaterThan(session.counters.processedFrames, 0)
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
