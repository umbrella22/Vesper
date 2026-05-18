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

private let vesperDownloadATSFailureMessage =
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
        precondition(sessionHandle != 0, "native download session handle must not be zero")
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

    private func preparedDownloadOutputURL(
        taskId: VesperDownloadTaskId,
        fileName: String?
    ) throws -> URL {
        let sourceURL = try outputURL(forTask: taskId)
        guard let fileName, !fileName.isEmpty else {
            return sourceURL
        }
        let directory = FileManager.default.temporaryDirectory
            .appendingPathComponent("vesper-download-share", isDirectory: true)
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        let targetURL = directory.appendingPathComponent(sanitizedOutputFileName(fileName))
        if FileManager.default.fileExists(atPath: targetURL.path) {
            try FileManager.default.removeItem(at: targetURL)
        }
        try FileManager.default.copyItem(at: sourceURL, to: targetURL)
        return targetURL
    }

    private func downloadOutputURL(from path: String) -> URL {
        if let url = URL(string: path), url.isFileURL {
            return url
        }
        return URL(fileURLWithPath: path)
    }

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

        func replaceDownloadTaskPlan(
            sessionHandle: UInt64,
            taskId: UInt64,
            source: UnsafePointer<VesperRuntimeDownloadSource>,
            profile: UnsafePointer<VesperRuntimeDownloadProfile>,
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

private final class DownloadTaskStore {
    private var tasksById: [VesperDownloadTaskId: VesperDownloadTaskSnapshot] = [:]
    private var order: [VesperDownloadTaskId] = []

    func replaceAll(_ snapshot: VesperDownloadSnapshot) {
        let activeTasks = snapshot.tasks.filter { $0.state != .removed }
        tasksById = Dictionary(uniqueKeysWithValues: activeTasks.map { ($0.taskId, $0) })
        order = activeTasks.map(\.taskId)
    }

    @discardableResult
    func apply(_ events: [VesperDownloadEvent]) -> VesperDownloadSnapshot {
        for event in events {
            switch event {
            case let .created(task), let .assetIndexUpdated(task):
                upsert(task)
            case let .stateChanged(patch):
                if patch.state == .removed {
                    remove(patch.taskId)
                    continue
                }
                guard let task = tasksById[patch.taskId] else {
                    continue
                }
                tasksById[patch.taskId] = task.withStatePatch(patch)
            case let .progressUpdated(patch):
                guard let task = tasksById[patch.taskId] else {
                    continue
                }
                tasksById[patch.taskId] = task.withProgress(patch.progress)
            }
        }
        return snapshot()
    }

    func snapshot() -> VesperDownloadSnapshot {
        VesperDownloadSnapshot(tasks: order.compactMap { tasksById[$0] })
    }

    private func upsert(_ task: VesperDownloadTaskSnapshot) {
        if tasksById[task.taskId] == nil {
            order.append(task.taskId)
        }
        tasksById[task.taskId] = task
    }

    private func remove(_ taskId: VesperDownloadTaskId) {
        tasksById.removeValue(forKey: taskId)
        order.removeAll { $0 == taskId }
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
            excludeDownloadItemFromBackup(fileURL.deletingLastPathComponent())
            let data = try encoder.encode(VesperDownloadSnapshot(tasks: tasks))
            try data.write(to: fileURL, options: .atomic)
            excludeDownloadItemFromBackup(fileURL)
        } catch {
            iosHostLog("download state persistence failed: \(error.localizedDescription)")
        }
    }
}

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
        defer { try? output.close() }
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
        defer { try? output.close() }
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
        defer { try? output.close() }
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

private func sanitizedDownloadHttpHeaders(_ headers: [String: String]) -> [String: String] {
    var result: [String: String] = [:]
    for (name, value) in headers {
        let sanitizedName = name.trimmingCharacters(in: .whitespacesAndNewlines)
        if !sanitizedName.isEmpty,
           !value.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
            result[sanitizedName] = value
        }
    }
    return result
}

private func sanitizedOutputFileName(_ value: String) -> String {
    let sanitized = value
        .replacingOccurrences(of: "[^A-Za-z0-9._ -]+", with: "_", options: .regularExpression)
        .trimmingCharacters(in: CharacterSet(charactersIn: ". "))
    return sanitized.isEmpty || sanitized == ".." ? "vesper-download" : sanitized
}

private func rejectInsecureHTTPURL(_ url: URL) throws {
    guard url.scheme?.lowercased() == "http" else {
        return
    }
    throw VesperForegroundDownloadPreparationError.invalidSource(
        "\(vesperDownloadATSFailureMessage) URL: \(url.absoluteString)"
    )
}

func excludeDownloadItemFromBackup(_ url: URL, fileManager: FileManager = .default) {
    guard fileManager.fileExists(atPath: url.path) else {
        return
    }
    var excludedURL = url
    var values = URLResourceValues()
    values.isExcludedFromBackup = true
    do {
        try excludedURL.setResourceValues(values)
    } catch {
        iosHostLog("failed to exclude download item from iCloud backup: \(error.localizedDescription)")
    }
}

