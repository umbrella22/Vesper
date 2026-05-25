import Combine
import Foundation
import VesperPlayerKitBridgeShim
#if canImport(UIKit)
import UIKit
#endif

@usableFromInline let vesperDownloadDefaultMinProgressBytes: UInt64 = 512 * 1024
@usableFromInline let vesperDownloadDefaultMinProgressIntervalMs: UInt64 = 250
@usableFromInline let vesperDownloadDefaultStalledTransferTimeoutMs: UInt64 = 30_000

public typealias VesperDownloadAssetId = String
public typealias VesperDownloadTaskId = UInt64

let vesperDownloadATSFailureMessage =
    "iOS offline downloads require HTTPS media URLs. The SDK does not relax App Transport Security for http:// resources; host apps that need insecure HTTP must fetch those resources outside the SDK and provide local file URLs."

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
    public let rangeChunkBytes: UInt64?
    public let minProgressBytes: UInt64
    public let minProgressIntervalMs: UInt64
    public let stalledTransferTimeoutMs: UInt64

    public init(
        autoStart: Bool = true,
        runPostProcessorsOnCompletion: Bool = true,
        resumePartialDownloads: Bool = true,
        restoreTasksOnStartup: Bool = true,
        baseDirectory: URL? = nil,
        pluginLibraryPaths: [String] = [],
        rangeChunkBytes: UInt64? = nil,
        minProgressBytes: UInt64 = vesperDownloadDefaultMinProgressBytes,
        minProgressIntervalMs: UInt64 = vesperDownloadDefaultMinProgressIntervalMs,
        stalledTransferTimeoutMs: UInt64 = vesperDownloadDefaultStalledTransferTimeoutMs
    ) {
        self.autoStart = autoStart
        self.runPostProcessorsOnCompletion = runPostProcessorsOnCompletion
        self.resumePartialDownloads = resumePartialDownloads
        self.restoreTasksOnStartup = restoreTasksOnStartup
        self.baseDirectory = baseDirectory
        self.pluginLibraryPaths = pluginLibraryPaths
        self.rangeChunkBytes = rangeChunkBytes.flatMap { $0 > 0 ? $0 : nil }
        self.minProgressBytes = max(minProgressBytes, 1)
        self.minProgressIntervalMs = minProgressIntervalMs
        self.stalledTransferTimeoutMs = stalledTransferTimeoutMs
    }
}

public enum VesperDownloadStaleResourcePhase: String, Equatable, Codable {
    case prepare
    case download
}

public struct VesperDownloadStaleResource: Equatable {
    public let taskId: VesperDownloadTaskId
    public let resourceId: String?
    public let segmentId: String?
    public let uri: String?
    public let phase: VesperDownloadStaleResourcePhase
    public let statusCode: Int?
    public let receivedBytes: UInt64
    public let message: String

    public init(
        taskId: VesperDownloadTaskId,
        resourceId: String? = nil,
        segmentId: String? = nil,
        uri: String? = nil,
        phase: VesperDownloadStaleResourcePhase = .prepare,
        statusCode: Int? = nil,
        receivedBytes: UInt64 = 0,
        message: String
    ) {
        self.taskId = taskId
        self.resourceId = resourceId
        self.segmentId = segmentId
        self.uri = uri
        self.phase = phase
        self.statusCode = statusCode
        self.receivedBytes = receivedBytes
        self.message = message
    }
}

public struct VesperDownloadRecoveredTaskPlan: Equatable {
    public let source: VesperDownloadSource
    public let profile: VesperDownloadProfile
    public let assetIndex: VesperDownloadAssetIndex

    public init(
        source: VesperDownloadSource,
        profile: VesperDownloadProfile,
        assetIndex: VesperDownloadAssetIndex
    ) {
        self.source = source
        self.profile = profile
        self.assetIndex = assetIndex
    }
}

@available(*, deprecated, message: "Use VesperDownloadStaleResourcePlanRecoveryHandler to refresh source, profile, and asset index together.")
public typealias VesperDownloadStaleResourceRecoveryHandler =
    @Sendable (VesperDownloadTaskSnapshot, VesperDownloadStaleResource) async -> VesperDownloadSource?

public typealias VesperDownloadStaleResourcePlanRecoveryHandler =
    @Sendable (VesperDownloadTaskSnapshot, VesperDownloadStaleResource) async -> VesperDownloadRecoveredTaskPlan?

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

