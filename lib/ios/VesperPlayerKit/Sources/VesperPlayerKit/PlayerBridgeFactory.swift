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
        preloadBudgetPolicy: VesperPreloadBudgetPolicy = VesperPreloadBudgetPolicy()
    ) -> VesperPlayerController {
        switch defaultBackend {
        case .fakeDemo:
            VesperPlayerController(
                FakePlayerBridge(
                    initialSource: initialSource,
                    resiliencePolicy: resiliencePolicy,
                    trackPreferencePolicy: trackPreferencePolicy,
                    preloadBudgetPolicy: preloadBudgetPolicy
                )
            )
        case .rustNativeStub:
            VesperPlayerController(
                VesperNativePlayerBridge(
                    initialSource: initialSource,
                    resiliencePolicy: resiliencePolicy,
                    trackPreferencePolicy: trackPreferencePolicy,
                    preloadBudgetPolicy: preloadBudgetPolicy
                )
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
        preloadBudgetPolicy: VesperPreloadBudgetPolicy = VesperPreloadBudgetPolicy()
    ) -> VesperPlayerController {
        PlayerBridgeFactory.makeDefaultBridge(
            initialSource: initialSource,
            resiliencePolicy: resiliencePolicy,
            trackPreferencePolicy: trackPreferencePolicy,
            preloadBudgetPolicy: preloadBudgetPolicy
        )
    }
}
