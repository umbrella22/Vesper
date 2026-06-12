package io.github.ikaros.vesper.player.android

import android.view.Surface
import androidx.media3.common.C
import androidx.media3.common.ColorInfo
import androidx.media3.common.Format
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class VesperNativePlayerBridgeTest {
    @Test
    fun benchmarkRecorderDefaultsDisabled() {
        val bridge = VesperNativePlayerBridge(bindings = FakeBindings())

        bridge.initialize()
        bridge.play()

        assertTrue(bridge.drainBenchmarkEvents().isEmpty())
        assertEquals(0L, bridge.benchmarkSummary().acceptedEvents)
    }

    @Test
    fun benchmarkRecorderDrainsRawEventsAndKeepsSummary() {
        val bridge =
            VesperNativePlayerBridge(
                bindings = FakeBindings(),
                benchmarkRecorder =
                    VesperBenchmarkRecorder(
                        VesperBenchmarkConfiguration(enabled = true),
                    ),
            )

        bridge.initialize()
        bridge.play()

        val events = bridge.drainBenchmarkEvents()
        val eventNames = events.map { it.eventName }.toSet()
        assertTrue(eventNames.contains("initialize_start"))
        assertTrue(eventNames.contains("initialize_without_source"))
        assertTrue(eventNames.contains("play_command"))
        assertTrue(bridge.drainBenchmarkEvents().isEmpty())
        assertEquals(events.size.toLong(), bridge.benchmarkSummary().acceptedEvents)
    }

    @Test
    fun refreshDrainsNativeRuntimeWarningsOnce() {
        val bindings = FakeBindings()
        val bridge = VesperNativePlayerBridge(bindings = bindings)
        bindings.events +=
            NativeBridgeEvent.Warning(
                VesperRuntimeWarning(
                    domain = "capability",
                    payload =
                        mapOf(
                            "reason" to "hdrNativeFrameUnsupported",
                            "recommendedPlaybackPath" to "systemPlayer",
                            "hdrKind" to "dolbyVision",
                        ),
                ),
            )

        bridge.refresh()

        val warnings = bridge.drainRuntimeWarnings()
        assertEquals(1, warnings.size)
        assertEquals("capability", warnings.single().domain)
        assertEquals("hdrNativeFrameUnsupported", warnings.single().payload["reason"])
        assertEquals("systemPlayer", warnings.single().payload["recommendedPlaybackPath"])
        assertEquals("dolbyVision", warnings.single().payload["hdrKind"])
        assertTrue(bridge.drainRuntimeWarnings().isEmpty())
    }

    @Test
    fun runtimeHdrEvidenceIncludesFormatColorMetadataAndStaticInfo() {
        val hdrStaticInfo =
            ByteArray(25).apply {
                this[21] = 0x03.toByte()
                this[22] = 0xE8.toByte()
                this[23] = 0x01.toByte()
                this[24] = 0x90.toByte()
            }
        val evidence =
            Format.Builder()
                .setCodecs("hvc1.2.4.L153.B0")
                .setSampleMimeType("video/hevc")
                .setWidth(3840)
                .setHeight(2160)
                .setFrameRate(59.94f)
                .setColorInfo(
                    ColorInfo.Builder()
                        .setColorSpace(C.COLOR_SPACE_BT2020)
                        .setColorRange(C.COLOR_RANGE_LIMITED)
                        .setColorTransfer(C.COLOR_TRANSFER_ST2084)
                        .setHdrStaticInfo(hdrStaticInfo)
                        .setLumaBitdepth(10)
                        .setChromaBitdepth(10)
                        .build()
                )
                .build()
                .androidRuntimeHdrEvidence()

        assertNotNull(evidence)
        val diagnostics = checkNotNull(evidence).diagnostics
        assertEquals("hdr10", evidence.hdrKind)
        assertEquals("media3FormatColorInfo", diagnostics["runtimeFormatHdrMetadataProbe"])
        assertEquals("hvc1.2.4.L153.B0", diagnostics["runtimeFormatCodecs"])
        assertEquals("video/hevc", diagnostics["runtimeFormatSampleMimeType"])
        assertEquals("3840", diagnostics["runtimeFormatWidth"])
        assertEquals("2160", diagnostics["runtimeFormatHeight"])
        assertEquals("bt2020", diagnostics["runtimeFormatColorSpace"])
        assertEquals("limited", diagnostics["runtimeFormatColorRange"])
        assertEquals("st2084", diagnostics["runtimeFormatColorTransfer"])
        assertEquals("10", diagnostics["runtimeFormatLumaBitDepth"])
        assertEquals("10", diagnostics["runtimeFormatChromaBitDepth"])
        assertEquals("true", diagnostics["runtimeFormatHdrStaticInfoPresent"])
        assertEquals("25", diagnostics["runtimeFormatHdrStaticInfoByteLength"])
        assertEquals("1000", diagnostics["runtimeFormatMaxContentLightLevelNits"])
        assertEquals("400", diagnostics["runtimeFormatMaxFrameAverageLightLevelNits"])

        val metadata = evidence.metadata
        assertEquals(10, metadata?.lumaBitDepth)
        assertEquals(10, metadata?.chromaBitDepth)
        assertEquals(true, metadata?.hdrStaticInfoPresent)
        assertEquals(25, metadata?.hdrStaticInfoByteLength)
        assertEquals(1000, metadata?.maxContentLightLevelNits)
        assertEquals(400, metadata?.maxFrameAverageLightLevelNits)
    }

    @Test
    fun runtimeHdrEvidenceRecognizesHlgAndDolbyVisionWithoutStaticInfo() {
        val hlgEvidence =
            Format.Builder()
                .setColorInfo(
                    ColorInfo.Builder()
                        .setColorSpace(C.COLOR_SPACE_BT2020)
                        .setColorRange(C.COLOR_RANGE_LIMITED)
                        .setColorTransfer(C.COLOR_TRANSFER_HLG)
                        .build()
                )
                .build()
                .androidRuntimeHdrEvidence()

        assertEquals("hlg", checkNotNull(hlgEvidence).hdrKind)
        assertEquals("hlg", hlgEvidence.diagnostics["runtimeFormatColorTransfer"])
        assertFalse(hlgEvidence.diagnostics.containsKey("runtimeFormatHdrStaticInfoPresent"))

        val dolbyVisionEvidence =
            Format.Builder()
                .setCodecs("dvhe.08.07")
                .build()
                .androidRuntimeHdrEvidence()

        assertEquals("dolbyVision", checkNotNull(dolbyVisionEvidence).hdrKind)
        assertEquals("dvhe.08.07", dolbyVisionEvidence.diagnostics["runtimeFormatCodecs"])
    }

    @Test
    fun runtimeDolbyVisionEvidencePayloadIncludesTypedMetadata() {
        val evidence =
            Format.Builder()
                .setCodecs("dvhe.08.07")
                .setSampleMimeType("video/dolby-vision")
                .setColorInfo(
                    ColorInfo.Builder()
                        .setColorSpace(C.COLOR_SPACE_BT2020)
                        .setColorRange(C.COLOR_RANGE_LIMITED)
                        .setColorTransfer(C.COLOR_TRANSFER_ST2084)
                        .build()
                )
                .build()
                .androidRuntimeHdrEvidence()

        val warningPayload = checkNotNull(evidence).capabilityWarningPayload()
        val metadata = warningPayload["hdrMetadata"] as? Map<*, *>

        assertEquals("hdrNativeFrameUnsupported", warningPayload["reason"])
        assertEquals("systemPlayer", warningPayload["recommendedPlaybackPath"])
        assertEquals("dolbyVision", warningPayload["hdrKind"])
        assertEquals("media3FormatColorInfo", evidence.metadata?.probe)
        assertEquals("dolbyVision", metadata?.get("hdrKind"))
        assertEquals("compatibleBaseLayer", metadata?.get("dolbyVisionMode"))
        assertEquals("dvhe.08.07", metadata?.get("dolbyVisionCodec"))
        assertEquals(8, metadata?.get("dolbyVisionProfile"))
        assertEquals(7, metadata?.get("dolbyVisionLevel"))
        assertEquals("profile8Hdr10BaseLayer", metadata?.get("dolbyVisionCompatibility"))
        assertEquals("profile8SingleLayerCompatible", metadata?.get("dolbyVisionProfileFamily"))
        assertEquals("hdr10BaseLayer", metadata?.get("dolbyVisionBaseLayer"))
        assertEquals("hdr10BaseLayerSystemPlayer", metadata?.get("dolbyVisionFallbackTarget"))
        assertEquals("runtimeFormatColorTransfer", metadata?.get("dolbyVisionBaseLayerEvidence"))
        assertEquals("st2084", metadata?.get("dolbyVisionBaseLayerTransferFunction"))
        assertEquals("runtimeFormatColorTransfer", warningPayload["dolbyVisionBaseLayerEvidence"])
        assertEquals("st2084", warningPayload["dolbyVisionBaseLayerTransferFunction"])
        assertEquals("profile8Hdr10BaseLayer", warningPayload["dolbyVisionCompatibility"])
    }

    @Test
    fun runtimeHdrFailurePayloadIncludesTypedEvidenceAndErrorCode() {
        val evidence =
            Format.Builder()
                .setColorInfo(
                    ColorInfo.Builder()
                        .setColorSpace(C.COLOR_SPACE_BT2020)
                        .setColorRange(C.COLOR_RANGE_LIMITED)
                        .setColorTransfer(C.COLOR_TRANSFER_HLG)
                        .build()
                )
                .build()
                .androidRuntimeHdrEvidence()

        val payload =
            checkNotNull(evidence).failureHintPayload(
                "ERROR_CODE_DECODING_FAILED",
                NativePlaybackError(
                    codeOrdinal = DECODE_FAILURE_ORDINAL,
                    categoryOrdinal = DECODE_CATEGORY_ORDINAL,
                    retriable = false,
                    likelyCapabilityIssue = true,
                    capabilityFailureCause = AndroidCapabilityFailureCause.DecodeFailed,
                    capabilityFailureAxis = AndroidCapabilityFailureAxis.DisplaySurface,
                    causeEvidence =
                        AndroidPlaybackFailureCauseEvidence(
                            causeClass = "android.media.MediaCodec.CodecException",
                            causeMessage = "codec init failed",
                            rootCauseClass = "java.lang.IllegalStateException",
                            rootCauseMessage = "surface rejected",
                        ),
                ),
            )
        val metadata = payload["hdrMetadata"] as? Map<*, *>

        assertEquals(true, payload["likelyHdrCapabilityIssue"])
        assertEquals("sessionProbe", payload["confidence"])
        assertEquals("ERROR_CODE_DECODING_FAILED", payload["errorCode"])
        assertEquals("decodeFailed", payload["capabilityFailureCause"])
        assertEquals("displaySurface", payload["capabilityFailureAxis"])
        assertEquals("android.media.MediaCodec.CodecException", payload["playbackFailureCauseClass"])
        assertEquals("codec init failed", payload["playbackFailureCauseMessage"])
        assertEquals("java.lang.IllegalStateException", payload["playbackFailureRootCauseClass"])
        assertEquals("surface rejected", payload["playbackFailureRootCauseMessage"])
        assertEquals("hlg", payload["hdrKind"])
        assertEquals("hlg", metadata?.get("hdrKind"))
        assertEquals("hlg", metadata?.get("transferFunction"))
        assertEquals("bt2020", metadata?.get("colorSpace"))
        assertEquals("hlg", payload["runtimeFormatColorTransfer"])
    }

    @Test
    fun runtimeHdrFailurePayloadIncludesRendererRuntimeConvergenceDiagnostics() {
        val evidence =
            Format.Builder()
                .setSampleMimeType("video/dolby-vision")
                .setCodecs("dvh1.08.06")
                .setWidth(3840)
                .setHeight(2160)
                .setFrameRate(59.94f)
                .setColorInfo(
                    ColorInfo.Builder()
                        .setColorSpace(C.COLOR_SPACE_BT2020)
                        .setColorRange(C.COLOR_RANGE_LIMITED)
                        .setColorTransfer(C.COLOR_TRANSFER_ST2084)
                        .build()
                )
                .build()
                .androidRuntimeHdrEvidence()

        val payload =
            checkNotNull(evidence).failureHintPayload(
                "ERROR_CODE_DECODING_FAILED",
                NativePlaybackError(
                    codeOrdinal = DECODE_FAILURE_ORDINAL,
                    categoryOrdinal = DECODE_CATEGORY_ORDINAL,
                    retriable = false,
                    likelyCapabilityIssue = true,
                    capabilityFailureCause = AndroidCapabilityFailureCause.DecodeFailed,
                    capabilityFailureAxis = AndroidCapabilityFailureAxis.Renderer,
                    causeEvidence =
                        AndroidPlaybackFailureCauseEvidence(
                            causeClass = "androidx.media3.exoplayer.video.MediaCodecVideoRenderer",
                            causeMessage = "renderer failed",
                            rootCauseClass = null,
                            rootCauseMessage = null,
                            rendererName = "MediaCodecVideoRenderer",
                            rendererIndex = 0,
                            rendererFormatSupport = "handled",
                            rendererFormatSampleMimeType = "video/dolby-vision",
                            rendererFormatCodecs = "dvh1.08.06",
                            rendererFormatWidth = 3840,
                            rendererFormatHeight = 2160,
                            rendererFormatFrameRate = 59.94f,
                        ),
                ),
            )

        assertEquals("renderer", payload["capabilityFailureAxis"])
        assertEquals("MediaCodecVideoRenderer", payload["playbackFailureRendererName"])
        assertEquals("handled", payload["playbackFailureRendererFormatSupport"])
        assertEquals("true", payload["playbackFailureRendererFormatSupported"])
        assertEquals("true", payload["playbackFailureRendererFormatMimeMatchesRuntime"])
        assertEquals("true", payload["playbackFailureRendererFormatCodecsMatchRuntime"])
        assertEquals("true", payload["playbackFailureRendererFormatSizeMatchesRuntime"])
        assertEquals("true", payload["playbackFailureRendererFormatFrameRateMatchesRuntime"])
    }

    @Test
    fun runtimeHdrFailurePayloadIncludesSessionProbeRuntimeConvergenceDiagnostics() {
        val evidence =
            Format.Builder()
                .setSampleMimeType("video/dolby-vision")
                .setCodecs("dvh1.08.06")
                .setWidth(3840)
                .setHeight(2160)
                .setFrameRate(59.94f)
                .setColorInfo(
                    ColorInfo.Builder()
                        .setColorSpace(C.COLOR_SPACE_BT2020)
                        .setColorRange(C.COLOR_RANGE_LIMITED)
                        .setColorTransfer(C.COLOR_TRANSFER_ST2084)
                        .build()
                )
                .build()
                .androidRuntimeHdrEvidence()
        val sessionProbe =
            AndroidRuntimeSessionProbeSnapshot(
                VesperPlaybackCapabilityProbeResult(
                    status = VesperPlaybackCapabilityProbeStatus.FallbackRequired,
                    codecFamily = VesperPlaybackCodecFamily.Hevc,
                    systemPlaybackSupported = true,
                    hardwareDecodeSupported = true,
                    sdkManagedNativeFrameSupported = false,
                    recommendedPlaybackPath = VesperRecommendedPlaybackPath.SystemPlayer,
                    outputFormat = VesperPlaybackCapabilityOutputFormat.SurfaceOpaque,
                    hdrKind = VesperPlaybackCapabilityHdrKind.DolbyVision,
                    dolbyVisionMode = VesperPlaybackCapabilityDolbyVisionMode.CompatibleBaseLayer,
                    confidence = VesperPlaybackCapabilityConfidence.SessionProbe,
                    missingCapabilities = listOf("hdrProgrammableProcessingNotSupported"),
                    diagnostics =
                        mapOf(
                            "codecFormatSupported" to "true",
                            "codecFormatSampleMimeType" to "video/dolby-vision",
                            "codecFormatCodecs" to "dvh1.08.06",
                            "codecFormatWidth" to "3840",
                            "codecFormatHeight" to "2160",
                            "codecFormatFrameRate" to "59.94",
                            "displayHdrSupported" to "true",
                            "displayFrameRateSupported" to "true",
                        ),
                )
            )

        val payload =
            checkNotNull(evidence).failureHintPayload(
                "ERROR_CODE_DECODING_FAILED",
                NativePlaybackError(
                    codeOrdinal = DECODE_FAILURE_ORDINAL,
                    categoryOrdinal = DECODE_CATEGORY_ORDINAL,
                    retriable = false,
                    likelyCapabilityIssue = true,
                    capabilityFailureCause = AndroidCapabilityFailureCause.DecodeFailed,
                    capabilityFailureAxis = AndroidCapabilityFailureAxis.Renderer,
                ),
                sessionProbe,
            )

        assertEquals("fallbackRequired", payload["runtimeSessionProbeStatus"])
        assertEquals("systemPlayer", payload["runtimeSessionProbeRecommendedPlaybackPath"])
        assertEquals("sessionProbe", payload["runtimeSessionProbeConfidence"])
        assertEquals("dolbyVision", payload["runtimeSessionProbeHdrKind"])
        assertEquals("compatibleBaseLayer", payload["runtimeSessionProbeDolbyVisionMode"])
        assertEquals(
            "hdrProgrammableProcessingNotSupported",
            payload["runtimeSessionProbeMissingCapabilities"],
        )
        assertEquals("true", payload["runtimeSessionProbeCodecFormatSupported"])
        assertEquals("video/dolby-vision", payload["runtimeSessionProbeCodecFormatSampleMimeType"])
        assertEquals("dvh1.08.06", payload["runtimeSessionProbeCodecFormatCodecs"])
        assertEquals("3840", payload["runtimeSessionProbeCodecFormatWidth"])
        assertEquals("2160", payload["runtimeSessionProbeCodecFormatHeight"])
        assertEquals("59.94", payload["runtimeSessionProbeCodecFormatFrameRate"])
        assertEquals("true", payload["runtimeSessionProbeDisplayHdrSupported"])
        assertEquals("true", payload["runtimeSessionProbeDisplayFrameRateSupported"])
        assertEquals("true", payload["runtimeSessionProbeCodecFormatMimeMatchesRuntime"])
        assertEquals("true", payload["runtimeSessionProbeCodecFormatCodecsMatchRuntime"])
        assertEquals("true", payload["runtimeSessionProbeCodecFormatSizeMatchesRuntime"])
        assertEquals("true", payload["runtimeSessionProbeCodecFormatFrameRateMatchesRuntime"])
    }

    @Test
    fun runtimeHdrEvidenceIgnoresSdrColorTransfer() {
        val evidence =
            Format.Builder()
                .setColorInfo(
                    ColorInfo.Builder()
                        .setColorSpace(C.COLOR_SPACE_BT709)
                        .setColorRange(C.COLOR_RANGE_LIMITED)
                        .setColorTransfer(C.COLOR_TRANSFER_SDR)
                        .build()
                )
                .build()
                .androidRuntimeHdrEvidence()

        assertNull(evidence)
    }

    @Test
    fun surfaceHostAspectFitSizeDoesNotCropWideVideo() {
        val size =
            calculateAspectFitSize(
                containerWidth = 400,
                containerHeight = 300,
                videoWidth = 1920,
                videoHeight = 1080,
            )

        assertEquals(AspectFitSize(width = 400, height = 225), size)
    }

    @Test
    fun surfaceHostAspectFitSizeDoesNotCropPortraitVideo() {
        val size =
            calculateAspectFitSize(
                containerWidth = 400,
                containerHeight = 300,
                videoWidth = 1080,
                videoHeight = 1920,
            )

        assertEquals(AspectFitSize(width = 168, height = 300), size)
    }

    @Test
    fun surfaceHostAspectFitScaleKeepsTextureViewInsideContainer() {
        val wideScale =
            calculateAspectFitScale(
                containerWidth = 400f,
                containerHeight = 300f,
                videoWidth = 1920,
                videoHeight = 1080,
            )
        val portraitScale =
            calculateAspectFitScale(
                containerWidth = 400f,
                containerHeight = 300f,
                videoWidth = 1080,
                videoHeight = 1920,
            )

        assertEquals(1.0f, wideScale?.scaleX)
        assertEquals(0.75f, wideScale?.scaleY)
        assertEquals(0.421875f, portraitScale?.scaleX)
        assertEquals(1.0f, portraitScale?.scaleY)
    }

    @Test
    fun surfaceHostAspectFitRejectsInvalidDimensions() {
        assertNull(
            calculateAspectFitSize(
                containerWidth = 0,
                containerHeight = 300,
                videoWidth = 1920,
                videoHeight = 1080,
            )
        )
        assertNull(
            calculateAspectFitScale(
                containerWidth = 400f,
                containerHeight = 300f,
                videoWidth = 1920,
                videoHeight = 0,
            )
        )
    }

    @Test
    fun refreshMirrorsEffectiveVideoTrackIdFromBindings() {
        val bindings =
            FakeBindings(
                trackCatalog =
                    VesperTrackCatalog(
                        tracks =
                            listOf(
                                VesperMediaTrack(
                                    id = "video:720p",
                                    kind = VesperMediaTrackKind.Video,
                                    height = 720,
                                    bitRate = 1_500_000L,
                                )
                            )
                ),
                trackSelection = VesperTrackSelectionSnapshot(abrPolicy = VesperAbrPolicy.auto()),
                effectiveVideoTrackId = "video:720p",
                videoVariantObservation =
                    VesperVideoVariantObservation(
                        bitRate = 1_500_000L,
                        width = 1280,
                        height = 720,
                    ),
            )
        val bridge = VesperNativePlayerBridge(bindings = bindings)

        bridge.refresh()
        assertEquals("video:720p", bridge.effectiveVideoTrackId.value)
        assertEquals(
            VesperVideoVariantObservation(
                bitRate = 1_500_000L,
                width = 1280,
                height = 720,
            ),
            bridge.videoVariantObservation.value,
        )

        bindings.effectiveVideoTrackId = null
        bindings.videoVariantObservation = null
        bridge.refresh()
        assertNull(bridge.effectiveVideoTrackId.value)
        assertNull(bridge.videoVariantObservation.value)
    }

    @Test
    fun selectSourceClearsStaleEffectiveVideoTrackIdUntilBindingsPublishNewState() {
        val bindings =
            FakeBindings(
                trackCatalog =
                    VesperTrackCatalog(
                        tracks =
                            listOf(
                                VesperMediaTrack(
                                    id = "video:old",
                                    kind = VesperMediaTrackKind.Video,
                                    height = 720,
                                    bitRate = 1_500_000L,
                                )
                            )
                    ),
                trackSelection =
                    VesperTrackSelectionSnapshot(
                        abrPolicy = VesperAbrPolicy.fixedTrack("video:old"),
                    ),
                effectiveVideoTrackId = "video:old",
                videoVariantObservation =
                    VesperVideoVariantObservation(
                        bitRate = 1_500_000L,
                        width = 1280,
                        height = 720,
                    ),
            )
        val bridge = VesperNativePlayerBridge(bindings = bindings)

        bridge.refresh()
        assertEquals("video:old", bridge.effectiveVideoTrackId.value)
        assertEquals(1_500_000L, bridge.videoVariantObservation.value?.bitRate)

        bindings.onInitialize = {
            bindings.trackCatalog = VesperTrackCatalog.Empty
            bindings.trackSelection = VesperTrackSelectionSnapshot()
            bindings.effectiveVideoTrackId = null
            bindings.videoVariantObservation = null
        }

        bridge.selectSource(VesperPlayerSource.hls("https://example.com/next.m3u8", "Next"))
        assertNull(bridge.effectiveVideoTrackId.value)
        assertNull(bridge.videoVariantObservation.value)

        bindings.trackCatalog =
            VesperTrackCatalog(
                tracks =
                    listOf(
                        VesperMediaTrack(
                            id = "video:new",
                            kind = VesperMediaTrackKind.Video,
                            height = 1080,
                            bitRate = 3_000_000L,
                        )
                    )
            )
        bindings.trackSelection = VesperTrackSelectionSnapshot(abrPolicy = VesperAbrPolicy.auto())
        bindings.effectiveVideoTrackId = "video:new"
        bindings.videoVariantObservation =
            VesperVideoVariantObservation(
                bitRate = 3_000_000L,
                width = 1920,
                height = 1080,
            )

        bridge.refresh()
        assertEquals("video:new", bridge.effectiveVideoTrackId.value)
        assertEquals(1920, bridge.videoVariantObservation.value?.width)
    }

    @Test
    fun mobilePluginProbeExposesDiagnosticsWithoutReplacingPlaybackSource() {
        val initialSource = VesperPlayerSource.hls("https://example.com/live.m3u8", "Live")
        val diagnostics =
            listOf(
                mapOf(
                    "pluginKind" to "source_normalizer",
                    "status" to "sourceNormalizerSupported",
                    "participation" to "available",
                )
            )
        val bindings =
            FakeBindings(
                mobilePluginDiagnostics = diagnostics,
            )
        val bridge =
            VesperNativePlayerBridge(
                bindings = bindings,
                initialSource = initialSource,
                sourceNormalizerConfiguration =
                    VesperSourceNormalizerConfiguration(
                        mode = VesperSourceNormalizerMode.PreflightOnly,
                        pluginLibraryPaths = listOf("/tmp/libvesper_source_normalizer_ffmpeg.so"),
                        runtimeProfile = "default",
                    ),
                frameProcessorConfiguration =
                    VesperFrameProcessorConfiguration(
                        mode = VesperFrameProcessorMode.DiagnosticsOnly,
                        pluginLibraryPaths = listOf("/tmp/libvesper_frame_processor_diagnostic.so"),
                    ),
            )

        bridge.initialize()

        assertEquals(initialSource, bindings.lastProbeSource)
        assertEquals(initialSource, bindings.lastInitializedSource)
        assertEquals(diagnostics, bridge.pluginDiagnostics)
        assertEquals(
            VesperSourceNormalizerMode.PreflightOnly,
            bindings.lastSourceNormalizerConfiguration?.mode,
        )
        assertEquals(
            VesperFrameProcessorMode.DiagnosticsOnly,
            bindings.lastFrameProcessorConfiguration?.mode,
        )
    }

    @Test
    fun nativeFramePipelineConfigurationAddsDiagnosticsWithoutReplacingPlaybackSource() {
        val initialSource =
            VesperPlayerSource.remote(
                uri = "https://example.com/video.mp4",
                label = "MP4",
                protocol = VesperPlayerSourceProtocol.Progressive,
            )
        val bindings = FakeBindings()
        val bridge =
            VesperNativePlayerBridge(
                bindings = bindings,
                initialSource = initialSource,
                nativeFramePipelineConfiguration =
                    VesperNativeFramePipelineConfiguration(
                        mode = VesperNativeFramePipelineMode.PreferNativeFrame,
                        decoderPluginLibraryPaths = listOf("/tmp/libdecoder.so"),
                        frameProcessorPluginLibraryPaths = listOf("/tmp/libframe.so"),
                        maxInFlightFrames = 2,
                    ),
            )

        bridge.initialize()

        assertEquals(initialSource, bindings.lastInitializedSource)
        assertTrue(
            bridge.pluginDiagnostics.any {
                it["pluginKind"] == "native_frame_pipeline" &&
                    it["participation"] == "fallback" &&
                    it["route"] == "systemPlayer" &&
                    it["fallbackTargetRoute"] == "systemPlayer" &&
                    it["fallbackReason"].toString().contains("SourceNormalizer packet-stream")
            }
        )
    }

    @Test
    fun nativeFramePipelineDiagnosticsUseSdkManagedRouteForRunnableAndroidContract() {
        val initialSource =
            VesperPlayerSource.remote(
                uri = "https://example.com/video.mp4",
                label = "MP4",
                protocol = VesperPlayerSourceProtocol.Progressive,
            )
        val bridge =
            VesperNativePlayerBridge(
                bindings = FakeBindings(),
                initialSource = initialSource,
                sourceNormalizerConfiguration =
                    VesperSourceNormalizerConfiguration(
                        mode = VesperSourceNormalizerMode.PreflightOnly,
                        pluginLibraryPaths = listOf("/tmp/libsource_normalizer.so"),
                    ),
                nativeFramePipelineConfiguration =
                    VesperNativeFramePipelineConfiguration(
                        mode = VesperNativeFramePipelineMode.PreferNativeFrame,
                        decoderPluginLibraryPaths = listOf("/tmp/libmediacodec_decoder.so"),
                        frameProcessorPluginLibraryPaths = listOf("/tmp/libframe.so"),
                        maxInFlightFrames = 2,
                    ),
            )

        val diagnostic =
            bridge.pluginDiagnostics.first {
                it["pluginKind"] == "native_frame_pipeline"
            }

        assertEquals("selected", diagnostic["participation"])
        assertEquals("sdkManagedNativeFrame", diagnostic["route"])
        assertEquals("sourceNormalizerPacket", diagnostic["sourceInput"])
        assertEquals("MediaCodec", diagnostic["decoderAdapter"])
        assertEquals("SurfaceView", diagnostic["presenterProfile"])
        assertEquals("media_codec_surface_texture", diagnostic["pipelineProfile"])
        assertNull(diagnostic["fallbackReason"])
    }

    @Test
    fun preferNativeFramePipelineOpensJniSessionAfterSystemStartup() {
        val initialSource =
            VesperPlayerSource.remote(
                uri = "https://example.com/video.mp4",
                label = "MP4",
                protocol = VesperPlayerSourceProtocol.Progressive,
            )
        val bindings = FakeBindings()
        val bridge =
            VesperNativePlayerBridge(
                bindings = bindings,
                initialSource = initialSource,
                sourceNormalizerConfiguration =
                    VesperSourceNormalizerConfiguration(
                        mode = VesperSourceNormalizerMode.PreflightOnly,
                        pluginLibraryPaths = listOf("/tmp/libsource_normalizer.so"),
                    ),
                nativeFramePipelineConfiguration =
                    VesperNativeFramePipelineConfiguration(
                        mode = VesperNativeFramePipelineMode.PreferNativeFrame,
                        decoderPluginLibraryPaths = listOf("/tmp/libmediacodec_decoder.so"),
                        frameProcessorPluginLibraryPaths = listOf("/tmp/libframe.so"),
                        maxInFlightFrames = 2,
                    ),
            )

        bridge.initialize()

        assertEquals(initialSource, bindings.lastInitializedSource)
        assertEquals(initialSource, bindings.lastNativeFramePipelineSource)
        assertEquals(1, bindings.openNativeFramePipelineCount)
        assertEquals(1, bindings.advanceNativeFramePipelineCount)
        assertEquals(0, bindings.closeNativeFramePipelineCount)
        assertEquals(NativeVideoSurfaceKind.SurfaceView, bindings.lastNativeFramePipelineSurfaceKind)
        assertEquals(
            VesperSourceNormalizerMode.PreflightOnly,
            bindings.lastNativeFramePipelineSourceNormalizerConfiguration?.mode,
        )
        assertEquals(
            VesperNativeFramePipelineMode.PreferNativeFrame,
            bindings.lastNativeFramePipelineConfiguration?.mode,
        )
        val diagnostic =
            bridge.pluginDiagnostics.first {
                it["pluginKind"] == "native_frame_pipeline"
            }
        assertEquals("selected", diagnostic["participation"])
        assertEquals("sdkManagedNativeFrame", diagnostic["route"])
        assertEquals("open", diagnostic["lifecycle"])
        assertEquals("pending", diagnostic["lastAdvanceStatus"])
    }

    @Test
    fun preferNativeFramePipelineSkipsSystemSourceNormalizerResourcePlayback() {
        val initialSource =
            VesperPlayerSource.local(
                uri = "file:///tmp/video.mp4",
                label = "Local MP4",
            )
        val bindings = FakeBindings()
        val bridge =
            VesperNativePlayerBridge(
                bindings = bindings,
                initialSource = initialSource,
                sourceNormalizerConfiguration =
                    VesperSourceNormalizerConfiguration(
                        mode = VesperSourceNormalizerMode.RequireNormalized,
                        pluginLibraryPaths = listOf("/tmp/libsource_normalizer.so"),
                    ),
                nativeFramePipelineConfiguration =
                    VesperNativeFramePipelineConfiguration(
                        mode = VesperNativeFramePipelineMode.PreferNativeFrame,
                        decoderPluginLibraryPaths = listOf("/tmp/libmediacodec_decoder.so"),
                    ),
            )

        bridge.initialize()

        assertEquals(false, bindings.lastSystemPlaybackUsesSourceNormalizerResource)
        assertEquals(false, bindings.lastSystemPlaybackVideoEnabled)
        assertEquals(initialSource, bindings.lastInitializedSource)
        assertEquals(initialSource, bindings.lastNativeFramePipelineSource)
        assertEquals(
            VesperSourceNormalizerMode.RequireNormalized,
            bindings.lastNativeFramePipelineSourceNormalizerConfiguration?.mode,
        )
    }

    @Test
    fun diagnosticsOnlyKeepsSystemSourceNormalizerResourcePlaybackEnabled() {
        val initialSource =
            VesperPlayerSource.local(
                uri = "file:///tmp/video.mp4",
                label = "Local MP4",
            )
        val bindings = FakeBindings()
        val bridge =
            VesperNativePlayerBridge(
                bindings = bindings,
                initialSource = initialSource,
                sourceNormalizerConfiguration =
                    VesperSourceNormalizerConfiguration(
                        mode = VesperSourceNormalizerMode.RequireNormalized,
                        pluginLibraryPaths = listOf("/tmp/libsource_normalizer.so"),
                    ),
                nativeFramePipelineConfiguration =
                    VesperNativeFramePipelineConfiguration(
                        mode = VesperNativeFramePipelineMode.DiagnosticsOnly,
                        decoderPluginLibraryPaths = listOf("/tmp/libmediacodec_decoder.so"),
                    ),
            )

        bridge.initialize()

        assertEquals(true, bindings.lastSystemPlaybackUsesSourceNormalizerResource)
        assertEquals(true, bindings.lastSystemPlaybackVideoEnabled)
        assertNull(bindings.lastNativeFramePipelineSource)
    }

    @Test
    fun sourceNormalizerResourcePlaybackSkipsHostHandledNetworkSources() {
        val preferNormalized =
            VesperSourceNormalizerConfiguration(
                mode = VesperSourceNormalizerMode.PreferNormalized,
                pluginLibraryPaths = listOf("/tmp/libsource_normalizer.so"),
            )
        val requireNormalized =
            VesperSourceNormalizerConfiguration(
                mode = VesperSourceNormalizerMode.RequireNormalized,
                pluginLibraryPaths = listOf("/tmp/libsource_normalizer.so"),
            )

        assertFalse(
            preferNormalized.shouldOpenNormalizedResourceForPlayback(
                VesperPlayerSource.hls("https://example.com/live.m3u8", "Live"),
            )
        )
        assertFalse(
            requireNormalized.shouldOpenNormalizedResourceForPlayback(
                VesperPlayerSource.dash("https://example.com/manifest.mpd", "Dash"),
            )
        )
        assertFalse(
            preferNormalized.shouldOpenNormalizedResourceForPlayback(
                VesperPlayerSource.remote(
                    uri = "https://example.com/video.mp4",
                    label = "Remote MP4",
                    protocol = VesperPlayerSourceProtocol.Progressive,
                ),
            )
        )
        assertTrue(
            preferNormalized.shouldOpenNormalizedResourceForPlayback(
                VesperPlayerSource.local("file:///tmp/video.mp4", "Local MP4"),
            )
        )
    }

    @Test
    fun sourceNormalizerBypassDiagnosticsDecodeHdrResourceReason() {
        val diagnostics =
            listOf(
                mapOf(
                    "path" to "/tmp/libsource_normalizer.so",
                    "pluginKind" to "source_normalizer",
                    "status" to "sourceNormalizerUnsupported",
                    "participation" to "bypassed",
                    "message" to
                        "HdrResourceMetadataNotPreserved: source normalizer fMP4 resource route cannot currently guarantee HDR/Dolby Vision metadata preservation for system playback",
                )
            )

        assertEquals(1, diagnostics.size)
        assertEquals("sourceNormalizerUnsupported", diagnostics.first()["status"])
        assertEquals("bypassed", diagnostics.first()["participation"])
        assertEquals("sourceNormalizerResourceBypassedForHdr", sourceNormalizerBypassReason(diagnostics))
    }

    @Test
    fun sourceNormalizerResourceOpenObjectIsNotParsedAsBypassDiagnostics() {
        val diagnostics =
            parseSourceNormalizerBypassDiagnostics(
                """
                {
                  "handle": 42,
                  "outputRoute": "fmp4LocalStream",
                  "primaryResourcePath": "/tmp/normalized.mp4"
                }
                """.trimIndent(),
            )

        assertNull(diagnostics)
    }

    @Test
    fun nativeFramePipelineDiagnosticsReportPresenterSurfaceState() {
        val initialSource =
            VesperPlayerSource.remote(
                uri = "https://example.com/video.mp4",
                label = "MP4",
                protocol = VesperPlayerSourceProtocol.Progressive,
            )
        val bindings =
            FakeBindings(
                nativeFramePipelineAdvanceStatus =
                    mapOf(
                        "status" to "pending",
                        "presenterReady" to false,
                        "presenterConfigured" to false,
                        "presenterState" to "waitingForPresenter",
                        "surfaceAttached" to true,
                        "surfaceProfile" to "SurfaceView",
                        "message" to "presenter surface attached",
                    )
            )
        val bridge =
            VesperNativePlayerBridge(
                bindings = bindings,
                initialSource = initialSource,
                sourceNormalizerConfiguration =
                    VesperSourceNormalizerConfiguration(
                        mode = VesperSourceNormalizerMode.PreflightOnly,
                        pluginLibraryPaths = listOf("/tmp/libsource_normalizer.so"),
                    ),
                nativeFramePipelineConfiguration =
                    VesperNativeFramePipelineConfiguration(
                        mode = VesperNativeFramePipelineMode.PreferNativeFrame,
                        decoderPluginLibraryPaths = listOf("/tmp/libmediacodec_decoder.so"),
                    ),
            )

        bridge.initialize()

        val diagnostic =
            bridge.pluginDiagnostics.first {
                it["pluginKind"] == "native_frame_pipeline"
            }
        assertEquals(false, diagnostic["presenterReady"])
        assertEquals(false, diagnostic["presenterConfigured"])
        assertEquals("waitingForPresenter", diagnostic["presenterState"])
        assertEquals(true, diagnostic["surfaceAttached"])
        assertEquals("SurfaceView", diagnostic["surfaceProfile"])
    }

    @Test
    fun nativeFramePipelineDiagnosticsUseLatestPresenterSurfaceState() {
        val initialSource =
            VesperPlayerSource.remote(
                uri = "https://example.com/video.mp4",
                label = "MP4",
                protocol = VesperPlayerSourceProtocol.Progressive,
            )
        val bindings =
            FakeBindings(
                nativeFramePipelineAdvanceStatus =
                    mapOf(
                        "status" to "pending",
                        "presenterReady" to true,
                        "presenterConfigured" to true,
                        "presenterState" to "ready",
                        "surfaceAttached" to true,
                        "surfaceProfile" to "SurfaceView",
                        "message" to "presenter surface attached",
                    )
            )
        val bridge =
            VesperNativePlayerBridge(
                bindings = bindings,
                initialSource = initialSource,
                sourceNormalizerConfiguration =
                    VesperSourceNormalizerConfiguration(
                        mode = VesperSourceNormalizerMode.PreflightOnly,
                        pluginLibraryPaths = listOf("/tmp/libsource_normalizer.so"),
                    ),
                nativeFramePipelineConfiguration =
                    VesperNativeFramePipelineConfiguration(
                        mode = VesperNativeFramePipelineMode.PreferNativeFrame,
                        decoderPluginLibraryPaths = listOf("/tmp/libmediacodec_decoder.so"),
                    ),
            )

        bridge.initialize()
        bindings.setCurrentNativeFramePipelineStatusForTest(
            bindings.nativeFramePipelineStatusForTest(
                "status" to "pending",
                "presenterReady" to false,
                "presenterConfigured" to false,
                "presenterState" to "waitingForSurface",
                "surfaceAttached" to false,
                "message" to "presenter surface detached",
            )
        )
        bridge.refresh()

        val diagnostic =
            bridge.pluginDiagnostics.first {
                it["pluginKind"] == "native_frame_pipeline"
            }
        assertEquals(false, diagnostic["presenterReady"])
        assertEquals(false, diagnostic["presenterConfigured"])
        assertEquals("waitingForSurface", diagnostic["presenterState"])
        assertEquals(false, diagnostic["surfaceAttached"])
        assertNull(diagnostic["surfaceProfile"])
    }

    @Test
    fun nativeFramePipelineRawFrameAdvanceIsReleasedWhenPresenterDoesNotAccept() {
        val initialSource =
            VesperPlayerSource.remote(
                uri = "https://example.com/video.mp4",
                label = "MP4",
                protocol = VesperPlayerSourceProtocol.Progressive,
            )
        val bindings =
            FakeBindings(
                nativeFramePipelineAdvanceStatus =
                    mapOf(
                        "status" to "frame",
                        "handle" to 77L,
                        "nativeHandle" to 1234L,
                        "message" to "decoded frame",
                        "requiresHostRelease" to false,
                        "counters" to
                            mapOf(
                                "processedFrames" to 1L,
                                "presentedFrames" to 0L,
                                "releasedFrames" to 0L,
                                "deadlineMisses" to 0L,
                                "backpressureCount" to 0L,
                                "lateDropped" to 0L,
                            ),
                    ),
            )
        val bridge =
            VesperNativePlayerBridge(
                bindings = bindings,
                initialSource = initialSource,
                sourceNormalizerConfiguration =
                    VesperSourceNormalizerConfiguration(
                        mode = VesperSourceNormalizerMode.PreflightOnly,
                        pluginLibraryPaths = listOf("/tmp/libsource_normalizer.so"),
                    ),
                nativeFramePipelineConfiguration =
                    VesperNativeFramePipelineConfiguration(
                        mode = VesperNativeFramePipelineMode.PreferNativeFrame,
                        decoderPluginLibraryPaths = listOf("/tmp/libmediacodec_decoder.so"),
                    ),
            )

        bridge.initialize()

        assertEquals(1, bindings.advanceNativeFramePipelineCount)
        assertEquals(listOf(77L to false), bindings.releasedNativeFramePipelineFrames)
        val diagnostic =
            bridge.pluginDiagnostics.first {
                it["pluginKind"] == "native_frame_pipeline"
            }
        assertEquals("released", diagnostic["lastAdvanceStatus"])
        assertEquals(1L, diagnostic["processedFrames"])
        assertEquals(0L, diagnostic["presentedFrames"])
    }

    @Test
    fun preferNativeFramePipelinePumpAdvancesWhilePlayingAndStopsAtEndOfStream() {
        val initialSource =
            VesperPlayerSource.remote(
                uri = "https://example.com/video.mp4",
                label = "MP4",
                protocol = VesperPlayerSourceProtocol.Progressive,
            )
        val scheduler = ManualNativeFramePipelinePumpScheduler()
        val bindings =
            FakeBindings(
                nativeFramePipelineAdvanceStatuses =
                    mutableListOf(
                        mapOf(
                            "status" to "pending",
                            "message" to "initial warmup",
                        ),
                        mapOf(
                            "status" to "presented",
                            "message" to "presented frame",
                            "counters" to
                                mapOf(
                                    "processedFrames" to 1L,
                                    "presentedFrames" to 1L,
                                    "deadlineMisses" to 0L,
                                    "backpressureCount" to 0L,
                                    "lateDropped" to 0L,
                                ),
                        ),
                        mapOf(
                            "status" to "pending",
                            "message" to "decoder needs more input",
                        ),
                        mapOf(
                            "status" to "endOfStream",
                            "message" to "end of stream",
                        ),
                    ),
            )
        val bridge =
            VesperNativePlayerBridge(
                bindings = bindings,
                initialSource = initialSource,
                sourceNormalizerConfiguration =
                    VesperSourceNormalizerConfiguration(
                        mode = VesperSourceNormalizerMode.PreflightOnly,
                        pluginLibraryPaths = listOf("/tmp/libsource_normalizer.so"),
                    ),
                nativeFramePipelineConfiguration =
                    VesperNativeFramePipelineConfiguration(
                        mode = VesperNativeFramePipelineMode.PreferNativeFrame,
                        decoderPluginLibraryPaths = listOf("/tmp/libmediacodec_decoder.so"),
                    ),
                nativeFramePipelinePumpScheduler = scheduler,
            )

        bridge.initialize()
        assertEquals(1, bindings.advanceNativeFramePipelineCount)
        assertFalse(scheduler.hasPendingActions())

        bridge.play()
        assertTrue(scheduler.hasPendingActions())

        scheduler.runNext()
        assertEquals(2, bindings.advanceNativeFramePipelineCount)
        assertTrue(scheduler.hasPendingActions())

        scheduler.runNext()
        assertEquals(3, bindings.advanceNativeFramePipelineCount)
        assertTrue(scheduler.hasPendingActions())

        scheduler.runNext()
        assertEquals(4, bindings.advanceNativeFramePipelineCount)
        assertFalse(scheduler.hasPendingActions())

        val diagnostic =
            bridge.pluginDiagnostics.first {
                it["pluginKind"] == "native_frame_pipeline"
            }
        assertEquals("endOfStream", diagnostic["lastAdvanceStatus"])
        assertEquals(false, diagnostic["pumpRunning"])
    }

    @Test
    fun preferNativeFramePipelinePlayFromFinishedSeeksNativeFramePipelineBeforeRestartingPump() {
        val initialSource =
            VesperPlayerSource.remote(
                uri = "https://example.com/video.mp4",
                label = "MP4",
                protocol = VesperPlayerSourceProtocol.Progressive,
            )
        val scheduler = ManualNativeFramePipelinePumpScheduler()
        val bindings =
            FakeBindings(
                nativeFramePipelineAdvanceStatuses =
                    mutableListOf(
                        mapOf(
                            "status" to "endOfStream",
                            "message" to "end of stream",
                        ),
                        mapOf(
                            "status" to "pending",
                            "message" to "waiting after replay seek",
                        ),
                    ),
            )
        val bridge =
            VesperNativePlayerBridge(
                bindings = bindings,
                initialSource = initialSource,
                sourceNormalizerConfiguration =
                    VesperSourceNormalizerConfiguration(
                        mode = VesperSourceNormalizerMode.PreflightOnly,
                        pluginLibraryPaths = listOf("/tmp/libsource_normalizer.so"),
                    ),
                nativeFramePipelineConfiguration =
                    VesperNativeFramePipelineConfiguration(
                        mode = VesperNativeFramePipelineMode.PreferNativeFrame,
                        decoderPluginLibraryPaths = listOf("/tmp/libmediacodec_decoder.so"),
                    ),
                nativeFramePipelinePumpScheduler = scheduler,
            )

        bridge.initialize()
        assertEquals("endOfStream", bridge.pluginDiagnostics.first {
            it["pluginKind"] == "native_frame_pipeline"
        }["lastAdvanceStatus"])
        bindings.events.add(NativeBridgeEvent.Ended())
        bridge.refresh()

        bridge.play()

        assertTrue(bindings.seekToPositions.isEmpty())
        assertEquals(listOf(0L), bindings.seekNativeFramePipelinePositions)
        assertEquals(0, bindings.flushNativeFramePipelineCount)
        assertEquals(1, bindings.playCount)
        assertTrue(scheduler.hasPendingActions())
        assertEquals(PlaybackStateUi.Playing, bridge.uiState.value.playbackState)
        val diagnostic =
            bridge.pluginDiagnostics.first {
                it["pluginKind"] == "native_frame_pipeline"
            }
        assertEquals("sdkManagedNativeFrame", diagnostic["route"])
        assertEquals("open", diagnostic["lifecycle"])
        assertEquals("seeked", diagnostic["lastAdvanceStatus"])
        assertEquals(true, diagnostic["pumpRunning"])
    }

    @Test
    fun preferNativeFramePipelineRuntimeAdvanceFailureFallsBackToSystemPlayback() {
        val initialSource =
            VesperPlayerSource.remote(
                uri = "https://example.com/video.mp4",
                label = "MP4",
                protocol = VesperPlayerSourceProtocol.Progressive,
            )
        val scheduler = ManualNativeFramePipelinePumpScheduler()
        val bindings =
            FakeBindings(
                nativeFramePipelineAdvanceStatuses =
                    mutableListOf(
                        mapOf(
                            "status" to "pending",
                            "message" to "initial warmup",
                        )
                    ),
                nativeFramePipelineAdvanceError =
                    IllegalStateException("simulated native-frame runtime failure"),
            )
        val bridge =
            VesperNativePlayerBridge(
                bindings = bindings,
                initialSource = initialSource,
                sourceNormalizerConfiguration =
                    VesperSourceNormalizerConfiguration(
                        mode = VesperSourceNormalizerMode.PreflightOnly,
                        pluginLibraryPaths = listOf("/tmp/libsource_normalizer.so"),
                    ),
                nativeFramePipelineConfiguration =
                    VesperNativeFramePipelineConfiguration(
                        mode = VesperNativeFramePipelineMode.PreferNativeFrame,
                        decoderPluginLibraryPaths = listOf("/tmp/libmediacodec_decoder.so"),
                    ),
                nativeFramePipelinePumpScheduler = scheduler,
            )

        bridge.initialize()
        assertEquals(1, bindings.advanceNativeFramePipelineCount)
        val closeCountBeforeRuntimeFailure = bindings.closeNativeFramePipelineCount
        bindings.events += NativeBridgeEvent.PlaybackStateChanged(PlaybackStateUi.Playing)

        bridge.refresh()
        assertTrue(scheduler.hasPendingActions())
        scheduler.runNext()

        assertEquals(2, bindings.advanceNativeFramePipelineCount)
        assertEquals(closeCountBeforeRuntimeFailure + 1, bindings.closeNativeFramePipelineCount)
        assertFalse(scheduler.hasPendingActions())
        val diagnostic =
            bridge.pluginDiagnostics.first {
                it["pluginKind"] == "native_frame_pipeline"
            }
        assertEquals("fallback", diagnostic["participation"])
        assertEquals("systemPlayer", diagnostic["route"])
        assertEquals("fallback", diagnostic["lifecycle"])
        assertEquals("systemPlayer", diagnostic["fallbackTargetRoute"])
        assertEquals("simulated native-frame runtime failure", diagnostic["fallbackReason"])
        assertEquals(false, diagnostic["pumpRunning"])
    }

    @Test
    fun requireNativeFramePipelineRuntimeAdvanceFailureKeepsBridgeRecoverable() {
        val initialSource =
            VesperPlayerSource.remote(
                uri = "https://example.com/video.mp4",
                label = "MP4",
                protocol = VesperPlayerSourceProtocol.Progressive,
            )
        val scheduler = ManualNativeFramePipelinePumpScheduler()
        val bindings =
            FakeBindings(
                nativeFramePipelineAdvanceStatuses =
                    mutableListOf(
                        mapOf(
                            "status" to "pending",
                            "message" to "initial warmup",
                        )
                    ),
                nativeFramePipelineAdvanceError =
                    IllegalStateException("simulated required native-frame runtime failure"),
            )
        val bridge =
            VesperNativePlayerBridge(
                bindings = bindings,
                initialSource = initialSource,
                sourceNormalizerConfiguration =
                    VesperSourceNormalizerConfiguration(
                        mode = VesperSourceNormalizerMode.PreflightOnly,
                        pluginLibraryPaths = listOf("/tmp/libsource_normalizer.so"),
                    ),
                nativeFramePipelineConfiguration =
                    VesperNativeFramePipelineConfiguration(
                        mode = VesperNativeFramePipelineMode.RequireNativeFrame,
                        decoderPluginLibraryPaths = listOf("/tmp/libmediacodec_decoder.so"),
                    ),
                nativeFramePipelinePumpScheduler = scheduler,
            )

        bridge.initialize()
        assertEquals(1, bindings.advanceNativeFramePipelineCount)
        val closeCountBeforeRuntimeFailure = bindings.closeNativeFramePipelineCount
        bindings.events += NativeBridgeEvent.PlaybackStateChanged(PlaybackStateUi.Playing)

        bridge.refresh()
        assertTrue(scheduler.hasPendingActions())
        scheduler.runNext()

        assertEquals(2, bindings.advanceNativeFramePipelineCount)
        assertEquals(closeCountBeforeRuntimeFailure + 1, bindings.closeNativeFramePipelineCount)
        assertEquals(0, bindings.disposeCount)
        assertEquals(1, bindings.clearSystemPlaybackCount)
        assertFalse(scheduler.hasPendingActions())
        assertEquals(PlaybackStateUi.Ready, bridge.uiState.value.playbackState)
        assertTrue(
            bridge.uiState.value.subtitle.contains("simulated required native-frame runtime failure")
        )
        val diagnostic =
            bridge.pluginDiagnostics.first {
                it["pluginKind"] == "native_frame_pipeline"
            }
        assertEquals("selected", diagnostic["participation"])
        assertEquals("sdkManagedNativeFrame", diagnostic["route"])
        assertEquals("failed", diagnostic["lifecycle"])
        assertNull(diagnostic["fallbackTargetRoute"])
        assertEquals(
            "simulated required native-frame runtime failure",
            diagnostic["fallbackReason"],
        )
        assertEquals(false, diagnostic["pumpRunning"])
    }

    @Test
    fun preferNativeFramePipelinePumpSchedulesHostTimedReleaseToSurface() {
        val initialSource =
            VesperPlayerSource.remote(
                uri = "https://example.com/video.mp4",
                label = "MP4",
                protocol = VesperPlayerSourceProtocol.Progressive,
            )
        val scheduler = ManualNativeFramePipelinePumpScheduler()
        val bindings =
            FakeBindings(
                snapshot =
                    NativeBridgeSnapshot(
                        playbackState = PlaybackStateUi.Playing,
                        playbackRate = 1.0f,
                        isBuffering = false,
                        isInterrupted = false,
                        timeline =
                            TimelineUiState(
                                kind = TimelineKind.Vod,
                                isSeekable = true,
                                seekableRange = SeekableRangeUi(0L, 10_000L),
                                liveEdgeMs = null,
                                positionMs = 1_000L,
                                durationMs = 10_000L,
                            ),
                    ),
                nativeFramePipelineAdvanceStatuses =
                    mutableListOf(
                        mapOf(
                            "status" to "pending",
                            "message" to "initial warmup",
                        ),
                        mapOf(
                            "status" to "frame",
                            "handle" to 88L,
                            "presentationTimeUs" to 1_050_000L,
                            "requiresHostRelease" to true,
                            "message" to "host-timed release",
                        ),
                    ),
            )
        val bridge =
            VesperNativePlayerBridge(
                bindings = bindings,
                initialSource = initialSource,
                sourceNormalizerConfiguration =
                    VesperSourceNormalizerConfiguration(
                        mode = VesperSourceNormalizerMode.PreflightOnly,
                        pluginLibraryPaths = listOf("/tmp/libsource_normalizer.so"),
                    ),
                nativeFramePipelineConfiguration =
                    VesperNativeFramePipelineConfiguration(
                        mode = VesperNativeFramePipelineMode.PreferNativeFrame,
                        decoderPluginLibraryPaths = listOf("/tmp/libmediacodec_decoder.so"),
                    ),
                nativeFramePipelinePumpScheduler = scheduler,
            )

        bridge.initialize()
        bridge.play()
        scheduler.runNext()

        assertEquals(2, bindings.advanceNativeFramePipelineCount)
        assertTrue(scheduler.hasPendingActions())
        assertTrue(bindings.releasedNativeFramePipelineFrames.isEmpty())

        scheduler.runNext()

        assertEquals(listOf(88L to true), bindings.releasedNativeFramePipelineFrames)
    }

    @Test
    fun preferNativeFramePipelineKeepsPumpRunningWhenSystemSnapshotReportsReady() {
        val initialSource =
            VesperPlayerSource.remote(
                uri = "https://example.com/video.mp4",
                label = "MP4",
                protocol = VesperPlayerSourceProtocol.Progressive,
            )
        val scheduler = ManualNativeFramePipelinePumpScheduler()
        val bindings =
            FakeBindings(
                snapshot =
                    NativeBridgeSnapshot(
                        playbackState = PlaybackStateUi.Ready,
                        playbackRate = 1.0f,
                        isBuffering = false,
                        isInterrupted = false,
                        timeline =
                            TimelineUiState(
                                kind = TimelineKind.Vod,
                                isSeekable = true,
                                seekableRange = SeekableRangeUi(0L, 10_000L),
                                liveEdgeMs = null,
                                positionMs = 0L,
                                durationMs = 10_000L,
                            ),
                    ),
                nativeFramePipelineAdvanceStatuses =
                    mutableListOf(
                        mapOf(
                            "status" to "pending",
                            "message" to "initial warmup",
                        ),
                        mapOf(
                            "status" to "pending",
                            "message" to "system snapshot still reports ready",
                        ),
                    ),
            )
        val bridge =
            VesperNativePlayerBridge(
                bindings = bindings,
                initialSource = initialSource,
                sourceNormalizerConfiguration =
                    VesperSourceNormalizerConfiguration(
                        mode = VesperSourceNormalizerMode.PreflightOnly,
                        pluginLibraryPaths = listOf("/tmp/libsource_normalizer.so"),
                    ),
                nativeFramePipelineConfiguration =
                    VesperNativeFramePipelineConfiguration(
                        mode = VesperNativeFramePipelineMode.PreferNativeFrame,
                        decoderPluginLibraryPaths = listOf("/tmp/libmediacodec_decoder.so"),
                    ),
                nativeFramePipelinePumpScheduler = scheduler,
            )

        bridge.initialize()
        bridge.play()

        assertTrue(scheduler.hasPendingActions())
        scheduler.runNext()

        assertEquals(2, bindings.advanceNativeFramePipelineCount)
        assertTrue(scheduler.hasPendingActions())
        val diagnostic =
            bridge.pluginDiagnostics.first {
                it["pluginKind"] == "native_frame_pipeline"
            }
        assertEquals(true, diagnostic["pumpRunning"])
    }

    @Test
    fun preferNativeFramePipelineReleasesPendingFrameWhenPumpEpochChangesBeforeRelease() {
        val initialSource =
            VesperPlayerSource.remote(
                uri = "https://example.com/video.mp4",
                label = "MP4",
                protocol = VesperPlayerSourceProtocol.Progressive,
            )
        lateinit var bridge: VesperNativePlayerBridge
        var schedulerRunCount = 0
        val bindings =
            FakeBindings(
                snapshot =
                    NativeBridgeSnapshot(
                        playbackState = PlaybackStateUi.Playing,
                        playbackRate = 1.0f,
                        isBuffering = false,
                        isInterrupted = false,
                        timeline =
                            TimelineUiState(
                                kind = TimelineKind.Vod,
                                isSeekable = true,
                                seekableRange = SeekableRangeUi(0L, 10_000L),
                                liveEdgeMs = null,
                                positionMs = 1_000L,
                                durationMs = 10_000L,
                            ),
                    ),
                nativeFramePipelineAdvanceStatuses =
                    mutableListOf(
                        mapOf("status" to "pending"),
                        mapOf(
                            "status" to "frame",
                            "handle" to 90L,
                            "presentationTimeUs" to 1_050_000L,
                            "requiresHostRelease" to true,
                        ),
                    ),
            )
        val scheduler =
            ManualNativeFramePipelinePumpScheduler(
                beforeRun = {
                    schedulerRunCount += 1
                    if (
                        schedulerRunCount == 2 &&
                            bindings.releasedNativeFramePipelineFrames.isEmpty()
                    ) {
                        bridge.pause()
                    }
                },
            )
        bridge =
            VesperNativePlayerBridge(
                bindings = bindings,
                initialSource = initialSource,
                sourceNormalizerConfiguration =
                    VesperSourceNormalizerConfiguration(
                        mode = VesperSourceNormalizerMode.PreflightOnly,
                        pluginLibraryPaths = listOf("/tmp/libsource_normalizer.so"),
                    ),
                nativeFramePipelineConfiguration =
                    VesperNativeFramePipelineConfiguration(
                        mode = VesperNativeFramePipelineMode.PreferNativeFrame,
                        decoderPluginLibraryPaths = listOf("/tmp/libmediacodec_decoder.so"),
                    ),
                nativeFramePipelinePumpScheduler = scheduler,
            )

        bridge.initialize()
        bridge.play()
        scheduler.runNext()
        assertTrue(scheduler.hasPendingActions())

        scheduler.runNext()

        assertEquals(listOf(90L to false), bindings.releasedNativeFramePipelineFrames)
    }

    @Test
    fun preferNativeFramePipelineHostTimedReleaseFailureFallsBackWithoutAdvancingAgain() {
        val initialSource =
            VesperPlayerSource.remote(
                uri = "https://example.com/video.mp4",
                label = "MP4",
                protocol = VesperPlayerSourceProtocol.Progressive,
            )
        val scheduler = ManualNativeFramePipelinePumpScheduler()
        val bindings =
            FakeBindings(
                snapshot =
                    NativeBridgeSnapshot(
                        playbackState = PlaybackStateUi.Playing,
                        playbackRate = 1.0f,
                        isBuffering = false,
                        isInterrupted = false,
                        timeline =
                            TimelineUiState(
                                kind = TimelineKind.Vod,
                                isSeekable = true,
                                seekableRange = SeekableRangeUi(0L, 10_000L),
                                liveEdgeMs = null,
                                positionMs = 1_000L,
                                durationMs = 10_000L,
                            ),
                    ),
                nativeFramePipelineAdvanceStatuses =
                    mutableListOf(
                        mapOf("status" to "pending"),
                        mapOf(
                            "status" to "frame",
                            "handle" to 91L,
                            "presentationTimeUs" to 1_080_000L,
                            "requiresHostRelease" to true,
                        ),
                    ),
                nativeFramePipelineReleaseError =
                    IllegalStateException("simulated native-frame release failure"),
            )
        val bridge =
            VesperNativePlayerBridge(
                bindings = bindings,
                initialSource = initialSource,
                sourceNormalizerConfiguration =
                    VesperSourceNormalizerConfiguration(
                        mode = VesperSourceNormalizerMode.PreflightOnly,
                        pluginLibraryPaths = listOf("/tmp/libsource_normalizer.so"),
                    ),
                nativeFramePipelineConfiguration =
                    VesperNativeFramePipelineConfiguration(
                        mode = VesperNativeFramePipelineMode.PreferNativeFrame,
                        decoderPluginLibraryPaths = listOf("/tmp/libmediacodec_decoder.so"),
                    ),
                nativeFramePipelinePumpScheduler = scheduler,
            )

        bridge.initialize()
        bridge.play()
        scheduler.runNext()
        val advanceCountBeforeReleaseFailure = bindings.advanceNativeFramePipelineCount
        val closeCountBeforeReleaseFailure = bindings.closeNativeFramePipelineCount

        scheduler.runNext()

        assertEquals(advanceCountBeforeReleaseFailure, bindings.advanceNativeFramePipelineCount)
        assertEquals(closeCountBeforeReleaseFailure + 1, bindings.closeNativeFramePipelineCount)
        assertFalse(scheduler.hasPendingActions())
        val diagnostic =
            bridge.pluginDiagnostics.first {
                it["pluginKind"] == "native_frame_pipeline"
            }
        assertEquals("fallback", diagnostic["participation"])
        assertEquals("systemPlayer", diagnostic["route"])
        assertEquals("fallback", diagnostic["lifecycle"])
        assertEquals("systemPlayer", diagnostic["fallbackTargetRoute"])
        assertEquals("simulated native-frame release failure", diagnostic["fallbackReason"])
    }

    @Test
    fun requireNativeFramePipelineHostTimedReleaseFailureDisposesSystemPlayback() {
        val initialSource =
            VesperPlayerSource.remote(
                uri = "https://example.com/video.mp4",
                label = "MP4",
                protocol = VesperPlayerSourceProtocol.Progressive,
            )
        val scheduler = ManualNativeFramePipelinePumpScheduler()
        val bindings =
            FakeBindings(
                snapshot =
                    NativeBridgeSnapshot(
                        playbackState = PlaybackStateUi.Playing,
                        playbackRate = 1.0f,
                        isBuffering = false,
                        isInterrupted = false,
                        timeline =
                            TimelineUiState(
                                kind = TimelineKind.Vod,
                                isSeekable = true,
                                seekableRange = SeekableRangeUi(0L, 10_000L),
                                liveEdgeMs = null,
                                positionMs = 1_000L,
                                durationMs = 10_000L,
                            ),
                    ),
                nativeFramePipelineAdvanceStatuses =
                    mutableListOf(
                        mapOf("status" to "pending"),
                        mapOf(
                            "status" to "frame",
                            "handle" to 91L,
                            "presentationTimeUs" to 1_080_000L,
                            "requiresHostRelease" to true,
                        ),
                    ),
                nativeFramePipelineReleaseError =
                    IllegalStateException("simulated required native-frame release failure"),
            )
        val bridge =
            VesperNativePlayerBridge(
                bindings = bindings,
                initialSource = initialSource,
                sourceNormalizerConfiguration =
                    VesperSourceNormalizerConfiguration(
                        mode = VesperSourceNormalizerMode.PreflightOnly,
                        pluginLibraryPaths = listOf("/tmp/libsource_normalizer.so"),
                    ),
                nativeFramePipelineConfiguration =
                    VesperNativeFramePipelineConfiguration(
                        mode = VesperNativeFramePipelineMode.RequireNativeFrame,
                        decoderPluginLibraryPaths = listOf("/tmp/libmediacodec_decoder.so"),
                    ),
                nativeFramePipelinePumpScheduler = scheduler,
            )

        bridge.initialize()
        bridge.play()
        scheduler.runNext()
        val closeCountBeforeReleaseFailure = bindings.closeNativeFramePipelineCount

        scheduler.runNext()

        assertEquals(closeCountBeforeReleaseFailure + 1, bindings.closeNativeFramePipelineCount)
        assertEquals(0, bindings.disposeCount)
        assertFalse(scheduler.hasPendingActions())
        assertEquals(PlaybackStateUi.Ready, bridge.uiState.value.playbackState)
        assertTrue(
            bridge.uiState.value.subtitle.contains("simulated required native-frame release failure")
        )
        val diagnostic =
            bridge.pluginDiagnostics.first {
                it["pluginKind"] == "native_frame_pipeline"
            }
        assertEquals("selected", diagnostic["participation"])
        assertEquals("sdkManagedNativeFrame", diagnostic["route"])
        assertEquals("failed", diagnostic["lifecycle"])
        assertNull(diagnostic["fallbackTargetRoute"])
        assertEquals(
            "simulated required native-frame release failure",
            diagnostic["fallbackReason"],
        )
        assertEquals(false, diagnostic["pumpRunning"])
    }

    @Test
    fun preferNativeFramePipelineSeekFlushesAndSeeksOpenSession() {
        val initialSource =
            VesperPlayerSource.remote(
                uri = "https://example.com/video.mp4",
                label = "MP4",
                protocol = VesperPlayerSourceProtocol.Progressive,
            )
        val bindings = FakeBindings()
        val bridge =
            VesperNativePlayerBridge(
                bindings = bindings,
                initialSource = initialSource,
                sourceNormalizerConfiguration =
                    VesperSourceNormalizerConfiguration(
                        mode = VesperSourceNormalizerMode.PreflightOnly,
                        pluginLibraryPaths = listOf("/tmp/libsource_normalizer.so"),
                    ),
                nativeFramePipelineConfiguration =
                    VesperNativeFramePipelineConfiguration(
                        mode = VesperNativeFramePipelineMode.PreferNativeFrame,
                        decoderPluginLibraryPaths = listOf("/tmp/libmediacodec_decoder.so"),
                    ),
            )

        bridge.initialize()
        bridge.seekBy(1_000L)

        assertEquals(1, bindings.flushNativeFramePipelineCount)
        assertEquals(listOf(1_000L), bindings.seekNativeFramePipelinePositions)
        val diagnostic =
            bridge.pluginDiagnostics.first {
                it["pluginKind"] == "native_frame_pipeline"
            }
        assertEquals("open", diagnostic["lifecycle"])
        assertEquals(1L, diagnostic["processedFrames"])
        assertEquals(1L, diagnostic["presentedFrames"])
    }

    @Test
    fun requireNativeFramePipelineSeekFailureKeepsBridgeRecoverable() {
        val initialSource =
            VesperPlayerSource.remote(
                uri = "https://example.com/video.mp4",
                label = "MP4",
                protocol = VesperPlayerSourceProtocol.Progressive,
            )
        val bindings =
            FakeBindings(
                nativeFramePipelineSeekError =
                    IllegalStateException("simulated required native-frame seek failure"),
            )
        val bridge =
            VesperNativePlayerBridge(
                bindings = bindings,
                initialSource = initialSource,
                sourceNormalizerConfiguration =
                    VesperSourceNormalizerConfiguration(
                        mode = VesperSourceNormalizerMode.PreflightOnly,
                        pluginLibraryPaths = listOf("/tmp/libsource_normalizer.so"),
                    ),
                nativeFramePipelineConfiguration =
                    VesperNativeFramePipelineConfiguration(
                        mode = VesperNativeFramePipelineMode.RequireNativeFrame,
                        decoderPluginLibraryPaths = listOf("/tmp/libmediacodec_decoder.so"),
                    ),
            )

        bridge.initialize()
        bridge.seekBy(1_000L)

        assertEquals(1, bindings.flushNativeFramePipelineCount)
        assertEquals(listOf(1_000L), bindings.seekNativeFramePipelinePositions)
        assertEquals(1, bindings.closeNativeFramePipelineCount)
        assertEquals(0, bindings.disposeCount)
        assertEquals(PlaybackStateUi.Ready, bridge.uiState.value.playbackState)
        assertEquals(0L, bridge.uiState.value.timeline.positionMs)
        assertTrue(
            bridge.uiState.value.subtitle.contains("simulated required native-frame seek failure")
        )
        val diagnostic =
            bridge.pluginDiagnostics.first {
                it["pluginKind"] == "native_frame_pipeline"
            }
        assertEquals("selected", diagnostic["participation"])
        assertEquals("sdkManagedNativeFrame", diagnostic["route"])
        assertEquals("failed", diagnostic["lifecycle"])
        assertNull(diagnostic["fallbackTargetRoute"])
        assertEquals(
            "simulated required native-frame seek failure",
            diagnostic["fallbackReason"],
        )
    }

    @Test
    fun requireNativeFramePipelineFlushFailureDisposesSystemPlayback() {
        val initialSource =
            VesperPlayerSource.remote(
                uri = "https://example.com/video.mp4",
                label = "MP4",
                protocol = VesperPlayerSourceProtocol.Progressive,
            )
        val bindings =
            FakeBindings(
                nativeFramePipelineFlushError =
                    IllegalStateException("simulated required native-frame flush failure"),
            )
        val bridge =
            VesperNativePlayerBridge(
                bindings = bindings,
                initialSource = initialSource,
                sourceNormalizerConfiguration =
                    VesperSourceNormalizerConfiguration(
                        mode = VesperSourceNormalizerMode.PreflightOnly,
                        pluginLibraryPaths = listOf("/tmp/libsource_normalizer.so"),
                    ),
                nativeFramePipelineConfiguration =
                    VesperNativeFramePipelineConfiguration(
                        mode = VesperNativeFramePipelineMode.RequireNativeFrame,
                        decoderPluginLibraryPaths = listOf("/tmp/libmediacodec_decoder.so"),
                    ),
            )

        bridge.initialize()
        bridge.seekBy(1_000L)

        assertEquals(1, bindings.flushNativeFramePipelineCount)
        assertTrue(bindings.seekNativeFramePipelinePositions.isEmpty())
        assertEquals(1, bindings.closeNativeFramePipelineCount)
        assertEquals(0, bindings.disposeCount)
        assertEquals(PlaybackStateUi.Ready, bridge.uiState.value.playbackState)
        assertEquals(0L, bridge.uiState.value.timeline.positionMs)
        assertTrue(
            bridge.uiState.value.subtitle.contains("simulated required native-frame flush failure")
        )
        val diagnostic =
            bridge.pluginDiagnostics.first {
                it["pluginKind"] == "native_frame_pipeline"
            }
        assertEquals("selected", diagnostic["participation"])
        assertEquals("sdkManagedNativeFrame", diagnostic["route"])
        assertEquals("failed", diagnostic["lifecycle"])
        assertNull(diagnostic["fallbackTargetRoute"])
        assertEquals(
            "simulated required native-frame flush failure",
            diagnostic["fallbackReason"],
        )
    }

    @Test
    fun requireNativeFramePipelineCanRecoverAfterRuntimeFailureOnReinitialize() {
        val initialSource =
            VesperPlayerSource.remote(
                uri = "https://example.com/video.mp4",
                label = "MP4",
                protocol = VesperPlayerSourceProtocol.Progressive,
            )
        val bindings =
            FakeBindings(
                nativeFramePipelineSeekError =
                    IllegalStateException("simulated required native-frame seek failure"),
            )
        val bridge =
            VesperNativePlayerBridge(
                bindings = bindings,
                initialSource = initialSource,
                sourceNormalizerConfiguration =
                    VesperSourceNormalizerConfiguration(
                        mode = VesperSourceNormalizerMode.PreflightOnly,
                        pluginLibraryPaths = listOf("/tmp/libsource_normalizer.so"),
                    ),
                nativeFramePipelineConfiguration =
                    VesperNativeFramePipelineConfiguration(
                        mode = VesperNativeFramePipelineMode.RequireNativeFrame,
                        decoderPluginLibraryPaths = listOf("/tmp/libmediacodec_decoder.so"),
                    ),
            )

        bridge.initialize()
        bridge.seekBy(1_000L)
        assertEquals(0, bindings.disposeCount)
        assertTrue(
            bridge.pluginDiagnostics.any {
                it["pluginKind"] == "native_frame_pipeline" &&
                    it["participation"] == "selected" &&
                    it["route"] == "sdkManagedNativeFrame" &&
                    it["lifecycle"] == "failed"
            }
        )

        bindings.nativeFramePipelineSeekError = null
        bridge.initialize()

        assertEquals(2, bindings.openNativeFramePipelineCount)
        val diagnostic =
            bridge.pluginDiagnostics.first {
                it["pluginKind"] == "native_frame_pipeline"
            }
        assertEquals("selected", diagnostic["participation"])
        assertEquals("sdkManagedNativeFrame", diagnostic["route"])
        assertEquals("open", diagnostic["lifecycle"])
        assertNull(diagnostic["fallbackReason"])
    }

    @Test
    fun requireNativeFramePipelineFailureDoesNotBlockNextSourceInitialization() {
        val hlsSource =
            VesperPlayerSource.hls(
                uri = "https://example.com/master.m3u8",
                label = "HLS",
            )
        val localSource =
            VesperPlayerSource.local(
                uri = "file:///tmp/local.mp4",
                label = "Local MP4",
            )
        val bindings =
            FakeBindings(
                nativeFramePipelineOpenError =
                    IllegalStateException("simulated native-frame open failure"),
            )
        val bridge =
            VesperNativePlayerBridge(
                bindings = bindings,
                initialSource = hlsSource,
                sourceNormalizerConfiguration =
                    VesperSourceNormalizerConfiguration(
                        mode = VesperSourceNormalizerMode.PreflightOnly,
                        pluginLibraryPaths = listOf("/tmp/libsource_normalizer.so"),
                    ),
                nativeFramePipelineConfiguration =
                    VesperNativeFramePipelineConfiguration(
                        mode = VesperNativeFramePipelineMode.RequireNativeFrame,
                        decoderPluginLibraryPaths = listOf("/tmp/libmediacodec_decoder.so"),
                    ),
            )

        bridge.initialize()
        assertEquals(0, bindings.disposeCount)
        assertTrue(
            bridge.pluginDiagnostics.any {
                it["pluginKind"] == "native_frame_pipeline" &&
                    it["lifecycle"] == "failed"
            }
        )

        bindings.nativeFramePipelineOpenError = null
        bridge.selectSource(localSource)

        assertEquals(localSource, bindings.lastInitializedSource)
        assertEquals(localSource, bindings.lastNativeFramePipelineSource)
        assertEquals(2, bindings.openNativeFramePipelineCount)
        assertEquals(1, bindings.playCount)
        val diagnostic =
            bridge.pluginDiagnostics.first {
                it["pluginKind"] == "native_frame_pipeline"
            }
        assertEquals("open", diagnostic["lifecycle"])
        assertNull(diagnostic["fallbackReason"])
    }

    @Test
    fun preferNativeFramePipelinePauseStopsPumpAndSeekRestartsWhenPlaying() {
        val initialSource =
            VesperPlayerSource.remote(
                uri = "https://example.com/video.mp4",
                label = "MP4",
                protocol = VesperPlayerSourceProtocol.Progressive,
            )
        val scheduler = ManualNativeFramePipelinePumpScheduler()
        val bindings = FakeBindings()
        val bridge =
            VesperNativePlayerBridge(
                bindings = bindings,
                initialSource = initialSource,
                sourceNormalizerConfiguration =
                    VesperSourceNormalizerConfiguration(
                        mode = VesperSourceNormalizerMode.PreflightOnly,
                        pluginLibraryPaths = listOf("/tmp/libsource_normalizer.so"),
                    ),
                nativeFramePipelineConfiguration =
                    VesperNativeFramePipelineConfiguration(
                        mode = VesperNativeFramePipelineMode.PreferNativeFrame,
                        decoderPluginLibraryPaths = listOf("/tmp/libmediacodec_decoder.so"),
                    ),
                nativeFramePipelinePumpScheduler = scheduler,
            )

        bridge.initialize()
        bridge.play()
        assertTrue(scheduler.hasPendingActions())

        bridge.pause()
        assertFalse(scheduler.hasPendingActions())
        val advanceCountAfterPause = bindings.advanceNativeFramePipelineCount
        scheduler.runNext()
        assertEquals(advanceCountAfterPause, bindings.advanceNativeFramePipelineCount)

        bridge.play()
        assertTrue(scheduler.hasPendingActions())
        bridge.seekBy(1_000L)

        assertEquals(1, bindings.flushNativeFramePipelineCount)
        assertEquals(listOf(1_000L), bindings.seekNativeFramePipelinePositions)
        assertTrue(scheduler.hasPendingActions())
    }

    @Test
    fun nativeFramePipelineSchedulerCloseClearsPendingWorkAndRejectsNewSchedules() {
        val scheduler = ManualNativeFramePipelinePumpScheduler()
        var runCount = 0

        scheduler.schedule(0L) { runCount += 1 }
        assertTrue(scheduler.hasPendingActions())

        scheduler.close()
        scheduler.runNext()
        scheduler.schedule(0L) { runCount += 1 }

        assertEquals(0, runCount)
        assertFalse(scheduler.hasPendingActions())
        assertTrue(scheduler.closeCount > 0)
    }

    @Test
    fun preferNativeFramePipelineBackgroundPumpIdleTickDoesNotCrashOnNullMainThreadResult() {
        val initialSource =
            VesperPlayerSource.remote(
                uri = "https://example.com/video.mp4",
                label = "MP4",
                protocol = VesperPlayerSourceProtocol.Progressive,
            )
        val scheduler = ThreadedNativeFramePipelinePumpScheduler()
        val bindings =
            FakeBindings(
                nativeFramePipelineAdvanceStatuses =
                    mutableListOf(
                        mapOf(
                            "status" to "pending",
                            "message" to "background warmup",
                        )
                    ),
            )
        val bridge =
            VesperNativePlayerBridge(
                bindings = bindings,
                initialSource = initialSource,
                sourceNormalizerConfiguration =
                    VesperSourceNormalizerConfiguration(
                        mode = VesperSourceNormalizerMode.PreflightOnly,
                        pluginLibraryPaths = listOf("/tmp/libsource_normalizer.so"),
                    ),
                nativeFramePipelineConfiguration =
                    VesperNativeFramePipelineConfiguration(
                        mode = VesperNativeFramePipelineMode.PreferNativeFrame,
                        decoderPluginLibraryPaths = listOf("/tmp/libmediacodec_decoder.so"),
                    ),
                nativeFramePipelinePumpScheduler = scheduler,
            )

        bridge.initialize()
        bridge.play()

        assertTrue(scheduler.awaitRun())
        assertNull(scheduler.lastError)
        assertTrue(bindings.advanceNativeFramePipelineCount >= 2)

        bridge.dispose()
        scheduler.close()
    }

    @Test
    fun preferNativeFramePipelinePauseDropsPendingHostTimedFrame() {
        val initialSource =
            VesperPlayerSource.remote(
                uri = "https://example.com/video.mp4",
                label = "MP4",
                protocol = VesperPlayerSourceProtocol.Progressive,
            )
        val scheduler = ManualNativeFramePipelinePumpScheduler()
        val bindings =
            FakeBindings(
                snapshot =
                    NativeBridgeSnapshot(
                        playbackState = PlaybackStateUi.Playing,
                        playbackRate = 1.0f,
                        isBuffering = false,
                        isInterrupted = false,
                        timeline =
                            TimelineUiState(
                                kind = TimelineKind.Vod,
                                isSeekable = true,
                                seekableRange = SeekableRangeUi(0L, 10_000L),
                                liveEdgeMs = null,
                                positionMs = 1_000L,
                                durationMs = 10_000L,
                            ),
                    ),
                nativeFramePipelineAdvanceStatuses =
                    mutableListOf(
                        mapOf("status" to "pending"),
                        mapOf(
                            "status" to "frame",
                            "handle" to 89L,
                            "presentationTimeUs" to 1_100_000L,
                            "requiresHostRelease" to true,
                        ),
                    ),
            )
        val bridge =
            VesperNativePlayerBridge(
                bindings = bindings,
                initialSource = initialSource,
                sourceNormalizerConfiguration =
                    VesperSourceNormalizerConfiguration(
                        mode = VesperSourceNormalizerMode.PreflightOnly,
                        pluginLibraryPaths = listOf("/tmp/libsource_normalizer.so"),
                    ),
                nativeFramePipelineConfiguration =
                    VesperNativeFramePipelineConfiguration(
                        mode = VesperNativeFramePipelineMode.PreferNativeFrame,
                        decoderPluginLibraryPaths = listOf("/tmp/libmediacodec_decoder.so"),
                    ),
                nativeFramePipelinePumpScheduler = scheduler,
            )

        bridge.initialize()
        bridge.play()
        scheduler.runNext()
        assertTrue(bindings.releasedNativeFramePipelineFrames.isEmpty())

        bridge.pause()
        scheduler.runNext()

        assertEquals(listOf(89L to false), bindings.releasedNativeFramePipelineFrames)
    }

    @Test
    fun requireNativeFramePipelinePauseReleaseFailureKeepsHardFailureState() {
        val initialSource =
            VesperPlayerSource.remote(
                uri = "https://example.com/video.mp4",
                label = "MP4",
                protocol = VesperPlayerSourceProtocol.Progressive,
            )
        val scheduler = ManualNativeFramePipelinePumpScheduler()
        val bindings =
            FakeBindings(
                snapshot =
                    NativeBridgeSnapshot(
                        playbackState = PlaybackStateUi.Playing,
                        playbackRate = 1.0f,
                        isBuffering = false,
                        isInterrupted = false,
                        timeline =
                            TimelineUiState(
                                kind = TimelineKind.Vod,
                                isSeekable = true,
                                seekableRange = SeekableRangeUi(0L, 10_000L),
                                liveEdgeMs = null,
                                positionMs = 1_000L,
                                durationMs = 10_000L,
                            ),
                    ),
                nativeFramePipelineAdvanceStatuses =
                    mutableListOf(
                        mapOf("status" to "pending"),
                        mapOf(
                            "status" to "frame",
                            "handle" to 95L,
                            "presentationTimeUs" to 1_100_000L,
                            "requiresHostRelease" to true,
                        ),
                    ),
            )
        val bridge =
            VesperNativePlayerBridge(
                bindings = bindings,
                initialSource = initialSource,
                sourceNormalizerConfiguration =
                    VesperSourceNormalizerConfiguration(
                        mode = VesperSourceNormalizerMode.PreflightOnly,
                        pluginLibraryPaths = listOf("/tmp/libsource_normalizer.so"),
                    ),
                nativeFramePipelineConfiguration =
                    VesperNativeFramePipelineConfiguration(
                        mode = VesperNativeFramePipelineMode.RequireNativeFrame,
                        decoderPluginLibraryPaths = listOf("/tmp/libmediacodec_decoder.so"),
                    ),
                nativeFramePipelinePumpScheduler = scheduler,
            )

        bridge.initialize()
        bridge.play()
        scheduler.runNext()
        bindings.nativeFramePipelineReleaseError =
            IllegalStateException("simulated release failure")

        bridge.pause()

        assertEquals(0, bindings.disposeCount)
        assertEquals(PlaybackStateUi.Ready, bridge.uiState.value.playbackState)
        assertTrue(bridge.uiState.value.subtitle.contains("simulated release failure"))
        assertTrue(
            bridge.pluginDiagnostics.any {
                it["pluginKind"] == "native_frame_pipeline" &&
                    it["participation"] == "selected" &&
                    it["route"] == "sdkManagedNativeFrame" &&
                    it["lifecycle"] == "failed" &&
                    it["fallbackTargetRoute"] == null &&
                    it["fallbackReason"] == "simulated release failure"
            }
        )
    }

    @Test
    fun requireNativeFramePipelineHardFailureIgnoresLaterPlaybackCommands() {
        val initialSource =
            VesperPlayerSource.remote(
                uri = "https://example.com/video.mp4",
                label = "MP4",
                protocol = VesperPlayerSourceProtocol.Progressive,
            )
        val bindings =
            FakeBindings(
                nativeFramePipelineSeekError =
                    IllegalStateException("simulated native-frame hard failure"),
            )
        val bridge =
            VesperNativePlayerBridge(
                bindings = bindings,
                initialSource = initialSource,
                sourceNormalizerConfiguration =
                    VesperSourceNormalizerConfiguration(
                        mode = VesperSourceNormalizerMode.PreflightOnly,
                        pluginLibraryPaths = listOf("/tmp/libsource_normalizer.so"),
                    ),
                nativeFramePipelineConfiguration =
                    VesperNativeFramePipelineConfiguration(
                        mode = VesperNativeFramePipelineMode.RequireNativeFrame,
                        decoderPluginLibraryPaths = listOf("/tmp/libmediacodec_decoder.so"),
                    ),
            )

        bridge.initialize()
        bridge.seekBy(1_000L)
        val playCountAfterFailure = bindings.playCount
        val stopCountAfterFailure = bindings.stopCount
        val seekToPositionsAfterFailure = bindings.seekToPositions.toList()
        val playbackRatesAfterFailure = bindings.playbackRates.toList()

        bridge.play()
        bridge.stop()
        bridge.seekBy(2_000L)
        bridge.setPlaybackRate(2.0f)

        assertEquals(playCountAfterFailure, bindings.playCount)
        assertEquals(stopCountAfterFailure, bindings.stopCount)
        assertEquals(seekToPositionsAfterFailure, bindings.seekToPositions)
        assertEquals(playbackRatesAfterFailure, bindings.playbackRates)
        assertEquals(PlaybackStateUi.Ready, bridge.uiState.value.playbackState)
        assertEquals(0L, bridge.uiState.value.timeline.positionMs)
        assertTrue(bridge.uiState.value.subtitle.contains("simulated native-frame hard failure"))
    }

    @Test
    fun requireNativeFramePipelineHardFailureIgnoresLaterConfigurationCommands() {
        val initialSource =
            VesperPlayerSource.remote(
                uri = "https://example.com/video.mp4",
                label = "MP4",
                protocol = VesperPlayerSourceProtocol.Progressive,
            )
        val bindings =
            FakeBindings(
                nativeFramePipelineSeekError =
                    IllegalStateException("simulated native-frame hard failure"),
            )
        val bridge =
            VesperNativePlayerBridge(
                bindings = bindings,
                initialSource = initialSource,
                sourceNormalizerConfiguration =
                    VesperSourceNormalizerConfiguration(
                        mode = VesperSourceNormalizerMode.PreflightOnly,
                        pluginLibraryPaths = listOf("/tmp/libsource_normalizer.so"),
                    ),
                nativeFramePipelineConfiguration =
                    VesperNativeFramePipelineConfiguration(
                        mode = VesperNativeFramePipelineMode.RequireNativeFrame,
                        decoderPluginLibraryPaths = listOf("/tmp/libmediacodec_decoder.so"),
                    ),
            )

        bridge.initialize()
        bridge.seekBy(1_000L)
        val initializedSourceAfterFailure = bindings.lastInitializedSource

        bridge.setVideoTrackSelection(VesperTrackSelection.track("video:720p"))
        bridge.setAudioTrackSelection(VesperTrackSelection.track("audio:main"))
        bridge.setSubtitleTrackSelection(VesperTrackSelection.disabled())
        bridge.setAbrPolicy(VesperAbrPolicy.fixedTrack("video:720p"))
        bridge.configureSystemPlayback(
            VesperSystemPlaybackConfiguration(
                metadata =
                    VesperSystemPlaybackMetadata(
                        title = "Ignored",
                        contentUri = initialSource.uri,
                    )
            )
        )
        bridge.updateSystemPlaybackMetadata(VesperSystemPlaybackMetadata(title = "Ignored"))
        bridge.clearSystemPlayback()
        bridge.setResiliencePolicy(VesperPlaybackResiliencePolicy.resilient())

        assertEquals(0, bindings.videoTrackSelectionCount)
        assertEquals(0, bindings.audioTrackSelectionCount)
        assertEquals(0, bindings.subtitleTrackSelectionCount)
        assertEquals(0, bindings.abrPolicyCount)
        assertEquals(0, bindings.configureSystemPlaybackCount)
        assertEquals(0, bindings.updateSystemPlaybackMetadataCount)
        assertEquals(1, bindings.clearSystemPlaybackCount)
        assertEquals(initializedSourceAfterFailure, bindings.lastInitializedSource)
        assertEquals(0, bindings.disposeCount)
        assertTrue(bridge.uiState.value.subtitle.contains("simulated native-frame hard failure"))
    }

    @Test
    fun requireNativeFramePipelineHardFailureIgnoresLaterRefreshAndNativeUpdates() {
        val initialSource =
            VesperPlayerSource.remote(
                uri = "https://example.com/video.mp4",
                label = "MP4",
                protocol = VesperPlayerSourceProtocol.Progressive,
            )
        val bindings =
            FakeBindings(
                nativeFramePipelineSeekError =
                    IllegalStateException("simulated native-frame hard failure"),
            )
        val bridge =
            VesperNativePlayerBridge(
                bindings = bindings,
                initialSource = initialSource,
                sourceNormalizerConfiguration =
                    VesperSourceNormalizerConfiguration(
                        mode = VesperSourceNormalizerMode.PreflightOnly,
                        pluginLibraryPaths = listOf("/tmp/libsource_normalizer.so"),
                    ),
                nativeFramePipelineConfiguration =
                    VesperNativeFramePipelineConfiguration(
                        mode = VesperNativeFramePipelineMode.RequireNativeFrame,
                        decoderPluginLibraryPaths = listOf("/tmp/libmediacodec_decoder.so"),
                    ),
            )

        bridge.initialize()
        val updateListener = checkNotNull(bindings.currentUpdateListener())
        bridge.seekBy(1_000L)
        val expectedUiState = bridge.uiState.value
        val refreshCountAfterFailure = bindings.refreshSnapshotCount

        bindings.snapshot =
            NativeBridgeSnapshot(
                playbackState = PlaybackStateUi.Playing,
                playbackRate = 2.0f,
                isBuffering = true,
                isInterrupted = true,
                timeline =
                    TimelineUiState(
                        kind = TimelineKind.Vod,
                        isSeekable = true,
                        seekableRange = SeekableRangeUi(0L, 10_000L),
                        liveEdgeMs = null,
                        positionMs = 5_000L,
                        durationMs = 10_000L,
                    ),
            )
        bindings.events.add(
            NativeBridgeEvent.PlaybackStateChanged(PlaybackStateUi.Playing)
        )
        bindings.events.add(
            NativeBridgeEvent.SeekCompleted(positionMs = 5_000L)
        )
        bindings.events.add(
            NativeBridgeEvent.Error(
                message = "stale playback error",
                codeOrdinal = 0,
                categoryOrdinal = 0,
                retriable = false,
            )
        )

        bridge.refresh()
        updateListener.invoke()

        assertEquals(expectedUiState, bridge.uiState.value)
        assertEquals(refreshCountAfterFailure + 1, bindings.refreshSnapshotCount)
        assertEquals(0, bindings.disposeCount)
        assertTrue(bridge.uiState.value.subtitle.contains("simulated native-frame hard failure"))
    }

    @Test
    fun preferNativeFramePipelineRefreshDoesNotReschedulePendingHostTimedFrame() {
        val initialSource =
            VesperPlayerSource.remote(
                uri = "https://example.com/video.mp4",
                label = "MP4",
                protocol = VesperPlayerSourceProtocol.Progressive,
            )
        val scheduler = ManualNativeFramePipelinePumpScheduler()
        val bindings =
            FakeBindings(
                snapshot =
                    NativeBridgeSnapshot(
                        playbackState = PlaybackStateUi.Playing,
                        playbackRate = 1.0f,
                        isBuffering = false,
                        isInterrupted = false,
                        timeline =
                            TimelineUiState(
                                kind = TimelineKind.Vod,
                                isSeekable = true,
                                seekableRange = SeekableRangeUi(0L, 10_000L),
                                liveEdgeMs = null,
                                positionMs = 1_000L,
                                durationMs = 10_000L,
                            ),
                    ),
                nativeFramePipelineAdvanceStatuses =
                    mutableListOf(
                        mapOf("status" to "pending"),
                        mapOf(
                            "status" to "frame",
                            "handle" to 90L,
                            "presentationTimeUs" to 1_080_000L,
                            "requiresHostRelease" to true,
                        ),
                    ),
            )
        val bridge =
            VesperNativePlayerBridge(
                bindings = bindings,
                initialSource = initialSource,
                sourceNormalizerConfiguration =
                    VesperSourceNormalizerConfiguration(
                        mode = VesperSourceNormalizerMode.PreflightOnly,
                        pluginLibraryPaths = listOf("/tmp/libsource_normalizer.so"),
                    ),
                nativeFramePipelineConfiguration =
                    VesperNativeFramePipelineConfiguration(
                        mode = VesperNativeFramePipelineMode.PreferNativeFrame,
                        decoderPluginLibraryPaths = listOf("/tmp/libmediacodec_decoder.so"),
                    ),
                nativeFramePipelinePumpScheduler = scheduler,
            )

        bridge.initialize()
        bridge.play()
        scheduler.runNext()
        assertTrue(bindings.releasedNativeFramePipelineFrames.isEmpty())

        bridge.refresh()
        assertTrue(bindings.releasedNativeFramePipelineFrames.isEmpty())

        scheduler.runNext()
        assertEquals(listOf(90L to true), bindings.releasedNativeFramePipelineFrames)
    }

    @Test
    fun preferNativeFramePipelineSelectSourceReleasesPendingHostTimedFrame() {
        val initialSource =
            VesperPlayerSource.remote(
                uri = "https://example.com/video.mp4",
                label = "MP4",
                protocol = VesperPlayerSourceProtocol.Progressive,
            )
        val scheduler = ManualNativeFramePipelinePumpScheduler()
        val bindings =
            FakeBindings(
                snapshot =
                    NativeBridgeSnapshot(
                        playbackState = PlaybackStateUi.Playing,
                        playbackRate = 1.0f,
                        isBuffering = false,
                        isInterrupted = false,
                        timeline =
                            TimelineUiState(
                                kind = TimelineKind.Vod,
                                isSeekable = true,
                                seekableRange = SeekableRangeUi(0L, 10_000L),
                                liveEdgeMs = null,
                                positionMs = 1_000L,
                                durationMs = 10_000L,
                            ),
                    ),
                nativeFramePipelineAdvanceStatuses =
                    mutableListOf(
                        mapOf("status" to "pending"),
                        mapOf(
                            "status" to "frame",
                            "handle" to 93L,
                            "presentationTimeUs" to 1_080_000L,
                            "requiresHostRelease" to true,
                        ),
                    ),
            )
        val bridge =
            VesperNativePlayerBridge(
                bindings = bindings,
                initialSource = initialSource,
                sourceNormalizerConfiguration =
                    VesperSourceNormalizerConfiguration(
                        mode = VesperSourceNormalizerMode.PreflightOnly,
                        pluginLibraryPaths = listOf("/tmp/libsource_normalizer.so"),
                    ),
                nativeFramePipelineConfiguration =
                    VesperNativeFramePipelineConfiguration(
                        mode = VesperNativeFramePipelineMode.PreferNativeFrame,
                        decoderPluginLibraryPaths = listOf("/tmp/libmediacodec_decoder.so"),
                    ),
                nativeFramePipelinePumpScheduler = scheduler,
            )

        bridge.initialize()
        bridge.play()
        scheduler.runNext()
        assertTrue(scheduler.hasPendingActions())
        val closeCountBeforeSelectSource = bindings.closeNativeFramePipelineCount
        val cancelCountBeforeSelectSource = scheduler.cancelCount

        bridge.selectSource(
            VesperPlayerSource.remote(
                uri = "https://example.com/next.mp4",
                label = "Next",
                protocol = VesperPlayerSourceProtocol.Progressive,
            )
        )

        assertTrue(scheduler.cancelCount > cancelCountBeforeSelectSource)
        assertEquals(listOf(93L to false), bindings.releasedNativeFramePipelineFrames)
        assertTrue(bindings.closeNativeFramePipelineCount > closeCountBeforeSelectSource)
    }

    @Test
    fun preferNativeFramePipelineInitializeReleasesPendingHostTimedFrame() {
        val initialSource =
            VesperPlayerSource.remote(
                uri = "https://example.com/video.mp4",
                label = "MP4",
                protocol = VesperPlayerSourceProtocol.Progressive,
            )
        val scheduler = ManualNativeFramePipelinePumpScheduler()
        val bindings =
            FakeBindings(
                snapshot =
                    NativeBridgeSnapshot(
                        playbackState = PlaybackStateUi.Playing,
                        playbackRate = 1.0f,
                        isBuffering = false,
                        isInterrupted = false,
                        timeline =
                            TimelineUiState(
                                kind = TimelineKind.Vod,
                                isSeekable = true,
                                seekableRange = SeekableRangeUi(0L, 10_000L),
                                liveEdgeMs = null,
                                positionMs = 1_000L,
                                durationMs = 10_000L,
                            ),
                    ),
                nativeFramePipelineAdvanceStatuses =
                    mutableListOf(
                        mapOf("status" to "pending"),
                        mapOf(
                            "status" to "frame",
                            "handle" to 94L,
                            "presentationTimeUs" to 1_080_000L,
                            "requiresHostRelease" to true,
                        ),
                    ),
            )
        val bridge =
            VesperNativePlayerBridge(
                bindings = bindings,
                initialSource = initialSource,
                sourceNormalizerConfiguration =
                    VesperSourceNormalizerConfiguration(
                        mode = VesperSourceNormalizerMode.PreflightOnly,
                        pluginLibraryPaths = listOf("/tmp/libsource_normalizer.so"),
                    ),
                nativeFramePipelineConfiguration =
                    VesperNativeFramePipelineConfiguration(
                        mode = VesperNativeFramePipelineMode.PreferNativeFrame,
                        decoderPluginLibraryPaths = listOf("/tmp/libmediacodec_decoder.so"),
                    ),
                nativeFramePipelinePumpScheduler = scheduler,
            )

        bridge.initialize()
        bridge.play()
        scheduler.runNext()
        assertTrue(scheduler.hasPendingActions())
        val closeCountBeforeInitialize = bindings.closeNativeFramePipelineCount
        val cancelCountBeforeInitialize = scheduler.cancelCount

        bridge.initialize()

        assertTrue(scheduler.cancelCount > cancelCountBeforeInitialize)
        assertEquals(listOf(94L to false), bindings.releasedNativeFramePipelineFrames)
        assertTrue(bindings.closeNativeFramePipelineCount > closeCountBeforeInitialize)
    }

    @Test
    fun preferNativeFramePipelineRateChangeReschedulesPendingHostTimedFrame() {
        val initialSource =
            VesperPlayerSource.remote(
                uri = "https://example.com/video.mp4",
                label = "MP4",
                protocol = VesperPlayerSourceProtocol.Progressive,
            )
        val scheduler = ManualNativeFramePipelinePumpScheduler()
        val bindings =
            FakeBindings(
                snapshot =
                    NativeBridgeSnapshot(
                        playbackState = PlaybackStateUi.Playing,
                        playbackRate = 1.0f,
                        isBuffering = false,
                        isInterrupted = false,
                        timeline =
                            TimelineUiState(
                                kind = TimelineKind.Vod,
                                isSeekable = true,
                                seekableRange = SeekableRangeUi(0L, 10_000L),
                                liveEdgeMs = null,
                                positionMs = 1_000L,
                                durationMs = 10_000L,
                            ),
                    ),
                nativeFramePipelineAdvanceStatuses =
                    mutableListOf(
                        mapOf("status" to "pending"),
                        mapOf(
                            "status" to "frame",
                            "handle" to 92L,
                            "presentationTimeUs" to 1_080_000L,
                            "requiresHostRelease" to true,
                        ),
                    ),
            )
        val bridge =
            VesperNativePlayerBridge(
                bindings = bindings,
                initialSource = initialSource,
                sourceNormalizerConfiguration =
                    VesperSourceNormalizerConfiguration(
                        mode = VesperSourceNormalizerMode.PreflightOnly,
                        pluginLibraryPaths = listOf("/tmp/libsource_normalizer.so"),
                    ),
                nativeFramePipelineConfiguration =
                    VesperNativeFramePipelineConfiguration(
                        mode = VesperNativeFramePipelineMode.PreferNativeFrame,
                        decoderPluginLibraryPaths = listOf("/tmp/libmediacodec_decoder.so"),
                    ),
                nativeFramePipelinePumpScheduler = scheduler,
            )

        bridge.initialize()
        bridge.play()
        scheduler.runNext()
        assertEquals(80L, scheduler.lastDelayMs)

        bindings.snapshot =
            NativeBridgeSnapshot(
                playbackState = PlaybackStateUi.Playing,
                playbackRate = 2.0f,
                isBuffering = false,
                isInterrupted = false,
                timeline =
                    TimelineUiState(
                        kind = TimelineKind.Vod,
                        isSeekable = true,
                        seekableRange = SeekableRangeUi(0L, 10_000L),
                        liveEdgeMs = null,
                        positionMs = 1_000L,
                        durationMs = 10_000L,
                    ),
            )
        bridge.setPlaybackRate(2.0f)

        assertEquals(40L, scheduler.lastDelayMs)
        scheduler.runNext()
        assertEquals(listOf(92L to true), bindings.releasedNativeFramePipelineFrames)
    }

    @Test
    fun preferNativeFramePipelineStopFlushesOpenSession() {
        val initialSource =
            VesperPlayerSource.remote(
                uri = "https://example.com/video.mp4",
                label = "MP4",
                protocol = VesperPlayerSourceProtocol.Progressive,
            )
        val bindings = FakeBindings()
        val bridge =
            VesperNativePlayerBridge(
                bindings = bindings,
                initialSource = initialSource,
                sourceNormalizerConfiguration =
                    VesperSourceNormalizerConfiguration(
                        mode = VesperSourceNormalizerMode.PreflightOnly,
                        pluginLibraryPaths = listOf("/tmp/libsource_normalizer.so"),
                    ),
                nativeFramePipelineConfiguration =
                    VesperNativeFramePipelineConfiguration(
                        mode = VesperNativeFramePipelineMode.PreferNativeFrame,
                        decoderPluginLibraryPaths = listOf("/tmp/libmediacodec_decoder.so"),
                    ),
            )

        bridge.initialize()
        bridge.stop()

        assertEquals(1, bindings.flushNativeFramePipelineCount)
    }

    @Test
    fun disposeClosesOpenNativeFramePipelineSession() {
        val initialSource =
            VesperPlayerSource.remote(
                uri = "https://example.com/video.mp4",
                label = "MP4",
                protocol = VesperPlayerSourceProtocol.Progressive,
            )
        val bindings = FakeBindings()
        val bridge =
            VesperNativePlayerBridge(
                bindings = bindings,
                initialSource = initialSource,
                sourceNormalizerConfiguration =
                    VesperSourceNormalizerConfiguration(
                        mode = VesperSourceNormalizerMode.PreflightOnly,
                        pluginLibraryPaths = listOf("/tmp/libsource_normalizer.so"),
                    ),
                nativeFramePipelineConfiguration =
                    VesperNativeFramePipelineConfiguration(
                        mode = VesperNativeFramePipelineMode.PreferNativeFrame,
                        decoderPluginLibraryPaths = listOf("/tmp/libmediacodec_decoder.so"),
                    ),
            )

        bridge.initialize()
        bridge.dispose()

        assertEquals(1, bindings.closeNativeFramePipelineCount)
        assertEquals(1, bindings.disposeCount)
    }

    @Test
    fun textureViewNativeFramePipelineStillFallsBackToSystemPlayback() {
        val initialSource =
            VesperPlayerSource.remote(
                uri = "https://example.com/video.mp4",
                label = "MP4",
                protocol = VesperPlayerSourceProtocol.Progressive,
            )
        val bindings = FakeBindings()
        val bridge =
            VesperNativePlayerBridge(
                bindings = bindings,
                initialSource = initialSource,
                surfaceKind = NativeVideoSurfaceKind.TextureView,
                sourceNormalizerConfiguration =
                    VesperSourceNormalizerConfiguration(
                        mode = VesperSourceNormalizerMode.PreflightOnly,
                        pluginLibraryPaths = listOf("/tmp/libsource_normalizer.so"),
                    ),
                nativeFramePipelineConfiguration =
                    VesperNativeFramePipelineConfiguration(
                        mode = VesperNativeFramePipelineMode.PreferNativeFrame,
                        decoderPluginLibraryPaths = listOf("/tmp/libmediacodec_decoder.so"),
                    ),
            )

        bridge.initialize()

        assertEquals(initialSource, bindings.lastInitializedSource)
        assertEquals(0, bindings.openNativeFramePipelineCount)
        assertTrue(
            bridge.pluginDiagnostics.any {
                it["pluginKind"] == "native_frame_pipeline" &&
                    it["participation"] == "fallback" &&
                    it["route"] == "systemPlayer" &&
                    it["fallbackReason"].toString().contains("TextureView")
            }
        )
    }

    @Test
    fun preferNativeFramePipelineOpenFailureFallsBackToSystemPlayback() {
        val initialSource =
            VesperPlayerSource.remote(
                uri = "https://example.com/video.mp4",
                label = "MP4",
                protocol = VesperPlayerSourceProtocol.Progressive,
            )
        val bindings =
            FakeBindings(
                nativeFramePipelineOpenError =
                    IllegalStateException("simulated native-frame open failure"),
            )
        val bridge =
            VesperNativePlayerBridge(
                bindings = bindings,
                initialSource = initialSource,
                sourceNormalizerConfiguration =
                    VesperSourceNormalizerConfiguration(
                        mode = VesperSourceNormalizerMode.PreflightOnly,
                        pluginLibraryPaths = listOf("/tmp/libsource_normalizer.so"),
                    ),
                nativeFramePipelineConfiguration =
                    VesperNativeFramePipelineConfiguration(
                        mode = VesperNativeFramePipelineMode.PreferNativeFrame,
                        decoderPluginLibraryPaths = listOf("/tmp/libmediacodec_decoder.so"),
                    ),
            )

        bridge.initialize()

        assertEquals(initialSource, bindings.lastInitializedSource)
        assertEquals(1, bindings.openNativeFramePipelineCount)
        assertEquals(0, bindings.advanceNativeFramePipelineCount)
        assertEquals(0, bindings.disposeCount)
        assertTrue(
            bridge.pluginDiagnostics.any {
                it["pluginKind"] == "native_frame_pipeline" &&
                    it["participation"] == "fallback" &&
                    it["route"] == "systemPlayer" &&
                    it["fallbackReason"] == "simulated native-frame open failure"
            }
        )
    }

    @Test
    fun preferNativeFramePipelineFallbackKeepsSystemPlayerCommandsActive() {
        val initialSource =
            VesperPlayerSource.remote(
                uri = "https://example.com/video.mp4",
                label = "MP4",
                protocol = VesperPlayerSourceProtocol.Progressive,
            )
        val bindings =
            FakeBindings(
                nativeFramePipelineOpenError =
                    IllegalStateException("simulated native-frame open failure"),
            )
        val bridge =
            VesperNativePlayerBridge(
                bindings = bindings,
                initialSource = initialSource,
                sourceNormalizerConfiguration =
                    VesperSourceNormalizerConfiguration(
                        mode = VesperSourceNormalizerMode.PreflightOnly,
                        pluginLibraryPaths = listOf("/tmp/libsource_normalizer.so"),
                    ),
                nativeFramePipelineConfiguration =
                    VesperNativeFramePipelineConfiguration(
                        mode = VesperNativeFramePipelineMode.PreferNativeFrame,
                        decoderPluginLibraryPaths = listOf("/tmp/libmediacodec_decoder.so"),
                    ),
            )

        bridge.initialize()
        bridge.play()
        bridge.seekBy(2_000L)
        bridge.setPlaybackRate(1.5f)
        bridge.setVideoTrackSelection(VesperTrackSelection.track("video:720p"))
        bridge.setAudioTrackSelection(VesperTrackSelection.track("audio:main"))
        bridge.setSubtitleTrackSelection(VesperTrackSelection.disabled())
        bridge.setAbrPolicy(VesperAbrPolicy.fixedTrack("video:720p"))
        bridge.configureSystemPlayback(
            VesperSystemPlaybackConfiguration(
                metadata =
                    VesperSystemPlaybackMetadata(
                        title = "Fallback",
                        contentUri = initialSource.uri,
                    )
            )
        )
        bridge.updateSystemPlaybackMetadata(VesperSystemPlaybackMetadata(title = "Fallback"))
        bridge.clearSystemPlayback()

        assertEquals(1, bindings.playCount)
        assertEquals(listOf(2_000L), bindings.seekToPositions)
        assertEquals(listOf(1.5f), bindings.playbackRates)
        assertEquals(1, bindings.videoTrackSelectionCount)
        assertEquals(1, bindings.audioTrackSelectionCount)
        assertEquals(1, bindings.subtitleTrackSelectionCount)
        assertEquals(1, bindings.abrPolicyCount)
        assertEquals(1, bindings.configureSystemPlaybackCount)
        assertEquals(1, bindings.updateSystemPlaybackMetadataCount)
        assertEquals(1, bindings.clearSystemPlaybackCount)
        val diagnostic =
            bridge.pluginDiagnostics.first {
                it["pluginKind"] == "native_frame_pipeline"
            }
        assertEquals("fallback", diagnostic["participation"])
        assertEquals("systemPlayer", diagnostic["route"])
        assertEquals("systemPlayer", diagnostic["fallbackTargetRoute"])
        assertEquals("simulated native-frame open failure", diagnostic["fallbackReason"])
    }

    @Test
    fun preferNativeFramePipelineSelectSourceClearsFallbackAndRetriesNativeFrame() {
        val initialSource =
            VesperPlayerSource.remote(
                uri = "https://example.com/video.mp4",
                label = "MP4",
                protocol = VesperPlayerSourceProtocol.Progressive,
            )
        val nextSource =
            VesperPlayerSource.remote(
                uri = "https://example.com/next.mp4",
                label = "Next MP4",
                protocol = VesperPlayerSourceProtocol.Progressive,
            )
        val scheduler = ManualNativeFramePipelinePumpScheduler()
        val bindings =
            FakeBindings(
                nativeFramePipelineOpenError =
                    IllegalStateException("simulated native-frame open failure"),
            )
        val bridge =
            VesperNativePlayerBridge(
                bindings = bindings,
                initialSource = initialSource,
                sourceNormalizerConfiguration =
                    VesperSourceNormalizerConfiguration(
                        mode = VesperSourceNormalizerMode.PreflightOnly,
                        pluginLibraryPaths = listOf("/tmp/libsource_normalizer.so"),
                    ),
                nativeFramePipelineConfiguration =
                    VesperNativeFramePipelineConfiguration(
                        mode = VesperNativeFramePipelineMode.PreferNativeFrame,
                        decoderPluginLibraryPaths = listOf("/tmp/libmediacodec_decoder.so"),
                    ),
                nativeFramePipelinePumpScheduler = scheduler,
            )

        bridge.initialize()
        assertTrue(
            bridge.pluginDiagnostics.any {
                it["pluginKind"] == "native_frame_pipeline" &&
                    it["lifecycle"] == "fallback"
            }
        )

        bindings.nativeFramePipelineOpenError = null
        bridge.selectSource(nextSource)

        assertEquals(2, bindings.openNativeFramePipelineCount)
        assertEquals(nextSource, bindings.lastNativeFramePipelineSource)
        assertEquals(1, bindings.playCount)
        assertTrue(scheduler.hasPendingActions())
        val diagnostic =
            bridge.pluginDiagnostics.first {
                it["pluginKind"] == "native_frame_pipeline"
            }
        assertEquals("selected", diagnostic["participation"])
        assertEquals("sdkManagedNativeFrame", diagnostic["route"])
        assertEquals("open", diagnostic["lifecycle"])
        assertNull(diagnostic["fallbackTargetRoute"])
        assertNull(diagnostic["fallbackReason"])
    }

    @Test
    fun requireNativeFramePipelineOpenFailureKeepsBridgeRecoverable() {
        val initialSource =
            VesperPlayerSource.remote(
                uri = "https://example.com/video.mp4",
                label = "MP4",
                protocol = VesperPlayerSourceProtocol.Progressive,
            )
        val bindings =
            FakeBindings(
                nativeFramePipelineOpenError =
                    IllegalStateException("simulated native-frame open failure"),
            )
        val bridge =
            VesperNativePlayerBridge(
                bindings = bindings,
                initialSource = initialSource,
                sourceNormalizerConfiguration =
                    VesperSourceNormalizerConfiguration(
                        mode = VesperSourceNormalizerMode.PreflightOnly,
                        pluginLibraryPaths = listOf("/tmp/libsource_normalizer.so"),
                    ),
                nativeFramePipelineConfiguration =
                    VesperNativeFramePipelineConfiguration(
                        mode = VesperNativeFramePipelineMode.RequireNativeFrame,
                        decoderPluginLibraryPaths = listOf("/tmp/libmediacodec_decoder.so"),
                    ),
            )

        bridge.initialize()

        assertEquals(initialSource, bindings.lastInitializedSource)
        assertEquals(1, bindings.openNativeFramePipelineCount)
        assertEquals(0, bindings.advanceNativeFramePipelineCount)
        assertEquals(0, bindings.disposeCount)
        assertTrue(bridge.uiState.value.subtitle.contains("simulated native-frame open failure"))
        assertTrue(
            bridge.pluginDiagnostics.any {
                it["pluginKind"] == "native_frame_pipeline" &&
                    it["participation"] == "selected" &&
                    it["route"] == "sdkManagedNativeFrame" &&
                    it["lifecycle"] == "failed" &&
                    it["fallbackTargetRoute"] == null &&
                    it["status"] == "unsupported" &&
                    it["fallbackReason"] == "simulated native-frame open failure"
            }
        )
    }

    @Test
    fun nativeFramePipelineDiagnosticsSurviveNativeStartupDiagnosticsReplacement() {
        val initialSource =
            VesperPlayerSource.remote(
                uri = "https://example.com/video.mp4",
                label = "MP4",
                protocol = VesperPlayerSourceProtocol.Progressive,
            )
        val bindings =
            FakeBindings(
                nativeStartupDiagnostics =
                    listOf(
                        mapOf(
                            "pluginKind" to "source_normalizer",
                            "status" to "sourceNormalizerSupported",
                            "participation" to "participated",
                        )
                    )
            )
        val bridge =
            VesperNativePlayerBridge(
                bindings = bindings,
                initialSource = initialSource,
                nativeFramePipelineConfiguration =
                    VesperNativeFramePipelineConfiguration(
                        mode = VesperNativeFramePipelineMode.DiagnosticsOnly,
                        frameProcessorPluginLibraryPaths = listOf("/tmp/libframe.so"),
                    ),
            )

        bridge.initialize()

        assertTrue(
            bridge.pluginDiagnostics.any {
                it["pluginKind"] == "source_normalizer" &&
                    it["participation"] == "participated"
            }
        )
        assertTrue(
            bridge.pluginDiagnostics.any {
                it["pluginKind"] == "native_frame_pipeline" &&
                    it["participation"] == "available" &&
                    it["route"] == "systemPlayer"
            }
        )
    }

    @Test
    fun requireNativeFramePipelineFailsWithoutInitializingSystemPlayback() {
        val initialSource =
            VesperPlayerSource.remote(
                uri = "https://example.com/video.mp4",
                label = "MP4",
                protocol = VesperPlayerSourceProtocol.Progressive,
            )
        val bindings = FakeBindings()
        val bridge =
            VesperNativePlayerBridge(
                bindings = bindings,
                initialSource = initialSource,
                nativeFramePipelineConfiguration =
                    VesperNativeFramePipelineConfiguration(
                        mode = VesperNativeFramePipelineMode.RequireNativeFrame,
                        decoderPluginLibraryPaths = listOf("/tmp/libdecoder.so"),
                    ),
            )

        bridge.initialize()

        assertNull(bindings.lastInitializedSource)
        assertTrue(bridge.uiState.value.subtitle.contains("SourceNormalizer packet-stream"))
        assertTrue(
            bridge.pluginDiagnostics.any {
                it["pluginKind"] == "native_frame_pipeline" &&
                    it["participation"] == "selected" &&
                    it["route"] == "sdkManagedNativeFrame" &&
                    it["lifecycle"] == "failed" &&
                    it["fallbackTargetRoute"] == null &&
                    it["status"] == "unsupported"
            }
        )
    }

    @Test
    fun disposeClearsEffectiveVideoTrackIdImmediately() {
        val bindings =
            FakeBindings(
                trackCatalog =
                    VesperTrackCatalog(
                        tracks =
                            listOf(
                                VesperMediaTrack(
                                    id = "video:720p",
                                    kind = VesperMediaTrackKind.Video,
                                    height = 720,
                                    bitRate = 1_500_000L,
                                )
                            )
                    ),
                trackSelection =
                    VesperTrackSelectionSnapshot(
                        abrPolicy = VesperAbrPolicy.fixedTrack("video:720p"),
                    ),
                effectiveVideoTrackId = "video:720p",
                videoVariantObservation =
                    VesperVideoVariantObservation(
                        bitRate = 1_500_000L,
                        width = 1280,
                        height = 720,
                    ),
            )
        val bridge = VesperNativePlayerBridge(bindings = bindings)

        bridge.refresh()
        assertEquals("video:720p", bridge.effectiveVideoTrackId.value)
        assertEquals(1280, bridge.videoVariantObservation.value?.width)

        bridge.dispose()
        assertEquals(VesperTrackCatalog.Empty, bridge.trackCatalog.value)
        assertEquals(VesperTrackSelectionSnapshot(), bridge.trackSelection.value)
        assertNull(bridge.effectiveVideoTrackId.value)
        assertNull(bridge.videoVariantObservation.value)

        bridge.refresh()
        assertEquals(VesperTrackCatalog.Empty, bridge.trackCatalog.value)
        assertEquals(VesperTrackSelectionSnapshot(), bridge.trackSelection.value)
        assertNull(bridge.effectiveVideoTrackId.value)
        assertNull(bridge.videoVariantObservation.value)
    }

    @Test
    fun disposeOnlyDelegatesOnce() {
        val bindings = FakeBindings()
        val bridge = VesperNativePlayerBridge(bindings = bindings)

        bridge.dispose()
        bridge.dispose()

        assertEquals(1, bindings.disposeCount)
    }

    @Test
    fun selectSourceFailureClearsStaleTrackState() {
        val bindings =
            FakeBindings(
                trackCatalog =
                    VesperTrackCatalog(
                        tracks =
                            listOf(
                                VesperMediaTrack(
                                    id = "video:old",
                                    kind = VesperMediaTrackKind.Video,
                                    height = 720,
                                    bitRate = 1_500_000L,
                                )
                            )
                    ),
                trackSelection =
                    VesperTrackSelectionSnapshot(
                        abrPolicy = VesperAbrPolicy.fixedTrack("video:old"),
                    ),
                effectiveVideoTrackId = "video:old",
                videoVariantObservation =
                    VesperVideoVariantObservation(
                        bitRate = 1_500_000L,
                        width = 1280,
                        height = 720,
                    ),
            )
        val bridge = VesperNativePlayerBridge(bindings = bindings)

        bridge.refresh()
        assertEquals(1, bridge.trackCatalog.value.tracks.size)
        assertEquals(
            VesperAbrPolicy.fixedTrack("video:old"),
            bridge.trackSelection.value.abrPolicy,
        )
        assertEquals("video:old", bridge.effectiveVideoTrackId.value)

        bindings.onInitialize = { error("simulated initialize failure") }

        bridge.selectSource(VesperPlayerSource.hls("https://example.com/next.m3u8", "Next"))

        assertEquals(VesperTrackCatalog.Empty, bridge.trackCatalog.value)
        assertEquals(VesperTrackSelectionSnapshot(), bridge.trackSelection.value)
        assertNull(bridge.effectiveVideoTrackId.value)
        assertNull(bridge.videoVariantObservation.value)
    }

    @Test
    fun staleNativeUpdateListenerFromPreviousSourceIsIgnored() {
        val oldTrackCatalog =
            VesperTrackCatalog(
                tracks =
                    listOf(
                        VesperMediaTrack(
                            id = "video:old",
                            kind = VesperMediaTrackKind.Video,
                            height = 720,
                            bitRate = 1_500_000L,
                        )
                    )
            )
        val oldTrackSelection =
            VesperTrackSelectionSnapshot(
                abrPolicy = VesperAbrPolicy.fixedTrack("video:old"),
            )
        val oldObservation =
            VesperVideoVariantObservation(
                bitRate = 1_500_000L,
                width = 1280,
                height = 720,
            )
        val newTrackCatalog =
            VesperTrackCatalog(
                tracks =
                    listOf(
                        VesperMediaTrack(
                            id = "video:new",
                            kind = VesperMediaTrackKind.Video,
                            height = 1080,
                            bitRate = 3_000_000L,
                        )
                    )
            )
        val newTrackSelection =
            VesperTrackSelectionSnapshot(
                abrPolicy = VesperAbrPolicy.auto(),
            )
        val newObservation =
            VesperVideoVariantObservation(
                bitRate = 3_000_000L,
                width = 1920,
                height = 1080,
            )
        val bindings =
            FakeBindings(
                trackCatalog = oldTrackCatalog,
                trackSelection = oldTrackSelection,
                effectiveVideoTrackId = "video:old",
                videoVariantObservation = oldObservation,
            )
        val bridge = VesperNativePlayerBridge(bindings = bindings)

        bridge.refresh()
        val staleListener = checkNotNull(bindings.currentUpdateListener())
        bindings.onInitialize = {
            bindings.trackCatalog = newTrackCatalog
            bindings.trackSelection = newTrackSelection
            bindings.effectiveVideoTrackId = "video:new"
            bindings.videoVariantObservation = newObservation
            bindings.events.clear()
        }

        bridge.selectSource(VesperPlayerSource.hls("https://example.com/next.m3u8", "Next"))

        val expectedUiState = bridge.uiState.value
        assertEquals(newTrackCatalog, bridge.trackCatalog.value)
        assertEquals(newTrackSelection, bridge.trackSelection.value)
        assertEquals("video:new", bridge.effectiveVideoTrackId.value)
        assertEquals(newObservation, bridge.videoVariantObservation.value)

        bindings.trackCatalog = oldTrackCatalog
        bindings.trackSelection = oldTrackSelection
        bindings.effectiveVideoTrackId = "video:old"
        bindings.videoVariantObservation = oldObservation
        bindings.events.add(
            NativeBridgeEvent.Error(
                message = "stale old error",
                codeOrdinal = 0,
                categoryOrdinal = 0,
                retriable = false,
            )
        )

        staleListener.invoke()

        assertEquals(expectedUiState, bridge.uiState.value)
        assertEquals(newTrackCatalog, bridge.trackCatalog.value)
        assertEquals(newTrackSelection, bridge.trackSelection.value)
        assertEquals("video:new", bridge.effectiveVideoTrackId.value)
        assertEquals(newObservation, bridge.videoVariantObservation.value)
    }

    @Test
    fun staleNativeUpdateListenerAfterDisposeIsIgnored() {
        val staleTrackCatalog =
            VesperTrackCatalog(
                tracks =
                    listOf(
                        VesperMediaTrack(
                            id = "video:stale",
                            kind = VesperMediaTrackKind.Video,
                            height = 720,
                            bitRate = 1_500_000L,
                        )
                    )
            )
        val staleTrackSelection =
            VesperTrackSelectionSnapshot(
                abrPolicy = VesperAbrPolicy.fixedTrack("video:stale"),
            )
        val staleObservation =
            VesperVideoVariantObservation(
                bitRate = 1_500_000L,
                width = 1280,
                height = 720,
            )
        val bindings =
            FakeBindings(
                trackCatalog = staleTrackCatalog,
                trackSelection = staleTrackSelection,
                effectiveVideoTrackId = "video:stale",
                videoVariantObservation = staleObservation,
            )
        val bridge = VesperNativePlayerBridge(bindings = bindings)

        bridge.refresh()
        val staleListener = checkNotNull(bindings.currentUpdateListener())

        bridge.dispose()
        val expectedUiState = bridge.uiState.value

        bindings.trackCatalog = staleTrackCatalog
        bindings.trackSelection = staleTrackSelection
        bindings.effectiveVideoTrackId = "video:stale"
        bindings.videoVariantObservation = staleObservation
        bindings.events.add(
            NativeBridgeEvent.Error(
                message = "stale disposed error",
                codeOrdinal = 0,
                categoryOrdinal = 0,
                retriable = false,
            )
        )

        staleListener.invoke()

        assertEquals(expectedUiState, bridge.uiState.value)
        assertEquals(VesperTrackCatalog.Empty, bridge.trackCatalog.value)
        assertEquals(VesperTrackSelectionSnapshot(), bridge.trackSelection.value)
        assertNull(bridge.effectiveVideoTrackId.value)
        assertNull(bridge.videoVariantObservation.value)
    }

    @Test
    fun staleNativeUpdateListenerAfterResilienceReinitIsIgnored() {
        val oldTrackCatalog =
            VesperTrackCatalog(
                tracks =
                    listOf(
                        VesperMediaTrack(
                            id = "video:old",
                            kind = VesperMediaTrackKind.Video,
                            height = 720,
                            bitRate = 1_500_000L,
                        )
                    )
            )
        val oldTrackSelection =
            VesperTrackSelectionSnapshot(
                abrPolicy = VesperAbrPolicy.fixedTrack("video:old"),
            )
        val oldObservation =
            VesperVideoVariantObservation(
                bitRate = 1_500_000L,
                width = 1280,
                height = 720,
            )
        val reinitTrackCatalog =
            VesperTrackCatalog(
                tracks =
                    listOf(
                        VesperMediaTrack(
                            id = "video:reinit",
                            kind = VesperMediaTrackKind.Video,
                            height = 1080,
                            bitRate = 3_000_000L,
                        )
                    )
            )
        val reinitTrackSelection =
            VesperTrackSelectionSnapshot(
                abrPolicy = VesperAbrPolicy.auto(),
            )
        val reinitObservation =
            VesperVideoVariantObservation(
                bitRate = 3_000_000L,
                width = 1920,
                height = 1080,
            )
        val bindings =
            FakeBindings(
                trackCatalog = oldTrackCatalog,
                trackSelection = oldTrackSelection,
                effectiveVideoTrackId = "video:old",
                videoVariantObservation = oldObservation,
            )
        val bridge =
            VesperNativePlayerBridge(
                bindings = bindings,
                initialSource = VesperPlayerSource.hls("https://example.com/live.m3u8", "Live"),
            )

        bridge.initialize()
        bridge.refresh()
        val staleListener = checkNotNull(bindings.currentUpdateListener())

        bindings.onInitialize = {
            bindings.trackCatalog = reinitTrackCatalog
            bindings.trackSelection = reinitTrackSelection
            bindings.effectiveVideoTrackId = "video:reinit"
            bindings.videoVariantObservation = reinitObservation
            bindings.events.clear()
        }

        bridge.setResiliencePolicy(VesperPlaybackResiliencePolicy.resilient())

        val expectedUiState = bridge.uiState.value
        assertEquals(reinitTrackCatalog, bridge.trackCatalog.value)
        assertEquals(reinitTrackSelection, bridge.trackSelection.value)
        assertEquals("video:reinit", bridge.effectiveVideoTrackId.value)
        assertEquals(reinitObservation, bridge.videoVariantObservation.value)

        bindings.trackCatalog = oldTrackCatalog
        bindings.trackSelection = oldTrackSelection
        bindings.effectiveVideoTrackId = "video:old"
        bindings.videoVariantObservation = oldObservation
        bindings.events.add(
            NativeBridgeEvent.Error(
                message = "stale resilience error",
                codeOrdinal = 0,
                categoryOrdinal = 0,
                retriable = false,
            )
        )

        staleListener.invoke()

        assertEquals(expectedUiState, bridge.uiState.value)
        assertEquals(reinitTrackCatalog, bridge.trackCatalog.value)
        assertEquals(reinitTrackSelection, bridge.trackSelection.value)
        assertEquals("video:reinit", bridge.effectiveVideoTrackId.value)
        assertEquals(reinitObservation, bridge.videoVariantObservation.value)
    }

    @Test
    fun resolveVideoVariantObservationUsesRenderedFormat() {
        val observation =
            resolveVideoVariantObservation(
                Format.Builder()
                    .setPeakBitrate(1_500_000)
                    .setWidth(1280)
                    .setHeight(720)
                    .build(),
            )

        assertEquals(
            VesperVideoVariantObservation(
                bitRate = 1_500_000L,
                width = 1280,
                height = 720,
            ),
            observation,
        )
    }

    @Test
    fun resolveVideoVariantObservationReturnsNilWhenFormatLacksSignal() {
        assertNull(resolveVideoVariantObservation(Format.Builder().build()))
    }

    @Test
    fun resolveEffectiveVideoTrackIdUsesCurrentRenderedFormat() {
        val effectiveTrackId =
            resolveEffectiveVideoTrackId(
                videoTracks =
                    listOf(
                        VesperMediaTrack(
                            id = "group:video-480:0",
                            kind = VesperMediaTrackKind.Video,
                            codec = "avc1.4d401f",
                            bitRate = 900_000L,
                            width = 854,
                            height = 480,
                            frameRate = 30f,
                        ),
                        VesperMediaTrack(
                            id = "group:video-720:1",
                            kind = VesperMediaTrackKind.Video,
                            codec = "avc1.4d401f",
                            bitRate = 1_500_000L,
                            width = 1280,
                            height = 720,
                            frameRate = 30f,
                        ),
                    ),
                currentVideoFormat =
                    Format.Builder()
                        .setId("video-720")
                        .setCodecs("avc1.4d401f")
                        .setPeakBitrate(1_500_000)
                        .setWidth(1280)
                        .setHeight(720)
                        .setFrameRate(30f)
                        .build(),
            )

        assertEquals("group:video-720:1", effectiveTrackId)
    }

    @Test
    fun resolveEffectiveVideoTrackIdStaysNilWhenFormatIsTooAmbiguous() {
        val effectiveTrackId =
            resolveEffectiveVideoTrackId(
                videoTracks =
                    listOf(
                        VesperMediaTrack(
                            id = "group:video-480:0",
                            kind = VesperMediaTrackKind.Video,
                            codec = "avc1.4d401f",
                            bitRate = 900_000L,
                        ),
                        VesperMediaTrack(
                            id = "group:video-720:1",
                            kind = VesperMediaTrackKind.Video,
                            codec = "avc1.4d401f",
                            bitRate = 1_500_000L,
                        ),
                    ),
                currentVideoFormat =
                    Format.Builder()
                        .setCodecs("avc1.4d401f")
                        .build(),
            )

        assertNull(effectiveTrackId)
    }
}