private extension URLRequest {
    mutating func applyDownloadHttpHeaders(_ headers: [String: String]) {
        for (name, value) in sanitizedDownloadHttpHeaders(headers) {
            setValue(value, forHTTPHeaderField: name)
        }
        if value(forHTTPHeaderField: "Accept-Encoding") == nil {
            setValue("identity", forHTTPHeaderField: "Accept-Encoding")
        }
    }
}

private struct VesperHTTPBodyStream {
    let response: URLResponse
    let chunks: AsyncThrowingStream<Data, Error>
    let cancel: @Sendable () -> Void
}

private final class VesperURLSessionDataStreamDelegate: NSObject, URLSessionDataDelegate, @unchecked Sendable {
    private let sourceDescription: String
    private let stalledTransferTimeoutNs: UInt64
    private let lock = NSLock()
    private let watchdogQueue: DispatchQueue
    private let chunksContinuation: AsyncThrowingStream<Data, Error>.Continuation
    private var responseContinuation: CheckedContinuation<URLResponse, Error>?
    private var responseResult: Result<URLResponse, Error>?
    private var session: URLSession?
    private var task: URLSessionDataTask?
    private var watchdog: DispatchSourceTimer?
    private var lastActivityNs: UInt64
    private var didFinish = false

    let chunks: AsyncThrowingStream<Data, Error>

    init(stalledTransferTimeoutMs: UInt64, sourceDescription: String) {
        self.sourceDescription = sourceDescription
        let (timeoutNs, overflow) = stalledTransferTimeoutMs.multipliedReportingOverflow(by: 1_000_000)
        stalledTransferTimeoutNs = overflow ? UInt64.max : timeoutNs
        watchdogQueue = DispatchQueue(
            label: "io.github.ikaros.vesper.player.download.http-watchdog.\(UUID().uuidString)"
        )
        lastActivityNs = DispatchTime.now().uptimeNanoseconds

        var continuation: AsyncThrowingStream<Data, Error>.Continuation!
        chunks = AsyncThrowingStream(Data.self, bufferingPolicy: .unbounded) { streamContinuation in
            continuation = streamContinuation
        }
        chunksContinuation = continuation
        super.init()
        chunksContinuation.onTermination = { @Sendable [weak self] _ in
            self?.cancel()
        }
    }

    func bind(session: URLSession, task: URLSessionDataTask) {
        lock.lock()
        self.session = session
        self.task = task
        lastActivityNs = DispatchTime.now().uptimeNanoseconds
        lock.unlock()
        startWatchdogIfNeeded()
    }

    func waitForResponse() async throws -> URLResponse {
        if let result = lockedResponseResult() {
            return try result.get()
        }
        return try await withCheckedThrowingContinuation { continuation in
            lock.lock()
            if let responseResult {
                lock.unlock()
                continuation.resume(with: responseResult)
            } else {
                responseContinuation = continuation
                lock.unlock()
            }
        }
    }

    func cancel() {
        var localTask: URLSessionDataTask?
        var localSession: URLSession?
        lock.lock()
        localTask = task
        localSession = session
        lock.unlock()
        localTask?.cancel()
        localSession?.invalidateAndCancel()
    }

    func urlSession(
        _ session: URLSession,
        dataTask: URLSessionDataTask,
        didReceive response: URLResponse,
        completionHandler: @escaping (URLSession.ResponseDisposition) -> Void
    ) {
        markActivity()
        completeResponse(.success(response))
        completionHandler(.allow)
    }

    func urlSession(_ session: URLSession, dataTask: URLSessionDataTask, didReceive data: Data) {
        guard !data.isEmpty else { return }
        markActivity()
        chunksContinuation.yield(data)
    }

    func urlSession(_ session: URLSession, task: URLSessionTask, didCompleteWithError error: Error?) {
        if let error {
            finish(throwing: error)
        } else {
            finish()
        }
    }

    private func lockedResponseResult() -> Result<URLResponse, Error>? {
        lock.lock()
        defer { lock.unlock() }
        return responseResult
    }

    private func completeResponse(_ result: Result<URLResponse, Error>) {
        var continuation: CheckedContinuation<URLResponse, Error>?
        lock.lock()
        if responseResult == nil {
            responseResult = result
            continuation = responseContinuation
            responseContinuation = nil
        }
        lock.unlock()
        continuation?.resume(with: result)
    }

    private func markActivity() {
        lock.lock()
        lastActivityNs = DispatchTime.now().uptimeNanoseconds
        lock.unlock()
    }

