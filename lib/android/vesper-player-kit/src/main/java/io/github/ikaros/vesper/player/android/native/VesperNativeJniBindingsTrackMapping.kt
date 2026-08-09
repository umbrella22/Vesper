package io.github.ikaros.vesper.player.android

internal fun VesperTrackSelection.toNativePayload(): NativeTrackSelectionPayload =
    NativeTrackSelectionPayload(
        modeOrdinal =
            when (mode) {
                VesperTrackSelectionMode.Auto -> NativeTrackSelectionMode.Auto.ordinal
                VesperTrackSelectionMode.Disabled -> NativeTrackSelectionMode.Disabled.ordinal
                VesperTrackSelectionMode.Track -> NativeTrackSelectionMode.Track.ordinal
            },
        trackId = trackId,
    )

internal fun NativeTrackKind.toPublicKind(): VesperMediaTrackKind =
    when (this) {
        NativeTrackKind.Video -> VesperMediaTrackKind.Video
        NativeTrackKind.Audio -> VesperMediaTrackKind.Audio
        NativeTrackKind.Subtitle -> VesperMediaTrackKind.Subtitle
    }

internal fun NativeTrackSupport.toPublicSupport(): VesperTrackSupport {
    val rawStatusFallback = statusOrdinal.toString()
    val rawReasonFallback = reasonOrdinal.toString()
    val rawSourceFallback = sourceOrdinal.toString()
    val status = NativeTrackSupportStatus.entries.getOrNull(statusOrdinal)
    val reason = NativeTrackSupportReason.entries.getOrNull(reasonOrdinal)
    val source = NativeTrackSupportSource.entries.getOrNull(sourceOrdinal)
    return VesperTrackSupport(
        status =
            when (status) {
                NativeTrackSupportStatus.Supported -> VesperTrackSupportStatus.Supported
                NativeTrackSupportStatus.ExceedsCapabilities -> VesperTrackSupportStatus.ExceedsCapabilities
                NativeTrackSupportStatus.Unsupported -> VesperTrackSupportStatus.Unsupported
                NativeTrackSupportStatus.Unknown, null -> VesperTrackSupportStatus.Unknown
            },
        reason =
            when (reason) {
                NativeTrackSupportReason.None -> VesperTrackSupportReason.None
                NativeTrackSupportReason.FormatExceedsCapabilities -> VesperTrackSupportReason.FormatExceedsCapabilities
                NativeTrackSupportReason.UnsupportedType -> VesperTrackSupportReason.UnsupportedType
                NativeTrackSupportReason.UnsupportedSubtype -> VesperTrackSupportReason.UnsupportedSubtype
                NativeTrackSupportReason.UnsupportedDrm -> VesperTrackSupportReason.UnsupportedDrm
                NativeTrackSupportReason.RouteUnavailable -> VesperTrackSupportReason.RouteUnavailable
                NativeTrackSupportReason.PresentationUnavailable -> VesperTrackSupportReason.PresentationUnavailable
                NativeTrackSupportReason.RuntimeFailure -> VesperTrackSupportReason.RuntimeFailure
                NativeTrackSupportReason.PlatformUnknown, null -> VesperTrackSupportReason.PlatformUnknown
                NativeTrackSupportReason.Unknown -> VesperTrackSupportReason.Unknown
            },
        source =
            when (source) {
                NativeTrackSupportSource.RuntimeTrackCatalog -> VesperTrackSupportSource.RuntimeTrackCatalog
                NativeTrackSupportSource.CapabilityProbe -> VesperTrackSupportSource.CapabilityProbe
                NativeTrackSupportSource.RuntimeFailure -> VesperTrackSupportSource.RuntimeFailure
                NativeTrackSupportSource.Unavailable, null -> VesperTrackSupportSource.Unavailable
                NativeTrackSupportSource.Unknown -> VesperTrackSupportSource.Unknown
            },
        statusRawValue = statusRawValue ?: if (status == null) rawStatusFallback else null,
        reasonRawValue = reasonRawValue ?: if (reason == null) rawReasonFallback else null,
        sourceRawValue = sourceRawValue ?: if (source == null) rawSourceFallback else null,
        playbackPath = playbackPath,
        formatSupportRawValue = formatSupportRawValue,
        diagnostics =
            VesperTrackSupportDiagnostics(
                decoderName = decoderName,
                surfaceKind = surfaceKind,
                hdrType = hdrType,
                secureDecoderRequired = secureDecoderRequired.takeIf { hasSecureDecoderRequired },
                secureOutputRequired = secureOutputRequired.takeIf { hasSecureOutputRequired },
            ),
    )
}

