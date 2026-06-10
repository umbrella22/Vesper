package io.github.ikaros.vesper.player.android

import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

class VesperBundledPluginResolverTest {
    @Test
    fun disabledSourceNormalizerDoesNotResolveBundledPlugins() {
        val resolved =
            VesperBundledPluginResolver.resolveSourceNormalizerConfiguration(
                configuration = VesperSourceNormalizerConfiguration(),
                libraryPathLookup = lookupReturning("/tmp/libplayer_source_normalizer_ffmpeg.so"),
            )

        assertEquals(VesperSourceNormalizerMode.Disabled, resolved.mode)
        assertTrue(resolved.pluginLibraryPaths.isEmpty())
    }

    @Test
    fun explicitSourceNormalizerPluginPathsOverrideBundledDiscovery() {
        val explicitPath = "/custom/libplayer_source_normalizer_ffmpeg.so"
        val resolved =
            VesperBundledPluginResolver.resolveSourceNormalizerConfiguration(
                configuration =
                    VesperSourceNormalizerConfiguration(
                        mode = VesperSourceNormalizerMode.PreferNormalized,
                        pluginLibraryPaths = listOf(explicitPath),
                    ),
                libraryPathLookup = lookupReturning("/bundled/libplayer_source_normalizer_ffmpeg.so"),
            )

        assertEquals(listOf(explicitPath), resolved.pluginLibraryPaths)
    }

    @Test
    fun enabledSourceNormalizerUsesBundledPluginWhenAvailable() {
        val bundledPath = "/bundled/libplayer_source_normalizer_ffmpeg.so"
        val resolved =
            VesperBundledPluginResolver.resolveSourceNormalizerConfiguration(
                configuration =
                    VesperSourceNormalizerConfiguration(
                        mode = VesperSourceNormalizerMode.PreferNormalized,
                    ),
                libraryPathLookup = lookupReturning(bundledPath),
            )

        assertEquals(VesperSourceNormalizerMode.PreferNormalized, resolved.mode)
        assertEquals(listOf(bundledPath), resolved.pluginLibraryPaths)
    }

    @Test
    fun preferNormalizedWithoutBundledPluginLeavesConfigurationNonFatal() {
        val resolved =
            VesperBundledPluginResolver.resolveSourceNormalizerConfiguration(
                configuration =
                    VesperSourceNormalizerConfiguration(
                        mode = VesperSourceNormalizerMode.PreferNormalized,
                    ),
                libraryPathLookup = lookupReturning(null),
            )

        assertEquals(VesperSourceNormalizerMode.PreferNormalized, resolved.mode)
        assertTrue(resolved.pluginLibraryPaths.isEmpty())
    }

    @Test
    fun requireNormalizedWithoutBundledPluginKeepsRequiredModeForNativeFailure() {
        val resolved =
            VesperBundledPluginResolver.resolveSourceNormalizerConfiguration(
                configuration =
                    VesperSourceNormalizerConfiguration(
                        mode = VesperSourceNormalizerMode.RequireNormalized,
                    ),
                libraryPathLookup = lookupReturning(null),
            )

        assertEquals(VesperSourceNormalizerMode.RequireNormalized, resolved.mode)
        assertTrue(resolved.pluginLibraryPaths.isEmpty())
    }

    private fun lookupReturning(path: String?): VesperBundledPluginResolver.LibraryPathLookup =
        VesperBundledPluginResolver.LibraryPathLookup { libraryName ->
            assertEquals("player_source_normalizer_ffmpeg", libraryName)
            path
        }
}
