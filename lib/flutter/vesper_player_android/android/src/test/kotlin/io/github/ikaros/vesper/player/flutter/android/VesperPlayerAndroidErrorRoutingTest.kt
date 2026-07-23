package io.github.ikaros.vesper.player.flutter.android

import io.github.ikaros.vesper.player.android.VesperPlayerUnsupportedOperation
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertSame
import org.junit.Test

class VesperPlayerAndroidErrorRoutingTest {
    @Test
    fun obsoleteSubtitleSelectionFailuresOnlyReturnTheMethodError() {
        listOf(
            "subtitle_selection_cancelled",
            "subtitle_source_changed",
            "subtitle_selection_superseded",
        ).forEachIndexed { index, subtitleCode ->
            val sentinelLastError = mapOf<String, Any?>("code" to "newer_error")
            var lastError = sentinelLastError
            var publishedErrorCount = 0
            var methodErrorCount = 0
            var returnedCode: String? = null
            var returnedDetails: Map<String, Any?>? = null
            val commandId = 40L + index
            val sourceEpoch = 9L + index

            routeAsyncSessionCommandFailure(
                error = subtitleFailure(subtitleCode, commandId, sourceEpoch),
                isCurrentSession = true,
                publishPlayerError = { publishedError ->
                    publishedErrorCount += 1
                    lastError = publishedError
                },
                returnMethodError = { code, _, details ->
                    methodErrorCount += 1
                    returnedCode = code
                    returnedDetails = details
                },
            )

            assertSame(sentinelLastError, lastError)
            assertEquals(0, publishedErrorCount)
            assertEquals(1, methodErrorCount)
            assertEquals("vesper_subtitle_error", returnedCode)
            assertEquals(subtitleCode, returnedDetails?.get("code"))
            assertEquals(commandId, returnedDetails?.get("commandId"))
            assertEquals(sourceEpoch, returnedDetails?.get("sourceEpoch"))
        }
    }

    @Test
    fun currentAndUnknownSubtitleFailuresStillPublishPlayerErrors() {
        listOf(
            "subtitle_selection_timeout",
            "future_subtitle_failure",
        ).forEach { subtitleCode ->
            var publishedError: Map<String, Any?>? = null
            var returnedDetails: Map<String, Any?>? = null

            routeAsyncSessionCommandFailure(
                error = subtitleFailure(subtitleCode, commandId = 51L, sourceEpoch = 12L),
                isCurrentSession = true,
                publishPlayerError = { publishedError = it },
                returnMethodError = { code, _, details ->
                    assertEquals("vesper_subtitle_error", code)
                    returnedDetails = details
                },
            )

            assertEquals("backendFailure", publishedError?.get("code"))
            assertEquals(returnedDetails, publishedError?.get("details"))
        }
    }

    @Test
    fun staleSessionsNeverPublishButStillReturnTheMethodError() {
        var publishedError: Map<String, Any?>? = null
        var returnedCode: String? = null

        routeAsyncSessionCommandFailure(
            error = IllegalStateException("stale failure"),
            isCurrentSession = false,
            publishPlayerError = { publishedError = it },
            returnMethodError = { code, _, _ -> returnedCode = code },
        )

        assertNull(publishedError)
        assertEquals("vesper_operation_failed", returnedCode)
    }

    private fun subtitleFailure(
        code: String,
        commandId: Long,
        sourceEpoch: Long,
    ): VesperPlayerUnsupportedOperation =
        VesperPlayerUnsupportedOperation(
            "subtitle selection failed",
            mapOf(
                "domain" to "subtitle",
                "code" to code,
                "phase" to "selection",
                "trackId" to "subtitle-b",
                "retriable" to true,
                "message" to "subtitle selection failed",
                "commandId" to commandId,
                "sourceEpoch" to sourceEpoch,
            ),
        )
}
