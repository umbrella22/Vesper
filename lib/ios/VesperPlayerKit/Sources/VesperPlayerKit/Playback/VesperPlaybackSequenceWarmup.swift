import CryptoKit
import Foundation

public struct VesperPlaybackSequenceWarmupSnapshot: Equatable {
    public let activeJobs: Int
    public let completedJobs: UInt64
    public let failedJobs: UInt64
    public let cancelledJobs: UInt64
    public let unsupportedJobs: UInt64
    public let cacheHits: UInt64
    public let cacheMisses: UInt64
    public let expectedBytes: UInt64
    public let actualBytes: UInt64
    public let evictedEntries: UInt64
    public let cacheEntries: Int
    public let cacheBytes: UInt64

    public init(
        activeJobs: Int = 0,
        completedJobs: UInt64 = 0,
        failedJobs: UInt64 = 0,
        cancelledJobs: UInt64 = 0,
        unsupportedJobs: UInt64 = 0,
        cacheHits: UInt64 = 0,
        cacheMisses: UInt64 = 0,
        expectedBytes: UInt64 = 0,
        actualBytes: UInt64 = 0,
        evictedEntries: UInt64 = 0,
        cacheEntries: Int = 0,
        cacheBytes: UInt64 = 0
    ) {
        self.activeJobs = activeJobs
        self.completedJobs = completedJobs
        self.failedJobs = failedJobs
        self.cancelledJobs = cancelledJobs
        self.unsupportedJobs = unsupportedJobs
        self.cacheHits = cacheHits
        self.cacheMisses = cacheMisses
        self.expectedBytes = expectedBytes
        self.actualBytes = actualBytes
        self.evictedEntries = evictedEntries
        self.cacheEntries = cacheEntries
        self.cacheBytes = cacheBytes
    }
}

private enum VesperSequenceWarmupPriority: String {
    case current
    case next
    case previous
}

private struct VesperSequenceWarmupIntent: Hashable {
    let sessionGeneration: UInt64
    let itemId: String
    let sourceReference: String
    let sourceRevision: UInt64
    let warmupTaskId: UInt64
    let cacheKey: String
    let warmupGoal: String
    let priority: VesperSequenceWarmupPriority
    let expectedBytes: UInt64
    let warmupWindowMs: UInt64

    var targetBytes: Int {
        64 * 1024
    }

    var key: VesperSequenceWarmupKey {
        VesperSequenceWarmupKey(
            sessionGeneration: sessionGeneration,
            itemId: itemId,
            sourceRevision: sourceRevision,
            warmupTaskId: warmupTaskId,
            cacheKey: cacheKey,
            warmupGoal: warmupGoal
        )
    }

    static func parse(_ value: [String: Any]) -> VesperSequenceWarmupIntent? {
        guard let sessionGeneration = value["sessionGeneration"] as? UInt64,
              sessionGeneration > 0,
              let itemId = value["itemId"] as? String, !itemId.isEmpty,
              let sourceReference = value["sourceReference"] as? String, !sourceReference.isEmpty,
              let sourceRevision = value["sourceRevision"] as? UInt64, sourceRevision > 0,
              let warmupTaskId = value["warmupTaskId"] as? UInt64, warmupTaskId > 0,
              let warmupGoal = value["warmupGoal"] as? String,
              warmupGoal == "progressiveRange",
              let priorityValue = value["priority"] as? String,
              let priority = VesperSequenceWarmupPriority(rawValue: priorityValue),
              let identity = value["cacheIdentity"] as? [String: Any],
              let cacheKey = identity["canonicalKey"] as? String,
              !cacheKey.isEmpty, cacheKey.count <= 2_048,
              !cacheKey.contains("://") else { return nil }
        let profile = value["profile"] as? [String: Any]
        let expectedBytes = profile?["expectedMemoryBytes"] as? UInt64 ?? 0
        let warmupWindowMs = profile?["warmupWindowMs"] as? UInt64 ?? 0
        return VesperSequenceWarmupIntent(
            sessionGeneration: sessionGeneration,
            itemId: itemId,
            sourceReference: sourceReference,
            sourceRevision: sourceRevision,
            warmupTaskId: warmupTaskId,
            cacheKey: cacheKey,
            warmupGoal: warmupGoal,
            priority: priority,
            expectedBytes: expectedBytes,
            warmupWindowMs: warmupWindowMs
        )
    }
}

