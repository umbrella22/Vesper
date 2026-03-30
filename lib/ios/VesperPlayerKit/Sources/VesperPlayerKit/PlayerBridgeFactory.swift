import Foundation

@MainActor
enum PlayerBridgeFactory {
    private static let defaultBackend: PlayerBridgeBackend = .rustNativeStub

    static func defaultBridgeBackend() -> PlayerBridgeBackend {
        defaultBackend
    }

    static func makeDefaultBridge(initialSource: VesperPlayerSource? = nil) -> VesperPlayerController {
        switch defaultBackend {
        case .fakeDemo:
            VesperPlayerController(FakePlayerBridge(initialSource: initialSource))
        case .rustNativeStub:
            VesperPlayerController(VesperNativePlayerBridge(initialSource: initialSource))
        }
    }
}

@MainActor
public enum VesperPlayerControllerFactory {
    public static func defaultBackend() -> PlayerBridgeBackend {
        PlayerBridgeFactory.defaultBridgeBackend()
    }

    public static func makeDefault(initialSource: VesperPlayerSource? = nil) -> VesperPlayerController {
        PlayerBridgeFactory.makeDefaultBridge(initialSource: initialSource)
    }
}