    private func startWatchdogIfNeeded() {
        guard stalledTransferTimeoutNs > 0 else { return }
        let timer = DispatchSource.makeTimerSource(queue: watchdogQueue)
        let interval = DispatchTimeInterval.nanoseconds(
            Int(min(stalledTransferTimeoutNs, UInt64(Int.max)))
        )
        timer.schedule(deadline: .now() + interval, repeating: interval)
        timer.setEventHandler { [weak self] in
            self?.failIfStalled()
        }
        lock.lock()
        watchdog = timer
        lock.unlock()
        timer.resume()
    }

    private func failIfStalled() {
        let shouldFail: Bool
        lock.lock()
        let elapsedNs = DispatchTime.now().uptimeNanoseconds - lastActivityNs
        shouldFail = !didFinish && stalledTransferTimeoutNs > 0 && elapsedNs >= stalledTransferTimeoutNs
        lock.unlock()
        guard shouldFail else { return }
        let error = VesperForegroundDownloadPreparationError.invalidSource(
            "network transfer stalled without progress for \(sourceDescription)"
        )
        finish(throwing: error)
        task?.cancel()
        session?.invalidateAndCancel()
    }

    private func finish(throwing error: Error? = nil) {
        var shouldFinishStream = false
        var localWatchdog: DispatchSourceTimer?
        var localSession: URLSession?
        lock.lock()
        if !didFinish {
            didFinish = true
            shouldFinishStream = true
            localWatchdog = watchdog
            watchdog = nil
            localSession = session
        }
        lock.unlock()

        if let error {
            completeResponse(.failure(error))
        } else {
            completeResponse(.failure(VesperForegroundDownloadPreparationError.invalidSource(
                "remote resource did not return a response for \(sourceDescription)"
            )))
        }
        localWatchdog?.cancel()
        if shouldFinishStream {
            if let error {
                chunksContinuation.finish(throwing: error)
            } else {
                chunksContinuation.finish()
            }
            localSession?.finishTasksAndInvalidate()
        }
    }
}

private func isExpiredHttpStatus(_ statusCode: Int) -> Bool {
    statusCode == 401 || statusCode == 403 || statusCode == 404 || statusCode == 410
}

struct VesperHTTPContentRange: Equatable {
    let start: UInt64?
    let end: UInt64?
    let total: UInt64?

    var isUnsatisfied: Bool {
        start == nil && end == nil
    }

    var length: UInt64? {
        guard let start, let end, end >= start else {
            return nil
        }
        return end - start + 1
    }
}

func parseHttpContentRange(_ contentRange: String?) -> VesperHTTPContentRange? {
    guard let contentRange else {
        return nil
    }
    let fields = contentRange.trimmingCharacters(in: .whitespacesAndNewlines)
        .split(separator: " ", maxSplits: 1)
    guard fields.count == 2,
          fields[0].lowercased() == "bytes"
    else {
        return nil
    }
    let value = fields[1]
    let rangeAndTotal = value.split(separator: "/", maxSplits: 1, omittingEmptySubsequences: false)
    guard rangeAndTotal.count == 2 else { return nil }
    let totalText = rangeAndTotal[1].trimmingCharacters(in: .whitespaces)
    let total = totalText == "*" ? nil : UInt64(totalText)
    if value.hasPrefix("*") {
        guard value.hasPrefix("*/") || rangeAndTotal[0] == "*" else { return nil }
        return VesperHTTPContentRange(start: nil, end: nil, total: total)
    }

    let rangeParts = rangeAndTotal[0].split(separator: "-", maxSplits: 1, omittingEmptySubsequences: false)
    guard rangeParts.count == 2,
          let start = UInt64(rangeParts[0].trimmingCharacters(in: .whitespaces)),
          let end = UInt64(rangeParts[1].trimmingCharacters(in: .whitespaces)),
          end >= start
    else {
        return nil
    }
    return VesperHTTPContentRange(start: start, end: end, total: total)
}

func parseHttpContentLength(_ contentLength: String?) -> UInt64? {
    guard let contentLength else { return nil }
    return UInt64(contentLength.trimmingCharacters(in: .whitespacesAndNewlines))
}

@discardableResult
func validateHTTPPartialContentRange(
    contentRangeHeader: String?,
    contentLengthHeader: String?,
    requestedStart: UInt64,
    requestedEndInclusive: UInt64?,
    expectedBodyLength: UInt64?,
    expectedTotalSizeBytes: UInt64?,
    sourceDescription: String
) throws -> VesperHTTPContentRange {
    guard let contentRange = parseHttpContentRange(contentRangeHeader),
          !contentRange.isUnsatisfied,
          contentRange.start == requestedStart,
          let responseEnd = contentRange.end
    else {
        throw VesperForegroundDownloadPreparationError.invalidSource(
            "remote server returned an unexpected Content-Range for \(sourceDescription)"
        )
    }
    if let requestedEndInclusive, responseEnd != requestedEndInclusive {
        throw VesperForegroundDownloadPreparationError.invalidSource(
            "remote server returned a Content-Range outside the requested byte range for \(sourceDescription)"
        )
    }
    if let expectedTotalSizeBytes,
       let total = contentRange.total,
       total != expectedTotalSizeBytes {
        throw VesperForegroundDownloadPreparationError.invalidSource(
            "remote server reported Content-Range total \(total), expected \(expectedTotalSizeBytes) for \(sourceDescription)"
        )
    }
    if let length = contentRange.length {
        if let expectedBodyLength, length != expectedBodyLength {
            throw VesperForegroundDownloadPreparationError.invalidSource(
                "remote server returned \(length) range bytes, expected \(expectedBodyLength) for \(sourceDescription)"
            )
        }
        if let contentLength = parseHttpContentLength(contentLengthHeader),
           contentLength != length {
            throw VesperForegroundDownloadPreparationError.invalidSource(
                "remote server reported Content-Length \(contentLength), expected \(length) from Content-Range for \(sourceDescription)"
            )
        }
    }
    return contentRange
}