private struct VesperSequenceWarmupKey: Hashable {
    let sessionGeneration: UInt64
    let itemId: String
    let sourceRevision: UInt64
    let warmupTaskId: UInt64
    let cacheKey: String
    let warmupGoal: String
}

internal struct VesperSequenceWarmupReport {
    let sessionGeneration: UInt64
    let taskId: UInt64
    let itemId: String
    let sourceRevision: UInt64
    let status: String
    let expectedBytes: UInt64
    let actualBytes: UInt64
    let cacheHit: Bool?
    let cacheEntries: Int
    let cacheBytes: UInt64
    let evictedEntries: UInt64
    let reasonCode: String?
}

internal struct VesperSequenceCacheInventory: Equatable, Sendable {
    let evicted: UInt64
    let entries: Int
    let bytes: UInt64
}

internal enum VesperSequenceCacheError: Error, Equatable {
    case directoryUnavailable
    case readFailed
    case storeFailed
    case inventoryFailed
    case evictionFailed
}

internal protocol VesperSequenceWarmupCaching: Sendable {
    func configure(maxBytes: UInt64) async throws -> VesperSequenceCacheInventory
    func read(key: String, length: Int) async throws -> Data?
    func store(key: String, data: Data) async throws -> VesperSequenceCacheInventory
    func inventory() async throws -> VesperSequenceCacheInventory
}

internal struct VesperSequenceWarmupHTTPResponse: Sendable {
    let statusCode: Int
    let data: Data
}

internal enum VesperSequenceWarmupLoadingError: Error {
    case nonHTTPResponse
}

internal protocol VesperSequenceWarmupLoading: Sendable {
    func load(
        request: URLRequest,
        maximumBytes: Int
    ) async throws -> VesperSequenceWarmupHTTPResponse
}

internal struct VesperSequenceURLSessionWarmupLoader: VesperSequenceWarmupLoading {
    func load(
        request: URLRequest,
        maximumBytes: Int
    ) async throws -> VesperSequenceWarmupHTTPResponse {
        let (bytes, response) = try await URLSession.shared.bytes(for: request)
        guard let http = response as? HTTPURLResponse else {
            throw VesperSequenceWarmupLoadingError.nonHTTPResponse
        }
        guard (200..<300).contains(http.statusCode), maximumBytes > 0 else {
            return VesperSequenceWarmupHTTPResponse(statusCode: http.statusCode, data: Data())
        }
        var bounded = Data()
        bounded.reserveCapacity(maximumBytes)
        for try await byte in bytes {
            try Task.checkCancellation()
            bounded.append(byte)
            if bounded.count >= maximumBytes { break }
        }
        return VesperSequenceWarmupHTTPResponse(statusCode: http.statusCode, data: bounded)
    }
}

