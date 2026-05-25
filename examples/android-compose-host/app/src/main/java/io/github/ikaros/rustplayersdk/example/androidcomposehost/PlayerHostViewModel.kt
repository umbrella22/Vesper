package io.github.ikaros.vesper.example.androidcomposehost

import android.app.Application
import androidx.lifecycle.AndroidViewModel
import dalvik.system.BaseDexClassLoader
import io.github.ikaros.vesper.player.android.VesperDownloadConfiguration
import io.github.ikaros.vesper.player.android.VesperFrameProcessorConfiguration
import io.github.ikaros.vesper.player.android.VesperFrameProcessorMode
import io.github.ikaros.vesper.player.android.VesperPlaylistConfiguration
import io.github.ikaros.vesper.player.android.VesperPlaylistCoordinator
import io.github.ikaros.vesper.player.android.VesperPlaylistNeighborWindow
import io.github.ikaros.vesper.player.android.VesperPlaylistPreloadWindow
import io.github.ikaros.vesper.player.android.VesperDownloadManager
import io.github.ikaros.vesper.player.android.VesperPlaybackResiliencePolicy
import io.github.ikaros.vesper.player.android.VesperPlayerController
import io.github.ikaros.vesper.player.android.VesperPlayerControllerFactory
import io.github.ikaros.vesper.player.android.VesperPlayerSource
import io.github.ikaros.vesper.player.android.VesperPreloadBudgetPolicy
import io.github.ikaros.vesper.player.android.VesperSourceNormalizerConfiguration
import io.github.ikaros.vesper.player.android.VesperVideoSurfaceKind
import io.github.ikaros.vesper.player.android.external.VesperExternalPlaybackController
import java.io.File
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow

internal class PlayerHostViewModel(
    private val application: Application,
) : AndroidViewModel(application) {
    private val playerPreloadBudgetPolicy =
        VesperPreloadBudgetPolicy(
            maxConcurrentTasks = 0,
            maxMemoryBytes = 0L,
            maxDiskBytes = 0L,
            warmupWindowMs = 0L,
        )

    private val preloadBudgetPolicy =
        VesperPreloadBudgetPolicy(
            maxConcurrentTasks = 2,
            maxMemoryBytes = 64L * 1024L * 1024L,
            maxDiskBytes = 256L * 1024L * 1024L,
            warmupWindowMs = 30_000L,
        )

    val sourceNormalizerPluginLibraryPaths: List<String> =
        bundledPluginLibraryPaths(application, "player_source_normalizer_ffmpeg")
    val frameProcessorPluginLibraryPaths: List<String> =
        bundledPluginLibraryPaths(application, "player_frame_processor_diagnostic")

    private val _controller =
        MutableStateFlow(
            createController(
                sourceNormalizerSetting = ExampleSourceNormalizerSetting.PreflightOnly,
                initialSource = null,
                resiliencePolicy = ExampleResilienceProfile.Balanced.policy,
            ),
        )
    val controller: StateFlow<VesperPlayerController> = _controller.asStateFlow()

    val playlistCoordinator =
        VesperPlaylistCoordinator(
            context = application.applicationContext,
            configuration =
                VesperPlaylistConfiguration(
                    playlistId = "android-compose-host",
                    neighborWindow = VesperPlaylistNeighborWindow(previous = 1, next = 1),
                    preloadWindow = VesperPlaylistPreloadWindow(nearVisible = 1, prefetchOnly = 2),
                    switchPolicy = examplePlaylistSwitchPolicy(),
                ),
            preloadBudgetPolicy = preloadBudgetPolicy,
            resiliencePolicy = ExampleResilienceProfile.Balanced.policy,
        )

    val downloadManager =
        VesperDownloadManager(
            context = application.applicationContext,
            configuration =
                VesperDownloadConfiguration(
                    runPostProcessorsOnCompletion = false,
                    pluginLibraryPaths = bundledDownloadPluginLibraryPaths(application),
                ),
        )
    val isDownloadExportPluginInstalled: Boolean =
        bundledDownloadPluginLibraryPaths(application).isNotEmpty()

    val externalPlaybackController =
        VesperExternalPlaybackController(application.applicationContext)

    fun rebuildController(
        sourceNormalizerSetting: ExampleSourceNormalizerSetting,
        initialSource: VesperPlayerSource?,
        resiliencePolicy: VesperPlaybackResiliencePolicy,
        shouldResumePlayback: Boolean,
        restorePositionMs: Long?,
    ): VesperPlayerController {
        val previous = _controller.value
        val next =
            createController(
                sourceNormalizerSetting = sourceNormalizerSetting,
                initialSource = initialSource,
                resiliencePolicy = resiliencePolicy,
            )
        _controller.value = next
        runCatching { previous.dispose() }
        if (initialSource != null) {
            restorePositionMs
                ?.takeIf { position -> position > 0L }
                ?.let { position -> runCatching { next.seekBy(position) } }
            if (shouldResumePlayback) {
                runCatching { next.play() }
            }
        }
        return next
    }

    override fun onCleared() {
        listOf(
            { externalPlaybackController.release() },
            { downloadManager.dispose() },
            { playlistCoordinator.dispose() },
            { _controller.value.dispose() },
        ).forEach { cleanup -> runCatching { cleanup() } }
    }

    private fun createController(
        sourceNormalizerSetting: ExampleSourceNormalizerSetting,
        initialSource: VesperPlayerSource?,
        resiliencePolicy: VesperPlaybackResiliencePolicy,
    ): VesperPlayerController =
        VesperPlayerControllerFactory.createDefault(
            context = application.applicationContext,
            initialSource = initialSource,
            resiliencePolicy = resiliencePolicy,
            // TextureView is more stable than SurfaceView for tab switches and scrolling hosts.
            surfaceKind = VesperVideoSurfaceKind.TextureView,
            preloadBudgetPolicy = playerPreloadBudgetPolicy,
            sourceNormalizerConfiguration =
                VesperSourceNormalizerConfiguration(
                    mode = sourceNormalizerSetting.mode,
                    pluginLibraryPaths = sourceNormalizerPluginLibraryPaths,
                ),
            frameProcessorConfiguration =
                VesperFrameProcessorConfiguration(
                    mode =
                        if (frameProcessorPluginLibraryPaths.isEmpty()) {
                            VesperFrameProcessorMode.Disabled
                        } else {
                            VesperFrameProcessorMode.DiagnosticsOnly
                        },
                    pluginLibraryPaths = frameProcessorPluginLibraryPaths,
                ),
        ).also { controller ->
            controller.initialize()
        }

    private fun bundledDownloadPluginLibraryPaths(application: Application): List<String> {
        return bundledPluginLibraryPaths(application, "vesper_remux_ffmpeg")
    }

    private fun bundledPluginLibraryPaths(
        application: Application,
        libraryName: String,
    ): List<String> {
        val resolvedPath =
            (application.classLoader as? BaseDexClassLoader)
                ?.findLibrary(libraryName)
                ?.takeIf { path -> path.isNotBlank() && File(path).isFile }
                ?: run {
                    val nativeLibraryDir = application.applicationInfo.nativeLibraryDir
                    val pluginLibrary =
                        nativeLibraryDir?.let { directory ->
                            File(directory, System.mapLibraryName(libraryName))
                        }
                    pluginLibrary?.takeIf(File::isFile)?.absolutePath
                }
        return resolvedPath?.let(::listOf) ?: emptyList()
    }
}
