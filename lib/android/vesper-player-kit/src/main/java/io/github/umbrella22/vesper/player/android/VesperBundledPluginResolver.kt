package io.github.umbrella22.vesper.player.android

import android.content.Context
import dalvik.system.BaseDexClassLoader
import java.io.File

internal data class VesperResolvedMobilePluginArtifact(
    val reference: VesperPluginReference,
    val libraryPath: String,
)

internal data class VesperResolvedMobilePluginArtifacts(
    val sourceNormalizerArtifacts: List<VesperResolvedMobilePluginArtifact> = emptyList(),
    val frameProcessorArtifacts: List<VesperResolvedMobilePluginArtifact> = emptyList(),
    val decoderArtifacts: List<VesperResolvedMobilePluginArtifact> = emptyList(),
    val nativeFrameProcessorArtifacts: List<VesperResolvedMobilePluginArtifact> = emptyList(),
)

internal object VesperBundledPluginResolver {
    private const val SOURCE_NORMALIZER_FFMPEG_LIBRARY_NAME = "vesper_source_normalizer_ffmpeg"
    private const val DECODER_MEDIACODEC_LIBRARY_NAME = "vesper_decoder_mediacodec"
    private const val FRAME_PROCESSOR_DIAGNOSTIC_LIBRARY_NAME = "vesper_frame_processor_diagnostic"
    private const val PERFORMANCE_DIAGNOSTICS_LIBRARY_NAME = "vesper_performance_diagnostics"

    fun requirePerformanceDiagnostics(context: Context) {
        resolveKnownNativeReferences(
            listOf(VesperBundledPluginReferences.performanceDiagnostics),
            mapOf(
                VesperBundledPluginReferences.performanceDiagnostics.pluginId to
                    PERFORMANCE_DIAGNOSTICS_LIBRARY_NAME,
            ),
            AndroidLibraryPathLookup(context.applicationContext),
        )
    }

    fun resolve(
        context: Context,
        sourceNormalizerConfiguration: VesperSourceNormalizerConfiguration,
        frameProcessorConfiguration: VesperFrameProcessorConfiguration,
        nativeFramePipelineConfiguration: VesperNativeFramePipelineConfiguration,
    ): VesperResolvedMobilePluginArtifacts =
        resolve(
            sourceNormalizerConfiguration = sourceNormalizerConfiguration,
            frameProcessorConfiguration = frameProcessorConfiguration,
            nativeFramePipelineConfiguration = nativeFramePipelineConfiguration,
            libraryPathLookup = AndroidLibraryPathLookup(context.applicationContext),
        )

    internal fun resolve(
        sourceNormalizerConfiguration: VesperSourceNormalizerConfiguration,
        frameProcessorConfiguration: VesperFrameProcessorConfiguration,
        nativeFramePipelineConfiguration: VesperNativeFramePipelineConfiguration,
        libraryPathLookup: LibraryPathLookup,
    ): VesperResolvedMobilePluginArtifacts =
        VesperResolvedMobilePluginArtifacts(
            sourceNormalizerArtifacts =
                resolveKnownNativeReferences(
                    sourceNormalizerConfiguration.pluginReferences.takeUnless {
                        sourceNormalizerConfiguration.isDisabled
                    }.orEmpty(),
                    mapOf(
                        VesperBundledPluginReferences.sourceNormalizerFfmpeg.pluginId to
                            SOURCE_NORMALIZER_FFMPEG_LIBRARY_NAME,
                    ),
                    libraryPathLookup,
                ),
            frameProcessorArtifacts =
                resolveKnownNativeReferences(
                    frameProcessorConfiguration.pluginReferences.takeUnless {
                        frameProcessorConfiguration.isDisabled
                    }.orEmpty(),
                    mapOf(
                        VesperBundledPluginReferences.frameProcessorDiagnostic.pluginId to
                            FRAME_PROCESSOR_DIAGNOSTIC_LIBRARY_NAME,
                    ),
                    libraryPathLookup,
                ),
            decoderArtifacts =
                resolveKnownNativeReferences(
                    nativeFramePipelineConfiguration.decoderPluginReferences.takeUnless {
                        nativeFramePipelineConfiguration.isDisabled
                    }.orEmpty(),
                    mapOf(
                        VesperBundledPluginReferences.decoderMediaCodec.pluginId to
                            DECODER_MEDIACODEC_LIBRARY_NAME,
                    ),
                    libraryPathLookup,
                ),
            nativeFrameProcessorArtifacts =
                resolveKnownNativeReferences(
                    nativeFramePipelineConfiguration.frameProcessorPluginReferences.takeUnless {
                        nativeFramePipelineConfiguration.isDisabled
                    }.orEmpty(),
                    mapOf(
                        VesperBundledPluginReferences.frameProcessorDiagnostic.pluginId to
                            FRAME_PROCESSOR_DIAGNOSTIC_LIBRARY_NAME,
                    ),
                    libraryPathLookup,
                ),
        )

    private fun resolveKnownNativeReferences(
        references: List<VesperPluginReference>,
        knownLibraries: Map<String, String>,
        libraryPathLookup: LibraryPathLookup,
    ): List<VesperResolvedMobilePluginArtifact> {
        val resolved = mutableListOf<VesperResolvedMobilePluginArtifact>()
        for (reference in references.distinct()) {
            require(reference.transport == VesperPluginTransport.Native) {
                "Android build-time plugins do not support transport `${reference.transportWireName}`"
            }
            val libraryName = requireNotNull(knownLibraries[reference.pluginId]) {
                "No embedded Android plugin artifact is registered for `${reference.pluginId}`"
            }
            val path = requireNotNull(libraryPathLookup.findLibrary(libraryName)) {
                "The embedded Android plugin artifact for `${reference.pluginId}` is unavailable"
            }
            resolved +=
                VesperResolvedMobilePluginArtifact(
                    reference = reference,
                    libraryPath = path,
                )
        }
        return resolved
    }

    internal fun interface LibraryPathLookup {
        fun findLibrary(libraryName: String): String?
    }

    private class AndroidLibraryPathLookup(
        private val context: Context,
    ) : LibraryPathLookup {
        override fun findLibrary(libraryName: String): String? {
            val classLoaderPath =
                (context.classLoader as? BaseDexClassLoader)
                    ?.findLibrary(libraryName)
                    ?.let(::nonBlankClassLoaderLibraryPath)
            if (classLoaderPath != null) {
                return classLoaderPath
            }

            val nativeLibraryDir = context.applicationInfo?.nativeLibraryDir ?: return null
            val fallbackPath = File(nativeLibraryDir, System.mapLibraryName(libraryName))
            return fallbackPath.absolutePath.takeIf { fallbackPath.isFile }
        }
    }
}

internal fun nonBlankClassLoaderLibraryPath(path: String): String? =
    path.takeIf(String::isNotBlank)
