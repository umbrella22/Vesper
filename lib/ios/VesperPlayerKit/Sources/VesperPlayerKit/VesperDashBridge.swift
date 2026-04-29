@preconcurrency import AVFoundation
import Foundation
@preconcurrency import Network

enum VesperDashBridgeError: LocalizedError {
    case invalidManifest(String)
    case unsupportedManifest(String)
    case invalidMp4(String)
    case unsupportedMp4(String)
    case network(String)

    var errorDescription: String? {
        switch self {
        case let .invalidManifest(message):
            "Invalid DASH manifest: \(message)"
        case let .unsupportedManifest(message):
            "Unsupported DASH manifest: \(message)"
        case let .invalidMp4(message):
            "Invalid MP4 index: \(message)"
        case let .unsupportedMp4(message):
            "Unsupported MP4 index: \(message)"
        case let .network(message):
            "DASH network request failed: \(message)"
        }
    }
}

struct VesperDashByteRange: Equatable {
    let start: UInt64
    let end: UInt64

    var length: UInt64 {
        end - start + 1
    }

    init(start: UInt64, end: UInt64) throws {
        guard end >= start else {
            throw VesperDashBridgeError.invalidManifest("byte range end is smaller than start")
        }
        self.start = start
        self.end = end
    }
}

struct VesperDashSegmentBase: Equatable {
    let initialization: VesperDashByteRange
    let indexRange: VesperDashByteRange
}

struct VesperDashSegmentTemplate: Equatable {
    let timescale: UInt64
    let duration: UInt64?
    let startNumber: UInt64
    let presentationTimeOffset: UInt64
    let initialization: String
    let media: String
    let timeline: [VesperDashSegmentTimelineEntry]
}

struct VesperDashSegmentTimelineEntry: Equatable {
    let startTime: UInt64?
    let duration: UInt64
    let repeatCount: Int
}

enum VesperDashAdaptationKind: String, Equatable {
    case video
    case audio
    case subtitle
    case unknown
}

struct VesperDashRepresentation: Equatable {
    let id: String
    let baseURL: String
    let mimeType: String
    let codecs: String
    let bandwidth: UInt64?
    let width: Int?
    let height: Int?
    let frameRate: String?
    let audioSamplingRate: String?
    let segmentBase: VesperDashSegmentBase?
    let segmentTemplate: VesperDashSegmentTemplate?
}

struct VesperDashAdaptationSet: Equatable {
    let id: String?
    let kind: VesperDashAdaptationKind
    let mimeType: String?
    let language: String?
    let representations: [VesperDashRepresentation]
}

struct VesperDashPeriod: Equatable {
    let id: String?
    let adaptationSets: [VesperDashAdaptationSet]
}

struct VesperDashManifest: Equatable {
    let durationMs: UInt64?
    let minBufferTimeMs: UInt64?
    let periods: [VesperDashPeriod]
}

struct VesperDashPlayableRepresentation: Equatable {
    let renditionId: String
    let adaptationSet: VesperDashAdaptationSet
    let representation: VesperDashRepresentation
}

enum VesperDashMasterPlaylistVariantPolicy: Equatable {
    case all
    case startupSingleVariant
}

struct VesperDashSidxBox: Equatable {
    let timescale: UInt32
    let earliestPresentationTime: UInt64
    let firstOffset: UInt64
    let references: [VesperDashSidxReference]
}

struct VesperDashSidxReference: Equatable {
    let referenceType: UInt8
    let referencedSize: UInt32
    let subsegmentDuration: UInt32
    let startsWithSap: Bool
    let sapType: UInt8
    let sapDeltaTime: UInt32
}

struct VesperDashMediaSegment: Equatable {
    let duration: Double
    let range: VesperDashByteRange
}

struct VesperDashTemplateSegment: Equatable {
    let duration: Double
    let number: UInt64
    let time: UInt64?
}

struct VesperDashHlsMap: Equatable {
    let uri: String
    let byteRange: VesperDashByteRange?
}

struct VesperDashHlsSegment: Equatable {
    let duration: Double
    let uri: String
    let byteRange: VesperDashByteRange?
}

enum VesperDashSegmentRequest: Hashable {
    case initialization
    case media(Int)
}

private struct VesperDashSegmentCacheKey: Hashable {
    let renditionId: String
    let segment: VesperDashSegmentRequest
}

enum VesperDashRoute: Equatable {
    case master
    case media(String)
    case segment(String, VesperDashSegmentRequest)
}

private enum VesperDashResourceResponse {
    case data(Data, contentType: String)
    case redirect(URL)
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

final class VesperDashLoopbackServer: @unchecked Sendable {
    typealias SegmentDataProvider = @Sendable (String, VesperDashSegmentRequest) async throws -> Data

    private let sessionId: String
    private let listener: NWListener
    private let queue: DispatchQueue
    private let segmentDataProvider: SegmentDataProvider
    private var port: UInt16?
    private var didStart = false

