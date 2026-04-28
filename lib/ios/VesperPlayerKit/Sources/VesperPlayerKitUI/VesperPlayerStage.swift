import SwiftUI
import VesperPlayerKit

@MainActor
public struct VesperPlayerStage: View {
    let surface: AnyView
    let uiState: PlayerHostUiState
    let trackCatalog: VesperTrackCatalog
    let trackSelection: VesperTrackSelectionSnapshot
    let effectiveVideoTrackId: String?
    let fixedTrackStatus: VesperFixedTrackStatus?
    @Binding var controlsVisible: Bool
    @Binding var pendingSeekRatio: Double?
    let isCompactLayout: Bool
    let isFullscreen: Bool
    let onSeekBy: (Int64) -> Void
    let onTogglePause: () -> Void
    let onSeekToRatio: (Double) -> Void
    let onSeekToLiveEdge: () -> Void
    let onSetPlaybackRate: (Float) -> Void
    let onToggleFullscreen: () -> Void
    let onOpenSheet: (VesperPlayerStageSheet) -> Void
    let currentBrightnessRatio: () -> Double?
    let onSetBrightnessRatio: (Double) -> Double?
    let currentVolumeRatio: () -> Double?
    let onSetVolumeRatio: (Double) -> Double?
    @State private var stageGestureKind: StageAreaGestureKind?
    @State private var deviceGestureStartRatio = 0.0
    @State private var seekGestureRatio = 0.0
    @State private var gestureFeedback: StageGestureFeedback?
    @State private var gestureFeedbackTask: Task<Void, Never>?
    @State private var speedGestureRestoreRate: Float?

    public init(
        surface: AnyView,
        uiState: PlayerHostUiState,
        trackCatalog: VesperTrackCatalog,
        trackSelection: VesperTrackSelectionSnapshot,
        effectiveVideoTrackId: String?,
        fixedTrackStatus: VesperFixedTrackStatus?,
        controlsVisible: Binding<Bool>,
        pendingSeekRatio: Binding<Double?>,
        isCompactLayout: Bool,
        isFullscreen: Bool,
        onSeekBy: @escaping (Int64) -> Void,
        onTogglePause: @escaping () -> Void,
        onSeekToRatio: @escaping (Double) -> Void,
        onSeekToLiveEdge: @escaping () -> Void,
        onSetPlaybackRate: @escaping (Float) -> Void = { _ in },
        onToggleFullscreen: @escaping () -> Void,
        onOpenSheet: @escaping (VesperPlayerStageSheet) -> Void,
        currentBrightnessRatio: @escaping () -> Double? = { nil },
        onSetBrightnessRatio: @escaping (Double) -> Double? = { _ in nil },
        currentVolumeRatio: @escaping () -> Double? = { nil },
        onSetVolumeRatio: @escaping (Double) -> Double? = { _ in nil }
    ) {
        self.surface = surface
        self.uiState = uiState
        self.trackCatalog = trackCatalog
        self.trackSelection = trackSelection
        self.effectiveVideoTrackId = effectiveVideoTrackId
        self.fixedTrackStatus = fixedTrackStatus
        _controlsVisible = controlsVisible
        _pendingSeekRatio = pendingSeekRatio
        self.isCompactLayout = isCompactLayout
        self.isFullscreen = isFullscreen
        self.onSeekBy = onSeekBy
        self.onTogglePause = onTogglePause
        self.onSeekToRatio = onSeekToRatio
        self.onSeekToLiveEdge = onSeekToLiveEdge
        self.onSetPlaybackRate = onSetPlaybackRate
        self.onToggleFullscreen = onToggleFullscreen
        self.onOpenSheet = onOpenSheet
        self.currentBrightnessRatio = currentBrightnessRatio
        self.onSetBrightnessRatio = onSetBrightnessRatio
        self.currentVolumeRatio = currentVolumeRatio
        self.onSetVolumeRatio = onSetVolumeRatio
    }