internal fun NativeTrackInfo.toPublicTrack(): VesperMediaTrack? {
    val kind = NativeTrackKind.entries.getOrNull(kindOrdinal)?.toPublicKind() ?: return null
    return VesperMediaTrack(
        id = id,
        kind = kind,
        label = label,
        language = language,
        codec = codec,
        bitRate = bitRate.takeIf { hasBitRate },
        width = width.takeIf { hasWidth },
        height = height.takeIf { hasHeight },
        frameRate = frameRate.takeIf { hasFrameRate },
        channels = channels.takeIf { hasChannels },
        sampleRate = sampleRate.takeIf { hasSampleRate },
        isDefault = isDefault,
        isForced = isForced,
        support = support.toPublicSupport(),
    )
}

internal fun NativeTrackCatalog.toPublicTrackCatalog(): VesperTrackCatalog =
    VesperTrackCatalog(
        tracks = tracks.mapNotNull { it.toPublicTrack() },
        adaptiveVideo = adaptiveVideo,
        adaptiveAudio = adaptiveAudio,
        catalogRevision = catalogRevision.coerceAtLeast(0L),
        playbackPath = playbackPath,
    )

internal fun NativeTrackSelectionPayload.toPublicTrackSelection(): VesperTrackSelection {
    val mode = NativeTrackSelectionMode.entries.getOrNull(modeOrdinal) ?: NativeTrackSelectionMode.Auto
    return when (mode) {
        NativeTrackSelectionMode.Auto ->
            VesperTrackSelection(
                mode = VesperTrackSelectionMode.Auto,
                trackId = trackId,
            )
        NativeTrackSelectionMode.Disabled -> VesperTrackSelection.disabled()
        NativeTrackSelectionMode.Track -> trackId?.let(VesperTrackSelection::track) ?: VesperTrackSelection.auto()
    }
}

internal fun NativeAbrPolicyPayload.toPublicAbrPolicy(): VesperAbrPolicy {
    val mode = NativeAbrMode.entries.getOrNull(modeOrdinal) ?: NativeAbrMode.Auto
    return when (mode) {
        NativeAbrMode.Auto -> VesperAbrPolicy.auto()
        NativeAbrMode.Constrained ->
            VesperAbrPolicy.constrained(
                maxBitRate = maxBitRate.takeIf { hasMaxBitRate },
                maxWidth = maxWidth.takeIf { hasMaxWidth },
                maxHeight = maxHeight.takeIf { hasMaxHeight },
            )
        NativeAbrMode.FixedTrack ->
            trackId?.let(VesperAbrPolicy::fixedTrack) ?: VesperAbrPolicy.auto()
    }
}

internal fun NativeTrackSelectionSnapshotPayload.toPublicTrackSelectionSnapshot():
    VesperTrackSelectionSnapshot =
    VesperTrackSelectionSnapshot(
        video = video.toPublicTrackSelection(),
        audio = audio.toPublicTrackSelection(),
        subtitle = subtitle.toPublicTrackSelection(),
        abrPolicy = abrPolicy.toPublicAbrPolicy(),
    )

internal fun NativeTrackPreferencePolicy.toPublicTrackPreferencePolicy():
    VesperTrackPreferencePolicy =
    VesperTrackPreferencePolicy(
        preferredAudioLanguage = preferredAudioLanguage,
        preferredSubtitleLanguage = preferredSubtitleLanguage,
        selectSubtitlesByDefault = selectSubtitlesByDefault,
        selectUndeterminedSubtitleLanguage = selectUndeterminedSubtitleLanguage,
        audioSelection = audioSelection.toPublicTrackSelection(),
        subtitleSelection = subtitleSelection.toPublicTrackSelection(),
        abrPolicy = abrPolicy.toPublicAbrPolicy(),
    )

internal fun VesperAbrPolicy.toNativePayload(): NativeAbrPolicyPayload =
    NativeAbrPolicyPayload(
        modeOrdinal =
            when (mode) {
                VesperAbrMode.Auto -> NativeAbrMode.Auto.ordinal
                VesperAbrMode.Constrained -> NativeAbrMode.Constrained.ordinal
                VesperAbrMode.FixedTrack -> NativeAbrMode.FixedTrack.ordinal
            },
        trackId = trackId,
        hasMaxBitRate = maxBitRate != null,
        maxBitRate = maxBitRate ?: 0L,
        hasMaxWidth = maxWidth != null,
        maxWidth = maxWidth ?: 0,
        hasMaxHeight = maxHeight != null,
        maxHeight = maxHeight ?: 0,
    )

internal fun VesperTrackPreferencePolicy.toNativePayload(): NativeTrackPreferencePolicy =
    NativeTrackPreferencePolicy(
        preferredAudioLanguage = preferredAudioLanguage,
        preferredSubtitleLanguage = preferredSubtitleLanguage,
        selectSubtitlesByDefault = selectSubtitlesByDefault,
        selectUndeterminedSubtitleLanguage = selectUndeterminedSubtitleLanguage,
        audioSelection = audioSelection.toNativePayload(),
        subtitleSelection = subtitleSelection.toNativePayload(),
        abrPolicy = abrPolicy.toNativePayload(),
    )
