import Combine
import Foundation
import VesperPlayerKitBridgeShim

public typealias VesperDownloadAssetId = String
public typealias VesperDownloadTaskId = UInt64

public enum VesperDownloadContentFormat: Int, Equatable {
    case hlsSegments = 0
    case dashSegments = 1
    case singleFile = 2
    case unknown = 3
}

public struct VesperDownloadConfiguration: Equatable {
    public let autoStart: Bool
    public let baseDirectory: URL?
    public let pluginLibraryPaths: [String]

    public init(
        autoStart: Bool = true,
        baseDirectory: URL? = nil,
        pluginLibraryPaths: [String] = []
    ) {
        self.autoStart = autoStart
        self.baseDirectory = baseDirectory
        self.pluginLibraryPaths = pluginLibraryPaths
    }
}

public struct VesperDownloadSource: Equatable {
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

public struct VesperDownloadProfile: Equatable {
    public let variantId: String?
    public let preferredAudioLanguage: String?
    public let preferredSubtitleLanguage: String?
    public let selectedTrackIds: [String]
    public let targetDirectory: URL?
    public let allowMeteredNetwork: Bool

    public init(
        variantId: String? = nil,
        preferredAudioLanguage: String? = nil,
        preferredSubtitleLanguage: String? = nil,
        selectedTrackIds: [String] = [],
        targetDirectory: URL? = nil,
        allowMeteredNetwork: Bool = false
    ) {
        self.variantId = variantId
        self.preferredAudioLanguage = preferredAudioLanguage
        self.preferredSubtitleLanguage = preferredSubtitleLanguage
        self.selectedTrackIds = selectedTrackIds
        self.targetDirectory = targetDirectory
        self.allowMeteredNetwork = allowMeteredNetwork
    }
}

public struct VesperDownloadResourceRecord: Equatable {
    public let resourceId: String
    public let uri: String
    public let relativePath: String?
    public let sizeBytes: UInt64?
    public let etag: String?
    public let checksum: String?

    public init(
        resourceId: String,
        uri: String,
        relativePath: String? = nil,
        sizeBytes: UInt64? = nil,
        etag: String? = nil,
        checksum: String? = nil
    ) {
        self.resourceId = resourceId
        self.uri = uri
        self.relativePath = relativePath
        self.sizeBytes = sizeBytes
        self.etag = etag
        self.checksum = checksum
    }
}

public struct VesperDownloadSegmentRecord: Equatable {
    public let segmentId: String
    public let uri: String
    public let relativePath: String?
    public let sequence: UInt64?
    public let sizeBytes: UInt64?
    public let checksum: String?

    public init(
        segmentId: String,
        uri: String,
        relativePath: String? = nil,
        sequence: UInt64? = nil,
        sizeBytes: UInt64? = nil,
        checksum: String? = nil
    ) {
        self.segmentId = segmentId
        self.uri = uri
        self.relativePath = relativePath
        self.sequence = sequence
        self.sizeBytes = sizeBytes
        self.checksum = checksum
    }
}

public struct VesperDownloadAssetIndex: Equatable {
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

public struct VesperDownloadProgressSnapshot: Equatable {
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

public enum VesperDownloadState: Int, Equatable {
    case queued = 0
    case preparing = 1
    case downloading = 2
    case paused = 3
    case completed = 4
    case failed = 5
    case removed = 6
}

public struct VesperDownloadError: Equatable {
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

public struct VesperDownloadTaskSnapshot: Equatable {
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

public struct VesperDownloadSnapshot: Equatable {
    public let tasks: [VesperDownloadTaskSnapshot]

    public init(tasks: [VesperDownloadTaskSnapshot]) {
        self.tasks = tasks
    }
}

public enum VesperDownloadEvent: Equatable {
    case created(VesperDownloadTaskSnapshot)
    case stateChanged(VesperDownloadTaskSnapshot)
    case progressUpdated(VesperDownloadTaskSnapshot)
}

@MainActor
public protocol VesperDownloadExecutionReporter: AnyObject {
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
    private var eventBuffer: [VesperDownloadEvent] = []
    private var sessionHandle: UInt64 = 0

    public init(
        configuration: VesperDownloadConfiguration = VesperDownloadConfiguration(),
        executor: (any VesperDownloadExecutor)? = nil
    ) {
        self.executor = executor ?? VesperForegroundDownloadExecutor(baseDirectory: configuration.baseDirectory)
        bindings = NativeDownloadBindings()
        snapshot = VesperDownloadSnapshot(tasks: [])
        sessionHandle = bindings.createDownloadSession(configuration: configuration)
        precondition(sessionHandle != 0, "native download session handle must not be zero")
        refresh()
    }

