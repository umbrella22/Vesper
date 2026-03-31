package io.github.ikaros.vesper.example.androidcomposehost

import androidx.compose.animation.AnimatedVisibility
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.gestures.detectHorizontalDragGestures
import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.offset
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.rounded.ClosedCaption
import androidx.compose.material.icons.rounded.Forward10
import androidx.compose.material.icons.rounded.Fullscreen
import androidx.compose.material.icons.rounded.FullscreenExit
import androidx.compose.material.icons.rounded.GraphicEq
import androidx.compose.material.icons.rounded.MoreVert
import androidx.compose.material.icons.rounded.Pause
import androidx.compose.material.icons.rounded.PlayArrow
import androidx.compose.material.icons.rounded.Replay10
import androidx.compose.material.icons.rounded.Speed
import androidx.compose.material.icons.rounded.Tune
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableFloatStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.layout.onSizeChanged
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.IntOffset
import androidx.compose.ui.unit.dp
import io.github.ikaros.vesper.player.android.PlaybackStateUi
import io.github.ikaros.vesper.player.android.PlayerHostUiState
import io.github.ikaros.vesper.player.android.TimelineKind
import io.github.ikaros.vesper.player.android.VesperPlayerController
import io.github.ikaros.vesper.player.android.compose.VesperPlayerSurface
import kotlin.math.roundToInt

