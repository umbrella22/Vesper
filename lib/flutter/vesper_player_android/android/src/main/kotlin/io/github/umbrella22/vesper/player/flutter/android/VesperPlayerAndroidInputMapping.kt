package io.github.umbrella22.vesper.player.flutter.android

import io.github.umbrella22.vesper.player.android.VesperAbrPolicy
import io.github.umbrella22.vesper.player.android.VesperBackgroundPlaybackMode
import io.github.umbrella22.vesper.player.android.VesperBenchmarkConfiguration
import io.github.umbrella22.vesper.player.android.VesperBufferingPolicy
import io.github.umbrella22.vesper.player.android.VesperBufferingPreset
import io.github.umbrella22.vesper.player.android.VesperCachePolicy
import io.github.umbrella22.vesper.player.android.VesperCachePreset
import io.github.umbrella22.vesper.player.android.VesperDownloadAssetIndex
import io.github.umbrella22.vesper.player.android.VesperDownloadAssetStream
import io.github.umbrella22.vesper.player.android.VesperDownloadByteRange
import io.github.umbrella22.vesper.player.android.VesperDownloadConfiguration
import io.github.umbrella22.vesper.player.android.VesperDownloadContentFormat
import io.github.umbrella22.vesper.player.android.VesperDownloadOutputFormat
import io.github.umbrella22.vesper.player.android.VesperDownloadProfile
import io.github.umbrella22.vesper.player.android.VesperDownloadRecoveredTaskPlan
import io.github.umbrella22.vesper.player.android.VesperDownloadResourceRecord
import io.github.umbrella22.vesper.player.android.VesperDownloadSegmentRecord
import io.github.umbrella22.vesper.player.android.VesperDownloadSource
import io.github.umbrella22.vesper.player.android.VesperDownloadStreamKind
import io.github.umbrella22.vesper.player.android.VesperExternalSubtitleSource
import io.github.umbrella22.vesper.player.android.VesperFrameProcessorConfiguration
import io.github.umbrella22.vesper.player.android.VesperFrameProcessorMode
import io.github.umbrella22.vesper.player.android.VesperNativeFramePipelineConfiguration
import io.github.umbrella22.vesper.player.android.VesperNativeFramePipelineMode
import io.github.umbrella22.vesper.player.android.VesperPlaybackCapabilityProbeRequest
import io.github.umbrella22.vesper.player.android.VesperPlaybackResiliencePolicy
import io.github.umbrella22.vesper.player.android.VesperPluginReference
import io.github.umbrella22.vesper.player.android.VesperPlayerSource
import io.github.umbrella22.vesper.player.android.VesperPlayerSourceKind
import io.github.umbrella22.vesper.player.android.VesperPlayerSourceProtocol
import io.github.umbrella22.vesper.player.android.VesperPlayerUnsupportedOperation
import io.github.umbrella22.vesper.player.android.VesperPreloadBudgetPolicy
import io.github.umbrella22.vesper.player.android.VesperPipelineEventHookConfiguration
import io.github.umbrella22.vesper.player.android.VesperRetryBackoff
import io.github.umbrella22.vesper.player.android.VesperRetryPolicy
import io.github.umbrella22.vesper.player.android.VesperSourceNormalizerConfiguration
import io.github.umbrella22.vesper.player.android.VesperSourceNormalizerMode
import io.github.umbrella22.vesper.player.android.VesperSubtitleStyle
import io.github.umbrella22.vesper.player.android.VesperSystemPlaybackControlButton
import io.github.umbrella22.vesper.player.android.VesperSystemPlaybackControlKind
import io.github.umbrella22.vesper.player.android.VesperSystemPlaybackConfiguration
import io.github.umbrella22.vesper.player.android.VesperSystemPlaybackControls
import io.github.umbrella22.vesper.player.android.VesperSystemPlaybackMetadata
import io.github.umbrella22.vesper.player.android.VesperTrackPreferencePolicy
import io.github.umbrella22.vesper.player.android.VesperTrackSelection
import io.github.umbrella22.vesper.player.android.VesperVideoSurfaceKind
import io.github.umbrella22.vesper.player.android.vesperPlayerDrmConfigurationFromWireMap
import java.io.File

