package io.github.ikaros.vesper.player.android.compose.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.rounded.VolumeUp
import androidx.compose.material.icons.rounded.Speed
import androidx.compose.material.icons.rounded.WbSunny
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.unit.dp
import kotlin.math.roundToInt

internal enum class StageAreaGestureKind {
    Brightness,
    Volume,
    Seek,
    Ignored,
}

internal enum class StageGestureKind {
    Brightness,
    Volume,
    Speed,
}

internal data class StageGestureFeedback(
    val kind: StageGestureKind,
    val progress: Float?,
    val label: String,
)

@Composable
internal fun StageGestureFeedbackPanel(feedback: StageGestureFeedback) {
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

internal fun percentLabel(value: Float): String = "${(value * 100f).roundToInt()}%"
