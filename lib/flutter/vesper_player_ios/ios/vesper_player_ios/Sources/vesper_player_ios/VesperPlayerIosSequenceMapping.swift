import Foundation
@_spi(VesperFlutter) import VesperPlayerKit

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
        let historyLimit = try sourceInt(self, "historyLimit", 16)
        let forwardWindow = try sourceInt(self, "forwardWindow", 1)
        let refillThreshold = try sourceInt(self, "refillThreshold", 1)
        let maxItems = try sourceInt(self, "maxItems", 512)
        let maxPendingRequests =
            try sourceInt(self, "maxPendingRequests", 32)
        let maxEvents = try sourceInt(self, "maxEvents", 512)
        let maxSourceRegistryEntries =
            try sourceInt(self, "maxSourceRegistryEntries", 1_024)
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
            requestTimeoutMs: try sourceUInt(self, "requestTimeoutMs", 15_000),
            sourceExpiryLeadMs: try sourceUInt(self, "sourceExpiryLeadMs", 15_000),
            maxSourceRegistryEntries: maxSourceRegistryEntries
        )
    }

    @MainActor
    func toPlaybackSequenceItem(resolve: ([String: Any]) throws -> VesperSourceHandle) throws -> VesperPlaybackSequenceItem {
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
        let source = try nestedMap(self["source"]).map(resolve)
        return VesperPlaybackSequenceItem(
            itemId: itemId,
            contentIdentity: VesperPlaybackSequenceContentIdentity(
                providerNamespace: providerNamespace,
                value: contentIdentity
            ),
            mediaKind: mediaKind,
            source: source,
            providerMetadataRef: self["providerMetadataRef"] as? String,
            preloadProfile: try nestedMap(self["preloadProfile"])?
                .toPlaybackSequencePreloadProfile() ?? VesperPlaybackSequencePreloadProfile()
        )
    }

    @MainActor
    func toPlaybackSequenceResolvedSource(resolve: ([String: Any]) throws -> VesperSourceHandle) throws -> VesperPlaybackSequenceResolvedSource {
        let sessionGeneration =
            try sourceUInt(self, "sessionGeneration", 0)
        let requestId = try sourceUInt(self, "requestId", 0)
        let resolutionAttemptId =
            try sourceUInt(self, "resolutionAttemptId", 0)
        let itemId = self["itemId"] as? String ?? ""
        let expectedSourceRevision =
            try sourceUInt(self, "expectedSourceRevision", 0)
        guard sessionGeneration > 0,
              requestId > 0,
              resolutionAttemptId > 0,
              !itemId.isEmpty
        else {
            throw PluginError.operationFailed("Invalid resolved sequence source.")
        }
        return VesperPlaybackSequenceResolvedSource(
            sessionGeneration: sessionGeneration,
            requestId: requestId,
            resolutionAttemptId: resolutionAttemptId,
            itemId: itemId,
            expectedSourceRevision: expectedSourceRevision,
            source: try resolve(requireNestedMap(arguments: self, key: "source"))
        )
    }

    func toPlaybackSequencePreloadProfile() throws -> VesperPlaybackSequencePreloadProfile {
        VesperPlaybackSequencePreloadProfile(
            expectedMemoryBytes: try sourceUInt(self, "expectedMemoryBytes", 0),
            expectedDiskBytes: try sourceUInt(self, "expectedDiskBytes", 0),
            ttlMs: try optionalSequenceUInt(self, "ttlMs"),
            warmupWindowMs: try optionalSequenceUInt(self, "warmupWindowMs")
        )
    }


}

extension Dictionary where Key == String, Value == Any {
    @MainActor
    func sequenceItems(resolve: ([String: Any]) throws -> VesperSourceHandle) throws -> [VesperPlaybackSequenceItem] {
        guard let values = self["items"] as? [Any] else { return [] }
        return try values.map { value in
            guard let map = stringKeyedMap(value) else {
                throw PluginError.operationFailed("Invalid sequence item.")
            }
            return try map.toPlaybackSequenceItem(resolve: resolve)
        }
    }
}

private func optionalSequenceUInt(_ value: [String: Any], _ key: String) throws -> UInt64? {
    guard value[key] != nil, !(value[key] is NSNull) else { return nil }
    return try sourceUInt(value, key, 0)
}
