import Combine
import Foundation
@_implementationOnly import VesperPlayerKitBridgeShim

public enum VesperPlaybackSequenceMode: String, Codable {
    case finite
    case replenishable
}

public enum VesperPlaybackSequenceMediaKind: String, Codable {
    case vod
    case live
    case liveDvr
}

public struct VesperPlaybackSequenceConfiguration: Equatable {
    public let sequenceId: String
    public let mode: VesperPlaybackSequenceMode
    public let historyLimit: Int
    public let forwardWindow: Int
    public let refillThreshold: Int
    public let maxItems: Int
    public let maxPendingRequests: Int
    public let maxEvents: Int
    public let requestTimeoutMs: UInt64
    public let sourceExpiryLeadMs: UInt64
    public let maxSourceRegistryEntries: Int

    public init(
        sequenceId: String,
        mode: VesperPlaybackSequenceMode = .finite,
        historyLimit: Int = 16,
        forwardWindow: Int = 1,
        refillThreshold: Int = 1,
        maxItems: Int = 512,
        maxPendingRequests: Int = 32,
        maxEvents: Int = 512,
        requestTimeoutMs: UInt64 = 15_000,
        sourceExpiryLeadMs: UInt64 = 15_000,
        maxSourceRegistryEntries: Int = 1_024
    ) {
        precondition(!sequenceId.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
        precondition((1...512).contains(maxItems))
        precondition((1...512).contains(maxPendingRequests))
        precondition((1...1_024).contains(maxEvents))
        precondition((maxItems...4_096).contains(maxSourceRegistryEntries))
        self.sequenceId = sequenceId
        self.mode = mode
        self.historyLimit = historyLimit
        self.forwardWindow = forwardWindow
        self.refillThreshold = refillThreshold
        self.maxItems = maxItems
        self.maxPendingRequests = maxPendingRequests
        self.maxEvents = maxEvents
        self.requestTimeoutMs = requestTimeoutMs
        self.sourceExpiryLeadMs = sourceExpiryLeadMs
        self.maxSourceRegistryEntries = maxSourceRegistryEntries
    }
}

public struct VesperPlaybackSequenceContentIdentity: Equatable {
    public let providerNamespace: String
    public let value: String

    public init(providerNamespace: String, value: String) {
        self.providerNamespace = providerNamespace
        self.value = value
    }
}

public struct VesperPlaybackSequencePreloadProfile: Equatable {
    public let expectedMemoryBytes: UInt64
    public let expectedDiskBytes: UInt64
    public let ttlMs: UInt64?
    public let warmupWindowMs: UInt64?

