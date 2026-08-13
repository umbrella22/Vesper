package io.github.umbrella22.vesper.example.androidcomposehost

import io.github.umbrella22.vesper.player.android.VesperPlayerDrmConfiguration
import io.github.umbrella22.vesper.player.android.VesperPlayerSource
import io.github.umbrella22.vesper.player.android.VesperPlayerSourceKind
import io.github.umbrella22.vesper.player.android.VesperPlayerSourceProtocol


internal const val EXAMPLE_DOLBY_ACCEPTANCE_WIDEVINE_LICENSE_URI: String =
    "https://widevine-dash.ezdrm.com/proxy?pX=E8A6EE"

internal val exampleDolbyAcceptanceFpsValues: List<Int> = listOf(24, 30, 50, 120)

internal enum class ExampleDolbyAcceptanceProfile(
    val pathSegment: String,
    val title: String,
    val sampleIdSegment: String,
    val dolbyVisionProfile: Int,
    val profileFamily: String,
    val fallbackTarget: String,
    val transferFunction: String,
) {
    P5(
        pathSegment = "p5",
        title = "P5",
        sampleIdSegment = "P5",
        dolbyVisionProfile = 5,
        profileFamily = "profile5",
        fallbackTarget = "none",
        transferFunction = "SMPTE_ST_2084_PQ",
    ),
    P81(
        pathSegment = "p81",
        title = "P8.1",
        sampleIdSegment = "P81",
        dolbyVisionProfile = 8,
        profileFamily = "profile8.1",
        fallbackTarget = "hdr10",
        transferFunction = "SMPTE_ST_2084_PQ",
    ),
    P84(
        pathSegment = "p84",
        title = "P8.4",
        sampleIdSegment = "P84",
        dolbyVisionProfile = 8,
        profileFamily = "profile8.4",
        fallbackTarget = "hlg",
        transferFunction = "ARIB_STD_B67_HLG",
    ),
}

internal enum class ExampleDolbyAcceptanceDrmKind(
    val title: String,
    val sampleIdSegment: String,
    val metadataValue: String,
) {
    Clear(
        title = "Clear",
        sampleIdSegment = "CLEAR",
        metadataValue = "none",
    ),
    Widevine(
        title = "Widevine",
        sampleIdSegment = "WIDEVINE",
        metadataValue = "widevine",
    ),
    FairPlayPending(
        title = "FairPlay pending",
        sampleIdSegment = "FAIRPLAY-PENDING",
        metadataValue = "fairPlayPending",
    ),
}

internal data class ExampleDolbyAcceptancePreset(
    val id: String,
    val label: String,
    val profile: ExampleDolbyAcceptanceProfile,
    val fps: Int,
    val protocol: VesperPlayerSourceProtocol,
    val drmKind: ExampleDolbyAcceptanceDrmKind,
    val source: VesperPlayerSource,
    val expectedHdrKind: String,
    val manualGate: String,
    val notes: List<String> = emptyList(),
    val enabled: Boolean = true,
) {
    val isDrm: Boolean
        get() = drmKind != ExampleDolbyAcceptanceDrmKind.Clear

    val isPlayable: Boolean
        get() = enabled && drmKind != ExampleDolbyAcceptanceDrmKind.FairPlayPending

    val protocolLabel: String
        get() =
            when (protocol) {
                VesperPlayerSourceProtocol.Dash -> "DASH"
                VesperPlayerSourceProtocol.Hls -> "HLS"
                else -> protocol.name
            }

    fun toHdrEvidencePreset(): ExampleHdrEvidenceSamplePreset =
        ExampleHdrEvidenceSamplePreset(
            sampleId = id,
            label = label,
            expectedAxis = "display",
            sourceMetadata =
                mapOf(
                    "sourceUri" to source.uri,
                    "sourceKind" to "remote",
                    "container" to protocolLabel.lowercase(),
                    "manifestKind" to protocolLabel.lowercase(),
                    "codec" to "dolby-vision",
                    "sampleMimeType" to "video/dolby-vision",
                    "width" to null,
                    "height" to null,
                    "frameRate" to fps.toDouble(),
                    "bitDepth" to 10,
                    "hdrKind" to expectedHdrKind,
                    "colorPrimaries" to "BT.2020",
                    "transferFunction" to profile.transferFunction,
                    "yCbCrMatrix" to "BT.2020_NCL",
                    "drmKind" to drmKind.metadataValue,
                    "manualGate" to manualGate,
                    "controlPurpose" to "dolbyVisionAcceptance",
                    "dolbyVision" to
                        mapOf(
                            "profile" to profile.dolbyVisionProfile,
                            "profileFamily" to profile.profileFamily,
                            "baseLayer" to "hevc-main10",
                            "fallbackTarget" to profile.fallbackTarget,
                            "containerEvidence" to "dolby-browser-test-kit",
                        ),
                    "metadataTool" to
                        mapOf(
                            "name" to "Dolby Browser Test Kit",
                            "version" to "public",
                            "command" to "catalog-url",
                        ),
                    "notes" to
                        listOf(
                            "Dolby Browser Test Kit public URL; media is not bundled.",
                        ) + notes,
                ),
        )
}

