package io.github.ikaros.vesper.player.android

import android.content.Context
import android.hardware.display.DisplayManager
import android.os.Build
import android.util.Log
import android.view.Display
import androidx.media3.common.Format
import androidx.media3.common.MimeTypes

object VesperAndroidDisplaySessionProbeProvider {
    fun fromContext(context: Context): VesperAndroidSessionProbeProvider =
        VesperAndroidSessionProbeProvider { request ->
            val display = context.primaryDisplayOrNull()
            val codecProbe = context.probeCodecFormatSupport(request)
            if (display == null && codecProbe == null) {
                return@VesperAndroidSessionProbeProvider null
            }
            val diagnostics =
                linkedMapOf(
                    "sessionProbe" to "androidDisplayAndCodecCapabilities",
                )
            val supportedHdrKinds = display?.supportedHdrKinds() ?: emptySet()
            if (display != null) {
                diagnostics[DISPLAY_HDR_PROBE_AVAILABLE_KEY] = "true"
                diagnostics["displayName"] = display.name ?: "unknown"
                display.refreshRateDiagnostics(request)?.let(diagnostics::putAll)
            }
            codecProbe?.appendDiagnosticsTo(diagnostics)
            VesperAndroidSessionProbeResult(
                supportedHdrKinds = supportedHdrKinds,
                diagnostics = diagnostics,
            )
        }
}

internal fun androidCodecFormatForSessionProbe(
    codec: String?,
    codecFamily: VesperPlaybackCodecFamily = codec.toPlaybackCodecFamily(),
): Format? {
    if (codec.isNullOrBlank()) {
        return null
    }
    val sampleMimeType = MimeTypes.getVideoMediaMimeType(codec) ?: codecFamily.androidMimeType ?: return null
    if (!MimeTypes.isVideo(sampleMimeType)) {
        return null
    }
    val builder = Format.Builder()
        .setCodecs(codec)
        .setSampleMimeType(sampleMimeType)
    return builder.build()
}

internal fun androidCodecFormatForSessionProbe(
    request: VesperPlaybackCapabilityProbeRequest,
): Format? {
    val format = androidCodecFormatForSessionProbe(request.codec) ?: return null
    val builder = format.buildUpon()
    request.width?.takeIf { it > 0 }?.let(builder::setWidth)
    request.height?.takeIf { it > 0 }?.let(builder::setHeight)
    request.frameRate?.takeIf { it > 0f }?.let(builder::setFrameRate)
    return builder.build()
}

private data class AndroidCodecFormatProbeResult(
    val sampleMimeType: String,
    val codecs: String?,
    val width: Int?,
    val height: Int?,
    val frameRate: Float?,
    val supported: Boolean,
    val decoderCount: Int,
    val decoderName: String?,
    val unsupportedReason: String?,
) {
    fun appendDiagnosticsTo(diagnostics: MutableMap<String, String>) {
        diagnostics[CODEC_FORMAT_PROBE_AVAILABLE_KEY] = "true"
        diagnostics[CODEC_FORMAT_SAMPLE_MIME_TYPE_KEY] = sampleMimeType
        codecs?.let { diagnostics["codecFormatCodecs"] = it }
        width?.let { diagnostics["codecFormatWidth"] = it.toString() }
        height?.let { diagnostics["codecFormatHeight"] = it.toString() }
        frameRate?.let { diagnostics["codecFormatFrameRate"] = it.toString() }
        diagnostics[CODEC_FORMAT_SUPPORTED_KEY] = supported.toString()
        diagnostics["codecFormatDecoderCount"] = decoderCount.toString()
        decoderName?.let { diagnostics["codecFormatDecoder"] = it }
        unsupportedReason?.let { reason ->
            diagnostics["codecFormatUnsupportedReason"] = reason
            diagnostics[CODEC_FORMAT_MISSING_CAPABILITY_KEY] =
                when (reason) {
                    "noHardwareDecoder" -> "deviceHardwareDecode"
                    "formatRejected" -> "codecProfileLevel"
                    else -> "codecFormatCapability"
                }
        }
    }
}

