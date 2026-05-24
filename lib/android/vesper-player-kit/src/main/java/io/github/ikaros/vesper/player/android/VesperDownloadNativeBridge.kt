package io.github.ikaros.vesper.player.android

import android.net.Uri

internal interface DownloadBindings {
    fun createDownloadSession(config: NativeDownloadConfig): Long

    fun disposeDownloadSession(sessionHandle: Long)

    fun createDownloadTask(
        sessionHandle: Long,
        assetId: String,
        source: NativeDownloadSource,
        profile: NativeDownloadProfile,
        assetIndex: NativeDownloadAssetIndex,
        nowEpochMs: Long,
    ): Long

    fun restoreDownloadTasks(
        sessionHandle: Long,
        tasks: Array<NativeDownloadTask>,
        nowEpochMs: Long,
    ): Boolean

    fun startDownloadTask(
        sessionHandle: Long,
        taskId: Long,
        nowEpochMs: Long,
    ): Boolean

    fun pauseDownloadTask(
        sessionHandle: Long,
        taskId: Long,
        nowEpochMs: Long,
    ): Boolean

    fun resumeDownloadTask(
        sessionHandle: Long,
        taskId: Long,
        nowEpochMs: Long,
    ): Boolean

    fun updateDownloadTaskProgress(
        sessionHandle: Long,
        taskId: Long,
        receivedBytes: Long,
        receivedSegments: Int,
        nowEpochMs: Long,
    ): Boolean

    fun completeDownloadTask(
        sessionHandle: Long,
        taskId: Long,
        completedPath: String,
        nowEpochMs: Long,
    ): Boolean

    fun completeDownloadPreparation(
        sessionHandle: Long,
        taskId: Long,
        assetIndex: NativeDownloadAssetIndex,
        nowEpochMs: Long,
    ): Boolean

    fun replaceDownloadTaskPlan(
        sessionHandle: Long,
        taskId: Long,
        source: NativeDownloadSource,
        profile: NativeDownloadProfile,
        assetIndex: NativeDownloadAssetIndex,
        nowEpochMs: Long,
    ): Boolean

    fun exportDownloadTask(
        sessionHandle: Long,
        taskId: Long,
        outputPath: String,
        progressCallback: NativeDownloadExportProgressCallback?,
    ): Boolean

    fun failDownloadTask(
        sessionHandle: Long,
        taskId: Long,
        codeOrdinal: Int,
        categoryOrdinal: Int,
        retriable: Boolean,
        message: String,
        nowEpochMs: Long,
    ): Boolean

    fun removeDownloadTask(
        sessionHandle: Long,
        taskId: Long,
        nowEpochMs: Long,
    ): Boolean

    fun pollDownloadSnapshot(sessionHandle: Long): NativeDownloadSnapshot?

    fun drainDownloadCommands(sessionHandle: Long): Array<NativeDownloadCommand>

    fun drainDownloadEvents(sessionHandle: Long): Array<NativeDownloadEvent>
}

internal object NativeDownloadBindings : DownloadBindings {
    override fun createDownloadSession(config: NativeDownloadConfig): Long =
        VesperNativeJni.createDownloadSession(config)

    override fun disposeDownloadSession(sessionHandle: Long) =
        VesperNativeJni.disposeDownloadSession(sessionHandle)

    override fun createDownloadTask(
        sessionHandle: Long,
        assetId: String,
        source: NativeDownloadSource,
        profile: NativeDownloadProfile,
        assetIndex: NativeDownloadAssetIndex,
        nowEpochMs: Long,
    ): Long =
        VesperNativeJni.createDownloadTask(
            sessionHandle = sessionHandle,
            assetId = assetId,
            source = source,
            profile = profile,
            assetIndex = assetIndex,
            nowEpochMs = nowEpochMs,
        )

    override fun restoreDownloadTasks(
        sessionHandle: Long,
        tasks: Array<NativeDownloadTask>,
        nowEpochMs: Long,
    ): Boolean =
        VesperNativeJni.restoreDownloadTasks(
            sessionHandle = sessionHandle,
            tasks = tasks,
            nowEpochMs = nowEpochMs,
        )

