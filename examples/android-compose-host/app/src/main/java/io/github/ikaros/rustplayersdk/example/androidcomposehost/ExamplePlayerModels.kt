package io.github.ikaros.vesper.example.androidcomposehost

import android.content.Context
import androidx.annotation.StringRes
import androidx.compose.ui.graphics.Color
import io.github.ikaros.vesper.player.android.PlaybackStateUi
import io.github.ikaros.vesper.player.android.PlayerHostUiState
import io.github.ikaros.vesper.player.android.VesperPlaylistFailureStrategy
import io.github.ikaros.vesper.player.android.VesperPlaylistItemPreloadProfile
import io.github.ikaros.vesper.player.android.VesperPlaylistQueueItem
import io.github.ikaros.vesper.player.android.VesperPlaylistRepeatMode
import io.github.ikaros.vesper.player.android.VesperPlaylistSwitchPolicy
import io.github.ikaros.vesper.player.android.VesperPlaylistViewportHint
import io.github.ikaros.vesper.player.android.VesperPlaylistViewportHintKind
import io.github.ikaros.vesper.player.android.VesperPlaybackResiliencePolicy
import io.github.ikaros.vesper.player.android.VesperNativeFramePipelineMode
import io.github.ikaros.vesper.player.android.VesperPlayerSource
import io.github.ikaros.vesper.player.android.VesperSourceNormalizerMode
import io.github.ikaros.vesper.player.android.VesperVideoSurfaceKind
import kotlin.math.abs

internal enum class ExamplePlayerSheet {
    Menu,
    Quality,
    Audio,
    Subtitle,
    Speed,
}

internal sealed interface ExamplePlaybackOrigin {
    data class Queue(val itemId: String) : ExamplePlaybackOrigin
    data class DolbyAdHoc(val presetId: String) : ExamplePlaybackOrigin
}

internal enum class ExampleHostLogSeverity {
    Info,
    Warning,
    Error,
}

internal data class ExampleHostLogEntry(
    val id: Long,
    val atMillis: Long,
    val severity: ExampleHostLogSeverity,
    val title: String,
    val detail: String? = null,
)

internal enum class ExampleThemeMode(
    @get:StringRes val titleRes: Int,
) {
    System(R.string.example_theme_system),
    Light(R.string.example_theme_light),
    Dark(R.string.example_theme_dark),
}

internal enum class ExampleResilienceProfile(
    @get:StringRes val titleRes: Int,
    @get:StringRes val subtitleRes: Int,
) {
    Balanced(
        R.string.example_resilience_balanced,
        R.string.example_resilience_balanced_subtitle,
    ),
    Streaming(
        R.string.example_resilience_streaming,
        R.string.example_resilience_streaming_subtitle,
    ),
    Resilient(
        R.string.example_resilience_resilient,
        R.string.example_resilience_resilient_subtitle,
    ),
    LowLatency(
        R.string.example_resilience_low_latency,
        R.string.example_resilience_low_latency_subtitle,
    ),
    ;

    val policy: VesperPlaybackResiliencePolicy
        get() =
            when (this) {
                Balanced -> VesperPlaybackResiliencePolicy.balanced()
                Streaming -> VesperPlaybackResiliencePolicy.streaming()
                Resilient -> VesperPlaybackResiliencePolicy.resilient()
                LowLatency -> VesperPlaybackResiliencePolicy.lowLatency()
            }
}

internal enum class ExampleSourceNormalizerSetting(
    @get:StringRes val titleRes: Int,
    @get:StringRes val subtitleRes: Int,
    val mode: VesperSourceNormalizerMode,
) {
    Disabled(
        R.string.example_plugins_source_normalizer_disabled,
        R.string.example_plugins_source_normalizer_disabled_subtitle,
        VesperSourceNormalizerMode.Disabled,
    ),
    DiagnosticsOnly(
        R.string.example_plugins_source_normalizer_diagnostics,
        R.string.example_plugins_source_normalizer_diagnostics_subtitle,
        VesperSourceNormalizerMode.DiagnosticsOnly,
    ),
    PreflightOnly(
        R.string.example_plugins_source_normalizer_preflight,
        R.string.example_plugins_source_normalizer_preflight_subtitle,
        VesperSourceNormalizerMode.PreflightOnly,
    ),
    PreferNormalized(
        R.string.example_plugins_source_normalizer_prefer,
        R.string.example_plugins_source_normalizer_prefer_subtitle,
        VesperSourceNormalizerMode.PreferNormalized,
    ),
    RequireNormalized(
        R.string.example_plugins_source_normalizer_require,
        R.string.example_plugins_source_normalizer_require_subtitle,
        VesperSourceNormalizerMode.RequireNormalized,
    ),
}

