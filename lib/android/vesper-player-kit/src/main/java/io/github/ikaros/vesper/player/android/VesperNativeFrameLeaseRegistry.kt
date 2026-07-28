package io.github.ikaros.vesper.player.android

internal class VesperNativeFrameLeaseRegistry {
    private val lock = Any()
    private val pipelineByFrame = mutableMapOf<Long, Long>()

    fun register(pipelineHandle: Long, frameHandle: Long) {
        require(pipelineHandle > 0L) { "native-frame pipeline handle must be positive" }
        require(frameHandle > 0L) { "native-frame handle must be positive" }
        synchronized(lock) {
            check(pipelineByFrame.putIfAbsent(frameHandle, pipelineHandle) == null) {
                "native-frame handle $frameHandle is already owned by a pipeline"
            }
        }
    }

    fun takePipelineHandle(frameHandle: Long): Long? =
        synchronized(lock) {
            pipelineByFrame.remove(frameHandle)
        }

    fun drainPipeline(pipelineHandle: Long): List<Long> =
        synchronized(lock) {
            val frameHandles =
                pipelineByFrame
                    .filterValues { it == pipelineHandle }
                    .keys
                    .toList()
            frameHandles.forEach(pipelineByFrame::remove)
            frameHandles
        }
}
