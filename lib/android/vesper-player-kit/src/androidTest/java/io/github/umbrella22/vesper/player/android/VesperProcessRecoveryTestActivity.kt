package io.github.umbrella22.vesper.player.android

import android.app.Activity
import android.app.KeyguardManager
import android.content.Intent
import android.os.Bundle
import android.os.PowerManager
import android.os.Process
import android.os.SystemClock
import android.system.Os
import android.util.Log
import android.widget.FrameLayout
import java.io.File
import java.io.FileOutputStream
import java.util.concurrent.atomic.AtomicBoolean
import kotlin.math.absoluteValue
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.cancel
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import org.json.JSONObject

/** ADB-driven host used to verify playback intent and checkpoint recovery after process death. */
class VesperProcessRecoveryTestActivity : Activity() {
    private val activityScope = CoroutineScope(SupervisorJob() + Dispatchers.Main.immediate)
    private val resourcesClosed = AtomicBoolean(false)
    private val processStartIdentity = VesperAndroidDeviceEvidenceWriter.processIdentity()

    private lateinit var surfaceHost: FrameLayout
    private lateinit var evidence: VesperAndroidDeviceEvidenceWriter
    private lateinit var checkpointFile: File
    private lateinit var launchId: String
    private lateinit var sourceUri: String
    private lateinit var protocol: VesperPlayerSourceProtocol

