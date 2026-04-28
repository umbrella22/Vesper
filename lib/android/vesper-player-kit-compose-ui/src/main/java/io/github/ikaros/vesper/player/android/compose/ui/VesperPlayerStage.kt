package io.github.ikaros.vesper.player.android.compose.ui

import androidx.compose.animation.AnimatedVisibility
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.gestures.detectDragGestures
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
import androidx.compose.material.icons.automirrored.rounded.VolumeUp
import androidx.compose.material.icons.rounded.WbSunny
import androidx.compose.material.icons.rounded.Fullscreen
import androidx.compose.material.icons.rounded.FullscreenExit
import androidx.compose.material.icons.rounded.MoreVert
import androidx.compose.material.icons.rounded.Pause
import androidx.compose.material.icons.rounded.PlayArrow
import androidx.compose.material.icons.rounded.Speed
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableFloatStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.layout.onSizeChanged
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.IntOffset
import androidx.compose.ui.unit.dp
import io.github.ikaros.vesper.player.android.PlaybackStateUi
import io.github.ikaros.vesper.player.android.PlayerHostUiState
import io.github.ikaros.vesper.player.android.TimelineKind
import io.github.ikaros.vesper.player.android.VesperPlayerController
import io.github.ikaros.vesper.player.android.VesperTrackCatalog
import io.github.ikaros.vesper.player.android.VesperTrackSelectionSnapshot
import io.github.ikaros.vesper.player.android.compose.VesperPlayerSurface
import kotlinx.coroutines.delay
import kotlin.math.abs
import kotlin.math.roundToInt

