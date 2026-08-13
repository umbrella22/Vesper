package io.github.umbrella22.vesper.example.androidcomposehost

import android.app.Application
import androidx.lifecycle.AndroidViewModel
import androidx.lifecycle.viewModelScope
import io.github.umbrella22.vesper.player.android.VesperBundledPluginReferences
import io.github.umbrella22.vesper.player.android.VesperDownloadConfiguration
import io.github.umbrella22.vesper.player.android.VesperDownloadManager
import io.github.umbrella22.vesper.player.android.VesperFrameProcessorConfiguration
import io.github.umbrella22.vesper.player.android.VesperFrameProcessorMode
import io.github.umbrella22.vesper.player.android.VesperPlaylistConfiguration
import io.github.umbrella22.vesper.player.android.VesperPlaylistCoordinator
import io.github.umbrella22.vesper.player.android.VesperPlaylistNeighborWindow
import io.github.umbrella22.vesper.player.android.VesperPlaylistPreloadWindow
import io.github.umbrella22.vesper.player.android.VesperNativeFramePipelineConfiguration
import io.github.umbrella22.vesper.player.android.VesperNativeFramePipelineMode
import io.github.umbrella22.vesper.player.android.VesperPlaybackResiliencePolicy
import io.github.umbrella22.vesper.player.android.VesperPlayerController
import io.github.umbrella22.vesper.player.android.VesperPlayerControllerFactory
import io.github.umbrella22.vesper.player.android.VesperPlayerSource
import io.github.umbrella22.vesper.player.android.VesperPluginReference
import io.github.umbrella22.vesper.player.android.VesperPreloadBudgetPolicy
import io.github.umbrella22.vesper.player.android.VesperSourceNormalizerConfiguration
import io.github.umbrella22.vesper.player.android.VesperSourceNormalizerMode
import io.github.umbrella22.vesper.player.android.external.VesperExternalPlaybackController
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.launch