    init(
        sessionId: String,
        segmentDataProvider: @escaping SegmentDataProvider
    ) throws {
        let parameters = NWParameters.tcp
        parameters.requiredLocalEndpoint = .hostPort(
            host: .ipv4(IPv4Address("127.0.0.1")!),
            port: 0
        )
        listener = try NWListener(using: parameters)
        queue = DispatchQueue(label: "io.github.ikaros.vesper.player.dash.loopback.\(sessionId)")
        self.sessionId = sessionId
        self.segmentDataProvider = segmentDataProvider
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
        let range: ClosedRange<Int>?
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

        var range: ClosedRange<Int>?
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
    private func parseRangeHeader(_ value: String) -> ClosedRange<Int>? {
        guard let equals = value.firstIndex(of: "=") else { return nil }
        let unit = value[..<equals].trimmingCharacters(in: .whitespaces).lowercased()
        guard unit == "bytes" else { return nil }
        let spec = value[value.index(after: equals)...]
        guard let dash = spec.firstIndex(of: "-") else { return nil }
        let startText = spec[..<dash].trimmingCharacters(in: .whitespaces)
        let endText = spec[spec.index(after: dash)...].trimmingCharacters(in: .whitespaces)
        guard let start = Int(startText), start >= 0 else { return nil }
        if endText.isEmpty {
            return start...Int.max
        }
        guard let end = Int(endText), end >= start else { return nil }
        return start...end
    }

    private func sendSegment(
        _ request: ParsedRequest,
        on connection: NWConnection
    ) {
        let startedAt = Date()
        Task {
            do {
                let data = try await self.segmentDataProvider(request.renditionId, request.segment)
                let elapsedMs = Int(Date().timeIntervalSince(startedAt) * 1_000)
                self.queue.async {
                    self.sendDataResponse(
                        data,
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

    private func sendDataResponse(
        _ data: Data,
        elapsedMs: Int,
        request: ParsedRequest,
        on connection: NWConnection
    ) {
        let totalLength = data.count
        let body: Data
        let statusLine: String
        let contentRange: String?
        if let range = request.range {
            let start = min(range.lowerBound, totalLength)
            let end = min(range.upperBound, totalLength - 1)
            if start >= totalLength || end < start {
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
            body = data.subdata(in: start..<(end + 1))
            statusLine = "HTTP/1.1 206 Partial Content\r\n"
            contentRange = "Content-Range: bytes \(start)-\(end)/\(totalLength)\r\n"
        } else {
            body = data
            statusLine = "HTTP/1.1 200 OK\r\n"
            contentRange = nil
        }
        var header = statusLine
            + "Content-Type: video/mp4\r\n"
            + "Content-Length: \(body.count)\r\n"
            + "Accept-Ranges: bytes\r\n"
            + "Cache-Control: no-store\r\n"
            + "Connection: close\r\n"
        if let contentRange {
            header += contentRange
        }
        header += "\r\n"
        var response = Data(header.utf8)
        // HEAD 不可以附带 body，否则 AVPlayer 会把 body 字节误当下一个响应的一部分。
        if request.method == .get {
            response.append(body)
        }
#if DEBUG
        if elapsedMs >= 500 {
            iosHostLog(
                "dashLoopbackSegment served rendition=\(request.renditionId) segment=\(request.segment) method=\(request.method) bytes=\(body.count)/\(totalLength) elapsedMs=\(elapsedMs)"
            )
        }
#endif
        connection.send(
            content: response,
            isComplete: true,
            completion: .contentProcessed { [weak self] _ in
                self?.scheduleGracefulClose(connection)
            }
        )
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
    nonisolated static let scheme = "vesper-dash"

    nonisolated let id: String
    nonisolated let sourceURL: URL
    nonisolated let segmentCacheDirectory: URL

    private let networkClient: VesperDashNetworkClient
    private var manifest: VesperDashManifest?
    private var sidxByRenditionId: [String: VesperDashSidxBox] = [:]
    private var segmentDataTasks: [VesperDashSegmentCacheKey: Task<Data, Error>] = [:]
    private var backgroundPrefetchRenditionIds: Set<String> = []
    private var loopbackServer: VesperDashLoopbackServer?
    private var loopbackServerStartTask: Task<VesperDashLoopbackServer, Error>?

    nonisolated var masterPlaylistURL: URL {
        URL(string: "\(Self.scheme)://master/\(id)/master.m3u8")!
    }

    init(
        sourceURL: URL,
        headers: [String: String] = [:],
        networkClient: VesperDashNetworkClient? = nil
    ) {
        let sessionId = UUID().uuidString
        id = sessionId
        self.sourceURL = sourceURL
        segmentCacheDirectory = FileManager.default.temporaryDirectory
            .appendingPathComponent("vesper-dash-\(sessionId)", isDirectory: true)
        self.networkClient = networkClient ?? VesperDashNetworkClient(headers: headers)
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
        let manifest = try await loadManifest()
        let variantPolicy = VesperDashMasterPlaylistVariantPolicy.startupSingleVariant
        let selected = try VesperDashHlsBuilder.selectedPlayableRepresentations(
            manifest: manifest,
            variantPolicy: variantPolicy
        )
        let playlist = try VesperDashHlsBuilder.buildMasterPlaylist(
            manifest: manifest,
            variantPolicy: variantPolicy,
            mediaURL: { [weak self] renditionId in
                guard let self else { return "" }
                return self.mediaPlaylistURL(for: renditionId).absoluteString
            }
        )
        startBackgroundPrefetch(for: selected.audio + selected.video, manifest: manifest)
#if DEBUG
        iosHostLog(
            "dashMasterPlaylist policy=startupSingleVariant video=\(selected.video.map(\.renditionId).joined(separator: ",")) audio=\(selected.audio.map(\.renditionId).joined(separator: ","))"
        )
#endif
        return Data(playlist.utf8)
    }

    func mediaPlaylistData(renditionId: String) async throws -> Data {
        let manifest = try await loadManifest()
        let playable = try await playableRepresentation(renditionId: renditionId)
        if let segmentBase = playable.representation.segmentBase {
            let sidx = try await loadSidx(for: playable)
            let segments = try VesperDashHlsBuilder.mediaSegments(segmentBase: segmentBase, sidx: sidx)
            let mediaURL = playable.representation.baseURL
            let playlist = try VesperDashHlsBuilder.buildExternalMediaPlaylist(
                map: VesperDashHlsMap(uri: mediaURL, byteRange: segmentBase.initialization),
                segments: segments.map {
                    VesperDashHlsSegment(
                        duration: $0.duration,
                        uri: mediaURL,
                        byteRange: $0.range
                    )
                }
            )
            return Data(playlist.utf8)
        }

        guard let segmentTemplate = playable.representation.segmentTemplate else {
            throw VesperDashBridgeError.unsupportedManifest(
                "Representation \(playable.representation.id) does not use SegmentBase or SegmentTemplate"
            )
        }
        let segments = try VesperDashHlsBuilder.templateSegments(
            durationMs: manifest.durationMs,
            segmentTemplate: segmentTemplate
        )
        let server = try await dashLoopbackServer()
        startBackgroundSegmentPrefetch(
            renditionId: playable.renditionId,
            segmentCount: segments.count
        )
        // EXT-X-MAP 指向 vesper-dash:// scheme，走 AVAssetResourceLoaderDelegate
        // 主路径。实验表明当 EXT-X-MAP 使用 loopback http URL 时，AVPlayer
        // 可能不会发起该请求（潜在原因是 AVPlayer 在验证同一 origin
        // 路径才会复用 loopback http），导致 init 段未交付 → 'frmt'。
        // 走 vesper-dash:// scheme 可以保证 AVPlayer 调用 resource loader
        // delegate，从而能记录 init 段是否被请求。
        let initializationURL = self.segmentURL(
            for: playable.renditionId,
            segment: .initialization
        )
        let playlist = try VesperDashHlsBuilder.buildExternalMediaPlaylist(
            map: VesperDashHlsMap(uri: initializationURL.absoluteString, byteRange: nil),
            segments: try segments.enumerated().map { index, segment in
                let segmentURL = try server.segmentURL(
                    for: playable.renditionId,
                    segment: .media(index)
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
            "dashMediaPlaylist rendition=\(playable.renditionId) loopbackSegments=true count=\(segments.count) init=\(initializationURL.absoluteString)"
        )
        // 打印 playlist 头部 7 行，便于排查 HLS 标签拼接错误（曾因 multiline
        // 字符串末尾缺换行导致 EXT-X-PLAYLIST-TYPE 与 EXT-X-MAP 粘到一行）。
        let head = playlist
            .split(separator: "\n", omittingEmptySubsequences: false)
            .prefix(7)
            .joined(separator: " | ")
        iosHostLog("dashMediaPlaylist head=\(head)")
#endif
        return Data(playlist.utf8)
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
            return try await self.segmentData(renditionId: renditionId, segment: segment)
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
        let manifest = try await loadManifest()
        let playable = try await playableRepresentation(renditionId: renditionId)
        if let segmentBase = playable.representation.segmentBase {
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
                let sidx = try await loadSidx(for: playable)
                let segments = try VesperDashHlsBuilder.mediaSegments(segmentBase: segmentBase, sidx: sidx)
                guard segments.indices.contains(index) else {
                    throw VesperDashBridgeError.invalidManifest(
                        "missing media segment \(index) for rendition \(renditionId)"
                    )
                }
                byteRange = segments[index].range
            }

            return try await networkClient.data(for: mediaURL, byteRange: byteRange)
        }

        guard let segmentTemplate = playable.representation.segmentTemplate else {
            throw VesperDashBridgeError.unsupportedManifest(
                "Representation \(playable.representation.id) does not use SegmentBase or SegmentTemplate"
            )
        }
        return try await cachedSegmentTemplateData(
            manifest: manifest,
            playable: playable,
            segmentTemplate: segmentTemplate,
            segment: segment
        )
    }

    private func cachedSegmentTemplateData(
        manifest: VesperDashManifest,
        playable: VesperDashPlayableRepresentation,
        segmentTemplate: VesperDashSegmentTemplate,
        segment: VesperDashSegmentRequest
    ) async throws -> Data {
        let key = VesperDashSegmentCacheKey(
            renditionId: playable.renditionId,
            segment: segment
        )
        let cacheURL = segmentCacheURL(
            renditionId: playable.renditionId,
            segment: segment
        )
        if FileManager.default.fileExists(atPath: cacheURL.path) {
            return try Data(contentsOf: cacheURL)
        }
        if let task = segmentDataTasks[key] {
            return try await task.value
        }

        let task = Task { () throws -> Data in
            let data = try await self.fetchSegmentTemplateData(
                manifest: manifest,
                playable: playable,
                segmentTemplate: segmentTemplate,
                segment: segment
            )
            try Task.checkCancellation()
            try self.writeSegmentTemplateCache(data, to: cacheURL)
            return data
        }
        segmentDataTasks[key] = task
        do {
            let data = try await task.value
            segmentDataTasks[key] = nil
            return data
        } catch {
            segmentDataTasks[key] = nil
            throw error
        }
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
#endif

    private func writeSegmentTemplateCache(_ data: Data, to url: URL) throws {
        try FileManager.default.createDirectory(
            at: segmentCacheDirectory,
            withIntermediateDirectories: true
        )
        try data.write(to: url, options: .atomic)
    }

    private func startBackgroundSegmentPrefetch(
        renditionId: String,
        segmentCount: Int
    ) {
        guard !sourceURL.isFileURL,
              segmentCount > 0,
              !backgroundPrefetchRenditionIds.contains(renditionId)
        else {
            return
        }
        backgroundPrefetchRenditionIds.insert(renditionId)
        Task(priority: .utility) { [weak self] in
            await self?.runBackgroundSegmentPrefetch(
                renditionId: renditionId,
                segmentCount: segmentCount
            )
        }
    }

    private func startBackgroundPrefetch(
        for playables: [VesperDashPlayableRepresentation],
        manifest: VesperDashManifest
    ) {
        for playable in playables {
            guard let segmentTemplate = playable.representation.segmentTemplate,
                  let segmentCount = try? VesperDashHlsBuilder.templateSegments(
                    durationMs: manifest.durationMs,
                    segmentTemplate: segmentTemplate
                  ).count
            else {
                continue
            }
            startBackgroundSegmentPrefetch(
                renditionId: playable.renditionId,
                segmentCount: segmentCount
            )
        }
    }

    private func runBackgroundSegmentPrefetch(
        renditionId: String,
        segmentCount: Int
    ) async {
        let prefetchLimit = min(segmentCount, 120)
        let requests = backgroundPrefetchRequests(count: prefetchLimit)
        let concurrency = min(4, requests.count)
        guard concurrency > 0 else { return }

        await withTaskGroup(of: Void.self) { group in
            var nextIndex = 0
            for _ in 0..<concurrency {
                let request = requests[nextIndex]
                nextIndex += 1
                group.addTask { [weak self] in
                    await self?.prefetchIgnoringFailure(
                        renditionId: renditionId,
                        segment: request
                    )
                }
            }

            while await group.next() != nil {
                guard nextIndex < requests.count else {
                    continue
                }
                let request = requests[nextIndex]
                nextIndex += 1
                group.addTask { [weak self] in
                    await self?.prefetchIgnoringFailure(
                        renditionId: renditionId,
                        segment: request
                    )
                }
            }
        }
#if DEBUG
        iosHostLog("dashSegmentPrefetch completed rendition=\(renditionId) count=\(prefetchLimit)")
#endif
    }

    private func prefetchIgnoringFailure(
        renditionId: String,
        segment: VesperDashSegmentRequest
    ) async {
        do {
            _ = try await segmentData(
                renditionId: renditionId,
                segment: segment
            )
        } catch {
            iosHostLog(
                "dashSegmentPrefetch failed rendition=\(renditionId) segment=\(segment) error=\(error.localizedDescription)"
            )
        }
    }

    func segmentRedirectURL(renditionId: String, segment: VesperDashSegmentRequest) async throws -> URL {
        let url = segmentCacheURL(renditionId: renditionId, segment: segment)
        if FileManager.default.fileExists(atPath: url.path) {
            return url
        }

        let data = try await segmentData(renditionId: renditionId, segment: segment)
        try FileManager.default.createDirectory(
            at: segmentCacheDirectory,
            withIntermediateDirectories: true
        )
        try data.write(to: url, options: .atomic)
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
            template = segmentTemplate.initialization
            number = nil
            time = nil
        case let .media(index):
            let segments = try VesperDashHlsBuilder.templateSegments(
                durationMs: manifest.durationMs,
                segmentTemplate: segmentTemplate
            )
            guard segments.indices.contains(index) else {
                throw VesperDashBridgeError.invalidManifest(
                    "missing media segment \(index) for rendition \(playable.renditionId)"
                )
            }
            template = segmentTemplate.media
            number = segments[index].number
            time = segments[index].time
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

    private func loadManifest() async throws -> VesperDashManifest {
        if let manifest {
            return manifest
        }
        let data = try await networkClient.data(for: sourceURL)
        let parsed = try VesperDashManifestParser.parse(data: data, manifestURL: sourceURL)
        manifest = parsed
        return parsed
    }

    private func playableRepresentation(renditionId: String) async throws -> VesperDashPlayableRepresentation {
        let manifest = try await loadManifest()
        guard let playable = try manifest.playableRepresentations().first(where: {
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
}

enum VesperDashManifestParser {
    static func parse(data: Data, manifestURL: URL) throws -> VesperDashManifest {
        let document = try VesperDashXmlParser.parse(data: data)
        guard let mpd = document.children.first(where: { $0.localName == "MPD" }) else {
            throw VesperDashBridgeError.invalidManifest("missing MPD root")
        }
        let mpdType = mpd.attr("type") ?? "static"
        guard mpdType.caseInsensitiveCompare("static") == .orderedSame else {
            throw VesperDashBridgeError.unsupportedManifest("MPD type \(mpdType) is not supported")
        }

        let manifestBase = manifestURL.absoluteString
        let mpdBase = mpd.childText("BaseURL").map {
            resolveDashURI(base: manifestBase, reference: $0)
        } ?? manifestBase
        let periods = try mpd.children(named: "Period").map {
            try parsePeriod($0, inheritedBaseURL: mpdBase)
        }
        guard !periods.isEmpty else {
            throw VesperDashBridgeError.unsupportedManifest("MPD must contain at least one Period")
        }

        return VesperDashManifest(
            durationMs: mpd.attr("mediaPresentationDuration").flatMap(parseIso8601DurationMs),
            minBufferTimeMs: mpd.attr("minBufferTime").flatMap(parseIso8601DurationMs),
            periods: periods
        )
    }

    private static func parsePeriod(
        _ node: VesperDashXmlNode,
        inheritedBaseURL: String
    ) throws -> VesperDashPeriod {
        let periodBase = node.childText("BaseURL").map {
            resolveDashURI(base: inheritedBaseURL, reference: $0)
        } ?? inheritedBaseURL
        return VesperDashPeriod(
            id: node.attr("id"),
            adaptationSets: try node.children(named: "AdaptationSet").map {
                try parseAdaptationSet($0, inheritedBaseURL: periodBase)
            }
        )
    }

    private static func parseAdaptationSet(
        _ node: VesperDashXmlNode,
        inheritedBaseURL: String
    ) throws -> VesperDashAdaptationSet {
        let adaptationBase = node.childText("BaseURL").map {
            resolveDashURI(base: inheritedBaseURL, reference: $0)
        } ?? inheritedBaseURL
        let mimeType = node.attr("mimeType")
        let kind = adaptationKind(
            contentType: node.attr("contentType"),
            mimeType: mimeType,
            language: node.attr("lang")
        )
        let inheritedSegmentBase = try parseSegmentBase(node)
        let inheritedSegmentTemplate = try parseSegmentTemplate(node)
        var representations: [VesperDashRepresentation] = []
        for representation in node.children(named: "Representation") {
            let representationId = representation.attr("id") ?? "representation-\(representations.count)"
            let baseURL = representation.childText("BaseURL").map {
                resolveDashURI(base: adaptationBase, reference: $0)
            } ?? adaptationBase
            representations.append(
                VesperDashRepresentation(
                    id: representationId,
                    baseURL: baseURL,
                    mimeType: representation.attr("mimeType") ?? mimeType ?? "",
                    codecs: representation.attr("codecs") ?? node.attr("codecs") ?? "",
                    bandwidth: representation.attr("bandwidth").flatMap(UInt64.init),
                    width: representation.attr("width").flatMap(Int.init),
                    height: representation.attr("height").flatMap(Int.init),
                    frameRate: representation.attr("frameRate"),
                    audioSamplingRate: representation.attr("audioSamplingRate"),
                    segmentBase: try parseSegmentBase(representation) ?? inheritedSegmentBase,
                    segmentTemplate: try parseSegmentTemplate(representation) ?? inheritedSegmentTemplate
                )
            )
        }
        return VesperDashAdaptationSet(
            id: node.attr("id"),
            kind: kind,
            mimeType: mimeType,
            language: node.attr("lang"),
            representations: representations
        )
    }

    private static func adaptationKind(
        contentType: String?,
        mimeType: String?,
        language: String?
    ) -> VesperDashAdaptationKind {
        let contentType = contentType?.lowercased() ?? ""
        let mimeType = mimeType?.lowercased() ?? ""
        if contentType == "video" || mimeType.hasPrefix("video/") {
            return .video
        }
        if contentType == "audio" || mimeType.hasPrefix("audio/") {
            return .audio
        }
        if contentType == "text" ||
            contentType == "subtitle" ||
            mimeType.contains("vtt") ||
            (language != nil && mimeType.hasPrefix("text/")) {
            return .subtitle
        }
        return .unknown
    }

    private static func parseSegmentBase(_ node: VesperDashXmlNode) throws -> VesperDashSegmentBase? {
        guard let segmentBase = node.children(named: "SegmentBase").first,
              let indexRangeValue = segmentBase.attr("indexRange"),
              let initializationValue = segmentBase.children(named: "Initialization").first?.attr("range")
        else {
            return nil
        }
        return VesperDashSegmentBase(
            initialization: try parseByteRange(initializationValue),
            indexRange: try parseByteRange(indexRangeValue)
        )
    }

    private static func parseSegmentTemplate(_ node: VesperDashXmlNode) throws -> VesperDashSegmentTemplate? {
        guard let segmentTemplate = node.children(named: "SegmentTemplate").first else {
            return nil
        }
        let duration = try parsePositiveUInt64(
            segmentTemplate.attr("duration"),
            field: "SegmentTemplate duration"
        )
        let timescale = segmentTemplate.attr("timescale").flatMap(UInt64.init) ?? 1
        guard timescale > 0 else {
            throw VesperDashBridgeError.invalidManifest("SegmentTemplate timescale must be positive")
        }
        let startNumber = segmentTemplate.attr("startNumber").flatMap(UInt64.init) ?? 1
        let presentationTimeOffset = segmentTemplate.attr("presentationTimeOffset").flatMap(UInt64.init) ?? 0
        let timeline = try parseSegmentTimeline(segmentTemplate)
        guard duration != nil || !timeline.isEmpty else {
            throw VesperDashBridgeError.unsupportedManifest(
                "SegmentTemplate requires duration or SegmentTimeline"
            )
        }
        guard let initialization = segmentTemplate.attr("initialization"),
              let media = segmentTemplate.attr("media"),
              !initialization.isEmpty,
              !media.isEmpty
        else {
            throw VesperDashBridgeError.unsupportedManifest(
                "SegmentTemplate must provide initialization and media templates"
            )
        }
        return VesperDashSegmentTemplate(
            timescale: timescale,
            duration: duration,
            startNumber: startNumber,
            presentationTimeOffset: presentationTimeOffset,
            initialization: initialization,
            media: media,
            timeline: timeline
        )
    }

    private static func parseSegmentTimeline(_ node: VesperDashXmlNode) throws -> [VesperDashSegmentTimelineEntry] {
        guard let timeline = node.children(named: "SegmentTimeline").first else {
            return []
        }
        let entries = try timeline.children(named: "S").map { entry -> VesperDashSegmentTimelineEntry in
            guard let duration = try parsePositiveUInt64(entry.attr("d"), field: "SegmentTimeline S@d") else {
                throw VesperDashBridgeError.invalidManifest("SegmentTimeline S must provide positive duration")
            }
            let repeatCount: Int
            if let repeatValue = entry.attr("r") {
                guard let parsed = Int(repeatValue), parsed >= -1 else {
                    throw VesperDashBridgeError.invalidManifest("invalid SegmentTimeline repeat count \(repeatValue)")
                }
                repeatCount = parsed
            } else {
                repeatCount = 0
            }
            return VesperDashSegmentTimelineEntry(
                startTime: entry.attr("t").flatMap(UInt64.init),
                duration: duration,
                repeatCount: repeatCount
            )
        }
        guard !entries.isEmpty else {
            throw VesperDashBridgeError.invalidManifest("SegmentTimeline must contain at least one S entry")
        }
        return entries
    }

    private static func parsePositiveUInt64(_ value: String?, field: String) throws -> UInt64? {
        guard let value else {
            return nil
        }
        guard let parsed = UInt64(value), parsed > 0 else {
            throw VesperDashBridgeError.invalidManifest("\(field) must be a positive integer")
        }
        return parsed
    }

    private static func parseByteRange(_ value: String) throws -> VesperDashByteRange {
        let parts = value.split(separator: "-", maxSplits: 1).map { $0.trimmingCharacters(in: .whitespacesAndNewlines) }
        guard parts.count == 2,
              let start = UInt64(parts[0]),
              let end = UInt64(parts[1])
        else {
            throw VesperDashBridgeError.invalidManifest("invalid byte range \(value)")
        }
        return try VesperDashByteRange(start: start, end: end)
    }

    private static func parseIso8601DurationMs(_ value: String) -> UInt64? {
        guard value.hasPrefix("PT") else { return nil }
        var number = ""
        var seconds = 0.0
        for character in value.dropFirst(2) {
            if character.isNumber || character == "." {
                number.append(character)
                continue
            }
            guard let parsed = Double(number) else { return nil }
            number.removeAll(keepingCapacity: true)
            switch character {
            case "H":
                seconds += parsed * 3_600
            case "M":
                seconds += parsed * 60
            case "S":
                seconds += parsed
            default:
                return nil
            }
        }
        guard number.isEmpty, seconds.isFinite, seconds >= 0 else { return nil }
        return UInt64((seconds * 1_000).rounded())
    }
}

enum VesperDashSidxParser {
    static func parse(data: Data) throws -> VesperDashSidxBox {
        let bytes = [UInt8](data)
        var cursor = 0
        while cursor < bytes.count {
            let header = try VesperMp4BoxHeader.parse(bytes: bytes, start: cursor)
            if header.boxType == [UInt8](arrayLiteral: 0x73, 0x69, 0x64, 0x78) {
                return try parseSidxPayload(Array(bytes[header.payloadStart..<header.end]))
            }
            cursor = header.end
        }
        throw VesperDashBridgeError.invalidMp4("missing sidx box")
    }

    private static func parseSidxPayload(_ bytes: [UInt8]) throws -> VesperDashSidxBox {
        var reader = VesperMp4Reader(bytes: bytes)
        let version = try reader.readUInt8(field: "sidx version")
        _ = try reader.readUInt24(field: "sidx flags")
        _ = try reader.readUInt32(field: "sidx reference_ID")
        let timescale = try reader.readUInt32(field: "sidx timescale")
        guard timescale != 0 else {
            throw VesperDashBridgeError.invalidMp4("sidx timescale must be non-zero")
        }

        let earliestPresentationTime: UInt64
        let firstOffset: UInt64
        switch version {
        case 0:
            earliestPresentationTime = UInt64(try reader.readUInt32(field: "sidx earliest_presentation_time"))
            firstOffset = UInt64(try reader.readUInt32(field: "sidx first_offset"))
        case 1:
            earliestPresentationTime = try reader.readUInt64(field: "sidx earliest_presentation_time")
            firstOffset = try reader.readUInt64(field: "sidx first_offset")
        default:
            throw VesperDashBridgeError.unsupportedMp4("unsupported sidx version \(version)")
        }

        _ = try reader.readUInt16(field: "sidx reserved")
        let referenceCount = try reader.readUInt16(field: "sidx reference_count")
        var references: [VesperDashSidxReference] = []
        references.reserveCapacity(Int(referenceCount))
        for _ in 0..<referenceCount {
            let reference = try reader.readUInt32(field: "sidx reference")
            let subsegmentDuration = try reader.readUInt32(field: "sidx subsegment_duration")
            let sap = try reader.readUInt32(field: "sidx SAP")
            references.append(
                VesperDashSidxReference(
                    referenceType: UInt8((reference >> 31) & 0x01),
                    referencedSize: reference & 0x7fff_ffff,
                    subsegmentDuration: subsegmentDuration,
                    startsWithSap: (sap & 0x8000_0000) != 0,
                    sapType: UInt8((sap >> 28) & 0x07),
                    sapDeltaTime: sap & 0x0fff_ffff
                )
            )
        }

        return VesperDashSidxBox(
            timescale: timescale,
            earliestPresentationTime: earliestPresentationTime,
            firstOffset: firstOffset,
            references: references
        )
    }
}

enum VesperDashMp4BoxFilter {
    static func removingTopLevelSidxBoxes(from data: Data) throws -> Data {
        let bytes = [UInt8](data)
        var cursor = 0
        var keptRanges: [Range<Int>] = []
        var removedSidx = false
        let sidxType = [UInt8](arrayLiteral: 0x73, 0x69, 0x64, 0x78)

        while cursor < bytes.count {
            let header = try VesperMp4BoxHeader.parse(bytes: bytes, start: cursor)
            if header.boxType == sidxType {
                removedSidx = true
            } else {
                keptRanges.append(cursor..<header.end)
            }
            cursor = header.end
        }

        guard removedSidx else {
            return data
        }

        var output = Data(capacity: data.count)
        for range in keptRanges {
            output.append(data.subdata(in: range))
        }
        return output
    }
}

enum VesperDashHlsBuilder {
    /// 构造 HLS master playlist。
    ///
    /// 实现注意：所有 playlist 文本生成都使用 `[String]` lines + `joined("\n")` 模式，
    /// 不再使用多行字符串字面量直接 `+=` 拼接。原因：Swift 多行字面量结尾的 `"""`
    /// 会吞掉前一个换行，曾经导致 `#EXT-X-PLAYLIST-TYPE:VOD` 与后面 `#EXT-X-MAP`
    /// 粘到同一行，HLS 解析器静默忽略未知整行 → AVPlayer 拿不到 init segment
    /// → 报 `'frmt'`。逐行 append 在物理上不可能粘行。
    static func buildMasterPlaylist(
        manifest: VesperDashManifest,
        variantPolicy: VesperDashMasterPlaylistVariantPolicy = .all,
        mediaURL: (String) -> String
    ) throws -> String {
        let selected = try selectedPlayableRepresentations(
            manifest: manifest,
            variantPolicy: variantPolicy
        )
        let audio = selected.audio
        let video = selected.video
        var lines: [String] = [
            "#EXTM3U",
            "#EXT-X-VERSION:7",
            "#EXT-X-INDEPENDENT-SEGMENTS",
        ]

        if !audio.isEmpty, !video.isEmpty {
            for (index, item) in audio.enumerated() {
                let name = item.adaptationSet.language ?? item.adaptationSet.id ?? "audio-\(index + 1)"
                var attrs = "TYPE=AUDIO,GROUP-ID=\"audio\",NAME=\"\(escapeAttribute(name))\",DEFAULT=\(index == 0 ? "YES" : "NO"),AUTOSELECT=YES,URI=\"\(escapeAttribute(mediaURL(item.renditionId)))\""
                if let language = item.adaptationSet.language {
                    attrs += ",LANGUAGE=\"\(escapeAttribute(language))\""
                }
                lines.append("#EXT-X-MEDIA:\(attrs)")
            }
        }

        if video.isEmpty {
            for item in audio {
                try appendVariantLines(
                    &lines,
                    item: item,
                    extraCodecs: [],
                    extraBandwidth: 0,
                    audioGroup: nil,
                    mediaURL: mediaURL
                )
            }
        } else {
            let audioCodecs = uniqueCodecs(audio.map { $0.representation.codecs })
            let maxAudioBandwidth = audio.compactMap { $0.representation.bandwidth }.max() ?? 0
            for item in video {
                try appendVariantLines(
                    &lines,
                    item: item,
                    extraCodecs: audioCodecs,
                    extraBandwidth: maxAudioBandwidth,
                    audioGroup: audio.isEmpty ? nil : "audio",
                    mediaURL: mediaURL
                )
            }
        }

        // 末尾追加空字符串 → joined 后字符串以 \n 结尾，避免依赖文件末尾换行约定。
        lines.append("")
        return lines.joined(separator: "\n")
    }

    static func selectedPlayableRepresentations(
        manifest: VesperDashManifest,
        variantPolicy: VesperDashMasterPlaylistVariantPolicy
    ) throws -> (audio: [VesperDashPlayableRepresentation], video: [VesperDashPlayableRepresentation]) {
        let playable = try manifest.playableRepresentations()
        var audio = playable.filter { $0.adaptationSet.kind == .audio }
        var video = playable.filter { $0.adaptationSet.kind == .video }

        guard variantPolicy == .startupSingleVariant else {
            return (audio, video)
        }

        if let selectedAudio = audio.first {
            audio = [selectedAudio]
        }
        if let selectedVideo = startupVideoRepresentation(from: video) {
            video = [selectedVideo]
        }
        return (audio, video)
    }

    static func buildMediaPlaylist(
        initializationURI: String,
        segments: [VesperDashMediaSegment],
        segmentURI: (Int) -> String
    ) throws -> String {
        try buildMediaPlaylist(
            initializationURI: initializationURI,
            segmentDurations: segments.map(\.duration),
            segmentURI: segmentURI
        )
    }

    static func buildMediaPlaylist(
        initializationURI: String,
        segments: [VesperDashTemplateSegment],
        segmentURI: (Int) -> String
    ) throws -> String {
        try buildMediaPlaylist(
            initializationURI: initializationURI,
            segmentDurations: segments.map(\.duration),
            segmentURI: segmentURI
        )
    }

    static func buildExternalMediaPlaylist(
        map: VesperDashHlsMap,
        segments: [VesperDashHlsSegment]
    ) throws -> String {
        try buildMediaPlaylist(
            map: map,
            segments: segments
        )
    }

    /// 构造 HLS media playlist（统一 init URI 版本）。详见 `buildMasterPlaylist`
    /// 文档说明的逐行 append 模式。
    private static func buildMediaPlaylist(
        initializationURI: String,
        segmentDurations: [Double],
        segmentURI: (Int) -> String
    ) throws -> String {
        guard !segmentDurations.isEmpty else {
            throw VesperDashBridgeError.invalidMp4("media playlist must contain at least one segment")
        }

        let targetDuration = max(Int(ceil(segmentDurations.max() ?? 1)), 1)
        var lines: [String] = [
            "#EXTM3U",
            "#EXT-X-VERSION:7",
            "#EXT-X-TARGETDURATION:\(targetDuration)",
            "#EXT-X-MEDIA-SEQUENCE:1",
            "#EXT-X-INDEPENDENT-SEGMENTS",
            "#EXT-X-PLAYLIST-TYPE:VOD",
            "#EXT-X-MAP:URI=\"\(escapeAttribute(initializationURI))\"",
        ]
        for (index, duration) in segmentDurations.enumerated() {
            lines.append("#EXTINF:\(formatDashDecimal(duration)),")
            lines.append(segmentURI(index))
        }
        lines.append("#EXT-X-ENDLIST")
        lines.append("")
        return lines.joined(separator: "\n")
    }

    /// 构造 HLS media playlist（外部 map / segment URI 版本，可选 byte range）。
    private static func buildMediaPlaylist(
        map: VesperDashHlsMap,
        segments: [VesperDashHlsSegment]
    ) throws -> String {
        guard !segments.isEmpty else {
            throw VesperDashBridgeError.invalidMp4("media playlist must contain at least one segment")
        }

        let targetDuration = max(Int(ceil(segments.map(\.duration).max() ?? 1)), 1)
        var lines: [String] = [
            "#EXTM3U",
            "#EXT-X-VERSION:7",
            "#EXT-X-TARGETDURATION:\(targetDuration)",
            "#EXT-X-MEDIA-SEQUENCE:1",
            "#EXT-X-INDEPENDENT-SEGMENTS",
            "#EXT-X-PLAYLIST-TYPE:VOD",
        ]
        if let byteRange = map.byteRange {
            lines.append("#EXT-X-MAP:URI=\"\(escapeAttribute(map.uri))\",BYTERANGE=\"\(byteRange.length)@\(byteRange.start)\"")
        } else {
            lines.append("#EXT-X-MAP:URI=\"\(escapeAttribute(map.uri))\"")
        }
        for segment in segments {
            lines.append("#EXTINF:\(formatDashDecimal(segment.duration)),")
            if let byteRange = segment.byteRange {
                lines.append("#EXT-X-BYTERANGE:\(byteRange.length)@\(byteRange.start)")
            }
            lines.append(segment.uri)
        }
        lines.append("#EXT-X-ENDLIST")
        lines.append("")
        return lines.joined(separator: "\n")
    }

    static func mediaSegments(
        segmentBase: VesperDashSegmentBase,
        sidx: VesperDashSidxBox
    ) throws -> [VesperDashMediaSegment] {
        guard !sidx.references.isEmpty else {
            throw VesperDashBridgeError.invalidMp4("sidx must contain at least one reference")
        }
        var offset = try checkedAdd(
            try checkedAdd(segmentBase.indexRange.end, 1, field: "sidx media offset"),
            sidx.firstOffset,
            field: "sidx media offset"
        )
        var segments: [VesperDashMediaSegment] = []
        for reference in sidx.references {
            guard reference.referenceType == 0 else {
                throw VesperDashBridgeError.unsupportedMp4("hierarchical sidx references are not supported")
            }
            guard reference.referencedSize > 0 else {
                throw VesperDashBridgeError.invalidMp4("sidx reference size must be non-zero")
            }
            guard reference.subsegmentDuration > 0 else {
                throw VesperDashBridgeError.invalidMp4("sidx subsegment duration must be non-zero")
            }
            let end = try checkedAdd(offset, UInt64(reference.referencedSize) - 1, field: "sidx media byte range")
            segments.append(
                VesperDashMediaSegment(
                    duration: Double(reference.subsegmentDuration) / Double(sidx.timescale),
                    range: try VesperDashByteRange(start: offset, end: end)
                )
            )
            offset = try checkedAdd(end, 1, field: "sidx next media offset")
        }
        return segments
    }

    static func templateSegments(
        durationMs: UInt64?,
        segmentTemplate: VesperDashSegmentTemplate
    ) throws -> [VesperDashTemplateSegment] {
        if !segmentTemplate.timeline.isEmpty {
            return try timelineTemplateSegments(
                durationMs: durationMs,
                segmentTemplate: segmentTemplate
            )
        }
        guard let declaredDuration = segmentTemplate.duration else {
            throw VesperDashBridgeError.unsupportedManifest(
                "SegmentTemplate without SegmentTimeline requires duration"
            )
        }
        guard let durationMs, durationMs > 0 else {
            throw VesperDashBridgeError.unsupportedManifest(
                "SegmentTemplate requires mediaPresentationDuration"
            )
        }
        let totalDuration = Double(durationMs) / 1_000
        let segmentDuration = normalizedFixedTemplateDuration(
            Double(declaredDuration) / Double(segmentTemplate.timescale)
        )
        guard totalDuration.isFinite,
              segmentDuration.isFinite,
              segmentDuration > 0
        else {
            throw VesperDashBridgeError.invalidManifest("invalid SegmentTemplate duration")
        }
        let segmentCountDouble = ceil(totalDuration / segmentDuration)
        guard segmentCountDouble.isFinite,
              segmentCountDouble > 0,
              segmentCountDouble <= Double(Int.max)
        else {
            throw VesperDashBridgeError.invalidManifest("invalid SegmentTemplate segment count")
        }

        let segmentCount = Int(segmentCountDouble)
        var segments: [VesperDashTemplateSegment] = []
        segments.reserveCapacity(segmentCount)
        for index in 0..<segmentCount {
            let numberOffset = UInt64(index)
            let (number, overflow) = segmentTemplate.startNumber.addingReportingOverflow(numberOffset)
            guard !overflow else {
                throw VesperDashBridgeError.invalidManifest("SegmentTemplate segment number overflows UInt64")
            }
            let remaining = totalDuration - (Double(index) * segmentDuration)
            let duration = min(segmentDuration, remaining)
            guard duration.isFinite, duration > 0 else {
                throw VesperDashBridgeError.invalidManifest("invalid SegmentTemplate segment duration")
            }
            segments.append(VesperDashTemplateSegment(duration: duration, number: number, time: nil))
        }
        return segments
    }

    private static func timelineTemplateSegments(
        durationMs: UInt64?,
        segmentTemplate: VesperDashSegmentTemplate
    ) throws -> [VesperDashTemplateSegment] {
        let timelineEnd = try timelineEndTick(
            durationMs: durationMs,
            segmentTemplate: segmentTemplate
        )
        var nextStart: UInt64?
        var segmentIndex: UInt64 = 0
        var segments: [VesperDashTemplateSegment] = []

        for (entryIndex, entry) in segmentTemplate.timeline.enumerated() {
            let entryStart = entry.startTime ?? nextStart ?? 0
            let nextExplicitStart = segmentTemplate.timeline[(entryIndex + 1)...]
                .lazy
                .compactMap(\.startTime)
                .first
            let repeatCount = try expandedTimelineRepeatCount(
                entry: entry,
                entryStart: entryStart,
                nextExplicitStart: nextExplicitStart,
                timelineEnd: timelineEnd
            )
            if repeatCount == 0 {
                nextStart = entryStart
                continue
            }

            var currentStart = entryStart
            for _ in 0..<repeatCount {
                if let timelineEnd, currentStart >= timelineEnd {
                    nextStart = currentStart
                    break
                }
                let unclippedEnd = try checkedAdd(
                    currentStart,
                    entry.duration,
                    field: "SegmentTimeline segment end"
                )
                let clippedEnd = minTimelineEnd(
                    unclippedEnd,
                    timelineEnd,
                    nextExplicitStart
                )
                guard clippedEnd > currentStart else {
                    break
                }
                let number = try checkedAdd(
                    segmentTemplate.startNumber,
                    segmentIndex,
                    field: "SegmentTemplate segment number"
                )
                let duration = Double(clippedEnd - currentStart) / Double(segmentTemplate.timescale)
                guard duration.isFinite, duration > 0 else {
                    throw VesperDashBridgeError.invalidManifest("invalid SegmentTimeline segment duration")
                }
                segments.append(
                    VesperDashTemplateSegment(
                        duration: duration,
                        number: number,
                        time: currentStart
                    )
                )
                segmentIndex = try checkedAdd(
                    segmentIndex,
                    1,
                    field: "SegmentTimeline segment index"
                )
                currentStart = unclippedEnd
            }
            nextStart = currentStart
        }

        guard !segments.isEmpty else {
            throw VesperDashBridgeError.invalidManifest("SegmentTimeline produced no media segments")
        }
        return segments
    }

    private static func expandedTimelineRepeatCount(
        entry: VesperDashSegmentTimelineEntry,
        entryStart: UInt64,
        nextExplicitStart: UInt64?,
        timelineEnd: UInt64?
    ) throws -> Int {
        if entry.repeatCount >= 0 {
            return try checkedInt(
                UInt64(entry.repeatCount) + 1,
                field: "SegmentTimeline repeat count"
            )
        }

        let boundary: UInt64
        if let nextExplicitStart {
            guard nextExplicitStart > entryStart else {
                throw VesperDashBridgeError.invalidManifest("SegmentTimeline next S@t must be greater than current time")
            }
            boundary = nextExplicitStart
        } else if let timelineEnd {
            guard timelineEnd > entryStart else {
                return 0
            }
            boundary = timelineEnd
        } else {
            throw VesperDashBridgeError.unsupportedManifest(
                "SegmentTimeline r=-1 requires next S@t or mediaPresentationDuration"
            )
        }

        let ticks = boundary - entryStart
        return try checkedInt(
            ceilDiv(ticks, entry.duration),
            field: "SegmentTimeline expanded repeat count"
        )
    }

    private static func timelineEndTick(
        durationMs: UInt64?,
        segmentTemplate: VesperDashSegmentTemplate
    ) throws -> UInt64? {
        guard let durationMs, durationMs > 0 else {
            return nil
        }
        let endTick = (Double(durationMs) * Double(segmentTemplate.timescale) / 1_000).rounded()
        guard endTick.isFinite, endTick >= 0, endTick <= Double(UInt64.max) else {
            throw VesperDashBridgeError.invalidManifest("SegmentTimeline media duration exceeds UInt64")
        }
        return try checkedAdd(
            segmentTemplate.presentationTimeOffset,
            UInt64(endTick),
            field: "SegmentTimeline media end"
        )
    }

    private static func minTimelineEnd(
        _ value: UInt64,
        _ timelineEnd: UInt64?,
        _ nextExplicitStart: UInt64?
    ) -> UInt64 {
        var result = value
        if let timelineEnd {
            result = min(result, timelineEnd)
        }
        if let nextExplicitStart {
            result = min(result, nextExplicitStart)
        }
        return result
    }

    private static func ceilDiv(_ value: UInt64, _ divisor: UInt64) -> UInt64 {
        guard divisor > 0 else { return 0 }
        let quotient = value / divisor
        return value % divisor == 0 ? quotient : quotient + 1
    }

    private static func normalizedFixedTemplateDuration(_ value: Double) -> Double {
        guard value.isFinite, value > 0 else {
            return value
        }
        let rounded = value.rounded()
        let tolerance = max(0.010, value * 0.005)
        if rounded > 0, abs(rounded - value) <= tolerance {
            return rounded
        }
        return value
    }

    /// 向 master playlist 行列表追加一个 STREAM-INF + URI 对。
    private static func appendVariantLines(
        _ lines: inout [String],
        item: VesperDashPlayableRepresentation,
        extraCodecs: [String],
        extraBandwidth: UInt64,
        audioGroup: String?,
        mediaURL: (String) -> String
    ) throws {
        guard let baseBandwidth = item.representation.bandwidth else {
            throw VesperDashBridgeError.invalidManifest(
                "Representation \(item.representation.id) is missing bandwidth"
            )
        }
        let averageBandwidth = try checkedAdd(baseBandwidth, extraBandwidth, field: "HLS AVERAGE-BANDWIDTH")
        let peakBandwidth = try checkedAdd(averageBandwidth, averageBandwidth, field: "HLS BANDWIDTH")
        var attrs = [
            "BANDWIDTH=\(peakBandwidth)",
            "AVERAGE-BANDWIDTH=\(averageBandwidth)",
        ]
        if let width = item.representation.width, let height = item.representation.height, width > 0, height > 0 {
            attrs.append("RESOLUTION=\(width)x\(height)")
        }
        if let frameRate = item.representation.frameRate.flatMap(formatFrameRate) {
            attrs.append("FRAME-RATE=\(frameRate)")
        }
        let codecs = uniqueCodecs([item.representation.codecs] + extraCodecs).joined(separator: ",")
        if !codecs.isEmpty {
            attrs.append("CODECS=\"\(escapeAttribute(codecs))\"")
        }
        if let audioGroup {
            attrs.append("AUDIO=\"\(escapeAttribute(audioGroup))\"")
        }
        lines.append("#EXT-X-STREAM-INF:\(attrs.joined(separator: ","))")
        lines.append(mediaURL(item.renditionId))
    }

    private static func formatFrameRate(_ value: String) -> String? {
        let trimmed = value.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty else { return nil }
        let rate: Double
        if let slash = trimmed.firstIndex(of: "/") {
            guard
                let numerator = Double(trimmed[..<slash]),
                let denominator = Double(trimmed[trimmed.index(after: slash)...]),
                denominator != 0
            else {
                return nil
            }
            rate = numerator / denominator
        } else {
            guard let parsed = Double(trimmed) else { return nil }
            rate = parsed
        }
        guard rate.isFinite, rate > 0 else { return nil }
        return formatDashDecimal(rate)
    }

    private static func startupVideoRepresentation(
        from video: [VesperDashPlayableRepresentation]
    ) -> VesperDashPlayableRepresentation? {
        video.first { isAvcCodec($0.representation.codecs) }
            ?? video.first { isHevcCodec($0.representation.codecs) }
            ?? video.first
    }

    private static func isAvcCodec(_ value: String) -> Bool {
        value
            .split(separator: ",")
            .map { $0.trimmingCharacters(in: .whitespacesAndNewlines).lowercased() }
            .contains { $0.hasPrefix("avc1") || $0.hasPrefix("avc3") }
    }

    private static func isHevcCodec(_ value: String) -> Bool {
        value
            .split(separator: ",")
            .map { $0.trimmingCharacters(in: .whitespacesAndNewlines).lowercased() }
            .contains { $0.hasPrefix("hvc1") || $0.hasPrefix("hev1") }
    }
}

enum VesperDashTemplateExpander {
    static func expand(
        _ template: String,
        representation: VesperDashRepresentation,
        number: UInt64?,
        time: UInt64? = nil
    ) throws -> String {
        var output = ""
        var cursor = template.startIndex
        while cursor < template.endIndex {
            let character = template[cursor]
            guard character == "$" else {
                output.append(character)
                cursor = template.index(after: cursor)
                continue
            }

            let tokenStart = template.index(after: cursor)
            guard tokenStart < template.endIndex else {
                throw VesperDashBridgeError.invalidManifest("unterminated SegmentTemplate token")
            }
            if template[tokenStart] == "$" {
                output.append("$")
                cursor = template.index(after: tokenStart)
                continue
            }
            guard let tokenEnd = template[tokenStart...].firstIndex(of: "$") else {
                throw VesperDashBridgeError.invalidManifest("unterminated SegmentTemplate token")
            }
            let token = String(template[tokenStart..<tokenEnd])
            output += try expandToken(token, representation: representation, number: number, time: time)
            cursor = template.index(after: tokenEnd)
        }
        return output
    }

    private static func expandToken(
        _ token: String,
        representation: VesperDashRepresentation,
        number: UInt64?,
        time: UInt64?
    ) throws -> String {
        let name: String
        let format: String?
        if let percent = token.firstIndex(of: "%") {
            name = String(token[..<percent])
            format = String(token[percent...])
        } else {
            name = token
            format = nil
        }

        switch name {
        case "RepresentationID":
            guard format == nil else {
                throw VesperDashBridgeError.unsupportedManifest(
                    "SegmentTemplate RepresentationID formatting is not supported"
                )
            }
            return representation.id
        case "Number":
            guard let number else {
                throw VesperDashBridgeError.invalidManifest("SegmentTemplate Number is not available")
            }
            return try formatTemplateNumber(number, format: format)
        case "Bandwidth":
            guard let bandwidth = representation.bandwidth else {
                throw VesperDashBridgeError.invalidManifest(
                    "SegmentTemplate Bandwidth requires representation bandwidth"
                )
            }
            return try formatTemplateNumber(bandwidth, format: format)
        case "Time":
            guard let time else {
                throw VesperDashBridgeError.invalidManifest("SegmentTemplate Time requires SegmentTimeline")
            }
            return try formatTemplateNumber(time, format: format)
        default:
            throw VesperDashBridgeError.unsupportedManifest("unsupported SegmentTemplate token \(name)")
        }
    }

    private static func formatTemplateNumber(_ value: UInt64, format: String?) throws -> String {
        guard let format else {
            return "\(value)"
        }
        guard format.first == "%" else {
            throw VesperDashBridgeError.invalidManifest("invalid SegmentTemplate format \(format)")
        }
        var cursor = format.index(after: format.startIndex)
        var padding = " "
        if cursor < format.endIndex, format[cursor] == "0" {
            padding = "0"
            cursor = format.index(after: cursor)
        }
        var widthText = ""
        while cursor < format.endIndex, format[cursor].isNumber {
            widthText.append(format[cursor])
            cursor = format.index(after: cursor)
        }
        guard cursor < format.endIndex,
              ["d", "i", "u"].contains(format[cursor]),
              format.index(after: cursor) == format.endIndex
        else {
            throw VesperDashBridgeError.unsupportedManifest(
                "unsupported SegmentTemplate format \(format)"
            )
        }
        let raw = "\(value)"
        let width = Int(widthText) ?? 0
        guard width > raw.count else {
            return raw
        }
        return String(repeating: padding, count: width - raw.count) + raw
    }
}

private final class VesperDashXmlParser: NSObject, XMLParserDelegate {
    private let root = VesperDashXmlNode(name: "#document")
    private var stack: [VesperDashXmlNode] = []
    private var capturedError: Error?

    static func parse(data: Data) throws -> VesperDashXmlNode {
        let delegate = VesperDashXmlParser()
        delegate.stack = [delegate.root]
        let parser = XMLParser(data: data)
        parser.delegate = delegate
        parser.shouldProcessNamespaces = false
        parser.shouldReportNamespacePrefixes = true
        guard parser.parse() else {
            throw parser.parserError ?? delegate.capturedError ?? VesperDashBridgeError.invalidManifest("XML parser failed")
        }
        guard delegate.stack.count == 1 else {
            throw VesperDashBridgeError.invalidManifest("unclosed XML element")
        }
        return delegate.root
    }

    func parser(
        _ parser: XMLParser,
        didStartElement elementName: String,
        namespaceURI: String?,
        qualifiedName qName: String?,
        attributes attributeDict: [String: String] = [:]
    ) {
        stack.append(VesperDashXmlNode(name: qName ?? elementName, attributes: attributeDict))
    }

    func parser(
        _ parser: XMLParser,
        didEndElement elementName: String,
        namespaceURI: String?,
        qualifiedName qName: String?
    ) {
        guard stack.count > 1, let node = stack.popLast() else {
            capturedError = VesperDashBridgeError.invalidManifest("unexpected XML closing tag")
            parser.abortParsing()
            return
        }
        stack[stack.count - 1].children.append(node)
    }

    func parser(_ parser: XMLParser, foundCharacters string: String) {
        stack.last?.text += string
    }

    func parser(_ parser: XMLParser, parseErrorOccurred parseError: Error) {
        capturedError = parseError
    }
}

private final class VesperDashXmlNode {
    let name: String
    let attributes: [String: String]
    var text: String
    var children: [VesperDashXmlNode]

    var localName: String {
        dashLocalName(name)
    }

    init(
        name: String,
        attributes: [String: String] = [:],
        text: String = "",
        children: [VesperDashXmlNode] = []
    ) {
        self.name = name
        self.attributes = attributes
        self.text = text
        self.children = children
    }

    func attr(_ name: String) -> String? {
        attributes[name] ?? attributes.first { dashLocalName($0.key) == name }?.value
    }

    func children(named name: String) -> [VesperDashXmlNode] {
        children.filter { $0.localName == name }
    }

    func childText(_ name: String) -> String? {
        children(named: name)
            .map { $0.text.trimmingCharacters(in: .whitespacesAndNewlines) }
            .first { !$0.isEmpty }
    }
}

private struct VesperMp4BoxHeader {
    let boxType: [UInt8]
    let payloadStart: Int
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
            payloadStart: start + headerSize,
            end: start + boxSize
        )
    }
}

private struct VesperMp4Reader {
    private let bytes: [UInt8]
    private var cursor: Int = 0

    init(bytes: [UInt8]) {
        self.bytes = bytes
    }

    mutating func readUInt8(field: String) throws -> UInt8 {
        guard cursor < bytes.count else {
            throw VesperDashBridgeError.invalidMp4("truncated MP4 field \(field)")
        }
        defer { cursor += 1 }
        return bytes[cursor]
    }

    mutating func readUInt16(field: String) throws -> UInt16 {
        let value = try readBigEndianUInt16(bytes, offset: cursor, field: field)
        cursor += 2
        return value
    }

    mutating func readUInt24(field: String) throws -> UInt32 {
        guard cursor + 3 <= bytes.count else {
            throw VesperDashBridgeError.invalidMp4("truncated MP4 field \(field)")
        }
        let value = (UInt32(bytes[cursor]) << 16)
            | (UInt32(bytes[cursor + 1]) << 8)
            | UInt32(bytes[cursor + 2])
        cursor += 3
        return value
    }

    mutating func readUInt32(field: String) throws -> UInt32 {
        let value = try readBigEndianUInt32(bytes, offset: cursor, field: field)
        cursor += 4
        return value
    }

    mutating func readUInt64(field: String) throws -> UInt64 {
        let value = try readBigEndianUInt64(bytes, offset: cursor, field: field)
        cursor += 8
        return value
    }
}

private extension VesperDashManifest {
    func playableRepresentations() throws -> [VesperDashPlayableRepresentation] {
        guard periods.count == 1, let period = periods.first else {
            throw VesperDashBridgeError.unsupportedManifest("multi-period DASH is not supported")
        }
        var usedIds: [String: Int] = [:]
        var playable: [VesperDashPlayableRepresentation] = []
        for (adaptationIndex, adaptationSet) in period.adaptationSets.enumerated() {
            guard adaptationSet.kind == .video || adaptationSet.kind == .audio else {
                continue
            }
            for (representationIndex, representation) in adaptationSet.representations.enumerated() {
                guard representation.segmentBase != nil || representation.segmentTemplate != nil else {
                    continue
                }
                let fallbackId = "\(adaptationSet.kind.rawValue)-\(adaptationIndex)-\(representationIndex)"
                let baseId = representation.id.isEmpty ? fallbackId : representation.id
                let seenCount = usedIds[baseId] ?? 0
                usedIds[baseId] = seenCount + 1
                let renditionId = seenCount == 0 ? baseId : "\(baseId)-\(seenCount + 1)"
                playable.append(
                    VesperDashPlayableRepresentation(
                        renditionId: renditionId,
                        adaptationSet: adaptationSet,
                        representation: representation
                    )
                )
            }
        }
        guard !playable.isEmpty else {
            throw VesperDashBridgeError.unsupportedManifest(
                "MPD has no SegmentBase or SegmentTemplate audio/video representations"
            )
        }
        return playable
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

private func dashLocalName(_ name: String) -> String {
    name.split(separator: ":").last.map(String.init) ?? name
}

private func uniqueCodecs(_ values: [String]) -> [String] {
    var codecs: [String] = []
    for value in values {
        for codec in value.split(separator: ",").map({ $0.trimmingCharacters(in: .whitespacesAndNewlines) }) where !codec.isEmpty {
            if !codecs.contains(codec) {
                codecs.append(codec)
            }
        }
    }
    return codecs
}

private func escapeAttribute(_ value: String) -> String {
    value.replacingOccurrences(of: "\"", with: "%22")
        .replacingOccurrences(of: "\n", with: "")
        .replacingOccurrences(of: "\r", with: "")
}

func applyHttpHeaders(_ headers: [String: String], to request: inout URLRequest) {
    for (field, value) in headers where !field.isEmpty {
        request.setValue(value, forHTTPHeaderField: field)
    }
}

let vesperAVURLAssetHTTPHeaderFieldsKey = "AVURLAssetHTTPHeaderFieldsKey"

private func formatDashDecimal(_ value: Double) -> String {
    String(format: "%.3f", locale: dashPlaylistLocale, value)
}

private let dashPlaylistLocale = Locale(identifier: "en_US_POSIX")

private let dashPathComponentAllowedCharacters: CharacterSet = {
    var characters = CharacterSet.urlPathAllowed
    characters.remove(charactersIn: "/")
    return characters
}()

private func checkedAdd(_ lhs: UInt64, _ rhs: UInt64, field: String) throws -> UInt64 {
    let (value, overflow) = lhs.addingReportingOverflow(rhs)
    guard !overflow else {
        throw VesperDashBridgeError.invalidMp4("\(field) overflows UInt64")
    }
    return value
}

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

private func backgroundPrefetchRequests(count: Int) -> [VesperDashSegmentRequest] {
    guard count > 0 else {
        return [.initialization]
    }
    let prioritized = startupPrefetchSegmentIndices(count: count)
    let orderedIndices = prioritized + (0..<count).filter { !prioritized.contains($0) }
    return [.initialization] + orderedIndices.map(VesperDashSegmentRequest.media)
}

private func readBigEndianUInt16(_ bytes: [UInt8], offset: Int, field: String) throws -> UInt16 {
    guard offset >= 0, offset + 2 <= bytes.count else {
        throw VesperDashBridgeError.invalidMp4("truncated MP4 field \(field)")
    }
    return (UInt16(bytes[offset]) << 8) | UInt16(bytes[offset + 1])
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
