@preconcurrency import AVFoundation
import Foundation
import VesperPlayerKitBridgeShim

@MainActor
final class VesperNativeFramePipelineCoordinator {
    typealias SessionFactory = @MainActor (
        VesperPlayerSource,
        VesperNativeFramePipelineConfiguration,
        VesperSourceNormalizerConfiguration,
        PlayerSurfaceView
    ) -> VesperNativeFramePipelineSession

    private enum DiagnosticRoute {
        static let systemPlayer = "systemPlayer"
        static let sdkManagedNativeFrame = "sdkManagedNativeFrame"
        static let softwareDecoder = "softwareDecoder"
    }

    private(set) var activeSession: VesperNativeFramePipelineSession?
    private var routeIssue: VesperNativeFramePipelineIssue?
    private var startupIssue: VesperNativeFramePipelineIssue?
    private let sessionFactory: SessionFactory

    init(sessionFactory: SessionFactory? = nil) {
        self.sessionFactory = sessionFactory ?? { source, configuration, sourceNormalizer, surfaceHost in
            VesperNativeFramePipelineSession(
                source: source,
                configuration: configuration,
                sourceNormalizer: sourceNormalizer,
                surfaceHost: surfaceHost
            )
        }
    }

    func makeDiagnostics(
        configuration: VesperNativeFramePipelineConfiguration,
        fallbackIssue explicitFallbackIssue: VesperNativeFramePipelineIssue? = nil,
        counters: VesperNativeFramePipelineCounters = VesperNativeFramePipelineCounters()
    ) -> [[String: Any]] {
        guard !configuration.isDisabled else {
            return []
        }
        let pendingIssue = routeIssue?.kind == .missingSurface ? routeIssue : nil
        let implicitFailureIssue =
            configuration.mode == .requireNativeFrame && routeIssue?.kind != .missingSurface ? routeIssue : nil
        let failureIssue = startupIssue ?? implicitFailureIssue
        let fallbackIssue = explicitFallbackIssue
        let participation: String
        if fallbackIssue != nil {
            participation = "fallback"
        } else if failureIssue != nil {
            participation = activeSession?.participation ?? "selected"
        } else {
            switch configuration.mode {
            case .preferNativeFrame, .requireNativeFrame:
                participation = activeSession?.participation ?? "selected"
            case .disabled, .diagnosticsOnly:
                participation = "available"
            }
        }
        let route: String
        switch configuration.mode {
        case .disabled, .diagnosticsOnly:
            route = DiagnosticRoute.systemPlayer
        case .preferNativeFrame, .requireNativeFrame:
            if fallbackIssue != nil {
                route = DiagnosticRoute.systemPlayer
            } else if failureIssue != nil || pendingIssue != nil {
                route = activeSession?.route ?? DiagnosticRoute.sdkManagedNativeFrame
            } else {
                route = activeSession?.route ?? DiagnosticRoute.sdkManagedNativeFrame
            }
        }
        let status: String
        if fallbackIssue != nil {
            status = "unsupported"
        } else if let failureIssue {
            status = startupIssue == nil && failureIssue == routeIssue ? "unsupported" : "loadFailed"
        } else {
            status = activeSession?.status ?? "loaded"
        }
        var message = Self.message(for: configuration.mode)
        if let fallbackIssue {
            message += " Fallback reason: \(fallbackIssue.message)"
        }
        if let failureIssue {
            message += " Startup failure: \(failureIssue.message)"
        }
        if let pendingIssue {
            message += " Pending reason: \(pendingIssue.message)"
        }
        let paths = configuration.decoderPluginLibraryPaths +
            configuration.frameProcessorPluginLibraryPaths
        let maxInFlight = configuration.maxInFlightFrames.map(String.init) ?? "default"
        let sessionCounters = activeSession?.counters ?? counters
        let sessionClockSource = activeSession?.clockSource ?? "pending"
        let sessionSeekable = activeSession?.seekable ?? false
        let sessionHasAudioTrack = activeSession?.hasAudioTrack ?? false
        let sessionAudioOutput = activeSession?.audioOutputKind ?? "pending"
        let sessionAudioDecoder = activeSession?.audioDecoderKind ?? "pending"
        let sessionAudioOutputIssue = activeSession?.audioOutputIssue
        let audioPipeline = activeSession?.audioPipelineKind ?? "pending"
        let audioRateControl = activeSession?.audioRateControlKind ?? "pending"
        let selectedVideoStreamIndex = activeSession?.selectedVideoStreamIndex
        let selectedVideoMediaKind = activeSession?.selectedVideoMediaKind ?? "pending"
        let audioStreamIndex = activeSession?.audioStreamIndex
        let audioMediaKind = activeSession?.audioMediaKind ?? "pending"
        var diagnostic: [String: Any] = [
            "path": paths.joined(separator: ":"),
            "pluginName": "vesper-ios-native-frame-pipeline",
            "pluginKind": "native_frame_pipeline",
            "status": status,
            "message":
                "\(message) decoderPlugins=\(configuration.decoderPluginLibraryPaths.count); " +
                "frameProcessors=\(configuration.frameProcessorPluginLibraryPaths.count); " +
                "maxInFlightFrames=\(maxInFlight)",
            "participation": participation,
            "route": route,
            "sourceInput": "sourceNormalizerPacket",
            "decoderAdapter": "VideoToolbox",
            "clockSource": sessionClockSource,
            "seekable": sessionSeekable,
            "hasAudioTrack": sessionHasAudioTrack,
            "selectedVideoMediaKind": selectedVideoMediaKind,
            "audioMediaKind": audioMediaKind,
            "audioDecoder": sessionAudioDecoder,
            "audioOutput": sessionAudioOutput,
            "audioPipeline": audioPipeline,
            "audioRateControl": audioRateControl,
            "presenterProfile": "MetalLayer",
            "pipelineProfile": "VideoToolboxCvPixelBuffer",
            "processedFrames": sessionCounters.processedFrames,
            "presentedFrames": sessionCounters.presentedFrames,
            "deadlineMisses": sessionCounters.deadlineMisses,
            "backpressureCount": sessionCounters.backpressureCount,
            "lateDropped": sessionCounters.lateDropped,
            "skippedAudioPackets": sessionCounters.skippedAudioPackets,
            "skippedVideoPackets": sessionCounters.skippedVideoPackets,
            "skippedOtherPackets": sessionCounters.skippedOtherPackets,
        ]
        if let selectedVideoStreamIndex {
            diagnostic["selectedVideoStreamIndex"] = selectedVideoStreamIndex
        }
        if let audioStreamIndex {
            diagnostic["audioStreamIndex"] = audioStreamIndex
        }
        if let fallbackIssue {
            diagnostic["fallbackReason"] = fallbackIssue.message
            diagnostic["fallbackKind"] = fallbackIssue.kind.rawValue
            diagnostic["fallbackTargetRoute"] = DiagnosticRoute.systemPlayer
        }
        if let failureIssue {
            diagnostic["failureReason"] = failureIssue.message
            diagnostic["failureKind"] = failureIssue.kind.rawValue
        }
        if let pendingIssue {
            diagnostic["pendingReason"] = pendingIssue.message
            diagnostic["pendingKind"] = pendingIssue.kind.rawValue
        }
        if let issue = fallbackIssue ?? failureIssue ?? pendingIssue {
            diagnostic["issueReason"] = issue.message
            diagnostic["issueKind"] = issue.kind.rawValue
        }
        if let sessionAudioOutputIssue {
            diagnostic["audioOutputIssue"] = sessionAudioOutputIssue
        }
        if let session = activeSession {
            diagnostic["sessionId"] = session.id.uuidString
        }
        return [diagnostic]
    }

