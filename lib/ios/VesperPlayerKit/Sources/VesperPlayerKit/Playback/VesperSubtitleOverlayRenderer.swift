import Foundation

@MainActor
final class VesperSubtitleOverlayRenderer {
    struct Cue: Equatable {
        let startMs: Int64
        let endMs: Int64
        let text: String
    }

    struct PreparationFailure {
        let trackId: String
        let error: VesperSubtitleError
    }

    struct PreparedConfiguration {
        let cuesByTrackId: [String: [Cue]]
        let orderedTrackIds: [String]
        let failures: [PreparationFailure]
        let advertisedTrackCount: Int
    }

    static let maximumTrackCount = 8
    static let maximumSubtitleBytes = 2 * 1024 * 1024
    static let maximumCueCount = 10_000
    static let maximumCueTextLength = 16_384
    static let resourceTimeoutNanoseconds: UInt64 = 10_000_000_000

    private var cuesByTrackId: [String: [Cue]] = [:]
    private var orderedTrackIds: [String] = []
    private var selectedTrackId: String?
    private var style = VesperSubtitleStyle.default
    private weak var surfaceHost: PlayerSurfaceView?
    private var renderedText = ""

    var hasTracks: Bool {
        !cuesByTrackId.isEmpty
    }

    var loadedTrackIds: [String] {
        orderedTrackIds
    }

    var renderedTextSnapshot: String {
        renderedText
    }

    func containsTrack(_ trackId: String) -> Bool {
        cuesByTrackId[trackId] != nil
    }

    func firstTrackId() -> String? {
        loadedTrackIds.first
    }

    func attach(surfaceHost: PlayerSurfaceView?) {
        self.surfaceHost = surfaceHost
        applyRenderedText()
    }

    func setStyle(_ style: VesperSubtitleStyle) {
        self.style = style
        applyRenderedText()
    }

    func select(trackId: String?) -> Bool {
        if let trackId, !containsTrack(trackId) {
            return false
        }
        selectedTrackId = trackId
        renderedText = ""
        applyRenderedText()
        return true
    }

    func reset() {
        cuesByTrackId = [:]
        orderedTrackIds = []
        selectedTrackId = nil
        renderedText = ""
        applyRenderedText()
    }

    func configure(_ configurations: [VesperExternalSubtitleSource]) async throws {
        let prepared = try await prepare(configurations)
        if let failure = prepared.failures.first {
            throw failure.error
        }
        install(prepared)
    }

