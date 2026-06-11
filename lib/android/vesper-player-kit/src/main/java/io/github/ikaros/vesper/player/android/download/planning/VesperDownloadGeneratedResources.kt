package io.github.ikaros.vesper.player.android

import java.io.File

internal class VesperGeneratedDownloadResourceMaterializer(
    private val baseDirectory: File?,
    private val fallbackBaseDirectory: File?,
) {
    fun materialize(
        assetId: VesperDownloadAssetId,
        taskId: VesperDownloadTaskId?,
        profile: VesperDownloadProfile,
        assetIndex: VesperDownloadAssetIndex,
    ): VesperDownloadAssetIndex {
        if (assetIndex.resources.none { it.generatedText != null }) {
            return assetIndex.compactedForPersistence()
        }
        val taskDirectory = taskBaseDirectory(assetId, taskId, profile)
        val generatedDirectory = File(taskDirectory, ".generated")
        check(generatedDirectory.mkdirs() || generatedDirectory.isDirectory) {
            "failed to create generated download resource directory ${generatedDirectory.absolutePath}"
        }
        val usedNames = linkedSetOf<String>()
        val resources =
            assetIndex.resources.map { resource ->
                val generatedText = resource.generatedText ?: return@map resource
                val data = generatedText.toByteArray(Charsets.UTF_8)
                val fileName = uniqueGeneratedFileName(resource, usedNames)
                val destination = File(generatedDirectory, fileName)
                runCatching {
                    destination.writeBytes(data)
                }.getOrElse { error ->
                    throw IllegalStateException(
                        "failed to persist generated download resource ${resource.resourceId}: ${error.message}",
                        error,
                    )
                }
                resource.copy(
                    uri = destination.toURI().toString(),
                    generatedText = null,
                    sizeBytes = data.size.toLong(),
                )
            }
        return assetIndex.copy(
            totalSizeBytes = recomputeTotalSizeBytes(assetIndex.totalSizeBytes, resources, assetIndex.segments),
            resources = resources,
        )
    }

    private fun taskBaseDirectory(
        assetId: VesperDownloadAssetId,
        taskId: VesperDownloadTaskId?,
        profile: VesperDownloadProfile,
    ): File =
        profile.targetDirectory
            ?.takeIf { it.isNotBlank() }
            ?.let(::File)
            ?: File(
                baseDirectory ?: fallbackBaseDirectory ?: File("vesper-downloads"),
                assetId.ifBlank { taskId?.toString() ?: "asset" },
            )

    private fun uniqueGeneratedFileName(
        resource: VesperDownloadResourceRecord,
        usedNames: MutableSet<String>,
    ): String {
        val baseName = generatedBaseName(resource)
        if (usedNames.add(baseName)) {
            return baseName
        }
        val hashed = appendStableHash(baseName, stableShortHash("${resource.resourceId}|${resource.relativePath}|${resource.uri}"))
        usedNames.add(hashed)
        return hashed
    }

    private fun generatedBaseName(resource: VesperDownloadResourceRecord): String {
        val raw = resource.relativePath?.substringAfterLast('/')?.substringAfterLast('\\') ?: resource.resourceId
        val sanitized = raw.replace(Regex("[^A-Za-z0-9._-]+"), "_").trim('.', ' ')
        return sanitized.takeIf { it.isNotBlank() && it != ".." }
            ?: "resource-${stableShortHash(resource.resourceId.ifBlank { resource.uri })}"
    }

    private fun appendStableHash(
        fileName: String,
        hash: String,
    ): String {
        val extension = fileName.substringAfterLast('.', "")
        val stem = if (extension.isBlank() || extension == fileName) fileName else fileName.removeSuffix(".$extension")
        return if (extension.isBlank() || extension == fileName) "$stem-$hash" else "$stem-$hash.$extension"
    }

    private fun stableShortHash(value: String): String {
        var hash = -3750763034362895579L
        value.toByteArray(Charsets.UTF_8).forEach { byte ->
            hash = hash xor (byte.toLong() and 0xffL)
            hash *= 1099511628211L
        }
        return java.lang.Long.toUnsignedString(hash, 16).takeLast(8)
    }

    private fun recomputeTotalSizeBytes(
        original: Long?,
        resources: List<VesperDownloadResourceRecord>,
        segments: List<VesperDownloadSegmentRecord>,
    ): Long? {
        var total = 0L
        resources.forEach { total += it.sizeBytes ?: return original }
        segments.forEach { total += it.sizeBytes ?: return original }
        return total
    }
}

