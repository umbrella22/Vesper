import Foundation

public final class VesperForegroundDownloadExecutor: VesperDownloadExecutor {
    private let lock = NSLock()
    private let fileManager = FileManager.default
    private var tasks: [VesperDownloadTaskId: Task<Void, Never>] = [:]
    private var recoveredSources: [VesperDownloadTaskId: VesperDownloadSource] = [:]
    private let baseDirectory: URL?
    private let resumePartialDownloads: Bool
    private let rangeChunkBytes: UInt64?
    private let minProgressBytes: UInt64
    private let minProgressIntervalMs: UInt64
    private let stalledTransferTimeoutMs: UInt64
    private let staleResourceRecoveryHandler: VesperDownloadStaleResourceRecoveryHandler?
    private let staleResourcePlanRecoveryHandler: VesperDownloadStaleResourcePlanRecoveryHandler?

    public init(
        baseDirectory: URL? = nil,
        resumePartialDownloads: Bool = true,
        rangeChunkBytes: UInt64? = nil,
        minProgressBytes: UInt64 = vesperDownloadDefaultMinProgressBytes,
        minProgressIntervalMs: UInt64 = vesperDownloadDefaultMinProgressIntervalMs,
        stalledTransferTimeoutMs: UInt64 = vesperDownloadDefaultStalledTransferTimeoutMs,
        staleResourceRecoveryHandler: VesperDownloadStaleResourceRecoveryHandler? = nil,
        staleResourcePlanRecoveryHandler: VesperDownloadStaleResourcePlanRecoveryHandler? = nil
    ) {
        self.baseDirectory = baseDirectory
        self.resumePartialDownloads = resumePartialDownloads
        self.rangeChunkBytes = rangeChunkBytes.flatMap { $0 > 0 ? $0 : nil }
        self.minProgressBytes = max(minProgressBytes, 1)
        self.minProgressIntervalMs = minProgressIntervalMs
        self.stalledTransferTimeoutMs = stalledTransferTimeoutMs
        self.staleResourceRecoveryHandler = staleResourceRecoveryHandler
        self.staleResourcePlanRecoveryHandler = staleResourcePlanRecoveryHandler
    }

    private func prepareAssetIndexWithRecovery(
        task: VesperDownloadTaskSnapshot,
        reporter: any VesperDownloadExecutionReporter
    ) async throws -> VesperDownloadAssetIndex {
        do {
            let assetIndex = try await prepareAssetIndex(task: task)
            return try materializeGeneratedResources(
                assetId: task.assetId,
                taskId: task.taskId,
                profile: task.profile,
                assetIndex: assetIndex
            )
        } catch let error as VesperStaleDownloadResourceError {
            let staleResource = error.staleResource(taskId: task.taskId, phase: .prepare)
            guard let recoveredPlan = await recoverTaskPlan(task: task, staleResource: staleResource) else {
                throw error
            }
            let materializedRecoveredIndex = try materializeGeneratedResources(
                assetId: task.assetId,
                taskId: task.taskId,
                profile: recoveredPlan.profile,
                assetIndex: recoveredPlan.assetIndex
            )
            let recoveredTask = VesperDownloadTaskSnapshot(
                taskId: task.taskId,
                assetId: task.assetId,
                source: recoveredPlan.source,
                profile: recoveredPlan.profile,
                state: task.state,
                progress: task.progress,
                assetIndex: materializedRecoveredIndex,
                error: task.error
            )
            await reporter.replaceTaskPlan(
                taskId: task.taskId,
                source: recoveredPlan.source,
                profile: recoveredPlan.profile,
                assetIndex: materializedRecoveredIndex
            )
            let assetIndex = try await prepareAssetIndex(task: recoveredTask)
            let materializedAssetIndex = try materializeGeneratedResources(
                assetId: task.assetId,
                taskId: task.taskId,
                profile: recoveredPlan.profile,
                assetIndex: assetIndex
            )
            storeRecoveredSource(recoveredPlan.source, forTaskId: task.taskId)
            return materializedAssetIndex
        }
    }

    private func recoverTaskPlan(
        task: VesperDownloadTaskSnapshot,
        staleResource: VesperDownloadStaleResource
    ) async -> VesperDownloadRecoveredTaskPlan? {
        if let staleResourcePlanRecoveryHandler {
            return await staleResourcePlanRecoveryHandler(task, staleResource)
        }
        guard let staleResourceRecoveryHandler,
              let source = await staleResourceRecoveryHandler(task, staleResource)
        else {
            return nil
        }
        return VesperDownloadRecoveredTaskPlan(
            source: source,
            profile: task.profile,
            assetIndex: VesperDownloadAssetIndex()
        )
    }

    private func materializeGeneratedResources(
        assetId: VesperDownloadAssetId,
        taskId: VesperDownloadTaskId?,
        profile: VesperDownloadProfile,
        assetIndex: VesperDownloadAssetIndex
    ) throws -> VesperDownloadAssetIndex {
        try VesperGeneratedDownloadResourceMaterializer(
            fileManager: fileManager,
            baseDirectory: baseDirectory
        ).materialize(
            assetId: assetId,
            taskId: taskId,
            profile: profile,
            assetIndex: assetIndex
        )
    }

    private func storeRecoveredSource(_ source: VesperDownloadSource, forTaskId taskId: VesperDownloadTaskId) {
        lock.lock()
        recoveredSources[taskId] = source
        lock.unlock()
    }

    private func taskWithRecoveredSource(_ task: VesperDownloadTaskSnapshot) -> VesperDownloadTaskSnapshot {
        lock.lock()
        let recoveredSource = recoveredSources[task.taskId]
        lock.unlock()
        guard let recoveredSource else {
            return task
        }
        return VesperDownloadTaskSnapshot(
            taskId: task.taskId,
            assetId: task.assetId,
            source: recoveredSource,
            profile: task.profile,
            state: task.state,
            progress: task.progress,
            assetIndex: task.assetIndex,
            error: task.error
        )
    }

    private func prepareAssetIndex(task: VesperDownloadTaskSnapshot) async throws -> VesperDownloadAssetIndex {
        let requestHeaders = task.source.source.headers
        if !task.assetIndex.resources.isEmpty || !task.assetIndex.segments.isEmpty {
            return try await completePreparedAssetIndex(
                contentFormat: task.source.contentFormat,
                assetIndex: task.assetIndex,
                requestHeaders: requestHeaders
            )
        }

        switch task.source.contentFormat {
        case .hlsSegments:
            return try await planHlsAssetIndex(task: task, requestHeaders: requestHeaders)
        case .dashSegments:
            return try await planDashAssetIndex(task: task, requestHeaders: requestHeaders)
        case .flvSegments:
            return try await planFlvAssetIndex(task: task, requestHeaders: requestHeaders)
        case .singleFile:
            return try await planSingleFileAssetIndex(task: task, requestHeaders: requestHeaders)
        case .unknown:
            throw VesperForegroundDownloadPreparationError.unsupported("download preparation cannot plan an unknown content format")
        }
    }

    private func completePreparedAssetIndex(
        contentFormat: VesperDownloadContentFormat,
        assetIndex: VesperDownloadAssetIndex,
        requestHeaders: [String: String]
    ) async throws -> VesperDownloadAssetIndex {
        var totalSizeBytes: UInt64 = 0
        var resources: [VesperDownloadResourceRecord] = []
        resources.reserveCapacity(assetIndex.resources.count)

        for resource in assetIndex.resources {
            if resource.generatedText != nil {
                resources.append(resource)
                continue
            }
            let sizeBytes: UInt64
            if let existingSizeBytes = resource.sizeBytes {
                sizeBytes = existingSizeBytes
            } else {
                sizeBytes = try await probeRequiredSize(resource.uri, byteRange: resource.byteRange, requestHeaders: requestHeaders)
            }
            totalSizeBytes += sizeBytes
            resources.append(resource.withSizeBytes(sizeBytes))
        }

        var segments: [VesperDownloadSegmentRecord] = []
        segments.reserveCapacity(assetIndex.segments.count)
        for segment in assetIndex.segments {
            let sizeBytes: UInt64
            if let existingSizeBytes = segment.sizeBytes {
                sizeBytes = existingSizeBytes
            } else {
                sizeBytes = try await probeRequiredSize(segment.uri, byteRange: segment.byteRange, requestHeaders: requestHeaders)
            }
            totalSizeBytes += sizeBytes
            segments.append(segment.withSizeBytes(sizeBytes))
        }

        return VesperDownloadAssetIndex(
            contentFormat: contentFormat,
            version: assetIndex.version,
            etag: assetIndex.etag,
            checksum: assetIndex.checksum,
            totalSizeBytes: assetIndex.totalSizeBytes ?? totalSizeBytes,
            resources: resources,
            segments: segments,
            completedPath: assetIndex.completedPath
        )
    }

