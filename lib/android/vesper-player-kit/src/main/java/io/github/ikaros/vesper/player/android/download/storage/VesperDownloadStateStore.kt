package io.github.ikaros.vesper.player.android

import java.io.File
import org.json.JSONArray
import org.json.JSONObject

internal data class ProgressPersistenceCheckpoint(
    val bytes: Long,
    val epochMs: Long,
)

internal class DownloadTaskStore {
    private val tasksById = linkedMapOf<VesperDownloadTaskId, VesperDownloadTaskSnapshot>()

    fun replaceAll(snapshot: VesperDownloadSnapshot) {
        tasksById.clear()
        snapshot.tasks.filter { it.state != VesperDownloadState.Removed }.forEach { task ->
            tasksById[task.taskId] = task
        }
    }

    fun apply(events: List<VesperDownloadEvent>): VesperDownloadSnapshot {
        events.forEach { event ->
            when (event) {
                is VesperDownloadEvent.Created -> tasksById[event.task.taskId] = event.task
                is VesperDownloadEvent.AssetIndexUpdated -> tasksById[event.task.taskId] = event.task
                is VesperDownloadEvent.StateChanged -> {
                    if (event.patch.state == VesperDownloadState.Removed) {
                        tasksById.remove(event.patch.taskId)
                        return@forEach
                    }
                    val task = tasksById[event.patch.taskId] ?: return@forEach
                    tasksById[event.patch.taskId] =
                        task.copy(
                            state = event.patch.state,
                            progress = event.patch.progress,
                            assetIndex =
                                task.assetIndex.copy(
                                    completedPath = event.patch.completedPath ?: task.assetIndex.completedPath,
                                ),
                            error = event.patch.error,
                        )
                }
                is VesperDownloadEvent.ProgressUpdated -> {
                    val task = tasksById[event.patch.taskId] ?: return@forEach
                    tasksById[event.patch.taskId] = task.copy(progress = event.patch.progress)
                }
            }
        }
        return snapshot()
    }

    fun snapshot(): VesperDownloadSnapshot = VesperDownloadSnapshot(tasksById.values.toList())
}


internal interface VesperDownloadStatePersistence {
    fun load(): VesperDownloadSnapshot?

    fun save(snapshot: VesperDownloadSnapshot)
}

internal class VesperDownloadStateStore(private val file: File) : VesperDownloadStatePersistence {
    override fun load(): VesperDownloadSnapshot? =
        runCatching {
            if (!file.isFile) {
                return@runCatching null
            }
            JSONObject(file.readText()).toDownloadSnapshot()
        }.getOrNull()

    override fun save(snapshot: VesperDownloadSnapshot) {
        runCatching {
            val tasks = snapshot.tasks.filter { it.state != VesperDownloadState.Removed }
            if (tasks.isEmpty()) {
                file.delete()
                return@runCatching
            }
            file.parentFile?.mkdirs()
            val root =
                JSONObject().apply {
                    put("version", 1)
                    put(
                        "tasks",
                        JSONArray().apply {
                            tasks.forEach { put(it.toJson()) }
                        },
                    )
                }
            file.writeText(root.toString())
        }
    }
}


internal fun VesperDownloadResourceRecord.compactedForPersistence(): VesperDownloadResourceRecord =
    copy(generatedText = null)

internal fun VesperDownloadAssetIndex.compactedForPersistence(): VesperDownloadAssetIndex =
    copy(resources = resources.map(VesperDownloadResourceRecord::compactedForPersistence))

internal fun VesperDownloadSnapshot.compactedForPersistence(): VesperDownloadSnapshot =
    VesperDownloadSnapshot(tasks = tasks.map { it.copy(assetIndex = it.assetIndex.compactedForPersistence()) })


private fun VesperDownloadTaskSnapshot.toJson(): JSONObject =
    JSONObject().apply {
        put("taskId", taskId)
        put("assetId", assetId)
        put("source", source.toJson())
        put("profile", profile.toJson())
        put("state", state.ordinal)
        put("progress", progress.toJson())
        put("assetIndex", assetIndex.toJson())
        put("error", error?.toJson())
    }

