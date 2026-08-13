package io.github.umbrella22.vesper.player.android

import android.content.Context
import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test
import java.util.Collections
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicBoolean

class VesperBenchmarkRecorderTest {
    @Test
    fun disabledRecorderRejectsEvents() {
        val recorder = VesperBenchmarkRecorder()

        recorder.record("dropped_video_frames", null, mapOf("count" to "1"))

        assertTrue(recorder.drainEvents().isEmpty())
        assertEquals(0L, recorder.summary().acceptedEvents)
    }

    @Test
    fun disposedRecorderRejectsLaterEvents() {
        val recorder =
            VesperBenchmarkRecorder(
                VesperBenchmarkConfiguration(enabled = true),
            )
        recorder.record("first_frame_rendered", null)

        recorder.dispose()
        recorder.record("dropped_video_frames", null, mapOf("count" to "1"))

        assertEquals(listOf("first_frame_rendered"), recorder.drainEvents().map { it.eventName })
        assertEquals(1L, recorder.summary().acceptedEvents)
    }

    @Test
    fun disposeFlushesSessionThenClosesRegistryExactlyOnce() {
        val operations = mutableListOf<String>()
        val runtime = RecordingBenchmarkSinkRuntime(operations)
        val recorder = VesperBenchmarkRecorder(configuration(), sinkRuntime = runtime)

        recorder.record("play_command", VesperPlayerSourceProtocol.Hls)
        recorder.dispose()
        recorder.dispose()
        recorder.flushSinks()
        assertTrue(recorder.awaitSinkShutdown(2_000))

        assertEquals(
            listOf("open", "submit", "flush", "dispose", "close"),
            operations,
        )
        assertEquals(1L, recorder.summary().pluginAcceptedEvents)
        assertEquals(0L, recorder.summary().pluginFinalReport?.acceptedEvents)
    }

    @Test
    fun registryOpenFailureIsReportedWithoutUsingASession() {
        val runtime =
            object : VesperBenchmarkSinkRuntime {
                override fun open(
                    context: Context?,
                    references: List<VesperPluginReference>,
                ): VesperBenchmarkSinkConnection = throw IllegalStateException("registry rejected")

                override fun submit(
                    sessionHandle: Long,
                    batchJson: String,
                ): String = error("unexpected submit")

                override fun flush(sessionHandle: Long): String = error("unexpected flush")

                override fun dispose(sessionHandle: Long) = error("unexpected dispose")
            }
        val recorder = VesperBenchmarkRecorder(configuration(), sinkRuntime = runtime)

        recorder.record("play_command", null)
        recorder.dispose()
        assertTrue(recorder.awaitSinkShutdown(2_000))

        assertTrue(recorder.summary().pluginErrors.any { it.contains("registry rejected") })
    }

    @Test
    fun registryClosesWhenNativeSessionDisposeFails() {
        val operations = mutableListOf<String>()
        val runtime = RecordingBenchmarkSinkRuntime(operations, failDispose = true)
        val recorder = VesperBenchmarkRecorder(configuration(), sinkRuntime = runtime)

        recorder.dispose()
        assertTrue(recorder.awaitSinkShutdown(2_000))

        assertEquals(listOf("open", "flush", "dispose", "close"), operations)
        assertTrue(recorder.summary().pluginErrors.any { it.contains("dispose rejected") })
    }

    @Test
    fun previewRejectsBenchmarkPluginReferencesWithoutAndroidContext() {
        val error =
            org.junit.Assert.assertThrows(IllegalArgumentException::class.java) {
                VesperPlayerControllerFactory.createPreview(
                    benchmarkConfiguration = configuration(),
                )
            }

        assertTrue(error.message.orEmpty().contains("Android Context"))
    }

    @Test
    fun concurrentDisposeSerializesSubmitBeforeFinalFlushAndClose() {
        val operations = Collections.synchronizedList(mutableListOf<String>())
        val submitStarted = CountDownLatch(1)
        val allowSubmit = CountDownLatch(1)
        val runtime =
            object : RecordingBenchmarkSinkRuntime(operations) {
                override fun submit(
                    sessionHandle: Long,
                    batchJson: String,
                ): String {
                    operations += "submit-start"
                    submitStarted.countDown()
                    allowSubmit.await(2, TimeUnit.SECONDS)
                    operations += "submit-end"
                    return emptyBenchmarkReportJson(acceptedEvents = 1)
                }
            }
        val recorder = VesperBenchmarkRecorder(configuration(), sinkRuntime = runtime)
        val recordThread = Thread {
            recorder.record("play_command", null)
        }
        recordThread.start()
        assertTrue(submitStarted.await(2, TimeUnit.SECONDS))

        val disposeThread = Thread { recorder.dispose() }
        disposeThread.start()
        Thread.sleep(20)
        allowSubmit.countDown()
        recordThread.join(2_000)
        disposeThread.join(2_000)
        assertTrue(recorder.awaitSinkShutdown(2_000))

        assertEquals(
            listOf("open", "submit-start", "submit-end", "flush", "dispose", "close"),
            operations,
        )
    }