    private func planSingleFileAssetIndex(
        task: VesperDownloadTaskSnapshot,
        requestHeaders: [String: String]
    ) async throws -> VesperDownloadAssetIndex {
        let uri = task.source.manifestUri ?? task.source.source.uri
        let sizeBytes = try await probeRequiredSize(uri, byteRange: nil, requestHeaders: requestHeaders)
        return VesperDownloadAssetIndex(
            contentFormat: .singleFile,
            totalSizeBytes: sizeBytes,
            resources: [
                VesperDownloadResourceRecord(
                    resourceId: "single-file",
                    uri: uri,
                    relativePath: inferredFileName(uri),
                    sizeBytes: sizeBytes
                )
            ]
        )
    }

    private func planHlsAssetIndex(
        task: VesperDownloadTaskSnapshot,
        requestHeaders: [String: String]
    ) async throws -> VesperDownloadAssetIndex {
        let manifestUri = task.source.manifestUri ?? task.source.source.uri
        let manifestText = try await fetchText(manifestUri, requestHeaders: requestHeaders)
        if manifestText.range(of: "#EXT-X-STREAM-INF", options: .caseInsensitive) != nil {
            return try await planHlsMasterAssetIndex(
                manifestUri: manifestUri,
                manifestText: manifestText,
                profile: task.profile,
                requestHeaders: requestHeaders
            )
        }

        let media = try parseHlsMediaPlaylist(playlistUri: manifestUri, playlistText: manifestText)
        return try await buildHlsMediaAssetIndex(
            manifestPath: "index.m3u8",
            mediaPlaylists: [("media", media)],
            requestHeaders: requestHeaders
        )
    }

    private func planHlsMasterAssetIndex(
        manifestUri: String,
        manifestText: String,
        profile: VesperDownloadProfile,
        requestHeaders: [String: String]
    ) async throws -> VesperDownloadAssetIndex {
        let master = parseHlsMasterPlaylist(manifestUri: manifestUri, manifestText: manifestText)
        guard
            let variant = profile.variantId.flatMap({ variantId in
                master.variants.first { $0.uri == variantId || $0.attributes["NAME"] == variantId }
            }) ?? master.variants.first
        else {
            throw VesperForegroundDownloadPreparationError.invalidSource("HLS master playlist did not contain a playable variant")
        }

        var mediaPlaylists: [(String, HlsMediaPlaylist)] = [
            (
                "video",
                try parseHlsMediaPlaylist(
                    playlistUri: variant.uri,
                    playlistText: try await fetchText(variant.uri, requestHeaders: requestHeaders)
                )
            )
        ]

        let audio = profile.preferredAudioLanguage.flatMap { language in
            master.audio.first { $0.attributes["LANGUAGE"]?.caseInsensitiveCompare(language) == .orderedSame }
        } ?? master.audio.first { $0.attributes["DEFAULT"]?.caseInsensitiveCompare("YES") == .orderedSame }
            ?? master.audio.first
        if let audio {
            mediaPlaylists.append(
                (
                    "audio",
                    try parseHlsMediaPlaylist(
                        playlistUri: audio.uri,
                        playlistText: try await fetchText(audio.uri, requestHeaders: requestHeaders)
                    )
                )
            )
        }

        let planned = try await buildHlsMediaAssetIndex(
            manifestPath: "index.m3u8",
            mediaPlaylists: mediaPlaylists,
            requestHeaders: requestHeaders
        )
        let mediaResourceNames = planned.resources.compactMap { resource -> String? in
            guard
                let relativePath = resource.relativePath,
                relativePath.hasSuffix(".m3u8"),
                relativePath != "index.m3u8"
            else {
                return nil
            }
            return URL(fileURLWithPath: relativePath).lastPathComponent
        }
        let masterText = rewriteHlsMaster(
            variantAttributes: variant.attributes,
            mediaResourceNames: mediaResourceNames
        )
        return planned.withResources(
            planned.resources.map { resource in
                resource.resourceId == "hls-master"
                    ? resource.withGeneratedText(masterText)
                    : resource
            }
        )
    }

    private func buildHlsMediaAssetIndex(
        manifestPath: String,
        mediaPlaylists: [(String, HlsMediaPlaylist)],
        requestHeaders: [String: String]
    ) async throws -> VesperDownloadAssetIndex {
        var resources = [
            VesperDownloadResourceRecord(
                resourceId: "hls-master",
                uri: "vesper-generated://hls/\(manifestPath)",
                relativePath: manifestPath
            )
        ]
        var segments: [VesperDownloadSegmentRecord] = []
        var seenMaps = Set<String>()
        var totalSizeBytes: UInt64 = 0

        for (mediaId, playlist) in mediaPlaylists {
            let playlistPath =
                mediaPlaylists.count == 1 && manifestPath == "index.m3u8"
                    ? "index.m3u8"
                    : "\(mediaId).m3u8"
            var localMaps: [String: String] = [:]

            for (index, map) in playlist.maps.enumerated() {
                let key = hlsByteRangeKey(uri: map.uri, byteRange: map.byteRange)
                if seenMaps.insert(key).inserted {
                    let sizeBytes = try await probeRequiredSize(map.uri, byteRange: map.byteRange, requestHeaders: requestHeaders)
                    totalSizeBytes += sizeBytes
                    let relativePath = "segments/\(mediaId)-init-\(index).\(extensionFromUri(map.uri, fallback: "mp4"))"
                    resources.append(
                        VesperDownloadResourceRecord(
                            resourceId: "hls-\(mediaId)-init-\(index)",
                            uri: map.uri,
                            relativePath: relativePath,
                            byteRange: map.byteRange,
                            sizeBytes: sizeBytes
                        )
                    )
                    localMaps[key] = relativePath
                }
            }

            for segment in playlist.segments {
                let sizeBytes = try await probeRequiredSize(segment.uri, byteRange: segment.byteRange, requestHeaders: requestHeaders)
                totalSizeBytes += sizeBytes
                segments.append(
                    VesperDownloadSegmentRecord(
                        segmentId: "hls-\(mediaId)-\(segment.sequence)",
                        uri: segment.uri,
                        relativePath: "segments/\(mediaId)-\(padded(segment.sequence, width: 5)).\(extensionFromUri(segment.uri, fallback: "ts"))",
                        sequence: segment.sequence,
                        byteRange: segment.byteRange,
                        sizeBytes: sizeBytes
                    )
                )
            }

            resources.append(
                VesperDownloadResourceRecord(
                    resourceId: "hls-\(mediaId)-playlist",
                    uri: "vesper-generated://hls/\(playlistPath)",
                    relativePath: playlistPath,
                    generatedText: rewriteHlsMedia(mediaId: mediaId, playlist: playlist, localMaps: localMaps)
                )
            )
        }

        if mediaPlaylists.count == 1,
           let mediaResourceIndex = resources.firstIndex(where: { $0.resourceId.hasSuffix("-playlist") }) {
            let mediaResource = resources.remove(at: mediaResourceIndex)
            resources[0] = resources[0].withGeneratedText(mediaResource.generatedText ?? "")
        }

        return VesperDownloadAssetIndex(
            contentFormat: .hlsSegments,
            totalSizeBytes: totalSizeBytes,
            resources: resources,
            segments: segments
        )
    }

