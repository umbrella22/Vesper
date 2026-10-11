package io.github.umbrella22.vesper.player.android

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
    fun trackSupportMapping_preservesMedia3CapabilityAxes() {
        val handled = trackSupportForFormatSupport(C.FORMAT_HANDLED, playbackPath = "systemPlayer")
        assertEquals(NativeTrackSupportStatus.Supported.ordinal, handled.statusOrdinal)
        assertEquals(NativeTrackSupportReason.None.ordinal, handled.reasonOrdinal)
        assertEquals("handled", handled.formatSupportRawValue)

        val exceeds = trackSupportForFormatSupport(C.FORMAT_EXCEEDS_CAPABILITIES)
        assertEquals(NativeTrackSupportStatus.ExceedsCapabilities.ordinal, exceeds.statusOrdinal)
        assertEquals(NativeTrackSupportReason.FormatExceedsCapabilities.ordinal, exceeds.reasonOrdinal)
        assertEquals("exceedsCapabilities", exceeds.formatSupportRawValue)

        val drm = trackSupportForFormatSupport(C.FORMAT_UNSUPPORTED_DRM)
        assertEquals(NativeTrackSupportStatus.Unsupported.ordinal, drm.statusOrdinal)
        assertEquals(NativeTrackSupportReason.UnsupportedDrm.ordinal, drm.reasonOrdinal)

        val unknown = trackSupportForFormatSupport(99)
        assertEquals(NativeTrackSupportStatus.Unknown.ordinal, unknown.statusOrdinal)
        assertEquals(NativeTrackSupportReason.PlatformUnknown.ordinal, unknown.reasonOrdinal)
        assertEquals("99", unknown.formatSupportRawValue)
    }

    @Test
    fun collectTrackCatalog_keepsUnsupportedVideoTracksWithSupportEvidence() {
        val formats =
            listOf(
                Format.Builder()
                    .setId("v720")
                    .setSampleMimeType(MimeTypes.VIDEO_H264)
                    .setWidth(1280)
                    .setHeight(720)
                    .build(),
                Format.Builder()
                    .setId("v4k")
                    .setSampleMimeType(MimeTypes.VIDEO_H265)
                    .setWidth(3840)
                    .setHeight(2160)
                    .build(),
            )
        val group = TrackGroup("video", *formats.toTypedArray())
        val tracks =
            Tracks(
                listOf(
                    Tracks.Group(
                        group,
                        true,
                        intArrayOf(C.FORMAT_HANDLED, C.FORMAT_EXCEEDS_CAPABILITIES),
                        booleanArrayOf(true, false),
                    ),
                ),
            )

        val catalog = collectTrackCatalog(tracks, playbackPath = "systemPlayer")

        assertEquals(listOf("video:v720:0", "video:v4k:1"), catalog.tracks.map { it.id })
        val exceeds = catalog.tracks.last().support
        assertEquals(NativeTrackSupportStatus.ExceedsCapabilities.ordinal, exceeds.statusOrdinal)
        assertEquals(NativeTrackSupportReason.FormatExceedsCapabilities.ordinal, exceeds.reasonOrdinal)
        assertEquals("systemPlayer", exceeds.playbackPath)
    }

    @Test
    fun validateFixedTrackSelection_rejectsCapabilityBeforeOverrideCreation() {
        val tracks = videoTracksWithSupport(C.FORMAT_UNSUPPORTED_SUBTYPE)
        val error =
            org.junit.Assert.assertThrows(VesperFixedTrackSelectionException::class.java) {
                validateFixedTrackSelection(
                    tracks = tracks,
                    trackId = "video:v0:0",
                    expectedCatalogRevision = null,
                    actualCatalogRevision = 7L,
                )
            }

        assertEquals("trackUnsupported", error.code)
        assertEquals("video:v0:0", error.trackId)
        assertNull(error.expectedCatalogRevision)
        assertEquals(7L, error.actualCatalogRevision)
        assertEquals("unsupportedSubtype", error.details["reason"])
    }

    @Test
    fun validateFixedTrackSelection_onlyReportsStaleWhenRequestCarriesRevision() {
        val tracks = videoTracksWithSupport(C.FORMAT_HANDLED)
        val error =
            org.junit.Assert.assertThrows(VesperFixedTrackSelectionException::class.java) {
                validateFixedTrackSelection(
                    tracks = tracks,
                    trackId = "video:v0:0",
                    expectedCatalogRevision = 6L,
                    actualCatalogRevision = 7L,
                )
            }

        assertEquals("staleCatalog", error.code)
        assertEquals(6L, error.expectedCatalogRevision)
        assertEquals(7L, error.actualCatalogRevision)

        // The same mismatch is not a stale-token error for a legacy command
        // that did not carry an expected revision.
        val missing =
            org.junit.Assert.assertThrows(VesperFixedTrackSelectionException::class.java) {
                validateFixedTrackSelection(
                    tracks = Tracks.EMPTY,
                    trackId = "video:missing",
                    expectedCatalogRevision = null,
                    actualCatalogRevision = 7L,
                )
            }
        assertEquals("trackUnavailable", missing.code)
    }

    @Test
    fun validateFixedTrackSelection_rejectsSameSourceRuntimeFailureWithoutChangingStaticSupport() {
        val tracks = videoTracksWithSupport(C.FORMAT_HANDLED)
        val rejection =
            NativeRuntimeTrackRejection(
                sourceEpoch = 11L,
                catalogRevision = 7L,
                trackId = "video:v0:0",
                code = "runtimeTrackRejected",
                details =
                    mapOf(
                        "reason" to "runtimeFailure",
                        "errorCodeName" to "ERROR_CODE_DECODING_FAILED",
                    ),
            )

        val error =
            org.junit.Assert.assertThrows(VesperFixedTrackSelectionException::class.java) {
                validateFixedTrackSelection(
                    tracks = tracks,
                    trackId = "video:v0:0",
                    expectedCatalogRevision = 8L,
                    actualCatalogRevision = 8L,
                    sourceEpoch = 11L,
                    runtimeTrackRejection = rejection,
                )
            }

        assertEquals("runtimeTrackRejected", error.code)
        assertEquals("fixedTrack", error.details["domain"])
        assertEquals("runtimeFailure", error.details["reason"])
        assertEquals(8L, error.actualCatalogRevision)
        assertEquals(
            NativeTrackSupportStatus.Supported.ordinal,
            trackSupportForFormatSupport(C.FORMAT_HANDLED).statusOrdinal,
        )

        val nextSourceLocation =
            validateFixedTrackSelection(
                tracks = tracks,
                trackId = "video:v0:0",
                expectedCatalogRevision = null,
                actualCatalogRevision = 9L,
                sourceEpoch = 12L,
                runtimeTrackRejection = rejection,
            )
        assertEquals(0, nextSourceLocation.trackIndex)
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
    fun mergedExternalSubtitleCatalog_prefersIndexedIdentityOverAnotherRawId() {
        val externalIds = listOf("2:captions", "captions")
        // Media3 prefixes Format.id by child index: main=0, sidecars=1,2.
        val tracks = separateTextTracks("1:2:captions" to false, "2:captions" to false)

        val catalog = collectTrackCatalog(tracks, externalSubtitleIds = externalIds)

        assertEquals(externalIds, catalog.tracks.map { it.id })
        assertNull(catalog.subtitleIdentityFailure)
        assertEquals(2, catalog.advertisedSubtitleTrackCount)
    }

    @Test
    fun mergedExternalSubtitleSelection_confirmsEachIndexedIdentityWithoutEchoingPendingOverride() {
        val externalIds = listOf("2:captions", "captions")
        val pending = separateTextTracks("1:2:captions" to false, "2:captions" to false)
        externalIds.forEachIndexed { index, externalId ->
            val parameters =
                TrackSelectionParameters.Builder()
                    .setOverrideForType(TrackSelectionOverride(pending.groups[index].mediaTrackGroup, 0))
                    .build()
            val confirmed =
                separateTextTracks("1:2:captions" to (index == 0), "2:captions" to (index == 1))

            assertEquals(
                externalId,
                collectTrackSelection(
                    confirmed,
                    parameters,
                    externalSubtitleIds = externalIds,
                    subtitleModeOrdinal = NativeTrackSelectionMode.Track.ordinal,
                ).subtitle.trackId,
            )
            assertTrue(
                isSubtitleTrackSelectable(pending, externalId, externalSubtitleIds = externalIds),
            )
            assertEquals(
                VesperTrackSelection.track(externalId),
                collectAppliedSubtitleSelection(
                    pending,
                    parameters,
                    externalSubtitleIds = externalIds,
                    requestedModeOrdinal = NativeTrackSelectionMode.Track.ordinal,
                ),
            )
            assertNull(
                collectTrackSelection(
                    pending,
                    parameters,
                    externalSubtitleIds = externalIds,
                    subtitleModeOrdinal = NativeTrackSelectionMode.Track.ordinal,
                ).subtitle.trackId,
            )
        }
    }

    @Test
    fun mergedPrimarySubtitleCatalog_keepsEmbeddedIdDistinctFromOpaqueExternalId() {
        val catalog =
            collectTrackCatalog(
                primaryAndOpaqueExternalTracks(),
                VesperPlayerSourceProtocol.Dash,
                listOf("0:captions"),
            )

        assertNull(catalog.subtitleIdentityFailure)
        assertEquals(listOf("subtitle:dash:captions", "0:captions"), catalog.tracks.map { it.id })
        assertEquals(2, catalog.advertisedSubtitleTrackCount)
    }

    @Test
    fun mergedPrimarySubtitleSelection_confirmsEachDistinctIdentity() {
        val pending = primaryAndOpaqueExternalTracks()
        listOf("subtitle:dash:captions", "0:captions").forEachIndexed { index, trackId ->
            val parameters =
                TrackSelectionParameters.Builder()
                    .setOverrideForType(TrackSelectionOverride(pending.groups[index].mediaTrackGroup, 0))
                    .build()
            val confirmed = primaryAndOpaqueExternalTracks(index == 0, index == 1)

            assertEquals(
                trackId,
                collectTrackSelection(
                    confirmed,
                    parameters,
                    VesperPlayerSourceProtocol.Dash,
                    listOf("0:captions"),
                    subtitleModeOrdinal = NativeTrackSelectionMode.Track.ordinal,
                ).subtitle.trackId,
            )
        }
    }

    @Test
    fun mergedPrimarySubtitleSelection_preservesPendingOverrideWithoutConfirmingIt() {
        val pending = primaryAndOpaqueExternalTracks()
        listOf("subtitle:dash:captions", "0:captions").forEachIndexed { index, trackId ->
            val parameters =
                TrackSelectionParameters.Builder()
                    .setOverrideForType(TrackSelectionOverride(pending.groups[index].mediaTrackGroup, 0))
                    .build()

            assertEquals(
                VesperTrackSelection.track(trackId),
                collectAppliedSubtitleSelection(
                    pending,
                    parameters,
                    VesperPlayerSourceProtocol.Dash,
                    listOf("0:captions"),
                    requestedModeOrdinal = NativeTrackSelectionMode.Track.ordinal,
                ),
            )
            assertNull(
                collectTrackSelection(
                    pending,
                    parameters,
                    VesperPlayerSourceProtocol.Dash,
                    listOf("0:captions"),
                    subtitleModeOrdinal = NativeTrackSelectionMode.Track.ordinal,
                ).subtitle.trackId,
            )
        }
    }

    @Test
    fun mergedPrimarySubtitleTargets_resolveEachIdentityToItsOwnGroup() {
        val tracks = primaryAndOpaqueExternalTracks()
        listOf("subtitle:dash:captions", "0:captions").forEachIndexed { index, trackId ->
            assertTrue(
                isSubtitleTrackSelectable(
                    tracks,
                    trackId,
                    VesperPlayerSourceProtocol.Dash,
                    listOf("0:captions"),
                ),
            )
            val location =
                findTrackLocation(
                    tracks,
                    C.TRACK_TYPE_TEXT,
                    trackId,
                    VesperPlayerSourceProtocol.Dash,
                    listOf("0:captions"),
                )
            assertEquals(tracks.groups[index], location?.group)
            assertEquals(0, location?.trackIndex)
        }
    }

    @Test
    fun mergedPrimaryAutomaticSubtitleOverride_doesNotRejectDistinctIdentities() {
        val result =
            findAutomaticSubtitleOverride(
                primaryAndOpaqueExternalTracks(),
                TrackSelectionParameters.Builder().build(),
                VesperPlayerSourceProtocol.Dash,
                listOf("0:captions"),
            )

        assertNull(result.failure)
        assertNotNull(result.override)
    }

    @Test
    fun unmergedOpaquePrimaryPrefixExternalId_remainsLossless() {
        val externalIds = listOf("0:captions")
        assertEquals("0:captions", subtitleTrackId(Format.Builder().setId("0:captions").build(), true, externalIds))
        assertEquals("0:captions", subtitleTrackId(Format.Builder().setId("1:0:captions").build(), true, externalIds))
        assertEquals(
            "subtitle:dash:7:0:captions",
            subtitleTrackId(Format.Builder().setId("7:0:captions").build(), true, externalIds),
        )
        val catalog = collectTrackCatalog(textTracks("0:captions" to false), VesperPlayerSourceProtocol.Dash, externalIds)
        assertNull(catalog.subtitleIdentityFailure)
        assertEquals(externalIds, catalog.tracks.map { it.id })
    }

    @Test
    fun externalSubtitleIdentity_preservesOpaqueRawIdAndOnlyItsDeclaredMergedPrefix() {
        val externalIds = listOf("2:captions")
        assertEquals("2:captions", subtitleTrackId(Format.Builder().setId("2:captions").build(), true, externalIds))
        assertEquals("2:captions", subtitleTrackId(Format.Builder().setId("1:2:captions").build(), true, externalIds))
        assertEquals(
            "subtitle:dash:7:2:captions",
            subtitleTrackId(Format.Builder().setId("7:2:captions").build(), true, externalIds),
        )
        assertEquals(
            "subtitle:dash:embedded:en",
            subtitleTrackId(Format.Builder().setId("0:embedded:en").build(), true, externalIds),
        )
    }

    @Test
    fun subtitleTargetReadinessUsesStableExternalIdentityAndRequiresSupport() {
        val externalId = "caller-sub-en"
        val format =
            Format.Builder()
                .setId("1:$externalId")
                .setSampleMimeType(MimeTypes.TEXT_VTT)
                .setLanguage("en")
                .build()

        assertTrue(
            isSubtitleTrackSelectable(
                tracks = textTracks(listOf(format), intArrayOf(C.FORMAT_HANDLED)),
                trackId = externalId,
                sourceProtocol = VesperPlayerSourceProtocol.Hls,
                externalSubtitleIds = listOf(externalId),
            )
        )
        assertFalse(
            isSubtitleTrackSelectable(
                tracks = textTracks(listOf(format), intArrayOf(C.FORMAT_UNSUPPORTED_TYPE)),
                trackId = externalId,
                sourceProtocol = VesperPlayerSourceProtocol.Hls,
                externalSubtitleIds = listOf(externalId),
            )
        )
        assertFalse(
            isSubtitleTrackSelectable(
                tracks = textTracks(listOf(format, format), intArrayOf(C.FORMAT_HANDLED, C.FORMAT_HANDLED)),
                trackId = externalId,
                sourceProtocol = VesperPlayerSourceProtocol.Hls,
                externalSubtitleIds = listOf(externalId),
            )
        )
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

    private fun primaryAndOpaqueExternalTracks(
        mainSelected: Boolean = false,
        externalSelected: Boolean = false,
    ): Tracks =
        Tracks(
            listOf("0:dash-text" to "0:captions", "1:" to "1:0:captions").mapIndexed { index, (groupId, formatId) ->
                // Media3 1.11.1 prefixes both group and format by child index.
                // These DTOs model its main=0 / SingleSample sidecar=1 output.
                val format =
                    Format.Builder()
                        .setId(formatId)
                        .setSampleMimeType(MimeTypes.TEXT_VTT)
                        .setLanguage("en")
                        .build()
                Tracks.Group(
                    TrackGroup(groupId, format),
                    false,
                    intArrayOf(C.FORMAT_HANDLED),
                    booleanArrayOf(if (index == 0) mainSelected else externalSelected),
                )
            },
        )

    private fun separateTextTracks(vararg tracks: Pair<String, Boolean>): Tracks =
        Tracks(
            tracks.mapIndexed { index, (id, selected) ->
                val format =
                    Format.Builder()
                        .setId(id)
                        .setSampleMimeType(MimeTypes.TEXT_VTT)
                        .setLanguage("en")
                        .build()
                Tracks.Group(
                    TrackGroup("sidecar-$index", format),
                    false,
                    intArrayOf(C.FORMAT_HANDLED),
                    booleanArrayOf(selected),
                )
            },
        )

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

    private fun videoTracksWithSupport(support: Int): Tracks {
        val format =
            Format.Builder()
                .setId("v0")
                .setSampleMimeType(MimeTypes.VIDEO_H264)
                .setWidth(1280)
                .setHeight(720)
                .build()
        val group = TrackGroup("video", format)
        return Tracks(
            listOf(
                Tracks.Group(
                    group,
                    false,
                    intArrayOf(support),
                    booleanArrayOf(false),
                ),
            ),
        )
    }
}