    override fun startDownloadTask(
        sessionHandle: Long,
        taskId: Long,
        nowEpochMs: Long,
    ): Boolean = VesperNativeJni.startDownloadTask(sessionHandle, taskId, nowEpochMs)

    override fun pauseDownloadTask(
        sessionHandle: Long,
        taskId: Long,
        nowEpochMs: Long,
    ): Boolean = VesperNativeJni.pauseDownloadTask(sessionHandle, taskId, nowEpochMs)

    override fun resumeDownloadTask(
        sessionHandle: Long,
        taskId: Long,
        nowEpochMs: Long,
    ): Boolean = VesperNativeJni.resumeDownloadTask(sessionHandle, taskId, nowEpochMs)

    override fun updateDownloadTaskProgress(
        sessionHandle: Long,
        taskId: Long,
        receivedBytes: Long,
        receivedSegments: Int,
        nowEpochMs: Long,
    ): Boolean =
        VesperNativeJni.updateDownloadTaskProgress(
            sessionHandle = sessionHandle,
            taskId = taskId,
            receivedBytes = receivedBytes,
            receivedSegments = receivedSegments,
            nowEpochMs = nowEpochMs,
        )

    override fun completeDownloadTask(
        sessionHandle: Long,
        taskId: Long,
        completedPath: String,
        nowEpochMs: Long,
    ): Boolean =
        VesperNativeJni.completeDownloadTask(
            sessionHandle = sessionHandle,
            taskId = taskId,
            completedPath = completedPath,
            nowEpochMs = nowEpochMs,
        )

    override fun completeDownloadPreparation(
        sessionHandle: Long,
        taskId: Long,
        assetIndex: NativeDownloadAssetIndex,
        nowEpochMs: Long,
    ): Boolean =
        VesperNativeJni.completeDownloadPreparation(
            sessionHandle = sessionHandle,
            taskId = taskId,
            assetIndex = assetIndex,
            nowEpochMs = nowEpochMs,
        )

    override fun replaceDownloadTaskPlan(
        sessionHandle: Long,
        taskId: Long,
        source: NativeDownloadSource,
        profile: NativeDownloadProfile,
        assetIndex: NativeDownloadAssetIndex,
        nowEpochMs: Long,
    ): Boolean =
        VesperNativeJni.replaceDownloadTaskPlan(
            sessionHandle = sessionHandle,
            taskId = taskId,
            source = source,
            profile = profile,
            assetIndex = assetIndex,
            nowEpochMs = nowEpochMs,
        )

    override fun exportDownloadTask(
        sessionHandle: Long,
        taskId: Long,
        outputPath: String,
        progressCallback: NativeDownloadExportProgressCallback?,
    ): Boolean =
        VesperNativeJni.exportDownloadTask(
            sessionHandle = sessionHandle,
            taskId = taskId,
            outputPath = outputPath,
            progressCallback = progressCallback,
        )

    override fun failDownloadTask(
        sessionHandle: Long,
        taskId: Long,
        codeOrdinal: Int,
        categoryOrdinal: Int,
        retriable: Boolean,
        message: String,
        nowEpochMs: Long,
    ): Boolean =
        VesperNativeJni.failDownloadTask(
            sessionHandle = sessionHandle,
            taskId = taskId,
            codeOrdinal = codeOrdinal,
            categoryOrdinal = categoryOrdinal,
            retriable = retriable,
            message = message,
            nowEpochMs = nowEpochMs,
        )

    override fun removeDownloadTask(
        sessionHandle: Long,
        taskId: Long,
        nowEpochMs: Long,
    ): Boolean = VesperNativeJni.removeDownloadTask(sessionHandle, taskId, nowEpochMs)

    override fun pollDownloadSnapshot(sessionHandle: Long): NativeDownloadSnapshot? =
        VesperNativeJni.pollDownloadSnapshot(sessionHandle)

    override fun drainDownloadCommands(sessionHandle: Long): Array<NativeDownloadCommand> =
        VesperNativeJni.drainDownloadCommands(sessionHandle)

