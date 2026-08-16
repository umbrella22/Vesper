import Darwin
import XCTest
@testable import VesperPlayerKit
import VesperPlayerKitBridgeShim

@MainActor
final class VesperDownloadManagerTests: XCTestCase {
    func testDownloadErrorCodableRequiresTypedFields() throws {
        let error = VesperDownloadError(
            code: .backendFailure,
            category: .network,
            retriable: true,
            message: "network stalled"
        )

        let data = try JSONEncoder().encode(error)
        let json = try XCTUnwrap(
            JSONSerialization.jsonObject(with: data) as? [String: Any]
        )
        XCTAssertEqual(json["code"] as? String, "backendFailure")
        XCTAssertEqual(json["category"] as? String, "network")
        XCTAssertEqual(json["retriable"] as? Bool, true)
        XCTAssertEqual(json["message"] as? String, "network stalled")

        let decoded = try JSONDecoder().decode(VesperDownloadError.self, from: data)
        XCTAssertEqual(decoded, error)
    }

    func testDownloadErrorCodableRejectsLegacyOrdinalPayload() {
        let payload: [String: Any] = [
            "code" + "Ordinal": 3,
            "category" + "Ordinal": 2,
            "retriable": false,
            "message": "legacy",
        ]
        let data = try! JSONSerialization.data(withJSONObject: payload)

        XCTAssertThrowsError(try JSONDecoder().decode(VesperDownloadError.self, from: data))
    }

    func testPlayerErrorFfiEnumBridgeMapping() {
        XCTAssertEqual(
            VesperPlayerErrorCode(ffiCode: PlayerFfiErrorCodeBackendFailure),
            .backendFailure
        )
        XCTAssertEqual(
            VesperPlayerErrorCode(ffiCode: PlayerFfiErrorCodeUnsupported),
            .unsupported
        )
        XCTAssertEqual(VesperPlayerErrorCode.timeout.ffiCode, PlayerFfiErrorCodeTimeout)
        XCTAssertEqual(
            VesperPlayerErrorCategory(ffiCategory: PlayerFfiErrorCategoryNetwork),
            .network
        )
        XCTAssertEqual(
            VesperPlayerErrorCategory(ffiCategory: PlayerFfiErrorCategoryCapability),
            .capability
        )
        XCTAssertEqual(
            VesperPlayerErrorCategory.playback.ffiCategory,
            PlayerFfiErrorCategoryPlayback
        )
    }

    func testDownloadBridgeShimTransfersAndFreesRustErrorMessage() throws {
        var config = VesperRuntimeDownloadConfig(
            auto_start: false,
            run_post_processors_on_completion: false,
            plugin_registry_handle: 0,
            post_download_plugin_references_json: nil,
            event_hook_plugin_references_json: nil
        )
        var handle: UInt64 = 99
        var errorMessage: UnsafeMutablePointer<CChar>?

        let created = withUnsafePointer(to: &config) { configPointer in
            withUnsafeMutablePointer(to: &handle) { handlePointer in
                withUnsafeMutablePointer(to: &errorMessage) { errorPointer in
                    vesper_runtime_download_session_create(
                        configPointer,
                        handlePointer,
                        errorPointer
                    )
                }
            }
        }

        XCTAssertFalse(created)
        XCTAssertEqual(handle, 0)
        let transferredMessage = try XCTUnwrap(errorMessage)
        defer { vesper_runtime_download_error_string_free(transferredMessage) }
        XCTAssertEqual(
            String(cString: transferredMessage),
            "config.post_download_plugin_references_json was null"
        )
    }

    /// Guards the main-thread back-pressure fix for `eventBuffer`: the capped
    /// append must keep the buffer at `maxEventBufferCapacity`, preserve FIFO
    /// order (drop oldest), and handle a single pathological drain that exceeds
    /// capacity without growing the buffer first.
    func testEventBufferAppendCapsToCapacityAndPreservesNewest() throws {
        let manager = try VesperDownloadManager(
            configuration: VesperDownloadConfiguration(),
            executor: RecordingDownloadExecutor(),
            bindings: FakeDownloadBindings(autoStart: false)
        )
        defer { manager.dispose() }

        let capacity = manager.maxEventBufferCapacity
        let progressEvent: (UInt64) -> VesperDownloadEvent = { taskId in
            .progressUpdated(
                VesperDownloadTaskProgressPatch(
                    taskId: taskId,
                    progress: VesperDownloadProgressSnapshot()
                )
            )
        }

        // A single batch larger than capacity: only the newest `capacity`
        // events are retained, in order.
        let oversizedBatch = (0..<(capacity + 500)).map { progressEvent(UInt64($0)) }
        manager.appendEventsCapped(oversizedBatch)
        XCTAssertEqual(manager.eventBuffer.count, capacity)

        // A normal-sized batch that overflows by a small amount drops the oldest
        // excess and keeps the newest.
        manager.appendEventsCapped(
            ((capacity + 500)..<(capacity + 510)).map { progressEvent(UInt64($0)) }
        )
        XCTAssertEqual(manager.eventBuffer.count, capacity)

        // A batch well under capacity appends without truncation.
        manager.appendEventsCapped(
            ((capacity + 510)..<(capacity + 515)).map { progressEvent(UInt64($0)) }
        )
        let retainedTaskIds = manager.eventBuffer.compactMap { event -> UInt64? in
            guard case let .progressUpdated(patch) = event else { return nil }
            return patch.taskId
        }
        XCTAssertEqual(retainedTaskIds.count, capacity)
        XCTAssertEqual(retainedTaskIds.first, 515)
        XCTAssertEqual(retainedTaskIds.last, UInt64(capacity + 514))

        let batch = manager.drainEvents()
        XCTAssertTrue(batch.events.isEmpty)
        XCTAssertEqual(batch.droppedEvents, UInt64(capacity + 515))
        XCTAssertTrue(batch.requiresSnapshotResync)
        XCTAssertTrue(batch.snapshotIsAuthoritative)

        let nextBatch = manager.drainEvents()
        XCTAssertTrue(nextBatch.events.isEmpty)
        XCTAssertEqual(nextBatch.droppedEvents, 0)
        XCTAssertFalse(nextBatch.requiresSnapshotResync)
        XCTAssertTrue(nextBatch.snapshotIsAuthoritative)
    }

    func testCreateTaskAutoStartRefreshesSnapshotAndStartsExecutor() throws {
        let bindings = FakeDownloadBindings(autoStart: true)
        let executor = RecordingDownloadExecutor()
        let manager = try VesperDownloadManager(
            configuration: VesperDownloadConfiguration(autoStart: true),
            executor: executor,
            bindings: bindings
        )
        defer { manager.dispose() }

        let taskId = try manager.createTask(
            assetId: "asset-a",
            source: VesperDownloadSource(
                source: .remoteUrl(URL(string: "https://example.com/video.mp4")!, label: "Video")
            ),
            assetIndex: VesperDownloadAssetIndex(totalSizeBytes: 1024)
        )

        XCTAssertEqual(taskId, 1)
        XCTAssertEqual(executor.startedTaskIds, [1])
        XCTAssertEqual(manager.task(1)?.state, .downloading)
        XCTAssertTrue(
            manager.drainEvents().events.contains { event in
                if case .created = event {
                    return true
                }
                return false
            }
        )
    }

    func testDroppedRuntimeEventsRetryAuthoritativeSnapshotResync() throws {
        let bindings = FakeDownloadBindings(autoStart: false)
        let manager = try VesperDownloadManager(
            configuration: VesperDownloadConfiguration(autoStart: false),
            executor: RecordingDownloadExecutor(),
            bindings: bindings
        )
        defer { manager.dispose() }

        let taskId = try XCTUnwrap(
            try manager.createTask(
                assetId: "asset-loss-recovery",
                source: VesperDownloadSource(
                    source: .remoteUrl(
                        URL(string: "https://example.com/video.mp4")!,
                        label: "Video"
                    )
                ),
                assetIndex: VesperDownloadAssetIndex(totalSizeBytes: 1024)
            )
        )
        _ = manager.drainEvents()
        let snapshotCallsBeforeLoss = bindings.snapshotCallCount
        bindings.simulateDroppedCompletion(
            taskId: taskId,
            completedPath: "/tmp/downloads/loss-recovered.mp4"
        )
        bindings.failNextSnapshot()

        manager.refresh()

        XCTAssertEqual(bindings.snapshotCallCount, snapshotCallsBeforeLoss + 1)
        XCTAssertEqual(manager.task(taskId)?.state, .queued)
        XCTAssertTrue(manager.needsAuthoritativeSnapshotResync)
        let lossBatch = manager.drainEvents()
        XCTAssertTrue(lossBatch.events.isEmpty)
        XCTAssertEqual(lossBatch.droppedEvents, 1)
        XCTAssertTrue(lossBatch.requiresSnapshotResync)
        XCTAssertFalse(lossBatch.snapshotIsAuthoritative)

        let pendingLossBatch = manager.drainEvents()
        XCTAssertTrue(pendingLossBatch.events.isEmpty)
        XCTAssertEqual(pendingLossBatch.droppedEvents, 1)
        XCTAssertTrue(pendingLossBatch.requiresSnapshotResync)
        XCTAssertFalse(pendingLossBatch.snapshotIsAuthoritative)

        manager.refresh()

        XCTAssertEqual(bindings.snapshotCallCount, snapshotCallsBeforeLoss + 2)
        XCTAssertEqual(manager.task(taskId)?.state, .completed)
        XCTAssertEqual(
            manager.task(taskId)?.assetIndex.completedPath,
            "/tmp/downloads/loss-recovered.mp4"
        )
        XCTAssertFalse(manager.needsAuthoritativeSnapshotResync)
        let recoveredBatch = manager.drainEvents()
        XCTAssertTrue(recoveredBatch.events.isEmpty)
        XCTAssertEqual(recoveredBatch.droppedEvents, 1)
        XCTAssertTrue(recoveredBatch.requiresSnapshotResync)
        XCTAssertTrue(recoveredBatch.snapshotIsAuthoritative)

        let nextBatch = manager.drainEvents()
        XCTAssertTrue(nextBatch.events.isEmpty)
        XCTAssertEqual(nextBatch.droppedEvents, 0)
        XCTAssertFalse(nextBatch.requiresSnapshotResync)
        XCTAssertTrue(nextBatch.snapshotIsAuthoritative)
    }

