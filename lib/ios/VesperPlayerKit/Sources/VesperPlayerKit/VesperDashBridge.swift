@preconcurrency import AVFoundation
import Foundation
@preconcurrency import Network
import VesperPlayerKitBridgeShim

private struct VesperDashSegmentCacheKey: Hashable {
    let renditionId: String
    let segment: VesperDashSegmentRequest
}

private struct VesperDashCachedSegmentFile {
    let url: URL
    let size: UInt64
    var lastAccessedAt: Date

    var isInitialization: Bool {
        segment == .initialization
    }

    private let segment: VesperDashSegmentRequest

    init(url: URL, size: UInt64, segment: VesperDashSegmentRequest, lastAccessedAt: Date) {
        self.url = url
        self.size = size
        self.segment = segment
        self.lastAccessedAt = lastAccessedAt
    }
}

private enum VesperDashResourceResponse {
    case data(Data, contentType: String)
    case redirect(URL)
}

fileprivate enum VesperDashSegmentPayload {
    case data(Data, contentType: String)
    case file(url: URL, offset: UInt64, size: UInt64, removeAfterServing: Bool, contentType: String)

    var size: UInt64 {
        switch self {
        case let .data(data, _):
            return UInt64(data.count)
        case let .file(_, _, size, _, _):
            return size
        }
    }

    var contentType: String {
        switch self {
        case let .data(_, contentType):
            return contentType
        case let .file(_, _, _, _, contentType):
            return contentType
        }
    }

    var isTemporaryFile: Bool {
        if case .file(_, _, _, true, _) = self {
            return true
        }
        return false
    }

    func readData() throws -> Data {
        switch self {
        case let .data(data, _):
            return data
        case let .file(url, offset, size, removeAfterServing, _):
            defer {
                if removeAfterServing {
                    try? FileManager.default.removeItem(at: url)
                }
            }
            let length = try checkedInt(size, field: "segment payload length")
            let handle = try FileHandle(forReadingFrom: url)
            defer { try? handle.close() }
            try handle.seek(toOffset: offset)
            let data = try handle.read(upToCount: length) ?? Data()
            guard data.count == length else {
                throw VesperDashBridgeError.network("segment file is shorter than requested")
            }
            return data
        }
    }

    func cleanupIfTemporary() {
        if case let .file(url, _, _, true, _) = self {
            try? FileManager.default.removeItem(at: url)
        }
    }
}

private struct VesperDashSegmentPayloadResult {
    let payload: VesperDashSegmentPayload
    let cacheHit: Bool
    let segmentType: String
    let byteRange: VesperDashByteRange?
    let delivery: String
}

final class VesperDashResourceLoaderDelegate: NSObject, AVAssetResourceLoaderDelegate {
    let resourceLoadingQueue: DispatchQueue

    private let session: VesperDashSession
    private var tasks: [ObjectIdentifier: Task<Void, Never>] = [:]

    init(session: VesperDashSession) {
        self.session = session
        resourceLoadingQueue = DispatchQueue(
            label: "io.github.ikaros.vesper.player.dash.resource-loader.\(session.id)"
        )
        super.init()
    }

    func resourceLoader(
        _ resourceLoader: AVAssetResourceLoader,
        shouldWaitForLoadingOfRequestedResource loadingRequest: AVAssetResourceLoadingRequest
    ) -> Bool {
        guard
            let url = loadingRequest.request.url,
            let route = session.route(for: url)
        else {
            return false
        }

        let requestId = ObjectIdentifier(loadingRequest)
        let task = Task { [weak self, session, loadingRequest] in
            do {
                let response: VesperDashResourceResponse
                switch route {
                case .master:
                    response = .data(
                        try await session.masterPlaylistData(),
                        contentType: "public.m3u-playlist"
                    )
                case let .media(renditionId):
                    response = .data(
                        try await session.mediaPlaylistData(renditionId: renditionId),
                        contentType: "public.m3u-playlist"
                    )
                case let .segment(renditionId, segment):
                    switch segment {
                    case .initialization:
                        // Init 段体积很小（~1KB）且 AVPlayer 只取一次，直接以
                        // 原始字节返回，不走 loopback。这使得可以准确记录“AVPlayer 是
                        // 否拉取了 init”这个关键事实（loopback 路径中 AVPlayer
                        // 不会请求 EXT-X-MAP 指向的走 http 的 URL，原因未明）。
                        let initData = try await session.segmentData(
                            renditionId: renditionId,
                            segment: .initialization
                        )
#if DEBUG
                        iosHostLog(
                            "dashResourceInit rendition=\(renditionId) bytes=\(initData.count)"
                        )
#endif
                        // contentType 必须是 UTI，不是 MIME。fMP4/ISO BMFF 对应 public.mpeg-4。
                        response = .data(initData, contentType: "public.mpeg-4")
                    case .media:
                        response = .redirect(
                            try await session.segmentRedirectURL(renditionId: renditionId, segment: segment)
                        )
                    }
                }
                self?.finish(loadingRequest, requestId: requestId, response: response)
            } catch {
                self?.finish(loadingRequest, requestId: requestId, error: error)
            }
        }
        tasks[requestId] = task
        return true
    }

    func resourceLoader(
        _ resourceLoader: AVAssetResourceLoader,
        didCancel loadingRequest: AVAssetResourceLoadingRequest
    ) {
        let requestId = ObjectIdentifier(loadingRequest)
        tasks.removeValue(forKey: requestId)?.cancel()
    }

    private func finish(
        _ loadingRequest: AVAssetResourceLoadingRequest,
        requestId: ObjectIdentifier,
        response: VesperDashResourceResponse
    ) {
        resourceLoadingQueue.async { [weak self] in
            guard let self else { return }
            self.tasks.removeValue(forKey: requestId)

            switch response {
            case let .data(data, contentType):
                loadingRequest.contentInformationRequest?.contentType = contentType
                loadingRequest.contentInformationRequest?.contentLength = Int64(data.count)
                loadingRequest.contentInformationRequest?.isByteRangeAccessSupported = true
                if let dataRequest = loadingRequest.dataRequest {
                    do {
                        try self.respond(to: dataRequest, with: data)
                    } catch {
                        loadingRequest.finishLoading(with: error)
                        return
                    }
                }
                loadingRequest.finishLoading()
            case let .redirect(url):
                var request = URLRequest(url: url)
                request.cachePolicy = .returnCacheDataElseLoad
                loadingRequest.redirect = request
#if DEBUG
                iosHostLog(
                    "dashResourceRedirect from=\(loadingRequest.request.url?.absoluteString ?? "nil") to=\(url.absoluteString)"
                )
#endif
                loadingRequest.response = HTTPURLResponse(
                    url: loadingRequest.request.url ?? url,
                    statusCode: 302,
                    httpVersion: nil,
                    headerFields: ["Location": url.absoluteString]
                )
                loadingRequest.finishLoading()
            }
        }
    }

    private func finish(
        _ loadingRequest: AVAssetResourceLoadingRequest,
        requestId: ObjectIdentifier,
        error: Error
    ) {
        resourceLoadingQueue.async { [weak self] in
            self?.tasks.removeValue(forKey: requestId)
            loadingRequest.finishLoading(with: error)
        }
    }

    private func respond(
        to dataRequest: AVAssetResourceLoadingDataRequest,
        with data: Data
    ) throws {
        let requestedOffset = dataRequest.currentOffset != 0
            ? dataRequest.currentOffset
            : dataRequest.requestedOffset
        guard requestedOffset >= 0 else {
            throw VesperDashBridgeError.invalidManifest("negative playlist byte offset requested")
        }
        let offset = Int(requestedOffset)
        guard offset <= data.count else {
            throw VesperDashBridgeError.invalidManifest("playlist byte offset exceeds response size")
        }
        let remaining = data.count - offset
        let requestedLength = dataRequest.requestedLength > 0
            ? min(dataRequest.requestedLength, remaining)
            : remaining
        guard requestedLength >= 0 else {
            throw VesperDashBridgeError.invalidManifest("negative playlist byte length requested")
        }
        dataRequest.respond(with: data.subdata(in: offset..<(offset + requestedLength)))
    }
}

private final class VesperDashLoopbackStartGate: @unchecked Sendable {
    private let lock = NSLock()
    private var didResume = false

    func resumeOnce(_ body: () -> Void) {
        lock.lock()
        defer { lock.unlock() }
        guard !didResume else { return }
        didResume = true
        body()
    }
}

