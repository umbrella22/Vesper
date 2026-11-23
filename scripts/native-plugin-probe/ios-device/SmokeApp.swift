import AVFoundation
import CoreImage
import CoreVideo
import Metal
import UIKit
@testable import VesperPlayerKit

@main
final class SmokeApp: UIResponder, UIApplicationDelegate {
    func application(_ application: UIApplication, configurationForConnecting session: UISceneSession, options: UIScene.ConnectionOptions) -> UISceneConfiguration {
        let config = UISceneConfiguration(name: nil, sessionRole: session.role)
        config.delegateClass = SmokeScene.self
        return config
    }
}

@MainActor
final class SmokeScene: UIResponder, UIWindowSceneDelegate {
    var window: UIWindow?
    private var launched = false
    func scene(_ scene: UIScene, willConnectTo session: UISceneSession, options connectionOptions: UIScene.ConnectionOptions) {
        guard let scene = scene as? UIWindowScene else { return }
        let window = UIWindow(windowScene: scene)
        let controller = UIViewController()
        controller.view.backgroundColor = .black
        window.rootViewController = controller
        window.makeKeyAndVisible()
        self.window = window
    }

    func sceneDidBecomeActive(_ scene: UIScene) {
        guard !launched else { return }
        launched = true
        Task { @MainActor in
            let smoke = SmokeCases(windowScene: scene as? UIWindowScene)
            do {
                try smoke.testFixedGainPcm()
                try await smoke.testNativeFramePipelineRealPluginPlaybackPresentsSeeksAndReleasesLocalMp4()
                try await smoke.testRequiredNormalizerResourcePlaysThroughAVPlayerAndCleansUp()
                try await smoke.testDirectNativeLocalPlayback()
                try await smoke.testDownloadedFilePlaysAfterOriginCloses()
                print("VESPER NATIVE PLUGIN SMOKE PASS")
                fflush(nil)
            } catch {
                print("VESPER NATIVE PLUGIN SMOKE FAILED: \(error)")
                fflush(nil)
            }
        }
    }
}

@MainActor
final class SmokeCases {
    let windowScene: UIWindowScene?
    init(windowScene: UIWindowScene?) { self.windowScene = windowScene }
    static func audioReference() throws -> VesperPluginReference {
        try VesperPluginReference(
            pluginId: "dev.vesper.example.gain-cpp",
            capabilityInstanceId: "dev.vesper.example.gain-cpp.audio",
            transport: .native
        )
    }

    func testFixedGainPcm() throws {
        let binding = try VesperNativeFrameAudioProcessorBinding(reference: Self.audioReference())
        let format = try requireValue(AVAudioFormat(standardFormatWithSampleRate: 48_000, channels: 2))
        let pcm = try requireValue(AVAudioPCMBuffer(pcmFormat: format, frameCapacity: 2051))
        pcm.frameLength = 2051
        let planes = try requireValue(pcm.floatChannelData)
        for index in 0..<2051 {
            planes[0][index] = 0.75
            planes[1][index] = -0.25
        }
        let processor = try binding.open(format: format, epoch: 4)
        try processor.process(pcm, ptsUs: 900_000)
        for index in 0..<2051 {
            expectEqual(planes[0][index], 0.375)
            expectEqual(planes[1][index], -0.125)
        }
        print("real iOS audio fixed gain matched 2051 stereo frames")
    }