    /// Loads and parses external subtitle tracks without changing the active
    /// renderer. The caller commits the result only after its source/item
    /// epoch is still current.
    func prepare(_ configurations: [VesperExternalSubtitleSource]) async throws -> PreparedConfiguration {
        let ids = configurations.map(\.id)
        if ids.contains(where: {
            $0.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
        }) || Set(ids).count != ids.count {
            let duplicateId = Dictionary(grouping: ids, by: { $0 })
                .first(where: { !$0.key.isEmpty && $0.value.count > 1 })?.key
            return PreparedConfiguration(
                cuesByTrackId: [:],
                orderedTrackIds: [],
                failures: [
                    PreparationFailure(
                        trackId: duplicateId ?? "",
                        error: Self.subtitleError(
                            code: "subtitle_track_identity_ambiguous",
                            phase: .identity,
                            trackId: duplicateId,
                            message: "External subtitle ids must be non-empty and unique."
                        )
                    )
                ],
                advertisedTrackCount: configurations.count
            )
        }
        if configurations.filter(\.isDefault).count > 1 {
            return PreparedConfiguration(
                cuesByTrackId: [:],
                orderedTrackIds: [],
                failures: [
                    PreparationFailure(
                        trackId: "",
                        error: Self.subtitleError(
                            code: "subtitle_default_track_ambiguous",
                            phase: .identity,
                            message: "A subtitle group may contain at most one default track."
                        )
                    )
                ],
                advertisedTrackCount: configurations.count
            )
        }
        guard configurations.count <= Self.maximumTrackCount else {
            return PreparedConfiguration(
                cuesByTrackId: [:],
                orderedTrackIds: [],
                failures: configurations.map { configuration in
                    PreparationFailure(
                        trackId: configuration.id,
                        error: Self.subtitleError(
                            code: "subtitle_resource_failed",
                            phase: .resource,
                            trackId: configuration.id,
                            message: "The external subtitle track limit was exceeded."
                        )
                    )
                },
                advertisedTrackCount: configurations.count
            )
        }

        var loaded: [String: [Cue]] = [:]
        var loadedOrder: [String] = []
        var failures: [PreparationFailure] = []
        var claimedIds = Set<String>()
        loaded.reserveCapacity(configurations.count)
        for configuration in configurations {
            try Task.checkCancellation()
            let trackId = configuration.id
            do {
                guard !trackId.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty,
                      claimedIds.insert(trackId).inserted
                else {
                    throw Self.subtitleError(
                        code: "subtitle_track_identity_ambiguous",
                        phase: .identity,
                        trackId: trackId.isEmpty ? nil : trackId,
                        message: "External subtitle ids must be non-empty and unique."
                    )
                }
                guard let url = URL(string: configuration.uri) else {
                    throw Self.subtitleError(
                        code: "subtitle_uri_invalid",
                        phase: .resource,
                        trackId: trackId,
                        message: "The external subtitle URI is invalid."
                    )
                }
                let data = try await Self.readBoundedData(
                    from: url,
                    headers: configuration.headers
                )
                guard let text = String(data: data, encoding: .utf8) else {
                    throw Self.subtitleError(
                        code: "subtitle_encoding_unsupported",
                        phase: .resource,
                        trackId: trackId,
                        message: "External subtitles must be UTF-8 encoded."
                    )
                }
                let cues: [Cue]
                switch configuration.mimeType.lowercased() {
                case VesperExternalSubtitleSource.mimeSubrip, "application/srt":
                    cues = try Self.parseBlockCues(text, webVtt: false)
                case VesperExternalSubtitleSource.mimeWebVtt:
                    cues = try Self.parseBlockCues(text, webVtt: true)
                case VesperExternalSubtitleSource.mimeSsa, "text/x-ass":
                    cues = try Self.parseSsaCues(text)
                default:
                    throw Self.subtitleError(
                        code: "subtitle_encoding_unsupported",
                        phase: .resource,
                        trackId: trackId,
                        message: "The external subtitle MIME type is not supported by the iOS backend."
                    )
                }
                if cues.isEmpty && Self.hasMalformedCuePayload(text, mimeType: configuration.mimeType) {
                    throw Self.subtitleError(
                        code: "subtitle_resource_failed",
                        phase: .resource,
                        trackId: trackId,
                        message: "The external subtitle contains no valid cues."
                    )
                }
                loaded[trackId] = cues
                loadedOrder.append(trackId)
            } catch is CancellationError {
                throw CancellationError()
            } catch let error as VesperSubtitleError {
                let scopedError = error.trackId == nil
                    ? VesperSubtitleError(
                        code: error.code,
                        phase: error.phase,
                        trackId: trackId.isEmpty ? nil : trackId,
                        retriable: error.retriable,
                        message: error.message,
                        phaseRawValue: error.phaseRawValue
                    )
                    : error
                failures.append(PreparationFailure(trackId: trackId, error: scopedError))
            } catch {
                failures.append(
                    PreparationFailure(
                        trackId: trackId,
                        error: Self.subtitleError(
                            code: "subtitle_resource_failed",
                            phase: .resource,
                            trackId: trackId,
                            retriable: true,
                            message: "The external subtitle resource could not be loaded."
                        )
                    )
                )
            }
        }

        try Task.checkCancellation()
        return PreparedConfiguration(
            cuesByTrackId: loaded,
            orderedTrackIds: loadedOrder,
            failures: failures,
            advertisedTrackCount: configurations.count
        )
    }

    /// Commits an already prepared configuration. This is intentionally
    /// separate from `prepare` so a stale source load cannot replace active
    /// cues after a source switch.
    func install(_ prepared: PreparedConfiguration) {
        cuesByTrackId = prepared.cuesByTrackId
        orderedTrackIds = prepared.orderedTrackIds
        selectedTrackId = nil
        renderedText = ""
        applyRenderedText()
    }