    func evaluateRoute(
        for source: VesperPlayerSource,
        configuration: VesperNativeFramePipelineConfiguration,
        sourceNormalizer: VesperSourceNormalizerConfiguration,
        surfaceHost: PlayerSurfaceView?
    ) -> VesperNativeFramePipelineRouteDecision {
        routeIssue = nil
        startupIssue = nil
        closeActiveSession()
        switch configuration.mode {
        case .disabled, .diagnosticsOnly:
            surfaceHost?.setNativeFramePresentationEnabled(false)
            return .systemPlayer
        case .preferNativeFrame, .requireNativeFrame:
            break
        }

        if let issue = unavailableIssue(
            for: source,
            configuration: configuration,
            sourceNormalizer: sourceNormalizer
        ) {
            routeIssue = issue
            surfaceHost?.setNativeFramePresentationEnabled(false)
            if configuration.mode == .requireNativeFrame {
                return .fail(issue)
            }
            return .fallback(issue)
        }

        guard let surfaceHost else {
            let issue = VesperNativeFramePipelineIssue(
                kind: .missingSurface,
                message: "iOS native-frame pipeline requires an attached PlayerSurfaceView before source load."
            )
            routeIssue = issue
            return .waitForSurface(issue)
        }

        surfaceHost.setNativeFramePresentationEnabled(true)
        activeSession = sessionFactory(source, configuration, sourceNormalizer, surfaceHost)
        return .nativeFrame
    }

    func startActiveSession() -> Result<VesperNativeFramePipelineSession, VesperNativeFramePipelineStartupError> {
        guard let activeSession else {
            let issue = VesperNativeFramePipelineIssue(
                kind: .sessionNotPrepared,
                message: "iOS native-frame pipeline session was not prepared."
            )
            startupIssue = issue
            return .failure(
                VesperNativeFramePipelineStartupError(issue: issue)
            )
        }
        let result = activeSession.start()
        switch result {
        case .success:
            startupIssue = nil
        case .failure(let error):
            startupIssue = error.issue
        }
        return result
    }

    func closeActiveSession() {
        activeSession?.close()
        activeSession = nil
    }

    private func unavailableIssue(
        for source: VesperPlayerSource,
        configuration: VesperNativeFramePipelineConfiguration,
        sourceNormalizer: VesperSourceNormalizerConfiguration
    ) -> VesperNativeFramePipelineIssue? {
        if let issue = Self.unsupportedSourceIssue(for: source) {
            return issue
        }
        guard !configuration.decoderPluginLibraryPaths.isEmpty else {
            return VesperNativeFramePipelineIssue(
                kind: .missingVideoToolboxDecoderPlugin,
                message: "iOS native-frame pipeline requires a VideoToolbox decoder plugin path."
            )
        }
        guard !sourceNormalizer.pluginLibraryPaths.isEmpty else {
            return VesperNativeFramePipelineIssue(
                kind: .missingSourceNormalizerPacketPlugin,
                message: "iOS native-frame pipeline requires a SourceNormalizer packet-stream plugin path."
            )
        }
        return nil
    }

    private static func unsupportedSourceIssue(for source: VesperPlayerSource) -> VesperNativeFramePipelineIssue? {
        switch source.protocol {
        case .hls, .dash:
            return VesperNativeFramePipelineIssue(
                kind: .unsupportedSource,
                message:
                    "iOS native-frame pipeline v1 does not handle \(source.protocol.rawValue.uppercased()) " +
                    "sources; AVFoundation system playback remains the supported route for HLS, DASH, live, and DVR."
            )
        case .content:
            return VesperNativeFramePipelineIssue(
                kind: .unsupportedSource,
                message:
                    "iOS native-frame pipeline v1 requires file URLs for local playback; " +
                    "content URLs remain on AVFoundation system playback."
            )
        case .unknown:
            return VesperNativeFramePipelineIssue(
                kind: .unsupportedSource,
                message:
                    "iOS native-frame pipeline v1 requires a known file or progressive VOD source; " +
                    "unknown sources remain on AVFoundation system playback."
            )
        case .file, .progressive:
            return nil
        }
    }

    private static func message(for mode: VesperNativeFramePipelineMode) -> String {
        switch mode {
        case .disabled:
            return "Mobile native-frame pipeline is disabled; system player remains selected."
        case .diagnosticsOnly:
            return "Mobile native-frame pipeline diagnostics are enabled; playback still uses the system player."
        case .preferNativeFrame:
            return "Mobile native-frame pipeline is explicitly preferred; iOS VideoToolbox and Metal presenter are selected when available."
        case .requireNativeFrame:
            return "Mobile native-frame pipeline is explicitly required; iOS VideoToolbox and Metal presenter must be available."
        }
    }
}

struct VesperNativeFramePipelineOpenResult {
    let handle: UInt64
    let status: [String: Any]
}

struct VesperNativeFramePipelineOperationError: LocalizedError, Equatable {
    let message: String

    var errorDescription: String? {
        message
    }
}

@MainActor
protocol VesperNativeFramePipelineBackend: AnyObject {
    func open(
        source: VesperPlayerSource,
        configuration: VesperNativeFramePipelineConfiguration,
        sourceNormalizer: VesperSourceNormalizerConfiguration
    ) -> Result<VesperNativeFramePipelineOpenResult, VesperNativeFramePipelineStartupError>

    func flush(handle: UInt64) -> Result<[String: Any], VesperNativeFramePipelineOperationError>

    func seek(
        handle: UInt64,
        positionMs: Int64
    ) -> Result<[String: Any], VesperNativeFramePipelineOperationError>

    func advance(handle: UInt64) -> Result<[String: Any], VesperNativeFramePipelineOperationError>

    func releaseFrame(
        handle: UInt64,
        frameHandle: UInt64,
        presented: Bool
    ) -> Result<[String: Any], VesperNativeFramePipelineOperationError>

    func close(handle: UInt64)
}