private class FakeBindings(
    var snapshot: NativeBridgeSnapshot? = null,
    var trackCatalog: VesperTrackCatalog = VesperTrackCatalog.Empty,
    var trackSelection: VesperTrackSelectionSnapshot = VesperTrackSelectionSnapshot(),
    var effectiveVideoTrackId: String? = null,
    var videoVariantObservation: VesperVideoVariantObservation? = null,
    var mobilePluginDiagnostics: List<Map<String, Any?>> = emptyList(),
    var nativeStartupDiagnostics: List<Map<String, Any?>> = emptyList(),
    var nativeFramePipelineOpenError: Throwable? = null,
    var nativeFramePipelineAdvanceError: Throwable? = null,
    var nativeFramePipelineReleaseError: Throwable? = null,
    var nativeFramePipelineFlushError: Throwable? = null,
    var nativeFramePipelineSeekError: Throwable? = null,
    var nativeFramePipelineAdvanceStatus: Map<String, Any?>? = null,
    var nativeFramePipelineAdvanceStatuses: MutableList<Map<String, Any?>> = mutableListOf(),
) : VesperNativeBindings {
    var onInitialize: (() -> Unit)? = null
    val events = mutableListOf<NativeBridgeEvent>()
    var disposeCount = 0
    var openNativeFramePipelineCount = 0
    var advanceNativeFramePipelineCount = 0
    var flushNativeFramePipelineCount = 0
    var closeNativeFramePipelineCount = 0
    var playCount = 0
    var pauseCount = 0
    var stopCount = 0
    var videoTrackSelectionCount = 0
    var audioTrackSelectionCount = 0
    var subtitleTrackSelectionCount = 0
    var abrPolicyCount = 0
    var configureSystemPlaybackCount = 0
    var updateSystemPlaybackMetadataCount = 0
    var clearSystemPlaybackCount = 0
    var refreshSnapshotCount = 0
    val releasedNativeFramePipelineFrames = mutableListOf<Pair<Long, Boolean>>()
    val seekNativeFramePipelinePositions = mutableListOf<Long>()
    val seekToPositions = mutableListOf<Long>()
    val playbackRates = mutableListOf<Float>()
    var lastProbeSource: VesperPlayerSource? = null
    var lastSourceNormalizerConfiguration: VesperSourceNormalizerConfiguration? = null
    var lastFrameProcessorConfiguration: VesperFrameProcessorConfiguration? = null
    var lastInitializedSource: VesperPlayerSource? = null
    var lastSystemPlaybackUsesSourceNormalizerResource: Boolean? = null
    var lastSystemPlaybackVideoEnabled: Boolean? = null
    var lastNativeFramePipelineSource: VesperPlayerSource? = null
    var lastNativeFramePipelineSourceNormalizerConfiguration:
        VesperSourceNormalizerConfiguration? = null
    var lastNativeFramePipelineConfiguration: VesperNativeFramePipelineConfiguration? = null
    var lastNativeFramePipelineSurfaceKind: NativeVideoSurfaceKind? = null
    private var currentNativeFramePipelineStatus: Map<String, Any?>? = null
    private var updateListener: (() -> Unit)? = null

    override fun probeMobilePlugins(
        source: VesperPlayerSource,
        sourceNormalizerConfiguration: VesperSourceNormalizerConfiguration,
        frameProcessorConfiguration: VesperFrameProcessorConfiguration,
    ): List<Map<String, Any?>> {
        lastProbeSource = source
        lastSourceNormalizerConfiguration = sourceNormalizerConfiguration
        lastFrameProcessorConfiguration = frameProcessorConfiguration
        return mobilePluginDiagnostics
    }

    override fun initialize(
        source: VesperPlayerSource,
        resiliencePolicy: VesperPlaybackResiliencePolicy,
        trackPreferencePolicy: VesperTrackPreferencePolicy,
        systemPlaybackUsesSourceNormalizerResource: Boolean,
        systemPlaybackVideoEnabled: Boolean,
    ): NativeBridgeStartup {
        lastInitializedSource = source
        lastSystemPlaybackUsesSourceNormalizerResource = systemPlaybackUsesSourceNormalizerResource
        lastSystemPlaybackVideoEnabled = systemPlaybackVideoEnabled
        onInitialize?.invoke()
        return NativeBridgeStartup(subtitle = null, pluginDiagnostics = nativeStartupDiagnostics)
    }

    override fun openNativeFramePipeline(
        source: VesperPlayerSource,
        sourceNormalizerConfiguration: VesperSourceNormalizerConfiguration,
        nativeFramePipelineConfiguration: VesperNativeFramePipelineConfiguration,
        surfaceKind: NativeVideoSurfaceKind,
    ): Map<String, Any?> {
        openNativeFramePipelineCount += 1
        nativeFramePipelineOpenError?.let { throw it }
        lastNativeFramePipelineSource = source
        lastNativeFramePipelineSourceNormalizerConfiguration = sourceNormalizerConfiguration
        lastNativeFramePipelineConfiguration = nativeFramePipelineConfiguration
        lastNativeFramePipelineSurfaceKind = surfaceKind
        return rememberNativeFramePipelineStatus(
            nativeFramePipelineStatus(
                status = "opened",
                message = "Android native-frame lifecycle opened for test session.",
            ) + mapOf(
                "handle" to 10L,
                "sourceUri" to source.uri,
                "sourceNormalizerMode" to sourceNormalizerConfiguration.mode.name,
            )
        )
    }

    override fun advanceNativeFramePipeline(): Map<String, Any?> {
        advanceNativeFramePipelineCount += 1
        val queuedStatus =
            if (nativeFramePipelineAdvanceStatuses.isNotEmpty()) {
                nativeFramePipelineAdvanceStatuses.removeAt(0)
            } else {
                nativeFramePipelineAdvanceStatus
            }
        queuedStatus?.let { status ->
            return rememberNativeFramePipelineStatus(
                nativeFramePipelineStatus(
                    status = status["status"]?.toString() ?: "frame",
                    message = status["message"]?.toString() ?: "frame",
                ) + status
            )
        }
        nativeFramePipelineAdvanceError?.let { throw it }
        return rememberNativeFramePipelineStatus(
            nativeFramePipelineStatus(
                status = "pending",
                message = "Android MediaCodec native-frame decoder is waiting for input or output",
            )
        )
    }

    override fun releaseNativeFramePipelineFrame(
        frameHandle: Long,
        presented: Boolean,
    ): Map<String, Any?> {
        nativeFramePipelineReleaseError?.let { throw it }
        releasedNativeFramePipelineFrames += frameHandle to presented
        return rememberNativeFramePipelineStatus(
            nativeFramePipelineStatus(
                status = "released",
                message = "released",
                processedFrames = 1L,
                presentedFrames = if (presented) 1L else 0L,
            )
        )
    }

    override fun attachNativeFramePipelineSurface(
        surface: Surface,
        surfaceKind: NativeVideoSurfaceKind,
    ): Map<String, Any?> =
        rememberNativeFramePipelineStatus(
            nativeFramePipelineStatus(
                status = "surfaceAttached",
                message = "presenter surface attached",
            ) + mapOf(
                "presenterReady" to true,
                "presenterConfigured" to true,
                "presenterState" to "ready",
                "surfaceAttached" to true,
                "surfaceProfile" to surfaceKind.name,
            )
        )

    override fun detachNativeFramePipelineSurface(): Map<String, Any?> =
        rememberNativeFramePipelineStatus(
            nativeFramePipelineStatus(
                status = "surfaceDetached",
                message = "presenter surface detached",
            ) + mapOf(
                "presenterReady" to false,
                "presenterConfigured" to false,
                "presenterState" to "waitingForSurface",
                "surfaceAttached" to false,
            )
        )

    override fun flushNativeFramePipeline(): Map<String, Any?> {
        flushNativeFramePipelineCount += 1
        nativeFramePipelineFlushError?.let { throw it }
        return rememberNativeFramePipelineStatus(
            nativeFramePipelineStatus(
                status = "flushed",
                message = "flushed",
                processedFrames = 1L,
                presentedFrames = 1L,
            )
        )
    }

    override fun seekNativeFramePipeline(positionMs: Long): Map<String, Any?> {
        seekNativeFramePipelinePositions.add(positionMs)
        nativeFramePipelineSeekError?.let { throw it }
        return rememberNativeFramePipelineStatus(
            nativeFramePipelineStatus(
                status = "seeked",
                message = "seeked",
                processedFrames = 1L,
                presentedFrames = 1L,
            )
        )
    }

    override fun currentNativeFramePipelineStatus(): Map<String, Any?>? =
        currentNativeFramePipelineStatus

    fun setCurrentNativeFramePipelineStatusForTest(status: Map<String, Any?>) {
        currentNativeFramePipelineStatus = status
    }

    override fun closeNativeFramePipeline() {
        currentNativeFramePipelineStatus = null
        if (openNativeFramePipelineCount > closeNativeFramePipelineCount) {
            closeNativeFramePipelineCount += 1
        }
    }

    private fun rememberNativeFramePipelineStatus(
        status: Map<String, Any?>,
    ): Map<String, Any?> {
        currentNativeFramePipelineStatus = status
        return status
    }

    override fun dispose() {
        disposeCount += 1
    }

    override fun refreshSnapshot() {
        refreshSnapshotCount += 1
    }

    override fun currentTrackCatalog(): VesperTrackCatalog = trackCatalog

    override fun currentTrackSelection(): VesperTrackSelectionSnapshot = trackSelection

    override fun currentEffectiveVideoTrackId(): String? = effectiveVideoTrackId

    override fun currentVideoVariantObservation(): VesperVideoVariantObservation? =
        videoVariantObservation

    override fun currentVideoLayoutInfo(): NativeVideoLayoutInfo? = null

    override fun setOnNativeUpdateListener(listener: (() -> Unit)?) {
        updateListener = listener
    }

    override fun attachSurface(surface: Surface, surfaceKind: NativeVideoSurfaceKind) = Unit

    override fun detachSurface() = Unit

    override fun pollSnapshot(): NativeBridgeSnapshot? = snapshot

    override fun drainEvents(): List<NativeBridgeEvent> = events.toList().also { events.clear() }

    override fun play() {
        playCount += 1
    }

    override fun pause() {
        pauseCount += 1
    }

    override fun stop() {
        stopCount += 1
    }

    override fun seekTo(positionMs: Long) {
        seekToPositions += positionMs
    }

    override fun setPlaybackRate(rate: Float) {
        playbackRates += rate
    }

    override fun setVideoTrackSelection(selection: VesperTrackSelection) {
        videoTrackSelectionCount += 1
    }

    override fun setAudioTrackSelection(selection: VesperTrackSelection) {
        audioTrackSelectionCount += 1
    }

    override fun setSubtitleTrackSelection(selection: VesperTrackSelection) {
        subtitleTrackSelectionCount += 1
    }

    override fun setAbrPolicy(policy: VesperAbrPolicy) {
        abrPolicyCount += 1
    }

    override fun configureSystemPlayback(configuration: VesperSystemPlaybackConfiguration) {
        configureSystemPlaybackCount += 1
    }

    override fun updateSystemPlaybackMetadata(metadata: VesperSystemPlaybackMetadata) {
        updateSystemPlaybackMetadataCount += 1
    }

    override fun clearSystemPlayback() {
        clearSystemPlaybackCount += 1
    }

    fun currentUpdateListener(): (() -> Unit)? = updateListener

    private fun nativeFramePipelineStatus(
        status: String,
        message: String,
        processedFrames: Long = 0L,
        presentedFrames: Long = 0L,
    ): Map<String, Any?> =
        mapOf(
            "status" to status,
            "route" to "sdkManagedNativeFrame",
            "participation" to "selected",
            "sourceInput" to "sourceNormalizerPacket",
            "decoderAdapter" to "MediaCodec",
            "presenterProfile" to "SurfaceView",
            "presenterReady" to false,
            "presenterConfigured" to false,
            "presenterState" to "waitingForSurface",
            "surfaceAttached" to false,
            "pipelineProfile" to "media_codec_surface_texture",
            "message" to message,
            "counters" to
                mapOf(
                    "processedFrames" to processedFrames,
                    "presentedFrames" to presentedFrames,
                    "deadlineMisses" to 0L,
                    "backpressureCount" to 0L,
                    "lateDropped" to 0L,
                ),
        )

    fun nativeFramePipelineStatusForTest(
        vararg overrides: Pair<String, Any?>,
    ): Map<String, Any?> =
        nativeFramePipelineStatus(
            status = overrides.firstOrNull { it.first == "status" }?.second?.toString()
                ?: "pending",
            message = overrides.firstOrNull { it.first == "message" }?.second?.toString()
                ?: "test status",
        ) + overrides.toMap()
}