private fun Context.probeCodecFormatSupport(
    request: VesperPlaybackCapabilityProbeRequest,
): AndroidCodecFormatProbeResult? {
    val format = androidCodecFormatForSessionProbe(request) ?: return null
    val sampleMimeType = format.sampleMimeType ?: return null
    val decoders =
        runCatching {
            VesperHardwareMediaCodecSelector.getDecoderInfos(
                sampleMimeType,
                requiresSecureDecoder = false,
                requiresTunnelingDecoder = false,
            )
        }.onFailure { error ->
            Log.w(SESSION_PROBE_TAG, "failed to query hardware decoder format support for $sampleMimeType", error)
        }.getOrElse {
            return AndroidCodecFormatProbeResult(
                sampleMimeType = sampleMimeType,
                codecs = format.codecs?.takeIf { it.isNotBlank() },
                width = format.codecFormatWidth,
                height = format.codecFormatHeight,
                frameRate = format.codecFormatFrameRate,
                supported = false,
                decoderCount = 0,
                decoderName = null,
                unsupportedReason = "decoderQueryFailed",
            )
        }
    if (decoders.isEmpty()) {
        return AndroidCodecFormatProbeResult(
            sampleMimeType = sampleMimeType,
            codecs = format.codecs?.takeIf { it.isNotBlank() },
            width = format.codecFormatWidth,
            height = format.codecFormatHeight,
            frameRate = format.codecFormatFrameRate,
            supported = false,
            decoderCount = 0,
            decoderName = null,
            unsupportedReason = "noHardwareDecoder",
        )
    }
    val supportedDecoder =
        decoders.firstOrNull { decoder ->
            runCatching {
                decoder.isFormatSupported(this, format)
            }.onFailure { error ->
                Log.w(SESSION_PROBE_TAG, "failed to probe decoder ${decoder.name} for $sampleMimeType", error)
            }.getOrDefault(false)
        }
    return AndroidCodecFormatProbeResult(
        sampleMimeType = sampleMimeType,
        codecs = format.codecs?.takeIf { it.isNotBlank() },
        width = format.codecFormatWidth,
        height = format.codecFormatHeight,
        frameRate = format.codecFormatFrameRate,
        supported = supportedDecoder != null,
        decoderCount = decoders.size,
        decoderName = supportedDecoder?.name,
        unsupportedReason = if (supportedDecoder == null) "formatRejected" else null,
    )
}

private val Format.codecFormatWidth: Int?
    get() = width.takeIf { it != Format.NO_VALUE && it > 0 }

private val Format.codecFormatHeight: Int?
    get() = height.takeIf { it != Format.NO_VALUE && it > 0 }

private val Format.codecFormatFrameRate: Float?
    get() = frameRate.takeIf { it.isFinite() && it > 0f }

internal val VesperPlaybackCodecFamily.androidMimeType: String?
    get() =
        when (this) {
            VesperPlaybackCodecFamily.H264 -> "video/avc"
            VesperPlaybackCodecFamily.Hevc -> "video/hevc"
            VesperPlaybackCodecFamily.Av1 -> "video/av01"
            VesperPlaybackCodecFamily.Vvc -> "video/vvc"
            VesperPlaybackCodecFamily.Unknown -> null
        }

internal fun String?.toPlaybackCodecFamily(): VesperPlaybackCodecFamily =
    when (vesperAndroidVideoCodecFamily(this)) {
        VesperAndroidVideoCodecFamily.Avc -> VesperPlaybackCodecFamily.H264
        VesperAndroidVideoCodecFamily.Hevc -> VesperPlaybackCodecFamily.Hevc
        VesperAndroidVideoCodecFamily.Av1 -> VesperPlaybackCodecFamily.Av1
        VesperAndroidVideoCodecFamily.Vvc -> VesperPlaybackCodecFamily.Vvc
        VesperAndroidVideoCodecFamily.Unknown -> VesperPlaybackCodecFamily.Unknown
    }

internal fun String?.detectHdrKind(): VesperPlaybackCapabilityHdrKind {
    if (isNullOrBlank()) {
        return VesperPlaybackCapabilityHdrKind.None
    }
    val normalizedCodecs =
        split(',')
        .map { it.trim().lowercase() }
        .filter(String::isNotBlank)
        .map { rawCodec ->
            val normalized =
                if (rawCodec.startsWith("video/")) {
                    rawCodec.removePrefix("video/")
                } else {
                    rawCodec
                }
            normalized
        }
    if (normalizedCodecs.any { it.startsWith("dvh1") || it.startsWith("dvhe") || it == "dolbyvision" }) {
        return VesperPlaybackCapabilityHdrKind.DolbyVision
    }
    if (normalizedCodecs.any { it == "hdr10" || it == "hdr10+" || it == "hdr10plus" }) {
        return VesperPlaybackCapabilityHdrKind.Hdr10
    }
    if (normalizedCodecs.any { it == "hlg" }) {
        return VesperPlaybackCapabilityHdrKind.Hlg
    }
    return VesperPlaybackCapabilityHdrKind.None
}

internal fun Map<String, String>.firstString(vararg keys: String): String? =
    keys.firstNotNullOfOrNull(::stringValue)