@Composable
fun VesperPlayerStage(
    controller: VesperPlayerController,
    uiState: PlayerHostUiState,
    controlsVisible: Boolean,
    pendingSeekRatio: Float?,
    isPortrait: Boolean,
    trackCatalog: VesperTrackCatalog = VesperTrackCatalog.Empty,
    trackSelection: VesperTrackSelectionSnapshot = VesperTrackSelectionSnapshot(),
    modifier: Modifier = Modifier,
    onControlsVisibilityChange: (Boolean) -> Unit,
    onPendingSeekRatioChange: (Float?) -> Unit,
    onOpenSheet: (VesperPlayerStageSheet) -> Unit,
    onToggleFullscreen: () -> Unit,
    currentBrightnessRatio: () -> Float? = { null },
    onSetBrightnessRatio: (Float) -> Float? = { null },
    currentVolumeRatio: () -> Float? = { null },
    onSetVolumeRatio: (Float) -> Float? = { null },
) {
    val currentRatio = uiState.timeline.displayedRatio ?: 0f
    val displayedRatio = pendingSeekRatio ?: currentRatio
    val shape = RoundedCornerShape(if (isPortrait) 20.dp else 0.dp)
    val isPlaying = uiState.playbackState == PlaybackStateUi.Playing
    val speedLabel = speedBadge(uiState.playbackRate)
    val temporarySpeedLabel = speedBadge(2f)
    val qualityLabel = qualityButtonLabel(trackCatalog, trackSelection)
    val latestControlsVisible by rememberUpdatedState(controlsVisible)
    val latestPlaybackRate by rememberUpdatedState(uiState.playbackRate)
    var gestureFeedback by remember { mutableStateOf<StageGestureFeedback?>(null) }
    var speedGestureRestoreRate by remember { mutableStateOf<Float?>(null) }

    fun endTemporarySpeedGesture() {
        val restoreRate = speedGestureRestoreRate ?: return
        speedGestureRestoreRate = null
        controller.setPlaybackRate(restoreRate)
    }

    LaunchedEffect(gestureFeedback) {
        if (gestureFeedback == null) {
            return@LaunchedEffect
        }
        delay(520)
        gestureFeedback = null
    }

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
                .pointerInput(controller) {
                    detectTapGestures(
                        onTap = {
                            onControlsVisibilityChange(!latestControlsVisible)
                        },
                        onDoubleTap = { _ ->
                            controller.togglePause()
                            onControlsVisibilityChange(true)
                        },
                        onLongPress = {
                            if (speedGestureRestoreRate == null) {
                                speedGestureRestoreRate = latestPlaybackRate
                                controller.setPlaybackRate(2f)
                            }
                            gestureFeedback =
                                StageGestureFeedback(
                                    kind = StageGestureKind.Speed,
                                    progress = null,
                                    label = temporarySpeedLabel,
                                )
                            onControlsVisibilityChange(true)
                        },
                        onPress = {
                            try {
                                tryAwaitRelease()
                            } finally {
                                endTemporarySpeedGesture()
                            }
                        },
                    )
                }
                .pointerInput(currentBrightnessRatio, currentVolumeRatio, uiState.timeline.isSeekable) {
                    var gestureKind: StageAreaGestureKind? = null
                    var deviceGestureStartRatio = 0f
                    var seekGestureRatio = 0f
                    var dragStartX = 0f
                    var totalDragX = 0f
                    var totalDragY = 0f

                    fun resetGesture() {
                        gestureKind = null
                        deviceGestureStartRatio = 0f
                        seekGestureRatio = 0f
                        dragStartX = 0f
                        totalDragX = 0f
                        totalDragY = 0f
                    }

                    detectDragGestures(
                        onDragStart = { offset ->
                            resetGesture()
                            dragStartX = offset.x
                        },
                        onDrag = { change, dragAmount ->
                            if (speedGestureRestoreRate != null) {
                                return@detectDragGestures
                            }
                            totalDragX += dragAmount.x
                            totalDragY += dragAmount.y
                            if (gestureKind == null) {
                                val horizontalDistance = abs(totalDragX)
                                val verticalDistance = abs(totalDragY)
                                if (verticalDistance < 8f && horizontalDistance < 8f) {
                                    return@detectDragGestures
                                }

                                if (horizontalDistance >= verticalDistance * 1.15f) {
                                    if (!uiState.timeline.isSeekable) {
                                        gestureKind = StageAreaGestureKind.Ignored
                                        return@detectDragGestures
                                    }
                                    gestureKind = StageAreaGestureKind.Seek
                                } else if (verticalDistance >= horizontalDistance * 1.15f) {
                                    val nextKind =
                                        if (dragStartX < size.width / 2f) {
                                            StageAreaGestureKind.Brightness
                                        } else {
                                            StageAreaGestureKind.Volume
                                        }
                                    val startRatio =
                                        when (nextKind) {
                                            StageAreaGestureKind.Brightness -> currentBrightnessRatio()
                                            StageAreaGestureKind.Volume -> currentVolumeRatio()
                                            StageAreaGestureKind.Seek,
                                            StageAreaGestureKind.Ignored,
                                            -> null
                                        }
                                    if (startRatio == null) {
                                        gestureKind = StageAreaGestureKind.Ignored
                                        return@detectDragGestures
                                    }
                                    gestureKind = nextKind
                                    deviceGestureStartRatio = startRatio.coerceIn(0f, 1f)
                                } else {
                                    return@detectDragGestures
                                }
                            }

                            val kind = gestureKind ?: return@detectDragGestures
                            if (kind == StageAreaGestureKind.Ignored) {
                                return@detectDragGestures
                            }
                            if (kind == StageAreaGestureKind.Seek) {
                                val stageWidth = size.width.toFloat().coerceAtLeast(1f)
                                seekGestureRatio = (change.position.x / stageWidth).coerceIn(0f, 1f)
                                onPendingSeekRatioChange(seekGestureRatio)
                                onControlsVisibilityChange(true)
                                change.consume()
                                return@detectDragGestures
                            }

                            val stageHeight = size.height.toFloat().coerceAtLeast(1f)
                            val requestedRatio =
                                (deviceGestureStartRatio - totalDragY / stageHeight * 1.15f)
                                    .coerceIn(0f, 1f)
                            val actualRatio =
                                when (kind) {
                                    StageAreaGestureKind.Brightness -> onSetBrightnessRatio(requestedRatio)
                                    StageAreaGestureKind.Volume -> onSetVolumeRatio(requestedRatio)
                                    StageAreaGestureKind.Seek,
                                    StageAreaGestureKind.Ignored,
                                    -> null
                                }?.coerceIn(0f, 1f)
                            if (actualRatio != null) {
                                val feedbackKind =
                                    when (kind) {
                                        StageAreaGestureKind.Brightness -> StageGestureKind.Brightness
                                        StageAreaGestureKind.Volume -> StageGestureKind.Volume
                                        StageAreaGestureKind.Seek,
                                        StageAreaGestureKind.Ignored,
                                        -> null
                                    }
                                if (feedbackKind != null) {
                                    val value = actualRatio.coerceIn(0f, 1f)
                                    gestureFeedback =
                                        StageGestureFeedback(
                                            kind = feedbackKind,
                                            progress = value,
                                            label = percentLabel(value),
                                        )
                                }
                                onControlsVisibilityChange(true)
                                change.consume()
                            }
                        },
                        onDragEnd = {
                            if (gestureKind == StageAreaGestureKind.Seek) {
                                controller.seekToRatio(seekGestureRatio)
                                onPendingSeekRatioChange(null)
                                onControlsVisibilityChange(true)
                            }
                            resetGesture()
                        },
                        onDragCancel = {
                            if (gestureKind == StageAreaGestureKind.Seek) {
                                onPendingSeekRatioChange(null)
                            }
                            resetGesture()
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
                                controller.togglePause()
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
                                controller.seekToRatio(ratio)
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
                                    controller.seekToLiveEdge()
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
                                    controller.togglePause()
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
                                            controller.seekToLiveEdge()
                                            onControlsVisibilityChange(true)
                                        },
                                    )
                                }
                                StagePillButton(
                                    label = speedLabel,
                                    compact = true,
                                    onClick = {
                                        onOpenSheet(VesperPlayerStageSheet.Speed)
                                    },
                                )
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

        AnimatedVisibility(
            visible = gestureFeedback != null,
            enter = fadeIn(),
            exit = fadeOut(),
            modifier = Modifier.align(Alignment.Center),
        ) {
            gestureFeedback?.let { feedback ->
                StageGestureFeedbackPanel(feedback = feedback)
            }
        }
    }
}