private class ManualNativeFramePipelinePumpScheduler(
    private val beforeRun: (() -> Unit)? = null,
) : NativeFramePipelinePumpScheduler {
    override val inlineCallbacksForTests: Boolean = true
    private var scheduledAction: (() -> Unit)? = null
    var cancelCount = 0
        private set
    var closeCount = 0
        private set
    var lastDelayMs: Long? = null
        private set
    private var closed = false

    override fun schedule(delayMs: Long, action: () -> Unit) {
        if (closed) {
            return
        }
        lastDelayMs = delayMs
        scheduledAction = action
    }

    override fun execute(action: () -> Unit) {
        if (closed) {
            return
        }
        action()
    }

    override fun cancel() {
        cancelCount += 1
        scheduledAction = null
    }

    override fun close() {
        closeCount += 1
        closed = true
        cancel()
    }

    fun hasPendingActions(): Boolean = scheduledAction != null

    fun runNext() {
        scheduledAction.also { scheduledAction = null }?.let { action ->
            beforeRun?.invoke()
            action()
        }
    }
}

private class ThreadedNativeFramePipelinePumpScheduler : NativeFramePipelinePumpScheduler {
    override val inlineCallbacksForTests: Boolean = true
    @Volatile
    var lastError: Throwable? = null
        private set

    private var latch = CountDownLatch(1)
    private var closed = false

    @Synchronized
    override fun schedule(delayMs: Long, action: () -> Unit) {
        if (closed) {
            return
        }
        val currentLatch = latch
        Thread {
            try {
                action()
            } catch (error: Throwable) {
                lastError = error
            } finally {
                currentLatch.countDown()
            }
        }.start()
    }

    @Synchronized
    override fun cancel() = Unit

    @Synchronized
    override fun close() {
        closed = true
    }

    fun awaitRun(): Boolean = latch.await(5, TimeUnit.SECONDS)
}
