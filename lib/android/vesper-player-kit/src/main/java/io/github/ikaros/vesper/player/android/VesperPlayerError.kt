package io.github.ikaros.vesper.player.android

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
        fun fromWireName(wireName: String?): VesperPlayerErrorCode =
            entries.firstOrNull { it.wireName == wireName } ?: BackendFailure

        internal fun fromJniOrdinal(ordinal: Int): VesperPlayerErrorCode =
            entries.firstOrNull { it.jniOrdinal == ordinal } ?: BackendFailure
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
        fun fromWireName(wireName: String?): VesperPlayerErrorCategory =
            entries.firstOrNull { it.wireName == wireName } ?: Platform

        internal fun fromJniOrdinal(ordinal: Int): VesperPlayerErrorCategory =
            entries.firstOrNull { it.jniOrdinal == ordinal } ?: Platform
    }
}
