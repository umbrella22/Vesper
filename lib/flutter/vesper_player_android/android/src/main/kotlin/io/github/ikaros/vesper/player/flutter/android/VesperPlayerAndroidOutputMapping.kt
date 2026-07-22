package io.github.ikaros.vesper.player.flutter.android

import io.github.ikaros.vesper.player.android.TimelineUiState
import io.github.ikaros.vesper.player.android.VesperAbrPolicy
import io.github.ikaros.vesper.player.android.VesperBufferingPolicy
import io.github.ikaros.vesper.player.android.VesperCachePolicy
import io.github.ikaros.vesper.player.android.VesperDownloadAssetIndex
import io.github.ikaros.vesper.player.android.VesperDownloadAssetStream
import io.github.ikaros.vesper.player.android.VesperDownloadByteRange
import io.github.ikaros.vesper.player.android.VesperDownloadContentFormat
import io.github.ikaros.vesper.player.android.VesperDownloadError
import io.github.ikaros.vesper.player.android.VesperDownloadOutputFormat
import io.github.ikaros.vesper.player.android.VesperDownloadProfile
import io.github.ikaros.vesper.player.android.VesperDownloadProgressSnapshot
import io.github.ikaros.vesper.player.android.VesperDownloadResourceRecord
import io.github.ikaros.vesper.player.android.VesperDownloadSegmentRecord
import io.github.ikaros.vesper.player.android.VesperDownloadSource
import io.github.ikaros.vesper.player.android.VesperDownloadStaleResource
import io.github.ikaros.vesper.player.android.VesperDownloadState
import io.github.ikaros.vesper.player.android.VesperDownloadTaskProgressPatch
import io.github.ikaros.vesper.player.android.VesperDownloadTaskSnapshot
import io.github.ikaros.vesper.player.android.VesperDownloadTaskStatePatch
import io.github.ikaros.vesper.player.android.VesperHdrChromaticityPoint
import io.github.ikaros.vesper.player.android.VesperMediaTrack
import io.github.ikaros.vesper.player.android.VesperPlaybackCapabilityConfidence
import io.github.ikaros.vesper.player.android.VesperPlaybackCapabilityDolbyVisionMode
import io.github.ikaros.vesper.player.android.VesperPlaybackCapabilityHdrKind
import io.github.ikaros.vesper.player.android.VesperPlaybackCapabilityHdrMetadata
import io.github.ikaros.vesper.player.android.VesperPlaybackCapabilityOutputFormat
import io.github.ikaros.vesper.player.android.VesperPlaybackCapabilityProbeStatus
import io.github.ikaros.vesper.player.android.VesperPlaybackCapabilityProbeResult
import io.github.ikaros.vesper.player.android.VesperPlaybackCodecFamily
import io.github.ikaros.vesper.player.android.VesperPlaybackResiliencePolicy
import io.github.ikaros.vesper.player.android.VesperPlayerUnsupportedOperation
import io.github.ikaros.vesper.player.android.VesperPlayerSource
import io.github.ikaros.vesper.player.android.VesperRecommendedPlaybackPath
import io.github.ikaros.vesper.player.android.VesperRetryPolicy
import io.github.ikaros.vesper.player.android.VesperTrackCatalog
import io.github.ikaros.vesper.player.android.VesperTrackSelection
import io.github.ikaros.vesper.player.android.VesperTrackSelectionSnapshot

internal fun TimelineUiState.toMap(): Map<String, Any?> =
    mapOf(
        "kind" to kind.toWireName(),
        "isSeekable" to isSeekable,
        "seekableRange" to seekableRange?.let { range ->
            mapOf(
                "startMs" to range.startMs,
                "endMs" to range.endMs,
            )
        },
        "liveEdgeMs" to liveEdgeMs,
        "positionMs" to positionMs,
        "durationMs" to durationMs,
    )

internal fun VesperTrackCatalog.toMap(): Map<String, Any?> =
    mapOf(
        "tracks" to tracks.map(VesperMediaTrack::toMap),
        "adaptiveVideo" to adaptiveVideo,
        "adaptiveAudio" to adaptiveAudio,
    )