    func testNativeFramePipelineRealPluginPlaybackPresentsSeeksAndReleasesLocalMp4() async throws {
        let smoke = try Self.nativeFrameSmokeConfiguration()
        let source = try VesperPlayerSource(
            uri: URL(fileURLWithPath: smoke.sourcePath).absoluteString,
            label: "Native Frame Smoke MP4",
            kind: .local,
            protocol: .file
        )
        let frame = CGRect(x: 0, y: 0, width: 320, height: 180)
        let surfaceView = PlayerSurfaceView(frame: frame, bundledFilterLibrary: smoke.metalFilterLibrary)
        guard surfaceView.supportsNativeFrameMetalPresentation else {
            throw SmokeFailure("Metal is unavailable on the current iOS destination.")
        }
        let window = UIWindow(windowScene: try requireValue(windowScene))
        let viewController = UIViewController()
        viewController.view.frame = window.bounds
        viewController.view.backgroundColor = .black
        window.rootViewController = viewController
        window.makeKeyAndVisible()
        viewController.view.addSubview(surfaceView)
        surfaceView.frame = viewController.view.bounds
        surfaceView.layoutIfNeeded()
        surfaceView.attachNativeFramePresenter()
        expectTrue(surfaceView.isBundledMetalFilterReady)

        let presenter = RecordingNativeFramePresenter(wrapped: surfaceView)
        let backend = VesperFfiNativeFramePipelineBackend()
        try await testMetalFilterColorAndOrientation(smoke)
        try await testLateMetalLease(smoke, backend: backend, source: source)
        let session = VesperNativeFramePipelineSession(
            source: source,
            configuration: VesperNativeFramePipelineConfiguration(
                mode: .requireNativeFrame,
                decoderPluginReferences: [VesperBundledPluginReferences.decoderVideoToolbox],
                maxInFlightFrames: 1,
                audioProcessorReference: try Self.audioReference()
            ),
            sourceNormalizer: VesperSourceNormalizerConfiguration(
                mode: .preflightOnly,
                pluginReferences: [VesperBundledPluginReferences.sourceNormalizerFfmpeg]
            ),
            surfaceHost: surfaceView,
            backend: backend,
            nativeFramePresenter: presenter
        )
        var lastTimeline: VesperNativeFramePipelineTimeline?
        session.onFramePresented = { timeline in
            lastTimeline = timeline
        }
        defer {
            session.close()
            surfaceView.detachBridgeIfNeeded()
            window.isHidden = true
        }

        switch await session.start() {
        case .success:
            break
        case .failure(let error):
            fail(
                "expected real native-frame plugin session to start; " +
                    "kind=\(error.issue.kind.rawValue) message=\(error.message)"
            )
            return
        }

        expectEqual(session.route, "sdkManagedNativeFrame")
        expectEqual(session.clockSource, "swiftNativeAudioBridge")
        expectEqual(session.audioDecoderKind, "swiftNativeAudioBridge")
        expectEqual(session.audioOutputKind, "swiftNativeAudioBridge")
        expectEqual(session.audioPipelineKind, "swiftNativeAudioBridge")
        expectTrue(session.hasAudioTrack)
        expectEqual(session.selectedVideoMediaKind, "video")
        expectEqual(session.audioMediaKind, "audio")
        expectGreaterThan(session.durationMs ?? 0, 0)
        expectTrue(session.seekable)

        session.play(rate: 1.0)
        let presentedFirstFrame = await waitForNativeFrameSmoke(timeout: 8) {
            session.counters.presentedFrames >= 1 &&
                presenter.presentedPixelBufferAddresses.count >= 1
        }
        expectTrue(
            presentedFirstFrame,
            "real native-frame plugin smoke did not present the first CVPixelBuffer"
        )

        session.pause()
        let presentedBeforeResume = session.counters.presentedFrames
        session.setPlaybackRate(1.25)
        session.play(rate: 1.25)
        let presentedAfterResume = await waitForNativeFrameSmoke(timeout: 8) {
            session.counters.presentedFrames > presentedBeforeResume
        }
        expectTrue(
            presentedAfterResume,
            "real native-frame plugin smoke did not resume after pause/rate update"
        )

        let presentedBeforeSeek = session.counters.presentedFrames
        let seekTargetMs = min(max((session.durationMs ?? 2_000) / 2, 250), 1_500)
        expectTrue(session.seek(toMs: seekTargetMs))
        let presentedAfterSeek = await waitForNativeFrameSmoke(timeout: 8) {
            session.counters.presentedFrames > presentedBeforeSeek
        }
        expectTrue(
            presentedAfterSeek,
            "real native-frame plugin smoke did not present after seek"
        )
        session.pause()

        expectGreaterThanOrEqual(try requireValue(lastTimeline).positionMs, 0)
        expectGreaterThanOrEqual(session.counters.presentedFrames, 3)
        expectEqual(session.counters.deadlineMisses, 0)
        expectEqual(session.counters.backpressureCount, 0)
        expectEqual(session.counters.lateDropped, 0)
        expectTrue(presenter.presentedPixelBufferAddresses.allSatisfy { $0 != 0 })
        expectTrue(presenter.presentedResults.allSatisfy { $0 })
        await session.close().value
        expectEqual(presenter.enabledStates.last, false)
        expectFalse(surfaceView.isBundledMetalFilterReady)
        print(
            "real iOS native-frame smoke presentedFrames=\(session.counters.presentedFrames) " +
                "processedFrames=\(session.counters.processedFrames) " +
                "skippedAudioPackets=\(session.counters.skippedAudioPackets) "
        )
    }

