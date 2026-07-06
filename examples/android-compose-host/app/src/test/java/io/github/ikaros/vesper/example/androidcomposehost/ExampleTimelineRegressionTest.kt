package io.github.ikaros.vesper.example.androidcomposehost

import io.github.ikaros.vesper.player.android.SeekableRangeUi
import io.github.ikaros.vesper.player.android.PlaybackStateUi
import io.github.ikaros.vesper.player.android.PlayerHostUiState
import io.github.ikaros.vesper.player.android.TimelineKind
import io.github.ikaros.vesper.player.android.TimelineUiState
import io.github.ikaros.vesper.player.android.VesperPlaylistQueueItem
import io.github.ikaros.vesper.player.android.VesperPlaylistQueueItemState
import io.github.ikaros.vesper.player.android.VesperPlaylistViewportHintKind
import io.github.ikaros.vesper.player.android.VesperPlayerSource
import io.github.ikaros.vesper.player.android.VesperPlayerSourceProtocol
import io.github.ikaros.vesper.player.android.VesperVideoSurfaceKind
import io.github.ikaros.vesper.player.android.external.VesperExternalPlaybackRoute
import io.github.ikaros.vesper.player.android.external.VesperExternalPlaybackRouteKind
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Test

class ExampleTimelineRegressionTest {
    @Test
    fun `live dvr acceptance source is hls and queueable`() {
        val source = androidLiveDvrAcceptanceSource(context = null)

        assertEquals(ANDROID_LIVE_DVR_ACCEPTANCE_URL, source.uri)
        assertEquals(VesperPlayerSourceProtocol.Hls, source.protocol)
    }

    @Test
    fun `dolby acceptance urls follow browser test kit patterns`() {
        assertEquals(
            "https://ott.dolby.com/browser_test_kit/clear/p5/24/dash.mpd",
            exampleDolbyAcceptanceUrl(
                profile = ExampleDolbyAcceptanceProfile.P5,
                fps = 24,
                protocol = VesperPlayerSourceProtocol.Dash,
                drmKind = ExampleDolbyAcceptanceDrmKind.Clear,
            ),
        )
        assertEquals(
            "https://ott.dolby.com/browser_test_kit/clear/p81/50/master.m3u8",
            exampleDolbyAcceptanceUrl(
                profile = ExampleDolbyAcceptanceProfile.P81,
                fps = 50,
                protocol = VesperPlayerSourceProtocol.Hls,
                drmKind = ExampleDolbyAcceptanceDrmKind.Clear,
            ),
        )
        assertEquals(
            "https://ott.dolby.com/browser_test_kit/cenc/p84/120/dash.mpd",
            exampleDolbyAcceptanceUrl(
                profile = ExampleDolbyAcceptanceProfile.P84,
                fps = 120,
                protocol = VesperPlayerSourceProtocol.Dash,
                drmKind = ExampleDolbyAcceptanceDrmKind.Widevine,
            ),
        )
    }

    @Test
    fun `dolby widevine presets carry drm configuration for direct dash`() {
        val preset =
            exampleDolbyAcceptanceCatalog.first {
                it.profile == ExampleDolbyAcceptanceProfile.P5 &&
                    it.fps == 24 &&
                    it.protocol == VesperPlayerSourceProtocol.Dash &&
                    it.drmKind == ExampleDolbyAcceptanceDrmKind.Widevine
            }

        assertEquals(true, preset.isPlayable)
        assertEquals("widevine", preset.source.drmConfiguration?.keySystem)
        assertEquals(
            EXAMPLE_DOLBY_ACCEPTANCE_WIDEVINE_LICENSE_URI,
            preset.source.drmConfiguration?.licenseUri,
        )
        assertEquals(VesperPlayerSourceProtocol.Dash, preset.source.protocol)
    }