    public var body: some View {
        ZStack {
            surface

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

            if let gestureFeedback {
                StageGestureFeedbackPanel(feedback: gestureFeedback)
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
        .onDisappear {
            endTemporarySpeedGesture()
        }
    }

    private func stageDragGesture(stageSize: CGSize) -> some Gesture {
        DragGesture(minimumDistance: 8)
            .onChanged { value in
                guard speedGestureRestoreRate == nil else {
                    return
                }
                let verticalDistance = abs(value.translation.height)
                let horizontalDistance = abs(value.translation.width)
                let stageWidth = max(stageSize.width, 1)
                let stageHeight = max(stageSize.height, 1)

                if stageGestureKind == nil {
                    guard verticalDistance >= 8 || horizontalDistance >= 8 else {
                        return
                    }

                    if horizontalDistance >= verticalDistance * 1.15 {
                        guard uiState.timeline.isSeekable else {
                            stageGestureKind = .ignored
                            return
                        }
                        stageGestureKind = .seek
                    } else if verticalDistance >= horizontalDistance * 1.15 {
                        let deviceKind: StageGestureKind =
                            value.startLocation.x < stageWidth / 2 ? .brightness : .volume
                        let startRatio: Double?
                        switch deviceKind {
                        case .brightness:
                            startRatio = currentBrightnessRatio()
                        case .volume:
                            startRatio = currentVolumeRatio()
                        case .speed:
                            startRatio = nil
                        }
                        guard let startRatio else {
                            stageGestureKind = .ignored
                            return
                        }
                        switch deviceKind {
                        case .brightness:
                            stageGestureKind = .brightness
                        case .volume:
                            stageGestureKind = .volume
                        case .speed:
                            stageGestureKind = .ignored
                        }
                        deviceGestureStartRatio = startRatio.clamped(to: 0...1)
                    } else {
                        return
                    }
                }

                guard let stageGestureKind, stageGestureKind != .ignored else {
                    return
                }

                if stageGestureKind == .seek {
                    seekGestureRatio = (value.location.x / stageWidth).clamped(to: 0...1)
                    pendingSeekRatio = seekGestureRatio
                    controlsVisible = true
                    return
                }

                let deviceKind: StageGestureKind
                switch stageGestureKind {
                case .brightness:
                    deviceKind = .brightness
                case .volume:
                    deviceKind = .volume
                case .seek, .ignored:
                    return
                }

                let requestedRatio =
                    (deviceGestureStartRatio - value.translation.height / stageHeight * 1.15)
                        .clamped(to: 0...1)
                let actualRatio: Double?
                switch deviceKind {
                case .brightness:
                    actualRatio = onSetBrightnessRatio(requestedRatio)
                case .volume:
                    actualRatio = onSetVolumeRatio(requestedRatio)
                case .speed:
                    actualRatio = nil
                }
                guard let actualRatio else {
                    return
                }
                controlsVisible = true
                let value = actualRatio.clamped(to: 0...1)
                showGestureFeedback(
                    StageGestureFeedback(kind: deviceKind, progress: value, label: percentLabel(value))
                )
            }
            .onEnded { _ in
                if stageGestureKind == .seek {
                    onSeekToRatio(seekGestureRatio)
                    pendingSeekRatio = nil
                    controlsVisible = true
                }
                stageGestureKind = nil
            }
    }

    private func temporarySpeedGesture() -> some Gesture {
        LongPressGesture(minimumDuration: 0.45)
            .sequenced(before: DragGesture(minimumDistance: 0))
            .onChanged { value in
                guard case .second(true, _) = value else {
                    return
                }
                startTemporarySpeedGesture()
            }
            .onEnded { _ in
                endTemporarySpeedGesture()
            }
    }

    private func startTemporarySpeedGesture() {
        if speedGestureRestoreRate == nil {
            speedGestureRestoreRate = uiState.playbackRate
            onSetPlaybackRate(2.0)
        }
        stageGestureKind = nil
        controlsVisible = true
        showGestureFeedback(
            StageGestureFeedback(kind: .speed, progress: nil, label: speedBadge(2.0))
        )
    }

    private func endTemporarySpeedGesture() {
        guard let restoreRate = speedGestureRestoreRate else {
            return
        }
        speedGestureRestoreRate = nil
        onSetPlaybackRate(restoreRate)
    }

    private func showGestureFeedback(_ feedback: StageGestureFeedback) {
        gestureFeedback = feedback
        gestureFeedbackTask?.cancel()
        gestureFeedbackTask = Task { @MainActor in
            try? await Task.sleep(for: .milliseconds(520))
            guard !Task.isCancelled else {
                return
            }
            gestureFeedback = nil
        }
    }

    private var isPlaying: Bool {
        uiState.playbackState == .playing
    }

    private var playButtonSymbol: String {
        isPlaying ? "pause.fill" : "play.fill"
    }

    private var playButtonLabel: String {
        isPlaying ? VesperPlayerStageStrings.pause : VesperPlayerStageStrings.play
    }

    private var qualityPillLabel: String {
        qualityButtonLabel(
            trackCatalog,
            trackSelection,
            effectiveVideoTrackId: effectiveVideoTrackId,
            fixedTrackStatus: fixedTrackStatus
        )
    }

    private var portraitControls: some View {
        HStack(spacing: 8) {
            StageIconButton(
                systemName: playButtonSymbol,
                size: 38,
                iconSize: 17,
                backgroundOpacity: 0.0
            ) {
                onTogglePause()
                controlsVisible = true
            }
            .accessibilityLabel(Text(playButtonLabel))

            TimelineScrubber(
                displayedRatio: pendingSeekRatio ?? uiState.timeline.displayedRatio ?? 0.0,
                compact: true,
                enabled: uiState.timeline.isSeekable,
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

            Text(compactTimelineSummary(uiState.timeline, pendingSeekRatio: pendingSeekRatio))
                .font(.caption2.monospacedDigit().weight(.semibold))
                .foregroundStyle(.white)
                .lineLimit(1)

            if uiState.timeline.kind == .liveDvr {
                StagePillButton(label: liveButtonLabel(uiState.timeline), compact: true) {
                    onSeekToLiveEdge()
                    controlsVisible = true
                }
            }

            StageIconButton(
                systemName: "arrow.up.left.and.arrow.down.right",
                size: 38,
                iconSize: 18,
                backgroundOpacity: 0.0
            ) {
                onToggleFullscreen()
                controlsVisible = true
            }
        }
        .padding(.horizontal, 18)
        .padding(.bottom, 18)
    }

    private var landscapeControls: some View {
        VStack(alignment: .leading, spacing: 4) {
            Text(timelineSummary(uiState.timeline, pendingSeekRatio: pendingSeekRatio))
                .font(.caption.monospacedDigit().weight(.semibold))
                .foregroundStyle(.white)
                .lineLimit(1)

            TimelineScrubber(
                displayedRatio: pendingSeekRatio ?? uiState.timeline.displayedRatio ?? 0.0,
                compact: true,
                enabled: uiState.timeline.isSeekable,
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
                StageIconButton(
                    systemName: playButtonSymbol,
                    size: 38,
                    iconSize: 17,
                    backgroundOpacity: 0.0
                ) {
                    onTogglePause()
                    controlsVisible = true
                }
                .accessibilityLabel(Text(playButtonLabel))

                Spacer(minLength: 12)

                if uiState.timeline.kind == .liveDvr {
                    StagePillButton(label: liveButtonLabel(uiState.timeline), compact: true) {
                        onSeekToLiveEdge()
                        controlsVisible = true
                    }
                }

                StagePillButton(label: speedBadge(uiState.playbackRate), compact: true) {
                    onOpenSheet(.speed)
                    controlsVisible = true
                }

                StagePillButton(label: qualityPillLabel, compact: true) {
                    onOpenSheet(.quality)
                    controlsVisible = true
                }

                StageIconButton(
                    systemName: "arrow.down.right.and.arrow.up.left",
                    size: 34,
                    iconSize: 17,
                    backgroundOpacity: 0.0
                ) {
                    onToggleFullscreen()
                    controlsVisible = true
                }
            }
        }
        .padding(.horizontal, 18)
        .padding(.bottom, 14)
    }
}

private enum StageAreaGestureKind {
    case brightness
    case volume
    case seek
    case ignored
}

private enum StageGestureKind {
    case brightness
    case volume
    case speed
}

private struct StageGestureFeedback {
    let kind: StageGestureKind
    let progress: Double?
    let label: String
}

private struct StageGestureFeedbackPanel: View {
    let feedback: StageGestureFeedback

    var body: some View {
        HStack(spacing: 10) {
            Image(systemName: symbolName)
                .font(.system(size: 23, weight: .semibold))
                .foregroundStyle(.white)

            if let progress = feedback.progress {
                GeometryReader { proxy in
                    ZStack(alignment: .leading) {
                        Capsule()
                            .fill(Color.white.opacity(0.18))

                        Capsule()
                            .fill(Color.white)
                            .frame(width: proxy.size.width * progress.clamped(to: 0...1))
                    }
                }
                .frame(height: 4)
            }

            Text(feedback.label)
                .font(.caption.weight(.semibold))
                .foregroundStyle(.white)
                .monospacedDigit()
        }
        .frame(width: feedback.progress == nil ? nil : 226)
        .padding(.horizontal, 14)
        .padding(.vertical, 10)
        .background(Color.black.opacity(0.72), in: Capsule())
    }

    private var symbolName: String {
        switch feedback.kind {
        case .brightness:
            return "sun.max.fill"
        case .volume:
            return "speaker.wave.2.fill"
        case .speed:
            return "speedometer"
        }
    }
}

private func percentLabel(_ value: Double) -> String {
    "\(Int((value * 100).rounded()))%"
}

struct TimelineScrubber: View {
    let displayedRatio: Double
    let compact: Bool
    var enabled: Bool = true
    let onSeekPreview: (Double) -> Void
    let onSeekCommit: (Double) -> Void
    let onSeekCancel: () -> Void

    var body: some View {
        GeometryReader { proxy in
            let width = max(proxy.size.width, 1)
            let ratio = displayedRatio.clamped(to: 0...1)
            let knobSize = compact ? 12.0 : 14.0
            let knobOffset = max(0, min(width - knobSize, width * ratio - knobSize / 2))
            let activeOpacity = enabled ? 1.0 : 0.42

            ZStack(alignment: .leading) {
                Capsule()
                    .fill(Color.white.opacity(enabled ? 0.16 : 0.10))
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
                    .opacity(activeOpacity)
                    .frame(width: width * ratio, height: 4)

                Circle()
                    .fill(Color.white.opacity(activeOpacity))
                    .frame(width: knobSize, height: knobSize)
                    .offset(x: knobOffset)
            }
            .frame(height: compact ? 22 : 28, alignment: .center)
            .contentShape(Rectangle())
            .allowsHitTesting(enabled)
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
    var compact: Bool = false
    let action: () -> Void

    init(systemName: String? = nil, label: String, compact: Bool = false, action: @escaping () -> Void) {
        self.systemName = systemName
        self.label = label
        self.compact = compact
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
                    .font((compact ? Font.caption2 : .caption).weight(.semibold))
                    .lineLimit(1)
            }
            .foregroundStyle(.white)
            .padding(.horizontal, compact ? 10 : 12)
            .padding(.vertical, compact ? 7 : 9)
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