    func testRequiredNormalizerResourcePlaysThroughAVPlayerAndCleansUp() async throws {
        let sourcePath = try Self.nativeFrameSmokeConfiguration().sourcePath
        let source = try VesperPlayerSource(
            uri: URL(fileURLWithPath: sourcePath).absoluteString,
            label: "Required Normalizer Smoke MP4",
            kind: .local,
            protocol: .file
        )
        let bridge = VesperNativePlayerBridge(
            initialSource: source,
            sourceNormalizerConfiguration: VesperSourceNormalizerConfiguration(
                mode: .requireNormalized,
                pluginReferences: [VesperBundledPluginReferences.sourceNormalizerFfmpeg]
            ),
            systemPlayerVolume: 0
        )
        defer { bridge.dispose() }
        bridge.initialize()
        let reachedTerminalState = await waitForNativeFrameSmoke(timeout: 10) {
            bridge.player?.currentItem?.status == .readyToPlay || bridge.lastError != nil
        }
        expectTrue(reachedTerminalState)
        expectNil(bridge.lastError)
        let player = try requireValue(bridge.player)
        let item = try requireValue(player.currentItem)
        expectEqual(item.status, .readyToPlay)
        let asset = try requireValue(item.asset as? AVURLAsset)
        expectEqual(asset.url.scheme, "vesper-normalized")
        let resource = try requireValue(bridge.currentSourceNormalizerResource)
        expectEqual(resource.outputRoute, "fmp4LocalStream")
        expectNotNil(bridge.sourceNormalizerResourceLoaderDelegate)
        expectTrue(FileManager.default.fileExists(atPath: resource.primaryResourcePath))

        bridge.play()
        let advanced = await waitForNativeFrameSmoke(timeout: 5) {
            player.currentTime().seconds > 0.2 || bridge.lastError != nil
        }
        expectTrue(advanced)
        expectNil(bridge.lastError)
        bridge.dispose()
        expectNil(bridge.currentSourceNormalizerResource)
        expectNil(bridge.sourceNormalizerResourceLoaderDelegate)
        let removed = await waitForNativeFrameSmoke(timeout: 5) {
            !FileManager.default.fileExists(atPath: resource.primaryResourcePath)
        }
        expectTrue(removed, "disposing the resource must remove the normalized output")
        print("real iOS normalized resource smoke advanced=\(advanced) removed=\(removed)")
    }

    func testDirectNativeLocalPlayback() async throws {
        let smoke = try Self.nativeFrameSmokeConfiguration()
        try await verifyDirectNativeFile(URL(fileURLWithPath: smoke.sourcePath))
        print("real iOS DirectNative file smoke advanced=true")
    }

