import CryptoKit
import Foundation

struct VesperDashStartupScope: Equatable, Sendable {
    let namespace: String
    let owner: String
    let sourceExpiresAtMs: UInt64?
    let ttlMs: UInt64
    init(namespace: String = UUID().uuidString, owner: String? = nil, sourceExpiresAtMs: UInt64? = nil, ttlMs: UInt64 = 30_000) {
        self.namespace = namespace
        self.owner = owner ?? namespace
        self.sourceExpiresAtMs = sourceExpiresAtMs
        self.ttlMs = min(max(ttlMs, 1), 30_000)
    }
}

struct VesperDashStartupResource: Equatable {
    let url: URL
    var range: VesperDashByteRange? = nil
}

struct VesperDashStartupBytes {
    let resource: VesperDashStartupResource
    let data: Data
    let finalURL: URL
}

actor VesperDashStartupCache {
    static let shared = VesperDashStartupCache()
    static let maxResourceBytes = 8 * 1024 * 1024
    static let maxWarmupBytes = 16 * 1024 * 1024
    static let maxCacheBytes = 32 * 1024 * 1024
    private struct Entry {
        let key: String
        let owner: String
        let value: VesperDashStartupBytes
        let expiresAtMs: UInt64
        let order: UInt64
    }
    private var entries: [String: Entry] = [:]
    private var bytes = 0
    private var order: UInt64 = 0
    private var generation: UInt64 = 0
    private let nowMs: @Sendable () -> UInt64
    private let wallMs: @Sendable () -> UInt64
    init(nowMs: @escaping @Sendable () -> UInt64 = { DispatchTime.now().uptimeNanoseconds / 1_000_000 },
         wallMs: @escaping @Sendable () -> UInt64 = { UInt64(max(0, Date().timeIntervalSince1970 * 1_000)) }) {
        self.nowMs = nowMs
        self.wallMs = wallMs
    }
    func currentGeneration() -> UInt64 { generation }
    func clear() { entries.removeAll(); bytes = 0; generation &+= 1 }
    func inventory() -> (entries: Int, bytes: UInt64) { expire(); return (entries.count, UInt64(bytes)) }

    func read(scope: VesperDashStartupScope, resource: VesperDashStartupResource, headers: [String: String]) -> VesperDashStartupBytes? {
        expire()
        if let expiry = scope.sourceExpiresAtMs, expiry <= wallMs() { return nil }
        let key = resourceKey(scope: scope, url: resource.url, headers: headers)
        let matching = entries.values.filter { $0.key == key }.sorted { ($0.value.resource.range?.start ?? 0) < ($1.value.resource.range?.start ?? 0) }
        let start = resource.range?.start ?? 0
        if let range = resource.range, range.end < range.start || range.end - range.start >= UInt64(Self.maxResourceBytes) { return nil }
        let length = resource.range?.length ?? matching.first(where: { $0.value.resource.range == nil }).map { UInt64($0.value.data.count) }
        guard let length, length > 0, length <= Self.maxResourceBytes, start <= UInt64.max - length else { return nil }
        let end = start + length
        var cursor = start
        var output = Data()
        var finalURL = resource.url
        for entry in matching {
            let value = entry.value
            let offset = value.resource.range?.start ?? 0
            let entryEnd = offset + UInt64(value.data.count)
            if offset > cursor { break }
            if entryEnd <= cursor { continue }
            let count = min(end, entryEnd) - cursor
            output.append(value.data.subdata(in: Int(cursor - offset)..<Int(cursor - offset + count)))
            cursor += count
            finalURL = value.finalURL
            if cursor == end { return .init(resource: resource, data: output, finalURL: finalURL) }
        }
        return nil
    }

    func store(scope: VesperDashStartupScope, values: [VesperDashStartupBytes], headers: [String: String], expectedGeneration: UInt64,
               maximumBytes: Int = maxWarmupBytes) throws -> Bool {
        try Task.checkCancellation()
        guard expectedGeneration == generation else { return false }
        let total = values.reduce(0, { $0 + $1.data.count })
        let budget = min(maximumBytes, Self.maxWarmupBytes)
        guard values.count <= 64, total <= budget,
              values.allSatisfy({ !$0.data.isEmpty && $0.data.count <= Self.maxResourceBytes }) else {
            throw VesperDashBridgeError.network("DASH startup cache budget exceeded")
        }
        expire()
        let wall = wallMs()
        if let expiry = scope.sourceExpiresAtMs, expiry <= wall { return false }
        let ttl = min(scope.ttlMs, scope.sourceExpiresAtMs.map { $0 - wall } ?? scope.ttlMs)
        let expires = nowMs() + ttl
        for value in values {
            if let range = value.resource.range {
                guard range.end >= range.start, range.end - range.start < UInt64(Self.maxResourceBytes),
                      range.length == UInt64(value.data.count), range.end < UInt64.max else {
                    throw VesperDashBridgeError.network("Invalid DASH startup cache entry range")
                }
            }
        }
        // The budget belongs to the sequence, across all of its accepted source revisions.
        while entries.values.filter({ $0.owner == scope.owner }).reduce(0, { $0 + $1.value.data.count }) + total > budget {
            guard let oldest = entries.filter({ $0.value.owner == scope.owner }).min(by: { $0.value.order < $1.value.order })?.key else { break }
            remove(oldest)
        }
        for value in values {
            let key = resourceKey(scope: scope, url: value.resource.url, headers: headers)
            let entryKey = key + ":\(value.resource.range?.start ?? 0):\(value.resource.range?.length.description ?? "full")"
            remove(entryKey)
            while !entries.isEmpty && (entries.count >= 64 || bytes + value.data.count > Self.maxCacheBytes) {
                if let oldest = entries.min(by: { $0.value.order < $1.value.order })?.key { remove(oldest) }
            }
            order &+= 1
            entries[entryKey] = Entry(key: key, owner: scope.owner, value: value, expiresAtMs: expires, order: order)
            bytes += value.data.count
        }
        return true
    }
    private func remove(_ key: String) { bytes -= entries.removeValue(forKey: key)?.value.data.count ?? 0 }
    private func expire() {
        let now = nowMs()
        entries.filter { $0.value.expiresAtMs <= now }.map(\.key).forEach(remove)
    }
    private func resourceKey(scope: VesperDashStartupScope, url: URL, headers: [String: String]) -> String {
        var hash = SHA256()
        func field(_ value: String) {
            let data = Data(value.utf8)
            hash.update(data: Data("\(data.count):".utf8))
            hash.update(data: data)
        }
        field(scope.namespace)
        field(url.absoluteString)
        headers.sorted { ($0.key.lowercased(), $0.value) < ($1.key.lowercased(), $1.value) }.forEach {
            field($0.key.lowercased()); field($0.value)
        }
        return hash.finalize().map { String(format: "%02x", $0) }.joined()
    }
}
