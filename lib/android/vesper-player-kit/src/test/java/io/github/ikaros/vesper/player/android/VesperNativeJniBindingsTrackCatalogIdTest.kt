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
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * Subtitle stable-id contract tests for the Android JNI track catalog.
 *
 * Verifies that subtitle track ids survive source refresh and track reorder.
 * DASH uses Representation@id, external tracks preserve the caller id, and
 * other Media3 tracks use a canonical metadata identity.
 */
@UnstableApi
class VesperNativeJniBindingsTrackCatalogIdTest {
    @Test
    fun manifestReadiness_requiresTypedManifestForDashAndHls() {
        assertTrue(
            !hasTypedSubtitleManifest(
                manifest = null,
                sourceProtocol = VesperPlayerSourceProtocol.Dash,
            ),
        )
        assertTrue(
            !hasTypedSubtitleManifest(
                manifest = null,
                sourceProtocol = VesperPlayerSourceProtocol.Hls,
            ),
        )
        assertTrue(
            hasTypedSubtitleManifest(
                manifest = null,
                sourceProtocol = VesperPlayerSourceProtocol.Progressive,
            ),
        )
    }

    @Test
    fun trackCatalogReadiness_requiresTracksForProgressiveAndManifestForDash() {
        assertFalse(
            resolveTrackCatalogReadiness(
                observedTrackCatalog = false,
                requiresManifestInspection = false,
                manifestInfo = null,
            ),
        )
        assertTrue(
            resolveTrackCatalogReadiness(
                observedTrackCatalog = true,
                requiresManifestInspection = false,
                manifestInfo = null,
            ),
        )
        assertFalse(
            resolveTrackCatalogReadiness(
                observedTrackCatalog = true,
                requiresManifestInspection = true,
                manifestInfo = null,
            ),
        )
        assertTrue(
            resolveTrackCatalogReadiness(
                observedTrackCatalog = false,
                requiresManifestInspection = true,
                manifestInfo = NativeSubtitleManifestInfo(emptyList(), defaultGroupCount = 0),
            ),
        )
    }

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
    fun nonDashSubtitleIdentity_survivesReorderAndRemoval() {
        val first =
            collectTrackCatalog(
                textTracks("A" to false, "B" to false),
                VesperPlayerSourceProtocol.Hls,
            ).tracks.map { it.id }.toSet()
        val reordered =
            collectTrackCatalog(
                textTracks("B" to false, "A" to false),
                VesperPlayerSourceProtocol.Hls,
            ).tracks.map { it.id }.toSet()
        val reduced =
            collectTrackCatalog(
                textTracks("B" to false),
                VesperPlayerSourceProtocol.Hls,
            ).tracks.map { it.id }.toSet()

        assertEquals(first, reordered)
        val expectedB =
            subtitleTrackId(
                Format.Builder()
                    .setId("B")
                    .setSampleMimeType(MimeTypes.TEXT_VTT)
                    .setLanguage("en")
                    .build(),
                false,
            )
        assertEquals(setOf(expectedB), reduced)
        assertTrue(first.containsAll(reduced))
    }

