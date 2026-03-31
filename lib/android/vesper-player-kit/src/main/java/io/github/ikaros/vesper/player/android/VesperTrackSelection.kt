package io.github.ikaros.vesper.player.android

enum class VesperTrackSelectionMode {
    Auto,
    Disabled,
    Track,
}

data class VesperTrackSelection(
    val mode: VesperTrackSelectionMode,
    val trackId: String? = null,
) {
    companion object {
        fun auto(): VesperTrackSelection = VesperTrackSelection(VesperTrackSelectionMode.Auto)

        fun disabled(): VesperTrackSelection =
            VesperTrackSelection(VesperTrackSelectionMode.Disabled)

        fun track(trackId: String): VesperTrackSelection =
            VesperTrackSelection(VesperTrackSelectionMode.Track, trackId)
    }
}

enum class VesperAbrMode {
    Auto,
    Constrained,
    FixedTrack,
}

data class VesperAbrPolicy(
    val mode: VesperAbrMode,
    val trackId: String? = null,
    val maxBitRate: Long? = null,
    val maxWidth: Int? = null,
    val maxHeight: Int? = null,
) {
    companion object {
        fun auto(): VesperAbrPolicy = VesperAbrPolicy(VesperAbrMode.Auto)

        fun constrained(
            maxBitRate: Long? = null,
            maxWidth: Int? = null,
            maxHeight: Int? = null,
        ): VesperAbrPolicy =
            VesperAbrPolicy(
                mode = VesperAbrMode.Constrained,
                maxBitRate = maxBitRate,
                maxWidth = maxWidth,
                maxHeight = maxHeight,
            )

        fun fixedTrack(trackId: String): VesperAbrPolicy =
            VesperAbrPolicy(
                mode = VesperAbrMode.FixedTrack,
                trackId = trackId,
            )
    }
}