    private func planDashAssetIndex(
        task: VesperDownloadTaskSnapshot,
        requestHeaders: [String: String]
    ) async throws -> VesperDownloadAssetIndex {
        let manifestUri = task.source.manifestUri ?? task.source.source.uri
        let manifestText = try await fetchText(manifestUri, requestHeaders: requestHeaders)
        let documentType = xmlAttr(manifestText, tag: "MPD", attr: "type")
        if let documentType, !documentType.isEmpty, documentType.caseInsensitiveCompare("static") != .orderedSame {
            throw VesperForegroundDownloadPreparationError.unsupported("DASH download preparation requires a static MPD")
        }
        guard let durationSeconds = parseIso8601DurationSeconds(xmlAttr(manifestText, tag: "MPD", attr: "mediaPresentationDuration")) else {
            throw VesperForegroundDownloadPreparationError.invalidSource("DASH SegmentTemplate planning requires a finite MPD duration")
        }

        let representations = selectDashRepresentations(
            manifestText: manifestText,
            manifestUri: manifestUri,
            profile: task.profile
        )
        if representations.isEmpty {
            throw VesperForegroundDownloadPreparationError.invalidSource("DASH MPD did not contain a supported SegmentTemplate or SegmentBase representation")
        }

        var resources: [VesperDownloadResourceRecord] = []
        var segments: [VesperDownloadSegmentRecord] = []
        var rewrittenAdaptationSets: [String] = []
        var totalSizeBytes: UInt64 = 0
        var globalSequence: UInt64 = 1

        for (index, representation) in representations.enumerated() {
            let mediaId = representation.mediaId.isEmpty ? "media\(index)" : representation.mediaId
            if let template = representation.template {
                guard template.duration > 0 else {
                    throw VesperForegroundDownloadPreparationError.invalidSource("DASH SegmentTemplate duration must be greater than zero")
                }
                let segmentSeconds = Double(template.duration) / Double(max(template.timescale, 1))
                let segmentCount = max(UInt64(ceil(durationSeconds / segmentSeconds)), 1)
                if let initialization = template.initialization, !initialization.isEmpty {
                    let remote = resolveRemoteReference(
                        baseUri: representation.baseUri,
                        reference: expandDashTemplate(initialization, representationId: representation.id, number: template.startNumber)
                    )
                    let sizeBytes = try await probeRequiredSize(remote, byteRange: nil, requestHeaders: requestHeaders)
                    totalSizeBytes += sizeBytes
                    resources.append(
                        VesperDownloadResourceRecord(
                            resourceId: "dash-\(mediaId)-init",
                            uri: remote,
                            relativePath: "segments/\(mediaId)-init.mp4",
                            sizeBytes: sizeBytes
                        )
                    )
                }

                for offset in 0..<segmentCount {
                    let number = template.startNumber + offset
                    let remote = resolveRemoteReference(
                        baseUri: representation.baseUri,
                        reference: expandDashTemplate(template.media, representationId: representation.id, number: number)
                    )
                    let sizeBytes = try await probeRequiredSize(remote, byteRange: nil, requestHeaders: requestHeaders)
                    totalSizeBytes += sizeBytes
                    segments.append(
                        VesperDownloadSegmentRecord(
                            segmentId: "dash-\(mediaId)-segment-\(number)",
                            uri: remote,
                            relativePath: "segments/\(mediaId)-\(number).m4s",
                            sequence: globalSequence,
                            sizeBytes: sizeBytes
                        )
                    )
                    globalSequence += 1
                }

                rewrittenAdaptationSets.append(
                    rewriteDashTemplateAdaptationSet(
                        representation: representation,
                        template: template,
                        mediaId: mediaId,
                        segmentCount: segmentCount
                    )
                )
            } else if let baseUrl = representation.baseUrl {
                let remote = resolveRemoteReference(baseUri: representation.baseUri, reference: baseUrl)
                let sizeBytes = try await probeRequiredSize(remote, byteRange: nil, requestHeaders: requestHeaders)
                totalSizeBytes += sizeBytes
                let localName = "media-\(mediaId).\(extensionFromUri(remote, fallback: "mp4"))"
                resources.append(
                    VesperDownloadResourceRecord(
                        resourceId: "dash-\(mediaId)-media",
                        uri: remote,
                        relativePath: localName,
                        sizeBytes: sizeBytes
                    )
                )
                rewrittenAdaptationSets.append(
                    rewriteDashSegmentBaseAdaptationSet(representation: representation, localName: localName)
                )
            }
        }

        resources.insert(
            VesperDownloadResourceRecord(
                resourceId: "dash-manifest",
                uri: "vesper-generated://dash/manifest.mpd",
                relativePath: "manifest.mpd",
                generatedText: rewriteDashMpd(
                    duration: xmlAttr(manifestText, tag: "MPD", attr: "mediaPresentationDuration"),
                    adaptationSets: rewrittenAdaptationSets
                )
            ),
            at: 0
        )

        return VesperDownloadAssetIndex(
            contentFormat: .dashSegments,
            totalSizeBytes: totalSizeBytes,
            resources: resources,
            segments: segments
        )
    }

    private func planFlvAssetIndex(
        task: VesperDownloadTaskSnapshot,
        requestHeaders: [String: String]
    ) async throws -> VesperDownloadAssetIndex {
        let uri = task.source.manifestUri ?? task.source.source.uri
        let clipUris =
            extensionFromUri(uri, fallback: "flv").caseInsensitiveCompare("flv") == .orderedSame
                ? [uri]
                : parseFlvClipManifest(baseUri: uri, manifestText: try await fetchText(uri, requestHeaders: requestHeaders))
        if clipUris.isEmpty {
            throw VesperForegroundDownloadPreparationError.invalidSource("FLV clip manifest did not contain any clip URI")
        }

        var totalSizeBytes: UInt64 = 0
        var concat = "ffconcat version 1.0\n"
        var segments: [VesperDownloadSegmentRecord] = []
        for (index, clipUri) in clipUris.enumerated() {
            let sequence = UInt64(index + 1)
            let sizeBytes = try await probeRequiredSize(clipUri, byteRange: nil, requestHeaders: requestHeaders)
            totalSizeBytes += sizeBytes
            let localPath = "clips/clip-\(padded(sequence, width: 5)).\(extensionFromUri(clipUri, fallback: "flv"))"
            concat += "file '\(escapeFfconcatPath(localPath))'\n"
            segments.append(
                VesperDownloadSegmentRecord(
                    segmentId: "flv-clip-\(sequence)",
                    uri: clipUri,
                    relativePath: localPath,
                    sequence: sequence,
                    sizeBytes: sizeBytes
                )
            )
        }

        return VesperDownloadAssetIndex(
            contentFormat: .flvSegments,
            totalSizeBytes: totalSizeBytes,
            resources: [
                VesperDownloadResourceRecord(
                    resourceId: "flv-concat",
                    uri: "vesper-generated://flv/manifest.ffconcat",
                    relativePath: "manifest.ffconcat",
                    generatedText: concat
                )
            ],
            segments: segments
        )
    }

    public func prepare(
        task: VesperDownloadTaskSnapshot,
        reporter: any VesperDownloadExecutionReporter
    ) {
        Task.detached(priority: .utility) {
            do {
                let assetIndex = try await self.prepareAssetIndexWithRecovery(task: task, reporter: reporter)
                await reporter.completePreparation(taskId: task.taskId, assetIndex: assetIndex)
            } catch {
                await reporter.fail(
                    taskId: task.taskId,
                    error: VesperDownloadError(
                        code: .backendFailure,
                        category: .network,
                        retriable: false,
                        message: error.localizedDescription
                    )
                )
            }
        }
    }

    public func start(
        task: VesperDownloadTaskSnapshot,
        reporter: any VesperDownloadExecutionReporter
    ) {
        launchDownload(task: taskWithRecoveredSource(task), reporter: reporter)
    }

