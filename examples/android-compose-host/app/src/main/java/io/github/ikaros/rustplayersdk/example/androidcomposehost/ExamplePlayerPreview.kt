package io.github.ikaros.vesper.example.androidcomposehost

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.runtime.Composable
import androidx.compose.runtime.remember
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.tooling.preview.Preview
import androidx.compose.ui.unit.dp
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.darkColorScheme
import androidx.compose.material3.lightColorScheme
import io.github.ikaros.vesper.player.android.PlaybackStateUi
import io.github.ikaros.vesper.player.android.PlayerHostUiState
import io.github.ikaros.vesper.player.android.SeekableRangeUi
import io.github.ikaros.vesper.player.android.TimelineKind
import io.github.ikaros.vesper.player.android.TimelineUiState
import io.github.ikaros.vesper.player.android.VesperAbrPolicy
import io.github.ikaros.vesper.player.android.VesperMediaTrack
import io.github.ikaros.vesper.player.android.VesperMediaTrackKind
import io.github.ikaros.vesper.player.android.VesperPlayerControllerFactory
import io.github.ikaros.vesper.player.android.VesperTrackCatalog
import io.github.ikaros.vesper.player.android.VesperTrackSelection
import io.github.ikaros.vesper.player.android.VesperTrackSelectionSnapshot

@Preview(
    name = "Player Portrait Dark",
    showBackground = true,
    backgroundColor = 0xFF06080D,
    widthDp = 392,
    heightDp = 280,
)
@Composable
private fun PreviewExamplePlayerStagePortraitDark() {
    ExamplePreviewTheme(useDarkTheme = true) {
        val controller = remember { VesperPlayerControllerFactory.createPreview(androidHlsDemoSource()) }
        ExamplePlayerStage(
            controller = controller,
            uiState = previewPlayerUiState(),
            controlsVisible = true,
            pendingSeekRatio = null,
            isPortrait = true,
            modifier = Modifier
                .fillMaxWidth()
                .height(248.dp),
            onControlsVisibilityChange = {},
            onPendingSeekRatioChange = {},
            onOpenSheet = {},
            onToggleFullscreen = {},
        )
    }
}

@Preview(
    name = "Player Portrait Light",
    showBackground = true,
    backgroundColor = 0xFFF2F4F9,
    widthDp = 392,
    heightDp = 280,
)
@Composable
private fun PreviewExamplePlayerStagePortraitLight() {
    ExamplePreviewTheme(useDarkTheme = false) {
        val controller = remember { VesperPlayerControllerFactory.createPreview(androidHlsDemoSource()) }
        Box(
            modifier = Modifier
                .fillMaxSize()
                .background(Color(0xFFF2F4F9))
                .padding(18.dp),
        ) {
            ExamplePlayerStage(
                controller = controller,
                uiState = previewPlayerUiState(),
                controlsVisible = true,
                pendingSeekRatio = null,
                isPortrait = true,
                modifier = Modifier
                    .fillMaxWidth()
                    .height(248.dp),
                onControlsVisibilityChange = {},
                onPendingSeekRatioChange = {},
                onOpenSheet = {},
                onToggleFullscreen = {},
            )
        }
    }
}

@Preview(
    name = "Player Landscape Dark",
    showBackground = true,
    backgroundColor = 0xFF06080D,
    widthDp = 640,
    heightDp = 360,
)
@Composable
private fun PreviewExamplePlayerStageLandscapeDark() {
    ExamplePreviewTheme(useDarkTheme = true) {
        val controller = remember { VesperPlayerControllerFactory.createPreview(androidHlsDemoSource()) }
        Box(
            modifier = Modifier
                .fillMaxSize()
                .background(Color(0xFF06080D)),
        ) {
            ExamplePlayerStage(
                controller = controller,
                uiState = previewPlayerUiState(),
                controlsVisible = true,
                pendingSeekRatio = null,
                isPortrait = false,
                modifier = Modifier.fillMaxSize(),
                onControlsVisibilityChange = {},
                onPendingSeekRatioChange = {},
                onOpenSheet = {},
                onToggleFullscreen = {},
            )
        }
    }
}