internal fun VesperMediaTrack.toMap(): Map<String, Any?> =
    mapOf(
        "id" to id,
        "kind" to kind.toWireName(),
        "label" to label,
        "language" to language,
        "codec" to codec,
        "bitRate" to bitRate,
        "width" to width,
        "height" to height,
        "frameRate" to frameRate?.toDouble(),
        "channels" to channels,
        "sampleRate" to sampleRate,
        "isDefault" to isDefault,
        "isForced" to isForced,
    )

internal fun VesperTrackSelectionSnapshot.toMap(): Map<String, Any?> =
    mapOf(
        "video" to video.toMap(),
        "audio" to audio.toMap(),
        "subtitle" to subtitle.toMap(),
        "confirmedSubtitle" to confirmedSubtitle.toMap(),
        "effectiveSubtitleTrackId" to effectiveSubtitleTrackId,
        "abrPolicy" to abrPolicy.toMap(),
    )

internal fun VesperTrackSelection.toMap(): Map<String, Any?> =
    mapOf(
        "mode" to mode.toWireName(),
        "trackId" to trackId,
    )

internal fun VesperAbrPolicy.toMap(): Map<String, Any?> =
    mapOf(
        "mode" to mode.toWireName(),
        "trackId" to trackId,
        "maxBitRate" to maxBitRate,
        "maxWidth" to maxWidth,
        "maxHeight" to maxHeight,
    )

internal fun VesperPlaybackResiliencePolicy.toMap(): Map<String, Any?> =
    mapOf(
        "buffering" to buffering.toMap(),
        "retry" to retry.toMap(),
        "cache" to cache.toMap(),
    )

internal fun VesperBufferingPolicy.toMap(): Map<String, Any?> =
    mapOf(
        "preset" to preset.toWireName(),
        "minBufferMs" to minBufferMs,
        "maxBufferMs" to maxBufferMs,
        "bufferForPlaybackMs" to bufferForPlaybackMs,
        "bufferForPlaybackAfterRebufferMs" to bufferForPlaybackAfterRebufferMs,
    )

internal fun VesperRetryPolicy.toMap(): Map<String, Any?> =
    mapOf(
        "maxAttempts" to maxAttempts,
        "baseDelayMs" to baseDelayMs,
        "maxDelayMs" to maxDelayMs,
        "backoff" to backoff.toWireName(),
    )

internal fun VesperCachePolicy.toMap(): Map<String, Any?> =
    mapOf(
        "preset" to preset.toWireName(),
        "maxMemoryBytes" to maxMemoryBytes,
        "maxDiskBytes" to maxDiskBytes,
    )

internal fun Throwable.toErrorMap(): Map<String, Any?> {
    if (this is VesperPlayerUnsupportedOperation) {
        val subtitleCode = details["code"] as? String
        val isSubtitleError = details["domain"] == "subtitle"
        if (subtitleCode != null && isSubtitleError) {
            val phase =
                details["phase"] as? String
                    ?: "selection"
            return mapOf(
                "domain" to "subtitle",
                "code" to subtitleCode,
                "phase" to phase,
                "trackId" to details["trackId"],
                "retriable" to (details["retriable"] as? Boolean ?: false),
                "message" to
                    (details["message"] as? String
                        ?: message
                        ?: "Subtitle operation failed."),
                "commandId" to details["commandId"],
                "sourceEpoch" to details["sourceEpoch"],
            )
        }
        val errorDetails = mapOf("exception" to this::class.java.name) + details
        return mapOf(
            "message" to (message ?: toString()),
            "code" to "unsupported",
            "category" to "capability",
            "retriable" to false,
            "details" to errorDetails,
        )
    }
    return mapOf(
        "message" to (message ?: toString()),
        "code" to "backendFailure",
        "category" to "platform",
        "retriable" to false,
        "details" to mapOf(
            "exception" to this::class.java.name,
        ),
    )
}