internal enum class ExampleNativeFramePipelineSetting(
    @get:StringRes val titleRes: Int,
    @get:StringRes val subtitleRes: Int,
    val mode: VesperNativeFramePipelineMode,
) {
    Disabled(
        R.string.example_plugins_native_frame_disabled,
        R.string.example_plugins_native_frame_disabled_subtitle,
        VesperNativeFramePipelineMode.Disabled,
    ),
    DiagnosticsOnly(
        R.string.example_plugins_native_frame_diagnostics,
        R.string.example_plugins_native_frame_diagnostics_subtitle,
        VesperNativeFramePipelineMode.DiagnosticsOnly,
    ),
    PreferNativeFrame(
        R.string.example_plugins_native_frame_prefer,
        R.string.example_plugins_native_frame_prefer_subtitle,
        VesperNativeFramePipelineMode.PreferNativeFrame,
    ),
    RequireNativeFrame(
        R.string.example_plugins_native_frame_require,
        R.string.example_plugins_native_frame_require_subtitle,
        VesperNativeFramePipelineMode.RequireNativeFrame,
    ),
}

internal enum class ExampleVideoSurfaceSetting(
    @get:StringRes val titleRes: Int,
    @get:StringRes val subtitleRes: Int,
) {
    Auto(
        R.string.example_plugins_video_surface_auto,
        R.string.example_plugins_video_surface_auto_subtitle,
    ),
    TextureView(
        R.string.example_plugins_video_surface_texture,
        R.string.example_plugins_video_surface_texture_subtitle,
    ),
    SurfaceView(
        R.string.example_plugins_video_surface_surface,
        R.string.example_plugins_video_surface_surface_subtitle,
    ),
}

internal fun exampleSurfaceKindForSettings(
    setting: ExampleNativeFramePipelineSetting,
    surfaceSetting: ExampleVideoSurfaceSetting,
    source: VesperPlayerSource? = null,
): VesperVideoSurfaceKind {
    val requiresSurfaceView =
        source?.drmConfiguration != null ||
            setting.mode == VesperNativeFramePipelineMode.PreferNativeFrame ||
            setting.mode == VesperNativeFramePipelineMode.RequireNativeFrame
    return when {
        requiresSurfaceView -> VesperVideoSurfaceKind.SurfaceView
        surfaceSetting == ExampleVideoSurfaceSetting.SurfaceView -> VesperVideoSurfaceKind.SurfaceView
        surfaceSetting == ExampleVideoSurfaceSetting.TextureView -> VesperVideoSurfaceKind.TextureView
        else ->
            when (setting.mode) {
                VesperNativeFramePipelineMode.PreferNativeFrame,
                VesperNativeFramePipelineMode.RequireNativeFrame -> VesperVideoSurfaceKind.SurfaceView
                VesperNativeFramePipelineMode.Disabled,
                VesperNativeFramePipelineMode.DiagnosticsOnly -> VesperVideoSurfaceKind.TextureView
            }
    }
}

internal fun exampleVideoSurfaceSettingRequiresControllerRebuild(
    previous: ExampleVideoSurfaceSetting,
    next: ExampleVideoSurfaceSetting,
): Boolean = previous != next

internal fun exampleSurfaceKindForNativeFrameSetting(
    setting: ExampleNativeFramePipelineSetting,
    source: VesperPlayerSource? = null,
): VesperVideoSurfaceKind =
    exampleSurfaceKindForSettings(
        setting = setting,
        surfaceSetting = ExampleVideoSurfaceSetting.Auto,
        source = source,
    )