    @Test
    fun fullQueueDropsIncomingEventWithoutReplacingNewestPendingEvent() {
        val operations = Collections.synchronizedList(mutableListOf<String>())
        val submittedEventNames = Collections.synchronizedList(mutableListOf<String>())
        val openStarted = CountDownLatch(1)
        val allowOpen = CountDownLatch(1)
        val firstSubmitStarted = CountDownLatch(1)
        val allowFirstSubmit = CountDownLatch(1)
        val firstSubmit = AtomicBoolean(true)
        val runtime =
            object : RecordingBenchmarkSinkRuntime(operations) {
                override fun open(
                    context: Context?,
                    references: List<VesperPluginReference>,
                ): VesperBenchmarkSinkConnection {
                    operations += "open"
                    openStarted.countDown()
                    check(allowOpen.await(2, TimeUnit.SECONDS)) { "open was not released" }
                    return connection(operations)
                }

                override fun submit(
                    sessionHandle: Long,
                    batchJson: String,
                ): String {
                    val eventName =
                        JSONObject(batchJson)
                            .getJSONArray("events")
                            .getJSONObject(0)
                            .getString("eventName")
                    submittedEventNames += eventName
                    if (firstSubmit.compareAndSet(true, false)) {
                        firstSubmitStarted.countDown()
                        check(allowFirstSubmit.await(2, TimeUnit.SECONDS)) {
                            "first submit was not released"
                        }
                    }
                    return emptyBenchmarkReportJson(acceptedEvents = 1)
                }
            }
        val recorder = VesperBenchmarkRecorder(configuration(), sinkRuntime = runtime)
        assertTrue(openStarted.await(2, TimeUnit.SECONDS))

        repeat(1_025) { index ->
            recorder.record("event-$index", null)
        }
        assertEquals(1L, recorder.summary().pluginDroppedEvents)

        allowOpen.countDown()
        assertTrue(firstSubmitStarted.await(2, TimeUnit.SECONDS))
        recorder.dispose()
        allowFirstSubmit.countDown()
        assertTrue(recorder.awaitSinkShutdown(5_000))

        assertEquals(1_024, submittedEventNames.size)
        assertTrue(submittedEventNames.contains("event-1023"))
        assertFalse(submittedEventNames.contains("event-1024"))
        assertEquals(1_024L, recorder.summary().pluginAcceptedEvents)
        assertEquals(1L, recorder.summary().pluginDroppedEvents)
        assertEquals(listOf("flush", "dispose", "close"), operations.takeLast(3))
    }

    @Test
    fun fullQueueKeepsFlushAndDisposeControls() {
        val operations = Collections.synchronizedList(mutableListOf<String>())
        val openStarted = CountDownLatch(1)
        val allowOpen = CountDownLatch(1)
        val runtime =
            object : RecordingBenchmarkSinkRuntime(operations) {
                override fun open(
                    context: Context?,
                    references: List<VesperPluginReference>,
                ): VesperBenchmarkSinkConnection {
                    operations += "open"
                    openStarted.countDown()
                    check(allowOpen.await(2, TimeUnit.SECONDS)) { "open was not released" }
                    return connection(operations)
                }
            }
        val recorder = VesperBenchmarkRecorder(configuration(), sinkRuntime = runtime)
        assertTrue(openStarted.await(2, TimeUnit.SECONDS))
        repeat(1_024) { index ->
            recorder.record("event-$index", null)
        }

        recorder.flushSinks()
        recorder.dispose()
        allowOpen.countDown()
        assertTrue(recorder.awaitSinkShutdown(5_000))

        assertEquals(2L, recorder.summary().pluginDroppedEvents)
        assertEquals(1_022L, recorder.summary().pluginAcceptedEvents)
        assertEquals(listOf("flush", "flush", "dispose", "close"), operations.takeLast(4))
    }

