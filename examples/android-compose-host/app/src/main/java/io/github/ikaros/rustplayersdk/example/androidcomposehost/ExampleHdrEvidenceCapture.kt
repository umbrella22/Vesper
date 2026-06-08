package io.github.ikaros.vesper.example.androidcomposehost

import android.content.Context
import android.hardware.display.DisplayManager
import android.media.MediaCodecInfo
import android.media.MediaCodecList
import android.os.Build
import android.os.SystemClock
import android.view.Display
import io.github.ikaros.vesper.player.android.PlaybackStateUi
import io.github.ikaros.vesper.player.android.VesperFrameProcessorConfiguration
import io.github.ikaros.vesper.player.android.VesperFrameProcessorMode
import io.github.ikaros.vesper.player.android.VesperHdrChromaticityPoint
import io.github.ikaros.vesper.player.android.VesperNativeFramePipelineConfiguration
import io.github.ikaros.vesper.player.android.VesperPlaybackCapabilityConfidence
import io.github.ikaros.vesper.player.android.VesperPlaybackCapabilityDolbyVisionMode
import io.github.ikaros.vesper.player.android.VesperPlaybackCapabilityHdrKind
import io.github.ikaros.vesper.player.android.VesperPlaybackCapabilityHdrMetadata
import io.github.ikaros.vesper.player.android.VesperPlaybackCapabilityOutputFormat
import io.github.ikaros.vesper.player.android.VesperPlaybackCapabilityProbeRequest
import io.github.ikaros.vesper.player.android.VesperPlaybackCapabilityProbeResult
import io.github.ikaros.vesper.player.android.VesperPlaybackCapabilityProbeStatus
import io.github.ikaros.vesper.player.android.VesperPlaybackCodecFamily
import io.github.ikaros.vesper.player.android.VesperPlayerController
import io.github.ikaros.vesper.player.android.VesperPlayerControllerFactory
import io.github.ikaros.vesper.player.android.VesperPlayerSource
import io.github.ikaros.vesper.player.android.VesperPlayerSourceKind
import io.github.ikaros.vesper.player.android.VesperPlayerSourceProtocol
import io.github.ikaros.vesper.player.android.VesperRecommendedPlaybackPath
import io.github.ikaros.vesper.player.android.VesperRuntimeWarning
import io.github.ikaros.vesper.player.android.VesperSourceNormalizerConfiguration
import java.io.File
import java.net.URL
import java.text.SimpleDateFormat
import java.util.Date
import java.util.Locale
import org.json.JSONArray
import org.json.JSONObject

internal const val ANDROID_HDR_EVIDENCE_NETWORK_CONTROL_URL =
    "https://127.0.0.1:9/vesper-hdr-network-control.mp4"

internal data class ExampleHdrEvidenceSamplePreset(
    val sampleId: String,
    val label: String,
    val expectedAxis: String,
    val sourceMetadata: Map<String, Any?>,
)

internal val exampleHdrEvidenceP0Presets =
    listOf(
        ExampleHdrEvidenceSamplePreset(
            sampleId = "HDR10-HEVC-MAIN10-2160P60-PQ",
            label = "HDR10 4K60 PQ",
            expectedAxis = "display",
            sourceMetadata =
                mapOf(
                    "container" to "mp4",
                    "codec" to "hvc1",
                    "sampleMimeType" to "video/hevc",
                    "width" to 3840,
                    "height" to 2160,
                    "frameRate" to 60.0,
                    "bitDepth" to 10,
                    "hdrKind" to "hdr10",
                    "colorPrimaries" to "BT.2020",
                    "transferFunction" to "SMPTE_ST_2084_PQ",
                    "yCbCrMatrix" to "BT.2020_NCL",
                    "controlPurpose" to "none",
                ),
        ),
        ExampleHdrEvidenceSamplePreset(
            sampleId = "HEVC-SDR-CONTROL",
            label = "HEVC SDR control",
            expectedAxis = "none",
            sourceMetadata =
                mapOf(
                    "container" to "mp4",
                    "codec" to "hvc1",
                    "sampleMimeType" to "video/hevc",
                    "width" to 1920,
                    "height" to 1080,
                    "frameRate" to 30.0,
                    "bitDepth" to 8,
                    "hdrKind" to "none",
                    "colorPrimaries" to "BT.709",
                    "transferFunction" to "BT.709",
                    "yCbCrMatrix" to "BT.709",
                    "controlPurpose" to "hevcSdrFalsePositive",
                ),
        ),
        ExampleHdrEvidenceSamplePreset(
            sampleId = "NETWORK-FAILURE-CONTROL",
            label = "Network failure control",
            expectedAxis = "network",
            sourceMetadata =
                mapOf(
                    "sourceKind" to "progressive",
                    "container" to "mp4",
                    "codec" to "none",
                    "sampleMimeType" to "video/mp4",
                    "hdrKind" to "none",
                    "sourceUri" to ANDROID_HDR_EVIDENCE_NETWORK_CONTROL_URL,
                    "manifestKind" to "none",
                    "controlPurpose" to "networkFailure",
                ),
        ),
    )

internal data class ExampleHdrEvidenceCaptureContext(
    val context: Context,
    val preset: ExampleHdrEvidenceSamplePreset,
    val source: VesperPlayerSource,
    val controller: VesperPlayerController,
    val networkFailureEvidence: ExampleHdrEvidenceNetworkFailureEvidence? = null,
    val sourceNormalizerSetting: ExampleSourceNormalizerSetting,
    val nativeFramePipelineSetting: ExampleNativeFramePipelineSetting,
    val sourceNormalizerPluginLibraryPaths: List<String>,
    val decoderMediaCodecPluginLibraryPaths: List<String>,
    val frameProcessorPluginLibraryPaths: List<String>,
)

internal data class ExampleHdrEvidenceNetworkFailureEvidence(
    val sourceUri: String,
    val observed: Boolean,
    val exceptionClass: String?,
    val message: String?,
    val durationMs: Long,
    val timedOut: Boolean = false,
)