    public init(
        expectedMemoryBytes: UInt64 = 0,
        expectedDiskBytes: UInt64 = 0,
        ttlMs: UInt64? = nil,
        warmupWindowMs: UInt64? = nil
    ) {
        self.expectedMemoryBytes = expectedMemoryBytes
        self.expectedDiskBytes = expectedDiskBytes
        self.ttlMs = ttlMs
        self.warmupWindowMs = warmupWindowMs
    }
}

public struct VesperPlaybackSequenceItem {
    public let itemId: String
    public let contentIdentity: VesperPlaybackSequenceContentIdentity
    public let mediaKind: VesperPlaybackSequenceMediaKind
    public let source: VesperSourceHandle?
    public let providerMetadataRef: String?
    public let preloadProfile: VesperPlaybackSequencePreloadProfile
    public init(itemId: String, contentIdentity: VesperPlaybackSequenceContentIdentity,
                mediaKind: VesperPlaybackSequenceMediaKind = .vod, source: VesperSourceHandle? = nil,
                providerMetadataRef: String? = nil, preloadProfile: VesperPlaybackSequencePreloadProfile = .init()) {
        self.itemId = itemId; self.contentIdentity = contentIdentity; self.mediaKind = mediaKind
        self.source = source; self.providerMetadataRef = providerMetadataRef; self.preloadProfile = preloadProfile
    }
}

@_spi(VesperFlutter) public struct VesperPlaybackSequenceResolvedSource {
    public let sessionGeneration: UInt64
    public let requestId: UInt64
    public let resolutionAttemptId: UInt64
    public let itemId: String
    public let expectedSourceRevision: UInt64
    public let source: VesperSourceHandle
    public init(sessionGeneration: UInt64, requestId: UInt64, resolutionAttemptId: UInt64,
                itemId: String, expectedSourceRevision: UInt64, source: VesperSourceHandle) {
        self.sessionGeneration = sessionGeneration; self.requestId = requestId
        self.resolutionAttemptId = resolutionAttemptId; self.itemId = itemId
        self.expectedSourceRevision = expectedSourceRevision; self.source = source
    }
}

public struct VesperPlaybackSequenceSourceRequest {
    public let itemId: String
    public let reason: String
    fileprivate let generation: UInt64
    fileprivate let requestId: UInt64
    fileprivate let attemptId: UInt64
    fileprivate let expectedRevision: UInt64
    fileprivate init?(_ value: [String: Any]) {
        let body = value["request"] as? [String: Any] ?? value
        guard body["type"] as? String == "sourceResolutionRequired",
              let itemId = body["itemId"] as? String,
              let generation = body["sessionGeneration"] as? UInt64,
              let requestId = body["requestId"] as? UInt64,
              let attempt = body["resolutionAttemptId"] as? UInt64,
              let revision = body["expectedSourceRevision"] as? UInt64 else { return nil }
        self.itemId = itemId; self.reason = body["reason"] as? String ?? "initial"
        self.generation = generation; self.requestId = requestId; self.attemptId = attempt; self.expectedRevision = revision
    }
}

public struct VesperPlaybackSequenceItemState {
    public let itemId: String
    public let index: Int
    public let isActive: Bool
    public let mediaKind: String
    public let sourceState: String
    public let sourceRevision: UInt64
    internal let sourceReference: String?
}

public struct VesperPlaybackSequenceSnapshot {
    public var sourceRequests: [VesperPlaybackSequenceSourceRequest] { pendingRequests.compactMap(VesperPlaybackSequenceSourceRequest.init) }
    public let sequenceId: String
    public let sessionGeneration: UInt64
    public let activationEpoch: UInt64
    public let items: [VesperPlaybackSequenceItemState]
    public let activeItemId: String?
    public let pendingRequests: [[String: Any]]
    public let requestFailures: [[String: Any]]
    public let previousEndReached: Bool
    public let nextEndReached: Bool
    public let droppedEvents: UInt64
    public let warmupTasks: [[String: Any]]
    public let warmupStats: [String: Any]

    public var wire: [String: Any] {
        [
            "sequenceId": sequenceId,
            "sessionGeneration": sessionGeneration,
            "activationEpoch": activationEpoch,
            "items": items.map { item in
                [
                    "index": item.index,
                    "isActive": item.isActive,
                    "item": [
                        "itemId": item.itemId,
                        "mediaKind": item.mediaKind,
                        "sourceState": [
                            "state": item.sourceState,
                            "sourceRevision": item.sourceRevision,
                            "sourceReference": item.sourceReference as Any,
                        ],
                    ],
                ]
            },
            "activeItemId": activeItemId as Any,
            "pendingRequests": pendingRequests,
            "requestFailures": requestFailures,
            "previousEndReached": previousEndReached,
            "nextEndReached": nextEndReached,
            "droppedEvents": droppedEvents,
            "warmupTasks": warmupTasks,
            "warmupStats": warmupStats,
        ]
    }
}

public struct VesperPlaybackSequenceEvent {
    public let eventSequence: UInt64
    public let sessionGeneration: UInt64
    public let event: [String: Any]

    public var wire: [String: Any] {
        [
            "type": "event",
            "sequenceId": event["sequenceId"] as Any,
            "sessionGeneration": sessionGeneration,
            "eventSequence": eventSequence,
            "event": event,
        ]
    }
}

@MainActor
public final class VesperPlaybackSequence: ObservableObject, VesperPlaybackSequenceAttachment {
    @Published public private(set) var snapshot: VesperPlaybackSequenceSnapshot
    public let events = PassthroughSubject<VesperPlaybackSequenceEvent, Never>()
    public let configuration: VesperPlaybackSequenceConfiguration

    private struct SourceRegistryEntry {
        let itemId: String
        let sourceRevision: UInt64
        let lease: VesperSourceLease
        let handle: VesperSourceHandle
    }

