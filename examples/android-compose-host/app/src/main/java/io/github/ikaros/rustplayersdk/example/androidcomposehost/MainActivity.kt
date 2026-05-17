package io.github.ikaros.vesper.example.androidcomposehost

import android.os.Bundle
import androidx.activity.compose.setContent
import androidx.activity.viewModels
import androidx.core.view.WindowCompat
import androidx.fragment.app.FragmentActivity

class MainActivity : FragmentActivity() {
    private val playerHostViewModel: PlayerHostViewModel by viewModels()

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        WindowCompat.setDecorFitsSystemWindows(window, false)
        setContent {
            PlayerHostApp(
                controller = playerHostViewModel.controller,
                playlistCoordinator = playerHostViewModel.playlistCoordinator,
                downloadManager = playerHostViewModel.downloadManager,
                externalPlaybackController = playerHostViewModel.externalPlaybackController,
                isDownloadExportPluginInstalled = playerHostViewModel.isDownloadExportPluginInstalled,
            )
        }
    }
}