private fun JSONObject.toDownloadTask(): VesperDownloadTaskSnapshot =
    VesperDownloadTaskSnapshot(
        taskId = optLong("taskId", 0L),
        assetId = optString("assetId", ""),
        source = optJSONObject("source")?.toDownloadSource() ?: VesperDownloadSource(
            source = VesperPlayerSource.remote("", ""),
            contentFormat = VesperDownloadContentFormat.Unknown,
        ),
        profile = optJSONObject("profile")?.toDownloadProfile() ?: VesperDownloadProfile(),
        state = enumValue<VesperDownloadState>(optInt("state", VesperDownloadState.Paused.ordinal)),
        progress = optJSONObject("progress")?.toDownloadProgress() ?: VesperDownloadProgressSnapshot(),
        assetIndex = optJSONObject("assetIndex")?.toDownloadAssetIndex() ?: VesperDownloadAssetIndex(),
        error = optJSONObject("error")?.toDownloadError(),
    )

private fun VesperDownloadSnapshot.toJson(): JSONObject =
    JSONObject().apply {
        put(
            "tasks",
            JSONArray().apply {
                tasks.forEach { put(it.toJson()) }
            },
        )
    }

private fun JSONObject.toDownloadSnapshot(): VesperDownloadSnapshot =
    VesperDownloadSnapshot(
        tasks =
            optJSONArray("tasks")
                ?.toObjectList { toDownloadTask() }
                ?: emptyList(),
    )

private fun VesperDownloadSource.toJson(): JSONObject =
    JSONObject().apply {
        put("source", source.toJson())
        put("contentFormat", contentFormat.ordinal)
        put("manifestUri", manifestUri)
    }

private fun JSONObject.toDownloadSource(): VesperDownloadSource =
    VesperDownloadSource(
        source = optJSONObject("source")?.toPlayerSource() ?: VesperPlayerSource.remote("", ""),
        contentFormat = enumValue(optInt("contentFormat", VesperDownloadContentFormat.Unknown.ordinal)),
        manifestUri = optStringOrNull("manifestUri"),
    )

private fun VesperPlayerSource.toJson(): JSONObject =
    JSONObject().apply {
        put("uri", uri)
        put("label", label)
        put("kind", kind.ordinal)
        put("protocol", protocol.ordinal)
        put(
            "headers",
            JSONObject().apply {
                headers.forEach { (key, value) -> put(key, value) }
            },
        )
    }

private fun JSONObject.toPlayerSource(): VesperPlayerSource =
    VesperPlayerSource(
        uri = optString("uri", ""),
        label = optString("label", ""),
        kind = enumValue(optInt("kind", VesperPlayerSourceKind.Remote.ordinal)),
        protocol = enumValue(optInt("protocol", VesperPlayerSourceProtocol.Unknown.ordinal)),
        headers =
            optJSONObject("headers")
                ?.keys()
                ?.asSequence()
                ?.associateWith { key -> optJSONObject("headers")?.optString(key, "") ?: "" }
                ?: emptyMap(),
    )

private fun VesperDownloadProfile.toJson(): JSONObject =
    JSONObject().apply {
        put("variantId", variantId)
        put("preferredAudioLanguage", preferredAudioLanguage)
        put("preferredSubtitleLanguage", preferredSubtitleLanguage)
        put("selectedTrackIds", JSONArray().apply { selectedTrackIds.forEach(::put) })
        put("targetOutputFormat", targetOutputFormat?.ordinal ?: -1)
        put("targetDirectory", targetDirectory)
        put("allowMeteredNetwork", allowMeteredNetwork)
    }

