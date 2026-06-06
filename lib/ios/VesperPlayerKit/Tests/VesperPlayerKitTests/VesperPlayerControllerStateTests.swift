import UIKit
import XCTest
@testable import VesperPlayerKit

@MainActor
final class VesperPlayerControllerStateTests: XCTestCase {
    func testControllerMirrorsBridgeFixedTrackStatusAndResiliencePolicy() async {
        let bridge = TestObservablePlayerBridge()
        let controller = VesperPlayerController(bridge)

        let updatedPolicy = VesperPlaybackResiliencePolicy.resilient()
        bridge.publishedTrackCatalog = sampleTrackCatalog
        bridge.publishedTrackSelection = VesperTrackSelectionSnapshot(
            abrPolicy: .fixedTrack("video:hls:cavc1:b1500000:w1280:h720:f3000")
        )
        bridge.publishedEffectiveVideoTrackId = "video:hls:cavc1:b1500000:w1280:h720:f3000"
        bridge.publishedVideoVariantObservation = VesperVideoVariantObservation(
            bitRate: 1_500_000,
            width: 1280,
            height: 720
        )
        bridge.publishedFixedTrackStatus = .locked
        bridge.publishedResiliencePolicy = updatedPolicy
        bridge.publishedLastError = VesperPlayerError(
            message: "temporary network hiccup",
            code: .backendFailure,
            category: .network,
            retriable: true
        )
        await settleControllerObservation()

        XCTAssertEqual(controller.trackCatalog, sampleTrackCatalog)
        XCTAssertEqual(
            controller.trackSelection.abrPolicy,
            .fixedTrack("video:hls:cavc1:b1500000:w1280:h720:f3000")
        )
        XCTAssertEqual(
            controller.effectiveVideoTrackId,
            "video:hls:cavc1:b1500000:w1280:h720:f3000"
        )
        XCTAssertEqual(
            controller.videoVariantObservation,
            VesperVideoVariantObservation(
                bitRate: 1_500_000,
                width: 1280,
                height: 720
            )
        )
        XCTAssertEqual(controller.fixedTrackStatus, .locked)
        XCTAssertEqual(controller.resiliencePolicy, updatedPolicy)
        XCTAssertEqual(controller.lastError?.category, .network)
        XCTAssertEqual(controller.lastError?.message, "temporary network hiccup")
    }

    func testControllerClearsStaleEffectiveTrackStateAfterSourceReset() async {
        let bridge = TestObservablePlayerBridge()
        let controller = VesperPlayerController(bridge)

        bridge.publishedTrackCatalog = sampleTrackCatalog
        bridge.publishedTrackSelection = VesperTrackSelectionSnapshot(
            abrPolicy: .fixedTrack("video:hls:cavc1:b1500000:w1280:h720:f3000")
        )
        bridge.publishedEffectiveVideoTrackId = "video:hls:cavc1:b1500000:w1280:h720:f3000"
        bridge.publishedVideoVariantObservation = VesperVideoVariantObservation(
            bitRate: 1_500_000,
            width: 1280,
            height: 720
        )
        bridge.publishedFixedTrackStatus = .locked
        await settleControllerObservation()

        bridge.publishedTrackCatalog = .empty
        bridge.publishedTrackSelection = VesperTrackSelectionSnapshot()
        bridge.publishedEffectiveVideoTrackId = nil
        bridge.publishedVideoVariantObservation = nil
        bridge.publishedFixedTrackStatus = nil
        bridge.publishedLastError = nil
        await settleControllerObservation()

        XCTAssertEqual(controller.trackCatalog, .empty)
        XCTAssertEqual(controller.trackSelection, VesperTrackSelectionSnapshot())
        XCTAssertNil(controller.effectiveVideoTrackId)
        XCTAssertNil(controller.videoVariantObservation)
        XCTAssertNil(controller.fixedTrackStatus)
        XCTAssertNil(controller.lastError)
    }

    func testBenchmarkRecorderDefaultsDisabled() {
        let bridge = FakePlayerBridge(benchmarkConfiguration: .disabled)
        let controller = VesperPlayerController(bridge)

        controller.initialize()
        controller.play()

        XCTAssertTrue(controller.drainBenchmarkEvents().isEmpty)
        XCTAssertEqual(controller.benchmarkSummary().acceptedEvents, 0)
    }

    func testBenchmarkRecorderDrainsRawEventsAndKeepsSummary() {
        let bridge = FakePlayerBridge(
            benchmarkConfiguration: VesperBenchmarkConfiguration(enabled: true)
        )
        let controller = VesperPlayerController(bridge)

        controller.initialize()
        controller.play()

        let events = controller.drainBenchmarkEvents()
        let eventNames = Set(events.map(\.eventName))
        XCTAssertTrue(eventNames.contains("initialize_start"))
        XCTAssertTrue(eventNames.contains("initialize_without_source"))
        XCTAssertTrue(eventNames.contains("play_command"))
        XCTAssertTrue(controller.drainBenchmarkEvents().isEmpty)
        XCTAssertEqual(controller.benchmarkSummary().acceptedEvents, UInt64(events.count))
    }

    func testNativeBridgeRecordsFirstFrameOncePerPlaybackEpoch() {
        let bridge = VesperNativePlayerBridge(
            benchmarkConfiguration: VesperBenchmarkConfiguration(enabled: true)
        )

        bridge.handleSurfaceReadyForDisplay()
        bridge.handleSurfaceReadyForDisplay()

        let events = bridge.drainBenchmarkEvents()
        let readyEvents = events.filter { $0.eventName == "ready_for_display" }
        let firstFrameEvents = events.filter { $0.eventName == "first_frame_rendered" }

        XCTAssertEqual(readyEvents.count, 2)
        XCTAssertEqual(firstFrameEvents.count, 1)
        XCTAssertEqual(firstFrameEvents.first?.attributes["playbackEpoch"], "0")
        XCTAssertEqual(readyEvents.last?.attributes["isFirstForEpoch"], "false")
    }

    func testNativeBridgeClearsReadyForDisplayCountsWhenEpochAdvances() {
        let bridge = VesperNativePlayerBridge(
            benchmarkConfiguration: VesperBenchmarkConfiguration(enabled: true)
        )

        bridge.handleSurfaceReadyForDisplay()
        bridge.handleSurfaceReadyForDisplay()
        XCTAssertEqual(bridge.readyForDisplayEpochCountSnapshot(), 1)

        bridge.dispose()

        XCTAssertEqual(bridge.playbackEpochSnapshot(), 1)
        XCTAssertLessThanOrEqual(bridge.readyForDisplayEpochCountSnapshot(), 1)
    }

    func testNativeFramePipelineConfigurationAddsDiagnosticsWithoutReplacingPlaybackSource() {
        let source = try! VesperPlayerSource(
            uri: "https://example.com/video.mp4",
            label: "MP4",
            kind: .remote,
            protocol: .progressive
        )
        let bridge = VesperNativePlayerBridge(
            initialSource: source,
            nativeFramePipelineConfiguration: VesperNativeFramePipelineConfiguration(
                mode: .preferNativeFrame,
                decoderPluginLibraryPaths: ["/tmp/libdecoder.dylib"],
                frameProcessorPluginLibraryPaths: ["/tmp/libframe.dylib"],
                maxInFlightFrames: 2
            )
        )

        XCTAssertTrue(
            bridge.pluginDiagnostics.contains { diagnostic in
                diagnostic["pluginKind"] as? String == "native_frame_pipeline" &&
                    diagnostic["participation"] as? String == "selected" &&
                    diagnostic["route"] as? String == "sdkManagedNativeFrame" &&
                    diagnostic["clockSource"] as? String == "pending" &&
                    diagnostic["presenterProfile"] as? String == "MetalLayer"
            }
        )
    }

    func testNativeFramePipelineDiagnosticsOnlyKeepsSystemPlayerRoute() {
        let source = try! VesperPlayerSource(
            uri: "https://example.com/video.mp4",
            label: "MP4",
            kind: .remote,
            protocol: .progressive
        )
        let bridge = VesperNativePlayerBridge(
            initialSource: source,
            nativeFramePipelineConfiguration: VesperNativeFramePipelineConfiguration(
                mode: .diagnosticsOnly,
                decoderPluginLibraryPaths: ["/tmp/libdecoder.dylib"]
            )
        )

        bridge.initialize()

        XCTAssertNil(bridge.lastError)
        XCTAssertTrue(
            bridge.pluginDiagnostics.contains { diagnostic in
                diagnostic["pluginKind"] as? String == "native_frame_pipeline" &&
                    diagnostic["status"] as? String == "loaded" &&
                    diagnostic["participation"] as? String == "available" &&
                    diagnostic["route"] as? String == "systemPlayer" &&
                    diagnostic["clockSource"] as? String == "pending" &&
                    (diagnostic["message"] as? String)?.contains("playback still uses the system player") == true
            }
        )
    }

    func testNativeFramePipelinePreferFallsBackWhenDecoderPluginPathIsMissing() {
        let source = try! VesperPlayerSource(
            uri: "https://example.com/video.mp4",
            label: "MP4",
            kind: .remote,
            protocol: .progressive
        )
        let bridge = VesperNativePlayerBridge(
            initialSource: source,
            sourceNormalizerConfiguration: VesperSourceNormalizerConfiguration(
                mode: .preflightOnly,
                pluginLibraryPaths: ["/tmp/libsource_normalizer.dylib"]
            ),
            nativeFramePipelineConfiguration: VesperNativeFramePipelineConfiguration(
                mode: .preferNativeFrame
            )
        )
        let surface = PlayerSurfaceView()
        bridge.attachSurfaceHost(surface)

        bridge.initialize()

        XCTAssertNil(bridge.lastError)
        XCTAssertTrue(
            bridge.pluginDiagnostics.contains { diagnostic in
                diagnostic["pluginKind"] as? String == "native_frame_pipeline" &&
                    diagnostic["status"] as? String == "unsupported" &&
                    diagnostic["participation"] as? String == "fallback" &&
                    diagnostic["route"] as? String == "systemPlayer" &&
                    diagnostic["fallbackKind"] as? String == "missingVideoToolboxDecoderPlugin" &&
                    diagnostic["fallbackTargetRoute"] as? String == "systemPlayer" &&
                    (diagnostic["fallbackReason"] as? String)?.contains("VideoToolbox decoder plugin path") == true
            }
        )
    }

    func testNativeFramePipelinePreferFallsBackWhenSourceNormalizerModeCannotProvidePackets() {
        let source = try! VesperPlayerSource(
            uri: "file:///tmp/example.mov",
            label: "Local MOV",
            kind: .local,
            protocol: .file
        )
        let bridge = VesperNativePlayerBridge(
            initialSource: source,
            sourceNormalizerConfiguration: VesperSourceNormalizerConfiguration(
                mode: .disabled,
                pluginLibraryPaths: ["/tmp/libsource_normalizer.dylib"]
            ),
            nativeFramePipelineConfiguration: VesperNativeFramePipelineConfiguration(
                mode: .preferNativeFrame,
                decoderPluginLibraryPaths: ["/tmp/libdecoder.dylib"]
            )
        )
        let surface = PlayerSurfaceView()
        bridge.attachSurfaceHost(surface)

        bridge.initialize()

        XCTAssertNil(bridge.lastError)
        XCTAssertTrue(
            bridge.pluginDiagnostics.contains { diagnostic in
                diagnostic["pluginKind"] as? String == "native_frame_pipeline" &&
                    diagnostic["status"] as? String == "unsupported" &&
                    diagnostic["participation"] as? String == "fallback" &&
                    diagnostic["route"] as? String == "systemPlayer" &&
                    diagnostic["fallbackKind"] as? String == "missingSourceNormalizerPacketPlugin" &&
                    diagnostic["fallbackTargetRoute"] as? String == "systemPlayer" &&
                    (diagnostic["fallbackReason"] as? String)?
                        .contains("SourceNormalizer packet-stream input") == true
            }
        )
    }

    func testNativeFramePipelineRequireFailsWhenDecoderPluginPathIsMissing() {
        let source = try! VesperPlayerSource(
            uri: "https://example.com/video.mp4",
            label: "MP4",
            kind: .remote,
            protocol: .progressive
        )
        let bridge = VesperNativePlayerBridge(
            initialSource: source,
            sourceNormalizerConfiguration: VesperSourceNormalizerConfiguration(
                mode: .preflightOnly,
                pluginLibraryPaths: ["/tmp/libsource_normalizer.dylib"]
            ),
            nativeFramePipelineConfiguration: VesperNativeFramePipelineConfiguration(
                mode: .requireNativeFrame
            )
        )
        let surface = PlayerSurfaceView()
        bridge.attachSurfaceHost(surface)

        bridge.initialize()

        XCTAssertEqual(bridge.lastError?.code, .unsupported)
        XCTAssertEqual(bridge.lastError?.category, .capability)
        XCTAssertTrue(bridge.lastError?.message.contains("VideoToolbox decoder plugin path") == true)
        XCTAssertTrue(
            bridge.pluginDiagnostics.contains { diagnostic in
                diagnostic["pluginKind"] as? String == "native_frame_pipeline" &&
                    diagnostic["status"] as? String == "unsupported" &&
                    diagnostic["participation"] as? String == "selected" &&
                    diagnostic["route"] as? String == "sdkManagedNativeFrame" &&
                    diagnostic["failureKind"] as? String == "missingVideoToolboxDecoderPlugin"
            }
        )
    }

