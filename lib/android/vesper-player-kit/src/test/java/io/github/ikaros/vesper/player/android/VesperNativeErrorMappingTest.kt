package io.github.ikaros.vesper.player.android

import androidx.media3.common.PlaybackException
import androidx.media3.common.C
import androidx.media3.common.Format
import androidx.media3.common.util.UnstableApi
import androidx.media3.exoplayer.ExoPlaybackException
import java.io.File
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertTrue
import org.junit.Test

class VesperNativeErrorMappingTest {
    @Test
    fun sharedSubtitleContractKeepsCanonicalStateAndErrorFields() {
        val state = contractText("subtitle_state.json")
        assertEquals(
            VesperSubtitleCatalogState.Ready,
            VesperSubtitleCatalogState.fromWire(contractString(state, "catalogState")),
        )
        assertEquals(
            VesperSubtitleSelectionState.Failed,
            VesperSubtitleSelectionState.fromWire(contractString(state, "selectionState")),
        )
        assertTrue(state.contains("\"advertisedTrackCount\": 3"))
        assertTrue(state.contains("\"selectableTrackCount\": 2"))
        assertTrue(state.contains("\"code\": \"subtitle_resource_failed\""))
        assertTrue(state.contains("\"trackId\": \"opaque-track-7\""))
        assertTrue(state.contains("\"commandId\": 42"))
        assertTrue(state.contains("\"sourceEpoch\": 9"))

        val error = contractText("subtitle_error.json")
        assertEquals("subtitle", contractString(error, "domain"))
        assertEquals("subtitle_selection_timeout", contractString(error, "code"))
        assertEquals(
            VesperSubtitleErrorPhase.Selection,
            VesperSubtitleErrorPhase.fromWire(contractString(error, "phase")),
        )
    }

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
    fun nativeSubtitleErrorPreservesTransactionIdentityAndRetryability() {
        val error =
            subtitleNativeError(
                code = "subtitle_selection_timeout",
                phase = "selection",
                trackId = "caption-en",
                retriable = true,
                commandId = 42,
                sourceEpoch = 9,
                message = "confirmation timed out",
            )

        assertEquals("subtitle", error.details["domain"])
        assertEquals("subtitle_selection_timeout", error.details["code"])
        assertEquals("selection", error.details["phase"])
        assertEquals("caption-en", error.details["trackId"])
        assertEquals(true, error.details["retriable"])
        assertEquals(42L, (error.details["commandId"] as Number).toLong())
        assertEquals(9L, (error.details["sourceEpoch"] as Number).toLong())
    }

    @Test
    fun nativeBridgeErrorDecodesSubtitleDetailsJsonAtTheJniBoundary() {
        val event =
            NativeBridgeEvent.Error(
                message = "selection timed out",
                codeOrdinal = VesperPlayerErrorCode.Timeout.jniOrdinal,
                categoryOrdinal = VesperPlayerErrorCategory.Playback.jniOrdinal,
                retriable = true,
                detailsJson =
                    """{"domain":"subtitle","code":"future_subtitle_code","phase":"future_phase","trackId":"opaque-track","retriable":true,"message":"selection timed out","commandId":42,"sourceEpoch":9}""",
            )

        val error = event.toPlayerErrorState()

        assertEquals("subtitle", error.details["domain"])
        assertEquals("future_subtitle_code", error.details["code"])
        assertEquals("future_phase", error.details["phase"])
        assertEquals("opaque-track", error.details["trackId"])
        assertEquals(42L, (error.details["commandId"] as Number).toLong())
        assertEquals(9L, (error.details["sourceEpoch"] as Number).toLong())
    }

    @Test
    fun nativeBridgeErrorPreservesMalformedDetailsJson() {
        val event =
            NativeBridgeEvent.Error(
                message = "malformed details",
                codeOrdinal = VesperPlayerErrorCode.BackendFailure.jniOrdinal,
                categoryOrdinal = VesperPlayerErrorCategory.Platform.jniOrdinal,
                retriable = false,
                detailsJson = "{not-json",
            )

        val error = event.toPlayerErrorState()

        assertEquals("{not-json", error.details["_rawDetailsJson"])
        assertEquals(true, error.details["_detailsJsonDecodeFailed"])
    }