@MainActor
final class VesperFfiNativeFramePipelineBackend: VesperNativeFramePipelineBackend {
    func open(
        source: VesperPlayerSource,
        configuration: VesperNativeFramePipelineConfiguration,
        sourceNormalizer: VesperSourceNormalizerConfiguration
    ) -> Result<VesperNativeFramePipelineOpenResult, VesperNativeFramePipelineStartupError> {
        var openedHandle: UInt64 = 0
        var outputPointer: UnsafeMutablePointer<CChar>?
        var errorPointer: UnsafeMutablePointer<CChar>?
        let maxInFlightFrames = UInt32(configuration.maxInFlightFrames ?? 0)
        let ok = source.uri.withCString { sourceUriPointer in
            withOptionalCString(sourceNormalizer.runtimeProfile) { runtimeProfilePointer in
                withCStringArray(sourceNormalizer.pluginLibraryPaths) {
                    sourcePathPointers,
                    sourcePathCount in
                    withCStringArray(configuration.decoderPluginLibraryPaths) {
                        decoderPathPointers,
                        decoderPathCount in
                        withCStringArray(configuration.frameProcessorPluginLibraryPaths) {
                            framePathPointers,
                            framePathCount in
                            vesper_ios_native_frame_pipeline_open(
                                sourceUriPointer,
                                sourceNormalizer.ffiMode,
                                sourcePathPointers,
                                UInt(sourcePathCount),
                                runtimeProfilePointer,
                                configuration.ffiMode,
                                decoderPathPointers,
                                UInt(decoderPathCount),
                                framePathPointers,
                                UInt(framePathCount),
                                maxInFlightFrames,
                                &openedHandle,
                                &outputPointer,
                                &errorPointer
                            )
                        }
                    }
                }
            }
        }
        defer {
            if let outputPointer {
                vesper_mobile_plugin_diagnostics_string_free(outputPointer)
            }
            if let errorPointer {
                vesper_mobile_plugin_diagnostics_string_free(errorPointer)
            }
        }

        guard ok, openedHandle != 0, let outputPointer else {
            let message = errorPointer.map { String(cString: $0) }
                ?? "iOS native-frame pipeline open failed."
            return .failure(
                VesperNativeFramePipelineStartupError(
                    issue: VesperNativeFramePipelineIssue.classifyStartupFailure(message)
                )
            )
        }

        return .success(
            VesperNativeFramePipelineOpenResult(
                handle: openedHandle,
                status: Self.jsonObject(from: outputPointer)
            )
        )
    }

    func flush(handle: UInt64) -> Result<[String: Any], VesperNativeFramePipelineOperationError> {
        var outputPointer: UnsafeMutablePointer<CChar>?
        var errorPointer: UnsafeMutablePointer<CChar>?
        let ok = vesper_ios_native_frame_pipeline_flush(
            handle,
            &outputPointer,
            &errorPointer
        )
        defer {
            if let outputPointer {
                vesper_mobile_plugin_diagnostics_string_free(outputPointer)
            }
            if let errorPointer {
                vesper_mobile_plugin_diagnostics_string_free(errorPointer)
            }
        }
        guard ok, let outputPointer else {
            return .failure(
                VesperNativeFramePipelineOperationError(
                    message: errorPointer.map { String(cString: $0) }
                        ?? "native-frame flush failed"
                )
            )
        }
        return .success(Self.jsonObject(from: outputPointer))
    }

    func seek(
        handle: UInt64,
        positionMs: Int64
    ) -> Result<[String: Any], VesperNativeFramePipelineOperationError> {
        var outputPointer: UnsafeMutablePointer<CChar>?
        var errorPointer: UnsafeMutablePointer<CChar>?
        let ok = vesper_ios_native_frame_pipeline_seek(
            handle,
            UInt64(max(positionMs, 0)),
            &outputPointer,
            &errorPointer
        )
        defer {
            if let outputPointer {
                vesper_mobile_plugin_diagnostics_string_free(outputPointer)
            }
            if let errorPointer {
                vesper_mobile_plugin_diagnostics_string_free(errorPointer)
            }
        }
        guard ok, let outputPointer else {
            return .failure(
                VesperNativeFramePipelineOperationError(
                    message: errorPointer.map { String(cString: $0) }
                        ?? "native-frame seek failed"
                )
            )
        }
        return .success(Self.jsonObject(from: outputPointer))
    }

    func advance(handle: UInt64) -> Result<[String: Any], VesperNativeFramePipelineOperationError> {
        var outputPointer: UnsafeMutablePointer<CChar>?
        var errorPointer: UnsafeMutablePointer<CChar>?
        let ok = vesper_ios_native_frame_pipeline_advance(
            handle,
            &outputPointer,
            &errorPointer
        )
        defer {
            if let outputPointer {
                vesper_mobile_plugin_diagnostics_string_free(outputPointer)
            }
            if let errorPointer {
                vesper_mobile_plugin_diagnostics_string_free(errorPointer)
            }
        }
        guard ok, let outputPointer else {
            return .failure(
                VesperNativeFramePipelineOperationError(
                    message: errorPointer.map { String(cString: $0) }
                        ?? "native-frame advance failed"
                )
            )
        }
        return .success(Self.jsonObject(from: outputPointer))
    }

    func releaseFrame(
        handle: UInt64,
        frameHandle: UInt64,
        presented: Bool
    ) -> Result<[String: Any], VesperNativeFramePipelineOperationError> {
        var outputPointer: UnsafeMutablePointer<CChar>?
        var errorPointer: UnsafeMutablePointer<CChar>?
        let ok = vesper_ios_native_frame_pipeline_release_frame(
            handle,
            frameHandle,
            presented,
            &outputPointer,
            &errorPointer
        )
        defer {
            if let outputPointer {
                vesper_mobile_plugin_diagnostics_string_free(outputPointer)
            }
            if let errorPointer {
                vesper_mobile_plugin_diagnostics_string_free(errorPointer)
            }
        }
        guard ok, let outputPointer else {
            return .failure(
                VesperNativeFramePipelineOperationError(
                    message: errorPointer.map { String(cString: $0) }
                        ?? "native-frame release failed"
                )
            )
        }
        return .success(Self.jsonObject(from: outputPointer))
    }

    func close(handle: UInt64) {
        vesper_ios_native_frame_pipeline_close(handle)
    }

    private static func jsonObject(from pointer: UnsafeMutablePointer<CChar>) -> [String: Any] {
        let json = String(cString: pointer)
        guard
            let data = json.data(using: .utf8),
            let object = try? JSONSerialization.jsonObject(with: data) as? [String: Any]
        else {
            return [:]
        }
        return object
    }
}

@MainActor
protocol VesperNativeFrameAudioOutputing: AnyObject {
    var currentPositionMs: Int64? { get }

    func prepare(
        source: VesperPlayerSource,
        hasAudioTrack: Bool
    ) -> VesperNativeFrameAudioBridgeState
    func play(rate: Float)
    func pause()
    func stop()
    func seek(toMs positionMs: Int64)
    func setPlaybackRate(_ rate: Float)
    func close()
}

@MainActor
protocol VesperNativeFramePresenting: AnyObject {
    func setNativeFramePresentationEnabled(_ enabled: Bool)
    func presentNativeFrame(pixelBufferAddress: UInt) async -> Bool
}