internal fun Map<String, Any?>.toBenchmarkConfiguration(): VesperBenchmarkConfiguration =
    VesperBenchmarkConfiguration(
        enabled = this["enabled"] as? Boolean ?: false,
        maxBufferedEvents = (this["maxBufferedEvents"] as? Number)?.toInt() ?: 2_048,
        includeRawEvents = this["includeRawEvents"] as? Boolean ?: true,
        consoleLogging = this["consoleLogging"] as? Boolean ?: false,
        pluginReferences = decodeVesperPluginReferences(this["pluginReferences"]),
    )

private fun decodeVesperPluginReferences(value: Any?): List<VesperPluginReference> {
    if (value == null) {
        return emptyList()
    }
    require(value is List<*>) { "pluginReferences must be a list" }
    return value.map { entry ->
        require(entry is Map<*, *>) { "invalid plugin reference" }
        val map = entry.stringMap()
        val pluginId = map["pluginId"] as? String
            ?: throw IllegalArgumentException("plugin reference pluginId is required")
        val capabilityInstanceId = map["capabilityInstanceId"]
        require(capabilityInstanceId == null || capabilityInstanceId is String) {
            "plugin reference capabilityInstanceId must be a string"
        }
        val transport = map["transport"] as? String
            ?: throw IllegalArgumentException("plugin reference transport is required")
        VesperPluginReference.fromWire(
            pluginId = pluginId,
            capabilityInstanceId = capabilityInstanceId,
            transportRawValue = transport,
        )
    }
}

internal fun Map<String, Any?>?.toSourceNormalizerConfiguration():
    VesperSourceNormalizerConfiguration {
    if (this == null) {
        return VesperSourceNormalizerConfiguration()
    }
    return VesperSourceNormalizerConfiguration(
        mode =
            when (this["mode"] as? String) {
                "diagnosticsOnly" -> VesperSourceNormalizerMode.DiagnosticsOnly
                "preflightOnly" -> VesperSourceNormalizerMode.PreflightOnly
                "preferNormalized" -> VesperSourceNormalizerMode.PreferNormalized
                "requireNormalized" -> VesperSourceNormalizerMode.RequireNormalized
                else -> VesperSourceNormalizerMode.Disabled
            },
        pluginReferences = decodeVesperPluginReferences(this["pluginReferences"]),
        runtimeProfile = (this["runtimeProfile"] as? String)?.takeIf(String::isNotEmpty),
    )
}

internal fun Map<String, Any?>?.toFrameProcessorConfiguration():
    VesperFrameProcessorConfiguration {
    if (this == null) {
        return VesperFrameProcessorConfiguration()
    }
    return VesperFrameProcessorConfiguration(
        mode =
            when (this["mode"] as? String) {
                "diagnosticsOnly" -> VesperFrameProcessorMode.DiagnosticsOnly
                else -> VesperFrameProcessorMode.Disabled
            },
        pluginReferences = decodeVesperPluginReferences(this["pluginReferences"]),
    )
}

internal fun Map<String, Any?>?.toNativeFramePipelineConfiguration():
    VesperNativeFramePipelineConfiguration {
    if (this == null) {
        return VesperNativeFramePipelineConfiguration()
    }
    return VesperNativeFramePipelineConfiguration(
        mode =
            when (this["mode"] as? String) {
                "diagnosticsOnly" -> VesperNativeFramePipelineMode.DiagnosticsOnly
                "preferNativeFrame" -> VesperNativeFramePipelineMode.PreferNativeFrame
                "requireNativeFrame" -> VesperNativeFramePipelineMode.RequireNativeFrame
                else -> VesperNativeFramePipelineMode.Disabled
            },
        decoderPluginReferences = decodeVesperPluginReferences(this["decoderPluginReferences"]),
        frameProcessorPluginReferences =
            decodeVesperPluginReferences(this["frameProcessorPluginReferences"]),
        maxInFlightFrames = (this["maxInFlightFrames"] as? Number)?.toInt(),
    )
}

internal fun Map<String, Any?>.toPipelineEventHookConfiguration():
    VesperPipelineEventHookConfiguration =
    VesperPipelineEventHookConfiguration(
        pluginReferences = decodeVesperPluginReferences(this["eventHookPluginReferences"]),
    )

