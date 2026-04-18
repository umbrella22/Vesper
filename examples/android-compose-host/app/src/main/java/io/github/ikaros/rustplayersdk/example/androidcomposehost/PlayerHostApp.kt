package io.github.ikaros.vesper.example.androidcomposehost

import android.content.Intent
import android.content.pm.ActivityInfo
import android.content.res.Configuration
import android.widget.Toast
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
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.rounded.Download
import androidx.compose.material.icons.rounded.VideoLibrary
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.NavigationBar
import androidx.compose.material3.NavigationBarItem
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.darkColorScheme
import androidx.compose.material3.lightColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.platform.LocalConfiguration
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import androidx.core.view.WindowCompat
import androidx.core.view.WindowInsetsCompat
import androidx.core.view.WindowInsetsControllerCompat
import io.github.ikaros.vesper.player.android.PlaybackStateUi
import io.github.ikaros.vesper.player.android.VesperDownloadManager
import io.github.ikaros.vesper.player.android.VesperDownloadContentFormat
import io.github.ikaros.vesper.player.android.VesperDownloadSource
import io.github.ikaros.vesper.player.android.VesperDownloadState
import io.github.ikaros.vesper.player.android.VesperDownloadTaskSnapshot
import io.github.ikaros.vesper.player.android.VesperPlaylistCoordinator
import io.github.ikaros.vesper.player.android.VesperPlayerController
import io.github.ikaros.vesper.player.android.VesperPlayerSource
import io.github.ikaros.vesper.player.android.compose.rememberVesperPlayerUiState
import java.io.File
import kotlinx.coroutines.launch