internal class PlayerHostViewModel(
    private val application: Application,
) : AndroidViewModel(application) {
    private val playerPreloadBudgetPolicy =
        VesperPreloadBudgetPolicy(
            maxConcurrentTasks = 0,
            maxMemoryBytes = 0L,
            maxDiskBytes = 0L,
            warmupWindowMs = 0L,
        )

    private val preloadBudgetPolicy =
        VesperPreloadBudgetPolicy(
            maxConcurrentTasks = 2,
            maxMemoryBytes = 64L * 1024L * 1024L,
            maxDiskBytes = 256L * 1024L * 1024L,
            warmupWindowMs = 30_000L,
        )

    val sourceNormalizerPluginReferences: List<VesperPluginReference> =
        listOf(VesperBundledPluginReferences.sourceNormalizerFfmpeg)
    val decoderMediaCodecPluginReferences: List<VesperPluginReference> =
        listOf(VesperBundledPluginReferences.decoderMediaCodec)
    val frameProcessorPluginReferences: List<VesperPluginReference> =
        listOf(VesperBundledPluginReferences.frameProcessorDiagnostic)

    private val _controller =
        MutableStateFlow(
            createController(
                sourceNormalizerSetting = ExampleSourceNormalizerSetting.PreflightOnly,
                nativeFramePipelineSetting = ExampleNativeFramePipelineSetting.DiagnosticsOnly,
                videoSurfaceSetting = ExampleVideoSurfaceSetting.SurfaceView,
                initialSource = null,
                resiliencePolicy = ExampleResilienceProfile.Balanced.policy,
            ),
        )
    val controller: StateFlow<VesperPlayerController> = _controller.asStateFlow()

    val playlistCoordinator =
        VesperPlaylistCoordinator(
            context = application.applicationContext,
            configuration =
                VesperPlaylistConfiguration(
                    playlistId = "android-compose-host",
                    neighborWindow = VesperPlaylistNeighborWindow(previous = 1, next = 1),
                    preloadWindow = VesperPlaylistPreloadWindow(nearVisible = 1, prefetchOnly = 2),
                    switchPolicy = examplePlaylistSwitchPolicy(),
                ),
            preloadBudgetPolicy = preloadBudgetPolicy,
            resiliencePolicy = ExampleResilienceProfile.Balanced.policy,
        )

    private val downloadManagerSelection = createDownloadManagerSelection(application)
    val downloadManager = downloadManagerSelection.first
    val isDownloadExportPluginInstalled: Boolean = downloadManagerSelection.second

    val externalPlaybackController =
        VesperExternalPlaybackController(application.applicationContext)

    fun rebuildController(
        sourceNormalizerSetting: ExampleSourceNormalizerSetting,
        nativeFramePipelineSetting: ExampleNativeFramePipelineSetting,
        videoSurfaceSetting: ExampleVideoSurfaceSetting,
        initialSource: VesperPlayerSource?,
        resiliencePolicy: VesperPlaybackResiliencePolicy,
        shouldResumePlayback: Boolean,
        restorePositionMs: Long?,
        restorePlaybackRate: Float,
    ): VesperPlayerController {
        val previous = _controller.value
        val next =
            createController(
                sourceNormalizerSetting = sourceNormalizerSetting,
                nativeFramePipelineSetting = nativeFramePipelineSetting,
                videoSurfaceSetting = videoSurfaceSetting,
                initialSource = initialSource,
                resiliencePolicy = resiliencePolicy,
                onInitialized = { initialized ->
                    if (_controller.value !== initialized || initialSource == null) {
                        return@createController
                    }
                    restorePositionMs
                        ?.takeIf { position -> position > 0L }
                        ?.let { position ->
                            val currentPositionMs = initialized.uiState.value.timeline.positionMs
                            runCatching { initialized.seekBy(position - currentPositionMs) }
                        }
                    if (restorePlaybackRate != 1.0f) {
                        runCatching { initialized.setPlaybackRate(restorePlaybackRate) }
                    }
                    if (shouldResumePlayback) {
                        runCatching { initialized.play() }
                    }
                },
            )
        _controller.value = next
        runCatching { previous.dispose() }
        return next
    }

    override fun onCleared() {
        listOf(
            { externalPlaybackController.release() },
            { downloadManager.dispose() },
            { playlistCoordinator.dispose() },
            { _controller.value.dispose() },
        ).forEach { cleanup -> runCatching { cleanup() } }
    }

    private fun createController(
        sourceNormalizerSetting: ExampleSourceNormalizerSetting,
        nativeFramePipelineSetting: ExampleNativeFramePipelineSetting,
        videoSurfaceSetting: ExampleVideoSurfaceSetting,
        initialSource: VesperPlayerSource?,
        resiliencePolicy: VesperPlaybackResiliencePolicy,
        onInitialized: suspend (VesperPlayerController) -> Unit = {},
    ): VesperPlayerController =
        VesperPlayerControllerFactory.createDefault(
            context = application.applicationContext,
            initialSource = initialSource,
            resiliencePolicy = resiliencePolicy,
            surfaceKind = exampleSurfaceKindForSettings(
                setting = nativeFramePipelineSetting,
                surfaceSetting = videoSurfaceSetting,
                source = initialSource,
            ),
            preloadBudgetPolicy = playerPreloadBudgetPolicy,
            sourceNormalizerConfiguration =
                VesperSourceNormalizerConfiguration(
                    mode = sourceNormalizerSetting.mode,
                    pluginReferences =
                        sourceNormalizerPluginReferences.takeUnless {
                            sourceNormalizerSetting.mode == VesperSourceNormalizerMode.Disabled
                        }.orEmpty(),
                ),
            frameProcessorConfiguration =
                VesperFrameProcessorConfiguration(
                    mode =
                        if (frameProcessorPluginReferences.isEmpty()) {
                            VesperFrameProcessorMode.Disabled
                        } else {
                            VesperFrameProcessorMode.DiagnosticsOnly
                        },
                    pluginReferences = frameProcessorPluginReferences,
                ),
            nativeFramePipelineConfiguration =
                nativeFramePipelineConfiguration(nativeFramePipelineSetting),
        ).also { controller ->
            viewModelScope.launch {
                controller.initializeAsync()
                onInitialized(controller)
            }
        }

    private fun nativeFramePipelineConfiguration(
        setting: ExampleNativeFramePipelineSetting,
    ): VesperNativeFramePipelineConfiguration =
        when (setting.mode) {
            VesperNativeFramePipelineMode.Disabled -> VesperNativeFramePipelineConfiguration()
            VesperNativeFramePipelineMode.DiagnosticsOnly,
            VesperNativeFramePipelineMode.PreferNativeFrame,
            VesperNativeFramePipelineMode.RequireNativeFrame ->
                VesperNativeFramePipelineConfiguration(
                    mode = setting.mode,
                    decoderPluginReferences = decoderMediaCodecPluginReferences,
                    frameProcessorPluginReferences = frameProcessorPluginReferences,
                    maxInFlightFrames = 3,
                )
        }

    private fun createDownloadManagerSelection(
        application: Application,
    ): Pair<VesperDownloadManager, Boolean> =
        runCatching {
            VesperDownloadManager(
                context = application.applicationContext,
                configuration =
                    VesperDownloadConfiguration(
                        postDownloadPluginReferences =
                            listOf(VesperBundledPluginReferences.remuxFfmpeg),
                        runPostProcessorsOnCompletion = false,
                    ),
            ) to true
        }.getOrElse {
            VesperDownloadManager(
                context = application.applicationContext,
                configuration =
                    VesperDownloadConfiguration(
                        runPostProcessorsOnCompletion = false,
                    ),
            ) to false
        }
}