internal actor VesperSequenceFileCache: VesperSequenceWarmupCaching {
    private static let maximumEntries = 4_096

    private struct FileRecord {
        let url: URL
        let size: UInt64
        let modificationDate: Date
    }

    private let directory: URL
    private var maxBytes: UInt64
    private let maxEntries: Int
    private let fileManager: FileManager
    private var initializationError: VesperSequenceCacheError?

    init(
        maxBytes: UInt64,
        directory: URL? = nil,
        maxEntries: Int = VesperSequenceFileCache.maximumEntries,
        fileManager: FileManager = .default
    ) {
        let manager = fileManager
        self.fileManager = manager
        self.maxBytes = min(maxBytes, 256 * 1024 * 1024)
        self.maxEntries = min(max(maxEntries, 0), Self.maximumEntries)
        if let directory {
            self.directory = directory
        } else {
            let root = manager.urls(for: .cachesDirectory, in: .userDomainMask).first
                ?? manager.temporaryDirectory
            self.directory = root.appendingPathComponent("vesper-sequence-cache", isDirectory: true)
        }
        do {
            try manager.createDirectory(at: self.directory, withIntermediateDirectories: true)
            initializationError = nil
        } catch {
            initializationError = .directoryUnavailable
        }
    }

    func configure(maxBytes: UInt64) throws -> VesperSequenceCacheInventory {
        try checkAvailable()
        self.maxBytes = min(maxBytes, 256 * 1024 * 1024)
        return try evictIfNeeded()
    }

    func read(key: String, length: Int) throws -> Data? {
        try checkAvailable()
        let url = fileURL(for: key)
        guard length > 0, fileManager.fileExists(atPath: url.path) else { return nil }
        let handle: FileHandle
        do {
            handle = try FileHandle(forReadingFrom: url)
        } catch {
            throw VesperSequenceCacheError.readFailed
        }
        defer { try? handle.close() }
        let data: Data
        do {
            data = try handle.read(upToCount: length) ?? Data()
        } catch {
            throw VesperSequenceCacheError.readFailed
        }
        guard data.count >= length else {
            return nil
        }
        try? fileManager.setAttributes([.modificationDate: Date()], ofItemAtPath: url.path)
        return Data(data.prefix(length))
    }

    func store(key: String, data: Data) throws -> VesperSequenceCacheInventory {
        try checkAvailable()
        let url = fileURL(for: key)
        let temporary = directory.appendingPathComponent(".tmp-\(UUID().uuidString)")
        do {
            try data.write(to: temporary, options: [.atomic])
            if fileManager.fileExists(atPath: url.path) {
                try fileManager.removeItem(at: url)
            }
            try fileManager.moveItem(at: temporary, to: url)
        } catch {
            _ = try? fileManager.removeItem(at: temporary)
            throw VesperSequenceCacheError.storeFailed
        }
        return try evictIfNeeded()
    }

    func inventory() throws -> VesperSequenceCacheInventory {
        try checkAvailable()
        let files = try cacheFiles()
        return VesperSequenceCacheInventory(
            evicted: 0,
            entries: files.count,
            bytes: saturatedTotal(files).value
        )
    }

    private func evictIfNeeded() throws -> VesperSequenceCacheInventory {
        var files = try cacheFiles()
        var evicted: UInt64 = 0
        files.sort {
            if $0.modificationDate == $1.modificationDate {
                return $0.url.lastPathComponent < $1.url.lastPathComponent
            }
            return $0.modificationDate < $1.modificationDate
        }
        while let oldest = files.first {
            let total = saturatedTotal(files)
            guard files.count > maxEntries || total.overflowed || total.value > maxBytes else {
                break
            }
            do {
                try fileManager.removeItem(at: oldest.url)
            } catch {
                throw VesperSequenceCacheError.evictionFailed
            }
            files.removeFirst()
            evicted = vesperSequenceSaturatingAdd(evicted, 1)
        }
        let total = saturatedTotal(files)
        guard !total.overflowed else {
            throw VesperSequenceCacheError.inventoryFailed
        }
        return VesperSequenceCacheInventory(evicted: evicted, entries: files.count, bytes: total.value)
    }

    private func cacheFiles() throws -> [FileRecord] {
        let contents: [URL]
        do {
            contents = try fileManager.contentsOfDirectory(
                at: directory,
                includingPropertiesForKeys: [.fileSizeKey, .contentModificationDateKey]
            )
        } catch {
            throw VesperSequenceCacheError.inventoryFailed
        }
        var records: [FileRecord] = []
        records.reserveCapacity(min(contents.count, maxEntries + 1))
        for url in contents {
            if url.lastPathComponent.hasPrefix(".tmp-") {
                do {
                    try fileManager.removeItem(at: url)
                } catch {
                    throw VesperSequenceCacheError.evictionFailed
                }
                continue
            }
            guard url.pathExtension == "bin" else { continue }
            do {
                let attributes = try fileManager.attributesOfItem(atPath: url.path)
                guard let size = attributes[.size] as? NSNumber,
                      let date = attributes[.modificationDate] as? Date else {
                    throw VesperSequenceCacheError.inventoryFailed
                }
                records.append(
                    FileRecord(url: url, size: size.uint64Value, modificationDate: date)
                )
            } catch let error as VesperSequenceCacheError {
                throw error
            } catch {
                throw VesperSequenceCacheError.inventoryFailed
            }
        }
        return records
    }

    private func saturatedTotal(_ files: [FileRecord]) -> (value: UInt64, overflowed: Bool) {
        var total: UInt64 = 0
        for file in files {
            let result = total.addingReportingOverflow(file.size)
            if result.overflow {
                return (UInt64.max, true)
            }
            total = result.partialValue
        }
        return (total, false)
    }

    private func checkAvailable() throws {
        if let initializationError {
            throw initializationError
        }
    }

    private func fileURL(for key: String) -> URL {
        let digest = SHA256.hash(data: Data(key.utf8))
        let name = digest.map { String(format: "%02x", $0) }.joined()
        return directory.appendingPathComponent("\(name).bin")
    }
}

