package io.github.umbrella22.vesper.player.android

import android.app.KeyguardManager
import android.content.Context
import android.content.Intent
import android.content.IntentFilter
import android.os.BatteryManager
import android.os.Build
import android.os.Debug
import android.os.PowerManager
import android.os.Process
import android.os.SystemClock
import java.io.Closeable
import java.io.File
import java.io.FileOutputStream
import java.io.Writer
import org.json.JSONArray
import org.json.JSONObject

/** Writes test-only process, playback, frame, thermal, and battery evidence as JSONL. */
internal class VesperAndroidDeviceEvidenceWriter(
    context: Context,
    testId: String,
    append: Boolean = false,
    fileSuffix: String? = null,
) : Closeable {
    val file: File = evidenceFile(context, testId, fileSuffix)
    val processIdentity: String = processIdentity()

    private val appContext = context.applicationContext
    private val batteryManager = appContext.getSystemService(BatteryManager::class.java)
    private val keyguardManager = appContext.getSystemService(KeyguardManager::class.java)
    private val powerManager = appContext.getSystemService(PowerManager::class.java)
    private val startedElapsedRealtimeMs = SystemClock.elapsedRealtime()
    private val writer: Writer
    private val eventCounts = linkedMapOf<String, Long>()
    private val decoderNames = linkedSetOf<String>()
    private var lastBenchmarkElapsedNs = -1L
    private var droppedVideoFrameCount = 0L
    private var frameMetadataWindowCount = 0L
    private var frameMetadataFrameCount = 0L
    private var nonMonotonicPresentationCount = 0L
    private var releaseTimestampRegressionCount = 0L
    private var largePresentationGapCount = 0L
    private var estimatedMissingFrameCount = 0L
    private var maximumPresentationGapUs = 0L
    private var minimumScheduledRateMilli: Long? = null
    private var maximumScheduledRateMilli: Long? = null
    private var minimumPresentationRateMilli: Long? = null
    private var maximumPresentationRateMilli: Long? = null

    init {
        file.parentFile?.mkdirs()
        writer = FileOutputStream(file, append).bufferedWriter()
    }

    fun record(
        phase: String,
        state: PlayerHostUiState? = null,
        events: List<VesperBenchmarkEvent> = emptyList(),
        details: Map<String, Any?> = emptyMap(),
        captureDetailedMemory: Boolean = true,
    ) {
        val newEvents = collectNewEvents(events)
        val memoryInfo =
            if (captureDetailedMemory) {
                Debug.MemoryInfo().also(Debug::getMemoryInfo)
            } else {
                null
            }
        val runtime = Runtime.getRuntime()
        val batteryIntent =
            appContext.registerReceiver(null, IntentFilter(Intent.ACTION_BATTERY_CHANGED))

        val record =
            JSONObject()
                .put("schemaVersion", SCHEMA_VERSION)
                .put("phase", phase)
                .put("wallTimeMs", System.currentTimeMillis())
                .put("elapsedRealtimeMs", SystemClock.elapsedRealtime())
                .put("testElapsedMs", SystemClock.elapsedRealtime() - startedElapsedRealtimeMs)
                .put("uptimeMs", SystemClock.uptimeMillis())
                .put("pid", Process.myPid())
                .put("processIdentity", processIdentity)
                .put("packageName", appContext.packageName)
                .put("evidenceFile", file.name)
                .put("processCpuTimeMs", Process.getElapsedCpuTime())
                .put("openFileDescriptorCount", File("/proc/self/fd").list()?.size ?: -1)
                .put("threadCount", File("/proc/self/task").list()?.size ?: -1)
                .put("memory", memoryJson(memoryInfo, runtime))
                .put("power", powerJson(batteryIntent))
                .put("benchmark", benchmarkJson())
                .put("newBenchmarkEvents", benchmarkEventsJson(newEvents))
                .put("details", JSONObject(details))
        state?.let { record.put("playback", playbackJson(it)) }

        writer.append(record.toString()).append('\n')
        writer.flush()
    }

    override fun close() {
        writer.close()
    }

    private fun collectNewEvents(events: List<VesperBenchmarkEvent>): List<VesperBenchmarkEvent> {
        val newEvents =
            events
                .asSequence()
                .filter { it.elapsedNs > lastBenchmarkElapsedNs }
                .sortedBy(VesperBenchmarkEvent::elapsedNs)
                .toList()
        if (newEvents.isEmpty()) return emptyList()

        lastBenchmarkElapsedNs = newEvents.last().elapsedNs
        newEvents.forEach(::accumulateEvent)
        return newEvents
    }

    private fun accumulateEvent(event: VesperBenchmarkEvent) {
        eventCounts[event.eventName] = (eventCounts[event.eventName] ?: 0L) + 1L
        when (event.eventName) {
            VIDEO_DECODER_INITIALIZED_EVENT -> {
                event.attributes["decoderName"]?.takeIf(String::isNotBlank)?.let(decoderNames::add)
            }
            DROPPED_VIDEO_FRAMES_EVENT -> {
                droppedVideoFrameCount += event.attributes.longValue("count")
            }
            VIDEO_FRAME_METADATA_WINDOW_EVENT -> {
                frameMetadataWindowCount += 1L
                frameMetadataFrameCount += event.attributes.longValue("frameCount")
                nonMonotonicPresentationCount +=
                    event.attributes.longValue("nonMonotonicPresentationCount")
                releaseTimestampRegressionCount +=
                    event.attributes.longValue("releaseTimestampRegressionCount")
                largePresentationGapCount +=
                    event.attributes.longValue("largePresentationGapCount")
                estimatedMissingFrameCount +=
                    event.attributes.longValue("estimatedMissingFrameCount")
                maximumPresentationGapUs =
                    maxOf(
                        maximumPresentationGapUs,
                        event.attributes.longValue("maximumPresentationGapUs"),
                    )
                event.attributes["scheduledRateMilli"]?.toLongOrNull()?.let { rate ->
                    minimumScheduledRateMilli = minimumScheduledRateMilli?.let { minOf(it, rate) } ?: rate
                    maximumScheduledRateMilli = maximumScheduledRateMilli?.let { maxOf(it, rate) } ?: rate
                }
                event.attributes["presentationRateMilli"]?.toLongOrNull()?.let { rate ->
                    minimumPresentationRateMilli = minimumPresentationRateMilli?.let { minOf(it, rate) } ?: rate
                    maximumPresentationRateMilli = maximumPresentationRateMilli?.let { maxOf(it, rate) } ?: rate
                }
            }
        }
    }

    private fun memoryJson(
        memoryInfo: Debug.MemoryInfo?,
        runtime: Runtime,
    ): JSONObject {
        val memoryStats = JSONObject()
        memoryInfo?.memoryStats?.toSortedMap()?.forEach(memoryStats::put)
        return JSONObject()
            .put("detailedPssCaptured", memoryInfo != null)
            .put("totalPssKb", memoryInfo?.totalPss ?: JSONObject.NULL)
            .put("dalvikPssKb", memoryInfo?.dalvikPss ?: JSONObject.NULL)
            .put("nativePssKb", memoryInfo?.nativePss ?: JSONObject.NULL)
            .put("otherPssKb", memoryInfo?.otherPss ?: JSONObject.NULL)
            .put("totalPrivateDirtyKb", memoryInfo?.totalPrivateDirty ?: JSONObject.NULL)
            .put("totalSharedDirtyKb", memoryInfo?.totalSharedDirty ?: JSONObject.NULL)
            .put("totalSwappablePssKb", memoryInfo?.totalSwappablePss ?: JSONObject.NULL)
            .put("javaHeapUsedBytes", runtime.totalMemory() - runtime.freeMemory())
            .put("javaHeapCommittedBytes", runtime.totalMemory())
            .put("javaHeapMaxBytes", runtime.maxMemory())
            .put("nativeHeapAllocatedBytes", Debug.getNativeHeapAllocatedSize())
            .put("nativeHeapFreeBytes", Debug.getNativeHeapFreeSize())
            .put("nativeHeapSizeBytes", Debug.getNativeHeapSize())
            .put("memoryStats", memoryStats)
    }

    private fun powerJson(batteryIntent: Intent?): JSONObject {
        val battery =
            JSONObject()
                .put("level", batteryIntent?.getIntExtra(BatteryManager.EXTRA_LEVEL, -1) ?: -1)
                .put("scale", batteryIntent?.getIntExtra(BatteryManager.EXTRA_SCALE, -1) ?: -1)
                .put(
                    "temperatureTenthsC",
                    batteryIntent?.getIntExtra(BatteryManager.EXTRA_TEMPERATURE, Int.MIN_VALUE)
                        ?: Int.MIN_VALUE,
                )
                .put("voltageMv", batteryIntent?.getIntExtra(BatteryManager.EXTRA_VOLTAGE, -1) ?: -1)
                .put("plugged", batteryIntent?.getIntExtra(BatteryManager.EXTRA_PLUGGED, -1) ?: -1)
                .put("status", batteryIntent?.getIntExtra(BatteryManager.EXTRA_STATUS, -1) ?: -1)
                .put("capacityPercent", batteryProperty(BatteryManager.BATTERY_PROPERTY_CAPACITY))
                .put("chargeCounterUah", batteryProperty(BatteryManager.BATTERY_PROPERTY_CHARGE_COUNTER))
                .put("currentNowUa", batteryProperty(BatteryManager.BATTERY_PROPERTY_CURRENT_NOW))
                .put("currentAverageUa", batteryProperty(BatteryManager.BATTERY_PROPERTY_CURRENT_AVERAGE))
                .put("energyCounterNwh", batteryProperty(BatteryManager.BATTERY_PROPERTY_ENERGY_COUNTER))
        return JSONObject()
            .put("interactive", powerManager?.isInteractive ?: false)
            .put("keyguardLocked", keyguardManager?.isKeyguardLocked ?: false)
            .put(
                "thermalStatus",
                if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
                    powerManager?.currentThermalStatus ?: PowerManager.THERMAL_STATUS_NONE
                } else {
                    JSONObject.NULL
                },
            )
            .put("battery", battery)
    }

    private fun batteryProperty(property: Int): Any {
        val value = batteryManager?.getLongProperty(property) ?: Long.MIN_VALUE
        return if (value == Long.MIN_VALUE || value == Int.MIN_VALUE.toLong()) JSONObject.NULL else value
    }

    private fun playbackJson(state: PlayerHostUiState): JSONObject =
        JSONObject()
            .put("state", state.playbackState.name)
            .put("buffering", state.isBuffering)
            .put("positionMs", state.timeline.positionMs)
            .put("durationMs", state.timeline.durationMs ?: JSONObject.NULL)
            .put("timelineKind", state.timeline.kind.name)
            .put("seekable", state.timeline.isSeekable)
            .put("liveOffsetMs", state.timeline.liveOffsetMs ?: JSONObject.NULL)
            .put("error", state.lastError?.toString() ?: JSONObject.NULL)

    private fun benchmarkJson(): JSONObject =
        JSONObject()
            .put("eventCounts", JSONObject(eventCounts.toSortedMap()))
            .put("decoderNames", JSONArray(decoderNames.toList()))
            .put("droppedVideoFrameCount", droppedVideoFrameCount)
            .put("frameMetadataWindowCount", frameMetadataWindowCount)
            .put("frameMetadataFrameCount", frameMetadataFrameCount)
            .put("nonMonotonicPresentationCount", nonMonotonicPresentationCount)
            .put("releaseTimestampRegressionCount", releaseTimestampRegressionCount)
            .put("largePresentationGapCount", largePresentationGapCount)
            .put("estimatedMissingFrameCount", estimatedMissingFrameCount)
            .put("maximumPresentationGapUs", maximumPresentationGapUs)
            .put("minimumScheduledRateMilli", minimumScheduledRateMilli ?: JSONObject.NULL)
            .put("maximumScheduledRateMilli", maximumScheduledRateMilli ?: JSONObject.NULL)
            .put("minimumPresentationRateMilli", minimumPresentationRateMilli ?: JSONObject.NULL)
            .put("maximumPresentationRateMilli", maximumPresentationRateMilli ?: JSONObject.NULL)

    private fun benchmarkEventsJson(events: List<VesperBenchmarkEvent>): JSONArray =
        JSONArray().also { array ->
            events.forEach { event ->
                array.put(
                    JSONObject()
                        .put("eventName", event.eventName)
                        .put("elapsedNs", event.elapsedNs)
                        .put("timestampNs", event.timestampNs)
                        .put("thread", event.thread ?: JSONObject.NULL)
                        .put("attributes", JSONObject(event.attributes)),
                )
            }
        }

    private fun Map<String, String>.longValue(name: String): Long =
        get(name)?.toLongOrNull() ?: 0L

    internal companion object {
        private const val SCHEMA_VERSION = 2
        private const val VIDEO_DECODER_INITIALIZED_EVENT = "video_decoder_initialized"
        private const val DROPPED_VIDEO_FRAMES_EVENT = "dropped_video_frames"
        private const val VIDEO_FRAME_METADATA_WINDOW_EVENT = "video_frame_metadata_window"

        fun evidenceFile(
            context: Context,
            testId: String,
            fileSuffix: String? = null,
        ): File {
            val safeTestId = testId.replace(Regex("[^A-Za-z0-9._-]"), "-")
            val safeSuffix = fileSuffix?.replace(Regex("[^A-Za-z0-9._-]"), "-")
            val name = if (safeSuffix.isNullOrBlank()) safeTestId else "$safeTestId-$safeSuffix"
            return File(context.filesDir, "device-evidence/$name.jsonl")
        }

        fun deleteEvidenceFiles(
            context: Context,
            testId: String,
        ) {
            val safeTestId = testId.replace(Regex("[^A-Za-z0-9._-]"), "-")
            val directory = File(context.filesDir, "device-evidence")
            directory.listFiles()?.forEach { candidate ->
                if (candidate.name == "$safeTestId.jsonl" ||
                    candidate.name.startsWith("$safeTestId-")
                ) {
                    candidate.delete()
                }
            }
        }

        fun writeLatestEvidenceIndex(
            context: Context,
            testId: String,
            evidenceFile: File,
        ): File {
            val safeTestId = testId.replace(Regex("[^A-Za-z0-9._-]"), "-")
            val directory = File(context.filesDir, "device-evidence").apply { mkdirs() }
            val index = File(directory, "$safeTestId-latest.txt")
            val temporary = File(directory, ".$safeTestId-latest-${Process.myPid()}.tmp")
            temporary.writeText("${evidenceFile.name}\n")
            if (!temporary.renameTo(index)) {
                index.writeText("${evidenceFile.name}\n")
                temporary.delete()
            }
            return index
        }

        fun processIdentity(): String {
            val bootId =
                runCatching { File("/proc/sys/kernel/random/boot_id").readText().trim() }
                    .getOrNull()
                    .orEmpty()
                    .ifBlank { "unknown-boot" }
            val startTicks =
                runCatching {
                    val stat = File("/proc/self/stat").readText()
                    val fieldsAfterCommand = stat.substring(stat.lastIndexOf(')') + 2).split(' ')
                    fieldsAfterCommand[19]
                }.getOrNull() ?: "unknown-start"
            return "$bootId:${Process.myPid()}:$startTicks"
        }
    }
}
