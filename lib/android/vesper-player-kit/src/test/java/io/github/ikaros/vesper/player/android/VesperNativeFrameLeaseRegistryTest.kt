package io.github.ikaros.vesper.player.android

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

class VesperNativeFrameLeaseRegistryTest {
    @Test
    fun frameLeaseKeepsItsOriginalPipelineOwner() {
        val registry = VesperNativeFrameLeaseRegistry()

        registry.register(pipelineHandle = 11L, frameHandle = 101L)
        registry.register(pipelineHandle = 22L, frameHandle = 202L)

        assertEquals(11L, registry.takePipelineHandle(101L))
        assertEquals(22L, registry.takePipelineHandle(202L))
        assertNull(registry.takePipelineHandle(101L))
    }

    @Test
    fun drainingPipelineReturnsEachFrameLeaseExactlyOnce() {
        val registry = VesperNativeFrameLeaseRegistry()
        registry.register(pipelineHandle = 11L, frameHandle = 101L)
        registry.register(pipelineHandle = 11L, frameHandle = 102L)
        registry.register(pipelineHandle = 22L, frameHandle = 202L)

        assertEquals(listOf(101L, 102L), registry.drainPipeline(11L).sorted())
        assertEquals(emptyList<Long>(), registry.drainPipeline(11L))
        assertNull(registry.takePipelineHandle(101L))
        assertEquals(22L, registry.takePipelineHandle(202L))
    }
}
