@preconcurrency import AVFoundation
import CoreAudio
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
        let videoOutputFormat = activeSession?.videoOutputFormat ?? "pending"
        let videoTransfer = activeSession?.videoTransfer ?? "unknown"
        let videoBitDepth = activeSession?.videoBitDepth.map(String.init) ?? "unknown"
        let hdrKind = activeSession?.hdrKind ?? "sdr"
        let dolbyVisionMode = activeSession?.dolbyVisionMode ?? "none"
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
            "videoOutputFormat": videoOutputFormat,
            "videoTransfer": videoTransfer,
            "videoBitDepth": videoBitDepth,
            "hdrKind": hdrKind,
            "dolbyVisionMode": dolbyVisionMode,
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

    func startActiveSession() async -> Result<VesperNativeFramePipelineSession, VesperNativeFramePipelineStartupError> {
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
        let result = await activeSession.start()
        switch result {
        case .success:
            startupIssue = nil
        case .failure(let error):
            startupIssue = error.issue
        }
        return result
    }

    func closeActiveSession() {
        closeSession(activeSession)
    }

    func closeActiveSession(ifSameAs session: VesperNativeFramePipelineSession?) {
        guard let session, activeSession === session else {
            return
        }
        closeSession(session)
    }

    func closeSession(_ session: VesperNativeFramePipelineSession?) {
        guard let session else {
            return
        }
        let isActiveSession = activeSession === session
        session.close(detachPresenter: isActiveSession)
        if isActiveSession {
            activeSession = nil
        }
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
        guard sourceNormalizer.supportsPacketInput else {
            return VesperNativeFramePipelineIssue(
                kind: .missingSourceNormalizerPacketPlugin,
                message:
                    "iOS native-frame pipeline v1 requires SourceNormalizer packet-stream input via " +
                    "preflightOnly, preferNormalized, or requireNormalized mode. Disabled and diagnosticsOnly " +
                    "modes remain on system playback."
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
