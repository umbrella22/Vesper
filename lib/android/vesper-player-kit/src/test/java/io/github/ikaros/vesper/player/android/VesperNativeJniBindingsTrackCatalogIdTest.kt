package io.github.ikaros.vesper.player.android

import androidx.media3.common.C
import androidx.media3.common.Format
import androidx.media3.common.MimeTypes
import androidx.media3.common.TrackGroup
import androidx.media3.common.TrackSelectionOverride
import androidx.media3.common.TrackSelectionParameters
import androidx.media3.common.Tracks
import androidx.media3.common.util.UnstableApi
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * Subtitle stable-id contract tests for the Android JNI track catalog.
 *
 * Verifies that subtitle track ids derive from the manifest
 * `Representation@id` (Media3 `Format.id`) only for DASH sources so they
 * survive source refresh and track reorder, while non-DASH subtitle
 * tracks (HLS CEA-608, MP4 embedded captions) and all video/audio tracks
 * keep the legacy positional `groupId:formatId:trackIndex` shape.
 */
@UnstableApi
class VesperNativeJniBindingsTrackCatalogIdTest {
    @Test
    fun subtitleStableTrackId_usesFormatIdWhenPresent() {
        val format = Format.Builder().setId("sub-en").build()
        assertEquals("subtitle:dash:sub-en", subtitleStableTrackId(format))
    }

    @Test
    fun subtitleStableTrackId_returnsEmptyWhenFormatIdBlank() {
        val format = Format.Builder().setId(null as String?).build()
        assertEquals("", subtitleStableTrackId(format))
    }

    @Test
    fun subtitleStableTrackId_returnsEmptyForBlankStringId() {
        val format = Format.Builder().setId("").build()
        assertEquals("", subtitleStableTrackId(format))
    }

    @Test
    fun nativeTrackId_remainsPositionalForVideoAndAudio() {
        // Stable DASH representation ids are scoped to subtitles; video and
        // audio retain their positional identity contract.
        val group = TrackGroup("group-v", Format.Builder().setId("v1").build())
        val format = group.getFormat(0)
        val id = nativeTrackId(group, 0, format)
        assertEquals("group-v:v1:0", id)
    }

    @Test
    fun nativeTrackId_fallsBackToTypeAndTrackIndexWhenIdsBlank() {
        val group = TrackGroup("", Format.Builder().setId(null as String?).build())
        val format = group.getFormat(0)
        // TrackGroup.type defaults to TRACK_TYPE_UNKNOWN (0) when no tracks
        // have been classified; the fallback id uses the type as-is.
        val id = nativeTrackId(group, 0, format)
        assertEquals("type${group.type}:track0:0", id)
    }

    @Test
    fun collectTrackCatalog_rejectsMissingDashSubtitleIdentity() {
        val catalog = collectTrackCatalog(textTracks(null to false), VesperPlayerSourceProtocol.Dash)

        assertTrue(catalog.tracks.isEmpty())
        assertEquals("subtitle_track_identity_ambiguous", catalog.subtitleIdentityFailure?.code)
        assertEquals("identity", catalog.subtitleIdentityFailure?.phase)
        assertNull(catalog.subtitleIdentityFailure?.trackId)
        assertEquals(1, catalog.subtitleIdentityFailure?.advertisedTrackCount)
    }

    @Test
    fun collectTrackCatalog_rejectsDuplicateDashSubtitleIdentity() {
        val catalog =
            collectTrackCatalog(
                textTracks("sub-en" to false, "sub-en" to false),
                VesperPlayerSourceProtocol.Dash,
            )

        assertTrue(catalog.tracks.isEmpty())
        assertEquals("subtitle_track_identity_ambiguous", catalog.subtitleIdentityFailure?.code)
        assertEquals("sub-en", catalog.subtitleIdentityFailure?.trackId)
        assertEquals(2, catalog.subtitleIdentityFailure?.advertisedTrackCount)
    }

    @Test
    fun collectTrackSelection_doesNotEchoPendingOverride() {
        val tracks = textTracks("sub-en" to false)
        val group = tracks.groups.single().mediaTrackGroup
        val parameters =
            TrackSelectionParameters.Builder()
                .setOverrideForType(TrackSelectionOverride(group, 0))
                .build()

        val selection =
            collectTrackSelection(tracks, parameters, VesperPlayerSourceProtocol.Dash).subtitle

        assertNull(selection.trackId)
        assertEquals(NativeTrackSelectionMode.Disabled.ordinal, selection.modeOrdinal)
    }

    @Test
    fun collectTrackSelection_publishesOnlyConfirmedDashSubtitleIdentity() {
        val tracks = textTracks("sub-en" to true)

        val selection =
            collectTrackSelection(
                tracks,
                TrackSelectionParameters.Builder().build(),
                VesperPlayerSourceProtocol.Dash,
            ).subtitle

        assertEquals("subtitle:dash:sub-en", selection.trackId)
        assertNotNull(selection.trackId)
    }

    @Test
    fun collectTrackSelection_doesNotPublishDuplicateDashSubtitleIdentity() {
        val tracks = textTracks("sub-en" to true, "sub-en" to false)

        val selection =
            collectTrackSelection(
                tracks,
                TrackSelectionParameters.Builder().build(),
                VesperPlayerSourceProtocol.Dash,
            ).subtitle

        assertNull(selection.trackId)
        assertEquals(NativeTrackSelectionMode.Disabled.ordinal, selection.modeOrdinal)
    }

    private fun textTracks(vararg tracks: Pair<String?, Boolean>): Tracks {
        val formats =
            tracks.map { (id, _) ->
                Format.Builder()
                    .setId(id)
                    .setSampleMimeType(MimeTypes.TEXT_VTT)
                    .setLanguage("en")
                    .build()
            }
        val mediaTrackGroup = TrackGroup("dash-text", *formats.toTypedArray())
        return Tracks(
            listOf(
                Tracks.Group(
                    mediaTrackGroup,
                    false,
                    IntArray(tracks.size) { C.FORMAT_HANDLED },
                    BooleanArray(tracks.size) { index -> tracks[index].second },
                ),
            ),
        )
    }
}
