package io.github.ikaros.vesper.example.androidcomposehost

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import io.github.ikaros.vesper.player.android.PlayerHostUiState
import io.github.ikaros.vesper.player.android.VesperPlayerController
import io.github.ikaros.vesper.player.android.VesperPlayerErrorCategory
import io.github.ikaros.vesper.player.android.VesperPlayerErrorState
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
    pictureInPicturePresentation: Boolean = false,
    onControlsVisibilityChange: (Boolean) -> Unit,
    onPendingSeekRatioChange: (Float?) -> Unit,
    onOpenSheet: (ExamplePlayerSheet) -> Unit,
    onToggleFullscreen: () -> Unit,
    onTogglePlayback: () -> Unit = controller::togglePause,
    onSeekToRatio: (Float) -> Unit = controller::seekToRatio,
    onSeekToLiveEdge: () -> Unit = controller::seekToLiveEdge,
    onSetPlaybackRate: (Float) -> Unit = controller::setPlaybackRate,
    playbackRateControlsEnabled: Boolean = true,
    currentBrightnessRatio: () -> Float? = { null },
    onSetBrightnessRatio: (Float) -> Float? = { null },
    currentVolumeRatio: () -> Float? = { null },
    onSetVolumeRatio: (Float) -> Float? = { null },
) {
    Box(modifier = modifier) {
        VesperPlayerStage(
            controller = controller,
            uiState = uiState,
            controlsVisible = controlsVisible,
            pendingSeekRatio = pendingSeekRatio,
            isPortrait = isPortrait,
            trackCatalog = trackCatalog,
            trackSelection = trackSelection,
            modifier = Modifier.matchParentSize(),
            pictureInPicturePresentation = pictureInPicturePresentation,
            onControlsVisibilityChange = onControlsVisibilityChange,
            onPendingSeekRatioChange = onPendingSeekRatioChange,
            onOpenSheet = { onOpenSheet(it.toExamplePlayerSheet()) },
            onToggleFullscreen = onToggleFullscreen,
            onTogglePlayback = onTogglePlayback,
            onSeekToRatio = onSeekToRatio,
            onSeekToLiveEdge = onSeekToLiveEdge,
            onSetPlaybackRate = onSetPlaybackRate,
            playbackRateControlsEnabled = playbackRateControlsEnabled,
            currentBrightnessRatio = currentBrightnessRatio,
            onSetBrightnessRatio = onSetBrightnessRatio,
            currentVolumeRatio = currentVolumeRatio,
            onSetVolumeRatio = onSetVolumeRatio,
        )
        uiState.lastError?.let { error ->
            ExampleStageTerminalError(
                error = error,
                isPortrait = isPortrait,
                modifier =
                    Modifier
                        .align(Alignment.Center)
                        .fillMaxWidth(if (isPortrait) 0.9f else 0.54f)
                        .padding(12.dp),
            )
        }
    }
}

@Composable
private fun ExampleStageTerminalError(
    error: VesperPlayerErrorState,
    isPortrait: Boolean,
    modifier: Modifier = Modifier,
) {
    val details = error.details
    val message =
        when {
            error.isDolbyVisionP5CapabilityFailure() ->
                stringResource(R.string.example_error_dolby_p5_device_unsupported)
            error.isWidevineNetworkExhausted() ->
                stringResource(
                    R.string.example_error_widevine_network_exhausted,
                    details["maxAttempts"]?.toString()?.takeIf(String::isNotBlank) ?: "3",
                )
            else -> error.message
        }
    val diagnosticLines =
        listOfNotNull(
            details["licenseUriHost"]?.toString()?.takeIf(String::isNotBlank)?.let {
                stringResource(R.string.example_error_license_host, it)
            },
            details["errorCodeName"]?.toString()?.takeIf(String::isNotBlank)?.let {
                stringResource(R.string.example_error_code_name, it)
            },
            details["codec"]?.toString()?.takeIf(String::isNotBlank)?.let {
                stringResource(R.string.example_error_codec, it)
            },
            details["decoderName"]?.toString()?.takeIf(String::isNotBlank)?.let {
                stringResource(R.string.example_error_decoder, it)
            },
        )
    Column(
        modifier =
            modifier
                .background(Color(0xF5FFF7F4), RoundedCornerShape(if (isPortrait) 18.dp else 14.dp))
                .border(1.dp, Color(0xFFE6A198), RoundedCornerShape(if (isPortrait) 18.dp else 14.dp))
                .padding(horizontal = 16.dp, vertical = 14.dp),
        verticalArrangement = Arrangement.spacedBy(6.dp),
    ) {
        Text(
            text = stringResource(R.string.example_error_terminal_title),
            style =
                MaterialTheme.typography.labelLarge.copy(
                    color = Color(0xFF9E2E28),
                    fontWeight = FontWeight.Bold,
                ),
        )
        Text(
            text = message,
            style = MaterialTheme.typography.bodyMedium.copy(color = Color(0xFF4B211D)),
            maxLines = if (isPortrait) 3 else 2,
            overflow = TextOverflow.Ellipsis,
        )
        diagnosticLines.take(3).forEach { line ->
            Text(
                text = line,
                style = MaterialTheme.typography.labelSmall.copy(color = Color(0xFF7F463E)),
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
            )
        }
    }
}

private fun VesperPlayerErrorState.isWidevineNetworkExhausted(): Boolean =
    category == VesperPlayerErrorCategory.Network &&
        details["keySystem"]?.toString()?.equals("widevine", ignoreCase = true) == true &&
        details["attemptsExhausted"]?.toString()?.equals("true", ignoreCase = true) == true

private fun VesperPlayerErrorState.isDolbyVisionP5CapabilityFailure(): Boolean {
    if (category != VesperPlayerErrorCategory.Decode &&
        category != VesperPlayerErrorCategory.Capability
    ) {
        return false
    }
    val hdrMetadata = details["hdrMetadata"] as? Map<*, *>
    val profile =
        details["dolbyVisionProfile"]?.toString()
            ?: hdrMetadata?.get("dolbyVisionProfile")?.toString()
    val codec = details["codec"]?.toString().orEmpty()
    val sampleMimeType = details["sampleMimeType"]?.toString().orEmpty()
    return profile == "5" ||
        codec.contains(".05", ignoreCase = true) ||
        sampleMimeType.equals("video/dolby-vision", ignoreCase = true) &&
        details["capabilityFailureCause"]?.toString()?.contains("decoder", ignoreCase = true) == true
}

private fun VesperPlayerStageSheet.toExamplePlayerSheet(): ExamplePlayerSheet =
    when (this) {
        VesperPlayerStageSheet.Menu -> ExamplePlayerSheet.Menu
        VesperPlayerStageSheet.Quality -> ExamplePlayerSheet.Quality
        VesperPlayerStageSheet.Audio -> ExamplePlayerSheet.Audio
        VesperPlayerStageSheet.Subtitle -> ExamplePlayerSheet.Subtitle
        VesperPlayerStageSheet.Speed -> ExamplePlayerSheet.Speed
    }