    func testNativeFramePipelinePreferWaitsForSurfaceHostBeforeLoading() {
        let source = try! VesperPlayerSource(
            uri: "https://example.com/video.mp4",
            label: "MP4",
            kind: .remote,
            protocol: .progressive
        )
        let bridge = VesperNativePlayerBridge(
            initialSource: source,
            sourceNormalizerConfiguration: VesperSourceNormalizerConfiguration(
                mode: .preflightOnly,
                pluginLibraryPaths: ["/tmp/libsource_normalizer.dylib"]
            ),
            nativeFramePipelineConfiguration: VesperNativeFramePipelineConfiguration(
                mode: .preferNativeFrame,
                decoderPluginLibraryPaths: ["/tmp/libdecoder.dylib"]
            )
        )

        bridge.initialize()

        XCTAssertNil(bridge.lastError)
        XCTAssertTrue(
            bridge.pluginDiagnostics.contains { diagnostic in
                    diagnostic["pluginKind"] as? String == "native_frame_pipeline" &&
                    diagnostic["status"] as? String == "loaded" &&
                    diagnostic["participation"] as? String == "selected" &&
                    diagnostic["route"] as? String == "sdkManagedNativeFrame"
            }
        )
    }

    func testNativeFramePipelinePreferFallsBackWhenStartupPluginCannotLoad() {
        let source = try! VesperPlayerSource(
            uri: "https://example.com/video.mp4",
            label: "MP4",
            kind: .remote,
            protocol: .progressive
        )
        let bridge = VesperNativePlayerBridge(
            initialSource: source,
            sourceNormalizerConfiguration: VesperSourceNormalizerConfiguration(
                mode: .preflightOnly,
                pluginLibraryPaths: ["/tmp/libsource_normalizer.dylib"]
            ),
            nativeFramePipelineConfiguration: VesperNativeFramePipelineConfiguration(
                mode: .preferNativeFrame,
                decoderPluginLibraryPaths: ["/tmp/libdecoder.dylib"]
            )
        )
        let surface = PlayerSurfaceView()
        bridge.attachSurfaceHost(surface)

        bridge.initialize()

        XCTAssertNil(bridge.lastError)
        XCTAssertTrue(
            bridge.pluginDiagnostics.contains { diagnostic in
                diagnostic["pluginKind"] as? String == "native_frame_pipeline" &&
                    diagnostic["status"] as? String == "unsupported" &&
                    diagnostic["participation"] as? String == "fallback" &&
                    diagnostic["route"] as? String == "systemPlayer" &&
                    diagnostic["fallbackKind"] as? String == "missingSourceNormalizerPacketPlugin" &&
                    diagnostic["fallbackTargetRoute"] as? String == "systemPlayer" &&
                    (diagnostic["fallbackReason"] as? String)?.contains("SourceNormalizer packet plugin") == true
            }
        )
    }

    func testNativeFramePipelinePreferFallsBackWhenStartupCodecUnsupported() {
        let source = try! VesperPlayerSource(
            uri: "file:///tmp/hdr.mov",
            label: "HDR MOV",
            kind: .local,
            protocol: .file
        )
        let backend = TestNativeFramePipelineBackend()
        backend.openResult = .failure(
            VesperNativeFramePipelineStartupError(
                issue: VesperNativeFramePipelineIssue(
                    kind: .unsupportedCodec,
                    message: "iOS native-frame pipeline supports H264/HEVC packet streams, got VP9"
                )
            )
        )
        let coordinator = VesperNativeFramePipelineCoordinator { source, configuration, sourceNormalizer, surfaceHost in
            VesperNativeFramePipelineSession(
                source: source,
                configuration: configuration,
                sourceNormalizer: sourceNormalizer,
                surfaceHost: surfaceHost,
                backend: backend,
                audioOutput: TestNativeFrameAudioOutput()
            )
        }
        let bridge = VesperNativePlayerBridge(
            initialSource: source,
            sourceNormalizerConfiguration: VesperSourceNormalizerConfiguration(
                mode: .preflightOnly,
                pluginLibraryPaths: ["/tmp/libsource_normalizer.dylib"]
            ),
            nativeFramePipelineConfiguration: VesperNativeFramePipelineConfiguration(
                mode: .preferNativeFrame,
                decoderPluginLibraryPaths: ["/tmp/libdecoder.dylib"]
            ),
            nativeFramePipelineCoordinator: coordinator
        )
        let surface = PlayerSurfaceView()
        bridge.attachSurfaceHost(surface)

        bridge.initialize()

        XCTAssertNil(bridge.lastError)
        XCTAssertNotNil(bridge.routePickerPlayer)
        XCTAssertEqual(backend.closeHandles, [])
        XCTAssertTrue(
            bridge.pluginDiagnostics.contains { diagnostic in
                diagnostic["pluginKind"] as? String == "native_frame_pipeline" &&
                    diagnostic["status"] as? String == "unsupported" &&
                    diagnostic["participation"] as? String == "fallback" &&
                    diagnostic["route"] as? String == "systemPlayer" &&
                    diagnostic["fallbackKind"] as? String == "unsupportedCodec" &&
                    diagnostic["fallbackTargetRoute"] as? String == "systemPlayer" &&
                    (diagnostic["fallbackReason"] as? String)?.contains("HEVC") == true
            }
        )
    }

    func testNativeFramePipelinePreferAttemptsStandardLocalFileBeforeFallback() {
        let source = try! VesperPlayerSource(
            uri: "file:///tmp/example.mov",
            label: "Local MOV",
            kind: .local,
            protocol: .file
        )
        let bridge = VesperNativePlayerBridge(
            initialSource: source,
            sourceNormalizerConfiguration: VesperSourceNormalizerConfiguration(
                mode: .preflightOnly,
                pluginLibraryPaths: ["/tmp/libsource_normalizer.dylib"]
            ),
            nativeFramePipelineConfiguration: VesperNativeFramePipelineConfiguration(
                mode: .preferNativeFrame,
                decoderPluginLibraryPaths: ["/tmp/libdecoder.dylib"]
            )
        )
        let surface = PlayerSurfaceView()
        bridge.attachSurfaceHost(surface)

        bridge.initialize()

        XCTAssertNil(bridge.lastError)
        XCTAssertTrue(
            bridge.pluginDiagnostics.contains { diagnostic in
                diagnostic["pluginKind"] as? String == "native_frame_pipeline" &&
                    diagnostic["status"] as? String == "unsupported" &&
                    diagnostic["participation"] as? String == "fallback" &&
                    diagnostic["route"] as? String == "systemPlayer" &&
                    diagnostic["fallbackKind"] as? String == "missingSourceNormalizerPacketPlugin" &&
                    diagnostic["fallbackTargetRoute"] as? String == "systemPlayer" &&
                    (diagnostic["fallbackReason"] as? String)?.contains("SourceNormalizer packet plugin") == true
            }
        )
    }

    func testNativeFramePipelineRequireDoesNotBypassStandardLocalFile() {
        let source = try! VesperPlayerSource(
            uri: "file:///tmp/example.mov",
            label: "Local MOV",
            kind: .local,
            protocol: .file
        )
        let bridge = VesperNativePlayerBridge(
            initialSource: source,
            sourceNormalizerConfiguration: VesperSourceNormalizerConfiguration(
                mode: .preflightOnly,
                pluginLibraryPaths: ["/tmp/libsource_normalizer.dylib"]
            ),
            nativeFramePipelineConfiguration: VesperNativeFramePipelineConfiguration(
                mode: .requireNativeFrame,
                decoderPluginLibraryPaths: ["/tmp/libdecoder.dylib"]
            )
        )
        let surface = PlayerSurfaceView()
        bridge.attachSurfaceHost(surface)

        bridge.initialize()

        XCTAssertEqual(bridge.lastError?.code, .unsupported)
        XCTAssertTrue(
            bridge.pluginDiagnostics.contains { diagnostic in
                diagnostic["pluginKind"] as? String == "native_frame_pipeline" &&
                    diagnostic["status"] as? String == "loadFailed" &&
                    diagnostic["participation"] as? String == "selected" &&
                    diagnostic["route"] as? String == "sdkManagedNativeFrame" &&
                    diagnostic["failureKind"] as? String == "missingSourceNormalizerPacketPlugin"
            }
        )
    }

    func testNativeFramePipelineRequireFailsWithoutPacketLane() {
        let source = try! VesperPlayerSource(
            uri: "https://example.com/video.mp4",
            label: "MP4",
            kind: .remote,
            protocol: .progressive
        )
        let bridge = VesperNativePlayerBridge(
            initialSource: source,
            nativeFramePipelineConfiguration: VesperNativeFramePipelineConfiguration(
                mode: .requireNativeFrame,
                decoderPluginLibraryPaths: ["/tmp/libdecoder.dylib"]
            )
        )
        let surface = PlayerSurfaceView()
        bridge.attachSurfaceHost(surface)

        bridge.initialize()

        XCTAssertEqual(bridge.lastError?.code, .unsupported)
        XCTAssertEqual(bridge.lastError?.category, .capability)
        XCTAssertTrue(bridge.lastError?.message.contains("SourceNormalizer packet-stream") == true)
        XCTAssertTrue(
            bridge.pluginDiagnostics.contains { diagnostic in
                    diagnostic["pluginKind"] as? String == "native_frame_pipeline" &&
                    diagnostic["status"] as? String == "unsupported" &&
                    diagnostic["participation"] as? String == "selected" &&
                    diagnostic["route"] as? String == "sdkManagedNativeFrame" &&
                    diagnostic["failureKind"] as? String == "missingSourceNormalizerPacketPlugin"
            }
        )
    }

    func testNativeFramePipelinePreferFallsBackForHlsSource() {
        let source = try! VesperPlayerSource(
            uri: "https://example.com/live/master.m3u8",
            label: "HLS",
            kind: .remote,
            protocol: .hls
        )
        let bridge = VesperNativePlayerBridge(
            initialSource: source,
            sourceNormalizerConfiguration: VesperSourceNormalizerConfiguration(
                mode: .preflightOnly,
                pluginLibraryPaths: ["/tmp/libsource_normalizer.dylib"]
            ),
            nativeFramePipelineConfiguration: VesperNativeFramePipelineConfiguration(
                mode: .preferNativeFrame,
                decoderPluginLibraryPaths: ["/tmp/libdecoder.dylib"]
            )
        )
        let surface = PlayerSurfaceView()
        bridge.attachSurfaceHost(surface)

        bridge.initialize()

        XCTAssertNil(bridge.lastError)
        XCTAssertNotNil(bridge.routePickerPlayer)
        XCTAssertTrue(
            bridge.pluginDiagnostics.contains { diagnostic in
                diagnostic["pluginKind"] as? String == "native_frame_pipeline" &&
                    diagnostic["status"] as? String == "unsupported" &&
                    diagnostic["participation"] as? String == "fallback" &&
                    diagnostic["route"] as? String == "systemPlayer" &&
                    diagnostic["fallbackKind"] as? String == "unsupportedSource" &&
                    diagnostic["fallbackTargetRoute"] as? String == "systemPlayer" &&
                    (diagnostic["fallbackReason"] as? String)?.contains("HLS") == true &&
                    (diagnostic["fallbackReason"] as? String)?.contains("system playback") == true
            }
        )
    }

    func testNativeFramePipelinePreferFallsBackForDashSource() {
        let source = try! VesperPlayerSource(
            uri: "https://example.com/vod/manifest.mpd",
            label: "DASH",
            kind: .remote,
            protocol: .dash
        )
        let bridge = VesperNativePlayerBridge(
            initialSource: source,
            sourceNormalizerConfiguration: VesperSourceNormalizerConfiguration(
                mode: .preflightOnly,
                pluginLibraryPaths: ["/tmp/libsource_normalizer.dylib"]
            ),
            nativeFramePipelineConfiguration: VesperNativeFramePipelineConfiguration(
                mode: .preferNativeFrame,
                decoderPluginLibraryPaths: ["/tmp/libdecoder.dylib"]
            )
        )
        let surface = PlayerSurfaceView()
        bridge.attachSurfaceHost(surface)

        bridge.initialize()

        XCTAssertNil(bridge.lastError)
        XCTAssertNotNil(bridge.routePickerPlayer)
        XCTAssertTrue(
            bridge.pluginDiagnostics.contains { diagnostic in
                diagnostic["pluginKind"] as? String == "native_frame_pipeline" &&
                    diagnostic["status"] as? String == "unsupported" &&
                    diagnostic["participation"] as? String == "fallback" &&
                    diagnostic["route"] as? String == "systemPlayer" &&
                    diagnostic["fallbackKind"] as? String == "unsupportedSource" &&
                    (diagnostic["fallbackReason"] as? String)?.contains("DASH") == true
            }
        )
    }

