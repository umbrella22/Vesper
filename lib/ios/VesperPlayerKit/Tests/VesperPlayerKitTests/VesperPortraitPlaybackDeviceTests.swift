@preconcurrency import AVFoundation
import SwiftUI
import XCTest
@testable import VesperPlayerKit

final class VesperPortraitPlaybackDeviceTests: XCTestCase {
    @MainActor
    func testRotatedPortraitPlaybackSurvivesRetainedFullscreenContainer() async throws {
        guard ProcessInfo.processInfo.environment["VESPER_IOS_PLAYBACK_DEVICE_TESTS"] == "1" else {
            throw XCTSkip("Set VESPER_IOS_PLAYBACK_DEVICE_TESTS=1 to run device presentation acceptance")
        }
        let fixture = try XCTUnwrap(Bundle(for: Self.self).url(
            forResource: "device-720p-h264-aac", withExtension: "m4v"
        ))
        let portrait = try await makePortraitFixture(from: fixture)
        defer { try? FileManager.default.removeItem(at: portrait) }
        let bridge = VesperNativePlayerBridge(
            initialSource: .localFile(url: portrait, label: "rotated-portrait"),
            systemPlayerVolume: 0, systemPlayerIsMuted: true
        )
        let controller = VesperPlayerController(bridge)
        let state = PortraitContainerState(controller: controller)
        let scene = try XCTUnwrap(UIApplication.shared.connectedScenes.compactMap { $0 as? UIWindowScene }.first)
        let window = UIWindow(windowScene: scene)
        window.rootViewController = UIHostingController(rootView: PortraitContainerHost(state: state))
        window.makeKeyAndVisible()
        defer {
            controller.dispose()
            window.isHidden = true
            window.rootViewController = nil
        }
        controller.initialize()

        try await waitFor("Inline portrait video did not render") {
            state.inline?.geometry != nil && state.inline?.pictureInPicturePlayerLayer?.isReadyForDisplay == true
        }
        let inline = try XCTUnwrap(state.inline)
        try assertPortrait(controller: controller, surface: inline, checkpoint: "inline")
        state.fullscreen = true
        try await waitFor("Fullscreen portrait video did not render") {
            state.modal?.geometry != nil && state.modal?.pictureInPicturePlayerLayer?.isReadyForDisplay == true
        }
        let modal = try XCTUnwrap(state.modal)
        XCTAssertNil(inline.pictureInPicturePlayerLayer)
        try assertPortrait(controller: controller, surface: modal, checkpoint: "fullscreen")
        let beforeDismiss = try XCTUnwrap(modal.pictureInPicturePlayerLayer?.player).currentTime().seconds
        state.fullscreen = false
        try await waitFor("Retained inline video did not resume after fullscreen dismissal") {
            inline.pictureInPicturePlayerLayer?.isReadyForDisplay == true
                && (inline.pictureInPicturePlayerLayer?.player?.currentTime().seconds ?? 0) > beforeDismiss + 0.5
                && inline.geometry != nil
        }
        XCTAssertTrue(state.inline === inline)
        XCTAssertTrue(bridge.surfaceHost === inline)
        try assertPortrait(controller: controller, surface: inline, checkpoint: "restored-inline")
        XCTAssertNil(controller.lastError)
    }

    @MainActor
    private func assertPortrait(
        controller: VesperPlayerController, surface: PlayerSurfaceView, checkpoint: String
    ) throws {
        let presentation = try XCTUnwrap(controller.videoPresentation)
        let geometry = try XCTUnwrap(surface.geometry)
        XCTAssertEqual(presentation.displayWidth, 720)
        XCTAssertEqual(presentation.displayHeight, 1280)
        XCTAssertEqual(geometry.contentRect.width / geometry.contentRect.height, 9.0 / 16, accuracy: 0.002)
        let evidence: [String: Any] = [
            "checkpoint": checkpoint,
            "displayWidth": presentation.displayWidth, "displayHeight": presentation.displayHeight,
            "viewWidth": geometry.width, "viewHeight": geometry.height,
            "contentRect": ["x": geometry.contentRect.minX, "y": geometry.contentRect.minY,
                            "width": geometry.contentRect.width, "height": geometry.contentRect.height],
            "readyForDisplay": surface.pictureInPicturePlayerLayer?.isReadyForDisplay == true,
            "positionSeconds": surface.pictureInPicturePlayerLayer?.player?.currentTime().seconds ?? 0,
        ]
        let attachment = XCTAttachment(data: try JSONSerialization.data(withJSONObject: evidence),
                                       uniformTypeIdentifier: "public.json")
        attachment.name = "portrait-\(checkpoint).json"
        attachment.lifetime = .keepAlways
        add(attachment)
    }

    @MainActor
    private func waitFor(_ message: String, condition: () -> Bool) async throws {
        let deadline = ContinuousClock.now.advanced(by: .seconds(15))
        while !condition(), ContinuousClock.now < deadline {
            try await Task.sleep(for: .milliseconds(100))
        }
        guard condition() else {
            throw NSError(domain: "PortraitDeviceTest", code: 1, userInfo: [NSLocalizedDescriptionKey: message])
        }
    }

    private func makePortraitFixture(from url: URL) async throws -> URL {
        let asset = AVURLAsset(url: url)
        let tracks = try await asset.loadTracks(withMediaType: .video)
        let original = try XCTUnwrap(tracks.first)
        let composition = AVMutableComposition()
        let track = try XCTUnwrap(composition.addMutableTrack(withMediaType: .video,
                                                            preferredTrackID: kCMPersistentTrackID_Invalid))
        let duration = try await asset.load(.duration)
        try track.insertTimeRange(CMTimeRange(start: .zero, duration: duration), of: original, at: .zero)
        let size = try await original.load(.naturalSize)
        track.preferredTransform = CGAffineTransform(a: 0, b: 1, c: -1, d: 0, tx: size.height, ty: 0)
        let export = try XCTUnwrap(AVAssetExportSession(asset: composition, presetName: AVAssetExportPresetPassthrough))
        let output = FileManager.default.temporaryDirectory.appendingPathComponent("portrait-\(UUID()).mov")
        export.outputURL = output
        export.outputFileType = .mov
        await export.export()
        guard export.status == .completed else {
            throw export.error ?? NSError(domain: "PortraitDeviceTest", code: 2)
        }
        return output
    }
}

@MainActor
private final class PortraitContainerState: ObservableObject {
    let controller: VesperPlayerController
    @Published var fullscreen = false
    var inline: PlayerSurfaceView?
    var modal: PlayerSurfaceView?

    init(controller: VesperPlayerController) { self.controller = controller }
}

private struct PortraitContainerHost: View {
    @ObservedObject var state: PortraitContainerState

    var body: some View {
        PlayerSurfaceContainer(controller: state.controller, onSurfaceReady: { state.inline = $0 })
            .frame(height: 240)
            .fullScreenCover(isPresented: $state.fullscreen) {
                PlayerSurfaceContainer(controller: state.controller, onSurfaceReady: { state.modal = $0 })
            }
    }
}
