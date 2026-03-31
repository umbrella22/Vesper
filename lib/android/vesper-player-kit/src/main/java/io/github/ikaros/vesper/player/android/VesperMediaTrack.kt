package io.github.ikaros.vesper.player.android

enum class VesperMediaTrackKind {
    Video,
    Audio,
    Subtitle,
}

data class VesperMediaTrack(
    val id: String,
    val kind: VesperMediaTrackKind,
    val label: String? = null,
    val language: String? = null,
    val codec: String? = null,
    val bitRate: Long? = null,
    val width: Int? = null,
    val height: Int? = null,
    val frameRate: Float? = null,
    val channels: Int? = null,
    val sampleRate: Int? = null,
    val isDefault: Boolean = false,
    val isForced: Boolean = false,
)

data class VesperTrackCatalog(
    val tracks: List<VesperMediaTrack> = emptyList(),
    val adaptiveVideo: Boolean = false,
    val adaptiveAudio: Boolean = false,
) {
    val videoTracks: List<VesperMediaTrack>
        get() = tracks.filter { it.kind == VesperMediaTrackKind.Video }

    val audioTracks: List<VesperMediaTrack>
        get() = tracks.filter { it.kind == VesperMediaTrackKind.Audio }

    val subtitleTracks: List<VesperMediaTrack>
        get() = tracks.filter { it.kind == VesperMediaTrackKind.Subtitle }

    companion object {
        val Empty = VesperTrackCatalog()
    }
}

data class VesperTrackSelectionSnapshot(
    val video: VesperTrackSelection = VesperTrackSelection.auto(),
    val audio: VesperTrackSelection = VesperTrackSelection.auto(),
    val subtitle: VesperTrackSelection = VesperTrackSelection.disabled(),
    val abrPolicy: VesperAbrPolicy = VesperAbrPolicy.auto(),
)
