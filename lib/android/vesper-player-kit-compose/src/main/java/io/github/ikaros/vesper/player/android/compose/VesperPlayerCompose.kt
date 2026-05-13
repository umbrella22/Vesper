package io.github.ikaros.vesper.player.android.compose

import android.view.ViewGroup
import android.widget.FrameLayout
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.remember
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalInspectionMode
import androidx.compose.ui.viewinterop.AndroidView
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import io.github.ikaros.vesper.player.android.PlaybackStateUi
import io.github.ikaros.vesper.player.android.PlayerHostUiState
import io.github.ikaros.vesper.player.android.VesperDecoderBackend
import io.github.ikaros.vesper.player.android.VesperPlaybackResiliencePolicy
import io.github.ikaros.vesper.player.android.VesperPlayerController
import io.github.ikaros.vesper.player.android.VesperPlayerControllerFactory
import io.github.ikaros.vesper.player.android.VesperPlayerSource
import io.github.ikaros.vesper.player.android.VesperVideoSurfaceKind
import kotlinx.coroutines.delay
import kotlinx.coroutines.isActive

private const val DEFAULT_PROGRESS_REFRESH_INTERVAL_MS = 250L

@Composable
fun rememberVesperPlayerController(
    initialSource: VesperPlayerSource? = null,
    resiliencePolicy: VesperPlaybackResiliencePolicy = VesperPlaybackResiliencePolicy(),
    decoderBackend: VesperDecoderBackend = VesperDecoderBackend.SystemOnly,
    surfaceKind: VesperVideoSurfaceKind = VesperVideoSurfaceKind.SurfaceView,
    keepScreenOnDuringPlayback: Boolean = true,
): VesperPlayerController {
    val isPreview = LocalInspectionMode.current
    val context = LocalContext.current.applicationContext
    val controller = remember(
        isPreview,
        context,
        initialSource,
        decoderBackend,
        surfaceKind,
        keepScreenOnDuringPlayback,
    ) {
        if (isPreview) {
            VesperPlayerControllerFactory.createPreview(
                initialSource = initialSource,
                keepScreenOnDuringPlayback = keepScreenOnDuringPlayback,
            )
        } else {
            VesperPlayerControllerFactory.createDefault(
                context = context,
                initialSource = initialSource,
                resiliencePolicy = resiliencePolicy,
                decoderBackend = decoderBackend,
                surfaceKind = surfaceKind,
                keepScreenOnDuringPlayback = keepScreenOnDuringPlayback,
            )
        }
    }
    LaunchedEffect(controller, resiliencePolicy) {
        controller.setResiliencePolicy(resiliencePolicy)
    }
    LaunchedEffect(controller, keepScreenOnDuringPlayback) {
        controller.setKeepScreenOnDuringPlayback(keepScreenOnDuringPlayback)
    }
    return controller
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
    manageControllerLifecycle: Boolean = true,
) {
    val surfaceHostRef = remember { arrayOfNulls<ViewGroup>(1) }
    if (manageControllerLifecycle) {
        DisposableEffect(controller) {
            controller.initialize()
            onDispose { controller.dispose() }
        }
    }
    AndroidView(
        modifier = modifier.fillMaxSize(),
        factory = { context ->
            object : FrameLayout(context) {}.apply {
                surfaceHostRef[0] = this
                controller.attachSurfaceHost(this)
            }
        },
        update = { host ->
            surfaceHostRef[0] = host
            controller.attachSurfaceHost(host)
        },
    )
    DisposableEffect(controller) {
        onDispose {
            controller.detachSurfaceHost(surfaceHostRef[0])
        }
    }
}

private fun shouldRefreshProgress(uiState: PlayerHostUiState): Boolean =
    uiState.playbackState == PlaybackStateUi.Playing || uiState.isBuffering
