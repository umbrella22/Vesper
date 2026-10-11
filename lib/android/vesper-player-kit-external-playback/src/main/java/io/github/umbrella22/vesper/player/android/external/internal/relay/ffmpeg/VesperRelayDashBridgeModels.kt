package io.github.umbrella22.vesper.player.android.external.internal.relay.ffmpeg

import java.util.concurrent.atomic.AtomicBoolean
import org.json.JSONArray
import org.json.JSONObject
import org.json.JSONTokener

internal const val MAX_HOST_PREPARED_DASH_SEGMENTS_PER_TRACK = 8_192

@Volatile
internal var VesperRelayDashBridgeExecutor: (String) -> String = { requestJson ->
    VesperRelayDashBridgeNative.ensureLoaded()
    VesperRelayDashBridgeNative.nativeExecuteJson(requestJson)
}

internal object VesperRelayDashBridgeNative {
    private val loaded = AtomicBoolean(false)

    fun ensureLoaded() {
        if (loaded.get()) {
            return
        }
        synchronized(this) {
            if (loaded.get()) {
                return
            }
        }
        // Load the native library outside the synchronized block to avoid
        // holding a monitor during file I/O (AGENTS.md rule).
        System.loadLibrary("vesper_player_relay_ffmpeg")
        loaded.set(true)
    }

    fun executeJson(requestJson: String): String = VesperRelayDashBridgeExecutor(requestJson)

    @JvmStatic
    external fun nativeExecuteJson(requestJson: String): String
}

internal interface VesperRelayDashBridgeApi {
    fun parseSidx(data: ByteArray): VesperRelayDashSidxBox

    fun mediaSegments(
        segmentBase: VesperRelayDashByteRangeSegmentBase,
        sidx: VesperRelayDashSidxBox,
    ): List<VesperRelayDashMediaSegment>

    fun templateSegments(
        manifestType: String,
        durationMs: Long?,
        template: DashTemplate,
    ): List<VesperRelayDashTemplateSegment>

    fun expandTemplate(
        template: String,
        representationId: String,
        bandwidth: Long?,
        number: Long?,
        time: Long?,
    ): String
}

@Volatile
internal var VesperRelayDashBridgeApiProvider: VesperRelayDashBridgeApi = JniVesperRelayDashBridgeApi

internal object JniVesperRelayDashBridgeApi : VesperRelayDashBridgeApi {
    override fun parseSidx(data: ByteArray): VesperRelayDashSidxBox =
        VesperRelayDashSidxBox.fromJson(
            JSONObject(
                VesperRelayDashBridgeNative.executeJson(
                    JSONObject()
                        .put("operation", "parse_sidx")
                        .put("data", JSONArray(data.map { it.toInt() and 0xff }))
                        .toString(),
                ),
            ),
        )

    override fun mediaSegments(
        segmentBase: VesperRelayDashByteRangeSegmentBase,
        sidx: VesperRelayDashSidxBox,
    ): List<VesperRelayDashMediaSegment> {
        val response =
            JSONArray(
                VesperRelayDashBridgeNative.executeJson(
                    JSONObject()
                        .put("operation", "media_segments")
                        .put("segmentBase", segmentBase.toJson())
                        .put("sidx", sidx.toJson())
                        .toString(),
                )
            )
        if (response.length() > MAX_HOST_PREPARED_DASH_SEGMENTS_PER_TRACK) {
            throw VesperRelayDashSegmentLimitException(response.length())
        }
        return VesperRelayDashMediaSegment.fromJsonArray(response)
    }

    override fun templateSegments(
        manifestType: String,
        durationMs: Long?,
        template: DashTemplate,
    ): List<VesperRelayDashTemplateSegment> {
        val response =
            JSONArray(
                VesperRelayDashBridgeNative.executeJson(
                    JSONObject()
                        .put("operation", "template_segments")
                        .put("manifestType", manifestType)
                        .put("durationMs", durationMs ?: JSONObject.NULL)
                        .put("segmentTemplate", template.toBridgeJson())
                        .toString(),
                ),
            )
        if (response.length() > MAX_HOST_PREPARED_DASH_SEGMENTS_PER_TRACK) {
            throw VesperRelayDashSegmentLimitException(response.length())
        }
        return VesperRelayDashTemplateSegment.fromJsonArray(response)
    }

    override fun expandTemplate(
        template: String,
        representationId: String,
        bandwidth: Long?,
        number: Long?,
        time: Long?,
    ): String {
        val response = VesperRelayDashBridgeNative.executeJson(
            JSONObject()
                .put("operation", "expand_template")
                .put("template", template)
                .put(
                    "representation",
                    JSONObject()
                        .put("id", representationId)
                        .put("baseURL", "")
                        .put("mimeType", "application/mp4")
                        .put("codecs", "")
                        .put("bandwidth", bandwidth ?: JSONObject.NULL)
                        .put("width", JSONObject.NULL)
                        .put("height", JSONObject.NULL)
                        .put("frameRate", JSONObject.NULL)
                        .put("audioSamplingRate", JSONObject.NULL)
                        .put("segmentBase", JSONObject.NULL)
                        .put("segmentTemplate", JSONObject.NULL),
                )
                .put("number", number ?: JSONObject.NULL)
                .put("time", time ?: JSONObject.NULL)
                .toString(),
        )
        return JSONTokener(response).nextValue() as? String
            ?: throw IllegalArgumentException("DASH template expansion returned a non-string result")
    }
}