internal fun exampleAutoSurfaceKindForNativeFrameSetting(
    setting: ExampleNativeFramePipelineSetting,
): VesperVideoSurfaceKind =
    when (setting.mode) {
            VesperNativeFramePipelineMode.PreferNativeFrame,
            VesperNativeFramePipelineMode.RequireNativeFrame -> VesperVideoSurfaceKind.SurfaceView
            VesperNativeFramePipelineMode.Disabled,
            VesperNativeFramePipelineMode.DiagnosticsOnly -> VesperVideoSurfaceKind.TextureView
    }

internal fun exampleNativeFrameSettingRequiresControllerRebuild(
    previous: ExampleNativeFramePipelineSetting,
    next: ExampleNativeFramePipelineSetting,
): Boolean {
    if (previous == next) {
        return false
    }
    val previousUsesNativeFrame =
        previous.mode == VesperNativeFramePipelineMode.PreferNativeFrame ||
            previous.mode == VesperNativeFramePipelineMode.RequireNativeFrame
    val nextUsesNativeFrame =
        next.mode == VesperNativeFramePipelineMode.PreferNativeFrame ||
            next.mode == VesperNativeFramePipelineMode.RequireNativeFrame
    return previousUsesNativeFrame || nextUsesNativeFrame
}

internal data class ExampleControllerRebuildSnapshot(
    val shouldResumePlayback: Boolean,
    val restorePositionMs: Long,
    val restorePlaybackRate: Float,
)

internal fun exampleControllerRebuildSnapshot(
    uiState: PlayerHostUiState,
): ExampleControllerRebuildSnapshot =
    ExampleControllerRebuildSnapshot(
        shouldResumePlayback = uiState.playbackState == PlaybackStateUi.Playing,
        restorePositionMs = uiState.timeline.positionMs,
        restorePlaybackRate = uiState.playbackRate,
    )

internal fun exampleControllerRebuildSource(
    activePlaybackSource: VesperPlayerSource?,
    activePlaylistSource: VesperPlayerSource?,
): VesperPlayerSource? = activePlaybackSource ?: activePlaylistSource

internal data class ExampleHostPalette(
    val pageTop: Color,
    val pageBottom: Color,
    val sectionBackground: Color,
    val sectionStroke: Color,
    val title: Color,
    val body: Color,
    val fieldBackground: Color,
    val fieldText: Color,
    val primaryAction: Color,
)

internal fun exampleHostPalette(useDarkTheme: Boolean): ExampleHostPalette =
    if (useDarkTheme) {
        ExampleHostPalette(
            pageTop = Color(0xFF0C1018),
            pageBottom = Color(0xFF06080D),
            sectionBackground = Color.White.copy(alpha = 0.04f),
            sectionStroke = Color.White.copy(alpha = 0.06f),
            title = Color.White,
            body = Color(0xFF94A0B5),
            fieldBackground = Color.White.copy(alpha = 0.06f),
            fieldText = Color.White,
            primaryAction = Color(0xFF2A8BFF),
        )
    } else {
        ExampleHostPalette(
            pageTop = Color(0xFFF8F2EA),
            pageBottom = Color(0xFFF2F4F9),
            sectionBackground = Color.White.copy(alpha = 0.86f),
            sectionStroke = Color(0x140B1220),
            title = Color(0xFF101521),
            body = Color(0xFF5C667A),
            fieldBackground = Color(0xFFF6F7FA),
            fieldText = Color(0xFF101521),
            primaryAction = Color(0xFF256DFF),
        )
    }

internal const val ANDROID_HLS_DEMO_URL: String =
    "https://devstreaming-cdn.apple.com/videos/streaming/examples/img_bipbop_adv_example_ts/master.m3u8"

internal const val ANDROID_DASH_DEMO_URL: String =
    "https://dash.akamaized.net/envivio/EnvivioDash3/manifest.mpd"

internal const val ANDROID_LIVE_DVR_ACCEPTANCE_URL: String =
    "https://demo.unified-streaming.com/k8s/live/scte35.isml/.m3u8"

