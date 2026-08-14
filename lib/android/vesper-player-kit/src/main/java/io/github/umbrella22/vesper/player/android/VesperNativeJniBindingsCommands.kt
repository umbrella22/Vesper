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
                    val timeline = exoPlayer.commandReadyTimeline() ?: return
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
): Long {
    check(Looper.myLooper() == Looper.getMainLooper()) {
        "seek completion must be awaited on the Android main looper"
    }
    val exoPlayer = player ?: throw commandFailure(
        message = "Android system playback is not ready for seek.",
        code = VesperPlayerErrorCode.InvalidState,
        category = VesperPlayerErrorCategory.Playback,
        reason = "seekRouteUnavailable",
        commandId = commandId,
        sourceEpoch = sourceEpoch,
    )
    val callbackGeneration = systemPlaybackCallbackGeneration.get()
    val targetMs = positionMs.coerceAtLeast(0L)

    try {
        return withTimeout(timeoutMs.coerceAtLeast(1L)) {
            suspendCancellableCoroutine { continuation ->
                var settled = false
                var observedSeekDiscontinuity = false
                lateinit var listener: Player.Listener

                fun cleanup() {
                    exoPlayer.removeListener(listener)
                    if (pendingSeekCommandId == commandId) {
                        pendingSeekCommandId = null
                        pendingSeekCommandCancellation = null
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

                fun confirmFromReadback() {
                    if (settled) return
                    if (systemPlaybackCallbackGeneration.get() != callbackGeneration || player !== exoPlayer) {
                        fail(
                            obsoleteCommandFailure(
                                message = "Android seek was superseded by a source change.",
                                category = VesperPlayerErrorCategory.Playback,
                                reason = "seekSourceChanged",
                                commandId = commandId,
                                sourceEpoch = sourceEpoch,
                            )
                        )
                        return
                    }
                    exoPlayer.playerError?.let { error ->
                        fail(error.toCommandFailure(commandId, sourceEpoch, "seekFailed"))
                        return
                    }
                    val completedMs = exoPlayer.currentTimelineSample().timelinePositionMs
                    if (!observedSeekDiscontinuity ||
                        (completedMs - targetMs).absoluteValue > SEEK_READBACK_TOLERANCE_MS
                    ) {
                        return
                    }
                    settled = true
                    cleanup()
                    if (continuation.isActive) {
                        continuation.resume(completedMs)
                    }
                }

                listener =
                    object : Player.Listener {
                        override fun onPositionDiscontinuity(
                            oldPosition: Player.PositionInfo,
                            newPosition: Player.PositionInfo,
                            reason: Int,
                        ) {
                            if (reason == Player.DISCONTINUITY_REASON_SEEK) {
                                observedSeekDiscontinuity = true
                                confirmFromReadback()
                            }
                        }

                        override fun onPlayerError(error: PlaybackException) {
                            fail(error.toCommandFailure(commandId, sourceEpoch, "seekFailed"))
                        }
                    }
                pendingSeekCommandCancellation?.invoke("seekCommandSuperseded")
                pendingSeekCommandId = commandId
                pendingSeekCommandCancellation = { reason ->
                    fail(
                        obsoleteCommandFailure(
                            message = "Android seek command is no longer current.",
                            category = VesperPlayerErrorCategory.Playback,
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
                        if (pendingSeekCommandId == commandId) {
                            pendingSeekCommandId = null
                            pendingSeekCommandCancellation = null
                        }
                    }
                }
                seekTo(targetMs)
                mainHandler.post { confirmFromReadback() }
            }
        }
    } catch (error: TimeoutCancellationException) {
        if (!currentCoroutineContext().isActive) {
            throw error
        }
        throw commandFailure(
            message = "Android seek did not complete before the deadline.",
            code = VesperPlayerErrorCode.Timeout,
            category = VesperPlayerErrorCategory.Playback,
            reason = "seekCommandTimeout",
            commandId = commandId,
            sourceEpoch = sourceEpoch,
            retriable = true,
        )
    }
}

internal fun VesperNativeJniBindings.cancelPendingSourceCommandInternal(reason: String) {
    pendingSourceCommandCancellation?.invoke(reason)
}

internal fun VesperNativeJniBindings.cancelPendingSeekCommandInternal(reason: String) {
    pendingSeekCommandCancellation?.invoke(reason)
}

private fun androidx.media3.exoplayer.ExoPlayer.commandReadyTimeline(): TimelineUiState? {
    return currentTimelineSample().commandReadyTimeline(
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
