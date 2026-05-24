package io.github.ikaros.vesper.player.android

import android.content.Context
import android.util.Log
import androidx.media3.datasource.DataSource
import androidx.media3.datasource.DataSpec
import androidx.media3.datasource.DefaultDataSource
import java.io.ByteArrayOutputStream
import java.io.File
import java.io.FileOutputStream
import java.net.HttpURLConnection
import java.net.URI
import java.net.URL
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.cancel
import kotlinx.coroutines.currentCoroutineContext
import kotlinx.coroutines.ensureActive
import kotlinx.coroutines.launch
import kotlinx.coroutines.runInterruptible

internal class VesperForegroundDownloadExecutor(
    context: Context?,
    private val baseDirectory: File?,
    private val resumePartialDownloads: Boolean = true,
    rangeChunkBytes: Long? = null,
    private val minProgressBytes: Long = ANDROID_DOWNLOAD_DEFAULT_MIN_PROGRESS_BYTES,
    private val minProgressIntervalMs: Long = ANDROID_DOWNLOAD_DEFAULT_MIN_PROGRESS_INTERVAL_MS,
    private val staleResourcePlanRecoverer: VesperDownloadStaleResourcePlanRecoverer? = null,
) : VesperDownloadExecutor {
    private val appContext = context?.applicationContext
    private val rangeChunkBytes = rangeChunkBytes?.takeIf { it > 0L }
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.IO)
    private val jobsLock = Any()
    private val jobs = mutableMapOf<VesperDownloadTaskId, Job>()
    private val recoveredSourcesLock = Any()
    private val recoveredSources = mutableMapOf<VesperDownloadTaskId, VesperDownloadSource>()
    private val dataSourceFactory by lazy {
        DefaultDataSource.Factory(checkNotNull(appContext) { "Android Context is required for non-HTTP downloads" })
    }

    private fun closeDataSourceQuietly(dataSource: DataSource, context: String) {
        runCatching { dataSource.close() }
            .onFailure { error -> Log.w(DOWNLOAD_TAG, "failed to close download data source for $context", error) }
    }

    private suspend fun prepareAssetIndexWithRecovery(
        task: VesperDownloadTaskSnapshot,
        reporter: VesperDownloadExecutionReporter,
    ): VesperDownloadAssetIndex {
        return try {
            materializeGeneratedResources(
                assetId = task.assetId,
                taskId = task.taskId,
                profile = task.profile,
                assetIndex = prepareAssetIndex(task),
            )
        } catch (error: VesperStaleDownloadResourceException) {
            val recoveredPlan =
                recoverTaskPlan(
                    task,
                    error.toStaleResource(
                        taskId = task.taskId,
                        fallbackPhase = VesperDownloadStaleResourcePhase.Prepare,
                    ),
                ) ?: throw error
            val recoveredAssetIndex =
                materializeGeneratedResources(
                    assetId = task.assetId,
                    taskId = task.taskId,
                    profile = recoveredPlan.profile,
                    assetIndex = recoveredPlan.assetIndex,
                )
            reporter.replaceTaskPlan(task.taskId, recoveredPlan.source, recoveredPlan.profile, recoveredAssetIndex)
            val recoveredTask = task.copy(
                source = recoveredPlan.source,
                profile = recoveredPlan.profile,
                assetIndex = recoveredAssetIndex,
            )
            val assetIndex =
                materializeGeneratedResources(
                    assetId = task.assetId,
                    taskId = task.taskId,
                    profile = recoveredPlan.profile,
                    assetIndex = prepareAssetIndex(recoveredTask),
                )
            synchronized(recoveredSourcesLock) {
                recoveredSources[task.taskId] = recoveredPlan.source
            }
            assetIndex
        }
    }

    private suspend fun recoverTaskPlan(
        task: VesperDownloadTaskSnapshot,
        staleResource: VesperDownloadStaleResource,
    ): VesperDownloadRecoveredTaskPlan? =
        staleResourcePlanRecoverer?.recoverPlan(task, staleResource)

    private fun materializeGeneratedResources(
        assetId: VesperDownloadAssetId,
        taskId: VesperDownloadTaskId?,
        profile: VesperDownloadProfile,
        assetIndex: VesperDownloadAssetIndex,
    ): VesperDownloadAssetIndex =
        VesperGeneratedDownloadResourceMaterializer(
            baseDirectory = baseDirectory,
            fallbackBaseDirectory = appContext?.filesDir?.let { vesperDefaultDownloadBaseDirectory(it, null) },
        ).materialize(assetId, taskId, profile, assetIndex)

    private fun VesperDownloadTaskSnapshot.withRecoveredSource(): VesperDownloadTaskSnapshot {
        val recoveredSource =
            synchronized(recoveredSourcesLock) {
                recoveredSources[taskId]
            } ?: return this
        return copy(source = recoveredSource)
    }

    private suspend fun prepareAssetIndex(task: VesperDownloadTaskSnapshot): VesperDownloadAssetIndex {
        val requestHeaders = task.source.source.headers
        if (task.assetIndex.resources.isNotEmpty() || task.assetIndex.segments.isNotEmpty()) {
            return completePreparedAssetIndex(task.source.contentFormat, task.assetIndex, requestHeaders)
        }

        return when (task.source.contentFormat) {
            VesperDownloadContentFormat.HlsSegments -> planHlsAssetIndex(task, requestHeaders)
            VesperDownloadContentFormat.DashSegments -> planDashAssetIndex(task, requestHeaders)
            VesperDownloadContentFormat.FlvSegments -> planFlvAssetIndex(task, requestHeaders)
            VesperDownloadContentFormat.SingleFile -> planSingleFileAssetIndex(task, requestHeaders)
            VesperDownloadContentFormat.Unknown -> error("download preparation cannot plan an unknown content format")
        }
    }

    private suspend fun completePreparedAssetIndex(
        contentFormat: VesperDownloadContentFormat,
        assetIndex: VesperDownloadAssetIndex,
        requestHeaders: Map<String, String>,
    ): VesperDownloadAssetIndex {
        val resources =
            assetIndex.resources.map { resource ->
                if (resource.sizeBytes != null || resource.generatedText != null) {
                    resource
                } else {
                    resource.copy(sizeBytes = probeRequiredSize(resource.uri, resource.byteRange, requestHeaders))
                }
            }
        val segments =
            assetIndex.segments.map { segment ->
                if (segment.sizeBytes != null) {
                    segment
                } else {
                    segment.copy(sizeBytes = probeRequiredSize(segment.uri, segment.byteRange, requestHeaders))
                }
            }
        val totalSizeBytes =
            assetIndex.totalSizeBytes
                ?: resources.sumOf { resource -> if (resource.generatedText == null) resource.sizeBytes ?: 0L else 0L }
                    .let { resourceBytes -> resourceBytes + segments.sumOf { it.sizeBytes ?: 0L } }
        return assetIndex.copy(
            contentFormat = contentFormat,
            totalSizeBytes = totalSizeBytes,
            resources = resources,
            segments = segments,
        )
    }

    private suspend fun planSingleFileAssetIndex(
        task: VesperDownloadTaskSnapshot,
        requestHeaders: Map<String, String>,
    ): VesperDownloadAssetIndex {
        val uri = task.source.manifestUri ?: task.source.source.uri
        val sizeBytes = probeRequiredSize(uri, null, requestHeaders)
        return VesperDownloadAssetIndex(
            contentFormat = task.source.contentFormat,
            totalSizeBytes = sizeBytes,
            resources =
                listOf(
                    VesperDownloadResourceRecord(
                        resourceId = "single-file",
                        uri = uri,
                        relativePath = inferredFileName(uri),
                        sizeBytes = sizeBytes,
                    ),
                ),
        )
    }

    private suspend fun planHlsAssetIndex(
        task: VesperDownloadTaskSnapshot,
        requestHeaders: Map<String, String>,
    ): VesperDownloadAssetIndex {
        val manifestUri = task.source.manifestUri ?: task.source.source.uri
        val manifestText = fetchText(manifestUri, requestHeaders)
        return if (manifestText.contains("#EXT-X-STREAM-INF", ignoreCase = true)) {
            planHlsMasterAssetIndex(manifestUri, manifestText, task.profile, requestHeaders)
        } else {
            val media = parseHlsMediaPlaylist(manifestUri, manifestText)
            buildHlsMediaAssetIndex("index.m3u8", listOf("media" to media), requestHeaders)
        }
    }

    private suspend fun planHlsMasterAssetIndex(
        manifestUri: String,
        manifestText: String,
        profile: VesperDownloadProfile,
        requestHeaders: Map<String, String>,
    ): VesperDownloadAssetIndex {
        val master = parseHlsMasterPlaylist(manifestUri, manifestText)
        val variant =
            profile.variantId
                ?.let { variantId ->
                    master.variants.firstOrNull { it.uri == variantId || it.attributes["NAME"] == variantId }
                }
                ?: master.variants.firstOrNull()
                ?: error("HLS master playlist did not contain a playable variant")
        val variantMedia = parseHlsMediaPlaylist(variant.uri, fetchText(variant.uri, requestHeaders))
        val media = mutableListOf("video" to variantMedia)
        val audio =
            profile.preferredAudioLanguage
                ?.let { language ->
                    master.audio.firstOrNull { it.attributes["LANGUAGE"]?.equals(language, ignoreCase = true) == true }
                }
                ?: master.audio.firstOrNull { it.attributes["DEFAULT"]?.equals("YES", ignoreCase = true) == true }
                ?: master.audio.firstOrNull()
        if (audio != null) {
            media += "audio" to parseHlsMediaPlaylist(audio.uri, fetchText(audio.uri, requestHeaders))
        }

        val planned = buildHlsMediaAssetIndex("index.m3u8", media, requestHeaders)
        val mediaResourceNames =
            planned.resources
                .mapNotNull { it.relativePath }
                .filter { it.endsWith(".m3u8") && it != "index.m3u8" }
        val masterText = rewriteHlsMaster(variant.attributes, mediaResourceNames)
        return planned.copy(
            resources =
                planned.resources.map { resource ->
                    if (resource.resourceId == "hls-master") {
                        resource.copy(generatedText = masterText)
                    } else {
                        resource
                    }
                },
        )
    }

    private suspend fun buildHlsMediaAssetIndex(
        manifestPath: String,
        mediaPlaylists: List<Pair<String, HlsMediaPlaylist>>,
        requestHeaders: Map<String, String>,
    ): VesperDownloadAssetIndex {
        val resources =
            mutableListOf(
                VesperDownloadResourceRecord(
                    resourceId = "hls-master",
                    uri = "vesper-generated://hls/$manifestPath",
                    relativePath = manifestPath,
                ),
            )
        val segments = mutableListOf<VesperDownloadSegmentRecord>()
        val seenMaps = linkedSetOf<String>()
        var totalSizeBytes = 0L

        mediaPlaylists.forEach { (mediaId, playlist) ->
            val playlistPath =
                if (mediaPlaylists.size == 1 && manifestPath == "index.m3u8") {
                    "index.m3u8"
                } else {
                    "$mediaId.m3u8"
                }
            val localMaps = linkedMapOf<String, String>()
            playlist.maps.forEachIndexed { index, map ->
                val key = "${map.uri}:${map.byteRange}"
                if (seenMaps.add(key)) {
                    val size = probeRequiredSize(map.uri, map.byteRange, requestHeaders)
                    totalSizeBytes += size
                    val relativePath = "segments/$mediaId-init-$index.${extensionFromUri(map.uri, "mp4")}"
                    resources +=
                        VesperDownloadResourceRecord(
                            resourceId = "hls-$mediaId-init-$index",
                            uri = map.uri,
                            relativePath = relativePath,
                            byteRange = map.byteRange,
                            sizeBytes = size,
                        )
                    localMaps[key] = relativePath
                }
            }

            playlist.segments.forEach { segment ->
                val size = probeRequiredSize(segment.uri, segment.byteRange, requestHeaders)
                totalSizeBytes += size
                segments +=
                    VesperDownloadSegmentRecord(
                        segmentId = "hls-$mediaId-${segment.sequence}",
                        uri = segment.uri,
                        relativePath = "segments/$mediaId-${segment.sequence.toString().padStart(5, '0')}.${extensionFromUri(segment.uri, "ts")}",
                        sequence = segment.sequence,
                        byteRange = segment.byteRange,
                        sizeBytes = size,
                    )
            }

            val mediaText = rewriteHlsMedia(mediaId, playlist, localMaps)
            resources +=
                VesperDownloadResourceRecord(
                    resourceId = "hls-$mediaId-playlist",
                    uri = "vesper-generated://hls/$playlistPath",
                    relativePath = playlistPath,
                    generatedText = mediaText,
                )
        }

        if (mediaPlaylists.size == 1 && manifestPath == "index.m3u8") {
            val mediaResource = resources.firstOrNull { it.resourceId.endsWith("-playlist") }
            if (mediaResource != null) {
                resources.remove(mediaResource)
                resources[0] = resources[0].copy(generatedText = mediaResource.generatedText)
            }
        }

        return VesperDownloadAssetIndex(
            contentFormat = VesperDownloadContentFormat.HlsSegments,
            totalSizeBytes = totalSizeBytes,
            resources = resources,
            segments = segments,
        )
    }

    private suspend fun planDashAssetIndex(
        task: VesperDownloadTaskSnapshot,
        requestHeaders: Map<String, String>,
    ): VesperDownloadAssetIndex {
        val manifestUri = task.source.manifestUri ?: task.source.source.uri
        val manifestText = fetchText(manifestUri, requestHeaders)
        val document = parseXmlDocument(manifestText)
        val documentType = document.documentElement.getAttribute("type")
        if (documentType.isNotBlank() && !documentType.equals("static", ignoreCase = true)) {
            error("DASH download preparation requires a static MPD")
        }
        val durationSeconds = parseIso8601DurationSeconds(document.documentElement.getAttribute("mediaPresentationDuration"))
        val plannedRepresentations = selectDashRepresentations(document, manifestUri, task.profile)
        if (plannedRepresentations.isEmpty()) {
            error("DASH MPD did not contain a supported SegmentTemplate or SegmentBase representation")
        }

        val resources = mutableListOf<VesperDownloadResourceRecord>()
        val segments = mutableListOf<VesperDownloadSegmentRecord>()
        val rewrittenAdaptationSets = mutableListOf<String>()
        var totalSizeBytes = 0L
        var globalSequence = 1L

        plannedRepresentations.forEachIndexed { index, representation ->
            val mediaId = representation.mediaId.ifBlank { "media$index" }
            if (representation.template != null) {
                val template = representation.template
                if (template.duration <= 0L) {
                    error("DASH SegmentTemplate duration must be greater than zero")
                }
                val segmentSeconds = template.duration.toDouble() / template.timescale.coerceAtLeast(1L).toDouble()
                val segmentCount =
                    durationSeconds
                        ?.let { kotlin.math.ceil(it / segmentSeconds).toLong().coerceAtLeast(1L) }
                        ?: error("DASH SegmentTemplate planning requires a finite MPD duration")
                val initLocalPath = "segments/$mediaId-init.mp4"
                template.initialization?.takeIf { it.isNotBlank() }?.let { initialization ->
                    val remote = resolveRemoteReference(representation.baseUri, expandDashTemplate(initialization, representation.id, template.startNumber))
                    val size = probeRequiredSize(remote, null, requestHeaders)
                    totalSizeBytes += size
                    resources +=
                        VesperDownloadResourceRecord(
                            resourceId = "dash-$mediaId-init",
                            uri = remote,
                            relativePath = initLocalPath,
                            sizeBytes = size,
                        )
                }
                repeat(segmentCount.toInt()) { offset ->
                    val number = template.startNumber + offset
                    val remote = resolveRemoteReference(representation.baseUri, expandDashTemplate(template.media, representation.id, number))
                    val size = probeRequiredSize(remote, null, requestHeaders)
                    totalSizeBytes += size
                    segments +=
                        VesperDownloadSegmentRecord(
                            segmentId = "dash-$mediaId-segment-$number",
                            uri = remote,
                            relativePath = "segments/$mediaId-$number.m4s",
                            sequence = globalSequence++,
                            sizeBytes = size,
                        )
                }
                rewrittenAdaptationSets += rewriteDashTemplateAdaptationSet(representation, template, mediaId, segmentCount)
            } else if (representation.baseUrl != null) {
                val remote = resolveRemoteReference(representation.baseUri, representation.baseUrl)
                val size = probeRequiredSize(remote, null, requestHeaders)
                totalSizeBytes += size
                val localName = "media-$mediaId.${extensionFromUri(remote, "mp4")}"
                resources +=
                    VesperDownloadResourceRecord(
                        resourceId = "dash-$mediaId-media",
                        uri = remote,
                        relativePath = localName,
                        sizeBytes = size,
                    )
                rewrittenAdaptationSets += rewriteDashSegmentBaseAdaptationSet(representation, localName)
            }
        }

        resources.add(
            0,
            VesperDownloadResourceRecord(
                resourceId = "dash-manifest",
                uri = "vesper-generated://dash/manifest.mpd",
                relativePath = "manifest.mpd",
                generatedText = rewriteDashMpd(document.documentElement.getAttribute("mediaPresentationDuration"), rewrittenAdaptationSets),
            ),
        )

        return VesperDownloadAssetIndex(
            contentFormat = VesperDownloadContentFormat.DashSegments,
            totalSizeBytes = totalSizeBytes,
            resources = resources,
            segments = segments,
        )
    }

    private suspend fun planFlvAssetIndex(
        task: VesperDownloadTaskSnapshot,
        requestHeaders: Map<String, String>,
    ): VesperDownloadAssetIndex {
        val uri = task.source.manifestUri ?: task.source.source.uri
        val clipUris =
            if (extensionFromUri(uri, "flv").equals("flv", ignoreCase = true)) {
                listOf(uri)
            } else {
                parseFlvClipManifest(uri, fetchText(uri, requestHeaders))
            }
        if (clipUris.isEmpty()) {
            error("FLV clip manifest did not contain any clip URI")
        }

        var totalSizeBytes = 0L
        val concat = StringBuilder("ffconcat version 1.0\n")
        val segments =
            clipUris.mapIndexed { index, clipUri ->
                val sequence = index + 1L
                val size = probeRequiredSize(clipUri, null, requestHeaders)
                totalSizeBytes += size
                val localPath = "clips/clip-${sequence.toString().padStart(5, '0')}.${extensionFromUri(clipUri, "flv")}"
                concat.append("file '").append(escapeFfconcatPath(localPath)).append("'\n")
                VesperDownloadSegmentRecord(
                    segmentId = "flv-clip-$sequence",
                    uri = clipUri,
                    relativePath = localPath,
                    sequence = sequence,
                    sizeBytes = size,
                )
            }

        return VesperDownloadAssetIndex(
            contentFormat = VesperDownloadContentFormat.FlvSegments,
            totalSizeBytes = totalSizeBytes,
            resources =
                listOf(
                    VesperDownloadResourceRecord(
                        resourceId = "flv-concat",
                        uri = "vesper-generated://flv/manifest.ffconcat",
                        relativePath = "manifest.ffconcat",
                        generatedText = concat.toString(),
                    ),
                ),
            segments = segments,
        )
    }

    override fun prepare(
        task: VesperDownloadTaskSnapshot,
        reporter: VesperDownloadExecutionReporter,
    ) {
        scope.launch {
            try {
                reporter.completePreparation(task.taskId, prepareAssetIndexWithRecovery(task, reporter))
            } catch (error: Exception) {
                reporter.fail(
                    task.taskId,
                    VesperDownloadError(
                        code = VesperPlayerErrorCode.BackendFailure,
                        category = VesperPlayerErrorCategory.Network,
                        retriable = false,
                        message = error.message ?: "android download preparation failed",
                    ),
                )
            }
        }
    }

    override fun start(
        task: VesperDownloadTaskSnapshot,
        reporter: VesperDownloadExecutionReporter,
    ) {
        launchDownload(task.withRecoveredSource(), reporter)
    }

    override fun resume(
        task: VesperDownloadTaskSnapshot,
        reporter: VesperDownloadExecutionReporter,
    ) {
        launchDownload(task.withRecoveredSource(), reporter)
    }

    override fun pause(taskId: VesperDownloadTaskId) {
        synchronized(jobsLock) {
            jobs.remove(taskId)
        }?.cancel()
    }

    override fun remove(task: VesperDownloadTaskSnapshot?) {
        if (task != null) {
            pause(task.taskId)
            synchronized(recoveredSourcesLock) {
                recoveredSources.remove(task.taskId)
            }
            val completedPath = task.assetIndex.completedPath?.let(::File)
            when {
                completedPath?.isFile == true -> completedPath.delete()
                completedPath?.isDirectory == true -> completedPath.deleteRecursively()
                task.profile.targetDirectory != null -> File(task.profile.targetDirectory).deleteRecursively()
                else -> resolveDefaultAssetDirectory(task).deleteRecursively()
            }
            return
        }
    }

    override fun dispose() {
        val activeJobs =
            synchronized(jobsLock) {
                jobs.values.toList().also { jobs.clear() }
            }
        activeJobs.forEach(Job::cancel)
        scope.cancel()
    }

    private fun launchDownload(
        task: VesperDownloadTaskSnapshot,
        reporter: VesperDownloadExecutionReporter,
    ) {
        pause(task.taskId)
        val job =
            scope.launch {
                var receivedBytes = 0L
                var receivedSegments = 0
                var activeEntry: ForegroundDownloadEntry? = null
                try {
                    val downloadContext = currentCoroutineContext()
                    downloadContext.ensureActive()
                    val materializedTask =
                        task.copy(
                            assetIndex =
                                materializeGeneratedResources(
                                    assetId = task.assetId,
                                    taskId = task.taskId,
                                    profile = task.profile,
                                    assetIndex = task.assetIndex,
                                ),
                        )
                    val plan = buildExecutionPlan(materializedTask)
                    val requestHeaders = materializedTask.source.source.headers
                    val trackSegments = materializedTask.assetIndex.segments.isNotEmpty()
                    val progressThrottle = DownloadProgressThrottle(minProgressBytes, minProgressIntervalMs)

                    for ((index, entry) in plan.withIndex()) {
                        downloadContext.ensureActive()
                        activeEntry = entry
                        val outputFile = resolveOutputFile(materializedTask, entry, index)
                        outputFile.parentFile?.mkdirs()

                        val bytesWritten =
                            if (entry.generatedText != null) {
                                runInterruptible {
                                    outputFile.writeText(entry.generatedText)
                                }
                                0L
                            } else {
                                val resumeFromBytes =
                                    resumableExistingBytes(
                                        destination = outputFile,
                                        expectedSizeBytes = entry.expectedSizeBytes,
                                    )
                                copyUriToFile(
                                    sourceUri = entry.uri,
                                    byteRange = entry.byteRange,
                                    requestHeaders = requestHeaders,
                                    destination = outputFile,
                                    expectedSizeBytes = entry.expectedSizeBytes,
                                    resumeFromBytes = resumeFromBytes,
                                ) { writtenBytes ->
                                    downloadContext.ensureActive()
                                    val nextBytes = receivedBytes + writtenBytes
                                    if (progressThrottle.shouldReport(nextBytes)) {
                                        reporter.updateProgress(task.taskId, nextBytes, receivedSegments)
                                    }
                                }
                            }

                        downloadContext.ensureActive()
                        receivedBytes += bytesWritten
                        if (trackSegments && entry.isSegment) {
                            receivedSegments += 1
                        }
                        progressThrottle.markReported(receivedBytes)
                        reporter.updateProgress(task.taskId, receivedBytes, receivedSegments)
                    }

                    downloadContext.ensureActive()
                    synchronized(recoveredSourcesLock) {
                        recoveredSources.remove(task.taskId)
                    }
                    reporter.complete(task.taskId, resolveCompletedPath(materializedTask, plan))
                } catch (_: CancellationException) {
                    return@launch
                } catch (error: VesperStaleDownloadResourceException) {
                    try {
                        if (
                            recoverStaleDownload(
                                task = task,
                                staleError = error,
                                activeEntry = activeEntry,
                                receivedBytes = receivedBytes,
                                reporter = reporter,
                            )
                        ) {
                            return@launch
                        }
                    } catch (recoveryError: Exception) {
                        reporter.fail(
                            task.taskId,
                            VesperDownloadError(
                                code = VesperPlayerErrorCode.BackendFailure,
                                category = VesperPlayerErrorCategory.Network,
                                retriable = false,
                                message = recoveryError.message ?: "android download recovery failed",
                            ),
                        )
                        return@launch
                    }
                    reporter.fail(
                        task.taskId,
                        VesperDownloadError(
                            code = VesperPlayerErrorCode.BackendFailure,
                            category = VesperPlayerErrorCategory.Network,
                            retriable = false,
                            message = error.message ?: "android foreground download failed",
                        ),
                    )
                } catch (error: Exception) {
                    reporter.fail(
                        task.taskId,
                        VesperDownloadError(
                            code = VesperPlayerErrorCode.BackendFailure,
                            category = VesperPlayerErrorCategory.Network,
                            retriable = false,
                            message = error.message ?: "android foreground download failed",
                        ),
                    )
                } finally {
                    synchronized(jobsLock) {
                        jobs.remove(task.taskId)
                    }
                }
            }

        synchronized(jobsLock) {
            jobs[task.taskId] = job
        }
    }

    private suspend fun recoverStaleDownload(
        task: VesperDownloadTaskSnapshot,
        staleError: VesperStaleDownloadResourceException,
        activeEntry: ForegroundDownloadEntry?,
        receivedBytes: Long,
        reporter: VesperDownloadExecutionReporter,
    ): Boolean {
        val staleResource =
            staleError.toStaleResource(
                taskId = task.taskId,
                fallbackResourceId = activeEntry?.resourceId,
                fallbackSegmentId = activeEntry?.segmentId,
                fallbackUri = activeEntry?.uri,
                fallbackPhase = VesperDownloadStaleResourcePhase.Download,
                fallbackReceivedBytes = receivedBytes,
            )
        val recoveredPlan = recoverTaskPlan(task, staleResource) ?: return false
        pause(task.taskId)
        runInterruptible { resolveBaseDirectory(task).deleteRecursively() }
        val recoveredAssetIndex =
            materializeGeneratedResources(
                assetId = task.assetId,
                taskId = task.taskId,
                profile = recoveredPlan.profile,
                assetIndex = recoveredPlan.assetIndex,
            )
        reporter.replaceTaskPlan(task.taskId, recoveredPlan.source, recoveredPlan.profile, recoveredAssetIndex)
        val recoveredTask =
            task.copy(
                source = recoveredPlan.source,
                profile = recoveredPlan.profile,
                state = VesperDownloadState.Preparing,
                progress = VesperDownloadProgressSnapshot(),
                assetIndex = recoveredAssetIndex,
                error = null,
            )
        val preparedAssetIndex =
            materializeGeneratedResources(
                assetId = task.assetId,
                taskId = task.taskId,
                profile = recoveredPlan.profile,
                assetIndex = prepareAssetIndex(recoveredTask),
            )
        reporter.completePreparation(task.taskId, preparedAssetIndex)
        return true
    }

    private fun buildExecutionPlan(task: VesperDownloadTaskSnapshot): List<ForegroundDownloadEntry> {
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

    private fun resolveOutputFile(
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

    private fun resolveCompletedPath(
        task: VesperDownloadTaskSnapshot,
        plan: List<ForegroundDownloadEntry>,
    ): String =
        if (plan.size == 1) {
            resolveOutputFile(task, plan.single(), 0).absolutePath
        } else {
            resolveBaseDirectory(task).absolutePath
        }

    private fun resolveBaseDirectory(task: VesperDownloadTaskSnapshot): File =
        task.profile.targetDirectory
            ?.takeIf { it.isNotBlank() }
            ?.let(::File)
            ?: resolveDefaultAssetDirectory(task)

    private fun resolveDefaultAssetDirectory(task: VesperDownloadTaskSnapshot): File =
        File(
            baseDirectory
                ?: vesperDefaultDownloadBaseDirectory(
                    checkNotNull(appContext) { "Android Context is required when no download base directory is configured" }.filesDir,
                    null,
                ),
            task.assetId.ifBlank { task.taskId.toString() },
        )

    private suspend fun copyUriToFile(
        sourceUri: String,
        byteRange: VesperDownloadByteRange?,
        requestHeaders: Map<String, String>,
        destination: File,
        expectedSizeBytes: Long?,
        resumeFromBytes: Long,
        onProgress: (Long) -> Unit,
    ): Long {
        if (isHttpUri(sourceUri)) {
            return copyHttpUriToFile(
                sourceUri = sourceUri,
                byteRange = byteRange,
                requestHeaders = requestHeaders,
                destination = destination,
                expectedSizeBytes = expectedSizeBytes,
                resumeFromBytes = resumeFromBytes,
                allowRestartAfterRangeMismatch = true,
                onProgress = onProgress,
            )
        }
        if (uriScheme(sourceUri).equals("file", ignoreCase = true)) {
            return copyLocalFileUriToFile(
                sourceUri = sourceUri,
                byteRange = byteRange,
                destination = destination,
                expectedSizeBytes = expectedSizeBytes,
                resumeFromBytes = resumeFromBytes,
                onProgress = onProgress,
            )
        }
        return copyDataSourceUriToFile(
            sourceUri = sourceUri,
            byteRange = byteRange,
            requestHeaders = requestHeaders,
            destination = destination,
            expectedSizeBytes = expectedSizeBytes,
            resumeFromBytes = resumeFromBytes,
            onProgress = onProgress,
        )
    }

    private suspend fun copyLocalFileUriToFile(
        sourceUri: String,
        byteRange: VesperDownloadByteRange?,
        destination: File,
        expectedSizeBytes: Long?,
        resumeFromBytes: Long,
        onProgress: (Long) -> Unit,
    ): Long {
        val copyContext = currentCoroutineContext()
        val expected = expectedSizeBytes?.coerceAtLeast(0L)
        val resumeOffset = resumeFromBytes.coerceAtLeast(0L)
        if (expected != null && resumeOffset >= expected) {
            return expected
        }
        val sourceFile = File(URI(sourceUri))
        val startOffset = (byteRange?.offset ?: 0L).coerceAtLeast(0L) + resumeOffset
        var remaining = byteRange?.let { (it.length.coerceAtLeast(0L) - resumeOffset).coerceAtLeast(0L) }
        var totalWritten = resumeOffset
        var reportedBytes = resumeOffset
        runInterruptible {
            sourceFile.inputStream().use { input ->
                if (startOffset > 0L) {
                    input.skip(startOffset)
                }
                FileOutputStream(destination, resumeOffset > 0L).use { output ->
                    val buffer = ByteArray(64 * 1024)
                    while (remaining == null || remaining!! > 0L) {
                        copyContext.ensureActive()
                        val limit = minOf(buffer.size.toLong(), remaining ?: buffer.size.toLong()).toInt()
                        val read = input.read(buffer, 0, limit)
                        if (read == -1) {
                            break
                        }
                        output.write(buffer, 0, read)
                        totalWritten += read.toLong()
                        remaining = remaining?.let { (it - read).coerceAtLeast(0L) }
                        if (totalWritten - reportedBytes >= minProgressBytes.coerceAtLeast(1L)) {
                            reportedBytes = totalWritten
                            onProgress(totalWritten)
                        }
                    }
                }
            }
        }
        if (expected != null && totalWritten != expected) {
            error("copied $totalWritten bytes for $sourceUri, expected $expected")
        }
        return totalWritten
    }

    private suspend fun copyDataSourceUriToFile(
        sourceUri: String,
        byteRange: VesperDownloadByteRange?,
        requestHeaders: Map<String, String>,
        destination: File,
        expectedSizeBytes: Long?,
        resumeFromBytes: Long,
        onProgress: (Long) -> Unit,
    ): Long {
        val copyContext = currentCoroutineContext()
        val expected = expectedSizeBytes?.coerceAtLeast(0L)
        val resumeOffset = resumeFromBytes.coerceAtLeast(0L)
        if (expected != null && resumeOffset >= expected) {
            return expected
        }
        val dataSource = dataSourceFactory.createDataSource()
        val dataSpecBuilder = DataSpec.Builder()
            .setUri(sourceUri)
            .setDownloadRequestHeaders(requestHeaders)
        if (byteRange != null) {
            val remaining = byteRange.length.coerceAtLeast(0L) - resumeOffset
            dataSpecBuilder
                .setPosition(byteRange.offset.coerceAtLeast(0L) + resumeOffset)
                .setLength(remaining.coerceAtLeast(0L))
        } else if (resumeOffset > 0L) {
            dataSpecBuilder.setPosition(resumeOffset)
        }
        val dataSpec = dataSpecBuilder.build()
        var totalWritten = resumeOffset
        var reportedBytes = resumeOffset
        FileOutputStream(destination, resumeOffset > 0L).use { output ->
            try {
                copyContext.ensureActive()
                runInterruptible {
                    dataSource.open(dataSpec)
                }
                val buffer = ByteArray(64 * 1024)
                while (true) {
                    copyContext.ensureActive()
                    val read =
                        runInterruptible {
                            dataSource.read(buffer, 0, buffer.size)
                        }
                    if (read == -1) {
                        break
                    }
                    copyContext.ensureActive()
                    runInterruptible {
                        output.write(buffer, 0, read)
                    }
                    totalWritten += read.toLong()
                    if (expected != null && totalWritten > expected) {
                        runCatching { destination.delete() }
                        error("remote server did not honor the requested resume range for $sourceUri")
                    }
                    if (totalWritten - reportedBytes >= minProgressBytes.coerceAtLeast(1L)) {
                        copyContext.ensureActive()
                        reportedBytes = totalWritten
                        onProgress(totalWritten)
                    }
                }
            } finally {
                closeDataSourceQuietly(dataSource, "copy $sourceUri")
            }
        }
        if (expected != null && totalWritten != expected) {
            error("downloaded ${totalWritten} bytes for $sourceUri, expected $expected")
        }
        return totalWritten
    }

    private suspend fun copyHttpUriToFile(
        sourceUri: String,
        byteRange: VesperDownloadByteRange?,
        requestHeaders: Map<String, String>,
        destination: File,
        expectedSizeBytes: Long?,
        resumeFromBytes: Long,
        allowRestartAfterRangeMismatch: Boolean,
        onProgress: (Long) -> Unit,
    ): Long {
        val copyContext = currentCoroutineContext()
        val expected = expectedSizeBytes?.coerceAtLeast(0L)
        val resumeOffset = resumeFromBytes.coerceAtLeast(0L)
        if (expected != null && resumeOffset >= expected) {
            return expected
        }
        if (byteRange == null && expected != null && expected > 0L && rangeChunkBytes != null) {
            return copyKnownSizeHttpUriToFile(
                sourceUri = sourceUri,
                requestHeaders = requestHeaders,
                destination = destination,
                expectedSizeBytes = expected,
                resumeFromBytes = resumeOffset,
                rangeChunkBytes = rangeChunkBytes,
                allowRestartAfterRangeMismatch = allowRestartAfterRangeMismatch,
                onProgress = onProgress,
            )
        }

        val rangeHeader = requestedHttpRangeHeader(byteRange, resumeOffset)
        val requestedRangeStart = requestedHttpRangeStart(byteRange, resumeOffset)
        val connection =
            runInterruptible {
                (URL(sourceUri).openConnection() as HttpURLConnection).apply {
                    applyDownloadRequestHeaders(requestHeaders)
                    rangeHeader?.let { setRequestProperty("Range", it) }
                    instanceFollowRedirects = true
                    connectTimeout = ANDROID_DOWNLOAD_PREPARE_TIMEOUT_MS
                    readTimeout = ANDROID_DOWNLOAD_PREPARE_TIMEOUT_MS
                }
            }

        try {
            val status = runInterruptible { connection.responseCode }
            when {
                status == HttpURLConnection.HTTP_PARTIAL -> {
                    val contentRangeStart = parseHttpContentRangeStart(connection.getHeaderField("Content-Range"))
                    if (requestedRangeStart == null || contentRangeStart != requestedRangeStart) {
                        throw staleDownloadResource(
                            "remote server returned an unexpected Content-Range for $sourceUri",
                        )
                    }
                }
                status == HttpURLConnection.HTTP_OK -> {
                    if (requestedRangeStart != null) {
                        if (byteRange == null && resumeOffset > 0L && allowRestartAfterRangeMismatch) {
                            connection.disconnect()
                            runInterruptible { destination.delete() }
                            onProgress(0L)
                            return copyHttpUriToFile(
                                sourceUri = sourceUri,
                                byteRange = byteRange,
                                requestHeaders = requestHeaders,
                                destination = destination,
                                expectedSizeBytes = expectedSizeBytes,
                                resumeFromBytes = 0L,
                                allowRestartAfterRangeMismatch = false,
                                onProgress = onProgress,
                            )
                        }
                        throw staleDownloadResource(
                            "remote server did not honor the requested byte range for $sourceUri",
                        )
                    }
                }
                status == ANDROID_HTTP_RANGE_NOT_SATISFIABLE -> {
                    if (resumeOffset > 0L && allowRestartAfterRangeMismatch) {
                        connection.disconnect()
                        runInterruptible { destination.delete() }
                        onProgress(0L)
                        return copyHttpUriToFile(
                            sourceUri = sourceUri,
                            byteRange = byteRange,
                            requestHeaders = requestHeaders,
                            destination = destination,
                            expectedSizeBytes = expectedSizeBytes,
                            resumeFromBytes = 0L,
                            allowRestartAfterRangeMismatch = false,
                            onProgress = onProgress,
                        )
                    }
                    throw staleDownloadResource(
                        "remote resource rejected the requested byte range for $sourceUri",
                    )
                }
                isExpiredHttpStatus(status) -> {
                    throw staleDownloadResource(
                        "offline download resource is stale or expired (HTTP $status) for $sourceUri; refresh the media link and prepare the task again",
                    )
                }
                status !in 200..299 -> {
                    throw staleDownloadResource("remote resource returned HTTP $status for $sourceUri")
                }
            }

            var totalWritten = resumeOffset
            var reportedBytes = resumeOffset
            val append = status == HttpURLConnection.HTTP_PARTIAL && resumeOffset > 0L
            FileOutputStream(destination, append).use { output ->
                val input =
                    runInterruptible {
                        connection.inputStream
                    }
                input.use { stream ->
                    val buffer = ByteArray(64 * 1024)
                    while (true) {
                        copyContext.ensureActive()
                        val read =
                            runInterruptible {
                                stream.read(buffer, 0, buffer.size)
                            }
                        if (read == -1) {
                            break
                        }
                        copyContext.ensureActive()
                        runInterruptible {
                            output.write(buffer, 0, read)
                        }
                        totalWritten += read.toLong()
                        if (expected != null && totalWritten > expected) {
                            runInterruptible { destination.delete() }
                            throw staleDownloadResource(
                                "remote server sent more bytes than expected for $sourceUri",
                            )
                        }
                        if (totalWritten - reportedBytes >= minProgressBytes.coerceAtLeast(1L)) {
                            copyContext.ensureActive()
                            reportedBytes = totalWritten
                            onProgress(totalWritten)
                        }
                    }
                }
            }
            if (expected != null && totalWritten != expected) {
                error("downloaded ${totalWritten} bytes for $sourceUri, expected $expected")
            }
            return totalWritten
        } finally {
            connection.disconnect()
        }
    }

    private fun resumableExistingBytes(
        destination: File,
        expectedSizeBytes: Long?,
    ): Long {
        if (!destination.exists()) {
            return 0L
        }
        if (!resumePartialDownloads) {
            destination.delete()
            return 0L
        }
        val expected = expectedSizeBytes?.coerceAtLeast(0L)
        if (expected == null) {
            destination.delete()
            return 0L
        }
        val existing = destination.length().coerceAtLeast(0L)
        return when {
            existing == expected -> existing
            existing in 1 until expected -> existing
            else -> {
                destination.delete()
                0L
            }
        }
    }

    private suspend fun copyKnownSizeHttpUriToFile(
        sourceUri: String,
        requestHeaders: Map<String, String>,
        destination: File,
        expectedSizeBytes: Long,
        resumeFromBytes: Long,
        rangeChunkBytes: Long,
        allowRestartAfterRangeMismatch: Boolean,
        onProgress: (Long) -> Unit,
    ): Long {
        val expected = expectedSizeBytes.coerceAtLeast(0L)
        var offset = resumeFromBytes.coerceAtLeast(0L)
        if (offset >= expected) {
            return expected
        }
        while (offset < expected) {
            val chunkLength = minOf(rangeChunkBytes, expected - offset)
            val chunkEnd = offset + chunkLength - 1L
            val nextOffset =
                copyHttpUriRangeChunkToFile(
                    sourceUri = sourceUri,
                    requestHeaders = requestHeaders,
                    destination = destination,
                    expectedSizeBytes = expected,
                    rangeStart = offset,
                    rangeEndInclusive = chunkEnd,
                    rangeChunkBytes = rangeChunkBytes,
                    allowRestartAfterRangeMismatch = allowRestartAfterRangeMismatch,
                    onProgress = onProgress,
                )
            check(nextOffset > offset) { "download range transfer did not advance for $sourceUri" }
            offset = nextOffset
        }
        return offset
    }

    private suspend fun copyHttpUriRangeChunkToFile(
        sourceUri: String,
        requestHeaders: Map<String, String>,
        destination: File,
        expectedSizeBytes: Long,
        rangeStart: Long,
        rangeEndInclusive: Long,
        rangeChunkBytes: Long,
        allowRestartAfterRangeMismatch: Boolean,
        onProgress: (Long) -> Unit,
    ): Long {
        val copyContext = currentCoroutineContext()
        val connection =
            runInterruptible {
                (URL(sourceUri).openConnection() as HttpURLConnection).apply {
                    applyDownloadRequestHeaders(requestHeaders)
                    setRequestProperty("Range", "bytes=$rangeStart-$rangeEndInclusive")
                    instanceFollowRedirects = true
                    connectTimeout = ANDROID_DOWNLOAD_PREPARE_TIMEOUT_MS
                    readTimeout = ANDROID_DOWNLOAD_PREPARE_TIMEOUT_MS
                }
            }

        try {
            val status = runInterruptible { connection.responseCode }
            val chunkCoversWholeResource = rangeStart == 0L && rangeEndInclusive + 1L >= expectedSizeBytes
            when {
                status == HttpURLConnection.HTTP_PARTIAL -> {
                    val contentRangeStart = parseHttpContentRangeStart(connection.getHeaderField("Content-Range"))
                    if (contentRangeStart != rangeStart) {
                        throw staleDownloadResource(
                            "remote server returned an unexpected Content-Range for $sourceUri",
                        )
                    }
                }
                status == HttpURLConnection.HTTP_OK -> {
                    if (!chunkCoversWholeResource) {
                        if (rangeStart > 0L && allowRestartAfterRangeMismatch) {
                            connection.disconnect()
                            runInterruptible { destination.delete() }
                            onProgress(0L)
                            return copyKnownSizeHttpUriToFile(
                                sourceUri = sourceUri,
                                requestHeaders = requestHeaders,
                                destination = destination,
                                expectedSizeBytes = expectedSizeBytes,
                                resumeFromBytes = 0L,
                                rangeChunkBytes = rangeChunkBytes,
                                allowRestartAfterRangeMismatch = false,
                                onProgress = onProgress,
                            )
                        }
                        throw staleDownloadResource(
                            "remote server did not honor the requested byte range for $sourceUri",
                        )
                    }
                }
                status == ANDROID_HTTP_RANGE_NOT_SATISFIABLE -> {
                    if (rangeStart > 0L && allowRestartAfterRangeMismatch) {
                        connection.disconnect()
                        runInterruptible { destination.delete() }
                        onProgress(0L)
                        return copyKnownSizeHttpUriToFile(
                            sourceUri = sourceUri,
                            requestHeaders = requestHeaders,
                            destination = destination,
                            expectedSizeBytes = expectedSizeBytes,
                            resumeFromBytes = 0L,
                            rangeChunkBytes = rangeChunkBytes,
                            allowRestartAfterRangeMismatch = false,
                            onProgress = onProgress,
                        )
                    }
                    throw staleDownloadResource(
                        "remote resource rejected the requested byte range for $sourceUri",
                    )
                }
                isExpiredHttpStatus(status) -> {
                    throw staleDownloadResource(
                        "offline download resource is stale or expired (HTTP $status) for $sourceUri; refresh the media link and prepare the task again",
                    )
                }
                status !in 200..299 -> {
                    throw staleDownloadResource("remote resource returned HTTP $status for $sourceUri")
                }
            }

            val append = status == HttpURLConnection.HTTP_PARTIAL && rangeStart > 0L
            var totalWritten = if (append) rangeStart else 0L
            var reportedBytes = totalWritten
            FileOutputStream(destination, append).use { output ->
                val input =
                    runInterruptible {
                        connection.inputStream
                    }
                input.use { stream ->
                    val buffer = ByteArray(64 * 1024)
                    while (true) {
                        copyContext.ensureActive()
                        val read =
                            runInterruptible {
                                stream.read(buffer, 0, buffer.size)
                            }
                        if (read == -1) {
                            break
                        }
                        copyContext.ensureActive()
                        runInterruptible {
                            output.write(buffer, 0, read)
                        }
                        totalWritten += read.toLong()
                        if (totalWritten > expectedSizeBytes) {
                            runInterruptible { destination.delete() }
                            throw staleDownloadResource(
                                "remote server sent more bytes than expected for $sourceUri",
                            )
                        }
                        if (status == HttpURLConnection.HTTP_PARTIAL && totalWritten > rangeEndInclusive + 1L) {
                            runInterruptible { destination.delete() }
                            throw staleDownloadResource(
                                "remote server sent more bytes than the requested byte range for $sourceUri",
                            )
                        }
                        if (totalWritten - reportedBytes >= minProgressBytes.coerceAtLeast(1L)) {
                            copyContext.ensureActive()
                            reportedBytes = totalWritten
                            onProgress(totalWritten)
                        }
                    }
                }
            }

            return if (status == HttpURLConnection.HTTP_PARTIAL) {
                val expectedNextOffset = rangeEndInclusive + 1L
                if (totalWritten != expectedNextOffset) {
                    throw staleDownloadResource(
                        "downloaded range ended at $totalWritten for $sourceUri, expected $expectedNextOffset",
                    )
                }
                totalWritten
            } else {
                if (totalWritten != expectedSizeBytes) {
                    throw staleDownloadResource(
                        "downloaded $totalWritten bytes for $sourceUri, expected $expectedSizeBytes",
                    )
                }
                totalWritten
            }
        } finally {
            connection.disconnect()
        }
    }

    private suspend fun fetchText(
        sourceUri: String,
        requestHeaders: Map<String, String>,
    ): String {
        if (isHttpUri(sourceUri)) {
            return fetchHttpText(sourceUri, requestHeaders)
        }
        val dataSource = dataSourceFactory.createDataSource()
        return try {
            runInterruptible {
                dataSource.open(
                    DataSpec.Builder()
                        .setUri(sourceUri)
                        .setDownloadRequestHeaders(requestHeaders)
                        .build(),
                )
            }
            val output = ByteArrayOutputStream()
            val buffer = ByteArray(32 * 1024)
            while (true) {
                val read =
                    runInterruptible {
                        dataSource.read(buffer, 0, buffer.size)
                    }
                if (read == -1) {
                    break
                }
                output.write(buffer, 0, read)
            }
            output.toString(Charsets.UTF_8.name())
        } finally {
            closeDataSourceQuietly(dataSource, "fetch text $sourceUri")
        }
    }

    private suspend fun probeRequiredSize(
        sourceUri: String,
        byteRange: VesperDownloadByteRange?,
        requestHeaders: Map<String, String>,
    ): Long {
        if (byteRange != null) {
            return byteRange.length.coerceAtLeast(0L)
        }
        return probeContentLength(sourceUri, requestHeaders)
    }

    private suspend fun probeContentLength(
        sourceUri: String,
        requestHeaders: Map<String, String>,
    ): Long {
        val scheme = uriScheme(sourceUri)
        if (scheme.equals("file", ignoreCase = true)) {
            val fileSize = runCatching { URI(sourceUri).path?.let(::File)?.length() }.getOrNull() ?: 0L
            if (fileSize > 0L) {
                return fileSize
            }
        }
        if (scheme.equals("http", ignoreCase = true) || scheme.equals("https", ignoreCase = true)) {
            probeHttpContentLength(sourceUri, requestHeaders)?.let { return it }
        }

        val dataSource = dataSourceFactory.createDataSource()
        return try {
            val length =
                runInterruptible {
                    dataSource.open(
                        DataSpec.Builder()
                            .setUri(sourceUri)
                            .setDownloadRequestHeaders(requestHeaders)
                            .build(),
                    )
                }
            if (length <= 0L) {
                error("remote resource did not expose a stable content length")
            }
            length
        } finally {
            closeDataSourceQuietly(dataSource, "probe content length $sourceUri")
        }
    }

    private suspend fun fetchHttpText(
        sourceUri: String,
        requestHeaders: Map<String, String>,
    ): String =
        runInterruptible {
            val connection = (URL(sourceUri).openConnection() as HttpURLConnection).apply {
                applyDownloadRequestHeaders(requestHeaders)
                instanceFollowRedirects = true
                connectTimeout = ANDROID_DOWNLOAD_PREPARE_TIMEOUT_MS
                readTimeout = ANDROID_DOWNLOAD_PREPARE_TIMEOUT_MS
            }
            try {
                val status = connection.responseCode
                if (isExpiredHttpStatus(status)) {
                    throw staleDownloadResource(
                        "offline download resource is stale or expired (HTTP $status) for $sourceUri; refresh the media link and prepare the task again",
                    )
                }
                if (status !in 200..299) {
                    throw staleDownloadResource("remote resource returned HTTP $status for $sourceUri")
                }
                connection.inputStream.use { input ->
                    input.readBytes().toString(Charsets.UTF_8)
                }
            } finally {
                connection.disconnect()
            }
        }

    private suspend fun probeHttpContentLength(
        sourceUri: String,
        requestHeaders: Map<String, String>,
    ): Long? =
        runInterruptible {
            val head = (URL(sourceUri).openConnection() as HttpURLConnection).apply {
                applyDownloadRequestHeaders(requestHeaders)
                requestMethod = "HEAD"
                instanceFollowRedirects = true
                connectTimeout = ANDROID_DOWNLOAD_PREPARE_TIMEOUT_MS
                readTimeout = ANDROID_DOWNLOAD_PREPARE_TIMEOUT_MS
            }
            val headStatus = head.responseCode
            try {
                head.inputStream.close()
            } catch (error: CancellationException) {
                throw error
            } catch (error: InterruptedException) {
                Thread.currentThread().interrupt()
                throw error
            } catch (error: Exception) {
                Log.w(DOWNLOAD_TAG, "failed to close HEAD probe stream for $sourceUri", error)
                runCatching { head.errorStream?.close() }
                    .onFailure { closeError ->
                        Log.w(DOWNLOAD_TAG, "failed to close HEAD probe error stream for $sourceUri", closeError)
                    }
            }
            if (isExpiredHttpStatus(headStatus)) {
                head.disconnect()
                throw staleDownloadResource(
                    "offline download resource is stale or expired (HTTP $headStatus) for $sourceUri; refresh the media link and prepare the task again",
                )
            }
            head.getHeaderField("Content-Length")?.toLongOrNull()?.takeIf { it > 0L }?.let {
                head.disconnect()
                return@runInterruptible it
            }
            head.disconnect()

            val range = (URL(sourceUri).openConnection() as HttpURLConnection).apply {
                applyDownloadRequestHeaders(requestHeaders)
                setRequestProperty("Range", "bytes=0-0")
                instanceFollowRedirects = true
                connectTimeout = ANDROID_DOWNLOAD_PREPARE_TIMEOUT_MS
                readTimeout = ANDROID_DOWNLOAD_PREPARE_TIMEOUT_MS
            }
            val rangeStatus = range.responseCode
            try {
                range.inputStream.close()
            } catch (error: CancellationException) {
                throw error
            } catch (error: InterruptedException) {
                Thread.currentThread().interrupt()
                throw error
            } catch (error: Exception) {
                Log.w(DOWNLOAD_TAG, "failed to close range probe stream for $sourceUri", error)
                runCatching { range.errorStream?.close() }
                    .onFailure { closeError ->
                        Log.w(DOWNLOAD_TAG, "failed to close range probe error stream for $sourceUri", closeError)
                    }
            }
            if (isExpiredHttpStatus(rangeStatus)) {
                range.disconnect()
                throw staleDownloadResource(
                    "offline download resource is stale or expired (HTTP $rangeStatus) for $sourceUri; refresh the media link and prepare the task again",
                )
            }
            val size =
                range.getHeaderField("Content-Range")
                    ?.substringAfterLast('/', "")
                    ?.toLongOrNull()
                    ?.takeIf { it > 0L }
            range.disconnect()
            size
        }

    private fun inferredFileName(uri: String): String =
        lastPathSegmentFromUri(uri) ?: "media.bin"
}

private data class ForegroundDownloadEntry(
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

