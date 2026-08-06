import Foundation

@MainActor
enum PlayerBridgeFactory {
    private static let defaultBackend: PlayerBridgeBackend = .rustNativeStub

    static func defaultBridgeBackend() -> PlayerBridgeBackend {
        defaultBackend
    }

    static func makeDefaultBridge(
        initialSource: VesperPlayerSource? = nil,
        resiliencePolicy: VesperPlaybackResiliencePolicy = VesperPlaybackResiliencePolicy(),
        trackPreferencePolicy: VesperTrackPreferencePolicy = VesperTrackPreferencePolicy(),
        preloadBudgetPolicy: VesperPreloadBudgetPolicy = VesperPreloadBudgetPolicy(),
        keepScreenOnDuringPlayback: Bool = true,
        benchmarkConfiguration: VesperBenchmarkConfiguration = .disabled,
        sourceNormalizerConfiguration: VesperSourceNormalizerConfiguration =
            VesperSourceNormalizerConfiguration(),
        frameProcessorConfiguration: VesperFrameProcessorConfiguration =
            VesperFrameProcessorConfiguration(),
        nativeFramePipelineConfiguration: VesperNativeFramePipelineConfiguration =
            VesperNativeFramePipelineConfiguration(),
        pipelineEventHookConfiguration: VesperPipelineEventHookConfiguration =
            VesperPipelineEventHookConfiguration()
    ) -> VesperPlayerController {
        switch defaultBackend {
        case .fakeDemo:
            return VesperPlayerController(
                FakePlayerBridge(
                    initialSource: initialSource,
                    resiliencePolicy: resiliencePolicy,
                    trackPreferencePolicy: trackPreferencePolicy,
                    preloadBudgetPolicy: preloadBudgetPolicy,
                    benchmarkConfiguration: benchmarkConfiguration
                ),
                keepScreenOnDuringPlayback: keepScreenOnDuringPlayback
            )
        case .rustNativeStub:
            return VesperPlayerController(
                VesperNativePlayerBridge(
                    initialSource: initialSource,
                    resiliencePolicy: resiliencePolicy,
                    trackPreferencePolicy: trackPreferencePolicy,
                    preloadBudgetPolicy: preloadBudgetPolicy,
                    benchmarkConfiguration: benchmarkConfiguration,
                    sourceNormalizerConfiguration: sourceNormalizerConfiguration,
                    frameProcessorConfiguration: frameProcessorConfiguration,
                    nativeFramePipelineConfiguration: nativeFramePipelineConfiguration,
                    pipelineEventHookConfiguration: pipelineEventHookConfiguration
                ),
                keepScreenOnDuringPlayback: keepScreenOnDuringPlayback
            )
        }
    }
}

@MainActor
public enum VesperPlayerControllerFactory {
    public static func defaultBackend() -> PlayerBridgeBackend {
        PlayerBridgeFactory.defaultBridgeBackend()
    }

    public static func makeDefault(
        initialSource: VesperPlayerSource? = nil,
        resiliencePolicy: VesperPlaybackResiliencePolicy = VesperPlaybackResiliencePolicy(),
        trackPreferencePolicy: VesperTrackPreferencePolicy = VesperTrackPreferencePolicy(),
        preloadBudgetPolicy: VesperPreloadBudgetPolicy = VesperPreloadBudgetPolicy(),
        keepScreenOnDuringPlayback: Bool = true,
        benchmarkConfiguration: VesperBenchmarkConfiguration = .disabled,
        sourceNormalizerConfiguration: VesperSourceNormalizerConfiguration =
            VesperSourceNormalizerConfiguration(),
        frameProcessorConfiguration: VesperFrameProcessorConfiguration =
            VesperFrameProcessorConfiguration(),
        nativeFramePipelineConfiguration: VesperNativeFramePipelineConfiguration =
            VesperNativeFramePipelineConfiguration(),
        pipelineEventHookConfiguration: VesperPipelineEventHookConfiguration =
            VesperPipelineEventHookConfiguration()
    ) -> VesperPlayerController {
        PlayerBridgeFactory.makeDefaultBridge(
            initialSource: initialSource,
            resiliencePolicy: resiliencePolicy,
            trackPreferencePolicy: trackPreferencePolicy,
            preloadBudgetPolicy: preloadBudgetPolicy,
            keepScreenOnDuringPlayback: keepScreenOnDuringPlayback,
            benchmarkConfiguration: benchmarkConfiguration,
            sourceNormalizerConfiguration: sourceNormalizerConfiguration,
            frameProcessorConfiguration: frameProcessorConfiguration,
            nativeFramePipelineConfiguration: nativeFramePipelineConfiguration,
            pipelineEventHookConfiguration: pipelineEventHookConfiguration
        )
    }

    public static func probePlaybackCapability(
        _ request: VesperPlaybackCapabilityProbeRequest
    ) -> VesperPlaybackCapabilityProbeResult {
        VesperPlaybackCapabilityProbe.probe(
            VesperPlaybackCapabilityProbeRequest(
                source: request.source,
                codec: request.codec,
                width: request.width,
                height: request.height,
                frameRate: request.frameRate,
                requiresNativeFrame: request.requiresNativeFrame,
                sourceNormalizerConfiguration: request.sourceNormalizerConfiguration,
                frameProcessorConfiguration: request.frameProcessorConfiguration,
                nativeFramePipelineConfiguration: request.nativeFramePipelineConfiguration
            ),
            sessionProbeProvider: VesperIOSSessionProbeProvider.currentDisplay()
        )
    }
}
