@preconcurrency import AVFoundation
import UIKit
@testable import VesperPlayerKit

struct DashStartupPlaybackResult: Codable {
    let firstFrame: Bool
    let position: Double
    let warmRequests: Int
    let originRequests: Int
    let state: String
    var passed: Bool { firstFrame && position > 3 && warmRequests == 4 && originRequests > 0 }
}

@MainActor
func runDashStartupPlaybackScenario() async throws -> DashStartupPlaybackResult {
        // A second loopback server is a deterministic counting origin for the unwarmed ranges.
        // The production session still uses its own startup server and its normal HLS byte ranges.
        let originTransport = try DashStartupFixtureTransport()
        let originClient = VesperDashStartupNetworkClient(scope: .init(), headers: [:], cache: VesperDashStartupCache(), transport: originTransport)
        let origin = VesperDashStartupServer(client: originClient)
        defer { origin.close() }
        let originURL = try await origin.register(.init(url: URL(string: "https://fixture.test/video.mp4")!))
        let warmTransport = try DashStartupFixtureTransport(mediaURL: originURL.absoluteString)
        let manifestURL = URL(string: "https://fixture.test/manifest.mpd")!
        let scope = VesperDashStartupScope()
        var source = VesperPlayerSource.dash(url: manifestURL)
        source.dashStartupScope = scope
        _ = try await vesperWarmDashStartup(source: source, scope: scope, transport: warmTransport)
        let bridge = VesperNativePlayerBridge(initialSource: source, resiliencePolicy: .streaming(),
                                              systemPlayerVolume: 0, systemPlayerIsMuted: true)
        let controller = VesperPlayerController(bridge, keepScreenOnDuringPlayback: false)
        let surface = PlayerSurfaceView(frame: CGRect(x: 0, y: 0, width: 160, height: 160))
        let window: UIWindow
        if let scene = UIApplication.shared.connectedScenes.compactMap({ $0 as? UIWindowScene }).first {
            window = UIWindow(windowScene: scene)
        } else { window = UIWindow(frame: surface.bounds) }
        let root = UIViewController()
        window.rootViewController = root
        root.view.addSubview(surface)
        window.makeKeyAndVisible()
        defer {
            controller.dispose()
            surface.detachBridgeIfNeeded()
            window.isHidden = true
        }
        controller.attachSurfaceHost(surface)
        controller.initialize()
        let deadline = ContinuousClock.now.advanced(by: .seconds(20))
        var firstFrame = false
        var position = 0.0
        while ContinuousClock.now < deadline {
            controller.refresh()
            surface.layoutIfNeeded()
            firstFrame = surface.pictureInPicturePlayerLayer?.isReadyForDisplay == true
            position = surface.pictureInPicturePlayerLayer?.player?.currentTime().seconds ?? 0
            if firstFrame && position > 3 { break }
            try await Task.sleep(for: .milliseconds(100))
        }
        return DashStartupPlaybackResult(firstFrame: firstFrame, position: position,
            warmRequests: await warmTransport.requests, originRequests: await originTransport.requests,
            state: String(describing: controller.uiState))
}
