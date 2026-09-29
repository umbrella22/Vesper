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
        if ProcessInfo.processInfo.arguments.contains("--source-lifecycle-smoke") {
            Task { @MainActor in
                let watchdog = Task { @MainActor in
                    do { try await Task.sleep(for: .seconds(60)) } catch { return }
                    var timeout = SourceLifecyclePlaybackResult()
                    timeout.errors = [.init(stage: "watchdog", code: "scenario_timeout")]
                    emitSourceLifecycleSmoke(timeout)
                    exit(1)
                }
                let arguments = ProcessInfo.processInfo.arguments
                func argument(_ name: String) throws -> URL? {
                    guard let index = arguments.firstIndex(of: name) else { return nil }
                    guard arguments.indices.contains(index + 1),
                          let url = URL(string: arguments[index + 1]), url.scheme != nil else {
                        throw NSError(domain: "SourceLifecycleSmokeArguments", code: 1)
                    }
                    return url
                }
                let result: SourceLifecyclePlaybackResult
                do {
                    let media = try argument("--source-lifecycle-smoke-media-url") ?? sourceLifecycleDefaultMediaURL
                    let stats = try argument("--source-lifecycle-smoke-stats-url")
                    result = await runSourceLifecyclePlaybackScenario(mediaURL: media, statsURL: stats)
                } catch {
                    var failure = SourceLifecyclePlaybackResult()
                    failure.errors = [.init(stage: "arguments", code: "invalid_argument")]
                    result = failure
                }
                watchdog.cancel()
                emitSourceLifecycleSmoke(result)
                exit(result.passed ? 0 : 1)
            }
            return
        }
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

private func emitSourceLifecycleSmoke(_ result: SourceLifecyclePlaybackResult) {
    let encoder = JSONEncoder()
    encoder.outputFormatting = [.sortedKeys]
    if let data = try? encoder.encode(result) {
        print("SOURCE_LIFECYCLE_SMOKE " + String(decoding: data, as: UTF8.self))
    } else {
        print("SOURCE_LIFECYCLE_SMOKE {\"passed\":false,\"errors\":[{\"stage\":\"encoding\",\"code\":\"invalid_result\"}]}")
    }
}