extension PlayerSurfaceView: VesperNativeFramePresenting {
    func presentNativeFrame(pixelBufferAddress: UInt) async -> Bool {
        await withCheckedContinuation { continuation in
            presentNativeFrame(pixelBufferAddress: pixelBufferAddress) { succeeded in
                continuation.resume(returning: succeeded)
            }
        }
    }
}

@MainActor
final class VesperNativeFramePipelineSession {
    let id = UUID()
    let source: VesperPlayerSource
    let configuration: VesperNativeFramePipelineConfiguration
    let sourceNormalizer: VesperSourceNormalizerConfiguration
    let surfaceHost: PlayerSurfaceView
    private(set) var counters = VesperNativeFramePipelineCounters()
    private(set) var isClosed = false
    private(set) var didStart = false
    private(set) var durationMs: Int64?
    private(set) var seekable = false
    private(set) var hasAudioTrack = false
    private(set) var selectedVideoStreamIndex: Int?
    private(set) var selectedVideoMediaKind = "pending"
    private(set) var audioStreamIndex: Int?
    private(set) var audioMediaKind = "pending"
    private(set) var clockSource = "pending"
    private(set) var audioDecoderKind = "pending"
    private(set) var audioOutputKind = "pending"
    private(set) var audioPipelineKind = "pending"
    private(set) var audioRateControlKind = "pending"
    private(set) var audioOutputIssue: String?
    var onFramePresented: ((VesperNativeFramePipelineTimeline) -> Void)?
    var onPlaybackEnded: (() -> Void)?
    private var handle: UInt64 = 0
    private var displayTask: Task<Void, Never>?
    private var isPlaying = false
    private var playbackRate: Float = 1.0
    private var playbackAnchorMediaUs: Int64?
    private var playbackAnchorHostNs: UInt64?
    private var frameLeaseGeneration: UInt64 = 1
    private let backend: VesperNativeFramePipelineBackend
    private let audioOutput: VesperNativeFrameAudioOutputing
    private let nativeFramePresenter: VesperNativeFramePresenting
    private var audioBridgeState: VesperNativeFrameAudioBridgeState?

    init(
        source: VesperPlayerSource,
        configuration: VesperNativeFramePipelineConfiguration,
        sourceNormalizer: VesperSourceNormalizerConfiguration,
        surfaceHost: PlayerSurfaceView,
        backend: VesperNativeFramePipelineBackend? = nil,
        audioOutput: VesperNativeFrameAudioOutputing? = nil,
        nativeFramePresenter: VesperNativeFramePresenting? = nil
    ) {
        self.source = source
        self.configuration = configuration
        self.sourceNormalizer = sourceNormalizer
        self.surfaceHost = surfaceHost
        self.backend = backend ?? VesperFfiNativeFramePipelineBackend()
        self.audioOutput = audioOutput ?? VesperNativeFrameAudioOutput()
        self.nativeFramePresenter = nativeFramePresenter ?? surfaceHost
    }

    var route: String {
        "sdkManagedNativeFrame"
    }

    var status: String {
        didStart ? "running" : "loaded"
    }

    var participation: String {
        didStart ? "participated" : "selected"
    }

    func start() -> Result<VesperNativeFramePipelineSession, VesperNativeFramePipelineStartupError> {
        guard !isClosed else {
            let issue = VesperNativeFramePipelineIssue(
                kind: .sessionClosed,
                message: "iOS native-frame pipeline session is already closed."
            )
            return .failure(
                VesperNativeFramePipelineStartupError(issue: issue)
            )
        }
        guard handle == 0 else {
            didStart = true
            return .success(self)
        }

        let openResult = backend.open(
            source: source,
            configuration: configuration,
            sourceNormalizer: sourceNormalizer
        )
        guard case .success(let opened) = openResult else {
            if case .failure(let error) = openResult {
                return .failure(error)
            }
            return .failure(
                VesperNativeFramePipelineStartupError(
                    issue: VesperNativeFramePipelineIssue.classifyStartupFailure(
                        "iOS native-frame pipeline open failed."
                    )
                )
            )
        }

        handle = opened.handle
        didStart = true
        mergeStatus(from: opened.status)
        let audioState = audioOutput.prepare(source: source, hasAudioTrack: hasAudioTrack)
        applyAudioBridgeState(audioState)
        if audioState.outputKind == "swiftNativeAudioBridge" {
            iosHostLog("native audio pipeline configured audioPipeline=\(audioPipelineKind) source=\(source.uri)")
        } else {
            let reason = audioState.issue.map { " reason=\($0)" } ?? ""
            iosHostLog(
                "native audio pipeline unavailable audioPipeline=\(audioPipelineKind); using video clock source=\(source.uri)\(reason)"
            )
        }
        return .success(self)
    }

    func play(rate: Float = 1.0) {
        guard didStart, !isClosed else { return }
        playbackRate = max(rate, 0.01)
        isPlaying = true
        playbackAnchorMediaUs = nil
        playbackAnchorHostNs = nil
        audioOutput.play(rate: playbackRate)
        if displayTask == nil {
            displayTask = Task { @MainActor [weak self] in
                await self?.displayLoop()
            }
        }
    }

    func pause() {
        isPlaying = false
        audioOutput.pause()
    }

    func stop() {
        isPlaying = false
        audioOutput.stop()
        seek(toMs: 0)
    }

    func flush() {
        guard handle != 0 else { return }
        isPlaying = false
        playbackAnchorMediaUs = nil
        playbackAnchorHostNs = nil
        invalidateFrameLeases()
        audioOutput.pause()
        switch backend.flush(handle: handle) {
        case .success(let object):
            mergeStatus(from: object)
        case .failure(let error):
            iosHostLog("native-frame flush failed: \(error.message)")
        }
    }

    func setPlaybackRate(_ rate: Float) {
        playbackRate = max(rate, 0.01)
        audioOutput.setPlaybackRate(playbackRate)
        playbackAnchorMediaUs = nil
        playbackAnchorHostNs = nil
    }

    func applyAudioBridgeState(_ state: VesperNativeFrameAudioBridgeState) {
        audioBridgeState = state
        applyAudioBridgeStateValues(state)
    }

    @discardableResult
    func seek(toMs positionMs: Int64) -> Bool {
        guard handle != 0 else { return false }
        guard seekable else {
            iosHostLog("native-frame seek failed: source is not seekable")
            return false
        }
        let targetMs = clampedSeekPositionMs(positionMs)
        let wasPlaying = isPlaying
        isPlaying = false
        playbackAnchorMediaUs = nil
        playbackAnchorHostNs = nil
        audioOutput.pause()
        invalidateFrameLeases()
        switch backend.seek(handle: handle, positionMs: targetMs) {
        case .success(let object):
            mergeStatus(from: object)
            audioOutput.seek(toMs: targetMs)
            onFramePresented?(
                VesperNativeFramePipelineTimeline(
                    positionMs: targetMs,
                    durationMs: durationMs
                )
            )
            if wasPlaying {
                isPlaying = true
                audioOutput.play(rate: playbackRate)
            }
            return true
        case .failure(let error):
            iosHostLog("native-frame seek failed: \(error.message)")
            if wasPlaying {
                isPlaying = true
                audioOutput.play(rate: playbackRate)
            }
            return false
        }
    }

