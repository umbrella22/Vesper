package io.github.ikaros.vesper.player.android

import org.json.JSONArray
import org.junit.Assert.assertEquals
import org.junit.Assert.assertThrows
import org.junit.Assert.assertTrue
import org.junit.Test

class VesperBundledPluginResolverTest {
    @Test
    fun emptyReferencesDoNotImplicitlySelectBundledPlugins() {
        val resolved = resolve()

        assertTrue(resolved.sourceNormalizerArtifacts.isEmpty())
        assertTrue(resolved.frameProcessorArtifacts.isEmpty())
        assertTrue(resolved.decoderArtifacts.isEmpty())
        assertTrue(resolved.nativeFrameProcessorArtifacts.isEmpty())
    }

    @Test
    fun disabledModesDoNotResolveStoredPluginReferences() {
        val resolved =
            resolve(
                sourceNormalizerConfiguration =
                    VesperSourceNormalizerConfiguration(
                        mode = VesperSourceNormalizerMode.Disabled,
                        pluginReferences = listOf(VesperBundledPluginReferences.sourceNormalizerFfmpeg),
                    ),
                frameProcessorConfiguration =
                    VesperFrameProcessorConfiguration(
                        mode = VesperFrameProcessorMode.Disabled,
                        pluginReferences = listOf(VesperBundledPluginReferences.frameProcessorDiagnostic),
                    ),
                nativeFramePipelineConfiguration =
                    VesperNativeFramePipelineConfiguration(
                        mode = VesperNativeFramePipelineMode.Disabled,
                        decoderPluginReferences = listOf(VesperBundledPluginReferences.decoderMediaCodec),
                        frameProcessorPluginReferences =
                            listOf(VesperBundledPluginReferences.frameProcessorDiagnostic),
                        maxInFlightFrames = 3,
                    ),
            )

        assertTrue(resolved.sourceNormalizerArtifacts.isEmpty())
        assertTrue(resolved.frameProcessorArtifacts.isEmpty())
        assertTrue(resolved.decoderArtifacts.isEmpty())
        assertTrue(resolved.nativeFrameProcessorArtifacts.isEmpty())
    }

    @Test
    fun explicitReferencesResolveOnlyTheirRegisteredArtifacts() {
        val resolved =
            resolve(
                sourceNormalizerConfiguration =
                    VesperSourceNormalizerConfiguration(
                        mode = VesperSourceNormalizerMode.RequireNormalized,
                        pluginReferences =
                            listOf(VesperBundledPluginReferences.sourceNormalizerFfmpeg),
                    ),
                frameProcessorConfiguration =
                    VesperFrameProcessorConfiguration(
                        mode = VesperFrameProcessorMode.DiagnosticsOnly,
                        pluginReferences =
                            listOf(VesperBundledPluginReferences.frameProcessorDiagnostic),
                    ),
                nativeFramePipelineConfiguration =
                    VesperNativeFramePipelineConfiguration(
                        mode = VesperNativeFramePipelineMode.PreferNativeFrame,
                        decoderPluginReferences =
                            listOf(VesperBundledPluginReferences.decoderMediaCodec),
                        frameProcessorPluginReferences =
                            listOf(VesperBundledPluginReferences.frameProcessorDiagnostic),
                    ),
                paths =
                    mapOf(
                        "vesper_source_normalizer_ffmpeg" to "/bundled/libsource.so",
                        "vesper_frame_processor_diagnostic" to "/bundled/libframe.so",
                        "vesper_decoder_mediacodec" to "/bundled/libdecoder.so",
                    ),
            )

        assertEquals(listOf("/bundled/libsource.so"), resolved.sourceNormalizerArtifacts.map { it.libraryPath })
        assertEquals(listOf("/bundled/libframe.so"), resolved.frameProcessorArtifacts.map { it.libraryPath })
        assertEquals(listOf("/bundled/libdecoder.so"), resolved.decoderArtifacts.map { it.libraryPath })
        assertEquals(
            listOf("/bundled/libframe.so"),
            resolved.nativeFrameProcessorArtifacts.map { it.libraryPath },
        )
    }

    @Test
    fun duplicateReferencesResolveOneArtifactPath() {
        val reference = VesperBundledPluginReferences.sourceNormalizerFfmpeg
        val resolved =
            resolve(
                sourceNormalizerConfiguration =
                    VesperSourceNormalizerConfiguration(
                        mode = VesperSourceNormalizerMode.PreferNormalized,
                        pluginReferences = listOf(reference, reference),
                    ),
                paths = mapOf("vesper_source_normalizer_ffmpeg" to "/bundled/libsource.so"),
            )

        assertEquals(listOf("/bundled/libsource.so"), resolved.sourceNormalizerArtifacts.map { it.libraryPath })
    }

