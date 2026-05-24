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

    func testCreateTaskAutoStartRefreshesSnapshotAndStartsExecutor() {
        let bindings = FakeDownloadBindings(autoStart: true)
        let executor = RecordingDownloadExecutor()
        let manager = VesperDownloadManager(
            configuration: VesperDownloadConfiguration(autoStart: true),
            executor: executor,
            bindings: bindings
        )
        defer { manager.dispose() }

        let taskId = manager.createTask(
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
            manager.drainEvents().contains { event in
                if case .created = event {
                    return true
                }
                return false
            }
        )
    }

    func testSourceHeadersSurviveNativeDownloadCommandRoundTrip() {
        let bindings = FakeDownloadBindings(autoStart: true)
        let executor = RecordingDownloadExecutor()
        let manager = VesperDownloadManager(
            configuration: VesperDownloadConfiguration(autoStart: true),
            executor: executor,
            bindings: bindings
        )
        defer { manager.dispose() }

        _ = manager.createTask(
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

    func testPauseResumeAndRemoveDelegateToExecutorWithoutForkingStateMachine() {
        let bindings = FakeDownloadBindings(autoStart: true)
        let executor = RecordingDownloadExecutor()
        let manager = VesperDownloadManager(
            configuration: VesperDownloadConfiguration(autoStart: true),
            executor: executor,
            bindings: bindings
        )
        defer { manager.dispose() }

        _ = manager.createTask(
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

    func testExecutorReporterUpdatesSharedSnapshotProgressAndCompletion() {
        let bindings = FakeDownloadBindings(autoStart: true)
        let executor = RecordingDownloadExecutor(autoComplete: true)
        let manager = VesperDownloadManager(
            configuration: VesperDownloadConfiguration(autoStart: true),
            executor: executor,
            bindings: bindings
        )
        defer { manager.dispose() }

        _ = manager.createTask(
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

    func testPluginLibraryPathsAreForwardedToBindingsConfiguration() {
        let bindings = FakeDownloadBindings(autoStart: false)
        let manager = VesperDownloadManager(
            configuration: VesperDownloadConfiguration(
                autoStart: false,
                runPostProcessorsOnCompletion: false,
                pluginLibraryPaths: [
                    "/Applications/VesperPlayerRemuxFfmpegPlugin.framework/VesperPlayerRemuxFfmpegPlugin",
                    "/Applications/VesperPlayerKit.framework/libvesper_metrics.dylib",
                ]
            ),
            executor: RecordingDownloadExecutor(),
            bindings: bindings
        )
        defer { manager.dispose() }

        XCTAssertEqual(
            bindings.createdConfiguration?.pluginLibraryPaths,
            [
                "/Applications/VesperPlayerRemuxFfmpegPlugin.framework/VesperPlayerRemuxFfmpegPlugin",
                "/Applications/VesperPlayerKit.framework/libvesper_metrics.dylib",
            ]
        )
        XCTAssertEqual(bindings.createdConfiguration?.runPostProcessorsOnCompletion, false)
    }

    func testNativeBridgeMaterializesGeneratedTextWithoutReturningBody() throws {
        let baseDirectory = FileManager.default.temporaryDirectory
            .appendingPathComponent("vesper-native-download-\(UUID().uuidString)", isDirectory: true)
        defer { try? FileManager.default.removeItem(at: baseDirectory) }

        let manager = VesperDownloadManager(
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
                    url: URL(string: "http://cdn.example.com/index.m3u8")!,
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
        XCTAssertTrue(reporter.failure?.message.contains("http://cdn.example.com/index.m3u8") == true)
    }

    func testForegroundExecutorRejectsInsecureHTTPSizeProbeBeforeATS() async throws {
        let baseDirectory = FileManager.default.temporaryDirectory
            .appendingPathComponent("vesper-http-probe-\(UUID().uuidString)", isDirectory: true)
        defer { try? FileManager.default.removeItem(at: baseDirectory) }
        let executor = VesperForegroundDownloadExecutor(baseDirectory: baseDirectory)
        defer { executor.dispose() }
        let failure = expectation(description: "insecure size probe should fail")
        let reporter = DownloadReporterProbe(failureExpectation: failure)
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
                        uri: "http://cdn.example.com/video.mp4",
                        relativePath: "video.mp4"
                    ),
                ]
            )
        )

        executor.prepare(task: task, reporter: reporter)
        await fulfillment(of: [failure], timeout: 2)

        XCTAssertTrue(reporter.failure?.message.contains("App Transport Security") == true)
        XCTAssertTrue(reporter.failure?.message.contains("http://cdn.example.com/video.mp4") == true)
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
                        uri: "http://cdn.example.com/video.mp4",
                        relativePath: "video.mp4",
                        sizeBytes: 4
                    ),
                ]
            )
        )

        executor.start(task: task, reporter: reporter)
        await fulfillment(of: [failure], timeout: 2)

        XCTAssertTrue(reporter.failure?.message.contains("App Transport Security") == true)
        XCTAssertTrue(reporter.failure?.message.contains("http://cdn.example.com/video.mp4") == true)
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
        XCTAssertNoThrow(try validateHTTPPartialContentRange(
            contentRangeHeader: "bytes 100-199/1000",
            contentLengthHeader: "100",
            requestedStart: 100,
            requestedEndInclusive: 199,
            expectedBodyLength: 100,
            expectedTotalSizeBytes: 1000,
            sourceDescription: "https://example.com/video.mp4"
        ))
        XCTAssertThrowsError(try validateHTTPPartialContentRange(
            contentRangeHeader: "bytes */1000",
            contentLengthHeader: "0",
            requestedStart: 100,
            requestedEndInclusive: 199,
            expectedBodyLength: 100,
            expectedTotalSizeBytes: 1000,
            sourceDescription: "https://example.com/video.mp4"
        ))
        XCTAssertThrowsError(try validateHTTPPartialContentRange(
            contentRangeHeader: "bytes 0-999/1000",
            contentLengthHeader: "1000",
            requestedStart: 100,
            requestedEndInclusive: 199,
            expectedBodyLength: 100,
            expectedTotalSizeBytes: 1000,
            sourceDescription: "https://example.com/video.mp4"
        ))
        XCTAssertThrowsError(try validateHTTPPartialContentRange(
            contentRangeHeader: "bytes 100-199/1000",
            contentLengthHeader: "101",
            requestedStart: 100,
            requestedEndInclusive: 199,
            expectedBodyLength: 100,
            expectedTotalSizeBytes: 1000,
            sourceDescription: "https://example.com/video.mp4"
        ))
    }

    func testExportTaskOutputForwardsProgressAndCancellationToBindings() async throws {
        let bindings = FakeDownloadBindings(autoStart: false)
        let manager = VesperDownloadManager(
            configuration: VesperDownloadConfiguration(autoStart: false),
            executor: RecordingDownloadExecutor(),
            bindings: bindings
        )
        defer { manager.dispose() }

        let taskId = manager.createTask(
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

private final class FakeDownloadBindings: @unchecked Sendable, DownloadBindings {
    private let autoStart: Bool
    private var tasks: [UInt64: StoredDownloadTask] = [:]
    private var commands: [StoredRuntimeCommand] = []
    private var events: [StoredRuntimeEvent] = []
    private var nextTaskId: UInt64 = 1
    private(set) var createdConfiguration: VesperDownloadConfiguration?
    var forwardedProgress: [Float] = []
    var exportWasCancelled = false

    init(autoStart: Bool) {
        self.autoStart = autoStart
    }

    func createDownloadSession(configuration: VesperDownloadConfiguration) -> UInt64 {
        createdConfiguration = configuration
        return 17
    }

    func disposeDownloadSession(_ sessionHandle: UInt64) {}

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
            status: autoStart ? .downloading : .queued,
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
            commands.append(.start(storedTask))
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
            let updated = task.with(status: .downloading)
            commands.append(.start(updated))
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
                status: autoStart ? .downloading : task.status,
                totalBytes: assetIndex.pointee.has_total_size_bytes ? assetIndex.pointee.total_size_bytes : nil,
                totalSegments: assetIndex.pointee.segments_len > 0 ? UInt32(assetIndex.pointee.segments_len) : nil,
                completedPath: stringFromOptionalRuntimeCString(assetIndex.pointee.completed_path)
            )
            events.append(.init(kind: .assetIndexUpdated, task: updated))
            if autoStart {
                commands.append(.start(updated))
                events.append(.init(kind: .stateChanged, task: updated))
            }
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
            commands.append(.remove(taskId))
            events.append(.init(kind: .stateChanged, task: updated))
            return updated
        }
    }

    func downloadSessionSnapshot(
        sessionHandle: UInt64,
        outSnapshot: inout VesperRuntimeDownloadSnapshot
    ) -> Bool {
        let orderedTasks = tasks.keys.sorted().compactMap { tasks[$0] }
        outSnapshot = makeRuntimeSnapshot(from: orderedTasks)
        return true
    }

    func drainDownloadCommands(
        sessionHandle: UInt64,
        outCommands: inout VesperRuntimeDownloadCommandList
    ) -> Bool {
        outCommands = makeRuntimeCommandList(from: commands)
        commands.removeAll(keepingCapacity: true)
        return true
    }

    func drainDownloadEvents(
        sessionHandle: UInt64,
        outEvents: inout VesperRuntimeDownloadEventList
    ) -> Bool {
        outEvents = makeRuntimeEventList(from: events)
        events.removeAll(keepingCapacity: true)
        return true
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

    static func start(_ task: StoredDownloadTask) -> Self {
        Self(kind: .start, task: task, taskId: task.taskId)
    }

    static func resume(_ task: StoredDownloadTask) -> Self {
        Self(kind: .resume, task: task, taskId: task.taskId)
    }

    static func pause(_ taskId: UInt64) -> Self {
        Self(kind: .pause, task: nil, taskId: taskId)
    }

    static func remove(_ taskId: UInt64) -> Self {
        Self(kind: .remove, task: nil, taskId: taskId)
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

private func makeRuntimeEventList(from events: [StoredRuntimeEvent]) -> VesperRuntimeDownloadEventList {
    guard !events.isEmpty else {
        return VesperRuntimeDownloadEventList(events: nil, len: 0)
    }
    let pointer = UnsafeMutablePointer<VesperRuntimeDownloadEvent>.allocate(capacity: events.count)
    for (index, event) in events.enumerated() {
        pointer[index] = makeRuntimeEvent(from: event)
    }
    return VesperRuntimeDownloadEventList(events: pointer, len: UInt(events.count))
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
        return
    }
    for index in 0..<Int(events.len) {
        var event = eventPointer[index]
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
    events = VesperRuntimeDownloadEventList(events: nil, len: 0)
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
