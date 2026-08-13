package io.github.umbrella22.vesper.example.androidcomposehost

import android.app.Activity
import android.os.Build
import android.os.Handler
import android.os.HandlerThread
import android.util.Log
import android.view.FrameMetrics
import android.view.Window
import java.util.Locale
import kotlin.math.roundToLong

internal class ExampleFrameMetricsProbe(
    private val activity: Activity,
    private val onSnapshot: (ExampleFrameMetricsSnapshot) -> Unit,
) {
    private var frameMetricsThread: HandlerThread? = null
    private var listener: Window.OnFrameMetricsAvailableListener? = null
    private var collector = ExampleFrameMetricsCollector()

    fun start() {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.N || listener != null) {
            return
        }
        collector = ExampleFrameMetricsCollector()
        val thread =
            HandlerThread("vesper-example-frame-metrics").also { thread ->
                thread.start()
            }
        val nextListener =
            Window.OnFrameMetricsAvailableListener { _, frameMetrics, _ ->
                val snapshot = collector.record(frameMetrics)
                if (snapshot != null) {
                    Log.d(FRAME_METRICS_PROBE_TAG, snapshot.logLine)
                    activity.runOnUiThread {
                        onSnapshot(snapshot)
                    }
                }
            }
        activity.window.addOnFrameMetricsAvailableListener(nextListener, Handler(thread.looper))
        frameMetricsThread = thread
        listener = nextListener
    }

    fun stop() {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.N) {
            return
        }
        listener?.let { activeListener ->
            runCatching {
                activity.window.removeOnFrameMetricsAvailableListener(activeListener)
            }
        }
        listener = null
        frameMetricsThread?.quitSafely()
        frameMetricsThread = null
    }
}

internal data class ExampleFrameMetricsSnapshot(
    val sampleCount: Int,
    val jankyFrameCount: Int,
    val slowUiFrameCount: Int,
    val slowDrawFrameCount: Int,
    val slowGpuFrameCount: Int,
    val totalP50Ms: Double,
    val totalP95Ms: Double,
    val totalMaxMs: Double,
    val inputP95Ms: Double,
    val layoutP95Ms: Double,
    val drawP95Ms: Double,
    val syncP95Ms: Double,
    val gpuP95Ms: Double,
) {
    val logLine: String
        get() =
            "frame-metrics frames=$sampleCount janky=$jankyFrameCount " +
                "slowUi=$slowUiFrameCount slowDraw=$slowDrawFrameCount slowGpu=$slowGpuFrameCount " +
                "total[p50=${totalP50Ms.formatMs()},p95=${totalP95Ms.formatMs()},max=${totalMaxMs.formatMs()}] " +
                "inputP95=${inputP95Ms.formatMs()} layoutP95=${layoutP95Ms.formatMs()} " +
                "drawP95=${drawP95Ms.formatMs()} syncP95=${syncP95Ms.formatMs()} gpuP95=${gpuP95Ms.formatMs()}"
}

private class ExampleFrameMetricsCollector {
    private val totalDurationsMs = ArrayList<Double>(FRAME_METRICS_SAMPLE_WINDOW)
    private val inputDurationsMs = ArrayList<Double>(FRAME_METRICS_SAMPLE_WINDOW)
    private val layoutDurationsMs = ArrayList<Double>(FRAME_METRICS_SAMPLE_WINDOW)
    private val drawDurationsMs = ArrayList<Double>(FRAME_METRICS_SAMPLE_WINDOW)
    private val syncDurationsMs = ArrayList<Double>(FRAME_METRICS_SAMPLE_WINDOW)
    private val gpuDurationsMs = ArrayList<Double>(FRAME_METRICS_SAMPLE_WINDOW)
    private var sampleCount = 0
    private var jankyFrameCount = 0
    private var slowUiFrameCount = 0
    private var slowDrawFrameCount = 0
    private var slowGpuFrameCount = 0