    @Test
    fun capabilityInstanceIdentitySurvivesArtifactResolutionAndEncoding() {
        val reference =
            VesperPluginReference(
                pluginId = VesperBundledPluginReferences.sourceNormalizerFfmpeg.pluginId,
                capabilityInstanceId = "io.github.ikaros.vesper.source-normalizer-ffmpeg.secondary",
                transport = VesperPluginTransport.Native,
            )
        val resolved =
            resolve(
                sourceNormalizerConfiguration =
                    VesperSourceNormalizerConfiguration(
                        mode = VesperSourceNormalizerMode.PreflightOnly,
                        pluginReferences = listOf(reference),
                    ),
                paths = mapOf("vesper_source_normalizer_ffmpeg" to "/bundled/libsource.so"),
            )

        assertEquals(reference, resolved.sourceNormalizerArtifacts.single().reference)
        val encoded = encodeVesperResolvedMobilePluginArtifacts(resolved.sourceNormalizerArtifacts)
        assertTrue(encoded.contains(reference.capabilityInstanceId.orEmpty()))
    }

    @Test
    fun distinctCapabilityInstancesSharingOneArtifactRemainOrdered() {
        val pluginId = VesperBundledPluginReferences.sourceNormalizerFfmpeg.pluginId
        val first =
            VesperPluginReference(
                pluginId = pluginId,
                capabilityInstanceId = "$pluginId.first",
                transport = VesperPluginTransport.Native,
            )
        val second =
            VesperPluginReference(
                pluginId = pluginId,
                capabilityInstanceId = "$pluginId.second",
                transport = VesperPluginTransport.Native,
            )
        val resolved =
            resolve(
                sourceNormalizerConfiguration =
                    VesperSourceNormalizerConfiguration(
                        mode = VesperSourceNormalizerMode.PreflightOnly,
                        pluginReferences = listOf(first, second),
                    ),
                paths = mapOf("vesper_source_normalizer_ffmpeg" to "/bundled/libsource.so"),
            )

        assertEquals(
            listOf(first, second),
            resolved.sourceNormalizerArtifacts.map { it.reference },
        )
        assertEquals(
            listOf("/bundled/libsource.so", "/bundled/libsource.so"),
            resolved.sourceNormalizerArtifacts.map { it.libraryPath },
        )
        val encoded =
            JSONArray(encodeVesperResolvedMobilePluginArtifacts(resolved.sourceNormalizerArtifacts))
        assertEquals("$pluginId.first", encoded.getJSONObject(0).getJSONObject("reference").getString("capabilityInstanceId"))
        assertEquals("$pluginId.second", encoded.getJSONObject(1).getJSONObject("reference").getString("capabilityInstanceId"))
    }

    @Test
    fun mobileResolutionRejectsWasmWithoutNativeFallback() {
        val wasmReference =
            VesperPluginReference(
                pluginId = VesperBundledPluginReferences.sourceNormalizerFfmpeg.pluginId,
                transport = VesperPluginTransport.Wasm,
            )

        val error =
            assertThrows(IllegalArgumentException::class.java) {
                resolve(
                    sourceNormalizerConfiguration =
                        VesperSourceNormalizerConfiguration(
                            mode = VesperSourceNormalizerMode.PreferNormalized,
                            pluginReferences = listOf(wasmReference),
                        ),
                )
            }

        assertTrue(error.message.orEmpty().contains("do not support transport `wasm`"))
    }

    @Test
    fun unknownReferenceDoesNotSelectTheBundledDefault() {
        val error =
            assertThrows(IllegalArgumentException::class.java) {
                resolve(
                    sourceNormalizerConfiguration =
                        VesperSourceNormalizerConfiguration(
                            mode = VesperSourceNormalizerMode.PreferNormalized,
                            pluginReferences =
                                listOf(
                                    VesperPluginReference(
                                        pluginId = "dev.vesper.unknown-normalizer",
                                        transport = VesperPluginTransport.Native,
                                    ),
                                ),
                        ),
                )
            }

        assertTrue(error.message.orEmpty().contains("No embedded Android plugin artifact"))
    }

    @Test
    fun missingRegisteredArtifactFailsResolution() {
        val error =
            assertThrows(IllegalArgumentException::class.java) {
                resolve(
                    nativeFramePipelineConfiguration =
                        VesperNativeFramePipelineConfiguration(
                            mode = VesperNativeFramePipelineMode.RequireNativeFrame,
                            decoderPluginReferences =
                                listOf(VesperBundledPluginReferences.decoderMediaCodec),
                        ),
                )
            }

        assertTrue(error.message.orEmpty().contains("artifact") && error.message.orEmpty().contains("unavailable"))
    }

    private fun resolve(
        sourceNormalizerConfiguration: VesperSourceNormalizerConfiguration =
            VesperSourceNormalizerConfiguration(),
        frameProcessorConfiguration: VesperFrameProcessorConfiguration =
            VesperFrameProcessorConfiguration(),
        nativeFramePipelineConfiguration: VesperNativeFramePipelineConfiguration =
            VesperNativeFramePipelineConfiguration(),
        paths: Map<String, String> = emptyMap(),
    ): VesperResolvedMobilePluginArtifacts =
        VesperBundledPluginResolver.resolve(
            sourceNormalizerConfiguration = sourceNormalizerConfiguration,
            frameProcessorConfiguration = frameProcessorConfiguration,
            nativeFramePipelineConfiguration = nativeFramePipelineConfiguration,
            libraryPathLookup = VesperBundledPluginResolver.LibraryPathLookup(paths::get),
        )
}
