package io.github.ikaros.vesper.player.android.external.internal.dlna

import java.net.URL
import java.util.Locale

data class VesperSsdpMessage(
    val startLine: String,
    val headers: Map<String, String>,
) {
    val location: String?
        get() = headers["location"]
    val usn: String?
        get() = headers["usn"]
    val st: String?
        get() = headers["st"]
    val nts: String?
        get() = headers["nts"]
    val server: String?
        get() = headers["server"]
    val nt: String?
        get() = headers["nt"]
    val cacheMaxAgeSeconds: Long?
        get() = headers["cache-control"]?.let(::parseCacheMaxAgeSeconds)
    val isAliveNotify: Boolean
        get() = nts.equals("ssdp:alive", ignoreCase = true)
    val isByebyeNotify: Boolean
        get() = nts.equals("ssdp:byebye", ignoreCase = true)
    val isMediaRenderer: Boolean
        get() = listOfNotNull(st, nt, usn).any {
            it.contains("MediaRenderer", ignoreCase = true)
        }
    val shouldFetchDescription: Boolean
        get() = location != null && !isByebyeNotify
}

object VesperSsdpParser {
    fun parse(raw: String): VesperSsdpMessage? {
        val lines = raw.replace("\r\n", "\n").split('\n')
            .map { it.trimEnd('\r') }
            .filter { it.isNotBlank() }
        val startLine = lines.firstOrNull()?.trim() ?: return null
        val headers = linkedMapOf<String, String>()
        for (line in lines.drop(1)) {
            val separator = line.indexOf(':')
            if (separator <= 0) {
                continue
            }
            headers[line.substring(0, separator).trim().lowercase(Locale.US)] =
                line.substring(separator + 1).trim()
        }
        return VesperSsdpMessage(startLine = startLine, headers = headers)
    }
}

fun VesperSsdpMessage.toDescriptionRequest(nowMillis: Long): VesperDlnaDescriptionRequest? {
    val locationValue = location ?: return null
    val usnValue = usn?.takeIf { it.isNotBlank() } ?: locationValue
    val locationUrl = runCatching { URL(locationValue) }.getOrNull() ?: return null
    val maxAgeMillis = (cacheMaxAgeSeconds ?: DEFAULT_ROUTE_TTL_SECONDS)
        .coerceAtLeast(MIN_ROUTE_TTL_SECONDS) * 1000L
    return VesperDlnaDescriptionRequest(
        location = locationUrl,
        usn = usnValue,
        expiresAtMillis = nowMillis + maxAgeMillis,
    )
}

data class VesperDlnaDescriptionRequest(
    val location: URL,
    val usn: String,
    val expiresAtMillis: Long,
)

private fun parseCacheMaxAgeSeconds(value: String): Long? =
    value.split(',')
        .map { it.trim() }
        .firstOrNull { it.startsWith("max-age", ignoreCase = true) }
        ?.substringAfter('=', missingDelimiterValue = "")
        ?.trim()
        ?.toLongOrNull()

internal fun canonicalDlnaRouteId(value: String): String =
    value.substringBefore("::")
        .trim()
        .takeIf { it.isNotEmpty() }
        ?: value

internal fun dlnaRouteIdentityKey(value: String): String =
    canonicalDlnaRouteId(value)
        .withoutUuidPrefix()
        .lowercase(Locale.US)

private fun String.withoutUuidPrefix(): String =
    when {
        startsWith("uuid:", ignoreCase = true) -> substring("uuid:".length)
        startsWith("urn:uuid:", ignoreCase = true) -> substring("urn:uuid:".length)
        else -> this
    }

private const val DEFAULT_ROUTE_TTL_SECONDS = 1800L
private const val MIN_ROUTE_TTL_SECONDS = 120L