    private func clampedSeekPositionMs(_ positionMs: Int64) -> Int64 {
        let lowerBounded = max(positionMs, 0)
        guard let durationMs, durationMs > 0 else {
            return lowerBounded
        }
        return min(lowerBounded, durationMs)
    }

    private func displayLoop() async {
        while !Task.isCancelled {
            guard !isClosed else { return }
            guard isPlaying else {
                try? await Task.sleep(nanoseconds: 20_000_000)
                continue
            }
            let frame: VesperNativeFramePipelineFrame
            switch advanceFrame() {
            case .frame(let advanced):
                frame = advanced
            case .pending:
                try? await Task.sleep(nanoseconds: 5_000_000)
                continue
            case .endOfStream:
                handleEndOfStream()
                continue
            }
            await waitForPresentationTime(frame.presentationTimeUs)
            guard frameLeaseIsCurrent(frame) else {
                continue
            }
            guard isPlaying else {
                release(frame: frame, presented: false)
                continue
            }
            let presented = await present(frame: frame)
            guard frameLeaseIsCurrent(frame) else {
                continue
            }
            release(frame: frame, presented: presented)
            if presented, isPlaying {
                onFramePresented?(
                    VesperNativeFramePipelineTimeline(
                        positionMs: timelinePositionMs(
                            framePresentationTimeUs: frame.presentationTimeUs
                        ),
                        durationMs: durationMs
                    )
                )
            }
        }
    }

    func timelinePositionMs(framePresentationTimeUs presentationTimeUs: Int64) -> Int64 {
        let videoPositionMs = max(presentationTimeUs / 1_000, 0)
        guard clockSource == "swiftNativeAudioBridge",
              let audioPositionMs = audioOutput.currentPositionMs else {
            return videoPositionMs
        }
        return max(audioPositionMs, 0)
    }

    private func waitForPresentationTime(_ presentationTimeUs: Int64) async {
        let hostNow = DispatchTime.now().uptimeNanoseconds
        if playbackAnchorMediaUs == nil || playbackAnchorHostNs == nil {
            playbackAnchorMediaUs = presentationTimeUs
            playbackAnchorHostNs = hostNow
            return
        }
        guard let anchorMediaUs = playbackAnchorMediaUs,
              let anchorHostNs = playbackAnchorHostNs else {
            return
        }
        let mediaDeltaUs = max(presentationTimeUs - anchorMediaUs, 0)
        let adjustedMediaDeltaUs = UInt64(Double(mediaDeltaUs) / Double(max(playbackRate, 0.01)))
        let mediaDeltaNs = adjustedMediaDeltaUs * 1_000
        let target = anchorHostNs.addingReportingOverflow(mediaDeltaNs)
        guard !target.overflow else {
            return
        }
        let targetHostNs = target.partialValue
        let now = DispatchTime.now().uptimeNanoseconds
        guard targetHostNs > now else {
            return
        }
        try? await Task.sleep(nanoseconds: targetHostNs - now)
    }

    private func advanceFrame() -> VesperNativeFramePipelineAdvanceOutcome {
        guard handle != 0 else { return .pending }
        let object: [String: Any]
        switch backend.advance(handle: handle) {
        case .success(let value):
            object = value
        case .failure(let error):
            iosHostLog("native-frame advance failed: \(error.message)")
            isPlaying = false
            return .pending
        }
        updateCounters(from: object["counters"] as? [String: Any])
        let status = object["status"] as? String
        if status == "endOfStream" {
            return .endOfStream
        }
        guard
            status == "frame",
            let frameHandle = (object["handle"] as? NSNumber)?.uint64Value,
            let pixelBuffer = (object["pixelBuffer"] as? NSNumber)?.uintValue
        else {
            return .pending
        }
        return .frame(
            VesperNativeFramePipelineFrame(
                frameHandle: frameHandle,
                pixelBufferAddress: pixelBuffer,
                presentationTimeUs: (object["presentationTimeUs"] as? NSNumber)?.int64Value ?? 0,
                durationUs: (object["durationUs"] as? NSNumber)?.int64Value,
                width: (object["width"] as? NSNumber)?.intValue ?? 0,
                height: (object["height"] as? NSNumber)?.intValue ?? 0,
                leaseGeneration: frameLeaseGeneration
            )
        )
    }

    /// Stops the display loop and reports end-of-playback once the SDK pipeline
    /// drains. A seek clears the Rust-side EOF state and bumps the frame lease, so
    /// `isPlaying` resumes the loop and a later EOF reports again.
    private func handleEndOfStream() {
        isPlaying = false
        playbackAnchorMediaUs = nil
        playbackAnchorHostNs = nil
        audioOutput.pause()
        if let durationMs {
            onFramePresented?(
                VesperNativeFramePipelineTimeline(
                    positionMs: durationMs,
                    durationMs: durationMs
                )
            )
        }
        onPlaybackEnded?()
    }

    private func present(frame: VesperNativeFramePipelineFrame) async -> Bool {
        await nativeFramePresenter.presentNativeFrame(pixelBufferAddress: frame.pixelBufferAddress)
    }

    private func release(frame: VesperNativeFramePipelineFrame, presented: Bool) {
        guard handle != 0, frameLeaseIsCurrent(frame) else { return }
        switch backend.releaseFrame(
            handle: handle,
            frameHandle: frame.frameHandle,
            presented: presented
        ) {
        case .success(let object):
            mergeStatus(from: object)
        case .failure(let error):
            iosHostLog("native-frame release failed: \(error.message)")
        }
    }

    private func mergeStatus(from object: [String: Any]) {
        updateDuration(from: object["durationMillis"] as? NSNumber)
        updateCounters(from: object["counters"] as? [String: Any])
        if let value = object["seekable"] as? Bool {
            seekable = value
        } else if let value = object["seekable"] as? NSNumber {
            seekable = value.boolValue
        }
        if let value = object["hasAudioTrack"] as? Bool {
            hasAudioTrack = value
        } else if let value = object["hasAudioTrack"] as? NSNumber {
            hasAudioTrack = value.boolValue
        }
        if let value = object["selectedVideoStreamIndex"] as? NSNumber {
            selectedVideoStreamIndex = value.intValue
        } else if let value = object["selectedVideoStreamIndex"] as? Int {
            selectedVideoStreamIndex = value
        }
        if let value = object["selectedVideoMediaKind"] as? String, !value.isEmpty {
            selectedVideoMediaKind = value
        }
        if let value = object["audioStreamIndex"] as? NSNumber {
            audioStreamIndex = value.intValue
        } else if let value = object["audioStreamIndex"] as? Int {
            audioStreamIndex = value
        }
        if let value = object["audioMediaKind"] as? String, !value.isEmpty {
            audioMediaKind = value
        }
        if let value = object["clockSource"] as? String, !value.isEmpty {
            clockSource = value
        }
        if let audioBridgeState {
            applyAudioBridgeStateValues(audioBridgeState)
        }
    }

