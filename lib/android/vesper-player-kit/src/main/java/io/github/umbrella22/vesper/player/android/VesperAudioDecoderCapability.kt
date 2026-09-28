package io.github.umbrella22.vesper.player.android

import android.content.Context
import androidx.media3.common.Format
import androidx.media3.common.MimeTypes
import androidx.media3.exoplayer.mediacodec.MediaCodecSelector
import androidx.media3.exoplayer.mediacodec.MediaCodecUtil
import java.util.Locale

data class VesperAudioDecoderCapabilityRequest(
    val sampleMimeType: String? = null,
    val codec: String? = null,
    val channels: Int? = null,
    val sampleRate: Int? = null,
) {
    init {
        require(channels == null || channels > 0) { "channels must be positive" }
        require(sampleRate == null || sampleRate > 0) { "sampleRate must be positive" }
    }
    fun toMap(): Map<String, Any?> = mapOf(
        "sampleMimeType" to sampleMimeType, "codec" to codec, "channels" to channels, "sampleRate" to sampleRate,
    )
}

enum class VesperAudioDecoderSupport(val wireName: String) {
    Supported("supported"), Unsupported("unsupported"), Unknown("unknown"),
}

data class VesperAudioDecoderCandidate(
    val name: String,
    val hardwareAccelerated: Boolean,
    val status: VesperAudioDecoderSupport,
    val reason: String,
) {
    fun toMap(): Map<String, Any?> = mapOf(
        "name" to name, "hardwareAccelerated" to hardwareAccelerated,
        "status" to status.wireName, "reason" to reason,
    )
}

/** Decoder-format evidence only: not container, DRM, route, or audible-output support. */
data class VesperAudioDecoderCapabilityResult(
    val request: VesperAudioDecoderCapabilityRequest,
    val status: VesperAudioDecoderSupport,
    val reason: String,
    val resolvedMimeType: String? = null,
    val candidates: List<VesperAudioDecoderCandidate> = emptyList(),
    val evidence: String = "mediaCodecList",
) {
    fun toMap(): Map<String, Any?> = mapOf(
        "request" to request.toMap(), "status" to status.wireName, "reason" to reason,
        "resolvedMimeType" to resolvedMimeType, "candidates" to candidates.map { it.toMap() },
        "evidence" to evidence,
    )
}

internal class AudioDecoderProbeCandidate(
    val name: String, val hardwareAccelerated: Boolean,
    val supportsProfile: (Int) -> Boolean? = { null },
    val supports: (Format) -> Boolean?,
)

internal object VesperAudioDecoderCapabilityProbe {
    fun probe(context: Context, request: VesperAudioDecoderCapabilityRequest): VesperAudioDecoderCapabilityResult =
        probe(request) { mime ->
            // The production audio selector permits software codecs. Video's
            // hardware-only helper deliberately rejects every audio MIME type.
            MediaCodecSelector.DEFAULT.getDecoderInfos(mime, false, false).map { decoder ->
                AudioDecoderProbeCandidate(decoder.name, decoder.hardwareAccelerated,
                    supportsProfile = { profile -> decoder.profileLevels.takeIf { it.isNotEmpty() }?.any { it.profile == profile } },
                ) { format ->
                    val audioCapabilities = decoder.capabilities?.audioCapabilities
                    if (audioCapabilities == null || request.channels == null || request.sampleRate == null) null
                    else audioCapabilities.isSampleRateSupported(request.sampleRate) &&
                        request.channels <= audioCapabilities.maxInputChannelCount && decoder.isFormatSupported(context, format)
                }
            }
        }

    fun probe(request: VesperAudioDecoderCapabilityRequest,
        query: (String) -> List<AudioDecoderProbeCandidate>): VesperAudioDecoderCapabilityResult {
        fun unknown(reason: String, mime: String? = null) = VesperAudioDecoderCapabilityResult(
            request, VesperAudioDecoderSupport.Unknown, reason, mime,
        )
        val declaredMime = request.sampleMimeType?.lowercase(Locale.ROOT)
        val codecMime = request.codec?.let(MimeTypes::getMediaMimeType)
        if (request.codec != null && (codecMime == null || request.codec.contains(','))) return unknown("unrecognizedCodec")
        if (declaredMime != null && codecMime != null && declaredMime != codecMime) return unknown("conflictingCodecMime")
        val mime = declaredMime ?: codecMime ?: return unknown("missingMimeOrCodec")
        if (!MimeTypes.isAudio(mime)) return unknown("notAudioMime", mime)
        // PCM may bypass MediaCodec in the playback route.
        if (mime == MimeTypes.AUDIO_RAW) return unknown("pcmRouteNotQueried", mime)
        val format = Format.Builder().setSampleMimeType(mime).setCodecs(request.codec)
            .setChannelCount(request.channels ?: Format.NO_VALUE)
            .setSampleRate(request.sampleRate ?: Format.NO_VALUE).build()
        val profile = try { MediaCodecUtil.getCodecProfileAndLevel(format)?.first } catch (_: Exception) { null }
        val codecKnown = request.codec in setOf("ac-3", "ec-3", "ac-4", "opus", "vorbis", "flac") || profile != null
        val complete = codecKnown && request.channels != null && request.sampleRate != null
        val decoders = try { query(mime) } catch (_: Exception) { return unknown("decoderQueryFailed", mime) }
        val candidates = decoders.map { evaluateCandidate(it, format, complete, profile) }
        val status = when {
            !complete -> VesperAudioDecoderSupport.Unknown
            candidates.any { it.status == VesperAudioDecoderSupport.Supported } -> VesperAudioDecoderSupport.Supported
            candidates.any { it.status == VesperAudioDecoderSupport.Unknown } -> VesperAudioDecoderSupport.Unknown
            else -> VesperAudioDecoderSupport.Unsupported
        }
        val reason = when {
            !complete -> "incompleteFormatConstraints"
            candidates.isEmpty() -> "noDecoder"
            status == VesperAudioDecoderSupport.Supported -> "formatAccepted"
            status == VesperAudioDecoderSupport.Unknown -> "formatSupportNotConfirmed"
            else -> "formatRejected"
        }
        return VesperAudioDecoderCapabilityResult(request, status, reason, mime, candidates)
    }

    internal fun evaluateCandidate(decoder: AudioDecoderProbeCandidate, format: Format, complete: Boolean,
        profile: Int?): VesperAudioDecoderCandidate {
        try {
            val accepted = decoder.supports(format)
            val reason = when {
                !complete -> "incompleteFormatConstraints"
                accepted == false -> "formatRejected"
                accepted == null -> "formatCapabilitiesUnavailable"
                profile != null && decoder.supportsProfile(profile) != true -> "codecProfileNotConfirmed"
                else -> "formatAccepted"
            }
            val status = when (reason) {
                "formatAccepted" -> VesperAudioDecoderSupport.Supported
                "formatRejected" -> VesperAudioDecoderSupport.Unsupported
                else -> VesperAudioDecoderSupport.Unknown
            }
            return VesperAudioDecoderCandidate(decoder.name, decoder.hardwareAccelerated, status, reason)
        } catch (_: Exception) {
            return VesperAudioDecoderCandidate(decoder.name, decoder.hardwareAccelerated,
                VesperAudioDecoderSupport.Unknown, "formatQueryFailed")
        }
    }
}
