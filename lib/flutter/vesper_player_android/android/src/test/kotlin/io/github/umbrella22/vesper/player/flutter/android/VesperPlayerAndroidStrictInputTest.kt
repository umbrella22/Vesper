package io.github.umbrella22.vesper.player.flutter.android

import io.github.umbrella22.vesper.player.android.VesperAbrPolicy
import io.github.umbrella22.vesper.player.android.VesperDownloadRecoveredTaskPlan
import io.github.umbrella22.vesper.player.android.VesperFrameProcessorMode
import io.github.umbrella22.vesper.player.android.VesperNativeFramePipelineMode
import io.github.umbrella22.vesper.player.android.VesperSourceNormalizerMode
import io.github.umbrella22.vesper.player.android.VesperTrackSelection
import io.flutter.plugin.common.MethodCall
import io.flutter.plugin.common.MethodChannel
import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.runBlocking
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Assert.fail
import org.junit.Test

class VesperPlayerAndroidStrictInputTest {
    private val resource = mapOf("resourceId" to "manifest", "uri" to "https://example.com/index.mpd")
    private val segment = mapOf("segmentId" to "segment", "uri" to "https://example.com/1.m4s")
    private val stream = mapOf("streamId" to "stream")

    @Test
    fun malformedGraphCollectionsRejectTheWholePlanEvenWithValidManifestResources() {
        for ((key, record) in listOf("resources" to resource, "segments" to segment, "streams" to stream)) {
            for (bad in listOf(null, 7, "not a record")) {
                assertInvalid(key) {
                    mapOf("resources" to listOf(resource), key to listOf(record, bad)).toDownloadAssetIndex()
                }
            }
            for (bad in listOf(7, "not a list", mapOf("record" to record))) {
                assertInvalid(key) { mapOf(key to bad).toDownloadAssetIndex() }
            }
        }
    }

    @Test
    fun graphReferencesAndSelectedTracksRejectWrongTypesWithoutStringifying() {
        for (key in listOf("resourceIds", "segmentIds")) {
            for (bad in listOf(listOf("valid", null), listOf("valid", 7), "not a list")) {
                assertInvalid(key) {
                    mapOf("streams" to listOf(stream + (key to bad))).toDownloadAssetIndex()
                }
            }
        }
        for (bad in listOf(listOf("valid", null), listOf("valid", 7), "not a list")) {
            assertInvalid("selectedTrackIds") { mapOf("selectedTrackIds" to bad).toDownloadProfile() }
        }
        for ((key, record) in listOf("resourceId" to resource, "segmentId" to segment, "streamId" to stream)) {
            assertInvalid(key) {
                when (key) {
                    "resourceId" -> (record + (key to 7)).toDownloadResourceRecord()
                    "segmentId" -> (record + (key to 7)).toDownloadSegmentRecord()
                    else -> (record + (key to 7)).toDownloadAssetStream()
                }
            }
        }
    }

    @Test
    fun adjacentGraphInputsRejectDiscardedConstraintsAndInvalidKeys() {
        assertInvalid("byteRange") { (resource + ("byteRange" to "not a map")).toDownloadResourceRecord() }
        assertInvalid("metadata") { (stream + ("metadata" to mapOf("key" to 7))).toDownloadAssetStream() }
        assertInvalid("metadata") { (stream + ("metadata" to mapOf(7 to "value"))).toDownloadAssetStream() }
        assertInvalid("resources") {
            mapOf("resources" to listOf(mapOf(7 to "field", "resourceId" to "r", "uri" to "https://example.com")))
                .toDownloadAssetIndex()
        }
        assertInvalid("kind") { (stream + ("kind" to "future-kind")).toDownloadAssetStream() }
        assertInvalid("targetOutputFormat") { mapOf("targetOutputFormat" to "future-format").toDownloadProfile() }
        assertInvalid("targetOutputFormat") { mapOf("targetOutputFormat" to 7).toDownloadProfile() }
        assertInvalid("externalSubtitles") {
            mapOf("uri" to "https://example.com/video", "externalSubtitles" to "not a list").toVesperPlayerSource()
        }
        for (key in listOf("audioSelection", "subtitleSelection", "abrPolicy")) {
            assertInvalid(key) { mapOf(key to "not a map").toTrackPreferencePolicy() }
        }
        for (key in listOf("buffering", "retry", "cache")) {
            assertInvalid(key) { mapOf(key to "not a map").toResiliencePolicy() }
        }
        assertInvalid("renderSurfaceKind") { 7.toVesperVideoSurfaceKind() }
    }