    func testMalformedRuntimeEventTriggersSnapshotResyncWithoutApplyingEvent() throws {
        let bindings = FakeDownloadBindings(autoStart: false)
        let manager = try VesperDownloadManager(
            configuration: VesperDownloadConfiguration(autoStart: false),
            executor: RecordingDownloadExecutor(),
            bindings: bindings
        )
        defer { manager.dispose() }

        let taskId = try XCTUnwrap(
            try manager.createTask(
                assetId: "asset-malformed-event",
                source: VesperDownloadSource(
                    source: .remoteUrl(URL(string: "https://example.com/video.mp4")!)
                )
            )
        )
        _ = manager.drainEvents()
        bindings.enqueueCreatedEventWithMalformedStatus(taskId: taskId)

        manager.refresh()

        let batch = manager.drainEvents()
        XCTAssertTrue(batch.events.isEmpty)
        XCTAssertEqual(batch.droppedEvents, 1)
        XCTAssertEqual(manager.task(taskId)?.state, .queued)
        XCTAssertFalse(manager.needsAuthoritativeSnapshotResync)
    }

    func testMalformedAuthoritativeSnapshotIsRejectedAndRetried() throws {
        let bindings = FakeDownloadBindings(autoStart: false)
        let manager = try VesperDownloadManager(
            configuration: VesperDownloadConfiguration(autoStart: false),
            executor: RecordingDownloadExecutor(),
            bindings: bindings
        )
        defer { manager.dispose() }

        let taskId = try XCTUnwrap(
            try manager.createTask(
                assetId: "asset-malformed-snapshot",
                source: VesperDownloadSource(
                    source: .remoteUrl(URL(string: "https://example.com/video.mp4")!)
                )
            )
        )
        _ = manager.drainEvents()
        bindings.simulateDroppedCompletion(
            taskId: taskId,
            completedPath: "/tmp/downloads/malformed-snapshot.mp4"
        )
        bindings.returnMalformedStatusFromNextSnapshot()

        manager.refresh()

        XCTAssertEqual(manager.task(taskId)?.state, .queued)
        XCTAssertTrue(manager.needsAuthoritativeSnapshotResync)

        manager.refresh()

        XCTAssertEqual(manager.task(taskId)?.state, .completed)
        XCTAssertFalse(manager.needsAuthoritativeSnapshotResync)
    }

    func testRuntimeDownloadDecodeRejectsNestedPointerCountMismatch() {
        var task = makeStrictDecodingRuntimeTask()
        task.asset_index.resources_len = 1
        XCTAssertNil(task.decodePublic())
        freeRuntimeDownloadTask(&task)

        var profileTask = makeStrictDecodingRuntimeTask()
        profileTask.profile.selected_track_ids_len = 1
        XCTAssertNil(profileTask.decodePublic())
        freeRuntimeDownloadTask(&profileTask)

        let malformedSnapshot = VesperRuntimeDownloadSnapshot(tasks: nil, len: 1)
        XCTAssertNil(malformedSnapshot.decodePublic())
    }

    func testRuntimeDownloadDecodeRejectsMalformedNestedRequiredValues() {
        var resourceTask = makeStrictDecodingRuntimeTask(
            assetIndex: VesperDownloadAssetIndex(
                resources: [
                    VesperDownloadResourceRecord(
                        resourceId: "resource",
                        uri: "https://example.com/resource"
                    ),
                ]
            )
        )
        if let resources = resourceTask.asset_index.resources {
            freeDownloadCString(resources[0].resource_id)
            resources[0].resource_id = nil
        }
        XCTAssertNil(resourceTask.decodePublic())
        freeRuntimeDownloadTask(&resourceTask)

        var streamTask = makeStrictDecodingRuntimeTask(
            assetIndex: VesperDownloadAssetIndex(
                streams: [VesperDownloadAssetStream(streamId: "stream")]
            )
        )
        if let streams = streamTask.asset_index.streams {
            streams[0].kind = VesperRuntimeDownloadStreamKind(rawValue: 99)
        }
        XCTAssertNil(streamTask.decodePublic())
        freeRuntimeDownloadTask(&streamTask)

        var outputTask = makeStrictDecodingRuntimeTask(
            profile: VesperDownloadProfile(targetOutputFormat: .mp4)
        )
        outputTask.profile.target_output_format = VesperRuntimeDownloadOutputFormat(rawValue: 99)
        XCTAssertNil(outputTask.decodePublic())
        freeRuntimeDownloadTask(&outputTask)
    }

    func testRuntimeDownloadDecodeRejectsNullNestedStringEntries() {
        var streamTask = makeStrictDecodingRuntimeTask(
            assetIndex: VesperDownloadAssetIndex(
                streams: [
                    VesperDownloadAssetStream(
                        streamId: "stream",
                        resourceIds: ["resource"]
                    ),
                ]
            )
        )
        if let streams = streamTask.asset_index.streams,
           let resourceIds = streams[0].resource_ids
        {
            freeDownloadCString(resourceIds[0])
            resourceIds[0] = nil
        }
        XCTAssertNil(streamTask.decodePublic())
        freeRuntimeDownloadTask(&streamTask)
    }

    func testMalformedCommandPoisonsProcessingAndRejectsLaterBatches() throws {
        let bindings = FakeDownloadBindings(autoStart: false)
        let executor = RecordingDownloadExecutor()
        let manager = try VesperDownloadManager(
            configuration: VesperDownloadConfiguration(autoStart: false),
            executor: executor,
            bindings: bindings
        )
        defer { manager.dispose() }

        let taskId = try XCTUnwrap(
            try manager.createTask(
                assetId: "asset-malformed-command",
                source: VesperDownloadSource(
                    source: .remoteUrl(URL(string: "https://example.com/video.mp4")!)
                )
            )
        )
        bindings.enqueueMismatchedRemoveCommand(taskId: taskId)

        manager.refresh()

        XCTAssertTrue(executor.removedTaskIds.isEmpty)
        XCTAssertEqual(bindings.acknowledgeCommandCallCount, 1)
        XCTAssertNotNil(manager.runtimeCommandDiagnostic)

        bindings.enqueueRemoveCommand(taskId: taskId)
        manager.refresh()

        XCTAssertTrue(executor.removedTaskIds.isEmpty)
        XCTAssertEqual(bindings.acknowledgeCommandCallCount, 1)
    }

    func testSynchronousPrepareReporterDoesNotReenterThePendingCommandBatch() throws {
        let bindings = FakeDownloadBindings(autoStart: false)
        let executor = RecordingDownloadExecutor()
        let manager = try VesperDownloadManager(
            configuration: VesperDownloadConfiguration(autoStart: false),
            executor: executor,
            bindings: bindings
        )
        defer { manager.dispose() }

        let taskId = try XCTUnwrap(try manager.createTask(
            assetId: "asset-reentrant-prepare",
            source: VesperDownloadSource(
                source: .remoteUrl(URL(string: "https://example.com/video.mp4")!)
            )
        ))
        bindings.enqueuePrepareCommand(taskId: taskId)
        manager.refresh()

        XCTAssertEqual(executor.preparedSourceHeaders.count, 1)
        XCTAssertEqual(executor.startedTaskIds, [taskId])
        XCTAssertEqual(bindings.acknowledgeCommandCallCount, 2)
        XCTAssertFalse(manager.isProcessingRuntimeCommands)
        XCTAssertEqual(manager.pendingRuntimeCommandAcknowledgementCount, 0)
    }

    func testAcknowledgementFailureRetriesAckWithoutRepeatingCommandSideEffects() throws {
        let bindings = FakeDownloadBindings(autoStart: false)
        let executor = RecordingDownloadExecutor()
        let manager = try VesperDownloadManager(
            configuration: VesperDownloadConfiguration(autoStart: false),
            executor: executor,
            bindings: bindings
        )
        defer { manager.dispose() }
        let taskId = try XCTUnwrap(try manager.createTask(
            assetId: "asset-ack-retry",
            source: VesperDownloadSource(
                source: .remoteUrl(URL(string: "https://example.com/video.mp4")!)
            )
        ))
        bindings.enqueuePauseCommand(taskId: taskId)
        bindings.failNextCommandAcknowledgement()
        manager.refresh()

        XCTAssertEqual(executor.pausedTaskIds, [taskId])
        XCTAssertEqual(bindings.acknowledgeCommandCallCount, 1)
        XCTAssertEqual(manager.pendingRuntimeCommandAcknowledgementCount, 1)

        manager.refresh()

        XCTAssertEqual(executor.pausedTaskIds, [taskId])
        XCTAssertEqual(bindings.acknowledgeCommandCallCount, 2)
        XCTAssertEqual(manager.pendingRuntimeCommandAcknowledgementCount, 0)
    }

    func testCommandBatchLimitAcknowledgesTheLastAppliedBatch() throws {
        let bindings = FakeDownloadBindings(autoStart: false)
        let executor = RecordingDownloadExecutor()
        let manager = try VesperDownloadManager(
            configuration: VesperDownloadConfiguration(autoStart: false),
            executor: executor,
            bindings: bindings
        )
        defer { manager.dispose() }
        let taskId = try XCTUnwrap(try manager.createTask(
            assetId: "asset-batch-limit",
            source: VesperDownloadSource(
                source: .remoteUrl(URL(string: "https://example.com/video.mp4")!)
            )
        ))
        bindings.commandBatchSize = 1
        for _ in 0..<manager.maxRuntimeCommandBatchesPerSync {
            bindings.enqueuePauseCommand(taskId: taskId)
        }

        manager.refresh()

        XCTAssertEqual(
            executor.pausedTaskIds,
            Array(repeating: taskId, count: manager.maxRuntimeCommandBatchesPerSync)
        )
        XCTAssertEqual(
            bindings.acknowledgeCommandCallCount,
            manager.maxRuntimeCommandBatchesPerSync
        )
        XCTAssertEqual(manager.pendingRuntimeCommandAcknowledgementCount, 0)
    }