    func testNativeFramePipelinePreferFallsBackForContentSource() {
        let source = try! VesperPlayerSource(
            uri: "content://media/external/video/media/42",
            label: "Content",
            kind: .local,
            protocol: .content
        )
        let bridge = VesperNativePlayerBridge(
            initialSource: source,
            sourceNormalizerConfiguration: VesperSourceNormalizerConfiguration(
                mode: .preflightOnly,
                pluginLibraryPaths: ["/tmp/libsource_normalizer.dylib"]
            ),
            nativeFramePipelineConfiguration: VesperNativeFramePipelineConfiguration(
                mode: .preferNativeFrame,
                decoderPluginLibraryPaths: ["/tmp/libdecoder.dylib"]
            )
        )
        let surface = PlayerSurfaceView()
        bridge.attachSurfaceHost(surface)

        bridge.initialize()

        XCTAssertNil(bridge.lastError)
        XCTAssertTrue(
            bridge.pluginDiagnostics.contains { diagnostic in
                diagnostic["pluginKind"] as? String == "native_frame_pipeline" &&
                    diagnostic["status"] as? String == "unsupported" &&
                    diagnostic["participation"] as? String == "fallback" &&
                    diagnostic["route"] as? String == "systemPlayer" &&
                    diagnostic["fallbackKind"] as? String == "unsupportedSource" &&
                    (diagnostic["fallbackReason"] as? String)?.contains("file URLs") == true
            }
        )
    }

    func testNativeFramePipelineRequireFailsForHlsSource() {
        let source = try! VesperPlayerSource(
            uri: "https://example.com/live/master.m3u8",
            label: "HLS",
            kind: .remote,
            protocol: .hls
        )
        let bridge = VesperNativePlayerBridge(
            initialSource: source,
            sourceNormalizerConfiguration: VesperSourceNormalizerConfiguration(
                mode: .preflightOnly,
                pluginLibraryPaths: ["/tmp/libsource_normalizer.dylib"]
            ),
            nativeFramePipelineConfiguration: VesperNativeFramePipelineConfiguration(
                mode: .requireNativeFrame,
                decoderPluginLibraryPaths: ["/tmp/libdecoder.dylib"]
            )
        )
        let surface = PlayerSurfaceView()
        bridge.attachSurfaceHost(surface)

        bridge.initialize()

        XCTAssertEqual(bridge.lastError?.code, .unsupported)
        XCTAssertEqual(bridge.lastError?.category, .capability)
        XCTAssertTrue(bridge.lastError?.message.contains("HLS") == true)
        XCTAssertNil(bridge.routePickerPlayer)
        XCTAssertTrue(
            bridge.pluginDiagnostics.contains { diagnostic in
                diagnostic["pluginKind"] as? String == "native_frame_pipeline" &&
                    diagnostic["status"] as? String == "unsupported" &&
                    diagnostic["participation"] as? String == "selected" &&
                    diagnostic["route"] as? String == "sdkManagedNativeFrame" &&
                    diagnostic["failureKind"] as? String == "unsupportedSource" &&
                    (diagnostic["failureReason"] as? String)?.contains("system playback") == true
            }
        )
    }

    func testNativeFramePipelineRequireWaitsForSurfaceHostBeforeLoading() {
        let source = try! VesperPlayerSource(
            uri: "https://example.com/video.mp4",
            label: "MP4",
            kind: .remote,
            protocol: .progressive
        )
        let bridge = VesperNativePlayerBridge(
            initialSource: source,
            sourceNormalizerConfiguration: VesperSourceNormalizerConfiguration(
                mode: .preflightOnly,
                pluginLibraryPaths: ["/tmp/libsource_normalizer.dylib"]
            ),
            nativeFramePipelineConfiguration: VesperNativeFramePipelineConfiguration(
                mode: .requireNativeFrame,
                decoderPluginLibraryPaths: ["/tmp/libdecoder.dylib"]
            )
        )

        bridge.initialize()

        XCTAssertNil(bridge.lastError)
        XCTAssertTrue(
            bridge.pluginDiagnostics.contains { diagnostic in
                diagnostic["pluginKind"] as? String == "native_frame_pipeline" &&
                    diagnostic["status"] as? String == "loaded" &&
                    diagnostic["participation"] as? String == "selected" &&
                    diagnostic["route"] as? String == "sdkManagedNativeFrame"
            }
        )

        let surface = PlayerSurfaceView()
        bridge.attachSurfaceHost(surface)

        XCTAssertEqual(bridge.lastError?.code, .unsupported)
        XCTAssertEqual(bridge.lastError?.category, .capability)
        XCTAssertTrue(bridge.lastError?.message.contains("SourceNormalizer") == true)
    }

    func testNativeFramePipelineRequireReportsPluginLoadFailureAndClosesSession() {
        let source = try! VesperPlayerSource(
            uri: "https://example.com/video.mp4",
            label: "MP4",
            kind: .remote,
            protocol: .progressive
        )
        let bridge = VesperNativePlayerBridge(
            initialSource: source,
            sourceNormalizerConfiguration: VesperSourceNormalizerConfiguration(
                mode: .preflightOnly,
                pluginLibraryPaths: ["/tmp/libsource_normalizer.dylib"]
            ),
            nativeFramePipelineConfiguration: VesperNativeFramePipelineConfiguration(
                mode: .requireNativeFrame,
                decoderPluginLibraryPaths: ["/tmp/libdecoder.dylib"]
            )
        )
        let surface = PlayerSurfaceView()
        bridge.attachSurfaceHost(surface)

        bridge.initialize()

        XCTAssertEqual(bridge.lastError?.code, .unsupported)
        XCTAssertEqual(bridge.lastError?.category, .capability)
        XCTAssertTrue(bridge.lastError?.message.contains("SourceNormalizer packet plugin") == true)
        XCTAssertTrue(
            bridge.pluginDiagnostics.contains { diagnostic in
                diagnostic["pluginKind"] as? String == "native_frame_pipeline" &&
                    diagnostic["status"] as? String == "loadFailed" &&
                    diagnostic["route"] as? String == "sdkManagedNativeFrame" &&
                    diagnostic["participation"] as? String == "selected" &&
                    diagnostic["failureKind"] as? String == "missingSourceNormalizerPacketPlugin" &&
                    (diagnostic["failureReason"] as? String)?.contains("SourceNormalizer packet plugin") == true
            }
        )
    }

    func testNativeFramePipelineBridgeRoutesRateSeekAndDiagnosticsThroughActiveSession() async {
        let source = try! VesperPlayerSource(
            uri: "file:///tmp/example.mov",
            label: "Local MOV",
            kind: .local,
            protocol: .file
        )
        let backend = TestNativeFramePipelineBackend()
        let audioOutput = TestNativeFrameAudioOutput()
        let coordinator = VesperNativeFramePipelineCoordinator { source, configuration, sourceNormalizer, surfaceHost in
            VesperNativeFramePipelineSession(
                source: source,
                configuration: configuration,
                sourceNormalizer: sourceNormalizer,
                surfaceHost: surfaceHost,
                backend: backend,
                audioOutput: audioOutput
            )
        }
        let bridge = VesperNativePlayerBridge(
            initialSource: source,
            sourceNormalizerConfiguration: VesperSourceNormalizerConfiguration(
                mode: .preflightOnly,
                pluginLibraryPaths: ["/tmp/libsource_normalizer.dylib"]
            ),
            nativeFramePipelineConfiguration: VesperNativeFramePipelineConfiguration(
                mode: .preferNativeFrame,
                decoderPluginLibraryPaths: ["/tmp/libdecoder.dylib"]
            ),
            nativeFramePipelineCoordinator: coordinator
        )
        let surface = PlayerSurfaceView()
        bridge.attachSurfaceHost(surface)

        bridge.initialize()
        bridge.play()
        bridge.setPlaybackRate(1.5)
        bridge.seek(toRatio: 0.5)
        _ = await waitForNativeFrameSmoke(timeout: 1.0) {
            backend.seekRequests == [30_000]
        }

        XCTAssertNil(bridge.lastError)
        XCTAssertEqual(backend.seekRequests, [30_000])
        XCTAssertEqual(bridge.uiState.playbackState, .playing)
        XCTAssertEqual(bridge.uiState.playbackRate, 1.5)
        XCTAssertEqual(bridge.uiState.timeline.positionMs, 30_000)
        XCTAssertEqual(bridge.uiState.timeline.durationMs, 60_000)
        XCTAssertEqual(bridge.uiState.timeline.seekableRange?.endMs, 60_000)
        XCTAssertTrue(bridge.uiState.timeline.isSeekable)
        XCTAssertTrue(
            bridge.pluginDiagnostics.contains { diagnostic in
                diagnostic["pluginKind"] as? String == "native_frame_pipeline" &&
                    diagnostic["route"] as? String == "sdkManagedNativeFrame" &&
                    diagnostic["participation"] as? String == "participated" &&
                    diagnostic["clockSource"] as? String == "swiftNativeAudioBridge" &&
                    diagnostic["audioDecoder"] as? String == "swiftNativeAudioBridge" &&
                    diagnostic["audioOutput"] as? String == "swiftNativeAudioBridge" &&
                    diagnostic["audioPipeline"] as? String == "swiftNativeAudioBridgeV1" &&
                    diagnostic["audioRateControl"] as? String == "swiftNativeAudioBridgeTimePitch" &&
                    diagnostic["selectedVideoStreamIndex"] as? Int == 0 &&
                    diagnostic["selectedVideoMediaKind"] as? String == "video" &&
                    diagnostic["audioStreamIndex"] as? Int == 1 &&
                    diagnostic["audioMediaKind"] as? String == "audio" &&
                    diagnostic["skippedAudioPackets"] as? Int == 2 &&
                    diagnostic["skippedVideoPackets"] as? Int == 1 &&
                    diagnostic["skippedOtherPackets"] as? Int == 3 &&
                    diagnostic["seekable"] as? Bool == true
            }
        )
        XCTAssertTrue(audioOutput.events.contains("rate:1.5"))
        XCTAssertTrue(audioOutput.events.contains("seek:30000"))
        XCTAssertEqual(audioOutput.events.last, "play:1.5")
    }

    func testNativeFramePipelineStopKeepsTimelineSeekable() async {
        let source = try! VesperPlayerSource(
            uri: "file:///tmp/example.mov",
            label: "Local MOV",
            kind: .local,
            protocol: .file
        )
        let backend = TestNativeFramePipelineBackend()
        let audioOutput = TestNativeFrameAudioOutput()
        let coordinator = VesperNativeFramePipelineCoordinator { source, configuration, sourceNormalizer, surfaceHost in
            VesperNativeFramePipelineSession(
                source: source,
                configuration: configuration,
                sourceNormalizer: sourceNormalizer,
                surfaceHost: surfaceHost,
                backend: backend,
                audioOutput: audioOutput
            )
        }
        let bridge = VesperNativePlayerBridge(
            initialSource: source,
            sourceNormalizerConfiguration: VesperSourceNormalizerConfiguration(
                mode: .preflightOnly,
                pluginLibraryPaths: ["/tmp/libsource_normalizer.dylib"]
            ),
            nativeFramePipelineConfiguration: VesperNativeFramePipelineConfiguration(
                mode: .preferNativeFrame,
                decoderPluginLibraryPaths: ["/tmp/libdecoder.dylib"]
            ),
            nativeFramePipelineCoordinator: coordinator
        )

        let surface = PlayerSurfaceView()
        bridge.attachSurfaceHost(surface)
        bridge.initialize()
        bridge.seek(toRatio: 0.5)
        bridge.stop()
        _ = await waitForNativeFrameSmoke(timeout: 1.0) {
            backend.seekRequests == [30_000, 0]
        }

        XCTAssertEqual(bridge.uiState.playbackState, .ready)
        XCTAssertEqual(bridge.uiState.timeline.positionMs, 0)
        XCTAssertEqual(bridge.uiState.timeline.durationMs, 60_000)
        XCTAssertEqual(bridge.uiState.timeline.seekableRange?.endMs, 60_000)
        XCTAssertTrue(bridge.uiState.timeline.isSeekable)
        XCTAssertEqual(backend.seekRequests, [30_000, 0])
        XCTAssertTrue(audioOutput.events.contains("stop"))
        XCTAssertTrue(audioOutput.events.contains("seek:0"))
    }