    @Test
    fun malformedByteRangesFailInsteadOfBecomingZeroOffsetsOrLengths() {
        for (key in listOf("offset", "length")) {
            for (bad in listOf("64", true, 1.5, emptyList<Any>(), emptyMap<String, Any>())) {
                assertInvalid(key) {
                    (resource + ("byteRange" to mapOf("offset" to 64L, "length" to 128L, key to bad)))
                        .toDownloadResourceRecord()
                }
            }
        }
        val range = mapOf("offset" to 64, "length" to 128L).toDownloadByteRange()
        assertEquals(64L, range.offset)
        assertEquals(128L, range.length)
        assertEquals(0L, emptyMap<String, Any?>().toDownloadByteRange().offset)
    }

    @Test
    fun optionalCollectionsKeepAbsentNullAndEmptyDefaults() {
        for (index in listOf(emptyMap(), mapOf("resources" to null, "segments" to null, "streams" to null),
            mapOf("resources" to emptyList<Any>(), "segments" to emptyList<Any>(), "streams" to emptyList<Any>()))) {
            val decoded = index.toDownloadAssetIndex()
            assertTrue(decoded.resources.isEmpty())
            assertTrue(decoded.segments.isEmpty())
            assertTrue(decoded.streams.isEmpty())
        }
        for (value in listOf(null, emptyList<String>())) {
            val decoded = (stream + mapOf("resourceIds" to value, "segmentIds" to value)).toDownloadAssetStream()
            assertTrue(decoded.resourceIds.isEmpty())
            assertTrue(decoded.segmentIds.isEmpty())
            assertTrue(mapOf("selectedTrackIds" to value).toDownloadProfile().selectedTrackIds.isEmpty())
        }
    }

    @Test
    fun awkwardValidIdentitiesAndOrderingSurviveDecoding() {
        val ids = listOf(" 中文 / audio:#?% ", "001", "e\u0301", "é")
        val decoded = mapOf(
            "resources" to ids.map { resource + ("resourceId" to it) },
            "segments" to ids.map { segment + ("segmentId" to it) },
            "streams" to listOf(stream + mapOf("streamId" to ids[0], "resourceIds" to ids, "segmentIds" to ids)),
        ).toDownloadAssetIndex()
        assertEquals(ids, decoded.resources.map { it.resourceId })
        assertEquals(ids, decoded.segments.map { it.segmentId })
        assertEquals(ids[0], decoded.streams.single().streamId)
        assertEquals(ids, decoded.streams.single().resourceIds)
        assertEquals(ids, decoded.streams.single().segmentIds)
        assertEquals(ids, mapOf("selectedTrackIds" to ids).toDownloadProfile().selectedTrackIds)
        assertEquals(ids[0], mapOf("mode" to "track", "trackId" to ids[0]).toTrackSelection().trackId)
        assertEquals(ids[0], mapOf("mode" to "fixedTrack", "trackId" to ids[0]).toAbrPolicy().trackId)
    }

    @Test
    fun malformedRecoveryCompletesAwaiterWithFailureInsteadOfEscapingCallback() = runBlocking {
        val source = mapOf("source" to mapOf("uri" to "https://example.com/index.mpd"))
        for (response in listOf("not a plan", mapOf("source" to source, "profile" to emptyMap<String, Any?>(),
            "assetIndex" to mapOf("resources" to listOf(resource), "segments" to listOf(null))))) {
            val deferred = CompletableDeferred<VesperDownloadRecoveredTaskPlan?>()
            completeDownloadRecoveryResponse(response, deferred)
            assertTrue(deferred.isCompleted)
            assertInvalid(null) { deferred.await() }
        }
        val declined = CompletableDeferred<VesperDownloadRecoveredTaskPlan?>()
        completeDownloadRecoveryResponse(null, declined)
        assertNull(declined.await())
        val valid = CompletableDeferred<VesperDownloadRecoveredTaskPlan?>()
        completeDownloadRecoveryResponse(mapOf("source" to source, "profile" to emptyMap<String, Any?>(),
            "assetIndex" to mapOf("resources" to listOf(resource))), valid)
        assertEquals("manifest", valid.await()!!.assetIndex.resources.single().resourceId)
    }