    func testSourceHeadersSurviveNativeDownloadCommandRoundTrip() throws {
        let bindings = FakeDownloadBindings(autoStart: true)
        let executor = RecordingDownloadExecutor()
        let manager = try VesperDownloadManager(
            configuration: VesperDownloadConfiguration(autoStart: true),
            executor: executor,
            bindings: bindings
        )
        defer { manager.dispose() }

        _ = try manager.createTask(
            assetId: "asset-a",
            source: VesperDownloadSource(
                source: .hls(
                    url: URL(string: "https://example.com/video.m3u8")!,
                    label: "Video",
                    headers: [
                        "User-Agent": "VesperTest/1.0",
                        "Referer": "https://example.com/player",
                        "": "ignored",
                        "Origin": "",
                    ]
                )
            )
        )

        let expected = [
            "User-Agent": "VesperTest/1.0",
            "Referer": "https://example.com/player",
        ]
        XCTAssertEqual(executor.startedSourceHeaders, [expected])
        XCTAssertEqual(manager.task(1)?.source.source.headers, expected)
    }

    func testCreateTaskRejectsDrmSourceWithTypedUnsupportedError() throws {
        let manager = try VesperDownloadManager(
            configuration: VesperDownloadConfiguration(autoStart: false),
            executor: RecordingDownloadExecutor(),
            bindings: FakeDownloadBindings(autoStart: false)
        )
        defer { manager.dispose() }

        XCTAssertThrowsError(
            try manager.createTask(
                assetId: "asset-drm",
                source: VesperDownloadSource(
                    source: .hls(
                        url: URL(string: "https://example.com/drm.m3u8")!,
                        label: "DRM",
                        drmConfiguration: VesperPlayerDrmConfiguration(
                            keySystem: "fairPlay",
                            licenseUri: "https://license.example.com/fairplay"
                        )
                    )
                )
            )
        ) { error in
            let drmError = error as? VesperPlayerDrmUnsupportedError
            XCTAssertEqual(drmError?.route, "download")
            XCTAssertEqual(drmError?.keySystem, "fairPlay")
            XCTAssertEqual(drmError?.reason, "drmUnsupportedRoute")
        }
    }

    func testRestoreTasksRejectsDrmSourceWithTypedUnsupportedError() throws {
        let manager = try VesperDownloadManager(
            configuration: VesperDownloadConfiguration(autoStart: false),
            executor: RecordingDownloadExecutor(),
            bindings: FakeDownloadBindings(autoStart: false)
        )
        defer { manager.dispose() }
        let task =
            VesperDownloadTaskSnapshot(
                taskId: 7,
                assetId: "asset-drm",
                source: VesperDownloadSource(
                    source: .hls(
                        url: URL(string: "https://example.com/drm.m3u8")!,
                        label: "DRM",
                        drmConfiguration: VesperPlayerDrmConfiguration(
                            keySystem: "fairPlay",
                            licenseUri: "https://license.example.com/fairplay"
                        )
                    )
                ),
                profile: VesperDownloadProfile(),
                state: .queued,
                progress: VesperDownloadProgressSnapshot(),
                assetIndex: VesperDownloadAssetIndex()
            )

        XCTAssertThrowsError(try manager.restoreTasks([task])) { error in
            let drmError = error as? VesperPlayerDrmUnsupportedError
            XCTAssertEqual(drmError?.route, "download")
            XCTAssertEqual(drmError?.keySystem, "fairPlay")
            XCTAssertEqual(drmError?.reason, "drmUnsupportedRoute")
        }
    }

    func testPauseResumeAndRemoveDelegateToExecutorWithoutForkingStateMachine() throws {
        let bindings = FakeDownloadBindings(autoStart: true)
        let executor = RecordingDownloadExecutor()
        let manager = try VesperDownloadManager(
            configuration: VesperDownloadConfiguration(autoStart: true),
            executor: executor,
            bindings: bindings
        )
        defer { manager.dispose() }

        _ = try manager.createTask(
            assetId: "asset-a",
            source: VesperDownloadSource(
                source: .remoteUrl(URL(string: "https://example.com/video.mp4")!, label: "Video")
            )
        )

        XCTAssertTrue(manager.pauseTask(1))
        XCTAssertEqual(executor.pausedTaskIds, [1])
        XCTAssertEqual(manager.task(1)?.state, .paused)

        XCTAssertTrue(manager.resumeTask(1))
        XCTAssertEqual(executor.resumedTaskIds, [1])
        XCTAssertEqual(manager.task(1)?.state, .downloading)

        XCTAssertTrue(manager.removeTask(1))
        XCTAssertEqual(executor.removedTaskIds, [1])
        XCTAssertNil(manager.task(1))
    }

    func testExecutorReporterUpdatesSharedSnapshotProgressAndCompletion() throws {
        let bindings = FakeDownloadBindings(autoStart: true)
        let executor = RecordingDownloadExecutor(autoComplete: true)
        let manager = try VesperDownloadManager(
            configuration: VesperDownloadConfiguration(autoStart: true),
            executor: executor,
            bindings: bindings
        )
        defer { manager.dispose() }

        _ = try manager.createTask(
            assetId: "asset-a",
            source: VesperDownloadSource(
                source: .remoteUrl(URL(string: "https://example.com/video.mp4")!, label: "Video")
            ),
            assetIndex: VesperDownloadAssetIndex(totalSizeBytes: 512)
        )

        let task = manager.task(1)
        XCTAssertNotNil(task)
        XCTAssertEqual(task?.state, .completed)
        XCTAssertEqual(task?.progress.receivedBytes, 512)
        XCTAssertEqual(task?.assetIndex.completedPath, "/tmp/downloads/1.bin")
    }

    func testCapabilitySpecificPluginReferencesAreForwardedToBindingsConfiguration() throws {
        let bindings = FakeDownloadBindings(autoStart: false)
        let postDownloadReference = try VesperPluginReference(
            pluginId: "io.github.umbrella22.vesper.remux-ffmpeg",
            capabilityInstanceId: "io.github.umbrella22.vesper.remux-ffmpeg.post-download",
            transport: .native
        )
        let eventHookReference = try VesperPluginReference(
            pluginId: "dev.vesper.event-hook",
            capabilityInstanceId: "dev.vesper.event-hook.download",
            transport: .native
        )
        let manager = try VesperDownloadManager(
            configuration: VesperDownloadConfiguration(
                autoStart: false,
                runPostProcessorsOnCompletion: false,
                postDownloadPluginReferences: [postDownloadReference],
                eventHookPluginReferences: [eventHookReference]
            ),
            executor: RecordingDownloadExecutor(),
            bindings: bindings
        )
        defer { manager.dispose() }

        XCTAssertEqual(
            bindings.createdConfiguration?.postDownloadPluginReferences,
            [postDownloadReference]
        )
        XCTAssertEqual(
            bindings.createdConfiguration?.eventHookPluginReferences,
            [eventHookReference]
        )
        XCTAssertEqual(bindings.createdConfiguration?.runPostProcessorsOnCompletion, false)
    }

    func testInitializationPropagatesSessionCreationErrorAndDisposesExecutor() {
        let bindings = FakeDownloadBindings(
            autoStart: false,
            sessionCreationError: .createFailed
        )
        let executor = RecordingDownloadExecutor()

        XCTAssertThrowsError(try VesperDownloadManager(
            configuration: VesperDownloadConfiguration(autoStart: false),
            executor: executor,
            bindings: bindings
        )) { error in
            XCTAssertEqual(error as? StubDownloadBindingsError, .createFailed)
        }
        XCTAssertEqual(executor.disposeCount, 1)
        XCTAssertEqual(bindings.disposeSessionCount, 0)
        XCTAssertEqual(bindings.snapshotCallCount, 0)
        XCTAssertEqual(bindings.drainCommandCallCount, 0)
        XCTAssertEqual(bindings.drainEventCallCount, 0)
    }

    func testInitializationRejectsZeroSessionHandleAndDisposesExecutor() {
        let bindings = FakeDownloadBindings(autoStart: false, sessionHandle: 0)
        let executor = RecordingDownloadExecutor()

        XCTAssertThrowsError(try VesperDownloadManager(
            configuration: VesperDownloadConfiguration(autoStart: false),
            executor: executor,
            bindings: bindings
        )) { error in
            XCTAssertEqual(
                error as? VesperDownloadManagerInitializationError,
                .invalidSessionHandle
            )
        }
        XCTAssertEqual(executor.disposeCount, 1)
        XCTAssertEqual(bindings.disposeSessionCount, 0)
        XCTAssertEqual(bindings.snapshotCallCount, 0)
        XCTAssertEqual(bindings.drainCommandCallCount, 0)
        XCTAssertEqual(bindings.drainEventCallCount, 0)
    }

    func testPreparedDownloadOutputURLUsesUniqueTempPathForSameFileName() throws {
        let baseDirectory = FileManager.default.temporaryDirectory
            .appendingPathComponent("vesper-output-test-\(UUID().uuidString)", isDirectory: true)
        try FileManager.default.createDirectory(at: baseDirectory, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: baseDirectory) }

        let firstSource = baseDirectory.appendingPathComponent("first.bin")
        let secondSource = baseDirectory.appendingPathComponent("second.bin")
        try Data("first".utf8).write(to: firstSource)
        try Data("second".utf8).write(to: secondSource)

        let firstPrepared = try prepareDownloadOutputURLFromSource(
            sourceURL: firstSource,
            fileName: "shared-name.mp4"
        )
        let secondPrepared = try prepareDownloadOutputURLFromSource(
            sourceURL: secondSource,
            fileName: "shared-name.mp4"
        )
        defer {
            try? FileManager.default.removeItem(at: firstPrepared.deletingLastPathComponent())
            try? FileManager.default.removeItem(at: secondPrepared.deletingLastPathComponent())
        }