    internal init(
        configuration: VesperDownloadConfiguration,
        executor: any VesperDownloadExecutor,
        bindings: any DownloadBindings
    ) {
        self.executor = executor
        self.bindings = bindings
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

    private final class RuntimeReporter: VesperDownloadExecutionReporter {
        private weak var manager: VesperDownloadManager?

        init(manager: VesperDownloadManager) {
            self.manager = manager
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

    internal protocol DownloadBindings {
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

public final class VesperForegroundDownloadExecutor: VesperDownloadExecutor {
    private let lock = NSLock()
    private let fileManager = FileManager.default
    private var tasks: [VesperDownloadTaskId: Task<Void, Never>] = [:]
    private let baseDirectory: URL?

    public init(baseDirectory: URL? = nil) {
        self.baseDirectory = baseDirectory
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

                    if self.fileManager.fileExists(atPath: destinationURL.path) {
                        try? self.fileManager.removeItem(at: destinationURL)
                    }

                    let writtenBytes = try await self.fetch(entry.url, to: destinationURL)
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
                fallbackName: $0.resourceId.isEmpty ? "resource" : $0.resourceId,
                isSegment: false
            )
        }
        if !resources.isEmpty {
            return resources
        }

        let segments = try task.assetIndex.segments.enumerated().map { index, segment in
            ForegroundDownloadEntry(
                url: try resolveURL(segment.uri),
                relativePath: segment.relativePath,
                fallbackName: segment.segmentId.isEmpty ? "segment-\(index + 1)" : segment.segmentId,
                isSegment: true
            )
        }
        if !segments.isEmpty {
            return segments
        }

        return [
            ForegroundDownloadEntry(
                url: try resolveURL(task.source.manifestUri ?? task.source.source.uri),
                relativePath: nil,
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

    private func fetch(_ sourceURL: URL, to destinationURL: URL) async throws -> UInt64 {
        if sourceURL.isFileURL {
            let data = try Data(contentsOf: sourceURL)
            try data.write(to: destinationURL, options: .atomic)
            return UInt64(data.count)
        }

        let (data, _) = try await URLSession.shared.data(from: sourceURL)
        try data.write(to: destinationURL, options: .atomic)
        return UInt64(data.count)
    }
}

private struct ForegroundDownloadEntry {
    let url: URL
    let relativePath: String?
    let fallbackName: String
    let isSegment: Bool
}

private struct RuntimeDownloadCommand {
    enum Kind {
        case start
        case pause
        case resume
        case remove
    }

    let kind: Kind
    let task: VesperDownloadTaskSnapshot?
    let taskId: UInt64

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
            plugin_library_paths: pointer,
            plugin_library_paths_len: UInt(pluginLibraryPaths.count)
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
            has_size_bytes: sizeBytes != nil,
            size_bytes: sizeBytes ?? 0,
            etag: etag.flatMap(duplicateDownloadCString),
            checksum: checksum.flatMap(duplicateDownloadCString)
        )
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
            sizeBytes: has_size_bytes ? size_bytes : nil,
            checksum: stringFromRuntimeCString(checksum)
        )
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
            case .progressUpdated:
                return .progressUpdated(event.task.toPublic())
            default:
                return nil
            }
        }
    }
}

private extension VesperRuntimeDownloadCommandKind {
    static var start: VesperRuntimeDownloadCommandKind { VesperRuntimeDownloadCommandKindStart }
    static var pause: VesperRuntimeDownloadCommandKind { VesperRuntimeDownloadCommandKindPause }
    static var resume: VesperRuntimeDownloadCommandKind { VesperRuntimeDownloadCommandKindResume }
    static var remove: VesperRuntimeDownloadCommandKind { VesperRuntimeDownloadCommandKindRemove }
}

private extension VesperRuntimeDownloadEventKind {
    static var created: VesperRuntimeDownloadEventKind { VesperRuntimeDownloadEventKindCreated }
    static var stateChanged: VesperRuntimeDownloadEventKind { VesperRuntimeDownloadEventKindStateChanged }
    static var progressUpdated: VesperRuntimeDownloadEventKind { VesperRuntimeDownloadEventKindProgressUpdated }
}

private extension VesperRuntimeDownloadContentFormat {
    init?(rawValue: Int) {
        switch rawValue {
        case 0: self = VesperRuntimeDownloadContentFormatHlsSegments
        case 1: self = VesperRuntimeDownloadContentFormatDashSegments
        case 2: self = VesperRuntimeDownloadContentFormatSingleFile
        case 3: self = VesperRuntimeDownloadContentFormatUnknown
        default: return nil
        }
    }
}
