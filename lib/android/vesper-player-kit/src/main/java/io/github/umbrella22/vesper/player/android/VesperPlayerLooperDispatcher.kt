package io.github.umbrella22.vesper.player.android

import android.os.Handler
import android.os.Looper
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicReference

internal class VesperPlayerLooperDispatcher(
    private val isOwnerThread: () -> Boolean,
    private val post: (Runnable) -> Boolean,
    private val remove: (Runnable) -> Unit,
    private val timeoutMs: Long = PLAYER_LOOPER_OPERATION_TIMEOUT_MS,
) {
    private enum class State {
        Pending,
        Running,
        Completed,
        Cancelled,
    }

    constructor(
        looper: Looper,
        timeoutMs: Long = PLAYER_LOOPER_OPERATION_TIMEOUT_MS,
    ) : this(
        isOwnerThread = { Looper.myLooper() === looper },
        post = Handler(looper)::post,
        remove = Handler(looper)::removeCallbacks,
        timeoutMs = timeoutMs,
    )

    fun dispatch(operation: () -> Unit): Boolean {
        if (isOwnerThread()) {
            operation()
            return true
        }

        val state = AtomicReference(State.Pending)
        val completed = CountDownLatch(1)
        val runnable =
            Runnable {
                if (!state.compareAndSet(State.Pending, State.Running)) {
                    completed.countDown()
                    return@Runnable
                }
                try {
                    operation()
                } finally {
                    state.set(State.Completed)
                    completed.countDown()
                }
            }
        if (!post(runnable)) {
            state.set(State.Cancelled)
            return false
        }

        val finished =
            try {
                completed.await(timeoutMs.coerceAtLeast(0L), TimeUnit.MILLISECONDS)
            } catch (_: InterruptedException) {
                Thread.currentThread().interrupt()
                false
            }
        if (finished) {
            return state.get() == State.Completed
        }

        if (state.compareAndSet(State.Pending, State.Cancelled)) {
            remove(runnable)
        }
        return false
    }
}

internal fun runPlayerSurfaceOperation(
    player: androidx.media3.exoplayer.ExoPlayer,
    operation: String,
    action: (androidx.media3.exoplayer.ExoPlayer) -> Unit,
) {
    check(VesperPlayerLooperDispatcher(player.applicationLooper).dispatch { action(player) }) {
        "Media3 $operation did not complete on the application looper within " +
            "$PLAYER_LOOPER_OPERATION_TIMEOUT_MS ms"
    }
}

private const val PLAYER_LOOPER_OPERATION_TIMEOUT_MS = 2_000L