private struct ForegroundDownloadEntry {
    let url: URL
    let resourceId: String?
    let segmentId: String?
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

private struct VesperStaleDownloadResourceError: LocalizedError {
    let message: String
    let resourceId: String?
    let segmentId: String?
    let uri: String?
    let phase: VesperDownloadStaleResourcePhase?
    let statusCode: Int?
    let receivedBytes: UInt64

    var errorDescription: String? {
        message
    }

    func staleResource(
        taskId: VesperDownloadTaskId,
        fallbackResourceId: String? = nil,
        fallbackSegmentId: String? = nil,
        fallbackUri: String? = nil,
        phase fallbackPhase: VesperDownloadStaleResourcePhase,
        receivedBytes fallbackReceivedBytes: UInt64 = 0
    ) -> VesperDownloadStaleResource {
        VesperDownloadStaleResource(
            taskId: taskId,
            resourceId: resourceId ?? fallbackResourceId,
            segmentId: segmentId ?? fallbackSegmentId,
            uri: uri ?? fallbackUri,
            phase: phase ?? fallbackPhase,
            statusCode: statusCode,
            receivedBytes: receivedBytes > 0 ? receivedBytes : fallbackReceivedBytes,
            message: message
        )
    }
}

private func staleDownloadResource(
    _ message: String,
    resourceId: String? = nil,
    segmentId: String? = nil,
    uri: String? = nil,
    phase: VesperDownloadStaleResourcePhase? = nil,
    statusCode: Int? = nil,
    receivedBytes: UInt64 = 0
) -> VesperStaleDownloadResourceError {
    VesperStaleDownloadResourceError(
        message: message,
        resourceId: resourceId,
        segmentId: segmentId,
        uri: uri,
        phase: phase,
        statusCode: statusCode,
        receivedBytes: receivedBytes
    )
}

private struct DownloadProgressThrottle {
    private let minProgressBytes: UInt64
    private let minProgressIntervalNs: UInt64
    private var lastReportedBytes: UInt64 = 0
    private var lastReportedTimeNs: UInt64 = 0

    init(minProgressBytes: UInt64, minProgressIntervalMs: UInt64) {
        self.minProgressBytes = max(minProgressBytes, 1)
        self.minProgressIntervalNs = minProgressIntervalMs * 1_000_000
    }

    mutating func shouldReport(receivedBytes: UInt64, force: Bool) -> Bool {
        if force || receivedBytes < lastReportedBytes {
            markReported(receivedBytes: receivedBytes)
            return true
        }
        if receivedBytes - lastReportedBytes < minProgressBytes {
            return false
        }
        let now = DispatchTime.now().uptimeNanoseconds
        if lastReportedTimeNs != 0, now - lastReportedTimeNs < minProgressIntervalNs {
            return false
        }
        markReported(receivedBytes: receivedBytes, now: now)
        return true
    }

    mutating func markReported(receivedBytes: UInt64) {
        markReported(receivedBytes: receivedBytes, now: DispatchTime.now().uptimeNanoseconds)
    }

    private mutating func markReported(receivedBytes: UInt64, now: UInt64) {
        lastReportedBytes = receivedBytes
        lastReportedTimeNs = now
    }
}

private struct VesperGeneratedDownloadResourceMaterializer {
    let fileManager: FileManager
    let baseDirectory: URL?

    init(fileManager: FileManager = .default, baseDirectory: URL?) {
        self.fileManager = fileManager
        self.baseDirectory = baseDirectory
    }

