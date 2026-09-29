import Foundation
import Combine

public struct VesperSourceSessionConfiguration: Equatable, Sendable {
    public let maxSources: Int
    public let maxConcurrentPreloads: Int
    public let maxPendingPreloads: Int
    public let maxMemoryBytes: UInt64

    public init(maxSources: Int = 128, maxConcurrentPreloads: Int = 2,
                maxPendingPreloads: Int = 4, maxMemoryBytes: UInt64 = 8 * 1024 * 1024) {
        self.maxSources = maxSources
        self.maxConcurrentPreloads = maxConcurrentPreloads
        self.maxPendingPreloads = maxPendingPreloads
        self.maxMemoryBytes = maxMemoryBytes
    }
}

public enum VesperSourceSessionError: String, Error, Sendable {
    case closed, invalidated, expired, foreignHandle, sourceLimit, preloadQueueFull, invalidConfiguration, invalidOptions
}

public enum VesperPreloadStatus: String, Sendable {
    case queued, running, completed, failed, unsupported, cancelled
    public var isTerminal: Bool { self != .queued && self != .running }
}

public enum VesperPreloadGoal: String, Sendable {
    case progressiveRange, dashSegmentBaseStartup, unsupported
}

/// Describes the native consumer, not evidence that a subsequent playback hit occurred.
public enum VesperPreloadCapability: String, Sendable {
    case playbackReusable, downloadOnly, none
}

public struct VesperPreloadOptions: Equatable, Sendable {
    public let maximumBytes: UInt64
    public let timeoutMs: UInt64
    public init(maximumBytes: UInt64 = 8 * 1024 * 1024, timeoutMs: UInt64 = 5_000) {
        self.maximumBytes = maximumBytes
        self.timeoutMs = timeoutMs
    }
}

public struct VesperPreloadResult: Equatable, Sendable {
    public let taskId: String
    public let handleId: String
    public let sessionId: String
    public let status: VesperPreloadStatus
    public let goal: VesperPreloadGoal
    public let capability: VesperPreloadCapability
    public let actualBytes: UInt64
    /// Whether the preload itself found all requested bytes already cached.
    public let cacheHit: Bool?
    public let reasonCode: String?
}

/// A small synchronous fence used only around in-memory cache publication.
/// Network and file I/O must never execute while holding this lock.
internal final class VesperPreloadCommitToken: @unchecked Sendable {
    private let lock = NSLock()
    private var valid = true
    func cancel() { lock.lock(); valid = false; lock.unlock() }
    func withValidity<T>(_ body: () throws -> T) throws -> T {
        lock.lock()
        defer { lock.unlock() }
        guard valid else { throw CancellationError() }
        return try body()
    }
}

@MainActor
public final class VesperPreloadTask {
    public let id: String
    public let handleId: String
    public let sessionId: String
    @Published public private(set) var snapshot: VesperPreloadResult
    fileprivate let commitToken = VesperPreloadCommitToken()
    private weak var session: VesperSourceSession?
    private var waiters: [CheckedContinuation<VesperPreloadResult, Never>] = []

    fileprivate init(handle: VesperSourceHandle, goal: VesperPreloadGoal,
                     capability: VesperPreloadCapability, session: VesperSourceSession) {
        id = UUID().uuidString
        handleId = handle.id
        sessionId = handle.sessionId
        self.session = session
        snapshot = .init(taskId: id, handleId: handleId, sessionId: sessionId,
                         status: .queued, goal: goal, capability: capability,
                         actualBytes: 0, cacheHit: nil, reasonCode: nil)
    }

    /// Cancels this shared preload, including when another caller obtained it by deduplication.
    public func cancel() { session?.cancel(self) }

    /// A retained terminal result. Cancelling a waiter does not cancel the shared preload.
    public var result: VesperPreloadResult {
        get async {
            if snapshot.status.isTerminal { return snapshot }
            return await withCheckedContinuation { waiters.append($0) }
        }
    }

    fileprivate func update(_ status: VesperPreloadStatus, bytes: UInt64 = 0,
                            hit: Bool? = nil, reason: String? = nil) {
        guard !snapshot.status.isTerminal else { return }
        snapshot = .init(taskId: id, handleId: handleId, sessionId: sessionId,
                         status: status, goal: snapshot.goal, capability: snapshot.capability,
                         actualBytes: bytes, cacheHit: hit, reasonCode: reason)
        if status.isTerminal {
            let pending = waiters
            waiters.removeAll()
            pending.forEach { $0.resume(returning: snapshot) }
        }
    }
}