    public func resume(
        task: VesperDownloadTaskSnapshot,
        reporter: any VesperDownloadExecutionReporter
    ) {
        launchDownload(task: taskWithRecoveredSource(task), reporter: reporter)
    }

    public func pause(taskId: VesperDownloadTaskId) {
        lock.lock()
        let task = tasks.removeValue(forKey: taskId)
        lock.unlock()
        task?.cancel()
    }

    public func remove(task: VesperDownloadTaskSnapshot?) {
        guard let task else {
            return
        }
        pause(taskId: task.taskId)
        lock.lock()
        recoveredSources.removeValue(forKey: task.taskId)
        lock.unlock()
        if let completedPath = task.assetIndex.completedPath {
            let url = URL(fileURLWithPath: completedPath)
            try? fileManager.removeItem(at: url)
            return
        }
        if let targetDirectory = task.profile.targetDirectory {
            try? fileManager.removeItem(at: targetDirectory)
            return
        }
        try? fileManager.removeItem(at: defaultAssetDirectory(for: task))
    }

    public func dispose() {
        lock.lock()
        let activeTasks = Array(tasks.values)
        tasks.removeAll(keepingCapacity: false)
        lock.unlock()
        activeTasks.forEach { $0.cancel() }
    }

    private func launchDownload(
        task: VesperDownloadTaskSnapshot,
        reporter: any VesperDownloadExecutionReporter
    ) {
        pause(taskId: task.taskId)

        let work = Task.detached(priority: .utility) { [weak self] in
            guard let self else {
                return
            }

            var receivedBytes: UInt64 = 0
            var receivedSegments: UInt32 = 0
            var activeEntry: ForegroundDownloadEntry?

            do {
                let materializedTask = try task.withAssetIndex(
                    self.materializeGeneratedResources(
                        assetId: task.assetId,
                        taskId: task.taskId,
                        profile: task.profile,
                        assetIndex: task.assetIndex
                    )
                )
                let plan = try self.executionPlan(for: materializedTask)
                let requestHeaders = materializedTask.source.source.headers
                let trackSegments = !materializedTask.assetIndex.segments.isEmpty
                var progressThrottle = DownloadProgressThrottle(
                    minProgressBytes: self.minProgressBytes,
                    minProgressIntervalMs: self.minProgressIntervalMs
                )

                for (index, entry) in plan.enumerated() {
                    try Task.checkCancellation()
                    activeEntry = entry

                    let destinationURL = try self.outputURL(for: materializedTask, entry: entry, index: index)
                    try self.fileManager.createDirectory(
                        at: destinationURL.deletingLastPathComponent(),
                        withIntermediateDirectories: true
                    )
                    excludeDownloadItemFromBackup(self.defaultBaseDirectory(for: materializedTask))
                    excludeDownloadItemFromBackup(destinationURL.deletingLastPathComponent())

                    if self.fileManager.fileExists(atPath: destinationURL.path),
                       entry.generatedText != nil {
                        try? self.fileManager.removeItem(at: destinationURL)
                    }

                    let writtenBytes: UInt64
                    if let generatedText = entry.generatedText {
                        try generatedText.write(to: destinationURL, atomically: true, encoding: .utf8)
                        writtenBytes = 0
                    } else {
                        let resumeFromBytes = self.resumableExistingBytes(
                            at: destinationURL,
                            expectedSizeBytes: entry.expectedSizeBytes
                        )
                        writtenBytes = try await self.fetch(
                            entry.url,
                            byteRange: entry.byteRange,
                            requestHeaders: requestHeaders,
                            expectedSizeBytes: entry.expectedSizeBytes,
                            resumeFromBytes: resumeFromBytes,
                            to: destinationURL
                        ) { entryBytes in
                            let nextBytes = receivedBytes + entryBytes
                            if progressThrottle.shouldReport(receivedBytes: nextBytes, force: false) {
                                await reporter.updateProgress(
                                    taskId: task.taskId,
                                    receivedBytes: nextBytes,
                                    receivedSegments: receivedSegments
                                )
                            }
                        }
                    }
                    excludeDownloadItemFromBackup(destinationURL)
                    receivedBytes += writtenBytes
                    if trackSegments, entry.isSegment {
                        receivedSegments += 1
                    }
                    progressThrottle.markReported(receivedBytes: receivedBytes)
                    await reporter.updateProgress(
                        taskId: task.taskId,
                        receivedBytes: receivedBytes,
                        receivedSegments: receivedSegments
                    )
                }

                await reporter.complete(
                    taskId: task.taskId,
                    completedPath: self.completedPath(for: materializedTask, plan: plan)
                )
            } catch is CancellationError {
                return
            } catch let staleError as VesperStaleDownloadResourceError {
                do {
                    let recovered = try await self.recoverStaleDownload(
                        task: task,
                        staleError: staleError,
                        activeEntry: activeEntry,
                        receivedBytes: receivedBytes,
                        reporter: reporter
                    )
                    if recovered {
                        return
                    }
                } catch {
                    await reporter.fail(
                        taskId: task.taskId,
                        error: VesperDownloadError(
                            code: .backendFailure,
                            category: .network,
                            retriable: false,
                            message: error.localizedDescription
                        )
                    )
                    return
                }
                await reporter.fail(
                    taskId: task.taskId,
                    error: VesperDownloadError(
                        code: .backendFailure,
                        category: .network,
                        retriable: false,
                        message: staleError.localizedDescription
                    )
                )
            } catch {
                await reporter.fail(
                    taskId: task.taskId,
                    error: VesperDownloadError(
                        code: .backendFailure,
                        category: .network,
                        retriable: false,
                        message: error.localizedDescription
                    )
                )
            }

            await MainActor.run {
                self.lock.lock()
                self.tasks.removeValue(forKey: task.taskId)
                self.recoveredSources.removeValue(forKey: task.taskId)
                self.lock.unlock()
            }
        }

        lock.lock()
        tasks[task.taskId] = work
        lock.unlock()
    }

    private func recoverStaleDownload(
        task: VesperDownloadTaskSnapshot,
        staleError: VesperStaleDownloadResourceError,
        activeEntry: ForegroundDownloadEntry?,
        receivedBytes: UInt64,
        reporter: any VesperDownloadExecutionReporter
    ) async throws -> Bool {
        let staleResource = staleError.staleResource(
            taskId: task.taskId,
            fallbackResourceId: activeEntry?.resourceId,
            fallbackSegmentId: activeEntry?.segmentId,
            fallbackUri: activeEntry?.url.absoluteString,
            phase: .download,
            receivedBytes: receivedBytes
        )
        guard let recoveredPlan = await recoverTaskPlan(task: task, staleResource: staleResource) else {
            return false
        }

        pause(taskId: task.taskId)
        try? fileManager.removeItem(at: defaultBaseDirectory(for: task))

        let materializedRecoveredIndex = try materializeGeneratedResources(
            assetId: task.assetId,
            taskId: task.taskId,
            profile: recoveredPlan.profile,
            assetIndex: recoveredPlan.assetIndex
        )
        let recoveredTask = VesperDownloadTaskSnapshot(
            taskId: task.taskId,
            assetId: task.assetId,
            source: recoveredPlan.source,
            profile: recoveredPlan.profile,
            state: .preparing,
            progress: VesperDownloadProgressSnapshot(),
            assetIndex: materializedRecoveredIndex,
            error: nil
        )
        await reporter.replaceTaskPlan(
            taskId: task.taskId,
            source: recoveredPlan.source,
            profile: recoveredPlan.profile,
            assetIndex: materializedRecoveredIndex
        )

        let preparedIndex = try await prepareAssetIndex(task: recoveredTask)
        let materializedPreparedIndex = try materializeGeneratedResources(
            assetId: task.assetId,
            taskId: task.taskId,
            profile: recoveredPlan.profile,
            assetIndex: preparedIndex
        )
        await reporter.completePreparation(taskId: task.taskId, assetIndex: materializedPreparedIndex)
        return true
    }

