package io.github.ikaros.vesper.player.android

import android.content.Context
import android.os.Handler
import android.os.HandlerThread
import android.os.Looper
import android.os.SystemClock
import android.util.Log
import android.view.Surface
import android.view.ViewGroup
import androidx.media3.common.C
import androidx.media3.common.ColorInfo
import androidx.media3.common.Format
import androidx.media3.common.MediaItem
import androidx.media3.common.MimeTypes
import androidx.media3.common.PlaybackException
import androidx.media3.common.PlaybackParameters
import androidx.media3.common.Player
import androidx.media3.common.Timeline
import androidx.media3.common.TrackGroup
import androidx.media3.common.TrackSelectionOverride
import androidx.media3.common.TrackSelectionParameters
import androidx.media3.common.Tracks
import androidx.media3.common.VideoSize
import androidx.media3.common.util.UnstableApi
import androidx.media3.database.StandaloneDatabaseProvider
import androidx.media3.datasource.DefaultDataSource
import androidx.media3.datasource.DefaultHttpDataSource
import androidx.media3.datasource.DataSpec
import androidx.media3.datasource.cache.CacheDataSource
import androidx.media3.datasource.cache.LeastRecentlyUsedCacheEvictor
import androidx.media3.datasource.cache.SimpleCache
import androidx.media3.exoplayer.DefaultLoadControl
import androidx.media3.exoplayer.DefaultRenderersFactory
import androidx.media3.exoplayer.DecoderReuseEvaluation
import androidx.media3.exoplayer.ExoPlaybackException
import androidx.media3.exoplayer.ExoPlayer
import androidx.media3.exoplayer.analytics.AnalyticsListener
import androidx.media3.exoplayer.hls.playlist.HlsPlaylistTracker
import androidx.media3.exoplayer.source.DefaultMediaSourceFactory
import androidx.media3.exoplayer.upstream.DefaultLoadErrorHandlingPolicy
import androidx.media3.exoplayer.upstream.LoadErrorHandlingPolicy.LoadErrorInfo
import java.io.File
import java.net.URI
import java.util.concurrent.atomic.AtomicBoolean
import kotlin.math.absoluteValue
import kotlin.math.pow
import kotlin.math.roundToLong
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import org.json.JSONArray
import org.json.JSONObject

internal data class AndroidRuntimeHdrEvidence(
    val hdrKind: String,
    val diagnostics: Map<String, Any?>,
) {
    val metadata: VesperPlaybackCapabilityHdrMetadata? =
        VesperPlaybackCapabilityProbe.buildHdrMetadata(
            hdrKind = hdrKind.toPlaybackCapabilityHdrKind(),
            dolbyVisionMode = diagnostics.dolbyVisionMode(hdrKind),
            diagnostics = diagnostics.stringDiagnostics(),
        )

    fun capabilityWarningPayload(): Map<String, Any?> =
        basePayload(
            message =
                "HDR and Dolby Vision content uses system playback; SDK-managed native-frame presentation is SDR-only.",
        )

    fun failureHintPayload(
        errorCodeName: String,
        classified: NativePlaybackError? = null,
        runtimeSessionProbe: AndroidRuntimeSessionProbeSnapshot? = null,
    ): Map<String, Any?> =
        basePayload(
            message =
                "Playback failed after an HDR/Dolby Vision runtime format was observed; this may be an HDR capability issue.",
            extras =
                linkedMapOf<String, Any?>(
                    "likelyHdrCapabilityIssue" to true,
                    "confidence" to "sessionProbe",
                    "errorCode" to errorCodeName,
                ).apply {
                    classified?.capabilityFailureCause?.let {
                        put("capabilityFailureCause", it.wireName)
                    }
                    classified?.capabilityFailureAxis?.let {
                        put("capabilityFailureAxis", it.wireName)
                    }
                    putAll(classified?.causeEvidence?.diagnostics().orEmpty())
                    putAll(classified?.causeEvidence.runtimeFormatConvergenceDiagnostics(diagnostics))
                    putAll(runtimeSessionProbe?.diagnostics.orEmpty())
                    putAll(runtimeSessionProbe?.runtimeFormatConvergenceDiagnostics(diagnostics).orEmpty())
                },
        )

    private fun basePayload(
        message: String,
        extras: Map<String, Any?> = emptyMap(),
    ): Map<String, Any?> =
        linkedMapOf<String, Any?>(
            "reason" to "hdrNativeFrameUnsupported",
            "recommendedPlaybackPath" to "systemPlayer",
            "hdrKind" to hdrKind,
            "message" to message,
        )
            .apply {
                putAll(extras)
                putAll(metadata?.runtimeDiagnostics().orEmpty())
                putAll(diagnostics)
            }
}

