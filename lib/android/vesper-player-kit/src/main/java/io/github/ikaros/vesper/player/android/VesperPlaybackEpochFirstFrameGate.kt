package io.github.ikaros.vesper.player.android

import java.util.concurrent.atomic.AtomicLong

internal class VesperPlaybackEpochFirstFrameGate {
    private val epoch = AtomicLong(0L)
    private val firstFrameRenderedEpoch = AtomicLong(NO_EPOCH)

    val currentEpoch: Long
        get() = epoch.get()

    fun advanceEpoch(): Long {
        val nextEpoch = epoch.incrementAndGet()
        firstFrameRenderedEpoch.set(NO_EPOCH)
        return nextEpoch
    }

    fun markFirstFrameRendered(): FirstFrameMark {
        val observedEpoch = epoch.get()
        while (true) {
            val renderedEpoch = firstFrameRenderedEpoch.get()
            if (renderedEpoch == observedEpoch) {
                return FirstFrameMark(
                    playbackEpoch = observedEpoch,
                    isFirstForEpoch = false,
                )
            }
            if (firstFrameRenderedEpoch.compareAndSet(renderedEpoch, observedEpoch)) {
                return FirstFrameMark(
                    playbackEpoch = observedEpoch,
                    isFirstForEpoch = true,
                )
            }
        }
    }

    private companion object {
        const val NO_EPOCH = -1L
    }
}

internal data class FirstFrameMark(
    val playbackEpoch: Long,
    val isFirstForEpoch: Boolean,
)