    func testDownloadedFilePlaysAfterOriginCloses() async throws {
        let smoke = try Self.nativeFrameSmokeConfiguration()
        let fixture = try Data(contentsOf: URL(fileURLWithPath: smoke.sourcePath))
        let originURL = try requireValue(URL(string: "https://offline.fixture.test/download.mp4"))
        OfflineFixtureProtocol.setResponse(fixture)
        defer { OfflineFixtureProtocol.setResponse(nil) }
        let sessionConfiguration = URLSessionConfiguration.ephemeral
        sessionConfiguration.protocolClasses = [OfflineFixtureProtocol.self]
        let directory = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        defer { try? FileManager.default.removeItem(at: directory) }
        let executor = VesperForegroundDownloadExecutor(baseDirectory: directory)
        executor.httpSessionConfiguration = sessionConfiguration
        let manager = try VesperDownloadManager(configuration: .init(
            runPostProcessorsOnCompletion: false, restoreTasksOnStartup: false, baseDirectory: directory
        ), executor: executor)
        defer { manager.dispose() }
        let source = VesperPlayerSource(uri: originURL.absoluteString, label: "Offline MP4",
                                        kind: .remote, protocol: .progressive)
        let id = try requireValue(manager.createTask(assetId: "offline-smoke", source: .init(source: source)))
        let finished = await waitForNativeFrameSmoke(timeout: 15) {
            manager.task(id).map { $0.state == .completed || $0.state == .failed } == true
        }
        let task = try requireValue(manager.task(id))
        guard finished && task.state == .completed else {
            throw SmokeFailure("progressive download state=\(task.state) error=\(String(describing: task.error))")
        }
        let output = try manager.outputURL(forTask: id)
        guard try Data(contentsOf: output) == fixture else {
            throw SmokeFailure("downloaded file differs from the origin fixture")
        }

        OfflineFixtureProtocol.setResponse(nil)
        let session = URLSession(configuration: sessionConfiguration)
        defer { session.invalidateAndCancel() }
        do {
            _ = try await session.data(for: URLRequest(url: originURL, timeoutInterval: 2))
            throw SmokeFailure("download origin still reachable")
        } catch let error as URLError where error.code == .cannotConnectToHost {
            try await verifyDirectNativeFile(output)
        }
        print("iOS DirectNative offline smoke fixtureOriginClosed=true bytes=\(fixture.count) advanced=true")
    }

    private func verifyDirectNativeFile(_ url: URL) async throws {
        let source = VesperPlayerSource.localFile(url: url)
        let bridge = VesperNativePlayerBridge(initialSource: source, systemPlayerVolume: 0)
        defer { bridge.dispose() }
        bridge.initialize()
        let ready = await waitForNativeFrameSmoke(timeout: 10) {
            bridge.player?.currentItem?.status == .readyToPlay || bridge.lastError != nil
        }
        expectTrue(ready)
        expectNil(bridge.lastError)
        let player = try requireValue(bridge.player)
        let asset = try requireValue(player.currentItem?.asset as? AVURLAsset)
        expectEqual(asset.url.standardizedFileURL, url.standardizedFileURL)
        expectNil(bridge.currentSourceNormalizerResource)
        bridge.play()
        let advanced = await waitForNativeFrameSmoke(timeout: 5) {
            player.currentTime().seconds > 0.2 || bridge.lastError != nil
        }
        expectTrue(advanced)
        expectNil(bridge.lastError)
    }