    private final class PendingNavigation {
        let id = UUID().uuidString
        var itemId: String?
        let options: VesperSourceActivationOptions
        let continuation: CheckedContinuation<VesperSourceActivation?, Error>
        var worker: Task<Void, Never>?
        var timeout: Task<Void, Never>?
        init(options: VesperSourceActivationOptions,
             continuation: CheckedContinuation<VesperSourceActivation?, Error>) {
            self.options = options; self.continuation = continuation
        }
    }
    private var pendingNavigation: PendingNavigation?
    private var sessionHandle: UInt64 = 0
    private weak var controller: VesperPlayerController?
    private var sourceRegistry: [String: SourceRegistryEntry] = [:]
    private var sourceReferenceCounter: UInt64 = 1
    private var attachEpoch: UInt64 = 0
    private var isDisposed = false

    public init(configuration: VesperPlaybackSequenceConfiguration) throws {
        self.configuration = configuration
        snapshot = VesperPlaybackSequenceSnapshot(
            sequenceId: configuration.sequenceId,
            sessionGeneration: 1,
            activationEpoch: 0,
            items: [],
            activeItemId: nil,
            pendingRequests: [],
            requestFailures: [],
            previousEndReached: false,
            nextEndReached: false,
            droppedEvents: 0,
            warmupTasks: [],
            warmupStats: [:]
        )
        let configObject: [String: Any] = [
            "sequenceId": configuration.sequenceId,
            "mode": configuration.mode.rawValue,
            "historyLimit": configuration.historyLimit,
            "forwardWindow": configuration.forwardWindow,
            "refillThreshold": configuration.refillThreshold,
            "maxItems": configuration.maxItems,
            "maxPendingRequests": configuration.maxPendingRequests,
            "maxEvents": configuration.maxEvents,
            "requestTimeoutMs": configuration.requestTimeoutMs,
            "sourceExpiryLeadMs": configuration.sourceExpiryLeadMs,
        ]
        var configData = try JSONSerialization.data(withJSONObject: configObject)
        configData.append(0) // The C bridge consumes a NUL-terminated UTF-8 string.
        var handle: UInt64 = 0
        let created = configData.withUnsafeBytes { bytes in
            guard let base = bytes.baseAddress else { return false }
            return vesper_runtime_sequence_session_create(
                base.assumingMemoryBound(to: CChar.self),
                &handle
            )
        }
        guard created, handle != 0 else {
            throw VesperPlayerError(
                message: "native sequence session creation failed",
                code: .backendFailure,
                category: .platform,
                retriable: false
            )
        }
        sessionHandle = handle
    }

    deinit {
        if sessionHandle != 0 {
            vesper_runtime_sequence_session_dispose(sessionHandle)
        }
    }

    public func attach(to target: VesperPlayerController) throws {
        try checkActive()
        guard controller == nil else { throw sequenceError("already_attached") }
        attachEpoch = attachEpoch == UInt64.max ? 1 : attachEpoch + 1
        try target.attachPlaybackSequence(self)
        controller = target
        try pumpPreloadIntents()
    }

    public func detach() {
        cancelNavigation(.detached)
        attachEpoch = attachEpoch == UInt64.max ? 1 : attachEpoch + 1
        controller?.detachPlaybackSequence(self)
        controller = nil
        cancelSequencePreloads()
    }

    public func onControllerDisposed(_ controller: VesperPlayerController) {
        guard self.controller === controller else { return }
        cancelNavigation(.disposed)
        self.controller = nil
        sourceRegistry.removeAll(keepingCapacity: false)
        cancelSequencePreloads()
        _ = try? execute(["type": "replace", "items": []])
    }

    public func dispose() {
        guard !isDisposed else { return }
        isDisposed = true
        cancelNavigation(.disposed)
        detach()
        sourceRegistry.removeAll(keepingCapacity: false)
        if sessionHandle != 0 {
            vesper_runtime_sequence_session_dispose(sessionHandle)
            sessionHandle = 0
        }
    }

    public func replace(_ items: [VesperPlaybackSequenceItem]) throws {
        try checkBatch(items)
        var staged: [String: SourceRegistryEntry] = [:]
        let wireItems = try items.map { try itemWire($0, staged: &staged) }
        let command: [String: Any] = ["type": "replace", "items": wireItems]
        _ = try execute(command)
        cancelNavigation(.superseded)
        sourceRegistry = staged
        try refreshAndPump()
    }