private enum class StageAreaGestureKind {
    Brightness,
    Volume,
    Seek,
    Ignored,
}

private enum class StageGestureKind {
    Brightness,
    Volume,
    Speed,
}

private data class StageGestureFeedback(
    val kind: StageGestureKind,
    val progress: Float?,
    val label: String,
)

@Composable
private fun StageGestureFeedbackPanel(feedback: StageGestureFeedback) {
    val icon =
        when (feedback.kind) {
            StageGestureKind.Brightness -> Icons.Rounded.WbSunny
            StageGestureKind.Volume -> Icons.AutoMirrored.Rounded.VolumeUp
            StageGestureKind.Speed -> Icons.Rounded.Speed
        }

    Surface(
        shape = RoundedCornerShape(999.dp),
        color = Color.Black.copy(alpha = 0.72f),
        contentColor = Color.White,
    ) {
        Row(
            modifier = Modifier
                .then(if (feedback.progress == null) Modifier else Modifier.width(226.dp))
                .padding(horizontal = 14.dp, vertical = 10.dp),
            horizontalArrangement = Arrangement.spacedBy(10.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Icon(
                imageVector = icon,
                contentDescription = null,
                modifier = Modifier.size(24.dp),
            )
            feedback.progress?.let { progress ->
                Box(
                    modifier = Modifier
                        .weight(1f)
                        .height(4.dp)
                        .background(Color.White.copy(alpha = 0.18f), RoundedCornerShape(999.dp)),
                ) {
                    Box(
                        modifier = Modifier
                            .fillMaxWidth(progress.coerceIn(0f, 1f))
                            .height(4.dp)
                            .background(Color.White, RoundedCornerShape(999.dp)),
                    )
                }
            }
            Text(
                text = feedback.label,
                style = MaterialTheme.typography.labelMedium,
                color = Color.White,
            )
        }
    }
}

private fun percentLabel(value: Float): String = "${(value * 100f).roundToInt()}%"

@Composable
internal fun TimelineScrubber(
    modifier: Modifier = Modifier,
    displayedRatio: Float,
    compact: Boolean = false,
    enabled: Boolean = true,
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
    val inactiveTrackColor = Color.White.copy(alpha = if (enabled) 0.16f else 0.10f)
    val activeStart = Color(0xFFFF6B8E).copy(alpha = if (enabled) 1f else 0.42f)
    val activeEnd = Color(0xFFFFB454).copy(alpha = if (enabled) 1f else 0.42f)
    val knobColor = Color.White.copy(alpha = if (enabled) 1f else 0.42f)

    var scrubberModifier =
        modifier
            .fillMaxWidth()
            .height(touchHeight)
            .onSizeChanged { widthPx = it.width.toFloat().coerceAtLeast(1f) }
    if (enabled) {
        scrubberModifier =
            scrubberModifier
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
                }
    }

    Box(
        modifier = scrubberModifier,
    ) {
        Box(
            modifier = Modifier
                .align(Alignment.CenterStart)
                .fillMaxWidth()
                .height(visualHeight),
        ) {
            Box(
                modifier = Modifier
                    .align(Alignment.CenterStart)
                    .fillMaxWidth()
                    .height(trackHeight)
                    .background(inactiveTrackColor, RoundedCornerShape(999.dp)),
            )
            Box(
                modifier = Modifier
                    .align(Alignment.CenterStart)
                    .fillMaxWidth(ratio)
                    .height(trackHeight)
                    .background(
                        Brush.horizontalGradient(
                            colors = listOf(activeStart, activeEnd),
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
                    .background(knobColor, CircleShape),
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
                contentDescription =
                    if (isPlaying) {
                        stringResource(R.string.vesper_player_stage_pause)
                    } else {
                        stringResource(R.string.vesper_player_stage_play)
                    },
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
    compact: Boolean = false,
    onClick: () -> Unit,
) {
    TextButton(
        onClick = onClick,
        colors = ButtonDefaults.textButtonColors(contentColor = Color.White),
        contentPadding =
            PaddingValues(
                horizontal = if (compact) 10.dp else 12.dp,
                vertical = if (compact) 6.dp else 8.dp,
            ),
        modifier = Modifier
            .heightIn(min = if (compact) 30.dp else 32.dp)
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
    compact: Boolean = false,
) {
    val dotSize = if (compact) 6.dp else 8.dp
    val horizontalPadding = if (compact) 8.dp else 10.dp
    val verticalPadding = if (compact) 5.dp else 7.dp
    val spacing = if (compact) 6.dp else 8.dp
    Row(
        modifier = modifier
            .background(Color.Black.copy(alpha = 0.36f), RoundedCornerShape(999.dp))
            .border(1.dp, Color.White.copy(alpha = 0.08f), RoundedCornerShape(999.dp))
            .padding(horizontal = horizontalPadding, vertical = verticalPadding),
        horizontalArrangement = Arrangement.spacedBy(spacing),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Box(
            modifier = Modifier
                .size(dotSize)
                .background(accent, CircleShape),
        )
        Text(
            text = label,
            color = Color.White,
            style =
                if (compact) {
                    MaterialTheme.typography.labelSmall
                } else {
                    MaterialTheme.typography.labelMedium
                },
        )
    }
}
