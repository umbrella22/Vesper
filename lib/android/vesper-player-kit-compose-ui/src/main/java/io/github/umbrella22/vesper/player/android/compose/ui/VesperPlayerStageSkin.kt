package io.github.umbrella22.vesper.player.android.compose.ui

import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.size
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.rounded.ArrowBack
import androidx.compose.material.icons.automirrored.rounded.VolumeUp
import androidx.compose.material.icons.rounded.Fullscreen
import androidx.compose.material.icons.rounded.FullscreenExit
import androidx.compose.material.icons.rounded.MoreVert
import androidx.compose.material.icons.rounded.Pause
import androidx.compose.material.icons.rounded.PlayArrow
import androidx.compose.material.icons.rounded.Speed
import androidx.compose.material.icons.rounded.WbSunny
import androidx.compose.material3.Icon
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.Immutable
import androidx.compose.runtime.staticCompositionLocalOf
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp

/** Action or feedback represented by an icon; independent of its asset. */
enum class VesperStageIconRole {
    Play, Pause, Fullscreen, ExitFullscreen, NavigateBack, More, Brightness, Volume, Speed,
}

enum class VesperStageButtonVariant {
    Standard, Toolbar, Navigation, Compact, CompactFullscreen, Expanded, ExpandedFullscreen, Primary,
}

@Immutable
data class VesperStageIconStyle(val size: Dp, val color: Color)

typealias VesperStageIconContent = @Composable (VesperStageIconStyle) -> Unit

@Immutable
data class VesperPlayerStageIcons(
    val play: ImageVector = Icons.Rounded.PlayArrow,
    val pause: ImageVector = Icons.Rounded.Pause,
    val fullscreen: ImageVector = Icons.Rounded.Fullscreen,
    val exitFullscreen: ImageVector = Icons.Rounded.FullscreenExit,
    val navigateBack: ImageVector = Icons.AutoMirrored.Rounded.ArrowBack,
    val more: ImageVector = Icons.Rounded.MoreVert,
    val brightness: ImageVector = Icons.Rounded.WbSunny,
    val volume: ImageVector = Icons.AutoMirrored.Rounded.VolumeUp,
    val speed: ImageVector = Icons.Rounded.Speed,
) {
    fun resolve(role: VesperStageIconRole): ImageVector = when (role) {
        VesperStageIconRole.Play -> play
        VesperStageIconRole.Pause -> pause
        VesperStageIconRole.Fullscreen -> fullscreen
        VesperStageIconRole.ExitFullscreen -> exitFullscreen
        VesperStageIconRole.NavigateBack -> navigateBack
        VesperStageIconRole.More -> more
        VesperStageIconRole.Brightness -> brightness
        VesperStageIconRole.Volume -> volume
        VesperStageIconRole.Speed -> speed
    }
}

@Immutable
data class VesperStageColors(
    val foreground: Color = Color.White,
    val secondaryForeground: Color = Color(0xFFBFC6D6),
    val background: Color = Color.Black,
    val scrim: Color = Color.Black,
    val buttonBackground: Color = Color.White,
    val accent: Color = Color(0xFFFFB454),
    val timelineStart: Color = Color(0xFFFF6B8E),
    val timelineEnd: Color = Color(0xFFFFB454),
    val timelineInactive: Color = Color.White,
    val timelineThumb: Color = Color.White,
    val hudBackground: Color = Color.Black.copy(alpha = 0.72f),
    val hudForeground: Color = Color.White,
)

/** Visual bounds; action buttons retain a minimum 48 dp touch target. */
@Immutable
data class VesperStageButtonStyle(
    val size: Dp = 52.dp,
    val iconSize: Dp = 24.dp,
    val backgroundOpacity: Float = 0.10f,
    val borderRadius: Dp = 999.dp,
) {
    init {
        require(size.value.isFinite() && size > 0.dp)
        require(iconSize.value.isFinite() && iconSize > 0.dp)
        require(backgroundOpacity in 0f..1f)
        require(borderRadius.value.isFinite() && borderRadius >= 0.dp)
    }
}

