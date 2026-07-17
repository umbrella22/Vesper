package io.github.ikaros.vesper.player.android

/**
 * Minimal subtitle styling shared by the stable mobile host kits.
 *
 * Per-cue typography, animation, and layout remain platform- or
 * content-specific concerns.
 */
data class VesperSubtitleStyle(
    /** Text scale factor relative to the platform default. `1.0` keeps default. */
    val fontScale: Float = 1.0f,
    /** Whether subtitle rendering is visible. */
    val visible: Boolean = true,
) {
    companion object {
        val Default = VesperSubtitleStyle()
    }
}

/**
 * A side-loaded external subtitle track to attach to a [VesperPlayerSource].
 *
 * ExoPlayer consumes these through `MediaItem.subtitleConfigurations`; Vesper
 * forwards the URI, MIME type and optional language/label so the host does not
 * need to touch Media3 types directly.
 */
data class VesperSubtitleSideLoad(
    /** Subtitle file URI (local `file://`, `content://`, or remote `https://`). */
    val uri: String,
    /** Subtitle codec: `application/x-subrip` (SRT), `text/vtt` (WebVTT), or `text/x-ssa`. */
    val mimeType: String = MIME_SUBRIP,
    /** Optional BCP-47 language tag for track selection. */
    val language: String? = null,
    /** Optional human-readable label. */
    val label: String? = null,
) {
    companion object {
        /** MIME type for SRT subtitles. */
        const val MIME_SUBRIP = "application/x-subrip"

        /** MIME type for WebVTT subtitles. */
        const val MIME_WEBVTT = "text/vtt"

        /** MIME type for SSA/ASS subtitles. */
        const val MIME_SSA = "text/x-ssa"
    }
}
