package io.github.ikaros.vesper.example.androidcomposehost

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import io.github.ikaros.vesper.player.android.PlayerHostUiState
import io.github.ikaros.vesper.player.android.VesperAbrMode
import io.github.ikaros.vesper.player.android.VesperAbrPolicy
import io.github.ikaros.vesper.player.android.VesperPlayerController
import io.github.ikaros.vesper.player.android.VesperTrackCatalog
import io.github.ikaros.vesper.player.android.VesperTrackSelection
import io.github.ikaros.vesper.player.android.VesperTrackSelectionMode
import io.github.ikaros.vesper.player.android.VesperTrackSelectionSnapshot

@Composable
@OptIn(ExperimentalMaterial3Api::class)
internal fun ExampleSelectionSheet(
    sheet: ExamplePlayerSheet,
    uiState: PlayerHostUiState,
    trackCatalog: VesperTrackCatalog,
    trackSelection: VesperTrackSelectionSnapshot,
    onDismiss: () -> Unit,
    onOpenSheet: (ExamplePlayerSheet) -> Unit,
    onSelectQuality: (VesperAbrPolicy) -> Unit,
    onSelectAudio: (VesperTrackSelection) -> Unit,
    onSelectSubtitle: (VesperTrackSelection) -> Unit,
    onSelectSpeed: (Float) -> Unit,
) {
    ModalBottomSheet(
        onDismissRequest = onDismiss,
        containerColor = Color(0xFF0C1018),
        contentColor = Color.White,
        tonalElevation = 0.dp,
        dragHandle = {},
    ) {
        Column(
            modifier = Modifier
                .fillMaxWidth()
                .navigationBarsPadding()
                .padding(horizontal = 18.dp, vertical = 10.dp),
            verticalArrangement = Arrangement.spacedBy(10.dp),
        ) {
            Column(
                modifier = Modifier.padding(start = 4.dp, end = 4.dp, top = 8.dp, bottom = 2.dp),
                verticalArrangement = Arrangement.spacedBy(6.dp),
            ) {
                Text(
                    text = when (sheet) {
                        ExamplePlayerSheet.Menu -> "Playback Tools"
                        ExamplePlayerSheet.Quality -> "Quality"
                        ExamplePlayerSheet.Audio -> "Audio"
                        ExamplePlayerSheet.Subtitle -> "Subtitles"
                        ExamplePlayerSheet.Speed -> "Playback Speed"
                    },
                    style = MaterialTheme.typography.headlineSmall.copy(fontWeight = FontWeight.Bold),
                )
                Text(
                    text = when (sheet) {
                        ExamplePlayerSheet.Menu ->
                            "Open track, subtitle, quality, and speed controls without crowding the player overlay."

                        ExamplePlayerSheet.Quality ->
                            "Switch adaptive video or pin the stream to a specific quality track."

                        ExamplePlayerSheet.Audio ->
                            "Pick an audio program exposed by the current stream."

                        ExamplePlayerSheet.Subtitle ->
                            "Choose subtitles or turn them off."

                        ExamplePlayerSheet.Speed ->
                            "Preview playback behavior at different speeds."
                    },
                    style = MaterialTheme.typography.bodySmall.copy(color = Color(0xFF98A1B3)),
                )
            }

            LazyColumn(
                modifier = Modifier.fillMaxWidth(),
                verticalArrangement = Arrangement.spacedBy(2.dp),
            ) {
                when (sheet) {
                    ExamplePlayerSheet.Menu -> {
                        item {
                            SelectionRow(
                                title = "Playback Speed",
                                subtitle = speedBadge(uiState.playbackRate),
                                selected = false,
                                onClick = { onOpenSheet(ExamplePlayerSheet.Speed) },
                            )
                        }
                        item {
                            SelectionRow(
                                title = "Audio",
                                subtitle = audioButtonLabel(trackCatalog, trackSelection),
                                selected = false,
                                onClick = { onOpenSheet(ExamplePlayerSheet.Audio) },
                            )
                        }
                        item {
                            SelectionRow(
                                title = "Subtitles",
                                subtitle = subtitleButtonLabel(trackCatalog, trackSelection),
                                selected = false,
                                onClick = { onOpenSheet(ExamplePlayerSheet.Subtitle) },
                            )
                        }
                        item {
                            SelectionRow(
                                title = "Quality",
                                subtitle = qualityButtonLabel(trackCatalog, trackSelection),
                                selected = false,
                                onClick = { onOpenSheet(ExamplePlayerSheet.Quality) },
                            )
                        }
                    }

                    ExamplePlayerSheet.Quality -> {
                        item {
                            SelectionRow(
                                title = "Auto",
                                subtitle = if (trackCatalog.adaptiveVideo) {
                                    "Let the player adapt quality automatically."
                                } else {
                                    "Current route does not expose adaptive video switching."
                                },
                                selected = trackSelection.abrPolicy.mode == VesperAbrMode.Auto,
                                onClick = { onSelectQuality(VesperAbrPolicy.auto()) },
                            )
                        }
                        items(trackCatalog.videoTracks.sortedByDescending { it.bitRate ?: 0L }) { track ->
                            SelectionRow(
                                title = qualityLabel(track),
                                subtitle = qualitySubtitle(track),
                                selected =
                                    trackSelection.abrPolicy.mode == VesperAbrMode.FixedTrack &&
                                        trackSelection.abrPolicy.trackId == track.id,
                                onClick = { onSelectQuality(VesperAbrPolicy.fixedTrack(track.id)) },
                            )
                        }
                    }

                    ExamplePlayerSheet.Audio -> {
                        item {
                            SelectionRow(
                                title = "Auto",
                                subtitle = "Use the player's default audio selection.",
                                selected = trackSelection.audio.mode == VesperTrackSelectionMode.Auto,
                                onClick = { onSelectAudio(VesperTrackSelection.auto()) },
                            )
                        }
                        items(trackCatalog.audioTracks) { track ->
                            SelectionRow(
                                title = audioLabel(track),
                                subtitle = audioSubtitle(track),
                                selected =
                                    trackSelection.audio.mode == VesperTrackSelectionMode.Track &&
                                        trackSelection.audio.trackId == track.id,
                                onClick = { onSelectAudio(VesperTrackSelection.track(track.id)) },
                            )
                        }
                    }

                    ExamplePlayerSheet.Subtitle -> {
                        item {
                            SelectionRow(
                                title = "Off",
                                subtitle = "Hide subtitle rendering.",
                                selected = trackSelection.subtitle.mode == VesperTrackSelectionMode.Disabled,
                                onClick = { onSelectSubtitle(VesperTrackSelection.disabled()) },
                            )
                        }
                        item {
                            SelectionRow(
                                title = "Auto",
                                subtitle = "Use the stream's default subtitle behavior.",
                                selected = trackSelection.subtitle.mode == VesperTrackSelectionMode.Auto,
                                onClick = { onSelectSubtitle(VesperTrackSelection.auto()) },
                            )
                        }
                        items(trackCatalog.subtitleTracks) { track ->
                            SelectionRow(
                                title = subtitleLabel(track),
                                subtitle = subtitleSubtitle(track),
                                selected =
                                    trackSelection.subtitle.mode == VesperTrackSelectionMode.Track &&
                                        trackSelection.subtitle.trackId == track.id,
                                onClick = { onSelectSubtitle(VesperTrackSelection.track(track.id)) },
                            )
                        }
                    }

                    ExamplePlayerSheet.Speed -> {
                        items(VesperPlayerController.supportedPlaybackRates) { rate ->
                            SelectionRow(
                                title = speedBadge(rate),
                                subtitle =
                                    if (rate == uiState.playbackRate) {
                                        "Currently active."
                                    } else {
                                        "Apply this speed immediately."
                                    },
                                selected = uiState.playbackRate == rate,
                                onClick = { onSelectSpeed(rate) },
                            )
                        }
                    }
                }
            }
        }
    }
}

@Composable
internal fun SelectionRow(
    title: String,
    subtitle: String,
    selected: Boolean,
    onClick: () -> Unit,
) {
    Surface(
        onClick = onClick,
        modifier = Modifier.fillMaxWidth(),
        shape = RoundedCornerShape(18.dp),
        color = if (selected) Color.White.copy(alpha = 0.10f) else Color.Transparent,
        contentColor = Color.White,
    ) {
        Column(
            modifier = Modifier
                .fillMaxWidth()
                .padding(horizontal = 14.dp, vertical = 12.dp),
            verticalArrangement = Arrangement.spacedBy(4.dp),
        ) {
            Text(
                text = title,
                style = MaterialTheme.typography.titleSmall.copy(fontWeight = FontWeight.SemiBold),
            )
            Text(
                text = subtitle,
                style = MaterialTheme.typography.bodySmall.copy(color = Color(0xFF98A1B3)),
            )
        }
    }
    HorizontalDivider(color = Color.White.copy(alpha = 0.04f))
}
