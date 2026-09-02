package io.github.umbrella22.vesper.player.android.compose.ui

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
import androidx.compose.foundation.layout.BoxScope
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.RowScope
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
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.layout.onSizeChanged
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.IntOffset
import androidx.compose.ui.unit.dp
import io.github.umbrella22.vesper.player.android.PlaybackStateUi
import io.github.umbrella22.vesper.player.android.PlayerHostUiState
import io.github.umbrella22.vesper.player.android.TimelineKind
import io.github.umbrella22.vesper.player.android.VesperPlayerController
import io.github.umbrella22.vesper.player.android.VesperTrackCatalog
import io.github.umbrella22.vesper.player.android.VesperTrackSelectionSnapshot
import io.github.umbrella22.vesper.player.android.compose.VesperPlayerSurface
import kotlinx.coroutines.delay
import kotlin.math.abs
import kotlin.math.roundToInt

/**
 * Renders the Vesper playback surface, Stage gestures, and playback controls.
 *
 * [contentOverlay] renders above video and below all Stage interaction. It is
 * removed during Picture in Picture presentation. [landscapeControlBarLeading]
 * is invoked as a direct row child after the landscape play button, so hosts
 * can supply fixed or weighted content without an SDK-owned placeholder.
 * [onNavigateBack] controls whether the leading top-bar action is present.
 */
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
    pictureInPicturePresentation: Boolean = false,
    onControlsVisibilityChange: (Boolean) -> Unit,
    onPendingSeekRatioChange: (Float?) -> Unit,
    onOpenSheet: (VesperPlayerStageSheet) -> Unit,
    onToggleFullscreen: () -> Unit,
    onTogglePlayback: () -> Unit = { controller.togglePause() },
    onSeekToRatio: (Float) -> Unit = controller::seekToRatio,
    onSeekToLiveEdge: () -> Unit = controller::seekToLiveEdge,
    onSetPlaybackRate: (Float) -> Unit = controller::setPlaybackRate,
    playbackRateControlsEnabled: Boolean = true,
    currentBrightnessRatio: () -> Float? = { null },
    onSetBrightnessRatio: (Float) -> Float? = { null },
    currentVolumeRatio: () -> Float? = { null },
    onSetVolumeRatio: (Float) -> Float? = { null },
    contentOverlay: (@Composable BoxScope.() -> Unit)? = null,
    landscapeControlBarLeading: (@Composable RowScope.() -> Unit)? = null,
    onNavigateBack: (() -> Unit)? = null,
    navigateBackContentDescription: String? = null,
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
        onSetPlaybackRate(restoreRate)
    }

    LaunchedEffect(gestureFeedback) {
        if (gestureFeedback == null) {
            return@LaunchedEffect
        }
        delay(520)
        gestureFeedback = null
    }

    LaunchedEffect(pictureInPicturePresentation) {
        if (!pictureInPicturePresentation) {
            return@LaunchedEffect
        }
        endTemporarySpeedGesture()
        gestureFeedback = null
        onPendingSeekRatioChange(null)
        onControlsVisibilityChange(false)
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
            manageControllerLifecycle = false,
        )

        if (!pictureInPicturePresentation && contentOverlay != null) {
            Box(
                modifier = Modifier
                    .fillMaxSize()
                    .graphicsLayer()
                    .clearAndSetSemantics {},
                content = contentOverlay,
            )
        }

        if (!pictureInPicturePresentation) {
            Box(
                modifier = Modifier
                    .fillMaxSize()
                    .pointerInput(controller) {
                        detectTapGestures(
                            onTap = {
                                onControlsVisibilityChange(!latestControlsVisible)
                            },
                            onDoubleTap = { _ ->
                                onTogglePlayback()
                                onControlsVisibilityChange(true)
                            },
                            onLongPress = {
                                if (!playbackRateControlsEnabled) {
                                    return@detectTapGestures
                                }
                                if (speedGestureRestoreRate == null) {
                                    speedGestureRestoreRate = latestPlaybackRate
                                    onSetPlaybackRate(2f)
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
                                    onSeekToRatio(seekGestureRatio)
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
        }

        if (!pictureInPicturePresentation) {
            StageControlsOverlay(
                controlsVisible = controlsVisible,
                uiState = uiState,
                isPortrait = isPortrait,
                isPlaying = isPlaying,
                displayedRatio = displayedRatio,
                pendingSeekRatio = pendingSeekRatio,
                speedLabel = speedLabel,
                qualityLabel = qualityLabel,
                playbackRateControlsEnabled = playbackRateControlsEnabled,
                landscapeControlBarLeading = landscapeControlBarLeading,
                onNavigateBack = onNavigateBack,
                navigateBackContentDescription = navigateBackContentDescription,
                onOpenSheet = onOpenSheet,
                onTogglePlayback = onTogglePlayback,
                onControlsVisibilityChange = onControlsVisibilityChange,
                onPendingSeekRatioChange = onPendingSeekRatioChange,
                onSeekToRatio = onSeekToRatio,
                onSeekToLiveEdge = onSeekToLiveEdge,
                onToggleFullscreen = onToggleFullscreen,
            )
        }
        AnimatedVisibility(
            visible = !pictureInPicturePresentation && gestureFeedback != null,
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
