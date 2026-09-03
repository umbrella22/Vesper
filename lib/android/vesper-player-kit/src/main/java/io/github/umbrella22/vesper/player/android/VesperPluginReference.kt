package io.github.umbrella22.vesper.player.android

enum class VesperPluginTransport {
    Native,
    Wasm,
    Unknown,
}

/** Explicit selection of one plugin transport and optional capability instance. */
data class VesperPluginReference(
    val pluginId: String,
    val capabilityInstanceId: String? = null,
    val transport: VesperPluginTransport,
    val transportRawValue: String? = null,
) {
    init {
        require(isValidPluginIdentity(pluginId)) {
            "pluginId must be a valid reverse-DNS identity"
        }
        require(capabilityInstanceId == null || isValidPluginIdentity(capabilityInstanceId)) {
            "capabilityInstanceId must be a valid reverse-DNS identity"
        }
        require(
            when (transport) {
                VesperPluginTransport.Native, VesperPluginTransport.Wasm ->
                    transportRawValue == null
                VesperPluginTransport.Unknown -> !transportRawValue.isNullOrEmpty()
            },
        ) {
            "transportRawValue is required only for an unknown transport"
        }
    }

    val transportWireName: String
        get() =
            when (transport) {
                VesperPluginTransport.Native -> "native"
                VesperPluginTransport.Wasm -> "wasm"
                VesperPluginTransport.Unknown -> transportRawValue.orEmpty()
            }

    companion object {
        fun fromWire(
            pluginId: String,
            capabilityInstanceId: String?,
            transportRawValue: String,
        ): VesperPluginReference {
            require(transportRawValue.isNotEmpty()) { "transport is required" }
            val transport =
                when (transportRawValue) {
                    "native" -> VesperPluginTransport.Native
                    "wasm" -> VesperPluginTransport.Wasm
                    else -> VesperPluginTransport.Unknown
                }
            return VesperPluginReference(
                pluginId = pluginId,
                capabilityInstanceId = capabilityInstanceId,
                transport = transport,
                transportRawValue = transportRawValue.takeIf {
                    transport == VesperPluginTransport.Unknown
                },
            )
        }
    }
}

/** Canonical references for plugins distributed with the Android host kit. */
object VesperBundledPluginReferences {
    @JvmField
    val sourceNormalizerFfmpeg =
        VesperPluginReference(
            pluginId = "io.github.umbrella22.vesper.source-normalizer-ffmpeg",
            transport = VesperPluginTransport.Native,
        )

    @JvmField
    val remuxFfmpeg =
        VesperPluginReference(
            pluginId = "io.github.umbrella22.vesper.remux-ffmpeg",
            transport = VesperPluginTransport.Native,
        )

    @JvmField
    val decoderMediaCodec =
        VesperPluginReference(
            pluginId = "io.github.umbrella22.vesper.decoder-mediacodec",
            transport = VesperPluginTransport.Native,
        )

    @JvmField
    val frameProcessorDiagnostic =
        VesperPluginReference(
            pluginId = "dev.vesper.frame-processor-diagnostic",
            transport = VesperPluginTransport.Native,
        )

    @JvmField
    val performanceDiagnostics =
        VesperPluginReference(
            pluginId = "io.github.umbrella22.vesper.performance-diagnostics",
            capabilityInstanceId =
                "io.github.umbrella22.vesper.performance-diagnostics.benchmark",
            transport = VesperPluginTransport.Native,
        )
}

internal fun isValidPluginIdentity(value: String): Boolean {
    if (value.isEmpty() || value.length > 255 || value.any { it.code > 0x7f }) {
        return false
    }
    val segments = value.split('.')
    return segments.size >= 2 && segments.all(::isValidPluginIdentitySegment)
}

private fun isValidPluginIdentitySegment(segment: String): Boolean {
    if (segment.isEmpty() || segment.first() !in 'a'..'z') {
        return false
    }
    if (segment.last() !in 'a'..'z' && segment.last() !in '0'..'9') {
        return false
    }
    return segment.all { character ->
        character in 'a'..'z' || character in '0'..'9' || character == '-'
    }
}
