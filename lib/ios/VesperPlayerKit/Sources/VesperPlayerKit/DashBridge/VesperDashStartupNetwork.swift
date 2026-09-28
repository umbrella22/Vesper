import Foundation

enum VesperDashStartupError: Error {
    case httpStatus(Int)
    case invalidResponse
    case budgetExceeded
    case invalidated
}

protocol VesperDashStartupTransport {
    func fetch(_ resource: VesperDashStartupResource, headers: [String: String], maximumBytes: Int) async throws -> VesperDashStartupBytes
}

/// Startup ranges must be complete and unencoded before they can enter the shared cache.
struct VesperDashStartupHTTPTransport: VesperDashStartupTransport {
    func fetch(_ resource: VesperDashStartupResource, headers: [String: String], maximumBytes: Int) async throws -> VesperDashStartupBytes {
        guard resource.url.scheme?.lowercased() == "https", resource.url.user == nil,
              headers.keys.allSatisfy({ $0.caseInsensitiveCompare("Range") != .orderedSame }),
              maximumBytes > 0, maximumBytes <= VesperDashStartupCache.maxResourceBytes else {
            throw VesperDashStartupError.invalidResponse
        }
        var request = URLRequest(url: resource.url)
        request.timeoutInterval = 5
        applyHttpHeaders(headers, to: &request)
        request.setValue("identity", forHTTPHeaderField: "Accept-Encoding")
        if let range = resource.range {
            guard range.end >= range.start, range.end - range.start < UInt64(maximumBytes) else {
                throw VesperDashStartupError.budgetExceeded
            }
            request.setValue("bytes=\(range.start)-\(range.end)", forHTTPHeaderField: "Range")
        }
        let config = URLSessionConfiguration.ephemeral
        config.urlCache = nil
        config.timeoutIntervalForRequest = 5
        config.timeoutIntervalForResource = 10
        let redirectPolicy = VesperDashStartupRedirectPolicy(hasHeaders: !headers.isEmpty)
        let session = URLSession(configuration: config, delegate: redirectPolicy, delegateQueue: nil)
        defer { session.invalidateAndCancel() }
        let (stream, response) = try await session.bytes(for: request)
        guard let response = response as? HTTPURLResponse, let finalURL = response.url else {
            throw VesperDashStartupError.invalidResponse
        }
        try Self.validate(response, resource: resource, maximumBytes: maximumBytes)
        var data = Data()
        for try await byte in stream {
            try Task.checkCancellation()
            guard data.count < maximumBytes else { throw VesperDashStartupError.budgetExceeded }
            data.append(byte)
        }
        try Task.checkCancellation()
        guard !data.isEmpty,
              response.expectedContentLength < 0 || response.expectedContentLength == data.count,
              resource.range == nil || resource.range?.length == UInt64(data.count) else {
            throw VesperDashStartupError.invalidResponse
        }
        return .init(resource: resource, data: data, finalURL: finalURL)
    }

    static func validate(_ response: HTTPURLResponse, resource: VesperDashStartupResource, maximumBytes: Int) throws {
        guard (200..<300).contains(response.statusCode) else { throw VesperDashStartupError.httpStatus(response.statusCode) }
        guard response.expectedContentLength <= maximumBytes,
              response.value(forHTTPHeaderField: "Content-Encoding").map({ $0.lowercased() == "identity" }) ?? true else {
            throw VesperDashStartupError.invalidResponse
        }
        if let range = resource.range {
            guard range.end >= range.start, range.end - range.start < UInt64(maximumBytes),
                  response.statusCode == 206,
                  let contentRange = response.value(forHTTPHeaderField: "Content-Range") else {
                throw VesperDashStartupError.invalidResponse
            }
            let parts = contentRange.split(separator: "/", omittingEmptySubsequences: false)
            guard parts.count == 2, parts[0] == "bytes \(range.start)-\(range.end)",
                  parts[1] == "*" || UInt64(parts[1]).map({ $0 > range.end }) == true else {
                throw VesperDashStartupError.invalidResponse
            }
        } else if response.statusCode != 200 {
            throw VesperDashStartupError.invalidResponse
        }
    }
}

private final class VesperDashStartupRedirectPolicy: NSObject, URLSessionTaskDelegate {
    let hasHeaders: Bool
    private var redirects = 0
    init(hasHeaders: Bool) { self.hasHeaders = hasHeaders }
    func urlSession(_ session: URLSession, task: URLSessionTask, willPerformHTTPRedirection response: HTTPURLResponse,
                    newRequest request: URLRequest, completionHandler: @escaping (URLRequest?) -> Void) {
        redirects += 1
        guard redirects <= 4, let from = response.url, let to = request.url,
              to.scheme?.lowercased() == "https", to.user == nil,
              !hasHeaders || (from.host == to.host && from.port == to.port && from.scheme == to.scheme) else {
            completionHandler(nil)
            return
        }
        completionHandler(request)
    }
}

class VesperDashStartupNetworkClient: VesperDashNetworkClient {
    let scope: VesperDashStartupScope
    let startupHeaders: [String: String]
    let cache: VesperDashStartupCache
    let transport: any VesperDashStartupTransport

