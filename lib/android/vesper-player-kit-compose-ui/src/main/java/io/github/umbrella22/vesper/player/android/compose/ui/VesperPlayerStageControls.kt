package io.github.umbrella22.vesper.player.android.compose.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.rounded.Pause
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.Icon
import androidx.compose.material3.LocalContentColor
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp

/** A primary playback action using the current skin and localized label. */
@Composable
fun VesperStagePrimaryPlayButton(
    isPlaying: Boolean,
    style: VesperStageButtonStyle? = null,
    onClick: () -> Unit,
) {
    VesperStageIconButton(
        label = stringResource(if (isPlaying) R.string.vesper_player_stage_pause else R.string.vesper_player_stage_play),
        variant = VesperStageButtonVariant.Primary,
        style = style,
        onClick = onClick,
    ) { VesperStageIcon(if (isPlaying) VesperStageIconRole.Pause else VesperStageIconRole.Play) }
}

/** Non-interactive icon content inside an accessible action with a 48 dp hit area. */
@Composable
fun VesperStageIconButton(
    label: String,
    modifier: Modifier = Modifier,
    variant: VesperStageButtonVariant = VesperStageButtonVariant.Standard,
    style: VesperStageButtonStyle? = null,
    onClick: () -> Unit,
    icon: @Composable () -> Unit,
) {
    val skin = LocalVesperPlayerStageSkin.current
    val appearance = style ?: skin.metrics.button(variant)
    Box(
        modifier = modifier
            .size(appearance.size.coerceAtLeast(48.dp)),
        contentAlignment = Alignment.Center,
    ) {
        Box(Modifier.size(appearance.size)
            .background(skin.colors.buttonBackground.copy(alpha = skin.colors.buttonBackground.alpha * appearance.backgroundOpacity),
                RoundedCornerShape(appearance.borderRadius)), contentAlignment = Alignment.Center) {
            CompositionLocalProvider(
                LocalContentColor provides skin.colors.foreground,
                LocalVesperStageIconStyle provides VesperStageIconStyle(appearance.iconSize, skin.colors.foreground),
            ) {
                Box(Modifier.size(appearance.iconSize).clearAndSetSemantics {}) { icon() }
            }
        }
        // The topmost hit layer gives the SDK action priority over icon content.
        Box(
            Modifier.matchParentSize()
                .clickable(role = Role.Button, onClick = onClick)
                .semantics { contentDescription = label },
        )
    }
}

@Composable
internal fun StageIconButton(
    icon: VesperStageIconRole,
    label: String,
    variant: VesperStageButtonVariant,
    onClick: () -> Unit,
) = VesperStageIconButton(label = label, variant = variant, onClick = onClick) { VesperStageIcon(icon) }

@Composable
fun VesperStagePillButton(
    label: String,
    icon: ImageVector? = null,
    compact: Boolean = false,
    onClick: () -> Unit,
) {
    val skin = LocalVesperPlayerStageSkin.current
    TextButton(
        onClick = onClick,
        colors = ButtonDefaults.textButtonColors(contentColor = skin.colors.foreground),
        contentPadding =
            PaddingValues(
                horizontal = if (compact) 10.dp else 12.dp,
                vertical = if (compact) 6.dp else 8.dp,
            ),
        modifier = Modifier
            .heightIn(min = if (compact) 30.dp else 32.dp)
            .background(skin.colors.buttonBackground.copy(alpha = skin.colors.buttonBackground.alpha * 0.10f), RoundedCornerShape(skin.metrics.standard.borderRadius)),
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
fun VesperStageChip(
    label: String,
    accent: Color,
    modifier: Modifier = Modifier,
    compact: Boolean = false,
) {
    val skin = LocalVesperPlayerStageSkin.current
    val dotSize = if (compact) 6.dp else 8.dp
    val horizontalPadding = if (compact) 8.dp else 10.dp
    val verticalPadding = if (compact) 5.dp else 7.dp
    val spacing = if (compact) 6.dp else 8.dp
    Row(
        modifier = modifier
            .background(skin.colors.scrim.copy(alpha = skin.colors.scrim.alpha * 0.36f), RoundedCornerShape(999.dp))
            .border(1.dp, skin.colors.foreground.copy(alpha = skin.colors.foreground.alpha * 0.08f), RoundedCornerShape(999.dp))
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
            color = skin.colors.foreground,
            style =
                if (compact) {
                    MaterialTheme.typography.labelSmall
                } else {
                    MaterialTheme.typography.labelMedium
                },
        )
    }
}