@MainActor
fileprivate final class VesperSourceSessionAccess {
    var invalidated = false
}

@MainActor
fileprivate final class VesperSourceRegistration {
    let source: VesperPlayerSource
    let scope: VesperDashStartupScope
    let expiresAtEpochMs: UInt64?
    var closed = false
    var invalidated = false
    let sessionAccess: VesperSourceSessionAccess
    private let monotonicDeadlineMs: UInt64?
    private let monotonicNow: () -> UInt64
    private var expired = false
    init(source: VesperPlayerSource, scope: VesperDashStartupScope, expiresAtEpochMs: UInt64?,
         sessionAccess: VesperSourceSessionAccess, now: UInt64, monotonicNow: @escaping () -> UInt64) {
        self.source = source
        self.scope = scope
        self.expiresAtEpochMs = expiresAtEpochMs
        self.sessionAccess = sessionAccess
        self.monotonicNow = monotonicNow
        self.monotonicDeadlineMs = expiresAtEpochMs.map { expiry in
            let (deadline, overflow) = monotonicNow().addingReportingOverflow(expiry - now)
            return overflow ? UInt64.max : deadline
        }
    }
    func validate(now: UInt64, allowClosed: Bool = false) throws {
        if invalidated || sessionAccess.invalidated { throw VesperSourceSessionError.invalidated }
        if closed && !allowClosed { throw VesperSourceSessionError.closed }
        if let expiry = expiresAtEpochMs, expiry <= now { expired = true }
        if let deadline = monotonicDeadlineMs, deadline <= monotonicNow() { expired = true }
        if expired { throw VesperSourceSessionError.expired }
    }
}

/// An accepted source registration. Its descriptor never exposes the native cache scope.
@MainActor
public final class VesperSourceHandle {
    public let id: String
    public let sessionId: String
    public let source: VesperPlayerSource
    public let expiresAtEpochMs: UInt64?
    public var isClosed: Bool { registration.closed }
    public var isInvalidated: Bool { registration.invalidated || registration.sessionAccess.invalidated }
    fileprivate let registration: VesperSourceRegistration
    fileprivate weak var session: VesperSourceSession?

    fileprivate init(session: VesperSourceSession, source: VesperPlayerSource,
                     registration: VesperSourceRegistration) {
        id = UUID().uuidString
        sessionId = session.id
        self.source = source
        self.expiresAtEpochMs = registration.expiresAtEpochMs
        self.registration = registration
        self.session = session
    }
    public func preload(options: VesperPreloadOptions = .init()) throws -> VesperPreloadTask {
        guard let session else { throw VesperSourceSessionError.closed }
        return try session.preload(self, options: options)
    }
    public func close() { registration.closed = true; session?.close(self) }
    public func dispose() { close() }
    public func invalidate() { registration.invalidated = true; close() }
    internal func acquire() throws -> VesperSourceLease {
        guard let session else { throw VesperSourceSessionError.closed }
        return try session.acquire(self)
    }
}

/// Internal playback/sequence ownership. Closing a registration does not revoke an existing lease.
@MainActor
internal final class VesperSourceLease {
    let handleId: String
    let sessionId: String
    private var registration: VesperSourceRegistration?
    private let now: () -> UInt64
    fileprivate init(handle: VesperSourceHandle, now: @escaping () -> UInt64) {
        handleId = handle.id
        sessionId = handle.sessionId
        registration = handle.registration
        self.now = now
    }
    func sourceForActivation() throws -> VesperPlayerSource {
        guard let registration else { throw VesperSourceSessionError.closed }
        try registration.validate(now: now(), allowClosed: true)
        return registration.source
    }
    func release() { registration = nil }
    private init(handleId: String, sessionId: String, registration: VesperSourceRegistration, now: @escaping () -> UInt64) {
        self.handleId = handleId; self.sessionId = sessionId
        self.registration = registration; self.now = now
    }
    func retained() throws -> VesperSourceLease {
        _ = try sourceForActivation()
        return VesperSourceLease(handleId: handleId, sessionId: sessionId, registration: registration!, now: now)
    }
}

