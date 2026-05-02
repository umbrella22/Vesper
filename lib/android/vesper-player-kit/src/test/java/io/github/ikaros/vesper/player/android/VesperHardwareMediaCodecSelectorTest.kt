package io.github.ikaros.vesper.player.android

import org.junit.Assert.assertEquals
import org.junit.Test

class VesperHardwareMediaCodecSelectorTest {
    @Test
    fun videoCodecFamilyDetectsModernCodecTokens() {
        assertEquals(
            VesperAndroidVideoCodecFamily.Vvc,
            vesperAndroidVideoCodecFamily("vvc1.1.L123"),
        )
        assertEquals(
            VesperAndroidVideoCodecFamily.Av1,
            vesperAndroidVideoCodecFamily("av01.0.05M.08"),
        )
        assertEquals(
            VesperAndroidVideoCodecFamily.Av1,
            vesperAndroidVideoCodecFamily("video/av01"),
        )
        assertEquals(
            VesperAndroidVideoCodecFamily.Hevc,
            vesperAndroidVideoCodecFamily("hvc1.1.6.L93.B0"),
        )
        assertEquals(
            VesperAndroidVideoCodecFamily.Hevc,
            vesperAndroidVideoCodecFamily("hev1.1.6.L93.B0"),
        )
        assertEquals(
            VesperAndroidVideoCodecFamily.Avc,
            vesperAndroidVideoCodecFamily("avc1.4d401f"),
        )
    }

    @Test
    fun videoCodecFamilyIgnoresAudioCodecTokens() {
        assertEquals(
            VesperAndroidVideoCodecFamily.Unknown,
            vesperAndroidVideoCodecFamily("mp4a.40.2"),
        )
    }
}