@Composable
internal fun ExamplePlayerStage(
    controller: VesperPlayerController,
    uiState: PlayerHostUiState,
    controlsVisible: Boolean,
    pendingSeekRatio: Float?,
    isPortrait: Boolean,
    modifier: Modifier = Modifier,
    onControlsVisibilityChange: (Boolean) -> Unit,
    onPendingSeekRatioChange: (Float?) -> Unit,
    onOpenSheet: (ExamplePlayerSheet) -> Unit,
    onToggleFullscreen: () -> Unit,
) {
    val currentRatio = uiState.timeline.displayedRatio ?: 0f
    val shape = RoundedCornerShape(if (isPortrait) 20.dp else 0.dp)

    Box(
        modifier = modifier
            .clip(shape)
            .background(
                color = Color(0xFF000000),
                shape = shape,
            ),
    ) {
        if (isPortrait) {
            Box(
                modifier = Modifier
                    .fillMaxSize()
                    .border(
                        width = 1.dp,
                        color = Color.White.copy(alpha = 0.08f),
                        shape = shape,
                    ),
            )
        }

        VesperPlayerSurface(
            controller = controller,
            modifier = Modifier.fillMaxSize(),
            cornerRadiusDp = if (isPortrait) 20.dp else 0.dp,
            manageControllerLifecycle = false,
        )

        Box(
            modifier = Modifier
                .fillMaxSize()
                .pointerInput(uiState.playbackState, controlsVisible) {
                    detectTapGestures(
                        onTap = {
                            onControlsVisibilityChange(!controlsVisible)
                        },
                        onDoubleTap = { offset ->
                            if (offset.x < size.width / 2f) {
                                controller.seekBy(-10_000L)
                            } else {
                                controller.seekBy(10_000L)
                            }
                            onControlsVisibilityChange(true)
                        },
                    )
                },
        )

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
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    Column(
                        modifier = Modifier.weight(1f),
                        verticalArrangement = Arrangement.spacedBy(4.dp),
                    ) {
                        Text(
                            text = uiState.sourceLabel,
                            color = Color.White,
                            style = MaterialTheme.typography.titleMedium.copy(fontWeight = FontWeight.Bold),
                            maxLines = 1,
                            overflow = TextOverflow.Ellipsis,
                        )
                        Text(
                            text = stageBadgeText(uiState.timeline),
                            color = Color(0xFFBFC6D6),
                            style = MaterialTheme.typography.bodySmall,
                        )
                    }

                    if (isPortrait) {
                        StageIconButton(
                            icon = Icons.Rounded.MoreVert,
                            label = "More",
                            size = 38.dp,
                            iconSize = 24.dp,
                            containerAlpha = 0f,
                            onClick = { onOpenSheet(ExamplePlayerSheet.Menu) },
                        )
                    } else {
                        Row(
                            horizontalArrangement = Arrangement.spacedBy(10.dp),
                            verticalAlignment = Alignment.CenterVertically,
                        ) {
                            StageIconButton(
                                icon = Icons.Rounded.Tune,
                                label = "Quality",
                                containerAlpha = 0f,
                                onClick = { onOpenSheet(ExamplePlayerSheet.Quality) },
                            )
                            StageIconButton(
                                icon = Icons.Rounded.GraphicEq,
                                label = "Audio",
                                containerAlpha = 0f,
                                onClick = { onOpenSheet(ExamplePlayerSheet.Audio) },
                            )
                            StageIconButton(
                                icon = Icons.Rounded.ClosedCaption,
                                label = "Subtitles",
                                containerAlpha = 0f,
                                onClick = { onOpenSheet(ExamplePlayerSheet.Subtitle) },
                            )
                            StageIconButton(
                                icon = Icons.Rounded.Speed,
                                label = "Speed",
                                containerAlpha = 0f,
                                onClick = { onOpenSheet(ExamplePlayerSheet.Speed) },
                            )
                        }
                    }
                }

                Row(
                    modifier = Modifier
                        .align(Alignment.Center)
                        .padding(horizontal = 18.dp),
                    horizontalArrangement = Arrangement.spacedBy(16.dp),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    StageIconButton(
                        icon = Icons.Rounded.Replay10,
                        label = "Replay 10",
                        size = if (isPortrait) 52.dp else 44.dp,
                        iconSize = if (isPortrait) 24.dp else 20.dp,
                        onClick = {
                            controller.seekBy(-10_000L)
                            onControlsVisibilityChange(true)
                        },
                    )
                    StagePrimaryPlayButton(
                        isPlaying = uiState.playbackState == PlaybackStateUi.Playing,
                        size = if (isPortrait) 72.dp else 60.dp,
                        iconSize = if (isPortrait) 36.dp else 28.dp,
                        onClick = {
                            controller.togglePause()
                            onControlsVisibilityChange(true)
                        },
                    )
                    StageIconButton(
                        icon = Icons.Rounded.Forward10,
                        label = "Forward 10",
                        size = if (isPortrait) 52.dp else 44.dp,
                        iconSize = if (isPortrait) 24.dp else 20.dp,
                        onClick = {
                            controller.seekBy(10_000L)
                            onControlsVisibilityChange(true)
                        },
                    )
                }

                Column(
                    modifier = Modifier
                        .align(Alignment.BottomStart)
                        .fillMaxWidth()
                        .padding(
                            horizontal = if (isPortrait) 18.dp else 12.dp,
                            vertical = if (isPortrait) 18.dp else 8.dp,
                        ),
                    verticalArrangement = Arrangement.spacedBy(if (isPortrait) 2.dp else 0.dp),
                ) {
                    TimelineScrubber(
                        modifier = if (isPortrait) Modifier else Modifier.padding(top = 3.dp),
                        displayedRatio = pendingSeekRatio ?: currentRatio,
                        compact = !isPortrait,
                        onSeekPreview = { ratio ->
                            onPendingSeekRatioChange(ratio)
                            onControlsVisibilityChange(true)
                        },
                        onSeekCommit = { ratio ->
                            controller.seekToRatio(ratio)
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
                        Text(
                            text = timelineSummary(uiState.timeline, pendingSeekRatio),
                            color = Color(0xFFF7F8FC),
                            style = MaterialTheme.typography.labelLarge,
                        )

                        Row(
                            horizontalArrangement = Arrangement.spacedBy(if (isPortrait) 8.dp else 6.dp),
                            verticalAlignment = Alignment.CenterVertically,
                        ) {
                            if (uiState.timeline.kind == TimelineKind.LiveDvr) {
                                StagePillButton(
                                    label = liveButtonLabel(uiState.timeline),
                                    onClick = {
                                        controller.seekToLiveEdge()
                                        onControlsVisibilityChange(true)
                                    },
                                )
                            }
                            StageIconButton(
                                icon = if (isPortrait) Icons.Rounded.Fullscreen else Icons.Rounded.FullscreenExit,
                                label = if (isPortrait) "Fullscreen" else "Exit Fullscreen",
                                size = if (isPortrait) 38.dp else 32.dp,
                                iconSize = if (isPortrait) 24.dp else 18.dp,
                                containerAlpha = 0f,
                                onClick = onToggleFullscreen,
                            )
                        }
                    }
                }
            }
        }

        if (uiState.isBuffering) {
            StageChip(
                label = "Buffering",
                accent = Color(0xFFFFB454),
                modifier = Modifier
                    .align(Alignment.BottomEnd)
                    .padding(18.dp),
            )
        }
    }
}