fileprivate final class VesperDashLoopbackServer: @unchecked Sendable {
    fileprivate typealias SegmentPayloadProvider = @Sendable (String, VesperDashSegmentRequest) async throws -> VesperDashSegmentPayload
    private static let fileChunkSize = 256 * 1024

    private let sessionId: String
    private let listener: NWListener
    private let queue: DispatchQueue
    private let segmentPayloadProvider: SegmentPayloadProvider
    private var port: UInt16?
    private var didStart = false

    fileprivate init(
        sessionId: String,
        segmentPayloadProvider: @escaping SegmentPayloadProvider
    ) throws {
        let parameters = NWParameters.tcp
        parameters.requiredLocalEndpoint = .hostPort(
            host: .ipv4(IPv4Address("127.0.0.1")!),
            port: 0
        )
        listener = try NWListener(using: parameters)
        queue = DispatchQueue(label: "io.github.ikaros.vesper.player.dash.loopback.\(sessionId)")
        self.sessionId = sessionId
        self.segmentPayloadProvider = segmentPayloadProvider
        listener.newConnectionHandler = { [weak self] connection in
            self?.handle(connection: connection)
        }
    }

    deinit {
        listener.cancel()
    }

    func start() async throws {
        guard !didStart else { return }
        try await withCheckedThrowingContinuation { continuation in
            let startGate = VesperDashLoopbackStartGate()
            listener.stateUpdateHandler = { [weak self] state in
                guard let self else { return }
                switch state {
                case .ready:
                    self.port = self.listener.port?.rawValue
                    startGate.resumeOnce {
                        continuation.resume()
                    }
                case let .failed(error):
                    startGate.resumeOnce {
                        continuation.resume(throwing: error)
                    }
                case .cancelled:
                    startGate.resumeOnce {
                        continuation.resume(
                            throwing: VesperDashBridgeError.network("DASH loopback server was cancelled")
                        )
                    }
                default:
                    break
                }
            }
            listener.start(queue: queue)
        }
        didStart = true
    }

    func segmentURL(for renditionId: String, segment: VesperDashSegmentRequest) throws -> URL {
        guard let port else {
            throw VesperDashBridgeError.network("DASH loopback server is not ready")
        }
        let encodedId = renditionId.addingPercentEncoding(withAllowedCharacters: dashPathComponentAllowedCharacters)
            ?? renditionId
        let segmentName: String
        switch segment {
        case .initialization:
            segmentName = "init.mp4"
        case let .media(index):
            segmentName = "\(index).m4s"
        }
        return URL(string: "http://127.0.0.1:\(port)/dash/\(sessionId)/\(encodedId)/\(segmentName)")!
    }

    private func handle(connection: NWConnection) {
        connection.start(queue: queue)
        receiveRequest(on: connection, accumulated: Data())
    }

    /// AVPlayer 发送的 HTTP 请求可能分成多个 TCP 包，或一个包里附带了到头部后的多余字节。这里
    /// 不停读取直到看见 "\r\n\r\n" 才手动解析请求。
    private func receiveRequest(on connection: NWConnection, accumulated: Data) {
        connection.receive(minimumIncompleteLength: 1, maximumLength: 32_768) { [weak self] data, _, isComplete, error in
            guard let self else {
                connection.cancel()
                return
            }
            if error != nil {
                connection.cancel()
                return
            }
            var buffer = accumulated
            if let data, !data.isEmpty {
                buffer.append(data)
            }
            if let headerEndRange = buffer.range(of: Data("\r\n\r\n".utf8)) {
                let headerData = buffer.prefix(upTo: headerEndRange.lowerBound)
                guard
                    let headerText = String(data: headerData, encoding: .utf8),
                    let parsed = self.parseRequest(headerText)
                else {
                    let firstLine = String(data: headerData, encoding: .utf8)?
                        .components(separatedBy: "\r\n").first ?? "<unparseable>"
                    iosHostLog("dashLoopbackRequest rejected session=\(self.sessionId) line=\(firstLine)")
                    self.sendStatus(404, reason: "Not Found", on: connection)
                    return
                }
                iosHostLog(
                    "dashLoopbackRequest session=\(self.sessionId) method=\(parsed.method) rendition=\(parsed.renditionId) segment=\(parsed.segment) range=\(parsed.range.map { "\($0.lowerBound)-\($0.upperBound)" } ?? "none")"
                )
                self.sendSegment(parsed, on: connection)
                return
            }
            if isComplete || buffer.count > 64_000 {
                self.sendStatus(400, reason: "Bad Request", on: connection)
                return
            }
            self.receiveRequest(on: connection, accumulated: buffer)
        }
    }

    private struct ParsedRequest {
        let method: HttpMethod
        let renditionId: String
        let segment: VesperDashSegmentRequest
        let range: ClosedRange<UInt64>?
    }

    private enum HttpMethod {
        case get
        case head
    }

    private func parseRequest(_ headerText: String) -> ParsedRequest? {
        let lines = headerText.components(separatedBy: "\r\n")
        guard let requestLine = lines.first else { return nil }
        let parts = requestLine.split(separator: " ")
        guard parts.count >= 2 else { return nil }
        let method: HttpMethod
        switch parts[0].uppercased() {
        case "GET":
            method = .get
        case "HEAD":
            method = .head
        default:
            return nil
        }
        let path = parts[1].split(separator: "?", maxSplits: 1).first.map(String.init) ?? String(parts[1])
        let components = path
            .split(separator: "/")
            .map(String.init)
        guard components.count == 4,
              components[0] == "dash",
              components[1] == sessionId
        else {
            return nil
        }
        let renditionId = components[2].removingPercentEncoding ?? components[2]
        let segmentName = components[3]
        let segment: VesperDashSegmentRequest
        if segmentName == "init.mp4" {
            segment = .initialization
        } else if segmentName.hasSuffix(".m4s"),
                  let index = Int(segmentName.dropLast(".m4s".count)),
                  index >= 0 {
            segment = .media(index)
        } else {
            return nil
        }

        var range: ClosedRange<UInt64>?
        for line in lines.dropFirst() {
            let trimmed = line.trimmingCharacters(in: .whitespaces)
            guard let colon = trimmed.firstIndex(of: ":") else { continue }
            let name = trimmed[..<colon].lowercased()
            guard name == "range" else { continue }
            let value = trimmed[trimmed.index(after: colon)...].trimmingCharacters(in: .whitespaces)
            range = parseRangeHeader(value)
            break
        }
        return ParsedRequest(method: method, renditionId: renditionId, segment: segment, range: range)
    }

    /// 仅支持 `bytes=start-end` 单区间格式（AVPlayer 只会请求这一种）。end 可选。
    private func parseRangeHeader(_ value: String) -> ClosedRange<UInt64>? {
        guard let equals = value.firstIndex(of: "=") else { return nil }
        let unit = value[..<equals].trimmingCharacters(in: .whitespaces).lowercased()
        guard unit == "bytes" else { return nil }
        let spec = value[value.index(after: equals)...]
        guard let dash = spec.firstIndex(of: "-") else { return nil }
        let startText = spec[..<dash].trimmingCharacters(in: .whitespaces)
        let endText = spec[spec.index(after: dash)...].trimmingCharacters(in: .whitespaces)
        guard let start = UInt64(startText) else { return nil }
        if endText.isEmpty {
            return start...UInt64.max
        }
        guard let end = UInt64(endText), end >= start else { return nil }
        return start...end
    }

    private func sendSegment(
        _ request: ParsedRequest,
        on connection: NWConnection
    ) {
        let startedAt = Date()
        Task {
            do {
                let payload = try await self.segmentPayloadProvider(request.renditionId, request.segment)
                let elapsedMs = Int(Date().timeIntervalSince(startedAt) * 1_000)
                self.queue.async {
                    self.sendPayloadResponse(
                        payload,
                        elapsedMs: elapsedMs,
                        request: request,
                        on: connection
                    )
                }
            } catch {
                iosHostLog("dashLoopbackSegment failed rendition=\(request.renditionId) segment=\(request.segment) error=\(error.localizedDescription)")
                self.queue.async {
                    self.sendStatus(502, reason: "Bad Gateway", on: connection)
                }
            }
        }
    }

    private func sendPayloadResponse(
        _ payload: VesperDashSegmentPayload,
        elapsedMs: Int,
        request: ParsedRequest,
        on connection: NWConnection
    ) {
        let totalLength = payload.size
        let bodyStart: UInt64
        let bodyLength: UInt64
        let statusLine: String
        let contentRange: String?
        if let range = request.range {
            if totalLength == 0 || range.lowerBound >= totalLength {
                payload.cleanupIfTemporary()
                let header = "HTTP/1.1 416 Range Not Satisfiable\r\n"
                    + "Content-Range: bytes */\(totalLength)\r\n"
                    + "Content-Length: 0\r\n"
                    + "Connection: close\r\n\r\n"
                connection.send(
                    content: Data(header.utf8),
                    isComplete: true,
                    completion: .contentProcessed { [weak self] _ in
                        self?.scheduleGracefulClose(connection)
                    }
                )
                return
            }
            let start = range.lowerBound
            let end = min(range.upperBound, totalLength - 1)
            bodyStart = start
            bodyLength = end - start + 1
            statusLine = "HTTP/1.1 206 Partial Content\r\n"
            contentRange = "Content-Range: bytes \(start)-\(end)/\(totalLength)\r\n"
        } else {
            bodyStart = 0
            bodyLength = totalLength
            statusLine = "HTTP/1.1 200 OK\r\n"
            contentRange = nil
        }
        var header = statusLine
            + "Content-Type: \(payload.contentType)\r\n"
            + "Content-Length: \(bodyLength)\r\n"
            + "Accept-Ranges: bytes\r\n"
            + "Cache-Control: no-store\r\n"
            + "Connection: close\r\n"
        if let contentRange {
            header += contentRange
        }
        header += "\r\n"
#if DEBUG
        if elapsedMs >= 500 {
            iosHostLog(
                "dashLoopbackSegment served rendition=\(request.renditionId) segment=\(request.segment) method=\(request.method) bytes=\(bodyLength)/\(totalLength) elapsedMs=\(elapsedMs)"
            )
        }
#endif
        // HEAD 不可以附带 body，否则 AVPlayer 会把 body 字节误当下一个响应的一部分。
        guard request.method == .get, bodyLength > 0 else {
            payload.cleanupIfTemporary()
            connection.send(
                content: Data(header.utf8),
                isComplete: true,
                completion: .contentProcessed { [weak self] _ in
                    self?.scheduleGracefulClose(connection)
                }
            )
            return
        }

        connection.send(
            content: Data(header.utf8),
            isComplete: false,
            completion: .contentProcessed { [weak self] error in
                guard let self else { return }
                if error != nil {
                    payload.cleanupIfTemporary()
                    self.scheduleGracefulClose(connection)
                    return
                }
                self.sendPayloadBody(
                    payload,
                    start: bodyStart,
                    length: bodyLength,
                    on: connection
                )
            }
        )
    }

    private func sendPayloadBody(
        _ payload: VesperDashSegmentPayload,
        start: UInt64,
        length: UInt64,
        on connection: NWConnection
    ) {
        switch payload {
        case let .data(data, _):
            let startIndex = Int(start)
            let endIndex = startIndex + Int(length)
            connection.send(
                content: data.subdata(in: startIndex..<endIndex),
                isComplete: true,
                completion: .contentProcessed { [weak self] _ in
                    self?.scheduleGracefulClose(connection)
                }
            )
        case let .file(url, offset, _, removeAfterServing, _):
            do {
                let handle = try FileHandle(forReadingFrom: url)
                try handle.seek(toOffset: offset + start)
                sendFileChunks(
                    handle: handle,
                    url: url,
                    remaining: length,
                    removeAfterServing: removeAfterServing,
                    on: connection
                )
            } catch {
                if removeAfterServing {
                    try? FileManager.default.removeItem(at: url)
                }
                connection.cancel()
            }
        }
    }

    private func sendFileChunks(
        handle: FileHandle,
        url: URL,
        remaining: UInt64,
        removeAfterServing: Bool,
        on connection: NWConnection
    ) {
        guard remaining > 0 else {
            try? handle.close()
            if removeAfterServing {
                try? FileManager.default.removeItem(at: url)
            }
            connection.send(
                content: nil,
                isComplete: true,
                completion: .contentProcessed { [weak self] _ in
                    self?.scheduleGracefulClose(connection)
                }
            )
            return
        }

        let count = min(Int(remaining), Self.fileChunkSize)
        do {
            let data = try handle.read(upToCount: count) ?? Data()
            guard !data.isEmpty else {
                try? handle.close()
                if removeAfterServing {
                    try? FileManager.default.removeItem(at: url)
                }
                connection.cancel()
                return
            }
            let nextRemaining = remaining.saturatingSubtract(UInt64(data.count))
            connection.send(
                content: data,
                isComplete: nextRemaining == 0,
                completion: .contentProcessed { [weak self] error in
                    guard let self else { return }
                    if error != nil {
                        try? handle.close()
                        if removeAfterServing {
                            try? FileManager.default.removeItem(at: url)
                        }
                        self.scheduleGracefulClose(connection)
                        return
                    }
                    if nextRemaining == 0 {
                        try? handle.close()
                        if removeAfterServing {
                            try? FileManager.default.removeItem(at: url)
                        }
                        self.scheduleGracefulClose(connection)
                    } else {
                        self.sendFileChunks(
                            handle: handle,
                            url: url,
                            remaining: nextRemaining,
                            removeAfterServing: removeAfterServing,
                            on: connection
                        )
                    }
                }
            )
        } catch {
            try? handle.close()
            if removeAfterServing {
                try? FileManager.default.removeItem(at: url)
            }
            connection.cancel()
        }
    }

    private func sendStatus(_ status: Int, reason: String, on connection: NWConnection) {
        let response = "HTTP/1.1 \(status) \(reason)\r\n"
            + "Content-Length: 0\r\n"
            + "Connection: close\r\n\r\n"
        connection.send(
            content: Data(response.utf8),
            isComplete: true,
            completion: .contentProcessed { [weak self] _ in
                self?.scheduleGracefulClose(connection)
            }
        )
    }

    /// 发完 HTTP 响应后不要立即 cancel，否则 NWConnection 会发 RST，导致 AVPlayer
    /// 在读尾部字节时偶发 truncate 错误。这里选择：
    /// 1. 从 socket 继续 receive，等对端 FIN 主动关闭；
    /// 2. 同时设一个超时兑底 cancel，避免连接泄漏。
    /// 用一个 box 标志位避免两条路径都触发 cancel 后输出
    /// `is already cancelled, ignoring cancel` 噪音。
    private func scheduleGracefulClose(_ connection: NWConnection) {
        let cancelled = VesperDashAtomicBool()
        let cancelOnce: () -> Void = {
            guard cancelled.swapTrue() == false else { return }
            connection.cancel()
        }
        connection.receive(minimumIncompleteLength: 1, maximumLength: 1) { _, _, _, _ in
            cancelOnce()
        }
        queue.asyncAfter(deadline: .now() + .seconds(2)) {
            cancelOnce()
        }
    }
}

