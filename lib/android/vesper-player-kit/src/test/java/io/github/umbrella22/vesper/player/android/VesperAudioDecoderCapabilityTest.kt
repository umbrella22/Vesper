package io.github.umbrella22.vesper.player.android

import androidx.media3.common.MimeTypes
import org.junit.Assert.*
import org.junit.Test

class VesperAudioDecoderCapabilityTest {
    private val request = VesperAudioDecoderCapabilityRequest(codec = "opus", channels = 6, sampleRate = 48_000)

    @Test fun softwareAudioCandidateReceivesFullFormat() {
        var checked = false
        val result = VesperAudioDecoderCapabilityProbe.probe(request) { mime ->
            assertEquals(MimeTypes.AUDIO_OPUS, mime)
            listOf(AudioDecoderProbeCandidate("software.opus", false) { format ->
                checked = true
                assertEquals(6, format.channelCount)
                assertEquals(48_000, format.sampleRate)
                assertEquals("opus", format.codecs)
                true
            })
        }
        assertTrue(checked)
        assertEquals(VesperAudioDecoderSupport.Supported, result.status)
        assertFalse(result.candidates.single().hardwareAccelerated)
    }

    @Test fun media3AcceptanceCannotSubstituteForPlatformProfileEvidence() {
        val format = androidx.media3.common.Format.Builder().setSampleMimeType(MimeTypes.AUDIO_AAC)
            .setCodecs("mp4a.40.5").setChannelCount(2).setSampleRate(48_000).build()
        val unconfirmed = AudioDecoderProbeCandidate("aac.lc.only", false, supportsProfile = { it == 2 }) { true }
        val result = VesperAudioDecoderCapabilityProbe.evaluateCandidate(unconfirmed, format, true, 5)
        assertEquals(VesperAudioDecoderSupport.Unknown, result.status)
        assertEquals("codecProfileNotConfirmed", result.reason)
        val confirmed = AudioDecoderProbeCandidate("aac.he", false, supportsProfile = { it == 5 }) { true }
        assertEquals(VesperAudioDecoderSupport.Supported,
            VesperAudioDecoderCapabilityProbe.evaluateCandidate(confirmed, format, true, 5).status)
    }

    @Test fun failuresAreUnknownRatherThanUnsupported() {
        assertEquals("decoderQueryFailed", VesperAudioDecoderCapabilityProbe.probe(request) { error("query") }.reason)
        val result = VesperAudioDecoderCapabilityProbe.probe(request) {
            listOf(AudioDecoderProbeCandidate("reject", false) { false }, AudioDecoderProbeCandidate("broken", false) { error("query") })
        }
        assertEquals(VesperAudioDecoderSupport.Unknown, result.status)
        assertEquals("formatQueryFailed", result.candidates.last().reason)
        assertEquals(VesperAudioDecoderSupport.Unsupported, VesperAudioDecoderCapabilityProbe.probe(request) { emptyList() }.status)
    }

    @Test fun conflictingOrIncompleteMetadataNeverClaimsSupport() {
        val query: (String) -> List<AudioDecoderProbeCandidate> = { listOf(AudioDecoderProbeCandidate("candidate", false) { true }) }
        assertEquals("conflictingCodecMime", VesperAudioDecoderCapabilityProbe.probe(request.copy(sampleMimeType = "audio/mp4a-latm"), query).reason)
        assertEquals("unrecognizedCodec", VesperAudioDecoderCapabilityProbe.probe(request.copy(codec = "future.5"), query).reason)
        assertEquals(VesperAudioDecoderSupport.Unknown, VesperAudioDecoderCapabilityProbe.probe(request.copy(channels = null), query).status)
        assertEquals(VesperAudioDecoderSupport.Unknown, VesperAudioDecoderCapabilityProbe.probe(request.copy(codec = "mp4a.40.999"), query).status)
        assertEquals("pcmRouteNotQueried", VesperAudioDecoderCapabilityProbe.probe(
            VesperAudioDecoderCapabilityRequest(sampleMimeType = "audio/raw"), query).reason)
    }
}