    func materialize(
        assetId: VesperDownloadAssetId,
        taskId: VesperDownloadTaskId?,
        profile: VesperDownloadProfile,
        assetIndex: VesperDownloadAssetIndex
    ) throws -> VesperDownloadAssetIndex {
        guard assetIndex.resources.contains(where: { $0.generatedText != nil }) else {
            return assetIndex.compactedForPersistence()
        }

        let taskDirectory = taskBaseDirectory(assetId: assetId, taskId: taskId, profile: profile)
        let generatedDirectory = taskDirectory.appendingPathComponent(".generated", isDirectory: true)
        try fileManager.createDirectory(at: generatedDirectory, withIntermediateDirectories: true)
        excludeDownloadItemFromBackup(taskDirectory, fileManager: fileManager)
        excludeDownloadItemFromBackup(generatedDirectory, fileManager: fileManager)

        var usedNames = Set<String>()
        let resources = try assetIndex.resources.map { resource in
            guard let generatedText = resource.generatedText else {
                return resource
            }
            let data = Data(generatedText.utf8)
            let fileName = uniqueGeneratedFileName(for: resource, usedNames: &usedNames)
            let destinationURL = generatedDirectory.appendingPathComponent(fileName, isDirectory: false)
            do {
                try data.write(to: destinationURL, options: .atomic)
                excludeDownloadItemFromBackup(destinationURL, fileManager: fileManager)
            } catch {
                throw VesperForegroundDownloadPreparationError.invalidSource(
                    "failed to persist generated download resource \(resource.resourceId): \(error.localizedDescription)"
                )
            }
            return resource.withMaterializedGeneratedText(
                uri: destinationURL.absoluteString,
                sizeBytes: UInt64(data.count)
            )
        }

        return VesperDownloadAssetIndex(
            contentFormat: assetIndex.contentFormat,
            version: assetIndex.version,
            etag: assetIndex.etag,
            checksum: assetIndex.checksum,
            totalSizeBytes: recomputedTotalSizeBytes(
                original: assetIndex.totalSizeBytes,
                resources: resources,
                segments: assetIndex.segments
            ),
            resources: resources,
            segments: assetIndex.segments,
            completedPath: assetIndex.completedPath
        )
    }

    private func taskBaseDirectory(
        assetId: VesperDownloadAssetId,
        taskId: VesperDownloadTaskId?,
        profile: VesperDownloadProfile
    ) -> URL {
        if let targetDirectory = profile.targetDirectory {
            return targetDirectory
        }
        let root = baseDirectory
            ?? fileManager.urls(for: .documentDirectory, in: .userDomainMask).first!
                .appendingPathComponent("vesper-downloads", isDirectory: true)
        let assetComponent = assetId.isEmpty ? taskId.map(String.init) ?? "asset" : assetId
        return root.appendingPathComponent(assetComponent, isDirectory: true)
    }

    private func uniqueGeneratedFileName(
        for resource: VesperDownloadResourceRecord,
        usedNames: inout Set<String>
    ) -> String {
        let baseName = generatedBaseName(for: resource)
        if usedNames.insert(baseName).inserted {
            return baseName
        }
        let hashed = appendStableHash(
            to: baseName,
            hash: stableShortHash("\(resource.resourceId)|\(resource.relativePath ?? "")|\(resource.uri)")
        )
        _ = usedNames.insert(hashed)
        return hashed
    }

    private func generatedBaseName(for resource: VesperDownloadResourceRecord) -> String {
        let raw = resource.relativePath.flatMap(lastPathComponent) ?? resource.resourceId
        let sanitized = raw
            .replacingOccurrences(of: "[^A-Za-z0-9._-]+", with: "_", options: .regularExpression)
            .trimmingCharacters(in: CharacterSet(charactersIn: ". "))
        if sanitized.isEmpty || sanitized == ".." {
            return "resource-\(stableShortHash(resource.resourceId.isEmpty ? resource.uri : resource.resourceId))"
        }
        return sanitized
    }

    private func lastPathComponent(_ value: String) -> String? {
        value.split(whereSeparator: { $0 == "/" || $0 == "\\" }).last.map(String.init)
    }

    private func appendStableHash(to fileName: String, hash: String) -> String {
        let nsName = fileName as NSString
        let ext = nsName.pathExtension
        let stem = nsName.deletingPathExtension
        return ext.isEmpty ? "\(stem)-\(hash)" : "\(stem)-\(hash).\(ext)"
    }

    private func stableShortHash(_ value: String) -> String {
        var hash: UInt64 = 0xcbf29ce484222325
        for byte in value.utf8 {
            hash ^= UInt64(byte)
            hash &*= 0x100000001b3
        }
        let text = String(hash, radix: 16)
        return String(text.suffix(8))
    }

