package io.github.ikaros.vesper.player.android

import androidx.media3.common.PlaybackException
import java.io.File
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertTrue
import org.junit.Test

class VesperNativeErrorMappingTest {
    @Test
    fun sharedPlayerErrorContractKeepsStableWireNames() {
        val payload = contractText("player_error.json")

        assertTrue(payload.contains("\"message\": \"fixture unsupported capability\""))
        assertEquals(
            VesperPlayerErrorCode.Unsupported,
            VesperPlayerErrorCode.fromWireName(contractString(payload, "code")),
        )
        assertEquals(
            VesperPlayerErrorCategory.Capability,
            VesperPlayerErrorCategory.fromWireName(contractString(payload, "category")),
        )
        assertTrue(payload.contains("\"retriable\": false"))
        assertTrue(payload.contains("\"operation\": \"setAbrPolicy\""))
    }

    @Test
    fun sharedPluginDiagnosticsContractKeepsStableWireNames() {
        val payload = contractText("plugin_diagnostics.json")

        assertTrue(payload.contains("\"status\": \"decoderSupported\""))
        assertTrue(payload.contains("\"participation\": \"participated\""))
        assertTrue(payload.contains("\"pluginKind\": \"decoder\""))
        assertTrue(payload.contains("\"codec\": \"h264\""))
        assertTrue(payload.contains("\"status\": \"frameProcessorSupported\""))
        assertTrue(payload.contains("\"participation\": \"available\""))
        assertTrue(payload.contains("\"kind\": \"frameProcessor\""))
        assertTrue(payload.contains("\"maxInFlightFrames\": 4"))
    }

    @Test
    fun playbackExceptionNetworkErrorsMapToRetriableNetworkBackendFailure() {
        val error =
            classifyPlaybackException(
                playbackException(PlaybackException.ERROR_CODE_IO_NETWORK_CONNECTION_TIMEOUT)
            )

        assertEquals(VesperPlayerErrorCode.BackendFailure, VesperPlayerErrorCode.fromJniOrdinal(error.codeOrdinal))
        assertEquals(VesperPlayerErrorCategory.Network, VesperPlayerErrorCategory.fromJniOrdinal(error.categoryOrdinal))
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

        assertEquals(VesperPlayerErrorCode.InvalidSource, VesperPlayerErrorCode.fromJniOrdinal(error.codeOrdinal))
        assertEquals(VesperPlayerErrorCategory.Source, VesperPlayerErrorCategory.fromJniOrdinal(error.categoryOrdinal))
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

        assertEquals(VesperPlayerErrorCode.Unsupported, VesperPlayerErrorCode.fromJniOrdinal(error.codeOrdinal))
        assertEquals(VesperPlayerErrorCategory.Capability, VesperPlayerErrorCategory.fromJniOrdinal(error.categoryOrdinal))
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

        assertEquals(VesperPlayerErrorCode.DecodeFailure, VesperPlayerErrorCode.fromJniOrdinal(error.codeOrdinal))
        assertEquals(VesperPlayerErrorCategory.Decode, VesperPlayerErrorCategory.fromJniOrdinal(error.categoryOrdinal))
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

        assertEquals(VesperPlayerErrorCode.AudioOutputUnavailable, VesperPlayerErrorCode.fromJniOrdinal(error.codeOrdinal))
        assertEquals(VesperPlayerErrorCategory.AudioOutput, VesperPlayerErrorCategory.fromJniOrdinal(error.categoryOrdinal))
        assertEquals(AUDIO_OUTPUT_UNAVAILABLE_ORDINAL, error.codeOrdinal)
        assertEquals(AUDIO_OUTPUT_CATEGORY_ORDINAL, error.categoryOrdinal)
        assertFalse(error.retriable)
    }

    @Test
    fun playbackExceptionUnknownErrorsMapToPlatformBackendFailure() {
        val error = classifyPlaybackException(playbackException(PlaybackException.ERROR_CODE_UNSPECIFIED))

        assertEquals(VesperPlayerErrorCode.BackendFailure, VesperPlayerErrorCode.fromJniOrdinal(error.codeOrdinal))
        assertEquals(VesperPlayerErrorCategory.Platform, VesperPlayerErrorCategory.fromJniOrdinal(error.categoryOrdinal))
        assertEquals(BACKEND_FAILURE_ORDINAL, error.codeOrdinal)
        assertEquals(PLATFORM_CATEGORY_ORDINAL, error.categoryOrdinal)
        assertFalse(error.retriable)
    }

    @Test
    fun nativeErrorJniOrdinalsPreserveStableValues() {
        assertEquals("invalidArgument", VesperPlayerErrorCode.InvalidArgument.wireName)
        assertEquals("audioOutput", VesperPlayerErrorCategory.AudioOutput.wireName)
        assertEquals(0, VesperPlayerErrorCode.InvalidArgument.jniOrdinal)
        assertEquals(11, VesperPlayerErrorCode.Timeout.jniOrdinal)
        assertEquals(0, VesperPlayerErrorCategory.Input.jniOrdinal)
        assertEquals(7, VesperPlayerErrorCategory.Platform.jniOrdinal)
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

internal fun contractText(name: String): String = contractFile(name).readText()

internal fun contractString(
    payload: String,
    key: String,
): String {
    val match = Regex("\"${Regex.escape(key)}\"\\s*:\\s*\"([^\"]*)\"").find(payload)
    assertNotNull("missing string key $key in contract fixture", match)
    return checkNotNull(match).groupValues[1]
}

private fun contractFile(name: String): File =
    listOf(
        File("fixtures/contracts/$name"),
        File("../fixtures/contracts/$name"),
        File("../../fixtures/contracts/$name"),
        File("../../../fixtures/contracts/$name"),
    ).firstOrNull { it.isFile }
        ?: error("contract fixture not found: $name")