@Preview(
    name = "Player Landscape Light",
    showBackground = true,
    backgroundColor = 0xFFF2F4F9,
    widthDp = 640,
    heightDp = 360,
)
@Composable
private fun PreviewExamplePlayerStageLandscapeLight() {
    ExamplePreviewTheme(useDarkTheme = false) {
        val controller = remember { VesperPlayerControllerFactory.createPreview(androidHlsDemoSource()) }
        Box(
            modifier = Modifier
                .fillMaxSize()
                .background(Color(0xFFF2F4F9)),
        ) {
            ExamplePlayerStage(
                controller = controller,
                uiState = previewPlayerUiState(),
                controlsVisible = true,
                pendingSeekRatio = null,
                isPortrait = false,
                modifier = Modifier.fillMaxSize(),
                onControlsVisibilityChange = {},
                onPendingSeekRatioChange = {},
                onOpenSheet = {},
                onToggleFullscreen = {},
            )
        }
    }
}

@Preview(
    name = "Player Landscape Minimal",
    showBackground = true,
    backgroundColor = 0xFF06080D,
    widthDp = 640,
    heightDp = 360,
)
@Composable
private fun PreviewExamplePlayerStageLandscapeMinimal() {
    ExamplePreviewTheme(useDarkTheme = true) {
        val controller = remember { VesperPlayerControllerFactory.createPreview(androidHlsDemoSource()) }
        Box(
            modifier = Modifier
                .fillMaxSize()
                .background(Color(0xFF06080D)),
        ) {
            ExamplePlayerStage(
                controller = controller,
                uiState = previewPlayerUiState(),
                controlsVisible = false,
                pendingSeekRatio = null,
                isPortrait = false,
                modifier = Modifier.fillMaxSize(),
                onControlsVisibilityChange = {},
                onPendingSeekRatioChange = {},
                onOpenSheet = {},
                onToggleFullscreen = {},
            )
        }
    }
}

@Preview(
    name = "Source Section Light",
    showBackground = true,
    backgroundColor = 0xFFF2F4F9,
    widthDp = 392,
    heightDp = 360,
)
@Composable
private fun PreviewExampleSourceSectionLight() {
    ExamplePreviewTheme(useDarkTheme = false) {
        val palette = exampleHostPalette(false)
        Box(
            modifier = Modifier
                .fillMaxSize()
                .background(
                    Brush.verticalGradient(
                        colors = listOf(palette.pageTop, palette.pageBottom),
                    ),
                )
                .padding(18.dp),
        ) {
            ExampleSourceSection(
                palette = palette,
                themeMode = ExampleThemeMode.System,
                remoteStreamUrl = ANDROID_HLS_DEMO_URL,
                onThemeModeChange = {},
                onRemoteStreamUrlChange = {},
                onPickVideo = {},
                onUseHlsDemo = {},
                onUseDashDemo = {},
                onUseLiveDvrAcceptance = {},
                onOpenRemote = {},
            )
        }
    }
}

@Preview(
    name = "Resilience Section Light",
    showBackground = true,
    backgroundColor = 0xFFF2F4F9,
    widthDp = 392,
    heightDp = 280,
)
@Composable
private fun PreviewExampleResilienceSectionLight() {
    ExamplePreviewTheme(useDarkTheme = false) {
        val palette = exampleHostPalette(false)
        Box(
            modifier = Modifier
                .fillMaxSize()
                .background(
                    Brush.verticalGradient(
                        colors = listOf(palette.pageTop, palette.pageBottom),
                    ),
                )
                .padding(18.dp),
        ) {
            ExampleResilienceSection(
                palette = palette,
                selectedProfile = ExampleResilienceProfile.Balanced,
                isApplyingProfile = true,
                onApplyProfile = {},
            )
        }
    }
}

@Preview(
    name = "Sheet Menu Dark",
    showBackground = true,
    backgroundColor = 0xFF06080D,
    widthDp = 392,
    heightDp = 760,
)
@Composable
private fun PreviewExampleSelectionSheetMenuDark() {
    ExamplePreviewTheme(useDarkTheme = true) {
        Box(
            modifier = Modifier
                .fillMaxSize()
                .background(Color(0xFF06080D)),
        ) {
            ExampleSelectionSheet(
                sheet = ExamplePlayerSheet.Menu,
                uiState = previewPlayerUiState(),
                trackCatalog = previewTrackCatalog(),
                trackSelection = previewTrackSelection(),
                onDismiss = {},
                onOpenSheet = {},
                onSelectQuality = {},
                onSelectAudio = {},
                onSelectSubtitle = {},
                onSelectSpeed = {},
            )
        }
    }
}