internal fun captureControlledNetworkFailureEvidence(
    sourceUri: String,
    timeoutMs: Int = 30_000,
): ExampleHdrEvidenceNetworkFailureEvidence {
    val startedAt = SystemClock.elapsedRealtime()
    return runCatching {
        val connection = URL(sourceUri).openConnection()
        connection.connectTimeout = timeoutMs
        connection.readTimeout = timeoutMs
        connection.connect()
        connection.getInputStream().use { stream ->
            val buffer = ByteArray(1)
            stream.read(buffer)
        }
    }.fold(
        onSuccess = {
            ExampleHdrEvidenceNetworkFailureEvidence(
                sourceUri = sourceUri,
                observed = false,
                exceptionClass = null,
                message = "controlled network failure URL unexpectedly connected",
                durationMs = SystemClock.elapsedRealtime() - startedAt,
            )
        },
        onFailure = { error ->
            ExampleHdrEvidenceNetworkFailureEvidence(
                sourceUri = sourceUri,
                observed = true,
                exceptionClass = error::class.java.name,
                message = error.message,
                durationMs = SystemClock.elapsedRealtime() - startedAt,
                timedOut = error::class.java.name.contains("Timeout", ignoreCase = true),
            )
        },
    )
}

internal fun captureExampleHdrEvidenceBundle(
    captureContext: ExampleHdrEvidenceCaptureContext,
): File {
    val sourceMetadata =
        exampleHdrEvidenceSourceMetadata(
            preset = captureContext.preset,
            source = captureContext.source,
        )
    val probe =
        VesperPlayerControllerFactory.probePlaybackCapability(
            captureContext.context,
            VesperPlaybackCapabilityProbeRequest(
                source = captureContext.source,
                codec = exampleHdrEvidenceProbeCodec(sourceMetadata),
                width = sourceMetadata["width"] as? Int,
                height = sourceMetadata["height"] as? Int,
                frameRate =
                    when (val frameRate = sourceMetadata["frameRate"]) {
                        is Float -> frameRate
                        is Double -> frameRate.toFloat()
                        is Number -> frameRate.toFloat()
                        else -> null
                    },
                sourceNormalizerConfiguration =
                    VesperSourceNormalizerConfiguration(
                        mode = captureContext.sourceNormalizerSetting.mode,
                        pluginLibraryPaths = captureContext.sourceNormalizerPluginLibraryPaths,
                    ),
                frameProcessorConfiguration =
                    VesperFrameProcessorConfiguration(
                        mode =
                            if (captureContext.frameProcessorPluginLibraryPaths.isEmpty()) {
                                VesperFrameProcessorMode.Disabled
                            } else {
                                VesperFrameProcessorMode.DiagnosticsOnly
                            },
                        pluginLibraryPaths = captureContext.frameProcessorPluginLibraryPaths,
                    ),
                nativeFramePipelineConfiguration =
                    VesperNativeFramePipelineConfiguration(
                        mode = captureContext.nativeFramePipelineSetting.mode,
                        decoderPluginLibraryPaths = captureContext.decoderMediaCodecPluginLibraryPaths,
                        frameProcessorPluginLibraryPaths = captureContext.frameProcessorPluginLibraryPaths,
                        maxInFlightFrames = 2,
                    ),
            ),
        )
    val captureDate = exampleHdrEvidenceCaptureDate()
    val deviceId = "android-example-host"
    val runtimeWarnings = captureContext.controller.drainRuntimeWarnings()
    val bundle =
        AndroidHdrEvidenceBundle(
            sampleId = captureContext.preset.sampleId,
            deviceId = deviceId,
            platform = "android",
            captureDate = captureDate,
            sdkCommit = "local-debug",
            sourceMetadata = sourceMetadata,
            device =
                exampleHdrEvidenceDevice(
                    context = captureContext.context,
                    deviceId = deviceId,
                    captureDate = captureDate,
                    sdkCommit = "local-debug",
                ),
            probe = probe.toEvidenceMap(),
            playbackOutcome =
                exampleHdrEvidencePlaybackOutcome(
                    controller = captureContext.controller,
                    probe = probe,
                    preset = captureContext.preset,
                    networkFailureEvidence = captureContext.networkFailureEvidence,
                ),
            runtimeWarning = runtimeWarnings.firstOrNull { it.domain == "capability" }
                ?: runtimeWarnings.firstOrNull(),
            expectedAxis = captureContext.preset.expectedAxis,
            missingEvidence =
                exampleHdrEvidenceMissingEvidence(
                    preset = captureContext.preset,
                    networkFailureEvidence = captureContext.networkFailureEvidence,
                ),
            networkFailureEvidence = captureContext.networkFailureEvidence,
            platformLog =
                exampleHdrEvidencePlatformLog(
                    source = captureContext.source,
                    controller = captureContext.controller,
                    probe = probe,
                    runtimeWarnings = runtimeWarnings,
                    networkFailureEvidence = captureContext.networkFailureEvidence,
                ),
        )
    return AndroidHdrEvidenceBundleWriter(
        outputRoot = exampleHdrEvidenceOutputRoot(captureContext.context),
    ).write(bundle, overwrite = true)
}

private data class AndroidHdrEvidenceBundle(
    val sampleId: String,
    val deviceId: String,
    val platform: String,
    val captureDate: String,
    val sdkCommit: String,
    val sourceMetadata: Map<String, Any?>,
    val device: Map<String, Any?>,
    val probe: Map<String, Any?>,
    val playbackOutcome: String,
    val runtimeWarning: VesperRuntimeWarning?,
    val expectedAxis: String,
    val missingEvidence: List<String>,
    val networkFailureEvidence: ExampleHdrEvidenceNetworkFailureEvidence?,
    val platformLog: String,
)

