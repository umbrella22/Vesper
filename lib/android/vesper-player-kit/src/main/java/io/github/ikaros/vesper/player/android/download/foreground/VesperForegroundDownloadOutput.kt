package io.github.ikaros.vesper.player.android

import java.io.File

internal fun VesperForegroundDownloadExecutor.buildExecutionPlan(task: VesperDownloadTaskSnapshot): List<ForegroundDownloadEntry> {
    val resources =
        task.assetIndex.resources.map { resource ->
            ForegroundDownloadEntry(
                uri = resource.uri,
                resourceId = resource.resourceId.ifBlank { null },
                segmentId = null,
                relativePath = resource.relativePath,
                byteRange = resource.byteRange,
                generatedText = resource.generatedText,
                expectedSizeBytes = resource.sizeBytes,
                fallbackName = resource.resourceId.ifBlank { "resource" },
                isSegment = false,
            )
        }
    val segments =
        task.assetIndex.segments.mapIndexed { index, segment ->
            ForegroundDownloadEntry(
                uri = segment.uri,
                resourceId = null,
                segmentId = segment.segmentId.ifBlank { null },
                relativePath = segment.relativePath,
                byteRange = segment.byteRange,
                generatedText = null,
                expectedSizeBytes = segment.sizeBytes,
                fallbackName =
                    segment.segmentId.ifBlank {
                        "segment-${segment.sequence ?: (index + 1).toLong()}"
                    },
                isSegment = true,
            )
        }
    if (resources.isNotEmpty() || segments.isNotEmpty()) {
        return buildList {
            addAll(resources)
            addAll(segments)
        }
    }

    val fallbackUri = task.source.manifestUri ?: task.source.source.uri
    return listOf(
        ForegroundDownloadEntry(
            uri = fallbackUri,
            resourceId = null,
            segmentId = null,
            relativePath = null,
            byteRange = null,
            generatedText = null,
            expectedSizeBytes = task.progress.totalBytes,
            fallbackName = task.assetId.ifBlank { "download-${task.taskId}" },
            isSegment = false,
        ),
    )
}

internal fun VesperForegroundDownloadExecutor.resolveOutputFile(
    task: VesperDownloadTaskSnapshot,
    entry: ForegroundDownloadEntry,
    index: Int,
): File {
    val baseDirectory = resolveBaseDirectory(task)
    val relativePath = entry.relativePath?.takeIf { it.isNotBlank() }
    if (relativePath != null) {
        val candidate = File(relativePath)
        if (candidate.isAbsolute) {
            return candidate
        }
        val parts = relativePath.split('/', '\\')
        require(parts.none { it == ".." }) { "download output path escapes the task directory: $relativePath" }
        val outputFile = File(baseDirectory, relativePath).canonicalFile
        val canonicalBase = baseDirectory.canonicalFile
        require(outputFile.path == canonicalBase.path || outputFile.path.startsWith(canonicalBase.path + File.separator)) {
            "download output path escapes the task directory: $relativePath"
        }
        return outputFile
    }

    val inferredName =
        lastPathSegmentFromUri(entry.uri)
            ?: "${entry.fallbackName}-${index + 1}.bin"
    return File(baseDirectory, inferredName)
}

internal fun VesperForegroundDownloadExecutor.resolveCompletedPath(
    task: VesperDownloadTaskSnapshot,
    plan: List<ForegroundDownloadEntry>,
): String =
    if (plan.size == 1) {
        resolveOutputFile(task, plan.single(), 0).absolutePath
    } else {
        resolveBaseDirectory(task).absolutePath
    }

internal fun VesperForegroundDownloadExecutor.resolveBaseDirectory(task: VesperDownloadTaskSnapshot): File =
    task.profile.targetDirectory
        ?.takeIf { it.isNotBlank() }
        ?.let(::File)
        ?: resolveDefaultAssetDirectory(task)

internal fun VesperForegroundDownloadExecutor.resolveDefaultAssetDirectory(task: VesperDownloadTaskSnapshot): File =
    File(
        baseDirectory
            ?: vesperDefaultDownloadBaseDirectory(
                checkNotNull(appContext) { "Android Context is required when no download base directory is configured" }.filesDir,
                null,
            ),
        task.assetId.ifBlank { task.taskId.toString() },
    )

internal data class ForegroundDownloadEntry(
    val uri: String,
    val resourceId: String?,
    val segmentId: String?,
    val relativePath: String?,
    val byteRange: VesperDownloadByteRange?,
    val generatedText: String?,
    val expectedSizeBytes: Long?,
    val fallbackName: String,
    val isSegment: Boolean,
)

