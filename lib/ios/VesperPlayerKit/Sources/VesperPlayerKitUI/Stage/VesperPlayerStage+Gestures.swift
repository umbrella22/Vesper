import SwiftUI
import VesperPlayerKit

extension VesperPlayerStage {
    func stageDragGesture(stageSize: CGSize) -> some Gesture {
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

    func temporarySpeedGesture() -> some Gesture {
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

    func startTemporarySpeedGesture() {
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

    func endTemporarySpeedGesture() {
        guard let restoreRate = speedGestureRestoreRate else {
            return
        }
        speedGestureRestoreRate = nil
        onSetPlaybackRate(restoreRate)
    }

    func showGestureFeedback(_ feedback: StageGestureFeedback) {
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
}

enum StageAreaGestureKind {
    case brightness
    case volume
    case seek
    case ignored
}

enum StageGestureKind {
    case brightness
    case volume
    case speed
}

struct StageGestureFeedback {
    let kind: StageGestureKind
    let progress: Double?
    let label: String
}

struct StageGestureFeedbackPanel: View {
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

func percentLabel(_ value: Double) -> String {
    "\(Int((value * 100).rounded()))%"
}