private class AndroidHdrEvidenceBundleWriter(
    private val outputRoot: File,
) {
    fun write(
        bundle: AndroidHdrEvidenceBundle,
        overwrite: Boolean,
    ): File {
        val directory =
            outputRoot
                .resolve(bundle.captureDate)
                .resolve(bundle.deviceId)
                .resolve(bundle.sampleId)
        if (directory.exists() && overwrite) {
            directory.deleteRecursively()
        }
        check(directory.mkdirs() || directory.isDirectory) {
            "failed to create HDR evidence directory: ${directory.absolutePath}"
        }
        writeJson(bundle.device, directory.resolve("device.json"))
        writeJson(exampleHdrEvidenceSourceMetadataJson(bundle), directory.resolve("source-metadata.json"))
        writeJson(
            exampleHdrEvidenceProbeJson(bundle, "vesper-hdr-dv-probe-host-v1"),
            directory.resolve("probe-host.json"),
        )
        writeJson(exampleHdrEvidenceFlutterProbeJson(bundle), directory.resolve("probe-flutter.json"))
        writeJson(exampleHdrEvidenceRuntimeWarningJson(bundle), directory.resolve("runtime-warning.json"))
        writeJson(exampleHdrEvidenceRuntimeErrorJson(bundle), directory.resolve("runtime-error.json"))
        writeJson(exampleHdrEvidenceTypedEvidenceJson(bundle), directory.resolve("typed-evidence.json"))
        directory.resolve("platform-log.txt").writeText(bundle.platformLog)
        directory.resolve("notes.md").writeText(exampleHdrEvidenceNotes(bundle, directory.absolutePath))
        return directory
    }

    private fun writeJson(
        value: Map<String, Any?>,
        file: File,
    ) {
        file.writeText((exampleHdrEvidenceJsonValue(value) as JSONObject).toString(2) + "\n")
    }
}

private fun exampleHdrEvidenceOutputRoot(context: Context): File {
    val root =
        context.getExternalFilesDir(null)?.resolve("hdr-dv-evidence")
            ?: context.filesDir.resolve("hdr-dv-evidence")
    check(root.mkdirs() || root.isDirectory) {
        "failed to create HDR evidence root: ${root.absolutePath}"
    }
    return root
}

private fun exampleHdrEvidenceDevice(
    context: Context,
    deviceId: String,
    captureDate: String,
    sdkCommit: String,
): Map<String, Any?> {
    val display =
        context
            .getSystemService(DisplayManager::class.java)
            ?.getDisplay(Display.DEFAULT_DISPLAY)
    return mapOf(
        "schema" to "vesper-hdr-dv-device-v1",
        "deviceId" to deviceId,
        "platform" to "android",
        "captureDate" to captureDate,
        "sdkCommit" to sdkCommit,
        "hostApp" to
            mapOf(
                "name" to "android-compose-host",
                "version" to "debug",
                "displayPath" to "ExoPlayer",
            ),
        "android" to
            mapOf(
                "manufacturer" to Build.MANUFACTURER,
                "model" to Build.MODEL,
                "apiLevel" to Build.VERSION.SDK_INT,
                "buildFingerprint" to Build.FINGERPRINT,
                "displayHdrTypes" to display?.hdrTypeNames().orEmpty(),
                "displayRefreshRate" to (display?.refreshRate?.toDouble() ?: 0.0),
                "displayModes" to
                    display
                        ?.supportedModes
                        ?.map { mode ->
                            "${mode.physicalWidth}x${mode.physicalHeight}@${"%.2f".format(mode.refreshRate)}"
                        }.orEmpty(),
                "media3Version" to "1.9.3",
                "decoderCandidates" to
                    mapOf(
                        "hevc" to decoderCandidates("video/hevc"),
                        "dolbyVision" to decoderCandidates("video/dolby-vision"),
                    ),
            ),
        "ios" to
            mapOf(
                "model" to "TBD",
                "iosVersion" to "TBD",
                "avPlayerEligibleForHdrPlayback" to null,
                "displayGamut" to "TBD",
                "nativeDisplaySize" to mapOf("width" to null, "height" to null),
                "maximumFramesPerSecond" to null,
            ),
        "knownCaveats" to
            listOf(
                "Captured through android-compose-host debug helper; native-host probe-flutter.json mirrors the host probe and is not Flutter parity evidence.",
            ),
    )
}

private fun Display.hdrTypeNames(): List<String> =
    hdrCapabilities.supportedHdrTypesCompat().map { type ->
        when (type) {
            Display.HdrCapabilities.HDR_TYPE_DOLBY_VISION -> "DOLBY_VISION"
            Display.HdrCapabilities.HDR_TYPE_HDR10 -> "HDR10"
            Display.HdrCapabilities.HDR_TYPE_HLG -> "HLG"
            Display.HdrCapabilities.HDR_TYPE_HDR10_PLUS -> "HDR10_PLUS"
            else -> "UNKNOWN_$type"
        }
    }

@Suppress("DEPRECATION")
private fun Display.HdrCapabilities.supportedHdrTypesCompat(): IntArray = supportedHdrTypes

private fun decoderCandidates(mimeType: String): List<String> =
    runCatching {
        MediaCodecList(MediaCodecList.REGULAR_CODECS).codecInfos
            .filter { codecInfo ->
                !codecInfo.isEncoder &&
                    codecInfo.supportedTypes.any { type ->
                        type.equals(mimeType, ignoreCase = true)
                    } &&
                    codecInfo.isHardwareAcceleratedCompat()
            }
            .map(MediaCodecInfo::getName)
            .distinct()
    }.getOrDefault(emptyList())

private fun MediaCodecInfo.isHardwareAcceleratedCompat(): Boolean =
    if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
        isHardwareAccelerated
    } else {
        val lowerName = name.lowercase()
        !lowerName.startsWith("omx.google.") &&
            !lowerName.startsWith("c2.android.") &&
            !lowerName.contains("software")
    }

private fun exampleHdrEvidenceSourceMetadata(
    preset: ExampleHdrEvidenceSamplePreset,
    source: VesperPlayerSource,
): Map<String, Any?> =
    preset.sourceMetadata.toMutableMap().apply {
        this["sourceUri"] = source.uri
        this["sourceKind"] = source.evidenceSourceKind()
        this["manifestKind"] = source.evidenceManifestKind()
    }