internal fun Map<String, Any?>.toEventErrorMap(): Map<String, Any?> {
    if (this["domain"] != "subtitle") {
        return this
    }
    return mapOf(
        "message" to (this["message"] as? String ?: "Subtitle operation failed."),
        "code" to "backendFailure",
        "category" to "platform",
        "retriable" to (this["retriable"] as? Boolean ?: false),
        "details" to this,
    )
}

internal fun VesperPlaybackCapabilityProbeResult.toMap(): Map<String, Any?> =
    mapOf(
        "status" to status.wireName,
        "codecFamily" to codecFamily.wireName,
        "systemPlaybackSupported" to systemPlaybackSupported,
        "hardwareDecodeSupported" to hardwareDecodeSupported,
        "sdkManagedNativeFrameSupported" to sdkManagedNativeFrameSupported,
        "recommendedPlaybackPath" to recommendedPlaybackPath.wireName,
        "outputFormat" to outputFormat.wireName,
        "hdrKind" to hdrKind.wireName,
        "dolbyVisionMode" to dolbyVisionMode.wireName,
        "confidence" to confidence.wireName,
        "hdrMetadata" to hdrMetadataMap(),
        "missingCapabilities" to missingCapabilities,
        "diagnostics" to diagnostics,
    )

private fun VesperPlaybackCapabilityProbeResult.hdrMetadataMap(): Map<String, Any?>? {
    val values = hdrMetadata?.toMap()?.toMutableMap() ?: linkedMapOf()
    if (!values.containsKey("hdrKind") &&
        hdrKind != VesperPlaybackCapabilityHdrKind.None &&
        hdrKind != VesperPlaybackCapabilityHdrKind.Unknown
    ) {
        values["hdrKind"] = hdrKind.wireName
    }
    if (!values.containsKey("dolbyVisionMode") &&
        dolbyVisionMode != VesperPlaybackCapabilityDolbyVisionMode.None
    ) {
        values["dolbyVisionMode"] = dolbyVisionMode.wireName
    }
    diagnostics.firstString("runtimeFormatHdrMetadataProbe", "assetVideoHdrMetadataProbe", "assetProbe")?.let {
        values["probe"] = it
    }
    diagnostics.firstString("assetVideoCodec", "runtimeFormatCodecs")?.let {
        values["codec"] = it
    }
    diagnostics.stringValue("runtimeFormatSampleMimeType")?.let {
        values["sampleMimeType"] = it
    }
    diagnostics.stringValue("assetVideoColorPrimaries")?.let {
        values["colorPrimaries"] = it
    }
    diagnostics.stringValue("runtimeFormatColorSpace")?.let {
        values["colorSpace"] = it
    }
    diagnostics.stringValue("runtimeFormatColorRange")?.let {
        values["colorRange"] = it
    }
    diagnostics.firstString("assetVideoTransferFunction", "runtimeFormatColorTransfer")?.let {
        values["transferFunction"] = it
    }
    diagnostics.stringValue("assetVideoYCbCrMatrix")?.let {
        values["yCbCrMatrix"] = it
    }
    diagnostics.stringValue("assetVideoAlternativeTransferCharacteristics")?.let {
        values["alternativeTransferCharacteristics"] = it
    }
    diagnostics.intValue("runtimeFormatLumaBitDepth")?.let {
        values["lumaBitDepth"] = it
    }
    diagnostics.intValue("runtimeFormatChromaBitDepth")?.let {
        values["chromaBitDepth"] = it
    }
    diagnostics.boolValue("runtimeFormatHdrStaticInfoPresent")?.let {
        values["hdrStaticInfoPresent"] = it
    }
    diagnostics.intValue("runtimeFormatHdrStaticInfoByteLength")?.let {
        values["hdrStaticInfoByteLength"] = it
    }
    diagnostics.stringValue("runtimeFormatHdrStaticInfoParseError")?.let {
        values["hdrStaticInfoParseError"] = it
    }
    diagnostics.firstInt("assetVideoMaxContentLightLevelNits", "runtimeFormatMaxContentLightLevelNits")?.let {
        values["maxContentLightLevelNits"] = it
    }
    diagnostics.firstInt("assetVideoMaxFrameAverageLightLevelNits", "runtimeFormatMaxFrameAverageLightLevelNits")?.let {
        values["maxFrameAverageLightLevelNits"] = it
    }
    diagnostics.boolValue("assetVideoMasteringDisplayColorVolumePresent")?.let {
        values["masteringDisplayColorVolumePresent"] = it
    }
    diagnostics.intValue("assetVideoMasteringDisplayColorVolumeByteLength")?.let {
        values["masteringDisplayColorVolumeByteLength"] = it
    }
    diagnostics.stringValue("assetVideoMasteringDisplayColorVolumeParseError")?.let {
        values["masteringDisplayColorVolumeParseError"] = it
    }
    diagnostics.chromaticityPoint("assetVideoMasteringDisplayPrimary0")?.let {
        values["masteringDisplayPrimary0"] = it
    }
    diagnostics.chromaticityPoint("assetVideoMasteringDisplayPrimary1")?.let {
        values["masteringDisplayPrimary1"] = it
    }
    diagnostics.chromaticityPoint("assetVideoMasteringDisplayPrimary2")?.let {
        values["masteringDisplayPrimary2"] = it
    }
    diagnostics.chromaticityPoint("assetVideoMasteringDisplayWhitePoint")?.let {
        values["masteringDisplayWhitePoint"] = it
    }
    diagnostics.doubleValue("assetVideoMasteringDisplayMaxLuminanceNits")?.let {
        values["masteringDisplayMaxLuminanceNits"] = it
    }
    diagnostics.doubleValue("assetVideoMasteringDisplayMinLuminanceNits")?.let {
        values["masteringDisplayMinLuminanceNits"] = it
    }
    diagnostics.stringValue("dolbyVisionCodec")?.let {
        values["dolbyVisionCodec"] = it
    }
    diagnostics.intValue("dolbyVisionProfile")?.let {
        values["dolbyVisionProfile"] = it
    }
    diagnostics.intValue("dolbyVisionLevel")?.let {
        values["dolbyVisionLevel"] = it
    }
    diagnostics.stringValue("dolbyVisionCompatibility")?.let {
        values["dolbyVisionCompatibility"] = it
    }
    diagnostics.stringValue("dolbyVisionProfileFamily")?.let {
        values["dolbyVisionProfileFamily"] = it
    }
    diagnostics.stringValue("dolbyVisionBaseLayer")?.let {
        values["dolbyVisionBaseLayer"] = it
    }
    diagnostics.stringValue("dolbyVisionFallbackTarget")?.let {
        values["dolbyVisionFallbackTarget"] = it
    }
    diagnostics.stringValue("dolbyVisionBaseLayerEvidence")?.let {
        values["dolbyVisionBaseLayerEvidence"] = it
    }
    diagnostics.stringValue("dolbyVisionBaseLayerTransferFunction")?.let {
        values["dolbyVisionBaseLayerTransferFunction"] = it
    }
    return values.takeIf { it.isNotEmpty() }
}

