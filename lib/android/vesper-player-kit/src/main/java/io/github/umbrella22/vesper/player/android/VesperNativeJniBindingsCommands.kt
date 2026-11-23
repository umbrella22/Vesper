package io.github.umbrella22.vesper.player.android

import android.os.Looper
import androidx.media3.common.C
import androidx.media3.common.PlaybackException
import androidx.media3.common.Player
import androidx.media3.common.Timeline
import kotlin.coroutines.resume
import kotlin.coroutines.resumeWithException
import kotlin.math.absoluteValue
import kotlinx.coroutines.TimeoutCancellationException
import kotlinx.coroutines.currentCoroutineContext
import kotlinx.coroutines.isActive
import kotlinx.coroutines.suspendCancellableCoroutine
import kotlinx.coroutines.withTimeout

private const val SEEK_READBACK_TOLERANCE_MS = 1_000L

internal suspend fun VesperNativeJniBindings.awaitMedia3SourceCommandReadiness(
    commandId: Long,
    sourceEpoch: Long,
    timeoutMs: Long,
): TimelineUiState {
    check(Looper.myLooper() == Looper.getMainLooper()) {
        "source command readiness must be awaited on the Android main looper"
    }
    playbackDiagnosticsTracker.stallDetector.resetWindow()
    val exoPlayer = player ?: throw commandFailure(
        message = "Android system playback is not available for source readiness.",
        code = VesperPlayerErrorCode.InvalidState,
        category = VesperPlayerErrorCategory.Source,
        reason = "sourceCommandRouteUnavailable",
        commandId = commandId,
        sourceEpoch = sourceEpoch,
    )
    val callbackGeneration = systemPlaybackCallbackGeneration.get()

    try {
        return withTimeout(timeoutMs.coerceAtLeast(1L)) {
            suspendCancellableCoroutine { continuation ->
                var settled = false
                lateinit var listener: Player.Listener

                fun cleanup() {
                    exoPlayer.removeListener(listener)
                    if (pendingSourceCommandId == commandId) {
                        pendingSourceCommandId = null
                        pendingSourceCommandCancellation = null
                    }
                }

                fun fail(error: Throwable) {
                    if (settled) return
                    settled = true
                    cleanup()
                    if (continuation.isActive) {
                        continuation.resumeWithException(error)
                    }
                }

                fun checkReadiness() {
                    if (settled) return
                    if (systemPlaybackCallbackGeneration.get() != callbackGeneration || player !== exoPlayer) {
                        fail(
                            obsoleteCommandFailure(
                                message = "Android source command was superseded.",
                                category = VesperPlayerErrorCategory.Source,
                                reason = "sourceCommandSuperseded",
                                commandId = commandId,
                                sourceEpoch = sourceEpoch,
                            )
                        )
                        return
                    }
                    exoPlayer.playerError?.let { error ->
                        fail(error.toCommandFailure(commandId, sourceEpoch, "sourceCommandFailed"))
                        return
                    }
                    val timeline = exoPlayer.commandReadyTimeline(liveOnly = isRtspBridgeActive) ?: return
                    settled = true
                    cleanup()
                    if (continuation.isActive) {
                        continuation.resume(timeline)
                    }
                }

                listener =
                    object : Player.Listener {
                        override fun onPlaybackStateChanged(playbackState: Int) = checkReadiness()

                        override fun onTimelineChanged(timeline: Timeline, reason: Int) = checkReadiness()

                        override fun onIsLoadingChanged(isLoading: Boolean) = checkReadiness()

                        override fun onPlayerError(error: PlaybackException) {
                            fail(error.toCommandFailure(commandId, sourceEpoch, "sourceCommandFailed"))
                        }
                    }
                pendingSourceCommandCancellation?.invoke("sourceCommandSuperseded")
                pendingSourceCommandId = commandId
                pendingSourceCommandCancellation = { reason ->
                    fail(
                        obsoleteCommandFailure(
                            message = "Android source command is no longer current.",
                            category = VesperPlayerErrorCategory.Source,
                            reason = reason,
                            commandId = commandId,
                            sourceEpoch = sourceEpoch,
                        )
                    )
                }
                exoPlayer.addListener(listener)
                continuation.invokeOnCancellation {
                    if (!settled) {
                        settled = true
                        exoPlayer.removeListener(listener)
                        if (pendingSourceCommandId == commandId) {
                            pendingSourceCommandId = null
                            pendingSourceCommandCancellation = null
                        }
                    }
                }
                checkReadiness()
            }
        }
    } catch (error: TimeoutCancellationException) {
        if (!currentCoroutineContext().isActive) {
            throw error
        }
        throw commandFailure(
            message = "Android source did not publish a command-ready timeline before the deadline.",
            code = VesperPlayerErrorCode.Timeout,
            category = VesperPlayerErrorCategory.Source,
            reason = "sourceCommandReadinessTimeout",
            commandId = commandId,
            sourceEpoch = sourceEpoch,
            retriable = true,
        )
    }
}