public enum VesperDownloadStreamKind: String, Equatable, Codable {
    case combined
    case video
    case audio
    case secondaryAudio
    case subtitle
    case auxiliary
}

public struct VesperDownloadAssetStream: Equatable, Codable {
    public let streamId: String
    public let kind: VesperDownloadStreamKind
    public let language: String?
    public let codec: String?
    public let label: String?
    public let qualityRank: UInt32?
    public let resourceIds: [String]
    public let segmentIds: [String]
    public let metadata: [String: String]

    public init(
        streamId: String,
        kind: VesperDownloadStreamKind = .combined,
        language: String? = nil,
        codec: String? = nil,
        label: String? = nil,
        qualityRank: UInt32? = nil,
        resourceIds: [String] = [],
        segmentIds: [String] = [],
        metadata: [String: String] = [:]
    ) {
        self.streamId = streamId
        self.kind = kind
        self.language = language
        self.codec = codec
        self.label = label
        self.qualityRank = qualityRank
        self.resourceIds = resourceIds
        self.segmentIds = segmentIds
        self.metadata = metadata
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
    public let streams: [VesperDownloadAssetStream]
    public let completedPath: String?

    public init(
        contentFormat: VesperDownloadContentFormat = .unknown,
        version: String? = nil,
        etag: String? = nil,
        checksum: String? = nil,
        totalSizeBytes: UInt64? = nil,
        resources: [VesperDownloadResourceRecord] = [],
        segments: [VesperDownloadSegmentRecord] = [],
        streams: [VesperDownloadAssetStream] = [],
        completedPath: String? = nil
    ) {
        self.contentFormat = contentFormat
        self.version = version
        self.etag = etag
        self.checksum = checksum
        self.totalSizeBytes = totalSizeBytes
        self.resources = resources
        self.segments = segments
        self.streams = streams
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
    public let code: VesperPlayerErrorCode
    public let category: VesperPlayerErrorCategory
    public let retriable: Bool
    public let message: String

    public init(
        code: VesperPlayerErrorCode,
        category: VesperPlayerErrorCategory,
        retriable: Bool,
        message: String
    ) {
        self.code = code
        self.category = category
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

public struct VesperDownloadTaskStatePatch: Equatable {
    public let taskId: VesperDownloadTaskId
    public let state: VesperDownloadState
    public let progress: VesperDownloadProgressSnapshot
    public let error: VesperDownloadError?
    public let completedPath: String?

    public init(
        taskId: VesperDownloadTaskId,
        state: VesperDownloadState,
        progress: VesperDownloadProgressSnapshot,
        error: VesperDownloadError? = nil,
        completedPath: String? = nil
    ) {
        self.taskId = taskId
        self.state = state
        self.progress = progress
        self.error = error
        self.completedPath = completedPath
    }
}

public struct VesperDownloadTaskProgressPatch: Equatable {
    public let taskId: VesperDownloadTaskId
    public let progress: VesperDownloadProgressSnapshot

    public init(taskId: VesperDownloadTaskId, progress: VesperDownloadProgressSnapshot) {
        self.taskId = taskId
        self.progress = progress
    }
}

public enum VesperDownloadEvent: Equatable {
    case created(VesperDownloadTaskSnapshot)
    case stateChanged(VesperDownloadTaskStatePatch)
    case assetIndexUpdated(VesperDownloadTaskSnapshot)
    case progressUpdated(VesperDownloadTaskProgressPatch)
}

private extension VesperDownloadEvent {
    var isRemovedStatePatch: Bool {
        if case let .stateChanged(patch) = self {
            return patch.state == .removed
        }
        return false
    }
}

@MainActor
public protocol VesperDownloadExecutionReporter: AnyObject {
    func completePreparation(
        taskId: VesperDownloadTaskId,
        assetIndex: VesperDownloadAssetIndex
    )

    func replaceTaskPlan(
        taskId: VesperDownloadTaskId,
        source: VesperDownloadSource,
        profile: VesperDownloadProfile,
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

public extension VesperDownloadExecutionReporter {
    func replaceTaskPlan(
        taskId: VesperDownloadTaskId,
        source: VesperDownloadSource,
        profile: VesperDownloadProfile,
        assetIndex: VesperDownloadAssetIndex
    ) {}
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
    private let taskStore = DownloadTaskStore()
    private var eventBuffer: [VesperDownloadEvent] = []
    private var lastProgressPersistence: [VesperDownloadTaskId: (bytes: UInt64, date: Date)] = [:]
    private var sessionHandle: UInt64 = 0

    public init(
        configuration: VesperDownloadConfiguration = VesperDownloadConfiguration(),
        executor: (any VesperDownloadExecutor)? = nil,
        staleResourceRecoveryHandler: VesperDownloadStaleResourceRecoveryHandler? = nil,
        staleResourcePlanRecoveryHandler: VesperDownloadStaleResourcePlanRecoveryHandler? = nil
    ) {
        self.configuration = configuration
        self.executor = executor ?? VesperForegroundDownloadExecutor(
            baseDirectory: configuration.baseDirectory,
            resumePartialDownloads: configuration.resumePartialDownloads,
            rangeChunkBytes: configuration.rangeChunkBytes,
            minProgressBytes: configuration.minProgressBytes,
            minProgressIntervalMs: configuration.minProgressIntervalMs,
            stalledTransferTimeoutMs: configuration.stalledTransferTimeoutMs,
            staleResourceRecoveryHandler: staleResourceRecoveryHandler,
            staleResourcePlanRecoveryHandler: staleResourcePlanRecoveryHandler
        )
        bindings = NativeDownloadBindings()
        let stateStoreURL = Self.stateStoreURL(for: configuration)
        stateStore = configuration.restoreTasksOnStartup
            ? VesperDownloadStateStore(fileURL: stateStoreURL)
            : nil
        snapshot = VesperDownloadSnapshot(tasks: [])
        excludeDownloadItemFromBackup(stateStoreURL.deletingLastPathComponent())
        sessionHandle = bindings.createDownloadSession(configuration: configuration)
        if sessionHandle == 0 {
            iosHostLog("native download session creation failed")
        }
        restorePersistedTasks()
        forceFullSync()
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
        if sessionHandle == 0 {
            iosHostLog("native download session creation failed")
        }
        forceFullSync()
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
        taskStore.replaceAll(VesperDownloadSnapshot(tasks: []))
        lastProgressPersistence.removeAll(keepingCapacity: false)
        snapshot = VesperDownloadSnapshot(tasks: [])
    }

    public func refresh() {
        syncRuntimeState(processCommands: true)
    }

    public func forceFullSync() {
        forceFullSync(processCommands: true)
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
        guard sessionHandle != 0 else {
            return nil
        }
        let normalizedAssetIndex: VesperDownloadAssetIndex
        do {
            normalizedAssetIndex = try VesperGeneratedDownloadResourceMaterializer(
                baseDirectory: configuration.baseDirectory
            ).materialize(
                assetId: assetId,
                taskId: nil,
                profile: profile,
                assetIndex: assetIndex
            )
        } catch {
            iosHostLog("download generated resource materialization failed: \(error.localizedDescription)")
            return nil
        }

        var runtimeSource = source.toRuntimeBridgePayload()
        var runtimeProfile = profile.toRuntimeBridgePayload()
        var runtimeAssetIndex = normalizedAssetIndex.toRuntimeBridgePayload()
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

        let materializer = VesperGeneratedDownloadResourceMaterializer(baseDirectory: configuration.baseDirectory)
        let normalizedTasks: [VesperDownloadTaskSnapshot]
        do {
            normalizedTasks = try tasks.map { task in
                try task.withAssetIndex(
                    materializer.materialize(
                        assetId: task.assetId,
                        taskId: task.taskId,
                        profile: task.profile,
                        assetIndex: task.assetIndex
                    )
                )
            }
        } catch {
            iosHostLog("download state restore failed while materializing generated resources: \(error.localizedDescription)")
            return false
        }

        let pointer = UnsafeMutablePointer<VesperRuntimeDownloadTask>.allocate(capacity: normalizedTasks.count)
        for (index, task) in normalizedTasks.enumerated() {
            pointer[index] = task.toRuntimeBridgePayload()
        }
        let restored = bindings.restoreDownloadTasks(
            sessionHandle: sessionHandle,
            tasks: UnsafePointer(pointer),
            taskCount: normalizedTasks.count
        )
        for index in 0..<normalizedTasks.count {
            freeRuntimeDownloadTask(&pointer[index])
        }
        pointer.deallocate()

        if restored {
            forceFullSync(processCommands: true)
        }
        return restored
    }

    public func startTask(_ taskId: VesperDownloadTaskId) -> Bool {
        guard sessionHandle != 0 else {
            return false
        }
        let started = bindings.startDownloadTask(sessionHandle: sessionHandle, taskId: taskId)
        if started {
            syncRuntimeState(processCommands: true)
        }
        return started
    }

    public func pauseTask(_ taskId: VesperDownloadTaskId) -> Bool {
        guard sessionHandle != 0 else {
            return false
        }
        let paused = bindings.pauseDownloadTask(sessionHandle: sessionHandle, taskId: taskId)
        if paused {
            syncRuntimeState(processCommands: true)
        }
        return paused
    }

    public func resumeTask(_ taskId: VesperDownloadTaskId) -> Bool {
        guard sessionHandle != 0 else {
            return false
        }
        let resumed = bindings.resumeDownloadTask(sessionHandle: sessionHandle, taskId: taskId)
        if resumed {
            syncRuntimeState(processCommands: true)
        }
        return resumed
    }

    public func removeTask(_ taskId: VesperDownloadTaskId) -> Bool {
        guard sessionHandle != 0 else {
            return false
        }
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

    public func outputURL(forTask taskId: VesperDownloadTaskId) throws -> URL {
        guard let task = task(taskId) else {
            throw DownloadExportBridgeError("download task \(taskId) was not found")
        }
        guard task.state == .completed else {
            throw DownloadExportBridgeError("download task \(taskId) must be completed before sharing or saving")
        }
        guard let completedPath = task.assetIndex.completedPath, !completedPath.isEmpty else {
            throw DownloadExportBridgeError("download task \(taskId) does not have an output file")
        }
        let url = downloadOutputURL(from: completedPath)
        guard FileManager.default.fileExists(atPath: url.path) else {
            throw DownloadExportBridgeError("download task output file does not exist")
        }
        return url
    }

    #if canImport(UIKit)
    public func shareTaskOutput(
        taskId: VesperDownloadTaskId,
        fileName: String? = nil,
        mimeType: String? = nil,
        from presenter: UIViewController
    ) throws {
        _ = mimeType
        let url = try preparedDownloadOutputURL(taskId: taskId, fileName: fileName)
        let controller = UIActivityViewController(activityItems: [url], applicationActivities: nil)
        if let popover = controller.popoverPresentationController {
            popover.sourceView = presenter.view
            popover.sourceRect = CGRect(
                x: presenter.view.bounds.midX,
                y: presenter.view.bounds.midY,
                width: 1,
                height: 1
            )
            popover.permittedArrowDirections = []
        }
        presenter.present(controller, animated: true)
    }

    @discardableResult
    public func saveTaskOutput(
        taskId: VesperDownloadTaskId,
        fileName: String? = nil,
        from presenter: UIViewController
    ) throws -> URL {
        let url = try preparedDownloadOutputURL(taskId: taskId, fileName: fileName)
        let picker = UIDocumentPickerViewController(forExporting: [url], asCopy: true)
        presenter.present(picker, animated: true)
        return url
    }
    #endif

    private func syncRuntimeState(processCommands: Bool) {
        guard sessionHandle != 0 else {
            taskStore.replaceAll(VesperDownloadSnapshot(tasks: []))
            snapshot = VesperDownloadSnapshot(tasks: [])
            eventBuffer.removeAll(keepingCapacity: false)
            lastProgressPersistence.removeAll(keepingCapacity: false)
            return
        }

        var runtimeEvents = VesperRuntimeDownloadEventList(events: nil, len: 0)
        var events: [VesperDownloadEvent] = []
        if bindings.drainDownloadEvents(sessionHandle: sessionHandle, outEvents: &runtimeEvents) {
            events = runtimeEvents.toPublic()
            eventBuffer.append(contentsOf: events)
            bindings.freeDownloadEventList(&runtimeEvents)
        }

        let immediateEvents = events.filter { !$0.isRemovedStatePatch }
        if !immediateEvents.isEmpty {
            let updatedSnapshot = taskStore.apply(immediateEvents)
            if updatedSnapshot != snapshot {
                snapshot = updatedSnapshot
            }
        }

        if processCommands {
            var runtimeCommands = VesperRuntimeDownloadCommandList(commands: nil, len: 0)
            if bindings.drainDownloadCommands(sessionHandle: sessionHandle, outCommands: &runtimeCommands) {
                let commands = runtimeCommands.toPublic()
                bindings.freeDownloadCommandList(&runtimeCommands)
                commands.forEach(applyCommand(_:))
            }
        }

        if !events.isEmpty {
            let removalEvents = events.filter(\.isRemovedStatePatch)
            if !removalEvents.isEmpty {
                let updatedSnapshot = taskStore.apply(removalEvents)
                if updatedSnapshot != snapshot {
                    snapshot = updatedSnapshot
                }
            }
            if shouldPersistSnapshot(after: events) {
                persistSnapshot(snapshot)
            }
        }
    }

    private func forceFullSync(processCommands: Bool) {
        guard sessionHandle != 0 else {
            taskStore.replaceAll(VesperDownloadSnapshot(tasks: []))
            snapshot = VesperDownloadSnapshot(tasks: [])
            eventBuffer.removeAll(keepingCapacity: false)
            lastProgressPersistence.removeAll(keepingCapacity: false)
            return
        }

        var runtimeSnapshot = VesperRuntimeDownloadSnapshot(tasks: nil, len: 0)
        if bindings.downloadSessionSnapshot(sessionHandle: sessionHandle, outSnapshot: &runtimeSnapshot) {
            let fullSnapshot = runtimeSnapshot.toPublic()
            taskStore.replaceAll(fullSnapshot)
            let activeSnapshot = taskStore.snapshot()
            snapshot = activeSnapshot
            persistSnapshot(activeSnapshot)
            bindings.freeDownloadSnapshot(&runtimeSnapshot)
        } else {
            taskStore.replaceAll(VesperDownloadSnapshot(tasks: []))
            snapshot = VesperDownloadSnapshot(tasks: [])
        }

        syncRuntimeState(processCommands: processCommands)
    }

    private func shouldPersistSnapshot(after events: [VesperDownloadEvent]) -> Bool {
        var shouldPersist = false
        for event in events {
            switch event {
            case .created, .assetIndexUpdated:
                shouldPersist = true
            case let .stateChanged(patch):
                shouldPersist = true
                lastProgressPersistence[patch.taskId] = (patch.progress.receivedBytes, Date())
            case let .progressUpdated(patch):
                if shouldPersistProgressCheckpoint(patch) {
                    shouldPersist = true
                }
            }
        }
        return shouldPersist
    }

    private func shouldPersistProgressCheckpoint(_ patch: VesperDownloadTaskProgressPatch) -> Bool {
        let now = Date()
        guard let previous = lastProgressPersistence[patch.taskId] else {
            lastProgressPersistence[patch.taskId] = (patch.progress.receivedBytes, now)
            return true
        }
        let byteDelta = patch.progress.receivedBytes >= previous.bytes
            ? patch.progress.receivedBytes - previous.bytes
            : 0
        let elapsedMs = UInt64(max(0, now.timeIntervalSince(previous.date) * 1000))
        guard byteDelta >= configuration.minProgressBytes,
              elapsedMs >= configuration.minProgressIntervalMs
        else {
            return false
        }
        lastProgressPersistence[patch.taskId] = (patch.progress.receivedBytes, now)
        return true
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
        stateStore?.save(snapshot.compactedForPersistence())
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

        func replaceTaskPlan(
            taskId: VesperDownloadTaskId,
            source: VesperDownloadSource,
            profile: VesperDownloadProfile,
            assetIndex: VesperDownloadAssetIndex
        ) {
            guard let manager, manager.sessionHandle != 0 else {
                return
            }
            var runtimeSource = source.toRuntimeBridgePayload()
            var runtimeProfile = profile.toRuntimeBridgePayload()
            var runtimeAssetIndex = assetIndex.toRuntimeBridgePayload()
            _ = withUnsafePointer(to: &runtimeSource) { sourcePointer in
                withUnsafePointer(to: &runtimeProfile) { profilePointer in
                    withUnsafePointer(to: &runtimeAssetIndex) { assetIndexPointer in
                        manager.bindings.replaceDownloadTaskPlan(
                            sessionHandle: manager.sessionHandle,
                            taskId: taskId,
                            source: sourcePointer,
                            profile: profilePointer,
                            assetIndex: assetIndexPointer
                        )
                    }
                }
            }
            freeRuntimeDownloadSource(&runtimeSource)
            freeRuntimeDownloadProfile(&runtimeProfile)
            freeRuntimeDownloadAssetIndex(&runtimeAssetIndex)
            manager.syncRuntimeState(processCommands: false)
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

}
