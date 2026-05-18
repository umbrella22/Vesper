package io.github.ikaros.vesper.player.android

import androidx.media3.common.PlaybackException
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class VesperNativeErrorMappingTest {
    @Test
    fun playbackExceptionNetworkErrorsMapToRetriableNetworkBackendFailure() {
        val error =
            classifyPlaybackException(
                playbackException(PlaybackException.ERROR_CODE_IO_NETWORK_CONNECTION_TIMEOUT)
            )

        assertEquals(VesperPlayerErrorCode.BackendFailure, VesperPlayerErrorCode.fromLegacyOrdinal(error.codeOrdinal))
        assertEquals(VesperPlayerErrorCategory.Network, VesperPlayerErrorCategory.fromLegacyOrdinal(error.categoryOrdinal))
        assertEquals(BACKEND_FAILURE_ORDINAL, error.codeOrdinal)
        assertEquals(NETWORK_CATEGORY_ORDINAL, error.categoryOrdinal)
        assertTrue(error.retriable)
    }

    @Test
    fun playbackExceptionSourceErrorsMapToSourceInvalidSource() {
        val error =
            classifyPlaybackException(
                playbackException(PlaybackException.ERROR_CODE_IO_FILE_NOT_FOUND)
            )

        assertEquals(VesperPlayerErrorCode.InvalidSource, VesperPlayerErrorCode.fromLegacyOrdinal(error.codeOrdinal))
        assertEquals(VesperPlayerErrorCategory.Source, VesperPlayerErrorCategory.fromLegacyOrdinal(error.categoryOrdinal))
        assertEquals(INVALID_SOURCE_ORDINAL, error.codeOrdinal)
        assertEquals(SOURCE_CATEGORY_ORDINAL, error.categoryOrdinal)
        assertFalse(error.retriable)
    }

    @Test
    fun playbackExceptionUnsupportedErrorsMapToCapabilityUnsupported() {
        val error =
            classifyPlaybackException(
                playbackException(PlaybackException.ERROR_CODE_PARSING_CONTAINER_UNSUPPORTED)
            )

        assertEquals(VesperPlayerErrorCode.Unsupported, VesperPlayerErrorCode.fromLegacyOrdinal(error.codeOrdinal))
        assertEquals(VesperPlayerErrorCategory.Capability, VesperPlayerErrorCategory.fromLegacyOrdinal(error.categoryOrdinal))
        assertEquals(UNSUPPORTED_ORDINAL, error.codeOrdinal)
        assertEquals(CAPABILITY_CATEGORY_ORDINAL, error.categoryOrdinal)
        assertFalse(error.retriable)
    }

    @Test
    fun playbackExceptionDecodeErrorsMapToDecodeFailure() {
        val error =
            classifyPlaybackException(
                playbackException(PlaybackException.ERROR_CODE_DECODING_FAILED)
            )

        assertEquals(VesperPlayerErrorCode.DecodeFailure, VesperPlayerErrorCode.fromLegacyOrdinal(error.codeOrdinal))
        assertEquals(VesperPlayerErrorCategory.Decode, VesperPlayerErrorCategory.fromLegacyOrdinal(error.categoryOrdinal))
        assertEquals(DECODE_FAILURE_ORDINAL, error.codeOrdinal)
        assertEquals(DECODE_CATEGORY_ORDINAL, error.categoryOrdinal)
        assertFalse(error.retriable)
    }

    @Test
    fun playbackExceptionAudioErrorsMapToAudioOutputUnavailable() {
        val error =
            classifyPlaybackException(
                playbackException(PlaybackException.ERROR_CODE_AUDIO_TRACK_INIT_FAILED)
            )

        assertEquals(VesperPlayerErrorCode.AudioOutputUnavailable, VesperPlayerErrorCode.fromLegacyOrdinal(error.codeOrdinal))
        assertEquals(VesperPlayerErrorCategory.AudioOutput, VesperPlayerErrorCategory.fromLegacyOrdinal(error.categoryOrdinal))
        assertEquals(AUDIO_OUTPUT_UNAVAILABLE_ORDINAL, error.codeOrdinal)
        assertEquals(AUDIO_OUTPUT_CATEGORY_ORDINAL, error.categoryOrdinal)
        assertFalse(error.retriable)
    }

    @Test
    fun playbackExceptionUnknownErrorsMapToPlatformBackendFailure() {
        val error = classifyPlaybackException(playbackException(PlaybackException.ERROR_CODE_UNSPECIFIED))

        assertEquals(VesperPlayerErrorCode.BackendFailure, VesperPlayerErrorCode.fromLegacyOrdinal(error.codeOrdinal))
        assertEquals(VesperPlayerErrorCategory.Platform, VesperPlayerErrorCategory.fromLegacyOrdinal(error.categoryOrdinal))
        assertEquals(BACKEND_FAILURE_ORDINAL, error.codeOrdinal)
        assertEquals(PLATFORM_CATEGORY_ORDINAL, error.categoryOrdinal)
        assertFalse(error.retriable)
    }

    @Test
    fun nativeErrorOrdinalsPreserveLegacyRuntimeValues() {
        assertEquals("invalidArgument", VesperPlayerErrorCode.InvalidArgument.wireName)
        assertEquals("audioOutput", VesperPlayerErrorCategory.AudioOutput.wireName)
        assertEquals(0, VesperPlayerErrorCode.InvalidArgument.legacyOrdinal)
        assertEquals(11, VesperPlayerErrorCode.Timeout.legacyOrdinal)
        assertEquals(0, VesperPlayerErrorCategory.Input.legacyOrdinal)
        assertEquals(7, VesperPlayerErrorCategory.Platform.legacyOrdinal)
        assertEquals(2, INVALID_SOURCE_ORDINAL)
        assertEquals(3, BACKEND_FAILURE_ORDINAL)
        assertEquals(4, AUDIO_OUTPUT_UNAVAILABLE_ORDINAL)
        assertEquals(5, DECODE_FAILURE_ORDINAL)
        assertEquals(7, UNSUPPORTED_ORDINAL)
        assertEquals(1, SOURCE_CATEGORY_ORDINAL)
        assertEquals(2, NETWORK_CATEGORY_ORDINAL)
        assertEquals(3, DECODE_CATEGORY_ORDINAL)
        assertEquals(4, AUDIO_OUTPUT_CATEGORY_ORDINAL)
        assertEquals(6, CAPABILITY_CATEGORY_ORDINAL)
        assertEquals(7, PLATFORM_CATEGORY_ORDINAL)
    }

    private fun playbackException(errorCode: Int): PlaybackException =
        PlaybackException("playback failed", null, errorCode)
}