        XCTAssertEqual(firstPrepared.lastPathComponent, "shared-name.mp4")
        XCTAssertEqual(secondPrepared.lastPathComponent, "shared-name.mp4")
        XCTAssertNotEqual(firstPrepared, secondPrepared)
        XCTAssertEqual(try Data(contentsOf: firstPrepared), Data("first".utf8))
        XCTAssertEqual(try Data(contentsOf: secondPrepared), Data("second".utf8))
    }

    func testNativeBridgeMaterializesGeneratedTextWithoutReturningBody() throws {
        let baseDirectory = FileManager.default.temporaryDirectory
            .appendingPathComponent("vesper-native-download-\(UUID().uuidString)", isDirectory: true)
        defer { try? FileManager.default.removeItem(at: baseDirectory) }

        let manager = try VesperDownloadManager(
            configuration: VesperDownloadConfiguration(
                autoStart: false,
                restoreTasksOnStartup: false,
                baseDirectory: baseDirectory
            ),
            executor: RecordingDownloadExecutor()
        )
        defer { manager.dispose() }

        let generatedBody = String(repeating: "<S id=\"segment\" />", count: 1024)
        let taskId = try XCTUnwrap(manager.createTask(
            assetId: "asset-generated",
            source: VesperDownloadSource(
                source: .dash(
                    url: URL(string: "https://example.com/manifest.mpd")!,
                    label: "DASH"
                )
            ),
            assetIndex: VesperDownloadAssetIndex(
                contentFormat: .dashSegments,
                resources: [
                    VesperDownloadResourceRecord(
                        resourceId: "manifest",
                        uri: "generated://manifest",
                        relativePath: "manifest.mpd",
                        generatedText: generatedBody
                    ),
                ]
            )
        ))

        let task = try XCTUnwrap(manager.task(taskId))
        let resource = try XCTUnwrap(task.assetIndex.resources.first)
        XCTAssertNil(resource.generatedText)
        XCTAssertTrue(resource.uri.hasPrefix("file://"))
        XCTAssertEqual(resource.relativePath, "manifest.mpd")
        XCTAssertEqual(resource.sizeBytes, UInt64(generatedBody.utf8.count))
        let materializedURL = try XCTUnwrap(URL(string: resource.uri))
        XCTAssertEqual(
            try materializedURL.resourceValues(forKeys: [.isExcludedFromBackupKey]).isExcludedFromBackup,
            true
        )
    }

    func testForegroundExecutorRejectsInsecureHTTPManifestBeforeATS() async throws {
        let baseDirectory = FileManager.default.temporaryDirectory
            .appendingPathComponent("vesper-http-manifest-\(UUID().uuidString)", isDirectory: true)
        defer { try? FileManager.default.removeItem(at: baseDirectory) }
        let executor = VesperForegroundDownloadExecutor(baseDirectory: baseDirectory)
        defer { executor.dispose() }
        let failure = expectation(description: "insecure manifest should fail")
        let reporter = DownloadReporterProbe(failureExpectation: failure)
        let task = VesperDownloadTaskSnapshot(
            taskId: 1,
            assetId: "asset-http",
            source: VesperDownloadSource(
                source: .hls(
                    url: URL(
                        string:
                            "http://viewer:password@cdn.example.com:8080/index.m3u8?deadline=123&upsig=secret#fragment"
                    )!,
                    label: "HTTP HLS"
                )
            ),
            profile: VesperDownloadProfile(),
            state: .preparing,
            progress: VesperDownloadProgressSnapshot(),
            assetIndex: VesperDownloadAssetIndex()
        )

        executor.prepare(task: task, reporter: reporter)
        await fulfillment(of: [failure], timeout: 2)

        XCTAssertTrue(reporter.failure?.message.contains("App Transport Security") == true)
        XCTAssertTrue(
            reporter.failure?.message.contains("http://cdn.example.com:8080/index.m3u8") == true
        )
        XCTAssertFalse(reporter.failure?.message.contains("password") == true)
        XCTAssertFalse(reporter.failure?.message.contains("upsig") == true)
        XCTAssertFalse(reporter.failure?.message.contains("secret") == true)
    }

    func testForegroundExecutorRejectsInsecureHTTPSizeProbeBeforeATS() async throws {
        let baseDirectory = FileManager.default.temporaryDirectory
            .appendingPathComponent("vesper-http-probe-\(UUID().uuidString)", isDirectory: true)
        defer { try? FileManager.default.removeItem(at: baseDirectory) }
        let executor = VesperForegroundDownloadExecutor(baseDirectory: baseDirectory)
        defer { executor.dispose() }
        let task = VesperDownloadTaskSnapshot(
            taskId: 2,
            assetId: "asset-http-probe",
            source: VesperDownloadSource(
                source: .remoteUrl(URL(string: "https://example.com/video.mp4")!, label: "Video")
            ),
            profile: VesperDownloadProfile(),
            state: .preparing,
            progress: VesperDownloadProgressSnapshot(),
            assetIndex: VesperDownloadAssetIndex(
                contentFormat: .singleFile,
                resources: [
                    VesperDownloadResourceRecord(
                        resourceId: "video",
                        uri:
                            "http://viewer:password@cdn.example.com:8080/video.mp4?deadline=123&upsig=secret#fragment",
                        relativePath: "video.mp4"
                    ),
                ]
            )
        )

        do {
            _ = try await executor.prepareAssetIndex(task: task)
            XCTFail("insecure size probe should fail")
        } catch {
            let message = error.localizedDescription
            XCTAssertTrue(message.contains("App Transport Security"))
            XCTAssertTrue(message.contains("http://cdn.example.com:8080/video.mp4"))
            XCTAssertFalse(message.contains("password"))
            XCTAssertFalse(message.contains("upsig"))
            XCTAssertFalse(message.contains("secret"))
        }
    }

    func testForegroundExecutorRejectsInsecureHTTPMediaTransferBeforeATS() async throws {
        let baseDirectory = FileManager.default.temporaryDirectory
            .appendingPathComponent("vesper-http-transfer-\(UUID().uuidString)", isDirectory: true)
        defer { try? FileManager.default.removeItem(at: baseDirectory) }
        let executor = VesperForegroundDownloadExecutor(baseDirectory: baseDirectory)
        defer { executor.dispose() }
        let failure = expectation(description: "insecure media transfer should fail")
        let reporter = DownloadReporterProbe(failureExpectation: failure)
        let task = VesperDownloadTaskSnapshot(
            taskId: 3,
            assetId: "asset-http-transfer",
            source: VesperDownloadSource(
                source: .remoteUrl(URL(string: "https://example.com/video.mp4")!, label: "Video")
            ),
            profile: VesperDownloadProfile(),
            state: .downloading,
            progress: VesperDownloadProgressSnapshot(totalBytes: 4),
            assetIndex: VesperDownloadAssetIndex(
                contentFormat: .singleFile,
                totalSizeBytes: 4,
                resources: [
                    VesperDownloadResourceRecord(
                        resourceId: "video",
                        uri:
                            "http://viewer:password@cdn.example.com:8080/video.mp4?deadline=123&upsig=secret#fragment",
                        relativePath: "video.mp4",
                        sizeBytes: 4
                    ),
                ]
            )
        )

        executor.start(task: task, reporter: reporter)
        await fulfillment(of: [failure], timeout: 2)

        XCTAssertTrue(reporter.failure?.message.contains("App Transport Security") == true)
        XCTAssertTrue(
            reporter.failure?.message.contains("http://cdn.example.com:8080/video.mp4") == true
        )
        XCTAssertFalse(reporter.failure?.message.contains("password") == true)
        XCTAssertFalse(reporter.failure?.message.contains("upsig") == true)
        XCTAssertFalse(reporter.failure?.message.contains("secret") == true)
    }

    func testHTTPContentRangeParserHandlesConcreteAndUnsatisfiedRanges() throws {
        XCTAssertEqual(
            parseHttpContentRange("bytes 10-19/1024"),
            VesperHTTPContentRange(start: 10, end: 19, total: 1024)
        )
        XCTAssertEqual(
            parseHttpContentRange("bytes */1024"),
            VesperHTTPContentRange(start: nil, end: nil, total: 1024)
        )
        XCTAssertNil(parseHttpContentRange("items 10-19/1024"))
        XCTAssertNil(parseHttpContentRange("bytes 19-10/1024"))
    }

    func testHTTPPartialContentRangeValidationRejectsMalformedAndMismatchedHeaders() throws {
        let signedSource = URL(
            string:
                "https://viewer:password@cdn.example.com:8443/video.mp4?deadline=123&upsig=secret#fragment"
        )!
        XCTAssertNoThrow(try validateHTTPPartialContentRange(
            contentRangeHeader: "bytes 100-199/1000",
            contentLengthHeader: "100",
            requestedStart: 100,
            requestedEndInclusive: 199,
            expectedBodyLength: 100,
            expectedTotalSizeBytes: 1000,
            sourceURL: signedSource
        ))
        XCTAssertThrowsError(try validateHTTPPartialContentRange(
            contentRangeHeader: "bytes */1000",
            contentLengthHeader: "0",
            requestedStart: 100,
            requestedEndInclusive: 199,
            expectedBodyLength: 100,
            expectedTotalSizeBytes: 1000,
            sourceURL: signedSource
        )) { error in
            XCTAssertTrue(error.localizedDescription.contains("https://cdn.example.com:8443/video.mp4"))
            XCTAssertFalse(error.localizedDescription.contains("password"))
            XCTAssertFalse(error.localizedDescription.contains("upsig"))
            XCTAssertFalse(error.localizedDescription.contains("secret"))
        }
        XCTAssertThrowsError(try validateHTTPPartialContentRange(
            contentRangeHeader: "bytes 0-999/1000",
            contentLengthHeader: "1000",
            requestedStart: 100,
            requestedEndInclusive: 199,
            expectedBodyLength: 100,
            expectedTotalSizeBytes: 1000,
            sourceURL: signedSource
        ))
        XCTAssertThrowsError(try validateHTTPPartialContentRange(
            contentRangeHeader: "bytes 100-199/1000",
            contentLengthHeader: "101",
            requestedStart: 100,
            requestedEndInclusive: 199,
            expectedBodyLength: 100,
            expectedTotalSizeBytes: 1000,
            sourceURL: signedSource
        ))
    }

    func testExpiredDownloadResourceRedactsMessageAndPreservesRecoveryUri() {
        let sourceURL = URL(
            string:
                "https://viewer:password@cdn.example.com:8443/video.mp4?deadline=123&upsig=secret#fragment"
        )!
        let error = expiredDownloadResource(
            sourceURL: sourceURL,
            statusCode: 403,
            phase: .prepare,
            receivedBytes: 128
        )
        let staleResource = error.staleResource(taskId: 42, phase: .prepare)

        XCTAssertEqual(staleResource.uri, sourceURL.absoluteString)
        XCTAssertEqual(staleResource.phase, .prepare)
        XCTAssertEqual(staleResource.statusCode, 403)
        XCTAssertEqual(staleResource.receivedBytes, 128)
        XCTAssertTrue(staleResource.message.contains("https://cdn.example.com:8443/video.mp4"))
        XCTAssertFalse(staleResource.message.contains("password"))
        XCTAssertFalse(staleResource.message.contains("upsig"))
        XCTAssertFalse(staleResource.message.contains("secret"))
    }

    func testExportTaskOutputForwardsProgressAndCancellationToBindings() async throws {
        let bindings = FakeDownloadBindings(autoStart: false)
        let manager = try VesperDownloadManager(
            configuration: VesperDownloadConfiguration(autoStart: false),
            executor: RecordingDownloadExecutor(),
            bindings: bindings
        )
        defer { manager.dispose() }

        let taskId = try manager.createTask(
            assetId: "asset-a",
            source: VesperDownloadSource(
                source: .remoteUrl(URL(string: "https://example.com/video.m3u8")!, label: "Video")
            )
        )

        try await manager.exportTaskOutput(
            taskId: taskId ?? 0,
            outputPath: "/tmp/exported.mp4",
            onProgress: { ratio in
                bindings.forwardedProgress.append(ratio)
            },
            isCancelled: { true }
        )

        XCTAssertEqual(bindings.forwardedProgress, [0.25, 1.0])
        XCTAssertEqual(bindings.exportWasCancelled, true)
    }
}