    init(scope: VesperDashStartupScope, headers: [String: String], cache: VesperDashStartupCache = .shared,
         transport: any VesperDashStartupTransport = VesperDashStartupHTTPTransport()) {
        self.scope = scope
        startupHeaders = headers
        self.cache = cache
        self.transport = transport
        super.init(headers: headers)
    }

    func cached(_ resource: VesperDashStartupResource) async -> VesperDashStartupBytes? {
        await cache.read(scope: scope, resource: resource, headers: startupHeaders)
    }

    func load(_ resource: VesperDashStartupResource, maximumBytes: Int = VesperDashStartupCache.maxResourceBytes) async throws -> VesperDashStartupBytes {
        try Task.checkCancellation()
        if let value = await cached(resource) { return value }
        return try await transport.fetch(resource, headers: startupHeaders, maximumBytes: maximumBytes)
    }

    override func manifestData(for url: URL) async throws -> (Data, URL) {
        if let value = await cached(.init(url: url)) { return (value.data, value.finalURL) }
        return try await super.dataWithFinalURL(for: url)
    }

    override func data(for url: URL, byteRange: VesperDashByteRange? = nil) async throws -> Data {
        if let value = await cached(.init(url: url, range: byteRange)) { return value.data }
        return try await super.data(for: url, byteRange: byteRange)
    }
}

func vesperWarmDashStartup(source: VesperPlayerSource, scope: VesperDashStartupScope,
                          cache: VesperDashStartupCache = .shared,
                          transport: any VesperDashStartupTransport = VesperDashStartupHTTPTransport(),
                          maximumBytes: Int = VesperDashStartupCache.maxWarmupBytes,
                          capability: VesperDashSession.VideoDecodeCapabilityProvider = { VesperDashSession.defaultVideoDecodeCapability(for: $0) }) async throws -> (bytes: UInt64, hit: Bool) {
    guard source.drmConfiguration == nil, let url = URL(string: source.uri) else { throw VesperDashStartupError.invalidResponse }
    let generation = await cache.currentGeneration()
    let client = VesperDashStartupNetworkClient(scope: scope, headers: source.headers, cache: cache, transport: transport)
    var staged: [VesperDashStartupBytes] = []
    var total = 0
    var hit = true
    func load(_ resource: VesperDashStartupResource, limit: Int) async throws -> VesperDashStartupBytes {
        try Task.checkCancellation()
        let maximum = min(limit, min(maximumBytes, VesperDashStartupCache.maxWarmupBytes) - total)
        guard maximum > 0 else { throw VesperDashStartupError.budgetExceeded }
        let cached = await client.cached(resource)
        let value: VesperDashStartupBytes
        if let cached { value = cached } else {
            hit = false
            value = try await transport.fetch(resource, headers: source.headers, maximumBytes: maximum)
        }
        guard !value.data.isEmpty, value.data.count <= maximum else { throw VesperDashStartupError.budgetExceeded }
        total += value.data.count
        staged.append(value)
        return value
    }
    let mpd = try await load(.init(url: url), limit: 1024 * 1024)
    // The FFI parser is intentionally reused so warmup and formal playback select the same representations.
    let manifest = try VesperDashManifestParser.parse(data: mpd.data, manifestURL: mpd.finalURL)
    guard manifest.type == .static, manifest.periods.count == 1 else {
        throw VesperDashBridgeError.unsupportedManifest("startup caching requires a static single-period manifest")
    }
    let all = try VesperDashHlsBuilder.selectedPlayableRepresentations(manifest: manifest, variantPolicy: .all, videoDecodeCapabilities: nil)
    let selected = try VesperDashHlsBuilder.selectedPlayableRepresentations(manifest: manifest, variantPolicy: .startupSingleVariant,
                                                                          videoDecodeCapabilities: all.video.map(capability))
    let representations = selected.video + selected.audio
    guard !representations.isEmpty, representations.count <= 2 else { throw VesperDashStartupError.budgetExceeded }
    for playable in representations {
        guard let base = playable.representation.segmentBase, let mediaURL = URL(string: playable.representation.baseURL) else {
            throw VesperDashBridgeError.unsupportedManifest("startup caching requires SegmentBase")
        }
        let index = try await load(.init(url: mediaURL, range: base.indexRange), limit: 1024 * 1024)
        let sidx = try VesperDashSidxParser.parse(data: index.data)
        let segments = try VesperDashHlsBuilder.mediaSegments(segmentBase: base, sidx: sidx)
        guard let first = segments.first else { throw VesperDashStartupError.invalidResponse }
        _ = try await load(.init(url: mediaURL, range: base.initialization), limit: 1024 * 1024)
        _ = try await load(.init(url: mediaURL, range: first.range), limit: VesperDashStartupCache.maxResourceBytes)
    }
    try Task.checkCancellation()
    guard try await cache.store(scope: scope, values: staged, headers: source.headers, expectedGeneration: generation, maximumBytes: maximumBytes) else {
        throw VesperDashStartupError.invalidated
    }
    return (UInt64(total), hit)
}