private fun exampleHdrEvidenceSourceMetadataJson(bundle: AndroidHdrEvidenceBundle): Map<String, Any?> =
    exampleMergeMaps(
        mapOf(
            "schema" to "vesper-hdr-dv-source-metadata-v1",
            "sampleId" to bundle.sampleId,
            "sourceKind" to "TBD",
            "sourceUri" to "TBD",
            "container" to "TBD",
            "manifestKind" to "none",
            "codec" to "TBD",
            "sampleMimeType" to "TBD",
            "width" to null,
            "height" to null,
            "frameRate" to null,
            "bitDepth" to null,
            "hdrKind" to "none",
            "colorPrimaries" to "TBD",
            "transferFunction" to "TBD",
            "yCbCrMatrix" to "TBD",
            "maxContentLightLevelNits" to null,
            "maxFrameAverageLightLevelNits" to null,
            "masteringDisplay" to
                mapOf(
                    "present" to null,
                    "primary0" to null,
                    "primary1" to null,
                    "primary2" to null,
                    "whitePoint" to null,
                    "maxLuminanceNits" to null,
                    "minLuminanceNits" to null,
                ),
            "dolbyVision" to
                mapOf(
                    "codec" to bundle.probe.pathValue("hdrMetadata", "dolbyVisionCodec"),
                    "profile" to bundle.probe.pathValue("hdrMetadata", "dolbyVisionProfile"),
                    "level" to bundle.probe.pathValue("hdrMetadata", "dolbyVisionLevel"),
                    "compatibility" to bundle.probe.pathValue("hdrMetadata", "dolbyVisionCompatibility"),
                    "profileFamily" to bundle.probe.pathValue("hdrMetadata", "dolbyVisionProfileFamily"),
                    "baseLayer" to bundle.probe.pathValue("hdrMetadata", "dolbyVisionBaseLayer"),
                    "fallbackTarget" to bundle.probe.pathValue("hdrMetadata", "dolbyVisionFallbackTarget"),
                    "baseLayerEvidence" to bundle.probe.pathValue("hdrMetadata", "dolbyVisionBaseLayerEvidence"),
                    "baseLayerTransferFunction" to
                        bundle.probe.pathValue(
                            "hdrMetadata",
                            "dolbyVisionBaseLayerTransferFunction",
                        ),
                    "containerEvidence" to null,
                ),
            "controlPurpose" to "none",
            "metadataTool" to
                mapOf(
                    "name" to "android-compose-host-preset",
                    "version" to "debug",
                    "command" to "example native HDR evidence capture",
                ),
            "notes" to emptyList<String>(),
        ),
        bundle.sourceMetadata,
    )

private fun exampleHdrEvidenceProbeJson(
    bundle: AndroidHdrEvidenceBundle,
    schema: String,
): Map<String, Any?> =
    mapOf(
        "schema" to schema,
        "sampleId" to bundle.sampleId,
        "deviceId" to bundle.deviceId,
        "platform" to bundle.platform,
        "captureDate" to bundle.captureDate,
        "request" to
            mapOf(
                "codec" to bundle.sourceMetadata["codec"],
                "width" to bundle.sourceMetadata["width"],
                "height" to bundle.sourceMetadata["height"],
                "frameRate" to bundle.sourceMetadata["frameRate"],
                "hdrKind" to bundle.sourceMetadata["hdrKind"],
                "sourceKind" to bundle.sourceMetadata["sourceKind"],
                "manifestKind" to bundle.sourceMetadata["manifestKind"],
            ),
        "result" to exampleHdrEvidenceProbeResult(bundle.probe),
        "diagnosticGroups" to exampleHdrEvidenceProbeDiagnosticGroups(bundle.probe),
        "capturedVia" to "android-compose-host",
    )

private fun exampleHdrEvidenceFlutterProbeJson(bundle: AndroidHdrEvidenceBundle): Map<String, Any?> =
    exampleHdrEvidenceProbeJson(bundle, "vesper-hdr-dv-probe-flutter-v1")
        .toMutableMap()
        .apply {
            this["capturedVia"] = "android-compose-host-native-mirror"
            this["matchesHostProbe"] = true
        }

private fun exampleHdrEvidenceProbeResult(probe: Map<String, Any?>): Map<String, Any?> =
    mapOf(
        "status" to (probe["status"] ?: "unknown"),
        "recommendedPlaybackPath" to (probe["recommendedPlaybackPath"] ?: "systemPlayer"),
        "confidence" to (probe["confidence"] ?: "codecOnly"),
        "hdrKind" to (probe["hdrKind"] ?: "none"),
        "missingCapabilities" to (probe["missingCapabilities"] ?: emptyList<String>()),
        "hdrMetadata" to ((probe["hdrMetadata"] as? Map<*, *>) ?: emptyMap<String, Any?>()),
    )

private fun exampleHdrEvidenceProbeDiagnosticGroups(probe: Map<String, Any?>): Map<String, Any?> {
    val diagnostics =
        (probe["diagnostics"] as? Map<*, *>)
            ?.mapNotNull { (key, value) ->
                val name = key as? String ?: return@mapNotNull null
                name to value.toString()
            }?.toMap()
            .orEmpty()
    return mapOf(
        "display" to
            diagnostics.filter {
                it.key.startsWith("display") ||
                    it.key.startsWith("requestedFrameRate")
            },
        "codecFormat" to diagnostics.filter { it.key.startsWith("codecFormat") },
        "asset" to diagnostics.filter { it.key.startsWith("asset") },
        "dolbyVision" to diagnostics.filter { it.key.startsWith("dolbyVision") },
        "other" to
            diagnostics.filter { entry ->
                !entry.key.startsWith("display") &&
                    !entry.key.startsWith("requestedFrameRate") &&
                    !entry.key.startsWith("codecFormat") &&
                    !entry.key.startsWith("asset") &&
                    !entry.key.startsWith("dolbyVision")
            },
    )
}

private fun exampleHdrEvidenceRuntimeWarningJson(bundle: AndroidHdrEvidenceBundle): Map<String, Any?> =
    mapOf(
        "schema" to "vesper-hdr-dv-runtime-warning-v1",
        "sampleId" to bundle.sampleId,
        "deviceId" to bundle.deviceId,
        "platform" to bundle.platform,
        "captureDate" to bundle.captureDate,
        "observed" to (bundle.runtimeWarning != null),
        "warning" to
            (
                bundle.runtimeWarning
                    ?.payload
                    ?.let { exampleCapabilityEvidence(present = true, probe = bundle.probe, payload = it) }
                    ?: exampleEmptyCapabilityEvidence(bundle.probe)
            ),
    )