    @discardableResult
    public func append(
        sessionGeneration: UInt64,
        requestId: UInt64,
        anchorItemId: String?,
        items: [VesperPlaybackSequenceItem],
        endReached: Bool
    ) throws -> Int {
        try submitItemsResponse(
            type: "append",
            sessionGeneration: sessionGeneration,
            requestId: requestId,
            anchorItemId: anchorItemId,
            items: items,
            endReached: endReached
        )
    }

    @discardableResult
    public func prepend(
        sessionGeneration: UInt64,
        requestId: UInt64,
        anchorItemId: String?,
        items: [VesperPlaybackSequenceItem],
        endReached: Bool
    ) throws -> Int {
        try submitItemsResponse(
            type: "prepend",
            sessionGeneration: sessionGeneration,
            requestId: requestId,
            anchorItemId: anchorItemId,
            items: items,
            endReached: endReached
        )
    }

    @discardableResult
    public func remove(itemId: String) throws -> Bool {
        let result = try execute(["type": "remove", "itemId": itemId])
        if result["removed"] as? Bool == true, pendingNavigation?.itemId == itemId {
            cancelNavigation(.superseded)
        }
        try refreshAndPump()
        return result["removed"] as? Bool ?? false
    }

    public func activate(_ itemId: String, options: VesperSourceActivationOptions = .init()) async throws -> VesperSourceActivation {
        guard let result = try await navigate(["type": "setActive", "itemId": itemId], options: options) else {
            throw sequenceError("activation_target_unavailable")
        }
        return result
    }
    public func next(options: VesperSourceActivationOptions = .init()) async throws -> VesperSourceActivation? {
        try await navigate(["type": "next"], options: options)
    }
    public func previous(options: VesperSourceActivationOptions = .init()) async throws -> VesperSourceActivation? {
        try await navigate(["type": "previous"], options: options)
    }

    private func navigate(_ command: [String: Any], options: VesperSourceActivationOptions) async throws -> VesperSourceActivation? {
        try checkActive()
        try options.validate()
        guard controller != nil else { throw VesperSourceActivationError.detached }
        try Task.checkCancellation()
        cancelNavigation(.superseded)
        let navigationId = UUID().uuidString
        return try await withTaskCancellationHandler {
            try await withCheckedThrowingContinuation { continuation in
                let pending = PendingNavigation(options: options, continuation: continuation)
                pendingNavigation = pending
                navigationCancellationId = navigationId
                do {
                    let result = try execute(command)
                    let outcome = result["outcome"] as? String
                    if outcome == "empty" || outcome == "reachedEnd" || outcome == "awaitingItems" {
                        pendingNavigation = nil
                        try refreshAndPump()
                        continuation.resume(returning: nil)
                        return
                    }
                    pending.itemId = command["itemId"] as? String ?? result["itemId"] as? String
                    pending.timeout = Task { [weak self, pending] in
                        do { try await Task.sleep(nanoseconds: options.timeoutMs * 1_000_000) } catch { return }
                        guard let self, self.pendingNavigation === pending else { return }
                        self.cancelNavigation(.timeout)
                    }
                    try refreshAndPump()
                } catch {
                    pendingNavigation = nil
                    pending.timeout?.cancel()
                    continuation.resume(throwing: error)
                }
            }
        } onCancel: {
            Task { @MainActor [weak self] in
                guard let self, self.navigationCancellationId == navigationId else { return }
                self.cancelNavigation(.cancelled)
            }
        }
    }
    private var navigationCancellationId = ""

    private func cancelNavigation(_ reason: VesperSourceActivationError) {
        guard let pending = pendingNavigation else { return }
        pendingNavigation = nil
        pending.worker?.cancel(); pending.timeout?.cancel()
        pending.continuation.resume(throwing: reason)
    }

    private func driveNavigation() {
        guard let pending = pendingNavigation, pending.worker == nil, let itemId = pending.itemId else { return }
        guard let item = snapshot.items.first(where: { $0.itemId == itemId }) else {
            cancelNavigation(.superseded); return
        }
        if item.sourceState == "failed" { cancelNavigation(.cancelled); return }
        guard let reference = item.sourceReference, let entry = sourceRegistry[reference], let target = controller else { return }
        pending.worker = Task { [weak self, pending] in
            do {
                let activation = try await target.activateLease(entry.lease.retained(), options: pending.options)
                guard let self, self.pendingNavigation === pending else { return }
                self.pendingNavigation = nil; pending.timeout?.cancel()
                pending.continuation.resume(returning: activation)
            } catch {
                guard let self, self.pendingNavigation === pending else { return }
                self.pendingNavigation = nil; pending.timeout?.cancel()
                pending.continuation.resume(throwing: error)
            }
        }
    }

