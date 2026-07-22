package io.github.ikaros.vesper.player.android

import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.TimeoutCancellationException
import kotlinx.coroutines.delay
import kotlinx.coroutines.withTimeout

private const val SUBTITLE_SELECTION_CONFIRMATION_TIMEOUT_MS = 3_000L

/**
 * One bounded subtitle-selection transaction. A bridge owns at most one
 * pending transaction; source and command epochs make callbacks from an old
 * player item harmless.
 */
internal data class PendingSubtitleSelection(
    val commandId: Long,
    val sourceEpoch: Long,
    val selection: VesperTrackSelection,
    val beforeAppliedSelection: VesperTrackSelection,
    val itemEpoch: Long,
    val startingGeneration: Long,
    val sourceCallbackGeneration: Long,
    val commandGeneration: Long,
    val completion: CompletableDeferred<Unit>,
)

internal suspend fun VesperNativePlayerBridge.applySubtitleSelectionTransaction(
    selection: VesperTrackSelection,
) {
    if (isDisposed.get()) {
        throw subtitleSelectionFailure(
            code = "subtitle_selection_cancelled",
            trackId = selection.trackId,
            message = "the player was disposed before the subtitle selection could start",
            retriable = true,
        )
    }
    if (isRequiredNativeFramePipelineFailureActive()) {
        throw subtitleSelectionFailure(
            code = "subtitle_platform_track_unavailable",
            trackId = selection.trackId,
            message = "the active playback backend cannot select subtitle tracks",
        )
    }
    val itemEpoch = activeNativeItemEpoch
    if (!hasInitializedSource || itemEpoch == null || itemEpoch != nativeUpdateEpoch) {
        throw subtitleSelectionFailure(
            code = "subtitle_source_changed",
            trackId = selection.trackId,
            message = "no prepared Media3 item is active for subtitle selection",
            retriable = true,
            sourceEpoch = subtitleSourceEpoch,
        )
    }

    pendingSubtitleSelection?.let { previous ->
        pendingSubtitleSelection = null
        previous.completion.completeExceptionally(
            subtitleSelectionFailure(
                code = "subtitle_selection_superseded",
                trackId = previous.selection.trackId,
                message =
                    "a newer subtitle selection replaced command ${previous.commandId}",
                retriable = true,
                commandId = previous.commandId,
                sourceEpoch = previous.sourceEpoch,
            ),
        )
    }

    val commandId = ++nextSubtitleCommandId
    val sourceEpoch = subtitleSourceEpoch
    val completion = CompletableDeferred<Unit>()
    val pending =
        PendingSubtitleSelection(
            commandId = commandId,
            sourceEpoch = sourceEpoch,
            selection = selection,
            beforeAppliedSelection = bindings.currentAppliedSubtitleSelection(),
            itemEpoch = itemEpoch,
            startingGeneration = bindings.trackSelectionChangeGeneration,
            sourceCallbackGeneration = bindings.sourceCallbackGeneration,
            commandGeneration = bindings.subtitleSelectionCommandGeneration + 1L,
            completion = completion,
        )
    pendingSubtitleSelection = pending
    subtitleSelectionCoordinatorMode = selection.mode
    _requestedSubtitleSelection.value = selection
    _trackSelection.value = _trackSelection.value.copy(subtitle = selection)
    clearPreviousSubtitleSelectionFailure()
    _subtitleState.value = _subtitleState.value.copy(
        selectionState = VesperSubtitleSelectionState.Applying,
        selectionError = null,
    )

    try {
        try {
            withTimeout(SUBTITLE_SELECTION_CONFIRMATION_TIMEOUT_MS) {
                awaitSubtitleCatalogReadiness(pending)
                if (selection.mode == VesperTrackSelectionMode.Track &&
                    (selection.trackId.isNullOrBlank() ||
                        selection.trackId !in bindings.currentTrackCatalog().subtitleTracks.map { it.id })
                ) {
                    throw subtitleSelectionFailure(
                        code = "subtitle_track_not_found",
                        trackId = selection.trackId,
                        message =
                            "the requested subtitle track ${selection.trackId ?: "<null>"} is not in the current catalog",
                        commandId = commandId,
                        sourceEpoch = sourceEpoch,
                    )
                }
                bindings.setSubtitleTrackSelection(selection)
                // Some Media3 fakes and no-op player implementations do not
                // dispatch when the request is already effective.
                refreshFromNative()
                completion.await()
            }
        } catch (_: TimeoutCancellationException) {
            throw subtitleSelectionFailure(
                code = "subtitle_selection_timeout",
                trackId = selection.trackId,
                message =
                    "Media3 did not confirm the subtitle selection before the 3 second deadline",
                retriable = true,
                commandId = commandId,
                sourceEpoch = sourceEpoch,
            )
        }

        if (!isCurrentSubtitleSelection(pending)) {
            throw subtitleSelectionFailure(
                code = "subtitle_source_changed",
                trackId = selection.trackId,
                message = "the source or player item changed while applying the selection",
                retriable = true,
                commandId = commandId,
                sourceEpoch = sourceEpoch,
            )
        }

        val effectiveSelection = bindings.currentTrackSelection().subtitle
        _confirmedSubtitleSelection.value = selection
        _effectiveSubtitleTrackId.value =
            when (selection.mode) {
                VesperTrackSelectionMode.Disabled -> null
                VesperTrackSelectionMode.Track -> effectiveSelection.trackId
                VesperTrackSelectionMode.Auto -> effectiveSelection.trackId
            }
        _trackSelection.value = _trackSelection.value.copy(
            confirmedSubtitle = selection,
            effectiveSubtitleTrackId = _effectiveSubtitleTrackId.value,
        )
        _subtitleState.value = _subtitleState.value.copy(
            selectionState = VesperSubtitleSelectionState.Confirmed,
            selectionError = null,
        )
    } catch (error: VesperPlayerUnsupportedOperation) {
        val details = error.details
        val failure =
            subtitleSelectionFailure(
                code = details["code"] as? String ?: "subtitle_selection_mismatch",
                phase = details["phase"] as? String ?: "selection",
                trackId = details["trackId"] as? String ?: selection.trackId,
                message = error.message ?: "native subtitle selection failed",
                retriable = details["retriable"] as? Boolean ?: false,
                commandId = details["commandId"] as? Long ?: commandId,
                sourceEpoch = details["sourceEpoch"] as? Long ?: sourceEpoch,
            )
        publishSubtitleSelectionFailure(pending, failure)
        throw failure
    } catch (error: CancellationException) {
        if (isCurrentSubtitleSelection(pending)) {
            _subtitleState.value = _subtitleState.value.copy(
                selectionState = VesperSubtitleSelectionState.Failed,
                selectionError = VesperSubtitleError(
                    code = "subtitle_selection_cancelled",
                    phase = VesperSubtitleErrorPhase.Selection,
                    trackId = selection.trackId,
                    retriable = true,
                    message = "the subtitle selection coroutine was cancelled",
                    commandId = commandId,
                    sourceEpoch = sourceEpoch,
                ),
            )
        }
        throw error
    } catch (error: Exception) {
        val failure =
            subtitleSelectionFailure(
                code = "subtitle_selection_mismatch",
                trackId = selection.trackId,
                message = error.message ?: "Media3 failed to apply the subtitle selection",
                commandId = commandId,
                sourceEpoch = sourceEpoch,
            )
        publishSubtitleSelectionFailure(pending, failure)
        throw failure
    } finally {
        if (pendingSubtitleSelection?.commandId == commandId &&
            pendingSubtitleSelection?.sourceEpoch == sourceEpoch
        ) {
            pendingSubtitleSelection = null
        }
    }
}