    @Test
    fun externalSubtitleIdentity_preservesCallerProvidedId() {
        val externalId = "caller-sub-en"
        val catalog =
            collectTrackCatalog(
                textTracks(externalId to false),
                VesperPlayerSourceProtocol.Hls,
                listOf(externalId),
            )

        assertEquals(listOf(externalId), catalog.tracks.map { it.id })
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
    fun collectAppliedSubtitleSelection_readsExternalOverrideBeforeRendererActivation() {
        val externalId = "external-b"
        val tracks = textTracks(externalId to false)
        val group = tracks.groups.single().mediaTrackGroup
        val parameters =
            TrackSelectionParameters.Builder()
                .setOverrideForType(TrackSelectionOverride(group, 0))
                .build()

        val selection =
            collectAppliedSubtitleSelection(
                tracks = tracks,
                parameters = parameters,
                externalSubtitleIds = listOf(externalId),
                requestedModeOrdinal = NativeTrackSelectionMode.Track.ordinal,
            )

        assertEquals(VesperTrackSelection.track(externalId), selection)
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

    @Test
    fun automaticSubtitleSelection_rejectsDuplicateIdentity() {
        val tracks = textTracks("sub-en" to false, "sub-en" to false)

        val result =
            findAutomaticSubtitleOverride(
                tracks,
                TrackSelectionParameters.Builder().build(),
                VesperPlayerSourceProtocol.Dash,
                emptyList(),
            )

        assertNull(result.override)
        assertEquals("subtitle_track_identity_ambiguous", result.failure?.code)
        assertEquals("identity", result.failure?.phase)
        assertEquals("subtitle:dash:sub-en", result.failure?.trackId)
        assertEquals(2, result.failure?.advertisedTrackCount)
    }

    @Test
    fun automaticSubtitleSelection_prefersNonForcedCandidate() {
        val formats =
            listOf(
                Format.Builder()
                    .setId("forced")
                    .setSampleMimeType(MimeTypes.TEXT_VTT)
                    .setSelectionFlags(C.SELECTION_FLAG_FORCED)
                    .build(),
                Format.Builder()
                    .setId("regular")
                    .setSampleMimeType(MimeTypes.TEXT_VTT)
                    .build(),
            )
        val tracks = textTracks(formats, intArrayOf(C.FORMAT_HANDLED, C.FORMAT_HANDLED))

        val result =
            findAutomaticSubtitleOverride(
                tracks,
                TrackSelectionParameters.Builder().build(),
                VesperPlayerSourceProtocol.Dash,
                emptyList(),
            )

        assertEquals(1, result.override?.trackIndices?.single())
    }

    @Test
    fun mixedDashAndExternalSubtitleIdentity_removesOnlyMedia3MergePrefix() {
        val externalId = "external:en"
        val catalog =
            collectTrackCatalog(
                textTracks("0:dash-en" to false, "1:$externalId" to false),
                VesperPlayerSourceProtocol.Dash,
                listOf(externalId),
            )

        assertNull(catalog.subtitleIdentityFailure)
        assertEquals(
            setOf("subtitle:dash:dash-en", externalId),
            catalog.tracks.map { it.id }.toSet(),
        )
    }

    @Test
    fun collectTrackCatalog_countsDeclaredFailedExternalAlongsideEmbeddedTracks() {
        val catalog =
            collectTrackCatalog(
                tracks = textTracks("0:embedded-en" to false, "1:external-ok" to false),
                sourceProtocol = VesperPlayerSourceProtocol.Hls,
                externalSubtitleIds = listOf("external-ok"),
                advertisedExternalSubtitleCount = 2,
            )

        assertNull(catalog.subtitleIdentityFailure)
        assertEquals(2, catalog.tracks.size)
        assertEquals(3, catalog.advertisedSubtitleTrackCount)
    }

    @Test
    fun collectTrackCatalog_rejectsDuplicateDefaultBeforeSupportFiltering() {
        val formats =
            listOf(
                Format.Builder()
                    .setId("0:embedded-en")
                    .setSampleMimeType(MimeTypes.TEXT_VTT)
                    .setSelectionFlags(C.SELECTION_FLAG_DEFAULT)
                    .build(),
                Format.Builder()
                    .setId("1:external-en")
                    .setSampleMimeType(MimeTypes.TEXT_VTT)
                    .setSelectionFlags(C.SELECTION_FLAG_DEFAULT)
                    .build(),
            )
        val catalog =
            collectTrackCatalog(
                tracks = textTracks(formats, intArrayOf(C.FORMAT_HANDLED, C.FORMAT_UNSUPPORTED_TYPE)),
                sourceProtocol = VesperPlayerSourceProtocol.Hls,
            )

        assertTrue(catalog.tracks.isEmpty())
        assertEquals("subtitle_default_track_ambiguous", catalog.subtitleIdentityFailure?.code)
        assertEquals("identity", catalog.subtitleIdentityFailure?.phase)
        assertEquals(2, catalog.subtitleIdentityFailure?.advertisedTrackCount)
    }

    @Test
    fun collectTrackCatalog_rejectsDuplicateIdentityBeforeSupportFiltering() {
        val formats =
            listOf(
                Format.Builder()
                    .setId("sub-en")
                    .setSampleMimeType(MimeTypes.TEXT_VTT)
                    .build(),
                Format.Builder()
                    .setId("sub-en")
                    .setSampleMimeType(MimeTypes.TEXT_VTT)
                    .build(),
            )
        val catalog =
            collectTrackCatalog(
                tracks = textTracks(formats, intArrayOf(C.FORMAT_HANDLED, C.FORMAT_UNSUPPORTED_TYPE)),
                sourceProtocol = VesperPlayerSourceProtocol.Dash,
            )

        assertTrue(catalog.tracks.isEmpty())
        assertEquals("subtitle_track_identity_ambiguous", catalog.subtitleIdentityFailure?.code)
        assertEquals("identity", catalog.subtitleIdentityFailure?.phase)
        assertEquals(2, catalog.subtitleIdentityFailure?.advertisedTrackCount)
    }

    @Test
    fun collectTrackCatalog_rejectsFailedDeclaredExternalIdCollision() {
        val manifestInfo =
            NativeSubtitleManifestInfo(
                declarations =
                    listOf(
                        NativeSubtitleManifestDeclaration(
                            id = "subtitle:dash:sub-en",
                            label = "English",
                            language = "en",
                            codec = MimeTypes.TEXT_VTT,
                            isDefault = false,
                            isForced = false,
                        ),
                    ),
                defaultGroupCount = 0,
            )

        val catalog =
            collectTrackCatalog(
                tracks = textTracks("sub-en" to false),
                sourceProtocol = VesperPlayerSourceProtocol.Dash,
                advertisedExternalSubtitleCount = 1,
                // The external resource failed before Media3 created a track,
                // but its declared id must still participate in identity checks.
                declaredExternalSubtitleIds = listOf("subtitle:dash:sub-en"),
                manifestInfo = manifestInfo,
            )

        assertTrue(catalog.tracks.isEmpty())
        assertEquals("subtitle_track_identity_ambiguous", catalog.subtitleIdentityFailure?.code)
        assertEquals("subtitle:dash:sub-en", catalog.subtitleIdentityFailure?.trackId)
        assertEquals(2, catalog.subtitleIdentityFailure?.advertisedTrackCount)
    }

    @Test
    fun collectTrackCatalog_normalizesDefaultAcrossRepresentationsInOneDashGroup() {
        val manifestInfo =
            NativeSubtitleManifestInfo(
                declarations =
                    listOf(
                        NativeSubtitleManifestDeclaration(
                            id = "subtitle:dash:sub-low",
                            label = "English",
                            language = "en",
                            codec = MimeTypes.TEXT_VTT,
                            isDefault = true,
                            isForced = false,
                        ),
                        NativeSubtitleManifestDeclaration(
                            id = "subtitle:dash:sub-high",
                            label = "English",
                            language = "en",
                            codec = MimeTypes.TEXT_VTT,
                            isDefault = false,
                            isForced = false,
                        ),
                    ),
                defaultGroupCount = 1,
            )
        val formats =
            listOf(
                Format.Builder()
                    .setId("sub-low")
                    .setSampleMimeType(MimeTypes.TEXT_VTT)
                    .setSelectionFlags(C.SELECTION_FLAG_DEFAULT)
                    .build(),
                Format.Builder()
                    .setId("sub-high")
                    .setSampleMimeType(MimeTypes.TEXT_VTT)
                    // Media3 may copy the adaptation-set default flag to all
                    // representations; manifest metadata must override it.
                    .setSelectionFlags(C.SELECTION_FLAG_DEFAULT)
                    .build(),
            )

        val catalog =
            collectTrackCatalog(
                tracks = textTracks(formats, intArrayOf(C.FORMAT_HANDLED, C.FORMAT_HANDLED)),
                sourceProtocol = VesperPlayerSourceProtocol.Dash,
                manifestInfo = manifestInfo,
            )

        assertNull(catalog.subtitleIdentityFailure)
        assertEquals(2, catalog.advertisedSubtitleTrackCount)
        assertEquals(listOf("subtitle:dash:sub-low"), catalog.tracks.filter { it.isDefault }.map { it.id })
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

    private fun textTracks(
        formats: List<Format>,
        support: IntArray,
    ): Tracks {
        val mediaTrackGroup = TrackGroup("text", *formats.toTypedArray())
        return Tracks(
            listOf(
                Tracks.Group(
                    mediaTrackGroup,
                    false,
                    support,
                    BooleanArray(formats.size),
                ),
            ),
        )
    }
}