    @Test
    fun supportedDefaultsAndRequiredModesRemainValid() {
        for (mode in listOf(emptyMap(), mapOf("mode" to null), mapOf("mode" to "auto"))) {
            assertEquals(VesperTrackSelection.auto(), mode.toTrackSelection())
            assertEquals(VesperAbrPolicy.auto(), mode.toAbrPolicy())
        }
        for (mode in listOf(emptyMap(), mapOf("mode" to null), mapOf("mode" to "disabled"))) {
            assertEquals(VesperSourceNormalizerMode.Disabled, mode.toSourceNormalizerConfiguration().mode)
            assertEquals(VesperFrameProcessorMode.Disabled, mode.toFrameProcessorConfiguration().mode)
            assertEquals(VesperNativeFramePipelineMode.Disabled, mode.toNativeFramePipelineConfiguration().mode)
        }
        assertEquals(VesperSourceNormalizerMode.RequireNormalized,
            mapOf("mode" to "requireNormalized").toSourceNormalizerConfiguration().mode)
        assertEquals(VesperNativeFramePipelineMode.RequireNativeFrame,
            mapOf("mode" to "requireNativeFrame").toNativeFramePipelineConfiguration().mode)
    }

    @Test
    fun explicitlyInvalidModesFailThroughExistingCommandErrorContract() {
        for (value in listOf("future-mode", 7, false, emptyList<String>())) {
            val map = mapOf("mode" to value)
            val decoders: List<() -> Any> = listOf(
                { map.toTrackSelection() }, { map.toAbrPolicy() },
                { map.toSourceNormalizerConfiguration() }, { map.toFrameProcessorConfiguration() },
                { map.toNativeFramePipelineConfiguration() },
            )
            for (decode in decoders) {
                val error = try { decode(); fail("Invalid mode must fail"); error("unreachable") }
                    catch (error: IllegalArgumentException) { error }
                var methodErrors = 0
                routeAsyncSessionCommandFailure(error, true, {}, { code, _, details ->
                    methodErrors += 1
                    assertEquals("vesper_operation_failed", code)
                    assertEquals(false, details["retriable"])
                    assertEquals(IllegalArgumentException::class.java.name,
                        (details["details"] as Map<*, *>)["exception"])
                })
                assertEquals(1, methodErrors)
            }
        }
    }

    @Test
    fun invalidCreationConfigurationReturnsChannelErrorBeforeCreatingNativePlayer() {
        for (key in listOf("sourceNormalizer", "frameProcessor", "nativeFramePipeline", "trackPreferencePolicy")) {
            for (bad in listOf("not a map", mapOf("mode" to "future-mode"))) {
                val value = if (key == "trackPreferencePolicy" && bad is Map<*, *>)
                    mapOf("audioSelection" to bad) else bad
                var errors = 0
                VesperPlayerAndroidPlugin().onMethodCall(MethodCall("createPlayer", mapOf(key to value)),
                    object : MethodChannel.Result {
                        override fun success(result: Any?) { fail("Invalid creation must fail") }
                        override fun notImplemented() { fail("Creation must be implemented") }
                        override fun error(code: String, message: String?, details: Any?) {
                            errors += 1
                            assertEquals("vesper_create_failed", code)
                            val payload = details as Map<*, *>
                            assertEquals(false, payload["retriable"])
                            assertEquals(IllegalArgumentException::class.java.name,
                                (payload["details"] as Map<*, *>)["exception"])
                        }
                    })
                assertEquals(1, errors)
            }
        }
    }

    private inline fun assertInvalid(field: String?, action: () -> Any?) {
        try {
            action()
            fail("Malformed supplied input must fail")
        } catch (error: IllegalArgumentException) {
            if (field != null) assertTrue(error.message.orEmpty(), error.message.orEmpty().contains(field))
        }
    }
}
