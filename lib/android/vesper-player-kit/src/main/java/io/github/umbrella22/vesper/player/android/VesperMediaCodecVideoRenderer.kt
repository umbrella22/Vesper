package io.github.umbrella22.vesper.player.android

import android.content.Context
import android.os.Build
import android.os.Handler
import androidx.media3.common.util.UnstableApi
import androidx.media3.exoplayer.DefaultRenderersFactory
import androidx.media3.exoplayer.mediacodec.MediaCodecAdapter
import androidx.media3.exoplayer.mediacodec.MediaCodecSelector
import androidx.media3.exoplayer.video.MediaCodecVideoRenderer
import androidx.media3.exoplayer.video.VideoRendererEventListener

private const val LAST_AFFECTED_MTK_SET_OUTPUT_SURFACE_API = 29
private const val MTK_OMX_CODEC_PREFIX = "OMX.MTK."

internal fun vesperCodecNeedsSetOutputSurfaceWorkaround(
    media3RequiresWorkaround: Boolean,
    sdkInt: Int,
    codecName: String,
): Boolean =
    media3RequiresWorkaround ||
        (sdkInt <= LAST_AFFECTED_MTK_SET_OUTPUT_SURFACE_API &&
            codecName.startsWith(MTK_OMX_CODEC_PREFIX))

@UnstableApi
internal class VesperMediaCodecVideoRenderer(
    context: Context,
    codecAdapterFactory: MediaCodecAdapter.Factory,
    mediaCodecSelector: MediaCodecSelector,
    allowedVideoJoiningTimeMs: Long,
    enableDecoderFallback: Boolean,
    eventHandler: Handler,
    eventListener: VideoRendererEventListener,
) : MediaCodecVideoRenderer(
        Builder(context)
            .setCodecAdapterFactory(codecAdapterFactory)
            .setMediaCodecSelector(mediaCodecSelector)
            .setAllowedJoiningTimeMs(allowedVideoJoiningTimeMs)
            .setEnableDecoderFallback(enableDecoderFallback)
            .setEventHandler(eventHandler)
            .setEventListener(eventListener)
            .setMaxDroppedFramesToNotify(
                DefaultRenderersFactory.MAX_DROPPED_VIDEO_FRAME_COUNT_TO_NOTIFY,
            ),
    ) {
    override fun codecNeedsSetOutputSurfaceWorkaround(codecName: String): Boolean =
        vesperCodecNeedsSetOutputSurfaceWorkaround(
            media3RequiresWorkaround = super.codecNeedsSetOutputSurfaceWorkaround(codecName),
            sdkInt = Build.VERSION.SDK_INT,
            codecName = codecName,
        )
}