internal func vesperSequenceSaturatingAdd(_ lhs: UInt64, _ rhs: UInt64) -> UInt64 {
    let result = lhs.addingReportingOverflow(rhs)
    return result.overflow ? UInt64.max : result.partialValue
}

@MainActor
internal final class VesperPlaybackSequenceWarmupExecutor {
    private static let maxConcurrentJobs = 2
    private static let maxDesiredIntents = 4
    private static let maxTerminalKeys = 512

    private struct JobRecord {
        let token: UInt64
        let task: Task<Void, Never>
    }

    // All sequence executors in one process share one actor-owned directory.
    // AVPlayer/URLSession callers can therefore never create competing owners
    // for the same physical cache path.
    private static let sharedCache = VesperSequenceFileCache(maxBytes: 256 * 1024 * 1024)

    private var jobs: [VesperSequenceWarmupKey: JobRecord] = [:]
    private var terminalKeys: Set<VesperSequenceWarmupKey> = []
    private var terminalKeyOrder: [VesperSequenceWarmupKey] = []
    private var nextJobToken: UInt64 = 1
    private var stats = VesperPlaybackSequenceWarmupSnapshot()
    private let cache: any VesperSequenceWarmupCaching
    private let loader: any VesperSequenceWarmupLoading
    private let requestedMaxDiskBytes: UInt64
    private let onSourceExpired: (String, UInt64) -> Void
    private let onReport: (VesperSequenceWarmupReport) -> Void
    private var isClosed = false

    init(
        maxDiskBytes: UInt64 = 256 * 1024 * 1024,
        onSourceExpired: @escaping (String, UInt64) -> Void,
        onReport: @escaping (VesperSequenceWarmupReport) -> Void = { _ in },
        cache: (any VesperSequenceWarmupCaching)? = nil,
        loader: any VesperSequenceWarmupLoading = VesperSequenceURLSessionWarmupLoader()
    ) {
        self.cache = cache ?? Self.sharedCache
        self.loader = loader
        requestedMaxDiskBytes = min(maxDiskBytes, 256 * 1024 * 1024)
        self.onSourceExpired = onSourceExpired
        self.onReport = onReport
    }

    var snapshot: VesperPlaybackSequenceWarmupSnapshot { stats }