    private func executionPlan(for task: VesperDownloadTaskSnapshot) throws -> [ForegroundDownloadEntry] {
        let resources = try task.assetIndex.resources.map {
            ForegroundDownloadEntry(
                url: try resolveURL($0.uri),
                resourceId: $0.resourceId.isEmpty ? nil : $0.resourceId,
                segmentId: nil,
                relativePath: $0.relativePath,
                byteRange: $0.byteRange,
                generatedText: $0.generatedText,
                expectedSizeBytes: $0.sizeBytes,
                fallbackName: $0.resourceId.isEmpty ? "resource" : $0.resourceId,
                isSegment: false
            )
        }
        let segments = try task.assetIndex.segments.enumerated().map { index, segment in
            ForegroundDownloadEntry(
                url: try resolveURL(segment.uri),
                resourceId: nil,
                segmentId: segment.segmentId.isEmpty ? nil : segment.segmentId,
                relativePath: segment.relativePath,
                byteRange: segment.byteRange,
                generatedText: nil,
                expectedSizeBytes: segment.sizeBytes,
                fallbackName: segment.segmentId.isEmpty ? "segment-\(index + 1)" : segment.segmentId,
                isSegment: true
            )
        }
        if !resources.isEmpty || !segments.isEmpty {
            return resources + segments
        }

        return [
            ForegroundDownloadEntry(
                url: try resolveURL(task.source.manifestUri ?? task.source.source.uri),
                resourceId: nil,
                segmentId: nil,
                relativePath: nil,
                byteRange: nil,
                generatedText: nil,
                expectedSizeBytes: task.progress.totalBytes,
                fallbackName: task.assetId.isEmpty ? "download-\(task.taskId)" : task.assetId,
                isSegment: false
            ),
        ]
    }

    private func resolveURL(_ value: String) throws -> URL {
        if let url = URL(string: value) {
            try rejectInsecureHTTPURL(url)
            return url
        }
        throw CocoaError(.fileReadInvalidFileName)
    }

    private func outputURL(
        for task: VesperDownloadTaskSnapshot,
        entry: ForegroundDownloadEntry,
        index: Int
    ) throws -> URL {
        let baseDirectory = defaultBaseDirectory(for: task)
        if let relativePath = entry.relativePath, !relativePath.isEmpty {
            if relativePath.hasPrefix("/") {
                return URL(fileURLWithPath: relativePath)
            }
            let components = relativePath.split(separator: "/", omittingEmptySubsequences: false)
            if components.contains(where: { $0 == ".." }) {
                throw VesperForegroundDownloadPreparationError.invalidSource(
                    "download output path escapes the task directory: \(relativePath)"
                )
            }
            let candidate = baseDirectory.appendingPathComponent(relativePath).standardizedFileURL
            let standardizedBase = baseDirectory.standardizedFileURL
            guard candidate.path == standardizedBase.path || candidate.path.hasPrefix(standardizedBase.path + "/") else {
                throw VesperForegroundDownloadPreparationError.invalidSource(
                    "download output path escapes the task directory: \(relativePath)"
                )
            }
            return candidate
        }

        let filename =
            entry.url.lastPathComponent.isEmpty
            ? "\(entry.fallbackName)-\(index + 1).bin"
            : entry.url.lastPathComponent
        return baseDirectory.appendingPathComponent(filename)
    }

    private func completedPath(
        for task: VesperDownloadTaskSnapshot,
        plan: [ForegroundDownloadEntry]
    ) -> String {
        guard plan.count == 1, let first = try? outputURL(for: task, entry: plan[0], index: 0) else {
            return defaultBaseDirectory(for: task).path
        }
        return first.path
    }

    private func defaultBaseDirectory(for task: VesperDownloadTaskSnapshot) -> URL {
        if let targetDirectory = task.profile.targetDirectory {
            return targetDirectory
        }
        return defaultAssetDirectory(for: task)
    }

    private func defaultAssetDirectory(for task: VesperDownloadTaskSnapshot) -> URL {
        let root = baseDirectory
            ?? fileManager.urls(for: .documentDirectory, in: .userDomainMask).first!
                .appendingPathComponent("vesper-downloads", isDirectory: true)
        return root.appendingPathComponent(task.assetId.isEmpty ? String(task.taskId) : task.assetId)
    }

    private func httpBodyStream(for request: URLRequest, sourceURL: URL) async throws -> VesperHTTPBodyStream {
        try rejectInsecureHTTPURL(sourceURL)

        let configuration = URLSessionConfiguration.ephemeral
        configuration.waitsForConnectivity = true
        let timeoutSeconds = max(TimeInterval(stalledTransferTimeoutMs) / 1_000, 1)
        configuration.timeoutIntervalForRequest = timeoutSeconds
        configuration.timeoutIntervalForResource = max(timeoutSeconds * 4, 60)

        let delegate = VesperURLSessionDataStreamDelegate(
            stalledTransferTimeoutMs: stalledTransferTimeoutMs,
            sourceDescription: sourceURL.absoluteString
        )
        let delegateQueue = OperationQueue()
        delegateQueue.maxConcurrentOperationCount = 1
        let session = URLSession(configuration: configuration, delegate: delegate, delegateQueue: delegateQueue)
        let task = session.dataTask(with: request)
        delegate.bind(session: session, task: task)
        task.resume()
        let response = try await delegate.waitForResponse()
        return VesperHTTPBodyStream(
            response: response,
            chunks: delegate.chunks,
            cancel: { delegate.cancel() }
        )
    }

    private func httpData(for request: URLRequest, sourceURL: URL) async throws -> (Data, URLResponse) {
        let stream = try await httpBodyStream(for: request, sourceURL: sourceURL)
        defer { stream.cancel() }

        var data = Data()
        for try await chunk in stream.chunks {
            try Task.checkCancellation()
            data.append(chunk)
        }
        return (data, stream.response)
    }

