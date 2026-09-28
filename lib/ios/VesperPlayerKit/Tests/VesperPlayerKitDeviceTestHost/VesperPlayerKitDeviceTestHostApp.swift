import UIKit

@main
final class VesperPlayerKitDeviceTestHostApp: UIResponder, UIApplicationDelegate {
    func application(
        _ application: UIApplication,
        didFinishLaunchingWithOptions launchOptions: [UIApplication.LaunchOptionsKey: Any]? = nil
    ) -> Bool {
        true
    }

    func application(
        _ application: UIApplication,
        configurationForConnecting connectingSceneSession: UISceneSession,
        options: UIScene.ConnectionOptions
    ) -> UISceneConfiguration {
        let configuration = UISceneConfiguration(
            name: "Device Test",
            sessionRole: connectingSceneSession.role
        )
        configuration.delegateClass = VesperPlayerKitDeviceTestHostSceneDelegate.self
        return configuration
    }
}

final class VesperPlayerKitDeviceTestHostSceneDelegate: UIResponder, UIWindowSceneDelegate {
    var window: UIWindow?

    func scene(
        _ scene: UIScene,
        willConnectTo session: UISceneSession,
        options connectionOptions: UIScene.ConnectionOptions
    ) {
        guard let windowScene = scene as? UIWindowScene else { return }
        let window: UIWindow
        if ProcessInfo.processInfo.arguments.contains("--stage-skin-ui-tests") {
            let testWindow = StageSkinInteractionWindow(windowScene: windowScene)
            testWindow.installHarness()
            window = testWindow
        } else {
            window = UIWindow(windowScene: windowScene)
            window.rootViewController = UIViewController()
        }
        window.makeKeyAndVisible()
        self.window = window
        if ProcessInfo.processInfo.arguments.contains("--dash-startup-smoke") {
            Task { @MainActor in
                do {
                    let result = try await runDashStartupPlaybackScenario()
                    let data = try JSONEncoder().encode(result)
                    print("DASH_STARTUP_SMOKE " + String(decoding: data, as: UTF8.self))
                    exit(result.passed ? 0 : 1)
                } catch {
                    print("DASH_STARTUP_SMOKE_FAILED \(error)")
                    exit(1)
                }
            }
        }
    }
}