private fun exampleHdrEvidenceRuntimeErrorJson(bundle: AndroidHdrEvidenceBundle): Map<String, Any?> =
    mapOf(
        "schema" to "vesper-hdr-dv-runtime-error-v1",
        "sampleId" to bundle.sampleId,
        "deviceId" to bundle.deviceId,
        "platform" to bundle.platform,
        "captureDate" to bundle.captureDate,
        "playbackOutcome" to bundle.playbackOutcome,
        "observed" to (bundle.networkFailureEvidence?.observed == true),
        "error" to
            mapOf(
                "code" to bundle.networkFailureEvidence?.let { "backendFailure" },
                "category" to bundle.networkFailureEvidence?.let { "network" },
                "message" to bundle.networkFailureEvidence?.message,
                "retriable" to bundle.networkFailureEvidence?.let { true },
                "details" to
                    (
                        bundle.networkFailureEvidence?.let { evidence ->
                            mapOf(
                                "sourceUri" to evidence.sourceUri,
                                "exceptionClass" to evidence.exceptionClass,
                                "durationMs" to evidence.durationMs,
                                "timedOut" to evidence.timedOut,
                                "androidRuntimeEvidenceSource" to "android-compose-host-controlled-url",
                            )
                        } ?: emptyMap<String, Any?>()
                    ),
            ),
        "android" to
            mapOf(
                "errorCodeName" to bundle.networkFailureEvidence?.let { "CONTROLLED_NETWORK_FAILURE" },
                "rendererName" to null,
                "rendererFormat" to null,
                "codecName" to null,
                "capabilityFailureCause" to null,
                "missingCapabilities" to null,
                "sessionProbe" to null,
                "networkEvidenceSource" to bundle.networkFailureEvidence?.let {
                    "android-compose-host-controlled-url"
                },
                "networkExceptionClass" to bundle.networkFailureEvidence?.exceptionClass,
                "networkFailureMessage" to bundle.networkFailureEvidence?.message,
                "networkFailureDurationMs" to bundle.networkFailureEvidence?.durationMs,
            ),
        "ios" to emptyMap<String, Any?>(),
        "expectedAxis" to bundle.expectedAxis,
        "axisSupportedByEvidence" to
            when {
                bundle.expectedAxis == "none" -> true
                bundle.expectedAxis == "network" && bundle.networkFailureEvidence?.observed == true -> true
                else -> null
            },
        "missingEvidence" to bundle.missingEvidence,
        "matchesHostEvidence" to true,
        "evidenceMismatches" to emptyList<String>(),
    )

private fun exampleHdrEvidenceTypedEvidenceJson(bundle: AndroidHdrEvidenceBundle): Map<String, Any?> {
    val warningPayload = bundle.runtimeWarning?.payload
    return mapOf(
        "schema" to "vesper-hdr-dv-typed-evidence-v1",
        "sampleId" to bundle.sampleId,
        "deviceId" to bundle.deviceId,
        "platform" to bundle.platform,
        "captureDate" to bundle.captureDate,
        "flutter" to
            mapOf(
                "vesperCapabilityWarning" to
                    if (warningPayload != null) {
                        exampleCapabilityEvidence(
                            present = true,
                            probe = bundle.probe,
                            payload = warningPayload,
                        )
                    } else {
                        exampleEmptyCapabilityEvidence(bundle.probe)
                    },
                "vesperHdrCapabilityEvidence" to exampleEmptyCapabilityEvidence(bundle.probe),
            ),
        "matchesHostEvidence" to true,
        "probeMismatches" to emptyList<String>(),
        "evidenceMismatches" to emptyList<String>(),
    )
}

private fun exampleCapabilityEvidence(
    present: Boolean,
    probe: Map<String, Any?>,
    payload: Map<String, Any?>,
): Map<String, Any?> =
    exampleEmptyCapabilityEvidence(probe)
        .toMutableMap()
        .apply {
            this["present"] = present
            putAll(payload)
            if (this["recommendedPlaybackPath"] == null) {
                this["recommendedPlaybackPath"] = probe["recommendedPlaybackPath"]
            }
            if (this["hdrKind"] == null) {
                this["hdrKind"] = probe["hdrKind"]
            }
            if (this["confidence"] == null) {
                this["confidence"] = probe["confidence"]
            }
            if (this["hdrMetadata"] == null) {
                this["hdrMetadata"] = probe["hdrMetadata"] ?: emptyMap<String, Any?>()
            }
        }

private fun exampleEmptyCapabilityEvidence(probe: Map<String, Any?>): Map<String, Any?> =
    mapOf(
        "present" to false,
        "reason" to null,
        "recommendedPlaybackPath" to null,
        "hdrKind" to (probe["hdrKind"] ?: "none"),
        "likelyHdrCapabilityIssue" to false,
        "confidence" to (probe["confidence"] ?: "codecOnly"),
        "errorCode" to null,
        "capabilityFailureCause" to null,
        "capabilityFailureAxis" to null,
        "hdrMetadata" to (probe["hdrMetadata"] ?: emptyMap<String, Any?>()),
        "diagnostics" to (probe["diagnostics"] ?: emptyMap<String, Any?>()),
        "message" to null,
    )

private fun exampleHdrEvidenceNotes(
    bundle: AndroidHdrEvidenceBundle,
    bundlePath: String,
): String =
    """
    # HDR / Dolby Vision Evidence Notes

    - Bundle path: `$bundlePath`
    - Sample ID: `${bundle.sampleId}`
    - Device ID: `${bundle.deviceId}`
    - Platform: `${bundle.platform}`
    - Capture date: `${bundle.captureDate}`
    - Host app: `android-compose-host`
    - Playback outcome: `${bundle.playbackOutcome}`
    - Expected axis: `${bundle.expectedAxis}`

    ## Evidence Summary

    - Native host probe captured in `probe-host.json`.
    - `probe-flutter.json` mirrors the native host probe so the existing validator can compare route policy; it is not Flutter parity evidence.
    - HDR/DV policy remains systemPlayer-only for this capture path.
    - Source metadata uses the selected P0 preset plus the active source URI.
    - Missing evidence: `${bundle.missingEvidence.ifEmpty { listOf("none") }.joinToString("; ")}`
    """.trimIndent() + "\n"

