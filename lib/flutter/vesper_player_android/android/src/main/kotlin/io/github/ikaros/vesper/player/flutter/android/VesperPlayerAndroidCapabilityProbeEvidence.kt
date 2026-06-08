package io.github.ikaros.vesper.player.flutter.android

import io.github.ikaros.vesper.player.android.VesperPlaybackCapabilityDolbyVisionMode
import io.github.ikaros.vesper.player.android.VesperPlaybackCapabilityHdrKind
import io.github.ikaros.vesper.player.android.VesperPlaybackCapabilityProbeRequest
import io.github.ikaros.vesper.player.android.VesperPlaybackCapabilityProbeResult
import io.github.ikaros.vesper.player.android.VesperRecommendedPlaybackPath

internal fun VesperPlaybackCapabilityProbeRequest.toSourceBoundProbe(
    result: VesperPlaybackCapabilityProbeResult,
): SourceBoundCapabilityProbe =
    SourceBoundCapabilityProbe(
        sourceUri = source?.uri,
        sourceProtocol = source?.protocol?.toWireName(),
        result = result,
    )

internal fun Map<String, Any?>.withAppProbeConvergence(
    recentProbe: SourceBoundCapabilityProbe?,
): Map<String, Any?> {
    if (!isLikelyHdrCapabilityPayload()) {
        return this
    }
    val probe = recentProbe ?: return this
    if (probe.sourceUri == null) {
        return this
    }
    if (!probeMatchesPayloadSource(probe)) {
        return this
    }

    val probeResult = probe.result
    val payloadSourceUri = stringValue("sourceUri")
    return toMutableMap().apply {
        put("appProbeStatus", probeResult.status.toWireName())
        put("appProbeRecommendedPlaybackPath", probeResult.recommendedPlaybackPath.toWireName())
        put("appProbeConfidence", probeResult.confidence.toWireName())
        put("appProbeHdrKind", probeResult.hdrKind.toWireName())
        put("appProbeDolbyVisionMode", probeResult.dolbyVisionMode.toWireName())
        put("appProbeMissingCapabilities", probeResult.missingCapabilities.joinToString(","))
        probe.sourceUri?.let { put("appProbeSourceUri", it) }
        probe.sourceProtocol?.let { put("appProbeSourceProtocol", it) }
        if (payloadSourceUri != null) {
            put("appProbeSourceMatchesRuntime", true)
        } else {
            put("appProbeSourceMatchBasis", "sessionRecentProbe")
        }
        put(
            "appProbeRuntimeRecommendedPathMatches",
            stringValue("recommendedPlaybackPath")
                ?.let { it == probeResult.recommendedPlaybackPath.toWireName() },
        )
        put(
            "appProbeRuntimeHdrKindMatches",
            stringValue("hdrKind")
                ?.let { it == probeResult.hdrKind.toWireName() },
        )
        put(
            "appProbeRuntimeDolbyVisionModeMatches",
            runtimeDolbyVisionMode()
                ?.let { it == probeResult.dolbyVisionMode.toWireName() },
        )
        put(
            "appProbeRuntimeSystemPlayerRecommendationConfirmed",
            probeResult.recommendedPlaybackPath == VesperRecommendedPlaybackPath.SystemPlayer &&
                stringValue("recommendedPlaybackPath") ==
                VesperRecommendedPlaybackPath.SystemPlayer.toWireName(),
        )
        put(
            "appProbeRuntimeHdrKindPresent",
            probeResult.hdrKind != VesperPlaybackCapabilityHdrKind.None &&
                probeResult.hdrKind != VesperPlaybackCapabilityHdrKind.Unknown,
        )
        put(
            "appProbeRuntimeDolbyVisionModePresent",
            probeResult.dolbyVisionMode != VesperPlaybackCapabilityDolbyVisionMode.None,
        )
        putAll(probeResult.appProbeDiagnostics())
    }
}

private fun Map<String, Any?>.isLikelyHdrCapabilityPayload(): Boolean =
    this["likelyHdrCapabilityIssue"] == true ||
        stringValue("likelyHdrCapabilityIssue") == "true" ||
        stringValue("reason") == "hdrNativeFrameUnsupported"

private fun Map<String, Any?>.probeMatchesPayloadSource(
    probe: SourceBoundCapabilityProbe,
): Boolean {
    val payloadSourceUri = stringValue("sourceUri")
    if (payloadSourceUri != null) {
        return payloadSourceUri == probe.sourceUri
    }
    return true
}

private fun VesperPlaybackCapabilityProbeResult.appProbeDiagnostics(): Map<String, Any?> =
    buildMap {
        diagnostics["displayHdrSupported"]?.let {
            put("appProbeDisplayHdrSupported", it)
        }
        diagnostics["displayFrameRateSupported"]?.let {
            put("appProbeDisplayFrameRateSupported", it)
        }
        diagnostics["codecFormatSupported"]?.let {
            put("appProbeCodecFormatSupported", it)
        }
        diagnostics["codecFormatMissingCapability"]?.let {
            put("appProbeCodecFormatMissingCapability", it)
        }
        diagnostics["codecFormatSampleMimeType"]?.let {
            put("appProbeCodecFormatSampleMimeType", it)
        }
        diagnostics["codecFormatCodecs"]?.let {
            put("appProbeCodecFormatCodecs", it)
        }
        diagnostics["codecFormatWidth"]?.let {
            put("appProbeCodecFormatWidth", it)
        }
        diagnostics["codecFormatHeight"]?.let {
            put("appProbeCodecFormatHeight", it)
        }
        diagnostics["codecFormatFrameRate"]?.let {
            put("appProbeCodecFormatFrameRate", it)
        }
    }

private fun Map<String, Any?>.runtimeDolbyVisionMode(): String? =
    stringValue("dolbyVisionMode")
        ?: (this["hdrMetadata"] as? Map<*, *>)?.get("dolbyVisionMode")?.toString()

private fun Map<String, Any?>.stringValue(key: String): String? =
    this[key]?.toString()?.takeIf(String::isNotBlank)
