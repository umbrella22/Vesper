package io.github.ikaros.vesper.player.android

import kotlinx.coroutines.runInterruptible

internal suspend fun VesperForegroundDownloadExecutor.prepareAssetIndexWithRecovery(
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

internal suspend fun VesperForegroundDownloadExecutor.recoverTaskPlan(
    task: VesperDownloadTaskSnapshot,
    staleResource: VesperDownloadStaleResource,
): VesperDownloadRecoveredTaskPlan? =
    staleResourcePlanRecoverer?.recoverPlan(task, staleResource)

internal fun VesperForegroundDownloadExecutor.materializeGeneratedResources(
    assetId: VesperDownloadAssetId,
    taskId: VesperDownloadTaskId?,
    profile: VesperDownloadProfile,
    assetIndex: VesperDownloadAssetIndex,
): VesperDownloadAssetIndex =
    VesperGeneratedDownloadResourceMaterializer(
        baseDirectory = baseDirectory,
        fallbackBaseDirectory = appContext?.filesDir?.let { vesperDefaultDownloadBaseDirectory(it, null) },
    ).materialize(assetId, taskId, profile, assetIndex)

internal fun VesperForegroundDownloadExecutor.withRecoveredSource(task: VesperDownloadTaskSnapshot): VesperDownloadTaskSnapshot {
    val recoveredSource =
        synchronized(recoveredSourcesLock) {
            recoveredSources[task.taskId]
        } ?: return task
    return task.copy(source = recoveredSource)
}

internal suspend fun VesperForegroundDownloadExecutor.prepareAssetIndex(task: VesperDownloadTaskSnapshot): VesperDownloadAssetIndex {
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

internal suspend fun VesperForegroundDownloadExecutor.completePreparedAssetIndex(
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

internal suspend fun VesperForegroundDownloadExecutor.planSingleFileAssetIndex(
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

internal suspend fun VesperForegroundDownloadExecutor.planHlsAssetIndex(
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

internal suspend fun VesperForegroundDownloadExecutor.planHlsMasterAssetIndex(
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

internal suspend fun VesperForegroundDownloadExecutor.buildHlsMediaAssetIndex(
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

internal suspend fun VesperForegroundDownloadExecutor.planDashAssetIndex(
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

internal suspend fun VesperForegroundDownloadExecutor.planFlvAssetIndex(
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

internal suspend fun VesperForegroundDownloadExecutor.recoverStaleDownload(
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
