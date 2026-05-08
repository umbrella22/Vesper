import Combine
import Foundation
import VesperPlayerKitBridgeShim

public typealias VesperDownloadAssetId = String
public typealias VesperDownloadTaskId = UInt64

public enum VesperDownloadContentFormat: Int, Equatable, Codable {
    case hlsSegments = 0
    case dashSegments = 1
    case flvSegments = 2
    case singleFile = 3
    case unknown = 4
}

public enum VesperDownloadOutputFormat: Int, Equatable, Codable {
    case mp4 = 0
    case mkv = 1
    case original = 2
}

public struct VesperDownloadConfiguration: Equatable {
    public let autoStart: Bool
    public let runPostProcessorsOnCompletion: Bool
    public let resumePartialDownloads: Bool
    public let restoreTasksOnStartup: Bool
    public let baseDirectory: URL?
    public let pluginLibraryPaths: [String]

    public init(
        autoStart: Bool = true,
        runPostProcessorsOnCompletion: Bool = true,
        resumePartialDownloads: Bool = true,
        restoreTasksOnStartup: Bool = true,
        baseDirectory: URL? = nil,
        pluginLibraryPaths: [String] = []
    ) {
        self.autoStart = autoStart
        self.runPostProcessorsOnCompletion = runPostProcessorsOnCompletion
        self.resumePartialDownloads = resumePartialDownloads
        self.restoreTasksOnStartup = restoreTasksOnStartup
        self.baseDirectory = baseDirectory
        self.pluginLibraryPaths = pluginLibraryPaths
    }
}

public struct VesperDownloadSource: Equatable, Codable {
    public let source: VesperPlayerSource
    public let contentFormat: VesperDownloadContentFormat
    public let manifestUri: String?

    public init(
        source: VesperPlayerSource,
        contentFormat: VesperDownloadContentFormat? = nil,
        manifestUri: String? = nil
    ) {
        self.source = source
        self.contentFormat = contentFormat ?? Self.inferContentFormat(for: source)
        self.manifestUri = manifestUri
    }

    private static func inferContentFormat(for source: VesperPlayerSource) -> VesperDownloadContentFormat {
        switch source.protocol {
        case .hls:
            return .hlsSegments
        case .dash:
            return .dashSegments
        case .file, .content, .progressive:
            return .singleFile
        case .unknown:
            return .unknown
        }
    }
}

public struct VesperDownloadProfile: Equatable, Codable {
    public let variantId: String?
    public let preferredAudioLanguage: String?
    public let preferredSubtitleLanguage: String?
    public let selectedTrackIds: [String]
    public let targetOutputFormat: VesperDownloadOutputFormat?
    public let targetDirectory: URL?
    public let allowMeteredNetwork: Bool

    public init(
        variantId: String? = nil,
        preferredAudioLanguage: String? = nil,
        preferredSubtitleLanguage: String? = nil,
        selectedTrackIds: [String] = [],
        targetOutputFormat: VesperDownloadOutputFormat? = nil,
        targetDirectory: URL? = nil,
        allowMeteredNetwork: Bool = false
    ) {
        self.variantId = variantId
        self.preferredAudioLanguage = preferredAudioLanguage
        self.preferredSubtitleLanguage = preferredSubtitleLanguage
        self.selectedTrackIds = selectedTrackIds
        self.targetOutputFormat = targetOutputFormat
        self.targetDirectory = targetDirectory
        self.allowMeteredNetwork = allowMeteredNetwork
    }
}

public struct VesperDownloadByteRange: Equatable, Codable {
    public let offset: UInt64
    public let length: UInt64

    public init(offset: UInt64, length: UInt64) {
        self.offset = offset
        self.length = length
    }
}

public struct VesperDownloadResourceRecord: Equatable, Codable {
    public let resourceId: String
    public let uri: String
    public let relativePath: String?
    public let byteRange: VesperDownloadByteRange?
    public let generatedText: String?
    public let sizeBytes: UInt64?
    public let etag: String?
    public let checksum: String?

    public init(
        resourceId: String,
        uri: String,
        relativePath: String? = nil,
        byteRange: VesperDownloadByteRange? = nil,
        generatedText: String? = nil,
        sizeBytes: UInt64? = nil,
        etag: String? = nil,
        checksum: String? = nil
    ) {
        self.resourceId = resourceId
        self.uri = uri
        self.relativePath = relativePath
        self.byteRange = byteRange
        self.generatedText = generatedText
        self.sizeBytes = sizeBytes
        self.etag = etag
        self.checksum = checksum
    }
}

public struct VesperDownloadSegmentRecord: Equatable, Codable {
    public let segmentId: String
    public let uri: String
    public let relativePath: String?
    public let sequence: UInt64?
    public let byteRange: VesperDownloadByteRange?
    public let sizeBytes: UInt64?
    public let checksum: String?

    public init(
        segmentId: String,
        uri: String,
        relativePath: String? = nil,
        sequence: UInt64? = nil,
        byteRange: VesperDownloadByteRange? = nil,
        sizeBytes: UInt64? = nil,
        checksum: String? = nil
    ) {
        self.segmentId = segmentId
        self.uri = uri
        self.relativePath = relativePath
        self.sequence = sequence
        self.byteRange = byteRange
        self.sizeBytes = sizeBytes
        self.checksum = checksum
    }
}

public struct VesperDownloadAssetIndex: Equatable, Codable {
    public let contentFormat: VesperDownloadContentFormat
    public let version: String?
    public let etag: String?
    public let checksum: String?
    public let totalSizeBytes: UInt64?
    public let resources: [VesperDownloadResourceRecord]
    public let segments: [VesperDownloadSegmentRecord]
    public let completedPath: String?

    public init(
        contentFormat: VesperDownloadContentFormat = .unknown,
        version: String? = nil,
        etag: String? = nil,
        checksum: String? = nil,
        totalSizeBytes: UInt64? = nil,
        resources: [VesperDownloadResourceRecord] = [],
        segments: [VesperDownloadSegmentRecord] = [],
        completedPath: String? = nil
    ) {
        self.contentFormat = contentFormat
        self.version = version
        self.etag = etag
        self.checksum = checksum
        self.totalSizeBytes = totalSizeBytes
        self.resources = resources
        self.segments = segments
        self.completedPath = completedPath
    }
}

public struct VesperDownloadProgressSnapshot: Equatable, Codable {
    public let receivedBytes: UInt64
    public let totalBytes: UInt64?
    public let receivedSegments: UInt32
    public let totalSegments: UInt32?

    public init(
        receivedBytes: UInt64 = 0,
        totalBytes: UInt64? = nil,
        receivedSegments: UInt32 = 0,
        totalSegments: UInt32? = nil
    ) {
        self.receivedBytes = receivedBytes
        self.totalBytes = totalBytes
        self.receivedSegments = receivedSegments
        self.totalSegments = totalSegments
    }

    public var completionRatio: Double? {
        guard let totalBytes, totalBytes > 0 else {
            return nil
        }
        return Double(receivedBytes) / Double(totalBytes)
    }
}

public enum VesperDownloadState: Int, Equatable, Codable {
    case queued = 0
    case preparing = 1
    case downloading = 2
    case paused = 3
    case completed = 4
    case failed = 5
    case removed = 6
}

public struct VesperDownloadError: Equatable, Codable {
    public let codeOrdinal: UInt32
    public let categoryOrdinal: UInt32
    public let retriable: Bool
    public let message: String

    public init(
        codeOrdinal: UInt32,
        categoryOrdinal: UInt32,
        retriable: Bool,
        message: String
    ) {
        self.codeOrdinal = codeOrdinal
        self.categoryOrdinal = categoryOrdinal
        self.retriable = retriable
        self.message = message
    }
}

public struct VesperDownloadTaskSnapshot: Equatable, Codable {
    public let taskId: VesperDownloadTaskId
    public let assetId: VesperDownloadAssetId
    public let source: VesperDownloadSource
    public let profile: VesperDownloadProfile
    public let state: VesperDownloadState
    public let progress: VesperDownloadProgressSnapshot
    public let assetIndex: VesperDownloadAssetIndex
    public let error: VesperDownloadError?

    public init(
        taskId: VesperDownloadTaskId,
        assetId: VesperDownloadAssetId,
        source: VesperDownloadSource,
        profile: VesperDownloadProfile,
        state: VesperDownloadState,
        progress: VesperDownloadProgressSnapshot,
        assetIndex: VesperDownloadAssetIndex,
        error: VesperDownloadError? = nil
    ) {
        self.taskId = taskId
        self.assetId = assetId
        self.source = source
        self.profile = profile
        self.state = state
        self.progress = progress
        self.assetIndex = assetIndex
        self.error = error
    }
}

public struct VesperDownloadSnapshot: Equatable, Codable {
    public let tasks: [VesperDownloadTaskSnapshot]

    public init(tasks: [VesperDownloadTaskSnapshot]) {
        self.tasks = tasks
    }
}

public enum VesperDownloadEvent: Equatable {
    case created(VesperDownloadTaskSnapshot)
    case stateChanged(VesperDownloadTaskSnapshot)
    case assetIndexUpdated(VesperDownloadTaskSnapshot)
    case progressUpdated(VesperDownloadTaskSnapshot)
}

@MainActor
public protocol VesperDownloadExecutionReporter: AnyObject {
    func completePreparation(
        taskId: VesperDownloadTaskId,
        assetIndex: VesperDownloadAssetIndex
    )

    func updateProgress(
        taskId: VesperDownloadTaskId,
        receivedBytes: UInt64,
        receivedSegments: UInt32
    )

    func complete(
        taskId: VesperDownloadTaskId,
        completedPath: String?
    )

    func fail(
        taskId: VesperDownloadTaskId,
        error: VesperDownloadError
    )
}

public protocol VesperDownloadExecutor: AnyObject {
    func prepare(
        task: VesperDownloadTaskSnapshot,
        reporter: any VesperDownloadExecutionReporter
    )

    func start(
        task: VesperDownloadTaskSnapshot,
        reporter: any VesperDownloadExecutionReporter
    )

    func resume(
        task: VesperDownloadTaskSnapshot,
        reporter: any VesperDownloadExecutionReporter
    )

    func pause(taskId: VesperDownloadTaskId)

    func remove(task: VesperDownloadTaskSnapshot?)

    func dispose()
}

public extension VesperDownloadExecutor {
    func prepare(
        task: VesperDownloadTaskSnapshot,
        reporter: any VesperDownloadExecutionReporter
    ) {
        Task { @MainActor in
            reporter.completePreparation(taskId: task.taskId, assetIndex: task.assetIndex)
        }
    }

    func resume(
        task: VesperDownloadTaskSnapshot,
        reporter: any VesperDownloadExecutionReporter
    ) {
        start(task: task, reporter: reporter)
    }

    func pause(taskId: VesperDownloadTaskId) {}

    func remove(task: VesperDownloadTaskSnapshot?) {}

    func dispose() {}
}

@MainActor
public final class VesperDownloadManager: ObservableObject {
    @Published public private(set) var snapshot: VesperDownloadSnapshot

    private let executor: any VesperDownloadExecutor
    private let bindings: any DownloadBindings
    private let configuration: VesperDownloadConfiguration
    private let stateStore: VesperDownloadStateStore?
    private var eventBuffer: [VesperDownloadEvent] = []
    private var sessionHandle: UInt64 = 0

    public init(
        configuration: VesperDownloadConfiguration = VesperDownloadConfiguration(),
        executor: (any VesperDownloadExecutor)? = nil
    ) {
        self.configuration = configuration
        self.executor = executor ?? VesperForegroundDownloadExecutor(
            baseDirectory: configuration.baseDirectory,
            resumePartialDownloads: configuration.resumePartialDownloads
        )
        bindings = NativeDownloadBindings()
        stateStore = configuration.restoreTasksOnStartup
            ? VesperDownloadStateStore(fileURL: Self.stateStoreURL(for: configuration))
            : nil
        snapshot = VesperDownloadSnapshot(tasks: [])
        sessionHandle = bindings.createDownloadSession(configuration: configuration)
        precondition(sessionHandle != 0, "native download session handle must not be zero")
        restorePersistedTasks()
        refresh()
    }