/// 单 bit 原子标志位，专供 loopback 连接关闭去重使用。
private final class VesperDashAtomicBool: @unchecked Sendable {
    private let lock = NSLock()
    private var value = false

    /// 把 value 设为 true，返回 *swap 之前* 的值。
    func swapTrue() -> Bool {
        lock.lock()
        defer { lock.unlock() }
        let previous = value
        value = true
        return previous
    }
}

actor VesperDashSession {
    typealias BenchmarkEventRecorder = @MainActor @Sendable (String, [String: String]) -> Void

    nonisolated static let scheme = "vesper-dash"
    nonisolated static let segmentCacheMaxBytes: UInt64 = 256 * 1024 * 1024
    nonisolated static let segmentCacheMaxEntryCount = 160
    nonisolated static let segmentCacheMaxSingleMediaBytes: UInt64 = 32 * 1024 * 1024
    nonisolated static let defaultDynamicManifestRefreshIntervalMs: UInt64 = 2_000
    nonisolated static let minimumDynamicManifestRefreshIntervalMs: UInt64 = 500

    nonisolated let id: String
    nonisolated let sourceURL: URL
    nonisolated let segmentCacheDirectory: URL

    private let networkClient: VesperDashNetworkClient
    private var manifest: VesperDashManifest?
    private var manifestLoadedAt: Date?
    private var masterPlaylistCache: Data?
    private var mediaPlaylistCacheByRenditionId: [String: Data] = [:]
    private var selectedPlayableByPolicy: [VesperDashMasterPlaylistVariantPolicy: VesperDashSelectedPlayableResponse] = [:]
    private var playableByRenditionId: [String: VesperDashPlayableRepresentation] = [:]
    private var sidxByRenditionId: [String: VesperDashSidxBox] = [:]
    private var mediaSegmentsByRenditionId: [String: [VesperDashMediaSegment]] = [:]
    private var templateSegmentsByRenditionId: [String: [VesperDashTemplateSegment]] = [:]
    private var cachedSegmentFiles: [VesperDashSegmentCacheKey: VesperDashCachedSegmentFile] = [:]
    private var segmentCacheTotalBytes: UInt64 = 0
    private var backgroundPrefetchRenditionIds: Set<String> = []
    private var backgroundPrefetchLargeMediaRenditionIds: Set<String> = []
    private var loopbackServer: VesperDashLoopbackServer?
    private var loopbackServerStartTask: Task<VesperDashLoopbackServer, Error>?
    private let benchmarkEventRecorder: BenchmarkEventRecorder?

    nonisolated var masterPlaylistURL: URL {
        URL(string: "\(Self.scheme)://master/\(id)/master.m3u8")!
    }

    init(
        sourceURL: URL,
        headers: [String: String] = [:],
        networkClient: VesperDashNetworkClient? = nil,
        benchmarkEventRecorder: BenchmarkEventRecorder? = nil
    ) {
        let sessionId = UUID().uuidString
        id = sessionId
        self.sourceURL = sourceURL
        segmentCacheDirectory = FileManager.default.temporaryDirectory
            .appendingPathComponent("vesper-dash-\(sessionId)", isDirectory: true)
        self.networkClient = networkClient ?? VesperDashNetworkClient(headers: headers)
        self.benchmarkEventRecorder = benchmarkEventRecorder
    }

    deinit {
        try? FileManager.default.removeItem(at: segmentCacheDirectory)
    }

    nonisolated func mediaPlaylistURL(for renditionId: String) -> URL {
        let encodedId = renditionId.addingPercentEncoding(withAllowedCharacters: dashPathComponentAllowedCharacters)
            ?? renditionId
        return URL(string: "\(Self.scheme)://media/\(id)/\(encodedId).m3u8")!
    }

    nonisolated func segmentURL(for renditionId: String, segment: VesperDashSegmentRequest) -> URL {
        let encodedId = renditionId.addingPercentEncoding(withAllowedCharacters: dashPathComponentAllowedCharacters)
            ?? renditionId
        let segmentName: String
        switch segment {
        case .initialization:
            segmentName = "init.mp4"
        case let .media(index):
            segmentName = "\(index).m4s"
        }
        return URL(string: "\(Self.scheme)://segment/\(id)/\(encodedId)/\(segmentName)")!
    }

    nonisolated func route(for url: URL) -> VesperDashRoute? {
        guard url.scheme == Self.scheme else { return nil }
        let encodedPath = URLComponents(url: url, resolvingAgainstBaseURL: false)?.percentEncodedPath
            ?? url.path
        let components = encodedPath
            .split(separator: "/")
            .map(String.init)
        guard components.first == id else { return nil }

        switch url.host {
        case "master":
            return .master
        case "media":
            guard components.count >= 2 else { return nil }
            var encodedId = components[1]
            if encodedId.hasSuffix(".m3u8") {
                encodedId.removeLast(".m3u8".count)
            }
            return .media(encodedId.removingPercentEncoding ?? encodedId)
        case "segment":
            guard components.count >= 3 else { return nil }
            let renditionId = components[1].removingPercentEncoding ?? components[1]
            let segmentName = components[2]
            if segmentName == "init.mp4" {
                return .segment(renditionId, .initialization)
            }
            guard segmentName.hasSuffix(".m4s") else { return nil }
            let indexText = String(segmentName.dropLast(".m4s".count))
            guard let index = Int(indexText), index >= 0 else { return nil }
            return .segment(renditionId, .media(index))
        default:
            return nil
        }
    }

    func masterPlaylistData() async throws -> Data {
        let startedAt = DispatchTime.now().uptimeNanoseconds
        await recordBenchmarkEvent("dash_master_playlist_request_start")
        if let masterPlaylistCache, manifest?.type != .dynamic {
            await recordBenchmarkEvent(
                "dash_master_playlist_request_end",
                attributes: playlistBenchmarkEndAttributes(
                    startedAt: startedAt,
                    bytes: masterPlaylistCache.count,
                    cacheHit: true
                )
            )
            return masterPlaylistCache
        }

        do {
            let manifest = try await loadManifest()
            let variantPolicy = VesperDashMasterPlaylistVariantPolicy.all
            let playlist = try VesperDashHlsBuilder.buildMasterPlaylist(
                manifest: manifest,
                variantPolicy: variantPolicy,
                mediaURL: { [weak self] renditionId in
                    guard let self else { return "" }
                    return self.mediaPlaylistURL(for: renditionId).absoluteString
                }
            )
            let data = Data(playlist.utf8)
            if manifest.type != .dynamic {
                masterPlaylistCache = data
            }

            let startupSelected = try selectedPlayableRepresentations(
                manifest: manifest,
                variantPolicy: .startupSingleVariant
            )
            startBackgroundPrefetch(for: startupSelected.audio + startupSelected.video, manifest: manifest)
#if DEBUG
            iosHostLog(
                "dashMasterPlaylist policy=all startupVideo=\(startupRenditionSummary(startupSelected.video)) startupAudio=\(startupRenditionSummary(startupSelected.audio))"
            )
#endif
            await recordBenchmarkEvent(
                "dash_master_playlist_request_end",
                attributes: playlistBenchmarkEndAttributes(
                    startedAt: startedAt,
                    bytes: data.count,
                    cacheHit: false
                )
            )
            return data
        } catch {
            await recordBenchmarkEvent(
                "dash_master_playlist_request_end",
                attributes: playlistBenchmarkEndAttributes(
                    startedAt: startedAt,
                    bytes: nil,
                    cacheHit: false,
                    error: error
                )
            )
            throw error
        }
    }

    func manifestTrackCatalogSnapshot() async throws -> VesperDashManifestTrackCatalogSnapshot {
        let manifest = try await loadManifest()
        let selected = try selectedPlayableRepresentations(
            manifest: manifest,
            variantPolicy: .all
        )
        return VesperDashManifestTrackCatalogSnapshot(
            audio: selected.audio,
            video: selected.video,
            subtitles: selected.subtitles
        )
    }

    func mediaPlaylistData(renditionId: String) async throws -> Data {
        let startedAt = DispatchTime.now().uptimeNanoseconds
        await recordBenchmarkEvent(
            "dash_media_playlist_request_start",
            attributes: ["renditionId": renditionId]
        )
        if let cached = mediaPlaylistCacheByRenditionId[renditionId], manifest?.type != .dynamic {
            await recordBenchmarkEvent(
                "dash_media_playlist_request_end",
                attributes: playlistBenchmarkEndAttributes(
                    startedAt: startedAt,
                    bytes: cached.count,
                    cacheHit: true,
                    extra: ["renditionId": renditionId]
                )
            )
            return cached
        }

        do {
            let data = try await buildMediaPlaylistData(renditionId: renditionId)
            await recordBenchmarkEvent(
                "dash_media_playlist_request_end",
                attributes: playlistBenchmarkEndAttributes(
                    startedAt: startedAt,
                    bytes: data.count,
                    cacheHit: false,
                    extra: ["renditionId": renditionId]
                )
            )
            return data
        } catch {
            await recordBenchmarkEvent(
                "dash_media_playlist_request_end",
                attributes: playlistBenchmarkEndAttributes(
                    startedAt: startedAt,
                    bytes: nil,
                    cacheHit: false,
                    error: error,
                    extra: ["renditionId": renditionId]
                )
            )
            throw error
        }
    }

    private func buildMediaPlaylistData(renditionId: String) async throws -> Data {
        let manifest = try await loadManifest()
        let playable = try await playableRepresentation(renditionId: renditionId)
        if let segmentBase = playable.representation.segmentBase {
            if manifest.type == .dynamic {
                throw VesperDashBridgeError.unsupportedManifest(
                    "dynamic DASH SegmentBase is not supported on iOS"
                )
            }
            let segments = try await mediaSegments(for: playable, segmentBase: segmentBase)
            let mediaURL = playable.representation.baseURL
            let playlist = try VesperDashHlsBuilder.buildExternalMediaPlaylist(
                map: VesperDashHlsMap(uri: mediaURL, byteRange: segmentBase.initialization),
                playlistKind: .vod,
                mediaSequence: nil,
                segments: segments.map {
                    VesperDashHlsSegment(
                        duration: $0.duration,
                        uri: mediaURL,
                        byteRange: $0.range
                    )
                }
            )
            let data = Data(playlist.utf8)
            mediaPlaylistCacheByRenditionId[renditionId] = data
            return data
        }

        guard let segmentTemplate = playable.representation.segmentTemplate else {
            throw VesperDashBridgeError.unsupportedManifest(
                "Representation \(playable.representation.id) does not use SegmentBase or SegmentTemplate"
            )
        }
        let segments = try templateSegments(
            for: playable,
            manifest: manifest,
            segmentTemplate: segmentTemplate
        )
        let server = try await dashLoopbackServer()
        startBackgroundSegmentPrefetch(
            renditionId: playable.renditionId,
            segmentCount: segments.count,
            prefetchMediaSegments: shouldPrefetchTemplateMediaSegments(
                playable: playable,
                segments: segments
            )
        )
        // EXT-X-MAP 指向 vesper-dash:// scheme，走 AVAssetResourceLoaderDelegate
        // 主路径。实验表明当 EXT-X-MAP 使用 loopback http URL 时，AVPlayer
        // 可能不会发起该请求（潜在原因是 AVPlayer 在验证同一 origin
        // 路径才会复用 loopback http），导致 init 段未交付 → 'frmt'。
        // 走 vesper-dash:// scheme 可以保证 AVPlayer 调用 resource loader
        // delegate，从而能记录 init 段是否被请求。
        let initializationURL = segmentTemplate.initialization.map { _ in
            self.segmentURL(for: playable.renditionId, segment: .initialization).absoluteString
        }
        let playlistKind: VesperDashHlsPlaylistKind = manifest.type == .dynamic ? .live : .vod
        let mediaSequence = manifest.type == .dynamic ? segments.first?.number : nil
        let playlist = try VesperDashHlsBuilder.buildExternalMediaPlaylist(
            map: initializationURL.map { VesperDashHlsMap(uri: $0, byteRange: nil) },
            playlistKind: playlistKind,
            mediaSequence: mediaSequence,
            segments: try segments.enumerated().map { index, segment in
                let segmentIndex = try hlsSegmentIndex(
                    manifest: manifest,
                    segment: segment,
                    fallbackIndex: index
                )
                let segmentURL = try server.segmentURL(
                    for: playable.renditionId,
                    segment: .media(segmentIndex)
                )
                return VesperDashHlsSegment(
                    duration: segment.duration,
                    uri: segmentURL.absoluteString,
                    byteRange: nil
                )
            }
        )
#if DEBUG
        iosHostLog(
            "dashMediaPlaylist rendition=\(playable.renditionId) loopbackSegments=true count=\(segments.count) init=\(initializationURL ?? "none")"
        )
        // 打印 playlist 头部 7 行，便于排查 HLS 标签拼接错误（曾因 multiline
        // 字符串末尾缺换行导致 EXT-X-PLAYLIST-TYPE 与 EXT-X-MAP 粘到一行）。
        let head = playlist
            .split(separator: "\n", omittingEmptySubsequences: false)
            .prefix(7)
            .joined(separator: " | ")
        iosHostLog("dashMediaPlaylist head=\(head)")
#endif
        let data = Data(playlist.utf8)
        mediaPlaylistCacheByRenditionId[renditionId] = data
        return data
    }

    private func hlsSegmentIndex(
        manifest: VesperDashManifest,
        segment: VesperDashTemplateSegment,
        fallbackIndex: Int
    ) throws -> Int {
        guard manifest.type == .dynamic else {
            return fallbackIndex
        }
        return try checkedInt(segment.number, field: "DASH live segment number")
    }

    private func dashSegmentContentType(
        for playable: VesperDashPlayableRepresentation,
        segment: VesperDashSegmentRequest
    ) -> String {
        if segment == .initialization {
            return "video/mp4"
        }
        let mimeType = playable.representation.mimeType
            .trimmingCharacters(in: .whitespacesAndNewlines)
            .lowercased()
        if mimeType == "text/vtt" || mimeType == "text/webvtt" || mimeType.contains("vtt") {
            return "text/vtt"
        }
        return "video/mp4"
    }

    private func dashLoopbackServer() async throws -> VesperDashLoopbackServer {
        if let loopbackServer {
            return loopbackServer
        }
        if let loopbackServerStartTask {
            return try await loopbackServerStartTask.value
        }
        let server = try VesperDashLoopbackServer(sessionId: id) { [weak self] renditionId, segment in
            guard let self else {
                throw VesperDashBridgeError.network("DASH session is no longer available")
            }
            return try await self.segmentPayload(
                renditionId: renditionId,
                segment: segment,
                requestOrigin: "loopback"
            )
        }
        let startTask = Task { () throws -> VesperDashLoopbackServer in
            try await server.start()
            return server
        }
        loopbackServerStartTask = startTask
        do {
            let startedServer = try await startTask.value
            if loopbackServer == nil {
                loopbackServer = startedServer
                loopbackServerStartTask = nil
#if DEBUG
                iosHostLog("dashLoopbackServer started session=\(id)")
#endif
            }
            return startedServer
        } catch {
            loopbackServerStartTask = nil
            throw error
        }
    }

    func segmentData(renditionId: String, segment: VesperDashSegmentRequest) async throws -> Data {
        try await segmentPayload(
            renditionId: renditionId,
            segment: segment,
            requestOrigin: "resourceLoader"
        ).readData()
    }

    private func segmentPayload(
        renditionId: String,
        segment: VesperDashSegmentRequest,
        requestOrigin: String = "playback"
    ) async throws -> VesperDashSegmentPayload {
        let startedAt = DispatchTime.now().uptimeNanoseconds
        await recordBenchmarkEvent(
            segmentBenchmarkEventName(segment, suffix: "start"),
            attributes: segmentBenchmarkStartAttributes(
                renditionId: renditionId,
                segment: segment,
                requestOrigin: requestOrigin
            )
        )
        do {
            let result = try await resolveSegmentPayload(renditionId: renditionId, segment: segment)
            await recordBenchmarkEvent(
                segmentBenchmarkEventName(segment, suffix: "end"),
                attributes: segmentBenchmarkEndAttributes(
                    startedAt: startedAt,
                    renditionId: renditionId,
                    segment: segment,
                    requestOrigin: requestOrigin,
                    result: result
                )
            )
            return result.payload
        } catch {
            await recordBenchmarkEvent(
                segmentBenchmarkEventName(segment, suffix: "end"),
                attributes: segmentBenchmarkEndAttributes(
                    startedAt: startedAt,
                    renditionId: renditionId,
                    segment: segment,
                    requestOrigin: requestOrigin,
                    error: error
                )
            )
            throw error
        }
    }

    private func resolveSegmentPayload(
        renditionId: String,
        segment: VesperDashSegmentRequest
    ) async throws -> VesperDashSegmentPayloadResult {
        let manifest = try await loadManifest()
        let playable = try await playableRepresentation(renditionId: renditionId)
        if let segmentBase = playable.representation.segmentBase {
            if manifest.type == .dynamic {
                throw VesperDashBridgeError.unsupportedManifest(
                    "dynamic DASH SegmentBase is not supported on iOS"
                )
            }
            guard let mediaURL = URL(string: playable.representation.baseURL) else {
                throw VesperDashBridgeError.invalidManifest(
                    "invalid media URL \(playable.representation.baseURL)"
                )
            }

            let byteRange: VesperDashByteRange
            switch segment {
            case .initialization:
                byteRange = segmentBase.initialization
            case let .media(index):
                let segments = try await mediaSegments(for: playable, segmentBase: segmentBase)
                guard segments.indices.contains(index) else {
                    throw VesperDashBridgeError.invalidManifest(
                        "missing media segment \(index) for rendition \(renditionId)"
                    )
                }
                byteRange = segments[index].range
            }

            if mediaURL.isFileURL {
                let payload = VesperDashSegmentPayload.file(
                    url: mediaURL,
                    offset: byteRange.start,
                    size: byteRange.length,
                    removeAfterServing: false,
                    contentType: dashSegmentContentType(for: playable, segment: segment)
                )
                return VesperDashSegmentPayloadResult(
                    payload: payload,
                    cacheHit: false,
                    segmentType: "base",
                    byteRange: byteRange,
                    delivery: "localFile"
                )
            }
            let data = try await networkClient.data(for: mediaURL, byteRange: byteRange)
            return VesperDashSegmentPayloadResult(
                payload: .data(
                    data,
                    contentType: dashSegmentContentType(for: playable, segment: segment)
                ),
                cacheHit: false,
                segmentType: "base",
                byteRange: byteRange,
                delivery: "networkData"
            )
        }

        guard let segmentTemplate = playable.representation.segmentTemplate else {
            throw VesperDashBridgeError.unsupportedManifest(
                "Representation \(playable.representation.id) does not use SegmentBase or SegmentTemplate"
            )
        }
        return try await cachedSegmentTemplatePayload(
            manifest: manifest,
            playable: playable,
            segmentTemplate: segmentTemplate,
            segment: segment
        )
    }

    private func cachedSegmentTemplatePayload(
        manifest: VesperDashManifest,
        playable: VesperDashPlayableRepresentation,
        segmentTemplate: VesperDashSegmentTemplate,
        segment: VesperDashSegmentRequest
    ) async throws -> VesperDashSegmentPayloadResult {
        let contentType = dashSegmentContentType(for: playable, segment: segment)
        let key = VesperDashSegmentCacheKey(
            renditionId: playable.renditionId,
            segment: segment
        )
        let cacheURL = segmentCacheURL(
            renditionId: playable.renditionId,
            segment: segment
        )
        if let cached = cachedSegmentFilePayload(for: key, at: cacheURL, contentType: contentType) {
            return VesperDashSegmentPayloadResult(
                payload: cached,
                cacheHit: true,
                segmentType: "template",
                byteRange: nil,
                delivery: "cacheFile"
            )
        }
        if case .media = segment {
            let payload = try await fetchSegmentTemplateFile(
                manifest: manifest,
                playable: playable,
                segmentTemplate: segmentTemplate,
                segment: segment,
                cacheURL: cacheURL,
                key: key,
                allowSkippingLargeMediaEntry: true,
                contentType: contentType
            )
            return VesperDashSegmentPayloadResult(
                payload: payload,
                cacheHit: false,
                segmentType: "template",
                byteRange: nil,
                delivery: payload.isTemporaryFile ? "temporaryFile" : "cacheFile"
            )
        }

        let data = try await fetchSegmentTemplateData(
            manifest: manifest,
            playable: playable,
            segmentTemplate: segmentTemplate,
            segment: segment
        )
        try Task.checkCancellation()
        if try writeSegmentTemplateCache(
            data,
            to: cacheURL,
            key: key,
            allowSkippingLargeMediaEntry: true
        ) {
            let payload = cachedSegmentFilePayload(for: key, at: cacheURL, contentType: contentType)
                ?? .data(data, contentType: contentType)
            return VesperDashSegmentPayloadResult(
                payload: payload,
                cacheHit: false,
                segmentType: "template",
                byteRange: nil,
                delivery: "cacheFile"
            )
        }
        return VesperDashSegmentPayloadResult(
            payload: .data(data, contentType: contentType),
            cacheHit: false,
            segmentType: "template",
            byteRange: nil,
            delivery: "networkData"
        )
    }

    private func fetchSegmentTemplateFile(
        manifest: VesperDashManifest,
        playable: VesperDashPlayableRepresentation,
        segmentTemplate: VesperDashSegmentTemplate,
        segment: VesperDashSegmentRequest,
        cacheURL: URL,
        key: VesperDashSegmentCacheKey,
        allowSkippingLargeMediaEntry: Bool,
        contentType: String
    ) async throws -> VesperDashSegmentPayload {
        let url = try templateSegmentURL(
            manifest: manifest,
            playable: playable,
            segmentTemplate: segmentTemplate,
            segment: segment
        )
        let temporaryURL = temporarySegmentDownloadURL(renditionId: playable.renditionId, segment: segment)
        let size = try await networkClient.download(for: url, to: temporaryURL)
#if DEBUG
        logTopLevelBoxes(
            fileURL: temporaryURL,
            totalBytes: size,
            label: "dashSegmentTemplate",
            renditionId: playable.renditionId,
            segment: segment
        )
#endif
        return try materializeSegmentTemplateFile(
            from: temporaryURL,
            to: cacheURL,
            size: size,
            key: key,
            allowSkippingLargeMediaEntry: allowSkippingLargeMediaEntry,
            contentType: contentType
        )
    }

    private func materializeSegmentTemplateFile(
        from temporaryURL: URL,
        to cacheURL: URL,
        size: UInt64,
        key: VesperDashSegmentCacheKey,
        allowSkippingLargeMediaEntry: Bool,
        contentType: String
    ) throws -> VesperDashSegmentPayload {
        if allowSkippingLargeMediaEntry,
           case .media = key.segment,
           size > Self.segmentCacheMaxSingleMediaBytes {
#if DEBUG
            iosHostLog(
                "dashSegmentCache streamLarge rendition=\(key.renditionId) segment=\(key.segment) bytes=\(size)"
            )
#endif
            return .file(
                url: temporaryURL,
                offset: 0,
                size: size,
                removeAfterServing: true,
                contentType: contentType
            )
        }

        try FileManager.default.createDirectory(
            at: segmentCacheDirectory,
            withIntermediateDirectories: true
        )
        let addsEntry = cachedSegmentFiles[key] == nil
        if let existing = cachedSegmentFiles[key] {
            segmentCacheTotalBytes = segmentCacheTotalBytes.saturatingSubtract(existing.size)
        }
        try trimSegmentCache(reserving: size, addingEntry: addsEntry, protecting: key)
        try? FileManager.default.removeItem(at: cacheURL)
        try FileManager.default.moveItem(at: temporaryURL, to: cacheURL)
        cachedSegmentFiles[key] = VesperDashCachedSegmentFile(
            url: cacheURL,
            size: size,
            segment: key.segment,
            lastAccessedAt: Date()
        )
        segmentCacheTotalBytes = segmentCacheTotalBytes.saturatingAdd(size)
        try trimSegmentCache(reserving: 0, addingEntry: false, protecting: key)
        return .file(
            url: cacheURL,
            offset: 0,
            size: size,
            removeAfterServing: false,
            contentType: contentType
        )
    }

    private func temporarySegmentDownloadURL(
        renditionId: String,
        segment: VesperDashSegmentRequest
    ) -> URL {
        let encodedId = renditionId.addingPercentEncoding(withAllowedCharacters: dashPathComponentAllowedCharacters)
            ?? renditionId
        let segmentName: String
        switch segment {
        case .initialization:
            segmentName = "init"
        case let .media(index):
            segmentName = "\(index)"
        }
        return segmentCacheDirectory
            .appendingPathComponent("tmp-\(encodedId)-\(segmentName)-\(UUID().uuidString)", isDirectory: false)
    }

    private func fetchSegmentTemplateData(
        manifest: VesperDashManifest,
        playable: VesperDashPlayableRepresentation,
        segmentTemplate: VesperDashSegmentTemplate,
        segment: VesperDashSegmentRequest
    ) async throws -> Data {
        let url = try templateSegmentURL(
            manifest: manifest,
            playable: playable,
            segmentTemplate: segmentTemplate,
            segment: segment
        )
        let data = try await networkClient.data(for: url)
        // 保留原始 fMP4 segment 字节。以前这里对 media 段调用 removingTopLevelSidxBoxes 剔掉顺序 sidx box，但许多
        // DASH 编码器生成的 tfhd.base_data_offset 是相对 segment 起点的绝对偏移，删掉 sidx 后 mdat 位置
        // 前移会让 AVPlayer 读出垃圾字节并报 CoreMediaErrorDomain 1718449215 ('frmt')。HLS fMP4 允许
        // segment 中保留 sidx，AVPlayer 会忽略。
#if DEBUG
        logTopLevelBoxes(
            data: data,
            label: "dashSegmentTemplate",
            renditionId: playable.renditionId,
            segment: segment
        )
#endif
        return data
    }

#if DEBUG
    private func logTopLevelBoxes(
        data: Data,
        label: String,
        renditionId: String,
        segment: VesperDashSegmentRequest
    ) {
        let bytes = [UInt8](data.prefix(4_096))
        var cursor = 0
        var types: [String] = []
        while cursor < bytes.count, types.count < 8 {
            guard let header = try? VesperMp4BoxHeader.parse(bytes: bytes, start: cursor) else { break }
            let typeString = String(bytes: header.boxType, encoding: .ascii) ?? "????"
            types.append(typeString)
            if header.end <= cursor { break }
            cursor = header.end
        }
        iosHostLog(
            "\(label) rendition=\(renditionId) segment=\(segment) bytes=\(data.count) topBoxes=\(types.joined(separator: ","))"
        )
    }

    private func logTopLevelBoxes(
        fileURL: URL,
        totalBytes: UInt64,
        label: String,
        renditionId: String,
        segment: VesperDashSegmentRequest
    ) {
        guard
            let handle = try? FileHandle(forReadingFrom: fileURL),
            let data = try? handle.read(upToCount: 4_096)
        else {
            iosHostLog(
                "\(label) rendition=\(renditionId) segment=\(segment) bytes=\(totalBytes) topBoxes=<unreadable>"
            )
            return
        }
        try? handle.close()
        let bytes = [UInt8](data)
        var cursor = 0
        var types: [String] = []
        while cursor < bytes.count, types.count < 8 {
            guard let header = try? VesperMp4BoxHeader.parse(bytes: bytes, start: cursor) else { break }
            let typeString = String(bytes: header.boxType, encoding: .ascii) ?? "????"
            types.append(typeString)
            if header.end <= cursor { break }
            cursor = header.end
        }
        iosHostLog(
            "\(label) rendition=\(renditionId) segment=\(segment) bytes=\(totalBytes) topBoxes=\(types.joined(separator: ","))"
        )
    }
#endif

    private func cachedSegmentFilePayload(
        for key: VesperDashSegmentCacheKey,
        at url: URL,
        contentType: String
    ) -> VesperDashSegmentPayload? {
        guard FileManager.default.fileExists(atPath: url.path) else {
            if let existing = cachedSegmentFiles.removeValue(forKey: key) {
                segmentCacheTotalBytes = segmentCacheTotalBytes.saturatingSubtract(existing.size)
            }
            return nil
        }
        let size = fileSize(at: url) ?? cachedSegmentFiles[key]?.size ?? 0
        touchCachedSegmentFile(key: key, url: url, size: size)
        return .file(
            url: url,
            offset: 0,
            size: size,
            removeAfterServing: false,
            contentType: contentType
        )
    }

    private func cachedSegmentFileExists(
        for key: VesperDashSegmentCacheKey,
        at url: URL
    ) -> Bool {
        guard FileManager.default.fileExists(atPath: url.path) else {
            if let existing = cachedSegmentFiles.removeValue(forKey: key) {
                segmentCacheTotalBytes = segmentCacheTotalBytes.saturatingSubtract(existing.size)
            }
            return false
        }
        let size = fileSize(at: url) ?? cachedSegmentFiles[key]?.size ?? 0
        touchCachedSegmentFile(key: key, url: url, size: size)
        return true
    }

    @discardableResult
    private func writeSegmentTemplateCache(
        _ data: Data,
        to url: URL,
        key: VesperDashSegmentCacheKey,
        allowSkippingLargeMediaEntry: Bool
    ) throws -> Bool {
        let size = UInt64(data.count)
        if allowSkippingLargeMediaEntry,
           case .media = key.segment,
           size > Self.segmentCacheMaxSingleMediaBytes {
#if DEBUG
            iosHostLog(
                "dashSegmentCache skipLarge rendition=\(key.renditionId) segment=\(key.segment) bytes=\(size)"
            )
#endif
            return false
        }

        try FileManager.default.createDirectory(
            at: segmentCacheDirectory,
            withIntermediateDirectories: true
        )
        let addsEntry = cachedSegmentFiles[key] == nil
        if let existing = cachedSegmentFiles[key] {
            segmentCacheTotalBytes = segmentCacheTotalBytes.saturatingSubtract(existing.size)
        }
        try trimSegmentCache(reserving: size, addingEntry: addsEntry, protecting: key)
        try data.write(to: url, options: .atomic)
        cachedSegmentFiles[key] = VesperDashCachedSegmentFile(
            url: url,
            size: size,
            segment: key.segment,
            lastAccessedAt: Date()
        )
        segmentCacheTotalBytes = segmentCacheTotalBytes.saturatingAdd(size)
        try trimSegmentCache(reserving: 0, addingEntry: false, protecting: key)
        return true
    }

    private func touchCachedSegmentFile(
        key: VesperDashSegmentCacheKey,
        url: URL,
        size: UInt64
    ) {
        if let existing = cachedSegmentFiles[key] {
            segmentCacheTotalBytes = segmentCacheTotalBytes
                .saturatingSubtract(existing.size)
                .saturatingAdd(size)
            cachedSegmentFiles[key] = VesperDashCachedSegmentFile(
                url: url,
                size: size,
                segment: key.segment,
                lastAccessedAt: Date()
            )
            return
        }
        cachedSegmentFiles[key] = VesperDashCachedSegmentFile(
            url: url,
            size: size,
            segment: key.segment,
            lastAccessedAt: Date()
        )
        segmentCacheTotalBytes = segmentCacheTotalBytes.saturatingAdd(size)
    }

    private func fileSize(at url: URL) -> UInt64? {
        guard
            let attributes = try? FileManager.default.attributesOfItem(atPath: url.path),
            let value = attributes[.size] as? NSNumber
        else {
            return nil
        }
        return value.uint64Value
    }

    private func trimSegmentCache(
        reserving additionalBytes: UInt64,
        addingEntry: Bool,
        protecting protectedKey: VesperDashSegmentCacheKey
    ) throws {
        var projectedBytes = segmentCacheTotalBytes.saturatingAdd(additionalBytes)
        while
            cachedSegmentFiles.count + (addingEntry ? 1 : 0) > Self.segmentCacheMaxEntryCount ||
            projectedBytes > Self.segmentCacheMaxBytes
        {
            guard let eviction = nextSegmentCacheEviction(protecting: protectedKey) else {
                return
            }
            cachedSegmentFiles[eviction.key] = nil
            segmentCacheTotalBytes = segmentCacheTotalBytes.saturatingSubtract(eviction.file.size)
            projectedBytes = projectedBytes.saturatingSubtract(eviction.file.size)
            try? FileManager.default.removeItem(at: eviction.file.url)
#if DEBUG
            iosHostLog(
                "dashSegmentCache evict rendition=\(eviction.key.renditionId) segment=\(eviction.key.segment) bytes=\(eviction.file.size)"
            )
#endif
        }
    }

    private func nextSegmentCacheEviction(
        protecting protectedKey: VesperDashSegmentCacheKey
    ) -> (key: VesperDashSegmentCacheKey, file: VesperDashCachedSegmentFile)? {
        let candidate = cachedSegmentFiles
            .filter { key, _ in key != protectedKey }
            .min { lhs, rhs in
                let lhsInit = lhs.value.isInitialization
                let rhsInit = rhs.value.isInitialization
                if lhsInit != rhsInit {
                    return !lhsInit
                }
                return lhs.value.lastAccessedAt < rhs.value.lastAccessedAt
            }
        return candidate.map { (key: $0.key, file: $0.value) }
    }

    private func startBackgroundSegmentPrefetch(
        renditionId: String,
        segmentCount: Int,
        prefetchMediaSegments: Bool
    ) {
        guard !sourceURL.isFileURL,
              segmentCount > 0,
              !backgroundPrefetchRenditionIds.contains(renditionId)
        else {
            return
        }
        backgroundPrefetchRenditionIds.insert(renditionId)
        let shouldPrefetchMediaSegments = prefetchMediaSegments
            && !backgroundPrefetchLargeMediaRenditionIds.contains(renditionId)
        Task(priority: .utility) { [weak self] in
            await self?.runBackgroundSegmentPrefetch(
                renditionId: renditionId,
                segmentCount: segmentCount,
                prefetchMediaSegments: shouldPrefetchMediaSegments
            )
        }
    }

    private func startBackgroundPrefetch(
        for playables: [VesperDashPlayableRepresentation],
        manifest: VesperDashManifest
    ) {
        for playable in playables {
            guard let segmentTemplate = playable.representation.segmentTemplate,
                  let segments = try? templateSegments(
                    for: playable,
                    manifest: manifest,
                    segmentTemplate: segmentTemplate
                  )
            else {
                continue
            }
            startBackgroundSegmentPrefetch(
                renditionId: playable.renditionId,
                segmentCount: segments.count,
                prefetchMediaSegments: shouldPrefetchTemplateMediaSegments(
                    playable: playable,
                    segments: segments
                )
            )
        }
    }

    private func shouldPrefetchTemplateMediaSegments(
        playable: VesperDashPlayableRepresentation,
        segments: [VesperDashTemplateSegment]
    ) -> Bool {
        guard let bandwidth = playable.representation.bandwidth, bandwidth > 0 else {
            return true
        }
        let maxDuration = segments.map(\.duration).max() ?? 0
        guard maxDuration.isFinite, maxDuration > 0 else {
            return true
        }
        let estimatedBytes = maxDuration * Double(bandwidth) / 8
        guard estimatedBytes.isFinite else {
            return false
        }
        let shouldPrefetch = estimatedBytes <= Double(Self.segmentCacheMaxSingleMediaBytes)
#if DEBUG
        if !shouldPrefetch {
            iosHostLog(
                "dashSegmentPrefetch skipMedia rendition=\(playable.renditionId) estimatedBytes=\(String(format: "%.0f", estimatedBytes)) limit=\(Self.segmentCacheMaxSingleMediaBytes)"
            )
        }
#endif
        return shouldPrefetch
    }

    private func startupRenditionSummary(
        _ playables: [VesperDashPlayableRepresentation]
    ) -> String {
        guard !playables.isEmpty else {
            return "none"
        }
        return playables
            .map(startupRenditionDescription)
            .joined(separator: ";")
    }

    private func startupRenditionDescription(
        _ playable: VesperDashPlayableRepresentation
    ) -> String {
        let representation = playable.representation
        return [
            "id=\(playable.renditionId)",
            "codec=\(emptyAsNil(representation.codecs))",
            "width=\(representation.width.map(String.init) ?? "nil")",
            "height=\(representation.height.map(String.init) ?? "nil")",
            "bitrate=\(representation.bandwidth.map(String.init) ?? "nil")",
            "frameRate=\(representation.frameRate ?? "nil")",
            "segmentType=\(dashSegmentTypeName(representation))",
        ].joined(separator: ",")
    }

    private func recordBenchmarkEvent(
        _ eventName: String,
        attributes: [String: String] = [:]
    ) async {
        guard let benchmarkEventRecorder else {
            return
        }
        await benchmarkEventRecorder(eventName, attributes)
    }

    private func playlistBenchmarkEndAttributes(
        startedAt: UInt64,
        bytes: Int?,
        cacheHit: Bool,
        error: Error? = nil,
        extra: [String: String] = [:]
    ) -> [String: String] {
        var attributes = extra
        attributes["elapsedMs"] = elapsedMillisecondsString(since: startedAt)
        attributes["cacheHit"] = "\(cacheHit)"
        if let bytes {
            attributes["bytes"] = "\(bytes)"
        }
        if let error {
            attributes["error"] = error.localizedDescription
        }
        return attributes
    }

    private func segmentBenchmarkEventName(
        _ segment: VesperDashSegmentRequest,
        suffix: String
    ) -> String {
        switch segment {
        case .initialization:
            return "dash_init_segment_request_\(suffix)"
        case .media:
            return "dash_media_segment_request_\(suffix)"
        }
    }

    private func segmentBenchmarkStartAttributes(
        renditionId: String,
        segment: VesperDashSegmentRequest,
        requestOrigin: String
    ) -> [String: String] {
        segmentBenchmarkBaseAttributes(
            renditionId: renditionId,
            segment: segment,
            requestOrigin: requestOrigin
        )
    }

    private func segmentBenchmarkEndAttributes(
        startedAt: UInt64,
        renditionId: String,
        segment: VesperDashSegmentRequest,
        requestOrigin: String,
        result: VesperDashSegmentPayloadResult
    ) -> [String: String] {
        var attributes = segmentBenchmarkBaseAttributes(
            renditionId: renditionId,
            segment: segment,
            requestOrigin: requestOrigin
        )
        attributes["elapsedMs"] = elapsedMillisecondsString(since: startedAt)
        attributes["bytes"] = "\(result.payload.size)"
        attributes["cacheHit"] = "\(result.cacheHit)"
        attributes["segmentType"] = result.segmentType
        attributes["delivery"] = result.delivery
        attributes["contentType"] = result.payload.contentType
        if let byteRange = result.byteRange {
            attributes["byteRange"] = "\(byteRange.start)-\(byteRange.end)"
        }
        return attributes
    }

    private func segmentBenchmarkEndAttributes(
        startedAt: UInt64,
        renditionId: String,
        segment: VesperDashSegmentRequest,
        requestOrigin: String,
        error: Error
    ) -> [String: String] {
        var attributes = segmentBenchmarkBaseAttributes(
            renditionId: renditionId,
            segment: segment,
            requestOrigin: requestOrigin
        )
        attributes["elapsedMs"] = elapsedMillisecondsString(since: startedAt)
        attributes["error"] = error.localizedDescription
        return attributes
    }

    private func segmentBenchmarkBaseAttributes(
        renditionId: String,
        segment: VesperDashSegmentRequest,
        requestOrigin: String
    ) -> [String: String] {
        var attributes = [
            "renditionId": renditionId,
            "segmentKind": dashSegmentKindName(segment),
            "requestOrigin": requestOrigin,
        ]
        if case let .media(index) = segment {
            attributes["index"] = "\(index)"
        }
        return attributes
    }

    private func elapsedMillisecondsString(since startedAt: UInt64) -> String {
        let now = DispatchTime.now().uptimeNanoseconds
        let elapsedNs = now >= startedAt ? now - startedAt : 0
        return "\(elapsedNs / 1_000_000)"
    }

    private func runBackgroundSegmentPrefetch(
        renditionId: String,
        segmentCount: Int,
        prefetchMediaSegments: Bool
    ) async {
        let prefetchLimit = prefetchMediaSegments ? min(segmentCount, 120) : 0
        let requests = backgroundPrefetchRequests(
            count: prefetchLimit,
            includeMediaSegments: prefetchMediaSegments
        )
        let concurrency = min(4, requests.count)
        guard concurrency > 0 else { return }

        await withTaskGroup(of: Bool.self) { group in
            var nextIndex = 0
            var shouldStopMediaPrefetch = false
            for _ in 0..<concurrency {
                let request = requests[nextIndex]
                nextIndex += 1
                group.addTask { [weak self] in
                    await self?.prefetchIgnoringFailure(
                        renditionId: renditionId,
                        segment: request
                    ) ?? false
                }
            }

            while let shouldContinue = await group.next() {
                if !shouldContinue {
                    shouldStopMediaPrefetch = true
                }
                guard !shouldStopMediaPrefetch, nextIndex < requests.count else {
                    continue
                }
                let request = requests[nextIndex]
                nextIndex += 1
                group.addTask { [weak self] in
                    await self?.prefetchIgnoringFailure(
                        renditionId: renditionId,
                        segment: request
                    ) ?? false
                }
            }
        }
#if DEBUG
        iosHostLog(
            "dashSegmentPrefetch completed rendition=\(renditionId) mediaPrefetch=\(prefetchMediaSegments) count=\(requests.count)"
        )
#endif
    }

    private func prefetchIgnoringFailure(
        renditionId: String,
        segment: VesperDashSegmentRequest
    ) async -> Bool {
        do {
            let payload = try await segmentPayload(
                renditionId: renditionId,
                segment: segment,
                requestOrigin: "prefetch"
            )
            let shouldContinue = !(segment.isMedia && payload.isTemporaryFile)
            if !shouldContinue {
                backgroundPrefetchLargeMediaRenditionIds.insert(renditionId)
#if DEBUG
                iosHostLog(
                    "dashSegmentPrefetch stopLargeMedia rendition=\(renditionId) segment=\(segment) bytes=\(payload.size)"
                )
#endif
            }
            payload.cleanupIfTemporary()
            return shouldContinue
        } catch {
            iosHostLog(
                "dashSegmentPrefetch failed rendition=\(renditionId) segment=\(segment) error=\(error.localizedDescription)"
            )
            return true
        }
    }

    func segmentRedirectURL(renditionId: String, segment: VesperDashSegmentRequest) async throws -> URL {
        let key = VesperDashSegmentCacheKey(renditionId: renditionId, segment: segment)
        let url = segmentCacheURL(renditionId: renditionId, segment: segment)
        if cachedSegmentFileExists(for: key, at: url) {
            return url
        }

        let manifest = try await loadManifest()
        let playable = try await playableRepresentation(renditionId: renditionId)
        if let segmentTemplate = playable.representation.segmentTemplate {
            let payload = try await fetchSegmentTemplateFile(
                manifest: manifest,
                playable: playable,
                segmentTemplate: segmentTemplate,
                segment: segment,
                cacheURL: url,
                key: key,
                allowSkippingLargeMediaEntry: false,
                contentType: dashSegmentContentType(for: playable, segment: segment)
            )
            guard case let .file(fileURL, 0, _, false, _) = payload else {
                throw VesperDashBridgeError.network("DASH segment redirect requires a persistent local file")
            }
            return fileURL
        }

        let data = try await segmentData(renditionId: renditionId, segment: segment)
        _ = try writeSegmentTemplateCache(
            data,
            to: url,
            key: key,
            allowSkippingLargeMediaEntry: false
        )
        return url
    }

    private func segmentCacheURL(renditionId: String, segment: VesperDashSegmentRequest) -> URL {
        let encodedId = renditionId.addingPercentEncoding(withAllowedCharacters: dashPathComponentAllowedCharacters)
            ?? renditionId
        let fileName: String
        switch segment {
        case .initialization:
            fileName = "\(encodedId)-init.mp4"
        case let .media(index):
            fileName = "\(encodedId)-\(index).m4s"
        }
        return segmentCacheDirectory.appendingPathComponent(fileName, isDirectory: false)
    }

    private func templateSegmentURL(
        manifest: VesperDashManifest,
        playable: VesperDashPlayableRepresentation,
        segmentTemplate: VesperDashSegmentTemplate,
        segment: VesperDashSegmentRequest
    ) throws -> URL {
        let template: String
        let number: UInt64?
        let time: UInt64?
        switch segment {
        case .initialization:
            guard let initialization = segmentTemplate.initialization else {
                throw VesperDashBridgeError.unsupportedManifest(
                    "Representation \(playable.representation.id) does not provide SegmentTemplate initialization"
                )
            }
            template = initialization
            number = nil
            time = nil
        case let .media(index):
            let segments = try templateSegments(
                for: playable,
                manifest: manifest,
                segmentTemplate: segmentTemplate
            )
            let selectedSegment: VesperDashTemplateSegment
            if manifest.type == .dynamic {
                guard let matched = segments.first(where: { $0.number == UInt64(index) }) else {
                    throw VesperDashBridgeError.invalidManifest(
                        "missing media segment number \(index) for rendition \(playable.renditionId)"
                    )
                }
                selectedSegment = matched
            } else {
                guard segments.indices.contains(index) else {
                    throw VesperDashBridgeError.invalidManifest(
                        "missing media segment \(index) for rendition \(playable.renditionId)"
                    )
                }
                selectedSegment = segments[index]
            }
            template = segmentTemplate.media
            number = selectedSegment.number
            time = selectedSegment.time
        }

        return try expandedTemplateURL(
            playable: playable,
            template: template,
            number: number,
            time: time
        )
    }

    private func expandedTemplateURL(
        playable: VesperDashPlayableRepresentation,
        template: String,
        number: UInt64?,
        time: UInt64?
    ) throws -> URL {
        let expanded = try VesperDashTemplateExpander.expand(
            template,
            representation: playable.representation,
            number: number,
            time: time
        )
        let resolved = resolveDashURI(base: playable.representation.baseURL, reference: expanded)
        guard let url = URL(string: resolved) else {
            throw VesperDashBridgeError.invalidManifest("invalid segment URL \(resolved)")
        }
        return url
    }

    private func selectedPlayableRepresentations(
        manifest: VesperDashManifest,
        variantPolicy: VesperDashMasterPlaylistVariantPolicy
    ) throws -> VesperDashSelectedPlayableResponse {
        if let cached = selectedPlayableByPolicy[variantPolicy] {
            return cached
        }
        let selected = try VesperDashHlsBuilder.selectedPlayableRepresentations(
            manifest: manifest,
            variantPolicy: variantPolicy
        )
        let response = VesperDashSelectedPlayableResponse(
            audio: selected.audio,
            video: selected.video,
            subtitles: selected.subtitles
        )
        selectedPlayableByPolicy[variantPolicy] = response
        if variantPolicy == .all {
            playableByRenditionId = Dictionary(
                uniqueKeysWithValues: (response.audio + response.video + response.subtitles).map {
                    ($0.renditionId, $0)
                }
            )
        }
        return response
    }

    private func mediaSegments(
        for playable: VesperDashPlayableRepresentation,
        segmentBase: VesperDashSegmentBase
    ) async throws -> [VesperDashMediaSegment] {
        if let cached = mediaSegmentsByRenditionId[playable.renditionId] {
            return cached
        }
        let sidx = try await loadSidx(for: playable)
        let segments = try VesperDashHlsBuilder.mediaSegments(
            segmentBase: segmentBase,
            sidx: sidx
        )
        mediaSegmentsByRenditionId[playable.renditionId] = segments
        return segments
    }

    private func templateSegments(
        for playable: VesperDashPlayableRepresentation,
        manifest: VesperDashManifest,
        segmentTemplate: VesperDashSegmentTemplate
    ) throws -> [VesperDashTemplateSegment] {
        if let cached = templateSegmentsByRenditionId[playable.renditionId] {
            return cached
        }
        let segments = try VesperDashHlsBuilder.templateSegments(
            manifestType: manifest.type,
            durationMs: manifest.durationMs,
            segmentTemplate: segmentTemplate
        )
        templateSegmentsByRenditionId[playable.renditionId] = segments
        return segments
    }

    private func loadManifest() async throws -> VesperDashManifest {
        if let manifest, !shouldRefreshManifest(manifest) {
            return manifest
        }
        let data = try await networkClient.data(for: sourceURL)
        let parsed = try VesperDashManifestParser.parse(data: data, manifestURL: sourceURL)
        if manifest != nil, parsed.type == .dynamic {
            clearManifestDerivedCaches()
        }
        manifest = parsed
        manifestLoadedAt = Date()
        return parsed
    }

    private func shouldRefreshManifest(_ manifest: VesperDashManifest) -> Bool {
        guard manifest.type == .dynamic else {
            return false
        }
        guard let manifestLoadedAt else {
            return true
        }
        let refreshIntervalMs = max(
            manifest.minimumUpdatePeriodMs ?? Self.defaultDynamicManifestRefreshIntervalMs,
            Self.minimumDynamicManifestRefreshIntervalMs
        )
        return Date().timeIntervalSince(manifestLoadedAt) * 1_000 >= Double(refreshIntervalMs)
    }

    private func clearManifestDerivedCaches() {
        masterPlaylistCache = nil
        mediaPlaylistCacheByRenditionId = [:]
        selectedPlayableByPolicy = [:]
        playableByRenditionId = [:]
        sidxByRenditionId = [:]
        mediaSegmentsByRenditionId = [:]
        templateSegmentsByRenditionId = [:]
    }

    private func playableRepresentation(renditionId: String) async throws -> VesperDashPlayableRepresentation {
        if let cached = playableByRenditionId[renditionId] {
            return cached
        }
        let manifest = try await loadManifest()
        let selected = try selectedPlayableRepresentations(
            manifest: manifest,
            variantPolicy: .all
        )
        guard let playable = (selected.audio + selected.video + selected.subtitles).first(where: {
            $0.renditionId == renditionId
        }) else {
            throw VesperDashBridgeError.invalidManifest(
                "missing DASH representation for rendition \(renditionId)"
            )
        }
        return playable
    }

    private func loadSidx(for playable: VesperDashPlayableRepresentation) async throws -> VesperDashSidxBox {
        if let cached = sidxByRenditionId[playable.renditionId] {
            return cached
        }
        guard let segmentBase = playable.representation.segmentBase else {
            throw VesperDashBridgeError.unsupportedManifest(
                "Representation \(playable.representation.id) does not use SegmentBase"
            )
        }
        guard let mediaURL = URL(string: playable.representation.baseURL) else {
            throw VesperDashBridgeError.invalidManifest(
                "invalid media URL \(playable.representation.baseURL)"
            )
        }
        let data = try await networkClient.data(for: mediaURL, byteRange: segmentBase.indexRange)
        let sidx = try VesperDashSidxParser.parse(data: data)
        sidxByRenditionId[playable.renditionId] = sidx
        return sidx
    }
}