private fun JSONObject.toDownloadProfile(): VesperDownloadProfile =
    VesperDownloadProfile(
        variantId = optStringOrNull("variantId"),
        preferredAudioLanguage = optStringOrNull("preferredAudioLanguage"),
        preferredSubtitleLanguage = optStringOrNull("preferredSubtitleLanguage"),
        selectedTrackIds = optJSONArray("selectedTrackIds")?.toStringList() ?: emptyList(),
        targetOutputFormat =
            optInt("targetOutputFormat", -1)
                .takeIf { it >= 0 }
                ?.let { enumValue<VesperDownloadOutputFormat>(it) },
        targetDirectory = optStringOrNull("targetDirectory"),
        allowMeteredNetwork = optBoolean("allowMeteredNetwork", false),
    )

private fun VesperDownloadProgressSnapshot.toJson(): JSONObject =
    JSONObject().apply {
        put("receivedBytes", receivedBytes)
        put("totalBytes", totalBytes)
        put("receivedSegments", receivedSegments)
        put("totalSegments", totalSegments)
    }

private fun JSONObject.toDownloadProgress(): VesperDownloadProgressSnapshot =
    VesperDownloadProgressSnapshot(
        receivedBytes = optLong("receivedBytes", 0L),
        totalBytes = optLongOrNull("totalBytes"),
        receivedSegments = optInt("receivedSegments", 0),
        totalSegments = optIntOrNull("totalSegments"),
    )

private fun VesperDownloadAssetIndex.toJson(): JSONObject =
    JSONObject().apply {
        put("contentFormat", contentFormat.ordinal)
        put("version", version)
        put("etag", etag)
        put("checksum", checksum)
        put("totalSizeBytes", totalSizeBytes)
        put("resources", JSONArray().apply { resources.forEach { put(it.toJson()) } })
        put("segments", JSONArray().apply { segments.forEach { put(it.toJson()) } })
        put("streams", JSONArray().apply { streams.forEach { put(it.toJson()) } })
        put("completedPath", completedPath)
    }

private fun JSONObject.toDownloadAssetIndex(): VesperDownloadAssetIndex =
    VesperDownloadAssetIndex(
        contentFormat = enumValue(optInt("contentFormat", VesperDownloadContentFormat.Unknown.ordinal)),
        version = optStringOrNull("version"),
        etag = optStringOrNull("etag"),
        checksum = optStringOrNull("checksum"),
        totalSizeBytes = optLongOrNull("totalSizeBytes"),
        resources = optJSONArray("resources")?.toObjectList { toDownloadResource() } ?: emptyList(),
        segments = optJSONArray("segments")?.toObjectList { toDownloadSegment() } ?: emptyList(),
        streams = optJSONArray("streams")?.toObjectList { toDownloadAssetStream() } ?: emptyList(),
        completedPath = optStringOrNull("completedPath"),
    )

private fun VesperDownloadResourceRecord.toJson(): JSONObject =
    JSONObject().apply {
        put("resourceId", resourceId)
        put("uri", uri)
        put("relativePath", relativePath)
        put("byteRange", byteRange?.toJson())
        put("generatedText", null)
        put("sizeBytes", sizeBytes)
        put("etag", etag)
        put("checksum", checksum)
    }

private fun JSONObject.toDownloadResource(): VesperDownloadResourceRecord =
    VesperDownloadResourceRecord(
        resourceId = optString("resourceId", ""),
        uri = optString("uri", ""),
        relativePath = optStringOrNull("relativePath"),
        byteRange = optJSONObject("byteRange")?.toDownloadByteRange(),
        generatedText = optStringOrNull("generatedText"),
        sizeBytes = optLongOrNull("sizeBytes"),
        etag = optStringOrNull("etag"),
        checksum = optStringOrNull("checksum"),
    )

private fun VesperDownloadSegmentRecord.toJson(): JSONObject =
    JSONObject().apply {
        put("segmentId", segmentId)
        put("uri", uri)
        put("relativePath", relativePath)
        put("sequence", sequence)
        put("byteRange", byteRange?.toJson())
        put("sizeBytes", sizeBytes)
        put("checksum", checksum)
    }