private fun exampleHdrEvidenceProbeCodec(metadata: Map<String, Any?>): String? {
    val codec = metadata["codec"]?.toString()?.trim()?.takeIf { it.isNotEmpty() && it != "none" }
        ?: return null
    val hdrKind = metadata["hdrKind"]?.toString()?.trim()
    return if (hdrKind.isNullOrEmpty() || hdrKind == "none" || hdrKind == "unknown") {
        codec
    } else {
        "$codec,$hdrKind"
    }
}

private fun exampleHdrEvidencePlaybackOutcome(
    controller: VesperPlayerController,
    probe: VesperPlaybackCapabilityProbeResult,
    preset: ExampleHdrEvidenceSamplePreset,
    networkFailureEvidence: ExampleHdrEvidenceNetworkFailureEvidence?,
): String =
    when {
        preset.sampleId == "NETWORK-FAILURE-CONTROL" && networkFailureEvidence?.observed == true ->
            "failure"
        preset.sampleId == "NETWORK-FAILURE-CONTROL" -> "notRun"
        probe.recommendedPlaybackPath == VesperRecommendedPlaybackPath.SystemPlayer &&
            probe.missingCapabilities.contains("hdrProgrammableProcessingNotSupported") -> "fallback"
        controller.uiState.value.playbackState == PlaybackStateUi.Playing -> "success"
        else -> "success"
    }

private fun exampleHdrEvidenceMissingEvidence(
    preset: ExampleHdrEvidenceSamplePreset,
    networkFailureEvidence: ExampleHdrEvidenceNetworkFailureEvidence?,
): List<String> =
    if (preset.sampleId == "NETWORK-FAILURE-CONTROL" && networkFailureEvidence?.observed != true) {
        listOf("controlled network failure must be observed in runtime-error.json")
    } else {
        emptyList()
    }

private fun exampleHdrEvidencePlatformLog(
    source: VesperPlayerSource,
    controller: VesperPlayerController,
    probe: VesperPlaybackCapabilityProbeResult,
    runtimeWarnings: List<VesperRuntimeWarning>,
    networkFailureEvidence: ExampleHdrEvidenceNetworkFailureEvidence?,
): String =
    """
    android-compose-host HDR evidence capture
    source=${source.uri}
    playbackState=${controller.uiState.value.playbackState}
    route=${probe.recommendedPlaybackPath.toWireName()}
    status=${probe.status.toWireName()}
    hdrKind=${probe.hdrKind.toWireName()}
    missingCapabilities=${probe.missingCapabilities.joinToString(",")}
    runtimeWarnings=${runtimeWarnings.size}
    networkFailureObserved=${networkFailureEvidence?.observed}
    networkFailureException=${networkFailureEvidence?.exceptionClass}
    networkFailureMessage=${networkFailureEvidence?.message}
    networkFailureDurationMs=${networkFailureEvidence?.durationMs}
    pluginDiagnostics=${controller.pluginDiagnostics}
    """.trimIndent() + "\n"

private fun exampleHdrEvidenceCaptureDate(): String =
    SimpleDateFormat("yyyy-MM-dd", Locale.US).format(Date())

private fun VesperPlayerSource.evidenceSourceKind(): String =
    when (kind) {
        VesperPlayerSourceKind.Local -> "local"
        VesperPlayerSourceKind.Remote ->
            when (protocol) {
                VesperPlayerSourceProtocol.Hls -> "hls"
                VesperPlayerSourceProtocol.Dash -> "dash"
                VesperPlayerSourceProtocol.Progressive -> "progressive"
                VesperPlayerSourceProtocol.Content -> "content"
                VesperPlayerSourceProtocol.File -> "file"
                VesperPlayerSourceProtocol.Unknown -> "remote"
            }
    }

private fun VesperPlayerSource.evidenceManifestKind(): String =
    when (protocol) {
        VesperPlayerSourceProtocol.Hls -> "hls"
        VesperPlayerSourceProtocol.Dash -> "dash"
        else -> "none"
    }

private fun VesperPlaybackCapabilityProbeResult.toEvidenceMap(): Map<String, Any?> =
    mapOf(
        "status" to status.toWireName(),
        "codecFamily" to codecFamily.toWireName(),
        "systemPlaybackSupported" to systemPlaybackSupported,
        "hardwareDecodeSupported" to hardwareDecodeSupported,
        "sdkManagedNativeFrameSupported" to sdkManagedNativeFrameSupported,
        "recommendedPlaybackPath" to recommendedPlaybackPath.toWireName(),
        "outputFormat" to outputFormat.toWireName(),
        "hdrKind" to hdrKind.toWireName(),
        "dolbyVisionMode" to dolbyVisionMode.toWireName(),
        "confidence" to confidence.toWireName(),
        "hdrMetadata" to hdrMetadataMap(),
        "missingCapabilities" to missingCapabilities,
        "diagnostics" to diagnostics,
    )