    func testNativeFramePipelinePendingRatioSeekAppliesAfterSurfaceAttach() async {
        let source = try! VesperPlayerSource(
            uri: "file:///tmp/example.mov",
            label: "Local MOV",
            kind: .local,
            protocol: .file
        )
        let backend = TestNativeFramePipelineBackend()
        let audioOutput = TestNativeFrameAudioOutput()
        let coordinator = VesperNativeFramePipelineCoordinator { source, configuration, sourceNormalizer, surfaceHost in
            VesperNativeFramePipelineSession(
                source: source,
                configuration: configuration,
                sourceNormalizer: sourceNormalizer,
                surfaceHost: surfaceHost,
                backend: backend,
                audioOutput: audioOutput
            )
        }
        let bridge = VesperNativePlayerBridge(
            initialSource: source,
            sourceNormalizerConfiguration: VesperSourceNormalizerConfiguration(
                mode: .preflightOnly,
                pluginLibraryPaths: ["/tmp/libsource_normalizer.dylib"]
            ),
            nativeFramePipelineConfiguration: VesperNativeFramePipelineConfiguration(
                mode: .preferNativeFrame,
                decoderPluginLibraryPaths: ["/tmp/libdecoder.dylib"]
            ),
            nativeFramePipelineCoordinator: coordinator
        )

        bridge.initialize()
        bridge.seek(toRatio: 0.5)
        bridge.attachSurfaceHost(PlayerSurfaceView())
        _ = await waitForNativeFrameSmoke(timeout: 1.0) {
            backend.seekRequests == [30_000]
        }

        XCTAssertEqual(backend.seekRequests, [30_000])
        XCTAssertEqual(bridge.uiState.timeline.positionMs, 30_000)
        XCTAssertEqual(bridge.uiState.timeline.durationMs, 60_000)
        XCTAssertTrue(audioOutput.events.contains("seek:30000"))
        XCTAssertEqual(audioOutput.events.last, "play:1.0")
    }

    func testNativeFramePipelinePendingRelativeSeekAppliesAfterSurfaceAttach() async {
        let source = try! VesperPlayerSource(
            uri: "file:///tmp/example.mov",
            label: "Local MOV",
            kind: .local,
            protocol: .file
        )
        let backend = TestNativeFramePipelineBackend()
        let audioOutput = TestNativeFrameAudioOutput()
        let coordinator = VesperNativeFramePipelineCoordinator { source, configuration, sourceNormalizer, surfaceHost in
            VesperNativeFramePipelineSession(
                source: source,
                configuration: configuration,
                sourceNormalizer: sourceNormalizer,
                surfaceHost: surfaceHost,
                backend: backend,
                audioOutput: audioOutput
            )
        }
        let bridge = VesperNativePlayerBridge(
            initialSource: source,
            sourceNormalizerConfiguration: VesperSourceNormalizerConfiguration(
                mode: .preflightOnly,
                pluginLibraryPaths: ["/tmp/libsource_normalizer.dylib"]
            ),
            nativeFramePipelineConfiguration: VesperNativeFramePipelineConfiguration(
                mode: .preferNativeFrame,
                decoderPluginLibraryPaths: ["/tmp/libdecoder.dylib"]
            ),
            nativeFramePipelineCoordinator: coordinator
        )

        bridge.initialize()
        bridge.seek(by: 12_000)
        bridge.attachSurfaceHost(PlayerSurfaceView())
        _ = await waitForNativeFrameSmoke(timeout: 1.0) {
            backend.seekRequests == [12_000]
        }

        XCTAssertEqual(backend.seekRequests, [12_000])
        XCTAssertEqual(bridge.uiState.timeline.positionMs, 12_000)
        XCTAssertEqual(bridge.uiState.timeline.durationMs, 60_000)
        XCTAssertTrue(audioOutput.events.contains("seek:12000"))
    }

    func testNativeFramePipelinePendingSeekDoesNotLeakAcrossSourceSwitch() {
        let firstSource = try! VesperPlayerSource(
            uri: "file:///tmp/first.mov",
            label: "First MOV",
            kind: .local,
            protocol: .file
        )
        let secondSource = try! VesperPlayerSource(
            uri: "file:///tmp/second.mov",
            label: "Second MOV",
            kind: .local,
            protocol: .file
        )
        let backend = TestNativeFramePipelineBackend()
        let audioOutput = TestNativeFrameAudioOutput()
        let coordinator = VesperNativeFramePipelineCoordinator { source, configuration, sourceNormalizer, surfaceHost in
            VesperNativeFramePipelineSession(
                source: source,
                configuration: configuration,
                sourceNormalizer: sourceNormalizer,
                surfaceHost: surfaceHost,
                backend: backend,
                audioOutput: audioOutput
            )
        }
        let bridge = VesperNativePlayerBridge(
            initialSource: firstSource,
            sourceNormalizerConfiguration: VesperSourceNormalizerConfiguration(
                mode: .preflightOnly,
                pluginLibraryPaths: ["/tmp/libsource_normalizer.dylib"]
            ),
            nativeFramePipelineConfiguration: VesperNativeFramePipelineConfiguration(
                mode: .preferNativeFrame,
                decoderPluginLibraryPaths: ["/tmp/libdecoder.dylib"]
            ),
            nativeFramePipelineCoordinator: coordinator
        )

        bridge.initialize()
        bridge.seek(by: 12_000)
        bridge.selectSource(secondSource)
        let surface = PlayerSurfaceView()
        bridge.attachSurfaceHost(surface)

        XCTAssertEqual(backend.openSourceUris, ["file:///tmp/second.mov"])
        XCTAssertTrue(backend.seekRequests.isEmpty)
        XCTAssertEqual(bridge.uiState.sourceLabel, "Second MOV")
        XCTAssertEqual(bridge.uiState.timeline.positionMs, 0)
        XCTAssertEqual(bridge.uiState.timeline.durationMs, 60_000)
    }

    func testNativeFramePipelineSourceSwitchClosesActiveSessionAndStartsNewSource() async {
        let firstSource = try! VesperPlayerSource(
            uri: "file:///tmp/first.mov",
            label: "First MOV",
            kind: .local,
            protocol: .file
        )
        let secondSource = try! VesperPlayerSource(
            uri: "file:///tmp/second.mov",
            label: "Second MOV",
            kind: .local,
            protocol: .file
        )
        let backend = TestNativeFramePipelineBackend()
        let audioOutput = TestNativeFrameAudioOutput()
        let coordinator = VesperNativeFramePipelineCoordinator { source, configuration, sourceNormalizer, surfaceHost in
            VesperNativeFramePipelineSession(
                source: source,
                configuration: configuration,
                sourceNormalizer: sourceNormalizer,
                surfaceHost: surfaceHost,
                backend: backend,
                audioOutput: audioOutput
            )
        }
        let bridge = VesperNativePlayerBridge(
            initialSource: firstSource,
            sourceNormalizerConfiguration: VesperSourceNormalizerConfiguration(
                mode: .preflightOnly,
                pluginLibraryPaths: ["/tmp/libsource_normalizer.dylib"]
            ),
            nativeFramePipelineConfiguration: VesperNativeFramePipelineConfiguration(
                mode: .preferNativeFrame,
                decoderPluginLibraryPaths: ["/tmp/libdecoder.dylib"]
            ),
            nativeFramePipelineCoordinator: coordinator
        )

        let surface = PlayerSurfaceView()
        bridge.attachSurfaceHost(surface)
        bridge.initialize()
        bridge.seek(toRatio: 0.5)
        bridge.selectSource(secondSource)
        _ = await waitForNativeFrameSmoke(timeout: 1.0) {
            backend.closeHandles == [42]
        }

        XCTAssertEqual(backend.openSourceUris, [
            "file:///tmp/first.mov",
            "file:///tmp/second.mov",
        ])
        XCTAssertEqual(backend.closeHandles, [42])
        XCTAssertTrue(backend.seekRequests.isEmpty)
        XCTAssertEqual(bridge.uiState.sourceLabel, "Second MOV")
        XCTAssertEqual(bridge.uiState.timeline.positionMs, 0)
        XCTAssertEqual(bridge.uiState.timeline.durationMs, 60_000)
        XCTAssertEqual(bridge.uiState.playbackState, .playing)
    }

    func testNativeFramePipelineSurfaceHostChangeRebindsWithoutReloadingSession() {
        let source = try! VesperPlayerSource(
            uri: "file:///tmp/example.mov",
            label: "Local MOV",
            kind: .local,
            protocol: .file
        )
        let backend = TestNativeFramePipelineBackend()
        let audioOutput = TestNativeFrameAudioOutput()
        let coordinator = VesperNativeFramePipelineCoordinator { source, configuration, sourceNormalizer, surfaceHost in
            VesperNativeFramePipelineSession(
                source: source,
                configuration: configuration,
                sourceNormalizer: sourceNormalizer,
                surfaceHost: surfaceHost,
                backend: backend,
                audioOutput: audioOutput
            )
        }
        let bridge = VesperNativePlayerBridge(
            initialSource: source,
            sourceNormalizerConfiguration: VesperSourceNormalizerConfiguration(
                mode: .preflightOnly,
                pluginLibraryPaths: ["/tmp/libsource_normalizer.dylib"]
            ),
            nativeFramePipelineConfiguration: VesperNativeFramePipelineConfiguration(
                mode: .preferNativeFrame,
                decoderPluginLibraryPaths: ["/tmp/libdecoder.dylib"]
            ),
            nativeFramePipelineCoordinator: coordinator
        )

        let firstSurface = PlayerSurfaceView()
        let secondSurface = PlayerSurfaceView()
        bridge.attachSurfaceHost(firstSurface)
        bridge.initialize()

        bridge.attachSurfaceHost(secondSurface)

        XCTAssertEqual(backend.openSourceUris, ["file:///tmp/example.mov"])
        XCTAssertEqual(backend.closeHandles, [])
        XCTAssertTrue(coordinator.activeSession?.surfaceHost === secondSurface)
        XCTAssertEqual(bridge.uiState.playbackState, .playing)
    }

    func testNativeFramePipelineSurfaceDetachClosesAndReattachRestoresPlayingSession() async {
        let source = try! VesperPlayerSource(
            uri: "file:///tmp/example.mov",
            label: "Local MOV",
            kind: .local,
            protocol: .file
        )
        let backend = TestNativeFramePipelineBackend()
        let audioOutput = TestNativeFrameAudioOutput()
        let coordinator = VesperNativeFramePipelineCoordinator { source, configuration, sourceNormalizer, surfaceHost in
            VesperNativeFramePipelineSession(
                source: source,
                configuration: configuration,
                sourceNormalizer: sourceNormalizer,
                surfaceHost: surfaceHost,
                backend: backend,
                audioOutput: audioOutput
            )
        }
        let bridge = VesperNativePlayerBridge(
            initialSource: source,
            sourceNormalizerConfiguration: VesperSourceNormalizerConfiguration(
                mode: .preflightOnly,
                pluginLibraryPaths: ["/tmp/libsource_normalizer.dylib"]
            ),
            nativeFramePipelineConfiguration: VesperNativeFramePipelineConfiguration(
                mode: .preferNativeFrame,
                decoderPluginLibraryPaths: ["/tmp/libdecoder.dylib"]
            ),
            nativeFramePipelineCoordinator: coordinator
        )

        let firstSurface = PlayerSurfaceView()
        bridge.attachSurfaceHost(firstSurface)
        bridge.initialize()
        bridge.setPlaybackRate(1.5)
        bridge.detachSurfaceHost()
        _ = await waitForNativeFrameSmoke(timeout: 1.0) {
            backend.closeHandles == [42]
        }

        XCTAssertEqual(backend.closeHandles, [42])

        let secondSurface = PlayerSurfaceView()
        bridge.attachSurfaceHost(secondSurface)

        XCTAssertEqual(backend.openSourceUris, [
            "file:///tmp/example.mov",
            "file:///tmp/example.mov",
        ])
        XCTAssertEqual(bridge.uiState.playbackState, .playing)
        XCTAssertEqual(bridge.uiState.playbackRate, 1.5)
        XCTAssertEqual(bridge.uiState.timeline.durationMs, 60_000)
        XCTAssertTrue(audioOutput.events.contains("close"))
        XCTAssertEqual(audioOutput.events.last, "play:1.5")
    }

    func testNativeFramePipelineCoordinatorPreparesSessionWhenCapabilitiesExist() {
        let source = try! VesperPlayerSource(
            uri: "https://example.com/video.mp4",
            label: "MP4",
            kind: .remote,
            protocol: .progressive
        )
        let coordinator = VesperNativeFramePipelineCoordinator()
        let surface = PlayerSurfaceView()
        let decision = coordinator.evaluateRoute(
            for: source,
            configuration: VesperNativeFramePipelineConfiguration(
                mode: .preferNativeFrame,
                decoderPluginLibraryPaths: ["/tmp/libdecoder.dylib"],
                frameProcessorPluginLibraryPaths: ["/tmp/libframe.dylib"],
                maxInFlightFrames: 2
            ),
            sourceNormalizer: VesperSourceNormalizerConfiguration(
                mode: .preflightOnly,
                pluginLibraryPaths: ["/tmp/libsource_normalizer.dylib"]
            ),
            surfaceHost: surface
        )

        XCTAssertEqual(decision, .nativeFrame)
        XCTAssertNotNil(coordinator.activeSession)
        XCTAssertTrue(
            coordinator.makeDiagnostics(
                configuration: VesperNativeFramePipelineConfiguration(
                    mode: .preferNativeFrame,
                    decoderPluginLibraryPaths: ["/tmp/libdecoder.dylib"]
                )
            ).contains { diagnostic in
                diagnostic["route"] as? String == "sdkManagedNativeFrame" &&
                    diagnostic["participation"] as? String == "selected" &&
                    diagnostic["sourceInput"] as? String == "sourceNormalizerPacket"
            }
        )
    }