    fun record(frameMetrics: FrameMetrics): ExampleFrameMetricsSnapshot? {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.N) {
            return null
        }
        val totalMs = frameMetrics.metricMs(FrameMetrics.TOTAL_DURATION)
        val inputMs = frameMetrics.metricMs(FrameMetrics.INPUT_HANDLING_DURATION)
        val layoutMs = frameMetrics.metricMs(FrameMetrics.LAYOUT_MEASURE_DURATION)
        val drawMs = frameMetrics.metricMs(FrameMetrics.DRAW_DURATION)
        val syncMs = frameMetrics.metricMs(FrameMetrics.SYNC_DURATION)
        val gpuMs =
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S) {
                frameMetrics.metricMs(FrameMetrics.GPU_DURATION)
            } else {
                0.0
            }
        sampleCount += 1
        totalDurationsMs += totalMs
        inputDurationsMs += inputMs
        layoutDurationsMs += layoutMs
        drawDurationsMs += drawMs
        syncDurationsMs += syncMs
        gpuDurationsMs += gpuMs
        if (totalMs > FRAME_METRICS_144HZ_BUDGET_MS) {
            jankyFrameCount += 1
        }
        if (inputMs + layoutMs > FRAME_METRICS_SLOW_UI_MS) {
            slowUiFrameCount += 1
        }
        if (drawMs + syncMs > FRAME_METRICS_SLOW_DRAW_MS) {
            slowDrawFrameCount += 1
        }
        if (gpuMs > FRAME_METRICS_SLOW_GPU_MS) {
            slowGpuFrameCount += 1
        }
        if (sampleCount < FRAME_METRICS_SAMPLE_WINDOW) {
            return null
        }
        return snapshotAndReset()
    }

    private fun snapshotAndReset(): ExampleFrameMetricsSnapshot {
        val snapshot =
            ExampleFrameMetricsSnapshot(
                sampleCount = sampleCount,
                jankyFrameCount = jankyFrameCount,
                slowUiFrameCount = slowUiFrameCount,
                slowDrawFrameCount = slowDrawFrameCount,
                slowGpuFrameCount = slowGpuFrameCount,
                totalP50Ms = totalDurationsMs.percentile(50),
                totalP95Ms = totalDurationsMs.percentile(95),
                totalMaxMs = totalDurationsMs.maxOrNull() ?: 0.0,
                inputP95Ms = inputDurationsMs.percentile(95),
                layoutP95Ms = layoutDurationsMs.percentile(95),
                drawP95Ms = drawDurationsMs.percentile(95),
                syncP95Ms = syncDurationsMs.percentile(95),
                gpuP95Ms = gpuDurationsMs.percentile(95),
            )
        totalDurationsMs.clear()
        inputDurationsMs.clear()
        layoutDurationsMs.clear()
        drawDurationsMs.clear()
        syncDurationsMs.clear()
        gpuDurationsMs.clear()
        sampleCount = 0
        jankyFrameCount = 0
        slowUiFrameCount = 0
        slowDrawFrameCount = 0
        slowGpuFrameCount = 0
        return snapshot
    }
}

private fun FrameMetrics.metricMs(metric: Int): Double = getMetric(metric) / 1_000_000.0

private fun List<Double>.percentile(percentile: Int): Double {
    if (isEmpty()) {
        return 0.0
    }
    val sorted = sorted()
    val index =
        ((percentile / 100.0) * (sorted.lastIndex)).roundToLong()
            .coerceIn(0L, sorted.lastIndex.toLong())
            .toInt()
    return sorted[index]
}

private fun Double.formatMs(): String = String.format(Locale.US, "%.2fms", this)

private const val FRAME_METRICS_SAMPLE_WINDOW = 120
private const val FRAME_METRICS_144HZ_BUDGET_MS = 6.94
private const val FRAME_METRICS_SLOW_UI_MS = 6.0
private const val FRAME_METRICS_SLOW_DRAW_MS = 6.0
private const val FRAME_METRICS_SLOW_GPU_MS = 8.0
private const val FRAME_METRICS_PROBE_TAG = "VesperHostFrameMetrics"