/// Bounded source registrations and preloads, independent of any player or sequence.
@MainActor
public final class VesperSourceSession {
    public let id = UUID().uuidString
    public let configuration: VesperSourceSessionConfiguration
    public private(set) var isClosed = false
    private var handles: [String: VesperSourceHandle] = [:]
    private var tasks: [String: VesperPreloadTask] = [:]
    private var pending: [(VesperPreloadTask, VesperPreloadOptions)] = []
    private var workers: [String: Task<Void, Never>] = [:]
    private var reservedBytes: [String: Int] = [:]
    private var timeouts: [String: Task<Void, Never>] = [:]
    private let cache: VesperDashStartupCache
    private let transport: any VesperDashStartupTransport
    private let progressiveLoader: any VesperSequenceWarmupLoading
    private let now: () -> UInt64
    private let capability: VesperDashSession.VideoDecodeCapabilityProvider
    private let monotonicNow: () -> UInt64
    private let access = VesperSourceSessionAccess()

    public convenience init(configuration: VesperSourceSessionConfiguration = .init()) throws {
        try self.init(configuration: configuration, cache: .shared,
                  transport: VesperDashStartupHTTPTransport(),
                  progressiveLoader: VesperSequenceURLSessionWarmupLoader())
    }

    internal init(configuration: VesperSourceSessionConfiguration = .init(),
                  cache: VesperDashStartupCache, transport: any VesperDashStartupTransport,
                  progressiveLoader: any VesperSequenceWarmupLoading = VesperSequenceURLSessionWarmupLoader(),
                  now: @escaping () -> UInt64 = { UInt64(max(0, Date().timeIntervalSince1970 * 1_000)) },
                  monotonicNow: @escaping () -> UInt64 = { DispatchTime.now().uptimeNanoseconds / 1_000_000 },
                  capability: @escaping VesperDashSession.VideoDecodeCapabilityProvider = {
                      VesperDashSession.defaultVideoDecodeCapability(for: $0)
                  }) throws {
        guard (1...512).contains(configuration.maxSources),
              (1...4).contains(configuration.maxConcurrentPreloads),
              (0...32).contains(configuration.maxPendingPreloads),
              configuration.maxMemoryBytes <= 16 * 1024 * 1024 else {
            throw VesperSourceSessionError.invalidConfiguration
        }
        self.configuration = configuration
        self.cache = cache
        self.transport = transport
        self.progressiveLoader = progressiveLoader
        self.now = now
        self.monotonicNow = monotonicNow
        self.capability = capability
    }

    public func register(_ source: VesperPlayerSource, expiresAtEpochMs: UInt64? = nil) throws -> VesperSourceHandle {
        guard !isClosed else { throw VesperSourceSessionError.closed }
        let registeredAt = now()
        if let expiry = expiresAtEpochMs, expiry <= registeredAt { throw VesperSourceSessionError.expired }
        guard handles.count < configuration.maxSources else { throw VesperSourceSessionError.sourceLimit }
        var descriptor = source
        descriptor.dashStartupScope = nil
        let scope = VesperDashStartupScope(owner: id, sourceExpiresAtMs: expiresAtEpochMs)
        var accepted = descriptor
        if accepted.protocol == .dash { accepted.dashStartupScope = scope }
        let registration = VesperSourceRegistration(source: accepted, scope: scope, expiresAtEpochMs: expiresAtEpochMs,
                                                    sessionAccess: access, now: registeredAt, monotonicNow: monotonicNow)
        let handle = VesperSourceHandle(session: self, source: descriptor, registration: registration)
        handles[handle.id] = handle
        return handle
    }