internal suspend fun VesperNativeJniBindings.awaitMedia3SeekCompletion(
    positionMs: Long,
    commandId: Long,
    sourceEpoch: Long,
    timeoutMs: Long,
): Long = suspendCancellableCoroutine { continuation ->
    val cancel = beginMedia3Seek(positionMs, commandId, sourceEpoch, timeoutMs) { result ->
        if (continuation.isActive) result.fold(continuation::resume, continuation::resumeWithException)
    }
    continuation.invokeOnCancellation {
        if (Looper.myLooper() == Looper.getMainLooper()) cancel() else mainHandler.post { cancel() }
    }
}

internal fun VesperNativeJniBindings.beginMedia3Seek(
    positionMs: Long,
    commandId: Long,
    sourceEpoch: Long,
    timeoutMs: Long,
    onCompletion: (Result<Long>) -> Unit,
): () -> Unit {
    check(Looper.myLooper() == Looper.getMainLooper()) { "Media3 seek requires the Android main looper" }
    fun rejected(message: String, reason: String = "seekCommandRejected") = commandFailure(
        message = message,
        code = VesperPlayerErrorCode.SeekFailure,
        category = VesperPlayerErrorCategory.Playback,
        reason = reason,
        commandId = commandId,
        sourceEpoch = sourceEpoch,
    )

    if (isRtspBridgeActive) {
        throw commandFailure(
            message = "Seeking is unavailable for the RTSP bridge.",
            code = VesperPlayerErrorCode.Unsupported,
            category = VesperPlayerErrorCategory.Capability,
            reason = "rtsp_seek_unsupported",
            commandId = commandId,
            sourceEpoch = sourceEpoch,
        )
    }
    val handle = sessionHandle
        ?: throw rejected("The Android seek session is unavailable.", "seekRouteUnavailable")
    val exoPlayer = player
        ?: throw rejected("Android system playback is unavailable for seek.", "seekRouteUnavailable")
    val callbackGeneration = systemPlaybackCallbackGeneration.get()
    if (isDisposed.get() || exoPlayer.currentTimeline.isEmpty || exoPlayer.isPlayingAd) {
        throw rejected("The current Android media item cannot accept a seek.", "seekCommandNotReady")
    }
    val targetMs = positionMs.coerceAtLeast(0L)
    var dispatching = false
    var accepted = false
    val targetWindowPositionMs = exoPlayer.windowPositionForTimelinePosition(targetMs)
    var maskedWindowPositionMs: Long? = null
    var adjustedPositionMs: Long? = null
    var observedInternalDiscontinuity = false
    var settled = false
    lateinit var listener: Player.Listener
    lateinit var timeout: Runnable

    fun cleanup() {
        exoPlayer.removeListener(listener)
        mainHandler.removeCallbacks(timeout)
        if (pendingSeekCommandId == commandId) {
            pendingSeekCommandId = null
            pendingSeekCommandCancellation = null
        }
    }

    fun settle(result: Result<Long>) {
        if (settled) return
        settled = true
        cleanup()
        onCompletion(result)
    }

    fun obsolete(reason: String) = obsoleteCommandFailure(
        message = "Android seek command is no longer current.",
        category = VesperPlayerErrorCategory.Playback,
        reason = reason,
        commandId = commandId,
        sourceEpoch = sourceEpoch,
    )

    fun confirmFromReadback() {
        if (!accepted || settled || maskedWindowPositionMs == null) return
        if (systemPlaybackCallbackGeneration.get() != callbackGeneration || player !== exoPlayer ||
            pendingSeekCommandId != commandId || isDisposed.get()
        ) {
            settle(Result.failure(obsolete("seekSourceChanged")))
            return
        }
        exoPlayer.playerError?.let {
            settle(Result.failure(it.toCommandFailure(commandId, sourceEpoch, "seekFailed")))
            return
        }
        if (exoPlayer.playbackState != Player.STATE_READY && exoPlayer.playbackState != Player.STATE_ENDED) {
            return
        }
        val adjustedMs = adjustedPositionMs
        val completedMs = adjustedMs ?: exoPlayer.currentTimelineSample().timelinePositionMs
        val positionChanged = (completedMs - targetMs).absoluteValue > SEEK_READBACK_TOLERANCE_MS
        if (adjustedMs == null && observedInternalDiscontinuity && positionChanged) {
            settle(Result.failure(rejected(
                "Android playback moved to $completedMs ms during seek.",
                "seekInterruptedByDiscontinuity",
            )))
            return
        }
        val event = if (adjustedMs != null || positionChanged) {
            VesperPlaybackEvent.SeekAdjusted(targetMs, completedMs)
        } else {
            VesperPlaybackEvent.SeekCompleted(targetMs, completedMs)
        }
        // Remove the observer before publishing. A collector can submit a new seek immediately.
        settled = true
        cleanup()
        VesperNativeJni.reportSeekCompleted(handle, completedMs)
        pushSnapshotToRust()
        publishPlaybackObservation(event)
        onCompletion(Result.success(completedMs))
    }

    listener = object : Player.Listener {
        override fun onPositionDiscontinuity(
            oldPosition: Player.PositionInfo,
            newPosition: Player.PositionInfo,
            reason: Int,
        ) {
            if (settled || systemPlaybackCallbackGeneration.get() != callbackGeneration || player !== exoPlayer) {
                return
            }
            when (reason) {
                // A reentrant seek can defer delivery of the masking callback until after dispatch.
                Player.DISCONTINUITY_REASON_SEEK -> if (dispatching ||
                    (accepted && (targetWindowPositionMs - newPosition.positionMs).absoluteValue <= 1L)
                ) {
                    maskedWindowPositionMs = newPosition.positionMs
                }
                Player.DISCONTINUITY_REASON_SEEK_ADJUSTMENT -> if (accepted &&
                    oldPosition.positionMs == maskedWindowPositionMs
                ) {
                    adjustedPositionMs = exoPlayer.timelinePositionForWindowPosition(newPosition.positionMs)
                }
                Player.DISCONTINUITY_REASON_INTERNAL -> if (accepted) observedInternalDiscontinuity = true
            }
        }

        override fun onEvents(player: Player, events: Player.Events) {
            if (events.containsAny(Player.EVENT_PLAYBACK_STATE_CHANGED, Player.EVENT_POSITION_DISCONTINUITY)) {
                confirmFromReadback()
            }
        }

        override fun onPlayerError(error: PlaybackException) {
            if (accepted) settle(Result.failure(error.toCommandFailure(commandId, sourceEpoch, "seekFailed")))
        }
    }
    timeout = Runnable {
        settle(Result.failure(commandFailure(
            message = "Android seek did not complete before the deadline.",
            code = VesperPlayerErrorCode.Timeout,
            category = VesperPlayerErrorCategory.Playback,
            reason = "seekCommandTimeout",
            commandId = commandId,
            sourceEpoch = sourceEpoch,
            retriable = true,
        )))
    }
    cancelPendingSeekCommandInternal("seekCommandSuperseded")
    pendingSeekCommandId = commandId
    pendingSeekCommandCancellation = { reason -> settle(Result.failure(obsolete(reason))) }
    exoPlayer.addListener(listener)
    dispatching = true
    val dispatched = try {
        dispatchSeekToMedia3(handle, targetMs)
    } catch (error: RuntimeException) {
        cleanup()
        throw rejected(error.message ?: "Android seek dispatch failed.")
    } finally {
        dispatching = false
    }
    if (settled) return {}
    if (!dispatched) {
        cleanup()
        throw rejected("The Android playback route rejected the seek.")
    }
    accepted = true
    if (!mainHandler.postDelayed(timeout, timeoutMs.coerceAtLeast(1L))) {
        cleanup()
        throw rejected("Android seek could not schedule its completion deadline.")
    }
    confirmFromReadback()
    return { settle(Result.failure(obsolete("seekCommandCancelled"))) }
}