    private func applyAudioBridgeStateValues(_ state: VesperNativeFrameAudioBridgeState) {
        hasAudioTrack = state.hasAudioTrack
        audioDecoderKind = state.decoderKind
        audioOutputKind = state.outputKind
        audioPipelineKind = state.pipelineKind
        audioRateControlKind = state.rateControlKind
        clockSource = state.clockSource
        audioOutputIssue = state.issue
    }

    private func updateCounters(from countersObject: [String: Any]?) {
        guard let countersObject else { return }
        counters = VesperNativeFramePipelineCounters(
            processedFrames: (countersObject["processedFrames"] as? NSNumber)?.intValue
                ?? (countersObject["processed_frames"] as? NSNumber)?.intValue
                ?? counters.processedFrames,
            presentedFrames: (countersObject["presentedFrames"] as? NSNumber)?.intValue
                ?? (countersObject["presented_frames"] as? NSNumber)?.intValue
                ?? counters.presentedFrames,
            deadlineMisses: (countersObject["deadlineMisses"] as? NSNumber)?.intValue
                ?? (countersObject["deadline_misses"] as? NSNumber)?.intValue
                ?? counters.deadlineMisses,
            backpressureCount: (countersObject["backpressureCount"] as? NSNumber)?.intValue
                ?? (countersObject["backpressure_count"] as? NSNumber)?.intValue
                ?? counters.backpressureCount,
            lateDropped: (countersObject["lateDropped"] as? NSNumber)?.intValue
                ?? (countersObject["late_dropped"] as? NSNumber)?.intValue
                ?? counters.lateDropped,
            skippedAudioPackets: (countersObject["skippedAudioPackets"] as? NSNumber)?.intValue
                ?? (countersObject["skipped_audio_packets"] as? NSNumber)?.intValue
                ?? counters.skippedAudioPackets,
            skippedVideoPackets: (countersObject["skippedVideoPackets"] as? NSNumber)?.intValue
                ?? (countersObject["skipped_video_packets"] as? NSNumber)?.intValue
                ?? counters.skippedVideoPackets,
            skippedOtherPackets: (countersObject["skippedOtherPackets"] as? NSNumber)?.intValue
                ?? (countersObject["skipped_other_packets"] as? NSNumber)?.intValue
                ?? counters.skippedOtherPackets
        )
    }

    private func updateDuration(from durationMillis: NSNumber?) {
        guard let durationMillis else { return }
        let value = durationMillis.int64Value
        if value > 0 {
            durationMs = value
        }
    }

    func close() {
        guard !isClosed else { return }
        isClosed = true
        isPlaying = false
        displayTask?.cancel()
        displayTask = nil
        if handle != 0 {
            backend.close(handle: handle)
            handle = 0
        }
        audioOutput.close()
        onFramePresented = nil
        nativeFramePresenter.setNativeFramePresentationEnabled(false)
    }

    private func invalidateFrameLeases() {
        frameLeaseGeneration = frameLeaseGeneration &+ 1
        if frameLeaseGeneration == 0 {
            frameLeaseGeneration = 1
        }
    }

    private func frameLeaseIsCurrent(_ frame: VesperNativeFramePipelineFrame) -> Bool {
        !isClosed && handle != 0 && frame.leaseGeneration == frameLeaseGeneration
    }
}

@MainActor
private final class VesperNativeFrameAudioOutput: VesperNativeFrameAudioOutputing {
    private var engine: AVAudioEngine?
    private var playerNode: AVAudioPlayerNode?
    private var timePitch: AVAudioUnitTimePitch?
    private var asset: AVURLAsset?
    private var sourceURL: URL?
    private var scheduledAudioFile: VesperNativeFrameAudioPcmFile?
    private let temporaryFiles = VesperNativeFrameAudioTemporaryFileStore()
    private let playbackGate = VesperNativeFrameAudioPlaybackGate()
    private var playbackRate: Float = 1.0
    private var isPrepared = false
    private var seekPositionMs: Int64 = 0

    var currentPositionMs: Int64? {
        guard isPrepared else { return nil }
        guard playerNode?.isPlaying == true else {
            return seekPositionMs
        }
        guard let nodeTime = playerNode?.lastRenderTime,
              let playerTime = playerNode?.playerTime(forNodeTime: nodeTime),
              playerTime.sampleRate > 0
        else {
            return seekPositionMs
        }
        let renderedMs = Int64(
            (Double(playerTime.sampleTime) / playerTime.sampleRate * 1_000.0).rounded(.down)
        )
        return max(seekPositionMs + renderedMs, 0)
    }

    func prepare(
        source: VesperPlayerSource,
        hasAudioTrack: Bool
    ) -> VesperNativeFrameAudioBridgeState {
        close()
        guard hasAudioTrack else {
            return VesperNativeFrameAudioBridgeState.resolved(
                hasAudioTrack: false,
                bridgePrepared: false
            )
        }
        guard source.kind == .local,
              let url = URL(string: source.uri),
              url.isFileURL
        else {
            return VesperNativeFrameAudioBridgeState.resolved(
                hasAudioTrack: true,
                bridgePrepared: false,
                unavailableReason: "Swift native audio bridge v1 only supports local file sources."
            )
        }
        sourceURL = url
        asset = AVURLAsset(url: url)
        isPrepared = true
        seekPositionMs = 0
        return VesperNativeFrameAudioBridgeState.resolved(
            hasAudioTrack: true,
            bridgePrepared: true
        )
    }

    func play(rate: Float) {
        guard isPrepared else { return }
        playbackRate = max(rate, 0.01)
        rebuildAndStart()
    }

    func pause() {
        seekPositionMs = currentPositionMs ?? seekPositionMs
        playbackGate.cancelPlayback()
        playerNode?.pause()
        engine?.pause()
    }

    func stop() {
        playbackGate.cancelPlayback()
        playerNode?.stop()
        engine?.stop()
        playerNode = nil
        timePitch = nil
        engine = nil
        scheduledAudioFile = nil
        temporaryFiles.cleanupActiveFile()
        seekPositionMs = 0
    }

    func seek(toMs positionMs: Int64) {
        seekPositionMs = max(positionMs, 0)
        if playerNode?.isPlaying == true || playbackGate.wantsPlayback {
            rebuildAndStart()
        }
    }

    func setPlaybackRate(_ rate: Float) {
        let positionBeforeRateChange = currentPositionMs
        playbackRate = max(rate, 0.01)
        timePitch?.rate = playbackRate
        if playerNode?.isPlaying == true || playbackGate.wantsPlayback {
            seekPositionMs = positionBeforeRateChange ?? seekPositionMs
            rebuildAndStart()
        }
    }

    func close() {
        stop()
        asset = nil
        sourceURL = nil
        isPrepared = false
    }

