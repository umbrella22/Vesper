import Foundation

@MainActor
enum PlayerBridgeFactory {
    private static let defaultBackend: PlayerBridgeBackend = .rustNativeStub

    static func defaultBridgeBackend() -> PlayerBridgeBackend {
        defaultBackend
    }

    static func makeDefaultBridge(
        initialSource: VesperPlayerSource? = nil,
        resiliencePolicy: VesperPlaybackResiliencePolicy = VesperPlaybackResiliencePolicy()
    ) -> VesperPlayerController {
        switch defaultBackend {
        case .fakeDemo:
            VesperPlayerController(
                FakePlayerBridge(initialSource: initialSource, resiliencePolicy: resiliencePolicy)
            )
        case .rustNativeStub:
            VesperPlayerController(
                VesperNativePlayerBridge(
                    initialSource: initialSource,
                    resiliencePolicy: resiliencePolicy
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
        resiliencePolicy: VesperPlaybackResiliencePolicy = VesperPlaybackResiliencePolicy()
    ) -> VesperPlayerController {
        PlayerBridgeFactory.makeDefaultBridge(
            initialSource: initialSource,
            resiliencePolicy: resiliencePolicy
        )
    }
}