    internal init(
        configuration: VesperDownloadConfiguration,
        executor: any VesperDownloadExecutor,
        bindings: any DownloadBindings
    ) {
        self.configuration = configuration
        self.executor = executor
        self.bindings = bindings
        stateStore = nil
        snapshot = VesperDownloadSnapshot(tasks: [])
        sessionHandle = bindings.createDownloadSession(configuration: configuration)
        refresh()
    }

    deinit {
        if sessionHandle != 0 {
            bindings.disposeDownloadSession(sessionHandle)
        }
    }

    public func dispose() {
        snapshot.tasks
            .filter { $0.state == .preparing || $0.state == .downloading }
            .forEach { _ = pauseTask($0.taskId) }
        persistSnapshot(snapshot)
        executor.dispose()
        if sessionHandle != 0 {
            bindings.disposeDownloadSession(sessionHandle)
            sessionHandle = 0
        }
        eventBuffer.removeAll(keepingCapacity: false)
        snapshot = VesperDownloadSnapshot(tasks: [])
    }

    public func refresh() {
        syncRuntimeState(processCommands: true)
    }

    public func drainEvents() -> [VesperDownloadEvent] {
        let events = eventBuffer
        eventBuffer.removeAll(keepingCapacity: true)
        return events
    }

    public func task(_ taskId: VesperDownloadTaskId) -> VesperDownloadTaskSnapshot? {
        snapshot.tasks.first(where: { $0.taskId == taskId })
    }

    public func tasks(forAsset assetId: VesperDownloadAssetId) -> [VesperDownloadTaskSnapshot] {
        snapshot.tasks.filter { $0.assetId == assetId }
    }

    public func createTask(
        assetId: VesperDownloadAssetId,
        source: VesperDownloadSource,
        profile: VesperDownloadProfile = VesperDownloadProfile(),
        assetIndex: VesperDownloadAssetIndex = VesperDownloadAssetIndex()
    ) -> VesperDownloadTaskId? {
        var runtimeSource = source.toRuntimeBridgePayload()
        var runtimeProfile = profile.toRuntimeBridgePayload()
        var runtimeAssetIndex = assetIndex.toRuntimeBridgePayload()
        var taskId: UInt64 = 0
        let created = withUnsafePointer(to: &runtimeSource) { sourcePointer in
            withUnsafePointer(to: &runtimeProfile) { profilePointer in
                withUnsafePointer(to: &runtimeAssetIndex) { assetIndexPointer in
                    withUnsafeMutablePointer(to: &taskId) { taskIdPointer in
                        bindings.createDownloadTask(
                            sessionHandle: sessionHandle,
                            assetId: assetId,
                            source: sourcePointer,
                            profile: profilePointer,
                            assetIndex: assetIndexPointer,
                            outTaskId: taskIdPointer
                        )
                    }
                }
            }
        }
        freeRuntimeDownloadSource(&runtimeSource)
        freeRuntimeDownloadProfile(&runtimeProfile)
        freeRuntimeDownloadAssetIndex(&runtimeAssetIndex)

        guard created, taskId != 0 else {
            return nil
        }
        syncRuntimeState(processCommands: true)
        return taskId
    }

    public func restoreTasks(_ tasks: [VesperDownloadTaskSnapshot]) -> Bool {
        guard sessionHandle != 0 else {
            return false
        }
        guard !tasks.isEmpty else {
            return true
        }

        let pointer = UnsafeMutablePointer<VesperRuntimeDownloadTask>.allocate(capacity: tasks.count)
        for (index, task) in tasks.enumerated() {
            pointer[index] = task.toRuntimeBridgePayload()
        }
        let restored = bindings.restoreDownloadTasks(
            sessionHandle: sessionHandle,
            tasks: UnsafePointer(pointer),
            taskCount: tasks.count
        )
        for index in 0..<tasks.count {
            freeRuntimeDownloadTask(&pointer[index])
        }
        pointer.deallocate()

        if restored {
            syncRuntimeState(processCommands: true)
        }
        return restored
    }

    public func startTask(_ taskId: VesperDownloadTaskId) -> Bool {
        let started = bindings.startDownloadTask(sessionHandle: sessionHandle, taskId: taskId)
        if started {
            syncRuntimeState(processCommands: true)
        }
        return started
    }

    public func pauseTask(_ taskId: VesperDownloadTaskId) -> Bool {
        let paused = bindings.pauseDownloadTask(sessionHandle: sessionHandle, taskId: taskId)
        if paused {
            syncRuntimeState(processCommands: true)
        }
        return paused
    }

    public func resumeTask(_ taskId: VesperDownloadTaskId) -> Bool {
        let resumed = bindings.resumeDownloadTask(sessionHandle: sessionHandle, taskId: taskId)
        if resumed {
            syncRuntimeState(processCommands: true)
        }
        return resumed
    }

    public func removeTask(_ taskId: VesperDownloadTaskId) -> Bool {
        let removed = bindings.removeDownloadTask(sessionHandle: sessionHandle, taskId: taskId)
        if removed {
            syncRuntimeState(processCommands: true)
        }
        return removed
    }

    public func exportTaskOutput(
        taskId: VesperDownloadTaskId,
        outputPath: String,
        onProgress: @escaping (Float) -> Void = { _ in },
        isCancelled: @escaping () -> Bool = { false }
    ) async throws {
        guard sessionHandle != 0 else {
            throw DownloadExportBridgeError("native download session handle must not be zero")
        }

        let bindings = self.bindings
        let sessionHandle = self.sessionHandle
        try await withCheckedThrowingContinuation { continuation in
            DispatchQueue.global(qos: .utility).async {
                do {
                    try bindings.exportDownloadTask(
                        sessionHandle: sessionHandle,
                        taskId: taskId,
                        outputPath: outputPath,
                        onProgress: onProgress,
                        isCancelled: isCancelled
                    )
                    continuation.resume(returning: ())
                } catch {
                    continuation.resume(throwing: error)
                }
            }
        }
    }

    private func syncRuntimeState(processCommands: Bool) {
        guard sessionHandle != 0 else {
            snapshot = VesperDownloadSnapshot(tasks: [])
            eventBuffer.removeAll(keepingCapacity: false)
            return
        }

        var runtimeSnapshot = VesperRuntimeDownloadSnapshot(tasks: nil, len: 0)
        if bindings.downloadSessionSnapshot(sessionHandle: sessionHandle, outSnapshot: &runtimeSnapshot) {
            snapshot = runtimeSnapshot.toPublic()
            bindings.freeDownloadSnapshot(&runtimeSnapshot)
        } else {
            snapshot = VesperDownloadSnapshot(tasks: [])
        }
        persistSnapshot(snapshot)

        var runtimeEvents = VesperRuntimeDownloadEventList(events: nil, len: 0)
        if bindings.drainDownloadEvents(sessionHandle: sessionHandle, outEvents: &runtimeEvents) {
            eventBuffer.append(contentsOf: runtimeEvents.toPublic())
            bindings.freeDownloadEventList(&runtimeEvents)
        }

        guard processCommands else {
            return
        }

        var runtimeCommands = VesperRuntimeDownloadCommandList(commands: nil, len: 0)
        if bindings.drainDownloadCommands(sessionHandle: sessionHandle, outCommands: &runtimeCommands) {
            let commands = runtimeCommands.toPublic()
            bindings.freeDownloadCommandList(&runtimeCommands)
            commands.forEach(applyCommand(_:))
        }
    }

    private func applyCommand(_ command: RuntimeDownloadCommand) {
        switch command.kind {
        case .prepare:
            guard let task = command.task else {
                return
            }
            executor.prepare(task: task, reporter: runtimeReporter)
        case .start:
            guard let task = command.task else {
                return
            }
            executor.start(task: task, reporter: runtimeReporter)
        case .resume:
            guard let task = command.task else {
                return
            }
            executor.resume(task: task, reporter: runtimeReporter)
        case .pause:
            executor.pause(taskId: command.taskId)
        case .remove:
            executor.remove(task: task(command.taskId))
        }
    }

    private var runtimeReporter: any VesperDownloadExecutionReporter {
        RuntimeReporter(manager: self)
    }

    private func restorePersistedTasks() {
        let storedTasks = stateStore?.load().tasks ?? []
        let restorable = storedTasks.filter { $0.state != .removed }
        guard !restorable.isEmpty else {
            return
        }
        let activeTaskIds = restorable
            .filter { $0.state == .preparing || $0.state == .downloading }
            .map(\.taskId)
        let queuedTaskIds = restorable
            .filter { $0.state == .queued }
            .map(\.taskId)
        guard restoreTasks(restorable), configuration.autoStart else {
            return
        }
        activeTaskIds.forEach { _ = resumeTask($0) }
        queuedTaskIds.forEach { _ = startTask($0) }
    }

    private func persistSnapshot(_ snapshot: VesperDownloadSnapshot) {
        stateStore?.save(snapshot)
    }

    private static func stateStoreURL(for configuration: VesperDownloadConfiguration) -> URL {
        let root = configuration.baseDirectory
            ?? FileManager.default.urls(for: .documentDirectory, in: .userDomainMask).first!
                .appendingPathComponent("vesper-downloads", isDirectory: true)
        return root.appendingPathComponent("download-state.json")
    }

    private final class RuntimeReporter: VesperDownloadExecutionReporter {
        private weak var manager: VesperDownloadManager?

        init(manager: VesperDownloadManager) {
            self.manager = manager
        }

        func completePreparation(
            taskId: VesperDownloadTaskId,
            assetIndex: VesperDownloadAssetIndex
        ) {
            guard let manager, manager.sessionHandle != 0 else {
                return
            }
            var runtimeAssetIndex = assetIndex.toRuntimeBridgePayload()
            _ = withUnsafePointer(to: &runtimeAssetIndex) { assetIndexPointer in
                manager.bindings.completeDownloadPreparation(
                    sessionHandle: manager.sessionHandle,
                    taskId: taskId,
                    assetIndex: assetIndexPointer
                )
            }
            freeRuntimeDownloadAssetIndex(&runtimeAssetIndex)
            manager.syncRuntimeState(processCommands: true)
        }

        func updateProgress(
            taskId: VesperDownloadTaskId,
            receivedBytes: UInt64,
            receivedSegments: UInt32
        ) {
            guard let manager, manager.sessionHandle != 0 else {
                return
            }
            _ = manager.bindings.updateDownloadProgress(
                sessionHandle: manager.sessionHandle,
                taskId: taskId,
                receivedBytes: receivedBytes,
                receivedSegments: receivedSegments
            )
            manager.syncRuntimeState(processCommands: false)
        }

        func complete(taskId: VesperDownloadTaskId, completedPath: String?) {
            guard let manager, manager.sessionHandle != 0 else {
                return
            }
            _ = manager.bindings.completeDownloadTask(
                sessionHandle: manager.sessionHandle,
                taskId: taskId,
                completedPath: completedPath
            )
            manager.syncRuntimeState(processCommands: false)
        }

        func fail(taskId: VesperDownloadTaskId, error: VesperDownloadError) {
            guard let manager, manager.sessionHandle != 0 else {
                return
            }
            _ = manager.bindings.failDownloadTask(
                sessionHandle: manager.sessionHandle,
                taskId: taskId,
                error: error
            )
            manager.syncRuntimeState(processCommands: false)
        }
    }

    internal protocol DownloadBindings: Sendable {
        func createDownloadSession(configuration: VesperDownloadConfiguration) -> UInt64