final class VesperDashNetworkClient {
    private let headers: [String: String]

    init(headers: [String: String] = [:]) {
        self.headers = headers
    }

    func data(for url: URL, byteRange: VesperDashByteRange? = nil) async throws -> Data {
        if url.isFileURL {
            return try readLocalFile(url: url, byteRange: byteRange)
        }

        var request = URLRequest(url: url)
        applyHttpHeaders(headers, to: &request)
        if let byteRange {
            request.setValue("bytes=\(byteRange.start)-\(byteRange.end)", forHTTPHeaderField: "Range")
        }
        let (data, response) = try await URLSession.shared.data(for: request)
        if let httpResponse = response as? HTTPURLResponse,
           !(200...299).contains(httpResponse.statusCode) {
            throw VesperDashBridgeError.network("HTTP \(httpResponse.statusCode) for \(url.absoluteString)")
        }
        return data
    }

    func download(
        for url: URL,
        byteRange: VesperDashByteRange? = nil,
        to destinationURL: URL
    ) async throws -> UInt64 {
        try FileManager.default.createDirectory(
            at: destinationURL.deletingLastPathComponent(),
            withIntermediateDirectories: true
        )
        try? FileManager.default.removeItem(at: destinationURL)

        if url.isFileURL {
            return try copyLocalFile(url: url, byteRange: byteRange, to: destinationURL)
        }

        var request = URLRequest(url: url)
        applyHttpHeaders(headers, to: &request)
        if let byteRange {
            request.setValue("bytes=\(byteRange.start)-\(byteRange.end)", forHTTPHeaderField: "Range")
        }
        let (temporaryURL, response) = try await URLSession.shared.download(for: request)
        if let httpResponse = response as? HTTPURLResponse,
           !(200...299).contains(httpResponse.statusCode) {
            try? FileManager.default.removeItem(at: temporaryURL)
            throw VesperDashBridgeError.network("HTTP \(httpResponse.statusCode) for \(url.absoluteString)")
        }
        try FileManager.default.moveItem(at: temporaryURL, to: destinationURL)
        return fileSize(at: destinationURL) ?? 0
    }