    private func fetch(
        _ sourceURL: URL,
        byteRange: VesperDownloadByteRange?,
        requestHeaders: [String: String],
        expectedSizeBytes: UInt64?,
        resumeFromBytes: UInt64,
        to destinationURL: URL,
        allowRestartAfterRangeMismatch: Bool = true,
        onProgress: (UInt64) async -> Void
    ) async throws -> UInt64 {
        if let expectedSizeBytes, resumeFromBytes >= expectedSizeBytes {
            return expectedSizeBytes
        }

        if sourceURL.isFileURL {
            return try await copyFileURL(
                sourceURL,
                byteRange: byteRange,
                expectedSizeBytes: expectedSizeBytes,
                resumeFromBytes: resumeFromBytes,
                to: destinationURL,
                onProgress: onProgress
            )
        }
        if byteRange == nil, let expectedSizeBytes, expectedSizeBytes > 0, let rangeChunkBytes {
            return try await fetchKnownSizeHTTPResource(
                sourceURL,
                requestHeaders: requestHeaders,
                expectedSizeBytes: expectedSizeBytes,
                resumeFromBytes: resumeFromBytes,
                rangeChunkBytes: rangeChunkBytes,
                to: destinationURL,
                allowRestartAfterRangeMismatch: allowRestartAfterRangeMismatch,
                onProgress: onProgress
            )
        }

        var request = URLRequest(url: sourceURL)
        request.applyDownloadHttpHeaders(requestHeaders)
        var requestedRangeStart: UInt64?
        var requestedRangeEndInclusive: UInt64?
        var expectedResponseBodyBytes: UInt64?
        if let byteRange {
            guard resumeFromBytes < byteRange.length else {
                return byteRange.length
            }
            let remaining = byteRange.length > resumeFromBytes ? byteRange.length - resumeFromBytes : 0
            let start = byteRange.offset + resumeFromBytes
            let end = remaining == 0 ? start : start + remaining - 1
            request.setValue("bytes=\(start)-\(end)", forHTTPHeaderField: "Range")
            requestedRangeStart = start
            requestedRangeEndInclusive = end
            expectedResponseBodyBytes = remaining
        } else if resumeFromBytes > 0 {
            request.setValue("bytes=\(resumeFromBytes)-", forHTTPHeaderField: "Range")
            requestedRangeStart = resumeFromBytes
            requestedRangeEndInclusive = expectedSizeBytes.flatMap { $0 > 0 ? $0 - 1 : nil }
            expectedResponseBodyBytes = expectedSizeBytes.map { $0 > resumeFromBytes ? $0 - resumeFromBytes : 0 }
        }

        let stream = try await httpBodyStream(for: request, sourceURL: sourceURL)
        defer { stream.cancel() }
        var expectedFinalBytesAfterResponse: UInt64?
        let response = stream.response
        if let http = response as? HTTPURLResponse {
            switch http.statusCode {
            case 206:
                guard let requestedRangeStart else {
                    throw VesperForegroundDownloadPreparationError.invalidSource(
                        "remote server returned an unexpected Content-Range for \(sourceURL.absoluteString)"
                    )
                }
                let contentRange = try validateHTTPPartialContentRange(
                    contentRangeHeader: http.value(forHTTPHeaderField: "Content-Range"),
                    contentLengthHeader: http.value(forHTTPHeaderField: "Content-Length"),
                    requestedStart: requestedRangeStart,
                    requestedEndInclusive: requestedRangeEndInclusive,
                    expectedBodyLength: expectedResponseBodyBytes,
                    expectedTotalSizeBytes: byteRange == nil ? expectedSizeBytes : nil,
                    sourceDescription: sourceURL.absoluteString
                )
                if let responseBytes = contentRange.length {
                    expectedFinalBytesAfterResponse = resumeFromBytes + responseBytes
                }
            case 200:
                if requestedRangeStart != nil {
                    if byteRange == nil, resumeFromBytes > 0, allowRestartAfterRangeMismatch {
                        try? fileManager.removeItem(at: destinationURL)
                        await onProgress(0)
                        return try await fetch(
                            sourceURL,
                            byteRange: byteRange,
                            requestHeaders: requestHeaders,
                            expectedSizeBytes: expectedSizeBytes,
                            resumeFromBytes: 0,
                            to: destinationURL,
                            allowRestartAfterRangeMismatch: false,
                            onProgress: onProgress
                        )
                    }
                    throw VesperForegroundDownloadPreparationError.invalidSource(
                        "remote server did not honor the requested byte range for \(sourceURL.absoluteString)"
                    )
                }
                if let expectedSizeBytes,
                   let contentLength = parseHttpContentLength(http.value(forHTTPHeaderField: "Content-Length")),
                   contentLength != expectedSizeBytes {
                    throw VesperForegroundDownloadPreparationError.invalidSource(
                        "remote server reported Content-Length \(contentLength), expected \(expectedSizeBytes) for \(sourceURL.absoluteString)"
                    )
                }
            case 416:
                if resumeFromBytes > 0, allowRestartAfterRangeMismatch {
                    try? fileManager.removeItem(at: destinationURL)
                    await onProgress(0)
                    return try await fetch(
                        sourceURL,
                        byteRange: byteRange,
                        requestHeaders: requestHeaders,
                        expectedSizeBytes: expectedSizeBytes,
                        resumeFromBytes: 0,
                        to: destinationURL,
                        allowRestartAfterRangeMismatch: false,
                        onProgress: onProgress
                    )
                }
                throw VesperForegroundDownloadPreparationError.invalidSource(
                    "remote resource rejected the requested byte range for \(sourceURL.absoluteString)"
                )
            case 401, 403, 404, 410:
                throw staleDownloadResource(
                    "offline download resource is stale or expired (HTTP \(http.statusCode)) for \(sourceURL.absoluteString); refresh the media link and prepare the task again",
                    uri: sourceURL.absoluteString,
                    phase: .download,
                    statusCode: http.statusCode
                )
            case 200..<300:
                break
            default:
                throw VesperForegroundDownloadPreparationError.invalidSource(
                    "remote resource returned HTTP \(http.statusCode) for \(sourceURL.absoluteString)"
                )
            }
        }

        if !fileManager.fileExists(atPath: destinationURL.path) {
            fileManager.createFile(atPath: destinationURL.path, contents: nil)
        }
        let output = try FileHandle(forWritingTo: destinationURL)
        defer { closeDownloadFileHandle(output, context: "streamed resource output") }
        if resumeFromBytes > 0 {
            try output.seekToEnd()
        } else {
            try output.truncate(atOffset: 0)
        }

        var totalWritten = resumeFromBytes
        var lastCleanFileSize = resumeFromBytes
        var buffer = Data()
        buffer.reserveCapacity(64 * 1024)

        do {
            for try await data in stream.chunks {
                try Task.checkCancellation()
                buffer.append(data)
                if buffer.count >= 64 * 1024 {
                    try output.write(contentsOf: buffer)
                    totalWritten += UInt64(buffer.count)
                    lastCleanFileSize = totalWritten
                    if let expectedFinalBytesAfterResponse,
                       totalWritten > expectedFinalBytesAfterResponse {
                        try? fileManager.removeItem(at: destinationURL)
                        throw VesperForegroundDownloadPreparationError.invalidSource(
                            "remote server sent more bytes than its Content-Range for \(sourceURL.absoluteString)"
                        )
                    }
                    if let expectedSizeBytes, totalWritten > expectedSizeBytes {
                        try? fileManager.removeItem(at: destinationURL)
                        throw VesperForegroundDownloadPreparationError.invalidSource(
                            "remote server sent more bytes than expected for \(sourceURL.absoluteString)"
                        )
                    }
                    buffer.removeAll(keepingCapacity: true)
                    await onProgress(totalWritten)
                }
            }
            if !buffer.isEmpty {
                try output.write(contentsOf: buffer)
                totalWritten += UInt64(buffer.count)
                lastCleanFileSize = totalWritten
                if let expectedFinalBytesAfterResponse,
                   totalWritten > expectedFinalBytesAfterResponse {
                    try? fileManager.removeItem(at: destinationURL)
                    throw VesperForegroundDownloadPreparationError.invalidSource(
                        "remote server sent more bytes than its Content-Range for \(sourceURL.absoluteString)"
                    )
                }
                if let expectedSizeBytes, totalWritten > expectedSizeBytes {
                    try? fileManager.removeItem(at: destinationURL)
                    throw VesperForegroundDownloadPreparationError.invalidSource(
                        "remote server sent more bytes than expected for \(sourceURL.absoluteString)"
                    )
                }
                buffer.removeAll(keepingCapacity: true)
                await onProgress(totalWritten)
            }
        } catch {
            try? truncateFile(at: destinationURL, to: lastCleanFileSize)
            throw error
        }

        if let expectedFinalBytesAfterResponse,
           totalWritten != expectedFinalBytesAfterResponse {
            throw VesperForegroundDownloadPreparationError.invalidSource(
                "downloaded \(totalWritten) bytes after resume, expected \(expectedFinalBytesAfterResponse)"
            )
        }

        if let expectedSizeBytes, totalWritten != expectedSizeBytes {
            throw VesperForegroundDownloadPreparationError.invalidSource(
                "downloaded \(totalWritten) bytes, expected \(expectedSizeBytes)"
            )
        }
        return totalWritten
    }

    private func fetchKnownSizeHTTPResource(
        _ sourceURL: URL,
        requestHeaders: [String: String],
        expectedSizeBytes: UInt64,
        resumeFromBytes: UInt64,
        rangeChunkBytes: UInt64,
        to destinationURL: URL,
        allowRestartAfterRangeMismatch: Bool,
        onProgress: (UInt64) async -> Void
    ) async throws -> UInt64 {
        var offset = resumeFromBytes
        if offset >= expectedSizeBytes {
            return expectedSizeBytes
        }
        while offset < expectedSizeBytes {
            let chunkLength = min(rangeChunkBytes, expectedSizeBytes - offset)
            let chunkEnd = offset + chunkLength - 1
            let nextOffset = try await fetchHTTPRangeChunk(
                sourceURL,
                requestHeaders: requestHeaders,
                expectedSizeBytes: expectedSizeBytes,
                rangeStart: offset,
                rangeEndInclusive: chunkEnd,
                rangeChunkBytes: rangeChunkBytes,
                to: destinationURL,
                allowRestartAfterRangeMismatch: allowRestartAfterRangeMismatch,
                onProgress: onProgress
            )
            guard nextOffset > offset else {
                throw VesperForegroundDownloadPreparationError.invalidSource(
                    "download range transfer did not advance for \(sourceURL.absoluteString)"
                )
            }
            offset = nextOffset
        }
        return offset
    }