private enum StubDownloadBindingsError: Error, Equatable {
    case createFailed
}

private final class FakeDownloadBindings: @unchecked Sendable, DownloadBindings {
    private let autoStart: Bool
    private let sessionHandle: UInt64
    private let sessionCreationError: StubDownloadBindingsError?
    private var tasks: [UInt64: StoredDownloadTask] = [:]
    private var commands: [StoredRuntimeCommand] = []
    private var pendingCommands: [StoredRuntimeCommand] = []
    private var events: [StoredRuntimeEvent] = []
    private var droppedEvents: UInt64 = 0
    private var snapshotFailuresRemaining = 0
    private var malformedSnapshotStatusesRemaining = 0
    private var malformedCreatedEventStatusesRemaining = 0
    private var acknowledgementFailuresRemaining = 0
    var commandBatchSize: Int?
    private var nextTaskId: UInt64 = 1
    private(set) var createdConfiguration: VesperDownloadConfiguration?
    private(set) var disposeSessionCount = 0
    private(set) var snapshotCallCount = 0
    private(set) var drainCommandCallCount = 0
    private(set) var acknowledgeCommandCallCount = 0
    private(set) var drainEventCallCount = 0
    var forwardedProgress: [Float] = []
    var exportWasCancelled = false

    init(
        autoStart: Bool,
        sessionHandle: UInt64 = 17,
        sessionCreationError: StubDownloadBindingsError? = nil
    ) {
        self.autoStart = autoStart
        self.sessionHandle = sessionHandle
        self.sessionCreationError = sessionCreationError
    }

    func createDownloadSession(configuration: VesperDownloadConfiguration) throws -> UInt64 {
        createdConfiguration = configuration
        if let sessionCreationError {
            throw sessionCreationError
        }
        return sessionHandle
    }

    func disposeDownloadSession(_ sessionHandle: UInt64) {
        disposeSessionCount += 1
    }

    func createDownloadTask(
        sessionHandle: UInt64,
        assetId: String,
        source: UnsafePointer<VesperRuntimeDownloadSource>,
        profile: UnsafePointer<VesperRuntimeDownloadProfile>,
        assetIndex: UnsafePointer<VesperRuntimeDownloadAssetIndex>,
        outTaskId: UnsafeMutablePointer<UInt64>
    ) -> Bool {
        let taskId = nextTaskId
        nextTaskId += 1

        let storedTask = StoredDownloadTask(
            taskId: taskId,
            assetId: assetId,
            sourceUri: stringFromOptionalRuntimeCString(source.pointee.source_uri) ?? "",
            contentFormat: source.pointee.content_format,
            manifestUri: stringFromOptionalRuntimeCString(source.pointee.manifest_uri),
            sourceHeaders: runtimeDownloadSourceHeaders(source.pointee),
            status: autoStart ? .preparing : .queued,
            totalBytes: assetIndex.pointee.has_total_size_bytes ? assetIndex.pointee.total_size_bytes : nil,
            receivedBytes: 0,
            totalSegments: assetIndex.pointee.segments_len > 0 ? UInt32(assetIndex.pointee.segments_len) : nil,
            receivedSegments: 0,
            completedPath: stringFromOptionalRuntimeCString(assetIndex.pointee.completed_path),
            error: nil,
            profileTargetDirectory: stringFromOptionalRuntimeCString(profile.pointee.target_directory)
        )
        tasks[taskId] = storedTask
        events.append(.init(kind: .created, task: storedTask))
        events.append(.init(kind: .stateChanged, task: storedTask))
        if autoStart {
            commands.append(.prepare(storedTask))
        }
        outTaskId.pointee = taskId
        return true
    }

    func restoreDownloadTasks(
        sessionHandle: UInt64,
        tasks: UnsafePointer<VesperRuntimeDownloadTask>?,
        taskCount: Int
    ) -> Bool {
        guard let tasks else {
            return taskCount == 0
        }
        for index in 0..<taskCount {
            let task = tasks[index]
            let storedTask = StoredDownloadTask(
                taskId: task.task_id,
                assetId: stringFromOptionalRuntimeCString(task.asset_id) ?? "",
                sourceUri: stringFromOptionalRuntimeCString(task.source.source_uri) ?? "",
                contentFormat: task.source.content_format,
                manifestUri: stringFromOptionalRuntimeCString(task.source.manifest_uri),
                sourceHeaders: runtimeDownloadSourceHeaders(task.source),
                status: task.status.toDownloadState(),
                totalBytes: task.progress.has_total_bytes ? task.progress.total_bytes : nil,
                receivedBytes: task.progress.received_bytes,
                totalSegments: task.progress.has_total_segments ? task.progress.total_segments : nil,
                receivedSegments: task.progress.received_segments,
                completedPath: stringFromOptionalRuntimeCString(task.asset_index.completed_path),
                error: nil,
                profileTargetDirectory: stringFromOptionalRuntimeCString(task.profile.target_directory)
            )
            self.tasks[storedTask.taskId] = storedTask
            nextTaskId = max(nextTaskId, storedTask.taskId + 1)
        }
        return true
    }

    func startDownloadTask(sessionHandle: UInt64, taskId: UInt64) -> Bool {
        updateTask(taskId) { task in
            let updated = task.with(status: .preparing)
            commands.append(.prepare(updated))
            events.append(.init(kind: .stateChanged, task: updated))
            return updated
        }
    }

    func pauseDownloadTask(sessionHandle: UInt64, taskId: UInt64) -> Bool {
        updateTask(taskId) { task in
            let updated = task.with(status: .paused)
            commands.append(.pause(taskId))
            events.append(.init(kind: .stateChanged, task: updated))
            return updated
        }
    }

    func resumeDownloadTask(sessionHandle: UInt64, taskId: UInt64) -> Bool {
        updateTask(taskId) { task in
            let updated = task.with(status: .downloading)
            commands.append(.resume(updated))
            events.append(.init(kind: .stateChanged, task: updated))
            return updated
        }
    }

    func updateDownloadProgress(
        sessionHandle: UInt64,
        taskId: UInt64,
        receivedBytes: UInt64,
        receivedSegments: UInt32
    ) -> Bool {
        updateTask(taskId) { task in
            let updated = task.with(
                receivedBytes: receivedBytes,
                receivedSegments: receivedSegments
            )
            events.append(.init(kind: .progressUpdated, task: updated))
            return updated
        }
    }

    func completeDownloadTask(
        sessionHandle: UInt64,
        taskId: UInt64,
        completedPath: String?
    ) -> Bool {
        updateTask(taskId) { task in
            let updated = task.with(
                status: .completed,
                receivedBytes: task.totalBytes ?? task.receivedBytes,
                receivedSegments: task.totalSegments ?? task.receivedSegments,
                completedPath: completedPath
            )
            events.append(.init(kind: .stateChanged, task: updated))
            return updated
        }
    }

    func completeDownloadPreparation(
        sessionHandle: UInt64,
        taskId: UInt64,
        assetIndex: UnsafePointer<VesperRuntimeDownloadAssetIndex>
    ) -> Bool {
        updateTask(taskId) { task in
            let updated = task.with(
                status: .downloading,
                totalBytes: assetIndex.pointee.has_total_size_bytes ? assetIndex.pointee.total_size_bytes : nil,
                totalSegments: assetIndex.pointee.segments_len > 0 ? UInt32(assetIndex.pointee.segments_len) : nil,
                completedPath: stringFromOptionalRuntimeCString(assetIndex.pointee.completed_path)
            )
            events.append(.init(kind: .assetIndexUpdated, task: updated))
            commands.append(.start(updated))
            events.append(.init(kind: .stateChanged, task: updated))
            return updated
        }
    }

    func replaceDownloadTaskPlan(
        sessionHandle: UInt64,
        taskId: UInt64,
        source: UnsafePointer<VesperRuntimeDownloadSource>,
        profile: UnsafePointer<VesperRuntimeDownloadProfile>,
        assetIndex: UnsafePointer<VesperRuntimeDownloadAssetIndex>
    ) -> Bool {
        updateTask(taskId) { task in
            let updated = StoredDownloadTask(
                taskId: task.taskId,
                assetId: task.assetId,
                sourceUri: stringFromOptionalRuntimeCString(source.pointee.source_uri) ?? "",
                contentFormat: source.pointee.content_format,
                manifestUri: stringFromOptionalRuntimeCString(source.pointee.manifest_uri),
                sourceHeaders: runtimeDownloadSourceHeaders(source.pointee),
                status: .preparing,
                totalBytes: assetIndex.pointee.has_total_size_bytes ? assetIndex.pointee.total_size_bytes : nil,
                receivedBytes: 0,
                totalSegments: assetIndex.pointee.segments_len > 0 ? UInt32(assetIndex.pointee.segments_len) : nil,
                receivedSegments: 0,
                completedPath: stringFromOptionalRuntimeCString(assetIndex.pointee.completed_path),
                error: nil,
                profileTargetDirectory: stringFromOptionalRuntimeCString(profile.pointee.target_directory)
            )
            events.append(.init(kind: .assetIndexUpdated, task: updated))
            events.append(.init(kind: .stateChanged, task: updated))
            return updated
        }
    }

