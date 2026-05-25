package io.github.ikaros.vesper.example.androidcomposehost

import android.Manifest
import android.app.Activity
import android.content.Context
import android.content.Intent
import android.content.pm.ActivityInfo
import android.content.pm.PackageManager
import android.content.res.Configuration
import android.media.AudioManager
import android.os.Build
import android.provider.Settings
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
import androidx.compose.runtime.rememberUpdatedState
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
import androidx.core.content.ContextCompat
import io.github.ikaros.vesper.player.android.PlaybackStateUi
import io.github.ikaros.vesper.player.android.VesperDownloadManager
import io.github.ikaros.vesper.player.android.VesperDownloadContentFormat
import io.github.ikaros.vesper.player.android.VesperDownloadSource
import io.github.ikaros.vesper.player.android.VesperDownloadPublicCollection
import io.github.ikaros.vesper.player.android.VesperDownloadState
import io.github.ikaros.vesper.player.android.VesperDownloadTaskSnapshot
import io.github.ikaros.vesper.player.android.VesperPlaybackResiliencePolicy
import io.github.ikaros.vesper.player.android.VesperPlaylistCoordinator
import io.github.ikaros.vesper.player.android.VesperPlayerController
import io.github.ikaros.vesper.player.android.VesperPlayerSource
import io.github.ikaros.vesper.player.android.VesperSystemPlaybackConfiguration
import io.github.ikaros.vesper.player.android.VesperSystemPlaybackControls
import io.github.ikaros.vesper.player.android.VesperSystemPlaybackMetadata
import io.github.ikaros.vesper.player.android.TimelineKind
import io.github.ikaros.vesper.player.android.TimelineUiState
import io.github.ikaros.vesper.player.android.compose.rememberVesperPlayerUiState
import io.github.ikaros.vesper.player.android.external.VesperExternalFallbackFormat
import io.github.ikaros.vesper.player.android.external.VesperExternalFormatAdaptationConfig
import io.github.ikaros.vesper.player.android.external.VesperExternalPlaybackController
import io.github.ikaros.vesper.player.android.external.VesperExternalPlaybackEventKind
import io.github.ikaros.vesper.player.android.external.VesperExternalPlaybackMediaItem
import io.github.ikaros.vesper.player.android.external.VesperExternalPlaybackResult
import io.github.ikaros.vesper.player.android.external.VesperExternalPlaybackRoute
import io.github.ikaros.vesper.player.android.external.VesperExternalPlaybackRouteKind
import java.io.File
import kotlin.math.roundToInt
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.collect
import kotlinx.coroutines.launch

