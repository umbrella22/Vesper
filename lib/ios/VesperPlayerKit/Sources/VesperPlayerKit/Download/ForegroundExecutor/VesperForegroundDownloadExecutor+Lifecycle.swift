import Foundation

extension VesperForegroundDownloadExecutor {
    public func prepare(
        task: VesperDownloadTaskSnapshot,
        reporter: any VesperDownloadExecutionReporter
    ) {
        let work = Task.detached(priority: .utility) { [weak self] in
            guard let self else {
                return
            }
            do {
                let assetIndex = try await self.prepareAssetIndexWithRecovery(task: task, reporter: reporter)
                await reporter.completePreparation(taskId: task.taskId, assetIndex: assetIndex)
            } catch is CancellationError {
                return
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
                self.lock.unlock()
            }
        }
        lock.lock()
        tasks[task.taskId] = work
        lock.unlock()
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

    func launchDownload(
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

    func recoverStaleDownload(
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

    func executionPlan(for task: VesperDownloadTaskSnapshot) throws -> [ForegroundDownloadEntry] {
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
}
