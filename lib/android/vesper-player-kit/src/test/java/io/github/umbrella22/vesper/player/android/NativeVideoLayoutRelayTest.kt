package io.github.umbrella22.vesper.player.android

import org.junit.Assert.assertEquals
import org.junit.Test

class NativeVideoLayoutRelayTest {
    @Test
    fun listenerReceivesLayoutThatArrivedBeforeBinding() {
        val relay = NativeVideoLayoutRelay()
        val layout = NativeVideoLayoutInfo(width = 1920, height = 1080)
        relay.update(layout)

        val observed = mutableListOf<NativeVideoLayoutInfo?>()
        relay.setListener(observed::add)

        assertEquals(listOf(layout), observed)
    }

    @Test
    fun replacementListenerReplaysLatestLayoutWithoutNotifyingDetachedListener() {
        val relay = NativeVideoLayoutRelay()
        val firstObserved = mutableListOf<NativeVideoLayoutInfo?>()
        relay.setListener(firstObserved::add)

        val first = NativeVideoLayoutInfo(width = 1280, height = 720)
        relay.update(first)
        relay.setListener(null)
        val latest = NativeVideoLayoutInfo(width = 720, height = 1280)
        relay.update(latest)

        val replacementObserved = mutableListOf<NativeVideoLayoutInfo?>()
        relay.setListener(replacementObserved::add)

        assertEquals(listOf(null, first), firstObserved)
        assertEquals(listOf(latest), replacementObserved)
    }
}