internal fun Map<String, Any?>.toPlaybackCapabilityProbeRequest():
    VesperPlaybackCapabilityProbeRequest =
    VesperPlaybackCapabilityProbeRequest(
        source = (this["source"] as? Map<*, *>)?.stringMap()?.toVesperPlayerSource(),
        codec = this["codec"] as? String,
        width = (this["width"] as? Number)?.toInt(),
        height = (this["height"] as? Number)?.toInt(),
        frameRate = (this["frameRate"] as? Number)?.toFloat(),
        requiresNativeFrame = this["requiresNativeFrame"] as? Boolean ?: false,
        sourceNormalizerConfiguration =
            (this["sourceNormalizer"] as? Map<*, *>)
                ?.stringMap()
                .toSourceNormalizerConfiguration(),
        frameProcessorConfiguration =
            (this["frameProcessor"] as? Map<*, *>)
                ?.stringMap()
                .toFrameProcessorConfiguration(),
        nativeFramePipelineConfiguration =
            (this["nativeFramePipeline"] as? Map<*, *>)
                ?.stringMap()
                .toNativeFramePipelineConfiguration(),
    )

internal fun Any?.toVesperVideoSurfaceKind(): VesperVideoSurfaceKind =
    when (this as? String ?: "auto") {
        "auto", "surfaceView" -> VesperVideoSurfaceKind.SurfaceView
        "textureView" -> VesperVideoSurfaceKind.TextureView
        else -> throw IllegalArgumentException("Unknown renderSurfaceKind: $this.")
    }

internal fun Map<String, Any?>.toVesperPlayerSource(): VesperPlayerSource {
    val uri = this["uri"] as? String ?: throw IllegalArgumentException("Missing source uri.")
    val label = this["label"] as? String ?: uri
    val externalSubtitleEntries = this["externalSubtitles"] ?: this["subtitleConfigurations"]
    return VesperPlayerSource(
        uri = uri,
        label = label,
        kind = when (this["kind"] as? String) {
            "remote" -> VesperPlayerSourceKind.Remote
            else -> VesperPlayerSourceKind.Local
        },
        protocol = when (this["protocol"] as? String) {
            "file" -> VesperPlayerSourceProtocol.File
            "content" -> VesperPlayerSourceProtocol.Content
            "progressive" -> VesperPlayerSourceProtocol.Progressive
            "hls" -> VesperPlayerSourceProtocol.Hls
            "dash" -> VesperPlayerSourceProtocol.Dash
            "rtmp" -> VesperPlayerSourceProtocol.Rtmp
            "rtsp" -> VesperPlayerSourceProtocol.Rtsp
            "flv" -> VesperPlayerSourceProtocol.Flv
            else -> VesperPlayerSourceProtocol.Unknown
        },
        headers = this["headers"].stringStringMap(),
        drmConfiguration =
            (this["drmConfiguration"] as? Map<*, *>)
                ?.stringMap()
                ?.let(::vesperPlayerDrmConfigurationFromWireMap),
        externalSubtitles =
            (externalSubtitleEntries as? List<*>)
                ?.map { value ->
                    (value as? Map<*, *>)?.stringMap()?.toVesperExternalSubtitleSource()
                        ?: throw IllegalArgumentException("Invalid externalSubtitles entry.")
                }
                ?: emptyList(),
    )
}

internal fun Map<String, Any?>.toVesperSubtitleStyle(): VesperSubtitleStyle {
    val fontScale = (this["fontScale"] as? Number)?.toFloat() ?: 1.0f
    require(fontScale.isFinite() && fontScale in 0.5f..3.0f) {
        "Subtitle fontScale must be finite and between 0.5 and 3.0."
    }
    return VesperSubtitleStyle(
        fontScale = fontScale,
        visible = this["visible"] as? Boolean ?: true,
    )
}

private fun Map<String, Any?>.toVesperExternalSubtitleSource(): VesperExternalSubtitleSource {
    val id = this["id"] as? String
        ?: throw IllegalArgumentException("Missing external subtitle id.")
    require(id.isNotBlank()) { "External subtitle id must not be blank." }
    val uri = this["uri"] as? String
        ?: throw IllegalArgumentException("Missing subtitle uri.")
    require(uri.isNotBlank()) { "Subtitle uri must not be blank." }
    val mimeType = this["mimeType"] as? String ?: VesperExternalSubtitleSource.MIME_SUBRIP
    require(mimeType.isNotBlank()) { "Subtitle MIME type must not be blank." }
    return VesperExternalSubtitleSource(
        id = id,
        uri = uri,
        mimeType = mimeType,
        language = this["language"] as? String,
        label = this["label"] as? String,
        headers = this["headers"].stringStringMap(),
        isDefault = this["isDefault"] as? Boolean ?: false,
        isForced = this["isForced"] as? Boolean ?: false,
    )
}