private suspend fun VesperNativePlayerBridge.awaitSubtitleCatalogReadiness(
    pending: PendingSubtitleSelection,
) {
    if (pending.selection.mode == VesperTrackSelectionMode.Disabled) return
    while (!bindings.isTrackCatalogReady()) {
        if (pending.completion.isCompleted) {
            pending.completion.await()
            return
        }
        if (!isCurrentSubtitleSelection(pending)) {
            throw subtitleSelectionFailure(
                code = "subtitle_source_changed",
                trackId = pending.selection.trackId,
                message = "the source or player item changed while waiting for the subtitle catalog",
                retriable = true,
                commandId = pending.commandId,
                sourceEpoch = pending.sourceEpoch,
            )
        }
        delay(25L)
    }
}

private fun VesperNativePlayerBridge.publishSubtitleSelectionFailure(
    pending: PendingSubtitleSelection,
    failure: VesperPlayerUnsupportedOperation,
) {
    if (!isCurrentSubtitleSelection(pending)) return
    val details = failure.details
    val code = details["code"] as? String ?: "subtitle_selection_mismatch"
    val phaseRaw = details["phase"] as? String
    _subtitleState.value = _subtitleState.value.copy(
        selectionState = VesperSubtitleSelectionState.Failed,
        selectionError = VesperSubtitleError(
            code = code,
            phase = VesperSubtitleErrorPhase.fromWire(phaseRaw),
            phaseRawValue = phaseRaw,
            trackId = details["trackId"] as? String ?: pending.selection.trackId,
            retriable = details["retriable"] as? Boolean ?: false,
            message = failure.message ?: "subtitle selection failed",
            commandId = details["commandId"] as? Long ?: pending.commandId,
            sourceEpoch = details["sourceEpoch"] as? Long ?: pending.sourceEpoch,
        ),
    )
}

