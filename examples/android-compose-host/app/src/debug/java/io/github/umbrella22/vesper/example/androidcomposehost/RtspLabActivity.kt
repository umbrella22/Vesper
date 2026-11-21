package io.github.umbrella22.vesper.example.androidcomposehost

import android.net.Uri
import android.os.Build
import android.os.Bundle
import android.os.SystemClock
import android.util.Log
import android.view.SurfaceView
import android.view.WindowManager
import android.widget.FrameLayout
import androidx.activity.ComponentActivity
import androidx.lifecycle.lifecycleScope
import androidx.media3.common.MediaItem
import androidx.media3.common.PlaybackException
import androidx.media3.common.Player
import androidx.media3.common.util.UnstableApi
import androidx.media3.exoplayer.DefaultRenderersFactory
import androidx.media3.exoplayer.ExoPlayer
import androidx.media3.exoplayer.analytics.AnalyticsListener
import androidx.media3.exoplayer.mediacodec.MediaCodecSelector
import androidx.media3.exoplayer.rtsp.RtspMediaSource
import io.github.umbrella22.vesper.player.android.VesperPlayerController
import io.github.umbrella22.vesper.player.android.VesperPlayerControllerFactory
import io.github.umbrella22.vesper.player.android.VesperPlayerSource
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch

private const val TAG = "VesperRtspLab"

@androidx.annotation.OptIn(UnstableApi::class)
class RtspLabActivity : ComponentActivity() {
    private var bridge: VesperPlayerController? = null
    private var direct: ExoPlayer? = null

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        val uri = Uri.parse(intent.getStringExtra("uri") ?: "rtsp://127.0.0.1:8554/clock")
        val route = intent.getStringExtra("route") ?: "bridge"
        if (uri.scheme != "rtsp" || uri.host.isNullOrBlank() || uri.userInfo != null || route !in setOf("bridge", "direct")) {
            Log.e(TAG, "Expected an unauthenticated RTSP URL and route bridge or direct")
            finish()
            return
        }
        window.addFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON)
        val container = FrameLayout(this)
        setContentView(container)
        Log.i(TAG, "start route=$route model=${Build.MODEL} api=${Build.VERSION.SDK_INT} abi=${Build.SUPPORTED_ABIS.firstOrNull()}")
        if (route == "bridge") {
            startBridge(container, uri)
        } else {
            startDirect(container, uri)
        }
        lifecycleScope.launch {
            repeat(30) {
                delay(1000L)
                val timeline = bridge?.sampleTimeline()
                val error = bridge?.uiState?.value?.lastError?.code?.name
                Log.i(TAG, "sample route=$route positionMs=${timeline?.positionMs ?: direct?.currentPosition} " +
                    "timeline=${timeline?.kind} buffering=${bridge?.uiState?.value?.isBuffering ?: (direct?.playbackState == Player.STATE_BUFFERING)} error=$error")
            }
        }
    }

    private fun startBridge(container: FrameLayout, uri: Uri) {
        val controller = VesperPlayerControllerFactory.createDefault(this)
        bridge = controller
        var reportedFirstFrame = false
        val start = SystemClock.elapsedRealtime()
        controller.setOnPlaybackDiagnosticsChangedListener { diagnostics ->
            diagnostics.firstFrame?.let {
                if (!reportedFirstFrame) {
                    reportedFirstFrame = true
                    Log.i(TAG, "firstFrame route=bridge elapsedMs=${SystemClock.elapsedRealtime() - start}")
                }
            }
        }
        controller.attachSurfaceHost(container)
        lifecycleScope.launch {
            try {
                controller.selectSourceAsync(VesperPlayerSource.remote(uri.toString(), "RTSP lab"))
                controller.play()
            } catch (cancelled: CancellationException) {
                throw cancelled
            } catch (failure: Exception) {
                Log.e(TAG, "startupFailed route=bridge ${failure.message}")
            }
        }
    }

    private fun startDirect(container: FrameLayout, uri: Uri) {
        val surface = SurfaceView(this)
        container.addView(surface, FrameLayout.LayoutParams(-1, -1))
        val renderers = DefaultRenderersFactory(this).setMediaCodecSelector { mime, secure, tunneling ->
            MediaCodecSelector.DEFAULT.getDecoderInfos(mime, secure, tunneling).filter { it.hardwareAccelerated }
        }
        val player = ExoPlayer.Builder(this, renderers).build()
        direct = player
        val start = SystemClock.elapsedRealtime()
        player.addListener(object : Player.Listener {
            override fun onRenderedFirstFrame() {
                Log.i(TAG, "firstFrame route=direct elapsedMs=${SystemClock.elapsedRealtime() - start}")
            }

            override fun onPlayerError(error: PlaybackException) {
                Log.e(TAG, "playbackFailed route=direct ${error.errorCodeName}")
            }
        })
        player.addAnalyticsListener(object : AnalyticsListener {
            override fun onVideoDecoderInitialized(
                eventTime: AnalyticsListener.EventTime,
                decoderName: String,
                initializedTimestampMs: Long,
                initializationDurationMs: Long,
            ) {
                Log.i(TAG, "decoder route=direct name=$decoderName")
            }
        })
        player.setVideoSurfaceView(surface)
        player.setMediaSource(
            RtspMediaSource.Factory().setForceUseRtpTcp(true).createMediaSource(MediaItem.fromUri(uri)),
        )
        player.prepare()
        player.play()
    }

    override fun onDestroy() {
        direct?.release()
        direct = null
        bridge?.dispose()
        bridge = null
        super.onDestroy()
    }
}
