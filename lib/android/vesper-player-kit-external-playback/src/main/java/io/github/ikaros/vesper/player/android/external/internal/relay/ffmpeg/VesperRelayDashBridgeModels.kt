package io.github.ikaros.vesper.player.android.external.internal.relay.ffmpeg

import java.util.concurrent.atomic.AtomicBoolean
import org.json.JSONArray
import org.json.JSONObject

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
            if (!loaded.get()) {
                System.loadLibrary("vesper_player_relay_ffmpeg")
                loaded.set(true)
            }
        }
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
    ): List<VesperRelayDashMediaSegment> =
        VesperRelayDashMediaSegment.fromJsonArray(
            JSONArray(
                VesperRelayDashBridgeNative.executeJson(
                    JSONObject()
                        .put("operation", "media_segments")
                        .put("segmentBase", segmentBase.toJson())
                        .put("sidx", sidx.toJson())
                        .toString(),
                ),
            ),
        )
}

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
