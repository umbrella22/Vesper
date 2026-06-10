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
            VesperNativeFramePipelineConfiguration()
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
            let resolvedSourceNormalizerConfiguration =
                VesperBundledPluginResolver.resolveSourceNormalizerConfiguration(
                    sourceNormalizerConfiguration
                )
            return VesperPlayerController(
                VesperNativePlayerBridge(
                    initialSource: initialSource,
                    resiliencePolicy: resiliencePolicy,
                    trackPreferencePolicy: trackPreferencePolicy,
                    preloadBudgetPolicy: preloadBudgetPolicy,
                    benchmarkConfiguration: benchmarkConfiguration,
                    sourceNormalizerConfiguration: resolvedSourceNormalizerConfiguration,
                    frameProcessorConfiguration: frameProcessorConfiguration,
                    nativeFramePipelineConfiguration: nativeFramePipelineConfiguration
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
            VesperNativeFramePipelineConfiguration()
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
            nativeFramePipelineConfiguration: nativeFramePipelineConfiguration
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
                sourceNormalizerConfiguration:
                    VesperBundledPluginResolver.resolveSourceNormalizerConfiguration(
                        request.sourceNormalizerConfiguration
                    ),
                frameProcessorConfiguration: request.frameProcessorConfiguration,
                nativeFramePipelineConfiguration: request.nativeFramePipelineConfiguration
            ),
            sessionProbeProvider: VesperIOSSessionProbeProvider.currentDisplay()
        )
    }
}
