import Darwin
import XCTest
@testable import VesperPlayerKit
import VesperPlayerKitBridgeShim

@MainActor
final class VesperDownloadManagerTests: XCTestCase {
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
        XCTAssertEqual(manager.task(1)?.state, .removed)
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
                pluginLibraryPaths: [
                    "/Applications/VesperPlayerKit.framework/libplayer_ffmpeg.dylib",
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
                "/Applications/VesperPlayerKit.framework/libplayer_ffmpeg.dylib",
                "/Applications/VesperPlayerKit.framework/libvesper_metrics.dylib",
            ]
        )
    }
}

private final class FakeDownloadBindings: VesperDownloadManager.DownloadBindings {
    private let autoStart: Bool
    private var tasks: [UInt64: StoredDownloadTask] = [:]
    private var commands: [StoredRuntimeCommand] = []
    private var events: [StoredRuntimeEvent] = []
    private var nextTaskId: UInt64 = 1
    private(set) var createdConfiguration: VesperDownloadConfiguration?

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

    func failDownloadTask(
        sessionHandle: UInt64,
        taskId: UInt64,
        error: VesperDownloadError
    ) -> Bool {
        updateTask(taskId) { task in
            let updated = task.with(
                status: .failed,
                error: StoredDownloadError(
                    code: error.codeOrdinal,
                    category: error.categoryOrdinal,
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
        receivedBytes: UInt64? = nil,
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
            status: status ?? self.status,
            totalBytes: totalBytes,
            receivedBytes: receivedBytes ?? self.receivedBytes,
            totalSegments: totalSegments,
            receivedSegments: receivedSegments ?? self.receivedSegments,
            completedPath: completedPath ?? self.completedPath,
            error: error ?? self.error,
            profileTargetDirectory: profileTargetDirectory
        )
    }
}

private struct StoredDownloadError {
    let code: UInt32
    let category: UInt32
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

    private(set) var startedTaskIds: [UInt64] = []
    private(set) var resumedTaskIds: [UInt64] = []
    private(set) var pausedTaskIds: [UInt64] = []
    private(set) var removedTaskIds: [UInt64] = []

    init(autoComplete: Bool = false) {
        self.autoComplete = autoComplete
    }

    func start(
        task: VesperDownloadTaskSnapshot,
        reporter: any VesperDownloadExecutionReporter
    ) {
        startedTaskIds.append(task.taskId)
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
        pointer[index] = VesperRuntimeDownloadEvent(
            kind: event.kind,
            task: makeRuntimeTask(from: event.task)
        )
    }
    return VesperRuntimeDownloadEventList(events: pointer, len: UInt(events.count))
}

private func makeRuntimeTask(from task: StoredDownloadTask) -> VesperRuntimeDownloadTask {
    VesperRuntimeDownloadTask(
        task_id: task.taskId,
        asset_id: duplicateRuntimeCString(task.assetId),
        source: VesperRuntimeDownloadSource(
            source_uri: duplicateRuntimeCString(task.sourceUri),
            content_format: task.contentFormat,
            manifest_uri: task.manifestUri.flatMap(duplicateRuntimeCString)
        ),
        profile: VesperRuntimeDownloadProfile(
            variant_id: nil,
            preferred_audio_language: nil,
            preferred_subtitle_language: nil,
            selected_track_ids: nil,
            selected_track_ids_len: 0,
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
            completed_path: task.completedPath.flatMap(duplicateRuntimeCString)
        ),
        has_error: task.error != nil,
        error_code: task.error?.code ?? 0,
        error_category: task.error?.category ?? 0,
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
            manifest_uri: nil
        ),
        profile: VesperRuntimeDownloadProfile(
            variant_id: nil,
            preferred_audio_language: nil,
            preferred_subtitle_language: nil,
            selected_track_ids: nil,
            selected_track_ids_len: 0,
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
            completed_path: nil
        ),
        has_error: false,
        error_code: 0,
        error_category: 0,
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
        freeRuntimeTask(&event.task)
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
    source = VesperRuntimeDownloadSource(
        source_uri: nil,
        content_format: VesperRuntimeDownloadContentFormatUnknown,
        manifest_uri: nil
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
        completed_path: nil
    )
}

private func duplicateRuntimeCString(_ value: String) -> UnsafeMutablePointer<CChar>? {
    strdup(value)
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

private extension VesperRuntimeDownloadCommandKind {
    static var start: Self { VesperRuntimeDownloadCommandKindStart }
    static var pause: Self { VesperRuntimeDownloadCommandKindPause }
    static var resume: Self { VesperRuntimeDownloadCommandKindResume }
    static var remove: Self { VesperRuntimeDownloadCommandKindRemove }
}

private extension VesperRuntimeDownloadEventKind {
    static var created: Self { VesperRuntimeDownloadEventKindCreated }
    static var stateChanged: Self { VesperRuntimeDownloadEventKindStateChanged }
    static var progressUpdated: Self { VesperRuntimeDownloadEventKindProgressUpdated }
}
