package io.github.ikaros.vesper.player.android

enum class VesperMediaTrackKind {
    Video,
    Audio,
    Subtitle,
}

enum class VesperTrackSupportStatus {
    Supported,
    ExceedsCapabilities,
    Unsupported,
    Unknown,
}

enum class VesperTrackSupportReason {
    None,
    FormatExceedsCapabilities,
    UnsupportedType,
    UnsupportedSubtype,
    UnsupportedDrm,
    RouteUnavailable,
    PresentationUnavailable,
    RuntimeFailure,
    PlatformUnknown,
    Unknown,
}

enum class VesperTrackSupportSource {
    RuntimeTrackCatalog,
    CapabilityProbe,
    RuntimeFailure,
    Unavailable,
    Unknown,
}

data class VesperTrackSupportDiagnostics(
    val decoderName: String? = null,
    val surfaceKind: String? = null,
    val hdrType: String? = null,
    val secureDecoderRequired: Boolean? = null,
    val secureOutputRequired: Boolean? = null,
)

data class VesperTrackSupport(
    val status: VesperTrackSupportStatus = VesperTrackSupportStatus.Unknown,
    val reason: VesperTrackSupportReason = VesperTrackSupportReason.PlatformUnknown,
    val source: VesperTrackSupportSource = VesperTrackSupportSource.Unavailable,
    val statusRawValue: String? = null,
    val reasonRawValue: String? = null,
    val sourceRawValue: String? = null,
    val playbackPath: String? = null,
    val formatSupportRawValue: String? = null,
    val diagnostics: VesperTrackSupportDiagnostics = VesperTrackSupportDiagnostics(),
) {
    val canAttemptExplicitSelection: Boolean
        get() =
            status == VesperTrackSupportStatus.Supported ||
                status == VesperTrackSupportStatus.Unknown
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
    val support: VesperTrackSupport = VesperTrackSupport(),
)

data class VesperTrackCatalog(
    val tracks: List<VesperMediaTrack> = emptyList(),
    val adaptiveVideo: Boolean = false,
    val adaptiveAudio: Boolean = false,
    val catalogRevision: Long = 0L,
    val playbackPath: String? = null,
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
    val confirmedSubtitle: VesperTrackSelection = subtitle,
    val effectiveSubtitleTrackId: String? = null,
    val abrPolicy: VesperAbrPolicy = VesperAbrPolicy.auto(),
)
