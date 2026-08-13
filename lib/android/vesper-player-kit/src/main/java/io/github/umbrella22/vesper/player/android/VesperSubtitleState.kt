package io.github.umbrella22.vesper.player.android

/** Canonical subtitle catalog lifecycle shared by Android, iOS, and Flutter. */
enum class VesperSubtitleCatalogState(val wireName: String) {
    Unavailable("unavailable"),
    Loading("loading"),
    Ready("ready"),
    Failed("failed"),
    Unknown("unknown");

    companion object {
        fun fromWire(raw: String?): VesperSubtitleCatalogState =
            values().firstOrNull { it.wireName == raw } ?:
                if (raw == null) Unavailable else Unknown
    }
}

/** Canonical subtitle selection transaction lifecycle. */
enum class VesperSubtitleSelectionState(val wireName: String) {
    Idle("idle"),
    Applying("applying"),
    Confirmed("confirmed"),
    Failed("failed"),
    Unknown("unknown");

    companion object {
        fun fromWire(raw: String?): VesperSubtitleSelectionState =
            values().firstOrNull { it.wireName == raw } ?:
                if (raw == null) Idle else Unknown
    }
}

/** Legacy catalog status retained as a read-only compatibility alias. */
enum class VesperSubtitleStatus(val wireName: String) {
    Unavailable("unavailable"),
    Loading("loading"),
    Ready("ready"),
    Failed("failed"),
    Unknown("unknown");

    companion object {
        fun fromWire(raw: String?): VesperSubtitleStatus =
            values().firstOrNull { it.wireName == raw } ?:
                if (raw == null) Unavailable else Unknown
    }
}

/** Phase where a subtitle failure originated. */
enum class VesperSubtitleErrorPhase(val wireName: String) {
    Manifest("manifest"),
    Resource("resource"),
    Discovery("discovery"),
    Identity("identity"),
    Selection("selection"),
    Unknown("unknown");

    companion object {
        fun fromWire(raw: String?): VesperSubtitleErrorPhase =
            values().firstOrNull { it.wireName == raw } ?: Unknown
    }
}

/** Structured subtitle error. Raw code and phase strings are intentionally preserved. */
data class VesperSubtitleError(
    val code: String,
    val phase: VesperSubtitleErrorPhase,
    val retriable: Boolean,
    val message: String,
    val trackId: String? = null,
    val commandId: Long? = null,
    val sourceEpoch: Long? = null,
    val phaseRawValue: String? = null,
) {
    fun toMap(): Map<String, Any?> =
        buildMap {
            put("code", code)
            put("phase", phaseRawValue ?: phase.wireName)
            put("retriable", retriable)
            put("message", message)
            put("trackId", trackId)
            commandId?.let { put("commandId", it) }
            sourceEpoch?.let { put("sourceEpoch", it) }
        }
}

/**
 * Canonical subtitle state. Selection failures update only selection fields;
 * catalog state, counts, and catalog errors remain intact.
 */
data class VesperSubtitleState(
    val catalogState: VesperSubtitleCatalogState = VesperSubtitleCatalogState.Unavailable,
    val selectionState: VesperSubtitleSelectionState = VesperSubtitleSelectionState.Idle,
    val advertisedTrackCount: Int = 0,
    val selectableTrackCount: Int = 0,
    val catalogError: VesperSubtitleError? = null,
    val selectionError: VesperSubtitleError? = null,
    val catalogStateRawValue: String? = null,
    val selectionStateRawValue: String? = null,
) {
    /** Compatibility alias for pre-0.4 callers. */
    val status: VesperSubtitleStatus
        get() = when (catalogState) {
            VesperSubtitleCatalogState.Unavailable -> VesperSubtitleStatus.Unavailable
            VesperSubtitleCatalogState.Loading -> VesperSubtitleStatus.Loading
            VesperSubtitleCatalogState.Ready -> VesperSubtitleStatus.Ready
            VesperSubtitleCatalogState.Failed -> VesperSubtitleStatus.Failed
            VesperSubtitleCatalogState.Unknown -> VesperSubtitleStatus.Unknown
        }

    /** Compatibility alias; selection failures take precedence for old hosts. */
    val error: VesperSubtitleError?
        get() = selectionError ?: catalogError

    fun toMap(): Map<String, Any?> =
        mapOf(
            "catalogState" to (catalogStateRawValue ?: catalogState.wireName),
            "selectionState" to (selectionStateRawValue ?: selectionState.wireName),
            "advertisedTrackCount" to advertisedTrackCount,
            "selectableTrackCount" to selectableTrackCount,
            "catalogError" to catalogError?.toMap(),
            "selectionError" to selectionError?.toMap(),
            // Compatibility aliases for pre-0.4 Flutter hosts.
            "status" to status.wireName,
            "error" to error?.toMap(),
        )

    companion object {
        val EMPTY = VesperSubtitleState()

        fun unavailable() = VesperSubtitleState()

        fun loading(advertisedTrackCount: Int) =
            VesperSubtitleState(
                catalogState = VesperSubtitleCatalogState.Loading,
                advertisedTrackCount = advertisedTrackCount,
            )

        fun ready(advertisedTrackCount: Int, selectableTrackCount: Int) =
            VesperSubtitleState(
                catalogState = VesperSubtitleCatalogState.Ready,
                selectionState = VesperSubtitleSelectionState.Idle,
                advertisedTrackCount = advertisedTrackCount,
                selectableTrackCount = selectableTrackCount,
            )

        fun failed(
            advertisedTrackCount: Int,
            code: String,
            phase: VesperSubtitleErrorPhase,
            trackId: String? = null,
            retriable: Boolean = false,
            message: String,
            selectableTrackCount: Int = 0,
        ) = if (phase == VesperSubtitleErrorPhase.Selection) {
            VesperSubtitleState(
                catalogState = VesperSubtitleCatalogState.Ready,
                selectionState = VesperSubtitleSelectionState.Failed,
                advertisedTrackCount = advertisedTrackCount,
                selectableTrackCount = selectableTrackCount,
                selectionError = VesperSubtitleError(
                    code = code,
                    phase = phase,
                    trackId = trackId,
                    retriable = retriable,
                    message = message,
                ),
            )
        } else {
            VesperSubtitleState(
                catalogState = VesperSubtitleCatalogState.Failed,
                advertisedTrackCount = advertisedTrackCount,
                selectableTrackCount = selectableTrackCount,
                catalogError = VesperSubtitleError(
                    code = code,
                    phase = phase,
                    trackId = trackId,
                    retriable = retriable,
                    message = message,
                ),
            )
        }
    }
}