internal const val ANDROID_HLS_PLAYLIST_ITEM_ID: String = "hls-demo"
internal const val ANDROID_DASH_PLAYLIST_ITEM_ID: String = "dash-demo"
internal const val ANDROID_LIVE_DVR_PLAYLIST_ITEM_ID: String = "live-dvr-acceptance"
internal const val ANDROID_REMOTE_PLAYLIST_ITEM_ID: String = "custom-remote"
internal const val ANDROID_LOCAL_PLAYLIST_ITEM_ID: String = "local-file"

private const val DOLBY_PLAYLIST_ITEM_PREFIX: String = "dolby-"
internal const val EXAMPLE_HOST_LOG_CAPACITY: Int = 80

internal fun dolbyPlaylistItemId(presetId: String): String = "$DOLBY_PLAYLIST_ITEM_PREFIX$presetId"

internal fun dolbyPresetIdFromPlaylistItemId(itemId: String): String? =
    itemId.removePrefix(DOLBY_PLAYLIST_ITEM_PREFIX)
        .takeIf { presetId -> itemId != presetId && presetId.isNotBlank() }

internal fun canQueueDolbyPreset(preset: ExampleDolbyAcceptancePreset): Boolean = preset.isPlayable

internal fun appendExampleHostLogEntry(
    entries: List<ExampleHostLogEntry>,
    entry: ExampleHostLogEntry,
    capacity: Int = EXAMPLE_HOST_LOG_CAPACITY,
): List<ExampleHostLogEntry> {
    if (capacity <= 0) {
        return emptyList()
    }
    return (listOf(entry) + entries).take(capacity)
}

internal fun shouldAdvancePlaylistOnFinished(
    origin: ExamplePlaybackOrigin?,
    activeItemId: String?,
): Boolean =
    origin is ExamplePlaybackOrigin.Queue &&
        activeItemId != null &&
        origin.itemId == activeItemId

internal fun androidHlsDemoSource(context: Context? = null): VesperPlayerSource =
    VesperPlayerSource.hls(
        uri = ANDROID_HLS_DEMO_URL,
        label = context?.getString(R.string.example_source_hls_demo_label) ?: "HLS Demo (BipBop)",
    )

internal fun androidDashDemoSource(context: Context? = null): VesperPlayerSource =
    VesperPlayerSource.dash(
        uri = ANDROID_DASH_DEMO_URL,
        label = context?.getString(R.string.example_source_dash_demo_label) ?: "DASH Demo (Envivio)",
    )

internal fun androidLiveDvrAcceptanceSource(context: Context? = null): VesperPlayerSource =
    VesperPlayerSource.hls(
        uri = ANDROID_LIVE_DVR_ACCEPTANCE_URL,
        label = context?.getString(R.string.example_source_live_dvr_acceptance_label)
            ?: "Live DVR Acceptance (Unified SCTE-35)",
    )