    @Test
    fun `dolby widevine presets use surface view even when plugin route is disabled`() {
        val preset =
            exampleDolbyAcceptanceCatalog.first {
                it.profile == ExampleDolbyAcceptanceProfile.P81 &&
                    it.fps == 24 &&
                    it.protocol == VesperPlayerSourceProtocol.Dash &&
                    it.drmKind == ExampleDolbyAcceptanceDrmKind.Widevine
            }

        assertEquals(
            VesperVideoSurfaceKind.SurfaceView,
            exampleSurfaceKindForNativeFrameSetting(
                ExampleNativeFramePipelineSetting.Disabled,
                preset.source,
            ),
        )
    }

    @Test
    fun `dolby fairplay presets remain pending and disabled`() {
        val preset =
            exampleDolbyAcceptanceCatalog.first {
                it.profile == ExampleDolbyAcceptanceProfile.P81 &&
                    it.fps == 30 &&
                    it.protocol == VesperPlayerSourceProtocol.Hls &&
                    it.drmKind == ExampleDolbyAcceptanceDrmKind.FairPlayPending
            }

        assertFalse(preset.enabled)
        assertFalse(preset.isPlayable)
        assertEquals(null, preset.source.drmConfiguration)
        assertEquals(
            "https://ott.dolby.com/browser_test_kit/cbcs/p81/30/master.m3u8",
            preset.source.uri,
        )
    }

    @Test
    fun `dolby catalog filter keeps drm profile and fps boundaries`() {
        val filtered =
            filterDolbyAcceptancePresets(
                presets = exampleDolbyAcceptanceCatalog,
                drmKind = ExampleDolbyAcceptanceDrmKind.Clear,
                profile = ExampleDolbyAcceptanceProfile.P81,
                fps = 50,
            )

        assertEquals(2, filtered.size)
        filtered.forEach { preset ->
            assertEquals(ExampleDolbyAcceptanceDrmKind.Clear, preset.drmKind)
            assertEquals(ExampleDolbyAcceptanceProfile.P81, preset.profile)
            assertEquals(50, preset.fps)
        }
    }

    @Test
    fun `fairplay pending presets cannot be queued`() {
        val preset =
            exampleDolbyAcceptanceCatalog.first {
                it.drmKind == ExampleDolbyAcceptanceDrmKind.FairPlayPending
            }

        assertFalse(canQueueDolbyPreset(preset))
    }

    @Test
    fun `dolby playlist item id resolves to queue item`() {
        val preset =
            exampleDolbyAcceptanceCatalog.first {
                it.profile == ExampleDolbyAcceptanceProfile.P5 &&
                    it.fps == 24 &&
                    it.protocol == VesperPlayerSourceProtocol.Dash &&
                    it.drmKind == ExampleDolbyAcceptanceDrmKind.Clear
            }
        val itemId = dolbyPlaylistItemId(preset.id)

        assertEquals(preset.id, dolbyPresetIdFromPlaylistItemId(itemId))
        val queue =
            examplePlaylistQueue(
                context = android.content.ContextWrapper(null),
                playlistItemIds = listOf(itemId),
            )

        assertEquals(1, queue.size)
        assertEquals(itemId, queue.single().itemId)
        assertEquals(preset.source.uri, queue.single().source.uri)
    }

    @Test
    fun `ad hoc dolby playback does not advance playlist on finished`() {
        assertFalse(
            shouldAdvancePlaylistOnFinished(
                origin = ExamplePlaybackOrigin.DolbyAdHoc("DOLBY-DV-P5-24-DASH-CLEAR"),
                activeItemId = ANDROID_HLS_PLAYLIST_ITEM_ID,
            ),
        )
        assertEquals(
            true,
            shouldAdvancePlaylistOnFinished(
                origin = ExamplePlaybackOrigin.Queue(ANDROID_HLS_PLAYLIST_ITEM_ID),
                activeItemId = ANDROID_HLS_PLAYLIST_ITEM_ID,
            ),
        )
    }