    private func fetchHTTPRangeChunk(
        _ sourceURL: URL,
        requestHeaders: [String: String],
        expectedSizeBytes: UInt64,
        rangeStart: UInt64,
        rangeEndInclusive: UInt64,
        rangeChunkBytes: UInt64,
        to destinationURL: URL,
        allowRestartAfterRangeMismatch: Bool,
        onProgress: (UInt64) async -> Void
    ) async throws -> UInt64 {
        var request = URLRequest(url: sourceURL)
        request.applyDownloadHttpHeaders(requestHeaders)
        request.setValue("bytes=\(rangeStart)-\(rangeEndInclusive)", forHTTPHeaderField: "Range")

        let stream = try await httpBodyStream(for: request, sourceURL: sourceURL)
        defer { stream.cancel() }
        let response = stream.response
        guard let http = response as? HTTPURLResponse else {
            throw VesperForegroundDownloadPreparationError.invalidSource(
                "remote resource did not return an HTTP response for \(sourceURL.absoluteString)"
            )
        }
        let statusCode = http.statusCode
        let chunkCoversWholeResource = rangeStart == 0 && rangeEndInclusive + 1 >= expectedSizeBytes

        switch statusCode {
        case 206:
            do {
                try validateHTTPPartialContentRange(
                    contentRangeHeader: http.value(forHTTPHeaderField: "Content-Range"),
                    contentLengthHeader: http.value(forHTTPHeaderField: "Content-Length"),
                    requestedStart: rangeStart,
                    requestedEndInclusive: rangeEndInclusive,
                    expectedBodyLength: rangeEndInclusive - rangeStart + 1,
                    expectedTotalSizeBytes: expectedSizeBytes,
                    sourceDescription: sourceURL.absoluteString
                )
            } catch {
                throw staleDownloadResource(
                    error.localizedDescription,
                    uri: sourceURL.absoluteString,
                    phase: .download,
                    receivedBytes: rangeStart
                )
            }
        case 200:
            if !chunkCoversWholeResource {
                if rangeStart > 0, allowRestartAfterRangeMismatch {
                    try? fileManager.removeItem(at: destinationURL)
                    await onProgress(0)
                    return try await fetchKnownSizeHTTPResource(
                        sourceURL,
                        requestHeaders: requestHeaders,
                        expectedSizeBytes: expectedSizeBytes,
                        resumeFromBytes: 0,
                        rangeChunkBytes: rangeChunkBytes,
                        to: destinationURL,
                        allowRestartAfterRangeMismatch: false,
                        onProgress: onProgress
                    )
                }
                throw staleDownloadResource(
                    "remote server did not honor the requested byte range for \(sourceURL.absoluteString)"
                )
            }
            if let contentLength = parseHttpContentLength(http.value(forHTTPHeaderField: "Content-Length")),
               contentLength != expectedSizeBytes {
                throw staleDownloadResource(
                    "remote server reported Content-Length \(contentLength), expected \(expectedSizeBytes) for \(sourceURL.absoluteString)"
                )
            }
        case 416:
            if rangeStart > 0, allowRestartAfterRangeMismatch {
                try? fileManager.removeItem(at: destinationURL)
                await onProgress(0)
                return try await fetchKnownSizeHTTPResource(
                    sourceURL,
                        requestHeaders: requestHeaders,
                        expectedSizeBytes: expectedSizeBytes,
                        resumeFromBytes: 0,
                        rangeChunkBytes: rangeChunkBytes,
                        to: destinationURL,
                        allowRestartAfterRangeMismatch: false,
                        onProgress: onProgress
                )
            }
            throw staleDownloadResource(
                "remote resource rejected the requested byte range for \(sourceURL.absoluteString)"
            )
        case 401, 403, 404, 410:
            throw staleDownloadResource(
                "offline download resource is stale or expired (HTTP \(statusCode)) for \(sourceURL.absoluteString); refresh the media link and prepare the task again"
            )
        case 200..<300:
            break
        default:
            throw staleDownloadResource(
                "remote resource returned HTTP \(statusCode) for \(sourceURL.absoluteString)"
            )
        }

        if !fileManager.fileExists(atPath: destinationURL.path) {
            fileManager.createFile(atPath: destinationURL.path, contents: nil)
        }
        let append = statusCode == 206 && rangeStart > 0
        if append {
            let existingBytes = UInt64(
                (try? destinationURL.resourceValues(forKeys: [.fileSizeKey]).fileSize) ?? 0
            )
            if existingBytes != rangeStart {
                try? fileManager.removeItem(at: destinationURL)
                await onProgress(0)
                return try await fetchKnownSizeHTTPResource(
                    sourceURL,
                    requestHeaders: requestHeaders,
                    expectedSizeBytes: expectedSizeBytes,
                    resumeFromBytes: 0,
                    rangeChunkBytes: rangeChunkBytes,
                    to: destinationURL,
                    allowRestartAfterRangeMismatch: false,
                    onProgress: onProgress
                )
            }
        }
        let output = try FileHandle(forWritingTo: destinationURL)
        defer { closeDownloadFileHandle(output, context: "known-size resource output") }
        if append {
            try output.seekToEnd()
        } else {
            try output.truncate(atOffset: 0)
        }

        var totalWritten = append ? rangeStart : 0
        var lastCleanFileSize = totalWritten
        var buffer = Data()
        buffer.reserveCapacity(64 * 1024)

        do {
            for try await data in stream.chunks {
                try Task.checkCancellation()
                buffer.append(data)
                if buffer.count >= 64 * 1024 {
                    try output.write(contentsOf: buffer)
                    totalWritten += UInt64(buffer.count)
                    lastCleanFileSize = totalWritten
                    try validateHTTPRangeProgress(
                        totalWritten: totalWritten,
                        expectedSizeBytes: expectedSizeBytes,
                        rangeEndInclusive: rangeEndInclusive,
                        isPartialResponse: statusCode == 206,
                        sourceURL: sourceURL,
                        destinationURL: destinationURL
                    )
                    buffer.removeAll(keepingCapacity: true)
                    await onProgress(totalWritten)
                }
            }
            if !buffer.isEmpty {
                try output.write(contentsOf: buffer)
                totalWritten += UInt64(buffer.count)
                lastCleanFileSize = totalWritten
                try validateHTTPRangeProgress(
                    totalWritten: totalWritten,
                    expectedSizeBytes: expectedSizeBytes,
                    rangeEndInclusive: rangeEndInclusive,
                    isPartialResponse: statusCode == 206,
                    sourceURL: sourceURL,
                    destinationURL: destinationURL
                )
                buffer.removeAll(keepingCapacity: true)
                await onProgress(totalWritten)
            }
        } catch {
            try? truncateFile(at: destinationURL, to: lastCleanFileSize)
            throw error
        }

        if statusCode == 206 {
            let expectedNextOffset = rangeEndInclusive + 1
            guard totalWritten == expectedNextOffset else {
                throw staleDownloadResource(
                    "downloaded range ended at \(totalWritten) for \(sourceURL.absoluteString), expected \(expectedNextOffset)"
                )
            }
            return totalWritten
        }
        guard totalWritten == expectedSizeBytes else {
            throw staleDownloadResource(
                "downloaded \(totalWritten) bytes for \(sourceURL.absoluteString), expected \(expectedSizeBytes)"
            )
        }
        return totalWritten
    }