    public func submitResolvedSource(request: VesperPlaybackSequenceSourceRequest, source: VesperSourceHandle) throws {
        try submitResolvedSource(.init(sessionGeneration: request.generation, requestId: request.requestId,
                                       resolutionAttemptId: request.attemptId, itemId: request.itemId,
                                       expectedSourceRevision: request.expectedRevision, source: source))
    }

    @_spi(VesperFlutter) public func submitResolvedSource(_ resolved: VesperPlaybackSequenceResolvedSource) throws {
        try checkActive()
        let sourceReference = nextSourceReference()
        guard sourceRegistry.count < configuration.maxSourceRegistryEntries else {
            throw sequenceError("source_registry_capacity_exceeded")
        }
        guard resolved.expectedSourceRevision < UInt64.max else { throw sequenceError("source_revision_exhausted") }
        let revision = resolved.expectedSourceRevision + 1
        let lease = try resolved.source.acquire()
        let descriptor = try lease.sourceForActivation()
        sourceRegistry[sourceReference] = SourceRegistryEntry(itemId: resolved.itemId, sourceRevision: revision, lease: lease, handle: resolved.source)
        let source: [String: Any] = [
            "sessionGeneration": resolved.sessionGeneration,
            "requestId": resolved.requestId,
            "resolutionAttemptId": resolved.resolutionAttemptId,
            "itemId": resolved.itemId,
            "expectedSourceRevision": resolved.expectedSourceRevision,
            "sourceRevision": revision,
            "sourceReference": sourceReference,
            "warmupGoal": descriptor.sequenceWarmupGoal,
            "cacheIdentity": cacheIdentity(lease, revision: revision),
            "expiresAtEpochMs": resolved.source.expiresAtEpochMs as Any,
        ]
        do {
            _ = try execute(["type": "submitResolvedSource", "source": source])
            try refreshAndPump()
        } catch {
            sourceRegistry.removeValue(forKey: sourceReference)
            throw error
        }
    }

    public func markSourceExpired(itemId: String, sourceRevision: UInt64) throws {
        _ = try execute([
            "type": "markSourceExpired",
            "itemId": itemId,
            "sourceRevision": sourceRevision,
        ])
        try refreshAndPump()
        pruneRegistry()
    }

    public func failRequest(
        sessionGeneration: UInt64,
        requestId: UInt64,
        reasonCode: String
    ) throws {
        _ = try execute([
            "type": "failRequest",
            "sessionGeneration": sessionGeneration,
            "requestId": requestId,
            "reasonCode": reasonCode,
        ])
        try refreshAndPump()
    }

    public func tick() throws {
        _ = try execute(["type": "tick"])
        try refreshAndPump()
    }

    public func resyncPendingRequests() throws {
        _ = try execute(["type": "resyncPendingRequests"])
        try refreshAndPump()
    }

    public func validateActivationCallback(
        itemId: String,
        activationEpoch: UInt64,
        sourceRevision: UInt64
    ) -> Bool {
        do {
            _ = try execute([
                "type": "validateActivationCallback",
                "itemId": itemId,
                "activationEpoch": activationEpoch,
                "sourceRevision": sourceRevision,
            ])
            return true
        } catch {
            return false
        }
    }

    private func submitItemsResponse(
        type: String,
        sessionGeneration: UInt64,
        requestId: UInt64,
        anchorItemId: String?,
        items: [VesperPlaybackSequenceItem],
        endReached: Bool
    ) throws -> Int {
        try checkBatch(items)
        var staged: [String: SourceRegistryEntry] = [:]
        let wireItems = try items.map { try itemWire($0, staged: &staged) }
        var command: [String: Any] = [
            "type": type,
            "sessionGeneration": sessionGeneration,
            "requestId": requestId,
            "items": wireItems,
            "endReached": endReached,
        ]
        command["anchorItemId"] = anchorItemId as Any
        guard sourceRegistry.count + staged.count <= configuration.maxSourceRegistryEntries else {
            throw sequenceError("source_registry_capacity_exceeded")
        }
        let result = try execute(command)
        sourceRegistry.merge(staged) { current, _ in current }
        try refreshAndPump()
        pruneRegistry()
        return result["acceptedCount"] as? Int ?? 0
    }