private fun VesperPlaybackCapabilityHdrMetadata.toMap(): Map<String, Any?> =
    linkedMapOf<String, Any?>().also { values ->
        hdrKind?.let { values["hdrKind"] = it.wireName }
        dolbyVisionMode?.let { values["dolbyVisionMode"] = it.wireName }
        probe?.let { values["probe"] = it }
        codec?.let { values["codec"] = it }
        sampleMimeType?.let { values["sampleMimeType"] = it }
        colorPrimaries?.let { values["colorPrimaries"] = it }
        colorSpace?.let { values["colorSpace"] = it }
        colorRange?.let { values["colorRange"] = it }
        transferFunction?.let { values["transferFunction"] = it }
        yCbCrMatrix?.let { values["yCbCrMatrix"] = it }
        alternativeTransferCharacteristics?.let {
            values["alternativeTransferCharacteristics"] = it
        }
        lumaBitDepth?.let { values["lumaBitDepth"] = it }
        chromaBitDepth?.let { values["chromaBitDepth"] = it }
        hdrStaticInfoPresent?.let { values["hdrStaticInfoPresent"] = it }
        hdrStaticInfoByteLength?.let { values["hdrStaticInfoByteLength"] = it }
        hdrStaticInfoParseError?.let { values["hdrStaticInfoParseError"] = it }
        maxContentLightLevelNits?.let { values["maxContentLightLevelNits"] = it }
        maxFrameAverageLightLevelNits?.let { values["maxFrameAverageLightLevelNits"] = it }
        masteringDisplayColorVolumePresent?.let {
            values["masteringDisplayColorVolumePresent"] = it
        }
        masteringDisplayColorVolumeByteLength?.let {
            values["masteringDisplayColorVolumeByteLength"] = it
        }
        masteringDisplayColorVolumeParseError?.let {
            values["masteringDisplayColorVolumeParseError"] = it
        }
        masteringDisplayPrimary0?.let { values["masteringDisplayPrimary0"] = it.toMap() }
        masteringDisplayPrimary1?.let { values["masteringDisplayPrimary1"] = it.toMap() }
        masteringDisplayPrimary2?.let { values["masteringDisplayPrimary2"] = it.toMap() }
        masteringDisplayWhitePoint?.let { values["masteringDisplayWhitePoint"] = it.toMap() }
        masteringDisplayMaxLuminanceNits?.let { values["masteringDisplayMaxLuminanceNits"] = it }
        masteringDisplayMinLuminanceNits?.let { values["masteringDisplayMinLuminanceNits"] = it }
        dolbyVisionCodec?.let { values["dolbyVisionCodec"] = it }
        dolbyVisionProfile?.let { values["dolbyVisionProfile"] = it }
        dolbyVisionLevel?.let { values["dolbyVisionLevel"] = it }
        dolbyVisionCompatibility?.let { values["dolbyVisionCompatibility"] = it }
        dolbyVisionProfileFamily?.let { values["dolbyVisionProfileFamily"] = it }
        dolbyVisionBaseLayer?.let { values["dolbyVisionBaseLayer"] = it }
        dolbyVisionFallbackTarget?.let { values["dolbyVisionFallbackTarget"] = it }
        dolbyVisionBaseLayerEvidence?.let { values["dolbyVisionBaseLayerEvidence"] = it }
        dolbyVisionBaseLayerTransferFunction?.let { values["dolbyVisionBaseLayerTransferFunction"] = it }
    }

