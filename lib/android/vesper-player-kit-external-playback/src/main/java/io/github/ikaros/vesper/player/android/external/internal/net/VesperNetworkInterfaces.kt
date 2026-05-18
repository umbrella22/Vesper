package io.github.ikaros.vesper.player.android.external.internal.net

import java.util.Locale

internal fun String.isLikelyTunnelInterfaceName(): Boolean {
    val normalizedName = lowercase(Locale.US)
    return normalizedName.startsWith("tun") ||
        normalizedName.startsWith("tap") ||
        normalizedName.startsWith("ppp") ||
        normalizedName.startsWith("wg")
}