internal fun examplePlaylistQueue(
    context: Context,
    playlistItemIds: List<String>,
    remoteSource: VesperPlayerSource? = null,
    localSource: VesperPlayerSource? = null,
): List<VesperPlaylistQueueItem> =
    buildList {
        playlistItemIds.forEach { itemId ->
            when (itemId) {
                ANDROID_HLS_PLAYLIST_ITEM_ID ->
                    add(
                        VesperPlaylistQueueItem(
                            itemId = ANDROID_HLS_PLAYLIST_ITEM_ID,
                            source = androidHlsDemoSource(context),
                            preloadProfile =
                                VesperPlaylistItemPreloadProfile(
                                    expectedMemoryBytes = 256 * 1024L,
                                    expectedDiskBytes = 512 * 1024L,
                                    warmupWindowMs = 30_000L,
                                ),
                        ),
                    )

                ANDROID_DASH_PLAYLIST_ITEM_ID ->
                    add(
                        VesperPlaylistQueueItem(
                            itemId = ANDROID_DASH_PLAYLIST_ITEM_ID,
                            source = androidDashDemoSource(context),
                            preloadProfile =
                                VesperPlaylistItemPreloadProfile(
                                    expectedMemoryBytes = 256 * 1024L,
                                    expectedDiskBytes = 512 * 1024L,
                                    warmupWindowMs = 30_000L,
                                ),
                        ),
                    )

                ANDROID_LIVE_DVR_PLAYLIST_ITEM_ID ->
                    add(
                        VesperPlaylistQueueItem(
                            itemId = ANDROID_LIVE_DVR_PLAYLIST_ITEM_ID,
                            source = androidLiveDvrAcceptanceSource(context),
                            preloadProfile =
                                VesperPlaylistItemPreloadProfile(
                                    expectedMemoryBytes = 256 * 1024L,
                                    expectedDiskBytes = 512 * 1024L,
                                    warmupWindowMs = 15_000L,
                                ),
                        ),
                    )

                ANDROID_LOCAL_PLAYLIST_ITEM_ID ->
                    localSource?.let { source ->
                        add(
                            VesperPlaylistQueueItem(
                                itemId = ANDROID_LOCAL_PLAYLIST_ITEM_ID,
                                source = source,
                                preloadProfile =
                                    VesperPlaylistItemPreloadProfile(expectedMemoryBytes = 128 * 1024L),
                            ),
                        )
                    }

                ANDROID_REMOTE_PLAYLIST_ITEM_ID ->
                    remoteSource?.let { source ->
                        add(
                            VesperPlaylistQueueItem(
                                itemId = ANDROID_REMOTE_PLAYLIST_ITEM_ID,
                                source = source,
                                preloadProfile =
                                    VesperPlaylistItemPreloadProfile(
                                        expectedMemoryBytes = 256 * 1024L,
                                        expectedDiskBytes = 512 * 1024L,
                                        warmupWindowMs = 30_000L,
                                    ),
                            ),
                        )
                    }

                else ->
                    dolbyPresetIdFromPlaylistItemId(itemId)
                        ?.let(::exampleDolbyAcceptancePresetById)
                        ?.takeIf(::canQueueDolbyPreset)
                        ?.let { preset ->
                            add(
                                VesperPlaylistQueueItem(
                                    itemId = itemId,
                                    source = preset.source,
                                    preloadProfile =
                                        VesperPlaylistItemPreloadProfile(
                                            expectedMemoryBytes = 256 * 1024L,
                                            expectedDiskBytes = 512 * 1024L,
                                            warmupWindowMs = 30_000L,
                                        ),
                                ),
                            )
                        }
            }
        }
    }

internal fun enqueuePlaylistItem(
    playlistItemIds: List<String>,
    itemId: String,
): List<String> =
    buildList {
        addAll(playlistItemIds.filterNot { existingItemId -> existingItemId == itemId })
        add(itemId)
    }

internal fun examplePlaylistViewportHints(
    queue: List<VesperPlaylistQueueItem>,
    focusedItemId: String?,
): List<VesperPlaylistViewportHint> {
    if (queue.isEmpty()) {
        return emptyList()
    }

    val focusIndex =
        focusedItemId
            ?.let { itemId -> queue.indexOfFirst { it.itemId == itemId } }
            ?.takeIf { it >= 0 } ?: 0

    return buildList {
        add(
            VesperPlaylistViewportHint(
                itemId = queue[focusIndex].itemId,
                kind = VesperPlaylistViewportHintKind.Visible,
                order = 0,
            ),
        )

        queue.indices
            .filter { it != focusIndex }
            .sortedWith(compareBy<Int> { abs(it - focusIndex) }.thenBy { it })
            .forEachIndexed { order, index ->
                val distance = abs(index - focusIndex)
                add(
                    VesperPlaylistViewportHint(
                        itemId = queue[index].itemId,
                        kind =
                            if (distance == 1) {
                                VesperPlaylistViewportHintKind.NearVisible
                            } else {
                                VesperPlaylistViewportHintKind.PrefetchOnly
                            },
                        order = order + 1,
                    ),
                )
            }
    }
}

internal fun examplePlaylistSwitchPolicy(): VesperPlaylistSwitchPolicy =
    VesperPlaylistSwitchPolicy(
        autoAdvance = true,
        repeatMode = VesperPlaylistRepeatMode.Off,
        failureStrategy = VesperPlaylistFailureStrategy.SkipToNext,
    )