private fun JSONObject.toDownloadSegment(): VesperDownloadSegmentRecord =
    VesperDownloadSegmentRecord(
        segmentId = optString("segmentId", ""),
        uri = optString("uri", ""),
        relativePath = optStringOrNull("relativePath"),
        sequence = optLongOrNull("sequence"),
        byteRange = optJSONObject("byteRange")?.toDownloadByteRange(),
        sizeBytes = optLongOrNull("sizeBytes"),
        checksum = optStringOrNull("checksum"),
    )

private fun VesperDownloadAssetStream.toJson(): JSONObject =
    JSONObject().apply {
        put("streamId", streamId)
        put("kind", kind.ordinal)
        put("language", language)
        put("codec", codec)
        put("label", label)
        put("qualityRank", qualityRank)
        put("resourceIds", JSONArray().apply { resourceIds.forEach(::put) })
        put("segmentIds", JSONArray().apply { segmentIds.forEach(::put) })
        put("metadata", JSONObject().apply { metadata.forEach { (key, value) -> put(key, value) } })
    }

private fun JSONObject.toDownloadAssetStream(): VesperDownloadAssetStream =
    VesperDownloadAssetStream(
        streamId = optString("streamId", ""),
        kind = enumValue(optInt("kind", VesperDownloadStreamKind.Combined.ordinal)),
        language = optStringOrNull("language"),
        codec = optStringOrNull("codec"),
        label = optStringOrNull("label"),
        qualityRank = optIntOrNull("qualityRank"),
        resourceIds = optJSONArray("resourceIds")?.toStringList() ?: emptyList(),
        segmentIds = optJSONArray("segmentIds")?.toStringList() ?: emptyList(),
        metadata = optJSONObject("metadata")?.toStringMap() ?: emptyMap(),
    )

private fun VesperDownloadByteRange.toJson(): JSONObject =
    JSONObject().apply {
        put("offset", offset)
        put("length", length)
    }

private fun JSONObject.toDownloadByteRange(): VesperDownloadByteRange =
    VesperDownloadByteRange(offset = optLong("offset", 0L), length = optLong("length", 0L))

private fun VesperDownloadError.toJson(): JSONObject =
    JSONObject().apply {
        put("code", code.wireName)
        put("category", category.wireName)
        put("retriable", retriable)
        put("message", message)
    }

private fun JSONObject.toDownloadError(): VesperDownloadError =
    VesperDownloadError(
        code =
            optStringOrNull("code")?.let(VesperPlayerErrorCode::fromWireName)
                ?: VesperPlayerErrorCode.fromJniOrdinal(optInt("codeOrdinal", 0)),
        category =
            optStringOrNull("category")?.let(VesperPlayerErrorCategory::fromWireName)
                ?: VesperPlayerErrorCategory.fromJniOrdinal(optInt("categoryOrdinal", 0)),
        retriable = optBoolean("retriable", false),
        message = optString("message", "download failed"),
    )

private inline fun <reified T : Enum<T>> enumValue(ordinal: Int): T =
    enumValues<T>().getOrElse(ordinal) { enumValues<T>().last() }

private fun JSONObject.optStringOrNull(key: String): String? =
    if (isNull(key)) null else optString(key).takeIf(String::isNotEmpty)

private fun JSONObject.optLongOrNull(key: String): Long? =
    if (isNull(key) || !has(key)) null else optLong(key)

private fun JSONObject.optIntOrNull(key: String): Int? =
    if (isNull(key) || !has(key)) null else optInt(key)

private fun JSONArray.toStringList(): List<String> =
    buildList {
        for (index in 0 until length()) {
            optString(index).takeIf(String::isNotEmpty)?.let(::add)
        }
    }

private fun JSONObject.toStringMap(): Map<String, String> =
    buildMap {
        keys().forEach { key ->
            optString(key).takeIf(String::isNotEmpty)?.let { put(key, it) }
        }
    }

private fun <T> JSONArray.toObjectList(transform: JSONObject.() -> T): List<T> =
    buildList {
        for (index in 0 until length()) {
            optJSONObject(index)?.let { add(it.transform()) }
        }
    }