internal data class AndroidRuntimeSessionProbeSnapshot(
    val result: VesperPlaybackCapabilityProbeResult,
) {
    val diagnostics: Map<String, Any?> =
        linkedMapOf<String, Any?>(
            "runtimeSessionProbeStatus" to result.status.runtimeWireName,
            "runtimeSessionProbeRecommendedPlaybackPath" to result.recommendedPlaybackPath.runtimeWireName,
            "runtimeSessionProbeConfidence" to result.confidence.runtimeWireName,
            "runtimeSessionProbeHdrKind" to result.hdrKind.runtimeWireName,
            "runtimeSessionProbeDolbyVisionMode" to result.dolbyVisionMode.runtimeWireName,
            "runtimeSessionProbeMissingCapabilities" to result.missingCapabilities.joinToString(","),
        ).apply {
            result.diagnostics["codecFormatSupported"]?.let {
                put("runtimeSessionProbeCodecFormatSupported", it)
            }
            result.diagnostics["codecFormatMissingCapability"]?.let {
                put("runtimeSessionProbeCodecFormatMissingCapability", it)
            }
            result.diagnostics["codecFormatSampleMimeType"]?.let {
                put("runtimeSessionProbeCodecFormatSampleMimeType", it)
            }
            result.diagnostics["codecFormatCodecs"]?.let {
                put("runtimeSessionProbeCodecFormatCodecs", it)
            }
            result.diagnostics["codecFormatWidth"]?.let {
                put("runtimeSessionProbeCodecFormatWidth", it)
            }
            result.diagnostics["codecFormatHeight"]?.let {
                put("runtimeSessionProbeCodecFormatHeight", it)
            }
            result.diagnostics["codecFormatFrameRate"]?.let {
                put("runtimeSessionProbeCodecFormatFrameRate", it)
            }
            result.diagnostics["displayHdrSupported"]?.let {
                put("runtimeSessionProbeDisplayHdrSupported", it)
            }
            result.diagnostics["displayFrameRateSupported"]?.let {
                put("runtimeSessionProbeDisplayFrameRateSupported", it)
            }
        }

    fun runtimeFormatConvergenceDiagnostics(
        runtimeDiagnostics: Map<String, Any?>,
    ): Map<String, Any?> {
        val output = linkedMapOf<String, Any?>()
        val probeDiagnostics = result.diagnostics
        compareString(
            output = output,
            key = "runtimeSessionProbeCodecFormatMimeMatchesRuntime",
            left = probeDiagnostics["codecFormatSampleMimeType"],
            right = runtimeDiagnostics.stringValue("runtimeFormatSampleMimeType"),
        )
        compareString(
            output = output,
            key = "runtimeSessionProbeCodecFormatCodecsMatchRuntime",
            left = probeDiagnostics["codecFormatCodecs"],
            right = runtimeDiagnostics.stringValue("runtimeFormatCodecs"),
        )
        compareInt(
            output = output,
            key = "runtimeSessionProbeCodecFormatSizeMatchesRuntime",
            leftWidth = probeDiagnostics["codecFormatWidth"]?.toIntOrNull(),
            leftHeight = probeDiagnostics["codecFormatHeight"]?.toIntOrNull(),
            rightWidth = runtimeDiagnostics.intValue("runtimeFormatWidth"),
            rightHeight = runtimeDiagnostics.intValue("runtimeFormatHeight"),
        )
        compareFloat(
            output = output,
            key = "runtimeSessionProbeCodecFormatFrameRateMatchesRuntime",
            left = probeDiagnostics["codecFormatFrameRate"]?.toFloatOrNull(),
            right = runtimeDiagnostics.floatValue("runtimeFormatFrameRate"),
        )
        return output
    }

    private fun compareString(
        output: MutableMap<String, Any?>,
        key: String,
        left: String?,
        right: String?,
    ) {
        if (left != null && right != null) {
            output[key] = (left == right).toString()
        }
    }

    private fun compareInt(
        output: MutableMap<String, Any?>,
        key: String,
        leftWidth: Int?,
        leftHeight: Int?,
        rightWidth: Int?,
        rightHeight: Int?,
    ) {
        if (leftWidth != null && leftHeight != null && rightWidth != null && rightHeight != null) {
            output[key] = (leftWidth == rightWidth && leftHeight == rightHeight).toString()
        }
    }

    private fun compareFloat(
        output: MutableMap<String, Any?>,
        key: String,
        left: Float?,
        right: Float?,
    ) {
        if (left != null && right != null) {
            output[key] = left.nearlyEquals(right).toString()
        }
    }
}

