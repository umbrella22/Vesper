import SwiftUI
import VesperPlayerKit
import VesperPlayerKitUI

@MainActor
private final class StageSkinInteractionModel: ObservableObject {
    @Published var playing = true
    @Published var fullscreen = false
    @Published var controlsVisible = true
    @Published var pendingSeekRatio: Double?
    @Published var customSkinEnabled = true
    @Published var layout = VesperStageControlLayout.compact
    @Published var actions = 0
    @Published var standaloneActions = 0
    @Published var decorativeActions = 0
    var hudVisible = false
    var hudIconFrame = CGRect.zero
    @Published var touchesDuringHUD = 0
    @Published var brightnessChanges = 0
}

/// Observes touch-down before gesture recognition without changing event delivery.
final class StageSkinInteractionWindow: UIWindow {
    private let model = StageSkinInteractionModel()

    func installHarness() {
        rootViewController = UIHostingController(rootView: StageSkinInteractionHarness(model: model))
    }

    override func sendEvent(_ event: UIEvent) {
        for touch in event.allTouches ?? [] where touch.phase == .began {
            if model.hudVisible && model.hudIconFrame.contains(touch.location(in: self)) {
                model.touchesDuringHUD += 1
            }
        }
        super.sendEvent(event)
    }
}

/// An inert surface makes view replacement observable without loading media.
private struct SkinTestSurface: UIViewRepresentable {
    func makeUIView(context: Context) -> UIView {
        let view = UIView()
        view.backgroundColor = .black
        view.isAccessibilityElement = true
        view.accessibilityIdentifier = "skin-surface"
        view.accessibilityLabel = "Test surface"
        view.accessibilityValue = UUID().uuidString
        return view
    }

    func updateUIView(_ uiView: UIView, context: Context) {}
}

@MainActor
private struct StageSkinInteractionHarness: View {
    @ObservedObject var model: StageSkinInteractionModel

    var body: some View {
        VStack(spacing: 12) {
            HStack {
                Button(model.customSkinEnabled ? "Use default skin" : "Use custom skin") {
                    model.customSkinEnabled.toggle()
                }.accessibilityIdentifier("toggle-skin")
                Button(model.layout == .compact ? "Expand controls" : "Compact controls") {
                    model.layout = model.layout == .compact ? .expanded : .compact
                }.accessibilityIdentifier("toggle-layout")
            }
            stage
                .frame(height: 320)
            HStack {
                VesperStageIconButton(
                    label: "Test action", style: .init(size: 20, iconSize: 12),
                    action: { model.standaloneActions += 1 }
                ) {
                    Button("Decorative action") { model.decorativeActions += 1 }
                }
                .accessibilityIdentifier("standalone-action")
                VesperStageIcon(.play)
            }
            .vesperPlayerStageSkin(customSkin)
            Text("\(model.actions)").accessibilityIdentifier("stage-actions")
            Text("\(model.standaloneActions)").accessibilityIdentifier("standalone-actions")
            Text("\(model.decorativeActions)").accessibilityIdentifier("decorative-actions")
            Text("\(model.touchesDuringHUD)").accessibilityIdentifier("hud-touches")
            Text(verbatim: String(model.controlsVisible)).accessibilityIdentifier("controls-visible")
            Text("\(model.brightnessChanges)").accessibilityIdentifier("brightness-changes")
            Spacer(minLength: 0)
        }
        .padding(.horizontal, 8)
        .padding(.top, 8)
    }

    private var customSkin: VesperPlayerStageSkin {
        VesperPlayerStageSkin(
            colors: .init(foreground: .green, hudForeground: .cyan),
            iconBuilder: { role, style in
                let isHUD = [.brightness, .volume, .speed].contains(role)
                return AnyView(
                    Button("Decorative icon") { model.decorativeActions += 1 }
                        .frame(width: style.size, height: style.size)
                        .background(style.color)
                        .onGeometryChange(for: CGRect.self) { $0.frame(in: .global) } action: { frame in
                            if isHUD { model.hudIconFrame = frame }
                        }
                        .onAppear { if isHUD { model.hudVisible = true } }
                        .onDisappear { if isHUD { model.hudVisible = false } }
                )
            }
        )
    }

    private var stage: some View {
        VesperPlayerStage(
            surface: AnyView(SkinTestSurface()),
            uiState: PlayerHostUiState(
                title: "Video", subtitle: "", sourceLabel: "Video",
                playbackState: model.playing ? .playing : .paused,
                playbackRate: 1, isBuffering: false, isInterrupted: false,
                timeline: .init(kind: .vod, isSeekable: true, seekableRange: nil,
                                liveEdgeMs: nil, positionMs: 25_000, durationMs: 100_000)
            ),
            trackCatalog: .empty, trackSelection: .init(),
            effectiveVideoTrackId: nil, fixedTrackStatus: nil,
            controlsVisible: $model.controlsVisible,
            pendingSeekRatio: $model.pendingSeekRatio,
            controlLayout: model.layout, isFullscreen: model.fullscreen,
            onSeekBy: { _ in model.actions += 1 },
            onTogglePause: { model.actions += 1; model.playing.toggle() },
            onSeekToRatio: { _ in model.actions += 1 },
            onSeekToLiveEdge: { model.actions += 1 },
            onToggleFullscreen: { model.actions += 1; model.fullscreen.toggle() },
            onOpenSheet: { _ in model.actions += 1 },
            currentBrightnessRatio: { 0.5 },
            onSetBrightnessRatio: { value in model.brightnessChanges += 1; return value },
            onNavigateBack: { model.actions += 1 },
            skin: model.customSkinEnabled ? customSkin : nil
        )
    }
}