    private func readLocalFile(url: URL, byteRange: VesperDashByteRange?) throws -> Data {
        guard let byteRange else {
            return try Data(contentsOf: url)
        }

        let length = try checkedInt(byteRange.length, field: "local file byte range length")
        let handle = try FileHandle(forReadingFrom: url)
        defer { try? handle.close() }
        try handle.seek(toOffset: byteRange.start)
        let data = try handle.read(upToCount: length) ?? Data()
        guard data.count == length else {
            throw VesperDashBridgeError.network("local file byte range is shorter than requested")
        }
        return data
    }

    private func copyLocalFile(
        url: URL,
        byteRange: VesperDashByteRange?,
        to destinationURL: URL
    ) throws -> UInt64 {
        guard let byteRange else {
            try FileManager.default.copyItem(at: url, to: destinationURL)
            return fileSize(at: destinationURL) ?? 0
        }

        let input = try FileHandle(forReadingFrom: url)
        defer { try? input.close() }
        FileManager.default.createFile(atPath: destinationURL.path, contents: nil)
        let output = try FileHandle(forWritingTo: destinationURL)
        defer { try? output.close() }

        try input.seek(toOffset: byteRange.start)
        var remaining = byteRange.length
        while remaining > 0 {
            let readCount = remaining > 256 * 1024 ? 256 * 1024 : Int(remaining)
            let data = try input.read(upToCount: readCount) ?? Data()
            guard !data.isEmpty else {
                throw VesperDashBridgeError.network("local file byte range is shorter than requested")
            }
            try output.write(contentsOf: data)
            remaining = remaining.saturatingSubtract(UInt64(data.count))
        }
        return byteRange.length
    }