    @Test
    fun finalReportDoesNotInflateSubmitCountersAndPreservesUnknownSeverity() {
        val operations = mutableListOf<String>()
        val runtime =
            object : RecordingBenchmarkSinkRuntime(operations) {
                override fun flush(sessionHandle: Long): String {
                    operations += "flush"
                    return """
                        {
                          "acceptedEvents": 3,
                          "droppedEvents": 2,
                          "measurements": [
                            {
                              "name": "startup",
                              "value": 12.5,
                              "unit": "ms",
                              "attributes": {"route": "native"}
                            }
                          ],
                          "thresholdViolations": [
                            {
                              "measurement": "startup",
                              "actual": 12.5,
                              "threshold": 10.0,
                              "comparison": "greaterThan"
                            }
                          ],
                          "diagnostics": [
                            {
                              "code": "benchmark.future",
                              "severity": "critical-future",
                              "message": "future severity retained",
                              "attributes": {"plugin": "fixture"}
                            }
                          ]
                        }
                    """.trimIndent()
                }
            }
        val recorder = VesperBenchmarkRecorder(configuration(), sinkRuntime = runtime)

        recorder.record("play_command", null)
        recorder.dispose()
        assertTrue(recorder.awaitSinkShutdown(2_000))

        val summary = recorder.summary()
        val finalReport = requireNotNull(summary.pluginFinalReport)
        assertEquals(1L, summary.pluginAcceptedEvents)
        assertEquals(0L, summary.pluginDroppedEvents)
        assertEquals(3L, finalReport.acceptedEvents)
        assertEquals(2L, finalReport.droppedEvents)
        assertEquals("startup", finalReport.measurements.single().name)
        assertEquals(
            10.0,
            finalReport.thresholdViolations.single().threshold,
            0.0,
        )
        assertEquals(
            "critical-future",
            finalReport.diagnostics.single().severity.rawValue,
        )
        assertTrue(summary.pluginErrors.isEmpty())
    }

    @Test
    fun shutdownTimeoutIsReportedAtMostOnce() {
        val operations = Collections.synchronizedList(mutableListOf<String>())
        val flushStarted = CountDownLatch(1)
        val allowFlush = CountDownLatch(1)
        val runtime =
            object : RecordingBenchmarkSinkRuntime(operations) {
                override fun flush(sessionHandle: Long): String {
                    operations += "flush"
                    flushStarted.countDown()
                    check(allowFlush.await(2, TimeUnit.SECONDS)) { "flush was not released" }
                    return emptyBenchmarkReportJson()
                }
            }
        val recorder = VesperBenchmarkRecorder(configuration(), sinkRuntime = runtime)

        recorder.dispose()
        assertTrue(flushStarted.await(2, TimeUnit.SECONDS))
        assertFalse(recorder.awaitSinkShutdown(0))
        assertFalse(recorder.awaitSinkShutdown(0))
        assertEquals(
            1,
            recorder.summary().pluginErrors.count {
                it == "benchmark sink shutdown timed out"
            },
        )

        allowFlush.countDown()
        assertTrue(recorder.awaitSinkShutdown(2_000))
    }

    private fun configuration() =
        VesperBenchmarkConfiguration(
            enabled = true,
            pluginReferences =
                listOf(
                    VesperPluginReference(
                        pluginId = "dev.vesper.benchmark-sink",
                        capabilityInstanceId = "dev.vesper.benchmark-sink.default",
                        transport = VesperPluginTransport.Native,
                    ),
                ),
        )

    private open class RecordingBenchmarkSinkRuntime(
        private val operations: MutableList<String>,
        private val failDispose: Boolean = false,
    ) : VesperBenchmarkSinkRuntime {
        override fun open(
            context: Context?,
            references: List<VesperPluginReference>,
        ): VesperBenchmarkSinkConnection {
            operations += "open"
            return connection(operations)
        }

        override fun submit(
            sessionHandle: Long,
            batchJson: String,
        ): String {
            operations += "submit"
            return emptyBenchmarkReportJson(acceptedEvents = 1)
        }

        override fun flush(sessionHandle: Long): String {
            operations += "flush"
            return emptyBenchmarkReportJson()
        }

        override fun dispose(sessionHandle: Long) {
            operations += "dispose"
            if (failDispose) {
                throw IllegalStateException("dispose rejected")
            }
        }
    }

    companion object {
        private fun emptyBenchmarkReportJson(
            acceptedEvents: Long = 0,
            droppedEvents: Long = 0,
        ): String =
            """
                {
                  "acceptedEvents": $acceptedEvents,
                  "droppedEvents": $droppedEvents,
                  "measurements": [],
                  "thresholdViolations": [],
                  "diagnostics": []
                }
            """.trimIndent()

        private fun connection(operations: MutableList<String>) =
            VesperBenchmarkSinkConnection(
                sessionHandle = 42L,
                registry = AutoCloseable { operations += "close" },
            )
    }
}
