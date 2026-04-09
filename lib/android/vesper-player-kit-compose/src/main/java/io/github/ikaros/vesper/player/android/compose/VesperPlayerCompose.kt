package io.github.ikaros.vesper.player.android.compose

import android.graphics.Color as AndroidColor
import android.graphics.drawable.GradientDrawable
import android.view.ViewOutlineProvider
import android.view.ViewGroup
import android.widget.FrameLayout
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.remember
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalInspectionMode
import androidx.compose.ui.unit.dp
import androidx.compose.ui.viewinterop.AndroidView
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import io.github.ikaros.vesper.player.android.NativeVideoSurfaceKind
import io.github.ikaros.vesper.player.android.PlaybackStateUi
import io.github.ikaros.vesper.player.android.PlayerHostUiState
import io.github.ikaros.vesper.player.android.VesperPlaybackResiliencePolicy
import io.github.ikaros.vesper.player.android.VesperPlayerController
import io.github.ikaros.vesper.player.android.VesperPlayerControllerFactory
import io.github.ikaros.vesper.player.android.VesperPlayerSource
import kotlinx.coroutines.delay
import kotlinx.coroutines.isActive

private const val DEFAULT_PROGRESS_REFRESH_INTERVAL_MS = 250L

@Composable
fun rememberVesperPlayerController(
    initialSource: VesperPlayerSource? = null,
    resiliencePolicy: VesperPlaybackResiliencePolicy = VesperPlaybackResiliencePolicy(),
    surfaceKind: NativeVideoSurfaceKind = NativeVideoSurfaceKind.SurfaceView,
): VesperPlayerController {
    val isPreview = LocalInspectionMode.current
    val context = LocalContext.current.applicationContext
    return remember(isPreview, context, initialSource, resiliencePolicy, surfaceKind) {
        if (isPreview) {
            VesperPlayerControllerFactory.createPreview(initialSource)
        } else {
            VesperPlayerControllerFactory.createDefault(
                context = context,
                initialSource = initialSource,
                resiliencePolicy = resiliencePolicy,
                surfaceKind = surfaceKind,
            )
        }
    }
}

@Composable
fun rememberVesperPlayerUiState(
    controller: VesperPlayerController,
    progressRefreshIntervalMs: Long = DEFAULT_PROGRESS_REFRESH_INTERVAL_MS,
): PlayerHostUiState {
    val uiState by controller.uiState.collectAsStateWithLifecycle()

    LaunchedEffect(
        controller,
        uiState.playbackState,
        uiState.isBuffering,
        progressRefreshIntervalMs,
    ) {
        if (!shouldRefreshProgress(uiState)) {
            return@LaunchedEffect
        }

        while (isActive) {
            delay(progressRefreshIntervalMs)
            controller.refresh()
            if (!shouldRefreshProgress(controller.uiState.value)) {
                break
            }
        }
    }

    return uiState
}

@Composable
fun VesperPlayerSurface(
    controller: VesperPlayerController,
    modifier: Modifier = Modifier,
    cornerRadiusDp: androidx.compose.ui.unit.Dp = 20.dp,
    manageControllerLifecycle: Boolean = true,
) {
    val surfaceHostRef = remember { arrayOfNulls<ViewGroup>(1) }
    Box(
        modifier = modifier
            .clip(RoundedCornerShape(cornerRadiusDp))
            .background(Color.Black, RoundedCornerShape(cornerRadiusDp)),
    ) {
        if (manageControllerLifecycle) {
            DisposableEffect(controller) {
                controller.initialize()
                onDispose { controller.dispose() }
            }
        }
        AndroidView(
            modifier = Modifier.fillMaxSize(),
            factory = { context ->
                object : FrameLayout(context) {}.apply {
                    surfaceHostRef[0] = this
                    applyHostShape(cornerRadiusDp.value)
                    controller.attachSurfaceHost(this)
                }
            },
            update = { host ->
                surfaceHostRef[0] = host
                (host as FrameLayout).applyHostShape(cornerRadiusDp.value)
                controller.attachSurfaceHost(host)
            },
        )
        DisposableEffect(controller) {
            onDispose { controller.detachSurfaceHost(surfaceHostRef[0]) }
        }
    }
}

private fun shouldRefreshProgress(uiState: PlayerHostUiState): Boolean =
    uiState.playbackState == PlaybackStateUi.Playing || uiState.isBuffering

private fun android.widget.FrameLayout.applyHostShape(cornerRadiusDp: Float) {
    val cornerRadiusPx = cornerRadiusDp * resources.displayMetrics.density
    background =
        GradientDrawable().apply {
            shape = GradientDrawable.RECTANGLE
            setColor(AndroidColor.BLACK)
            cornerRadius = cornerRadiusPx
        }
    clipToOutline = cornerRadiusPx > 0f
    outlineProvider = ViewOutlineProvider.BACKGROUND
}
