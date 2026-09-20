import SwiftUI
import VesperPlayerKit

extension VesperPlayerStage {
    public var body: some View {
        ZStack {
            surface
                .frame(maxWidth: .infinity, maxHeight: .infinity)

            if !pictureInPicturePresentation, let contentOverlay {
                contentOverlay
                    .frame(maxWidth: .infinity, maxHeight: .infinity)
                    .compositingGroup()
                    .allowsHitTesting(false)
                    .accessibilityHidden(true)
            }

            if !pictureInPicturePresentation {
                GeometryReader { proxy in
                    Color.clear
                        .contentShape(Rectangle())
                        .gesture(
                            TapGesture(count: 2)
                                .onEnded {
                                    onTogglePause()
                                    controlsVisible = true
                                }
                                .exclusively(
                                    before: TapGesture()
                                        .onEnded {
                                            controlsVisible.toggle()
                                        }
                                )
                        )
                        .simultaneousGesture(stageDragGesture(stageSize: proxy.size))
                        .simultaneousGesture(temporarySpeedGesture())
                        .id(interactionRevision)
                }
            }

            if !pictureInPicturePresentation &&
                (controlsVisible || uiState.playbackState != .playing) {
                ZStack {
                    VStack(spacing: 0) {
                        LinearGradient(
                            colors: [skin.colors.scrim.opacity(0.72), Color.clear],
                            startPoint: .top,
                            endPoint: .bottom
                        )
                        .frame(height: 108)

                        Spacer(minLength: 0)

                        LinearGradient(
                            colors: [Color.clear, skin.colors.scrim.opacity(0.82)],
                            startPoint: .top,
                            endPoint: .bottom
                        )
                        .frame(height: 144)
                    }
                    .allowsHitTesting(false)

                    VStack(spacing: 0) {
                        HStack(alignment: .top, spacing: skin.metrics.buttonSpacing) {
                            if let onNavigateBack {
                                StageIconButton(
                                    icon: .navigateBack,
                                    label: navigateBackAccessibilityLabel ?? VesperPlayerStageStrings.navigateBack,
                                    variant: .navigation,
                                    action: onNavigateBack
                                )
                            }

                            VStack(alignment: .leading, spacing: 4) {
                                HStack(spacing: skin.metrics.buttonSpacing) {
                                    Text(uiState.sourceLabel)
                                        .font(.headline.weight(.bold))
                                        .foregroundStyle(skin.colors.foreground)
                                        .lineLimit(1)

                                    if uiState.isBuffering {
                                        StageChip(
                                            label: VesperPlayerStageStrings.buffering,
                                            accent: skin.colors.accent,
                                            compact: true
                                        )
                                    }
                                }
                                Text(stageBadgeText(uiState.timeline))
                                    .font(.caption)
                                    .foregroundStyle(skin.colors.secondaryForeground)
                            }

                            Spacer(minLength: 12)

                            StageIconButton(
                                icon: .more,
                                label: VesperPlayerStageStrings.more,
                                variant: .toolbar
                            ) {
                                onOpenSheet(.menu)
                                controlsVisible = true
                            }
                        }
                        .padding(.horizontal, 18)
                        .padding(.top, 16)

                        Spacer(minLength: 0)

                        if controlLayout == .expanded {
                            expandedControls.id(interactionRevision)
                        } else {
                            compactControls.id(interactionRevision)
                        }
                    }
                }
                .transition(.opacity)
            }

            if !pictureInPicturePresentation, let gestureFeedback {
                StageGestureFeedbackPanel(feedback: gestureFeedback)
                    .allowsHitTesting(false)
                    .transition(.opacity)
            }
        }
        .environment(\.vesperPlayerStageSkin, skin)
        .background(skin.colors.background)
        .clipped()
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .overlay {
            if controlLayout == .compact {
                Rectangle()
                    .stroke(skin.colors.foreground.opacity(0.08), lineWidth: 1)
                    .allowsHitTesting(false)
            }
        }
        .onGeometryChange(for: CGSize.self) { $0.size } action: { size in
            if stageSize != .zero && stageSize != size { cancelInteraction() }
            stageSize = size
        }
        .onChange(of: controlLayout) { _, _ in cancelInteraction() }
        .onChange(of: isFullscreen) { _, _ in cancelInteraction() }
        .onDisappear { cancelInteraction() }
        .onChange(of: pictureInPicturePresentation) { _, enabled in
            guard enabled else {
                return
            }
            enterPictureInPicturePresentation()
        }
    }
}