internal fun Map<String, Any?>.toSystemPlaybackConfiguration(): VesperSystemPlaybackConfiguration =
    VesperSystemPlaybackConfiguration(
        enabled = this["enabled"] as? Boolean ?: true,
        backgroundMode =
            when (this["backgroundMode"] as? String) {
                "disabled" -> VesperBackgroundPlaybackMode.Disabled
                else -> VesperBackgroundPlaybackMode.ContinueAudio
            },
        showSystemControls = this["showSystemControls"] as? Boolean ?: true,
        showSeekActions = this["showSeekActions"] as? Boolean ?: true,
        metadata =
            (this["metadata"] as? Map<*, *>)
                ?.stringMap()
                ?.toSystemPlaybackMetadata(),
        controls =
            (this["controls"] as? Map<*, *>)
                ?.stringMap()
                ?.toSystemPlaybackControls()
                ?: VesperSystemPlaybackControls.videoDefault(),
    )

internal fun Map<String, Any?>.toSystemPlaybackMetadata(): VesperSystemPlaybackMetadata =
    VesperSystemPlaybackMetadata(
        title = this["title"] as? String ?: "",
        artist = this["artist"] as? String,
        albumTitle = this["albumTitle"] as? String,
        artworkUri = this["artworkUri"] as? String,
        contentUri = this["contentUri"] as? String,
        durationMs = (this["durationMs"] as? Number)?.toLong(),
        isLive = this["isLive"] as? Boolean ?: false,
    )

internal fun Map<String, Any?>.toSystemPlaybackControls(): VesperSystemPlaybackControls =
    VesperSystemPlaybackControls(
        compactButtons =
            (this["compactButtons"] as? List<*>)
                ?.mapNotNull { (it as? Map<*, *>)?.stringMap()?.toSystemPlaybackControlButton() }
                ?: emptyList(),
    )

internal fun Map<String, Any?>.toSystemPlaybackControlButton(): VesperSystemPlaybackControlButton {
    val kind =
        when (this["kind"] as? String) {
            "seekBack" -> VesperSystemPlaybackControlKind.SeekBack
            "seekForward" -> VesperSystemPlaybackControlKind.SeekForward
            else -> VesperSystemPlaybackControlKind.PlayPause
        }
    return VesperSystemPlaybackControlButton(
        kind = kind,
        seekOffsetMs = (this["seekOffsetMs"] as? Number)?.toLong(),
    )
}

internal fun Map<String, Any?>.toDownloadConfiguration(): VesperDownloadConfiguration =
    VesperDownloadConfiguration(
        autoStart = this["autoStart"] as? Boolean ?: true,
        runPostProcessorsOnCompletion =
            this["runPostProcessorsOnCompletion"] as? Boolean ?: true,
        resumePartialDownloads = this["resumePartialDownloads"] as? Boolean ?: true,
        restoreTasksOnStartup = this["restoreTasksOnStartup"] as? Boolean ?: true,
        baseDirectory = (this["baseDirectory"] as? String)?.let(::File),
        postDownloadPluginReferences = decodeVesperPluginReferences(
            this["postDownloadPluginReferences"],
        ),
        eventHookPluginReferences = decodeVesperPluginReferences(
            this["eventHookPluginReferences"],
        ),
        rangeChunkBytes = (this["rangeChunkBytes"] as? Number)?.toLong(),
        minProgressBytes = (this["minProgressBytes"] as? Number)?.toLong() ?: 512L * 1024L,
        minProgressIntervalMs = (this["minProgressIntervalMs"] as? Number)?.toLong() ?: 250L,
    )

internal fun Map<String, Any?>.toDownloadRecoveredTaskPlan(): VesperDownloadRecoveredTaskPlan =
    VesperDownloadRecoveredTaskPlan(
        source = requireNestedMap(this, "source").toDownloadSource(),
        profile = requireNestedMap(this, "profile").toDownloadProfile(),
        assetIndex = requireNestedMap(this, "assetIndex").toDownloadAssetIndex(),
    )