internal fun Map<String, String>.firstInt(vararg keys: String): Int? =
    keys.firstNotNullOfOrNull(::intValue)

internal fun Map<String, String>.withDolbyVisionProfile8Refinement(): Map<String, String> =
    toMutableMap().also { it.applyDolbyVisionProfile8Refinement() }

internal fun MutableMap<String, String>.applyDolbyVisionProfile8Refinement() {
    if (intValue("dolbyVisionProfile") != 8) {
        return
    }
    val evidence = dolbyVisionProfile8BaseLayerEvidence() ?: return
    put("dolbyVisionCompatibility", evidence.compatibility)
    put("dolbyVisionProfileFamily", "profile8SingleLayerCompatible")
    put("dolbyVisionBaseLayer", evidence.baseLayer)
    put("dolbyVisionFallbackTarget", evidence.fallbackTarget)
    put("dolbyVisionBaseLayerEvidence", evidence.key)
    put("dolbyVisionBaseLayerTransferFunction", evidence.transferFunction)
}

private fun Map<String, String>.dolbyVisionProfile8BaseLayerEvidence(): DolbyVisionProfile8BaseLayerEvidence? =
    listOf(
        "assetVideoTransferFunction",
        "assetVideoAlternativeTransferCharacteristics",
        "runtimeFormatColorTransfer",
    )
        .firstNotNullOfOrNull { key ->
            stringValue(key)?.let { transferFunction ->
                DolbyVisionProfile8BaseLayerEvidence.fromTransferFunction(
                    key = key,
                    transferFunction = transferFunction,
                )
            }
        }

private data class DolbyVisionProfile8BaseLayerEvidence(
    val key: String,
    val transferFunction: String,
    val compatibility: String,
    val baseLayer: String,
    val fallbackTarget: String,
) {
    companion object {
        fun fromTransferFunction(
            key: String,
            transferFunction: String,
        ): DolbyVisionProfile8BaseLayerEvidence? {
            val normalized = transferFunction.lowercase()
            return when {
                normalized.contains("hlg") ||
                    normalized.contains("arib") ||
                    normalized.contains("std-b67") ||
                    normalized.contains("std_b67") ->
                    DolbyVisionProfile8BaseLayerEvidence(
                        key = key,
                        transferFunction = transferFunction,
                        compatibility = "profile8HlgBaseLayer",
                        baseLayer = "hlgBaseLayer",
                        fallbackTarget = "hlgBaseLayerSystemPlayer",
                    )
                normalized.contains("pq") ||
                    normalized.contains("2084") ||
                    normalized.contains("st2084") ||
                    normalized.contains("st_2084") ->
                    DolbyVisionProfile8BaseLayerEvidence(
                        key = key,
                        transferFunction = transferFunction,
                        compatibility = "profile8Hdr10BaseLayer",
                        baseLayer = "hdr10BaseLayer",
                        fallbackTarget = "hdr10BaseLayerSystemPlayer",
                    )
                normalized == "sdr" ||
                    normalized == "srgb" ||
                    normalized.contains("bt709") ||
                    normalized.contains("bt.709") ||
                    normalized.contains("itu_r_709") ||
                    normalized.contains("gamma") ->
                    DolbyVisionProfile8BaseLayerEvidence(
                        key = key,
                        transferFunction = transferFunction,
                        compatibility = "profile8SdrBaseLayer",
                        baseLayer = "sdrBaseLayer",
                        fallbackTarget = "sdrBaseLayerSystemPlayer",
                    )
                else -> null
            }
        }
    }
}

internal fun Map<String, String>.stringValue(key: String): String? =
    this[key]?.takeIf(String::isNotEmpty)

internal fun Map<String, String>.boolValue(key: String): Boolean? =
    when (this[key]) {
        "true" -> true
        "false" -> false
        else -> null
    }

internal fun Map<String, String>.intValue(key: String): Int? =
    stringValue(key)?.toIntOrNull()

internal fun Map<String, String>.doubleValue(key: String): Double? =
    stringValue(key)?.toDoubleOrNull()?.takeIf(Double::isFinite)

internal fun Map<String, String>.chromaticityPoint(key: String): VesperHdrChromaticityPoint? {
    val value = stringValue(key) ?: return null
    val parts = value.split(',')
    if (parts.size != 2) {
        return null
    }
    val x = parts[0].trim().toDoubleOrNull() ?: return null
    val y = parts[1].trim().toDoubleOrNull() ?: return null
    return VesperHdrChromaticityPoint(x = x, y = y)
}