    func reconcile(
        intents: [[String: Any]],
        sourceLookup: (String, String, UInt64) -> VesperPlayerSource?
    ) {
        guard !isClosed else { return }
        let parsed = intents.compactMap(VesperSequenceWarmupIntent.parse)
            .sorted { lhs, rhs in
                let rank: [VesperSequenceWarmupPriority: Int] = [.current: 0, .next: 1, .previous: 2]
                return rank[lhs.priority, default: 3] < rank[rhs.priority, default: 3]
            }
            .prefix(Self.maxDesiredIntents)
        let desired = Set(parsed.map(\.key))
        let staleKeys = jobs.keys.filter { !desired.contains($0) }
        for key in staleKeys {
            // Keep the record until its task exits. This prevents a canceled
            // task from racing with a replacement that uses the same key.
            jobs[key]?.task.cancel()
        }
        for intent in parsed {
            let key = intent.key
            guard jobs[key] == nil,
                  !terminalKeys.contains(key),
                  jobs.count < Self.maxConcurrentJobs else { continue }
            guard let source = sourceLookup(intent.sourceReference, intent.itemId, intent.sourceRevision) else {
                reject(intent: intent, reasonCode: "source_reference_missing")
                continue
            }
            guard !isClosed,
                  jobs[key] == nil,
                  !terminalKeys.contains(key),
                  jobs.count < Self.maxConcurrentJobs else { continue }
            let token = nextJobToken
            nextJobToken = nextJobToken == UInt64.max ? 1 : nextJobToken + 1
            let task = Task { [weak self] in
                guard let self else { return }
                await self.run(intent: intent, source: source, token: token)
            }
            jobs[key] = JobRecord(token: token, task: task)
        }
        stats = stats.withActiveJobs(jobs.count)
    }

    func close() {
        guard !isClosed else { return }
        isClosed = true
        jobs.values.forEach { $0.task.cancel() }
        jobs.removeAll()
        terminalKeys.removeAll()
        terminalKeyOrder.removeAll()
        stats = stats.withActiveJobs(0)
    }