private fun VesperPlaybackCapabilityProbeResult.hdrMetadataMap(): Map<String, Any?> {
    val values = hdrMetadata?.toEvidenceMap()?.toMutableMap() ?: linkedMapOf()
    if (!values.containsKey("hdrKind") &&
        hdrKind != VesperPlaybackCapabilityHdrKind.None &&
        hdrKind != VesperPlaybackCapabilityHdrKind.Unknown
    ) {
        values["hdrKind"] = hdrKind.toWireName()
    }
    if (!values.containsKey("dolbyVisionMode") &&
        dolbyVisionMode != VesperPlaybackCapabilityDolbyVisionMode.None
    ) {
        values["dolbyVisionMode"] = dolbyVisionMode.toWireName()
    }
    diagnostics["runtimeFormatHdrMetadataProbe"]?.let { values["probe"] = it }
    diagnostics["assetVideoHdrMetadataProbe"]?.let { values.putIfAbsent("probe", it) }
    diagnostics["assetProbe"]?.let { values.putIfAbsent("probe", it) }
    diagnostics["assetVideoCodec"]?.let { values["codec"] = it }
    diagnostics["runtimeFormatCodecs"]?.let { values.putIfAbsent("codec", it) }
    diagnostics["runtimeFormatSampleMimeType"]?.let { values["sampleMimeType"] = it }
    diagnostics["assetVideoColorPrimaries"]?.let { values["colorPrimaries"] = it }
    diagnostics["runtimeFormatColorSpace"]?.let { values["colorSpace"] = it }
    diagnostics["runtimeFormatColorRange"]?.let { values["colorRange"] = it }
    diagnostics["assetVideoTransferFunction"]?.let { values["transferFunction"] = it }
    diagnostics["runtimeFormatColorTransfer"]?.let { values.putIfAbsent("transferFunction", it) }
    diagnostics["assetVideoYCbCrMatrix"]?.let { values["yCbCrMatrix"] = it }
    diagnostics["assetVideoAlternativeTransferCharacteristics"]?.let {
        values["alternativeTransferCharacteristics"] = it
    }
    diagnostics["runtimeFormatLumaBitDepth"]?.toIntOrNull()?.let { values["lumaBitDepth"] = it }
    diagnostics["runtimeFormatChromaBitDepth"]?.toIntOrNull()?.let { values["chromaBitDepth"] = it }
    diagnostics["runtimeFormatHdrStaticInfoPresent"]?.toBooleanStrictOrNull()?.let {
        values["hdrStaticInfoPresent"] = it
    }
    diagnostics["runtimeFormatHdrStaticInfoByteLength"]?.toIntOrNull()?.let {
        values["hdrStaticInfoByteLength"] = it
    }
    diagnostics["runtimeFormatHdrStaticInfoParseError"]?.let {
        values["hdrStaticInfoParseError"] = it
    }
    diagnostics["assetVideoMaxContentLightLevelNits"]?.toIntOrNull()?.let {
        values["maxContentLightLevelNits"] = it
    }
    diagnostics["runtimeFormatMaxContentLightLevelNits"]?.toIntOrNull()?.let {
        values.putIfAbsent("maxContentLightLevelNits", it)
    }
    diagnostics["assetVideoMaxFrameAverageLightLevelNits"]?.toIntOrNull()?.let {
        values["maxFrameAverageLightLevelNits"] = it
    }
    diagnostics["runtimeFormatMaxFrameAverageLightLevelNits"]?.toIntOrNull()?.let {
        values.putIfAbsent("maxFrameAverageLightLevelNits", it)
    }
    diagnostics["assetVideoMasteringDisplayColorVolumePresent"]?.toBooleanStrictOrNull()?.let {
        values["masteringDisplayColorVolumePresent"] = it
    }
    diagnostics["assetVideoMasteringDisplayColorVolumeByteLength"]?.toIntOrNull()?.let {
        values["masteringDisplayColorVolumeByteLength"] = it
    }
    diagnostics["assetVideoMasteringDisplayColorVolumeParseError"]?.let {
        values["masteringDisplayColorVolumeParseError"] = it
    }
    diagnostics["dolbyVisionCodec"]?.let { values["dolbyVisionCodec"] = it }
    diagnostics["dolbyVisionProfile"]?.toIntOrNull()?.let { values["dolbyVisionProfile"] = it }
    diagnostics["dolbyVisionLevel"]?.toIntOrNull()?.let { values["dolbyVisionLevel"] = it }
    diagnostics["dolbyVisionCompatibility"]?.let { values["dolbyVisionCompatibility"] = it }
    diagnostics["dolbyVisionProfileFamily"]?.let { values["dolbyVisionProfileFamily"] = it }
    diagnostics["dolbyVisionBaseLayer"]?.let { values["dolbyVisionBaseLayer"] = it }
    diagnostics["dolbyVisionFallbackTarget"]?.let { values["dolbyVisionFallbackTarget"] = it }
    diagnostics["dolbyVisionBaseLayerEvidence"]?.let { values["dolbyVisionBaseLayerEvidence"] = it }
    diagnostics["dolbyVisionBaseLayerTransferFunction"]?.let {
        values["dolbyVisionBaseLayerTransferFunction"] = it
    }
    return values
}

