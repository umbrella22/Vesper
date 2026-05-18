package io.github.ikaros.vesper.player.android.external.internal.net

import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class VesperNetworkInterfacesTest {
    @Test
    fun detectsLikelyTunnelInterfaceNames() {
        assertTrue("tun0".isLikelyTunnelInterfaceName())
        assertTrue("TapAdapter".isLikelyTunnelInterfaceName())
        assertTrue("ppp42".isLikelyTunnelInterfaceName())
        assertTrue("wg-vesper".isLikelyTunnelInterfaceName())
        assertFalse("wlan0".isLikelyTunnelInterfaceName())
        assertFalse("eth0".isLikelyTunnelInterfaceName())
    }
}