    private func itemWire(
        _ item: VesperPlaybackSequenceItem,
        staged: inout [String: SourceRegistryEntry]
    ) throws -> [String: Any] {
        var wire: [String: Any] = [
            "itemId": item.itemId,
            "providerNamespace": item.contentIdentity.providerNamespace,
            "contentIdentity": item.contentIdentity.value,
            "mediaKind": item.mediaKind.rawValue,
            "preloadProfile": [
                "expectedMemoryBytes": item.preloadProfile.expectedMemoryBytes,
                "expectedDiskBytes": item.preloadProfile.expectedDiskBytes,
                "ttlMs": item.preloadProfile.ttlMs as Any,
                "warmupWindowMs": item.preloadProfile.warmupWindowMs as Any,
            ],
        ]
        if let providerMetadataRef = item.providerMetadataRef {
            wire["providerMetadataRef"] = providerMetadataRef
        }
        if let handle = item.source {
            let reference = nextSourceReference()
            let existing = sourceRegistry.values.first { $0.itemId == item.itemId && $0.lease.handleId == handle.id && $0.lease.sessionId == handle.sessionId }
            let lease = try existing?.lease.retained() ?? handle.acquire()
            let descriptor = try lease.sourceForActivation()
            let oldRevision = snapshot.items.first(where: { $0.itemId == item.itemId })?.sourceRevision ?? 0
            guard oldRevision < UInt64.max else { throw sequenceError("source_revision_exhausted") }
            let revision = existing?.sourceRevision ?? oldRevision + 1
            staged[reference] = SourceRegistryEntry(itemId: item.itemId, sourceRevision: revision, lease: lease, handle: handle)
            wire["resolvedSource"] = [
                "sourceReference": reference,
                "warmupGoal": descriptor.sequenceWarmupGoal,
                "cacheIdentity": cacheIdentity(lease, revision: revision),
                "expiresAtEpochMs": handle.expiresAtEpochMs as Any,
            ]
        }
        return wire
    }

    private func execute(_ command: [String: Any]) throws -> [String: Any] {
        try checkActive()
        var commandData = try JSONSerialization.data(withJSONObject: command)
        commandData.append(0)
        var output: UnsafeMutablePointer<CChar>?
        let succeeded = commandData.withUnsafeBytes { bytes in
            guard let base = bytes.baseAddress else { return false }
            return vesper_runtime_sequence_session_execute(
                sessionHandle,
                base.assumingMemoryBound(to: CChar.self),
                UInt64(Date().timeIntervalSince1970 * 1_000),
                &output
            )
        }
        guard succeeded, let output else { throw sequenceError("sequence_bridge_failure") }
        defer { vesper_runtime_sequence_string_free(output) }
        let data = Data(bytes: output, count: strlen(output))
        let envelope = try JSONSerialization.jsonObject(with: data) as? [String: Any]
        guard let envelope else { throw sequenceError("invalid_sequence_response") }
        guard envelope["ok"] as? Bool == true else {
            let error = envelope["error"] as? [String: Any]
            throw sequenceError(error?["code"] as? String ?? "sequence_error")
        }
        return envelope["result"] as? [String: Any] ?? [:]
    }

    private func refreshAndPump() throws {
        try refreshSnapshot()
        pruneRegistry()
        try drainEvents()
        driveNavigation()
        try pumpPreloadIntents()
    }