private fun VesperPlaybackCapabilityHdrMetadata.toEvidenceMap(): Map<String, Any?> =
    linkedMapOf<String, Any?>().also { values ->
        hdrKind?.let { values["hdrKind"] = it.toWireName() }
        dolbyVisionMode?.let { values["dolbyVisionMode"] = it.toWireName() }
        probe?.let { values["probe"] = it }
        codec?.let { values["codec"] = it }
        sampleMimeType?.let { values["sampleMimeType"] = it }
        colorPrimaries?.let { values["colorPrimaries"] = it }
        colorSpace?.let { values["colorSpace"] = it }
        colorRange?.let { values["colorRange"] = it }
        transferFunction?.let { values["transferFunction"] = it }
        yCbCrMatrix?.let { values["yCbCrMatrix"] = it }
        alternativeTransferCharacteristics?.let { values["alternativeTransferCharacteristics"] = it }
        lumaBitDepth?.let { values["lumaBitDepth"] = it }
        chromaBitDepth?.let { values["chromaBitDepth"] = it }
        hdrStaticInfoPresent?.let { values["hdrStaticInfoPresent"] = it }
        hdrStaticInfoByteLength?.let { values["hdrStaticInfoByteLength"] = it }
        hdrStaticInfoParseError?.let { values["hdrStaticInfoParseError"] = it }
        maxContentLightLevelNits?.let { values["maxContentLightLevelNits"] = it }
        maxFrameAverageLightLevelNits?.let { values["maxFrameAverageLightLevelNits"] = it }
        masteringDisplayColorVolumePresent?.let {
            values["masteringDisplayColorVolumePresent"] = it
        }
        masteringDisplayColorVolumeByteLength?.let {
            values["masteringDisplayColorVolumeByteLength"] = it
        }
        masteringDisplayColorVolumeParseError?.let {
            values["masteringDisplayColorVolumeParseError"] = it
        }
        masteringDisplayPrimary0?.let { values["masteringDisplayPrimary0"] = it.toEvidenceMap() }
        masteringDisplayPrimary1?.let { values["masteringDisplayPrimary1"] = it.toEvidenceMap() }
        masteringDisplayPrimary2?.let { values["masteringDisplayPrimary2"] = it.toEvidenceMap() }
        masteringDisplayWhitePoint?.let { values["masteringDisplayWhitePoint"] = it.toEvidenceMap() }
        masteringDisplayMaxLuminanceNits?.let { values["masteringDisplayMaxLuminanceNits"] = it }
        masteringDisplayMinLuminanceNits?.let { values["masteringDisplayMinLuminanceNits"] = it }
        dolbyVisionCodec?.let { values["dolbyVisionCodec"] = it }
        dolbyVisionProfile?.let { values["dolbyVisionProfile"] = it }
        dolbyVisionLevel?.let { values["dolbyVisionLevel"] = it }
        dolbyVisionCompatibility?.let { values["dolbyVisionCompatibility"] = it }
        dolbyVisionProfileFamily?.let { values["dolbyVisionProfileFamily"] = it }
        dolbyVisionBaseLayer?.let { values["dolbyVisionBaseLayer"] = it }
        dolbyVisionFallbackTarget?.let { values["dolbyVisionFallbackTarget"] = it }
        dolbyVisionBaseLayerEvidence?.let { values["dolbyVisionBaseLayerEvidence"] = it }
        dolbyVisionBaseLayerTransferFunction?.let {
            values["dolbyVisionBaseLayerTransferFunction"] = it
        }
    }

private fun VesperHdrChromaticityPoint.toEvidenceMap(): Map<String, Double> =
    mapOf("x" to x, "y" to y)

private fun VesperPlaybackCapabilityProbeStatus.toWireName(): String =
    when (this) {
        VesperPlaybackCapabilityProbeStatus.Supported -> "supported"
        VesperPlaybackCapabilityProbeStatus.FallbackRequired -> "fallbackRequired"
        VesperPlaybackCapabilityProbeStatus.Unsupported -> "unsupported"
        VesperPlaybackCapabilityProbeStatus.Unknown -> "unknown"
    }

private fun VesperPlaybackCodecFamily.toWireName(): String =
    when (this) {
        VesperPlaybackCodecFamily.H264 -> "h264"
        VesperPlaybackCodecFamily.Hevc -> "hevc"
        VesperPlaybackCodecFamily.Av1 -> "av1"
        VesperPlaybackCodecFamily.Vvc -> "vvc"
        VesperPlaybackCodecFamily.Unknown -> "unknown"
    }

private fun VesperPlaybackCapabilityOutputFormat.toWireName(): String =
    when (this) {
        VesperPlaybackCapabilityOutputFormat.Nv12 -> "nv12"
        VesperPlaybackCapabilityOutputFormat.P010 -> "p010"
        VesperPlaybackCapabilityOutputFormat.SurfaceOpaque -> "surfaceOpaque"
        VesperPlaybackCapabilityOutputFormat.Unknown -> "unknown"
    }

private fun VesperPlaybackCapabilityHdrKind.toWireName(): String =
    when (this) {
        VesperPlaybackCapabilityHdrKind.None -> "none"
        VesperPlaybackCapabilityHdrKind.Hdr10 -> "hdr10"
        VesperPlaybackCapabilityHdrKind.Hlg -> "hlg"
        VesperPlaybackCapabilityHdrKind.DolbyVision -> "dolbyVision"
        VesperPlaybackCapabilityHdrKind.Unknown -> "unknown"
    }

private fun VesperPlaybackCapabilityDolbyVisionMode.toWireName(): String =
    when (this) {
        VesperPlaybackCapabilityDolbyVisionMode.None -> "none"
        VesperPlaybackCapabilityDolbyVisionMode.FullChainCandidate -> "fullChainCandidate"
        VesperPlaybackCapabilityDolbyVisionMode.CompatibleBaseLayer -> "compatibleBaseLayer"
        VesperPlaybackCapabilityDolbyVisionMode.Unsupported -> "unsupported"
    }

private fun VesperPlaybackCapabilityConfidence.toWireName(): String =
    when (this) {
        VesperPlaybackCapabilityConfidence.CodecOnly -> "codecOnly"
        VesperPlaybackCapabilityConfidence.SourceMetadata -> "sourceMetadata"
        VesperPlaybackCapabilityConfidence.SessionProbe -> "sessionProbe"
    }

private fun VesperRecommendedPlaybackPath.toWireName(): String =
    when (this) {
        VesperRecommendedPlaybackPath.NativeFramePipeline -> "nativeFramePipeline"
        VesperRecommendedPlaybackPath.SystemPlayer -> "systemPlayer"
    }

private fun exampleMergeMaps(
    base: Map<String, Any?>,
    overrides: Map<String, Any?>,
): Map<String, Any?> =
    base.toMutableMap().apply { putAll(overrides) }

private fun Map<String, Any?>.pathValue(vararg path: String): Any? {
    var current: Any? = this
    path.forEach { key ->
        current = (current as? Map<*, *>)?.get(key) ?: return null
    }
    return current
}

private fun exampleHdrEvidenceJsonValue(value: Any?): Any =
    when (value) {
        null -> JSONObject.NULL
        is JSONObject, is JSONArray, is String, is Number, is Boolean -> value
        is Map<*, *> ->
            JSONObject().also { output ->
                value.forEach { (key, item) ->
                    output.put(key.toString(), exampleHdrEvidenceJsonValue(item))
                }
            }
        is Iterable<*> ->
            JSONArray().also { output ->
                value.forEach { output.put(exampleHdrEvidenceJsonValue(it)) }
            }
        is Array<*> ->
            JSONArray().also { output ->
                value.forEach { output.put(exampleHdrEvidenceJsonValue(it)) }
            }
        else -> value.toString()
    }