    private func run(
        intent: VesperSequenceWarmupIntent,
        source: VesperPlayerSource,
        token: UInt64
    ) async {
        guard isCurrent(intent: intent, token: token), !isClosed else { return }
        guard source.drmConfiguration == nil, source.protocol == .progressive,
              let url = URL(string: source.uri) else {
            if finish(intent: intent, token: token, transform: { $0.incrementUnsupported() }) {
                emitReport(intent: intent, status: "unsupported", reasonCode: "protocol_or_drm_unsupported")
            }
            return
        }
        let target = intent.targetBytes
        let configuredInventory: VesperSequenceCacheInventory
        do {
            configuredInventory = try await cache.configure(maxBytes: requestedMaxDiskBytes)
        } catch {
            finishSuspensionFailure(
                error,
                intent: intent,
                token: token,
                expectedBytes: 0,
                reasonCode: "cache_configuration_failed"
            )
            return
        }
        guard continueAfterSuspension(intent: intent, token: token, expectedBytes: UInt64(target)) else {
            return
        }
        stats = stats.addExpected(UInt64(target))
        emitReport(intent: intent, status: "started", expectedBytes: UInt64(target))
        let cached: Data?
        do {
            cached = try await cache.read(key: intent.cacheKey, length: target)
        } catch {
            finishSuspensionFailure(
                error,
                intent: intent,
                token: token,
                expectedBytes: UInt64(target),
                reasonCode: "cache_read_failed"
            )
            return
        }
        guard continueAfterSuspension(intent: intent, token: token, expectedBytes: UInt64(target)) else {
            return
        }
        if let cached, cached.count == target {
            let observedInventory: VesperSequenceCacheInventory
            do {
                observedInventory = try await cache.inventory()
            } catch {
                finishSuspensionFailure(
                    error,
                    intent: intent,
                    token: token,
                    expectedBytes: UInt64(target),
                    actualBytes: UInt64(cached.count),
                    reasonCode: "cache_inventory_failed"
                )
                return
            }
            guard continueAfterSuspension(
                intent: intent,
                token: token,
                expectedBytes: UInt64(target)
            ) else { return }
            let inventory = combiningEvictions(configuredInventory, observedInventory)
            if finish(
                intent: intent,
                token: token,
                transform: { $0.addHit(actual: UInt64(cached.count), inventory: inventory) }
            ) {
                emitReport(
                    intent: intent,
                    status: "completed",
                    expectedBytes: UInt64(target),
                    actualBytes: UInt64(cached.count),
                    cacheHit: true,
                    cacheEntries: inventory.entries,
                    cacheBytes: inventory.bytes,
                    evictedEntries: inventory.evicted
                )
            }
            return
        }

        var request = URLRequest(url: url)
        let timeoutMillis = min(max(intent.warmupWindowMs, 1_000), 60_000)
        request.timeoutInterval = Double(timeoutMillis) / 1_000.0
        source.headers.forEach { header, value in
            if header.caseInsensitiveCompare("Range") != .orderedSame {
                request.setValue(value, forHTTPHeaderField: header)
            }
        }
        request.setValue("bytes=0-\(target - 1)", forHTTPHeaderField: "Range")
        do {
            try Task.checkCancellation()
            let response = try await loader.load(request: request, maximumBytes: target)
            try Task.checkCancellation()
            guard continueAfterSuspension(intent: intent, token: token, expectedBytes: UInt64(target)) else {
                return
            }
            if [401, 403, 410].contains(response.statusCode) {
                if finish(intent: intent, token: token, transform: { $0.incrementFailed() }) {
                    onSourceExpired(intent.itemId, intent.sourceRevision)
                    emitReport(intent: intent, status: "failed", expectedBytes: UInt64(target), reasonCode: "source_expired")
                }
            } else if !(200..<300).contains(response.statusCode) {
                if finish(intent: intent, token: token, transform: { $0.incrementFailed() }) {
                    emitReport(intent: intent, status: "failed", expectedBytes: UInt64(target), reasonCode: "http_failure")
                }
            } else {
                let bounded = Data(response.data.prefix(target))
                let inventory: VesperSequenceCacheInventory
                do {
                    inventory = try await cache.store(key: intent.cacheKey, data: bounded)
                } catch {
                    finishSuspensionFailure(
                        error,
                        intent: intent,
                        token: token,
                        expectedBytes: UInt64(target),
                        actualBytes: UInt64(bounded.count),
                        reasonCode: "cache_store_failed"
                    )
                    return
                }
                guard continueAfterSuspension(
                    intent: intent,
                    token: token,
                    expectedBytes: UInt64(target)
                ) else { return }
                let operationInventory = combiningEvictions(configuredInventory, inventory)
                if finish(
                    intent: intent,
                    token: token,
                    transform: { $0.addMiss(actual: UInt64(bounded.count), inventory: operationInventory) }
                ) {
                    emitReport(
                        intent: intent,
                        status: "completed",
                        expectedBytes: UInt64(target),
                        actualBytes: UInt64(bounded.count),
                        cacheHit: false,
                        cacheEntries: operationInventory.entries,
                        cacheBytes: operationInventory.bytes,
                        evictedEntries: operationInventory.evicted
                    )
                }
            }
        } catch {
            let reasonCode = error is VesperSequenceWarmupLoadingError
                ? "non_http_response"
                : "warmup_failed"
            finishSuspensionFailure(
                error,
                intent: intent,
                token: token,
                expectedBytes: UInt64(target),
                reasonCode: reasonCode
            )
        }
    }

    private func combiningEvictions(
        _ initial: VesperSequenceCacheInventory,
        _ final: VesperSequenceCacheInventory
    ) -> VesperSequenceCacheInventory {
        VesperSequenceCacheInventory(
            evicted: vesperSequenceSaturatingAdd(initial.evicted, final.evicted),
            entries: final.entries,
            bytes: final.bytes
        )
    }

    private func finishSuspensionFailure(
        _ error: Error,
        intent: VesperSequenceWarmupIntent,
        token: UInt64,
        expectedBytes: UInt64,
        actualBytes: UInt64 = 0,
        reasonCode: String
    ) {
        if Task.isCancelled
            || error is CancellationError
            || (error as? URLError)?.code == .cancelled {
            finishCancelled(intent: intent, token: token, expectedBytes: expectedBytes)
        } else if finish(intent: intent, token: token, transform: { $0.incrementFailed() }) {
            emitReport(
                intent: intent,
                status: "failed",
                expectedBytes: expectedBytes,
                actualBytes: actualBytes,
                reasonCode: reasonCode
            )
        }
    }