/**
 * Called after every native snapshot refresh. Only a Media3 parameter change,
 * or an already-applied request, may complete a pending command. The command
 * and source epochs reject stale callbacks. Renderer-active state remains
 * separate and drives the effective subtitle id.
 */
internal fun VesperNativePlayerBridge.observeSubtitleSelectionConfirmation(
    appliedSelection: VesperTrackSelection,
) {
    val pending = pendingSubtitleSelection ?: return
    if (!isCurrentSubtitleSelection(pending)) {
        pending.completion.completeExceptionally(
            subtitleSelectionFailure(
                code = "subtitle_source_changed",
                trackId = pending.selection.trackId,
                message = "the source or player item changed while applying the selection",
                retriable = true,
                commandId = pending.commandId,
                sourceEpoch = pending.sourceEpoch,
            ),
        )
        return
    }

    if (!subtitleSelectionMatches(pending.selection, appliedSelection)) {
        return
    }

    val generationChanged = bindings.trackSelectionChangeGeneration > pending.startingGeneration
    val alreadyApplied = appliedSelection == pending.beforeAppliedSelection
    if (generationChanged || alreadyApplied) {
        pending.completion.complete(Unit)
    }
}

internal fun VesperNativePlayerBridge.failPendingSubtitleSelection(
    failure: NativeTrackSelectionFailure,
): Boolean {
    val pending = pendingSubtitleSelection ?: return false
    if (pending.sourceEpoch != subtitleSourceEpoch ||
        pending.commandId != nextSubtitleCommandId ||
        pending.completion.isCompleted
    ) {
        return false
    }
    // A failure without exact source and command identity cannot safely be
    // associated with the pending request. Let the bounded transaction time
    // out instead of allowing a delayed callback to fail a newer command.
    if (failure.sourceCallbackGeneration != pending.sourceCallbackGeneration ||
        failure.commandGeneration != pending.commandGeneration
    ) {
        return false
    }
    if (pending.selection.mode != VesperTrackSelectionMode.Track &&
        failure.code == "subtitle_track_not_found"
    ) {
        return false
    }
    if (pending.selection.mode != VesperTrackSelectionMode.Auto &&
        failure.code == "subtitle_auto_candidate_unavailable"
    ) {
        return false
    }
    if (pending.selection.trackId != null &&
        failure.trackId != null &&
        pending.selection.trackId != failure.trackId
    ) {
        return false
    }
    pending.completion.completeExceptionally(
        subtitleSelectionFailure(
            code = failure.code,
            phase = failure.phase,
            trackId = failure.trackId,
            message = failure.message,
            retriable = failure.retriable,
            commandId = pending.commandId,
            sourceEpoch = pending.sourceEpoch,
        ),
    )
    return true
}