@Preview(
    name = "Sheet Quality Dark",
    showBackground = true,
    backgroundColor = 0xFF06080D,
    widthDp = 392,
    heightDp = 760,
)
@Composable
private fun PreviewExampleSelectionSheetQualityDark() {
    ExamplePreviewTheme(useDarkTheme = true) {
        Box(
            modifier = Modifier
                .fillMaxSize()
                .background(Color(0xFF06080D)),
        ) {
            ExampleSelectionSheet(
                sheet = ExamplePlayerSheet.Quality,
                uiState = previewPlayerUiState(),
                trackCatalog = previewTrackCatalog(),
                trackSelection = previewTrackSelection(),
                onDismiss = {},
                onOpenSheet = {},
                onSelectQuality = {},
                onSelectAudio = {},
                onSelectSubtitle = {},
                onSelectSpeed = {},
            )
        }
    }
}

@Composable
private fun ExamplePreviewTheme(
    useDarkTheme: Boolean,
    content: @Composable () -> Unit,
) {
    val palette = exampleHostPalette(useDarkTheme)
    val colorScheme =
        if (useDarkTheme) {
            darkColorScheme(
                primary = palette.primaryAction,
                surface = palette.sectionBackground,
                background = palette.pageBottom,
                onBackground = palette.title,
                onSurface = palette.title,
            )
        } else {
            lightColorScheme(
                primary = palette.primaryAction,
                surface = palette.sectionBackground,
                background = palette.pageBottom,
                onBackground = palette.title,
                onSurface = palette.title,
            )
        }

    MaterialTheme(colorScheme = colorScheme, content = content)
}

private fun previewPlayerUiState(): PlayerHostUiState =
    PlayerHostUiState(
        title = "Vesper",
        subtitle = "Android JNI/ExoPlayer bridge",
        sourceLabel = "VID_20260216_223628.mp4",
        playbackState = PlaybackStateUi.Playing,
        playbackRate = 1.0f,
        isBuffering = false,
        isInterrupted = false,
        timeline = TimelineUiState(
            kind = TimelineKind.Vod,
            isSeekable = true,
            seekableRange = SeekableRangeUi(0L, 48_000L),
            liveEdgeMs = null,
            positionMs = 2_000L,
            durationMs = 48_000L,
        ),
    )

private fun previewTrackCatalog(): VesperTrackCatalog =
    VesperTrackCatalog(
        tracks =
            listOf(
                VesperMediaTrack(
                    id = "video-1080",
                    kind = VesperMediaTrackKind.Video,
                    label = "1080p",
                    codec = "h264",
                    bitRate = 5_800_000L,
                    width = 1920,
                    height = 1080,
                ),
                VesperMediaTrack(
                    id = "video-720",
                    kind = VesperMediaTrackKind.Video,
                    label = "720p",
                    codec = "h264",
                    bitRate = 3_200_000L,
                    width = 1280,
                    height = 720,
                ),
                VesperMediaTrack(
                    id = "audio-ja",
                    kind = VesperMediaTrackKind.Audio,
                    label = "Japanese",
                    language = "ja",
                    channels = 2,
                    sampleRate = 48_000,
                ),
                VesperMediaTrack(
                    id = "subtitle-zh",
                    kind = VesperMediaTrackKind.Subtitle,
                    label = "简体中文",
                    language = "zh",
                    isDefault = true,
                ),
            ),
        adaptiveVideo = true,
        adaptiveAudio = false,
    )

private fun previewTrackSelection(): VesperTrackSelectionSnapshot =
    VesperTrackSelectionSnapshot(
        video = VesperTrackSelection.auto(),
        audio = VesperTrackSelection.track("audio-ja"),
        subtitle = VesperTrackSelection.track("subtitle-zh"),
        abrPolicy = VesperAbrPolicy.auto(),
    )