    private func pumpPreloadIntents() throws {
        guard controller != nil, !isDisposed else { return }
        var output: UnsafeMutablePointer<CChar>?
        guard vesper_runtime_sequence_session_preload_intents(
            sessionHandle,
            UInt64(Date().timeIntervalSince1970 * 1_000),
            &output
        ), let output else { return }
        defer { vesper_runtime_sequence_string_free(output) }
        let data = Data(bytes: output, count: strlen(output))
        guard let envelope = try JSONSerialization.jsonObject(with: data) as? [String: Any],
              envelope["ok"] as? Bool == true,
              let result = envelope["result"] as? [String: Any],
              let rawIntents = result["intents"] as? [[String: Any]] else { return }
        let desired = Set(rawIntents.prefix(Self.maxPreloadObservations).compactMap { $0["warmupTaskId"] as? UInt64 })
        for (id, observation) in preloadObservations where !desired.contains(id) {
            observation.active = false
            observation.startedObservation = nil
        }
        for intent in rawIntents.prefix(Self.maxPreloadObservations) {
            guard preloadObservations.count < Self.maxPreloadObservations,
                  let id = intent["warmupTaskId"] as? UInt64, preloadObservations[id] == nil,
                  let reference = intent["sourceReference"] as? String,
                  let entry = sourceRegistry[reference],
                  intent["itemId"] as? String == entry.itemId,
                  intent["sourceRevision"] as? UInt64 == entry.sourceRevision else { continue }
            do {
                _ = try entry.lease.sourceForActivation()
                let task = try entry.handle.preload()
                let observation = PreloadObservation(task: task, intent: intent)
                preloadObservations[id] = observation
                observation.startedObservation = task.$snapshot.filter { $0.status == .running }.prefix(1).sink { [weak self] _ in
                    guard let self, observation.active, !self.isDisposed else { return }
                    _ = try? self.execute(["type": "reportWarmup", "sessionGeneration": intent["sessionGeneration"] as Any,
                                  "taskId": id, "itemId": entry.itemId, "sourceRevision": entry.sourceRevision,
                                  "warmupGoal": intent["warmupGoal"] as Any, "status": "started",
                                  "expectedBytes": 0, "actualBytes": 0, "cacheEntries": 0, "cacheBytes": 0, "evictedEntries": 0])
                }
                Task { @MainActor [weak self, observation] in
                    let result = await task.result
                    let inventory = await VesperDashStartupCache.shared.inventory()
                    guard let self else { return }
                    self.preloadObservations.removeValue(forKey: id)
                    observation.startedObservation = nil
                    defer { if !self.isDisposed { try? self.pumpPreloadIntents() } }
                    guard observation.active, !self.isDisposed else { return }
                    var report: [String: Any] = [
                        "type": "reportWarmup", "sessionGeneration": intent["sessionGeneration"] as Any,
                        "taskId": id, "itemId": entry.itemId, "sourceRevision": entry.sourceRevision,
                        "warmupGoal": intent["warmupGoal"] as Any, "status": result.status.rawValue,
                        "expectedBytes": 0, "actualBytes": result.actualBytes,
                        "cacheEntries": inventory.entries, "cacheBytes": inventory.bytes, "evictedEntries": 0,
                    ]
                    if let hit = result.cacheHit { report["cacheHit"] = hit }
                    if let reason = result.reasonCode { report["reasonCode"] = reason }
                    _ = try? self.execute(report)
                    try? self.refreshSnapshot()
                    try? self.drainEvents()
                }
            } catch {
                _ = try? execute(["type": "reportWarmup", "sessionGeneration": intent["sessionGeneration"] as Any,
                                  "taskId": id, "itemId": entry.itemId, "sourceRevision": entry.sourceRevision,
                                  "warmupGoal": intent["warmupGoal"] as Any, "status": "unsupported",
                                  "expectedBytes": 0, "actualBytes": 0, "cacheEntries": 0, "cacheBytes": 0,
                                  "evictedEntries": 0, "reasonCode": "source_unavailable"])
            }
        }
    }

    private final class PreloadObservation {
        let task: VesperPreloadTask
        let intent: [String: Any]
        var active = true
        var startedObservation: AnyCancellable?
        init(task: VesperPreloadTask, intent: [String: Any]) { self.task = task; self.intent = intent }
    }
    private var preloadObservations: [UInt64: PreloadObservation] = [:]
    private static let maxPreloadObservations = 4
    private func cancelSequencePreloads() {
        for observation in preloadObservations.values {
            observation.active = false
            observation.startedObservation = nil
        }
    }

    private func refreshSnapshot() throws {
        var output: UnsafeMutablePointer<CChar>?
        guard vesper_runtime_sequence_session_snapshot(sessionHandle, &output), let output else {
            throw sequenceError("sequence_bridge_failure")
        }
        defer { vesper_runtime_sequence_string_free(output) }
        let data = Data(bytes: output, count: strlen(output))
        guard let envelope = try JSONSerialization.jsonObject(with: data) as? [String: Any],
              let result = envelope["result"] as? [String: Any]
        else { throw sequenceError("invalid_sequence_snapshot") }
        snapshot = try parseSnapshot(result)
    }

