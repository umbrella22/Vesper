package io.github.ikaros.vesper.example.androidcomposehost

import android.app.Application
import androidx.lifecycle.AndroidViewModel
import io.github.ikaros.vesper.player.android.VesperPlayerController
import io.github.ikaros.vesper.player.android.VesperPlayerControllerFactory

internal class PlayerHostViewModel(
    application: Application,
) : AndroidViewModel(application) {
    val controller: VesperPlayerController =
        VesperPlayerControllerFactory.createDefault(
            context = application.applicationContext,
            initialSource = androidHlsDemoSource(),
        ).also { controller ->
            controller.initialize()
        }

    override fun onCleared() {
        controller.dispose()
    }
}