    override fun drainDownloadEvents(sessionHandle: Long): Array<NativeDownloadEvent> =
        VesperNativeJni.drainDownloadEvents(sessionHandle)
}

internal fun VesperDownloadConfiguration.toNativePayload(): NativeDownloadConfig =
    NativeDownloadConfig(
        autoStart = autoStart,
        runPostProcessorsOnCompletion = runPostProcessorsOnCompletion,
        pluginLibraryPaths = pluginLibraryPaths.toTypedArray(),
    )

internal fun VesperDownloadSource.toNativePayload(): NativeDownloadSource =
    sanitizeDownloadRequestHeaders(source.headers).let { headers ->
        NativeDownloadSource(
            sourceUri = source.uri,
            contentFormatOrdinal = contentFormat.ordinal,
            manifestUri = manifestUri,
            headerNames = headers.keys.toTypedArray(),
            headerValues = headers.values.toTypedArray(),
        )
    }

internal fun VesperDownloadProfile.toNativePayload(): NativeDownloadProfile =
    NativeDownloadProfile(
        variantId = variantId,
        preferredAudioLanguage = preferredAudioLanguage,
        preferredSubtitleLanguage = preferredSubtitleLanguage,
        selectedTrackIds = selectedTrackIds.toTypedArray(),
        targetOutputFormatOrdinal = targetOutputFormat?.ordinal ?: -1,
        targetDirectory = targetDirectory,
        allowMeteredNetwork = allowMeteredNetwork,
    )

internal fun VesperDownloadAssetIndex.toNativePayload(): NativeDownloadAssetIndex =
    NativeDownloadAssetIndex(
        contentFormatOrdinal = contentFormat.ordinal,
        version = version,
        etag = etag,
        checksum = checksum,
        hasTotalSizeBytes = totalSizeBytes != null,
        totalSizeBytes = totalSizeBytes ?: 0L,
        resources = resources.map(VesperDownloadResourceRecord::toNativePayload).toTypedArray(),
        segments = segments.map(VesperDownloadSegmentRecord::toNativePayload).toTypedArray(),
        streams = streams.map(VesperDownloadAssetStream::toNativePayload).toTypedArray(),
        completedPath = completedPath,
    )

internal fun VesperDownloadResourceRecord.toNativePayload(): NativeDownloadResourceRecord =
    NativeDownloadResourceRecord(
        resourceId = resourceId,
        uri = uri,
        relativePath = relativePath,
        byteRange = byteRange?.toNativePayload(),
        generatedText = generatedText,
        hasSizeBytes = sizeBytes != null,
        sizeBytes = sizeBytes ?: 0L,
        etag = etag,
        checksum = checksum,
    )

internal fun VesperDownloadSegmentRecord.toNativePayload(): NativeDownloadSegmentRecord =
    NativeDownloadSegmentRecord(
        segmentId = segmentId,
        uri = uri,
        relativePath = relativePath,
        hasSequence = sequence != null,
        sequence = sequence ?: 0L,
        byteRange = byteRange?.toNativePayload(),
        hasSizeBytes = sizeBytes != null,
        sizeBytes = sizeBytes ?: 0L,
        checksum = checksum,
    )

internal fun VesperDownloadAssetStream.toNativePayload(): NativeDownloadAssetStream =
    NativeDownloadAssetStream(
        streamId = streamId,
        kindOrdinal = kind.ordinal,
        language = language,
        codec = codec,
        label = label,
        hasQualityRank = qualityRank != null,
        qualityRank = qualityRank ?: 0,
        resourceIds = resourceIds.toTypedArray(),
        segmentIds = segmentIds.toTypedArray(),
        metadataKeys = metadata.entries.map { it.key }.toTypedArray(),
        metadataValues = metadata.entries.map { it.value }.toTypedArray(),
    )

internal fun VesperDownloadByteRange.toNativePayload(): NativeDownloadByteRange =
    NativeDownloadByteRange(offset = offset, length = length)

internal fun VesperDownloadProgressSnapshot.toNativePayload(): NativeDownloadProgress =
    NativeDownloadProgress(
        receivedBytes = receivedBytes,
        hasTotalBytes = totalBytes != null,
        totalBytes = totalBytes ?: 0L,
        receivedSegments = receivedSegments,
        hasTotalSegments = totalSegments != null,
        totalSegments = totalSegments ?: 0,
    )