internal class VesperRelayDashSegmentLimitException(
    val segmentCount: Int,
) : IllegalArgumentException(
    "DASH track expands to $segmentCount segments, max is $MAX_HOST_PREPARED_DASH_SEGMENTS_PER_TRACK.",
)

internal data class VesperRelayDashByteRangeSegmentBase(
    val initialization: VesperRelayDashByteRange,
    val indexRange: VesperRelayDashByteRange,
) {
    fun toJson(): JSONObject =
        JSONObject()
            .put("initialization", initialization.toJson())
            .put("indexRange", indexRange.toJson())
}

internal data class VesperRelayDashSidxBox(
    val timescale: Long,
    val earliestPresentationTime: Long,
    val firstOffset: Long,
    val references: List<VesperRelayDashSidxReference>,
) {
    fun toJson(): JSONObject =
        JSONObject()
            .put("timescale", timescale)
            .put("earliestPresentationTime", earliestPresentationTime)
            .put("firstOffset", firstOffset)
            .put("references", JSONArray(references.map { it.toJson() }))

    companion object {
        fun fromJson(json: JSONObject): VesperRelayDashSidxBox =
            VesperRelayDashSidxBox(
                timescale = json.getLong("timescale"),
                earliestPresentationTime = json.getLong("earliestPresentationTime"),
                firstOffset = json.getLong("firstOffset"),
                references = json.getJSONArray("references").toJsonObjectList()
                    .map(VesperRelayDashSidxReference::fromJson),
            )
    }
}

internal data class VesperRelayDashSidxReference(
    val referenceType: Int,
    val referencedSize: Long,
    val subsegmentDuration: Long,
    val startsWithSap: Boolean,
    val sapType: Int,
    val sapDeltaTime: Long,
) {
    fun toJson(): JSONObject =
        JSONObject()
            .put("referenceType", referenceType)
            .put("referencedSize", referencedSize)
            .put("subsegmentDuration", subsegmentDuration)
            .put("startsWithSap", startsWithSap)
            .put("sapType", sapType)
            .put("sapDeltaTime", sapDeltaTime)

    companion object {
        fun fromJson(json: JSONObject): VesperRelayDashSidxReference =
            VesperRelayDashSidxReference(
                referenceType = json.getInt("referenceType"),
                referencedSize = json.getLong("referencedSize"),
                subsegmentDuration = json.getLong("subsegmentDuration"),
                startsWithSap = json.optBoolean("startsWithSap"),
                sapType = json.optInt("sapType"),
                sapDeltaTime = json.optLong("sapDeltaTime"),
            )
    }
}

internal data class VesperRelayDashMediaSegment(
    val duration: Double,
    val range: VesperRelayDashByteRange,
) {
    companion object {
        fun fromJson(json: JSONObject): VesperRelayDashMediaSegment =
            VesperRelayDashMediaSegment(
                duration = json.getDouble("duration"),
                range = VesperRelayDashByteRange.fromJson(json.getJSONObject("range")),
            )

        fun fromJsonArray(json: JSONArray): List<VesperRelayDashMediaSegment> =
            json.toJsonObjectList().map(::fromJson)
    }
}

internal data class VesperRelayDashTemplateSegment(
    val duration: Double,
    val number: Long,
    val time: Long?,
    val presentationTime: Long,
) {
    companion object {
        fun fromJson(json: JSONObject): VesperRelayDashTemplateSegment =
            VesperRelayDashTemplateSegment(
                duration = json.getDouble("duration"),
                number = json.getDashTemplateLong("number"),
                time = if (json.isNull("time")) null else json.getDashTemplateLong("time"),
                presentationTime = json.getDashTemplateLong("presentationTime"),
            )

        fun fromJsonArray(json: JSONArray): List<VesperRelayDashTemplateSegment> =
            json.toJsonObjectList().map(::fromJson)
    }
}

private fun JSONObject.getDashTemplateLong(field: String): Long =
    get(field).toString().toLongOrNull()?.takeIf { it >= 0L }
        ?: throw IllegalArgumentException("DASH template $field exceeds the host Long range")

internal data class VesperRelayDashByteRange(
    val start: Long,
    val end: Long,
) {
    fun toJson(): JSONObject =
        JSONObject()
            .put("start", start)
            .put("end", end)

    fun toHeaderValue(): String = "bytes=$start-$end"

    val length: Long
        get() = end - start + 1

    fun lengthAsInt(): Int = length.coerceAtMost(Int.MAX_VALUE.toLong()).toInt()

    companion object {
        fun fromJson(json: JSONObject): VesperRelayDashByteRange =
            VesperRelayDashByteRange(
                start = json.getLong("start"),
                end = json.getLong("end"),
            )
    }
}

internal fun JSONArray.toJsonObjectList(): List<JSONObject> =
    buildList {
        for (index in 0 until length()) {
            add(getJSONObject(index))
        }
    }

private fun DashTemplate.toBridgeJson(): JSONObject =
    JSONObject()
        .put("timescale", timescale)
        .put("duration", duration ?: JSONObject.NULL)
        .put("startNumber", startNumber)
        .put("presentationTimeOffset", presentationTimeOffset)
        .put("initialization", initialization ?: JSONObject.NULL)
        .put("media", media)
        .put(
            "timeline",
            JSONArray(
                timeline.map { entry ->
                    JSONObject()
                        .put("startTime", entry.startTime ?: JSONObject.NULL)
                        .put("duration", entry.duration)
                        .put("repeatCount", entry.repeatCount)
                },
            ),
        )