internal fun Map<String, Any?>.toDownloadSource(): VesperDownloadSource =
    VesperDownloadSource(
        source = requireNestedMap(this, "source").toVesperPlayerSource(),
        contentFormat =
            when (this["contentFormat"] as? String) {
                "hlsSegments" -> VesperDownloadContentFormat.HlsSegments
                "dashSegments" -> VesperDownloadContentFormat.DashSegments
                "flvSegments" -> VesperDownloadContentFormat.FlvSegments
                "singleFile" -> VesperDownloadContentFormat.SingleFile
                else -> VesperDownloadContentFormat.Unknown
            },
        manifestUri = this["manifestUri"] as? String,
    )

internal fun Map<String, Any?>.toDownloadProfile(): VesperDownloadProfile =
    VesperDownloadProfile(
        variantId = this["variantId"] as? String,
        preferredAudioLanguage = this["preferredAudioLanguage"] as? String,
        preferredSubtitleLanguage = this["preferredSubtitleLanguage"] as? String,
        selectedTrackIds =
            (this["selectedTrackIds"] as? List<*>)
                ?.mapNotNull { value -> value?.toString() }
                ?: emptyList(),
        targetOutputFormat =
            when (this["targetOutputFormat"] as? String) {
                "mp4" -> VesperDownloadOutputFormat.Mp4
                "mkv" -> VesperDownloadOutputFormat.Mkv
                "original" -> VesperDownloadOutputFormat.Original
                else -> null
            },
        targetDirectory = this["targetDirectory"] as? String,
        allowMeteredNetwork = this["allowMeteredNetwork"] as? Boolean ?: false,
    )

internal fun Map<String, Any?>.toDownloadAssetIndex(): VesperDownloadAssetIndex =
    VesperDownloadAssetIndex(
        contentFormat =
            when (this["contentFormat"] as? String) {
                "hlsSegments" -> VesperDownloadContentFormat.HlsSegments
                "dashSegments" -> VesperDownloadContentFormat.DashSegments
                "flvSegments" -> VesperDownloadContentFormat.FlvSegments
                "singleFile" -> VesperDownloadContentFormat.SingleFile
                else -> VesperDownloadContentFormat.Unknown
            },
        version = this["version"] as? String,
        etag = this["etag"] as? String,
        checksum = this["checksum"] as? String,
        totalSizeBytes = (this["totalSizeBytes"] as? Number)?.toLong(),
        resources =
            (this["resources"] as? List<*>)
                ?.mapNotNull { value ->
                    (value as? Map<*, *>)?.stringMap()?.toDownloadResourceRecord()
                }
                ?: emptyList(),
        segments =
            (this["segments"] as? List<*>)
                ?.mapNotNull { value ->
                    (value as? Map<*, *>)?.stringMap()?.toDownloadSegmentRecord()
                }
                ?: emptyList(),
        streams =
            (this["streams"] as? List<*>)
                ?.mapNotNull { value ->
                    (value as? Map<*, *>)?.stringMap()?.toDownloadAssetStream()
                }
                ?: emptyList(),
        completedPath = this["completedPath"] as? String,
    )

internal fun Map<String, Any?>.toDownloadResourceRecord(): VesperDownloadResourceRecord =
    VesperDownloadResourceRecord(
        resourceId = this["resourceId"] as? String ?: "",
        uri = this["uri"] as? String ?: "",
        relativePath = this["relativePath"] as? String,
        byteRange = (this["byteRange"] as? Map<*, *>)?.stringMap()?.toDownloadByteRange(),
        generatedText = this["generatedText"] as? String,
        sizeBytes = (this["sizeBytes"] as? Number)?.toLong(),
        etag = this["etag"] as? String,
        checksum = this["checksum"] as? String,
    )

internal fun Map<String, Any?>.toDownloadSegmentRecord(): VesperDownloadSegmentRecord =
    VesperDownloadSegmentRecord(
        segmentId = this["segmentId"] as? String ?: "",
        uri = this["uri"] as? String ?: "",
        relativePath = this["relativePath"] as? String,
        sequence = (this["sequence"] as? Number)?.toLong(),
        byteRange = (this["byteRange"] as? Map<*, *>)?.stringMap()?.toDownloadByteRange(),
        sizeBytes = (this["sizeBytes"] as? Number)?.toLong(),
        checksum = this["checksum"] as? String,
    )

