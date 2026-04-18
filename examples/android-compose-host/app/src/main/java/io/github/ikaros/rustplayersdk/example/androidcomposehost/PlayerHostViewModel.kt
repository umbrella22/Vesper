package io.github.ikaros.vesper.example.androidcomposehost

import android.app.Application
import androidx.lifecycle.AndroidViewModel
import dalvik.system.BaseDexClassLoader
import io.github.ikaros.vesper.player.android.NativeVideoSurfaceKind
import io.github.ikaros.vesper.player.android.VesperDownloadConfiguration
import io.github.ikaros.vesper.player.android.VesperPlaylistConfiguration
import io.github.ikaros.vesper.player.android.VesperPlaylistCoordinator
import io.github.ikaros.vesper.player.android.VesperPlaylistNeighborWindow
import io.github.ikaros.vesper.player.android.VesperPlaylistPreloadWindow
import io.github.ikaros.vesper.player.android.VesperDownloadManager
import io.github.ikaros.vesper.player.android.VesperPlayerController
import io.github.ikaros.vesper.player.android.VesperPlayerControllerFactory
import io.github.ikaros.vesper.player.android.VesperPreloadBudgetPolicy
import java.io.File

internal class PlayerHostViewModel(
    application: Application,
) : AndroidViewModel(application) {
    private val preloadBudgetPolicy =
        VesperPreloadBudgetPolicy(
            maxConcurrentTasks = 2,
            maxMemoryBytes = 64L * 1024L * 1024L,
            maxDiskBytes = 256L * 1024L * 1024L,
            warmupWindowMs = 30_000L,
        )

    val controller: VesperPlayerController =
        VesperPlayerControllerFactory.createDefault(
            context = application.applicationContext,
            initialSource = null,
            resiliencePolicy = ExampleResilienceProfile.Balanced.policy,
            // tab 切换和滚动场景下，TextureView 比 SurfaceView 更稳定。
            surfaceKind = NativeVideoSurfaceKind.TextureView,
            preloadBudgetPolicy =
                VesperPreloadBudgetPolicy(
                    maxConcurrentTasks = 0,
                    maxMemoryBytes = 0L,
                    maxDiskBytes = 0L,
                    warmupWindowMs = 0L,
                ),
        ).also { controller ->
            controller.initialize()
        }

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

    override fun onCleared() {
        downloadManager.dispose()
        playlistCoordinator.dispose()
        controller.dispose()
    }

    private fun bundledDownloadPluginLibraryPaths(application: Application): List<String> {
        val libraryName = "player_ffmpeg"
        val resolvedPath =
            (
                application.classLoader as? BaseDexClassLoader
            )?.findLibrary(libraryName)?.takeIf { path ->
                path.isNotBlank() && File(path).isFile
            }
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