internal fun VesperNativeJniBindings.cancelPendingSourceCommandInternal(reason: String) {
    pendingSourceCommandCancellation?.invoke(reason)
}

internal fun VesperNativeJniBindings.cancelPendingSeekCommandInternal(reason: String) {
    pendingSeekCommandCancellation?.invoke(reason)
}

private fun androidx.media3.exoplayer.ExoPlayer.commandReadyTimeline(liveOnly: Boolean): TimelineUiState? {
    return currentTimelineSample(liveOnly).commandReadyTimeline(
        playbackReady = playbackState == Player.STATE_READY,
        timelineEmpty = currentTimeline.isEmpty,
    )
}

internal fun ExoTimelineSample.commandReadyTimeline(
    playbackReady: Boolean,
    timelineEmpty: Boolean,
): TimelineUiState? {
    if (!playbackReady || timelineEmpty) return null
    return when {
        isLive && isSeekable &&
            seekableStartMs != C.TIME_UNSET &&
            seekableEndMs > seekableStartMs -> toCommandTimeline()
        isLive && !isSeekable -> toCommandTimeline()
        !isLive && durationMs != C.TIME_UNSET && durationMs > 0L -> toCommandTimeline()
        else -> null
    }
}

private fun ExoTimelineSample.toCommandTimeline(): TimelineUiState {
    val hasSeekableWindow =
        seekableStartMs != C.TIME_UNSET && seekableEndMs > seekableStartMs
    val kind = when {
        isLive && hasSeekableWindow -> TimelineKind.LiveDvr
        isLive -> TimelineKind.Live
        else -> TimelineKind.Vod
    }
    val normalizedDuration = durationMs.takeIf { it != C.TIME_UNSET && it > 0L }
    return TimelineUiState(
        kind = kind,
        isSeekable = when (kind) {
            TimelineKind.Live -> false
            TimelineKind.LiveDvr -> true
            TimelineKind.Vod -> isSeekable
        },
        seekableRange = when {
            hasSeekableWindow -> SeekableRangeUi(seekableStartMs, seekableEndMs)
            kind == TimelineKind.Vod && normalizedDuration != null ->
                SeekableRangeUi(0L, normalizedDuration)
            else -> null
        },
        liveEdgeMs = liveEdgeMs.takeIf { it != C.TIME_UNSET && it >= 0L },
        positionMs = timelinePositionMs.coerceAtLeast(0L),
        durationMs = normalizedDuration,
    )
}