internal fun Map<String, Any?>.toDownloadAssetStream(): VesperDownloadAssetStream =
    VesperDownloadAssetStream(
        streamId = this["streamId"] as? String ?: "",
        kind =
            (this["kind"] as? String)?.let { wire ->
                VesperDownloadStreamKind.entries.find { it.name.equals(wire, ignoreCase = true) }
            } ?: VesperDownloadStreamKind.Combined,
        language = this["language"] as? String,
        codec = this["codec"] as? String,
        label = this["label"] as? String,
        qualityRank = (this["qualityRank"] as? Number)?.toInt(),
        resourceIds = (this["resourceIds"] as? List<*>)?.filterIsInstance<String>() ?: emptyList(),
        segmentIds = (this["segmentIds"] as? List<*>)?.filterIsInstance<String>() ?: emptyList(),
        metadata =
            (this["metadata"] as? Map<*, *>)
                ?.mapNotNull { (key, value) ->
                    val name = key as? String ?: return@mapNotNull null
                    val text = value as? String ?: return@mapNotNull null
                    name to text
                }
                ?.toMap()
                ?: emptyMap(),
    )

internal fun Map<String, Any?>.toDownloadByteRange(): VesperDownloadByteRange =
    VesperDownloadByteRange(
        offset = (this["offset"] as? Number)?.toLong() ?: 0L,
        length = (this["length"] as? Number)?.toLong() ?: 0L,
    )

internal fun Map<String, Any?>.toTrackSelection(
    isSubtitle: Boolean = false,
): VesperTrackSelection =
    when (val mode = this["mode"] as? String) {
        "auto" -> VesperTrackSelection.auto()
        "disabled" -> VesperTrackSelection.disabled()
        "track" -> {
            val trackId = (this["trackId"] as? String)?.takeIf { it.isNotBlank() }
                ?: if (isSubtitle) {
                    throw subtitleSelectionInputError(
                        trackId = this["trackId"] as? String,
                        message = "Missing trackId for subtitle track selection.",
                    )
                } else {
                    throw IllegalArgumentException("Missing trackId for track selection.")
                }
            VesperTrackSelection.track(trackId)
        }
        else -> {
            if (isSubtitle) {
                throw subtitleSelectionInputError(
                    trackId = this["trackId"] as? String,
                    message = "Unknown subtitle selection mode: ${mode ?: "<missing>"}.",
                )
            }
            VesperTrackSelection.auto()
        }
    }

private fun subtitleSelectionInputError(
    trackId: String?,
    message: String,
): VesperPlayerUnsupportedOperation =
    VesperPlayerUnsupportedOperation(
        message,
        mapOf(
            "domain" to "subtitle",
            "code" to "subtitle_selection_invalid",
            "phase" to "selection",
            "trackId" to trackId,
            "retriable" to false,
        ),
    )

internal fun Map<String, Any?>.toAbrPolicy(): VesperAbrPolicy =
    when (this["mode"] as? String) {
        "constrained" -> VesperAbrPolicy.constrained(
            maxBitRate = (this["maxBitRate"] as? Number)?.toLong(),
            maxWidth = (this["maxWidth"] as? Number)?.toInt(),
            maxHeight = (this["maxHeight"] as? Number)?.toInt(),
        )
        "fixedTrack" -> {
            val trackId = this["trackId"] as? String
                ?: throw IllegalArgumentException("Missing trackId for fixed track policy.")
            VesperAbrPolicy.fixedTrack(trackId)
        }
        else -> VesperAbrPolicy.auto()
    }

internal fun Map<String, Any?>.toResiliencePolicy(): VesperPlaybackResiliencePolicy {
    val buffering = (this["buffering"] as? Map<*, *>)?.stringMap()?.toBufferingPolicy()
        ?: VesperBufferingPolicy()
    val retry = (this["retry"] as? Map<*, *>)?.stringMap()?.toRetryPolicy()
        ?: VesperRetryPolicy()
    val cache = (this["cache"] as? Map<*, *>)?.stringMap()?.toCachePolicy()
        ?: VesperCachePolicy()
    return VesperPlaybackResiliencePolicy(
        buffering = buffering,
        retry = retry,
        cache = cache,
    )
}

