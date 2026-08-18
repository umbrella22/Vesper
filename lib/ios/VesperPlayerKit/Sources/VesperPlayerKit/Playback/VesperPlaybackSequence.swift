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

public struct VesperPlaybackSequenceCacheIdentity: Equatable {
    public let providerNamespace: String
    public let contentIdentity: String
    public let renditionIdentity: String
    public let resourceIdentity: String
    public let accessPartition: String
    public let sourceRevision: UInt64

    public init(
        providerNamespace: String,
        contentIdentity: String,
        renditionIdentity: String,
        resourceIdentity: String,
        accessPartition: String,
        sourceRevision: UInt64
    ) {
        self.providerNamespace = providerNamespace
        self.contentIdentity = contentIdentity
        self.renditionIdentity = renditionIdentity
        self.resourceIdentity = resourceIdentity
        self.accessPartition = accessPartition
        self.sourceRevision = sourceRevision
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

public struct VesperPlaybackSequenceItem: Equatable {
    public let itemId: String
    public let contentIdentity: VesperPlaybackSequenceContentIdentity
    public let mediaKind: VesperPlaybackSequenceMediaKind
    public let source: VesperPlayerSource?
    public let cacheIdentity: VesperPlaybackSequenceCacheIdentity?
    public let sourceRevision: UInt64
    public let expiresAtEpochMs: UInt64?
    public let providerMetadataRef: String?
    public let preloadProfile: VesperPlaybackSequencePreloadProfile

    public init(
        itemId: String,
        contentIdentity: VesperPlaybackSequenceContentIdentity,
        mediaKind: VesperPlaybackSequenceMediaKind = .vod,
        source: VesperPlayerSource? = nil,
        cacheIdentity: VesperPlaybackSequenceCacheIdentity? = nil,
        sourceRevision: UInt64 = 0,
        expiresAtEpochMs: UInt64? = nil,
        providerMetadataRef: String? = nil,
        preloadProfile: VesperPlaybackSequencePreloadProfile =
            VesperPlaybackSequencePreloadProfile()
    ) {
        precondition(!itemId.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
        precondition(!contentIdentity.providerNamespace.isEmpty)
        precondition(!contentIdentity.value.isEmpty)
        precondition((source == nil) == (cacheIdentity == nil))
        if let cacheIdentity {
            precondition(sourceRevision > 0 && cacheIdentity.sourceRevision == sourceRevision)
        }
        self.itemId = itemId
        self.contentIdentity = contentIdentity
        self.mediaKind = mediaKind
        self.source = source
        self.cacheIdentity = cacheIdentity
        self.sourceRevision = sourceRevision
        self.expiresAtEpochMs = expiresAtEpochMs
        self.providerMetadataRef = providerMetadataRef
        self.preloadProfile = preloadProfile
    }
}

public struct VesperPlaybackSequenceResolvedSource {
    public let sessionGeneration: UInt64
    public let requestId: UInt64
    public let resolutionAttemptId: UInt64
    public let itemId: String
    public let expectedSourceRevision: UInt64
    public let sourceRevision: UInt64
    public let source: VesperPlayerSource
    public let cacheIdentity: VesperPlaybackSequenceCacheIdentity
    public let expiresAtEpochMs: UInt64?

    public init(
        sessionGeneration: UInt64,
        requestId: UInt64,
        resolutionAttemptId: UInt64,
        itemId: String,
        expectedSourceRevision: UInt64,
        sourceRevision: UInt64,
        source: VesperPlayerSource,
        cacheIdentity: VesperPlaybackSequenceCacheIdentity,
        expiresAtEpochMs: UInt64? = nil
    ) {
        precondition(sourceRevision > expectedSourceRevision)
        precondition(cacheIdentity.sourceRevision == sourceRevision)
        self.sessionGeneration = sessionGeneration
        self.requestId = requestId
        self.resolutionAttemptId = resolutionAttemptId
        self.itemId = itemId
        self.expectedSourceRevision = expectedSourceRevision
        self.sourceRevision = sourceRevision
        self.source = source
        self.cacheIdentity = cacheIdentity
        self.expiresAtEpochMs = expiresAtEpochMs
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
        let source: VesperPlayerSource
    }

    private struct AppliedActivation: Equatable {
        let itemId: String
        let sourceRevision: UInt64
        let activationEpoch: UInt64
    }

    private var sessionHandle: UInt64 = 0
    private weak var controller: VesperPlayerController?
    private var sourceRegistry: [String: SourceRegistryEntry] = [:]
    private var sourceReferenceCounter: UInt64 = 1
    private var appliedActivation: AppliedActivation?
    private var warmupExecutor: VesperPlaybackSequenceWarmupExecutor?
    private var attachEpoch: UInt64 = 0
    private var isDisposed = false

    public var warmupSnapshot: VesperPlaybackSequenceWarmupSnapshot {
        warmupExecutor?.snapshot ?? VesperPlaybackSequenceWarmupSnapshot()
    }

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
            droppedEvents: 0
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
        let configData = try JSONSerialization.data(withJSONObject: configObject)
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
        let epoch = attachEpoch
        let maxDiskBytes = UInt64(max(target.resiliencePolicy.cache.maxDiskBytes ?? 256 * 1024 * 1024, 0))
        let executor = VesperPlaybackSequenceWarmupExecutor(
            maxDiskBytes: maxDiskBytes,
            onSourceExpired: { [weak self] itemId, sourceRevision in
                Task { @MainActor [weak self] in
                    guard let self, !self.isDisposed, self.attachEpoch == epoch else { return }
                    _ = try? self.markSourceExpired(itemId: itemId, sourceRevision: sourceRevision)
                }
            },
            onReport: { [weak self] report in
                guard let self, !self.isDisposed, self.attachEpoch == epoch else { return }
                var command: [String: Any] = [
                    "type": "reportWarmup",
                    "sessionGeneration": report.sessionGeneration,
                    "taskId": report.taskId,
                    "itemId": report.itemId,
                    "sourceRevision": report.sourceRevision,
                    "warmupGoal": "progressiveRange",
                    "status": report.status,
                    "expectedBytes": report.expectedBytes,
                    "actualBytes": report.actualBytes,
                    "cacheEntries": report.cacheEntries,
                    "cacheBytes": report.cacheBytes,
                    "evictedEntries": report.evictedEntries,
                ]
                if let cacheHit = report.cacheHit { command["cacheHit"] = cacheHit }
                if let reasonCode = report.reasonCode { command["reasonCode"] = reasonCode }
                if (try? self.execute(command)) != nil {
                    try? self.refreshSnapshot()
                    try? self.drainEvents()
                }
            }
        )
        do {
            try target.attachPlaybackSequence(self)
            controller = target
            warmupExecutor = executor
            try applyActiveSource()
            try pumpPreloadIntents()
        } catch {
            executor.close()
            if controller === target {
                target.detachPlaybackSequence(self)
                controller = nil
            }
            warmupExecutor = nil
            appliedActivation = nil
            throw error
        }
    }

    public func detach() {
        attachEpoch = attachEpoch == UInt64.max ? 1 : attachEpoch + 1
        controller?.detachPlaybackSequence(self)
        controller = nil
        appliedActivation = nil
        warmupExecutor?.close()
        warmupExecutor = nil
    }

    public func onControllerDisposed(_ controller: VesperPlayerController) {
        guard self.controller === controller else { return }
        self.controller = nil
        appliedActivation = nil
        sourceRegistry.removeAll(keepingCapacity: false)
        warmupExecutor?.close()
        warmupExecutor = nil
        _ = try? execute(["type": "replace", "items": []])
    }

    public func dispose() {
        guard !isDisposed else { return }
        isDisposed = true
        detach()
        sourceRegistry.removeAll(keepingCapacity: false)
        if sessionHandle != 0 {
            vesper_runtime_sequence_session_dispose(sessionHandle)
            sessionHandle = 0
        }
    }

    public func replace(
        _ items: [VesperPlaybackSequenceItem],
        activeItemId: String? = nil
    ) throws {
        try checkBatch(items)
        var staged: [String: SourceRegistryEntry] = [:]
        let wireItems = try items.map { try itemWire($0, staged: &staged) }
        var command: [String: Any] = ["type": "replace", "items": wireItems]
        command["activeItemId"] = activeItemId ?? items.first?.itemId
        _ = try execute(command)
        sourceRegistry = staged
        appliedActivation = nil
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
        try refreshAndPump()
        return result["removed"] as? Bool ?? false
    }

    public func setActive(_ itemId: String) throws {
        _ = try execute(["type": "setActive", "itemId": itemId])
        try refreshAndPump()
    }

    public func next() throws {
        _ = try execute(["type": "next"])
        try refreshAndPump()
    }

    public func previous() throws {
        _ = try execute(["type": "previous"])
        try refreshAndPump()
    }

    public func submitResolvedSource(_ resolved: VesperPlaybackSequenceResolvedSource) throws {
        try checkActive()
        let sourceReference = nextSourceReference()
        guard sourceRegistry.count < configuration.maxSourceRegistryEntries else {
            throw sequenceError("source_registry_capacity_exceeded")
        }
        sourceRegistry[sourceReference] = SourceRegistryEntry(
            itemId: resolved.itemId,
            sourceRevision: resolved.sourceRevision,
            source: resolved.source
        )
        let source: [String: Any] = [
            "sessionGeneration": resolved.sessionGeneration,
            "requestId": resolved.requestId,
            "resolutionAttemptId": resolved.resolutionAttemptId,
            "itemId": resolved.itemId,
            "expectedSourceRevision": resolved.expectedSourceRevision,
            "sourceRevision": resolved.sourceRevision,
            "sourceReference": sourceReference,
            "cacheIdentity": resolved.cacheIdentity.wire,
            "expiresAtEpochMs": resolved.expiresAtEpochMs as Any,
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
        if let source = item.source, let cacheIdentity = item.cacheIdentity {
            let reference = nextSourceReference()
            staged[reference] = SourceRegistryEntry(
                itemId: item.itemId,
                sourceRevision: item.sourceRevision,
                source: source
            )
            wire["resolvedSource"] = [
                "sourceReference": reference,
                "cacheIdentity": cacheIdentity.wire,
                "expiresAtEpochMs": item.expiresAtEpochMs as Any,
            ]
        }
        return wire
    }

    private func execute(_ command: [String: Any]) throws -> [String: Any] {
        try checkActive()
        let commandData = try JSONSerialization.data(withJSONObject: command)
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
        try drainEvents()
        try applyActiveSource()
        try pumpPreloadIntents()
    }

    private func pumpPreloadIntents() throws {
        guard let warmupExecutor, !isDisposed else { return }
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
        warmupExecutor.reconcile(intents: rawIntents) { [weak self] sourceReference, itemId, sourceRevision in
            guard let self,
                  let entry = self.sourceRegistry[sourceReference],
                  entry.itemId == itemId,
                  entry.sourceRevision == sourceRevision else { return nil }
            return entry.source
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
            droppedEvents: value["droppedEvents"] as? UInt64 ?? 0
        )
    }

    private func applyActiveSource() throws {
        guard let active = snapshot.items.first(where: { $0.isActive }),
              let reference = active.sourceReference,
              let target = controller,
              let entry = sourceRegistry[reference] else { return }
        guard entry.itemId == active.itemId, entry.sourceRevision == active.sourceRevision else {
            throw sequenceError("stale_source_registry_entry")
        }
        let activation = AppliedActivation(
            itemId: active.itemId,
            sourceRevision: active.sourceRevision,
            activationEpoch: snapshot.activationEpoch
        )
        guard activation != appliedActivation else { return }
        try target.activateSequenceSource(self, source: entry.source)
        appliedActivation = activation
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

private extension VesperPlaybackSequenceCacheIdentity {
    var wire: [String: Any] {
        [
            "providerNamespace": providerNamespace,
            "contentIdentity": contentIdentity,
            "renditionIdentity": renditionIdentity,
            "resourceIdentity": resourceIdentity,
            "accessPartition": accessPartition,
            "sourceRevision": sourceRevision,
        ]
    }
}
