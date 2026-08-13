package io.github.umbrella22.vesper.player.flutter.android

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertSame
import org.junit.Test

class VesperPlayerAndroidSurfaceHostLifecycleTest {
    @Test
    fun platformViewDisposeOnlySchedulesSurfaceDetach() {
        val session = FakeSurfaceSession()
        val host = FakeSurfaceHost("first")
        val pending = mutableListOf<() -> Unit>()
        val coordinator = fakeCoordinator(session, pending)

        coordinator.bind("player", host)
        coordinator.unbind("player", host)

        assertSame(host, session.host)
        assertEquals(listOf("attach:first"), session.events)
        assertEquals(1, pending.size)

        pending.removeAt(0).invoke()

        assertNull(session.host)
        assertEquals(
            listOf("attach:first", "detach:first"),
            session.events,
        )
    }

    @Test
    fun rebindBeforeGraceDelayKeepsSessionAttachedToNewHost() {
        val session = FakeSurfaceSession()
        val firstHost = FakeSurfaceHost("first")
        val nextHost = FakeSurfaceHost("next")
        val pending = mutableListOf<() -> Unit>()
        val coordinator = fakeCoordinator(session, pending)

        coordinator.bind("player", firstHost)
        coordinator.unbind("player", firstHost)
        coordinator.bind("player", nextHost)

        assertSame(nextHost, session.host)
        assertEquals(listOf("first"), session.clearedHosts)
        assertEquals(
            listOf("attach:first", "attach:next"),
            session.events,
        )

        pending.forEach { it.invoke() }

        assertSame(nextHost, session.host)
        assertEquals(
            listOf("attach:first", "attach:next"),
            session.events,
        )
    }

    @Test
    fun disposeSessionDetachesCurrentHostWithoutDisposingPlayer() {
        val session = FakeSurfaceSession()
        val host = FakeSurfaceHost("current")
        val pending = mutableListOf<() -> Unit>()
        val coordinator = fakeCoordinator(session, pending)

        coordinator.bind("player", host)
        coordinator.detachSession(session)

        assertNull(session.host)
        assertEquals(
            listOf("attach:current", "detach:current"),
            session.events,
        )
    }

    private fun fakeCoordinator(
        session: FakeSurfaceSession,
        pending: MutableList<() -> Unit>,
    ): SurfaceHostLifecycleCoordinator<FakeSurfaceSession, FakeSurfaceHost> =
        SurfaceHostLifecycleCoordinator(
            findSession = { id -> session.takeIf { id == "player" } },
            getHost = { it.host },
            setHost = { target, host -> target.host = host },
            cancelPendingDetach = {
                it.pendingDetachCanceled += 1
                pending.clear()
            },
            clearPendingDetach = { it.pendingDetachCleared += 1 },
            advanceDetachGeneration = {
                it.generation += 1L
                it.generation
            },
            currentDetachGeneration = { it.generation },
            schedulePendingDetach = { _, _, action -> pending += action },
            attachHost = { target, host -> target.events += "attach:${host.name}" },
            detachHost = { target, host -> target.events += "detach:${host.name}" },
            clearHostView = { host -> session.clearedHosts += host.name },
            emitSnapshot = { it.snapshotCount += 1 },
        )

    private data class FakeSurfaceHost(val name: String)

    private class FakeSurfaceSession {
        var host: FakeSurfaceHost? = null
        var generation = 0L
        var pendingDetachCanceled = 0
        var pendingDetachCleared = 0
        var snapshotCount = 0
        val clearedHosts = mutableListOf<String>()
        val events = mutableListOf<String>()
    }
}
