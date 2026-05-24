package io.github.ikaros.vesper.player.android.external.internal.relay.ffmpeg

import java.io.File
import java.net.URI
import java.util.Locale

internal fun resolveDashReference(
    baseUri: String,
    reference: String,
    origin: VesperRelayDashSourceOrigin,
    baseDetails: Map<String, String>,
): String {
    val resolved =
        runCatching {
            val ref = URI(reference)
            if (ref.isAbsolute || baseUri.isBlank()) {
                ref.toString()
            } else {
                URI(baseUri).resolve(ref).normalize().toString()
            }
        }.getOrElse { reference }
    val scheme = resolved.substringBefore(':', missingDelimiterValue = "").lowercase(Locale.US)
    return when (origin.kind) {
        "remote" -> {
            if (scheme == "http" || scheme == "https") {
                resolved
            } else {
                throw unsupportedMixedDashOrigin(baseDetails, origin, resolved)
            }
        }
        "file" -> {
            if (resolved.isRemoteDashUri() && origin.allowRemoteMediaReferences) {
                resolved
            } else if (scheme == "file") {
                val root = File(URI(origin.rootUri)).canonicalFile.toPath()
                val candidate = File(URI(resolved)).canonicalFile.toPath()
                if (candidate.startsWith(root)) {
                    resolved
                } else {
                    throw unsupportedMixedDashOrigin(baseDetails, origin, resolved)
                }
            } else {
                throw unsupportedMixedDashOrigin(baseDetails, origin, resolved)
            }
        }
        "content" -> {
            if (resolved.isRemoteDashUri() && origin.allowRemoteMediaReferences) {
                resolved
            } else if (scheme == "content" && contentUriWithinRoot(resolved, origin.rootUri)) {
                resolved
            } else {
                throw unsupportedMixedDashOrigin(baseDetails, origin, resolved)
            }
        }
        else -> throw unsupportedMixedDashOrigin(baseDetails, origin, resolved)
    }
}

internal fun remoteDashOrigin(uri: String): VesperRelayDashSourceOrigin =
    VesperRelayDashSourceOrigin(
        kind = "remote",
        manifestUri = uri,
        rootUri = uri,
    )