    @Test
    fun `host event log keeps newest entries within capacity`() {
        val entries =
            (1L..85L).fold(emptyList<ExampleHostLogEntry>()) { current, id ->
                appendExampleHostLogEntry(
                    entries = current,
                    entry =
                        ExampleHostLogEntry(
                            id = id,
                            atMillis = id,
                            severity = ExampleHostLogSeverity.Info,
                            title = "event-$id",
                        ),
                )
            }

        assertEquals(EXAMPLE_HOST_LOG_CAPACITY, entries.size)
        assertEquals(85L, entries.first().id)
        assertEquals(6L, entries.last().id)
    }

    @Test
    fun `compact queue keeps active item and nearest neighbors`() {
        val source = androidHlsDemoSource(context = null)
        val queue =
            (0 until 8).map { index ->
                VesperPlaylistQueueItemState(
                    item = VesperPlaylistQueueItem(itemId = "item-$index", source = source),
                    index = index,
                    viewportHint = VesperPlaylistViewportHintKind.Hidden,
                    isActive = index == 4,
                )
            }

        val compact = compactPlaylistQueueItems(queue, maxVisibleItems = 5)

        assertEquals(
            listOf("item-2", "item-3", "item-4", "item-5", "item-6"),
            compact.map { it.item.itemId },
        )
    }

    @Test
    fun `dolby hdr evidence presets preserve profile fps protocol and drm metadata`() {
        val preset =
            exampleDolbyAcceptanceCatalog.first {
                it.profile == ExampleDolbyAcceptanceProfile.P84 &&
                    it.fps == 50 &&
                    it.protocol == VesperPlayerSourceProtocol.Hls &&
                    it.drmKind == ExampleDolbyAcceptanceDrmKind.Clear
            }.toHdrEvidencePreset()

        assertEquals("dolbyVision", preset.sourceMetadata["hdrKind"])
        assertEquals("hls", preset.sourceMetadata["manifestKind"])
        assertEquals(50.0, preset.sourceMetadata["frameRate"])
        assertEquals("none", preset.sourceMetadata["drmKind"])
        assertEquals("requiresDolbyVisionDisplay", preset.sourceMetadata["manualGate"])
        assertEquals(
            "profile8.4",
            (preset.sourceMetadata["dolbyVision"] as Map<*, *>)["profileFamily"],
        )
    }

    @Test
    fun `go live falls back to seekable end for live dvr`() {
        val timeline =
            TimelineUiState(
                kind = TimelineKind.LiveDvr,
                isSeekable = true,
                seekableRange = SeekableRangeUi(startMs = 10_000L, endMs = 60_000L),
                liveEdgeMs = null,
                positionMs = 55_000L,
                durationMs = 60_000L,
            )

        assertEquals(ExampleLiveButtonState.LiveBehind(5_000L), liveButtonState(timeline))
        assertEquals(
            ExampleTimelineSummaryState.Window(positionMs = 45_000L, endMs = 50_000L),
            timelineSummaryState(timeline, pendingSeekRatio = null),
        )
    }

    @Test
    fun `live edge tolerance keeps live badge active`() {
        val timeline =
            TimelineUiState(
                kind = TimelineKind.Live,
                isSeekable = false,
                seekableRange = null,
                liveEdgeMs = 120_000L,
                positionMs = 119_100L,
                durationMs = null,
            )

        assertEquals(ExampleLiveButtonState.Live, liveButtonState(timeline))
        assertEquals(
            ExampleTimelineSummaryState.LiveEdge(liveEdgeMs = 120_000L),
            timelineSummaryState(timeline, pendingSeekRatio = null),
        )
    }

