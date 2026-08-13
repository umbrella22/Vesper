package io.github.umbrella22.vesper.player.android

enum class VesperPlayerErrorCode(
    val wireName: String,
    internal val jniOrdinal: Int,
) {
    InvalidArgument("invalidArgument", 0),
    InvalidState("invalidState", 1),
    InvalidSource("invalidSource", 2),
    BackendFailure("backendFailure", 3),
    AudioOutputUnavailable("audioOutputUnavailable", 4),
    DecodeFailure("decodeFailure", 5),
    SeekFailure("seekFailure", 6),
    Unsupported("unsupported", 7),
    CommandChannelClosed("commandChannelClosed", 8),
    EventChannelClosed("eventChannelClosed", 9),
    Cancelled("cancelled", 10),
    Timeout("timeout", 11);

    companion object {
        private val logger = java.util.logging.Logger.getLogger(VesperPlayerErrorCode::class.java.name)

        fun fromWireName(wireName: String?): VesperPlayerErrorCode =
            entries.firstOrNull { it.wireName == wireName } ?: BackendFailure

        internal fun fromJniOrdinal(ordinal: Int): VesperPlayerErrorCode {
            val entry = entries.firstOrNull { it.jniOrdinal == ordinal }
            if (entry == null) {
                logger.warning { "Unknown VesperPlayerErrorCode ordinal $ordinal from native layer; falling back to BackendFailure" }
                return BackendFailure
            }
            return entry
        }
    }
}

enum class VesperPlayerErrorCategory(
    val wireName: String,
    internal val jniOrdinal: Int,
) {
    Input("input", 0),
    Source("source", 1),
    Network("network", 2),
    Decode("decode", 3),
    AudioOutput("audioOutput", 4),
    Playback("playback", 5),
    Capability("capability", 6),
    Platform("platform", 7);

    companion object {
        private val logger = java.util.logging.Logger.getLogger(VesperPlayerErrorCategory::class.java.name)

        fun fromWireName(wireName: String?): VesperPlayerErrorCategory =
            entries.firstOrNull { it.wireName == wireName } ?: Platform

        internal fun fromJniOrdinal(ordinal: Int): VesperPlayerErrorCategory {
            val entry = entries.firstOrNull { it.jniOrdinal == ordinal }
            if (entry == null) {
                logger.warning { "Unknown VesperPlayerErrorCategory ordinal $ordinal from native layer; falling back to Platform" }
                return Platform
            }
            return entry
        }
    }
}

data class VesperPlayerErrorState(
    val message: String,
    val code: VesperPlayerErrorCode,
    val category: VesperPlayerErrorCategory,
    val retriable: Boolean,
    val details: Map<String, Any?> = emptyMap(),
) {
    fun toMap(): Map<String, Any?> =
        mapOf(
            "message" to message,
            "code" to code.wireName,
            "category" to category.wireName,
            "retriable" to retriable,
            "details" to details,
        )
}
