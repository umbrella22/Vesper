package io.github.ikaros.vesper.player.android

enum class VesperPlayerSourceKind {
    Local,
    Remote,
}

enum class VesperPlayerSourceProtocol {
    Unknown,
    File,
    Content,
    Progressive,
    Hls,
    Dash,
}

data class VesperPlayerSource(
    val uri: String,
    val label: String,
    val kind: VesperPlayerSourceKind,
    val protocol: VesperPlayerSourceProtocol,
    val headers: Map<String, String> = emptyMap(),
) {
    companion object {
        fun local(
            uri: String,
            label: String,
            headers: Map<String, String> = emptyMap(),
        ): VesperPlayerSource =
            VesperPlayerSource(
                uri = uri,
                label = label,
                kind = VesperPlayerSourceKind.Local,
                protocol = inferLocalProtocol(uri),
                headers = headers,
            )

        fun remote(
            uri: String,
            label: String,
            protocol: VesperPlayerSourceProtocol = inferRemoteProtocol(uri),
            headers: Map<String, String> = emptyMap(),
        ): VesperPlayerSource =
            VesperPlayerSource(
                uri = uri,
                label = label,
                kind = VesperPlayerSourceKind.Remote,
                protocol = protocol,
                headers = headers,
            )

        fun hls(
            uri: String,
            label: String,
            headers: Map<String, String> = emptyMap(),
        ): VesperPlayerSource =
            remote(
                uri = uri,
                label = label,
                protocol = VesperPlayerSourceProtocol.Hls,
                headers = headers,
            )

        fun dash(
            uri: String,
            label: String,
            headers: Map<String, String> = emptyMap(),
        ): VesperPlayerSource =
            remote(
                uri = uri,
                label = label,
                protocol = VesperPlayerSourceProtocol.Dash,
                headers = headers,
            )

        private fun inferLocalProtocol(uri: String): VesperPlayerSourceProtocol =
            when {
                uri.startsWith("content://", ignoreCase = true) -> VesperPlayerSourceProtocol.Content
                uri.startsWith("file://", ignoreCase = true) -> VesperPlayerSourceProtocol.File
                else -> VesperPlayerSourceProtocol.Unknown
            }

        private fun inferRemoteProtocol(uri: String): VesperPlayerSourceProtocol {
            val normalized = uri.lowercase()
            val normalizedPath = normalized
                .substringBefore('#')
                .substringBefore('?')
            return when {
                normalizedPath.endsWith(".m3u8") -> VesperPlayerSourceProtocol.Hls
                normalizedPath.endsWith(".mpd") -> VesperPlayerSourceProtocol.Dash
                normalized.startsWith("http://") || normalized.startsWith("https://") ->
                    VesperPlayerSourceProtocol.Progressive
                else -> VesperPlayerSourceProtocol.Unknown
            }
        }
    }
}
