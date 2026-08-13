package io.github.umbrella22.vesper.player.android.compose.ui

import androidx.compose.animation.AnimatedVisibility
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.rounded.Fullscreen
import androidx.compose.material.icons.rounded.FullscreenExit
import androidx.compose.material.icons.rounded.MoreVert
import androidx.compose.material.icons.rounded.Pause
import androidx.compose.material.icons.rounded.PlayArrow
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import io.github.umbrella22.vesper.player.android.PlaybackStateUi
import io.github.umbrella22.vesper.player.android.PlayerHostUiState
import io.github.umbrella22.vesper.player.android.TimelineKind

@Composable
internal fun StageControlsOverlay(
    controlsVisible: Boolean,
    uiState: PlayerHostUiState,
    isPortrait: Boolean,
    isPlaying: Boolean,
    displayedRatio: Float,
    pendingSeekRatio: Float?,
    speedLabel: String,
    qualityLabel: String,
    playbackRateControlsEnabled: Boolean,
    onOpenSheet: (VesperPlayerStageSheet) -> Unit,
    onTogglePlayback: () -> Unit,
    onControlsVisibilityChange: (Boolean) -> Unit,
    onPendingSeekRatioChange: (Float?) -> Unit,
    onSeekToRatio: (Float) -> Unit,
    onSeekToLiveEdge: () -> Unit,
    onToggleFullscreen: () -> Unit,
) {
    AnimatedVisibility(
        visible = controlsVisible || uiState.playbackState != PlaybackStateUi.Playing,
        enter = fadeIn(),
        exit = fadeOut(),
    ) {
        Box(
            modifier = Modifier
                .fillMaxSize()
                .background(
                    brush = Brush.verticalGradient(
                        colors = listOf(
                            Color.Black.copy(alpha = 0.68f),
                            Color.Transparent,
                            Color.Transparent,
                            Color.Black.copy(alpha = 0.82f),
                        ),
                    ),
                ),
        ) {
            Row(
                modifier = Modifier
                    .fillMaxWidth()
                    .padding(horizontal = 18.dp, vertical = 16.dp),
                horizontalArrangement = Arrangement.SpaceBetween,
                verticalAlignment = Alignment.Top,
            ) {
                Column(
                    modifier = Modifier.weight(1f),
                    verticalArrangement = Arrangement.spacedBy(4.dp),
                ) {
                    Row(
                        modifier = Modifier.fillMaxWidth(),
                        horizontalArrangement = Arrangement.spacedBy(8.dp),
                        verticalAlignment = Alignment.CenterVertically,
                    ) {
                        Text(
                            text = uiState.sourceLabel,
                            modifier = Modifier.weight(1f),
                            color = Color.White,
                            style = MaterialTheme.typography.titleMedium.copy(fontWeight = FontWeight.Bold),
                            maxLines = 1,
                            overflow = TextOverflow.Ellipsis,
                        )
                        if (uiState.isBuffering) {
                            StageChip(
                                label = stringResource(R.string.vesper_player_stage_buffering),
                                accent = Color(0xFFFFB454),
                                compact = true,
                            )
                        }
                    }
                    Text(
                        text = stageBadgeText(uiState.timeline),
                        color = Color(0xFFBFC6D6),
                        style = MaterialTheme.typography.bodySmall,
                    )
                }

                StageIconButton(
                    icon = Icons.Rounded.MoreVert,
                    label = stringResource(R.string.vesper_player_stage_more),
                    size = 38.dp,
                    iconSize = 24.dp,
                    containerAlpha = 0f,
                    onClick = { onOpenSheet(VesperPlayerStageSheet.Menu) },
                )
            }

            if (isPortrait) {
                Row(
                    modifier = Modifier
                        .align(Alignment.BottomStart)
                        .fillMaxWidth()
                        .padding(horizontal = 18.dp, vertical = 18.dp),
                    horizontalArrangement = Arrangement.spacedBy(8.dp),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    StageIconButton(
                        icon = if (isPlaying) Icons.Rounded.Pause else Icons.Rounded.PlayArrow,
                        label =
                            if (isPlaying) {
                                stringResource(R.string.vesper_player_stage_pause)
                            } else {
                                stringResource(R.string.vesper_player_stage_play)
                            },
                        size = 38.dp,
                        iconSize = 24.dp,
                        containerAlpha = 0f,
                        onClick = {
                            onTogglePlayback()
                            onControlsVisibilityChange(true)
                        },
                    )
                    TimelineScrubber(
                        modifier = Modifier.weight(1f),
                        displayedRatio = displayedRatio,
                        compact = true,
                        enabled = uiState.timeline.isSeekable,
                        onSeekPreview = { ratio ->
                            onPendingSeekRatioChange(ratio)
                            onControlsVisibilityChange(true)
                        },
                        onSeekCommit = { ratio ->
                            onSeekToRatio(ratio)
                            onPendingSeekRatioChange(null)
                            onControlsVisibilityChange(true)
                        },
                        onSeekCancel = {
                            onPendingSeekRatioChange(null)
                        },
                    )
                    Text(
                        text = compactTimelineSummary(uiState.timeline, pendingSeekRatio),
                        color = Color(0xFFF7F8FC),
                        style = MaterialTheme.typography.labelSmall,
                        maxLines = 1,
                        overflow = TextOverflow.Ellipsis,
                    )
                    if (uiState.timeline.kind == TimelineKind.LiveDvr) {
                        StagePillButton(
                            label = liveButtonLabel(uiState.timeline),
                            compact = true,
                            onClick = {
                                onSeekToLiveEdge()
                                onControlsVisibilityChange(true)
                            },
                        )
                    }
                    StageIconButton(
                        icon = Icons.Rounded.Fullscreen,
                        label = stringResource(R.string.vesper_player_stage_fullscreen),
                        size = 38.dp,
                        iconSize = 24.dp,
                        containerAlpha = 0f,
                        onClick = onToggleFullscreen,
                    )
                }
            } else {
                Column(
                    modifier = Modifier
                        .align(Alignment.BottomStart)
                        .fillMaxWidth()
                        .padding(horizontal = 12.dp, vertical = 8.dp),
                    verticalArrangement = Arrangement.spacedBy(4.dp),
                ) {
                    Text(
                        text = timelineSummary(uiState.timeline, pendingSeekRatio),
                        color = Color(0xFFF7F8FC),
                        style = MaterialTheme.typography.labelLarge,
                        maxLines = 1,
                        overflow = TextOverflow.Ellipsis,
                    )
                    TimelineScrubber(
                        displayedRatio = displayedRatio,
                        compact = true,
                        enabled = uiState.timeline.isSeekable,
                        onSeekPreview = { ratio ->
                            onPendingSeekRatioChange(ratio)
                            onControlsVisibilityChange(true)
                        },
                        onSeekCommit = { ratio ->
                            onSeekToRatio(ratio)
                            onPendingSeekRatioChange(null)
                            onControlsVisibilityChange(true)
                        },
                        onSeekCancel = {
                            onPendingSeekRatioChange(null)
                        },
                    )

                    Row(
                        modifier = Modifier.fillMaxWidth(),
                        horizontalArrangement = Arrangement.SpaceBetween,
                        verticalAlignment = Alignment.CenterVertically,
                    ) {
                        StageIconButton(
                            icon = if (isPlaying) Icons.Rounded.Pause else Icons.Rounded.PlayArrow,
                            label =
                                if (isPlaying) {
                                    stringResource(R.string.vesper_player_stage_pause)
                                } else {
                                    stringResource(R.string.vesper_player_stage_play)
                                },
                            size = 38.dp,
                            iconSize = 22.dp,
                            containerAlpha = 0f,
                            onClick = {
                                onTogglePlayback()
                                onControlsVisibilityChange(true)
                            },
                        )
                        Row(
                            horizontalArrangement = Arrangement.spacedBy(8.dp),
                            verticalAlignment = Alignment.CenterVertically,
                        ) {
                            if (uiState.timeline.kind == TimelineKind.LiveDvr) {
                                StagePillButton(
                                    label = liveButtonLabel(uiState.timeline),
                                    compact = true,
                                    onClick = {
                                        onSeekToLiveEdge()
                                        onControlsVisibilityChange(true)
                                    },
                                )
                            }
                            if (playbackRateControlsEnabled) {
                                StagePillButton(
                                    label = speedLabel,
                                    compact = true,
                                    onClick = {
                                        onOpenSheet(VesperPlayerStageSheet.Speed)
                                    },
                                )
                            }
                            StagePillButton(
                                label = qualityLabel,
                                compact = true,
                                onClick = {
                                    onOpenSheet(VesperPlayerStageSheet.Quality)
                                },
                            )
                            StageIconButton(
                                icon = Icons.Rounded.FullscreenExit,
                                label = stringResource(R.string.vesper_player_stage_exit_fullscreen),
                                size = 34.dp,
                                iconSize = 19.dp,
                                containerAlpha = 0f,
                                onClick = onToggleFullscreen,
                            )
                        }
                    }
                }
            }
        }
    }
}