internal fun exampleDolbyAcceptanceUrl(
    profile: ExampleDolbyAcceptanceProfile,
    fps: Int,
    protocol: VesperPlayerSourceProtocol,
    drmKind: ExampleDolbyAcceptanceDrmKind,
): String {
    val protocolFile =
        when (protocol) {
            VesperPlayerSourceProtocol.Dash -> "dash.mpd"
            VesperPlayerSourceProtocol.Hls -> "master.m3u8"
            else -> error("DASH or HLS only")
        }
    val pathKind =
        when (drmKind) {
            ExampleDolbyAcceptanceDrmKind.Clear -> "clear"
            ExampleDolbyAcceptanceDrmKind.Widevine -> "cenc"
            ExampleDolbyAcceptanceDrmKind.FairPlayPending -> "cbcs"
        }
    return "https://ott.dolby.com/browser_test_kit/$pathKind/" +
        "${profile.pathSegment}/$fps/$protocolFile"
}

internal fun buildExampleDolbyAcceptanceCatalog(): List<ExampleDolbyAcceptancePreset> =
    buildList {
        ExampleDolbyAcceptanceProfile.values().forEach { profile ->
            exampleDolbyAcceptanceFpsValues.forEach { fps ->
                add(
                    buildExampleDolbyAcceptancePreset(
                        profile = profile,
                        fps = fps,
                        protocol = VesperPlayerSourceProtocol.Dash,
                        drmKind = ExampleDolbyAcceptanceDrmKind.Clear,
                    ),
                )
                add(
                    buildExampleDolbyAcceptancePreset(
                        profile = profile,
                        fps = fps,
                        protocol = VesperPlayerSourceProtocol.Hls,
                        drmKind = ExampleDolbyAcceptanceDrmKind.Clear,
                    ),
                )
                add(
                    buildExampleDolbyAcceptancePreset(
                        profile = profile,
                        fps = fps,
                        protocol = VesperPlayerSourceProtocol.Dash,
                        drmKind = ExampleDolbyAcceptanceDrmKind.Widevine,
                    ),
                )
                add(
                    buildExampleDolbyAcceptancePreset(
                        profile = profile,
                        fps = fps,
                        protocol = VesperPlayerSourceProtocol.Hls,
                        drmKind = ExampleDolbyAcceptanceDrmKind.FairPlayPending,
                        enabled = false,
                    ),
                )
            }
        }
    }

internal val exampleDolbyAcceptanceCatalog: List<ExampleDolbyAcceptancePreset> =
    buildExampleDolbyAcceptanceCatalog()

internal fun exampleDolbyAcceptancePresetById(id: String): ExampleDolbyAcceptancePreset? =
    exampleDolbyAcceptanceCatalog.firstOrNull { preset -> preset.id == id }

internal fun filterDolbyAcceptancePresets(
    presets: List<ExampleDolbyAcceptancePreset>,
    drmKind: ExampleDolbyAcceptanceDrmKind,
    profile: ExampleDolbyAcceptanceProfile?,
    fps: Int?,
): List<ExampleDolbyAcceptancePreset> =
    presets.filter { preset ->
        preset.drmKind == drmKind &&
            (profile == null || preset.profile == profile) &&
            (fps == null || preset.fps == fps)
    }

internal fun exampleDolbyAcceptanceHdrEvidencePresets(): List<ExampleHdrEvidenceSamplePreset> =
    exampleDolbyAcceptanceCatalog
        .filter { preset -> preset.isPlayable }
        .map { preset -> preset.toHdrEvidencePreset() }

private fun buildExampleDolbyAcceptancePreset(
    profile: ExampleDolbyAcceptanceProfile,
    fps: Int,
    protocol: VesperPlayerSourceProtocol,
    drmKind: ExampleDolbyAcceptanceDrmKind,
    enabled: Boolean = true,
): ExampleDolbyAcceptancePreset {
    val protocolSegment =
        when (protocol) {
            VesperPlayerSourceProtocol.Dash -> "DASH"
            VesperPlayerSourceProtocol.Hls -> "HLS"
            else -> error("DASH or HLS only")
        }
    val id =
        "DOLBY-DV-${profile.sampleIdSegment}-$fps-" +
            "$protocolSegment-${drmKind.sampleIdSegment}"
    val label = "${profile.title} ${fps}fps $protocolSegment ${drmKind.title}"
    val uri =
        exampleDolbyAcceptanceUrl(
            profile = profile,
            fps = fps,
            protocol = protocol,
            drmKind = drmKind,
        )
    val source =
        VesperPlayerSource(
            uri = uri,
            label = label,
            kind = VesperPlayerSourceKind.Remote,
            protocol = protocol,
            drmConfiguration =
                if (drmKind == ExampleDolbyAcceptanceDrmKind.Widevine) {
                    VesperPlayerDrmConfiguration(
                        keySystem = "widevine",
                        licenseUri = EXAMPLE_DOLBY_ACCEPTANCE_WIDEVINE_LICENSE_URI,
                    )
                } else {
                    null
                },
        )
    val notes =
        buildList {
            if (drmKind == ExampleDolbyAcceptanceDrmKind.Widevine) {
                add("Widevine DASH direct native route only.")
            }
            if (drmKind == ExampleDolbyAcceptanceDrmKind.FairPlayPending) {
                add("FairPlay certificate URI/base64 is not available yet; preset is disabled.")
            }
            if (fps == 50) {
                add("Dolby 50fps signal covers the 60-ish validation bucket.")
            }
            add("MP4 zip assets remain manual local-file material and are not bundled.")
        }
    return ExampleDolbyAcceptancePreset(
        id = id,
        label = label,
        profile = profile,
        fps = fps,
        protocol = protocol,
        drmKind = drmKind,
        source = source,
        expectedHdrKind = "dolbyVision",
        manualGate = "requiresDolbyVisionDisplay",
        notes = notes,
        enabled = enabled,
    )
}
