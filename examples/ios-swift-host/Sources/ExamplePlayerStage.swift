import SwiftUI
import VesperPlayerKit

struct ExamplePlayerStage: View {
    let surface: AnyView
    let uiState: PlayerHostUiState
    @Binding var controlsVisible: Bool
    @Binding var pendingSeekRatio: Double?
    let isCompactLayout: Bool
    let isFullscreen: Bool
    let onSeekBy: (Int64) -> Void
    let onTogglePause: () -> Void
    let onSeekToRatio: (Double) -> Void
    let onSeekToLiveEdge: () -> Void
    let onToggleFullscreen: () -> Void
    let onOpenSheet: (ExamplePlayerSheet) -> Void

    var body: some View {
        ZStack {
            surface

            HStack(spacing: 0) {
                Color.clear
                    .contentShape(Rectangle())
                    .onTapGesture(count: 2) {
                        onSeekBy(-10_000)
                        controlsVisible = true
                    }

                Color.clear
                    .contentShape(Rectangle())
                    .onTapGesture(count: 2) {
                        onSeekBy(10_000)
                        controlsVisible = true
                    }
            }

            Color.clear
                .contentShape(Rectangle())
                .onTapGesture {
                    controlsVisible.toggle()
                }

            if controlsVisible || uiState.playbackState != .playing {
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
                            VStack(alignment: .leading, spacing: 4) {
                                HStack(spacing: 8) {
                                    Text(uiState.sourceLabel)
                                        .font(.headline.weight(.bold))
                                        .foregroundStyle(.white)
                                        .lineLimit(1)

                                    if uiState.isBuffering {
                                        StageChip(
                                            label: ExampleI18n.buffering,
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

                            if isCompactLayout {
                                StageIconButton(
                                    systemName: "ellipsis",
                                    size: 38,
                                    iconSize: 22,
                                    backgroundOpacity: 0.0
                                ) {
                                    onOpenSheet(.menu)
                                }
                            } else {
                                HStack(spacing: 10) {
                                    StageIconButton(systemName: "dial.high", backgroundOpacity: 0.0) {
                                        onOpenSheet(.quality)
                                    }
                                    StageIconButton(systemName: "waveform", backgroundOpacity: 0.0) {
                                        onOpenSheet(.audio)
                                    }
                                    StageIconButton(systemName: "captions.bubble", backgroundOpacity: 0.0) {
                                        onOpenSheet(.subtitle)
                                    }
                                    StageIconButton(systemName: "speedometer", backgroundOpacity: 0.0) {
                                        onOpenSheet(.speed)
                                    }
                                }
                            }
                        }
                        .padding(.horizontal, 18)
                        .padding(.top, 16)

                        Spacer(minLength: 0)

                        HStack(spacing: 16) {
                            StageIconButton(systemName: "gobackward.10") {
                                onSeekBy(-10_000)
                                controlsVisible = true
                            }

                            StagePrimaryPlayButton(isPlaying: uiState.playbackState == .playing) {
                                onTogglePause()
                                controlsVisible = true
                            }

                            StageIconButton(systemName: "goforward.10") {
                                onSeekBy(10_000)
                                controlsVisible = true
                            }
                        }

                        Spacer(minLength: 0)

                        VStack(alignment: .leading, spacing: isCompactLayout ? 6 : 4) {
                            TimelineScrubber(
                                displayedRatio: pendingSeekRatio ?? uiState.timeline.displayedRatio ?? 0.0,
                                compact: !isCompactLayout,
                                onSeekPreview: { ratio in
                                    pendingSeekRatio = ratio
                                    controlsVisible = true
                                },
                                onSeekCommit: { ratio in
                                    onSeekToRatio(ratio)
                                    pendingSeekRatio = nil
                                    controlsVisible = true
                                },
                                onSeekCancel: {
                                    pendingSeekRatio = nil
                                }
                            )

                            HStack(alignment: .center) {
                                Text(timelineSummary(uiState.timeline, pendingSeekRatio: pendingSeekRatio))
                                    .font(.caption.weight(.semibold))
                                    .foregroundStyle(.white)

                                Spacer(minLength: 12)

                                if uiState.timeline.kind == .liveDvr {
                                    StagePillButton(label: liveButtonLabel(uiState.timeline)) {
                                        onSeekToLiveEdge()
                                        controlsVisible = true
                                    }
                                }

                                StageIconButton(
                                    systemName: isFullscreen
                                        ? "arrow.down.right.and.arrow.up.left"
                                        : "arrow.up.left.and.arrow.down.right",
                                    size: isCompactLayout ? 36 : 38,
                                    iconSize: 18,
                                    backgroundOpacity: 0.0
                                ) {
                                    onToggleFullscreen()
                                    controlsVisible = true
                                }
                            }
                        }
                        .padding(.horizontal, 18)
                        .padding(.bottom, 18)
                    }
                }
                .transition(.opacity)
            }
        }
        .clipShape(RoundedRectangle(cornerRadius: isFullscreen ? 0 : 28, style: .continuous))
        .overlay {
            if !isFullscreen {
                RoundedRectangle(cornerRadius: 28, style: .continuous)
                    .stroke(Color.white.opacity(0.08), lineWidth: 1)
            }
        }
    }
}

struct TimelineScrubber: View {
    let displayedRatio: Double
    let compact: Bool
    let onSeekPreview: (Double) -> Void
    let onSeekCommit: (Double) -> Void
    let onSeekCancel: () -> Void

