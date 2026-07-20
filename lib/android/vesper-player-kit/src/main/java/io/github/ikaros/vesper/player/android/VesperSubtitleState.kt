package io.github.ikaros.vesper.player.android

/**
 * Subtitle lifecycle status shared by Android, iOS, and Flutter.
 *
 * Mirrors the Swift `VesperSubtitleStatus` enum. Wire names are lowercase to match the
 * Dart enum names in `subtitle_state_models.dart`.
 */
enum class VesperSubtitleStatus(val wireName: String) {
    Unavailable("unavailable"),
    Loading("loading"),
    Ready("ready"),
    Failed("failed"),
    Unknown("unknown");

    companion object {
        fun fromWire(raw: String?): VesperSubtitleStatus {
            if (raw == null) return Unavailable
            return values().firstOrNull { it.wireName == raw } ?: Unknown
        }
    }
}

/**
 * Phase where a subtitle failure originated.
 */
enum class VesperSubtitleErrorPhase(val wireName: String) {
    Manifest("manifest"),
    Resource("resource"),
    Discovery("discovery"),
    Identity("identity"),
    Selection("selection"),
    Unknown("unknown");

    companion object {
        fun fromWire(raw: String?): VesperSubtitleErrorPhase {
            if (raw == null) return Unknown
            return values().firstOrNull { it.wireName == raw } ?: Unknown
        }
    }
}

/**
 * Structured subtitle error carried alongside [VesperSubtitleState].
 *
 * The [code] is a stable string (e.g. `subtitle_track_not_found`) defined
 * by the cross-platform subtitle contract. Unknown codes are preserved verbatim so
 * a newer native side does not silently lose diagnostic information.
 */
data class VesperSubtitleError(
    val code: String,
    val phase: VesperSubtitleErrorPhase,
    val retriable: Boolean,
    val message: String,
    val trackId: String? = null,
) {
    fun toMap(): Map<String, Any?> =
        mapOf(
            "code" to code,
            "phase" to phase.wireName,
            "retriable" to retriable,
            "message" to message,
            "trackId" to trackId,
        )

    companion object {
        fun fromMap(map: Map<String, Any?>): VesperSubtitleError =
            VesperSubtitleError(
                code = (map["code"] as? String) ?: "unknown",
                phase = VesperSubtitleErrorPhase.fromWire(map["phase"] as? String),
                retriable = (map["retriable"] as? Boolean) ?: false,
                message = (map["message"] as? String) ?: "",
                trackId = map["trackId"] as? String,
            )
    }
}

/**
 * Snapshot of subtitle catalog lifecycle exposed to Flutter. Mirrors the
 * Swift `VesperSubtitleState`.
 */
data class VesperSubtitleState(
    val status: VesperSubtitleStatus = VesperSubtitleStatus.Unavailable,
    val advertisedTrackCount: Int = 0,
    val selectableTrackCount: Int = 0,
    val error: VesperSubtitleError? = null,
) {
    fun toMap(): Map<String, Any?> =
        mapOf(
            "status" to status.wireName,
            "advertisedTrackCount" to advertisedTrackCount,
            "selectableTrackCount" to selectableTrackCount,
            "error" to error?.toMap(),
        )

    companion object {
        val EMPTY = VesperSubtitleState()

        fun unavailable() = VesperSubtitleState(status = VesperSubtitleStatus.Unavailable)

        fun loading(advertisedTrackCount: Int) =
            VesperSubtitleState(
                status = VesperSubtitleStatus.Loading,
                advertisedTrackCount = advertisedTrackCount,
            )

        fun ready(advertisedTrackCount: Int, selectableTrackCount: Int) =
            VesperSubtitleState(
                status = VesperSubtitleStatus.Ready,
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
        ) = VesperSubtitleState(
            status = VesperSubtitleStatus.Failed,
            advertisedTrackCount = advertisedTrackCount,
            selectableTrackCount = 0,
            error =
                VesperSubtitleError(
                    code = code,
                    phase = phase,
                    trackId = trackId,
                    retriable = retriable,
                    message = message,
                ),
        )
    }
}
