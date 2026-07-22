package io.github.ikaros.vesper.example.androidcomposehost

import android.Manifest
import android.app.Activity
import android.app.PictureInPictureParams
import android.content.Context
import android.content.Intent
import android.content.pm.ActivityInfo
import android.content.pm.PackageManager
import android.content.res.Configuration
import android.media.AudioManager
import android.os.Build
import android.provider.Settings
import android.util.Log
import android.util.Rational
import android.widget.Toast
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.background
import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.safeDrawing
import androidx.compose.foundation.layout.windowInsetsPadding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.LazyListScope
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.rounded.Download
import androidx.compose.material.icons.rounded.Settings
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
import androidx.compose.runtime.DisposableEffect
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
import androidx.compose.runtime.withFrameNanos
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
import io.github.ikaros.vesper.player.android.VesperPlayerSourceProtocol
import io.github.ikaros.vesper.player.android.VesperPlayerUnsupportedOperation
import io.github.ikaros.vesper.player.android.VesperBackgroundPlaybackMode
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
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.flow.collect
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext

@Composable
internal fun PlayerHostApp(
    controller: VesperPlayerController,
    isInPictureInPictureMode: Boolean = false,
    userLeaveHintGeneration: Long = 0L,
    onRebuildController: (
        ExampleSourceNormalizerSetting,
        ExampleNativeFramePipelineSetting,
        ExampleVideoSurfaceSetting,
        VesperPlayerSource?,
        VesperPlaybackResiliencePolicy,
        Boolean,
        Long?,
        Float,
    ) -> VesperPlayerController,
    playlistCoordinator: VesperPlaylistCoordinator,
    downloadManager: VesperDownloadManager,
    externalPlaybackController: VesperExternalPlaybackController,
    isDownloadExportPluginInstalled: Boolean,
    sourceNormalizerPluginLibraryPaths: List<String>,
    decoderMediaCodecPluginLibraryPaths: List<String>,
    frameProcessorPluginLibraryPaths: List<String>,
) {
    val context = LocalContext.current
    val activity = remember(context) { context.findActivity() }
    val deviceControls = remember(context, activity) {
        ExampleAndroidDeviceControls(context.applicationContext, activity)
    }
    val configuration = LocalConfiguration.current
    val isLandscape = configuration.orientation == Configuration.ORIENTATION_LANDSCAPE
    var selectedTab by rememberSaveable { mutableStateOf(ExampleHostTab.Play) }

    var themeMode by rememberSaveable { mutableStateOf(ExampleThemeMode.System) }
    var selectedResilienceProfile by rememberSaveable {
        mutableStateOf(ExampleResilienceProfile.Balanced)
    }
    var sourceNormalizerSetting by rememberSaveable {
        mutableStateOf(ExampleSourceNormalizerSetting.PreflightOnly)
    }
    var nativeFramePipelineSetting by rememberSaveable {
        mutableStateOf(ExampleNativeFramePipelineSetting.DiagnosticsOnly)
    }
    var videoSurfaceSetting by rememberSaveable {
        mutableStateOf(ExampleVideoSurfaceSetting.SurfaceView)
    }
    var selectedHdrEvidencePreset by remember {
        mutableStateOf(exampleHdrEvidenceP0Presets[1])
    }
    var selectedDolbyDrmKind by rememberSaveable {
        mutableStateOf(ExampleDolbyAcceptanceDrmKind.Clear)
    }
    var selectedDolbyProfile by rememberSaveable {
        mutableStateOf<ExampleDolbyAcceptanceProfile?>(null)
    }
    var selectedDolbyFps by rememberSaveable {
        mutableStateOf<Int?>(null)
    }
    var isCapturingHdrEvidence by remember { mutableStateOf(false) }
    val systemDarkTheme = isSystemInDarkTheme()
    val useDarkTheme =
        when (themeMode) {
            ExampleThemeMode.System -> systemDarkTheme
            ExampleThemeMode.Light -> false
            ExampleThemeMode.Dark -> true
        }

    val immersivePlayer = isLandscape && selectedTab == ExampleHostTab.Play

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
    val playlistSnapshot by playlistCoordinator.snapshot.collectAsState()

    var remoteStreamUrl by rememberSaveable { mutableStateOf(ANDROID_HLS_DEMO_URL) }
    var downloadRemoteUrl by rememberSaveable { mutableStateOf(ANDROID_HLS_DEMO_URL) }
    var controlsVisible by rememberSaveable { mutableStateOf(true) }
    var activeSheet by rememberSaveable { mutableStateOf<ExamplePlayerSheet?>(null) }
    var pendingSeekRatio by remember { mutableStateOf<Float?>(null) }
    var isApplyingResilienceProfile by remember { mutableStateOf(false) }
    var hasHandledFinishedPlayback by remember { mutableStateOf(false) }
    var queuedRemoteSource by remember { mutableStateOf<VesperPlayerSource?>(null) }
    var queuedLocalSource by remember { mutableStateOf<VesperPlayerSource?>(null) }
    var activePlaybackSource by remember { mutableStateOf<VesperPlayerSource?>(null) }
    var localPickRequestId by remember { mutableStateOf(0L) }
    var playlistItemIds by remember {
        mutableStateOf(listOf(ANDROID_HLS_PLAYLIST_ITEM_ID))
    }
    var pendingDownloadTasks by remember { mutableStateOf<List<ExamplePendingDownloadTask>>(emptyList()) }
    var savingTaskIds by remember { mutableStateOf(setOf<Long>()) }
    var exportProgressByTaskId by remember { mutableStateOf<Map<Long, Float>>(emptyMap()) }
    var externalSession by remember { mutableStateOf<ExampleExternalPlaybackSession?>(null) }
    var playbackOrigin by remember { mutableStateOf<ExamplePlaybackOrigin?>(null) }
    var hostLogEntries by remember { mutableStateOf<List<ExampleHostLogEntry>>(emptyList()) }
    var nextHostLogId by remember { mutableStateOf(0L) }
    var pictureInPictureEnabled by rememberSaveable { mutableStateOf(false) }
    var pictureInPicturePresentationState by remember {
        mutableStateOf(ExamplePictureInPicturePresentationState())
    }
    var isExternalDiscoveryRunning by rememberSaveable { mutableStateOf(false) }
    var isCastRoutePickerOpening by remember { mutableStateOf(false) }
    var castRoutePickerRequestId by remember { mutableStateOf(0L) }
    var externalNowMillis by remember { mutableStateOf(System.currentTimeMillis()) }
    var frameMetricsEnabled by rememberSaveable { mutableStateOf(false) }
    var frameMetricsSnapshot by remember { mutableStateOf<ExampleFrameMetricsSnapshot?>(null) }
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
    val controllerRebuildSource =
        exampleControllerRebuildSource(activePlaybackSource, activePlaylistSource)
    val latestActivePlaybackSource by rememberUpdatedState(controllerRebuildSource)
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

    val pictureInPicturePresentation = pictureInPicturePresentationState.presentation

    fun recordHostLog(
        severity: ExampleHostLogSeverity,
        title: String,
        detail: String? = null,
    ) {
        nextHostLogId += 1L
        hostLogEntries =
            appendExampleHostLogEntry(
                entries = hostLogEntries,
                entry =
                    ExampleHostLogEntry(
                        id = nextHostLogId,
                        atMillis = System.currentTimeMillis(),
                        severity = severity,
                        title = title,
                        detail = detail,
                    ),
            )
    }

    LaunchedEffect(isInPictureInPictureMode) {
        pictureInPicturePresentationState =
            pictureInPicturePresentationState.onPictureInPictureModeChanged(
                isInPictureInPictureMode,
            )
    }

    LaunchedEffect(userLeaveHintGeneration) {
        if (userLeaveHintGeneration == 0L) {
            return@LaunchedEffect
        }
        pictureInPicturePresentationState =
            pictureInPicturePresentationState.onPictureInPictureUserLeaveHint(
                pictureInPictureEnabled,
            )
        if (!pictureInPictureEnabled) {
            return@LaunchedEffect
        }
        delay(900)
        pictureInPicturePresentationState =
            pictureInPicturePresentationState.onPictureInPictureAutoEnterTimeout()
    }

    LaunchedEffect(activity, pictureInPictureEnabled) {
        val hostActivity = activity ?: return@LaunchedEffect
        if (!hostActivity.supportsExamplePictureInPicture()) {
            return@LaunchedEffect
        }
        runCatching {
            hostActivity.setPictureInPictureParams(
                buildExamplePictureInPictureParams(autoEnter = pictureInPictureEnabled),
            )
        }
    }

    DisposableEffect(activity, frameMetricsEnabled) {
        if (activity == null || !frameMetricsEnabled) {
            onDispose { }
        } else {
            val probe =
                ExampleFrameMetricsProbe(activity) { snapshot ->
                    frameMetricsSnapshot = snapshot
                }
            probe.start()
            onDispose { probe.stop() }
        }
    }

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
                recordHostLog(
                    severity = ExampleHostLogSeverity.Error,
                    title = context.getString(R.string.example_log_download_create_failed),
                    detail = error.localizedMessage ?: error::class.java.simpleName,
                )
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

    fun requestExamplePictureInPicture() {
        val hostActivity = activity
        if (!pictureInPictureEnabled || hostActivity == null) {
            Toast
                .makeText(
                    context,
                    context.getString(R.string.example_pip_unavailable),
                    Toast.LENGTH_SHORT,
                ).show()
            return
        }
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.O) {
            Toast
                .makeText(
                    context,
                    context.getString(R.string.example_pip_unavailable),
                    Toast.LENGTH_SHORT,
                ).show()
            return
        }
        pictureInPicturePresentationState =
            pictureInPicturePresentationState.onPictureInPictureRequestStarted()
        activeSheet = null
        controlsVisible = false
        pendingSeekRatio = null
        scope.launch {
            withFrameNanos { }
            val params =
                buildExamplePictureInPictureParams(autoEnter = pictureInPictureEnabled)
            val entered =
                runCatching { hostActivity.enterPictureInPictureMode(params) }
                    .getOrDefault(false)
            if (!entered && !hostActivity.isInPictureInPictureMode) {
                pictureInPicturePresentationState =
                    pictureInPicturePresentationState.onPictureInPictureRequestRejected()
                Toast
                    .makeText(
                        context,
                        context.getString(R.string.example_pip_unavailable),
                        Toast.LENGTH_SHORT,
                    ).show()
            }
        }
    }

    fun captureHdrEvidence() {
        if (isCapturingHdrEvidence) {
            return
        }
        val preset = selectedHdrEvidencePreset
        val dolbyAcceptancePreset = exampleDolbyAcceptancePresetById(preset.sampleId)
        val source =
            if (preset.sampleId == "NETWORK-FAILURE-CONTROL") {
                VesperPlayerSource.remote(
                    uri = ANDROID_HDR_EVIDENCE_NETWORK_CONTROL_URL,
                    label = context.getString(R.string.example_plugins_hdr_evidence_network_control_label),
                    protocol = VesperPlayerSourceProtocol.Progressive,
                )
            } else if (dolbyAcceptancePreset != null) {
                dolbyAcceptancePreset.source
            } else {
                controllerRebuildSource
            }
        if (source == null) {
            Toast
                .makeText(
                    context,
                    R.string.example_plugins_hdr_evidence_select_source,
                    Toast.LENGTH_SHORT,
                ).show()
            return
        }

        isCapturingHdrEvidence = true
        Toast
            .makeText(
                context,
                R.string.example_plugins_hdr_evidence_capturing,
                Toast.LENGTH_SHORT,
            ).show()
        scope.launch {
            val result =
                runCatching {
                    val networkFailureEvidence =
                        if (preset.sampleId == "NETWORK-FAILURE-CONTROL") {
                            activePlaybackSource = source
                            controller.selectSourceAsync(source)
                            controller.configureSystemPlayback(
                                VesperSystemPlaybackConfiguration(
                                    metadata =
                                        VesperSystemPlaybackMetadata(
                                            title = source.label.ifBlank { source.uri },
                                            contentUri = source.uri,
                                        ),
                                    backgroundMode = VesperBackgroundPlaybackMode.Disabled,
                                    controls = VesperSystemPlaybackControls.videoDefault(),
                                ),
                            )
                            controller.play()
                            withContext(Dispatchers.IO) {
                                captureControlledNetworkFailureEvidence(source.uri)
                            }
                        } else {
                            null
                        }
                    captureExampleHdrEvidenceBundle(
                        ExampleHdrEvidenceCaptureContext(
                            context = context,
                            preset = preset,
                            source = source,
                            controller = controller,
                            networkFailureEvidence = networkFailureEvidence,
                            sourceNormalizerSetting = sourceNormalizerSetting,
                            nativeFramePipelineSetting = nativeFramePipelineSetting,
                            sourceNormalizerPluginLibraryPaths = sourceNormalizerPluginLibraryPaths,
                            decoderMediaCodecPluginLibraryPaths = decoderMediaCodecPluginLibraryPaths,
                            frameProcessorPluginLibraryPaths = frameProcessorPluginLibraryPaths,
                        ),
                    )
                }
            result.fold(
                onSuccess = { directory ->
                    recordHostLog(
                        severity = ExampleHostLogSeverity.Info,
                        title = context.getString(R.string.example_log_hdr_evidence_written),
                        detail = directory.absolutePath,
                    )
                    Toast
                        .makeText(
                            context,
                            context.getString(
                                R.string.example_plugins_hdr_evidence_written,
                                directory.absolutePath,
                            ),
                            Toast.LENGTH_LONG,
                        ).show()
                },
                onFailure = { error ->
                    recordHostLog(
                        severity = ExampleHostLogSeverity.Error,
                        title = context.getString(R.string.example_log_hdr_evidence_failed),
                        detail = error.message ?: error::class.java.simpleName,
                    )
                    Toast
                        .makeText(
                            context,
                            context.getString(
                                R.string.example_plugins_hdr_evidence_failed,
                                error.message ?: error::class.java.simpleName,
                            ),
                            Toast.LENGTH_LONG,
                        ).show()
                },
            )
            isCapturingHdrEvidence = false
        }
    }

    fun selectSourceForPlayback(
        source: VesperPlayerSource,
        origin: ExamplePlaybackOrigin?,
    ) {
        activePlaybackSource = source
        playbackOrigin = origin
        scope.launch {
            runCatching {
                controller.selectSourceAsync(source)
            }.onFailure { error ->
                Log.e(
                    PLAYER_HOST_EXAMPLE_TAG,
                    "failed to select source=${source.uri}",
                    error,
                )
                Toast
                    .makeText(
                        context,
                        error.localizedMessage ?: error::class.java.simpleName,
                        Toast.LENGTH_LONG,
                    ).show()
                return@launch
            }
            controller.configureSystemPlayback(
                VesperSystemPlaybackConfiguration(
                    metadata =
                        VesperSystemPlaybackMetadata(
                            title = source.label.ifBlank { source.uri },
                            contentUri = source.uri,
                        ),
                    backgroundMode = VesperBackgroundPlaybackMode.Disabled,
                    controls = VesperSystemPlaybackControls.videoDefault(),
                ),
            )
            recordHostLog(
                severity = ExampleHostLogSeverity.Info,
                title = context.getString(R.string.example_log_source_selected),
                detail = source.label.ifBlank { source.uri },
            )
        }
    }

    fun handleDolbyAcceptanceSelectionFailure(
        preset: ExampleDolbyAcceptancePreset,
        error: Throwable,
    ) {
        val details =
            (error as? VesperPlayerUnsupportedOperation)
                ?.details
                ?.entries
                ?.joinToString(separator = ", ") { (key, value) -> "$key=$value" }
                ?.takeUnless(String::isBlank)
        val message =
            listOfNotNull(
                error.localizedMessage ?: error::class.java.simpleName,
                details?.let { "details: $it" },
            ).joinToString(separator = "\n")
        Log.e(
            PLAYER_HOST_EXAMPLE_TAG,
            "dolby acceptance failed preset=${preset.id} $details",
            error,
        )
        Toast.makeText(context, message, Toast.LENGTH_LONG).show()
        controlsVisible = true
    }

    fun activateDolbyAcceptancePreset(
        preset: ExampleDolbyAcceptancePreset,
        origin: ExamplePlaybackOrigin,
    ) {
        if (!preset.isPlayable) {
            Toast
                .makeText(
                    context,
                    R.string.example_dolby_acceptance_pending_toast,
                    Toast.LENGTH_SHORT,
                ).show()
            return
        }
        playbackOrigin = origin
        val previousSourceNormalizerSetting = sourceNormalizerSetting
        val previousNativeFramePipelineSetting = nativeFramePipelineSetting
        var nextSourceNormalizerSetting = sourceNormalizerSetting
        var nextNativeFramePipelineSetting = nativeFramePipelineSetting
        val requiresDirectNativeRoute = preset.source.drmConfiguration != null
        if (requiresDirectNativeRoute && sourceNormalizerSetting != ExampleSourceNormalizerSetting.Disabled) {
            nextSourceNormalizerSetting = ExampleSourceNormalizerSetting.Disabled
        } else if (
            !requiresDirectNativeRoute &&
            sourceNormalizerSetting != ExampleSourceNormalizerSetting.Disabled &&
            sourceNormalizerSetting != ExampleSourceNormalizerSetting.DiagnosticsOnly
        ) {
            nextSourceNormalizerSetting = ExampleSourceNormalizerSetting.Disabled
        }
        if (requiresDirectNativeRoute && nativeFramePipelineSetting != ExampleNativeFramePipelineSetting.Disabled) {
            nextNativeFramePipelineSetting = ExampleNativeFramePipelineSetting.Disabled
        } else if (
            !requiresDirectNativeRoute &&
            nativeFramePipelineSetting != ExampleNativeFramePipelineSetting.Disabled &&
            nativeFramePipelineSetting != ExampleNativeFramePipelineSetting.DiagnosticsOnly
        ) {
            nextNativeFramePipelineSetting = ExampleNativeFramePipelineSetting.DiagnosticsOnly
        }
        val needsControllerRebuild =
            requiresDirectNativeRoute ||
                nextSourceNormalizerSetting != sourceNormalizerSetting ||
                nextNativeFramePipelineSetting != nativeFramePipelineSetting
        activePlaybackSource = preset.source
        if (externalSession != null) {
            scope.launch {
                runCatching { externalPlaybackController.disconnectAsync() }
            }
            externalSession = null
        }
        if (needsControllerRebuild) {
            val rebuildSnapshot = exampleControllerRebuildSnapshot(latestUiState)
            sourceNormalizerSetting = nextSourceNormalizerSetting
            nativeFramePipelineSetting = nextNativeFramePipelineSetting
            val nextController =
                runCatching {
                    onRebuildController(
                        nextSourceNormalizerSetting,
                        nextNativeFramePipelineSetting,
                        videoSurfaceSetting,
                        preset.source,
                        selectedResilienceProfile.policy,
                        rebuildSnapshot.shouldResumePlayback,
                        null,
                        rebuildSnapshot.restorePlaybackRate,
                    )
                }.getOrElse { error ->
                    sourceNormalizerSetting = previousSourceNormalizerSetting
                    nativeFramePipelineSetting = previousNativeFramePipelineSetting
                    handleDolbyAcceptanceSelectionFailure(preset, error)
                    return
                }
            nextController.configureSystemPlayback(
                VesperSystemPlaybackConfiguration(
                    metadata =
                        VesperSystemPlaybackMetadata(
                            title = preset.source.label.ifBlank { preset.source.uri },
                            contentUri = preset.source.uri,
                        ),
                    backgroundMode = VesperBackgroundPlaybackMode.Disabled,
                    controls = VesperSystemPlaybackControls.videoDefault(),
                ),
            )
            recordHostLog(
                severity = ExampleHostLogSeverity.Info,
                title = context.getString(R.string.example_log_controller_rebuilt),
                detail = preset.label,
            )
            Toast
                .makeText(
                    context,
                    R.string.example_dolby_acceptance_direct_route_toast,
                    Toast.LENGTH_SHORT,
                ).show()
        } else {
            selectSourceForPlayback(preset.source, origin)
        }
        selectedHdrEvidencePreset = preset.toHdrEvidencePreset()
        controlsVisible = true
        recordHostLog(
            severity = ExampleHostLogSeverity.Info,
            title = context.getString(R.string.example_log_dolby_play_now),
            detail = preset.label,
        )
        Log.i(
            PLAYER_HOST_EXAMPLE_TAG,
            "dolby acceptance preset=${preset.id} route=directNative " +
                "sourceNormalizer=$previousSourceNormalizerSetting->$nextSourceNormalizerSetting " +
                "nativeFrame=$previousNativeFramePipelineSetting->$nextNativeFramePipelineSetting",
        )
    }

    fun applySourceNormalizerSetting(setting: ExampleSourceNormalizerSetting) {
        if (setting == sourceNormalizerSetting) {
            return
        }
        val activeSource = latestActivePlaybackSource
        val rebuildSnapshot = exampleControllerRebuildSnapshot(latestUiState)
        Log.i(
            PLAYER_HOST_EXAMPLE_TAG,
            "source-normalizer setting previous=$sourceNormalizerSetting next=$setting " +
                "source=${activeSource?.uri ?: "none"} " +
                "resume=${rebuildSnapshot.shouldResumePlayback} " +
                "positionMs=${rebuildSnapshot.restorePositionMs}",
        )
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
                nativeFramePipelineSetting,
                videoSurfaceSetting,
                activeSource,
                selectedResilienceProfile.policy,
                rebuildSnapshot.shouldResumePlayback,
                rebuildSnapshot.restorePositionMs,
                rebuildSnapshot.restorePlaybackRate,
            )
        recordHostLog(
            severity = ExampleHostLogSeverity.Info,
            title = context.getString(R.string.example_log_source_normalizer_changed),
            detail = context.getString(setting.titleRes),
        )
        if (activeSource != null) {
            nextController.configureSystemPlayback(
                VesperSystemPlaybackConfiguration(
                    metadata =
                        VesperSystemPlaybackMetadata(
                            title = activeSource.label.ifBlank { activeSource.uri },
                            contentUri = activeSource.uri,
                        ),
                    backgroundMode = VesperBackgroundPlaybackMode.Disabled,
                    controls = VesperSystemPlaybackControls.videoDefault(),
                ),
            )
        }
        controlsVisible = true
    }

    fun applyNativeFramePipelineSetting(setting: ExampleNativeFramePipelineSetting) {
        if (setting == nativeFramePipelineSetting) {
            return
        }
        val activeSource = latestActivePlaybackSource
        val rebuildSnapshot = exampleControllerRebuildSnapshot(latestUiState)
        val previousSetting = nativeFramePipelineSetting
        val requiresControllerRebuild =
            exampleNativeFrameSettingRequiresControllerRebuild(previousSetting, setting)
        Log.i(
            PLAYER_HOST_EXAMPLE_TAG,
            "native-frame setting previous=$previousSetting next=$setting " +
                "rebuild=$requiresControllerRebuild source=${activeSource?.uri ?: "none"} " +
                "resume=${rebuildSnapshot.shouldResumePlayback} " +
                "positionMs=${rebuildSnapshot.restorePositionMs}",
        )
        nativeFramePipelineSetting = setting
        if (!requiresControllerRebuild) {
            recordHostLog(
                severity = ExampleHostLogSeverity.Info,
                title = context.getString(R.string.example_log_native_frame_changed),
                detail = context.getString(setting.titleRes),
            )
            controlsVisible = true
            return
        }
        if (externalSession != null) {
            scope.launch {
                runCatching { externalPlaybackController.disconnectAsync() }
            }
            externalSession = null
        }
        val nextController =
            onRebuildController(
                sourceNormalizerSetting,
                setting,
                videoSurfaceSetting,
                activeSource,
                selectedResilienceProfile.policy,
                rebuildSnapshot.shouldResumePlayback,
                rebuildSnapshot.restorePositionMs,
                rebuildSnapshot.restorePlaybackRate,
            )
        recordHostLog(
            severity = ExampleHostLogSeverity.Info,
            title = context.getString(R.string.example_log_native_frame_changed),
            detail = context.getString(setting.titleRes),
        )
        if (activeSource != null) {
            nextController.configureSystemPlayback(
                VesperSystemPlaybackConfiguration(
                    metadata =
                        VesperSystemPlaybackMetadata(
                            title = activeSource.label.ifBlank { activeSource.uri },
                            contentUri = activeSource.uri,
                        ),
                    backgroundMode = VesperBackgroundPlaybackMode.Disabled,
                    controls = VesperSystemPlaybackControls.videoDefault(),
                ),
            )
        }
        controlsVisible = true
    }

    fun applyVideoSurfaceSetting(setting: ExampleVideoSurfaceSetting) {
        if (setting == videoSurfaceSetting) {
            return
        }
        val activeSource = latestActivePlaybackSource
        val rebuildSnapshot = exampleControllerRebuildSnapshot(latestUiState)
        val previousSetting = videoSurfaceSetting
        val previousSurfaceKind =
            exampleSurfaceKindForSettings(
                setting = nativeFramePipelineSetting,
                surfaceSetting = previousSetting,
                source = activeSource,
            )
        val nextSurfaceKind =
            exampleSurfaceKindForSettings(
                setting = nativeFramePipelineSetting,
                surfaceSetting = setting,
                source = activeSource,
            )
        val requiresControllerRebuild = previousSurfaceKind != nextSurfaceKind
        Log.i(
            PLAYER_HOST_EXAMPLE_TAG,
            "video-surface setting previous=$previousSetting next=$setting " +
                "effective=$previousSurfaceKind->$nextSurfaceKind " +
                "rebuild=$requiresControllerRebuild source=${activeSource?.uri ?: "none"} " +
                "resume=${rebuildSnapshot.shouldResumePlayback} " +
                "positionMs=${rebuildSnapshot.restorePositionMs}",
        )
        videoSurfaceSetting = setting
        if (!requiresControllerRebuild) {
            recordHostLog(
                severity = ExampleHostLogSeverity.Info,
                title = context.getString(R.string.example_log_video_surface_changed),
                detail = context.getString(setting.titleRes),
            )
            controlsVisible = true
            return
        }
        if (externalSession != null) {
            scope.launch {
                runCatching { externalPlaybackController.disconnectAsync() }
            }
            externalSession = null
        }
        val nextController =
            onRebuildController(
                sourceNormalizerSetting,
                nativeFramePipelineSetting,
                setting,
                activeSource,
                selectedResilienceProfile.policy,
                rebuildSnapshot.shouldResumePlayback,
                rebuildSnapshot.restorePositionMs,
                rebuildSnapshot.restorePlaybackRate,
            )
        recordHostLog(
            severity = ExampleHostLogSeverity.Info,
            title = context.getString(R.string.example_log_video_surface_changed),
            detail = context.getString(setting.titleRes),
        )
        if (activeSource != null) {
            nextController.configureSystemPlayback(
                VesperSystemPlaybackConfiguration(
                    metadata =
                        VesperSystemPlaybackMetadata(
                            title = activeSource.label.ifBlank { activeSource.uri },
                            contentUri = activeSource.uri,
                        ),
                    backgroundMode = VesperBackgroundPlaybackMode.Disabled,
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
        val source = sourceOverride ?: controllerRebuildSource
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
                source = controllerRebuildSource,
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
        val activeRoute = externalPlaybackController.routes.value.firstOrNull { route -> route.active }
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
                        withContext(Dispatchers.IO) {
                            downloadManager.saveTaskOutput(
                                context = context,
                                taskId = task.taskId,
                                collection = VesperDownloadPublicCollection.Movies,
                            )
                        }
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

    fun addDolbyPresetToQueue(preset: ExampleDolbyAcceptancePreset) {
        if (!canQueueDolbyPreset(preset)) {
            Toast
                .makeText(
                    context,
                    R.string.example_dolby_acceptance_pending_toast,
                    Toast.LENGTH_SHORT,
                ).show()
            return
        }
        val itemId = dolbyPlaylistItemId(preset.id)
        val nextPlaylistItems =
            enqueuePlaylistItem(
                playlistItemIds = playlistItemIds,
                itemId = itemId,
            )
        applyPlaylistQueue(
            focusItemId = playlistSnapshot.activeItem?.itemId,
            playlistItems = nextPlaylistItems,
        )
        recordHostLog(
            severity = ExampleHostLogSeverity.Info,
            title = context.getString(R.string.example_log_dolby_added_to_queue),
            detail = preset.label,
        )
    }

    val pickVideoLauncher = rememberLauncherForActivityResult(
        contract = ActivityResultContracts.OpenDocument(),
    ) { uri ->
        uri ?: return@rememberLauncherForActivityResult
        val label = displayNameForUri(context, uri)
        localPickRequestId += 1
        val requestId = localPickRequestId
        runCatching {
            context.contentResolver.takePersistableUriPermission(
                uri,
                Intent.FLAG_GRANT_READ_URI_PERMISSION,
            )
        }
        Toast
            .makeText(context, R.string.example_sources_prepare_local_video, Toast.LENGTH_SHORT)
            .show()
        scope.launch {
            val result =
                runCatching {
                    materializeExampleLocalVideoSource(
                        context = context,
                        uri = uri,
                        label = label,
                    )
                }
            if (requestId != localPickRequestId) {
                return@launch
            }
            val localSource =
                result.getOrElse { error ->
                    Log.w(
                        PLAYER_HOST_EXAMPLE_TAG,
                        "failed to materialize local video uri=$uri",
                        error,
                    )
                    Toast
                        .makeText(
                            context,
                            context.getString(
                                R.string.example_sources_prepare_local_failed,
                                error.localizedMessage
                                    ?: context.getString(R.string.example_download_save_to_gallery_failed_unknown),
                            ),
                            Toast.LENGTH_SHORT,
                        ).show()
                    return@launch
                }
            Log.i(
                PLAYER_HOST_EXAMPLE_TAG,
                "picked local video materialized uri=${localSource.uri} original=$uri",
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
    }

    LaunchedEffect(Unit) {
        applyPlaylistQueue(focusItemId = ANDROID_HLS_PLAYLIST_ITEM_ID)
    }

    LaunchedEffect(playlistSnapshot.activeItem?.itemId, activePlaylistSource?.uri) {
        val activeItem = playlistSnapshot.activeItem ?: return@LaunchedEffect
        val source =
            playlistSnapshot.queue
                .firstOrNull { it.item.itemId == activeItem.itemId }
                ?.item?.source ?: return@LaunchedEffect
        if (externalSession != null) {
            disconnectExternalPlayback()
        }
        val queueOrigin = ExamplePlaybackOrigin.Queue(activeItem.itemId)
        val dolbyPreset =
            dolbyPresetIdFromPlaylistItemId(activeItem.itemId)
                ?.let(::exampleDolbyAcceptancePresetById)
        if (dolbyPreset != null) {
            activateDolbyAcceptancePreset(dolbyPreset, queueOrigin)
        } else {
            selectSourceForPlayback(source, queueOrigin)
        }
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
                        externalPlaybackController.routes.value
                            .firstOrNull { candidate -> candidate.routeId == routeId }
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
                    val source = latestActivePlaybackSource
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
                    recordHostLog(
                        severity = ExampleHostLogSeverity.Warning,
                        title = context.getString(R.string.example_log_external_disconnected),
                        detail = event.routeName ?: event.routeId,
                    )
                    externalSession = null
                }

                VesperExternalPlaybackEventKind.Error,
                VesperExternalPlaybackEventKind.DiscoveryDiagnostic,
                -> {
                    event.message?.takeIf(String::isNotBlank)?.let { message ->
                        recordHostLog(
                            severity =
                                if (event.kind == VesperExternalPlaybackEventKind.Error) {
                                    ExampleHostLogSeverity.Error
                                } else {
                                    ExampleHostLogSeverity.Warning
                                },
                            title = context.getString(R.string.example_log_external_event),
                            detail = message,
                        )
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
        if (
            !hasHandledFinishedPlayback &&
            shouldAdvancePlaylistOnFinished(
                origin = playbackOrigin,
                activeItemId = playlistSnapshot.activeItem?.itemId,
            )
        ) {
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
        if (pictureInPicturePresentation) {
            Surface(
                modifier = Modifier.fillMaxSize(),
                color = palette.pageBottom,
            ) {
                ExamplePlayerStageWithTracks(
                    controller = controller,
                    uiState = displayedUiState,
                    controlsVisible = false,
                    pendingSeekRatio = null,
                    isPortrait = false,
                    modifier = Modifier.fillMaxSize(),
                    pictureInPicturePresentation = true,
                    onControlsVisibilityChange = { _ -> },
                    onPendingSeekRatioChange = { _ -> },
                    onOpenSheet = { _ -> },
                    onToggleFullscreen = {},
                    onTogglePlayback = {},
                    onSeekToRatio = { _ -> },
                    onSeekToLiveEdge = {},
                    onSetPlaybackRate = { _ -> },
                    playbackRateControlsEnabled = false,
                )
            }
        } else {
            Scaffold(
            modifier = Modifier.fillMaxSize(),
            containerColor = palette.pageBottom,
            bottomBar = {
                if (!immersivePlayer) {
                    NavigationBar {
                        NavigationBarItem(
                            selected = selectedTab == ExampleHostTab.Play,
                            onClick = { selectedTab = ExampleHostTab.Play },
                            icon = {
                                androidx.compose.material3.Icon(
                                    imageVector = Icons.Rounded.VideoLibrary,
                                    contentDescription = null,
                                )
                            },
                            label = { Text(stringResource(R.string.example_tab_player)) },
                        )
                        NavigationBarItem(
                            selected = selectedTab == ExampleHostTab.Diagnostics,
                            onClick = { selectedTab = ExampleHostTab.Diagnostics },
                            icon = {
                                androidx.compose.material3.Icon(
                                    imageVector = Icons.Rounded.Settings,
                                    contentDescription = null,
                                )
                            },
                            label = { Text(stringResource(R.string.example_tab_diagnostics)) },
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
                            ExamplePlayerStageWithTracks(
                                controller = controller,
                                uiState = displayedUiState,
                                controlsVisible = controlsVisible,
                                pendingSeekRatio = pendingSeekRatio,
                                isPortrait = false,
                                modifier = Modifier.fillMaxSize(),
                                pictureInPicturePresentation = pictureInPicturePresentation,
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

                        selectedTab == ExampleHostTab.Play -> {
                            ExamplePlayScreen(
                                palette = palette,
                                sourceLabel = displayedUiState.sourceLabel,
                                subtitle = displayedUiState.subtitle,
                                themeSelector = {
                                    ExampleThemeModeSelector(
                                        themeMode = themeMode,
                                        onThemeModeChange = { themeMode = it },
                                    )
                                },
                                playerStage = {
                                    ExamplePlayerStageWithTracks(
                                        controller = controller,
                                        uiState = displayedUiState,
                                        controlsVisible = controlsVisible,
                                        pendingSeekRatio = pendingSeekRatio,
                                        isPortrait = true,
                                        modifier = Modifier
                                            .fillMaxWidth()
                                            .height(248.dp),
                                        pictureInPicturePresentation = pictureInPicturePresentation,
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
                                },
                            ) {
                                    item {
                                        ExampleQuickSourcePanel(
                                            palette = palette,
                                            remoteStreamUrl = remoteStreamUrl,
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
                                    }

                                    item {
                                        ExampleExternalPlaybackSectionState(
                                            externalPlaybackController = externalPlaybackController,
                                            palette = palette,
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
                                    }

                                    item {
                                        ExamplePictureInPictureSection(
                                            palette = palette,
                                            enabled = pictureInPictureEnabled,
                                            onEnabledChange = { pictureInPictureEnabled = it },
                                            onRequestPictureInPicture =
                                                ::requestExamplePictureInPicture,
                                        )
                                    }

                                    item {
                                        ExampleQueuePanel(
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
                                    }

                            }
                        }

                        selectedTab == ExampleHostTab.Diagnostics -> {
                            ExampleDiagnosticsScreen {
                                item {
                                    ExampleDiagnosticsSummarySection(
                                        palette = palette,
                                        sourceLabel = displayedUiState.sourceLabel,
                                        sourceProtocol =
                                            controllerRebuildSource?.protocol?.name
                                                ?: stringResource(R.string.example_diagnostics_none),
                                        routeLabel =
                                            externalSession?.routeName
                                                ?: stringResource(R.string.example_diagnostics_none),
                                        playbackOrigin = playbackOrigin,
                                        sourceNormalizerSetting = sourceNormalizerSetting,
                                        nativeFramePipelineSetting = nativeFramePipelineSetting,
                                        videoSurfaceSetting = videoSurfaceSetting,
                                    )
                                }
                                item {
                                    ExampleEventLogSection(
                                        palette = palette,
                                        entries = hostLogEntries,
                                    )
                                }
                                item {
                                    ExampleFrameMetricsSection(
                                        palette = palette,
                                        enabled = frameMetricsEnabled,
                                        snapshot = frameMetricsSnapshot,
                                        onEnabledChange = { enabled ->
                                            frameMetricsEnabled = enabled
                                            if (!enabled) {
                                                frameMetricsSnapshot = null
                                            }
                                        },
                                    )
                                }
                                item {
                                    ExampleDolbyCatalogPanel(
                                        palette = palette,
                                        presets = exampleDolbyAcceptanceCatalog,
                                        selectedDrmKind = selectedDolbyDrmKind,
                                        selectedProfile = selectedDolbyProfile,
                                        selectedFps = selectedDolbyFps,
                                        onDrmKindChange = { drmKind ->
                                            selectedDolbyDrmKind = drmKind
                                        },
                                        onProfileChange = { profile ->
                                            selectedDolbyProfile = profile
                                        },
                                        onFpsChange = { fps ->
                                            selectedDolbyFps = fps
                                        },
                                        onPresetPlayNow = { preset ->
                                            activateDolbyAcceptancePreset(
                                                preset,
                                                ExamplePlaybackOrigin.DolbyAdHoc(preset.id),
                                            )
                                        },
                                        onPresetAddToQueue = ::addDolbyPresetToQueue,
                                    )
                                }
                                item {
                                    ExamplePluginDiagnosticsSection(
                                        palette = palette,
                                        sourceNormalizerSetting = sourceNormalizerSetting,
                                        nativeFramePipelineSetting = nativeFramePipelineSetting,
                                        videoSurfaceSetting = videoSurfaceSetting,
                                        sourceNormalizerPluginLibraryPaths = sourceNormalizerPluginLibraryPaths,
                                        decoderMediaCodecPluginLibraryPaths =
                                            decoderMediaCodecPluginLibraryPaths,
                                        frameProcessorPluginLibraryPaths = frameProcessorPluginLibraryPaths,
                                        pluginDiagnostics = controller.pluginDiagnostics,
                                        hdrEvidencePresets =
                                            exampleHdrEvidenceP0Presets +
                                                exampleDolbyAcceptanceHdrEvidencePresets(),
                                        selectedHdrEvidencePreset = selectedHdrEvidencePreset,
                                        isCapturingHdrEvidence = isCapturingHdrEvidence,
                                        hdrEvidenceActiveSourceAvailable = controllerRebuildSource != null,
                                        onSourceNormalizerSettingChange = ::applySourceNormalizerSetting,
                                        onNativeFramePipelineSettingChange = ::applyNativeFramePipelineSetting,
                                        onVideoSurfaceSettingChange = ::applyVideoSurfaceSetting,
                                        onHdrEvidencePresetChange = { preset ->
                                            selectedHdrEvidencePreset = preset
                                        },
                                        onCaptureHdrEvidence = ::captureHdrEvidence,
                                    )
                                }
                                item {
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
                                                        recordHostLog(
                                                            severity = ExampleHostLogSeverity.Error,
                                                            title = context.getString(R.string.example_log_resilience_failed),
                                                            detail = result.exceptionOrNull()?.localizedMessage,
                                                        )
                                                    } else {
                                                        recordHostLog(
                                                            severity = ExampleHostLogSeverity.Info,
                                                            title = context.getString(R.string.example_log_resilience_applied),
                                                            detail = context.getString(profile.titleRes),
                                                        )
                                                    }
                                                    isApplyingResilienceProfile = false
                                                }
                                            }
                                        },
                                    )
                                }
                            }
                        }

                        else -> {
                            ExampleDownloadsScreen {
                                item {
                                    ExampleDownloadHeader(
                                        palette = palette,
                                        isDownloadExportPluginInstalled = isDownloadExportPluginInstalled,
                                    )
                                }
                                item {
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
                                }
                                item {
                                    ExampleDownloadTasksSectionState(
                                        downloadManager = downloadManager,
                                        palette = palette,
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
                    }

                    activeSheet?.let { sheet ->
                        ExampleSelectionSheetWithTracks(
                            sheet = sheet,
                            controller = controller,
                            uiState = displayedUiState,
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
                                scope.launch {
                                    runCatching {
                                        controller.setSubtitleTrackSelection(selection)
                                    }.onSuccess {
                                        activeSheet = null
                                    }.onFailure { error ->
                                        recordHostLog(
                                            severity = ExampleHostLogSeverity.Error,
                                            title = context.getString(R.string.example_common_subtitles),
                                            detail = error.localizedMessage,
                                        )
                                    }
                                }
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
}

private fun Activity.supportsExamplePictureInPicture(): Boolean =
    Build.VERSION.SDK_INT >= Build.VERSION_CODES.O &&
        packageManager.hasSystemFeature(PackageManager.FEATURE_PICTURE_IN_PICTURE)

@Composable
private fun ExamplePlayScreen(
    palette: ExampleHostPalette,
    sourceLabel: String,
    subtitle: String,
    themeSelector: @Composable () -> Unit,
    playerStage: @Composable () -> Unit,
    content: LazyListScope.() -> Unit,
) {
    LazyColumn(
        modifier = Modifier.fillMaxSize(),
        contentPadding = PaddingValues(horizontal = 18.dp, vertical = 18.dp),
        verticalArrangement = Arrangement.spacedBy(18.dp),
    ) {
        item {
            ExamplePlayerHeader(
                sourceLabel = sourceLabel,
                subtitle = subtitle,
                palette = palette,
            )
        }
        item {
            themeSelector()
        }
        item {
            playerStage()
        }
        content()
    }
}

@Composable
private fun ExampleDiagnosticsScreen(
    content: LazyListScope.() -> Unit,
) {
    LazyColumn(
        modifier = Modifier.fillMaxSize(),
        contentPadding = PaddingValues(horizontal = 18.dp, vertical = 18.dp),
        verticalArrangement = Arrangement.spacedBy(18.dp),
        content = content,
    )
}

@Composable
private fun ExampleDownloadsScreen(
    content: LazyListScope.() -> Unit,
) {
    LazyColumn(
        modifier = Modifier.fillMaxSize(),
        contentPadding = PaddingValues(horizontal = 18.dp, vertical = 18.dp),
        verticalArrangement = Arrangement.spacedBy(18.dp),
        content = content,
    )
}

@Composable
private fun ExamplePlayerStageWithTracks(
    controller: VesperPlayerController,
    uiState: io.github.ikaros.vesper.player.android.PlayerHostUiState,
    controlsVisible: Boolean,
    pendingSeekRatio: Float?,
    isPortrait: Boolean,
    modifier: Modifier,
    pictureInPicturePresentation: Boolean,
    onControlsVisibilityChange: (Boolean) -> Unit,
    onPendingSeekRatioChange: (Float?) -> Unit,
    onOpenSheet: (ExamplePlayerSheet) -> Unit,
    onToggleFullscreen: () -> Unit,
    onTogglePlayback: () -> Unit,
    onSeekToRatio: (Float) -> Unit,
    onSeekToLiveEdge: () -> Unit,
    onSetPlaybackRate: (Float) -> Unit,
    playbackRateControlsEnabled: Boolean,
    currentBrightnessRatio: () -> Float? = { null },
    onSetBrightnessRatio: (Float) -> Float? = { null },
    currentVolumeRatio: () -> Float? = { null },
    onSetVolumeRatio: (Float) -> Float? = { null },
) {
    val trackCatalog by controller.trackCatalog.collectAsState()
    val trackSelection by controller.trackSelection.collectAsState()
    ExamplePlayerStage(
        controller = controller,
        uiState = uiState,
        controlsVisible = controlsVisible,
        pendingSeekRatio = pendingSeekRatio,
        isPortrait = isPortrait,
        trackCatalog = trackCatalog,
        trackSelection = trackSelection,
        modifier = modifier,
        pictureInPicturePresentation = pictureInPicturePresentation,
        onControlsVisibilityChange = onControlsVisibilityChange,
        onPendingSeekRatioChange = onPendingSeekRatioChange,
        onOpenSheet = onOpenSheet,
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
}

@Composable
private fun ExampleSelectionSheetWithTracks(
    sheet: ExamplePlayerSheet,
    controller: VesperPlayerController,
    uiState: io.github.ikaros.vesper.player.android.PlayerHostUiState,
    onDismiss: () -> Unit,
    playbackRateControlsEnabled: Boolean,
    onOpenSheet: (ExamplePlayerSheet) -> Unit,
    onSelectQuality: (io.github.ikaros.vesper.player.android.VesperAbrPolicy) -> Unit,
    onSelectAudio: (io.github.ikaros.vesper.player.android.VesperTrackSelection) -> Unit,
    onSelectSubtitle: (io.github.ikaros.vesper.player.android.VesperTrackSelection) -> Unit,
    onSelectSpeed: (Float) -> Unit,
) {
    val trackCatalog by controller.trackCatalog.collectAsState()
    val trackSelection by controller.trackSelection.collectAsState()
    ExampleSelectionSheet(
        sheet = sheet,
        uiState = uiState,
        trackCatalog = trackCatalog,
        trackSelection = trackSelection,
        onDismiss = onDismiss,
        playbackRateControlsEnabled = playbackRateControlsEnabled,
        onOpenSheet = onOpenSheet,
        onSelectQuality = onSelectQuality,
        onSelectAudio = onSelectAudio,
        onSelectSubtitle = onSelectSubtitle,
        onSelectSpeed = onSelectSpeed,
    )
}

@Composable
private fun ExampleExternalPlaybackSectionState(
    externalPlaybackController: VesperExternalPlaybackController,
    palette: ExampleHostPalette,
    session: ExampleExternalPlaybackSession?,
    isDiscovering: Boolean,
    isCastRoutePickerOpening: Boolean,
    castRoutePickerRequestId: Long,
    hasDlnaPermission: Boolean,
    onOpenCastRoutes: () -> Unit,
    onRequestDlnaPermission: () -> Unit,
    onStartDiscovery: () -> Unit,
    onStopDiscovery: () -> Unit,
    onConnectRoute: (VesperExternalPlaybackRoute) -> Unit,
    onLoadCurrent: () -> Unit,
    onDisconnect: () -> Unit,
) {
    val externalRoutes by externalPlaybackController.routes.collectAsState()
    ExampleExternalPlaybackSection(
        palette = palette,
        routes = externalRoutes,
        session = session,
        isDiscovering = isDiscovering,
        isCastRoutePickerOpening = isCastRoutePickerOpening,
        castRoutePickerRequestId = castRoutePickerRequestId,
        hasDlnaPermission = hasDlnaPermission,
        onOpenCastRoutes = onOpenCastRoutes,
        onRequestDlnaPermission = onRequestDlnaPermission,
        onStartDiscovery = onStartDiscovery,
        onStopDiscovery = onStopDiscovery,
        onConnectRoute = onConnectRoute,
        onLoadCurrent = onLoadCurrent,
        onDisconnect = onDisconnect,
    )
}

@Composable
private fun ExampleDownloadTasksSectionState(
    downloadManager: VesperDownloadManager,
    palette: ExampleHostPalette,
    pendingTasks: List<ExamplePendingDownloadTask>,
    isDownloadExportPluginInstalled: Boolean,
    savingTaskIds: Set<Long>,
    exportProgressByTaskId: Map<Long, Float>,
    onPrimaryAction: (VesperDownloadTaskSnapshot) -> Unit,
    onSaveToGallery: (VesperDownloadTaskSnapshot) -> Unit,
    onRemoveTask: (VesperDownloadTaskSnapshot) -> Unit,
) {
    val downloadSnapshot by downloadManager.snapshot.collectAsState()
    ExampleDownloadTasksSection(
        palette = palette,
        tasks = downloadSnapshot.tasks,
        pendingTasks = pendingTasks,
        isDownloadExportPluginInstalled = isDownloadExportPluginInstalled,
        savingTaskIds = savingTaskIds,
        exportProgressByTaskId = exportProgressByTaskId,
        onPrimaryAction = onPrimaryAction,
        onSaveToGallery = onSaveToGallery,
        onRemoveTask = onRemoveTask,
    )
}

private fun buildExamplePictureInPictureParams(autoEnter: Boolean): PictureInPictureParams {
    check(Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
        "Picture in Picture params require Android O or newer."
    }
    val builder =
        PictureInPictureParams.Builder()
            .setAspectRatio(Rational(16, 9))
    if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S) {
        builder.setAutoEnterEnabled(autoEnter)
    }
    return builder.build()
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
    Play,
    Diagnostics,
    Downloads,
}

private const val PLAYER_HOST_EXAMPLE_TAG = "VesperPlayerExample"