internal fun String?.detectDolbyVisionCodecInfo(): AndroidDolbyVisionCodecInfo? {
    if (isNullOrBlank()) {
        return null
    }
    val codec =
        split(',')
            .map { it.trim().lowercase().removePrefix("video/") }
            .firstOrNull { it.startsWith("dvh1") || it.startsWith("dvhe") || it == "dolbyvision" }
            ?: return null
    val match = Regex("^(?:dvh1|dvhe)\\.(\\d{1,2})(?:\\.(\\d{1,2}))?.*").matchEntire(codec)
    return AndroidDolbyVisionCodecInfo(
        codec = codec,
        profile = match?.groupValues?.getOrNull(1)?.takeIf(String::isNotBlank)?.toIntOrNull(),
        level = match?.groupValues?.getOrNull(2)?.takeIf(String::isNotBlank)?.toIntOrNull(),
    )
}

internal val VesperPlayerSourceKind.wireName: String
    get() =
        when (this) {
            VesperPlayerSourceKind.Local -> "local"
            VesperPlayerSourceKind.Remote -> "remote"
        }

internal val VesperPlayerSourceProtocol.wireName: String
    get() =
        when (this) {
            VesperPlayerSourceProtocol.Unknown -> "unknown"
            VesperPlayerSourceProtocol.File -> "file"
            VesperPlayerSourceProtocol.Content -> "content"
            VesperPlayerSourceProtocol.Progressive -> "progressive"
            VesperPlayerSourceProtocol.Hls -> "hls"
            VesperPlayerSourceProtocol.Dash -> "dash"
        }

internal const val DISPLAY_HDR_PROBE_AVAILABLE_KEY = "displayHdrProbeAvailable"
internal const val DISPLAY_FRAME_RATE_SUPPORTED_KEY = "displayFrameRateSupported"
internal const val CODEC_FORMAT_PROBE_AVAILABLE_KEY = "codecFormatProbeAvailable"
internal const val CODEC_FORMAT_SAMPLE_MIME_TYPE_KEY = "codecFormatSampleMimeType"
internal const val CODEC_FORMAT_SUPPORTED_KEY = "codecFormatSupported"
internal const val CODEC_FORMAT_MISSING_CAPABILITY_KEY = "codecFormatMissingCapability"
private const val SESSION_PROBE_TAG = "VesperSessionProbe"

private fun Context.primaryDisplayOrNull(): Display? {
    if (Build.VERSION.SDK_INT >= 30) {
        runCatching { display }
            .getOrNull()
            ?.let { return it }
    }
    @Suppress("DEPRECATION")
    return (getSystemService(Context.DISPLAY_SERVICE) as? DisplayManager)
        ?.getDisplay(Display.DEFAULT_DISPLAY)
}

private fun Display.supportedHdrKinds(): Set<VesperPlaybackCapabilityHdrKind> {
    if (Build.VERSION.SDK_INT < 24) {
        return emptySet()
    }
    val kinds = linkedSetOf<VesperPlaybackCapabilityHdrKind>()
    hdrCapabilities.supportedHdrTypesCompat().forEach { hdrType ->
        when (hdrType) {
                Display.HdrCapabilities.HDR_TYPE_DOLBY_VISION ->
                    kinds += VesperPlaybackCapabilityHdrKind.DolbyVision
                Display.HdrCapabilities.HDR_TYPE_HDR10 ->
                    kinds += VesperPlaybackCapabilityHdrKind.Hdr10
                Display.HdrCapabilities.HDR_TYPE_HLG ->
                    kinds += VesperPlaybackCapabilityHdrKind.Hlg
        }
    }
    return kinds
}

@Suppress("DEPRECATION")
private fun Display.HdrCapabilities.supportedHdrTypesCompat(): IntArray = supportedHdrTypes

private fun Display.refreshRateDiagnostics(
    request: VesperPlaybackCapabilityProbeRequest,
): Map<String, String>? =
    refreshRateDiagnostics(
        requestedFrameRate = request.frameRate,
        displayRefreshRate = mode?.refreshRate?.takeIf { it > 0f } ?: refreshRate.takeIf { it > 0f },
    )

internal fun refreshRateDiagnostics(
    requestedFrameRate: Float?,
    displayRefreshRate: Float?,
): Map<String, String>? {
    val refreshRate = displayRefreshRate?.takeIf { it.isFinite() && it > 0f } ?: return null
    return buildMap {
        put("displayRefreshRate", refreshRate.toString())
        val frameRate = requestedFrameRate?.takeIf { it.isFinite() && it > 0f } ?: return@buildMap
        put("requestedFrameRate", frameRate.toString())
        put(DISPLAY_FRAME_RATE_SUPPORTED_KEY, (frameRate <= refreshRate + 0.01f).toString())
    }
}