internal fun VesperDownloadTaskSnapshot.toNativePayload(): NativeDownloadTask =
    NativeDownloadTask(
        taskId = taskId,
        assetId = assetId,
        source = source.toNativePayload(),
        profile = profile.toNativePayload(),
        statusOrdinal = state.ordinal,
        progress = progress.toNativePayload(),
        assetIndex = assetIndex.toNativePayload(),
        hasError = error != null,
        errorCodeOrdinal = error?.code?.jniOrdinal ?: 0,
        errorCategoryOrdinal = error?.category?.jniOrdinal ?: 0,
        errorRetriable = error?.retriable ?: false,
        errorMessage = error?.message,
    )

internal fun NativeDownloadSnapshot.toPublic(): VesperDownloadSnapshot =
    VesperDownloadSnapshot(tasks = tasks.map(NativeDownloadTask::toPublic))

internal fun NativeDownloadTask.toPublic(): VesperDownloadTaskSnapshot =
    VesperDownloadTaskSnapshot(
        taskId = taskId,
        assetId = assetId,
        source = source.toPublic(),
        profile = profile.toPublic(),
        state = statusOrdinal.toDownloadState(),
        progress = progress.toPublic(),
        assetIndex = assetIndex.toPublic(),
        error =
            if (hasError) {
                VesperDownloadError(
                    code = VesperPlayerErrorCode.fromJniOrdinal(errorCodeOrdinal),
                    category = VesperPlayerErrorCategory.fromJniOrdinal(errorCategoryOrdinal),
                    retriable = errorRetriable,
                    message = errorMessage ?: "download failed",
                )
            } else {
                null
            },
    )

internal fun Int.toDownloadState(): VesperDownloadState =
    when (this) {
        0 -> VesperDownloadState.Queued
        1 -> VesperDownloadState.Preparing
        2 -> VesperDownloadState.Downloading
        3 -> VesperDownloadState.Paused
        4 -> VesperDownloadState.Completed
        5 -> VesperDownloadState.Failed
        6 -> VesperDownloadState.Removed
        else -> VesperDownloadState.Queued
    }

internal fun NativeDownloadSource.toPublic(): VesperDownloadSource =
    downloadSourceHeaders().let { headers ->
        VesperDownloadSource(
            source =
                when {
                    sourceUri.startsWith("content://", ignoreCase = true) ||
                        sourceUri.startsWith("file://", ignoreCase = true) -> {
                        VesperPlayerSource.local(
                            uri = sourceUri,
                            label = Uri.parse(sourceUri).lastPathSegment ?: sourceUri,
                            headers = headers,
                        )
                    }
                    else -> {
                        VesperPlayerSource.remote(uri = sourceUri, label = sourceUri, headers = headers)
                    }
                },
            contentFormat =
                when (contentFormatOrdinal) {
                    0 -> VesperDownloadContentFormat.HlsSegments
                    1 -> VesperDownloadContentFormat.DashSegments
                    2 -> VesperDownloadContentFormat.FlvSegments
                    3 -> VesperDownloadContentFormat.SingleFile
                    else -> VesperDownloadContentFormat.Unknown
                },
            manifestUri = manifestUri,
        )
    }

internal fun NativeDownloadProfile.toPublic(): VesperDownloadProfile =
    VesperDownloadProfile(
        variantId = variantId,
        preferredAudioLanguage = preferredAudioLanguage,
        preferredSubtitleLanguage = preferredSubtitleLanguage,
        selectedTrackIds = selectedTrackIds.toList(),
        targetOutputFormat =
            when (targetOutputFormatOrdinal) {
                0 -> VesperDownloadOutputFormat.Mp4
                1 -> VesperDownloadOutputFormat.Mkv
                2 -> VesperDownloadOutputFormat.Original
                else -> null
            },
        targetDirectory = targetDirectory,
        allowMeteredNetwork = allowMeteredNetwork,
    )