    func testNativeFramePipelineCoordinatorReportsMissingSurfaceAsPendingIssue() {
        let source = try! VesperPlayerSource(
            uri: "https://example.com/video.mp4",
            label: "MP4",
            kind: .remote,
            protocol: .progressive
        )
        let coordinator = VesperNativeFramePipelineCoordinator()
        let configuration = VesperNativeFramePipelineConfiguration(
            mode: .requireNativeFrame,
            decoderPluginLibraryPaths: ["/tmp/libdecoder.dylib"]
        )
        let sourceNormalizer = VesperSourceNormalizerConfiguration(
            mode: .preflightOnly,
            pluginLibraryPaths: ["/tmp/libsource_normalizer.dylib"]
        )

        let decision = coordinator.evaluateRoute(
            for: source,
            configuration: configuration,
            sourceNormalizer: sourceNormalizer,
            surfaceHost: nil
        )

        XCTAssertEqual(
            decision,
            .waitForSurface(
                VesperNativeFramePipelineIssue(
                    kind: .missingSurface,
                    message: "iOS native-frame pipeline requires an attached PlayerSurfaceView before source load."
                )
            )
        )
        XCTAssertTrue(
            coordinator.makeDiagnostics(configuration: configuration).contains { diagnostic in
                diagnostic["pluginKind"] as? String == "native_frame_pipeline" &&
                    diagnostic["status"] as? String == "loaded" &&
                    diagnostic["participation"] as? String == "selected" &&
                    diagnostic["route"] as? String == "sdkManagedNativeFrame" &&
                    diagnostic["pendingKind"] as? String == "missingSurface"
            }
        )
    }

    func testNativeFramePipelineCoordinatorStartFailsWhenConfiguredPluginCannotLoad() {
        let source = try! VesperPlayerSource(
            uri: "https://example.com/video.mp4",
            label: "MP4",
            kind: .remote,
            protocol: .progressive
        )
        let coordinator = VesperNativeFramePipelineCoordinator()
        let surface = PlayerSurfaceView()
        _ = coordinator.evaluateRoute(
            for: source,
            configuration: VesperNativeFramePipelineConfiguration(
                mode: .requireNativeFrame,
                decoderPluginLibraryPaths: ["/tmp/libdecoder.dylib"]
            ),
            sourceNormalizer: VesperSourceNormalizerConfiguration(
                mode: .preflightOnly,
                pluginLibraryPaths: ["/tmp/libsource_normalizer.dylib"]
            ),
            surfaceHost: surface
        )

        let startup = coordinator.startActiveSession()

        if case .failure(let error) = startup {
            XCTAssertTrue(error.message.contains("SourceNormalizer packet plugin"))
        } else {
            XCTFail("expected native-frame startup to fail without loadable packet and decoder plugins")
        }
        XCTAssertNotNil(coordinator.activeSession)
        XCTAssertTrue(
            coordinator.makeDiagnostics(
                configuration: VesperNativeFramePipelineConfiguration(
                    mode: .requireNativeFrame,
                    decoderPluginLibraryPaths: ["/tmp/libdecoder.dylib"]
                )
            ).contains { diagnostic in
                diagnostic["status"] as? String == "loadFailed" &&
                    diagnostic["failureKind"] as? String == "missingSourceNormalizerPacketPlugin" &&
                    (diagnostic["failureReason"] as? String)?.contains("SourceNormalizer packet plugin") == true
            }
        )
        coordinator.closeActiveSession()
        XCTAssertNil(coordinator.activeSession)
    }

    func testNativeFramePipelineDiagnosticsReportSwiftNativeAudioBridgeClock() {
        let source = try! VesperPlayerSource(
            uri: "file:///tmp/example.mov",
            label: "Local MOV",
            kind: .local,
            protocol: .file
        )
        let coordinator = VesperNativeFramePipelineCoordinator()
        let configuration = VesperNativeFramePipelineConfiguration(
            mode: .preferNativeFrame,
            decoderPluginLibraryPaths: ["/tmp/libdecoder.dylib"]
        )
        let surface = PlayerSurfaceView()

        let decision = coordinator.evaluateRoute(
            for: source,
            configuration: configuration,
            sourceNormalizer: VesperSourceNormalizerConfiguration(
                mode: .preflightOnly,
                pluginLibraryPaths: ["/tmp/libsource_normalizer.dylib"]
            ),
            surfaceHost: surface
        )
        XCTAssertEqual(decision, .nativeFrame)

        coordinator.activeSession?.applyAudioBridgeState(
            VesperNativeFrameAudioBridgeState.resolved(
                hasAudioTrack: true,
                bridgePrepared: true
            )
        )

        XCTAssertTrue(
            coordinator.makeDiagnostics(configuration: configuration).contains { diagnostic in
                diagnostic["pluginKind"] as? String == "native_frame_pipeline" &&
                    diagnostic["audioDecoder"] as? String == "swiftNativeAudioBridge" &&
                    diagnostic["audioOutput"] as? String == "swiftNativeAudioBridge" &&
                    diagnostic["clockSource"] as? String == "swiftNativeAudioBridge" &&
                    diagnostic["audioPipeline"] as? String == "swiftNativeAudioBridgeV1" &&
                    diagnostic["audioRateControl"] as? String == "swiftNativeAudioBridgeTimePitch" &&
                    diagnostic["hasAudioTrack"] as? Bool == true &&
                    diagnostic["audioOutputIssue"] == nil
            }
        )
    }

    func testNativeFramePipelineDiagnosticsReportSwiftAudioUnavailable() {
        let source = try! VesperPlayerSource(
            uri: "https://example.com/video.mp4",
            label: "Remote MP4",
            kind: .remote,
            protocol: .progressive
        )
        let coordinator = VesperNativeFramePipelineCoordinator()
        let configuration = VesperNativeFramePipelineConfiguration(
            mode: .preferNativeFrame,
            decoderPluginLibraryPaths: ["/tmp/libdecoder.dylib"]
        )
        let surface = PlayerSurfaceView()

        let decision = coordinator.evaluateRoute(
            for: source,
            configuration: configuration,
            sourceNormalizer: VesperSourceNormalizerConfiguration(
                mode: .preflightOnly,
                pluginLibraryPaths: ["/tmp/libsource_normalizer.dylib"]
            ),
            surfaceHost: surface
        )
        XCTAssertEqual(decision, .nativeFrame)

        coordinator.activeSession?.applyAudioBridgeState(
            VesperNativeFrameAudioBridgeState.resolved(
                hasAudioTrack: true,
                bridgePrepared: false,
                unavailableReason: "Swift native audio bridge v1 only supports local file sources."
            )
        )

        XCTAssertTrue(
            coordinator.makeDiagnostics(configuration: configuration).contains { diagnostic in
                diagnostic["pluginKind"] as? String == "native_frame_pipeline" &&
                    diagnostic["audioDecoder"] as? String == "unavailable" &&
                    diagnostic["audioOutput"] as? String == "unavailable" &&
                    diagnostic["clockSource"] as? String == "video" &&
                    diagnostic["audioPipeline"] as? String == "swiftNativeAudioBridgeV1" &&
                    diagnostic["audioRateControl"] as? String == "unavailable" &&
                    diagnostic["hasAudioTrack"] as? Bool == true &&
                    (diagnostic["audioOutputIssue"] as? String)?
                        .contains("local file sources") == true
            }
        )
    }

    func testNativeFramePipelineStartupFailsWhenAudioTrackBridgeUnavailable() {
        let source = try! VesperPlayerSource(
            uri: "file:///tmp/example.mov",
            label: "Local MOV",
            kind: .local,
            protocol: .file
        )
        let backend = TestNativeFramePipelineBackend()
        let audioOutput = TestNativeFrameAudioOutput()
        audioOutput.prepareResult = VesperNativeFrameAudioBridgeState.resolved(
            hasAudioTrack: true,
            bridgePrepared: false,
            unavailableReason: "Swift native audio bridge preflight failed in test"
        )
        let session = VesperNativeFramePipelineSession(
            source: source,
            configuration: VesperNativeFramePipelineConfiguration(
                mode: .preferNativeFrame,
                decoderPluginLibraryPaths: ["/tmp/libdecoder.dylib"]
            ),
            sourceNormalizer: VesperSourceNormalizerConfiguration(
                mode: .preflightOnly,
                pluginLibraryPaths: ["/tmp/libsource_normalizer.dylib"]
            ),
            surfaceHost: PlayerSurfaceView(),
            backend: backend,
            audioOutput: audioOutput
        )

        guard case .failure(let error) = session.start() else {
            XCTFail("expected native-frame startup to fail when audio bridge is unavailable")
            return
        }
        XCTAssertEqual(error.issue.kind, .nativeAudioBridgeUnavailable)
        XCTAssertTrue(error.message.contains("preflight failed"))
        XCTAssertEqual(session.clockSource, "video")
        XCTAssertEqual(session.audioOutputKind, "unavailable")
        XCTAssertEqual(backend.closeHandles, [42])
    }

    func testNativeFramePipelineSeekRestoresPlayingStateRateAndTimeline() async {
        let source = try! VesperPlayerSource(
            uri: "file:///tmp/example.mov",
            label: "Local MOV",
            kind: .local,
            protocol: .file
        )
        let backend = TestNativeFramePipelineBackend()
        let audioOutput = TestNativeFrameAudioOutput()
        let session = VesperNativeFramePipelineSession(
            source: source,
            configuration: VesperNativeFramePipelineConfiguration(
                mode: .preferNativeFrame,
                decoderPluginLibraryPaths: ["/tmp/libdecoder.dylib"]
            ),
            sourceNormalizer: VesperSourceNormalizerConfiguration(
                mode: .preflightOnly,
                pluginLibraryPaths: ["/tmp/libsource_normalizer.dylib"]
            ),
            surfaceHost: PlayerSurfaceView(),
            backend: backend,
            audioOutput: audioOutput
        )
        var timelines: [VesperNativeFramePipelineTimeline] = []
        session.onFramePresented = { timelines.append($0) }

        guard case .success = session.start() else {
            XCTFail("expected fake native-frame session to start")
            return
        }
        session.play(rate: 1.5)

        XCTAssertTrue(session.seek(toMs: 12_345))
        _ = await waitForNativeFrameSmoke(timeout: 1.0) {
            backend.seekRequests == [12_345]
        }

        XCTAssertEqual(backend.seekRequests, [12_345])
        XCTAssertEqual(audioOutput.events, [
            "prepare:true",
            "play:1.5",
            "pause",
            "seek:12345",
            "play:1.5",
        ])
        XCTAssertEqual(
            timelines.last,
            VesperNativeFramePipelineTimeline(positionMs: 12_345, durationMs: 60_000)
        )
        XCTAssertEqual(session.durationMs, 60_000)
        XCTAssertTrue(session.seekable)
        XCTAssertEqual(session.clockSource, "swiftNativeAudioBridge")
        XCTAssertEqual(session.audioDecoderKind, "swiftNativeAudioBridge")
        XCTAssertEqual(session.audioPipelineKind, "swiftNativeAudioBridgeV1")
    }

    func testNativeFramePipelineSeekClampsNegativePositionToStart() async {
        let source = try! VesperPlayerSource(
            uri: "file:///tmp/example.mov",
            label: "Local MOV",
            kind: .local,
            protocol: .file
        )
        let backend = TestNativeFramePipelineBackend()
        let audioOutput = TestNativeFrameAudioOutput()
        let session = VesperNativeFramePipelineSession(
            source: source,
            configuration: VesperNativeFramePipelineConfiguration(
                mode: .preferNativeFrame,
                decoderPluginLibraryPaths: ["/tmp/libdecoder.dylib"]
            ),
            sourceNormalizer: VesperSourceNormalizerConfiguration(
                mode: .preflightOnly,
                pluginLibraryPaths: ["/tmp/libsource_normalizer.dylib"]
            ),
            surfaceHost: PlayerSurfaceView(),
            backend: backend,
            audioOutput: audioOutput
        )
        var timelines: [VesperNativeFramePipelineTimeline] = []
        session.onFramePresented = { timelines.append($0) }

        guard case .success = session.start() else {
            XCTFail("expected fake native-frame session to start")
            return
        }

        XCTAssertTrue(session.seek(toMs: -1_000))
        _ = await waitForNativeFrameSmoke(timeout: 1.0) {
            backend.seekRequests == [0]
        }

        XCTAssertEqual(backend.seekRequests, [0])
        XCTAssertTrue(audioOutput.events.contains("seek:0"))
        XCTAssertEqual(
            timelines.last,
            VesperNativeFramePipelineTimeline(positionMs: 0, durationMs: 60_000)
        )
    }

