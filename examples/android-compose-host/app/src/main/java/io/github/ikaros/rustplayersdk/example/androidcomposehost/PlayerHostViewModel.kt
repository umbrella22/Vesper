package io.github.ikaros.vesper.example.androidcomposehost

import android.app.Application
import androidx.lifecycle.AndroidViewModel
import io.github.ikaros.vesper.player.android.VesperPlaylistConfiguration
import io.github.ikaros.vesper.player.android.VesperPlaylistCoordinator
import io.github.ikaros.vesper.player.android.VesperPlaylistNeighborWindow
import io.github.ikaros.vesper.player.android.VesperPlaylistPreloadWindow
import io.github.ikaros.vesper.player.android.VesperPlayerController
import io.github.ikaros.vesper.player.android.VesperPlayerControllerFactory
import io.github.ikaros.vesper.player.android.VesperPreloadBudgetPolicy

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

    override fun onCleared() {
        playlistCoordinator.dispose()
        controller.dispose()
    }
}
