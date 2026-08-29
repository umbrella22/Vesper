package io.github.umbrella22.vesper.player.android.compose.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.gestures.detectHorizontalDragGestures
import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.offset
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableFloatStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.layout.onSizeChanged
import androidx.compose.ui.unit.IntOffset
import androidx.compose.ui.unit.dp
import kotlin.math.roundToInt

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
    val latestOnSeekPreview by rememberUpdatedState(onSeekPreview)
    val latestOnSeekCommit by rememberUpdatedState(onSeekCommit)
    val latestOnSeekCancel by rememberUpdatedState(onSeekCancel)

    var scrubberModifier =
        modifier
            .fillMaxWidth()
            .height(touchHeight)
            .onSizeChanged { widthPx = it.width.toFloat().coerceAtLeast(1f) }
    if (enabled) {
        scrubberModifier =
            scrubberModifier
                .pointerInput(Unit) {
                    detectTapGestures { offset ->
                        val currentWidth = size.width.toFloat().coerceAtLeast(1f)
                        val targetRatio = (offset.x / currentWidth).coerceIn(0f, 1f)
                        latestOnSeekPreview(targetRatio)
                        latestOnSeekCommit(targetRatio)
                    }
                }
                .pointerInput(Unit) {
                    var dragRatio = ratio
                    detectHorizontalDragGestures(
                        onDragStart = { offset ->
                            val currentWidth = size.width.toFloat().coerceAtLeast(1f)
                            dragRatio = (offset.x / currentWidth).coerceIn(0f, 1f)
                            latestOnSeekPreview(dragRatio)
                        },
                        onHorizontalDrag = { change, _ ->
                            val currentWidth = size.width.toFloat().coerceAtLeast(1f)
                            dragRatio = (change.position.x / currentWidth).coerceIn(0f, 1f)
                            latestOnSeekPreview(dragRatio)
                        },
                        onDragCancel = latestOnSeekCancel,
                        onDragEnd = {
                            latestOnSeekCommit(dragRatio)
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