    @Test
    fun `pending ratio is clamped to seekable range`() {
        val timeline =
            TimelineUiState(
                kind = TimelineKind.LiveDvr,
                isSeekable = true,
                seekableRange = SeekableRangeUi(startMs = 30_000L, endMs = 90_000L),
                liveEdgeMs = 90_000L,
                positionMs = 48_000L,
                durationMs = 90_000L,
            )

        assertEquals(90_000L, displayedTimelinePositionMs(timeline, pendingSeekRatio = 1.4f))
        assertEquals(
            ExampleTimelineSummaryState.Window(positionMs = 60_000L, endMs = 60_000L),
            timelineSummaryState(timeline, pendingSeekRatio = 1.4f),
        )
    }

    @Test
    fun `window shrink clamps stale position before rendering`() {
        val timeline =
            TimelineUiState(
                kind = TimelineKind.LiveDvr,
                isSeekable = true,
                seekableRange = SeekableRangeUi(startMs = 40_000L, endMs = 70_000L),
                liveEdgeMs = null,
                positionMs = 82_000L,
                durationMs = 120_000L,
            )

        assertEquals(70_000L, displayedTimelinePositionMs(timeline, pendingSeekRatio = null))
        assertEquals(ExampleLiveButtonState.Live, liveButtonState(timeline))
        assertEquals(
            ExampleTimelineSummaryState.Window(positionMs = 30_000L, endMs = 30_000L),
            timelineSummaryState(timeline, pendingSeekRatio = null),
        )
    }

    @Test
    fun `external route label includes kind and device metadata`() {
        val route =
            VesperExternalPlaybackRoute(
                routeId = "dlna:living-room",
                name = "Living Room",
                kind = VesperExternalPlaybackRouteKind.Dlna,
                manufacturer = "Acme",
                modelName = "Panel",
            )

        assertEquals("Living Room · DLNA · Acme Panel", exampleExternalRouteLabel(route))
    }

    @Test
    fun `external optimistic position advances only while playing and clamps to duration`() {
        val source = VesperPlayerSource.hls(ANDROID_HLS_DEMO_URL, "HLS")
        val session =
            ExampleExternalPlaybackSession(
                routeId = "cast:active",
                routeName = "Cast device",
                routeKind = VesperExternalPlaybackRouteKind.Cast,
                status = ExampleExternalPlaybackStatus.Playing,
                source = source,
                basePositionMs = 9_000L,
                durationMs = 10_000L,
                seekableRange = null,
                startedAtMillis = 1_000L,
            )

        assertEquals(10_000L, exampleEstimatedExternalPositionMs(session, nowMillis = 4_500L))
        assertEquals(
            9_000L,
            exampleEstimatedExternalPositionMs(
                session.copy(status = ExampleExternalPlaybackStatus.Paused),
                nowMillis = 4_500L,
            ),
        )
    }

    @Test
    fun `external disconnect returns latest estimated remote position`() {
        val session =
            ExampleExternalPlaybackSession(
                routeId = "dlna:tv",
                routeName = "TV",
                routeKind = VesperExternalPlaybackRouteKind.Dlna,
                status = ExampleExternalPlaybackStatus.Playing,
                source = androidHlsDemoSource(context = null),
                basePositionMs = 12_000L,
                durationMs = 60_000L,
                seekableRange = null,
                startedAtMillis = 2_000L,
            )

        assertEquals(17_000L, exampleDisconnectLocalPositionMs(session, nowMillis = 7_000L))
    }

    @Test
    fun `external timeline uses seekable range for clamped live dvr progress`() {
        val timeline =
            TimelineUiState(
                kind = TimelineKind.LiveDvr,
                isSeekable = true,
                seekableRange = SeekableRangeUi(startMs = 40_000L, endMs = 70_000L),
                liveEdgeMs = 70_000L,
                positionMs = 45_000L,
                durationMs = 120_000L,
            )
        val session =
            ExampleExternalPlaybackSession(
                routeId = "dlna:tv",
                routeName = "TV",
                routeKind = VesperExternalPlaybackRouteKind.Dlna,
                status = ExampleExternalPlaybackStatus.Playing,
                source = androidLiveDvrAcceptanceSource(context = null),
                basePositionMs = 68_000L,
                durationMs = 120_000L,
                seekableRange = 40_000L to 70_000L,
                startedAtMillis = 1_000L,
            )

        assertEquals(70_000L, exampleExternalTimeline(timeline, session, nowMillis = 6_000L).positionMs)
        assertEquals(55_000L, exampleExternalPositionForRatio(timeline, ratio = 0.5f))
    }

