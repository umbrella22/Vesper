package io.github.umbrella22.vesper.player.android

import java.security.MessageDigest

/** A correlation label for logs only; never a resource or authorization identity. */
internal fun diagnosticSourceDescription(source: VesperPlayerSource): String =
    "id=${diagnosticResourceId(source.uri)} kind=${source.kind} protocol=${source.protocol}"

/** Exact locators produce opaque correlation IDs; this does not change request or cache identity. */
internal fun diagnosticResourceId(uri: String): String =
    MessageDigest.getInstance("SHA-256")
        .digest(uri.toByteArray(Charsets.UTF_8))
        .take(8).joinToString("") { "%02x".format(it) }

/** Throwable messages and toString can include signed locators or request headers. */
internal fun diagnosticPlaybackFailure(error: Throwable): String =
    generateSequence(error) { it.cause }.take(8)
        .joinToString(" <- ") { it.javaClass.simpleName }