    func testNativeFramePipelineSeekClampsPastDurationToEnd() async {
        let source = try! VesperPlayerSource(
            uri: "file:///tmp/example.mov",
            label: "Local MOV",
            kind: .local,
            protocol: .file
        )
        let backend = TestNativeFramePipelineBackend()
        let audioOutput = TestNativeFrameAudioOutput()
        let session = VesperNativeFramePipelineSession(
            source: source,
            configuration: VesperNativeFramePipelineConfiguration(
                mode: .preferNativeFrame,
                decoderPluginLibraryPaths: ["/tmp/libdecoder.dylib"]
            ),
            sourceNormalizer: VesperSourceNormalizerConfiguration(
                mode: .preflightOnly,
                pluginLibraryPaths: ["/tmp/libsource_normalizer.dylib"]
            ),
            surfaceHost: PlayerSurfaceView(),
            backend: backend,
            audioOutput: audioOutput
        )
        var timelines: [VesperNativeFramePipelineTimeline] = []
        session.onFramePresented = { timelines.append($0) }

        guard case .success = session.start() else {
            XCTFail("expected fake native-frame session to start")
            return
        }

        XCTAssertTrue(session.seek(toMs: 90_000))
        _ = await waitForNativeFrameSmoke(timeout: 1.0) {
            backend.seekRequests == [60_000]
        }

        XCTAssertEqual(backend.seekRequests, [60_000])
        XCTAssertTrue(audioOutput.events.contains("seek:60000"))
        XCTAssertEqual(
            timelines.last,
            VesperNativeFramePipelineTimeline(positionMs: 60_000, durationMs: 60_000)
        )
    }

    func testNativeFramePipelineSeekFailureRestoresPlaybackWithoutTimelineMutation() async {
        let source = try! VesperPlayerSource(
            uri: "file:///tmp/example.mov",
            label: "Local MOV",
            kind: .local,
            protocol: .file
        )
        let backend = TestNativeFramePipelineBackend()
        backend.seekResult = .failure(
            VesperNativeFramePipelineOperationError(message: "seek failed in fake backend")
        )
        let audioOutput = TestNativeFrameAudioOutput()
        let session = VesperNativeFramePipelineSession(
            source: source,
            configuration: VesperNativeFramePipelineConfiguration(
                mode: .preferNativeFrame,
                decoderPluginLibraryPaths: ["/tmp/libdecoder.dylib"]
            ),
            sourceNormalizer: VesperSourceNormalizerConfiguration(
                mode: .preflightOnly,
                pluginLibraryPaths: ["/tmp/libsource_normalizer.dylib"]
            ),
            surfaceHost: PlayerSurfaceView(),
            backend: backend,
            audioOutput: audioOutput
        )
        var timelines: [VesperNativeFramePipelineTimeline] = []
        session.onFramePresented = { timelines.append($0) }

        guard case .success = session.start() else {
            XCTFail("expected fake native-frame session to start")
            return
        }
        session.play(rate: 2.0)

        XCTAssertTrue(session.seek(toMs: 22_000))
        _ = await waitForNativeFrameSmoke(timeout: 1.0) {
            backend.seekRequests == [22_000] && audioOutput.events.last == "play:2.0"
        }

        XCTAssertEqual(backend.seekRequests, [22_000])
        XCTAssertEqual(audioOutput.events, [
            "prepare:true",
            "play:2.0",
            "pause",
            "play:2.0",
        ])
        XCTAssertTrue(timelines.isEmpty)
    }

    func testNativeFramePipelineFlushStopsPlaybackAndAudioOutput() async {
        let source = try! VesperPlayerSource(
            uri: "file:///tmp/example.mov",
            label: "Local MOV",
            kind: .local,
            protocol: .file
        )
        let backend = TestNativeFramePipelineBackend()
        let audioOutput = TestNativeFrameAudioOutput()
        let session = VesperNativeFramePipelineSession(
            source: source,
            configuration: VesperNativeFramePipelineConfiguration(
                mode: .preferNativeFrame,
                decoderPluginLibraryPaths: ["/tmp/libdecoder.dylib"]
            ),
            sourceNormalizer: VesperSourceNormalizerConfiguration(
                mode: .preflightOnly,
                pluginLibraryPaths: ["/tmp/libsource_normalizer.dylib"]
            ),
            surfaceHost: PlayerSurfaceView(),
            backend: backend,
            audioOutput: audioOutput
        )

        guard case .success = session.start() else {
            XCTFail("expected fake native-frame session to start")
            return
        }
        session.play(rate: 1.25)

        session.flush()
        _ = await waitForNativeFrameSmoke(timeout: 1.0) {
            backend.flushRequests == [42]
        }

        XCTAssertEqual(backend.flushRequests, [42])
        XCTAssertEqual(audioOutput.events, [
            "prepare:true",
            "play:1.25",
            "pause",
        ])
        XCTAssertEqual(session.durationMs, 60_000)
        XCTAssertTrue(session.seekable)
    }

    func testNativeFramePipelineTimelinePrefersSwiftAudioClockWhenAvailable() {
        let source = try! VesperPlayerSource(
            uri: "file:///tmp/example.mov",
            label: "Local MOV",
            kind: .local,
            protocol: .file
        )
        let backend = TestNativeFramePipelineBackend()
        let audioOutput = TestNativeFrameAudioOutput()
        let session = VesperNativeFramePipelineSession(
            source: source,
            configuration: VesperNativeFramePipelineConfiguration(
                mode: .preferNativeFrame,
                decoderPluginLibraryPaths: ["/tmp/libdecoder.dylib"]
            ),
            sourceNormalizer: VesperSourceNormalizerConfiguration(
                mode: .preflightOnly,
                pluginLibraryPaths: ["/tmp/libsource_normalizer.dylib"]
            ),
            surfaceHost: PlayerSurfaceView(),
            backend: backend,
            audioOutput: audioOutput
        )

        guard case .success = session.start() else {
            XCTFail("expected fake native-frame session to start")
            return
        }
        audioOutput.currentPositionMs = 24_000

        XCTAssertEqual(
            session.timelinePositionMs(framePresentationTimeUs: 12_000_000),
            24_000
        )
    }

    func testNativeFramePipelineRuntimeAudioFailureReportsPlaybackFailure() {
        let source = try! VesperPlayerSource(
            uri: "file:///tmp/example.mov",
            label: "Local MOV",
            kind: .local,
            protocol: .file
        )
        let backend = TestNativeFramePipelineBackend()
        let audioOutput = TestNativeFrameAudioOutput()
        let session = VesperNativeFramePipelineSession(
            source: source,
            configuration: VesperNativeFramePipelineConfiguration(
                mode: .preferNativeFrame,
                decoderPluginLibraryPaths: ["/tmp/libdecoder.dylib"]
            ),
            sourceNormalizer: VesperSourceNormalizerConfiguration(
                mode: .preflightOnly,
                pluginLibraryPaths: ["/tmp/libsource_normalizer.dylib"]
            ),
            surfaceHost: PlayerSurfaceView(),
            backend: backend,
            audioOutput: audioOutput
        )
        var failedIssue: VesperNativeFramePipelineIssue?
        session.onPlaybackFailed = { failedIssue = $0 }

        guard case .success = session.start() else {
            XCTFail("expected fake native-frame session to start")
            return
        }
        audioOutput.currentPositionMs = 24_000
        XCTAssertEqual(session.clockSource, "swiftNativeAudioBridge")
        XCTAssertEqual(
            session.timelinePositionMs(framePresentationTimeUs: 12_000_000),
            24_000
        )

        audioOutput.emitState(
            VesperNativeFrameAudioBridgeState.resolved(
                hasAudioTrack: true,
                bridgePrepared: false,
                unavailableReason: "Swift native audio bridge decode failed in test"
            )
        )
        audioOutput.currentPositionMs = 48_000

        XCTAssertEqual(session.clockSource, "video")
        XCTAssertEqual(session.audioOutputKind, "unavailable")
        XCTAssertTrue(session.audioOutputIssue?.contains("decode failed") == true)
        XCTAssertEqual(failedIssue?.kind, .nativeAudioBridgeUnavailable)
        XCTAssertTrue(failedIssue?.message.contains("decode failed") == true)
    }

    func testNativeFramePipelinePlaybackAdvancesPresentsReleasesAndUpdatesAudioClockTimeline() async {
        let source = try! VesperPlayerSource(
            uri: "file:///tmp/example.mov",
            label: "Local MOV",
            kind: .local,
            protocol: .file
        )
        let backend = TestNativeFramePipelineBackend()
        backend.advanceResults = [
            .success([
                "status": "frame",
                "handle": NSNumber(value: UInt64(7)),
                "pixelBuffer": NSNumber(value: UInt(0x1234)),
                "presentationTimeUs": NSNumber(value: Int64(12_000_000)),
                "durationUs": NSNumber(value: Int64(33_333)),
                "width": NSNumber(value: 320),
                "height": NSNumber(value: 180),
                "counters": [
                    "processedFrames": 1,
                ],
            ]),
        ]
        backend.releaseResult = [
            "counters": [
                "presentedFrames": 1,
            ],
        ]
        let audioOutput = TestNativeFrameAudioOutput()
        audioOutput.currentPositionMs = 24_000
        let presenter = TestNativeFramePresenter()
        let session = VesperNativeFramePipelineSession(
            source: source,
            configuration: VesperNativeFramePipelineConfiguration(
                mode: .preferNativeFrame,
                decoderPluginLibraryPaths: ["/tmp/libdecoder.dylib"]
            ),
            sourceNormalizer: VesperSourceNormalizerConfiguration(
                mode: .preflightOnly,
                pluginLibraryPaths: ["/tmp/libsource_normalizer.dylib"]
            ),
            surfaceHost: PlayerSurfaceView(),
            backend: backend,
            audioOutput: audioOutput,
            nativeFramePresenter: presenter
        )
        let presentedFrame = expectation(description: "native-frame session presented one frame")
        var timelines: [VesperNativeFramePipelineTimeline] = []
        session.onFramePresented = { timeline in
            timelines.append(timeline)
            presentedFrame.fulfill()
        }

        guard case .success = session.start() else {
            XCTFail("expected fake native-frame session to start")
            return
        }
        session.play(rate: 1.25)

        await fulfillment(of: [presentedFrame], timeout: 1.0)
        session.close()

        XCTAssertFalse(backend.advanceRequests.isEmpty)
        XCTAssertEqual(presenter.presentedPixelBufferAddresses, [0x1234])
        XCTAssertEqual(backend.releasedFrameHandles, [7])
        XCTAssertEqual(backend.releasePresentedFlags, [true])
        XCTAssertEqual(
            timelines.last,
            VesperNativeFramePipelineTimeline(positionMs: 24_000, durationMs: 60_000)
        )
        XCTAssertEqual(session.counters.processedFrames, 1)
        XCTAssertEqual(session.counters.presentedFrames, 1)
        XCTAssertTrue(audioOutput.events.contains("play:1.25"))
        XCTAssertEqual(presenter.enabledStates.last, false)
    }

    func testNativeFramePipelineEndOfStreamStopsLoopAndReportsPlaybackEnded() async {
        let source = try! VesperPlayerSource(
            uri: "file:///tmp/example.mov",
            label: "Local MOV",
            kind: .local,
            protocol: .file
        )
        let backend = TestNativeFramePipelineBackend()
        backend.advanceResults = [
            .success([
                "status": "frame",
                "handle": NSNumber(value: UInt64(7)),
                "pixelBuffer": NSNumber(value: UInt(0x1234)),
                "presentationTimeUs": NSNumber(value: Int64(59_000_000)),
                "durationUs": NSNumber(value: Int64(33_333)),
                "width": NSNumber(value: 320),
                "height": NSNumber(value: 180),
                "counters": ["processedFrames": 1],
            ]),
            .success(["status": "endOfStream", "counters": ["presentedFrames": 1]]),
        ]
        let audioOutput = TestNativeFrameAudioOutput()
        let presenter = TestNativeFramePresenter()
        let session = VesperNativeFramePipelineSession(
            source: source,
            configuration: VesperNativeFramePipelineConfiguration(
                mode: .preferNativeFrame,
                decoderPluginLibraryPaths: ["/tmp/libdecoder.dylib"]
            ),
            sourceNormalizer: VesperSourceNormalizerConfiguration(
                mode: .preflightOnly,
                pluginLibraryPaths: ["/tmp/libsource_normalizer.dylib"]
            ),
            surfaceHost: PlayerSurfaceView(),
            backend: backend,
            audioOutput: audioOutput,
            nativeFramePresenter: presenter
        )
        let playbackEnded = expectation(description: "native-frame session reported end of playback")
        session.onPlaybackEnded = { playbackEnded.fulfill() }

        guard case .success = session.start() else {
            XCTFail("expected fake native-frame session to start")
            return
        }
        session.play(rate: 1.0)

        await fulfillment(of: [playbackEnded], timeout: 1.0)
        // Confirm the loop stopped polling once end-of-stream was reported.
        let advanceCountAtEnd = backend.advanceRequests.count
        try? await Task.sleep(nanoseconds: 50_000_000)
        session.close()

        XCTAssertEqual(
            backend.advanceRequests.count,
            advanceCountAtEnd,
            "display loop must stop polling once end-of-stream is reached"
        )
        XCTAssertTrue(audioOutput.events.contains("pause"))
    }

