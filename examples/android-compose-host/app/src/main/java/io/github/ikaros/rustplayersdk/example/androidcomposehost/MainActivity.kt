package io.github.ikaros.vesper.example.androidcomposehost

import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.viewModels
import androidx.core.view.WindowCompat

class MainActivity : ComponentActivity() {
    private val playerHostViewModel: PlayerHostViewModel by viewModels()

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        WindowCompat.setDecorFitsSystemWindows(window, false)
        setContent {
            PlayerHostApp(
                controller = playerHostViewModel.controller,
                playlistCoordinator = playerHostViewModel.playlistCoordinator,
            )
        }
    }
}