internal val VesperPlaybackCapabilityProbeStatus.runtimeWireName: String
    get() =
        when (this) {
            VesperPlaybackCapabilityProbeStatus.Supported -> "supported"
            VesperPlaybackCapabilityProbeStatus.FallbackRequired -> "fallbackRequired"
            VesperPlaybackCapabilityProbeStatus.Unsupported -> "unsupported"
            VesperPlaybackCapabilityProbeStatus.Unknown -> "unknown"
        }

internal val VesperRecommendedPlaybackPath.runtimeWireName: String
    get() =
        when (this) {
            VesperRecommendedPlaybackPath.NativeFramePipeline -> "nativeFramePipeline"
            VesperRecommendedPlaybackPath.SystemPlayer -> "systemPlayer"
        }

internal val VesperPlaybackCapabilityConfidence.runtimeWireName: String
    get() =
        when (this) {
            VesperPlaybackCapabilityConfidence.CodecOnly -> "codecOnly"
            VesperPlaybackCapabilityConfidence.SourceMetadata -> "sourceMetadata"
            VesperPlaybackCapabilityConfidence.SessionProbe -> "sessionProbe"
        }

internal fun String.toPlaybackCapabilityHdrKind(): VesperPlaybackCapabilityHdrKind =
    when (this) {
        "hdr10" -> VesperPlaybackCapabilityHdrKind.Hdr10
        "hlg" -> VesperPlaybackCapabilityHdrKind.Hlg
        "dolbyVision" -> VesperPlaybackCapabilityHdrKind.DolbyVision
        else -> VesperPlaybackCapabilityHdrKind.Unknown
    }

internal fun Map<String, Any?>.dolbyVisionMode(hdrKind: String): VesperPlaybackCapabilityDolbyVisionMode {
    stringValue("runtimeFormatCodecs")
        .detectDolbyVisionCodecInfo()
        ?.dolbyVisionMode
        ?.let { return it }
    stringValue("dolbyVisionCodec")
        .detectDolbyVisionCodecInfo()
        ?.dolbyVisionMode
        ?.let { return it }
    return if (hdrKind == "dolbyVision") {
        VesperPlaybackCapabilityDolbyVisionMode.Unsupported
    } else {
        VesperPlaybackCapabilityDolbyVisionMode.None
    }
}

internal fun Map<String, Any?>.stringDiagnostics(): Map<String, String> =
    mapNotNull { (key, value) ->
        value?.toString()?.let { key to it }
    }.toMap()

