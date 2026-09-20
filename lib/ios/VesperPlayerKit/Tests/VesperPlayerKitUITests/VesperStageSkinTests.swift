import SwiftUI
import UIKit
import XCTest
import VesperPlayerKit
@testable import VesperPlayerKitUI

@MainActor
final class VesperStageSkinTests: XCTestCase {
    func testBothLayoutsResolveActionIconsAndSkinChangesPreserveSurface() {
        for layout in [VesperStageControlLayout.compact, .expanded] {
            let recorder = Recorder()
            let model = SkinModel()
            model.skin = skin(recorder, color: .green)
            let host = UIHostingController(rootView: StageHarness(model: model, recorder: recorder, layout: layout))
            let window = mount(host, size: CGSize(width: 600, height: 340))
            defer { window.isHidden = true }
            XCTAssertTrue(recorder.roles.isSuperset(of: [.pause, .fullscreen, .navigateBack, .more]))
            XCTAssertEqual(recorder.surfaceCreations, 1)
            XCTAssertEqual(recorder.colors[.pause], .green)
            recorder.roles.removeAll()
            model.skin = skin(recorder, color: .orange)
            render(host)
            XCTAssertTrue(recorder.roles.contains(.pause))
            XCTAssertEqual(recorder.colors[.pause], .orange)
            XCTAssertEqual(recorder.surfaceCreations, 1)
            XCTAssertEqual(recorder.actions, 0)
            model.playing = false
            model.fullscreen = true
            render(host)
            XCTAssertTrue(recorder.roles.isSuperset(of: [.play, .exitFullscreen]))
            model.skin = nil
            render(host)
            XCTAssertEqual(recorder.surfaceCreations, 1)
            XCTAssertEqual(recorder.actions, 0)
        }
    }

    func testNilBuilderResultRendersConfiguredSymbol() {
        let icons = VesperPlayerStageIcons(play: "stop.fill")
        let normal = image(VesperStageIcon(.play).vesperPlayerStageSkin(.init(icons: icons)))
        let fallback = image(VesperStageIcon(.play).vesperPlayerStageSkin(.init(icons: icons, iconBuilder: { _, _ in nil })))
        let other = image(VesperStageIcon(.play))
        XCTAssertEqual(normal.pngData(), fallback.pngData())
        XCTAssertNotEqual(normal.pngData(), other.pngData())
    }

    func testHUDAndStandaloneButtonsReadTheSameSkin() {
        let recorder = Recorder()
        let content = VStack {
            VesperStagePrimaryPlayButton(isPlaying: true, action: {})
            ForEach([StageGestureKind.brightness, .volume, .speed], id: \.self) { kind in
                StageGestureFeedbackPanel(feedback: .init(kind: kind, progress: 0.5, label: "50%"))
            }
        }.vesperPlayerStageSkin(skin(recorder, color: .pink))
        _ = image(content, size: CGSize(width: 320, height: 300))
        XCTAssertTrue(recorder.roles.isSuperset(of: [.pause, .brightness, .volume, .speed]))
        XCTAssertEqual(recorder.colors[.pause], .pink)
        XCTAssertEqual(recorder.colors[.brightness], .cyan)
    }

    func testSmallVisualButtonRetains44PointLayout() {
        let button = VesperStageIconButton(label: "Custom", style: .init(size: 20, iconSize: 12), action: {}) {
            Image(systemName: "heart.fill")
        }
        let host = UIHostingController(rootView: button)
        XCTAssertEqual(host.sizeThatFits(in: CGSize(width: 200, height: 200)), CGSize(width: 44, height: 44))
    }

    private func skin(_ recorder: Recorder, color: Color) -> VesperPlayerStageSkin {
        VesperPlayerStageSkin(colors: .init(foreground: color, hudForeground: .cyan), iconBuilder: { role, style in
            recorder.roles.insert(role)
            recorder.colors[role] = style.color
            return AnyView(Rectangle().fill(style.color).frame(width: style.size, height: style.size))
        })
    }

    private func mount<Content: View>(_ host: UIHostingController<Content>, size: CGSize) -> UIWindow {
        let window = UIWindow(frame: CGRect(origin: .zero, size: size))
        window.rootViewController = host
        window.makeKeyAndVisible()
        host.view.frame = window.bounds
        render(host)
        return window
    }

    private func render<Content: View>(_ host: UIHostingController<Content>) {
        host.view.setNeedsLayout()
        host.view.layoutIfNeeded()
        RunLoop.main.run(until: Date().addingTimeInterval(0.05))
        host.view.layoutIfNeeded()
    }

    private func image<Content: View>(_ view: Content, size: CGSize = CGSize(width: 64, height: 64)) -> UIImage {
        let renderer = ImageRenderer(content: view.frame(width: size.width, height: size.height))
        renderer.scale = 1
        return renderer.uiImage!
    }
}

@MainActor
private final class Recorder {
    var roles: Set<VesperStageIconRole> = []
    var colors: [VesperStageIconRole: Color] = [:]
    var surfaceCreations = 0
    var actions = 0
}

@MainActor
private final class SkinModel: ObservableObject {
    @Published var skin: VesperPlayerStageSkin?
    @Published var playing = true
    @Published var fullscreen = false
}

@MainActor
private struct CountingSurface: UIViewRepresentable {
    let recorder: Recorder
    func makeUIView(context: Context) -> UIView { recorder.surfaceCreations += 1; return UIView() }
    func updateUIView(_ uiView: UIView, context: Context) {}
}

@MainActor
private struct StageHarness: View {
    @ObservedObject var model: SkinModel
    let recorder: Recorder
    let layout: VesperStageControlLayout
    var body: some View {
        VesperPlayerStage(
            surface: AnyView(CountingSurface(recorder: recorder)),
            uiState: PlayerHostUiState(title: "Video", subtitle: "", sourceLabel: "Video",
                playbackState: model.playing ? .playing : .paused, playbackRate: 1,
                isBuffering: false, isInterrupted: false,
                timeline: .init(kind: .vod, isSeekable: true, seekableRange: nil,
                    liveEdgeMs: nil, positionMs: 25000, durationMs: 100000)),
            trackCatalog: .empty, trackSelection: .init(), effectiveVideoTrackId: nil, fixedTrackStatus: nil,
            controlsVisible: .constant(true), pendingSeekRatio: .constant(nil),
            controlLayout: layout, isFullscreen: model.fullscreen,
            onSeekBy: { _ in recorder.actions += 1 }, onTogglePause: { recorder.actions += 1 },
            onSeekToRatio: { _ in recorder.actions += 1 }, onSeekToLiveEdge: { recorder.actions += 1 },
            onToggleFullscreen: { recorder.actions += 1 }, onOpenSheet: { _ in recorder.actions += 1 },
            onNavigateBack: { recorder.actions += 1 }, skin: model.skin
        )
    }
}