@Composable
fun PlayerHostApp(
    controller: VesperPlayerController,
    playlistCoordinator: VesperPlaylistCoordinator,
    downloadManager: VesperDownloadManager,
    isDownloadExportPluginInstalled: Boolean,
) {
    val context = LocalContext.current
    val activity = remember(context) { context.findActivity() }
    val configuration = LocalConfiguration.current
    val isLandscape = configuration.orientation == Configuration.ORIENTATION_LANDSCAPE
    var selectedTab by rememberSaveable { mutableStateOf(ExampleHostTab.Player) }

    var themeMode by rememberSaveable { mutableStateOf(ExampleThemeMode.System) }
    var selectedResilienceProfile by rememberSaveable {
        mutableStateOf(ExampleResilienceProfile.Balanced)
    }
    val systemDarkTheme = isSystemInDarkTheme()
    val useDarkTheme =
        when (themeMode) {
            ExampleThemeMode.System -> systemDarkTheme
            ExampleThemeMode.Light -> false
            ExampleThemeMode.Dark -> true
        }

    val immersivePlayer = isLandscape && selectedTab == ExampleHostTab.Player

    LaunchedEffect(activity, immersivePlayer, useDarkTheme) {
        val window = activity?.window ?: return@LaunchedEffect
        val controllerInsets = WindowCompat.getInsetsController(window, window.decorView)
        controllerInsets.systemBarsBehavior =
            WindowInsetsControllerCompat.BEHAVIOR_SHOW_TRANSIENT_BARS_BY_SWIPE
        if (immersivePlayer) {
            controllerInsets.hide(WindowInsetsCompat.Type.systemBars())
        } else {
            controllerInsets.show(WindowInsetsCompat.Type.systemBars())
        }
        controllerInsets.isAppearanceLightStatusBars = !useDarkTheme && !immersivePlayer
        controllerInsets.isAppearanceLightNavigationBars = !useDarkTheme && !immersivePlayer
    }

    val palette = remember(useDarkTheme) { exampleHostPalette(useDarkTheme) }
    val uiState = rememberVesperPlayerUiState(controller)
    val trackCatalog by controller.trackCatalog.collectAsState()
    val trackSelection by controller.trackSelection.collectAsState()
    val playlistSnapshot by playlistCoordinator.snapshot.collectAsState()
    val downloadSnapshot by downloadManager.snapshot.collectAsState()

    var remoteStreamUrl by rememberSaveable { mutableStateOf(ANDROID_HLS_DEMO_URL) }
    var downloadRemoteUrl by rememberSaveable { mutableStateOf(ANDROID_HLS_DEMO_URL) }
    var controlsVisible by rememberSaveable { mutableStateOf(true) }
    var activeSheet by rememberSaveable { mutableStateOf<ExamplePlayerSheet?>(null) }
    var pendingSeekRatio by remember { mutableStateOf<Float?>(null) }
    var isApplyingResilienceProfile by remember { mutableStateOf(false) }
    var hasHandledFinishedPlayback by remember { mutableStateOf(false) }
    var queuedRemoteSource by remember { mutableStateOf<VesperPlayerSource?>(null) }
    var queuedLocalSource by remember { mutableStateOf<VesperPlayerSource?>(null) }
    var playlistItemIds by remember {
        mutableStateOf(listOf(ANDROID_HLS_PLAYLIST_ITEM_ID))
    }
    var pendingDownloadTasks by remember { mutableStateOf<List<ExamplePendingDownloadTask>>(emptyList()) }
    var savingTaskIds by remember { mutableStateOf(setOf<Long>()) }
    var exportProgressByTaskId by remember { mutableStateOf<Map<Long, Float>>(emptyMap()) }
    val scope = rememberCoroutineScope()

    fun createDownloadTask(
        assetIdPrefix: String,
        source: VesperPlayerSource,
    ) {
        val assetId = "$assetIdPrefix-${System.currentTimeMillis()}"
        pendingDownloadTasks =
            pendingDownloadTasks + ExamplePendingDownloadTask(
                requestId = assetId,
                assetId = assetId,
                label = exampleDraftDownloadLabel(source),
                sourceUri = source.uri,
            )
        scope.launch {
            val result =
                runCatching {
                    val preparedTask =
                        prepareExampleDownloadTask(
                            context = context,
                            assetId = assetId,
                            source = source,
                        )
                    checkNotNull(
                        downloadManager.createTask(
                            assetId = assetId,
                            source = preparedTask.source,
                            profile = preparedTask.profile,
                            assetIndex = preparedTask.assetIndex,
                        ),
                    ) { "native download task was not created" }
                }
            pendingDownloadTasks =
                pendingDownloadTasks.filterNot { pendingTask -> pendingTask.requestId == assetId }
            result.exceptionOrNull()?.let { error ->
                Toast
                    .makeText(
                        context,
                        context.getString(
                            R.string.example_download_create_task_failed,
                            error.localizedMessage
                                ?: context.getString(R.string.example_download_save_to_gallery_failed_unknown),
                        ),
                        Toast.LENGTH_SHORT,
                    ).show()
            }
        }
    }

    fun handleDownloadPrimaryAction(task: VesperDownloadTaskSnapshot) {
        when (task.state) {
            VesperDownloadState.Queued,
            VesperDownloadState.Failed,
            -> downloadManager.startTask(task.taskId)
            VesperDownloadState.Preparing,
            VesperDownloadState.Downloading,
            -> downloadManager.pauseTask(task.taskId)
            VesperDownloadState.Paused -> downloadManager.resumeTask(task.taskId)
            VesperDownloadState.Completed,
            VesperDownloadState.Removed,
            -> Unit
        }
    }

    fun handleSaveDownloadToGallery(task: VesperDownloadTaskSnapshot) {
        if (savingTaskIds.contains(task.taskId)) {
            return
        }
        val completedPath = task.assetIndex.completedPath?.takeIf { it.isNotBlank() }
        if (completedPath == null) {
            Toast
                .makeText(
                    context,
                    context.getString(R.string.example_download_save_to_gallery_missing_output),
                    Toast.LENGTH_SHORT,
                ).show()
            return
        }

        val needsExport =
            task.source.contentFormat == VesperDownloadContentFormat.HlsSegments ||
                task.source.contentFormat == VesperDownloadContentFormat.DashSegments
        if (needsExport && !isDownloadExportPluginInstalled) {
            Toast
                .makeText(
                    context,
                    context.getString(R.string.example_download_export_plugin_missing),
                    Toast.LENGTH_SHORT,
                ).show()
            return
        }

        scope.launch {
            savingTaskIds = savingTaskIds + task.taskId
            if (needsExport) {
                exportProgressByTaskId = exportProgressByTaskId + (task.taskId to 0f)
            }
            var exportFile: File? = null
            var manifestMutation: DownloadExportManifestMutation? = null
            val message =
                runCatching {
                    val gallerySourcePath =
                        if (needsExport) {
                            manifestMutation = prepareSegmentedExportManifestIfNeeded(task)
                            exportFile = createDownloadExportFile(context, task)
                            runCatching { exportFile.delete() }
                            downloadManager.exportTaskOutput(
                                taskId = task.taskId,
                                outputPath = exportFile.absolutePath,
                                onProgress = { ratio ->
                                    scope.launch {
                                        exportProgressByTaskId =
                                            exportProgressByTaskId + (
                                                task.taskId to ratio.coerceIn(0f, 1f)
                                            )
                                    }
                                },
                            )
                            exportFile.absolutePath
                        } else {
                            completedPath
                        }
                    saveVideoToGallery(context, gallerySourcePath)
                }.fold(
                    onSuccess = {
                        context.getString(R.string.example_download_save_to_gallery_success)
                    },
                    onFailure = { error ->
                        context.getString(
                            R.string.example_download_save_to_gallery_failed,
                            error.localizedMessage
                                ?: context.getString(R.string.example_download_save_to_gallery_failed_unknown),
                        )
                    },
                )
            try {
                manifestMutation?.restore()
            } catch (_: Throwable) {
            }
            runCatching { exportFile?.delete() }
            savingTaskIds = savingTaskIds - task.taskId
            exportProgressByTaskId = exportProgressByTaskId - task.taskId
            Toast.makeText(context, message, Toast.LENGTH_SHORT).show()
        }
    }

    fun applyPlaylistQueue(
        focusItemId: String? = playlistSnapshot.activeItem?.itemId,
        playlistItems: List<String> = playlistItemIds,
        remoteSource: VesperPlayerSource? = queuedRemoteSource,
        localSource: VesperPlayerSource? = queuedLocalSource,
    ) {
        val queue =
            examplePlaylistQueue(
                context = context,
                playlistItemIds = playlistItems,
                remoteSource = remoteSource,
                localSource = localSource,
            )
        playlistItemIds = queue.map { item -> item.itemId }
        playlistCoordinator.replaceQueue(queue)
        val resolvedFocusId =
            focusItemId?.takeIf { itemId -> queue.any { item -> item.itemId == itemId } }
                ?: queue.firstOrNull()?.itemId
        if (resolvedFocusId == null) {
            playlistCoordinator.clearViewportHints()
        } else {
            playlistCoordinator.updateViewportHints(
                examplePlaylistViewportHints(queue, resolvedFocusId),
            )
        }
    }

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
        val localSource =
            VesperPlayerSource.local(
                uri = uri.toString(),
                label = displayNameForUri(context, uri),
            )
        queuedLocalSource = localSource
        val nextPlaylistItems =
            enqueuePlaylistItem(
                playlistItemIds = playlistItemIds,
                itemId = ANDROID_LOCAL_PLAYLIST_ITEM_ID,
            )
        applyPlaylistQueue(
            focusItemId = ANDROID_LOCAL_PLAYLIST_ITEM_ID,
            playlistItems = nextPlaylistItems,
            localSource = localSource,
        )
        controlsVisible = true
    }

    LaunchedEffect(Unit) {
        applyPlaylistQueue(focusItemId = ANDROID_HLS_PLAYLIST_ITEM_ID)
    }

    LaunchedEffect(playlistSnapshot.activeItem?.itemId) {
        val activeItem = playlistSnapshot.activeItem ?: return@LaunchedEffect
        val source =
            playlistSnapshot.queue
                .firstOrNull { it.item.itemId == activeItem.itemId }
                ?.item?.source ?: return@LaunchedEffect
        controller.selectSource(source)
        controlsVisible = true
    }

    LaunchedEffect(uiState.playbackState, playlistSnapshot.activeItem?.itemId) {
        if (uiState.playbackState != PlaybackStateUi.Finished) {
            hasHandledFinishedPlayback = false
            return@LaunchedEffect
        }
        if (!hasHandledFinishedPlayback && playlistSnapshot.activeItem != null) {
            hasHandledFinishedPlayback = true
            playlistCoordinator.handlePlaybackCompleted()
        }
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
        Scaffold(
            modifier = Modifier.fillMaxSize(),
            containerColor = palette.pageBottom,
            bottomBar = {
                if (!immersivePlayer) {
                    NavigationBar {
                        NavigationBarItem(
                            selected = selectedTab == ExampleHostTab.Player,
                            onClick = { selectedTab = ExampleHostTab.Player },
                            icon = {
                                androidx.compose.material3.Icon(
                                    imageVector = Icons.Rounded.VideoLibrary,
                                    contentDescription = null,
                                )
                            },
                            label = { Text(stringResource(R.string.example_tab_player)) },
                        )
                        NavigationBarItem(
                            selected = selectedTab == ExampleHostTab.Downloads,
                            onClick = { selectedTab = ExampleHostTab.Downloads },
                            icon = {
                                androidx.compose.material3.Icon(
                                    imageVector = Icons.Rounded.Download,
                                    contentDescription = null,
                                )
                            },
                            label = { Text(stringResource(R.string.example_tab_downloads)) },
                        )
                    }
                }
            },
        ) { innerPadding ->
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
                        .padding(innerPadding)
                        .then(
                            if (immersivePlayer) {
                                Modifier
                            } else {
                                Modifier.windowInsetsPadding(WindowInsets.safeDrawing)
                            }
                        ),
                ) {
                    when {
                        immersivePlayer -> {
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
                        }

                        selectedTab == ExampleHostTab.Player -> {
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
                                        val nextPlaylistItems =
                                            enqueuePlaylistItem(
                                                playlistItemIds = playlistItemIds,
                                                itemId = ANDROID_HLS_PLAYLIST_ITEM_ID,
                                            )
                                        applyPlaylistQueue(
                                            focusItemId = ANDROID_HLS_PLAYLIST_ITEM_ID,
                                            playlistItems = nextPlaylistItems,
                                        )
                                        controlsVisible = true
                                    },
                                    onUseDashDemo = {
                                        val nextPlaylistItems =
                                            enqueuePlaylistItem(
                                                playlistItemIds = playlistItemIds,
                                                itemId = ANDROID_DASH_PLAYLIST_ITEM_ID,
                                            )
                                        applyPlaylistQueue(
                                            focusItemId = ANDROID_DASH_PLAYLIST_ITEM_ID,
                                            playlistItems = nextPlaylistItems,
                                        )
                                        controlsVisible = true
                                    },
                                    onOpenRemote = {
                                        val url = remoteStreamUrl.trim()
                                        if (url.isNotEmpty()) {
                                            val remoteSource =
                                                VesperPlayerSource.remote(
                                                    uri = url,
                                                    label = context.getString(R.string.example_source_custom_remote_label),
                                                )
                                            queuedRemoteSource = remoteSource
                                            val nextPlaylistItems =
                                                enqueuePlaylistItem(
                                                    playlistItemIds = playlistItemIds,
                                                    itemId = ANDROID_REMOTE_PLAYLIST_ITEM_ID,
                                                )
                                            applyPlaylistQueue(
                                                focusItemId = ANDROID_REMOTE_PLAYLIST_ITEM_ID,
                                                playlistItems = nextPlaylistItems,
                                                remoteSource = remoteSource,
                                            )
                                            controlsVisible = true
                                        }
                                    },
                                )

                                ExamplePlaylistSection(
                                    palette = palette,
                                    playlistQueue = playlistSnapshot.queue,
                                    onFocusPlaylistItem = { itemId ->
                                        val queue =
                                            playlistSnapshot.queue.map { itemState -> itemState.item }
                                        playlistCoordinator.updateViewportHints(
                                            examplePlaylistViewportHints(queue, itemId),
                                        )
                                        controlsVisible = true
                                    },
                                )

                                ExampleResilienceSection(
                                    palette = palette,
                                    selectedProfile = selectedResilienceProfile,
                                    isApplyingProfile = isApplyingResilienceProfile,
                                    onApplyProfile = { profile ->
                                        if (
                                            !isApplyingResilienceProfile &&
                                            profile != selectedResilienceProfile
                                        ) {
                                            val previousProfile = selectedResilienceProfile
                                            selectedResilienceProfile = profile
                                            scope.launch {
                                                isApplyingResilienceProfile = true
                                                kotlinx.coroutines.yield()
                                                val result =
                                                    runCatching {
                                                        controller.setResiliencePolicy(profile.policy)
                                                        playlistCoordinator.setResiliencePolicy(profile.policy)
                                                    }
                                                if (result.isFailure) {
                                                    selectedResilienceProfile = previousProfile
                                                }
                                                isApplyingResilienceProfile = false
                                            }
                                        }
                                    },
                                )
                            }
                        }

                        else -> {
                            Column(
                                modifier = Modifier
                                    .fillMaxSize()
                                    .verticalScroll(rememberScrollState())
                                    .padding(horizontal = 18.dp, vertical = 18.dp),
                                verticalArrangement = androidx.compose.foundation.layout.Arrangement.spacedBy(18.dp),
                            ) {
                                ExampleDownloadHeader(
                                    palette = palette,
                                    isDownloadExportPluginInstalled = isDownloadExportPluginInstalled,
                                )
                                ExampleDownloadCreateSection(
                                    palette = palette,
                                    remoteUrl = downloadRemoteUrl,
                                    onRemoteUrlChange = { downloadRemoteUrl = it },
                                    onUseHlsDemo = {
                                        createDownloadTask(
                                            assetIdPrefix = ANDROID_HLS_PLAYLIST_ITEM_ID,
                                            source = androidHlsDemoSource(context),
                                        )
                                    },
                                    onUseDashDemo = {
                                        createDownloadTask(
                                            assetIdPrefix = ANDROID_DASH_PLAYLIST_ITEM_ID,
                                            source = androidDashDemoSource(context),
                                        )
                                    },
                                    onCreateRemote = {
                                        val url = downloadRemoteUrl.trim()
                                        if (url.isNotEmpty()) {
                                            createDownloadTask(
                                                assetIdPrefix = ANDROID_REMOTE_PLAYLIST_ITEM_ID,
                                                source =
                                                    VesperPlayerSource.remote(
                                                        uri = url,
                                                        label = exampleDraftDownloadLabel(url),
                                                    ),
                                            )
                                        }
                                    },
                                )
                                ExampleDownloadTasksSection(
                                    palette = palette,
                                    tasks = downloadSnapshot.tasks,
                                    pendingTasks = pendingDownloadTasks,
                                    isDownloadExportPluginInstalled = isDownloadExportPluginInstalled,
                                    savingTaskIds = savingTaskIds,
                                    exportProgressByTaskId = exportProgressByTaskId,
                                    onPrimaryAction = ::handleDownloadPrimaryAction,
                                    onSaveToGallery = ::handleSaveDownloadToGallery,
                                    onRemoveTask = { task ->
                                        downloadManager.removeTask(task.taskId)
                                    },
                                )
                            }
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
}

private enum class ExampleHostTab {
    Player,
    Downloads,
}