    @Test
    fun `plugin diagnostics split out native frame pipeline route`() {
        val diagnostics =
            listOf(
                mapOf(
                    "pluginKind" to "source_normalizer",
                    "status" to "sourceNormalizerSupported",
                    "route" to "normalizedPlayback",
                ),
                mapOf(
                    "pluginKind" to "frame_processor",
                    "status" to "frameProcessorSupported",
                    "route" to "systemPlayer",
                ),
                mapOf(
                    "pluginKind" to "native_frame_pipeline",
                    "status" to "loaded",
                    "participation" to "fallback",
                    "route" to "systemPlayer",
                    "lifecycle" to "fallback",
                    "presenterState" to "waitingForSurface",
                    "surfaceProfile" to "SurfaceView",
                    "processedFrames" to 2L,
                    "presentedFrames" to 1L,
                    "deadlineMisses" to 0L,
                    "backpressureCount" to 3L,
                    "fallbackTargetRoute" to "systemPlayer",
                    "fallbackReason" to
                        "Android native-frame pipeline requires a MediaCodec decoder plugin path.",
                ),
            )

        val nativeFrameDiagnostics = exampleNativeFramePipelineDiagnostics(diagnostics)

        assertEquals(1, nativeFrameDiagnostics.size)
        assertEquals("systemPlayer", nativeFrameDiagnostics.single()["route"])
        assertEquals("fallback", nativeFrameDiagnostics.single()["participation"])
        assertEquals("systemPlayer", nativeFrameDiagnostics.single()["fallbackTargetRoute"])
        assertEquals(
            "lifecycle=fallback · presenter=waitingForSurface · surface=SurfaceView · " +
                "processed=2 · presented=1 · deadlineMisses=0 · backpressure=3",
            exampleNativeFramePipelineStatusLine(nativeFrameDiagnostics.single()),
        )
        assertFalse(
            exampleSourceNormalizerDiagnostics(diagnostics)
                .any { diagnostic -> diagnostic["pluginKind"] == "native_frame_pipeline" },
        )
        assertFalse(
            exampleFrameProcessorDiagnostics(diagnostics)
                .any { diagnostic -> diagnostic["pluginKind"] == "native_frame_pipeline" },
        )
    }

    @Test
    fun `prefer native frame selects surface view while diagnostics keep texture view`() {
        assertEquals(
            VesperVideoSurfaceKind.TextureView,
            exampleSurfaceKindForNativeFrameSetting(ExampleNativeFramePipelineSetting.Disabled),
        )
        assertEquals(
            VesperVideoSurfaceKind.TextureView,
            exampleSurfaceKindForNativeFrameSetting(ExampleNativeFramePipelineSetting.DiagnosticsOnly),
        )
        assertEquals(
            VesperVideoSurfaceKind.SurfaceView,
            exampleSurfaceKindForNativeFrameSetting(ExampleNativeFramePipelineSetting.PreferNativeFrame),
        )
        assertEquals(
            VesperVideoSurfaceKind.SurfaceView,
            exampleSurfaceKindForNativeFrameSetting(ExampleNativeFramePipelineSetting.RequireNativeFrame),
        )
    }