    private func fileSize(at url: URL) -> UInt64? {
        guard
            let attributes = try? FileManager.default.attributesOfItem(atPath: url.path),
            let value = attributes[.size] as? NSNumber
        else {
            return nil
        }
        return value.uint64Value
    }
}

private struct VesperMp4BoxHeader {
    let boxType: [UInt8]
    let end: Int

    static func parse(bytes: [UInt8], start: Int) throws -> VesperMp4BoxHeader {
        let remaining = bytes.count - start
        guard remaining >= 8 else {
            throw VesperDashBridgeError.invalidMp4("truncated MP4 box header")
        }
        let size32 = try readBigEndianUInt32(bytes, offset: start, field: "MP4 box size")
        let boxType = Array(bytes[(start + 4)..<(start + 8)])
        let boxSize: Int
        let headerSize: Int
        if size32 == 0 {
            boxSize = remaining
            headerSize = 8
        } else if size32 == 1 {
            guard remaining >= 16 else {
                throw VesperDashBridgeError.invalidMp4("truncated extended MP4 box header")
            }
            let size64 = try readBigEndianUInt64(bytes, offset: start + 8, field: "extended MP4 box size")
            boxSize = try checkedInt(size64, field: "extended MP4 box size")
            headerSize = 16
        } else {
            boxSize = Int(size32)
            headerSize = 8
        }
        guard boxSize >= headerSize else {
            throw VesperDashBridgeError.invalidMp4("MP4 box size is smaller than its header")
        }
        guard boxSize <= remaining else {
            throw VesperDashBridgeError.invalidMp4("MP4 box exceeds input data")
        }
        return VesperMp4BoxHeader(
            boxType: boxType,
            end: start + boxSize
        )
    }
}

