package io.github.ikaros.vesper.example.androidcomposehost

import android.content.Intent
import android.content.pm.ActivityInfo
import android.content.res.Configuration
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.background
import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.safeDrawing
import androidx.compose.foundation.layout.windowInsetsPadding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.darkColorScheme
import androidx.compose.material3.lightColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.platform.LocalConfiguration
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.unit.dp
import androidx.core.view.WindowCompat
import androidx.core.view.WindowInsetsCompat
import androidx.core.view.WindowInsetsControllerCompat
import io.github.ikaros.vesper.player.android.PlaybackStateUi
import io.github.ikaros.vesper.player.android.VesperPlayerController
import io.github.ikaros.vesper.player.android.VesperPlayerSource
import io.github.ikaros.vesper.player.android.compose.rememberVesperPlayerUiState

@Composable
fun PlayerHostApp(
    controller: VesperPlayerController,
) {
    val context = LocalContext.current
    val activity = remember(context) { context.findActivity() }
    val configuration = LocalConfiguration.current
    val isLandscape = configuration.orientation == Configuration.ORIENTATION_LANDSCAPE

    var themeMode by rememberSaveable { mutableStateOf(ExampleThemeMode.System) }
    val systemDarkTheme = isSystemInDarkTheme()
    val useDarkTheme =
        when (themeMode) {
            ExampleThemeMode.System -> systemDarkTheme
            ExampleThemeMode.Light -> false
            ExampleThemeMode.Dark -> true
        }

    LaunchedEffect(activity, isLandscape, useDarkTheme) {
        val window = activity?.window ?: return@LaunchedEffect
        val controllerInsets = WindowCompat.getInsetsController(window, window.decorView)
        controllerInsets.systemBarsBehavior =
            WindowInsetsControllerCompat.BEHAVIOR_SHOW_TRANSIENT_BARS_BY_SWIPE
        if (isLandscape) {
            controllerInsets.hide(WindowInsetsCompat.Type.systemBars())
        } else {
            controllerInsets.show(WindowInsetsCompat.Type.systemBars())
        }
        controllerInsets.isAppearanceLightStatusBars = !useDarkTheme && !isLandscape
        controllerInsets.isAppearanceLightNavigationBars = !useDarkTheme && !isLandscape
    }

    val palette = remember(useDarkTheme) { exampleHostPalette(useDarkTheme) }
    val uiState = rememberVesperPlayerUiState(controller)
    val trackCatalog by controller.trackCatalog.collectAsState()
    val trackSelection by controller.trackSelection.collectAsState()

    var remoteStreamUrl by rememberSaveable { mutableStateOf(ANDROID_HLS_DEMO_URL) }
    var controlsVisible by rememberSaveable { mutableStateOf(true) }
    var activeSheet by rememberSaveable { mutableStateOf<ExamplePlayerSheet?>(null) }
    var pendingSeekRatio by remember { mutableStateOf<Float?>(null) }

    val pickVideoLauncher = rememberLauncherForActivityResult(
        contract = ActivityResultContracts.OpenDocument(),
    ) { uri ->
        uri ?: return@rememberLauncherForActivityResult
        runCatching {
            context.contentResolver.takePersistableUriPermission(
                uri,
                Intent.FLAG_GRANT_READ_URI_PERMISSION,
            )
        }
        controller.selectSource(
            VesperPlayerSource.local(
                uri = uri.toString(),
                label = displayNameForUri(context, uri),
            ),
        )
        controlsVisible = true
    }

    LaunchedEffect(
        uiState.playbackState,
        uiState.isBuffering,
        controlsVisible,
        activeSheet,
        pendingSeekRatio,
    ) {
        if (
            uiState.playbackState != PlaybackStateUi.Playing ||
            uiState.isBuffering ||
            !controlsVisible ||
            activeSheet != null ||
            pendingSeekRatio != null
        ) {
            return@LaunchedEffect
        }

        kotlinx.coroutines.delay(3_000)
        if (
            uiState.playbackState == PlaybackStateUi.Playing &&
            !uiState.isBuffering &&
            activeSheet == null &&
            pendingSeekRatio == null
        ) {
            controlsVisible = false
        }
    }

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

    MaterialTheme(colorScheme = colorScheme) {
        Surface(
            modifier = Modifier.fillMaxSize(),
            color = palette.pageBottom,
        ) {
            Box(
                modifier = Modifier
                    .fillMaxSize()
                    .background(
                        brush = Brush.verticalGradient(
                            colors = listOf(palette.pageTop, palette.pageBottom),
                        ),
                    )
                    .then(
                        if (isLandscape) {
                            Modifier
                        } else {
                            Modifier.windowInsetsPadding(WindowInsets.safeDrawing)
                        }
                    ),
            ) {
                if (isLandscape) {
                    ExamplePlayerStage(
                        controller = controller,
                        uiState = uiState,
                        controlsVisible = controlsVisible,
                        pendingSeekRatio = pendingSeekRatio,
                        isPortrait = false,
                        modifier = Modifier.fillMaxSize(),
                        onControlsVisibilityChange = { controlsVisible = it },
                        onPendingSeekRatioChange = { pendingSeekRatio = it },
                        onOpenSheet = { activeSheet = it },
                        onToggleFullscreen = {
                            activity?.requestedOrientation =
                                ActivityInfo.SCREEN_ORIENTATION_SENSOR_PORTRAIT
                        },
                    )
                } else {
                    Column(
                        modifier = Modifier
                            .fillMaxSize()
                            .verticalScroll(rememberScrollState())
                            .padding(horizontal = 18.dp, vertical = 18.dp),
                        verticalArrangement = androidx.compose.foundation.layout.Arrangement.spacedBy(18.dp),
                    ) {
                        ExamplePlayerHeader(
                            sourceLabel = uiState.sourceLabel,
                            subtitle = uiState.subtitle,
                            palette = palette,
                        )

                        ExamplePlayerStage(
                            controller = controller,
                            uiState = uiState,
                            controlsVisible = controlsVisible,
                            pendingSeekRatio = pendingSeekRatio,
                            isPortrait = true,
                            modifier = Modifier
                                .fillMaxWidth()
                                .height(248.dp),
                            onControlsVisibilityChange = { controlsVisible = it },
                            onPendingSeekRatioChange = { pendingSeekRatio = it },
                            onOpenSheet = { activeSheet = it },
                            onToggleFullscreen = {
                                activity?.requestedOrientation =
                                    ActivityInfo.SCREEN_ORIENTATION_SENSOR_LANDSCAPE
                            },
                        )

                        ExampleSourceSection(
                            palette = palette,
                            themeMode = themeMode,
                            remoteStreamUrl = remoteStreamUrl,
                            onThemeModeChange = { themeMode = it },
                            onRemoteStreamUrlChange = { remoteStreamUrl = it },
                            onPickVideo = {
                                pickVideoLauncher.launch(arrayOf("video/*"))
                            },
                            onUseHlsDemo = {
                                controller.selectSource(androidHlsDemoSource(context))
                                controlsVisible = true
                            },
                            onUseDashDemo = {
                                controller.selectSource(androidDashDemoSource(context))
                                controlsVisible = true
                            },
                            onOpenRemote = {
                                val url = remoteStreamUrl.trim()
                                if (url.isNotEmpty()) {
                                    controller.selectSource(
                                        VesperPlayerSource.remote(
                                            uri = url,
                                            label = context.getString(R.string.example_source_custom_remote_label),
                                        ),
                                    )
                                    controlsVisible = true
                                }
                            },
                        )
                    }
                }

                activeSheet?.let { sheet ->
                    ExampleSelectionSheet(
                        sheet = sheet,
                        uiState = uiState,
                        trackCatalog = trackCatalog,
                        trackSelection = trackSelection,
                        onDismiss = { activeSheet = null },
                        onOpenSheet = { activeSheet = it },
                        onSelectQuality = { policy ->
                            controller.setAbrPolicy(policy)
                            activeSheet = null
                        },
                        onSelectAudio = { selection ->
                            controller.setAudioTrackSelection(selection)
                            activeSheet = null
                        },
                        onSelectSubtitle = { selection ->
                            controller.setSubtitleTrackSelection(selection)
                            activeSheet = null
                        },
                        onSelectSpeed = { rate ->
                            controller.setPlaybackRate(rate)
                            activeSheet = null
                        },
                    )
                }
            }
        }
    }
}
