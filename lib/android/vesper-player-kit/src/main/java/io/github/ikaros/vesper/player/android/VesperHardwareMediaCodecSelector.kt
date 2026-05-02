package io.github.ikaros.vesper.player.android

import androidx.media3.common.MimeTypes
import androidx.media3.exoplayer.mediacodec.MediaCodecInfo
import androidx.media3.exoplayer.mediacodec.MediaCodecSelector

internal enum class VesperAndroidVideoCodecFamily {
    Vvc,
    Av1,
    Hevc,
    Avc,
    Unknown,
}

internal object VesperHardwareMediaCodecSelector : MediaCodecSelector {
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
        }.getOrDefault(false)
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

internal fun VesperAndroidVideoCodecFamily.toBenchmarkValue(): String =
    when (this) {
        VesperAndroidVideoCodecFamily.Vvc -> "vvc"
        VesperAndroidVideoCodecFamily.Av1 -> "av1"
        VesperAndroidVideoCodecFamily.Hevc -> "hevc"
        VesperAndroidVideoCodecFamily.Avc -> "avc"
        VesperAndroidVideoCodecFamily.Unknown -> "unknown"
    }
