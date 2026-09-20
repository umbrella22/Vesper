import SwiftUI
import VesperPlayerKit

extension VesperPlayerStage {
    var isPlaying: Bool {
        uiState.playbackState == .playing
    }

    var playButtonIcon: VesperStageIconRole {
        isPlaying ? .pause : .play
    }

    var playButtonLabel: String {
        isPlaying ? VesperPlayerStageStrings.pause : VesperPlayerStageStrings.play
    }

    var fullscreenIcon: VesperStageIconRole {
        isFullscreen ? .exitFullscreen : .fullscreen
    }

    var fullscreenLabel: String {
        isFullscreen ? VesperPlayerStageStrings.exitFullscreen : VesperPlayerStageStrings.fullscreen
    }

    var qualityPillLabel: String {
        qualityButtonLabel(
            trackCatalog,
            trackSelection,
            effectiveVideoTrackId: effectiveVideoTrackId,
            fixedTrackStatus: fixedTrackStatus
        )
    }

    var compactControls: some View {
        HStack(spacing: skin.metrics.buttonSpacing) {
            StageIconButton(
                icon: playButtonIcon,
                label: playButtonLabel,
                variant: .compact
            ) {
                onTogglePause()
                controlsVisible = true
            }

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
                .foregroundStyle(skin.colors.foreground)
                .lineLimit(1)

            if uiState.timeline.kind == .liveDvr {
                StagePillButton(label: liveButtonLabel(uiState.timeline), compact: true) {
                    onSeekToLiveEdge()
                    controlsVisible = true
                }
            }

            StageIconButton(
                icon: fullscreenIcon,
                label: fullscreenLabel,
                variant: .compactFullscreen
            ) {
                onToggleFullscreen()
                controlsVisible = true
            }
        }
        .padding(.horizontal, 18)
        .padding(.bottom, 18)
    }

    var expandedControls: some View {
        VStack(alignment: .leading, spacing: 4) {
            Text(timelineSummary(uiState.timeline, pendingSeekRatio: pendingSeekRatio))
                .font(.caption.monospacedDigit().weight(.semibold))
                .foregroundStyle(skin.colors.foreground)
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

            HStack(alignment: .center, spacing: skin.metrics.buttonSpacing) {
                StageIconButton(
                    icon: playButtonIcon,
                label: playButtonLabel,
                    variant: .expanded
                ) {
                    onTogglePause()
                    controlsVisible = true
                }

                if let expandedControlBarLeading {
                    expandedControlBarLeading
                }

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
                    icon: fullscreenIcon,
                label: fullscreenLabel,
                    variant: .expandedFullscreen
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
