package io.github.umbrella22.vesper.player.android.compose

import android.util.Log
import android.view.ViewGroup
import android.widget.FrameLayout
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalInspectionMode
import androidx.compose.ui.viewinterop.AndroidView
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import io.github.umbrella22.vesper.player.android.PlaybackStateUi
import io.github.umbrella22.vesper.player.android.PlayerHostUiState
import io.github.umbrella22.vesper.player.android.TimelineUiState
import io.github.umbrella22.vesper.player.android.VesperDecoderBackend
import io.github.umbrella22.vesper.player.android.VesperPlaybackResiliencePolicy
import io.github.umbrella22.vesper.player.android.VesperPlayerController
import io.github.umbrella22.vesper.player.android.VesperPlayerControllerFactory
import io.github.umbrella22.vesper.player.android.VesperPlayerSource
import io.github.umbrella22.vesper.player.android.VesperVideoSurfaceKind
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.delay
import kotlinx.coroutines.isActive

private const val DEFAULT_PROGRESS_REFRESH_INTERVAL_MS = 1_000L
private const val MAX_PROGRESS_REFRESH_BACKOFF_MS = 8_000L

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
    val latestUiState by rememberUpdatedState(uiState)
    var timelineSample by remember(controller) {
        mutableStateOf<PresentedTimelineSample?>(null)
    }

    LaunchedEffect(
        controller,
        uiState.playbackState,
        uiState.isBuffering,
        progressRefreshIntervalMs,
    ) {
        if (!shouldRefreshProgress(uiState)) {
            return@LaunchedEffect
        }

        val baseDelayMs = progressRefreshIntervalMs.coerceAtLeast(1L)
        val maxDelayMs = maxOf(baseDelayMs, MAX_PROGRESS_REFRESH_BACKOFF_MS)
        var refreshDelayMs = baseDelayMs
        while (isActive) {
            delay(refreshDelayMs)
            val authoritativeState = latestUiState
            if (!shouldRefreshProgress(authoritativeState)) {
                break
            }
            try {
                val sampledTimeline = controller.sampleTimeline()
                if (
                    sampledTimeline != null &&
                    controller.uiState.value == authoritativeState &&
                    shouldRefreshProgress(latestUiState)
                ) {
                    timelineSample =
                        PresentedTimelineSample(
                            authoritativeState = authoritativeState,
                            timeline = sampledTimeline,
                        )
                }
                refreshDelayMs = baseDelayMs
            } catch (cancellation: CancellationException) {
                throw cancellation
            } catch (_: Exception) {
                refreshDelayMs = nextProgressRefreshDelay(refreshDelayMs, maxDelayMs)
            }
        }
    }

    return uiState.withTimelineSample(timelineSample)
}

@Composable
fun VesperPlayerSurface(
    controller: VesperPlayerController,
    modifier: Modifier = Modifier,
    manageControllerLifecycle: Boolean = true,
) {
    var surfaceHost by remember { mutableStateOf<ViewGroup?>(null) }
    val attachedControllerRef = remember { arrayOfNulls<VesperPlayerController>(1) }

    fun attachControllerToHost(
        host: ViewGroup,
        nextController: VesperPlayerController,
    ) {
        val previousController = attachedControllerRef[0]
        if (previousController !== nextController) {
            Log.d(
                TAG,
                "surface composable switching controller previous=${previousController?.identity()} " +
                    "next=${nextController.identity()}",
            )
            previousController?.detachSurfaceHost(host)
            attachedControllerRef[0] = nextController
        }
        Log.d(
            TAG,
            "surface composable attach controller=${nextController.identity()} " +
                "hostAttached=${host.isAttachedToWindow} hostSize=${host.width}x${host.height}",
        )
        nextController.attachSurfaceHost(host)
    }

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
                Log.d(TAG, "surface composable factory controller=${controller.identity()}")
                surfaceHost = this
                attachControllerToHost(this, controller)
            }
        },
        update = { host ->
            Log.d(TAG, "surface composable update controller=${controller.identity()}")
            surfaceHost = host
            attachControllerToHost(host, controller)
        },
    )
    surfaceHost?.let { host ->
        DisposableEffect(controller, host) {
            attachControllerToHost(host, controller)
            onDispose {
                if (attachedControllerRef[0] === controller) {
                    Log.d(TAG, "surface composable dispose controller=${controller.identity()}")
                    controller.detachSurfaceHost(host)
                    attachedControllerRef[0] = null
                }
            }
        }
    }
    DisposableEffect(Unit) {
        onDispose {
            Log.d(TAG, "surface composable final dispose")
            attachedControllerRef[0]?.detachSurfaceHost(surfaceHost)
            attachedControllerRef[0] = null
        }
    }
}

private fun shouldRefreshProgress(uiState: PlayerHostUiState): Boolean =
    uiState.playbackState == PlaybackStateUi.Playing || uiState.isBuffering

internal data class PresentedTimelineSample(
    val authoritativeState: PlayerHostUiState,
    val timeline: TimelineUiState,
)

internal fun PlayerHostUiState.withTimelineSample(
    sample: PresentedTimelineSample?,
): PlayerHostUiState =
    if (sample?.authoritativeState == this) copy(timeline = sample.timeline) else this

internal fun nextProgressRefreshDelay(currentDelayMs: Long, maxDelayMs: Long): Long {
    require(currentDelayMs > 0L)
    require(maxDelayMs >= currentDelayMs)
    return if (currentDelayMs >= maxDelayMs / 2L) maxDelayMs else currentDelayMs * 2L
}

private fun VesperPlayerController.identity(): String =
    Integer.toHexString(System.identityHashCode(this))

private const val TAG = "VesperPlayerAndroidHost"