    func exportDownloadTask(
        sessionHandle: UInt64,
        taskId: UInt64,
        outputPath: String,
        onProgress: @escaping (Float) -> Void,
        isCancelled: @escaping () -> Bool
    ) throws {
        onProgress(0.25)
        onProgress(1.0)
        exportWasCancelled = isCancelled()
    }

    func failDownloadTask(
        sessionHandle: UInt64,
        taskId: UInt64,
        error: VesperDownloadError
    ) -> Bool {
        updateTask(taskId) { task in
            let updated = task.with(
                status: .failed,
                error: StoredDownloadError(
                    code: error.code.ffiCode,
                    category: error.category.ffiCategory,
                    retriable: error.retriable,
                    message: error.message
                )
            )
            events.append(.init(kind: .stateChanged, task: updated))
            return updated
        }
    }

    func removeDownloadTask(sessionHandle: UInt64, taskId: UInt64) -> Bool {
        updateTask(taskId) { task in
            let updated = task.with(status: .removed)
            commands.append(.remove(updated))
            events.append(.init(kind: .stateChanged, task: updated))
            return updated
        }
    }

    func downloadSessionSnapshot(
        sessionHandle: UInt64,
        outSnapshot: inout VesperRuntimeDownloadSnapshot
    ) -> Bool {
        snapshotCallCount += 1
        if snapshotFailuresRemaining > 0 {
            snapshotFailuresRemaining -= 1
            return false
        }
        let orderedTasks = tasks.keys.sorted().compactMap { tasks[$0] }
        outSnapshot = makeRuntimeSnapshot(from: orderedTasks)
        if malformedSnapshotStatusesRemaining > 0,
           let runtimeTasks = outSnapshot.tasks,
           outSnapshot.len > 0
        {
            malformedSnapshotStatusesRemaining -= 1
            runtimeTasks[0].status = VesperRuntimeDownloadTaskStatus(rawValue: 99)
        }
        return true
    }

    func peekDownloadCommands(
        sessionHandle: UInt64,
        outCommands: inout VesperRuntimeDownloadCommandList
    ) -> Bool {
        drainCommandCallCount += 1
        if pendingCommands.isEmpty {
            let requestedBatchSize = commandBatchSize ?? commands.count
            let batchSize = min(max(0, requestedBatchSize), commands.count)
            pendingCommands = Array(commands.prefix(batchSize))
            commands.removeFirst(batchSize)
        }
        outCommands = makeRuntimeCommandList(from: pendingCommands)
        return true
    }

    func acknowledgeDownloadCommands(sessionHandle: UInt64, commandCount: UInt) -> Bool {
        acknowledgeCommandCallCount += 1
        if acknowledgementFailuresRemaining > 0 {
            acknowledgementFailuresRemaining -= 1
            return false
        }
        guard commandCount == UInt(pendingCommands.count) else {
            return false
        }
        pendingCommands.removeAll(keepingCapacity: true)
        return true
    }

    func failNextCommandAcknowledgement() {
        acknowledgementFailuresRemaining += 1
    }

    func enqueuePrepareCommand(taskId: UInt64) {
        guard let task = tasks[taskId] else {
            return
        }
        commands.append(.prepare(task))
    }

    func enqueuePauseCommand(taskId: UInt64) {
        guard tasks[taskId] != nil else {
            return
        }
        commands.append(.pause(taskId))
    }

    func drainDownloadEvents(
        sessionHandle: UInt64,
        outEvents: inout VesperRuntimeDownloadEventList
    ) -> Bool {
        drainEventCallCount += 1
        outEvents = makeRuntimeEventList(from: events, droppedEvents: droppedEvents)
        if malformedCreatedEventStatusesRemaining > 0,
           let runtimeEvents = outEvents.events,
           outEvents.len > 0,
           let task = runtimeEvents[0].task
        {
            malformedCreatedEventStatusesRemaining -= 1
            task.pointee.status = VesperRuntimeDownloadTaskStatus(rawValue: 99)
        }
        events.removeAll(keepingCapacity: true)
        droppedEvents = 0
        return true
    }

    func simulateDroppedCompletion(taskId: UInt64, completedPath: String) {
        guard let task = tasks[taskId] else {
            return
        }
        tasks[taskId] = task.with(
            status: .completed,
            receivedBytes: task.totalBytes ?? task.receivedBytes,
            completedPath: completedPath
        )
        events.removeAll(keepingCapacity: true)
        droppedEvents += 1
    }

    func failNextSnapshot() {
        snapshotFailuresRemaining += 1
    }

    func returnMalformedStatusFromNextSnapshot() {
        malformedSnapshotStatusesRemaining += 1
    }

    func enqueueCreatedEventWithMalformedStatus(taskId: UInt64) {
        guard let task = tasks[taskId] else {
            return
        }
        events.append(.init(kind: .created, task: task))
        malformedCreatedEventStatusesRemaining += 1
    }

    func enqueueMismatchedRemoveCommand(taskId: UInt64) {
        guard let task = tasks[taskId] else {
            return
        }
        commands.append(
            StoredRuntimeCommand(
                kind: .remove,
                task: task,
                taskId: taskId + 1
            )
        )
    }

    func enqueueRemoveCommand(taskId: UInt64) {
        guard let task = tasks[taskId] else {
            return
        }
        commands.append(.remove(task))
    }

    func freeDownloadSnapshot(_ snapshot: inout VesperRuntimeDownloadSnapshot) {
        freeRuntimeSnapshot(&snapshot)
    }

    func freeDownloadCommandList(_ commands: inout VesperRuntimeDownloadCommandList) {
        freeRuntimeCommandList(&commands)
    }

    func freeDownloadEventList(_ events: inout VesperRuntimeDownloadEventList) {
        freeRuntimeEventList(&events)
    }

    private func updateTask(
        _ taskId: UInt64,
        transform: (StoredDownloadTask) -> StoredDownloadTask
    ) -> Bool {
        guard let task = tasks[taskId] else {
            return false
        }
        tasks[taskId] = transform(task)
        return true
    }
}

private struct StoredDownloadTask {
    let taskId: UInt64
    let assetId: String
    let sourceUri: String
    let contentFormat: VesperRuntimeDownloadContentFormat
    let manifestUri: String?
    let sourceHeaders: [String: String]
    let status: VesperDownloadState
    let totalBytes: UInt64?
    let receivedBytes: UInt64
    let totalSegments: UInt32?
    let receivedSegments: UInt32
    let completedPath: String?
    let error: StoredDownloadError?
    let profileTargetDirectory: String?

    func with(
        status: VesperDownloadState? = nil,
        totalBytes: UInt64? = nil,
        receivedBytes: UInt64? = nil,
        totalSegments: UInt32? = nil,
        receivedSegments: UInt32? = nil,
        completedPath: String? = nil,
        error: StoredDownloadError? = nil
    ) -> Self {
        Self(
            taskId: taskId,
            assetId: assetId,
            sourceUri: sourceUri,
            contentFormat: contentFormat,
            manifestUri: manifestUri,
            sourceHeaders: sourceHeaders,
            status: status ?? self.status,
            totalBytes: totalBytes ?? self.totalBytes,
            receivedBytes: receivedBytes ?? self.receivedBytes,
            totalSegments: totalSegments ?? self.totalSegments,
            receivedSegments: receivedSegments ?? self.receivedSegments,
            completedPath: completedPath ?? self.completedPath,
            error: error ?? self.error,
            profileTargetDirectory: profileTargetDirectory
        )
    }
}

private struct StoredDownloadError {
    let code: PlayerFfiErrorCode
    let category: PlayerFfiErrorCategory
    let retriable: Bool
    let message: String
}

private struct StoredRuntimeEvent {
    let kind: VesperRuntimeDownloadEventKind
    let task: StoredDownloadTask
}

private struct StoredRuntimeCommand {
    let kind: VesperRuntimeDownloadCommandKind
    let task: StoredDownloadTask?
    let taskId: UInt64

    static func prepare(_ task: StoredDownloadTask) -> Self {
        Self(kind: .prepare, task: task, taskId: task.taskId)
    }

    static func start(_ task: StoredDownloadTask) -> Self {
        Self(kind: .start, task: task, taskId: task.taskId)
    }

    static func resume(_ task: StoredDownloadTask) -> Self {
        Self(kind: .resume, task: task, taskId: task.taskId)
    }

    static func pause(_ taskId: UInt64) -> Self {
        Self(kind: .pause, task: nil, taskId: taskId)
    }

    static func remove(_ task: StoredDownloadTask) -> Self {
        Self(kind: .remove, task: task, taskId: task.taskId)
    }
}

private final class RecordingDownloadExecutor: VesperDownloadExecutor {
    private let autoComplete: Bool

    private(set) var preparedSourceHeaders: [[String: String]] = []
    private(set) var startedSourceHeaders: [[String: String]] = []
    private(set) var resumedSourceHeaders: [[String: String]] = []
    private(set) var startedTaskIds: [UInt64] = []
    private(set) var resumedTaskIds: [UInt64] = []
    private(set) var pausedTaskIds: [UInt64] = []
    private(set) var removedTaskIds: [UInt64] = []
    private(set) var disposeCount = 0

    init(autoComplete: Bool = false) {
        self.autoComplete = autoComplete
    }

    func prepare(
        task: VesperDownloadTaskSnapshot,
        reporter: any VesperDownloadExecutionReporter
    ) {
        preparedSourceHeaders.append(task.source.source.headers)
        MainActor.assumeIsolated {
            reporter.completePreparation(taskId: task.taskId, assetIndex: task.assetIndex)
        }
    }

    func start(
        task: VesperDownloadTaskSnapshot,
        reporter: any VesperDownloadExecutionReporter
    ) {
        startedTaskIds.append(task.taskId)
        startedSourceHeaders.append(task.source.source.headers)
        if autoComplete {
            MainActor.assumeIsolated {
                reporter.updateProgress(
                    taskId: task.taskId,
                    receivedBytes: 512,
                    receivedSegments: 0
                )
                reporter.complete(
                    taskId: task.taskId,
                    completedPath: "/tmp/downloads/\(task.taskId).bin"
                )
            }
        }
    }