    private func drainEvents() throws {
        var output: UnsafeMutablePointer<CChar>?
        guard vesper_runtime_sequence_session_drain_events(sessionHandle, 512, &output),
              let output else { throw sequenceError("sequence_bridge_failure") }
        defer { vesper_runtime_sequence_string_free(output) }
        let data = Data(bytes: output, count: strlen(output))
        guard let envelope = try JSONSerialization.jsonObject(with: data) as? [String: Any],
              let result = envelope["result"] as? [String: Any],
              let events = result["events"] as? [[String: Any]]
        else { throw sequenceError("invalid_sequence_events") }
        for value in events {
            guard let sequence = value["eventSequence"] as? UInt64,
                  let generation = value["sessionGeneration"] as? UInt64,
                  let event = value["event"] as? [String: Any] else { continue }
            self.events.send(
                VesperPlaybackSequenceEvent(
                    eventSequence: sequence,
                    sessionGeneration: generation,
                    event: event
                )
            )
        }
    }

    private func parseSnapshot(_ value: [String: Any]) throws -> VesperPlaybackSequenceSnapshot {
        guard let sequenceId = value["sequenceId"] as? String,
              let generation = value["sessionGeneration"] as? UInt64,
              let activationEpoch = value["activationEpoch"] as? UInt64,
              let rawItems = value["items"] as? [[String: Any]] else {
            throw sequenceError("invalid_sequence_snapshot")
        }
        let items = rawItems.compactMap { raw -> VesperPlaybackSequenceItemState? in
            guard let item = raw["item"] as? [String: Any],
                  let state = item["sourceState"] as? [String: Any],
                  let itemId = item["itemId"] as? String,
                  let index = raw["index"] as? Int,
                  let isActive = raw["isActive"] as? Bool,
                  let mediaKind = item["mediaKind"] as? String,
                  let sourceState = state["state"] as? String else { return nil }
            return VesperPlaybackSequenceItemState(
                itemId: itemId,
                index: index,
                isActive: isActive,
                mediaKind: mediaKind,
                sourceState: sourceState,
                sourceRevision: state["sourceRevision"] as? UInt64
                    ?? state["expectedSourceRevision"] as? UInt64
                    ?? 0,
                sourceReference: state["sourceReference"] as? String
            )
        }
        return VesperPlaybackSequenceSnapshot(
            sequenceId: sequenceId,
            sessionGeneration: generation,
            activationEpoch: activationEpoch,
            items: items,
            activeItemId: value["activeItemId"] as? String,
            pendingRequests: value["pendingRequests"] as? [[String: Any]] ?? [],
            requestFailures: value["requestFailures"] as? [[String: Any]] ?? [],
            previousEndReached: value["previousEndReached"] as? Bool ?? false,
            nextEndReached: value["nextEndReached"] as? Bool ?? false,
            droppedEvents: value["droppedEvents"] as? UInt64 ?? 0,
            warmupTasks: value["warmupTasks"] as? [[String: Any]] ?? [],
            warmupStats: value["warmupStats"] as? [String: Any] ?? [:]
        )
    }

    private func cacheIdentity(_ lease: VesperSourceLease, revision: UInt64) -> [String: Any] {
        ["providerNamespace": "vesper", "contentIdentity": lease.handleId,
         "renditionIdentity": "source", "resourceIdentity": lease.handleId,
         "accessPartition": lease.sessionId, "sourceRevision": revision]
    }

    private func pruneRegistry() {
        let retained = Set(snapshot.items.compactMap(\.sourceReference))
        sourceRegistry = sourceRegistry.filter { retained.contains($0.key) }
    }

    private func checkBatch(_ items: [VesperPlaybackSequenceItem]) throws {
        try checkActive()
        guard items.count <= configuration.maxItems else {
            throw sequenceError("capacity_exceeded")
        }
        guard Set(items.map(\.itemId)).count == items.count else {
            throw sequenceError("duplicate_item_id")
        }
    }

    private func nextSourceReference() -> String {
        let value = sourceReferenceCounter
        sourceReferenceCounter = sourceReferenceCounter == UInt64.max ? 1 : value + 1
        return "sequence-source-\(value)"
    }

    private func checkActive() throws {
        if isDisposed { throw sequenceError("sequence_disposed") }
    }

    private func sequenceError(_ code: String) -> VesperPlayerError {
        VesperPlayerError(
            message: code,
            code: .invalidState,
            category: .playback,
            retriable: false,
            details: ["code": code]
        )
    }
}