    func testNativeFramePipelineSeekInvalidatesFrameAlreadyOwnedByDisplayLoop() async {
        let source = try! VesperPlayerSource(
            uri: "file:///tmp/example.mov",
            label: "Local MOV",
            kind: .local,
            protocol: .file
        )
        let backend = TestNativeFramePipelineBackend()
        backend.advanceResults = [
            .success([
                "status": "frame",
                "handle": NSNumber(value: UInt64(7)),
                "pixelBuffer": NSNumber(value: UInt(0x1234)),
                "presentationTimeUs": NSNumber(value: Int64(12_000_000)),
                "durationUs": NSNumber(value: Int64(33_333)),
                "width": NSNumber(value: 320),
                "height": NSNumber(value: 180),
                "counters": [
                    "processedFrames": 1,
                ],
            ]),
        ]
        let audioOutput = TestNativeFrameAudioOutput()
        let presenter = TestNativeFramePresenter()
        let session = VesperNativeFramePipelineSession(
            source: source,
            configuration: VesperNativeFramePipelineConfiguration(
                mode: .preferNativeFrame,
                decoderPluginLibraryPaths: ["/tmp/libdecoder.dylib"]
            ),
            sourceNormalizer: VesperSourceNormalizerConfiguration(
                mode: .preflightOnly,
                pluginLibraryPaths: ["/tmp/libsource_normalizer.dylib"]
            ),
            surfaceHost: PlayerSurfaceView(),
            backend: backend,
            audioOutput: audioOutput,
            nativeFramePresenter: presenter
        )
        defer {
            presenter.resumePresentation()
            session.close()
        }

        guard case .success = session.start() else {
            XCTFail("expected fake native-frame session to start")
            return
        }
        presenter.holdPresentation = true
        session.play()
        let presentationStarted = await waitForNativeFrameSmoke(timeout: 1.0) {
            presenter.presentedPixelBufferAddresses == [0x1234]
        }
        XCTAssertTrue(presentationStarted, "display loop did not pick up the fake frame")

        XCTAssertTrue(session.seek(toMs: 12_345))
        presenter.resumePresentation()
        _ = await waitForNativeFrameSmoke(timeout: 1.0) {
            backend.seekRequests == [12_345] &&
                backend.releasedFrameHandles == [7] &&
                backend.releasePresentedFlags == [false]
        }

        XCTAssertEqual(backend.seekRequests, [12_345])
        XCTAssertEqual(backend.releasedFrameHandles, [7])
        XCTAssertEqual(backend.releasePresentedFlags, [false])
        XCTAssertEqual(session.counters.presentedFrames, 0)
    }

