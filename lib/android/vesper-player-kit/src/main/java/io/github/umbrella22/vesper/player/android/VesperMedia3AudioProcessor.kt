package io.github.umbrella22.vesper.player.android

import android.content.Context
import android.os.Handler
import android.util.Log
import androidx.annotation.OptIn
import androidx.media3.common.C
import androidx.media3.common.Format
import androidx.media3.common.audio.AudioProcessor.AudioFormat
import androidx.media3.common.audio.BaseAudioProcessor
import androidx.media3.common.util.UnstableApi
import androidx.media3.exoplayer.DecoderReuseEvaluation
import androidx.media3.exoplayer.FormatHolder
import androidx.media3.exoplayer.audio.AudioRendererEventListener
import androidx.media3.exoplayer.audio.AudioSink
import androidx.media3.exoplayer.audio.MediaCodecAudioRenderer
import androidx.media3.exoplayer.mediacodec.MediaCodecAdapter
import androidx.media3.exoplayer.mediacodec.MediaCodecSelector
import androidx.media3.exoplayer.source.TrackGroupArray
import androidx.media3.exoplayer.trackselection.DefaultTrackSelector
import androidx.media3.exoplayer.trackselection.ExoTrackSelection
import androidx.media3.exoplayer.trackselection.MappingTrackSelector.MappedTrackInfo
import java.nio.ByteBuffer

internal object VesperMedia3DspJni {
    @JvmStatic external fun open(id: String, sampleRate: Int, channels: Int): Long
    @JvmStatic external fun process(
        handle: Long,
        input: ByteBuffer,
        inputPosition: Int,
        output: ByteBuffer,
        length: Int,
    ): Boolean
    @JvmStatic external fun close(handle: Long)
}

@OptIn(UnstableApi::class)
internal class VesperMedia3AudioProcessor(private val pluginId: String) : BaseAudioProcessor() {
    // Media3 invokes the track selector, renderer and processor on the playback thread.
    private var trackCatalogChecked = false
    private var clearAudioFormat = false
    private var protectedSource = false
    private var handle = 0L
    private var unavailableForFormat = false
    @Volatile
    internal var processedClearPcm = false
        private set

    fun inspectTrackCatalog(groups: Sequence<TrackGroupArray>) {
        groups.forEach(::inspectTracks)
        trackCatalogChecked = true
    }

    fun inspectInputFormat(format: Format?, hasDrmSession: Boolean) {
        if (hasDrmSession || format?.isProtectedFormat() == true) excludeProtected()
        clearAudioFormat = format != null && !protectedSource
    }

    private fun inspectTracks(groups: TrackGroupArray) {
        for (groupIndex in 0 until groups.length) {
            val group = groups[groupIndex]
            for (track in 0 until group.length) {
                if (group.getFormat(track).isProtectedFormat()) excludeProtected()
            }
        }
    }

    private fun excludeProtected() {
        if (!protectedSource) {
            protectedSource = true
            closeSession()
            Log.i(TAG, "Fixed-block DSP excluded by Media3 protection metadata")
        }
    }

    override fun onConfigure(inputAudioFormat: AudioFormat): AudioFormat =
        if (inputAudioFormat.encoding == C.ENCODING_PCM_16BIT && inputAudioFormat.channelCount in 1..8) {
            inputAudioFormat
        } else {
            AudioFormat.NOT_SET
        }

    override fun queueInput(inputBuffer: ByteBuffer) {
        val maxBytes = BLOCK_FRAMES * inputAudioFormat.bytesPerFrame
        val count = minOf(inputBuffer.remaining(), maxBytes)
        if (count == 0) return
        val output = replaceOutputBuffer(count)
        val start = inputBuffer.position()
        val allowed = !protectedSource && trackCatalogChecked && clearAudioFormat && inputBuffer.isDirect
        if (allowed && handle == 0L && !unavailableForFormat) {
            handle = VesperMedia3DspJni.open(pluginId, inputAudioFormat.sampleRate, inputAudioFormat.channelCount)
            if (handle == 0L) {
                unavailableForFormat = true
                Log.w(TAG, "Fixed-block DSP unavailable for the configured Media3 PCM format")
            }
        }
        if (allowed && handle != 0L && VesperMedia3DspJni.process(handle, inputBuffer, start, output, count)) {
            inputBuffer.position(start + count)
            output.position(count)
            if (!processedClearPcm) {
                processedClearPcm = true
                Log.i(TAG, "Fixed-block DSP processed Media3 PCM through native plugin ABI")
            }
        } else {
            if (handle != 0L) {
                unavailableForFormat = true
                closeSession()
                Log.w(TAG, "Fixed-block DSP was disabled after a processing failure")
            }
            val limit = inputBuffer.limit()
            inputBuffer.limit(start + count)
            try {
                output.put(inputBuffer)
            } finally {
                inputBuffer.limit(limit)
            }
        }
        output.flip()
    }

    override fun onFlush(streamMetadata: androidx.media3.common.audio.AudioProcessor.StreamMetadata) {
        closeSession()
        unavailableForFormat = false
    }

    override fun onReset() {
        closeSession()
    }

    private fun closeSession() {
        val previous = handle
        handle = 0L
        if (previous != 0L) VesperMedia3DspJni.close(previous)
    }

    private fun Format.isProtectedFormat(): Boolean =
        drmInitData != null || cryptoType != C.CRYPTO_TYPE_NONE

    private companion object {
        const val BLOCK_FRAMES = 1024
        const val TAG = "VesperMedia3Dsp"
    }
}

@OptIn(UnstableApi::class)
internal class VesperMedia3DspTrackSelector(
    context: Context,
    private val processor: VesperMedia3AudioProcessor,
) : DefaultTrackSelector(context) {
    override fun selectAllTracks(
        definitions: Array<ExoTrackSelection.Definition?>,
        mappedTrackInfo: MappedTrackInfo,
        rendererFormatSupports: Array<Array<IntArray>>,
        rendererMixedMimeTypeAdaptationSupports: IntArray,
        params: Parameters,
    ) {
        processor.inspectTrackCatalog(sequence {
            for (renderer in 0 until mappedTrackInfo.rendererCount) {
                yield(mappedTrackInfo.getTrackGroups(renderer))
            }
            yield(mappedTrackInfo.unmappedTrackGroups)
        })
        super.selectAllTracks(
            definitions, mappedTrackInfo, rendererFormatSupports, rendererMixedMimeTypeAdaptationSupports, params,
        )
    }
}

@OptIn(UnstableApi::class)
internal class VesperMedia3DspAudioRenderer(
    context: Context,
    codecAdapterFactory: MediaCodecAdapter.Factory,
    codecSelector: MediaCodecSelector,
    enableDecoderFallback: Boolean,
    eventHandler: Handler,
    eventListener: AudioRendererEventListener,
    audioSink: AudioSink,
    private val processor: VesperMedia3AudioProcessor,
) : MediaCodecAudioRenderer(
    context, codecAdapterFactory, codecSelector, enableDecoderFallback, eventHandler, eventListener, audioSink,
) {
    override fun onInputFormatChanged(formatHolder: FormatHolder): DecoderReuseEvaluation? {
        processor.inspectInputFormat(formatHolder.format, formatHolder.drmSession != null)
        return super.onInputFormatChanged(formatHolder)
    }
}
