package io.github.ikaros.vesper.player.android

internal fun VesperNativePlayerBridge.pictureInPictureReadinessForNativeBridge():
    VesperPictureInPictureReadiness {
    val source = currentSource
    val diagnostics =
        linkedMapOf<String, Any?>(
            "backend" to backend.toBackendFamily().wireName,
            "hasSource" to (source != null),
            "hasInitializedSource" to hasInitializedSource,
            "nativeFramePipelineMode" to nativeFramePipelineConfiguration.mode.wireName,
            "nativeFramePipelineActive" to (nativeFramePipelineOpenStatus != null),
            "nativeFramePipelineFallbackReason" to nativeFramePipelineFallbackReason,
            "videoTrackCount" to _trackCatalog.value.videoTracks.size,
        )

    if (source == null) {
        return unavailable(
            VesperPictureInPictureErrorCode.PictureInPictureSystemPlayerUnavailable,
            "No source is selected for Picture in Picture.",
            diagnostics,
        )
    }
    if (!hasInitializedSource) {
        return unavailable(
            VesperPictureInPictureErrorCode.PictureInPictureSystemPlayerUnavailable,
            "System player is not initialized for Picture in Picture.",
            diagnostics,
        )
    }
    if (nativeFramePipelineOpenStatus != null && nativeFramePipelineFallbackReason == null) {
        return unavailable(
            VesperPictureInPictureErrorCode.PictureInPictureNativeFrameRouteCannotHandOff,
            "Native-frame route cannot hand off to the system player.",
            diagnostics,
        )
    }
    if (_trackCatalog.value.videoTracks.isEmpty() &&
        _videoVariantObservation.value?.width == null
    ) {
        return unavailable(
            VesperPictureInPictureErrorCode.PictureInPictureSourceUnsupportedBySystemPlayer,
            "Current source has no system-player video track for Picture in Picture.",
            diagnostics,
        )
    }

    return VesperPictureInPictureReadiness(
        isAvailable = true,
        diagnostics = diagnostics,
    )
}

private fun unavailable(
    code: VesperPictureInPictureErrorCode,
    message: String,
    diagnostics: Map<String, Any?>,
): VesperPictureInPictureReadiness =
    VesperPictureInPictureReadiness(
        isAvailable = false,
        error =
            VesperPictureInPictureError(
                code = code,
                message = message,
                diagnostics = diagnostics,
            ),
        diagnostics = diagnostics,
    )

private val VesperPlayerBackendFamily.wireName: String
    get() = when (this) {
        VesperPlayerBackendFamily.AndroidHostKit -> "androidHostKit"
        VesperPlayerBackendFamily.FakeDemo -> "fakeDemo"
    }

private val VesperNativeFramePipelineMode.wireName: String
    get() = when (this) {
        VesperNativeFramePipelineMode.Disabled -> "disabled"
        VesperNativeFramePipelineMode.DiagnosticsOnly -> "diagnosticsOnly"
        VesperNativeFramePipelineMode.PreferNativeFrame -> "preferNativeFrame"
        VesperNativeFramePipelineMode.RequireNativeFrame -> "requireNativeFrame"
    }