    private func reject(intent: VesperSequenceWarmupIntent, reasonCode: String) {
        guard !isClosed,
              jobs[intent.key] == nil,
              recordTerminalKey(intent.key) else { return }
        stats = stats.incrementFailed()
        emitReport(intent: intent, status: "failed", reasonCode: reasonCode)
    }

    private func continueAfterSuspension(
        intent: VesperSequenceWarmupIntent,
        token: UInt64,
        expectedBytes: UInt64
    ) -> Bool {
        guard isCurrent(intent: intent, token: token), !isClosed else { return false }
        guard !Task.isCancelled else {
            finishCancelled(intent: intent, token: token, expectedBytes: expectedBytes)
            return false
        }
        return true
    }

    private func finishCancelled(
        intent: VesperSequenceWarmupIntent,
        token: UInt64,
        expectedBytes: UInt64
    ) {
        if finish(intent: intent, token: token, transform: { $0.incrementCancelled() }) {
            emitReport(intent: intent, status: "cancelled", expectedBytes: expectedBytes)
        }
    }

    private func emitReport(
        intent: VesperSequenceWarmupIntent,
        status: String,
        expectedBytes: UInt64 = 0,
        actualBytes: UInt64 = 0,
        cacheHit: Bool? = nil,
        cacheEntries: Int = 0,
        cacheBytes: UInt64 = 0,
        evictedEntries: UInt64 = 0,
        reasonCode: String? = nil
    ) {
        guard !isClosed else { return }
        onReport(
            VesperSequenceWarmupReport(
                sessionGeneration: intent.sessionGeneration,
                taskId: intent.warmupTaskId,
                itemId: intent.itemId,
                sourceRevision: intent.sourceRevision,
                status: status,
                expectedBytes: expectedBytes,
                actualBytes: actualBytes,
                cacheHit: cacheHit,
                cacheEntries: cacheEntries,
                cacheBytes: cacheBytes,
                evictedEntries: evictedEntries,
                reasonCode: reasonCode
            )
        )
    }

    private func isCurrent(intent: VesperSequenceWarmupIntent, token: UInt64) -> Bool {
        jobs[intent.key]?.token == token
    }

    private func finish(
        intent: VesperSequenceWarmupIntent,
        token: UInt64,
        transform: (VesperPlaybackSequenceWarmupSnapshot) -> VesperPlaybackSequenceWarmupSnapshot
    ) -> Bool {
        let key = intent.key
        guard jobs[key]?.token == token else { return false }
        stats = transform(stats)
        _ = recordTerminalKey(key)
        jobs.removeValue(forKey: key)
        stats = stats.withActiveJobs(jobs.count)
        return true
    }

    private func recordTerminalKey(_ key: VesperSequenceWarmupKey) -> Bool {
        guard terminalKeys.insert(key).inserted else { return false }
        terminalKeyOrder.append(key)
        while terminalKeyOrder.count > Self.maxTerminalKeys {
            terminalKeys.remove(terminalKeyOrder.removeFirst())
        }
        return true
    }
}

private extension VesperPlaybackSequenceWarmupSnapshot {
    func withActiveJobs(_ value: Int) -> Self {
        VesperPlaybackSequenceWarmupSnapshot(
            activeJobs: value,
            completedJobs: completedJobs,
            failedJobs: failedJobs,
            cancelledJobs: cancelledJobs,
            unsupportedJobs: unsupportedJobs,
            cacheHits: cacheHits,
            cacheMisses: cacheMisses,
            expectedBytes: expectedBytes,
            actualBytes: actualBytes,
            evictedEntries: evictedEntries,
            cacheEntries: cacheEntries,
            cacheBytes: cacheBytes
        )
    }