private fun VesperHdrChromaticityPoint.toMap(): Map<String, Double> =
    mapOf("x" to x, "y" to y)

internal fun VesperPlaybackCapabilityProbeStatus.toWireName(): String =
    wireName

private val VesperPlaybackCapabilityProbeStatus.wireName: String
    get() =
        when (this) {
            VesperPlaybackCapabilityProbeStatus.Supported -> "supported"
            VesperPlaybackCapabilityProbeStatus.FallbackRequired -> "fallbackRequired"
            VesperPlaybackCapabilityProbeStatus.Unsupported -> "unsupported"
            VesperPlaybackCapabilityProbeStatus.Unknown -> "unknown"
        }

private val VesperPlaybackCodecFamily.wireName: String
    get() =
        when (this) {
            VesperPlaybackCodecFamily.H264 -> "h264"
            VesperPlaybackCodecFamily.Hevc -> "hevc"
            VesperPlaybackCodecFamily.Av1 -> "av1"
            VesperPlaybackCodecFamily.Vvc -> "vvc"
            VesperPlaybackCodecFamily.Unknown -> "unknown"
        }

private val VesperPlaybackCapabilityOutputFormat.wireName: String
    get() =
        when (this) {
            VesperPlaybackCapabilityOutputFormat.Nv12 -> "nv12"
            VesperPlaybackCapabilityOutputFormat.P010 -> "p010"
            VesperPlaybackCapabilityOutputFormat.SurfaceOpaque -> "surfaceOpaque"
            VesperPlaybackCapabilityOutputFormat.Unknown -> "unknown"
        }

internal fun VesperPlaybackCapabilityHdrKind.toWireName(): String =
    wireName

