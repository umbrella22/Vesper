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
                }
            }

            if !pictureInPicturePresentation &&
                (controlsVisible || uiState.playbackState != .playing) {
                ZStack {
                    VStack(spacing: 0) {
                        LinearGradient(
                            colors: [Color.black.opacity(0.72), Color.clear],
                            startPoint: .top,
                            endPoint: .bottom
                        )
                        .frame(height: 108)

                        Spacer(minLength: 0)

                        LinearGradient(
                            colors: [Color.clear, Color.black.opacity(0.82)],
                            startPoint: .top,
                            endPoint: .bottom
                        )
                        .frame(height: 144)
                    }

                    VStack(spacing: 0) {
                        HStack(alignment: .top) {
                            if let onNavigateBack {
                                StageIconButton(
                                    systemName: "chevron.left",
                                    size: 38,
                                    iconSize: 19,
                                    backgroundOpacity: 0.0,
                                    action: onNavigateBack
                                )
                                .accessibilityLabel(
                                    Text(
                                        navigateBackAccessibilityLabel
                                            ?? VesperPlayerStageStrings.navigateBack
                                    )
                                )
                            }

                            VStack(alignment: .leading, spacing: 4) {
                                HStack(spacing: 8) {
                                    Text(uiState.sourceLabel)
                                        .font(.headline.weight(.bold))
                                        .foregroundStyle(.white)
                                        .lineLimit(1)

                                    if uiState.isBuffering {
                                        StageChip(
                                            label: VesperPlayerStageStrings.buffering,
                                            accent: Color(red: 1.0, green: 0.71, blue: 0.33),
                                            compact: true
                                        )
                                    }
                                }
                                Text(stageBadgeText(uiState.timeline))
                                    .font(.caption)
                                    .foregroundStyle(Color.white.opacity(0.70))
                            }

                            Spacer(minLength: 12)

                            StageIconButton(
                                systemName: "ellipsis",
                                size: 38,
                                iconSize: 22,
                                backgroundOpacity: 0.0
                            ) {
                                onOpenSheet(.menu)
                                controlsVisible = true
                            }
                        }
                        .padding(.horizontal, 18)
                        .padding(.top, 16)

                        Spacer(minLength: 0)

                        if isFullscreen {
                            landscapeControls
                        } else {
                            portraitControls
                        }
                    }
                }
                .transition(.opacity)
            }

            if !pictureInPicturePresentation, let gestureFeedback {
                StageGestureFeedbackPanel(feedback: gestureFeedback)
                    .transition(.opacity)
            }
        }
        .clipShape(RoundedRectangle(cornerRadius: isFullscreen ? 0 : 28, style: .continuous))
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .overlay {
            if !isFullscreen {
                RoundedRectangle(cornerRadius: 28, style: .continuous)
                    .stroke(Color.white.opacity(0.08), lineWidth: 1)
            }
        }
        .onDisappear {
            endTemporarySpeedGesture()
        }
        .onChange(of: pictureInPicturePresentation) { _, enabled in
            guard enabled else {
                return
            }
            enterPictureInPicturePresentation()
        }
    }
}
