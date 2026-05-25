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
            VesperFrameProcessorConfiguration()
    ) -> VesperPlayerController {
        switch defaultBackend {
        case .fakeDemo:
            VesperPlayerController(
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
            VesperPlayerController(
                VesperNativePlayerBridge(
                    initialSource: initialSource,
                    resiliencePolicy: resiliencePolicy,
                    trackPreferencePolicy: trackPreferencePolicy,
                    preloadBudgetPolicy: preloadBudgetPolicy,
                    benchmarkConfiguration: benchmarkConfiguration,
                    sourceNormalizerConfiguration: sourceNormalizerConfiguration,
                    frameProcessorConfiguration: frameProcessorConfiguration
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
            VesperFrameProcessorConfiguration()
    ) -> VesperPlayerController {
        PlayerBridgeFactory.makeDefaultBridge(
            initialSource: initialSource,
            resiliencePolicy: resiliencePolicy,
            trackPreferencePolicy: trackPreferencePolicy,
            preloadBudgetPolicy: preloadBudgetPolicy,
            keepScreenOnDuringPlayback: keepScreenOnDuringPlayback,
            benchmarkConfiguration: benchmarkConfiguration,
            sourceNormalizerConfiguration: sourceNormalizerConfiguration,
            frameProcessorConfiguration: frameProcessorConfiguration
        )
    }
}