    private func validateHTTPRangeProgress(
        totalWritten: UInt64,
        expectedSizeBytes: UInt64,
        rangeEndInclusive: UInt64,
        isPartialResponse: Bool,
        sourceURL: URL,
        destinationURL: URL
    ) throws {
        if totalWritten > expectedSizeBytes {
            try? fileManager.removeItem(at: destinationURL)
            throw staleDownloadResource(
                "remote server sent more bytes than expected for \(sourceURL.absoluteString)"
            )
        }
        if isPartialResponse, totalWritten > rangeEndInclusive + 1 {
            try? fileManager.removeItem(at: destinationURL)
            throw staleDownloadResource(
                "remote server sent more bytes than the requested byte range for \(sourceURL.absoluteString)"
            )
        }
    }

    private func truncateFile(at url: URL, to size: UInt64) throws {
        guard fileManager.fileExists(atPath: url.path) else {
            return
        }
        let output = try FileHandle(forWritingTo: url)
        defer { closeDownloadFileHandle(output, context: "download file truncation") }
        try output.truncate(atOffset: size)
    }

    private func copyFileURL(
        _ sourceURL: URL,
        byteRange: VesperDownloadByteRange?,
        expectedSizeBytes: UInt64?,
        resumeFromBytes: UInt64,
        to destinationURL: URL,
        onProgress: (UInt64) async -> Void
    ) async throws -> UInt64 {
        if !fileManager.fileExists(atPath: destinationURL.path) {
            fileManager.createFile(atPath: destinationURL.path, contents: nil)
        }

        let input = try FileHandle(forReadingFrom: sourceURL)
        let output = try FileHandle(forWritingTo: destinationURL)
        defer {
            closeDownloadFileHandle(input, context: "local file input")
            closeDownloadFileHandle(output, context: "local file output")
        }

        try input.seek(toOffset: (byteRange?.offset ?? 0) + resumeFromBytes)
        if resumeFromBytes > 0 {
            try output.seekToEnd()
        } else {
            try output.truncate(atOffset: 0)
        }

        var totalWritten = resumeFromBytes
        var lastCleanFileSize = resumeFromBytes
        var remaining = byteRange.map { $0.length > resumeFromBytes ? $0.length - resumeFromBytes : 0 }
        do {
            while remaining == nil || remaining! > 0 {
                try Task.checkCancellation()
                let chunkSize = Int(min(UInt64(64 * 1024), remaining ?? UInt64(64 * 1024)))
                let data = try input.read(upToCount: chunkSize) ?? Data()
                if data.isEmpty {
                    break
                }
                try output.write(contentsOf: data)
                let count = UInt64(data.count)
                totalWritten += count
                lastCleanFileSize = totalWritten
                if let currentRemaining = remaining {
                    remaining = currentRemaining > count ? currentRemaining - count : 0
                }
                await onProgress(totalWritten)
            }
        } catch {
            try? truncateFile(at: destinationURL, to: lastCleanFileSize)
            throw error
        }

        if let expectedSizeBytes, totalWritten != expectedSizeBytes {
            throw VesperForegroundDownloadPreparationError.invalidSource(
                "copied \(totalWritten) bytes, expected \(expectedSizeBytes)"
            )
        }
        return totalWritten
    }

    private func resumableExistingBytes(
        at destinationURL: URL,
        expectedSizeBytes: UInt64?
    ) -> UInt64 {
        guard fileManager.fileExists(atPath: destinationURL.path) else {
            return 0
        }
        guard resumePartialDownloads else {
            try? fileManager.removeItem(at: destinationURL)
            return 0
        }
        guard let expectedSizeBytes else {
            try? fileManager.removeItem(at: destinationURL)
            return 0
        }

        let existingBytes = (try? destinationURL.resourceValues(forKeys: [.fileSizeKey]).fileSize)
            .map { UInt64(max($0, 0)) } ?? 0
        if existingBytes == expectedSizeBytes {
            return existingBytes
        }
        if expectedSizeBytes > 1 && existingBytes > 0 && existingBytes < expectedSizeBytes {
            return existingBytes
        }
        try? fileManager.removeItem(at: destinationURL)
        return 0
    }

    private func fetchText(
        _ sourceUri: String,
        requestHeaders: [String: String]
    ) async throws -> String {
        let sourceURL = try resolveURL(sourceUri)
        let data: Data
        if sourceURL.isFileURL {
            data = try Data(contentsOf: sourceURL)
        } else {
            var request = URLRequest(url: sourceURL)
            request.applyDownloadHttpHeaders(requestHeaders)
            let (responseData, response) = try await httpData(for: request, sourceURL: sourceURL)
            if let http = response as? HTTPURLResponse {
                if isExpiredHttpStatus(http.statusCode) {
                    throw staleDownloadResource(
                        "offline download resource is stale or expired (HTTP \(http.statusCode)) for \(sourceURL.absoluteString); refresh the media link and prepare the task again"
                    )
                }
                if !(200..<300).contains(http.statusCode) {
                    throw VesperForegroundDownloadPreparationError.invalidSource(
                        "remote resource returned HTTP \(http.statusCode) for \(sourceURL.absoluteString)"
                    )
                }
            }
            data = responseData
        }
        guard let text = String(data: data, encoding: .utf8) else {
            throw VesperForegroundDownloadPreparationError.invalidSource("remote manifest was not valid UTF-8")
        }
        return text
    }

    private func probeRequiredSize(
        _ sourceUri: String,
        byteRange: VesperDownloadByteRange?,
        requestHeaders: [String: String]
    ) async throws -> UInt64 {
        if let byteRange {
            return byteRange.length
        }
        return try await probeContentLength(try resolveURL(sourceUri), requestHeaders: requestHeaders)
    }

    private func probeContentLength(
        _ sourceURL: URL,
        requestHeaders: [String: String]
    ) async throws -> UInt64 {
        if sourceURL.isFileURL {
            let values = try sourceURL.resourceValues(forKeys: [.fileSizeKey])
            guard let size = values.fileSize, size > 0 else {
                throw CocoaError(.fileReadUnknown)
            }
            return UInt64(size)
        }

        var request = URLRequest(url: sourceURL)
        request.applyDownloadHttpHeaders(requestHeaders)
        request.httpMethod = "HEAD"
        let (_, response) = try await httpData(for: request, sourceURL: sourceURL)
        if let http = response as? HTTPURLResponse,
           isExpiredHttpStatus(http.statusCode) {
            throw staleDownloadResource(
                "offline download resource is stale or expired (HTTP \(http.statusCode)) for \(sourceURL.absoluteString); refresh the media link and prepare the task again"
            )
        }
        if let http = response as? HTTPURLResponse,
           let value = http.value(forHTTPHeaderField: "Content-Length"),
           let size = UInt64(value), size > 0
        {
            return size
        }

        var rangeRequest = URLRequest(url: sourceURL)
        rangeRequest.applyDownloadHttpHeaders(requestHeaders)
        rangeRequest.setValue("bytes=0-0", forHTTPHeaderField: "Range")
        let (_, rangeResponse) = try await httpData(for: rangeRequest, sourceURL: sourceURL)
        if let http = rangeResponse as? HTTPURLResponse,
           isExpiredHttpStatus(http.statusCode) {
            throw staleDownloadResource(
                "offline download resource is stale or expired (HTTP \(http.statusCode)) for \(sourceURL.absoluteString); refresh the media link and prepare the task again"
            )
        }
        if let http = rangeResponse as? HTTPURLResponse,
           let contentRange = parseHttpContentRange(http.value(forHTTPHeaderField: "Content-Range")),
           let size = contentRange.total,
           size > 0
        {
            return size
        }

        throw CocoaError(.fileReadUnknown)
    }

    private func inferredFileName(_ uri: String) -> String {
        let name = URL(string: uri)?.lastPathComponent.trimmingCharacters(in: .whitespacesAndNewlines) ?? ""
        return name.isEmpty ? "media.bin" : name
    }
}