internal fun VesperPlaybackCapabilityHdrMetadata.runtimeDiagnostics(): Map<String, Any?> {
    val values =
        linkedMapOf<String, Any?>().also { output ->
            hdrKind?.let { output["hdrKind"] = it.runtimeWireName }
            dolbyVisionMode?.let { output["dolbyVisionMode"] = it.runtimeWireName }
            probe?.let { output["probe"] = it }
            codec?.let { output["codec"] = it }
            sampleMimeType?.let { output["sampleMimeType"] = it }
            colorPrimaries?.let { output["colorPrimaries"] = it }
            colorSpace?.let { output["colorSpace"] = it }
            colorRange?.let { output["colorRange"] = it }
            transferFunction?.let { output["transferFunction"] = it }
            yCbCrMatrix?.let { output["yCbCrMatrix"] = it }
            alternativeTransferCharacteristics?.let {
                output["alternativeTransferCharacteristics"] = it
            }
            lumaBitDepth?.let { output["lumaBitDepth"] = it }
            chromaBitDepth?.let { output["chromaBitDepth"] = it }
            hdrStaticInfoPresent?.let { output["hdrStaticInfoPresent"] = it }
            hdrStaticInfoByteLength?.let { output["hdrStaticInfoByteLength"] = it }
            hdrStaticInfoParseError?.let { output["hdrStaticInfoParseError"] = it }
            maxContentLightLevelNits?.let { output["maxContentLightLevelNits"] = it }
            maxFrameAverageLightLevelNits?.let { output["maxFrameAverageLightLevelNits"] = it }
            masteringDisplayColorVolumePresent?.let {
                output["masteringDisplayColorVolumePresent"] = it
            }
            masteringDisplayColorVolumeByteLength?.let {
                output["masteringDisplayColorVolumeByteLength"] = it
            }
            masteringDisplayColorVolumeParseError?.let {
                output["masteringDisplayColorVolumeParseError"] = it
            }
            masteringDisplayPrimary0?.let { output["masteringDisplayPrimary0"] = it.runtimeMap() }
            masteringDisplayPrimary1?.let { output["masteringDisplayPrimary1"] = it.runtimeMap() }
            masteringDisplayPrimary2?.let { output["masteringDisplayPrimary2"] = it.runtimeMap() }
            masteringDisplayWhitePoint?.let { output["masteringDisplayWhitePoint"] = it.runtimeMap() }
            masteringDisplayMaxLuminanceNits?.let { output["masteringDisplayMaxLuminanceNits"] = it }
            masteringDisplayMinLuminanceNits?.let { output["masteringDisplayMinLuminanceNits"] = it }
            dolbyVisionCodec?.let { output["dolbyVisionCodec"] = it }
            dolbyVisionProfile?.let { output["dolbyVisionProfile"] = it }
            dolbyVisionLevel?.let { output["dolbyVisionLevel"] = it }
            dolbyVisionCompatibility?.let { output["dolbyVisionCompatibility"] = it }
            dolbyVisionProfileFamily?.let { output["dolbyVisionProfileFamily"] = it }
            dolbyVisionBaseLayer?.let { output["dolbyVisionBaseLayer"] = it }
            dolbyVisionFallbackTarget?.let { output["dolbyVisionFallbackTarget"] = it }
            dolbyVisionBaseLayerEvidence?.let { output["dolbyVisionBaseLayerEvidence"] = it }
            dolbyVisionBaseLayerTransferFunction?.let { output["dolbyVisionBaseLayerTransferFunction"] = it }
        }
    return values.takeIf { it.isNotEmpty() }?.let { mapOf("hdrMetadata" to it) }.orEmpty()
}

internal val VesperPlaybackCapabilityHdrKind.runtimeWireName: String
    get() =
        when (this) {
            VesperPlaybackCapabilityHdrKind.None -> "none"
            VesperPlaybackCapabilityHdrKind.Hdr10 -> "hdr10"
            VesperPlaybackCapabilityHdrKind.Hlg -> "hlg"
            VesperPlaybackCapabilityHdrKind.DolbyVision -> "dolbyVision"
            VesperPlaybackCapabilityHdrKind.Unknown -> "unknown"
        }

internal val VesperPlaybackCapabilityDolbyVisionMode.runtimeWireName: String
    get() =
        when (this) {
            VesperPlaybackCapabilityDolbyVisionMode.None -> "none"
            VesperPlaybackCapabilityDolbyVisionMode.FullChainCandidate -> "fullChainCandidate"
            VesperPlaybackCapabilityDolbyVisionMode.CompatibleBaseLayer -> "compatibleBaseLayer"
            VesperPlaybackCapabilityDolbyVisionMode.Unsupported -> "unsupported"
        }

internal fun VesperHdrChromaticityPoint.runtimeMap(): Map<String, Double> =
    mapOf("x" to x, "y" to y)

internal fun Map<String, Any?>.stringValue(key: String): String? =
    when (val value = this[key]) {
        is String -> value.takeIf(String::isNotEmpty)
        is Number, is Boolean -> value.toString()
        else -> null
    }

internal fun Map<String, Any?>.intValue(key: String): Int? =
    when (val value = this[key]) {
        is Number -> value.toInt()
        is String -> value.toIntOrNull()
        else -> null
    }

internal fun Map<String, Any?>.floatValue(key: String): Float? =
    when (val value = this[key]) {
        is Number -> value.toFloat()
        is String -> value.toFloatOrNull()
        else -> null
    }

internal fun Float.nearlyEquals(other: Float): Boolean =
    kotlin.math.abs(this - other) <= 0.01f