        func disposeDownloadSession(_ sessionHandle: UInt64)

        func createDownloadTask(
            sessionHandle: UInt64,
            assetId: String,
            source: UnsafePointer<VesperRuntimeDownloadSource>,
            profile: UnsafePointer<VesperRuntimeDownloadProfile>,
            assetIndex: UnsafePointer<VesperRuntimeDownloadAssetIndex>,
            outTaskId: UnsafeMutablePointer<UInt64>
        ) -> Bool

        func restoreDownloadTasks(
            sessionHandle: UInt64,
            tasks: UnsafePointer<VesperRuntimeDownloadTask>?,
            taskCount: Int
        ) -> Bool

        func startDownloadTask(sessionHandle: UInt64, taskId: UInt64) -> Bool

        func pauseDownloadTask(sessionHandle: UInt64, taskId: UInt64) -> Bool

        func resumeDownloadTask(sessionHandle: UInt64, taskId: UInt64) -> Bool

        func updateDownloadProgress(
            sessionHandle: UInt64,
            taskId: UInt64,
            receivedBytes: UInt64,
            receivedSegments: UInt32
        ) -> Bool

        func completeDownloadTask(
            sessionHandle: UInt64,
            taskId: UInt64,
            completedPath: String?
        ) -> Bool

        func completeDownloadPreparation(
            sessionHandle: UInt64,
            taskId: UInt64,
            assetIndex: UnsafePointer<VesperRuntimeDownloadAssetIndex>
        ) -> Bool

        func exportDownloadTask(
            sessionHandle: UInt64,
            taskId: UInt64,
            outputPath: String,
            onProgress: @escaping (Float) -> Void,
            isCancelled: @escaping () -> Bool
        ) throws

        func failDownloadTask(
            sessionHandle: UInt64,
            taskId: UInt64,
            error: VesperDownloadError
        ) -> Bool

        func removeDownloadTask(sessionHandle: UInt64, taskId: UInt64) -> Bool

        func downloadSessionSnapshot(
            sessionHandle: UInt64,
            outSnapshot: inout VesperRuntimeDownloadSnapshot
        ) -> Bool

        func drainDownloadCommands(
            sessionHandle: UInt64,
            outCommands: inout VesperRuntimeDownloadCommandList
        ) -> Bool

        func drainDownloadEvents(
            sessionHandle: UInt64,
            outEvents: inout VesperRuntimeDownloadEventList
        ) -> Bool

        func freeDownloadSnapshot(_ snapshot: inout VesperRuntimeDownloadSnapshot)

        func freeDownloadCommandList(_ commands: inout VesperRuntimeDownloadCommandList)

        func freeDownloadEventList(_ events: inout VesperRuntimeDownloadEventList)
    }
}

private final class VesperDownloadStateStore {
    private let fileURL: URL
    private let encoder = JSONEncoder()
    private let decoder = JSONDecoder()

    init(fileURL: URL) {
        self.fileURL = fileURL
    }

    func load() -> VesperDownloadSnapshot {
        guard let data = try? Data(contentsOf: fileURL),
              let snapshot = try? decoder.decode(VesperDownloadSnapshot.self, from: data)
        else {
            return VesperDownloadSnapshot(tasks: [])
        }
        return snapshot
    }

    func save(_ snapshot: VesperDownloadSnapshot) {
        let tasks = snapshot.tasks.filter { $0.state != .removed }
        guard !tasks.isEmpty else {
            try? FileManager.default.removeItem(at: fileURL)
            return
        }
        do {
            try FileManager.default.createDirectory(
                at: fileURL.deletingLastPathComponent(),
                withIntermediateDirectories: true
            )
            let data = try encoder.encode(VesperDownloadSnapshot(tasks: tasks))
            try data.write(to: fileURL, options: .atomic)
        } catch {
            iosHostLog("download state persistence failed: \(error.localizedDescription)")
        }
    }
}

public final class VesperForegroundDownloadExecutor: VesperDownloadExecutor {
    private let lock = NSLock()
    private let fileManager = FileManager.default
    private var tasks: [VesperDownloadTaskId: Task<Void, Never>] = [:]
    private let baseDirectory: URL?
    private let resumePartialDownloads: Bool

    public init(baseDirectory: URL? = nil, resumePartialDownloads: Bool = true) {
        self.baseDirectory = baseDirectory
        self.resumePartialDownloads = resumePartialDownloads
    }

    private func prepareAssetIndex(task: VesperDownloadTaskSnapshot) async throws -> VesperDownloadAssetIndex {
        if !task.assetIndex.resources.isEmpty || !task.assetIndex.segments.isEmpty {
            return try await completePreparedAssetIndex(
                contentFormat: task.source.contentFormat,
                assetIndex: task.assetIndex
            )
        }

        switch task.source.contentFormat {
        case .hlsSegments:
            return try await planHlsAssetIndex(task: task)
        case .dashSegments:
            return try await planDashAssetIndex(task: task)
        case .flvSegments:
            return try await planFlvAssetIndex(task: task)
        case .singleFile:
            return try await planSingleFileAssetIndex(task: task)
        case .unknown:
            throw VesperForegroundDownloadPreparationError.unsupported("download preparation cannot plan an unknown content format")
        }
    }