@Composable
internal fun PlayerHostApp(
    controller: VesperPlayerController,
    onRebuildController: (
        ExampleSourceNormalizerSetting,
        VesperPlayerSource?,
        VesperPlaybackResiliencePolicy,
        Boolean,
        Long?,
    ) -> VesperPlayerController,
    playlistCoordinator: VesperPlaylistCoordinator,
    downloadManager: VesperDownloadManager,
    externalPlaybackController: VesperExternalPlaybackController,
    isDownloadExportPluginInstalled: Boolean,
    sourceNormalizerPluginLibraryPaths: List<String>,
    frameProcessorPluginLibraryPaths: List<String>,
) {
    val context = LocalContext.current
    val activity = remember(context) { context.findActivity() }
    val deviceControls = remember(context, activity) {
        ExampleAndroidDeviceControls(context.applicationContext, activity)
    }
    val configuration = LocalConfiguration.current
    val isLandscape = configuration.orientation == Configuration.ORIENTATION_LANDSCAPE
    var selectedTab by rememberSaveable { mutableStateOf(ExampleHostTab.Player) }

    var themeMode by rememberSaveable { mutableStateOf(ExampleThemeMode.System) }
    var selectedResilienceProfile by rememberSaveable {
        mutableStateOf(ExampleResilienceProfile.Balanced)
    }
    var sourceNormalizerSetting by rememberSaveable {
        mutableStateOf(ExampleSourceNormalizerSetting.PreflightOnly)
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
    val externalRoutes by externalPlaybackController.routes.collectAsState()

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
    var externalSession by remember { mutableStateOf<ExampleExternalPlaybackSession?>(null) }
    var isExternalDiscoveryRunning by rememberSaveable { mutableStateOf(false) }
    var isCastRoutePickerOpening by remember { mutableStateOf(false) }
    var castRoutePickerRequestId by remember { mutableStateOf(0L) }
    var externalNowMillis by remember { mutableStateOf(System.currentTimeMillis()) }
    var hasNearbyWifiPermission by remember {
        mutableStateOf(context.hasNearbyWifiPermission())
    }
    val scope = rememberCoroutineScope()

    val dlnaPermissionLauncher = rememberLauncherForActivityResult(
        contract = ActivityResultContracts.RequestPermission(),
    ) { granted ->
        hasNearbyWifiPermission = granted || context.hasNearbyWifiPermission()
        if (hasNearbyWifiPermission) {
            externalPlaybackController.startDiscovery()
            isExternalDiscoveryRunning = true
        } else {
            Toast
                .makeText(
                    context,
                    context.getString(R.string.example_external_permission_required),
                    Toast.LENGTH_SHORT,
                ).show()
        }
    }

    val activePlaylistSource =
        playlistSnapshot.activeItem?.itemId?.let { activeItemId ->
            playlistSnapshot.queue.firstOrNull { itemState ->
                itemState.item.itemId == activeItemId
            }?.item?.source
        }
    val latestExternalRoutes by rememberUpdatedState(externalRoutes)
    val latestActivePlaylistSource by rememberUpdatedState(activePlaylistSource)
    val latestUiState by rememberUpdatedState(uiState)

    val displayedUiState =
        externalSession?.let { session ->
            uiState.copy(
                subtitle = context.getString(R.string.example_external_connected_route, session.routeName),
                playbackState = exampleExternalPlaybackState(session),
                isBuffering = session.status == ExampleExternalPlaybackStatus.Loading,
                timeline = exampleExternalTimeline(uiState.timeline, session, externalNowMillis),
            )
        } ?: uiState

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

    fun selectSourceForPlayback(source: VesperPlayerSource) {
        controller.selectSource(source)
        controller.configureSystemPlayback(
            VesperSystemPlaybackConfiguration(
                metadata =
                    VesperSystemPlaybackMetadata(
                        title = source.label.ifBlank { source.uri },
                        contentUri = source.uri,
                    ),
                controls = VesperSystemPlaybackControls.videoDefault(),
            ),
        )
    }

    fun applySourceNormalizerSetting(setting: ExampleSourceNormalizerSetting) {
        if (setting == sourceNormalizerSetting) {
            return
        }
        val activeSource = activePlaylistSource
        val shouldResumePlayback = uiState.playbackState == PlaybackStateUi.Playing
        val restorePositionMs = uiState.timeline.positionMs
        sourceNormalizerSetting = setting
        if (externalSession != null) {
            scope.launch {
                runCatching { externalPlaybackController.disconnectAsync() }
            }
            externalSession = null
        }
        val nextController =
            onRebuildController(
                setting,
                activeSource,
                selectedResilienceProfile.policy,
                shouldResumePlayback,
                restorePositionMs,
            )
        if (activeSource != null) {
            nextController.configureSystemPlayback(
                VesperSystemPlaybackConfiguration(
                    metadata =
                        VesperSystemPlaybackMetadata(
                            title = activeSource.label.ifBlank { activeSource.uri },
                            contentUri = activeSource.uri,
                        ),
                    controls = VesperSystemPlaybackControls.videoDefault(),
                ),
            )
        }
        controlsVisible = true
    }

    fun externalMediaItemFor(
        source: VesperPlayerSource,
        timeline: TimelineUiState = uiState.timeline,
    ): VesperExternalPlaybackMediaItem =
        VesperExternalPlaybackMediaItem(
            sources = listOf(source),
            metadata =
                VesperSystemPlaybackMetadata(
                    title = source.label.ifBlank { source.uri },
                    contentUri = source.uri,
                    durationMs = timeline.durationMs,
                    isLive = timeline.kind != TimelineKind.Vod,
                ),
            formatAdaptation =
                VesperExternalFormatAdaptationConfig(
                    enabled = true,
                    preferredFallback = VesperExternalFallbackFormat.MpegTs,
                ),
        )

    fun updateExternalSessionError(message: String) {
        externalSession =
            externalSession?.copy(
                status = ExampleExternalPlaybackStatus.Error,
                message = message,
            )
        Toast
            .makeText(
                context,
                context.getString(R.string.example_external_route_error, message),
                Toast.LENGTH_SHORT,
            ).show()
    }

    fun applyExternalLoadResult(
        routeId: String,
        routeName: String,
        routeKind: VesperExternalPlaybackRouteKind,
        source: VesperPlayerSource,
        result: VesperExternalPlaybackResult,
        timeline: TimelineUiState = uiState.timeline,
    ) {
        when (result) {
            is VesperExternalPlaybackResult.Success -> {
                val nowMillis = System.currentTimeMillis()
                externalNowMillis = nowMillis
                externalSession =
                    ExampleExternalPlaybackSession(
                        routeId = result.routeId ?: routeId,
                        routeName = routeName,
                        routeKind = routeKind,
                        status = ExampleExternalPlaybackStatus.Playing,
                        source = source,
                        basePositionMs = timeline.externalStartPositionMs(),
                        durationMs = timeline.durationMs,
                        seekableRange = exampleSeekableRangePair(timeline),
                        startedAtMillis = nowMillis,
                        relayEnabled = result.relayEnabled,
                    )
                controller.pause()
                controlsVisible = true
            }

            is VesperExternalPlaybackResult.Unavailable -> updateExternalSessionError(result.message)
            is VesperExternalPlaybackResult.Unsupported -> updateExternalSessionError(result.message)
            is VesperExternalPlaybackResult.Failed -> updateExternalSessionError(result.message)
        }
    }

    fun loadCurrentSourceOnExternalRoute(
        routeId: String,
        routeName: String,
        routeKind: VesperExternalPlaybackRouteKind,
        sourceOverride: VesperPlayerSource? = null,
        timelineOverride: TimelineUiState? = null,
    ) {
        val source = sourceOverride ?: activePlaylistSource
        if (source == null) {
            Toast
                .makeText(
                    context,
                    context.getString(R.string.example_external_no_active_source),
                    Toast.LENGTH_SHORT,
                ).show()
            return
        }
        val timeline = timelineOverride ?: uiState.timeline
        externalSession =
            ExampleExternalPlaybackSession(
                routeId = routeId,
                routeName = routeName,
                routeKind = routeKind,
                status = ExampleExternalPlaybackStatus.Loading,
                source = source,
                basePositionMs = timeline.externalStartPositionMs(),
                durationMs = timeline.durationMs,
                seekableRange = exampleSeekableRangePair(timeline),
                startedAtMillis = null,
            )
        scope.launch {
            val result =
                externalPlaybackController.loadAsync(
                    item = externalMediaItemFor(source, timeline),
                    startPositionMs = timeline.externalStartPositionMs(),
                    autoplay = true,
                )
            applyExternalLoadResult(
                routeId = routeId,
                routeName = routeName,
                routeKind = routeKind,
                source = source,
                result = result,
                timeline = timeline,
            )
        }
    }

    fun connectExternalRoute(route: VesperExternalPlaybackRoute) {
        externalSession =
            ExampleExternalPlaybackSession(
                routeId = route.routeId,
                routeName = route.name,
                routeKind = route.kind,
                status = ExampleExternalPlaybackStatus.Connecting,
                source = activePlaylistSource,
                basePositionMs = uiState.timeline.externalStartPositionMs(),
                durationMs = uiState.timeline.durationMs,
                seekableRange = exampleSeekableRangePair(uiState.timeline),
                startedAtMillis = null,
            )
        scope.launch {
            when (val result = externalPlaybackController.connect(route.routeId)) {
                is VesperExternalPlaybackResult.Success -> {
                    externalSession =
                        externalSession?.copy(
                            status = ExampleExternalPlaybackStatus.Connected,
                            routeId = result.routeId ?: route.routeId,
                        )
                    loadCurrentSourceOnExternalRoute(
                        routeId = result.routeId ?: route.routeId,
                        routeName = route.name,
                        routeKind = route.kind,
                    )
                }
                is VesperExternalPlaybackResult.Unavailable -> updateExternalSessionError(result.message)
                is VesperExternalPlaybackResult.Unsupported -> updateExternalSessionError(result.message)
                is VesperExternalPlaybackResult.Failed -> updateExternalSessionError(result.message)
            }
        }
    }

    fun loadCurrentExternalSession() {
        val session = externalSession
        if (session != null) {
            loadCurrentSourceOnExternalRoute(
                routeId = session.routeId,
                routeName = session.routeName,
                routeKind = session.routeKind,
            )
            return
        }
        val activeRoute = externalRoutes.firstOrNull { route -> route.active }
        if (activeRoute != null) {
            connectExternalRoute(activeRoute)
        } else {
            Toast
                .makeText(
                    context,
                    context.getString(R.string.example_external_no_active_source),
                    Toast.LENGTH_SHORT,
                ).show()
        }
    }

    fun openCastRoutePicker() {
        if (isCastRoutePickerOpening) {
            return
        }
        isCastRoutePickerOpening = true
        externalPlaybackController.prepareCastAsync { available, message ->
            if (available) {
                castRoutePickerRequestId = System.currentTimeMillis()
            } else {
                Toast
                    .makeText(
                        context,
                        message ?: context.getString(R.string.example_external_route_error, "Cast is unavailable."),
                        Toast.LENGTH_SHORT,
                    ).show()
            }
            scope.launch {
                delay(700)
                isCastRoutePickerOpening = false
            }
        }
    }

    fun toggleExternalPlayback() {
        val session = externalSession ?: return
        scope.launch {
            val nowMillis = System.currentTimeMillis()
            val result =
                if (session.status == ExampleExternalPlaybackStatus.Playing) {
                    externalPlaybackController.pauseAsync()
                } else {
                    externalPlaybackController.playAsync()
                }
            when (result) {
                is VesperExternalPlaybackResult.Success -> {
                    externalNowMillis = nowMillis
                    externalSession =
                        if (session.status == ExampleExternalPlaybackStatus.Playing) {
                            examplePausedExternalSession(session, nowMillis)
                        } else {
                            examplePlayingExternalSession(session, nowMillis)
                        }.copy(relayEnabled = session.relayEnabled || result.relayEnabled)
                }
                is VesperExternalPlaybackResult.Unavailable -> updateExternalSessionError(result.message)
                is VesperExternalPlaybackResult.Unsupported -> updateExternalSessionError(result.message)
                is VesperExternalPlaybackResult.Failed -> updateExternalSessionError(result.message)
            }
        }
    }

    fun seekExternalToRatio(ratio: Float) {
        val session = externalSession ?: return
        val targetPosition = exampleExternalPositionForRatio(displayedUiState.timeline, ratio)
        scope.launch {
            when (val result = externalPlaybackController.seekToAsync(targetPosition)) {
                is VesperExternalPlaybackResult.Success -> {
                    val nowMillis = System.currentTimeMillis()
                    externalNowMillis = nowMillis
                    externalSession = exampleSeekedExternalSession(session, targetPosition, nowMillis)
                }
                is VesperExternalPlaybackResult.Unavailable -> updateExternalSessionError(result.message)
                is VesperExternalPlaybackResult.Unsupported -> updateExternalSessionError(result.message)
                is VesperExternalPlaybackResult.Failed -> updateExternalSessionError(result.message)
            }
        }
    }

    fun seekExternalToLiveEdge() {
        val targetPosition = displayedUiState.timeline.goLivePositionMs ?: return
        val session = externalSession ?: return
        scope.launch {
            when (val result = externalPlaybackController.seekToAsync(targetPosition)) {
                is VesperExternalPlaybackResult.Success -> {
                    val nowMillis = System.currentTimeMillis()
                    externalNowMillis = nowMillis
                    externalSession = exampleSeekedExternalSession(session, targetPosition, nowMillis)
                }
                is VesperExternalPlaybackResult.Unavailable -> updateExternalSessionError(result.message)
                is VesperExternalPlaybackResult.Unsupported -> updateExternalSessionError(result.message)
                is VesperExternalPlaybackResult.Failed -> updateExternalSessionError(result.message)
            }
        }
    }

    fun disconnectExternalPlayback() {
        val resumePosition = exampleDisconnectLocalPositionMs(externalSession, System.currentTimeMillis())
        scope.launch {
            runCatching { externalPlaybackController.disconnectAsync() }
            externalSession = null
            if (resumePosition != null) {
                controller.seekBy(resumePosition - uiState.timeline.positionMs)
            }
            controller.pause()
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
                        saveVideoToGallery(context, exportFile.absolutePath)
                    } else {
                        downloadManager.saveTaskOutput(
                            context = context,
                            taskId = task.taskId,
                            collection = VesperDownloadPublicCollection.Movies,
                        )
                    }
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
        if (externalSession != null) {
            disconnectExternalPlayback()
        }
        selectSourceForPlayback(source)
        controlsVisible = true
    }

    LaunchedEffect(externalSession?.status) {
        while (externalSession?.status == ExampleExternalPlaybackStatus.Playing) {
            externalNowMillis = System.currentTimeMillis()
            delay(1_000)
        }
    }

    LaunchedEffect(Unit) {
        externalPlaybackController.events.collect { event ->
            when (event.kind) {
                VesperExternalPlaybackEventKind.RouteConnected -> {
                    val routeId = event.routeId ?: return@collect
                    val routeName = event.routeName ?: "External route"
                    val route =
                        latestExternalRoutes.firstOrNull { candidate -> candidate.routeId == routeId }
                    val resolvedRouteKind = route?.kind ?: VesperExternalPlaybackRouteKind.Cast
                    val resolvedRouteName = route?.name ?: routeName
                    val currentSession = externalSession
                    if (
                        currentSession != null &&
                        currentSession.routeId == routeId &&
                        currentSession.status != ExampleExternalPlaybackStatus.Error
                    ) {
                        externalSession =
                            currentSession.copy(
                                routeName = resolvedRouteName,
                                routeKind = resolvedRouteKind,
                            )
                        return@collect
                    }
                    val source = latestActivePlaylistSource
                    externalSession =
                        ExampleExternalPlaybackSession(
                            routeId = routeId,
                            routeName = resolvedRouteName,
                            routeKind = resolvedRouteKind,
                            status = ExampleExternalPlaybackStatus.Connected,
                            source = source,
                            basePositionMs = latestUiState.timeline.externalStartPositionMs(),
                            durationMs = latestUiState.timeline.durationMs,
                            seekableRange = exampleSeekableRangePair(latestUiState.timeline),
                            startedAtMillis = null,
                        )
                    if (source != null) {
                        loadCurrentSourceOnExternalRoute(
                            routeId = routeId,
                            routeName = resolvedRouteName,
                            routeKind = resolvedRouteKind,
                            sourceOverride = source,
                            timelineOverride = latestUiState.timeline,
                        )
                    }
                }

                VesperExternalPlaybackEventKind.RouteDisconnected,
                VesperExternalPlaybackEventKind.Stopped,
                -> {
                    externalSession = null
                }

                VesperExternalPlaybackEventKind.Error,
                VesperExternalPlaybackEventKind.DiscoveryDiagnostic,
                -> {
                    event.message?.takeIf(String::isNotBlank)?.let { message ->
                        externalSession =
                            externalSession?.copy(
                                status =
                                    if (event.kind == VesperExternalPlaybackEventKind.Error) {
                                        ExampleExternalPlaybackStatus.Error
                                    } else {
                                        externalSession?.status ?: ExampleExternalPlaybackStatus.Discovering
                                    },
                                message = message,
                            )
                    }
                }

                VesperExternalPlaybackEventKind.Loaded,
                VesperExternalPlaybackEventKind.Playing,
                VesperExternalPlaybackEventKind.Paused,
                VesperExternalPlaybackEventKind.Suspended,
                -> Unit
            }
        }
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
        displayedUiState.playbackState,
        displayedUiState.isBuffering,
        controlsVisible,
        activeSheet,
        pendingSeekRatio,
    ) {
        if (
            displayedUiState.playbackState != PlaybackStateUi.Playing ||
            displayedUiState.isBuffering ||
            !controlsVisible ||
            activeSheet != null ||
            pendingSeekRatio != null
        ) {
            return@LaunchedEffect
        }

        delay(3_000)
        if (
            displayedUiState.playbackState == PlaybackStateUi.Playing &&
            !displayedUiState.isBuffering &&
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
                                uiState = displayedUiState,
                                controlsVisible = controlsVisible,
                                pendingSeekRatio = pendingSeekRatio,
                                isPortrait = false,
                                trackCatalog = trackCatalog,
                                trackSelection = trackSelection,
                                modifier = Modifier.fillMaxSize(),
                                onControlsVisibilityChange = { controlsVisible = it },
                                onPendingSeekRatioChange = { pendingSeekRatio = it },
                                onOpenSheet = { activeSheet = it },
                                onToggleFullscreen = {
                                    activity?.requestedOrientation =
                                        ActivityInfo.SCREEN_ORIENTATION_SENSOR_PORTRAIT
                                },
                                onTogglePlayback =
                                    if (externalSession.isActiveRemotePlayback()) {
                                        ::toggleExternalPlayback
                                    } else {
                                        controller::togglePause
                                    },
                                onSeekToRatio =
                                    if (externalSession.isActiveRemotePlayback()) {
                                        ::seekExternalToRatio
                                    } else {
                                        controller::seekToRatio
                                    },
                                onSeekToLiveEdge =
                                    if (externalSession.isActiveRemotePlayback()) {
                                        ::seekExternalToLiveEdge
                                    } else {
                                        controller::seekToLiveEdge
                                    },
                                onSetPlaybackRate = controller::setPlaybackRate,
                                playbackRateControlsEnabled = !externalSession.isActiveRemotePlayback(),
                                currentBrightnessRatio = deviceControls::currentBrightnessRatio,
                                onSetBrightnessRatio = deviceControls::setBrightnessRatio,
                                currentVolumeRatio = deviceControls::currentVolumeRatio,
                                onSetVolumeRatio = deviceControls::setVolumeRatio,
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
                                    sourceLabel = displayedUiState.sourceLabel,
                                    subtitle = displayedUiState.subtitle,
                                    palette = palette,
                                )

                                ExamplePlayerStage(
                                    controller = controller,
                                    uiState = displayedUiState,
                                    controlsVisible = controlsVisible,
                                    pendingSeekRatio = pendingSeekRatio,
                                    isPortrait = true,
                                    trackCatalog = trackCatalog,
                                    trackSelection = trackSelection,
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
                                    onTogglePlayback =
                                        if (externalSession.isActiveRemotePlayback()) {
                                            ::toggleExternalPlayback
                                        } else {
                                            controller::togglePause
                                        },
                                    onSeekToRatio =
                                        if (externalSession.isActiveRemotePlayback()) {
                                            ::seekExternalToRatio
                                        } else {
                                            controller::seekToRatio
                                        },
                                    onSeekToLiveEdge =
                                        if (externalSession.isActiveRemotePlayback()) {
                                            ::seekExternalToLiveEdge
                                        } else {
                                            controller::seekToLiveEdge
                                        },
                                    onSetPlaybackRate = controller::setPlaybackRate,
                                    playbackRateControlsEnabled = !externalSession.isActiveRemotePlayback(),
                                    currentBrightnessRatio = deviceControls::currentBrightnessRatio,
                                    onSetBrightnessRatio = deviceControls::setBrightnessRatio,
                                    currentVolumeRatio = deviceControls::currentVolumeRatio,
                                    onSetVolumeRatio = deviceControls::setVolumeRatio,
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
                                    onUseLiveDvrAcceptance = {
                                        val nextPlaylistItems =
                                            enqueuePlaylistItem(
                                                playlistItemIds = playlistItemIds,
                                                itemId = ANDROID_LIVE_DVR_PLAYLIST_ITEM_ID,
                                            )
                                        applyPlaylistQueue(
                                            focusItemId = ANDROID_LIVE_DVR_PLAYLIST_ITEM_ID,
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

                                ExampleExternalPlaybackSection(
                                    palette = palette,
                                    routes = externalRoutes,
                                    session = externalSession,
                                    isDiscovering = isExternalDiscoveryRunning,
                                    isCastRoutePickerOpening = isCastRoutePickerOpening,
                                    castRoutePickerRequestId = castRoutePickerRequestId,
                                    hasDlnaPermission = hasNearbyWifiPermission,
                                    onOpenCastRoutes = ::openCastRoutePicker,
                                    onRequestDlnaPermission = {
                                        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
                                            dlnaPermissionLauncher.launch(Manifest.permission.NEARBY_WIFI_DEVICES)
                                        } else {
                                            hasNearbyWifiPermission = true
                                            externalPlaybackController.startDiscovery()
                                            isExternalDiscoveryRunning = true
                                        }
                                    },
                                    onStartDiscovery = {
                                        externalPlaybackController.startDiscovery()
                                        isExternalDiscoveryRunning = true
                                    },
                                    onStopDiscovery = {
                                        externalPlaybackController.stopDiscovery()
                                        isExternalDiscoveryRunning = false
                                    },
                                    onConnectRoute = ::connectExternalRoute,
                                    onLoadCurrent = ::loadCurrentExternalSession,
                                    onDisconnect = ::disconnectExternalPlayback,
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

                                ExamplePluginDiagnosticsSection(
                                    palette = palette,
                                    sourceNormalizerSetting = sourceNormalizerSetting,
                                    sourceNormalizerPluginLibraryPaths = sourceNormalizerPluginLibraryPaths,
                                    frameProcessorPluginLibraryPaths = frameProcessorPluginLibraryPaths,
                                    pluginDiagnostics = controller.pluginDiagnostics,
                                    onSourceNormalizerSettingChange = ::applySourceNormalizerSetting,
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
                            uiState = displayedUiState,
                            trackCatalog = trackCatalog,
                            trackSelection = trackSelection,
                            onDismiss = { activeSheet = null },
                            playbackRateControlsEnabled = !externalSession.isActiveRemotePlayback(),
                            onOpenSheet = {
                                if (it != ExamplePlayerSheet.Speed || !externalSession.isActiveRemotePlayback()) {
                                    activeSheet = it
                                }
                            },
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

private class ExampleAndroidDeviceControls(
    private val context: Context,
    private val activity: Activity?,
) {
    private val audioManager: AudioManager?
        get() = context.getSystemService(Context.AUDIO_SERVICE) as? AudioManager

    fun currentBrightnessRatio(): Float? {
        val windowBrightness = activity?.window?.attributes?.screenBrightness
        if (windowBrightness != null && windowBrightness >= 0f) {
            return windowBrightness.coerceIn(0f, 1f)
        }
        return runCatching {
            Settings.System.getInt(context.contentResolver, Settings.System.SCREEN_BRIGHTNESS) / 255f
        }.getOrDefault(0.5f).coerceIn(0f, 1f)
    }

    fun setBrightnessRatio(ratio: Float): Float? {
        val window = activity?.window ?: return null
        val nextRatio = ratio.coerceIn(0.02f, 1f)
        val attributes = window.attributes
        attributes.screenBrightness = nextRatio
        window.attributes = attributes
        return nextRatio
    }

    fun currentVolumeRatio(): Float? {
        val audioManager = audioManager ?: return null
        val maxVolume = audioManager.getStreamMaxVolume(AudioManager.STREAM_MUSIC)
        if (maxVolume <= 0) {
            return null
        }
        return (audioManager.getStreamVolume(AudioManager.STREAM_MUSIC).toFloat() / maxVolume)
            .coerceIn(0f, 1f)
    }

    fun setVolumeRatio(ratio: Float): Float? {
        val audioManager = audioManager ?: return null
        val maxVolume = audioManager.getStreamMaxVolume(AudioManager.STREAM_MUSIC)
        if (maxVolume <= 0) {
            return null
        }
        val nextVolume = (ratio.coerceIn(0f, 1f) * maxVolume).roundToInt().coerceIn(0, maxVolume)
        return runCatching {
            audioManager.setStreamVolume(AudioManager.STREAM_MUSIC, nextVolume, 0)
            audioManager.getStreamVolume(AudioManager.STREAM_MUSIC).toFloat() / maxVolume
        }.getOrNull()?.coerceIn(0f, 1f)
    }
}

private fun Context.hasNearbyWifiPermission(): Boolean =
    Build.VERSION.SDK_INT < Build.VERSION_CODES.TIRAMISU ||
        ContextCompat.checkSelfPermission(
            this,
            Manifest.permission.NEARBY_WIFI_DEVICES,
        ) == PackageManager.PERMISSION_GRANTED

private enum class ExampleHostTab {
    Player,
    Downloads,
}
