package io.github.ikaros.vesper.player.android

import android.util.Log
import androidx.media3.common.MimeTypes
import androidx.media3.exoplayer.mediacodec.MediaCodecInfo
import androidx.media3.exoplayer.mediacodec.MediaCodecSelector
import java.util.concurrent.ConcurrentHashMap

internal enum class VesperAndroidVideoCodecFamily {
    Vvc,
    Av1,
    Hevc,
    Avc,
    Unknown,
}

internal object VesperHardwareMediaCodecSelector : MediaCodecSelector {
    private val decoderDiagnosticsCache = ConcurrentHashMap<String, Map<String, String>>()

    override fun getDecoderInfos(
        mimeType: String,
        requiresSecureDecoder: Boolean,
        requiresTunnelingDecoder: Boolean,
    ): List<MediaCodecInfo> {
        val decoders =
            MediaCodecSelector.DEFAULT.getDecoderInfos(
                mimeType,
                requiresSecureDecoder,
                requiresTunnelingDecoder,
            )
        if (!MimeTypes.isVideo(mimeType)) {
            return decoders
        }
        return decoders.filter { decoder ->
            decoder.hardwareAccelerated && !decoder.softwareOnly
        }
    }

    fun hasHardwareDecoder(mimeType: String?): Boolean {
        if (mimeType.isNullOrBlank() || !MimeTypes.isVideo(mimeType)) {
            return false
        }
        return runCatching {
            getDecoderInfos(
                mimeType,
                requiresSecureDecoder = false,
                requiresTunnelingDecoder = false,
            ).isNotEmpty()
        }.onFailure { error ->
            Log.w(TAG, "failed to probe hardware decoder for $mimeType", error)
        }.getOrDefault(false)
    }

    fun preferredHardwareDecoderName(mimeType: String?): String? {
        if (mimeType.isNullOrBlank() || !MimeTypes.isVideo(mimeType)) {
            return null
        }
        return runCatching {
            getDecoderInfos(
                mimeType,
                requiresSecureDecoder = false,
                requiresTunnelingDecoder = false,
            ).firstOrNull()?.name
        }.onFailure { error ->
            Log.w(TAG, "failed to select hardware decoder for $mimeType", error)
        }.getOrNull()
    }

    fun decoderDiagnostics(mimeType: String?): Map<String, String> {
        if (mimeType.isNullOrBlank() || !MimeTypes.isVideo(mimeType)) {
            return mapOf(
                "mimeType" to (mimeType ?: ""),
                "hardwareDecoderCount" to "0",
                "secureHardwareDecoderCount" to "0",
            )
        }
        decoderDiagnosticsCache[mimeType]?.let { return it }
        return runCatching {
            val clearDecoders =
                getDecoderInfos(
                    mimeType,
                    requiresSecureDecoder = false,
                    requiresTunnelingDecoder = false,
                )
            val secureDecoders =
                getDecoderInfos(
                    mimeType,
                    requiresSecureDecoder = true,
                    requiresTunnelingDecoder = false,
                )
            linkedMapOf(
                "mimeType" to mimeType,
                "hardwareDecoderCount" to clearDecoders.size.toString(),
                "secureHardwareDecoderCount" to secureDecoders.size.toString(),
                "hardwareDecoders" to clearDecoders.joinToString(separator = ",") { it.name },
                "secureHardwareDecoders" to secureDecoders.joinToString(separator = ",") { it.name },
            ).also { decoderDiagnosticsCache[mimeType] = it }
        }.onFailure { error ->
            Log.w(TAG, "failed to probe decoder diagnostics for $mimeType", error)
        }.getOrElse { error ->
            mapOf(
                "mimeType" to mimeType,
                "decoderProbeError" to (error.message ?: error::class.java.simpleName),
            )
        }
    }
}

internal fun vesperAndroidVideoCodecFamily(codec: String?): VesperAndroidVideoCodecFamily {
    if (codec.isNullOrBlank()) {
        return VesperAndroidVideoCodecFamily.Unknown
    }
    codec
        .split(',')
        .map { it.trim().lowercase() }
        .filter(String::isNotBlank)
        .forEach { rawCodec ->
            val normalized =
                if (rawCodec.startsWith("video/")) {
                    rawCodec.removePrefix("video/")
                } else {
                    rawCodec
                }
            when {
                normalized.startsWith("vvc1") ||
                    normalized.startsWith("vvi1") ||
                    normalized == "vvc" ||
                    normalized == "h266" -> return VesperAndroidVideoCodecFamily.Vvc
                normalized.startsWith("av01") ||
                    normalized == "av1" -> return VesperAndroidVideoCodecFamily.Av1
                normalized.startsWith("hvc1") ||
                    normalized.startsWith("hev1") ||
                    normalized.startsWith("dvh1") ||
                    normalized.startsWith("dvhe") ||
                    normalized == "hevc" ||
                    normalized == "h265" -> return VesperAndroidVideoCodecFamily.Hevc
                normalized.startsWith("avc1") ||
                    normalized.startsWith("avc3") ||
                    normalized == "avc" ||
                    normalized == "h264" -> return VesperAndroidVideoCodecFamily.Avc
            }
        }
    return VesperAndroidVideoCodecFamily.Unknown
}

private const val TAG = "VesperMediaCodec"

internal fun VesperAndroidVideoCodecFamily.toBenchmarkValue(): String =
    when (this) {
        VesperAndroidVideoCodecFamily.Vvc -> "vvc"
        VesperAndroidVideoCodecFamily.Av1 -> "av1"
        VesperAndroidVideoCodecFamily.Hevc -> "hevc"
        VesperAndroidVideoCodecFamily.Avc -> "avc"
        VesperAndroidVideoCodecFamily.Unknown -> "unknown"
    }
