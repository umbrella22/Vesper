@preconcurrency import AVFoundation
import CoreAudio
import Foundation
import VesperPlayerKitBridgeShim

@MainActor
final class VesperNativeFramePipelineSession {
    let id = UUID()
    let source: VesperPlayerSource
    let configuration: VesperNativeFramePipelineConfiguration
    let sourceNormalizer: VesperSourceNormalizerConfiguration
    var surfaceHost: PlayerSurfaceView
    var counters = VesperNativeFramePipelineCounters()
    var isClosed = false
    var didStart = false
    var durationMs: Int64?
    var seekable = false
    var hasAudioTrack = false
    var selectedVideoStreamIndex: Int?
    var selectedVideoMediaKind = "pending"
    var videoOutputFormat = "pending"
    var videoTransfer: String?
    var videoBitDepth: Int?
    var hdrKind: String?
    var dolbyVisionMode: String?
    var audioStreamIndex: Int?
    var audioMediaKind = "pending"
    var clockSource = "pending"
    var audioDecoderKind = "pending"
    var audioOutputKind = "pending"
    var audioPipelineKind = "pending"
    var audioRateControlKind = "pending"
    var audioOutputIssue: String?
    var onFramePresented: ((VesperNativeFramePipelineTimeline) -> Void)?
    var onPlaybackEnded: (() -> Void)?
    var onPlaybackFailed: ((VesperNativeFramePipelineIssue) -> Void)?
    var isPlaying = false
    var playbackRate: Float = 1.0
    let backend: VesperNativeFramePipelineBackend
    var runtime: VesperNativeFramePipelineRuntime?
    let audioOutput: VesperNativeFrameAudioOutputing
    var nativeFramePresenter: VesperNativeFramePresenting
    let usesSurfaceHostPresenter: Bool
    var audioBridgeState: VesperNativeFrameAudioBridgeState?
    let commandQueue = VesperNativeFramePipelineCommandQueue()

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
        usesSurfaceHostPresenter = nativeFramePresenter == nil
        self.audioOutput.onStateChanged = { [weak self] state in
            guard let self, !self.isClosed else { return }
            self.applyAudioBridgeState(state)
            if state.outputKind == "unavailable", let issue = state.issue {
                if state.hasAudioTrack {
                    iosHostLog("native audio pipeline unavailable; failing playback reason=\(issue)")
                    self.failPlaybackForAudioBridge(reason: issue)
                } else {
                    iosHostLog("native audio pipeline unavailable; using video clock reason=\(issue)")
                }
            }
        }
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

    func start() async -> Result<VesperNativeFramePipelineSession, VesperNativeFramePipelineStartupError> {
        guard !isClosed else {
            let issue = VesperNativeFramePipelineIssue(
                kind: .sessionClosed,
                message: "iOS native-frame pipeline session is already closed."
            )
            return .failure(
                VesperNativeFramePipelineStartupError(issue: issue)
            )
        }
        guard !didStart else {
            didStart = true
            return .success(self)
        }

        let backend = backend
        let source = source
        let configuration = configuration
        let sourceNormalizer = sourceNormalizer
        let openResult = await VesperBoundedUtilityQueue.shared.run(
            fallback: { Result.failure(Self.utilityQueueSaturatedStartupError()) }
        ) {
            backend.open(
                source: source,
                configuration: configuration,
                sourceNormalizer: sourceNormalizer
            )
        }
        guard !Task.isCancelled, !isClosed else {
            if case .success(let opened) = openResult {
                await closeOpenedHandleOffMain(opened.handle)
            }
            didStart = false
            return .failure(Self.closedStartupError())
        }
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

        didStart = true
        runtime = VesperNativeFramePipelineRuntime(
            owner: self,
            backend: backend,
            openedHandle: opened.handle
        )
        mergeStatus(from: opened.status)
        let audioState = await audioOutput.prepare(source: source, hasAudioTrack: hasAudioTrack)
        guard !Task.isCancelled, !isClosed else {
            runtime = nil
            await closeOpenedHandleOffMain(opened.handle)
            didStart = false
            return .failure(Self.closedStartupError())
        }
        applyAudioBridgeState(audioState)
        if audioState.outputKind == "swiftNativeAudioBridge" {
            iosHostLog("native audio pipeline configured audioPipeline=\(audioPipelineKind) source=\(source.uri)")
        } else if audioState.hasAudioTrack {
            let reason = audioState.issue ?? "Swift native audio bridge is unavailable."
            iosHostLog(
                "native audio pipeline unavailable audioPipeline=\(audioPipelineKind); playback cannot start source=\(source.uri) reason=\(reason)"
            )
            runtime = nil
            await closeOpenedHandleOffMain(opened.handle)
            didStart = false
            let issue = VesperNativeFramePipelineIssue(
                kind: .nativeAudioBridgeUnavailable,
                message: "nativeFrameIssueKind=nativeAudioBridgeUnavailable; \(reason)"
            )
            return .failure(VesperNativeFramePipelineStartupError(issue: issue))
        } else {
            let reason = audioState.issue.map { " reason=\($0)" } ?? ""
            iosHostLog(
                "native audio pipeline unavailable audioPipeline=\(audioPipelineKind); using video clock source=\(source.uri)\(reason)"
            )
        }
        return .success(self)
    }

    private func closeOpenedHandleOffMain(_ handle: UInt64) async {
        let backend = backend
        await VesperBoundedUtilityQueue.shared.runRequiredVoid {
            backend.close(handle: handle)
        }
    }

    private static func closedStartupError() -> VesperNativeFramePipelineStartupError {
        VesperNativeFramePipelineStartupError(
            issue: VesperNativeFramePipelineIssue(
                kind: .sessionClosed,
                message: "iOS native-frame pipeline session closed before startup completed."
            )
        )
    }

    private static func utilityQueueSaturatedStartupError() -> VesperNativeFramePipelineStartupError {
        VesperNativeFramePipelineStartupError(
            issue: VesperNativeFramePipelineIssue(
                kind: .startupFailure,
                message: "iOS native-frame pipeline utility queue is saturated."
            )
        )
    }
}