    @Test
    fun `diagnostics native frame switch does not require controller rebuild`() {
        assertFalse(
            exampleNativeFrameSettingRequiresControllerRebuild(
                ExampleNativeFramePipelineSetting.Disabled,
                ExampleNativeFramePipelineSetting.DiagnosticsOnly,
            )
        )
        assertFalse(
            exampleNativeFrameSettingRequiresControllerRebuild(
                ExampleNativeFramePipelineSetting.DiagnosticsOnly,
                ExampleNativeFramePipelineSetting.Disabled,
            )
        )
        assertEquals(
            true,
            exampleNativeFrameSettingRequiresControllerRebuild(
                ExampleNativeFramePipelineSetting.DiagnosticsOnly,
                ExampleNativeFramePipelineSetting.PreferNativeFrame,
            ),
        )
        assertEquals(
            true,
            exampleNativeFrameSettingRequiresControllerRebuild(
                ExampleNativeFramePipelineSetting.RequireNativeFrame,
                ExampleNativeFramePipelineSetting.DiagnosticsOnly,
            ),
        )
    }

    @Test
    fun `plugin diagnostics compact long messages and paths`() {
        assertEquals(
            "line one line two",
            exampleDiagnosticCompactMessage("line one\n   line two"),
        )
        assertEquals(
            220,
            exampleDiagnosticCompactMessage("x".repeat(500)).length,
        )
        val pathSeparator = java.io.File.pathSeparator
        assertEquals(
            "libfirst.so${pathSeparator}libsecond.so",
            exampleDiagnosticCompactPath("/tmp/libfirst.so${pathSeparator}/data/app/libsecond.so"),
        )
    }

    @Test
    fun `controller rebuild snapshot preserves playback state position and rate`() {
        val snapshot =
            exampleControllerRebuildSnapshot(
                PlayerHostUiState(
                    title = "Title",
                    subtitle = "Subtitle",
                    sourceLabel = "Source",
                    playbackState = PlaybackStateUi.Playing,
                    playbackRate = 1.5f,
                    isBuffering = false,
                    isInterrupted = false,
                    timeline =
                        TimelineUiState(
                            kind = TimelineKind.Vod,
                            isSeekable = true,
                            seekableRange = SeekableRangeUi(startMs = 0L, endMs = 120_000L),
                            liveEdgeMs = null,
                            positionMs = 42_000L,
                            durationMs = 120_000L,
                        ),
                ),
            )

        assertEquals(true, snapshot.shouldResumePlayback)
        assertEquals(42_000L, snapshot.restorePositionMs)
        assertEquals(1.5f, snapshot.restorePlaybackRate)
    }

    @Test
    fun `controller rebuild source keeps last playback source before playlist fallback`() {
        val playlistSource = androidHlsDemoSource(context = null)
        val playbackSource =
            VesperPlayerSource.local(
                uri = "content://example/video.mp4",
                label = "Local video",
            )

        assertEquals(
            playbackSource,
            exampleControllerRebuildSource(
                activePlaybackSource = playbackSource,
                activePlaylistSource = playlistSource,
            ),
        )
        assertEquals(
            playlistSource,
            exampleControllerRebuildSource(
                activePlaybackSource = null,
                activePlaylistSource = playlistSource,
            ),
        )
    }

    @Test
    fun `native frame status formatter ignores non native frame diagnostics`() {
        assertEquals(
            "",
            exampleNativeFramePipelineStatusLine(
                mapOf(
                    "pluginKind" to "source_normalizer",
                    "lifecycle" to "open",
                )
            ),
        )
    }

    @Test
    fun `local media cache filename is ascii safe and keeps extension`() {
        assertEquals(
            "1234-VID_20260217_163223.mp4",
            exampleLocalMediaCacheFileName("VID_20260217_163223.mp4", nowMillis = 1234L),
        )
        assertEquals(
            "1234-video.mov",
            exampleLocalMediaCacheFileName("视频.mov", nowMillis = 1234L),
        )
        assertEquals(
            "1234-from_picker",
            exampleLocalMediaCacheFileName("../from picker", nowMillis = 1234L),
        )
    }
}
