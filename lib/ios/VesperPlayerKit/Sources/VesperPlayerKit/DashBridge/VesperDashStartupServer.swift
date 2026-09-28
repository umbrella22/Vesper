import Foundation
import Network

/// AVPlayer consumes cached fMP4 through HTTP. Each route is one complete sliced segment,
/// so HTTP ranges are relative to that segment, never to the original DASH file.
final class VesperDashStartupServer: @unchecked Sendable {
    private let queue = DispatchQueue(label: "io.github.umbrella22.vesper.dash-startup-http")
    private let client: VesperDashStartupNetworkClient
    private var listener: NWListener?
    private var port: UInt16?
    private var closed = false
    private var ready: [(Result<UInt16, Error>) -> Void] = []
    private var routes: [String: VesperDashStartupResource] = [:]
    private var connections: [UUID: NWConnection] = [:]
    private var loads: [UUID: Task<Void, Never>] = [:]
    private var deadlines: [UUID: DispatchWorkItem] = [:]

    init(client: VesperDashStartupNetworkClient) { self.client = client }

    func register(_ resource: VesperDashStartupResource) async throws -> URL {
        try await withCheckedThrowingContinuation { continuation in
            queue.async { [self] in
                guard !closed, ready.count < 8 else {
                    continuation.resume(throwing: VesperDashStartupError.invalidated)
                    return
                }
                let register: (Result<UInt16, Error>) -> Void = { [self] result in
                    do {
                        let port = try result.get()
                        let path: String
                        if let existing = routes.first(where: { $0.value == resource }) { path = existing.key } else {
                            guard routes.count < 8 else { throw VesperDashStartupError.budgetExceeded }
                            path = "/\(UUID().uuidString).mp4"
                            routes[path] = resource
                        }
                        guard let url = URL(string: "http://127.0.0.1:\(port)\(path)") else {
                            throw VesperDashStartupError.invalidResponse
                        }
                        continuation.resume(returning: url)
                    } catch { continuation.resume(throwing: error) }
                }
                if let port { register(.success(port)); return }
                ready.append(register)
                guard listener == nil else { return }
                do {
                    let parameters = NWParameters.tcp
                    parameters.requiredLocalEndpoint = .hostPort(host: "127.0.0.1", port: .any)
                    let listener = try NWListener(using: parameters)
                    self.listener = listener
                    listener.stateUpdateHandler = { [weak self, weak listener] state in
                        guard let self, !self.closed else { return }
                        switch state {
                        case .ready:
                            guard let port = listener?.port?.rawValue else { self.closeOnQueue(); return }
                            self.port = port
                            let waiting = self.ready
                            self.ready.removeAll()
                            waiting.forEach { $0(.success(port)) }
                        case .failed, .cancelled: self.closeOnQueue()
                        default: break
                        }
                    }
                    listener.newConnectionHandler = { [weak self] connection in self?.accept(connection) }
                    listener.start(queue: queue)
                    queue.asyncAfter(deadline: .now() + 3) { [weak self] in
                        guard let self, self.port == nil else { return }
                        self.closeOnQueue()
                    }
                } catch { closeOnQueue() }
            }
        }
    }

    /// Explicitly invoked at source teardown; handlers do not own the server lifetime.
    func close() { queue.async { [self] in closeOnQueue() } }

    private func closeOnQueue() {
        guard !closed else { return }
        closed = true
        listener?.cancel()
        listener = nil
        let waiting = ready
        ready.removeAll()
        waiting.forEach { $0(.failure(VesperDashStartupError.invalidated)) }
        Array(connections.keys).forEach(finish)
        routes.removeAll()
    }

    private func accept(_ connection: NWConnection) {
        guard !closed, connections.count < 8 else { connection.cancel(); return }
        let id = UUID()
        connections[id] = connection
        let timeout = DispatchWorkItem { [weak self] in self?.finish(id) }
        deadlines[id] = timeout
        queue.asyncAfter(deadline: .now() + 12, execute: timeout)
        connection.stateUpdateHandler = { [weak self] state in
            if case .failed = state { self?.finish(id) }
        }
        connection.start(queue: queue)
        receive(id, buffered: Data())
    }

    private func finish(_ id: UUID) {
        deadlines.removeValue(forKey: id)?.cancel()
        loads.removeValue(forKey: id)?.cancel()
        connections.removeValue(forKey: id)?.cancel()
    }