    private func testMetalFilterColorAndOrientation(_ smoke: NativeFrameSmokeConfiguration) async throws {
        let device = try requireValue(MTLCreateSystemDefaultDevice())
        let queue = try requireValue(device.makeCommandQueue())
        let filter = try requireValue(VesperBundledMetalFilter(device: device, queue: queue, library: smoke.metalFilterLibrary))
        let context = CIContext(mtlDevice: device)
        let colorSpace = try requireValue(CGColorSpace(name: CGColorSpace.sRGB))
        let size = 32
        var buffer: CVPixelBuffer?
        let status = CVPixelBufferCreate(nil, size, size, kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange, [
            kCVPixelBufferIOSurfacePropertiesKey: [:],
            kCVPixelBufferMetalCompatibilityKey: true,
        ] as CFDictionary, &buffer)
        guard status == kCVReturnSuccess else { throw SmokeFailure("CVPixelBufferCreate returned \(status)") }
        let pixelBuffer = try requireValue(buffer)
        let lockStatus = CVPixelBufferLockBaseAddress(pixelBuffer, [])
        guard lockStatus == kCVReturnSuccess else { throw SmokeFailure("CVPixelBufferLockBaseAddress returned \(lockStatus)") }
        do {
            defer { CVPixelBufferUnlockBaseAddress(pixelBuffer, []) }
            let luma = try requireValue(CVPixelBufferGetBaseAddressOfPlane(pixelBuffer, 0))
            let lumaStride = CVPixelBufferGetBytesPerRowOfPlane(pixelBuffer, 0)
            for row in 0..<size {
                memset(luma.advanced(by: row * lumaStride), row < size / 2 ? 32 : 216, size)
            }
            let chroma = try requireValue(CVPixelBufferGetBaseAddressOfPlane(pixelBuffer, 1))
            memset(chroma, 128, CVPixelBufferGetBytesPerRowOfPlane(pixelBuffer, 1) * size / 2)
        }

        func rowDifference(_ image: CIImage) -> Int {
            let stride = size * 4
            var pixels = [UInt8](repeating: 0, count: stride * size)
            pixels.withUnsafeMutableBytes { bytes in
                guard let baseAddress = bytes.baseAddress else { return }
                context.render(image, toBitmap: baseAddress, rowBytes: stride, bounds: image.extent,
                               format: .RGBA8, colorSpace: colorSpace)
            }
            return Int(pixels[stride * (size - 2)]) - Int(pixels[stride])
        }

        CVBufferSetAttachment(pixelBuffer, kCVImageBufferTransferFunctionKey,
                              kCVImageBufferTransferFunction_ITU_R_709_2, .shouldPropagate)
        for (name, primaries, matrix) in [
            ("BT.709", kCVImageBufferColorPrimaries_ITU_R_709_2, kCVImageBufferYCbCrMatrix_ITU_R_709_2),
            ("BT.601", kCVImageBufferColorPrimaries_SMPTE_C, kCVImageBufferYCbCrMatrix_ITU_R_601_4),
        ] {
            CVBufferSetAttachment(pixelBuffer, kCVImageBufferColorPrimariesKey, primaries, .shouldPropagate)
            CVBufferSetAttachment(pixelBuffer, kCVImageBufferYCbCrMatrixKey, matrix, .shouldPropagate)
            let baseline = rowDifference(CIImage(cvPixelBuffer: pixelBuffer))
            let command = try requireValue(queue.makeCommandBuffer())
            guard let filtered = filter.encode(pixelBuffer: pixelBuffer, into: command) else {
                throw SmokeFailure("Metal filter rejected \(name)")
            }
            command.commit()
            let finished = await waitForNativeFrameSmoke(timeout: 5) {
                command.status == .completed || command.status == .error
            }
            guard finished && command.status == .completed else {
                throw SmokeFailure("Metal filter did not complete for \(name): \(String(describing: command.error))")
            }
            let presented = rowDifference(filtered)
            guard abs(baseline) > 64 && abs(presented) > 64 && (baseline > 0) == (presented > 0) else {
                throw SmokeFailure("Metal filter row order for \(name): unfiltered=\(baseline) filtered=\(presented)")
            }
            print("iOS Metal \(name) smoke unfilteredRowDifference=\(baseline) filteredRowDifference=\(presented)")
        }
    }

