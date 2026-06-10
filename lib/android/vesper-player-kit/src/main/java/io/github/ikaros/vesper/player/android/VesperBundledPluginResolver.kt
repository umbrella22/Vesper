package io.github.ikaros.vesper.player.android

import android.content.Context
import dalvik.system.BaseDexClassLoader
import java.io.File

internal object VesperBundledPluginResolver {
    private const val SOURCE_NORMALIZER_FFMPEG_LIBRARY_NAME = "player_source_normalizer_ffmpeg"

    fun resolveSourceNormalizerConfiguration(
        context: Context,
        configuration: VesperSourceNormalizerConfiguration,
    ): VesperSourceNormalizerConfiguration =
        resolveSourceNormalizerConfiguration(
            configuration = configuration,
            libraryPathLookup = AndroidLibraryPathLookup(context.applicationContext),
        )

    internal fun resolveSourceNormalizerConfiguration(
        configuration: VesperSourceNormalizerConfiguration,
        libraryPathLookup: LibraryPathLookup,
    ): VesperSourceNormalizerConfiguration {
        if (
            configuration.mode == VesperSourceNormalizerMode.Disabled ||
                configuration.pluginLibraryPaths.isNotEmpty()
        ) {
            return configuration
        }

        val sourceNormalizerPath =
            libraryPathLookup.findLibrary(SOURCE_NORMALIZER_FFMPEG_LIBRARY_NAME)
                ?: return configuration
        return configuration.copy(pluginLibraryPaths = listOf(sourceNormalizerPath))
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
                    ?.takeIf { it.isNotBlank() && File(it).isFile }
            if (classLoaderPath != null) {
                return classLoaderPath
            }

            val nativeLibraryDir = context.applicationInfo?.nativeLibraryDir ?: return null
            val fallbackPath = File(nativeLibraryDir, System.mapLibraryName(libraryName))
            return fallbackPath.absolutePath.takeIf { fallbackPath.isFile }
        }
    }
}