    private func completePreparedAssetIndex(
        contentFormat: VesperDownloadContentFormat,
        assetIndex: VesperDownloadAssetIndex
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
                sizeBytes = try await probeRequiredSize(resource.uri, byteRange: resource.byteRange)
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
                sizeBytes = try await probeRequiredSize(segment.uri, byteRange: segment.byteRange)
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

    private func planSingleFileAssetIndex(task: VesperDownloadTaskSnapshot) async throws -> VesperDownloadAssetIndex {
        let uri = task.source.manifestUri ?? task.source.source.uri
        let sizeBytes = try await probeRequiredSize(uri, byteRange: nil)
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

    private func planHlsAssetIndex(task: VesperDownloadTaskSnapshot) async throws -> VesperDownloadAssetIndex {
        let manifestUri = task.source.manifestUri ?? task.source.source.uri
        let manifestText = try await fetchText(manifestUri)
        if manifestText.range(of: "#EXT-X-STREAM-INF", options: .caseInsensitive) != nil {
            return try await planHlsMasterAssetIndex(
                manifestUri: manifestUri,
                manifestText: manifestText,
                profile: task.profile
            )
        }

        let media = try parseHlsMediaPlaylist(playlistUri: manifestUri, playlistText: manifestText)
        return try await buildHlsMediaAssetIndex(
            manifestPath: "index.m3u8",
            mediaPlaylists: [("media", media)]
        )
    }

    private func planHlsMasterAssetIndex(
        manifestUri: String,
        manifestText: String,
        profile: VesperDownloadProfile
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
                    playlistText: try await fetchText(variant.uri)
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
                        playlistText: try await fetchText(audio.uri)
                    )
                )
            )
        }

        let planned = try await buildHlsMediaAssetIndex(
            manifestPath: "index.m3u8",
            mediaPlaylists: mediaPlaylists
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
        mediaPlaylists: [(String, HlsMediaPlaylist)]
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
                    let sizeBytes = try await probeRequiredSize(map.uri, byteRange: map.byteRange)
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
                let sizeBytes = try await probeRequiredSize(segment.uri, byteRange: segment.byteRange)
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

    private func planDashAssetIndex(task: VesperDownloadTaskSnapshot) async throws -> VesperDownloadAssetIndex {
        let manifestUri = task.source.manifestUri ?? task.source.source.uri
        let manifestText = try await fetchText(manifestUri)
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
                    let sizeBytes = try await probeRequiredSize(remote, byteRange: nil)
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
                    let sizeBytes = try await probeRequiredSize(remote, byteRange: nil)
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
                let sizeBytes = try await probeRequiredSize(remote, byteRange: nil)
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

    private func planFlvAssetIndex(task: VesperDownloadTaskSnapshot) async throws -> VesperDownloadAssetIndex {
        let uri = task.source.manifestUri ?? task.source.source.uri
        let clipUris =
            extensionFromUri(uri, fallback: "flv").caseInsensitiveCompare("flv") == .orderedSame
                ? [uri]
                : parseFlvClipManifest(baseUri: uri, manifestText: try await fetchText(uri))
        if clipUris.isEmpty {
            throw VesperForegroundDownloadPreparationError.invalidSource("FLV clip manifest did not contain any clip URI")
        }

        var totalSizeBytes: UInt64 = 0
        var concat = "ffconcat version 1.0\n"
        var segments: [VesperDownloadSegmentRecord] = []
        for (index, clipUri) in clipUris.enumerated() {
            let sequence = UInt64(index + 1)
            let sizeBytes = try await probeRequiredSize(clipUri, byteRange: nil)
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
                let assetIndex = try await self.prepareAssetIndex(task: task)
                await reporter.completePreparation(taskId: task.taskId, assetIndex: assetIndex)
            } catch {
                await reporter.fail(
                    taskId: task.taskId,
                    error: VesperDownloadError(
                        codeOrdinal: 3,
                        categoryOrdinal: 2,
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
        launchDownload(task: task, reporter: reporter)
    }

    public func resume(
        task: VesperDownloadTaskSnapshot,
        reporter: any VesperDownloadExecutionReporter
    ) {
        launchDownload(task: task, reporter: reporter)
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

            do {
                let plan = try self.executionPlan(for: task)
                var receivedBytes: UInt64 = 0
                var receivedSegments: UInt32 = 0
                let trackSegments = !task.assetIndex.segments.isEmpty

                for (index, entry) in plan.enumerated() {
                    try Task.checkCancellation()

                    let destinationURL = try self.outputURL(for: task, entry: entry, index: index)
                    try self.fileManager.createDirectory(
                        at: destinationURL.deletingLastPathComponent(),
                        withIntermediateDirectories: true
                    )

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
                            expectedSizeBytes: entry.expectedSizeBytes,
                            resumeFromBytes: resumeFromBytes,
                            to: destinationURL
                        ) { entryBytes in
                            await reporter.updateProgress(
                                taskId: task.taskId,
                                receivedBytes: receivedBytes + entryBytes,
                                receivedSegments: receivedSegments
                            )
                        }
                    }
                    receivedBytes += writtenBytes
                    if trackSegments, entry.isSegment {
                        receivedSegments += 1
                    }
                    await reporter.updateProgress(
                        taskId: task.taskId,
                        receivedBytes: receivedBytes,
                        receivedSegments: receivedSegments
                    )
                }

                await reporter.complete(
                    taskId: task.taskId,
                    completedPath: self.completedPath(for: task, plan: plan)
                )
            } catch is CancellationError {
                return
            } catch {
                await reporter.fail(
                    taskId: task.taskId,
                    error: VesperDownloadError(
                        codeOrdinal: 3,
                        categoryOrdinal: 2,
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

    private func executionPlan(for task: VesperDownloadTaskSnapshot) throws -> [ForegroundDownloadEntry] {
        let resources = try task.assetIndex.resources.map {
            ForegroundDownloadEntry(
                url: try resolveURL($0.uri),
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
            let relativeURL = URL(fileURLWithPath: relativePath)
            if relativeURL.path.hasPrefix("/") {
                return relativeURL
            }
            return baseDirectory.appendingPathComponent(relativePath)
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

    private func fetch(
        _ sourceURL: URL,
        byteRange: VesperDownloadByteRange?,
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

        var request = URLRequest(url: sourceURL)
        var requestedRangeStart: UInt64?
        if let byteRange {
            let remaining = byteRange.length > resumeFromBytes ? byteRange.length - resumeFromBytes : 0
            let start = byteRange.offset + resumeFromBytes
            let end = remaining == 0 ? start : start + remaining - 1
            request.setValue("bytes=\(start)-\(end)", forHTTPHeaderField: "Range")
            requestedRangeStart = start
        } else if resumeFromBytes > 0 {
            request.setValue("bytes=\(resumeFromBytes)-", forHTTPHeaderField: "Range")
            requestedRangeStart = resumeFromBytes
        }

        let (bytes, response) = try await URLSession.shared.bytes(for: request)
        if let http = response as? HTTPURLResponse {
            switch http.statusCode {
            case 206:
                let contentRangeStart = parseHttpContentRangeStart(http.value(forHTTPHeaderField: "Content-Range"))
                guard let requestedRangeStart, contentRangeStart == requestedRangeStart else {
                    throw VesperForegroundDownloadPreparationError.invalidSource(
                        "remote server returned an unexpected Content-Range for \(sourceURL.absoluteString)"
                    )
                }
            case 200:
                if requestedRangeStart != nil {
                    if byteRange == nil, resumeFromBytes > 0, allowRestartAfterRangeMismatch {
                        try? fileManager.removeItem(at: destinationURL)
                        await onProgress(0)
                        return try await fetch(
                            sourceURL,
                            byteRange: byteRange,
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
            case 416:
                if resumeFromBytes > 0, allowRestartAfterRangeMismatch {
                    try? fileManager.removeItem(at: destinationURL)
                    await onProgress(0)
                    return try await fetch(
                        sourceURL,
                        byteRange: byteRange,
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
                throw VesperForegroundDownloadPreparationError.invalidSource(
                    "offline download resource is stale or expired (HTTP \(http.statusCode)) for \(sourceURL.absoluteString); refresh the video link and prepare the task again"
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
        defer { try? output.close() }
        if resumeFromBytes > 0 {
            try output.seekToEnd()
        } else {
            try output.truncate(atOffset: 0)
        }

        var totalWritten = resumeFromBytes
        var buffer = Data()
        buffer.reserveCapacity(64 * 1024)

        for try await byte in bytes {
            try Task.checkCancellation()
            buffer.append(byte)
            if buffer.count >= 64 * 1024 {
                try output.write(contentsOf: buffer)
                totalWritten += UInt64(buffer.count)
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
            await onProgress(totalWritten)
        }

        if let expectedSizeBytes, totalWritten != expectedSizeBytes {
            throw VesperForegroundDownloadPreparationError.invalidSource(
                "downloaded \(totalWritten) bytes, expected \(expectedSizeBytes)"
            )
        }
        return totalWritten
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
            try? input.close()
            try? output.close()
        }

        try input.seek(toOffset: (byteRange?.offset ?? 0) + resumeFromBytes)
        if resumeFromBytes > 0 {
            try output.seekToEnd()
        } else {
            try output.truncate(atOffset: 0)
        }

        var totalWritten = resumeFromBytes
        var remaining = byteRange.map { $0.length > resumeFromBytes ? $0.length - resumeFromBytes : 0 }
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
            if let currentRemaining = remaining {
                remaining = currentRemaining > count ? currentRemaining - count : 0
            }
            await onProgress(totalWritten)
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

    private func fetchText(_ sourceUri: String) async throws -> String {
        let sourceURL = try resolveURL(sourceUri)
        let data: Data
        if sourceURL.isFileURL {
            data = try Data(contentsOf: sourceURL)
        } else {
            let (responseData, _) = try await URLSession.shared.data(from: sourceURL)
            data = responseData
        }
        guard let text = String(data: data, encoding: .utf8) else {
            throw VesperForegroundDownloadPreparationError.invalidSource("remote manifest was not valid UTF-8")
        }
        return text
    }

    private func probeRequiredSize(
        _ sourceUri: String,
        byteRange: VesperDownloadByteRange?
    ) async throws -> UInt64 {
        if let byteRange {
            return byteRange.length
        }
        return try await probeContentLength(try resolveURL(sourceUri))
    }

    private func probeContentLength(_ sourceURL: URL) async throws -> UInt64 {
        if sourceURL.isFileURL {
            let values = try sourceURL.resourceValues(forKeys: [.fileSizeKey])
            guard let size = values.fileSize, size > 0 else {
                throw CocoaError(.fileReadUnknown)
            }
            return UInt64(size)
        }

        var request = URLRequest(url: sourceURL)
        request.httpMethod = "HEAD"
        let (_, response) = try await URLSession.shared.data(for: request)
        if let http = response as? HTTPURLResponse,
           let value = http.value(forHTTPHeaderField: "Content-Length"),
           let size = UInt64(value), size > 0
        {
            return size
        }

        var rangeRequest = URLRequest(url: sourceURL)
        rangeRequest.setValue("bytes=0-0", forHTTPHeaderField: "Range")
        let (_, rangeResponse) = try await URLSession.shared.data(for: rangeRequest)
        if let http = rangeResponse as? HTTPURLResponse,
           let contentRange = http.value(forHTTPHeaderField: "Content-Range"),
           let totalText = contentRange.split(separator: "/").last,
           let size = UInt64(totalText), size > 0
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

private func parseHttpContentRangeStart(_ contentRange: String?) -> UInt64? {
    guard let contentRange else {
        return nil
    }
    let fields = contentRange.split(separator: " ", maxSplits: 1)
    guard fields.count == 2 else {
        return nil
    }
    let range = fields[1]
    guard !range.hasPrefix("*"), let startText = range.split(separator: "-", maxSplits: 1).first else {
        return nil
    }
    return UInt64(startText)
}

private struct ForegroundDownloadEntry {
    let url: URL
    let relativePath: String?
    let byteRange: VesperDownloadByteRange?
    let generatedText: String?
    let expectedSizeBytes: UInt64?
    let fallbackName: String
    let isSegment: Bool
}

private enum VesperForegroundDownloadPreparationError: LocalizedError {
    case invalidSource(String)
    case unsupported(String)

    var errorDescription: String? {
        switch self {
        case let .invalidSource(message), let .unsupported(message):
            return message
        }
    }
}

private struct HlsMasterPlaylist {
    let variants: [HlsVariant]
    let audio: [HlsRendition]
}

private struct HlsVariant {
    let uri: String
    let attributes: [String: String]
}

private struct HlsRendition {
    let uri: String
    let attributes: [String: String]
}

private struct HlsMediaPlaylist {
    let targetDuration: String?
    let version: String?
    let maps: [HlsMap]
    let segments: [HlsSegment]
}

private struct HlsMap {
    let uri: String
    let byteRange: VesperDownloadByteRange?
}

private struct HlsSegment {
    let uri: String
    let duration: String?
    let byteRange: VesperDownloadByteRange?
    let sequence: UInt64
}

private struct DashPlannedRepresentation {
    let id: String
    let mediaId: String
    let mimeType: String?
    let codecs: String?
    let bandwidth: String?
    let baseUri: String
    let baseUrl: String?
    let template: DashTemplate?
}

private struct DashTemplate {
    let media: String
    let initialization: String?
    let startNumber: UInt64
    let timescale: UInt64
    let duration: UInt64
}

private func parseHlsMasterPlaylist(
    manifestUri: String,
    manifestText: String
) -> HlsMasterPlaylist {
    var variants: [HlsVariant] = []
    var audio: [HlsRendition] = []
    var pendingVariant: [String: String]?

    for line in nonEmptyTrimmedLines(manifestText) {
        if let value = valueAfterPrefix("#EXT-X-STREAM-INF:", in: line) {
            pendingVariant = parseHlsAttributes(value)
            continue
        }
        if let value = valueAfterPrefix("#EXT-X-MEDIA:", in: line) {
            let attributes = parseHlsAttributes(value)
            if attributes["TYPE"]?.caseInsensitiveCompare("AUDIO") == .orderedSame,
               let uri = attributes["URI"] {
                audio.append(
                    HlsRendition(
                        uri: resolveRemoteReference(baseUri: manifestUri, reference: uri),
                        attributes: attributes
                    )
                )
            }
            continue
        }
        if line.hasPrefix("#") {
            continue
        }
        if let attributes = pendingVariant {
            variants.append(
                HlsVariant(
                    uri: resolveRemoteReference(baseUri: manifestUri, reference: line),
                    attributes: attributes
                )
            )
            pendingVariant = nil
        }
    }

    return HlsMasterPlaylist(variants: variants, audio: audio)
}

private func parseHlsMediaPlaylist(
    playlistUri: String,
    playlistText: String
) throws -> HlsMediaPlaylist {
    var targetDuration: String?
    var version: String?
    var endList = false
    var playlistTypeVod = false
    var pendingDuration: String?
    var pendingByteRange: VesperDownloadByteRange?
    var previousRangeEnd: UInt64 = 0
    var sequence: UInt64 = 0
    var maps: [HlsMap] = []
    var segments: [HlsSegment] = []

    for line in nonEmptyTrimmedLines(playlistText) {
        if let value = valueAfterPrefix("#EXT-X-TARGETDURATION:", in: line) {
            targetDuration = value.trimmingCharacters(in: .whitespacesAndNewlines)
            continue
        }
        if let value = valueAfterPrefix("#EXT-X-VERSION:", in: line) {
            version = value.trimmingCharacters(in: .whitespacesAndNewlines)
            continue
        }
        if line.caseInsensitiveCompare("#EXT-X-ENDLIST") == .orderedSame {
            endList = true
            continue
        }
        if let value = valueAfterPrefix("#EXT-X-PLAYLIST-TYPE:", in: line) {
            playlistTypeVod = value.trimmingCharacters(in: .whitespacesAndNewlines)
                .caseInsensitiveCompare("VOD") == .orderedSame
            continue
        }
        if let value = valueAfterPrefix("#EXT-X-MAP:", in: line) {
            let attributes = parseHlsAttributes(value)
            guard let uri = attributes["URI"] else {
                throw VesperForegroundDownloadPreparationError.invalidSource("HLS EXT-X-MAP was missing URI")
            }
            let byteRange = attributes["BYTERANGE"].flatMap {
                parseHlsByteRange($0, previousRangeEnd: &previousRangeEnd)
            }
            maps.append(
                HlsMap(
                    uri: resolveRemoteReference(baseUri: playlistUri, reference: uri),
                    byteRange: byteRange
                )
            )
            continue
        }
        if let value = valueAfterPrefix("#EXT-X-BYTERANGE:", in: line) {
            pendingByteRange = parseHlsByteRange(value, previousRangeEnd: &previousRangeEnd)
            continue
        }
        if let value = valueAfterPrefix("#EXTINF:", in: line) {
            pendingDuration = value.components(separatedBy: ",").first?
                .trimmingCharacters(in: .whitespacesAndNewlines)
            continue
        }
        if line.hasPrefix("#") {
            continue
        }

        sequence += 1
        segments.append(
            HlsSegment(
                uri: resolveRemoteReference(baseUri: playlistUri, reference: line),
                duration: pendingDuration,
                byteRange: pendingByteRange,
                sequence: sequence
            )
        )
        pendingDuration = nil
        pendingByteRange = nil
    }

    if !endList && !playlistTypeVod {
        throw VesperForegroundDownloadPreparationError.unsupported("HLS download preparation requires a VOD playlist or EXT-X-ENDLIST")
    }
    if segments.isEmpty {
        throw VesperForegroundDownloadPreparationError.invalidSource("HLS media playlist did not contain any segments")
    }

    return HlsMediaPlaylist(
        targetDuration: targetDuration,
        version: version,
        maps: maps,
        segments: segments
    )
}

private func rewriteHlsMaster(
    variantAttributes: [String: String],
    mediaResourceNames: [String]
) -> String {
    let audioPlaylist = mediaResourceNames.first { $0.hasPrefix("audio") }
    let videoPlaylist = mediaResourceNames.first { $0.hasPrefix("video") }
        ?? mediaResourceNames.first
        ?? "video.m3u8"
    let bandwidth = variantAttributes["BANDWIDTH"] ?? "1"
    var text = "#EXTM3U\n#EXT-X-VERSION:3\n"
    if let audioPlaylist {
        text += "#EXT-X-MEDIA:TYPE=AUDIO,GROUP-ID=\"audio\",NAME=\"audio\",DEFAULT=YES,AUTOSELECT=YES,URI=\"\(audioPlaylist)\"\n"
        text += "#EXT-X-STREAM-INF:BANDWIDTH=\(bandwidth),AUDIO=\"audio\"\n"
    } else {
        text += "#EXT-X-STREAM-INF:BANDWIDTH=\(bandwidth)\n"
    }
    text += "\(videoPlaylist)\n"
    return text
}

private func rewriteHlsMedia(
    mediaId: String,
    playlist: HlsMediaPlaylist,
    localMaps: [String: String]
) -> String {
    var text = "#EXTM3U\n"
    text += "#EXT-X-VERSION:\(playlist.version ?? "3")\n"
    text += "#EXT-X-PLAYLIST-TYPE:VOD\n"
    if let targetDuration = playlist.targetDuration {
        text += "#EXT-X-TARGETDURATION:\(targetDuration)\n"
    }
    if let map = playlist.maps.last,
       let path = localMaps[hlsByteRangeKey(uri: map.uri, byteRange: map.byteRange)] {
        text += "#EXT-X-MAP:URI=\"\(path)\"\n"
    }
    for segment in playlist.segments {
        text += "#EXTINF:\(segment.duration ?? "0"),\n"
        text += "segments/\(mediaId)-\(padded(segment.sequence, width: 5)).\(extensionFromUri(segment.uri, fallback: "ts"))\n"
    }
    text += "#EXT-X-ENDLIST\n"
    return text
}

private func parseHlsAttributes(_ input: String) -> [String: String] {
    var attributes: [String: String] = [:]
    for pair in splitQuoted(input, delimiter: ",") {
        let parts = pair.split(separator: "=", maxSplits: 1, omittingEmptySubsequences: false)
        guard parts.count == 2 else { continue }
        let key = parts[0].trimmingCharacters(in: .whitespacesAndNewlines)
        let value = parts[1]
            .trimmingCharacters(in: .whitespacesAndNewlines)
            .trimmingCharacters(in: CharacterSet(charactersIn: "\""))
        if !key.isEmpty {
            attributes[key] = value
        }
    }
    return attributes
}

private func parseHlsByteRange(
    _ value: String,
    previousRangeEnd: inout UInt64
) -> VesperDownloadByteRange? {
    let parts = value.trimmingCharacters(in: .whitespacesAndNewlines)
        .split(separator: "@", maxSplits: 1, omittingEmptySubsequences: false)
    guard let length = UInt64(parts.first?.trimmingCharacters(in: .whitespacesAndNewlines) ?? "") else {
        return nil
    }
    let offset = parts.count > 1
        ? UInt64(parts[1].trimmingCharacters(in: .whitespacesAndNewlines)) ?? previousRangeEnd
        : previousRangeEnd
    previousRangeEnd = offset + length
    return VesperDownloadByteRange(offset: offset, length: length)
}

private func selectDashRepresentations(
    manifestText: String,
    manifestUri: String,
    profile: VesperDownloadProfile
) -> [DashPlannedRepresentation] {
    let mpdBase = directXmlText(manifestText, tag: "BaseURL", before: ["Period", "AdaptationSet", "Representation"])
        .map { resolveRemoteReference(baseUri: manifestUri, reference: $0) }
        ?? manifestUri
    var result: [DashPlannedRepresentation] = []
    let adaptationSets = xmlBlocks(manifestText, tag: "AdaptationSet")

    for (index, adaptationSet) in adaptationSets.enumerated() {
        let adaptationOpenTag = xmlOpenTag(adaptationSet, tag: "AdaptationSet") ?? ""
        let adaptationMimeType = xmlAttrFromTag(adaptationOpenTag, attr: "mimeType")
        let adaptationContentType = xmlAttrFromTag(adaptationOpenTag, attr: "contentType")
        if let adaptationMimeType,
           !adaptationMimeType.hasPrefix("video/"),
           !adaptationMimeType.hasPrefix("audio/") {
            continue
        }

        let adaptationBase = directXmlText(adaptationSet, tag: "BaseURL", before: ["Representation"])
            .map { resolveRemoteReference(baseUri: mpdBase, reference: $0) }
            ?? mpdBase
        let adaptationTemplate = findDashTemplate(prefixBeforeTag(adaptationSet, tag: "Representation"))
        let representations = xmlBlocks(adaptationSet, tag: "Representation")
        guard !representations.isEmpty else {
            continue
        }

        let selectedRepresentation = profile.variantId.flatMap { variantId in
            representations.first { representation in
                xmlAttrFromTag(xmlOpenTag(representation, tag: "Representation") ?? "", attr: "id") == variantId
            }
        } ?? representations.first
        guard let selectedRepresentation else {
            continue
        }

        let representationOpenTag = xmlOpenTag(selectedRepresentation, tag: "Representation") ?? ""
        let id = xmlAttrFromTag(representationOpenTag, attr: "id") ?? "\(index)"
        let representationBase = xmlText(selectedRepresentation, tag: "BaseURL")
        let template = findDashTemplate(selectedRepresentation) ?? adaptationTemplate
        let mimeType = xmlAttrFromTag(representationOpenTag, attr: "mimeType") ?? adaptationMimeType
        let mediaKind: String
        if mimeType?.hasPrefix("audio/") == true || adaptationContentType == "audio" {
            mediaKind = "audio"
        } else if mimeType?.hasPrefix("video/") == true || adaptationContentType == "video" {
            mediaKind = "video"
        } else {
            mediaKind = "media"
        }

        result.append(
            DashPlannedRepresentation(
                id: id,
                mediaId: "\(mediaKind)\(index)",
                mimeType: mimeType,
                codecs: xmlAttrFromTag(representationOpenTag, attr: "codecs"),
                bandwidth: xmlAttrFromTag(representationOpenTag, attr: "bandwidth"),
                baseUri: representationBase.map { resolveRemoteReference(baseUri: adaptationBase, reference: $0) } ?? adaptationBase,
                baseUrl: template == nil ? representationBase : nil,
                template: template
            )
        )
    }

    if result.isEmpty,
       let baseURL = directXmlText(manifestText, tag: "BaseURL", before: ["Period", "AdaptationSet", "Representation"]) {
        result.append(
            DashPlannedRepresentation(
                id: "0",
                mediaId: "media0",
                mimeType: nil,
                codecs: nil,
                bandwidth: nil,
                baseUri: manifestUri,
                baseUrl: baseURL,
                template: nil
            )
        )
    }

    return result
}

private func findDashTemplate(_ input: String) -> DashTemplate? {
    guard
        let tag = xmlOpenTag(input, tag: "SegmentTemplate"),
        let media = xmlAttrFromTag(tag, attr: "media")
    else {
        return nil
    }
    return DashTemplate(
        media: media,
        initialization: xmlAttrFromTag(tag, attr: "initialization"),
        startNumber: xmlAttrFromTag(tag, attr: "startNumber").flatMap(UInt64.init) ?? 1,
        timescale: xmlAttrFromTag(tag, attr: "timescale").flatMap(UInt64.init) ?? 1,
        duration: xmlAttrFromTag(tag, attr: "duration").flatMap(UInt64.init) ?? 0
    )
}

private func rewriteDashMpd(
    duration: String?,
    adaptationSets: [String]
) -> String {
    var text = "<MPD type=\"static\""
    if let duration, !duration.isEmpty {
        text += " mediaPresentationDuration=\"\(escapeXml(duration))\""
    }
    text += " xmlns=\"urn:mpeg:dash:schema:mpd:2011\"><Period>"
    text += adaptationSets.joined()
    text += "</Period></MPD>\n"
    return text
}

private func rewriteDashTemplateAdaptationSet(
    representation: DashPlannedRepresentation,
    template: DashTemplate,
    mediaId: String,
    segmentCount: UInt64
) -> String {
    let mime = representation.mimeType.map { " mimeType=\"\(escapeXml($0))\"" } ?? ""
    let codecs = representation.codecs.map { " codecs=\"\(escapeXml($0))\"" } ?? ""
    let bandwidth = representation.bandwidth ?? "1"
    let initialization = template.initialization == nil ? "" : " initialization=\"segments/\(mediaId)-init.mp4\""
    return "<AdaptationSet\(mime)><Representation id=\"\(escapeXml(representation.id))\" bandwidth=\"\(escapeXml(bandwidth))\"\(codecs)><SegmentTemplate timescale=\"\(template.timescale)\" duration=\"\(template.duration)\" startNumber=\"\(template.startNumber)\"\(initialization) media=\"segments/\(mediaId)-$Number$.m4s\" /></Representation></AdaptationSet><!-- plannedSegments=\(segmentCount) -->"
}

private func rewriteDashSegmentBaseAdaptationSet(
    representation: DashPlannedRepresentation,
    localName: String
) -> String {
    let mime = representation.mimeType.map { " mimeType=\"\(escapeXml($0))\"" } ?? ""
    let codecs = representation.codecs.map { " codecs=\"\(escapeXml($0))\"" } ?? ""
    let bandwidth = representation.bandwidth ?? "1"
    return "<AdaptationSet\(mime)><Representation id=\"\(escapeXml(representation.id))\" bandwidth=\"\(escapeXml(bandwidth))\"\(codecs)><BaseURL>\(escapeXml(localName))</BaseURL><SegmentBase /></Representation></AdaptationSet>"
}

private func expandDashTemplate(
    _ template: String,
    representationId: String,
    number: UInt64
) -> String {
    replaceDashNumberToken(
        template.replacingOccurrences(of: "$RepresentationID$", with: representationId),
        number: number
    )
}

private func replaceDashNumberToken(_ value: String, number: UInt64) -> String {
    var output = value.replacingOccurrences(of: "$Number$", with: "\(number)")
    while let start = output.range(of: "$Number%") {
        guard let end = output[start.upperBound...].firstIndex(of: "$") else {
            return output
        }
        let formatSpec = String(output[start.upperBound..<end])
        let width = Int(formatSpec.trimmingCharacters(in: CharacterSet(charactersIn: "d")).dropFirst()) ?? 0
        output.replaceSubrange(start.lowerBound...end, with: padded(number, width: width))
    }
    return output
}

private func parseIso8601DurationSeconds(_ value: String?) -> Double? {
    guard let value, value.hasPrefix("PT") else {
        return nil
    }
    var number = ""
    var total = 0.0
    for character in value.dropFirst(2) {
        if character.isNumber || character == "." {
            number.append(character)
            continue
        }
        guard let parsed = Double(number) else {
            return nil
        }
        number = ""
        switch character {
        case "H":
            total += parsed * 3600
        case "M":
            total += parsed * 60
        case "S":
            total += parsed
        default:
            return nil
        }
    }
    return total > 0 ? total : nil
}

private func xmlAttr(_ input: String, tag: String, attr: String) -> String? {
    xmlOpenTag(input, tag: tag).flatMap { xmlAttrFromTag($0, attr: attr) }
}

private func xmlOpenTag(_ input: String, tag: String) -> String? {
    guard let start = input.range(of: "<\(tag)") else {
        return nil
    }
    guard let end = input[start.lowerBound...].firstIndex(of: ">") else {
        return nil
    }
    return String(input[start.lowerBound...end])
}

private func xmlAttrFromTag(_ tag: String, attr: String) -> String? {
    guard let attrRange = tag.range(of: "\(attr)=") else {
        return nil
    }
    let valueStartCandidate = attrRange.upperBound
    guard valueStartCandidate < tag.endIndex else {
        return nil
    }
    let quote = tag[valueStartCandidate]
    guard quote == "\"" || quote == "'" else {
        return nil
    }
    let valueStart = tag.index(after: valueStartCandidate)
    guard let valueEnd = tag[valueStart...].firstIndex(of: quote) else {
        return nil
    }
    return String(tag[valueStart..<valueEnd])
}

private func xmlBlocks(_ input: String, tag: String) -> [String] {
    var blocks: [String] = []
    var searchStart = input.startIndex
    let open = "<\(tag)"
    let close = "</\(tag)>"
    while let start = input[searchStart...].range(of: open)?.lowerBound {
        let candidate = input[start...]
        if let closeRange = candidate.range(of: close) {
            blocks.append(String(input[start..<closeRange.upperBound]))
            searchStart = closeRange.upperBound
        } else if let selfCloseRange = candidate.range(of: "/>") {
            blocks.append(String(input[start..<selfCloseRange.upperBound]))
            searchStart = selfCloseRange.upperBound
        } else {
            break
        }
    }
    return blocks
}

private func xmlText(_ input: String, tag: String) -> String? {
    guard let openStart = input.range(of: "<\(tag)")?.lowerBound else {
        return nil
    }
    guard let openEnd = input[openStart...].firstIndex(of: ">") else {
        return nil
    }
    let bodyStart = input.index(after: openEnd)
    guard let closeStart = input[bodyStart...].range(of: "</\(tag)>")?.lowerBound else {
        return nil
    }
    return String(input[bodyStart..<closeStart]).trimmingCharacters(in: .whitespacesAndNewlines)
}

private func directXmlText(_ input: String, tag: String, before childTags: [String]) -> String? {
    let upperBound = childTags
        .compactMap { input.range(of: "<\($0)")?.lowerBound }
        .min() ?? input.endIndex
    return xmlText(String(input[..<upperBound]), tag: tag)
}

private func prefixBeforeTag(_ input: String, tag: String) -> String {
    guard let end = input.range(of: "<\(tag)")?.lowerBound else {
        return input
    }
    return String(input[..<end])
}

private func parseFlvClipManifest(baseUri: String, manifestText: String) -> [String] {
    nonEmptyTrimmedLines(manifestText).compactMap { line in
        if line.hasPrefix("#") || line.caseInsensitiveCompare("ffconcat version 1.0") == .orderedSame {
            return nil
        }
        let rawUri: String
        if valueAfterPrefix("file ", in: line) != nil {
            rawUri = line.dropFirst("file ".count)
                .trimmingCharacters(in: .whitespacesAndNewlines)
                .trimmingCharacters(in: CharacterSet(charactersIn: "\"'"))
        } else {
            rawUri = line
        }
        return rawUri.isEmpty ? nil : resolveRemoteReference(baseUri: baseUri, reference: rawUri)
    }
}

private func resolveRemoteReference(baseUri: String, reference: String) -> String {
    let trimmedReference = reference.trimmingCharacters(in: .whitespacesAndNewlines)
    if let url = URL(string: trimmedReference), url.scheme != nil {
        return url.absoluteString
    }
    if let baseURL = URL(string: baseUri),
       let resolved = URL(string: trimmedReference, relativeTo: baseURL)?.absoluteURL {
        return resolved.absoluteString
    }
    return trimmedReference
}

private func extensionFromUri(_ uri: String, fallback: String) -> String {
    let withoutFragment = uri.components(separatedBy: "#").first ?? uri
    let path = withoutFragment.components(separatedBy: "?").first ?? withoutFragment
    let name = path.components(separatedBy: "/").last ?? ""
    let parts = name.split(separator: ".", omittingEmptySubsequences: false)
    guard
        parts.count > 1,
        let rawExtension = parts.last,
        !rawExtension.isEmpty,
        rawExtension.allSatisfy({ $0.isLetter || $0.isNumber })
    else {
        return fallback
    }
    return String(rawExtension)
}

private func escapeXml(_ value: String) -> String {
    value
        .replacingOccurrences(of: "&", with: "&amp;")
        .replacingOccurrences(of: "\"", with: "&quot;")
        .replacingOccurrences(of: "<", with: "&lt;")
        .replacingOccurrences(of: ">", with: "&gt;")
}

private func escapeFfconcatPath(_ path: String) -> String {
    path.replacingOccurrences(of: "'", with: "'\\''")
}

private func splitQuoted(_ input: String, delimiter: Character) -> [String] {
    var result: [String] = []
    var start = input.startIndex
    var index = input.startIndex
    var inQuotes = false
    while index < input.endIndex {
        let character = input[index]
        if character == "\"" {
            inQuotes.toggle()
        } else if character == delimiter, !inQuotes {
            result.append(String(input[start..<index]).trimmingCharacters(in: .whitespacesAndNewlines))
            start = input.index(after: index)
        }
        index = input.index(after: index)
    }
    result.append(String(input[start...]).trimmingCharacters(in: .whitespacesAndNewlines))
    return result
}

private func valueAfterPrefix(_ prefix: String, in line: String) -> String? {
    guard let range = line.range(of: prefix, options: [.caseInsensitive, .anchored]) else {
        return nil
    }
    return String(line[range.upperBound...])
}

private func nonEmptyTrimmedLines(_ text: String) -> [String] {
    text.components(separatedBy: .newlines)
        .map { $0.trimmingCharacters(in: .whitespacesAndNewlines) }
        .filter { !$0.isEmpty }
}

private func hlsByteRangeKey(uri: String, byteRange: VesperDownloadByteRange?) -> String {
    guard let byteRange else {
        return "\(uri):none"
    }
    return "\(uri):\(byteRange.offset):\(byteRange.length)"
}

private func padded(_ value: UInt64, width: Int) -> String {
    let text = "\(value)"
    guard text.count < width else {
        return text
    }
    return String(repeating: "0", count: width - text.count) + text
}

private extension VesperDownloadResourceRecord {
    func withSizeBytes(_ sizeBytes: UInt64) -> VesperDownloadResourceRecord {
        VesperDownloadResourceRecord(
            resourceId: resourceId,
            uri: uri,
            relativePath: relativePath,
            byteRange: byteRange,
            generatedText: generatedText,
            sizeBytes: sizeBytes,
            etag: etag,
            checksum: checksum
        )
    }

    func withGeneratedText(_ generatedText: String) -> VesperDownloadResourceRecord {
        VesperDownloadResourceRecord(
            resourceId: resourceId,
            uri: uri,
            relativePath: relativePath,
            byteRange: byteRange,
            generatedText: generatedText,
            sizeBytes: sizeBytes,
            etag: etag,
            checksum: checksum
        )
    }
}

private extension VesperDownloadSegmentRecord {
    func withSizeBytes(_ sizeBytes: UInt64) -> VesperDownloadSegmentRecord {
        VesperDownloadSegmentRecord(
            segmentId: segmentId,
            uri: uri,
            relativePath: relativePath,
            sequence: sequence,
            byteRange: byteRange,
            sizeBytes: sizeBytes,
            checksum: checksum
        )
    }
}

private extension VesperDownloadAssetIndex {
    func withResources(_ resources: [VesperDownloadResourceRecord]) -> VesperDownloadAssetIndex {
        VesperDownloadAssetIndex(
            contentFormat: contentFormat,
            version: version,
            etag: etag,
            checksum: checksum,
            totalSizeBytes: totalSizeBytes,
            resources: resources,
            segments: segments,
            completedPath: completedPath
        )
    }
}

private struct RuntimeDownloadCommand {
    enum Kind {
        case prepare
        case start
        case pause
        case resume
        case remove
    }

    let kind: Kind
    let task: VesperDownloadTaskSnapshot?
    let taskId: UInt64

    static func prepare(_ task: VesperDownloadTaskSnapshot) -> Self {
        Self(kind: .prepare, task: task, taskId: task.taskId)
    }

    static func start(_ task: VesperDownloadTaskSnapshot) -> Self {
        Self(kind: .start, task: task, taskId: task.taskId)
    }

    static func resume(_ task: VesperDownloadTaskSnapshot) -> Self {
        Self(kind: .resume, task: task, taskId: task.taskId)
    }

    static func pause(_ taskId: UInt64) -> Self {
        Self(kind: .pause, task: nil, taskId: taskId)
    }

    static func remove(_ taskId: UInt64) -> Self {
        Self(kind: .remove, task: nil, taskId: taskId)
    }
}

private struct NativeDownloadBindings: VesperDownloadManager.DownloadBindings {
    func createDownloadSession(configuration: VesperDownloadConfiguration) -> UInt64 {
        var runtimeConfig = configuration.toRuntimeBridgePayload()
        var handle: UInt64 = 0
        let created = withUnsafePointer(to: &runtimeConfig) { configPointer in
            withUnsafeMutablePointer(to: &handle) { handlePointer in
                vesper_runtime_download_session_create(configPointer, handlePointer)
            }
        }
        freeRuntimeDownloadConfig(&runtimeConfig)
        return created ? handle : 0
    }

    func disposeDownloadSession(_ sessionHandle: UInt64) {
        vesper_runtime_download_session_dispose(sessionHandle)
    }

    func createDownloadTask(
        sessionHandle: UInt64,
        assetId: String,
        source: UnsafePointer<VesperRuntimeDownloadSource>,
        profile: UnsafePointer<VesperRuntimeDownloadProfile>,
        assetIndex: UnsafePointer<VesperRuntimeDownloadAssetIndex>,
        outTaskId: UnsafeMutablePointer<UInt64>
    ) -> Bool {
        assetId.withCString { assetIdPointer in
            vesper_runtime_download_session_create_task(
                sessionHandle,
                assetIdPointer,
                source,
                profile,
                assetIndex,
                outTaskId
            )
        }
    }

    func restoreDownloadTasks(
        sessionHandle: UInt64,
        tasks: UnsafePointer<VesperRuntimeDownloadTask>?,
        taskCount: Int
    ) -> Bool {
        vesper_runtime_download_session_restore_tasks(
            sessionHandle,
            tasks,
            taskCount
        )
    }

    func startDownloadTask(sessionHandle: UInt64, taskId: UInt64) -> Bool {
        vesper_runtime_download_session_start_task(sessionHandle, taskId)
    }

    func pauseDownloadTask(sessionHandle: UInt64, taskId: UInt64) -> Bool {
        vesper_runtime_download_session_pause_task(sessionHandle, taskId)
    }

    func resumeDownloadTask(sessionHandle: UInt64, taskId: UInt64) -> Bool {
        vesper_runtime_download_session_resume_task(sessionHandle, taskId)
    }

    func updateDownloadProgress(
        sessionHandle: UInt64,
        taskId: UInt64,
        receivedBytes: UInt64,
        receivedSegments: UInt32
    ) -> Bool {
        vesper_runtime_download_session_update_progress(
            sessionHandle,
            taskId,
            receivedBytes,
            receivedSegments
        )
    }

    func completeDownloadTask(
        sessionHandle: UInt64,
        taskId: UInt64,
        completedPath: String?
    ) -> Bool {
        guard let completedPath else {
            return vesper_runtime_download_session_complete_task(sessionHandle, taskId, nil)
        }
        return completedPath.withCString { pathPointer in
            vesper_runtime_download_session_complete_task(
                sessionHandle,
                taskId,
                pathPointer
            )
        }
    }

    func completeDownloadPreparation(
        sessionHandle: UInt64,
        taskId: UInt64,
        assetIndex: UnsafePointer<VesperRuntimeDownloadAssetIndex>
    ) -> Bool {
        vesper_runtime_download_session_complete_preparation(
            sessionHandle,
            taskId,
            assetIndex
        )
    }

    func exportDownloadTask(
        sessionHandle: UInt64,
        taskId: UInt64,
        outputPath: String,
        onProgress: @escaping (Float) -> Void,
        isCancelled: @escaping () -> Bool
    ) throws {
        let bridge = DownloadExportProgressBridge(
            onProgress: onProgress,
            isCancelled: isCancelled
        )
        let context = bridge.retainContext()
        let callbacks = VesperRuntimeDownloadExportCallbacks(
            context: context,
            on_progress: { context, ratio in
                DownloadExportProgressBridge.fromContext(context)?.onProgress(ratio)
            },
            is_cancelled: { context in
                DownloadExportProgressBridge.fromContext(context)?.isCancelled() ?? false
            }
        )
        let exported = outputPath.withCString { outputPathPointer in
            vesper_runtime_download_session_export_task(
                sessionHandle,
                taskId,
                outputPathPointer,
                callbacks
            )
        }
        DownloadExportProgressBridge.releaseContext(context)
        if !exported {
            throw DownloadExportBridgeError("download export failed for task \(taskId)")
        }
    }

    func failDownloadTask(
        sessionHandle: UInt64,
        taskId: UInt64,
        error: VesperDownloadError
    ) -> Bool {
        error.message.withCString { messagePointer in
            vesper_runtime_download_session_fail_task(
                sessionHandle,
                taskId,
                error.codeOrdinal,
                error.categoryOrdinal,
                error.retriable,
                messagePointer
            )
        }
    }

    func removeDownloadTask(sessionHandle: UInt64, taskId: UInt64) -> Bool {
        vesper_runtime_download_session_remove_task(sessionHandle, taskId)
    }

    func downloadSessionSnapshot(
        sessionHandle: UInt64,
        outSnapshot: inout VesperRuntimeDownloadSnapshot
    ) -> Bool {
        vesper_runtime_download_session_snapshot(sessionHandle, &outSnapshot)
    }

    func drainDownloadCommands(
        sessionHandle: UInt64,
        outCommands: inout VesperRuntimeDownloadCommandList
    ) -> Bool {
        vesper_runtime_download_session_drain_commands(sessionHandle, &outCommands)
    }

    func drainDownloadEvents(
        sessionHandle: UInt64,
        outEvents: inout VesperRuntimeDownloadEventList
    ) -> Bool {
        vesper_runtime_download_session_drain_events(sessionHandle, &outEvents)
    }

    func freeDownloadSnapshot(_ snapshot: inout VesperRuntimeDownloadSnapshot) {
        vesper_runtime_download_snapshot_free(&snapshot)
    }

    func freeDownloadCommandList(_ commands: inout VesperRuntimeDownloadCommandList) {
        vesper_runtime_download_command_list_free(&commands)
    }

    func freeDownloadEventList(_ events: inout VesperRuntimeDownloadEventList) {
        vesper_runtime_download_event_list_free(&events)
    }
}

private func duplicateDownloadCString(_ value: String) -> UnsafeMutablePointer<CChar>? {
    strdup(value)
}

private func stringFromRuntimeCString(_ pointer: UnsafeMutablePointer<CChar>?) -> String? {
    guard let pointer else {
        return nil
    }
    return String(cString: pointer)
}

private func freeDownloadCString(_ pointer: UnsafeMutablePointer<CChar>?) {
    guard let pointer else {
        return
    }
    free(pointer)
}

private func freeRuntimeDownloadSource(_ source: inout VesperRuntimeDownloadSource) {
    freeDownloadCString(source.source_uri)
    freeDownloadCString(source.manifest_uri)
    source = VesperRuntimeDownloadSource(source_uri: nil, content_format: VesperRuntimeDownloadContentFormatUnknown, manifest_uri: nil)
}

private func freeRuntimeDownloadConfig(_ config: inout VesperRuntimeDownloadConfig) {
    if let pointers = config.plugin_library_paths, config.plugin_library_paths_len > 0 {
        for index in 0..<Int(config.plugin_library_paths_len) {
            freeDownloadCString(pointers[index])
        }
        pointers.deallocate()
    }
    config = VesperRuntimeDownloadConfig(
        auto_start: false,
        run_post_processors_on_completion: false,
        plugin_library_paths: nil,
        plugin_library_paths_len: 0
    )
}

private func freeRuntimeDownloadProfile(_ profile: inout VesperRuntimeDownloadProfile) {
    freeDownloadCString(profile.variant_id)
    freeDownloadCString(profile.preferred_audio_language)
    freeDownloadCString(profile.preferred_subtitle_language)
    if let pointers = profile.selected_track_ids, profile.selected_track_ids_len > 0 {
        for index in 0..<Int(profile.selected_track_ids_len) {
            freeDownloadCString(pointers[index])
        }
        pointers.deallocate()
    }
    freeDownloadCString(profile.target_directory)
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
    freeDownloadCString(assetIndex.version)
    freeDownloadCString(assetIndex.etag)
    freeDownloadCString(assetIndex.checksum)
    if let resources = assetIndex.resources, assetIndex.resources_len > 0 {
        for index in 0..<Int(assetIndex.resources_len) {
            freeDownloadCString(resources[index].resource_id)
            freeDownloadCString(resources[index].uri)
            freeDownloadCString(resources[index].relative_path)
            freeDownloadCString(resources[index].generated_text)
            freeDownloadCString(resources[index].etag)
            freeDownloadCString(resources[index].checksum)
        }
        resources.deallocate()
    }
    if let segments = assetIndex.segments, assetIndex.segments_len > 0 {
        for index in 0..<Int(assetIndex.segments_len) {
            freeDownloadCString(segments[index].segment_id)
            freeDownloadCString(segments[index].uri)
            freeDownloadCString(segments[index].relative_path)
            freeDownloadCString(segments[index].checksum)
        }
        segments.deallocate()
    }
    freeDownloadCString(assetIndex.completed_path)
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

private func freeRuntimeDownloadTask(_ task: inout VesperRuntimeDownloadTask) {
    freeDownloadCString(task.asset_id)
    freeRuntimeDownloadSource(&task.source)
    freeRuntimeDownloadProfile(&task.profile)
    freeRuntimeDownloadAssetIndex(&task.asset_index)
    freeDownloadCString(task.error_message)
    task = VesperRuntimeDownloadTask(
        task_id: 0,
        asset_id: nil,
        source: VesperRuntimeDownloadSource(source_uri: nil, content_format: VesperRuntimeDownloadContentFormatUnknown, manifest_uri: nil),
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
            completed_path: nil
        ),
        has_error: false,
        error_code: 0,
        error_category: 0,
        error_retriable: false,
        error_message: nil
    )
}

private extension VesperDownloadConfiguration {
    func toRuntimeBridgePayload() -> VesperRuntimeDownloadConfig {
        let pointer: UnsafeMutablePointer<UnsafeMutablePointer<CChar>?>?
        if pluginLibraryPaths.isEmpty {
            pointer = nil
        } else {
            pointer = .allocate(capacity: pluginLibraryPaths.count)
            for (index, value) in pluginLibraryPaths.enumerated() {
                pointer?[index] = duplicateDownloadCString(value)
            }
        }

        return VesperRuntimeDownloadConfig(
            auto_start: autoStart,
            run_post_processors_on_completion: runPostProcessorsOnCompletion,
            plugin_library_paths: pointer,
            plugin_library_paths_len: UInt(pluginLibraryPaths.count)
        )
    }
}

private extension VesperDownloadProgressSnapshot {
    func toRuntimeBridgePayload() -> VesperRuntimeDownloadProgressSnapshot {
        VesperRuntimeDownloadProgressSnapshot(
            received_bytes: receivedBytes,
            has_total_bytes: totalBytes != nil,
            total_bytes: totalBytes ?? 0,
            received_segments: receivedSegments,
            has_total_segments: totalSegments != nil,
            total_segments: totalSegments ?? 0
        )
    }
}

private extension VesperDownloadTaskSnapshot {
    func toRuntimeBridgePayload() -> VesperRuntimeDownloadTask {
        VesperRuntimeDownloadTask(
            task_id: taskId,
            asset_id: duplicateDownloadCString(assetId),
            source: source.toRuntimeBridgePayload(),
            profile: profile.toRuntimeBridgePayload(),
            status: VesperRuntimeDownloadTaskStatus(rawValue: UInt32(state.rawValue)),
            progress: progress.toRuntimeBridgePayload(),
            asset_index: assetIndex.toRuntimeBridgePayload(),
            has_error: error != nil,
            error_code: error?.codeOrdinal ?? 0,
            error_category: error?.categoryOrdinal ?? 0,
            error_retriable: error?.retriable ?? false,
            error_message: error.flatMap { duplicateDownloadCString($0.message) }
        )
    }
}

private extension VesperDownloadSource {
    func toRuntimeBridgePayload() -> VesperRuntimeDownloadSource {
        VesperRuntimeDownloadSource(
            source_uri: duplicateDownloadCString(source.uri),
            content_format: VesperRuntimeDownloadContentFormat(rawValue: contentFormat.rawValue)
                ?? VesperRuntimeDownloadContentFormatUnknown,
            manifest_uri: manifestUri.flatMap(duplicateDownloadCString)
        )
    }
}

private extension VesperDownloadProfile {
    func toRuntimeBridgePayload() -> VesperRuntimeDownloadProfile {
        let pointer: UnsafeMutablePointer<UnsafeMutablePointer<CChar>?>?
        if selectedTrackIds.isEmpty {
            pointer = nil
        } else {
            pointer = .allocate(capacity: selectedTrackIds.count)
            for (index, value) in selectedTrackIds.enumerated() {
                pointer?[index] = duplicateDownloadCString(value)
            }
        }

        return VesperRuntimeDownloadProfile(
            variant_id: variantId.flatMap(duplicateDownloadCString),
            preferred_audio_language: preferredAudioLanguage.flatMap(duplicateDownloadCString),
            preferred_subtitle_language: preferredSubtitleLanguage.flatMap(duplicateDownloadCString),
            selected_track_ids: pointer,
            selected_track_ids_len: UInt(selectedTrackIds.count),
            has_target_output_format: targetOutputFormat != nil,
            target_output_format: VesperRuntimeDownloadOutputFormat(
                rawValue: UInt32(targetOutputFormat?.rawValue ?? 2)
            ),
            target_directory: targetDirectory.flatMap { duplicateDownloadCString($0.path) },
            allow_metered_network: allowMeteredNetwork
        )
    }
}

private extension VesperDownloadResourceRecord {
    func toRuntimeBridgePayload() -> VesperRuntimeDownloadResourceRecord {
        VesperRuntimeDownloadResourceRecord(
            resource_id: duplicateDownloadCString(resourceId),
            uri: duplicateDownloadCString(uri),
            relative_path: relativePath.flatMap(duplicateDownloadCString),
            has_byte_range: byteRange != nil,
            byte_range: byteRange?.toRuntimeBridgePayload() ?? VesperRuntimeDownloadByteRange(offset: 0, length: 0),
            generated_text: generatedText.flatMap(duplicateDownloadCString),
            has_size_bytes: sizeBytes != nil,
            size_bytes: sizeBytes ?? 0,
            etag: etag.flatMap(duplicateDownloadCString),
            checksum: checksum.flatMap(duplicateDownloadCString)
        )
    }
}

private extension VesperDownloadByteRange {
    func toRuntimeBridgePayload() -> VesperRuntimeDownloadByteRange {
        VesperRuntimeDownloadByteRange(offset: offset, length: length)
    }
}

private extension VesperDownloadSegmentRecord {
    func toRuntimeBridgePayload() -> VesperRuntimeDownloadSegmentRecord {
        VesperRuntimeDownloadSegmentRecord(
            segment_id: duplicateDownloadCString(segmentId),
            uri: duplicateDownloadCString(uri),
            relative_path: relativePath.flatMap(duplicateDownloadCString),
            has_sequence: sequence != nil,
            sequence: sequence ?? 0,
            has_byte_range: byteRange != nil,
            byte_range: byteRange?.toRuntimeBridgePayload() ?? VesperRuntimeDownloadByteRange(offset: 0, length: 0),
            has_size_bytes: sizeBytes != nil,
            size_bytes: sizeBytes ?? 0,
            checksum: checksum.flatMap(duplicateDownloadCString)
        )
    }
}

private extension VesperDownloadAssetIndex {
    func toRuntimeBridgePayload() -> VesperRuntimeDownloadAssetIndex {
        let resourcePointer: UnsafeMutablePointer<VesperRuntimeDownloadResourceRecord>?
        if resources.isEmpty {
            resourcePointer = nil
        } else {
            resourcePointer = .allocate(capacity: resources.count)
            for (index, item) in resources.enumerated() {
                resourcePointer?[index] = item.toRuntimeBridgePayload()
            }
        }

        let segmentPointer: UnsafeMutablePointer<VesperRuntimeDownloadSegmentRecord>?
        if segments.isEmpty {
            segmentPointer = nil
        } else {
            segmentPointer = .allocate(capacity: segments.count)
            for (index, item) in segments.enumerated() {
                segmentPointer?[index] = item.toRuntimeBridgePayload()
            }
        }

        return VesperRuntimeDownloadAssetIndex(
            content_format: VesperRuntimeDownloadContentFormat(rawValue: contentFormat.rawValue)
                ?? VesperRuntimeDownloadContentFormatUnknown,
            version: version.flatMap(duplicateDownloadCString),
            etag: etag.flatMap(duplicateDownloadCString),
            checksum: checksum.flatMap(duplicateDownloadCString),
            has_total_size_bytes: totalSizeBytes != nil,
            total_size_bytes: totalSizeBytes ?? 0,
            resources: resourcePointer,
            resources_len: UInt(resources.count),
            segments: segmentPointer,
            segments_len: UInt(segments.count),
            completed_path: completedPath.flatMap(duplicateDownloadCString)
        )
    }
}

private extension VesperRuntimeDownloadSnapshot {
    func toPublic() -> VesperDownloadSnapshot {
        guard let tasks, len > 0 else {
            return VesperDownloadSnapshot(tasks: [])
        }
        return VesperDownloadSnapshot(
            tasks: Array(UnsafeBufferPointer(start: tasks, count: Int(len))).map { $0.toPublic() }
        )
    }
}

private extension VesperRuntimeDownloadTask {
    func toPublic() -> VesperDownloadTaskSnapshot {
        let assetId = stringFromRuntimeCString(asset_id) ?? ""
        let error: VesperDownloadError?
        if has_error {
            error = VesperDownloadError(
                codeOrdinal: error_code,
                categoryOrdinal: error_category,
                retriable: error_retriable,
                message: stringFromRuntimeCString(error_message) ?? "download failed"
            )
        } else {
            error = nil
        }

        return VesperDownloadTaskSnapshot(
            taskId: task_id,
            assetId: assetId,
            source: source.toPublic(),
            profile: profile.toPublic(),
            state: VesperDownloadState(rawValue: Int(status.rawValue)) ?? .queued,
            progress: progress.toPublic(),
            assetIndex: asset_index.toPublic(),
            error: error
        )
    }
}

private extension VesperRuntimeDownloadSource {
    func toPublic() -> VesperDownloadSource {
        let uri = stringFromRuntimeCString(source_uri) ?? ""
        let source: VesperPlayerSource
        if let url = URL(string: uri), url.isFileURL {
            source = .localFile(url: url)
        } else if let url = URL(string: uri) {
            source = .remoteUrl(url)
        } else {
            source = VesperPlayerSource(uri: uri, label: uri, kind: .remote, protocol: .unknown)
        }
        return VesperDownloadSource(
            source: source,
            contentFormat: VesperDownloadContentFormat(rawValue: Int(content_format.rawValue)) ?? .unknown,
            manifestUri: stringFromRuntimeCString(manifest_uri)
        )
    }
}

private extension VesperRuntimeDownloadProfile {
    func toPublic() -> VesperDownloadProfile {
        let selectedTrackIds: [String]
        if let selected_track_ids, selected_track_ids_len > 0 {
            selectedTrackIds = (0..<Int(selected_track_ids_len)).compactMap { index in
                stringFromRuntimeCString(selected_track_ids[index])
            }
        } else {
            selectedTrackIds = []
        }

        return VesperDownloadProfile(
            variantId: stringFromRuntimeCString(variant_id),
            preferredAudioLanguage: stringFromRuntimeCString(preferred_audio_language),
            preferredSubtitleLanguage: stringFromRuntimeCString(preferred_subtitle_language),
            selectedTrackIds: selectedTrackIds,
            targetOutputFormat: has_target_output_format
                ? VesperDownloadOutputFormat(rawValue: Int(target_output_format.rawValue))
                : nil,
            targetDirectory: stringFromRuntimeCString(target_directory).map(URL.init(fileURLWithPath:)),
            allowMeteredNetwork: allow_metered_network
        )
    }
}

private extension VesperRuntimeDownloadAssetIndex {
    func toPublic() -> VesperDownloadAssetIndex {
        let publicResources: [VesperDownloadResourceRecord]
        if let resourcesPointer = self.resources, self.resources_len > 0 {
            publicResources = Array(
                UnsafeBufferPointer(start: resourcesPointer, count: Int(self.resources_len))
            )
                .map { $0.toPublic() }
        } else {
            publicResources = []
        }

        let publicSegments: [VesperDownloadSegmentRecord]
        if let segmentsPointer = self.segments, self.segments_len > 0 {
            publicSegments = Array(
                UnsafeBufferPointer(start: segmentsPointer, count: Int(self.segments_len))
            )
                .map { $0.toPublic() }
        } else {
            publicSegments = []
        }

        return VesperDownloadAssetIndex(
            contentFormat: VesperDownloadContentFormat(rawValue: Int(content_format.rawValue)) ?? .unknown,
            version: stringFromRuntimeCString(version),
            etag: stringFromRuntimeCString(etag),
            checksum: stringFromRuntimeCString(checksum),
            totalSizeBytes: has_total_size_bytes ? total_size_bytes : nil,
            resources: publicResources,
            segments: publicSegments,
            completedPath: stringFromRuntimeCString(completed_path)
        )
    }
}

private extension VesperRuntimeDownloadResourceRecord {
    func toPublic() -> VesperDownloadResourceRecord {
        VesperDownloadResourceRecord(
            resourceId: stringFromRuntimeCString(resource_id) ?? "",
            uri: stringFromRuntimeCString(uri) ?? "",
            relativePath: stringFromRuntimeCString(relative_path),
            byteRange: has_byte_range ? byte_range.toPublic() : nil,
            generatedText: stringFromRuntimeCString(generated_text),
            sizeBytes: has_size_bytes ? size_bytes : nil,
            etag: stringFromRuntimeCString(etag),
            checksum: stringFromRuntimeCString(checksum)
        )
    }
}

private extension VesperRuntimeDownloadSegmentRecord {
    func toPublic() -> VesperDownloadSegmentRecord {
        VesperDownloadSegmentRecord(
            segmentId: stringFromRuntimeCString(segment_id) ?? "",
            uri: stringFromRuntimeCString(uri) ?? "",
            relativePath: stringFromRuntimeCString(relative_path),
            sequence: has_sequence ? sequence : nil,
            byteRange: has_byte_range ? byte_range.toPublic() : nil,
            sizeBytes: has_size_bytes ? size_bytes : nil,
            checksum: stringFromRuntimeCString(checksum)
        )
    }
}

private extension VesperRuntimeDownloadByteRange {
    func toPublic() -> VesperDownloadByteRange {
        VesperDownloadByteRange(offset: offset, length: length)
    }
}

private extension VesperRuntimeDownloadProgressSnapshot {
    func toPublic() -> VesperDownloadProgressSnapshot {
        VesperDownloadProgressSnapshot(
            receivedBytes: received_bytes,
            totalBytes: has_total_bytes ? total_bytes : nil,
            receivedSegments: received_segments,
            totalSegments: has_total_segments ? total_segments : nil
        )
    }
}

private extension VesperRuntimeDownloadCommandList {
    func toPublic() -> [RuntimeDownloadCommand] {
        guard let commands, len > 0 else {
            return []
        }
        return Array(UnsafeBufferPointer(start: commands, count: Int(len))).compactMap { command in
            switch command.kind {
            case .prepare:
                return .prepare(command.task.toPublic())
            case .start:
                return .start(command.task.toPublic())
            case .pause:
                return .pause(command.task_id)
            case .resume:
                return .resume(command.task.toPublic())
            case .remove:
                return .remove(command.task_id)
            default:
                return nil
            }
        }
    }
}

private extension VesperRuntimeDownloadEventList {
    func toPublic() -> [VesperDownloadEvent] {
        guard let events, len > 0 else {
            return []
        }
        return Array(UnsafeBufferPointer(start: events, count: Int(len))).compactMap { event in
            switch event.kind {
            case .created:
                return .created(event.task.toPublic())
            case .stateChanged:
                return .stateChanged(event.task.toPublic())
            case .assetIndexUpdated:
                return .assetIndexUpdated(event.task.toPublic())
            case .progressUpdated:
                return .progressUpdated(event.task.toPublic())
            default:
                return nil
            }
        }
    }
}

private extension VesperRuntimeDownloadCommandKind {
    static var prepare: VesperRuntimeDownloadCommandKind { VesperRuntimeDownloadCommandKindPrepare }
    static var start: VesperRuntimeDownloadCommandKind { VesperRuntimeDownloadCommandKindStart }
    static var pause: VesperRuntimeDownloadCommandKind { VesperRuntimeDownloadCommandKindPause }
    static var resume: VesperRuntimeDownloadCommandKind { VesperRuntimeDownloadCommandKindResume }
    static var remove: VesperRuntimeDownloadCommandKind { VesperRuntimeDownloadCommandKindRemove }
}

private extension VesperRuntimeDownloadEventKind {
    static var created: VesperRuntimeDownloadEventKind { VesperRuntimeDownloadEventKindCreated }
    static var stateChanged: VesperRuntimeDownloadEventKind { VesperRuntimeDownloadEventKindStateChanged }
    static var assetIndexUpdated: VesperRuntimeDownloadEventKind { VesperRuntimeDownloadEventKindAssetIndexUpdated }
    static var progressUpdated: VesperRuntimeDownloadEventKind { VesperRuntimeDownloadEventKindProgressUpdated }
}

private extension VesperRuntimeDownloadContentFormat {
    init?(rawValue: Int) {
        switch rawValue {
        case 0: self = VesperRuntimeDownloadContentFormatHlsSegments
        case 1: self = VesperRuntimeDownloadContentFormatDashSegments
        case 2: self = VesperRuntimeDownloadContentFormatFlvSegments
        case 3: self = VesperRuntimeDownloadContentFormatSingleFile
        case 4: self = VesperRuntimeDownloadContentFormatUnknown
        default: return nil
        }
    }
}

private final class DownloadExportProgressBridge: @unchecked Sendable {
    let onProgress: (Float) -> Void
    let isCancelled: () -> Bool

    init(
        onProgress: @escaping (Float) -> Void,
        isCancelled: @escaping () -> Bool
    ) {
        self.onProgress = onProgress
        self.isCancelled = isCancelled
    }

    func retainContext() -> UnsafeMutableRawPointer {
        UnsafeMutableRawPointer(Unmanaged.passRetained(self).toOpaque())
    }

    static func releaseContext(_ context: UnsafeMutableRawPointer?) {
        guard let context else {
            return
        }
        Unmanaged<DownloadExportProgressBridge>.fromOpaque(context).release()
    }

    static func fromContext(_ context: UnsafeMutableRawPointer?) -> DownloadExportProgressBridge? {
        guard let context else {
            return nil
        }
        return Unmanaged<DownloadExportProgressBridge>.fromOpaque(context).takeUnretainedValue()
    }
}

private struct DownloadExportBridgeError: LocalizedError {
    let message: String

    init(_ message: String) {
        self.message = message
    }

    var errorDescription: String? { message }
}