internal fun Format.androidRuntimeHdrEvidence(): AndroidRuntimeHdrEvidence? {
    val hdrKind = detectRuntimeHdrKind() ?: return null
    return AndroidRuntimeHdrEvidence(
        hdrKind = hdrKind,
        diagnostics = androidRuntimeHdrDiagnostics(),
    )
}

internal fun Format.detectRuntimeHdrKind(): String? {
    val codec = codecs?.lowercase().orEmpty()
    if (codec.split(',').any { value ->
            val normalized = value.trim().removePrefix("video/")
            normalized.startsWith("dvh1") || normalized.startsWith("dvhe")
        }
    ) {
        return "dolbyVision"
    }
    return when (colorInfo?.colorTransfer) {
        C.COLOR_TRANSFER_ST2084 -> "hdr10"
        C.COLOR_TRANSFER_HLG -> "hlg"
        else -> null
    }
}

internal fun Format.androidRuntimeHdrDiagnostics(): Map<String, Any?> {
    val diagnostics = linkedMapOf<String, Any?>(
        "runtimeFormatHdrMetadataProbe" to "media3FormatColorInfo",
    )
    codecs?.takeIf(String::isNotBlank)?.let {
        diagnostics["runtimeFormatCodecs"] = it
        it.detectDolbyVisionCodecInfo()?.diagnostics?.let(diagnostics::putAll)
    }
    sampleMimeType?.takeIf(String::isNotBlank)?.let {
        diagnostics["runtimeFormatSampleMimeType"] = it
    }
    width.takeIf { it != Format.NO_VALUE && it > 0 }?.let {
        diagnostics["runtimeFormatWidth"] = it
    }
    height.takeIf { it != Format.NO_VALUE && it > 0 }?.let {
        diagnostics["runtimeFormatHeight"] = it
    }
    frameRate.takeIf { it.isFinite() && it > 0f }?.let {
        diagnostics["runtimeFormatFrameRate"] = it.toString()
    }
    colorInfo?.appendRuntimeColorDiagnosticsTo(diagnostics)
    diagnostics.putAll(diagnostics.stringDiagnostics().withDolbyVisionProfile8Refinement())
    return diagnostics
}

internal fun ColorInfo.appendRuntimeColorDiagnosticsTo(diagnostics: MutableMap<String, Any?>) {
    diagnostics["runtimeFormatColorSpace"] = colorSpace.toRuntimeColorSpaceName()
    diagnostics["runtimeFormatColorRange"] = colorRange.toRuntimeColorRangeName()
    diagnostics["runtimeFormatColorTransfer"] = colorTransfer.toRuntimeColorTransferName()
    lumaBitdepth.takeIf { it != Format.NO_VALUE && it > 0 }?.let {
        diagnostics["runtimeFormatLumaBitDepth"] = it
    }
    chromaBitdepth.takeIf { it != Format.NO_VALUE && it > 0 }?.let {
        diagnostics["runtimeFormatChromaBitDepth"] = it
    }
    hdrStaticInfo?.appendRuntimeHdrStaticInfoDiagnosticsTo(diagnostics)
}

internal fun ByteArray.appendRuntimeHdrStaticInfoDiagnosticsTo(diagnostics: MutableMap<String, Any?>) {
    diagnostics["runtimeFormatHdrStaticInfoPresent"] = true
    diagnostics["runtimeFormatHdrStaticInfoByteLength"] = size
    if (size < 25) {
        diagnostics["runtimeFormatHdrStaticInfoParseError"] = "tooShort"
        return
    }
    diagnostics["runtimeFormatMaxContentLightLevelNits"] = readUnsignedShortBigEndian(21)
    diagnostics["runtimeFormatMaxFrameAverageLightLevelNits"] = readUnsignedShortBigEndian(23)
}

internal fun ByteArray.readUnsignedShortBigEndian(offset: Int): Int =
    ((this[offset].toInt() and 0xFF) shl 8) or (this[offset + 1].toInt() and 0xFF)

internal fun Int.toRuntimeColorSpaceName(): String =

    when (this) {
        Format.NO_VALUE -> "unknown"
        C.COLOR_SPACE_BT601 -> "bt601"
        C.COLOR_SPACE_BT709 -> "bt709"
        C.COLOR_SPACE_BT2020 -> "bt2020"
        else -> "unknown($this)"
    }