    private var controller: VesperPlayerController? = null
    private var checkpointJob: Job? = null
    private var launchCount = 0L
    private var priorPid: Int? = null
    private var priorProcessStartIdentity: String? = null
    private var desiredPlayback = true
    private var systemPlaybackEnabled = true
    private var sourceChanged = false
    private var sourceReady = false
    private var pendingRestorePositionMs: Long? = null
    private var resumedSinceElapsedRealtimeMs: Long? = null
    private var lastDetailedMemoryCaptureElapsedRealtimeMs: Long? = null

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)

        surfaceHost = FrameLayout(this)
        setContentView(surfaceHost)
        checkpointFile = File(filesDir, CHECKPOINT_RELATIVE_PATH)

        val resetRequested = intent.booleanExtra(RESET_EXTRA_NAMES) ?: false
        val reset = resetRequested && savedInstanceState == null
        if (reset) {
            checkpointFile.delete()
        }
        val priorCheckpoint = readCheckpoint(checkpointFile)
        launchCount = (priorCheckpoint?.launchCount ?: 0L) + 1L
        priorPid = priorCheckpoint?.pid
        priorProcessStartIdentity = priorCheckpoint?.processStartIdentity
        launchId = "${System.currentTimeMillis()}-${Process.myPid()}-$launchCount"
        evidence =
            VesperAndroidDeviceEvidenceWriter(
                context = applicationContext,
                testId = EVIDENCE_TEST_ID,
                append = false,
                fileSuffix = launchId,
            )
        val latestIndex =
            VesperAndroidDeviceEvidenceWriter.writeLatestEvidenceIndex(
                context = applicationContext,
                testId = EVIDENCE_TEST_ID,
                evidenceFile = evidence.file,
            )

        Log.i(
            TAG,
            "launchId=$launchId pid=${Process.myPid()} processStartIdentity=$processStartIdentity " +
                "evidencePath=${evidence.file.absolutePath} latestIndex=${latestIndex.absolutePath} " +
                "checkpointPath=${checkpointFile.absolutePath}",
        )

        val requestedSource = intent.firstStringExtra(SOURCE_EXTRA_NAMES)?.trim().orEmpty()
        if (requestedSource.isEmpty()) {
            record(
                phase = "missing_source",
                details = mapOf("reset" to reset, "finishReason" to "missingSourceExtra"),
            )
            closeResources()
            finish()
            return
        }

        sourceUri = requestedSource
        protocol = resolveProtocol(intent, requestedSource, priorCheckpoint)
        sourceChanged =
            priorCheckpoint != null &&
                (priorCheckpoint.sourceUri != sourceUri || priorCheckpoint.protocol != protocol.name)
        val matchingCheckpoint = priorCheckpoint?.takeUnless { sourceChanged }
        desiredPlayback = matchingCheckpoint?.wasPlaying ?: true
        pendingRestorePositionMs = matchingCheckpoint?.positionMs
        intent.playbackCommand()?.let { desiredPlayback = it == PlaybackCommand.Resume }
        systemPlaybackEnabled =
            intent.booleanExtra(SYSTEM_PLAYBACK_EXTRA_NAMES) ?: true

        record(
            phase = "on_create",
            details =
                mapOf(
                    "resetRequested" to resetRequested,
                    "reset" to reset,
                    "processRecreated" to (savedInstanceState != null),
                    "sourceChanged" to sourceChanged,
                    "restoredPositionMs" to (matchingCheckpoint?.positionMs ?: 0L),
                    "restoredWasPlaying" to matchingCheckpoint?.wasPlaying,
                    "priorLaunchCount" to priorCheckpoint?.launchCount,
                ),
        )

        val source =
            VesperPlayerSource(
                uri = sourceUri,
                label = "process recovery fixture",
                kind = sourceKind(sourceUri),
                protocol = protocol,
            )
        controller =
            VesperPlayerControllerFactory.createDefault(
                context = applicationContext,
                initialSource = source,
                resiliencePolicy = VesperPlaybackResiliencePolicy.streaming(),
                decoderBackend = VesperDecoderBackend.SystemOnly,
                surfaceKind = VesperVideoSurfaceKind.SurfaceView,
                keepScreenOnDuringPlayback = false,
                benchmarkConfiguration =
                    VesperBenchmarkConfiguration(
                        enabled = true,
                        maxBufferedEvents = MAX_BUFFERED_BENCHMARK_EVENTS,
                    ),
            ).also { player ->
                player.attachSurfaceHost(surfaceHost)
                player.configureSystemPlayback(
                    VesperSystemPlaybackConfiguration(
                        enabled = systemPlaybackEnabled,
                        backgroundMode = VesperBackgroundPlaybackMode.ContinueAudio,
                        metadata =
                            VesperSystemPlaybackMetadata(
                                title = "Vesper process recovery",
                                contentUri = sourceUri,
                            ),
                    )
                )
            }

        writeCheckpoint()
        startCheckpointLoop()
        activityScope.launch {
            initializeAndRestore(matchingCheckpoint)
        }
    }

    override fun onNewIntent(intent: Intent) {
        super.onNewIntent(intent)
        val command = intent.playbackCommand()
        when (command) {
            PlaybackCommand.Pause -> {
                desiredPlayback = false
                controller?.pause()
            }
            PlaybackCommand.Resume -> {
                desiredPlayback = true
                controller?.play()
            }
            null -> Unit
        }
        writeCheckpoint()
        record(
            phase = "on_new_intent",
            details =
                mapOf(
                    "intentAction" to intent.action,
                    "playbackCommand" to command?.name,
                    "commandApplied" to (command != null),
                ),
        )
    }

    override fun onStart() {
        super.onStart()
        recordLifecycle("on_start")
    }

    override fun onResume() {
        super.onResume()
        resumedSinceElapsedRealtimeMs = SystemClock.elapsedRealtime()
        recordLifecycle("on_resume")
    }

    override fun onPause() {
        resumedSinceElapsedRealtimeMs = null
        recordLifecycle("on_pause")
        super.onPause()
    }

    override fun onStop() {
        resumedSinceElapsedRealtimeMs = null
        writeCheckpoint()
        recordLifecycle("on_stop")
        super.onStop()
    }

    override fun onDestroy() {
        resumedSinceElapsedRealtimeMs = null
        writeCheckpoint()
        recordLifecycle("on_destroy")
        closeResources()
        super.onDestroy()
    }

    private suspend fun initializeAndRestore(checkpoint: RecoveryCheckpoint?) {
        val player = controller ?: return
        val targetPositionMs = checkpoint?.positionMs?.coerceAtLeast(0L) ?: 0L
        try {
            player.initializeAsync()
            if (!desiredPlayback) {
                player.pause()
            }

            val timelineReady = awaitResolvedTimeline(player)
            sourceReady = timelineReady
            val resolvedTimeline = player.uiState.value.timeline
            val effectiveTargetPositionMs =
                if (timelineReady) {
                    resolvedTimeline.clampedPosition(targetPositionMs)
                } else {
                    targetPositionMs
                }
            val restoreRatio =
                if (targetPositionMs > 0L && timelineReady) {
                    restoreRatio(resolvedTimeline, effectiveTargetPositionMs)
                } else {
                    null
                }
            var seekCompleted = targetPositionMs == 0L && timelineReady
            if (restoreRatio != null) {
                player.seekToRatioAsync(restoreRatio)
                seekCompleted = true
                pendingRestorePositionMs = null
            }

            if (desiredPlayback) {
                player.play()
            } else {
                player.pause()
            }

            val intentMatches = timelineReady && awaitPlaybackIntent(player, desiredPlayback)
            player.refresh()
            val state = player.uiState.value
            val actualPositionMs = currentPositionMs(player)
            val absoluteErrorMs = (actualPositionMs - effectiveTargetPositionMs).absoluteValue
            val noError = state.lastError == null
            val restorationRequired = checkpoint != null
            val withinTolerance =
                !restorationRequired || absoluteErrorMs <= RESTORE_POSITION_TOLERANCE_MS
            val restorePassed =
                timelineReady && seekCompleted && withinTolerance && noError && intentMatches
            if (restorePassed) {
                pendingRestorePositionMs = null
            }
            writeCheckpoint()
            record(
                phase = "restore_complete",
                details =
                    mapOf(
                        "restorationRequired" to restorationRequired,
                        "timelineReady" to timelineReady,
                        "seekCompleted" to seekCompleted,
                        "targetPositionMs" to targetPositionMs,
                        "effectiveTargetPositionMs" to effectiveTargetPositionMs,
                        "restoreRatio" to restoreRatio,
                        "actualPositionMs" to actualPositionMs,
                        "absoluteErrorMs" to absoluteErrorMs,
                        "positionToleranceMs" to RESTORE_POSITION_TOLERANCE_MS,
                        "positionValidationRequired" to restorationRequired,
                        "withinTolerance" to withinTolerance,
                        "noError" to noError,
                        "desiredPlayback" to desiredPlayback,
                        "actualPlaybackState" to state.playbackState.name,
                        "playbackIntentMatches" to intentMatches,
                        "restorePassed" to restorePassed,
                    ),
            )
            Log.i(
                TAG,
                "launchId=$launchId restorePassed=$restorePassed targetPositionMs=$targetPositionMs " +
                    "effectiveTargetPositionMs=$effectiveTargetPositionMs " +
                    "actualPositionMs=$actualPositionMs absoluteErrorMs=$absoluteErrorMs " +
                    "desiredPlayback=$desiredPlayback actualPlaybackState=${state.playbackState}",
            )
        } catch (error: Throwable) {
            record(
                phase = "restore_failed",
                details =
                    mapOf(
                        "targetPositionMs" to targetPositionMs,
                        "desiredPlayback" to desiredPlayback,
                        "errorType" to error.javaClass.name,
                        "errorMessage" to error.message,
                    ),
            )
            Log.e(TAG, "launchId=$launchId initialization or restore failed", error)
        }
    }

    private fun startCheckpointLoop() {
        checkpointJob?.cancel()
        checkpointJob =
            activityScope.launch {
                while (true) {
                    delay(CHECKPOINT_INTERVAL_MS)
                    writeCheckpoint()
                    record(phase = "checkpoint", captureDetailedMemory = false)
                    recordDetailedMemoryIfStableResumed()
                }
            }
    }

    private fun recordDetailedMemoryIfStableResumed() {
        val resumedSinceMs = resumedSinceElapsedRealtimeMs ?: return
        if (!sourceReady || controller?.uiState?.value?.lastError != null) return
        if (getSystemService(PowerManager::class.java)?.isInteractive != true ||
            getSystemService(KeyguardManager::class.java)?.isKeyguardLocked == true ||
            !hasWindowFocus()
        ) {
            return
        }
        val nowMs = SystemClock.elapsedRealtime()
        val resumedForMs = nowMs - resumedSinceMs
        if (resumedForMs < DETAILED_MEMORY_STABLE_RESUMED_MS) return
        val lastCaptureMs = lastDetailedMemoryCaptureElapsedRealtimeMs
        if (lastCaptureMs != null && nowMs - lastCaptureMs < DETAILED_MEMORY_INTERVAL_MS) return

        lastDetailedMemoryCaptureElapsedRealtimeMs = nowMs
        record(
            phase = "resumed_memory_sample",
            details =
                mapOf(
                    "activityLifecycle" to "resumed",
                    "resumedForMs" to resumedForMs,
                    "detailedMemoryIntervalMs" to DETAILED_MEMORY_INTERVAL_MS,
                ),
            captureDetailedMemory = true,
        )
    }

    private fun writeCheckpoint() {
        if (!::checkpointFile.isInitialized || !::sourceUri.isInitialized ||
            !::protocol.isInitialized
        ) {
            return
        }
        val currentController = controller
        val observedPlaybackState = currentController?.uiState?.value?.playbackState
        if (sourceReady) {
            when (observedPlaybackState) {
                PlaybackStateUi.Playing -> desiredPlayback = true
                PlaybackStateUi.Paused,
                PlaybackStateUi.Finished,
                -> desiredPlayback = false
                PlaybackStateUi.Ready,
                null,
                -> Unit
            }
        }
        val checkpoint =
            RecoveryCheckpoint(
                sourceUri = sourceUri,
                protocol = protocol.name,
                positionMs =
                    pendingRestorePositionMs
                        ?: currentController?.let(::currentPositionMs)
                        ?: 0L,
                wasPlaying = desiredPlayback,
                launchCount = launchCount,
                pid = Process.myPid(),
                processStartIdentity = processStartIdentity,
                priorPid = priorPid,
                priorProcessStartIdentity = priorProcessStartIdentity,
                savedAtEpochMs = System.currentTimeMillis(),
                savedAtElapsedRealtimeMs = SystemClock.elapsedRealtime(),
            )
        runCatching { writeCheckpointAtomically(checkpointFile, checkpoint) }
            .onFailure { error ->
                Log.e(TAG, "launchId=$launchId failed to write checkpoint", error)
            }
    }

    private fun currentPositionMs(player: VesperPlayerController): Long =
        (player.sampleTimeline()?.positionMs ?: player.uiState.value.timeline.positionMs)
            .coerceAtLeast(0L)

    private suspend fun awaitResolvedTimeline(player: VesperPlayerController): Boolean {
        val deadline = SystemClock.elapsedRealtime() + SOURCE_READY_TIMEOUT_MS
        while (SystemClock.elapsedRealtime() < deadline) {
            player.refresh()
            val state = player.uiState.value
            if (state.lastError != null) return false
            if (state.timeline.isResolved()) return true
            delay(PLAYBACK_INTENT_POLL_MS)
        }
        return player.uiState.value.timeline.isResolved()
    }

    private fun TimelineUiState.isResolved(): Boolean =
        when (kind) {
            TimelineKind.Vod -> durationMs != null && durationMs > 0L
            TimelineKind.Live -> liveEdgeMs != null
            TimelineKind.LiveDvr ->
                seekableRange?.let { range -> range.endMs > range.startMs } == true
        }

    private fun restoreRatio(
        timeline: TimelineUiState,
        targetPositionMs: Long,
    ): Float? {
        timeline.seekableRange?.let { range ->
            val widthMs = range.endMs - range.startMs
            if (widthMs > 0L) {
                return ((targetPositionMs - range.startMs).toDouble() / widthMs.toDouble())
                    .toFloat()
                    .coerceIn(0f, 1f)
            }
        }
        val durationMs = timeline.durationMs ?: return null
        if (durationMs <= 0L) return null
        return (targetPositionMs.toDouble() / durationMs.toDouble())
            .toFloat()
            .coerceIn(0f, 1f)
    }

    private suspend fun awaitPlaybackIntent(
        player: VesperPlayerController,
        shouldPlay: Boolean,
    ): Boolean {
        val deadline = SystemClock.elapsedRealtime() + PLAYBACK_INTENT_TIMEOUT_MS
        while (SystemClock.elapsedRealtime() < deadline) {
            player.refresh()
            val state = player.uiState.value
            if (state.lastError != null) return false
            if (state.timeline.isResolved() &&
                !state.isBuffering &&
                playbackIntentMatches(state.playbackState, shouldPlay)
            ) {
                return true
            }
            delay(PLAYBACK_INTENT_POLL_MS)
        }
        player.refresh()
        val state = player.uiState.value
        return state.timeline.isResolved() &&
            !state.isBuffering &&
            playbackIntentMatches(state.playbackState, shouldPlay)
    }

    private fun playbackIntentMatches(
        state: PlaybackStateUi,
        shouldPlay: Boolean,
    ): Boolean =
        if (shouldPlay) {
            state == PlaybackStateUi.Playing
        } else {
            state == PlaybackStateUi.Paused
        }

    private fun recordLifecycle(phase: String) {
        record(
            phase = phase,
            details = mapOf("activityLifecycle" to phase),
            captureDetailedMemory = false,
        )
    }

    private fun record(
        phase: String,
        details: Map<String, Any?> = emptyMap(),
        captureDetailedMemory: Boolean = false,
    ) {
        if (!::evidence.isInitialized || resourcesClosed.get()) return
        val powerManager = getSystemService(PowerManager::class.java)
        val keyguardManager = getSystemService(KeyguardManager::class.java)
        val commonDetails =
            mapOf(
                "launchId" to launchId,
                "launchCount" to launchCount,
                "pid" to Process.myPid(),
                "processStartIdentity" to processStartIdentity,
                "priorPid" to priorPid,
                "priorProcessStartIdentity" to priorProcessStartIdentity,
                "sourceUri" to (if (::sourceUri.isInitialized) sourceUri else null),
                "protocol" to (if (::protocol.isInitialized) protocol.name else null),
                "desiredPlayback" to desiredPlayback,
                "systemPlaybackEnabled" to systemPlaybackEnabled,
                "systemPlaybackBackgroundMode" to VesperBackgroundPlaybackMode.ContinueAudio.name,
                "mediaSessionProcessPid" to Process.myPid(),
                "interactive" to (powerManager?.isInteractive ?: false),
                "keyguardLocked" to (keyguardManager?.isKeyguardLocked ?: false),
                "sourceChanged" to sourceChanged,
            ) + details
        runCatching {
            val player = controller
            evidence.record(
                phase = phase,
                state = player?.uiState?.value,
                events = player?.drainBenchmarkEvents().orEmpty(),
                details = commonDetails,
                captureDetailedMemory = captureDetailedMemory,
            )
        }.onFailure { error ->
            Log.e(TAG, "launchId=$launchId failed to record phase=$phase", error)
        }
    }

    private fun closeResources() {
        if (!resourcesClosed.compareAndSet(false, true)) return
        checkpointJob?.cancel()
        checkpointJob = null
        activityScope.cancel()
        runCatching { controller?.dispose() }
            .onFailure { Log.e(TAG, "launchId=$launchId failed to dispose player", it) }
        controller = null
        if (::evidence.isInitialized) {
            runCatching { evidence.close() }
                .onFailure { Log.e(TAG, "launchId=$launchId failed to close evidence", it) }
        }
    }

    private fun resolveProtocol(
        intent: Intent,
        uri: String,
        checkpoint: RecoveryCheckpoint?,
    ): VesperPlayerSourceProtocol {
        val requested = intent.firstStringExtra(PROTOCOL_EXTRA_NAMES)?.trim()
        val checkpointProtocol = checkpoint?.takeIf { it.sourceUri == uri }?.protocol
        val raw = requested?.takeIf(String::isNotEmpty) ?: checkpointProtocol
        return raw?.let { value ->
            VesperPlayerSourceProtocol.entries.firstOrNull {
                it.name.equals(value, ignoreCase = true)
            }
        } ?: VesperPlayerSource.remote(uri, "process recovery fixture").protocol
    }

    private fun sourceKind(uri: String): VesperPlayerSourceKind =
        when (uri.substringBefore(':').lowercase()) {
            "file", "content" -> VesperPlayerSourceKind.Local
            else -> VesperPlayerSourceKind.Remote
        }

    private fun Intent.firstStringExtra(names: List<String>): String? =
        names.firstNotNullOfOrNull { name -> getStringExtra(name) }

    @Suppress("DEPRECATION")
    private fun Intent.booleanExtra(names: List<String>): Boolean? {
        names.forEach { name ->
            if (!hasExtra(name)) return@forEach
            return when (val value = extras?.get(name)) {
                is Boolean -> value
                is Number -> value.toInt() != 0
                is String ->
                    when (value.trim().lowercase()) {
                        "1", "true", "yes", "on" -> true
                        "0", "false", "no", "off" -> false
                        else -> null
                    }
                else -> null
            }
        }
        return null
    }

    private fun Intent.playbackCommand(): PlaybackCommand? {
        val command = firstStringExtra(COMMAND_EXTRA_NAMES)?.trim()?.lowercase()
        if (command == "pause") return PlaybackCommand.Pause
        if (command == "resume" || command == "play") return PlaybackCommand.Resume
        if (booleanExtra(PAUSE_EXTRA_NAMES) == true) return PlaybackCommand.Pause
        if (booleanExtra(RESUME_EXTRA_NAMES) == true) return PlaybackCommand.Resume
        val normalizedAction = action?.substringAfterLast('.')?.lowercase()
        return when (normalizedAction) {
            "pause" -> PlaybackCommand.Pause
            "resume", "play" -> PlaybackCommand.Resume
            else -> null
        }
    }

    private enum class PlaybackCommand {
        Pause,
        Resume,
    }

    private data class RecoveryCheckpoint(
        val sourceUri: String,
        val protocol: String,
        val positionMs: Long,
        val wasPlaying: Boolean,
        val launchCount: Long,
        val pid: Int,
        val processStartIdentity: String,
        val priorPid: Int?,
        val priorProcessStartIdentity: String?,
        val savedAtEpochMs: Long,
        val savedAtElapsedRealtimeMs: Long,
    ) {
        fun toJson(): JSONObject =
            JSONObject()
                .put("schemaVersion", CHECKPOINT_SCHEMA_VERSION)
                .put("sourceUri", sourceUri)
                .put("protocol", protocol)
                .put("positionMs", positionMs)
                .put("wasPlaying", wasPlaying)
                .put("launchCount", launchCount)
                .put("pid", pid)
                .put("processStartIdentity", processStartIdentity)
                .put("priorPid", priorPid ?: JSONObject.NULL)
                .put("priorProcessStartIdentity", priorProcessStartIdentity ?: JSONObject.NULL)
                .put("savedAtEpochMs", savedAtEpochMs)
                .put("savedAtElapsedRealtimeMs", savedAtElapsedRealtimeMs)

        companion object {
            fun fromJson(json: JSONObject): RecoveryCheckpoint =
                RecoveryCheckpoint(
                    sourceUri = json.getString("sourceUri"),
                    protocol = json.getString("protocol"),
                    positionMs = json.getLong("positionMs").coerceAtLeast(0L),
                    wasPlaying = json.getBoolean("wasPlaying"),
                    launchCount = json.optLong("launchCount", 0L).coerceAtLeast(0L),
                    pid = json.optInt("pid", -1),
                    processStartIdentity = json.optString("processStartIdentity", "unknown"),
                    priorPid = json.optInt("priorPid", -1).takeIf { it >= 0 },
                    priorProcessStartIdentity =
                        json.optString("priorProcessStartIdentity")
                            .takeIf { it.isNotBlank() && it != "null" },
                    savedAtEpochMs = json.optLong("savedAtEpochMs", 0L),
                    savedAtElapsedRealtimeMs = json.optLong("savedAtElapsedRealtimeMs", 0L),
                )
        }
    }

    companion object {
        private const val TAG = "VesperProcessRecovery"
        private const val EVIDENCE_TEST_ID = "process-recovery"
        private const val CHECKPOINT_RELATIVE_PATH = "device-evidence/process-recovery-checkpoint.json"
        private const val CHECKPOINT_SCHEMA_VERSION = 1
        private const val CHECKPOINT_INTERVAL_MS = 1_000L
        private const val DETAILED_MEMORY_STABLE_RESUMED_MS = 30_000L
        private const val DETAILED_MEMORY_INTERVAL_MS = 30_000L
        private const val SOURCE_READY_TIMEOUT_MS = 30_000L
        private const val PLAYBACK_INTENT_TIMEOUT_MS = 10_000L
        private const val PLAYBACK_INTENT_POLL_MS = 100L
        private const val RESTORE_POSITION_TOLERANCE_MS = 5_000L
        private const val MAX_BUFFERED_BENCHMARK_EVENTS = 2_048

        private val SOURCE_EXTRA_NAMES =
            listOf("source", "sourceUri", "source_uri", "vesper.source_uri")
        private val PROTOCOL_EXTRA_NAMES = listOf("protocol", "vesper.protocol")
        private val RESET_EXTRA_NAMES = listOf("reset", "vesper.reset")
        private val SYSTEM_PLAYBACK_EXTRA_NAMES =
            listOf(
                "configureSystemPlayback",
                "configure_system_playback",
                "vesper.configure_system_playback",
            )
        private val COMMAND_EXTRA_NAMES =
            listOf("command", "playbackAction", "playback_action", "vesper.playback_action")
        private val PAUSE_EXTRA_NAMES = listOf("pause", "vesper.pause")
        private val RESUME_EXTRA_NAMES = listOf("resume", "play", "vesper.resume")

        private fun readCheckpoint(file: File): RecoveryCheckpoint? =
            runCatching {
                if (!file.isFile) return null
                RecoveryCheckpoint.fromJson(JSONObject(file.readText()))
            }.onFailure { error ->
                Log.w(TAG, "Ignoring unreadable checkpoint ${file.absolutePath}", error)
            }.getOrNull()

        private fun writeCheckpointAtomically(
            destination: File,
            checkpoint: RecoveryCheckpoint,
        ) {
            val directory = destination.parentFile ?: error("checkpoint has no parent directory")
            check(directory.mkdirs() || directory.isDirectory) {
                "failed to create checkpoint directory ${directory.absolutePath}"
            }
            val temporary = File(directory, ".${destination.name}.${Process.myPid()}.tmp")
            try {
                FileOutputStream(temporary).use { output ->
                    output.write(checkpoint.toJson().toString().toByteArray(Charsets.UTF_8))
                    output.write('\n'.code)
                    output.flush()
                    output.fd.sync()
                }
                Os.rename(temporary.absolutePath, destination.absolutePath)
            } finally {
                temporary.delete()
            }
        }
    }
}