    /// Concurrent requests for one handle share its current task; the first request's options apply.
    /// At most one terminal task per registered handle is retained by the session.
    public func preload(_ handle: VesperSourceHandle, options: VesperPreloadOptions = .init()) throws -> VesperPreloadTask {
        try validate(handle)
        guard (1...16 * 1024 * 1024).contains(options.maximumBytes),
              (1...60_000).contains(options.timeoutMs) else { throw VesperSourceSessionError.invalidOptions }
        if let existing = tasks[handle.id], !existing.snapshot.status.isTerminal { return existing }
        // Cancelled work continues occupying its concurrency slot until the worker exits.
        if workers[handle.id] != nil { throw VesperSourceSessionError.preloadQueueFull }
        let source = handle.registration.source
        let supported = source.drmConfiguration == nil && (source.protocol == .dash || source.protocol == .progressive)
        let goal: VesperPreloadGoal = supported ? (source.protocol == .dash ? .dashSegmentBaseStartup : .progressiveRange) : .unsupported
        let task = VesperPreloadTask(handle: handle, goal: goal,
                                   capability: supported && configuration.maxMemoryBytes > 0
                                       ? (source.protocol == .dash ? .playbackReusable : .downloadOnly) : .none,
                                   session: self)
        guard supported else { tasks[handle.id] = task; task.update(.unsupported, reason: "unsupported_source"); return task }
        guard configuration.maxMemoryBytes > 0 else {
            tasks[handle.id] = task; task.update(.unsupported, reason: "cache_disabled"); return task
        }
        guard (pending.isEmpty && canStart(task, options)) || pending.count < configuration.maxPendingPreloads else {
            throw VesperSourceSessionError.preloadQueueFull
        }
        tasks[handle.id] = task
        pending.append((task, options))
        timeouts[handle.id] = Task { [weak self, task] in
            do { try await Task.sleep(nanoseconds: options.timeoutMs * 1_000_000) }
            catch { return }
            guard let self, !task.snapshot.status.isTerminal else { return }
            task.commitToken.cancel()
            self.workers[task.handleId]?.cancel()
            self.pending.removeAll { $0.0 === task }
            self.timeouts.removeValue(forKey: task.handleId)
            task.update(.failed, reason: "timeout")
            self.pump()
        }
        pump()
        return task
    }

    internal func acquire(_ handle: VesperSourceHandle) throws -> VesperSourceLease {
        try validate(handle)
        return VesperSourceLease(handle: handle, now: now)
    }

    private func validate(_ handle: VesperSourceHandle) throws {
        guard handle.sessionId == id else { throw VesperSourceSessionError.foreignHandle }
        try handle.registration.validate(now: now())
        guard !isClosed, handles[handle.id] === handle else { throw VesperSourceSessionError.closed }
    }

    public func close() {
        guard !isClosed else { return }
        isClosed = true
        for handle in Array(handles.values) { close(handle) }
    }
    public func dispose() { close() }

    /// Revokes every registration, including leases retained by an existing player.
    /// Playback is not stopped here; consumers must validate their lease before new activation.
    public func invalidate() {
        access.invalidated = true
        for handle in Array(handles.values) { invalidate(handle) }
        close()
    }

    fileprivate func close(_ handle: VesperSourceHandle) {
        guard handle.sessionId == id else { return }
        handle.registration.closed = true
        if let task = tasks[handle.id] { cancel(task) }
        tasks.removeValue(forKey: handle.id)
        handles.removeValue(forKey: handle.id)
    }

    fileprivate func invalidate(_ handle: VesperSourceHandle) {
        guard handle.sessionId == id else { return }
        handle.registration.invalidated = true
        close(handle)
    }

    fileprivate func cancel(_ task: VesperPreloadTask) {
        guard !task.snapshot.status.isTerminal else { return }
        task.commitToken.cancel()
        workers[task.handleId]?.cancel()
        timeouts.removeValue(forKey: task.handleId)?.cancel()
        pending.removeAll { $0.0 === task }
        task.update(.cancelled, reason: "cancelled")
        pump()
    }

    private func pump() {
        while !isClosed, let next = pending.first, canStart(next.0, next.1) {
            let (task, options) = pending.removeFirst()
            guard let handle = handles[task.handleId] else { task.update(.cancelled, reason: "closed"); continue }
            do { try validate(handle) } catch {
                timeouts.removeValue(forKey: task.handleId)?.cancel()
                task.update(.failed, reason: (error as? VesperSourceSessionError)?.rawValue ?? "source_unavailable")
                continue
            }
            task.update(.running)
            let source = handle.registration.source
            let scope = handle.registration.scope
            let token = task.commitToken
            let cache = cache, transport = transport, loader = progressiveLoader, capability = capability
            let budget = Int(configuration.maxMemoryBytes)
            let taskBudget = reservation(task, options)
            let handleId = handle.id
            reservedBytes[handleId] = taskBudget
            workers[handleId] = Task.detached { [self, task] in
                do {
                    let result = try await Self.run(source: source, scope: scope, options: options,
                                                    budget: budget, taskBudget: taskBudget, cache: cache, transport: transport,
                                                    loader: loader, capability: capability, token: token)
                    await self.finish(task, bytes: result.bytes, hit: result.hit, error: nil)
                } catch {
                    await self.finish(task, bytes: 0, hit: nil, error: error)
                }
            }
        }
    }

