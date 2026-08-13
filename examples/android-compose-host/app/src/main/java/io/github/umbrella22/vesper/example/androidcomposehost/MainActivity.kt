package io.github.umbrella22.vesper.example.androidcomposehost

import android.os.Bundle
import androidx.activity.compose.setContent
import androidx.activity.viewModels
import androidx.compose.runtime.MutableState
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.core.view.WindowCompat
import androidx.fragment.app.FragmentActivity

class MainActivity : FragmentActivity() {
    private val playerHostViewModel: PlayerHostViewModel by viewModels()
    private var pictureInPictureModeState: MutableState<Boolean>? = null
    private var pictureInPictureUserLeaveHintState: MutableState<Long>? = null

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        WindowCompat.setDecorFitsSystemWindows(window, false)
        setContent {
            val controller by playerHostViewModel.controller.collectAsState()
            val pipModeState = remember { mutableStateOf(isInPictureInPictureMode) }
                .also { pictureInPictureModeState = it }
            val userLeaveHintGenerationState = remember { mutableStateOf(0L) }
                .also { pictureInPictureUserLeaveHintState = it }
            PlayerHostApp(
                controller = controller,
                isInPictureInPictureMode = pipModeState.value,
                userLeaveHintGeneration = userLeaveHintGenerationState.value,
                onRebuildController = playerHostViewModel::rebuildController,
                playlistCoordinator = playerHostViewModel.playlistCoordinator,
                downloadManager = playerHostViewModel.downloadManager,
                externalPlaybackController = playerHostViewModel.externalPlaybackController,
                isDownloadExportPluginInstalled = playerHostViewModel.isDownloadExportPluginInstalled,
                sourceNormalizerPluginReferences =
                    playerHostViewModel.sourceNormalizerPluginReferences,
                decoderMediaCodecPluginReferences =
                    playerHostViewModel.decoderMediaCodecPluginReferences,
                frameProcessorPluginReferences =
                    playerHostViewModel.frameProcessorPluginReferences,
            )
        }
    }

    override fun onPictureInPictureModeChanged(
        isInPictureInPictureMode: Boolean,
        newConfig: android.content.res.Configuration,
    ) {
        super.onPictureInPictureModeChanged(isInPictureInPictureMode, newConfig)
        pictureInPictureModeState?.value = isInPictureInPictureMode
    }

    override fun onUserLeaveHint() {
        pictureInPictureUserLeaveHintState?.let { state ->
            state.value += 1L
        }
        super.onUserLeaveHint()
    }
}