    private func rebuildAndStart() {
        guard let asset else { return }
        playbackGate.cancelPlayback()
        playerNode?.stop()
        engine?.stop()
        scheduledAudioFile = nil
        temporaryFiles.cleanupActiveFile()
        let engine = AVAudioEngine()
        let playerNode = AVAudioPlayerNode()
        let timePitch = AVAudioUnitTimePitch()
        timePitch.rate = playbackRate
        engine.attach(playerNode)
        engine.attach(timePitch)
        engine.connect(playerNode, to: timePitch, format: nil)
        engine.connect(timePitch, to: engine.mainMixerNode, format: nil)
        do {
            try engine.start()
        } catch {
            iosHostLog("native audio engine start failed: \(error.localizedDescription)")
            return
        }
        self.engine = engine
        self.playerNode = playerNode
        self.timePitch = timePitch
        let playbackGeneration = playbackGate.beginPlayback()

        Task.detached(priority: .userInitiated) { [asset, seekPositionMs] in
            do {
                let pcmFile = try await Self.makePcmFile(asset: asset, startMs: seekPositionMs)
                await MainActor.run {
                    guard self.playerNode === playerNode,
                          self.playbackGate.isCurrent(playbackGeneration) else {
                        VesperNativeFrameAudioTemporaryFileStore.removeFile(at: pcmFile.url)
                        return
                    }
                    self.temporaryFiles.replaceActiveFile(with: pcmFile.url)
                    self.scheduledAudioFile = pcmFile
                    playerNode.scheduleFile(pcmFile.file, at: nil)
                    playerNode.play()
                }
            } catch {
                await MainActor.run {
                    iosHostLog("native audio decode failed: \(error.localizedDescription)")
                }
            }
        }
    }

    nonisolated private static func makePcmFile(
        asset: AVURLAsset,
        startMs: Int64
    ) async throws -> VesperNativeFrameAudioPcmFile {
        let tracks = try await asset.loadTracks(withMediaType: .audio)
        guard let track = tracks.first else {
            throw VesperNativeFrameAudioOutputError.noAudioTrack
        }
        let outputSettings: [String: Any] = [
            AVFormatIDKey: kAudioFormatLinearPCM,
            AVLinearPCMBitDepthKey: 32,
            AVLinearPCMIsFloatKey: true,
            AVLinearPCMIsNonInterleaved: false,
        ]
        let reader = try AVAssetReader(asset: asset)
        let output = AVAssetReaderTrackOutput(track: track, outputSettings: outputSettings)
        output.alwaysCopiesSampleData = false
        guard reader.canAdd(output) else {
            throw VesperNativeFrameAudioOutputError.readerOutputRejected
        }
        reader.add(output)
        if startMs > 0 {
            let start = CMTime(value: CMTimeValue(startMs), timescale: 1_000)
            reader.timeRange = CMTimeRange(start: start, duration: .positiveInfinity)
        }
        guard reader.startReading() else {
            throw reader.error ?? VesperNativeFrameAudioOutputError.readerStartFailed
        }
        let outputURL = FileManager.default.temporaryDirectory
            .appendingPathComponent("vesper-native-audio-\(UUID().uuidString).caf")
        do {
            var audioFile: AVAudioFile?
            while let sampleBuffer = output.copyNextSampleBuffer() {
                if let pcmBuffer = pcmBuffer(from: sampleBuffer) {
                    if audioFile == nil {
                        audioFile = try AVAudioFile(
                            forWriting: outputURL,
                            settings: pcmBuffer.format.settings
                        )
                    }
                    try audioFile?.write(from: pcmBuffer)
                }
            }
            if reader.status == .failed {
                throw reader.error ?? VesperNativeFrameAudioOutputError.readerFailed
            }
            guard audioFile != nil else {
                throw VesperNativeFrameAudioOutputError.readerProducedNoAudio
            }
            audioFile = nil
            return VesperNativeFrameAudioPcmFile(
                file: try AVAudioFile(forReading: outputURL),
                url: outputURL
            )
        } catch {
            VesperNativeFrameAudioTemporaryFileStore.removeFile(at: outputURL)
            throw error
        }
    }

    nonisolated private static func pcmBuffer(from sampleBuffer: CMSampleBuffer) -> AVAudioPCMBuffer? {
        guard let formatDescription = CMSampleBufferGetFormatDescription(sampleBuffer) else {
            return nil
        }
        let audioFormat = AVAudioFormat(cmAudioFormatDescription: formatDescription)
        let frameCount = AVAudioFrameCount(CMSampleBufferGetNumSamples(sampleBuffer))
        guard let buffer = AVAudioPCMBuffer(pcmFormat: audioFormat, frameCapacity: frameCount) else {
            return nil
        }
        buffer.frameLength = frameCount
        var blockBuffer: CMBlockBuffer?
        var audioBufferList = AudioBufferList()
        let status = CMSampleBufferGetAudioBufferListWithRetainedBlockBuffer(
            sampleBuffer,
            bufferListSizeNeededOut: nil,
            bufferListOut: &audioBufferList,
            bufferListSize: MemoryLayout<AudioBufferList>.size,
            blockBufferAllocator: kCFAllocatorDefault,
            blockBufferMemoryAllocator: kCFAllocatorDefault,
            flags: 0,
            blockBufferOut: &blockBuffer
        )
        guard status == noErr else { return nil }
        let audioBuffer = audioBufferList.mBuffers
        guard let sourceData = audioBuffer.mData else { return nil }
        let byteCount = Int(audioBuffer.mDataByteSize)
        if let target = buffer.floatChannelData?[0] {
            memcpy(target, sourceData, byteCount)
        } else if let target = buffer.mutableAudioBufferList.pointee.mBuffers.mData {
            memcpy(target, sourceData, byteCount)
        }
        return buffer
    }
}

private struct VesperNativeFrameAudioPcmFile {
    let file: AVAudioFile
    let url: URL
}

@MainActor
final class VesperNativeFrameAudioTemporaryFileStore {
    private let fileManager: FileManager
    private(set) var activeFileURL: URL?

    init(fileManager: FileManager = .default) {
        self.fileManager = fileManager
    }

    func replaceActiveFile(with url: URL?) {
        cleanupActiveFile()
        activeFileURL = url
    }

    func cleanupActiveFile() {
        guard let activeFileURL else { return }
        Self.removeFile(at: activeFileURL, fileManager: fileManager)
        self.activeFileURL = nil
    }

    nonisolated static func removeFile(at url: URL, fileManager: FileManager = .default) {
        try? fileManager.removeItem(at: url)
    }
}

@MainActor
final class VesperNativeFrameAudioPlaybackGate {
    private(set) var generation: UInt64 = 0
    private(set) var wantsPlayback = false

    func beginPlayback() -> UInt64 {
        wantsPlayback = true
        generation = generation &+ 1
        return generation
    }

    func cancelPlayback() {
        wantsPlayback = false
        generation = generation &+ 1
    }

