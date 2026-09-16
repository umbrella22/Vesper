package io.github.umbrella22.vesper.player.android.external.internal.dlna

import android.content.Context
import android.content.ContextWrapper
import java.net.URL
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicReference
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertSame
import org.junit.Assert.assertTrue
import org.junit.Test

class VesperDlnaDiscoveryRoutesTest {
    private val context = object : ContextWrapper(null) {
        override fun getApplicationContext(): Context = this
    }

    @Test
    fun callbacksRunOutsideRouteLockAndReentrantStopIsDeliveredLast() {
        val delivered = mutableListOf<Int>()
        lateinit var discovery: VesperDlnaDiscovery
        discovery = VesperDlnaDiscovery(context, object : VesperDlnaDiscovery.Listener {
            override fun onRoutesChanged(routes: List<VesperDlnaDevice>) {
                assertFalse(Thread.holdsLock(discovery.routeLock))
                if (routes.isNotEmpty()) discovery.stop()
                delivered += routes.size
            }
            override fun onDiscoveryError(message: String) = Unit
        })
        discovery.running.set(true)
        discovery.upsertDevice(device(), 0)
        assertEquals(listOf(1, 0), delivered)
        discovery.upsertDevice(device(), 0)
        assertEquals(listOf(1, 0), delivered)
    }

    @Test
    fun slowCallbackDoesNotBlockMutationAndCannotPublishStaleRoutesAfterStop() {
        for (throwAfterDelivery in listOf(false, true)) {
            val entered = CountDownLatch(1)
            val release = CountDownLatch(1)
            val delivered = mutableListOf<Int>()
            val callbackFailure = IllegalStateException("listener failed")
            val workerFailure = AtomicReference<Throwable>()
            val discovery = VesperDlnaDiscovery(context, object : VesperDlnaDiscovery.Listener {
                override fun onRoutesChanged(routes: List<VesperDlnaDevice>) {
                    if (routes.isNotEmpty()) {
                        entered.countDown()
                        assertTrue(release.await(5, TimeUnit.SECONDS))
                    }
                    delivered += routes.size
                    if (throwAfterDelivery && routes.isNotEmpty()) throw callbackFailure
                }
                override fun onDiscoveryError(message: String) = Unit
            })
            discovery.running.set(true)
            val worker = Thread {
                runCatching { discovery.upsertDevice(device(), 0) }.onFailure(workerFailure::set)
            }
            worker.start()
            try {
                assertTrue(entered.await(2, TimeUnit.SECONDS))
                val stopped = CountDownLatch(1)
                val stopper = Thread { discovery.stop(); stopped.countDown() }
                stopper.start()
                assertTrue("stop must not wait for a host callback", stopped.await(2, TimeUnit.SECONDS))
                stopper.join(2_000)
            } finally {
                release.countDown()
                worker.join(2_000)
            }
            assertFalse(worker.isAlive)
            assertEquals(listOf(1, 0), delivered)
            assertSame(if (throwAfterDelivery) callbackFailure else null, workerFailure.get())
        }
    }

    private fun device(): VesperDlnaDevice {
        val url = URL("http://192.0.2.1/description.xml")
        return VesperDlnaDevice(
            routeId = "uuid:test", location = url, usn = "uuid:test", friendlyName = "TV",
            avTransport = VesperDlnaService(
                serviceType = "urn:schemas-upnp-org:service:AVTransport:1",
                serviceId = "urn:upnp-org:serviceId:AVTransport", controlUrl = url,
                eventSubUrl = url, scpdUrl = url,
            ),
        )
    }
}