/** Invalidate every callback belonging to the previous source/player item. */
internal fun VesperNativePlayerBridge.advanceSubtitleSourceEpoch() {
    subtitleSourceEpoch += 1L
    pendingSubtitleSelection?.let { pending ->
        pendingSubtitleSelection = null
        pending.completion.completeExceptionally(
            subtitleSelectionFailure(
                code = "subtitle_source_changed",
                trackId = pending.selection.trackId,
                message = "the source or player item changed while applying the selection",
                retriable = true,
                commandId = pending.commandId,
                sourceEpoch = pending.sourceEpoch,
            ),
        )
    }
    _requestedSubtitleSelection.value = VesperTrackSelection.disabled()
    _confirmedSubtitleSelection.value = VesperTrackSelection.disabled()
    _effectiveSubtitleTrackId.value = null
    subtitleSelectionCoordinatorMode = null
    _subtitleState.value =
        _subtitleState.value.copy(
            selectionState = VesperSubtitleSelectionState.Idle,
            selectionError = null,
        )
}

internal fun VesperNativePlayerBridge.cancelPendingSubtitleSelectionForDispose() {
    val pending = pendingSubtitleSelection ?: return
    pendingSubtitleSelection = null
    pending.completion.completeExceptionally(
        subtitleSelectionFailure(
            code = "subtitle_selection_cancelled",
            trackId = pending.selection.trackId,
            message = "the player was disposed while applying the subtitle selection",
            retriable = true,
            commandId = pending.commandId,
            sourceEpoch = pending.sourceEpoch,
        ),
    )
}

internal fun VesperNativePlayerBridge.isCurrentSubtitleSelection(
    pending: PendingSubtitleSelection,
): Boolean =
    pendingSubtitleSelection?.commandId == pending.commandId &&
        pendingSubtitleSelection?.sourceEpoch == pending.sourceEpoch &&
        subtitleSourceEpoch == pending.sourceEpoch &&
        activeNativeItemEpoch == pending.itemEpoch &&
        nativeUpdateEpoch == pending.itemEpoch &&
        bindings.sourceCallbackGeneration == pending.sourceCallbackGeneration &&
        hasInitializedSource &&
        !isDisposed.get()

private fun subtitleSelectionMatches(
    requested: VesperTrackSelection,
    actual: VesperTrackSelection,
): Boolean =
    when (requested.mode) {
        VesperTrackSelectionMode.Disabled ->
            actual.mode == VesperTrackSelectionMode.Disabled
        VesperTrackSelectionMode.Track ->
            actual.mode != VesperTrackSelectionMode.Disabled &&
                actual.trackId == requested.trackId
        VesperTrackSelectionMode.Auto ->
            actual.mode == VesperTrackSelectionMode.Auto && actual.trackId != null
    }

private fun subtitleSelectionFailure(
    code: String,
    phase: String = "selection",
    trackId: String?,
    message: String,
    retriable: Boolean = false,
    commandId: Long? = null,
    sourceEpoch: Long? = null,
): VesperPlayerUnsupportedOperation =
    VesperPlayerUnsupportedOperation(
        message,
        mapOf(
            "domain" to "subtitle",
            "code" to code,
            "phase" to phase,
            "trackId" to trackId,
            "commandId" to commandId,
            "sourceEpoch" to sourceEpoch,
            "retriable" to retriable,
            "message" to message,
        ),
    )
