package io.github.umbrella22.vesper.player.android

import android.content.Context
import android.net.Uri
import android.os.SystemClock
import androidx.annotation.OptIn
import androidx.media3.common.C
import androidx.media3.common.DrmInitData
import androidx.media3.common.Format
import androidx.media3.common.MediaItem
import androidx.media3.common.MimeTypes
import androidx.media3.common.TrackGroup
import androidx.media3.common.audio.AudioProcessor
import androidx.media3.common.util.UnstableApi
import androidx.media3.exoplayer.ExoPlayer
import androidx.media3.exoplayer.source.TrackGroupArray
import androidx.test.core.app.ApplicationProvider
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import java.io.File
import java.nio.ByteBuffer
import java.nio.ByteOrder
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith

@OptIn(UnstableApi::class)
@RunWith(AndroidJUnit4::class)
class VesperMedia3DspInstrumentationTest {
    @Test
    fun jniProcessesReadOnlyPcmAndRejectsRetiredHandle() {
        VesperNativeLibrary.ensureLoaded()
        val handle = VesperMedia3DspJni.open(REFERENCE_GAIN, 48000, 1)
        assertTrue(handle != 0L)
        val input = ByteBuffer.allocateDirect(8).order(ByteOrder.nativeOrder())
        input.putShort(2, 16000)
        input.putShort(4, -12000)
        val readonly = input.asReadOnlyBuffer()
        val output = ByteBuffer.allocateDirect(4).order(ByteOrder.nativeOrder())
        try {
            assertTrue(VesperMedia3DspJni.process(handle, readonly, 2, output, 4))
            assertEquals(8000, output.getShort(0).toInt())
            assertEquals(-6000, output.getShort(2).toInt())
            assertEquals(16000, input.getShort(2).toInt())
            assertEquals(-12000, input.getShort(4).toInt())
        } finally {
            VesperMedia3DspJni.close(handle)
        }
        assertFalse(VesperMedia3DspJni.process(handle, readonly, 2, output, 4))
    }

    @Test
    fun media3SystemAudioRendererRunsNativeDspForClearTrack() {
        VesperNativeLibrary.ensureLoaded()
        val context = ApplicationProvider.getApplicationContext<Context>()
        val file = File.createTempFile("vesper-dsp", ".wav", context.cacheDir)
        val processor = VesperMedia3AudioProcessor(REFERENCE_GAIN)
        val instrumentation = InstrumentationRegistry.getInstrumentation()
        var player: ExoPlayer? = null
        try {
            writeFixture(file)
            instrumentation.runOnMainSync {
                val factory = VesperExternalSubtitleRenderersFactory(context, processor)
                    .setMediaCodecSelector(VesperHardwareMediaCodecSelector)
                player = ExoPlayer.Builder(context, factory)
                    .setTrackSelector(VesperMedia3DspTrackSelector(context, processor))
                    .build()
                    .apply {
                        setMediaItem(MediaItem.fromUri(Uri.fromFile(file)))
                        volume = 0f
                        prepare()
                        play()
                    }
            }
            val deadline = SystemClock.elapsedRealtime() + 10000L
            while (!processor.processedClearPcm && SystemClock.elapsedRealtime() < deadline) {
                SystemClock.sleep(25)
            }
            assertTrue("Media3 audio did not reach the native fixed-block plugin", processor.processedClearPcm)
        } finally {
            instrumentation.runOnMainSync { player?.release() }
            file.delete()
        }
    }

    @Test
    fun protectionMetadataInVideoTrackRetiresDspForClearAudioInSamePresentation() {
        VesperNativeLibrary.ensureLoaded()
        val audio = Format.Builder().setSampleMimeType(MimeTypes.AUDIO_RAW).build()
        val protectedVideo = Format.Builder()
            .setSampleMimeType(MimeTypes.VIDEO_H264)
            .setDrmInitData(DrmInitData(DrmInitData.SchemeData(C.WIDEVINE_UUID, MimeTypes.VIDEO_MP4, byteArrayOf(1))))
            .build()
        val processor = VesperMedia3AudioProcessor(REFERENCE_GAIN)
        try {
            processor.configure(AudioProcessor.AudioFormat(48000, 1, C.ENCODING_PCM_16BIT))
            processor.flush(AudioProcessor.StreamMetadata.DEFAULT)
            processor.inspectTrackCatalog(sequenceOf(TrackGroupArray(TrackGroup("audio", audio))))
            processor.inspectInputFormat(audio, hasDrmSession = false)
            assertEquals(8000, processOneSample(processor, 16000))

            processor.inspectTrackCatalog(sequenceOf(
                TrackGroupArray(TrackGroup("audio", audio)),
                TrackGroupArray(TrackGroup("video", protectedVideo)),
            ))
            processor.inspectInputFormat(audio, hasDrmSession = false)
            assertEquals(16000, processOneSample(processor, 16000))

            // A later clear format cannot reopen DSP for this presentation.
            processor.flush(AudioProcessor.StreamMetadata.DEFAULT)
            processor.inspectTrackCatalog(sequenceOf(TrackGroupArray(TrackGroup("audio", audio))))
            processor.inspectInputFormat(audio, hasDrmSession = false)
            assertEquals(16000, processOneSample(processor, 16000))
        } finally {
            processor.reset()
        }
    }

    private fun processOneSample(processor: VesperMedia3AudioProcessor, sample: Short): Int {
        val input = ByteBuffer.allocateDirect(2).order(ByteOrder.nativeOrder())
        input.putShort(sample).flip()
        processor.queueInput(input.asReadOnlyBuffer().order(ByteOrder.nativeOrder()))
        assertEquals(sample, input.getShort(0))
        return processor.output.order(ByteOrder.nativeOrder()).getShort().toInt()
    }

    private fun writeFixture(file: File) {
        val rate = 48000
        val frameCount = rate * 3
        val sampleBytes = frameCount * 2
        val wave = ByteBuffer.allocate(44 + sampleBytes).order(ByteOrder.LITTLE_ENDIAN)
        wave.put("RIFF".toByteArray()).putInt(36 + sampleBytes).put("WAVEfmt ".toByteArray())
        wave.putInt(16).putShort(1).putShort(1).putInt(rate).putInt(rate * 2).putShort(2).putShort(16)
        wave.put("data".toByteArray()).putInt(sampleBytes)
        repeat(frameCount) { frame -> wave.putShort(if ((frame / 64) % 2 == 0) 8192 else -8192) }
        file.writeBytes(wave.array())
    }

    private companion object {
        const val REFERENCE_GAIN = "io.github.umbrella22.vesper.media3-gain"
    }
}
