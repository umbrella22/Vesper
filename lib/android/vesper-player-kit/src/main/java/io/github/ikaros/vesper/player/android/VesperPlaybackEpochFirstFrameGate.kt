package io.github.ikaros.vesper.player.android

internal class VesperPlaybackEpochFirstFrameGate {
    var currentEpoch: Long = 0L
        private set

    private var firstFrameRenderedEpoch: Long? = null

    fun advanceEpoch(): Long {
        currentEpoch += 1L
        firstFrameRenderedEpoch = null
        return currentEpoch
    }

    fun markFirstFrameRendered(): FirstFrameMark {
        val isFirstForEpoch = firstFrameRenderedEpoch != currentEpoch
        if (isFirstForEpoch) {
            firstFrameRenderedEpoch = currentEpoch
        }
        return FirstFrameMark(
            playbackEpoch = currentEpoch,
            isFirstForEpoch = isFirstForEpoch,
        )
    }
}

internal data class FirstFrameMark(
    val playbackEpoch: Long,
    val isFirstForEpoch: Boolean,
)