@Immutable
data class VesperStageMetrics(
    val standard: VesperStageButtonStyle = VesperStageButtonStyle(),
    val toolbar: VesperStageButtonStyle = VesperStageButtonStyle(size = 38.dp, backgroundOpacity = 0f),
    val navigation: VesperStageButtonStyle = VesperStageButtonStyle(size = 38.dp, iconSize = 23.dp, backgroundOpacity = 0f),
    val compact: VesperStageButtonStyle = VesperStageButtonStyle(size = 38.dp, backgroundOpacity = 0f),
    val compactFullscreen: VesperStageButtonStyle = VesperStageButtonStyle(size = 38.dp, backgroundOpacity = 0f),
    val expanded: VesperStageButtonStyle = VesperStageButtonStyle(size = 38.dp, iconSize = 22.dp, backgroundOpacity = 0f),
    val expandedFullscreen: VesperStageButtonStyle = VesperStageButtonStyle(size = 34.dp, iconSize = 19.dp, backgroundOpacity = 0f),
    val primary: VesperStageButtonStyle = VesperStageButtonStyle(size = 72.dp, iconSize = 36.dp, backgroundOpacity = 0.14f),
    val buttonSpacing: Dp = 8.dp,
    val hudIconSize: Dp = 24.dp,
    val hudBorderRadius: Dp = 999.dp,
    val timelineTrackHeight: Dp = 4.dp,
    val timelineThumbSize: Dp = 11.dp,
    val timelineLargeThumbSize: Dp = 14.dp,
) {
    init {
        require(buttonSpacing.value.isFinite() && buttonSpacing >= 0.dp)
        require(hudIconSize.value.isFinite() && hudIconSize > 0.dp)
        require(hudBorderRadius.value.isFinite() && hudBorderRadius >= 0.dp)
        require(timelineTrackHeight.value.isFinite() && timelineTrackHeight > 0.dp)
        require(timelineThumbSize.value.isFinite() && timelineThumbSize > 0.dp)
        require(timelineLargeThumbSize.value.isFinite() && timelineLargeThumbSize > 0.dp)
    }
    fun button(variant: VesperStageButtonVariant): VesperStageButtonStyle = when (variant) {
        VesperStageButtonVariant.Standard -> standard
        VesperStageButtonVariant.Toolbar -> toolbar
        VesperStageButtonVariant.Navigation -> navigation
        VesperStageButtonVariant.Compact -> compact
        VesperStageButtonVariant.CompactFullscreen -> compactFullscreen
        VesperStageButtonVariant.Expanded -> expanded
        VesperStageButtonVariant.ExpandedFullscreen -> expandedFullscreen
        VesperStageButtonVariant.Primary -> primary
    }
}

/** Presentation-only skin. A null iconContent result uses the configured vector. */
data class VesperPlayerStageSkin(
    val icons: VesperPlayerStageIcons = VesperPlayerStageIcons(),
    val colors: VesperStageColors = VesperStageColors(),
    val metrics: VesperStageMetrics = VesperStageMetrics(),
    val iconContent: ((VesperStageIconRole) -> VesperStageIconContent?)? = null,
)

val LocalVesperPlayerStageSkin = staticCompositionLocalOf { VesperPlayerStageSkin() }
internal val LocalVesperStageIconStyle = staticCompositionLocalOf<VesperStageIconStyle?> { null }

/** Scopes standalone controls and host content. Each Stage establishes its own scope. */
@Composable
fun VesperPlayerStageTheme(skin: VesperPlayerStageSkin, content: @Composable () -> Unit) {
    CompositionLocalProvider(LocalVesperPlayerStageSkin provides skin, content = content)
}

/** Decorative icon. Its containing action supplies accessibility and pointer handling. */
@Composable
fun VesperStageIcon(role: VesperStageIconRole, style: VesperStageIconStyle? = null) {
    val skin = LocalVesperPlayerStageSkin.current
    val effectiveStyle = style ?: LocalVesperStageIconStyle.current
        ?: VesperStageIconStyle(skin.metrics.standard.iconSize, skin.colors.foreground)
    val content = skin.iconContent?.invoke(role)
    Box(Modifier.size(effectiveStyle.size).clearAndSetSemantics {}) {
        if (content != null) content(effectiveStyle)
        else Icon(skin.icons.resolve(role), contentDescription = null,
            modifier = Modifier.size(effectiveStyle.size), tint = effectiveStyle.color)
    }
}