@Composable
internal fun TimelineScrubber(
    modifier: Modifier = Modifier,
    displayedRatio: Float,
    compact: Boolean = false,
    onSeekPreview: (Float) -> Unit,
    onSeekCommit: (Float) -> Unit,
    onSeekCancel: () -> Unit,
) {
    var widthPx by remember { mutableFloatStateOf(1f) }
    val knobDiameter = if (compact) 11.dp else 14.dp
    val knobRadiusPx =
        with(androidx.compose.ui.platform.LocalDensity.current) { (knobDiameter / 2).toPx() }
    val touchHeight = if (compact) 22.dp else 28.dp
    val visualHeight = if (compact) 14.dp else 18.dp
    val trackHeight = 4.dp
    val ratio = displayedRatio.coerceIn(0f, 1f)

    Box(
        modifier = modifier
            .fillMaxWidth()
            .height(touchHeight)
            .onSizeChanged { widthPx = it.width.toFloat().coerceAtLeast(1f) }
            .pointerInput(widthPx) {
                detectTapGestures { offset ->
                    val targetRatio = (offset.x / widthPx).coerceIn(0f, 1f)
                    onSeekPreview(targetRatio)
                    onSeekCommit(targetRatio)
                }
            }
            .pointerInput(widthPx) {
                var dragRatio = ratio
                detectHorizontalDragGestures(
                    onDragStart = { offset ->
                        dragRatio = (offset.x / widthPx).coerceIn(0f, 1f)
                        onSeekPreview(dragRatio)
                    },
                    onHorizontalDrag = { change, _ ->
                        dragRatio = (change.position.x / widthPx).coerceIn(0f, 1f)
                        onSeekPreview(dragRatio)
                    },
                    onDragCancel = onSeekCancel,
                    onDragEnd = {
                        onSeekCommit(dragRatio)
                    },
                )
            },
    ) {
        Box(
            modifier = Modifier
                .align(Alignment.BottomStart)
                .fillMaxWidth()
                .height(visualHeight),
        ) {
            Box(
                modifier = Modifier
                    .align(Alignment.CenterStart)
                    .fillMaxWidth()
                    .height(trackHeight)
                    .background(Color.White.copy(alpha = 0.16f), RoundedCornerShape(999.dp)),
            )
            Box(
                modifier = Modifier
                    .align(Alignment.CenterStart)
                    .fillMaxWidth(ratio)
                    .height(trackHeight)
                    .background(
                        Brush.horizontalGradient(
                            colors = listOf(Color(0xFFFF6B8E), Color(0xFFFFB454)),
                        ),
                        RoundedCornerShape(999.dp),
                    ),
            )
            Box(
                modifier = Modifier
                    .align(Alignment.CenterStart)
                    .offset {
                        IntOffset(
                            x = ((widthPx - knobRadiusPx * 2f) * ratio).roundToInt(),
                            y = 0,
                        )
                    }
                    .size(knobDiameter)
                    .background(Color.White, CircleShape),
            )
        }
    }
}

@Composable
internal fun StagePrimaryPlayButton(
    isPlaying: Boolean,
    size: Dp = 72.dp,
    iconSize: Dp = 36.dp,
    onClick: () -> Unit,
) {
    Surface(
        onClick = onClick,
        modifier = Modifier.size(size),
        shape = CircleShape,
        color = Color.White.copy(alpha = 0.14f),
        contentColor = Color.White,
    ) {
        Box(contentAlignment = Alignment.Center) {
            Icon(
                imageVector = if (isPlaying) Icons.Rounded.Pause else Icons.Rounded.PlayArrow,
                contentDescription = if (isPlaying) "Pause" else "Play",
                modifier = Modifier.size(iconSize),
            )
        }
    }
}

@Composable
internal fun StageIconButton(
    icon: ImageVector,
    label: String,
    size: Dp = 52.dp,
    iconSize: Dp = 24.dp,
    containerAlpha: Float = 0.10f,
    onClick: () -> Unit,
) {
    Surface(
        onClick = onClick,
        modifier = Modifier.size(size),
        shape = CircleShape,
        color = Color.White.copy(alpha = containerAlpha),
        contentColor = Color.White,
    ) {
        Box(contentAlignment = Alignment.Center) {
            Icon(
                imageVector = icon,
                contentDescription = label,
                modifier = Modifier.size(iconSize),
            )
        }
    }
}

@Composable
internal fun StagePillButton(
    label: String,
    icon: ImageVector? = null,
    onClick: () -> Unit,
) {
    TextButton(
        onClick = onClick,
        colors = ButtonDefaults.textButtonColors(contentColor = Color.White),
        contentPadding = PaddingValues(horizontal = 12.dp, vertical = 8.dp),
        modifier = Modifier
            .heightIn(min = 32.dp)
            .background(Color.White.copy(alpha = 0.10f), RoundedCornerShape(999.dp)),
    ) {
        if (icon != null) {
            Icon(
                imageVector = icon,
                contentDescription = null,
                modifier = Modifier.size(16.dp),
            )
            Spacer(modifier = Modifier.width(6.dp))
        }
        Text(
            text = label,
            maxLines = 1,
            overflow = TextOverflow.Ellipsis,
        )
    }
}

@Composable
internal fun StageChip(
    label: String,
    accent: Color,
    modifier: Modifier = Modifier,
) {
    Row(
        modifier = modifier
            .background(Color.Black.copy(alpha = 0.36f), RoundedCornerShape(999.dp))
            .border(1.dp, Color.White.copy(alpha = 0.08f), RoundedCornerShape(999.dp))
            .padding(horizontal = 10.dp, vertical = 7.dp),
        horizontalArrangement = Arrangement.spacedBy(8.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Box(
            modifier = Modifier
                .size(8.dp)
                .background(accent, CircleShape),
        )
        Text(
            text = label,
            color = Color.White,
            style = MaterialTheme.typography.labelMedium,
        )
    }
}