    func render(positionMs: Int64) {
        guard let selectedTrackId, let cues = cuesByTrackId[selectedTrackId] else {
            if !renderedText.isEmpty {
                renderedText = ""
                applyRenderedText()
            }
            return
        }
        let activeText = cues.lazy
            .filter { cue in cue.startMs <= positionMs && positionMs < cue.endMs }
            .map(\.text)
            .joined(separator: "\n")
        if activeText != renderedText {
            renderedText = activeText
            applyRenderedText()
        }
    }

    private func applyRenderedText() {
        surfaceHost?.updateSubtitleOverlay(text: renderedText, style: style)
    }

    private static func readBoundedData(
        from url: URL,
        headers: [String: String]
    ) async throws -> Data {
        if url.isFileURL {
            let handle = try FileHandle(forReadingFrom: url)
            defer { try? handle.close() }
            let data = try handle.read(upToCount: maximumSubtitleBytes + 1) ?? Data()
            guard data.count <= maximumSubtitleBytes else {
                throw subtitleError(
                    code: "subtitle_resource_failed",
                    phase: .resource,
                    message: "The external subtitle file exceeds the 2 MiB limit."
                )
            }
            return data
        }

        guard let scheme = url.scheme?.lowercased(), scheme == "http" || scheme == "https" else {
            throw subtitleError(
                code: "subtitle_uri_invalid",
                phase: .resource,
                message: "The external subtitle URI scheme is not supported."
            )
        }
        return try await withThrowingTaskGroup(of: Data.self) { group in
            group.addTask { @MainActor in
                try await readBoundedNetworkData(from: url, headers: headers)
            }
            group.addTask { @MainActor in
                try await Task.sleep(nanoseconds: resourceTimeoutNanoseconds)
                throw subtitleError(
                    code: "subtitle_resource_failed",
                    phase: .resource,
                    retriable: true,
                    message: "The external subtitle request exceeded the 10 second deadline."
                )
            }
            defer { group.cancelAll() }
            guard let result = try await group.next() else {
                throw CancellationError()
            }
            return result
        }
    }

    private static func readBoundedNetworkData(
        from url: URL,
        headers: [String: String]
    ) async throws -> Data {
        var request = URLRequest(url: url)
        request.timeoutInterval = 10
        for (name, value) in headers {
            request.setValue(value, forHTTPHeaderField: name)
        }
        let configuration = URLSessionConfiguration.ephemeral
        configuration.requestCachePolicy = .reloadIgnoringLocalCacheData
        configuration.urlCache = nil
        configuration.httpCookieStorage = nil
        configuration.httpShouldSetCookies = false
        let delegate = VesperExternalSubtitleSessionDelegate(
            originalURL: url,
            headerNames: Array(headers.keys)
        )
        let session = URLSession(configuration: configuration, delegate: delegate, delegateQueue: nil)
        defer { session.invalidateAndCancel() }
        let (bytes, response) = try await session.bytes(for: request)
        if let response = response as? HTTPURLResponse,
           !(200...299).contains(response.statusCode)
        {
            throw subtitleError(
                code: "subtitle_resource_failed",
                phase: .resource,
                retriable: response.statusCode >= 500,
                message: "The external subtitle request failed with HTTP \(response.statusCode)."
            )
        }
        var data = Data()
        data.reserveCapacity(min(maximumSubtitleBytes, 64 * 1024))
        for try await byte in bytes {
            if data.count >= maximumSubtitleBytes {
                throw subtitleError(
                    code: "subtitle_resource_failed",
                    phase: .resource,
                    message: "The external subtitle response exceeds the 2 MiB limit."
                )
            }
            data.append(byte)
        }
        return data
    }