    private func receive(_ id: UUID, buffered: Data) {
        guard let connection = connections[id] else { return }
        connection.receive(minimumIncompleteLength: 1, maximumLength: 8193 - buffered.count) { [weak self] data, _, complete, error in
            guard let self, self.connections[id] != nil else { return }
            var request = buffered
            if let data { request.append(data) }
            guard error == nil, request.count <= 8192 else { self.finish(id); return }
            if let boundary = request.range(of: Data("\r\n\r\n".utf8)) {
                guard boundary.upperBound == request.count,
                      let text = String(data: request, encoding: .utf8) else { self.finish(id); return }
                self.respond(id, request: text)
            } else if complete || request.count == 8192 { self.finish(id) } else { self.receive(id, buffered: request) }
        }
    }

    private func respond(_ id: UUID, request: String) {
        let lines = request.components(separatedBy: "\r\n")
        let first = (lines.first ?? "").split(separator: " ")
        guard first.count == 3, ["GET", "HEAD"].contains(first[0]), first[2] == "HTTP/1.1",
              let resource = routes[String(first[1])] else { send(id, status: "404 Not Found"); return }
        var headers: [String: String] = [:]
        for line in lines.dropFirst() where !line.isEmpty {
            guard let colon = line.firstIndex(of: ":") else { send(id, status: "400 Bad Request"); return }
            let name = line[..<colon].lowercased()
            guard headers[name] == nil else { send(id, status: "400 Bad Request"); return }
            headers[name] = line[line.index(after: colon)...].trimmingCharacters(in: .whitespaces)
        }
        guard headers["host"] == port.map({ "127.0.0.1:\($0)" }), headers["transfer-encoding"] == nil,
              headers["content-length"].map({ $0 == "0" }) ?? true else { send(id, status: "400 Bad Request"); return }
        let head = first[0] == "HEAD"
        let requestedRange = headers["range"]
        loads[id] = Task { [weak self, client] in
            do {
                let value = try await client.load(resource)
                try Task.checkCancellation()
                self?.queue.async { [weak self] in
                    guard let self, self.connections[id] != nil else { return }
                    let total = value.data.count
                    guard let range = Self.responseRange(requestedRange, count: total) else {
                        self.send(id, status: "416 Range Not Satisfiable", fields: "Content-Range: bytes */\(total)\r\n")
                        return
                    }
                    let fields = requestedRange == nil ? "" : "Content-Range: bytes \(range.lowerBound)-\(range.upperBound - 1)/\(total)\r\n"
                    self.send(id, status: requestedRange == nil ? "200 OK" : "206 Partial Content", fields: fields,
                              body: value.data.subdata(in: range), head: head)
                }
            } catch {
                self?.queue.async { [weak self] in self?.send(id, status: "502 Bad Gateway") }
            }
        }
    }

    static func responseRange(_ header: String?, count: Int) -> Range<Int>? {
        guard count > 0 else { return nil }
        guard let header else { return 0..<count }
        guard header.hasPrefix("bytes=") else { return nil }
        let parts = header.dropFirst(6).split(separator: "-", omittingEmptySubsequences: false)
        guard parts.count == 2 else { return nil }
        func number(_ text: Substring) -> Int? {
            guard !text.isEmpty, text.allSatisfy({ $0.isASCII && $0.isNumber }) else { return nil }
            return Int(text)
        }
        if parts[0].isEmpty {
            guard let suffix = number(parts[1]), suffix > 0 else { return nil }
            return max(0, count - suffix)..<count
        }
        guard let start = number(parts[0]), start < count else { return nil }
        if parts[1].isEmpty { return start..<count }
        guard let end = number(parts[1]), end >= start else { return nil }
        return start..<(min(end, count - 1) + 1)
    }

    private func send(_ id: UUID, status: String, fields: String = "", body: Data = Data(), head: Bool = false) {
        guard let connection = connections[id] else { return }
        let header = Data("HTTP/1.1 \(status)\r\nContent-Length: \(body.count)\r\nContent-Type: video/mp4\r\nAccept-Ranges: bytes\r\nConnection: close\r\n\(fields)\r\n".utf8)
        connection.send(content: header, completion: .contentProcessed { [weak self] error in
            guard let self else { return }
            if error != nil || head || body.isEmpty { self.finish(id) } else { self.sendBody(id, body: body, offset: 0) }
        })
    }

    private func sendBody(_ id: UUID, body: Data, offset: Int) {
        guard let connection = connections[id] else { return }
        let end = min(body.count, offset + 64 * 1024)
        connection.send(content: body.subdata(in: offset..<end), completion: .contentProcessed { [weak self] error in
            guard let self else { return }
            if error != nil || end == body.count { self.finish(id) } else { self.sendBody(id, body: body, offset: end) }
        })
    }
}