    func addExpected(_ value: UInt64) -> Self {
        var copy = self
        copy = VesperPlaybackSequenceWarmupSnapshot(
            activeJobs: activeJobs,
            completedJobs: completedJobs,
            failedJobs: failedJobs,
            cancelledJobs: cancelledJobs,
            unsupportedJobs: unsupportedJobs,
            cacheHits: cacheHits,
            cacheMisses: cacheMisses,
            expectedBytes: vesperSequenceSaturatingAdd(expectedBytes, value),
            actualBytes: actualBytes,
            evictedEntries: evictedEntries,
            cacheEntries: cacheEntries,
            cacheBytes: cacheBytes
        )
        return copy
    }

    func addHit(actual: UInt64, inventory: VesperSequenceCacheInventory) -> Self {
        VesperPlaybackSequenceWarmupSnapshot(
            activeJobs: activeJobs,
            completedJobs: vesperSequenceSaturatingAdd(completedJobs, 1),
            failedJobs: failedJobs,
            cancelledJobs: cancelledJobs,
            unsupportedJobs: unsupportedJobs,
            cacheHits: vesperSequenceSaturatingAdd(cacheHits, 1),
            cacheMisses: cacheMisses,
            expectedBytes: expectedBytes,
            actualBytes: vesperSequenceSaturatingAdd(actualBytes, actual),
            evictedEntries: vesperSequenceSaturatingAdd(evictedEntries, inventory.evicted),
            cacheEntries: inventory.entries,
            cacheBytes: inventory.bytes
        )
    }

    func addMiss(actual: UInt64, inventory: VesperSequenceCacheInventory) -> Self {
        VesperPlaybackSequenceWarmupSnapshot(
            activeJobs: activeJobs,
            completedJobs: vesperSequenceSaturatingAdd(completedJobs, 1),
            failedJobs: failedJobs,
            cancelledJobs: cancelledJobs,
            unsupportedJobs: unsupportedJobs,
            cacheHits: cacheHits,
            cacheMisses: vesperSequenceSaturatingAdd(cacheMisses, 1),
            expectedBytes: expectedBytes,
            actualBytes: vesperSequenceSaturatingAdd(actualBytes, actual),
            evictedEntries: vesperSequenceSaturatingAdd(evictedEntries, inventory.evicted),
            cacheEntries: inventory.entries,
            cacheBytes: inventory.bytes
        )
    }

    func incrementFailed() -> Self {
        var copy = self
        copy = VesperPlaybackSequenceWarmupSnapshot(
            activeJobs: activeJobs,
            completedJobs: completedJobs,
            failedJobs: vesperSequenceSaturatingAdd(failedJobs, 1),
            cancelledJobs: cancelledJobs,
            unsupportedJobs: unsupportedJobs,
            cacheHits: cacheHits,
            cacheMisses: cacheMisses,
            expectedBytes: expectedBytes,
            actualBytes: actualBytes,
            evictedEntries: evictedEntries,
            cacheEntries: cacheEntries,
            cacheBytes: cacheBytes
        )
        return copy
    }

    func incrementCancelled() -> Self {
        VesperPlaybackSequenceWarmupSnapshot(
            activeJobs: activeJobs,
            completedJobs: completedJobs,
            failedJobs: failedJobs,
            cancelledJobs: vesperSequenceSaturatingAdd(cancelledJobs, 1),
            unsupportedJobs: unsupportedJobs,
            cacheHits: cacheHits,
            cacheMisses: cacheMisses,
            expectedBytes: expectedBytes,
            actualBytes: actualBytes,
            evictedEntries: evictedEntries,
            cacheEntries: cacheEntries,
            cacheBytes: cacheBytes
        )
    }

    func incrementUnsupported() -> Self {
        VesperPlaybackSequenceWarmupSnapshot(
            activeJobs: activeJobs,
            completedJobs: completedJobs,
            failedJobs: failedJobs,
            cancelledJobs: cancelledJobs,
            unsupportedJobs: vesperSequenceSaturatingAdd(unsupportedJobs, 1),
            cacheHits: cacheHits,
            cacheMisses: cacheMisses,
            expectedBytes: expectedBytes,
            actualBytes: actualBytes,
            evictedEntries: evictedEntries,
            cacheEntries: cacheEntries,
            cacheBytes: cacheBytes
        )
    }
}