    private static func parseBlockCues(_ source: String, webVtt: Bool) throws -> [Cue] {
        let normalized = source
            .replacingOccurrences(of: "\r\n", with: "\n")
            .replacingOccurrences(of: "\r", with: "\n")
        let blocks = normalized.components(separatedBy: "\n\n")
        var cues: [Cue] = []
        cues.reserveCapacity(min(blocks.count, maximumCueCount))

        for rawBlock in blocks {
            var lines = rawBlock.split(separator: "\n", omittingEmptySubsequences: false)
                .map(String.init)
            while lines.first?.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty == true {
                lines.removeFirst()
            }
            guard !lines.isEmpty else { continue }
            if webVtt {
                let first = lines[0].trimmingCharacters(in: .whitespaces)
                if first.hasPrefix("WEBVTT") || first.hasPrefix("NOTE") || first == "STYLE" || first == "REGION" {
                    continue
                }
            }
            let timingIndex = lines.firstIndex(where: { $0.contains("-->") })
            guard let timingIndex else { continue }
            let timingParts = lines[timingIndex].components(separatedBy: "-->")
            guard timingParts.count == 2,
                  let startMs = parseTimestamp(timingParts[0]),
                  let endMs = parseTimestamp(timingParts[1].split(separator: " ").first.map(String.init) ?? timingParts[1]),
                  endMs > startMs
            else { continue }
            let text = sanitizeCueText(lines.dropFirst(timingIndex + 1).joined(separator: "\n"))
            guard !text.isEmpty else { continue }
            guard text.count <= maximumCueTextLength else {
                throw subtitleError(
                    code: "subtitle_resource_failed",
                    phase: .resource,
                    message: "An external subtitle cue exceeds the supported text limit."
                )
            }
            cues.append(Cue(startMs: startMs, endMs: endMs, text: text))
            if cues.count > maximumCueCount {
                throw subtitleError(
                    code: "subtitle_resource_failed",
                    phase: .resource,
                    message: "The external subtitle file exceeds the cue limit."
                )
            }
        }
        return cues.sorted { left, right in
            left.startMs == right.startMs ? left.endMs < right.endMs : left.startMs < right.startMs
        }
    }

    private static func hasMalformedCuePayload(_ source: String, mimeType: String) -> Bool {
        let normalized = source
            .replacingOccurrences(of: "\r\n", with: "\n")
            .replacingOccurrences(of: "\r", with: "\n")
        if mimeType.lowercased() == VesperExternalSubtitleSource.mimeSsa ||
            mimeType.lowercased() == "text/x-ass" {
            return normalized.contains("Dialogue:")
        }
        let blocks = normalized.components(separatedBy: "\n\n")
        return blocks.contains { rawBlock in
            let firstLine = rawBlock
                .split(separator: "\n", omittingEmptySubsequences: true)
                .first
                .map { $0.trimmingCharacters(in: .whitespacesAndNewlines) } ?? ""
            guard !firstLine.isEmpty else { return false }
            if firstLine.hasPrefix("WEBVTT") || firstLine.hasPrefix("NOTE") ||
                firstLine == "STYLE" || firstLine == "REGION" {
                return false
            }
            return true
        }
    }

    private static func parseSsaCues(_ source: String) throws -> [Cue] {
        var cues: [Cue] = []
        for rawLine in source.split(whereSeparator: \.isNewline) {
            let line = String(rawLine)
            guard line.hasPrefix("Dialogue:") else { continue }
            let payload = line.dropFirst("Dialogue:".count).trimmingCharacters(in: .whitespaces)
            let fields = payload.split(separator: ",", maxSplits: 9, omittingEmptySubsequences: false)
            guard fields.count == 10,
                  let startMs = parseTimestamp(String(fields[1])),
                  let endMs = parseTimestamp(String(fields[2])),
                  endMs > startMs
            else { continue }
            let text = sanitizeCueText(
                String(fields[9])
                    .replacingOccurrences(of: "\\N", with: "\n")
                    .replacingOccurrences(of: "\\n", with: "\n")
            )
            guard !text.isEmpty else { continue }
            guard text.count <= maximumCueTextLength else {
                throw subtitleError(
                    code: "subtitle_resource_failed",
                    phase: .resource,
                    message: "An external subtitle cue exceeds the supported text limit."
                )
            }
            cues.append(Cue(startMs: startMs, endMs: endMs, text: text))
            if cues.count > maximumCueCount {
                throw subtitleError(
                    code: "subtitle_resource_failed",
                    phase: .resource,
                    message: "The external subtitle file exceeds the cue limit."
                )
            }
        }
        return cues.sorted { left, right in
            left.startMs == right.startMs ? left.endMs < right.endMs : left.startMs < right.startMs
        }
    }