    func resume(
        task: VesperDownloadTaskSnapshot,
        reporter: any VesperDownloadExecutionReporter
    ) {
        resumedTaskIds.append(task.taskId)
        resumedSourceHeaders.append(task.source.source.headers)
    }

    func pause(taskId: VesperDownloadTaskId) {
        pausedTaskIds.append(taskId)
    }

    func remove(task: VesperDownloadTaskSnapshot?) {
        guard let task else {
            return
        }
        removedTaskIds.append(task.taskId)
    }

    func dispose() {
        disposeCount += 1
    }
}

@MainActor
private final class DownloadReporterProbe: VesperDownloadExecutionReporter {
    private let failureExpectation: XCTestExpectation
    private(set) var failure: VesperDownloadError?

    init(failureExpectation: XCTestExpectation) {
        self.failureExpectation = failureExpectation
    }

    func completePreparation(
        taskId: VesperDownloadTaskId,
        assetIndex: VesperDownloadAssetIndex
    ) {}

    func updateProgress(
        taskId: VesperDownloadTaskId,
        receivedBytes: UInt64,
        receivedSegments: UInt32
    ) {}

    func complete(
        taskId: VesperDownloadTaskId,
        completedPath: String?
    ) {}

    func fail(
        taskId: VesperDownloadTaskId,
        error: VesperDownloadError
    ) {
        failure = error
        failureExpectation.fulfill()
    }
}

private func makeRuntimeSnapshot(from tasks: [StoredDownloadTask]) -> VesperRuntimeDownloadSnapshot {
    guard !tasks.isEmpty else {
        return VesperRuntimeDownloadSnapshot(tasks: nil, len: 0)
    }
    let pointer = UnsafeMutablePointer<VesperRuntimeDownloadTask>.allocate(capacity: tasks.count)
    for (index, task) in tasks.enumerated() {
        pointer[index] = makeRuntimeTask(from: task)
    }
    return VesperRuntimeDownloadSnapshot(tasks: pointer, len: UInt(tasks.count))
}

private func makeStrictDecodingRuntimeTask(
    profile: VesperDownloadProfile = VesperDownloadProfile(),
    assetIndex: VesperDownloadAssetIndex = VesperDownloadAssetIndex()
) -> VesperRuntimeDownloadTask {
    VesperDownloadTaskSnapshot(
        taskId: 1,
        assetId: "asset-strict-decode",
        source: VesperDownloadSource(
            source: .remoteUrl(URL(string: "https://example.com/video.mp4")!)
        ),
        profile: profile,
        state: .queued,
        progress: VesperDownloadProgressSnapshot(),
        assetIndex: assetIndex
    ).toRuntimeBridgePayload()
}

private func makeRuntimeCommandList(from commands: [StoredRuntimeCommand]) -> VesperRuntimeDownloadCommandList {
    guard !commands.isEmpty else {
        return VesperRuntimeDownloadCommandList(commands: nil, len: 0)
    }
    let pointer = UnsafeMutablePointer<VesperRuntimeDownloadCommand>.allocate(capacity: commands.count)
    for (index, command) in commands.enumerated() {
        pointer[index] = VesperRuntimeDownloadCommand(
            kind: command.kind,
            task: command.task.map(makeRuntimeTask(from:)) ?? emptyRuntimeTask(),
            task_id: command.taskId
        )
    }
    return VesperRuntimeDownloadCommandList(commands: pointer, len: UInt(commands.count))
}

private func makeRuntimeEventList(
    from events: [StoredRuntimeEvent],
    droppedEvents: UInt64 = 0
) -> VesperRuntimeDownloadEventList {
    guard !events.isEmpty else {
        return VesperRuntimeDownloadEventList(
            events: nil,
            len: 0,
            dropped_events: droppedEvents
        )
    }
    let pointer = UnsafeMutablePointer<VesperRuntimeDownloadEvent>.allocate(capacity: events.count)
    for (index, event) in events.enumerated() {
        pointer[index] = makeRuntimeEvent(from: event)
    }
    return VesperRuntimeDownloadEventList(
        events: pointer,
        len: UInt(events.count),
        dropped_events: droppedEvents
    )
}

private func makeRuntimeEvent(from event: StoredRuntimeEvent) -> VesperRuntimeDownloadEvent {
    let task = event.task
    let error = task.error
    let taskPayload: UnsafeMutablePointer<VesperRuntimeDownloadTask>?
    if event.kind == .created || event.kind == .assetIndexUpdated {
        let pointer = UnsafeMutablePointer<VesperRuntimeDownloadTask>.allocate(capacity: 1)
        pointer.initialize(to: makeRuntimeTask(from: task))
        taskPayload = pointer
    } else {
        taskPayload = nil
    }
    let stateErrorMessage: UnsafeMutablePointer<CChar>? = event.kind == .stateChanged
        ? error.flatMap { duplicateRuntimeCString($0.message) }
        : nil
    let stateCompletedPath: UnsafeMutablePointer<CChar>? = event.kind == .stateChanged
        ? task.completedPath.flatMap(duplicateRuntimeCString)
        : nil
    return VesperRuntimeDownloadEvent(
        kind: event.kind,
        task: taskPayload,
        task_id: task.taskId,
        state_status: task.status.toRuntimeStatus(),
        state_progress: makeRuntimeProgress(from: task),
        state_has_error: event.kind == .stateChanged && error != nil,
        state_error_code: event.kind == .stateChanged ? (error?.code ?? PlayerFfiErrorCodeNone) : PlayerFfiErrorCodeNone,
        state_error_category: event.kind == .stateChanged ? (error?.category ?? PlayerFfiErrorCategoryPlatform) : PlayerFfiErrorCategoryPlatform,
        state_error_retriable: event.kind == .stateChanged ? (error?.retriable ?? false) : false,
        state_error_message: stateErrorMessage,
        state_completed_path: stateCompletedPath,
        progress: makeRuntimeProgress(from: task)
    )
}

private func makeRuntimeProgress(from task: StoredDownloadTask) -> VesperRuntimeDownloadProgressSnapshot {
    VesperRuntimeDownloadProgressSnapshot(
        received_bytes: task.receivedBytes,
        has_total_bytes: task.totalBytes != nil,
        total_bytes: task.totalBytes ?? 0,
        received_segments: task.receivedSegments,
        has_total_segments: task.totalSegments != nil,
        total_segments: task.totalSegments ?? 0
    )
}

private func makeRuntimeTask(from task: StoredDownloadTask) -> VesperRuntimeDownloadTask {
    let headerNames = Array(task.sourceHeaders.keys)
    let headerValues = headerNames.map { task.sourceHeaders[$0] ?? "" }
    return VesperRuntimeDownloadTask(
        task_id: task.taskId,
        asset_id: duplicateRuntimeCString(task.assetId),
        source: VesperRuntimeDownloadSource(
            source_uri: duplicateRuntimeCString(task.sourceUri),
            content_format: task.contentFormat,
            manifest_uri: task.manifestUri.flatMap(duplicateRuntimeCString),
            header_names: duplicateRuntimeCStringArray(headerNames),
            header_values: duplicateRuntimeCStringArray(headerValues),
            headers_len: UInt(headerNames.count)
        ),
        profile: VesperRuntimeDownloadProfile(
            variant_id: nil,
            preferred_audio_language: nil,
            preferred_subtitle_language: nil,
            selected_track_ids: nil,
            selected_track_ids_len: 0,
            has_target_output_format: false,
            target_output_format: VesperRuntimeDownloadOutputFormatOriginal,
            target_directory: task.profileTargetDirectory.flatMap(duplicateRuntimeCString),
            allow_metered_network: false
        ),
        status: task.status.toRuntimeStatus(),
        progress: VesperRuntimeDownloadProgressSnapshot(
            received_bytes: task.receivedBytes,
            has_total_bytes: task.totalBytes != nil,
            total_bytes: task.totalBytes ?? 0,
            received_segments: task.receivedSegments,
            has_total_segments: task.totalSegments != nil,
            total_segments: task.totalSegments ?? 0
        ),
        asset_index: VesperRuntimeDownloadAssetIndex(
            content_format: task.contentFormat,
            version: nil,
            etag: nil,
            checksum: nil,
            has_total_size_bytes: task.totalBytes != nil,
            total_size_bytes: task.totalBytes ?? 0,
            resources: nil,
            resources_len: 0,
            segments: nil,
            segments_len: 0,
            streams: nil,
            streams_len: 0,
            completed_path: task.completedPath.flatMap(duplicateRuntimeCString)
        ),
        has_error: task.error != nil,
        error_code: task.error?.code ?? PlayerFfiErrorCodeNone,
        error_category: task.error?.category ?? PlayerFfiErrorCategoryPlatform,
        error_retriable: task.error?.retriable ?? false,
        error_message: task.error.flatMap { duplicateRuntimeCString($0.message) }
    )
}

private func emptyRuntimeTask() -> VesperRuntimeDownloadTask {
    VesperRuntimeDownloadTask(
        task_id: 0,
        asset_id: nil,
        source: VesperRuntimeDownloadSource(
            source_uri: nil,
            content_format: VesperRuntimeDownloadContentFormatUnknown,
            manifest_uri: nil,
            header_names: nil,
            header_values: nil,
            headers_len: 0
        ),
        profile: VesperRuntimeDownloadProfile(
            variant_id: nil,
            preferred_audio_language: nil,
            preferred_subtitle_language: nil,
            selected_track_ids: nil,
            selected_track_ids_len: 0,
            has_target_output_format: false,
            target_output_format: VesperRuntimeDownloadOutputFormatOriginal,
            target_directory: nil,
            allow_metered_network: false
        ),
        status: VesperRuntimeDownloadTaskStatusQueued,
        progress: VesperRuntimeDownloadProgressSnapshot(
            received_bytes: 0,
            has_total_bytes: false,
            total_bytes: 0,
            received_segments: 0,
            has_total_segments: false,
            total_segments: 0
        ),
        asset_index: VesperRuntimeDownloadAssetIndex(
            content_format: VesperRuntimeDownloadContentFormatUnknown,
            version: nil,
            etag: nil,
            checksum: nil,
            has_total_size_bytes: false,
            total_size_bytes: 0,
            resources: nil,
            resources_len: 0,
            segments: nil,
            segments_len: 0,
            streams: nil,
            streams_len: 0,
            completed_path: nil
        ),
        has_error: false,
        error_code: PlayerFfiErrorCodeNone,
        error_category: PlayerFfiErrorCategoryPlatform,
        error_retriable: false,
        error_message: nil
    )
}