    private func finish(_ task: VesperPreloadTask, bytes: UInt64, hit: Bool?, error: Error?) {
        workers.removeValue(forKey: task.handleId)
        reservedBytes.removeValue(forKey: task.handleId)
        timeouts.removeValue(forKey: task.handleId)?.cancel()
        if error is CancellationError { task.update(.cancelled, reason: "cancelled") }
        else if let error { task.update(.failed, reason: Self.preloadFailureCode(error)) }
        else { task.update(.completed, bytes: bytes, hit: hit) }
        pump()
    }

    // Error descriptions can contain signed URLs or headers. Expose only typed causes.
    nonisolated private static func preloadFailureCode(_ error: Error) -> String {
        if let error = error as? VesperDashStartupError {
            switch error {
            case .httpStatus(let status): return "http_\(status)"
            case .invalidResponse: return "invalid_response"
            case .budgetExceeded: return "budget_exceeded"
            case .invalidated: return "invalidated"
            }
        }
        if let error = error as? VesperDashBridgeError {
            switch error {
            case .invalidManifest: return "invalid_manifest"
            case .unsupportedManifest: return "unsupported_manifest"
            case .invalidMp4: return "invalid_mp4"
            case .unsupportedMp4: return "unsupported_mp4"
            case .subtitle: return "unsupported_subtitle"
            case .network: return "network_error"
            }
        }
        if let error = error as? URLError {
            return error.code == .timedOut ? "network_timeout" : "network_error"
        }
        if error is CocoaError { return "local_resource_error" }
        return "preload_failed"
    }

    private func reservation(_ task: VesperPreloadTask, _ options: VesperPreloadOptions) -> Int {
        Int(min(configuration.maxMemoryBytes, options.maximumBytes,
                task.snapshot.goal == .progressiveRange ? 64 * 1024 : configuration.maxMemoryBytes))
    }

    private func canStart(_ task: VesperPreloadTask, _ options: VesperPreloadOptions) -> Bool {
        let bytes = reservation(task, options)
        return workers.count < configuration.maxConcurrentPreloads &&
            reservedBytes.values.reduce(0, +) + bytes <= Int(configuration.maxMemoryBytes)
    }

    nonisolated private static func run(source: VesperPlayerSource, scope: VesperDashStartupScope,
                                        options: VesperPreloadOptions, budget: Int, taskBudget: Int,
                                        cache: VesperDashStartupCache, transport: any VesperDashStartupTransport,
                                        loader: any VesperSequenceWarmupLoading,
                                        capability: @escaping VesperDashSession.VideoDecodeCapabilityProvider,
                                        token: VesperPreloadCommitToken) async throws -> (bytes: UInt64, hit: Bool) {
        try Task.checkCancellation()
        if source.protocol == .dash {
            return try await vesperWarmDashStartup(source: source, scope: scope, cache: cache,
                                                  transport: transport, maximumBytes: taskBudget,
                                                  capability: capability, commitToken: token,
                                                  residentMaximumBytes: budget)
        }
        guard let url = URL(string: source.uri), url.scheme?.lowercased() == "https", url.user == nil else {
            throw VesperDashStartupError.invalidResponse
        }
        let target = min(64 * 1024, taskBudget)
        let resource = VesperDashStartupResource(url: url, range: try .init(start: 0, end: UInt64(target - 1)))
        if let cached = await cache.read(scope: scope, resource: resource, headers: source.headers), cached.data.count >= target {
            return (UInt64(target), true)
        }
        let generation = await cache.currentGeneration()
        var request = URLRequest(url: url)
        request.timeoutInterval = Double(options.timeoutMs) / 1_000
        source.headers.forEach { request.setValue($1, forHTTPHeaderField: $0) }
        request.setValue("bytes=0-\(target - 1)", forHTTPHeaderField: "Range")
        let response = try await loader.load(request: request, maximumBytes: target)
        try Task.checkCancellation()
        guard (200..<300).contains(response.statusCode), !response.data.isEmpty, response.data.count <= target else {
            throw VesperDashStartupError.invalidResponse
        }
        let bytes = VesperDashStartupBytes(resource: .init(url: url, range: try .init(start: 0, end: UInt64(response.data.count - 1))),
                                            data: response.data, finalURL: url)
        guard try await cache.store(scope: scope, values: [bytes], headers: source.headers,
                                    expectedGeneration: generation, maximumBytes: budget, commitToken: token) else {
            throw VesperDashStartupError.invalidated
        }
        return (UInt64(response.data.count), false)
    }
}