    private static func parseTimestamp(_ rawValue: String) -> Int64? {
        let value = rawValue.trimmingCharacters(in: .whitespacesAndNewlines)
        let components = value.split(separator: ":", omittingEmptySubsequences: false)
        guard components.count == 2 || components.count == 3 else { return nil }
        let hoursIndex = components.count == 3 ? 0 : nil
        let minutesIndex = components.count == 3 ? 1 : 0
        let secondsIndex = components.count == 3 ? 2 : 1
        let hours: Int64
        if let hoursIndex {
            guard let parsedHours = Int64(components[hoursIndex]) else { return nil }
            hours = parsedHours
        } else {
            hours = 0
        }
        guard let minutes = Int64(components[minutesIndex]),
              hours >= 0,
              minutes >= 0,
              minutes < 60
        else { return nil }
        let secondsParts = components[secondsIndex]
            .replacingOccurrences(of: ",", with: ".")
            .split(separator: ".", maxSplits: 1, omittingEmptySubsequences: false)
        guard let seconds = Int64(secondsParts[0]), seconds >= 0, seconds < 60 else { return nil }
        let fraction = secondsParts.count == 2 ? String(secondsParts[1]) : ""
        let millisecondsText = String((fraction + "000").prefix(3))
        guard let milliseconds = Int64(millisecondsText) else { return nil }
        return ((hours * 60 + minutes) * 60 + seconds) * 1_000 + milliseconds
    }

    private static func sanitizeCueText(_ text: String) -> String {
        let withoutOverrideTags = text.replacingOccurrences(
            of: #"\{[^}]*\}"#,
            with: "",
            options: .regularExpression
        )
        let withoutMarkup = withoutOverrideTags.replacingOccurrences(
            of: #"<[^>]+>"#,
            with: "",
            options: .regularExpression
        )
        return withoutMarkup
            .replacingOccurrences(of: "&amp;", with: "&")
            .replacingOccurrences(of: "&lt;", with: "<")
            .replacingOccurrences(of: "&gt;", with: ">")
            .trimmingCharacters(in: .whitespacesAndNewlines)
    }

    private static func subtitleError(
        code: String,
        phase: VesperSubtitleErrorPhase,
        trackId: String? = nil,
        retriable: Bool = false,
        message: String
    ) -> VesperSubtitleError {
        VesperSubtitleError(
            code: code,
            phase: phase,
            trackId: trackId,
            retriable: retriable,
            message: message
        )
    }
}

private final class VesperExternalSubtitleSessionDelegate: NSObject, URLSessionTaskDelegate {
    private let originalURL: URL
    private let headerNames: [String]

    init(originalURL: URL, headerNames: [String]) {
        self.originalURL = originalURL
        self.headerNames = headerNames
    }

    func urlSession(
        _ session: URLSession,
        task: URLSessionTask,
        willPerformHTTPRedirection response: HTTPURLResponse,
        newRequest request: URLRequest,
        completionHandler: @escaping (URLRequest?) -> Void
    ) {
        completionHandler(
            externalSubtitleRedirectRequest(
                originalURL: originalURL,
                headerNames: headerNames,
                request: request
            )
        )
    }
}

func externalSubtitleRedirectRequest(
    originalURL: URL,
    headerNames: [String],
    request: URLRequest
) -> URLRequest {
    guard !sameExternalSubtitleOrigin(originalURL, request.url) else {
        return request
    }
    var stripped = request
    for headerName in headerNames {
        stripped.setValue(nil, forHTTPHeaderField: headerName)
    }
    return stripped
}

private func sameExternalSubtitleOrigin(_ left: URL, _ right: URL?) -> Bool {
    guard let right else { return false }
    return left.scheme?.lowercased() == right.scheme?.lowercased()
        && left.host?.lowercased() == right.host?.lowercased()
        && externalSubtitleEffectivePort(left) == externalSubtitleEffectivePort(right)
}

private func externalSubtitleEffectivePort(_ url: URL) -> Int? {
    if let port = url.port { return port }
    switch url.scheme?.lowercased() {
    case "http": return 80
    case "https": return 443
    default: return nil
    }
}