    @Test
    fun nativeBridgeErrorPreservesBothUnknownOrdinals() {
        val event =
            NativeBridgeEvent.Error(
                message = "future enum values",
                codeOrdinal = 99,
                categoryOrdinal = 88,
                retriable = false,
                detailsJson = null,
            )

        val error = event.toPlayerErrorState()

        assertEquals(99, error.details["_rawCodeOrdinal"])
        assertEquals(88, error.details["_rawCategoryOrdinal"])
    }

    @Test
    fun sharedPluginDiagnosticsContractKeepsStableWireNames() {
        val payload = contractText("plugin_diagnostics.json")

        assertTrue(payload.contains("\"status\": \"decoderSupported\""))
        assertTrue(payload.contains("\"participation\": \"participated\""))
        assertTrue(payload.contains("\"pluginKind\": \"decoder\""))
        assertTrue(payload.contains("\"details\""))
        assertTrue(payload.contains("\"route\": \"sdkManagedNativeFrame\""))
        assertTrue(payload.contains("\"selectedVideoStreamIndex\": \"0\""))
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
    fun playbackExceptionDrmRuntimeErrorsMapToRetriableNetworkBackendFailure() {
        listOf(
            PlaybackException.ERROR_CODE_DRM_PROVISIONING_FAILED,
            PlaybackException.ERROR_CODE_DRM_LICENSE_ACQUISITION_FAILED,
            PlaybackException.ERROR_CODE_DRM_SYSTEM_ERROR,
            PlaybackException.ERROR_CODE_DRM_LICENSE_EXPIRED,
            PlaybackException.ERROR_CODE_DRM_UNSPECIFIED,
            PlaybackException.ERROR_CODE_DRM_CONTENT_ERROR,
        ).forEach { errorCode ->
            val error =
                classifyPlaybackException(
                    playbackException(errorCode)
                )

            assertEquals(VesperPlayerErrorCode.BackendFailure, VesperPlayerErrorCode.fromJniOrdinal(error.codeOrdinal))
            assertEquals(VesperPlayerErrorCategory.Network, VesperPlayerErrorCategory.fromJniOrdinal(error.categoryOrdinal))
            assertEquals(BACKEND_FAILURE_ORDINAL, error.codeOrdinal)
            assertEquals(NETWORK_CATEGORY_ORDINAL, error.categoryOrdinal)
            assertTrue(error.retriable)
            assertFalse(error.likelyCapabilityIssue)
        }
    }

    @Test
    fun playbackExceptionDrmCapabilityErrorsMapToUnsupported() {
        listOf(
            PlaybackException.ERROR_CODE_DRM_SCHEME_UNSUPPORTED,
            PlaybackException.ERROR_CODE_DRM_DISALLOWED_OPERATION,
            PlaybackException.ERROR_CODE_DRM_DEVICE_REVOKED,
        ).forEach { errorCode ->
            val error =
                classifyPlaybackException(
                    playbackException(errorCode)
                )

            assertEquals(VesperPlayerErrorCode.Unsupported, VesperPlayerErrorCode.fromJniOrdinal(error.codeOrdinal))
            assertEquals(VesperPlayerErrorCategory.Capability, VesperPlayerErrorCategory.fromJniOrdinal(error.categoryOrdinal))
            assertEquals(UNSUPPORTED_ORDINAL, error.codeOrdinal)
            assertEquals(CAPABILITY_CATEGORY_ORDINAL, error.categoryOrdinal)
            assertFalse(error.retriable)
            assertTrue(error.likelyCapabilityIssue)
        }
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
        assertTrue(error.likelyCapabilityIssue)
        assertEquals(AndroidCapabilityFailureCause.DecodeFailed, error.capabilityFailureCause)
    }

    @Test
    fun playbackExceptionUnsupportedErrorsAreLikelyCapabilityIssues() {
        val error =
            classifyPlaybackException(
                playbackException(PlaybackException.ERROR_CODE_PARSING_CONTAINER_UNSUPPORTED)
            )

        assertTrue(error.likelyCapabilityIssue)
    }

    @Test
    fun playbackExceptionCapabilityErrorsExposeFailureCause() {
        val container =
            classifyPlaybackException(
                playbackException(PlaybackException.ERROR_CODE_PARSING_CONTAINER_UNSUPPORTED)
            )
        val manifest =
            classifyPlaybackException(
                playbackException(PlaybackException.ERROR_CODE_PARSING_MANIFEST_UNSUPPORTED)
            )
        val decoderInit =
            classifyPlaybackException(
                playbackException(PlaybackException.ERROR_CODE_DECODER_INIT_FAILED)
            )
        val decoderQuery =
            classifyPlaybackException(
                playbackException(PlaybackException.ERROR_CODE_DECODER_QUERY_FAILED)
            )

        assertEquals(AndroidCapabilityFailureCause.ContainerUnsupported, container.capabilityFailureCause)
        assertEquals(AndroidCapabilityFailureCause.ManifestUnsupported, manifest.capabilityFailureCause)
        assertEquals(AndroidCapabilityFailureCause.DecoderInit, decoderInit.capabilityFailureCause)
        assertEquals(AndroidCapabilityFailureCause.DecoderQuery, decoderQuery.capabilityFailureCause)
        assertEquals(AndroidCapabilityFailureAxis.Container, container.capabilityFailureAxis)
        assertEquals(AndroidCapabilityFailureAxis.Manifest, manifest.capabilityFailureAxis)
        assertEquals(AndroidCapabilityFailureAxis.Decoder, decoderInit.capabilityFailureAxis)
        assertEquals(AndroidCapabilityFailureAxis.Decoder, decoderQuery.capabilityFailureAxis)
    }

    @Test
    fun playbackExceptionClassificationCarriesBoundedCauseEvidence() {
        val rootMessage = "root decoder detail"
        val directMessage = "x".repeat(300)
        val error =
            classifyPlaybackException(
                playbackException(
                    PlaybackException.ERROR_CODE_DECODER_INIT_FAILED,
                    IllegalStateException(
                        directMessage,
                        IllegalArgumentException(rootMessage),
                    ),
                )
            )

        val causeEvidence = checkNotNull(error.causeEvidence)

        assertEquals(IllegalStateException::class.java.name, causeEvidence.causeClass)
        assertEquals(256, causeEvidence.causeMessage?.length)
        assertTrue(causeEvidence.causeMessage?.endsWith("...") ?: false)
        assertEquals(IllegalArgumentException::class.java.name, causeEvidence.rootCauseClass)
        assertEquals(rootMessage, causeEvidence.rootCauseMessage)
    }

    @Test
    fun playbackExceptionClassificationRefinesRuntimeFailureAxisFromCauseEvidence() {
        val surface =
            classifyPlaybackException(
                playbackException(
                    PlaybackException.ERROR_CODE_DECODING_FAILED,
                    IllegalStateException(
                        "decoder failed",
                        IllegalArgumentException("surface rejected frame"),
                    ),
                )
            )
        val renderer =
            classifyPlaybackException(
                playbackException(
                    PlaybackException.ERROR_CODE_DECODING_FAILED,
                    IllegalStateException("MediaCodecVideoRenderer failed"),
                )
            )

        assertEquals(AndroidCapabilityFailureCause.DecodeFailed, surface.capabilityFailureCause)
        assertEquals(AndroidCapabilityFailureAxis.DisplaySurface, surface.capabilityFailureAxis)
        assertEquals(AndroidCapabilityFailureCause.DecodeFailed, renderer.capabilityFailureCause)
        assertEquals(AndroidCapabilityFailureAxis.Renderer, renderer.capabilityFailureAxis)
    }

    @androidx.annotation.OptIn(UnstableApi::class)
    @Test
    fun exoRendererExceptionCarriesRendererContextAndRefinesFailureAxis() {
        val error =
            classifyPlaybackException(
                ExoPlaybackException.createForRenderer(
                    IllegalStateException("renderer failed"),
                    "MediaCodecVideoRenderer",
                    2,
                    Format.Builder()
                        .setSampleMimeType("video/dolby-vision")
                        .setCodecs("dvh1.08.06")
                        .setWidth(3840)
                        .setHeight(2160)
                        .setFrameRate(60f)
                        .build(),
                    C.FORMAT_HANDLED,
                    null,
                    false,
                    PlaybackException.ERROR_CODE_DECODING_FAILED,
                )
            )

        val diagnostics = checkNotNull(error.causeEvidence).diagnostics()

        assertEquals(AndroidCapabilityFailureCause.DecodeFailed, error.capabilityFailureCause)
        assertEquals(AndroidCapabilityFailureAxis.Renderer, error.capabilityFailureAxis)
        assertEquals("MediaCodecVideoRenderer", diagnostics["playbackFailureRendererName"])
        assertEquals("2", diagnostics["playbackFailureRendererIndex"])
        assertEquals("handled", diagnostics["playbackFailureRendererFormatSupport"])
        assertEquals("video/dolby-vision", diagnostics["playbackFailureRendererFormatSampleMimeType"])
        assertEquals("dvh1.08.06", diagnostics["playbackFailureRendererFormatCodecs"])
        assertEquals("3840", diagnostics["playbackFailureRendererFormatWidth"])
        assertEquals("2160", diagnostics["playbackFailureRendererFormatHeight"])
        assertEquals("60.0", diagnostics["playbackFailureRendererFormatFrameRate"])
    }

    @androidx.annotation.OptIn(UnstableApi::class)
    @Test
    fun exoRendererExceptionWithUnsupportedFormatRefinesFailureAxisToDecoder() {
        val error =
            classifyPlaybackException(
                ExoPlaybackException.createForRenderer(
                    IllegalStateException("format exceeds renderer capabilities"),
                    "MediaCodecVideoRenderer",
                    1,
                    Format.Builder()
                        .setSampleMimeType("video/hevc")
                        .setWidth(3840)
                        .setHeight(2160)
                        .setFrameRate(120f)
                        .build(),
                    C.FORMAT_EXCEEDS_CAPABILITIES,
                    null,
                    false,
                    PlaybackException.ERROR_CODE_DECODING_FAILED,
                )
            )

        val diagnostics = checkNotNull(error.causeEvidence).diagnostics()

        assertEquals(AndroidCapabilityFailureCause.DecodeFailed, error.capabilityFailureCause)
        assertEquals(AndroidCapabilityFailureAxis.Decoder, error.capabilityFailureAxis)
        assertEquals("exceedsCapabilities", diagnostics["playbackFailureRendererFormatSupport"])
        assertEquals("120.0", diagnostics["playbackFailureRendererFormatFrameRate"])
    }

    @Test
    fun playbackExceptionNetworkAndSourceErrorsDoNotExposeCapabilityFailureCause() {
        val network =
            classifyPlaybackException(
                playbackException(PlaybackException.ERROR_CODE_IO_NETWORK_CONNECTION_TIMEOUT)
            )
        val source =
            classifyPlaybackException(
                playbackException(PlaybackException.ERROR_CODE_IO_FILE_NOT_FOUND)
            )

        assertEquals(null, network.capabilityFailureCause)
        assertEquals(null, source.capabilityFailureCause)
        assertEquals(null, network.capabilityFailureAxis)
        assertEquals(null, source.capabilityFailureAxis)
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
        assertFalse(error.likelyCapabilityIssue)
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

    private fun playbackException(
        errorCode: Int,
        cause: Throwable? = null,
    ): PlaybackException =
        PlaybackException("playback failed", cause, errorCode)
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
