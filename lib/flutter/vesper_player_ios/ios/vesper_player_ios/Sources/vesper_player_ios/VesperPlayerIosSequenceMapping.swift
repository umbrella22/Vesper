import Foundation
import VesperPlayerKit

extension Dictionary where Key == String, Value == Any {
    func toPlaybackSequenceConfiguration() throws -> VesperPlaybackSequenceConfiguration {
        guard let sequenceId = self["sequenceId"] as? String, !sequenceId.isEmpty else {
            throw PluginError.missingArgument("sequenceId")
        }
        let mode: VesperPlaybackSequenceMode
        switch self["mode"] as? String {
        case "finite", nil:
            mode = .finite
        case "replenishable":
            mode = .replenishable
        default:
            throw PluginError.operationFailed("Unknown sequence mode.")
        }
        let historyLimit = (self["historyLimit"] as? NSNumber)?.intValue ?? 16
        let forwardWindow = (self["forwardWindow"] as? NSNumber)?.intValue ?? 1
        let refillThreshold = (self["refillThreshold"] as? NSNumber)?.intValue ?? 1
        let maxItems = (self["maxItems"] as? NSNumber)?.intValue ?? 512
        let maxPendingRequests =
            (self["maxPendingRequests"] as? NSNumber)?.intValue ?? 32
        let maxEvents = (self["maxEvents"] as? NSNumber)?.intValue ?? 512
        let maxSourceRegistryEntries =
            (self["maxSourceRegistryEntries"] as? NSNumber)?.intValue ?? 1_024
        guard historyLimit >= 0,
              forwardWindow >= 0,
              refillThreshold >= 0,
              (1...512).contains(maxItems),
              (1...512).contains(maxPendingRequests),
              (1...1_024).contains(maxEvents),
              (maxItems...4_096).contains(maxSourceRegistryEntries)
        else {
            throw PluginError.operationFailed("Invalid sequence capacity.")
        }
        return VesperPlaybackSequenceConfiguration(
            sequenceId: sequenceId,
            mode: mode,
            historyLimit: historyLimit,
            forwardWindow: forwardWindow,
            refillThreshold: refillThreshold,
            maxItems: maxItems,
            maxPendingRequests: maxPendingRequests,
            maxEvents: maxEvents,
            requestTimeoutMs: (self["requestTimeoutMs"] as? NSNumber)?.uint64Value ?? 15_000,
            sourceExpiryLeadMs: (self["sourceExpiryLeadMs"] as? NSNumber)?.uint64Value ?? 15_000,
            maxSourceRegistryEntries: maxSourceRegistryEntries
        )
    }

    func toPlaybackSequenceItem() throws -> VesperPlaybackSequenceItem {
        guard let itemId = self["itemId"] as? String, !itemId.isEmpty else {
            throw PluginError.missingArgument("itemId")
        }
        guard let providerNamespace = self["providerNamespace"] as? String,
              !providerNamespace.isEmpty,
              let contentIdentity = self["contentIdentity"] as? String,
              !contentIdentity.isEmpty
        else { throw PluginError.operationFailed("Missing sequence content identity.") }
        let mediaKind: VesperPlaybackSequenceMediaKind
        switch self["mediaKind"] as? String {
        case "vod", nil: mediaKind = .vod
        case "live": mediaKind = .live
        case "liveDvr": mediaKind = .liveDvr
        default: throw PluginError.operationFailed("Unknown sequence media kind.")
        }
        let source = try nestedMap(self["source"])?.toVesperPlayerSource()
        let cache = try nestedMap(self["cacheIdentity"])?.toPlaybackSequenceCacheIdentity()
        guard (source == nil) == (cache == nil) else {
            throw PluginError.operationFailed("Sequence source/cache identity mismatch.")
        }
        let revision = (self["sourceRevision"] as? NSNumber)?.uint64Value ?? cache?.sourceRevision ?? 0
        if let cache {
            guard revision > 0, cache.sourceRevision == revision else {
                throw PluginError.operationFailed("Invalid sequence source revision.")
            }
        }
        return VesperPlaybackSequenceItem(
            itemId: itemId,
            contentIdentity: VesperPlaybackSequenceContentIdentity(
                providerNamespace: providerNamespace,
                value: contentIdentity
            ),
            mediaKind: mediaKind,
            source: source,
            cacheIdentity: cache,
            sourceRevision: revision,
            expiresAtEpochMs: (self["expiresAtEpochMs"] as? NSNumber)?.uint64Value,
            providerMetadataRef: self["providerMetadataRef"] as? String,
            preloadProfile: try nestedMap(self["preloadProfile"])?
                .toPlaybackSequencePreloadProfile() ?? VesperPlaybackSequencePreloadProfile()
        )
    }