    func testNativeFramePipelineRealPluginPlaybackPresentsSeeksAndReleasesLocalMp4() async throws {
        let smoke = try Self.nativeFrameSmokeConfiguration()
        let source = try VesperPlayerSource(
            uri: URL(fileURLWithPath: smoke.sourcePath).absoluteString,
            label: "Native Frame Smoke MP4",
            kind: .local,
            protocol: .file
        )
        let surfaceView = PlayerSurfaceView(frame: CGRect(x: 0, y: 0, width: 320, height: 180))
        guard surfaceView.supportsNativeFrameMetalPresentation else {
            throw XCTSkip("Metal is unavailable in this iOS Simulator runtime.")
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

        let presenter = RecordingNativeFramePresenter(wrapped: surfaceView)
        let session = VesperNativeFramePipelineSession(
            source: source,
            configuration: VesperNativeFramePipelineConfiguration(
                mode: .requireNativeFrame,
                decoderPluginLibraryPaths: [smoke.decoderPluginPath],
                frameProcessorPluginLibraryPaths: smoke.frameProcessorPluginPath.map { [$0] } ?? [],
                maxInFlightFrames: 1
            ),
            sourceNormalizer: VesperSourceNormalizerConfiguration(
                mode: .preflightOnly,
                pluginLibraryPaths: [smoke.sourceNormalizerPluginPath],
                runtimeProfile: smoke.runtimeProfile
            ),
            surfaceHost: surfaceView,
            nativeFramePresenter: presenter
        )
        var timelines: [VesperNativeFramePipelineTimeline] = []
        session.onFramePresented = { timeline in
            timelines.append(timeline)
        }
        defer {
            session.close()
            surfaceView.detachBridgeIfNeeded()
            window.isHidden = true
        }

        guard case .success = session.start() else {
            XCTFail("expected real native-frame plugin session to start")
            return
        }

        XCTAssertEqual(session.route, "sdkManagedNativeFrame")
        XCTAssertEqual(session.clockSource, "swiftNativeAudioBridge")
        XCTAssertEqual(session.audioDecoderKind, "swiftNativeAudioBridge")
        XCTAssertEqual(session.audioOutputKind, "swiftNativeAudioBridge")
        XCTAssertEqual(session.audioPipelineKind, "swiftNativeAudioBridgeV1")
        XCTAssertTrue(session.hasAudioTrack)
        XCTAssertEqual(session.selectedVideoMediaKind, "video")
        XCTAssertEqual(session.audioMediaKind, "audio")
        XCTAssertGreaterThan(session.durationMs ?? 0, 0)
        XCTAssertTrue(session.seekable)

        session.play(rate: 1.0)
        let presentedFirstFrame = await waitForNativeFrameSmoke(timeout: 8) {
            session.counters.presentedFrames >= 1 &&
                presenter.presentedPixelBufferAddresses.count >= 1
        }
        XCTAssertTrue(
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
        XCTAssertTrue(
            presentedAfterResume,
            "real native-frame plugin smoke did not resume after pause/rate update"
        )

        let presentedBeforeSeek = session.counters.presentedFrames
        let seekTargetMs = min(max((session.durationMs ?? 2_000) / 2, 250), 1_500)
        XCTAssertTrue(session.seek(toMs: seekTargetMs))
        let presentedAfterSeek = await waitForNativeFrameSmoke(timeout: 8) {
            session.counters.presentedFrames > presentedBeforeSeek
        }
        XCTAssertTrue(
            presentedAfterSeek,
            "real native-frame plugin smoke did not present after seek"
        )
        session.pause()

        XCTAssertFalse(timelines.isEmpty)
        XCTAssertGreaterThanOrEqual(timelines.last?.positionMs ?? 0, 0)
        XCTAssertGreaterThanOrEqual(session.counters.presentedFrames, 3)
        XCTAssertGreaterThanOrEqual(session.counters.skippedAudioPackets, 0)
        XCTAssertGreaterThanOrEqual(session.counters.skippedVideoPackets, 0)
        XCTAssertGreaterThanOrEqual(session.counters.skippedOtherPackets, 0)
        XCTAssertEqual(session.counters.deadlineMisses, 0)
        XCTAssertEqual(session.counters.backpressureCount, 0)
        XCTAssertEqual(session.counters.lateDropped, 0)
        if smoke.frameProcessorPluginPath != nil {
            XCTAssertGreaterThan(session.counters.processedFrames, 0)
        }
        XCTAssertTrue(presenter.presentedPixelBufferAddresses.allSatisfy { $0 != 0 })
        XCTAssertTrue(presenter.presentedResults.allSatisfy { $0 })
        session.close()
        XCTAssertEqual(presenter.enabledStates.last, false)
        print(
            "real iOS native-frame smoke presentedFrames=\(session.counters.presentedFrames) " +
                "processedFrames=\(session.counters.processedFrames) " +
                "skippedAudioPackets=\(session.counters.skippedAudioPackets)"
        )
    }

    func testNativeFramePipelineTimelineFallsBackToVideoClockWithoutAudio() {
        let source = try! VesperPlayerSource(
            uri: "file:///tmp/example.mov",
            label: "Local MOV",
            kind: .local,
            protocol: .file
        )
        let backend = TestNativeFramePipelineBackend()
        let audioOutput = TestNativeFrameAudioOutput()
        audioOutput.prepareResult = VesperNativeFrameAudioBridgeState.resolved(
            hasAudioTrack: false,
            bridgePrepared: false
        )
        let session = VesperNativeFramePipelineSession(
            source: source,
            configuration: VesperNativeFramePipelineConfiguration(
                mode: .preferNativeFrame,
                decoderPluginLibraryPaths: ["/tmp/libdecoder.dylib"]
            ),
            sourceNormalizer: VesperSourceNormalizerConfiguration(
                mode: .preflightOnly,
                pluginLibraryPaths: ["/tmp/libsource_normalizer.dylib"]
            ),
            surfaceHost: PlayerSurfaceView(),
            backend: backend,
            audioOutput: audioOutput
        )

        guard case .success = session.start() else {
            XCTFail("expected fake native-frame session to start")
            return
        }
        audioOutput.currentPositionMs = 24_000

        XCTAssertEqual(
            session.timelinePositionMs(framePresentationTimeUs: 12_000_000),
            12_000
        )
        XCTAssertEqual(session.clockSource, "video")
        XCTAssertEqual(session.audioDecoderKind, "none")
    }

    func testNativeFrameAudioScheduledBufferGateBlocksUntilSlotIsReleased() async throws {
        let gate = VesperNativeFrameAudioScheduledBufferGate(maxQueuedBuffers: 1)
        try gate.waitUntilSlotAvailable()
        let blocked = XCTestExpectation(description: "second buffer waits for a release slot")
        blocked.isInverted = true
        let resumed = XCTestExpectation(description: "second buffer resumes after release")

        Task.detached {
            try? gate.waitUntilSlotAvailable()
            blocked.fulfill()
            resumed.fulfill()
        }
        await fulfillment(of: [blocked], timeout: 0.02)

        gate.releaseSlot()
        await fulfillment(of: [resumed], timeout: 0.2)
        gate.releaseSlot()
    }

    func testNativeFrameAudioPlaybackGateRejectsStalePlayback() {
        let gate = VesperNativeFrameAudioPlaybackGate()

        let first = gate.beginPlayback()
        XCTAssertTrue(gate.isCurrent(first))
        XCTAssertTrue(gate.wantsPlayback)

        gate.cancelPlayback()
        XCTAssertFalse(gate.isCurrent(first))
        XCTAssertFalse(gate.wantsPlayback)

        let second = gate.beginPlayback()
        XCTAssertFalse(gate.isCurrent(first))
        XCTAssertTrue(gate.isCurrent(second))
    }

    func testNativeFramePipelineIssueParsesStructuredFfiIssueKind() {
        let issue = VesperNativeFramePipelineIssue.classifyStartupFailure(
            "nativeFrameIssueKind=unsupportedCodec; iOS native-frame pipeline supports H264/HEVC packet streams, got VP9"
        )

        XCTAssertEqual(issue.kind, .unsupportedCodec)
        XCTAssertEqual(
            issue.message,
            "iOS native-frame pipeline supports H264/HEVC packet streams, got VP9"
        )
    }

    func testNativeFramePipelineIssueParsesStructuredUnsupportedSourceKind() {
        let issue = VesperNativeFramePipelineIssue.classifyStartupFailure(
            "nativeFrameIssueKind=unsupportedSource; iOS native-frame pipeline v1 does not handle HLS sources"
        )

        XCTAssertEqual(issue.kind, .unsupportedSource)
        XCTAssertEqual(
            issue.message,
            "iOS native-frame pipeline v1 does not handle HLS sources"
        )
    }

    private func settleControllerObservation() async {
        await Task.yield()
        await Task.yield()
    }

    private static func nativeFrameSmokeConfiguration() throws -> NativeFrameSmokeConfiguration {
        let environment = ProcessInfo.processInfo.environment
        guard smokeSetting(environment, key: "VESPER_IOS_NATIVE_FRAME_SMOKE_ENABLED") == "1" else {
            throw XCTSkip("Set VESPER_IOS_NATIVE_FRAME_SMOKE_ENABLED=1 to run the real iOS native-frame smoke.")
        }
        return try NativeFrameSmokeConfiguration(
            sourcePath: requiredExistingFilePath(
                environment,
                key: "VESPER_IOS_NATIVE_FRAME_SMOKE_SOURCE"
            ),
            sourceNormalizerPluginPath: requiredExistingFilePath(
                environment,
                key: "VESPER_IOS_SOURCE_NORMALIZER_PLUGIN_PATH"
            ),
            decoderPluginPath: requiredExistingFilePath(
                environment,
                key: "VESPER_IOS_DECODER_VIDEOTOOLBOX_PLUGIN_PATH"
            ),
            frameProcessorPluginPath: optionalExistingFilePath(
                environment,
                key: "VESPER_IOS_FRAME_PROCESSOR_DIAGNOSTIC_PLUGIN_PATH"
            ),
            runtimeProfile: smokeSetting(
                environment,
                key: "VESPER_IOS_SOURCE_NORMALIZER_RUNTIME_PROFILE"
            )
        )
    }

    private static func requiredExistingFilePath(
        _ environment: [String: String],
        key: String
    ) throws -> String {
        guard let path = smokeSetting(environment, key: key), !path.isEmpty else {
            throw XCTSkip("\(key) is required for the real iOS native-frame smoke.")
        }
        guard FileManager.default.fileExists(atPath: path) else {
            XCTFail("\(key) points to a missing file: \(path)")
            throw XCTSkip("\(key) points to a missing file.")
        }
        return path
    }

    private static func optionalExistingFilePath(
        _ environment: [String: String],
        key: String
    ) throws -> String? {
        guard let path = smokeSetting(environment, key: key), !path.isEmpty else { return nil }
        guard FileManager.default.fileExists(atPath: path) else {
            XCTFail("\(key) points to a missing file: \(path)")
            throw XCTSkip("\(key) points to a missing file.")
        }
        return path
    }

    private static func smokeSetting(
        _ environment: [String: String],
        key: String
    ) -> String? {
        if let value = environment[key], !value.isEmpty {
            return value
        }
        if let value = smokeConfigurationFileValue(environment, key: key), !value.isEmpty {
            return value
        }
        return Bundle(for: Self.self).object(forInfoDictionaryKey: key) as? String
    }

    private static func smokeConfigurationFileValue(
        _ environment: [String: String],
        key: String
    ) -> String? {
        let configPath = environment["VESPER_IOS_NATIVE_FRAME_SMOKE_CONFIG"]
            ?? "/tmp/vesper-ios-native-frame-smoke.plist"
        guard FileManager.default.fileExists(atPath: configPath),
              let dictionary = NSDictionary(contentsOfFile: configPath) as? [String: Any]
        else {
            return nil
        }
        return dictionary[key] as? String
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

private struct NativeFrameSmokeConfiguration {
    let sourcePath: String
    let sourceNormalizerPluginPath: String
    let decoderPluginPath: String
    let frameProcessorPluginPath: String?
    let runtimeProfile: String?
}

@MainActor
private final class TestObservablePlayerBridge: ObservableObject, ObservablePlayerBridge {
    @Published var publishedUiState = PlayerHostUiState(
        title: "Test Player",
        subtitle: "Ready",
        sourceLabel: "Test Source",
        playbackState: .ready,
        playbackRate: 1.0,
        isBuffering: false,
        isInterrupted: false,
        timeline: TimelineUiState(
            kind: .vod,
            isSeekable: true,
            seekableRange: SeekableRangeUi(startMs: 0, endMs: 60_000),
            liveEdgeMs: nil,
            positionMs: 0,
            durationMs: 60_000
        )
    )
    @Published var publishedTrackCatalog: VesperTrackCatalog = .empty
    @Published var publishedTrackSelection = VesperTrackSelectionSnapshot()
    @Published var publishedEffectiveVideoTrackId: String?
    @Published var publishedVideoVariantObservation: VesperVideoVariantObservation?
    @Published var publishedFixedTrackStatus: VesperFixedTrackStatus?
    @Published var publishedResiliencePolicy = VesperPlaybackResiliencePolicy()
    @Published var publishedLastError: VesperPlayerError?

    let backend: PlayerBridgeBackend = .fakeDemo

    var uiState: PlayerHostUiState { publishedUiState }
    var trackCatalog: VesperTrackCatalog { publishedTrackCatalog }
    var trackSelection: VesperTrackSelectionSnapshot { publishedTrackSelection }
    var effectiveVideoTrackId: String? { publishedEffectiveVideoTrackId }
    var videoVariantObservation: VesperVideoVariantObservation? { publishedVideoVariantObservation }
    var fixedTrackStatus: VesperFixedTrackStatus? { publishedFixedTrackStatus }
    var resiliencePolicy: VesperPlaybackResiliencePolicy { publishedResiliencePolicy }
    var lastError: VesperPlayerError? { publishedLastError }
    var pluginDiagnostics: [[String: Any]] { [] }

    func initialize() {}
    func dispose() {}
    func refresh() {}
    func selectSource(_ source: VesperPlayerSource) {}
    func attachSurfaceHost(_ host: UIView) {}
    func detachSurfaceHost() {}
    func play() {}
    func pause() {}
    func togglePause() {}
    func stop() {}
    func seek(by deltaMs: Int64) {}
    func seek(toRatio ratio: Double) {}
    func seekToLiveEdge() {}
    func setPlaybackRate(_ rate: Float) {}
    func setVideoTrackSelection(_ selection: VesperTrackSelection) {}
    func setAudioTrackSelection(_ selection: VesperTrackSelection) {}
    func setSubtitleTrackSelection(_ selection: VesperTrackSelection) {}
    func setAbrPolicy(_ policy: VesperAbrPolicy) {}
    func setResiliencePolicy(_ policy: VesperPlaybackResiliencePolicy) {}
    func setAudioSessionInterrupted(_ interrupted: Bool) {}
    func drainBenchmarkEvents() -> [VesperBenchmarkEvent] { [] }
    func benchmarkSummary() -> VesperBenchmarkSummary {
        VesperBenchmarkSummary(
            runId: "test-run",
            sessionId: "test-session",
            acceptedEvents: 0,
            droppedEvents: 0,
            pluginAcceptedEvents: 0,
            pluginDroppedEvents: 0,
            metrics: [],
            pluginErrors: []
        )
    }
}

private final class TestNativeFramePipelineBackend: VesperNativeFramePipelineBackend, @unchecked Sendable {
    private let lock = NSLock()
    private var storedSeekRequests: [Int64] = []
    private var storedFlushRequests: [UInt64] = []
    private var storedAdvanceRequests: [UInt64] = []
    private var storedReleasedFrameHandles: [UInt64] = []
    private var storedReleasePresentedFlags: [Bool] = []
    var seekResult: Result<[String: Any], VesperNativeFramePipelineOperationError>?
    var advanceResults: [Result<[String: Any], VesperNativeFramePipelineOperationError>] = []
    var releaseResult: [String: Any] = ["counters": [:]]
    var openResult: Result<VesperNativeFramePipelineOpenResult, VesperNativeFramePipelineStartupError>?
    private var storedOpenSourceUris: [String] = []
    private var storedCloseHandles: [UInt64] = []
    private var nextHandle: UInt64 = 42

    var seekRequests: [Int64] {
        withLock { storedSeekRequests }
    }

    var flushRequests: [UInt64] {
        withLock { storedFlushRequests }
    }

    var advanceRequests: [UInt64] {
        withLock { storedAdvanceRequests }
    }

    var releasedFrameHandles: [UInt64] {
        withLock { storedReleasedFrameHandles }
    }

    var releasePresentedFlags: [Bool] {
        withLock { storedReleasePresentedFlags }
    }

    var openSourceUris: [String] {
        withLock { storedOpenSourceUris }
    }

    var closeHandles: [UInt64] {
        withLock { storedCloseHandles }
    }

    func open(
        source: VesperPlayerSource,
        configuration _: VesperNativeFramePipelineConfiguration,
        sourceNormalizer _: VesperSourceNormalizerConfiguration
    ) -> Result<VesperNativeFramePipelineOpenResult, VesperNativeFramePipelineStartupError> {
        let handle = withLock {
            storedOpenSourceUris.append(source.uri)
            let handle = nextHandle
            nextHandle += 1
            return handle
        }
        if let openResult {
            return openResult
        }
        return .success(
            VesperNativeFramePipelineOpenResult(
                handle: handle,
                status: [
                    "durationMillis": NSNumber(value: 60_000),
                    "seekable": true,
                    "hasAudioTrack": true,
                    "selectedVideoStreamIndex": 0,
                    "selectedVideoMediaKind": "video",
                    "audioStreamIndex": 1,
                    "audioMediaKind": "audio",
                    "clockSource": "swiftNativeAudioBridge",
                    "counters": [
                        "skippedAudioPackets": 2,
                        "skipped_video_packets": 1,
                        "skippedOtherPackets": 3,
                    ],
                ]
            )
        )
    }

    func flush(handle: UInt64) -> Result<[String: Any], VesperNativeFramePipelineOperationError> {
        withLock {
            storedFlushRequests.append(handle)
        }
        return .success([
            "durationMillis": NSNumber(value: 60_000),
            "seekable": true,
            "hasAudioTrack": true,
            "selectedVideoStreamIndex": 0,
            "selectedVideoMediaKind": "video",
            "audioStreamIndex": 1,
            "audioMediaKind": "audio",
            "clockSource": "swiftNativeAudioBridge",
            "counters": [:],
        ])
    }

    func seek(
        handle _: UInt64,
        positionMs: Int64
    ) -> Result<[String: Any], VesperNativeFramePipelineOperationError> {
        withLock {
            storedSeekRequests.append(positionMs)
        }
        if let seekResult {
            return seekResult
        }
        return .success([
            "durationMillis": NSNumber(value: 60_000),
            "seekable": true,
            "hasAudioTrack": true,
            "selectedVideoStreamIndex": 0,
            "selectedVideoMediaKind": "video",
            "audioStreamIndex": 1,
            "audioMediaKind": "audio",
            "clockSource": "swiftNativeAudioBridge",
            "counters": [
                "processedFrames": 0,
                "presentedFrames": 0,
            ],
        ])
    }

    func advance(handle: UInt64) -> Result<[String: Any], VesperNativeFramePipelineOperationError> {
        let nextResult = withLock {
            storedAdvanceRequests.append(handle)
            return advanceResults.isEmpty ? nil : advanceResults.removeFirst()
        }
        if let nextResult {
            return nextResult
        }
        return .success(["status": "pending", "counters": [:]])
    }

    func releaseFrame(
        handle _: UInt64,
        frameHandle: UInt64,
        presented: Bool
    ) -> Result<[String: Any], VesperNativeFramePipelineOperationError> {
        withLock {
            storedReleasedFrameHandles.append(frameHandle)
            storedReleasePresentedFlags.append(presented)
        }
        return .success(releaseResult)
    }

    func close(handle: UInt64) {
        withLock {
            storedCloseHandles.append(handle)
        }
    }

    private func withLock<T>(_ body: () -> T) -> T {
        lock.lock()
        defer { lock.unlock() }
        return body()
    }
}

@MainActor
private final class TestNativeFramePresenter: VesperNativeFramePresenting {
    private(set) var enabledStates: [Bool] = []
    private(set) var presentedPixelBufferAddresses: [UInt] = []
    var presentResult = true
    var holdPresentation = false
    private var heldContinuations: [CheckedContinuation<Bool, Never>] = []

    func setNativeFramePresentationEnabled(_ enabled: Bool) {
        enabledStates.append(enabled)
    }

    func presentNativeFrame(pixelBufferAddress: UInt) async -> Bool {
        presentedPixelBufferAddresses.append(pixelBufferAddress)
        if holdPresentation {
            return await withCheckedContinuation { continuation in
                heldContinuations.append(continuation)
            }
        }
        return presentResult
    }

    func resumePresentation() {
        let continuations = heldContinuations
        heldContinuations.removeAll()
        for continuation in continuations {
            continuation.resume(returning: presentResult)
        }
    }
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

    func presentNativeFrame(pixelBufferAddress: UInt) async -> Bool {
        presentedPixelBufferAddresses.append(pixelBufferAddress)
        let result = await wrapped.presentNativeFrame(pixelBufferAddress: pixelBufferAddress)
        presentedResults.append(result)
        return result
    }
}

@MainActor
private final class TestNativeFrameAudioOutput: VesperNativeFrameAudioOutputing {
    private(set) var events: [String] = []
    var onStateChanged: ((VesperNativeFrameAudioBridgeState) -> Void)?
    var currentPositionMs: Int64?
    var prepareResult: VesperNativeFrameAudioBridgeState?

    func prepare(
        source _: VesperPlayerSource,
        hasAudioTrack: Bool
    ) -> VesperNativeFrameAudioBridgeState {
        events.append("prepare:\(hasAudioTrack)")
        if let prepareResult {
            return prepareResult
        }
        return VesperNativeFrameAudioBridgeState.resolved(
            hasAudioTrack: hasAudioTrack,
            bridgePrepared: hasAudioTrack
        )
    }

    func play(rate: Float) {
        events.append("play:\(rate)")
    }

    func pause() {
        events.append("pause")
    }

    func stop() {
        events.append("stop")
    }

    func seek(toMs positionMs: Int64) {
        events.append("seek:\(positionMs)")
    }

    func setPlaybackRate(_ rate: Float) {
        events.append("rate:\(rate)")
    }

    func close() {
        events.append("close")
    }

    func emitState(_ state: VesperNativeFrameAudioBridgeState) {
        onStateChanged?(state)
    }
}

private let sampleTrackCatalog = VesperTrackCatalog(
    tracks: [
        VesperMediaTrack(
            id: "video:hls:cavc1:b854000:w854:h480:f3000",
            kind: .video,
            label: "480p",
            codec: "avc1",
            bitRate: 854_000,
            width: 854,
            height: 480,
            frameRate: 30
        ),
        VesperMediaTrack(
            id: "video:hls:cavc1:b1500000:w1280:h720:f3000",
            kind: .video,
            label: "720p",
            codec: "avc1",
            bitRate: 1_500_000,
            width: 1280,
            height: 720,
            frameRate: 30
        ),
    ],
    adaptiveVideo: true,
    adaptiveAudio: false
)