    private func recomputedTotalSizeBytes(
        original: UInt64?,
        resources: [VesperDownloadResourceRecord],
        segments: [VesperDownloadSegmentRecord]
    ) -> UInt64? {
        var total: UInt64 = 0
        for resource in resources {
            guard let sizeBytes = resource.sizeBytes else {
                return original
            }
            total += sizeBytes
        }
        for segment in segments {
            guard let sizeBytes = segment.sizeBytes else {
                return original
            }
            total += sizeBytes
        }
        return total
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
    func compactedForPersistence() -> VesperDownloadResourceRecord {
        VesperDownloadResourceRecord(
            resourceId: resourceId,
            uri: uri,
            relativePath: relativePath,
            byteRange: byteRange,
            generatedText: nil,
            sizeBytes: sizeBytes,
            etag: etag,
            checksum: checksum
        )
    }

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

    func withMaterializedGeneratedText(uri: String, sizeBytes: UInt64) -> VesperDownloadResourceRecord {
        VesperDownloadResourceRecord(
            resourceId: resourceId,
            uri: uri,
            relativePath: relativePath,
            byteRange: byteRange,
            generatedText: nil,
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
    func compactedForPersistence() -> VesperDownloadAssetIndex {
        VesperDownloadAssetIndex(
            contentFormat: contentFormat,
            version: version,
            etag: etag,
            checksum: checksum,
            totalSizeBytes: totalSizeBytes,
            resources: resources.map { $0.compactedForPersistence() },
            segments: segments,
            completedPath: completedPath
        )
    }

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

    func withCompletedPath(_ completedPath: String?) -> VesperDownloadAssetIndex {
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

private extension VesperDownloadTaskSnapshot {
    func withAssetIndex(_ assetIndex: VesperDownloadAssetIndex) -> VesperDownloadTaskSnapshot {
        VesperDownloadTaskSnapshot(
            taskId: taskId,
            assetId: assetId,
            source: source,
            profile: profile,
            state: state,
            progress: progress,
            assetIndex: assetIndex,
            error: error
        )
    }

    func withStatePatch(_ patch: VesperDownloadTaskStatePatch) -> VesperDownloadTaskSnapshot {
        VesperDownloadTaskSnapshot(
            taskId: taskId,
            assetId: assetId,
            source: source,
            profile: profile,
            state: patch.state,
            progress: patch.progress,
            assetIndex: assetIndex.withCompletedPath(patch.completedPath ?? assetIndex.completedPath),
            error: patch.error
        )
    }

    func withProgress(_ progress: VesperDownloadProgressSnapshot) -> VesperDownloadTaskSnapshot {
        VesperDownloadTaskSnapshot(
            taskId: taskId,
            assetId: assetId,
            source: source,
            profile: profile,
            state: state,
            progress: progress,
            assetIndex: assetIndex,
            error: error
        )
    }
}

private extension VesperDownloadSnapshot {
    func compactedForPersistence() -> VesperDownloadSnapshot {
        VesperDownloadSnapshot(
            tasks: tasks.map { $0.withAssetIndex($0.assetIndex.compactedForPersistence()) }
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

    func replaceDownloadTaskPlan(
        sessionHandle: UInt64,
        taskId: UInt64,
        source: UnsafePointer<VesperRuntimeDownloadSource>,
        profile: UnsafePointer<VesperRuntimeDownloadProfile>,
        assetIndex: UnsafePointer<VesperRuntimeDownloadAssetIndex>
    ) -> Bool {
        vesper_runtime_download_session_replace_task_plan(
            sessionHandle,
            taskId,
            source,
            profile,
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
                error.code.ffiCode,
                error.category.ffiCategory,
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

private func duplicateDownloadCStringArray(_ values: [String]) -> UnsafeMutablePointer<UnsafeMutablePointer<CChar>?>? {
    guard !values.isEmpty else {
        return nil
    }
    let pointer = UnsafeMutablePointer<UnsafeMutablePointer<CChar>?>.allocate(capacity: values.count)
    for (index, value) in values.enumerated() {
        pointer[index] = duplicateDownloadCString(value)
    }
    return pointer
}

private func freeDownloadCStringArray(
    _ values: UnsafeMutablePointer<UnsafeMutablePointer<CChar>?>?,
    count: Int
) {
    guard let values, count > 0 else {
        return
    }
    for index in 0..<count {
        freeDownloadCString(values[index])
    }
    values.deallocate()
}

private func stringFromRuntimeCString(_ pointer: UnsafeMutablePointer<CChar>?) -> String? {
    guard let pointer else {
        return nil
    }
    return String(cString: pointer)
}

private func stringArrayFromRuntimeCStringArray(
    _ pointer: UnsafeMutablePointer<UnsafeMutablePointer<CChar>?>?,
    count: Int
) -> [String] {
    guard let pointer, count > 0 else {
        return []
    }
    return (0..<count).compactMap { index in
        stringFromRuntimeCString(pointer[index])
    }
}

private func stringDictionaryFromRuntimeCStringArrays(
    keys: UnsafeMutablePointer<UnsafeMutablePointer<CChar>?>?,
    values: UnsafeMutablePointer<UnsafeMutablePointer<CChar>?>?,
    count: Int
) -> [String: String] {
    guard let keys, let values, count > 0 else {
        return [:]
    }
    var result: [String: String] = [:]
    for index in 0..<count {
        guard let key = stringFromRuntimeCString(keys[index]),
              let value = stringFromRuntimeCString(values[index])
        else {
            continue
        }
        result[key] = value
    }
    return result
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
    if let headerNames = source.header_names, source.headers_len > 0 {
        for index in 0..<Int(source.headers_len) {
            freeDownloadCString(headerNames[index])
        }
        headerNames.deallocate()
    }
    if let headerValues = source.header_values, source.headers_len > 0 {
        for index in 0..<Int(source.headers_len) {
            freeDownloadCString(headerValues[index])
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
    if let streams = assetIndex.streams, assetIndex.streams_len > 0 {
        for index in 0..<Int(assetIndex.streams_len) {
            freeDownloadCString(streams[index].stream_id)
            freeDownloadCString(streams[index].language)
            freeDownloadCString(streams[index].codec)
            freeDownloadCString(streams[index].label)
            freeDownloadCStringArray(streams[index].resource_ids, count: Int(streams[index].resource_ids_len))
            freeDownloadCStringArray(streams[index].segment_ids, count: Int(streams[index].segment_ids_len))
            freeDownloadCStringArray(streams[index].metadata_keys, count: Int(streams[index].metadata_len))
            freeDownloadCStringArray(streams[index].metadata_values, count: Int(streams[index].metadata_len))
        }
        streams.deallocate()
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
        streams: nil,
        streams_len: 0,
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
            error_code: error?.code.ffiCode ?? PlayerFfiErrorCodeNone,
            error_category: error?.category.ffiCategory ?? PlayerFfiErrorCategoryPlatform,
            error_retriable: error?.retriable ?? false,
            error_message: error.flatMap { duplicateDownloadCString($0.message) }
        )
    }
}

private extension VesperDownloadSource {
    func toRuntimeBridgePayload() -> VesperRuntimeDownloadSource {
        let headers = sanitizedDownloadHttpHeaders(source.headers)
        let headerNames = Array(headers.keys)
        let headerValues = headerNames.map { headers[$0] ?? "" }
        return VesperRuntimeDownloadSource(
            source_uri: duplicateDownloadCString(source.uri),
            content_format: VesperRuntimeDownloadContentFormat(rawValue: contentFormat.rawValue)
                ?? VesperRuntimeDownloadContentFormatUnknown,
            manifest_uri: manifestUri.flatMap(duplicateDownloadCString),
            header_names: duplicateDownloadCStringArray(headerNames),
            header_values: duplicateDownloadCStringArray(headerValues),
            headers_len: UInt(headerNames.count)
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

private extension VesperDownloadAssetStream {
    func toRuntimeBridgePayload() -> VesperRuntimeDownloadAssetStream {
        let metadataPairs = metadata.sorted { lhs, rhs in lhs.key < rhs.key }
        return VesperRuntimeDownloadAssetStream(
            stream_id: duplicateDownloadCString(streamId),
            kind: kind.toRuntimeBridgePayload(),
            language: language.flatMap(duplicateDownloadCString),
            codec: codec.flatMap(duplicateDownloadCString),
            label: label.flatMap(duplicateDownloadCString),
            has_quality_rank: qualityRank != nil,
            quality_rank: qualityRank ?? 0,
            resource_ids: duplicateDownloadCStringArray(resourceIds),
            resource_ids_len: UInt(resourceIds.count),
            segment_ids: duplicateDownloadCStringArray(segmentIds),
            segment_ids_len: UInt(segmentIds.count),
            metadata_keys: duplicateDownloadCStringArray(metadataPairs.map(\.key)),
            metadata_values: duplicateDownloadCStringArray(metadataPairs.map(\.value)),
            metadata_len: UInt(metadataPairs.count)
        )
    }
}

private extension VesperDownloadStreamKind {
    func toRuntimeBridgePayload() -> VesperRuntimeDownloadStreamKind {
        switch self {
        case .combined:
            return VesperRuntimeDownloadStreamKindCombined
        case .video:
            return VesperRuntimeDownloadStreamKindVideo
        case .audio:
            return VesperRuntimeDownloadStreamKindAudio
        case .secondaryAudio:
            return VesperRuntimeDownloadStreamKindSecondaryAudio
        case .subtitle:
            return VesperRuntimeDownloadStreamKindSubtitle
        case .auxiliary:
            return VesperRuntimeDownloadStreamKindAuxiliary
        }
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

        let streamPointer: UnsafeMutablePointer<VesperRuntimeDownloadAssetStream>?
        if streams.isEmpty {
            streamPointer = nil
        } else {
            streamPointer = .allocate(capacity: streams.count)
            for (index, item) in streams.enumerated() {
                streamPointer?[index] = item.toRuntimeBridgePayload()
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
            streams: streamPointer,
            streams_len: UInt(streams.count),
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
                code: VesperPlayerErrorCode(ffiCode: error_code),
                category: VesperPlayerErrorCategory(ffiCategory: error_category),
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
        let headers = downloadSourceHeaders()
        let source: VesperPlayerSource
        if let url = URL(string: uri), url.isFileURL {
            source = VesperPlayerSource(
                uri: url.absoluteString,
                label: url.lastPathComponent,
                kind: .local,
                protocol: .file,
                headers: headers
            )
        } else if let url = URL(string: uri) {
            source = .remoteUrl(url, headers: headers)
        } else {
            source = VesperPlayerSource(uri: uri, label: uri, kind: .remote, protocol: .unknown, headers: headers)
        }
        return VesperDownloadSource(
            source: source,
            contentFormat: VesperDownloadContentFormat(rawValue: Int(content_format.rawValue)) ?? .unknown,
            manifestUri: stringFromRuntimeCString(manifest_uri)
        )
    }

    private func downloadSourceHeaders() -> [String: String] {
        guard let header_names, let header_values, headers_len > 0 else {
            return [:]
        }
        var headers: [String: String] = [:]
        for index in 0..<Int(headers_len) {
            guard let name = stringFromRuntimeCString(header_names[index]),
                  let value = stringFromRuntimeCString(header_values[index])
            else {
                continue
            }
            headers[name] = value
        }
        return sanitizedDownloadHttpHeaders(headers)
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

        let publicStreams: [VesperDownloadAssetStream]
        if let streamsPointer = self.streams, self.streams_len > 0 {
            publicStreams = Array(
                UnsafeBufferPointer(start: streamsPointer, count: Int(self.streams_len))
            )
                .map { $0.toPublic() }
        } else {
            publicStreams = []
        }

        return VesperDownloadAssetIndex(
            contentFormat: VesperDownloadContentFormat(rawValue: Int(content_format.rawValue)) ?? .unknown,
            version: stringFromRuntimeCString(version),
            etag: stringFromRuntimeCString(etag),
            checksum: stringFromRuntimeCString(checksum),
            totalSizeBytes: has_total_size_bytes ? total_size_bytes : nil,
            resources: publicResources,
            segments: publicSegments,
            streams: publicStreams,
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
            generatedText: nil,
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

private extension VesperRuntimeDownloadAssetStream {
    func toPublic() -> VesperDownloadAssetStream {
        VesperDownloadAssetStream(
            streamId: stringFromRuntimeCString(stream_id) ?? "",
            kind: kind.toPublic(),
            language: stringFromRuntimeCString(language),
            codec: stringFromRuntimeCString(codec),
            label: stringFromRuntimeCString(label),
            qualityRank: has_quality_rank ? quality_rank : nil,
            resourceIds: stringArrayFromRuntimeCStringArray(resource_ids, count: Int(resource_ids_len)),
            segmentIds: stringArrayFromRuntimeCStringArray(segment_ids, count: Int(segment_ids_len)),
            metadata: stringDictionaryFromRuntimeCStringArrays(
                keys: metadata_keys,
                values: metadata_values,
                count: Int(metadata_len)
            )
        )
    }
}

private extension VesperRuntimeDownloadStreamKind {
    func toPublic() -> VesperDownloadStreamKind {
        switch self {
        case VesperRuntimeDownloadStreamKindVideo:
            return .video
        case VesperRuntimeDownloadStreamKindAudio:
            return .audio
        case VesperRuntimeDownloadStreamKindSecondaryAudio:
            return .secondaryAudio
        case VesperRuntimeDownloadStreamKindSubtitle:
            return .subtitle
        case VesperRuntimeDownloadStreamKindAuxiliary:
            return .auxiliary
        default:
            return .combined
        }
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
        let buffer = UnsafeBufferPointer<VesperRuntimeDownloadEvent>(start: events, count: Int(len))
        return buffer.compactMap { event in
            switch event.kind {
            case .created:
                guard let task = event.task else {
                    return nil
                }
                return .created(task.pointee.toPublic())
            case .stateChanged:
                return .stateChanged(
                    VesperDownloadTaskStatePatch(
                        taskId: event.task_id,
                        state: VesperDownloadState(rawValue: Int(event.state_status.rawValue)) ?? .queued,
                        progress: event.state_progress.toPublic(),
                        error: event.state_has_error ? event.toDownloadError() : nil,
                        completedPath: stringFromRuntimeCString(event.state_completed_path)
                    )
                )
            case .assetIndexUpdated:
                guard let task = event.task else {
                    return nil
                }
                return .assetIndexUpdated(task.pointee.toPublic())
            case .progressUpdated:
                return .progressUpdated(
                    VesperDownloadTaskProgressPatch(
                        taskId: event.task_id,
                        progress: event.progress.toPublic()
                    )
                )
            default:
                return nil
            }
        }
    }
}

private extension VesperRuntimeDownloadEvent {
    func toDownloadError() -> VesperDownloadError {
        VesperDownloadError(
            code: VesperPlayerErrorCode(ffiCode: state_error_code),
            category: VesperPlayerErrorCategory(ffiCategory: state_error_category),
            retriable: state_error_retriable,
            message: stringFromRuntimeCString(state_error_message) ?? ""
        )
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