private fun PlaybackException.toCommandFailure(
    commandId: Long,
    sourceEpoch: Long,
    reason: String,
): VesperPlayerCommandException {
    val classified = classifyPlaybackException(this)
    return commandFailure(
        message = message ?: errorCodeName,
        code = VesperPlayerErrorCode.fromJniOrdinal(classified.codeOrdinal),
        category = VesperPlayerErrorCategory.fromJniOrdinal(classified.categoryOrdinal),
        reason = reason,
        commandId = commandId,
        sourceEpoch = sourceEpoch,
        retriable = classified.retriable,
        extraDetails = mapOf("media3ErrorCode" to errorCodeName),
    )
}

internal fun commandFailure(
    message: String,
    code: VesperPlayerErrorCode,
    category: VesperPlayerErrorCategory,
    reason: String,
    commandId: Long,
    sourceEpoch: Long,
    retriable: Boolean = false,
    extraDetails: Map<String, Any?> = emptyMap(),
): VesperPlayerCommandException =
    VesperPlayerCommandException(
        VesperPlayerErrorState(
            message = message,
            code = code,
            category = category,
            retriable = retriable,
            details =
                buildMap {
                    putAll(extraDetails)
                    put("commandReason", reason)
                    if (this["reason"] == null) {
                        put("reason", reason)
                    }
                    put("commandId", commandId)
                    put("sourceEpoch", sourceEpoch)
                },
        )
    )

internal fun obsoleteCommandFailure(
    message: String,
    category: VesperPlayerErrorCategory,
    reason: String,
    commandId: Long,
    sourceEpoch: Long,
): VesperPlayerCommandException =
    commandFailure(
        message = message,
        code = VesperPlayerErrorCode.Cancelled,
        category = category,
        reason = reason,
        commandId = commandId,
        sourceEpoch = sourceEpoch,
        retriable = true,
        extraDetails = mapOf("obsolete" to true),
    )