private func freeRuntimeSnapshot(_ snapshot: inout VesperRuntimeDownloadSnapshot) {
    guard let tasks = snapshot.tasks else {
        return
    }
    for index in 0..<Int(snapshot.len) {
        var task = tasks[index]
        freeRuntimeTask(&task)
    }
    tasks.deinitialize(count: Int(snapshot.len))
    tasks.deallocate()
    snapshot = VesperRuntimeDownloadSnapshot(tasks: nil, len: 0)
}

private func freeRuntimeCommandList(_ commands: inout VesperRuntimeDownloadCommandList) {
    guard let commandPointer = commands.commands else {
        return
    }
    for index in 0..<Int(commands.len) {
        var command = commandPointer[index]
        freeRuntimeTask(&command.task)
    }
    commandPointer.deinitialize(count: Int(commands.len))
    commandPointer.deallocate()
    commands = VesperRuntimeDownloadCommandList(commands: nil, len: 0)
}

private func freeRuntimeEventList(_ events: inout VesperRuntimeDownloadEventList) {
    guard let eventPointer = events.events else {
        events = VesperRuntimeDownloadEventList(events: nil, len: 0, dropped_events: 0)
        return
    }
    for index in 0..<Int(events.len) {
        let event = eventPointer[index]
        if let taskPointer = event.task {
            var task = taskPointer.pointee
            freeRuntimeTask(&task)
            taskPointer.deinitialize(count: 1)
            taskPointer.deallocate()
        }
        freeRuntimeCString(event.state_error_message)
        freeRuntimeCString(event.state_completed_path)
    }
    eventPointer.deinitialize(count: Int(events.len))
    eventPointer.deallocate()
    events = VesperRuntimeDownloadEventList(events: nil, len: 0, dropped_events: 0)
}

private func freeRuntimeTask(_ task: inout VesperRuntimeDownloadTask) {
    freeRuntimeCString(task.asset_id)
    freeRuntimeDownloadSource(&task.source)
    freeRuntimeDownloadProfile(&task.profile)
    freeRuntimeDownloadAssetIndex(&task.asset_index)
    freeRuntimeCString(task.error_message)
    task = emptyRuntimeTask()
}

private func freeRuntimeDownloadSource(_ source: inout VesperRuntimeDownloadSource) {
    freeRuntimeCString(source.source_uri)
    freeRuntimeCString(source.manifest_uri)
    if let headerNames = source.header_names, source.headers_len > 0 {
        for index in 0..<Int(source.headers_len) {
            freeRuntimeCString(headerNames[index])
        }
        headerNames.deallocate()
    }
    if let headerValues = source.header_values, source.headers_len > 0 {
        for index in 0..<Int(source.headers_len) {
            freeRuntimeCString(headerValues[index])
        }
        headerValues.deallocate()
    }
    source = VesperRuntimeDownloadSource(
        source_uri: nil,
        content_format: VesperRuntimeDownloadContentFormatUnknown,
        manifest_uri: nil,
        header_names: nil,
        header_values: nil,
        headers_len: 0
    )
}

private func freeRuntimeDownloadProfile(_ profile: inout VesperRuntimeDownloadProfile) {
    freeRuntimeCString(profile.variant_id)
    freeRuntimeCString(profile.preferred_audio_language)
    freeRuntimeCString(profile.preferred_subtitle_language)
    if let selectedTrackIds = profile.selected_track_ids {
        for index in 0..<Int(profile.selected_track_ids_len) {
            freeRuntimeCString(selectedTrackIds[index])
        }
        selectedTrackIds.deinitialize(count: Int(profile.selected_track_ids_len))
        selectedTrackIds.deallocate()
    }
    freeRuntimeCString(profile.target_directory)
    profile = VesperRuntimeDownloadProfile(
        variant_id: nil,
        preferred_audio_language: nil,
        preferred_subtitle_language: nil,
        selected_track_ids: nil,
        selected_track_ids_len: 0,
        has_target_output_format: false,
        target_output_format: VesperRuntimeDownloadOutputFormatOriginal,
        target_directory: nil,
        allow_metered_network: false
    )
}

private func freeRuntimeDownloadAssetIndex(_ assetIndex: inout VesperRuntimeDownloadAssetIndex) {
    freeRuntimeCString(assetIndex.version)
    freeRuntimeCString(assetIndex.etag)
    freeRuntimeCString(assetIndex.checksum)
    if let resources = assetIndex.resources {
        for index in 0..<Int(assetIndex.resources_len) {
            freeRuntimeCString(resources[index].resource_id)
            freeRuntimeCString(resources[index].uri)
            freeRuntimeCString(resources[index].relative_path)
            freeRuntimeCString(resources[index].etag)
            freeRuntimeCString(resources[index].checksum)
        }
        resources.deinitialize(count: Int(assetIndex.resources_len))
        resources.deallocate()
    }
    if let segments = assetIndex.segments {
        for index in 0..<Int(assetIndex.segments_len) {
            freeRuntimeCString(segments[index].segment_id)
            freeRuntimeCString(segments[index].uri)
            freeRuntimeCString(segments[index].relative_path)
            freeRuntimeCString(segments[index].checksum)
        }
        segments.deinitialize(count: Int(assetIndex.segments_len))
        segments.deallocate()
    }
    if let streams = assetIndex.streams {
        for index in 0..<Int(assetIndex.streams_len) {
            freeRuntimeCString(streams[index].stream_id)
            freeRuntimeCString(streams[index].language)
            freeRuntimeCString(streams[index].codec)
            freeRuntimeCString(streams[index].label)
            freeRuntimeCStringArray(streams[index].resource_ids, count: Int(streams[index].resource_ids_len))
            freeRuntimeCStringArray(streams[index].segment_ids, count: Int(streams[index].segment_ids_len))
            freeRuntimeCStringArray(streams[index].metadata_keys, count: Int(streams[index].metadata_len))
            freeRuntimeCStringArray(streams[index].metadata_values, count: Int(streams[index].metadata_len))
        }
        streams.deinitialize(count: Int(assetIndex.streams_len))
        streams.deallocate()
    }
    freeRuntimeCString(assetIndex.completed_path)
    assetIndex = VesperRuntimeDownloadAssetIndex(
        content_format: VesperRuntimeDownloadContentFormatUnknown,
        version: nil,
        etag: nil,
        checksum: nil,
        has_total_size_bytes: false,
        total_size_bytes: 0,
        resources: nil,
        resources_len: 0,
        segments: nil,
        segments_len: 0,
        streams: nil,
        streams_len: 0,
        completed_path: nil
    )
}

private func duplicateRuntimeCString(_ value: String) -> UnsafeMutablePointer<CChar>? {
    strdup(value)
}

private func freeRuntimeCStringArray(
    _ values: UnsafeMutablePointer<UnsafeMutablePointer<CChar>?>?,
    count: Int
) {
    guard let values else {
        return
    }
    for index in 0..<count {
        freeRuntimeCString(values[index])
    }
    values.deinitialize(count: count)
    values.deallocate()
}

private func duplicateRuntimeCStringArray(_ values: [String]) -> UnsafeMutablePointer<UnsafeMutablePointer<CChar>?>? {
    guard !values.isEmpty else {
        return nil
    }
    let pointer = UnsafeMutablePointer<UnsafeMutablePointer<CChar>?>.allocate(capacity: values.count)
    for (index, value) in values.enumerated() {
        pointer[index] = duplicateRuntimeCString(value)
    }
    return pointer
}

private func runtimeDownloadSourceHeaders(_ source: VesperRuntimeDownloadSource) -> [String: String] {
    guard let headerNames = source.header_names,
          let headerValues = source.header_values,
          source.headers_len > 0
    else {
        return [:]
    }
    var headers: [String: String] = [:]
    for index in 0..<Int(source.headers_len) {
        guard let name = stringFromOptionalRuntimeCString(headerNames[index]),
              let value = stringFromOptionalRuntimeCString(headerValues[index])
        else {
            continue
        }
        headers[name] = value
    }
    return headers
}

private func stringFromOptionalRuntimeCString(_ pointer: UnsafeMutablePointer<CChar>?) -> String? {
    guard let pointer else {
        return nil
    }
    return String(cString: pointer)
}

private func freeRuntimeCString(_ pointer: UnsafeMutablePointer<CChar>?) {
    guard let pointer else {
        return
    }
    free(pointer)
}

private extension VesperDownloadState {
    func toRuntimeStatus() -> VesperRuntimeDownloadTaskStatus {
        switch self {
        case .queued:
            return VesperRuntimeDownloadTaskStatusQueued
        case .preparing:
            return VesperRuntimeDownloadTaskStatusPreparing
        case .downloading:
            return VesperRuntimeDownloadTaskStatusDownloading
        case .paused:
            return VesperRuntimeDownloadTaskStatusPaused
        case .completed:
            return VesperRuntimeDownloadTaskStatusCompleted
        case .failed:
            return VesperRuntimeDownloadTaskStatusFailed
        case .removed:
            return VesperRuntimeDownloadTaskStatusRemoved
        }
    }
}

private extension VesperRuntimeDownloadTaskStatus {
    func toDownloadState() -> VesperDownloadState {
        VesperDownloadState(rawValue: Int(rawValue)) ?? .queued
    }
}

private extension VesperRuntimeDownloadCommandKind {
    static var start: Self { VesperRuntimeDownloadCommandKindStart }
    static var pause: Self { VesperRuntimeDownloadCommandKindPause }
    static var resume: Self { VesperRuntimeDownloadCommandKindResume }
    static var remove: Self { VesperRuntimeDownloadCommandKindRemove }
}

private extension VesperRuntimeDownloadEventKind {
    static var created: Self { VesperRuntimeDownloadEventKindCreated }
    static var stateChanged: Self { VesperRuntimeDownloadEventKindStateChanged }
    static var assetIndexUpdated: Self { VesperRuntimeDownloadEventKindAssetIndexUpdated }
    static var progressUpdated: Self { VesperRuntimeDownloadEventKindProgressUpdated }
}