private func resolveDashURI(base: String, reference: String) -> String {
    let reference = reference.trimmingCharacters(in: .whitespacesAndNewlines)
    guard !reference.isEmpty else { return base }
    if URL(string: reference)?.scheme != nil {
        return reference
    }
    guard let baseURL = URL(string: base),
          let resolved = URL(string: reference, relativeTo: baseURL)?.absoluteURL
    else {
        return reference
    }
    return resolved.absoluteString
}

private func dashSegmentTypeName(_ representation: VesperDashRepresentation) -> String {
    if representation.segmentTemplate != nil {
        return "template"
    }
    if representation.segmentBase != nil {
        return "base"
    }
    return "unknown"
}

private func emptyAsNil(_ value: String) -> String {
    value.isEmpty ? "nil" : value
}

private func dashSegmentKindName(_ segment: VesperDashSegmentRequest) -> String {
    switch segment {
    case .initialization:
        return "initialization"
    case .media:
        return "media"
    }
}

func applyHttpHeaders(_ headers: [String: String], to request: inout URLRequest) {
    for (field, value) in headers where !field.isEmpty {
        request.setValue(value, forHTTPHeaderField: field)
    }
}

let vesperAVURLAssetHTTPHeaderFieldsKey = "AVURLAssetHTTPHeaderFieldsKey"