private val VesperPlaybackCapabilityHdrKind.wireName: String
    get() =
        when (this) {
            VesperPlaybackCapabilityHdrKind.None -> "none"
            VesperPlaybackCapabilityHdrKind.Hdr10 -> "hdr10"
            VesperPlaybackCapabilityHdrKind.Hlg -> "hlg"
            VesperPlaybackCapabilityHdrKind.DolbyVision -> "dolbyVision"
            VesperPlaybackCapabilityHdrKind.Unknown -> "unknown"
        }

internal fun VesperPlaybackCapabilityDolbyVisionMode.toWireName(): String =
    wireName

private val VesperPlaybackCapabilityDolbyVisionMode.wireName: String
    get() =
        when (this) {
            VesperPlaybackCapabilityDolbyVisionMode.None -> "none"
            VesperPlaybackCapabilityDolbyVisionMode.FullChainCandidate -> "fullChainCandidate"
            VesperPlaybackCapabilityDolbyVisionMode.CompatibleBaseLayer -> "compatibleBaseLayer"
            VesperPlaybackCapabilityDolbyVisionMode.Unsupported -> "unsupported"
        }

internal fun VesperPlaybackCapabilityConfidence.toWireName(): String =
    wireName

private val VesperPlaybackCapabilityConfidence.wireName: String
    get() =
        when (this) {
            VesperPlaybackCapabilityConfidence.CodecOnly -> "codecOnly"
            VesperPlaybackCapabilityConfidence.SourceMetadata -> "sourceMetadata"
            VesperPlaybackCapabilityConfidence.SessionProbe -> "sessionProbe"
        }

internal fun VesperRecommendedPlaybackPath.toWireName(): String =
    wireName

private val VesperRecommendedPlaybackPath.wireName: String
    get() =
        when (this) {
            VesperRecommendedPlaybackPath.NativeFramePipeline -> "nativeFramePipeline"
            VesperRecommendedPlaybackPath.SystemPlayer -> "systemPlayer"
        }

private fun Map<String, Any?>.firstString(vararg keys: String): String? =
    keys.firstNotNullOfOrNull(::stringValue)

private fun Map<String, Any?>.firstInt(vararg keys: String): Int? =
    keys.firstNotNullOfOrNull(::intValue)

private fun Map<String, Any?>.stringValue(key: String): String? =
    (this[key] as? String)?.takeIf(String::isNotEmpty)

private fun Map<String, Any?>.boolValue(key: String): Boolean? =
    when (val value = this[key]) {
        is Boolean -> value
        is String -> value.toBooleanStrictOrNull()
        else -> null
    }

private fun Map<String, Any?>.intValue(key: String): Int? =
    when (val value = this[key]) {
        is Number -> value.toInt()
        is String -> value.toIntOrNull()
        else -> null
    }

private fun Map<String, Any?>.doubleValue(key: String): Double? =
    when (val value = this[key]) {
        is Number -> value.toDouble()
        is String -> value.toDoubleOrNull()
        else -> null
    }?.takeIf(Double::isFinite)

private fun Map<String, Any?>.chromaticityPoint(key: String): Map<String, Double>? {
    val value = this[key] as? String ?: return null
    val parts = value.split(',')
    if (parts.size != 2) {
        return null
    }
    val x = parts[0].trim().toDoubleOrNull() ?: return null
    val y = parts[1].trim().toDoubleOrNull() ?: return null
    return mapOf("x" to x, "y" to y)
}

internal fun Throwable.toDownloadErrorMap(): Map<String, Any?> {
    if (this is VesperPlayerUnsupportedOperation) {
        val errorDetails = mapOf("exception" to this::class.java.name) + details
        return mapOf(
            "code" to "unsupported",
            "category" to "capability",
            "retriable" to false,
            "message" to (message ?: toString()),
            "details" to errorDetails,
        )
    }
    return mapOf(
        "code" to "backendFailure",
        "category" to "platform",
        "retriable" to false,
        "message" to (message ?: toString()),
        "details" to mapOf(
            "exception" to this::class.java.name,
        ),
    )
}
