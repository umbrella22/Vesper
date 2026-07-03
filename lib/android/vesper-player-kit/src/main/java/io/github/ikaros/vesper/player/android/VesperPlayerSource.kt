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

data class VesperPlayerDrmConfiguration(
    val keySystem: String,
    val licenseUri: String,
    val licenseHeaders: Map<String, String> = emptyMap(),
    val fairPlayCertificateUri: String? = null,
    val fairPlayCertificateBase64: String? = null,
    val multiSession: Boolean = false,
)

fun vesperPlayerDrmConfigurationFromWireMap(map: Map<String, Any?>): VesperPlayerDrmConfiguration =
    VesperPlayerDrmConfiguration(
        keySystem = map["keySystem"] as? String ?: "",
        licenseUri = map["licenseUri"] as? String ?: "",
        licenseHeaders = map["licenseHeaders"].vesperStringStringMap(),
        fairPlayCertificateUri = map["fairPlayCertificateUri"] as? String,
        fairPlayCertificateBase64 = map["fairPlayCertificateBase64"] as? String,
        multiSession = map["multiSession"] as? Boolean ?: false,
    )

private fun Any?.vesperStringStringMap(): Map<String, String> =
    (this as? Map<*, *>)
        ?.mapNotNull { (key, value) ->
            val name = key as? String ?: return@mapNotNull null
            val text = value as? String ?: return@mapNotNull null
            name to text
        }
        ?.toMap()
        ?: emptyMap()

data class VesperPlayerSource(
    val uri: String,
    val label: String,
    val kind: VesperPlayerSourceKind,
    val protocol: VesperPlayerSourceProtocol,
    val headers: Map<String, String> = emptyMap(),
    val drmConfiguration: VesperPlayerDrmConfiguration? = null,
) {
    companion object {
        fun local(
            uri: String,
            label: String,
            headers: Map<String, String> = emptyMap(),
            drmConfiguration: VesperPlayerDrmConfiguration? = null,
        ): VesperPlayerSource =
            VesperPlayerSource(
                uri = uri,
                label = label,
                kind = VesperPlayerSourceKind.Local,
                protocol = inferLocalProtocol(uri),
                headers = headers,
                drmConfiguration = drmConfiguration,
            )

        fun localDash(
            uri: String,
            label: String,
            headers: Map<String, String> = emptyMap(),
            drmConfiguration: VesperPlayerDrmConfiguration? = null,
        ): VesperPlayerSource =
            VesperPlayerSource(
                uri = uri,
                label = label,
                kind = VesperPlayerSourceKind.Local,
                protocol = VesperPlayerSourceProtocol.Dash,
                headers = headers,
                drmConfiguration = drmConfiguration,
            )

        fun remote(
            uri: String,
            label: String,
            protocol: VesperPlayerSourceProtocol = inferRemoteProtocol(uri),
            headers: Map<String, String> = emptyMap(),
            drmConfiguration: VesperPlayerDrmConfiguration? = null,
        ): VesperPlayerSource =
            VesperPlayerSource(
                uri = uri,
                label = label,
                kind = VesperPlayerSourceKind.Remote,
                protocol = protocol,
                headers = headers,
                drmConfiguration = drmConfiguration,
            )

        fun hls(
            uri: String,
            label: String,
            headers: Map<String, String> = emptyMap(),
            drmConfiguration: VesperPlayerDrmConfiguration? = null,
        ): VesperPlayerSource =
            remote(
                uri = uri,
                label = label,
                protocol = VesperPlayerSourceProtocol.Hls,
                headers = headers,
                drmConfiguration = drmConfiguration,
            )

        fun dash(
            uri: String,
            label: String,
            headers: Map<String, String> = emptyMap(),
            drmConfiguration: VesperPlayerDrmConfiguration? = null,
        ): VesperPlayerSource =
            remote(
                uri = uri,
                label = label,
                protocol = VesperPlayerSourceProtocol.Dash,
                headers = headers,
                drmConfiguration = drmConfiguration,
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