    var body: some View {
        GeometryReader { proxy in
            let width = max(proxy.size.width, 1)
            let ratio = displayedRatio.clamped(to: 0...1)
            let knobSize = compact ? 12.0 : 14.0
            let knobOffset = max(0, min(width - knobSize, width * ratio - knobSize / 2))

            ZStack(alignment: .leading) {
                Capsule()
                    .fill(Color.white.opacity(0.16))
                    .frame(height: 4)

                Capsule()
                    .fill(
                        LinearGradient(
                            colors: [
                                Color(red: 1.0, green: 0.42, blue: 0.56),
                                Color(red: 1.0, green: 0.71, blue: 0.33),
                            ],
                            startPoint: .leading,
                            endPoint: .trailing
                        )
                    )
                    .frame(width: width * ratio, height: 4)

                Circle()
                    .fill(Color.white)
                    .frame(width: knobSize, height: knobSize)
                    .offset(x: knobOffset)
            }
            .frame(height: compact ? 22 : 28, alignment: .bottom)
            .contentShape(Rectangle())
            .gesture(
                DragGesture(minimumDistance: 0)
                    .onChanged { value in
                        onSeekPreview((value.location.x / width).clamped(to: 0...1))
                    }
                    .onEnded { value in
                        onSeekCommit((value.location.x / width).clamped(to: 0...1))
                    }
            )
        }
        .frame(height: compact ? 22 : 28)
    }
}

struct StagePrimaryPlayButton: View {
    let isPlaying: Bool
    let action: () -> Void

    var body: some View {
        Button(action: action) {
            Image(systemName: isPlaying ? "pause.fill" : "play.fill")
                .font(.system(size: 28, weight: .bold))
                .foregroundStyle(.white)
                .frame(width: 72, height: 72)
                .background(Color.white.opacity(0.14), in: Circle())
        }
        .buttonStyle(.plain)
    }
}

struct StageIconButton: View {
    let systemName: String
    var size: CGFloat = 52
    var iconSize: CGFloat = 18
    var backgroundOpacity: Double = 0.10
    let action: () -> Void

    var body: some View {
        Button(action: action) {
            Image(systemName: systemName)
                .font(.system(size: iconSize, weight: .semibold))
                .foregroundStyle(.white)
                .frame(width: size, height: size)
                .background(Color.white.opacity(backgroundOpacity), in: Circle())
        }
        .buttonStyle(.plain)
    }
}

struct StagePillButton: View {
    let systemName: String?
    let label: String
    let action: () -> Void

    init(systemName: String? = nil, label: String, action: @escaping () -> Void) {
        self.systemName = systemName
        self.label = label
        self.action = action
    }

    var body: some View {
        Button(action: action) {
            HStack(spacing: 6) {
                if let systemName {
                    Image(systemName: systemName)
                        .font(.system(size: 13, weight: .semibold))
                }
                Text(label)
                    .font(.caption.weight(.semibold))
                    .lineLimit(1)
            }
            .foregroundStyle(.white)
            .padding(.horizontal, 12)
            .padding(.vertical, 9)
            .background(Color.white.opacity(0.10), in: Capsule())
        }
        .buttonStyle(.plain)
    }
}

struct StageChip: View {
    let label: String
    let accent: Color
    var compact: Bool = false

    var body: some View {
        HStack(spacing: compact ? 6 : 8) {
            Circle()
                .fill(accent)
                .frame(width: compact ? 6 : 8, height: compact ? 6 : 8)

            Text(label)
                .font((compact ? Font.caption2 : .caption).weight(.semibold))
                .foregroundStyle(.white)
        }
        .padding(.horizontal, compact ? 8 : 10)
        .padding(.vertical, compact ? 5 : 7)
        .background(Color.black.opacity(0.36), in: Capsule())
        .overlay(
            Capsule()
                .stroke(Color.white.opacity(0.08), lineWidth: 1)
        )
    }
}