    private func testLateMetalLease(
        _ smoke: NativeFrameSmokeConfiguration,
        backend: VesperFfiNativeFramePipelineBackend,
        source: VesperPlayerSource
    ) async throws {
        let device = try requireValue(MTLCreateSystemDefaultDevice())
        let queue = try requireValue(device.makeCommandQueue())
        let gate = try requireValue(device.makeSharedEvent())
        let command = try requireValue(queue.makeCommandBuffer())
        var filter = VesperBundledMetalFilter(device: device, queue: queue, library: smoke.metalFilterLibrary)
        let opened = try backend.open(source: source,
            configuration: VesperNativeFramePipelineConfiguration(mode: .requireNativeFrame,
                decoderPluginReferences: [VesperBundledPluginReferences.decoderVideoToolbox],
                maxInFlightFrames: 2),
            sourceNormalizer: VesperSourceNormalizerConfiguration(mode: .preflightOnly,
                pluginReferences: [VesperBundledPluginReferences.sourceNormalizerFfmpeg])).get()
        var closed = false
        defer { if !closed { backend.close(handle: opened.handle) } }
        var frame: [String: Any]?
        for _ in 0..<200 {
            let next = try backend.advance(handle: opened.handle).get()
            if next["status"] as? String == "frame" { frame = next; break }
            try await Task.sleep(for: .milliseconds(5))
        }
        let frameHandle = try requireValue((frame?["handle"] as? NSNumber)?.uint64Value)
        let address = try requireValue((frame?["pixelBuffer"] as? NSNumber)?.uintValue)
        let pointer = try requireValue(UnsafeRawPointer(bitPattern: address))
        let lease = try requireValue(backend.takePresentationLease(handle: opened.handle, frameHandle: frameHandle))
        let pixel = Unmanaged<CVPixelBuffer>.fromOpaque(pointer).takeUnretainedValue()
        command.encodeWaitForEvent(gate, value: 1)
        expectNotNil(filter?.encode(pixelBuffer: pixel, into: command))
        command.commit()
        defer { gate.signaledValue = 1 }
        _ = try backend.seek(handle: opened.handle, positionMs: 250).get()
        _ = try backend.seek(handle: opened.handle, positionMs: 500).get()
        backend.close(handle: opened.handle)
        closed = true
        filter = nil
        expectTrue(command.status != .completed)
        guard case .failure = backend.advance(handle: opened.handle) else {
            fail("Closed native-frame handle remained usable")
            return
        }
        gate.signaledValue = 1
        let completed = await waitForNativeFrameSmoke(timeout: 5) {
            command.status == .completed || command.status == .error
        }
        expectTrue(completed)
        expectEqual(command.status, .completed)
        lease.finish(presented: false)
        lease.finish(presented: false)
        print("real iOS Metal lease completed after two seeks and decoder close")
    }

    private static func nativeFrameSmokeConfiguration() throws -> NativeFrameSmokeConfiguration {
        try NativeFrameSmokeConfiguration(
            sourcePath: requireValue(Bundle.main.url(forResource: "vesper-native-frame-smoke", withExtension: "mp4")).path,
            metalFilterLibrary: requireValue(Bundle.main.url(forResource: "vesper-color-filter", withExtension: "metallib"))
        )
    }

    private func waitForNativeFrameSmoke(
        timeout: TimeInterval,
        condition: @escaping () -> Bool
    ) async -> Bool {
        let deadline = Date().addingTimeInterval(timeout)
        while Date() < deadline {
            if condition() {
                return true
            }
            try? await Task.sleep(nanoseconds: 20_000_000)
        }
        return condition()
    }
}

/// Supplies a deterministic HTTPS response without changing the SDK's network policy.
private final class OfflineFixtureProtocol: URLProtocol {
    private static let lock = NSLock()
    nonisolated(unsafe) private static var data: Data?

    static func setResponse(_ data: Data?) { lock.withLock { Self.data = data } }

