import SwiftUI
import VesperPlayerKit

extension VesperPlayerStage {
    var isPlaying: Bool {
        uiState.playbackState == .playing
    }

    var playButtonSymbol: String {
        isPlaying ? "pause.fill" : "play.fill"
    }

    var playButtonLabel: String {
        isPlaying ? VesperPlayerStageStrings.pause : VesperPlayerStageStrings.play
    }

    var qualityPillLabel: String {
        qualityButtonLabel(
            trackCatalog,
            trackSelection,
            effectiveVideoTrackId: effectiveVideoTrackId,
            fixedTrackStatus: fixedTrackStatus
        )
    }

    var portraitControls: some View {
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

    var landscapeControls: some View {
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