    func toPlaybackSequenceResolvedSource() throws -> VesperPlaybackSequenceResolvedSource {
        let cache = try requireNestedMap(arguments: self, key: "cacheIdentity")
            .toPlaybackSequenceCacheIdentity()
        let sessionGeneration =
            (self["sessionGeneration"] as? NSNumber)?.uint64Value ?? 0
        let requestId = (self["requestId"] as? NSNumber)?.uint64Value ?? 0
        let resolutionAttemptId =
            (self["resolutionAttemptId"] as? NSNumber)?.uint64Value ?? 0
        let itemId = self["itemId"] as? String ?? ""
        let expectedSourceRevision =
            (self["expectedSourceRevision"] as? NSNumber)?.uint64Value ?? 0
        let sourceRevision =
            (self["sourceRevision"] as? NSNumber)?.uint64Value ?? 0
        guard sessionGeneration > 0,
              requestId > 0,
              resolutionAttemptId > 0,
              !itemId.isEmpty,
              sourceRevision > expectedSourceRevision,
              cache.sourceRevision == sourceRevision
        else {
            throw PluginError.operationFailed("Invalid resolved sequence source.")
        }
        return VesperPlaybackSequenceResolvedSource(
            sessionGeneration: sessionGeneration,
            requestId: requestId,
            resolutionAttemptId: resolutionAttemptId,
            itemId: itemId,
            expectedSourceRevision: expectedSourceRevision,
            sourceRevision: sourceRevision,
            source: try requireNestedMap(arguments: self, key: "source").toVesperPlayerSource(),
            cacheIdentity: cache,
            expiresAtEpochMs: (self["expiresAtEpochMs"] as? NSNumber)?.uint64Value
        )
    }

    func toPlaybackSequencePreloadProfile() -> VesperPlaybackSequencePreloadProfile {
        VesperPlaybackSequencePreloadProfile(
            expectedMemoryBytes: (self["expectedMemoryBytes"] as? NSNumber)?.uint64Value ?? 0,
            expectedDiskBytes: (self["expectedDiskBytes"] as? NSNumber)?.uint64Value ?? 0,
            ttlMs: (self["ttlMs"] as? NSNumber)?.uint64Value,
            warmupWindowMs: (self["warmupWindowMs"] as? NSNumber)?.uint64Value
        )
    }

    func toPlaybackSequenceCacheIdentity() -> VesperPlaybackSequenceCacheIdentity {
        VesperPlaybackSequenceCacheIdentity(
            providerNamespace: self["providerNamespace"] as? String ?? "",
            contentIdentity: self["contentIdentity"] as? String ?? "",
            renditionIdentity: self["renditionIdentity"] as? String ?? "",
            resourceIdentity: self["resourceIdentity"] as? String ?? "",
            accessPartition: self["accessPartition"] as? String ?? "",
            sourceRevision: (self["sourceRevision"] as? NSNumber)?.uint64Value ?? 0
        )
    }
}

extension Dictionary where Key == String, Value == Any {
    func sequenceItems() throws -> [VesperPlaybackSequenceItem] {
        guard let values = self["items"] as? [Any] else { return [] }
        return try values.map { value in
            guard let map = stringKeyedMap(value) else {
                throw PluginError.operationFailed("Invalid sequence item.")
            }
            return try map.toPlaybackSequenceItem()
        }
    }
}