    override class func canInit(with request: URLRequest) -> Bool {
        request.url?.scheme == "https" && request.url?.host == "offline.fixture.test"
    }

    override class func canonicalRequest(for request: URLRequest) -> URLRequest { request }

    override func startLoading() {
        guard let data = Self.lock.withLock({ Self.data }), let url = request.url,
              let response = HTTPURLResponse(url: url, statusCode: 200, httpVersion: nil, headerFields: [
                "Content-Length": String(data.count), "Content-Type": "video/mp4",
              ]) else {
            client?.urlProtocol(self, didFailWithError: URLError(.cannotConnectToHost))
            return
        }
        client?.urlProtocol(self, didReceive: response, cacheStoragePolicy: .notAllowed)
        if request.httpMethod != "HEAD" { client?.urlProtocol(self, didLoad: data) }
        client?.urlProtocolDidFinishLoading(self)
    }

    override func stopLoading() {}
}

private struct NativeFrameSmokeConfiguration {
    let sourcePath: String
    let metalFilterLibrary: URL
}

@MainActor
private final class RecordingNativeFramePresenter: VesperNativeFramePresenting {
    private let wrapped: VesperNativeFramePresenting
    private(set) var enabledStates: [Bool] = []
    private(set) var presentedPixelBufferAddresses: [UInt] = []
    private(set) var presentedResults: [Bool] = []

    init(wrapped: VesperNativeFramePresenting) {
        self.wrapped = wrapped
    }

    func setNativeFramePresentationEnabled(_ enabled: Bool) {
        enabledStates.append(enabled)
        wrapped.setNativeFramePresentationEnabled(enabled)
    }

    func presentNativeFrame(pixelBuffer: CVPixelBuffer) async -> Bool {
        let pixelBufferAddress = testPixelBufferAddress(pixelBuffer)
        presentedPixelBufferAddresses.append(pixelBufferAddress)
        let result = await wrapped.presentNativeFrame(pixelBuffer: pixelBuffer)
        presentedResults.append(result)
        return result
    }
}


private func testPixelBufferAddress(_ pixelBuffer: CVPixelBuffer) -> UInt {
    UInt(bitPattern: Unmanaged.passUnretained(pixelBuffer).toOpaque())
}
private struct SmokeFailure: Error { let reason: String; init(_ reason: String) { self.reason = reason } }
private func requireValue<T>(_ value: T?) throws -> T {
    guard let value else { throw SmokeFailure("unexpected nil") }
    return value
}
private func fail(_ message: String, line: UInt = #line) {
    print("VESPER NATIVE PLUGIN SMOKE FAILED line=\(line) \(message)")
    fflush(nil)
    fatalError(message, line: line)
}
private func expectTrue(_ value: Bool, _ message: String = "expected true", line: UInt = #line) { if !value { fail(message, line: line) } }
private func expectFalse(_ value: Bool, line: UInt = #line) { expectTrue(!value, "expected false", line: line) }
private func expectNil<T>(_ value: T?, line: UInt = #line) { expectTrue(value == nil, "expected nil, received \(String(describing: value))", line: line) }
private func expectNotNil<T>(_ value: T?, line: UInt = #line) { expectTrue(value != nil, "unexpected nil", line: line) }
private func expectEqual<T: Equatable>(_ lhs: T, _ rhs: T, line: UInt = #line) { expectTrue(lhs == rhs, "expected \(rhs), received \(lhs)", line: line) }
private func expectGreaterThan<T: Comparable>(_ lhs: T, _ rhs: T, line: UInt = #line) { expectTrue(lhs > rhs, "expected \(lhs) > \(rhs)", line: line) }
private func expectGreaterThanOrEqual<T: Comparable>(_ lhs: T, _ rhs: T, line: UInt = #line) { expectTrue(lhs >= rhs, "expected \(lhs) >= \(rhs)", line: line) }
