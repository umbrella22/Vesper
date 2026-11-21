import Foundation
@_implementationOnly import VesperPlayerKitBridgeShim

/// Owns the optional Rust listener and ingest worker for a single AVPlayer item.
@MainActor
final class VesperRtspSession {
    private struct Reply: Decodable {
        var phase: String
        var handle: UInt64?
        var playlistUri: URL?
        var code: String?
        var message: String?
    }

    private var handle: UInt64?

    init(source: VesperPlayerSource) throws {
        guard source.kind == .remote, source.headers.isEmpty, source.drmConfiguration == nil else {
            throw Self.error("rtsp_unsupported_input", "RTSP requires a remote, unprotected source without HTTP headers.")
        }
        let reply = try Self.command(["op": "open", "uri": source.uri])
        guard reply.phase == "connecting", let handle = reply.handle else {
            throw Self.error(reply.code ?? "rtsp_invalid_response", reply.message ?? "Invalid RTSP bridge open response.")
        }
        self.handle = handle
    }

    deinit {
        if let handle { _ = try? Self.command(["op": "close", "handle": handle]) }
    }

    func close() -> UInt64? {
        guard let handle else { return nil }
        self.handle = nil
        _ = try? Self.command(["op": "close", "handle": handle])
        return handle
    }

    static func waitForRetirement(_ handle: UInt64, deadline: ContinuousClock.Instant) async throws {
        let clock = ContinuousClock()
        while true {
            try Task.checkCancellation()
            guard clock.now < deadline else {
                throw error("rtsp_bridge_busy", "The previous RTSP bridge session did not close before the activation deadline.")
            }
            let reply = try command(["op": "retirementStatus", "handle": handle])
            switch reply.phase {
            case "closed": return
            case "closing":
                try await clock.sleep(until: min(clock.now + .milliseconds(20), deadline))
            default:
                throw error("rtsp_invalid_response", "The RTSP bridge returned an invalid retirement status.")
            }
        }
    }

    func playlist(deadline: ContinuousClock.Instant) async throws -> URL {
        let clock = ContinuousClock()
        while clock.now < deadline {
            try Task.checkCancellation()
            let reply = try status()
            if reply.phase == "ready", let url = reply.playlistUri {
                return url
            }
            try await Task.sleep(for: .milliseconds(50))
        }
        throw Self.error("rtsp_origin_timeout", "The RTSP bridge did not prepare its playlist before the activation deadline.")
    }

    func checkHealth() throws { _ = try status() }

    private func status() throws -> Reply {
        guard let handle else { throw CancellationError() }
        let reply = try Self.command(["op": "status", "handle": handle])
        switch reply.phase {
        case "connecting", "reconnecting": return reply
        case "ready" where reply.playlistUri != nil: return reply
        case "failed":
            throw Self.error(reply.code ?? "rtsp_invalid_response", reply.message ?? "RTSP bridge failed.")
        default:
            throw Self.error("rtsp_invalid_response", "The RTSP bridge returned an unknown phase.")
        }
    }

    nonisolated private static func command(_ request: [String: Any]) throws -> Reply {
        let bytes = try JSONSerialization.data(withJSONObject: request)
        let command = String(decoding: bytes, as: UTF8.self)
        guard let result = command.withCString({ vesper_rtsp_command($0) }) else {
            throw error("rtsp_invalid_response", "The RTSP bridge did not return a response.")
        }
        defer { vesper_dash_bridge_string_free(result) }
        return try JSONDecoder().decode(Reply.self, from: Data(String(cString: result).utf8))
    }

    nonisolated private static func error(_ reason: String, _ message: String) -> VesperPlayerError {
        let classification: (VesperPlayerErrorCode, VesperPlayerErrorCategory, Bool)
        switch reason {
        case "rtsp_origin_timeout":
            classification = (.timeout, .network, true)
        case "rtsp_origin_unavailable":
            classification = (.backendFailure, .network, true)
        case "rtsp_bridge_local_http":
            classification = (.backendFailure, .platform, false)
        case "rtsp_bridge_busy":
            classification = (.invalidState, .platform, true)
        case let unsupported where unsupported == "rtsp_bridge_not_bundled" || unsupported.hasPrefix("rtsp_unsupported_"):
            classification = (.unsupported, .capability, false)
        default:
            classification = (.invalidSource, .source, false)
        }
        return .init(message: message, code: classification.0,
                     category: classification.1, retriable: classification.2,
                     details: ["reason": reason, "route": "rtspBridge", "protocol": "rtsp"])
    }
}