internal fun NativeDownloadAssetIndex.toPublic(): VesperDownloadAssetIndex =
    VesperDownloadAssetIndex(
        contentFormat =
            when (contentFormatOrdinal) {
                0 -> VesperDownloadContentFormat.HlsSegments
                1 -> VesperDownloadContentFormat.DashSegments
                2 -> VesperDownloadContentFormat.FlvSegments
                3 -> VesperDownloadContentFormat.SingleFile
                else -> VesperDownloadContentFormat.Unknown
            },
        version = version,
        etag = etag,
        checksum = checksum,
        totalSizeBytes = if (hasTotalSizeBytes) totalSizeBytes else null,
        resources = resources.map(NativeDownloadResourceRecord::toPublic),
        segments = segments.map(NativeDownloadSegmentRecord::toPublic),
        streams = streams.map(NativeDownloadAssetStream::toPublic),
        completedPath = completedPath,
    )

internal fun NativeDownloadResourceRecord.toPublic(): VesperDownloadResourceRecord =
    VesperDownloadResourceRecord(
        resourceId = resourceId,
        uri = uri,
        relativePath = relativePath,
        byteRange = byteRange?.toPublic(),
        generatedText = null,
        sizeBytes = if (hasSizeBytes) sizeBytes else null,
        etag = etag,
        checksum = checksum,
    )

internal fun NativeDownloadSegmentRecord.toPublic(): VesperDownloadSegmentRecord =
    VesperDownloadSegmentRecord(
        segmentId = segmentId,
        uri = uri,
        relativePath = relativePath,
        sequence = if (hasSequence) sequence else null,
        byteRange = byteRange?.toPublic(),
        sizeBytes = if (hasSizeBytes) sizeBytes else null,
        checksum = checksum,
    )

internal fun NativeDownloadAssetStream.toPublic(): VesperDownloadAssetStream =
    VesperDownloadAssetStream(
        streamId = streamId,
        kind = VesperDownloadStreamKind.entries.getOrElse(kindOrdinal) { VesperDownloadStreamKind.Combined },
        language = language,
        codec = codec,
        label = label,
        qualityRank = if (hasQualityRank) qualityRank else null,
        resourceIds = resourceIds.toList(),
        segmentIds = segmentIds.toList(),
        metadata = metadataKeys.zip(metadataValues).toMap(),
    )

internal fun NativeDownloadByteRange.toPublic(): VesperDownloadByteRange =
    VesperDownloadByteRange(offset = offset, length = length)

internal fun NativeDownloadProgress.toPublic(): VesperDownloadProgressSnapshot =
    VesperDownloadProgressSnapshot(
        receivedBytes = receivedBytes,
        totalBytes = if (hasTotalBytes) totalBytes else null,
        receivedSegments = receivedSegments,
        totalSegments = if (hasTotalSegments) totalSegments else null,
    )

internal fun NativeDownloadEvent.toPublic(): VesperDownloadEvent =
    when (this) {
        is NativeDownloadEvent.Created -> VesperDownloadEvent.Created(task.toPublic())
        is NativeDownloadEvent.StateChanged ->
            VesperDownloadEvent.StateChanged(
                VesperDownloadTaskStatePatch(
                    taskId = taskId,
                    state = statusOrdinal.toDownloadState(),
                    progress = progress.toPublic(),
                    error =
                        if (hasError) {
                            VesperDownloadError(
                                code = VesperPlayerErrorCode.fromJniOrdinal(errorCodeOrdinal),
                                category = VesperPlayerErrorCategory.fromJniOrdinal(errorCategoryOrdinal),
                                retriable = errorRetriable,
                                message = errorMessage.orEmpty(),
                            )
                        } else {
                            null
                        },
                    completedPath = completedPath,
                ),
            )
        is NativeDownloadEvent.AssetIndexUpdated -> VesperDownloadEvent.AssetIndexUpdated(task.toPublic())
        is NativeDownloadEvent.ProgressUpdated ->
            VesperDownloadEvent.ProgressUpdated(
                VesperDownloadTaskProgressPatch(
                    taskId = taskId,
                    progress = progress.toPublic(),
                ),
            )
    }