internal fun Map<String, Any?>.toTrackPreferencePolicy(): VesperTrackPreferencePolicy {
    val audioSelection =
        (this["audioSelection"] as? Map<*, *>)?.stringMap()?.toTrackSelection()
            ?: VesperTrackSelection.auto()
    val subtitleSelection =
        (this["subtitleSelection"] as? Map<*, *>)?.stringMap()?.toTrackSelection(isSubtitle = true)
            ?: VesperTrackSelection.disabled()
    val abrPolicy =
        (this["abrPolicy"] as? Map<*, *>)?.stringMap()?.toAbrPolicy()
            ?: VesperAbrPolicy.auto()
    return VesperTrackPreferencePolicy(
        preferredAudioLanguage = this["preferredAudioLanguage"] as? String,
        preferredSubtitleLanguage = this["preferredSubtitleLanguage"] as? String,
        selectSubtitlesByDefault = this["selectSubtitlesByDefault"] as? Boolean ?: false,
        selectUndeterminedSubtitleLanguage =
            this["selectUndeterminedSubtitleLanguage"] as? Boolean ?: false,
        audioSelection = audioSelection,
        subtitleSelection = subtitleSelection,
        abrPolicy = abrPolicy,
    )
}

internal fun Map<String, Any?>.toPreloadBudgetPolicy(): VesperPreloadBudgetPolicy =
    VesperPreloadBudgetPolicy(
        maxConcurrentTasks = (this["maxConcurrentTasks"] as? Number)?.toInt(),
        maxMemoryBytes = (this["maxMemoryBytes"] as? Number)?.toLong(),
        maxDiskBytes = (this["maxDiskBytes"] as? Number)?.toLong(),
        warmupWindowMs = (this["warmupWindowMs"] as? Number)?.toLong(),
    )

internal fun Map<String, Any?>.toFlutterViewport(): FlutterViewport =
    FlutterViewport(
        left = (this["left"] as? Number)?.toDouble() ?: 0.0,
        top = (this["top"] as? Number)?.toDouble() ?: 0.0,
        width = (this["width"] as? Number)?.toDouble() ?: 0.0,
        height = (this["height"] as? Number)?.toDouble() ?: 0.0,
    )

internal fun Map<String, Any?>.toFlutterViewportHint(): FlutterViewportHint =
    FlutterViewportHint(
        kind =
            when (this["kind"] as? String) {
                "visible" -> "visible"
                "nearVisible" -> "nearVisible"
                "prefetchOnly" -> "prefetchOnly"
                else -> "hidden"
            },
        visibleFraction =
            ((this["visibleFraction"] as? Number)?.toDouble() ?: 0.0).coerceIn(0.0, 1.0),
    )

internal fun Map<String, Any?>.toBufferingPolicy(): VesperBufferingPolicy =
    VesperBufferingPolicy(
        preset = when (this["preset"] as? String) {
            "balanced" -> VesperBufferingPreset.Balanced
            "streaming" -> VesperBufferingPreset.Streaming
            "resilient" -> VesperBufferingPreset.Resilient
            "lowLatency" -> VesperBufferingPreset.LowLatency
            else -> VesperBufferingPreset.Default
        },
        minBufferMs = (this["minBufferMs"] as? Number)?.toInt(),
        maxBufferMs = (this["maxBufferMs"] as? Number)?.toInt(),
        bufferForPlaybackMs = (this["bufferForPlaybackMs"] as? Number)?.toInt(),
        bufferForPlaybackAfterRebufferMs =
            (this["bufferForPlaybackAfterRebufferMs"] as? Number)?.toInt(),
    )

internal fun Map<String, Any?>.toRetryPolicy(): VesperRetryPolicy =
    VesperRetryPolicy(
        maxAttempts =
            when {
                !containsKey("maxAttempts") -> 3
                this["maxAttempts"] == null -> null
                else -> (this["maxAttempts"] as? Number)?.toInt() ?: 3
            },
        baseDelayMs = (this["baseDelayMs"] as? Number)?.toLong(),
        maxDelayMs = (this["maxDelayMs"] as? Number)?.toLong(),
        backoff = when (this["backoff"] as? String) {
            "fixed" -> VesperRetryBackoff.Fixed
            "linear" -> VesperRetryBackoff.Linear
            "exponential" -> VesperRetryBackoff.Exponential
            else -> null
        },
    )

internal fun Map<String, Any?>.toCachePolicy(): VesperCachePolicy =
    VesperCachePolicy(
        preset = when (this["preset"] as? String) {
            "disabled" -> VesperCachePreset.Disabled
            "streaming" -> VesperCachePreset.Streaming
            "resilient" -> VesperCachePreset.Resilient
            else -> VesperCachePreset.Default
        },
        maxMemoryBytes = (this["maxMemoryBytes"] as? Number)?.toLong(),
        maxDiskBytes = (this["maxDiskBytes"] as? Number)?.toLong(),
    )