    func isCurrent(_ generation: UInt64) -> Bool {
        wantsPlayback && self.generation == generation
    }
}

private enum VesperNativeFrameAudioOutputError: LocalizedError {
    case noAudioTrack
    case readerOutputRejected
    case readerStartFailed
    case readerFailed
    case readerProducedNoAudio

    var errorDescription: String? {
        switch self {
        case .noAudioTrack:
            return "source has no audio track"
        case .readerOutputRejected:
            return "AVAssetReader rejected the native audio output"
        case .readerStartFailed:
            return "AVAssetReader failed to start"
        case .readerFailed:
            return "AVAssetReader failed"
        case .readerProducedNoAudio:
            return "AVAssetReader produced no audio samples"
        }
    }
}

struct VesperNativeFrameAudioBridgeState: Equatable {
    let hasAudioTrack: Bool
    let decoderKind: String
    let outputKind: String
    let pipelineKind: String
    let rateControlKind: String
    let clockSource: String
    let issue: String?

    static func resolved(
        hasAudioTrack: Bool,
        bridgePrepared: Bool,
        unavailableReason: String? = nil
    ) -> VesperNativeFrameAudioBridgeState {
        if bridgePrepared {
            return VesperNativeFrameAudioBridgeState(
                hasAudioTrack: true,
                decoderKind: "swiftNativeAudioBridge",
                outputKind: "swiftNativeAudioBridge",
                pipelineKind: "swiftNativeAudioBridgeV1",
                rateControlKind: "swiftNativeAudioBridgeTimePitch",
                clockSource: "swiftNativeAudioBridge",
                issue: nil
            )
        }
        if hasAudioTrack {
            return VesperNativeFrameAudioBridgeState(
                hasAudioTrack: true,
                decoderKind: "unavailable",
                outputKind: "unavailable",
                pipelineKind: "swiftNativeAudioBridgeV1",
                rateControlKind: "unavailable",
                clockSource: "video",
                issue: unavailableReason ?? "Swift native audio bridge is unavailable."
            )
        }
        return VesperNativeFrameAudioBridgeState(
            hasAudioTrack: false,
            decoderKind: "none",
            outputKind: "none",
            pipelineKind: "none",
            rateControlKind: "none",
            clockSource: "video",
            issue: nil
        )
    }
}

struct VesperNativeFramePipelineFrame {
    let frameHandle: UInt64
    let pixelBufferAddress: UInt
    let presentationTimeUs: Int64
    let durationUs: Int64?
    let width: Int
    let height: Int
    let leaseGeneration: UInt64
}

/// Result of polling the SDK pipeline for the next frame. `endOfStream` is a
/// terminal signal distinct from `pending` (decoder still draining) so the
/// display loop can stop polling and report end-of-playback.
enum VesperNativeFramePipelineAdvanceOutcome {
    case frame(VesperNativeFramePipelineFrame)
    case pending
    case endOfStream
}

struct VesperNativeFramePipelineTimeline: Equatable {
    let positionMs: Int64
    let durationMs: Int64?
}

enum VesperNativeFramePipelineRouteDecision: Equatable {
    case systemPlayer
    case fallback(VesperNativeFramePipelineIssue)
    case fail(VesperNativeFramePipelineIssue)
    case waitForSurface(VesperNativeFramePipelineIssue)
    case nativeFrame
}

struct VesperNativeFramePipelineCounters: Equatable {
    var processedFrames = 0
    var presentedFrames = 0
    var deadlineMisses = 0
    var backpressureCount = 0
    var lateDropped = 0
    var skippedAudioPackets = 0
    var skippedVideoPackets = 0
    var skippedOtherPackets = 0
}

struct VesperNativeFramePipelineStartupError: LocalizedError, Equatable {
    let issue: VesperNativeFramePipelineIssue

    var message: String {
        issue.message
    }

    var errorDescription: String? {
        message
    }
}

struct VesperNativeFramePipelineIssue: Equatable {
    enum Kind: String {
        case missingSurface
        case missingSourceNormalizerPacketPlugin
        case missingVideoToolboxDecoderPlugin
        case unsupportedSource
        case unsupportedCodec
        case sessionNotPrepared
        case sessionClosed
        case startupFailure
    }

    let kind: Kind
    let message: String

    static func classifyStartupFailure(_ message: String) -> VesperNativeFramePipelineIssue {
        if let parsed = parseWireIssue(message) {
            return parsed
        }
        let normalized = message.lowercased()
        if normalized.contains("playersurfaceview") || normalized.contains("surface view") {
            return VesperNativeFramePipelineIssue(kind: .missingSurface, message: message)
        }
        if normalized.contains("sourcenormalizer packet-stream plugin path") ||
            normalized.contains("sourcenormalizer packet plugin") ||
            normalized.contains("source normalizer packet plugin") ||
            normalized.contains("failed to open plugin library")
        {
            return VesperNativeFramePipelineIssue(
                kind: .missingSourceNormalizerPacketPlugin,
                message: message
            )
        }
        if normalized.contains("videotoolbox decoder plugin path") ||
            normalized.contains("is not a native-frame decoder plugin") ||
            normalized.contains("failed to load native-frame decoder plugin")
        {
            return VesperNativeFramePipelineIssue(
                kind: .missingVideoToolboxDecoderPlugin,
                message: message
            )
        }
        if normalized.contains("unsupported source") ||
            normalized.contains("does not handle hls") ||
            normalized.contains("does not handle dash") ||
            normalized.contains("system playback remains the supported route")
        {
            return VesperNativeFramePipelineIssue(kind: .unsupportedSource, message: message)
        }
        if normalized.contains("unsupported codec") ||
            normalized.contains("does not support") ||
            normalized.contains("first pass only supports") ||
            normalized.contains("decoder not found") ||
            normalized.contains("failed to inspect video stream")
        {
            return VesperNativeFramePipelineIssue(kind: .unsupportedCodec, message: message)
        }
        if normalized.contains("already closed") {
            return VesperNativeFramePipelineIssue(kind: .sessionClosed, message: message)
        }
        if normalized.contains("not prepared") {
            return VesperNativeFramePipelineIssue(kind: .sessionNotPrepared, message: message)
        }
        return VesperNativeFramePipelineIssue(kind: .startupFailure, message: message)
    }

    private static func parseWireIssue(_ message: String) -> VesperNativeFramePipelineIssue? {
        let prefix = "nativeFrameIssueKind="
        guard message.hasPrefix(prefix),
              let separator = message.firstIndex(of: ";") else {
            return nil
        }
        let kindStart = message.index(message.startIndex, offsetBy: prefix.count)
        let rawKind = String(message[kindStart..<separator])
        let detailsStart = message.index(after: separator)
        let details = message[detailsStart...].trimmingCharacters(in: .whitespacesAndNewlines)
        guard let kind = Kind(rawValue: rawKind) else {
            return VesperNativeFramePipelineIssue(kind: .startupFailure, message: details)
        }
        return VesperNativeFramePipelineIssue(kind: kind, message: details)
    }
}
