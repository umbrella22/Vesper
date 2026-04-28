package io.github.ikaros.vesper.example.androidcomposehost

import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import io.github.ikaros.vesper.player.android.PlayerHostUiState
import io.github.ikaros.vesper.player.android.VesperPlayerController
import io.github.ikaros.vesper.player.android.VesperTrackCatalog
import io.github.ikaros.vesper.player.android.VesperTrackSelectionSnapshot
import io.github.ikaros.vesper.player.android.compose.ui.VesperPlayerStage
import io.github.ikaros.vesper.player.android.compose.ui.VesperPlayerStageSheet

@Composable
internal fun ExamplePlayerStage(
    controller: VesperPlayerController,
    uiState: PlayerHostUiState,
    controlsVisible: Boolean,
    pendingSeekRatio: Float?,
    isPortrait: Boolean,
    trackCatalog: VesperTrackCatalog = VesperTrackCatalog.Empty,
    trackSelection: VesperTrackSelectionSnapshot = VesperTrackSelectionSnapshot(),
    modifier: Modifier = Modifier,
    onControlsVisibilityChange: (Boolean) -> Unit,
    onPendingSeekRatioChange: (Float?) -> Unit,
    onOpenSheet: (ExamplePlayerSheet) -> Unit,
    onToggleFullscreen: () -> Unit,
    currentBrightnessRatio: () -> Float? = { null },
    onSetBrightnessRatio: (Float) -> Float? = { null },
    currentVolumeRatio: () -> Float? = { null },
    onSetVolumeRatio: (Float) -> Float? = { null },
) {
    VesperPlayerStage(
        controller = controller,
        uiState = uiState,
        controlsVisible = controlsVisible,
        pendingSeekRatio = pendingSeekRatio,
        isPortrait = isPortrait,
        trackCatalog = trackCatalog,
        trackSelection = trackSelection,
        modifier = modifier,
        onControlsVisibilityChange = onControlsVisibilityChange,
        onPendingSeekRatioChange = onPendingSeekRatioChange,
        onOpenSheet = { onOpenSheet(it.toExamplePlayerSheet()) },
        onToggleFullscreen = onToggleFullscreen,
        currentBrightnessRatio = currentBrightnessRatio,
        onSetBrightnessRatio = onSetBrightnessRatio,
        currentVolumeRatio = currentVolumeRatio,
        onSetVolumeRatio = onSetVolumeRatio,
    )
}

private fun VesperPlayerStageSheet.toExamplePlayerSheet(): ExamplePlayerSheet =
    when (this) {
        VesperPlayerStageSheet.Menu -> ExamplePlayerSheet.Menu
        VesperPlayerStageSheet.Quality -> ExamplePlayerSheet.Quality
        VesperPlayerStageSheet.Audio -> ExamplePlayerSheet.Audio
        VesperPlayerStageSheet.Subtitle -> ExamplePlayerSheet.Subtitle
        VesperPlayerStageSheet.Speed -> ExamplePlayerSheet.Speed
    }