private let dashPathComponentAllowedCharacters: CharacterSet = {
    var characters = CharacterSet.urlPathAllowed
    characters.remove(charactersIn: "/")
    return characters
}()

private func checkedInt(_ value: UInt64, field: String) throws -> Int {
    guard value <= UInt64(Int.max) else {
        throw VesperDashBridgeError.invalidMp4("\(field) exceeds Int.max")
    }
    return Int(value)
}

private func startupPrefetchSegmentIndices(count: Int) -> [Int] {
    guard count > 0 else {
        return []
    }
    let candidates = [
        0,
        min(1, count - 1),
        min((count + 2) / 3, count - 1),
        min(((count * 2) + 2) / 3, count - 1),
    ]
    return Array(Set(candidates)).sorted()
}

private func backgroundPrefetchRequests(
    count: Int,
    includeMediaSegments: Bool = true
) -> [VesperDashSegmentRequest] {
    guard includeMediaSegments, count > 0 else {
        return [.initialization]
    }
    let prioritized = startupPrefetchSegmentIndices(count: count)
    let orderedIndices = prioritized + (0..<count).filter { !prioritized.contains($0) }
    return [.initialization] + orderedIndices.map(VesperDashSegmentRequest.media)
}

private func readBigEndianUInt32(_ bytes: [UInt8], offset: Int, field: String) throws -> UInt32 {
    guard offset >= 0, offset + 4 <= bytes.count else {
        throw VesperDashBridgeError.invalidMp4("truncated MP4 field \(field)")
    }
    return (UInt32(bytes[offset]) << 24)
        | (UInt32(bytes[offset + 1]) << 16)
        | (UInt32(bytes[offset + 2]) << 8)
        | UInt32(bytes[offset + 3])
}

private func readBigEndianUInt64(_ bytes: [UInt8], offset: Int, field: String) throws -> UInt64 {
    guard offset >= 0, offset + 8 <= bytes.count else {
        throw VesperDashBridgeError.invalidMp4("truncated MP4 field \(field)")
    }
    var value: UInt64 = 0
    for byte in bytes[offset..<(offset + 8)] {
        value = (value << 8) | UInt64(byte)
    }
    return value
}

private extension UInt64 {
    func saturatingAdd(_ rhs: UInt64) -> UInt64 {
        let (value, overflow) = addingReportingOverflow(rhs)
        return overflow ? UInt64.max : value
    }

    func saturatingSubtract(_ rhs: UInt64) -> UInt64 {
        let (value, overflow) = subtractingReportingOverflow(rhs)
        return overflow ? 0 : value
    }
}
