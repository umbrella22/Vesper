import Foundation

@MainActor
final class VesperSubtitleOverlayRenderer {
    private struct Cue: Equatable {
        let startMs: Int64
        let endMs: Int64
        let text: String
    }

    static let maximumTrackCount = 8
    static let maximumSubtitleBytes = 2 * 1024 * 1024
    static let maximumCueCount = 10_000
    static let maximumCueTextLength = 16_384

    private var cuesByTrackId: [String: [Cue]] = [:]
    private var selectedTrackId: String?
    private var style = VesperSubtitleStyle.default
    private weak var surfaceHost: PlayerSurfaceView?
    private var renderedText = ""

    var hasTracks: Bool {
        !cuesByTrackId.isEmpty
    }

    var renderedTextSnapshot: String {
        renderedText
    }

    func containsTrack(_ trackId: String) -> Bool {
        cuesByTrackId[trackId] != nil
    }

    func firstTrackId() -> String? {
        cuesByTrackId.keys.sorted().first
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
        selectedTrackId = nil
        renderedText = ""
        applyRenderedText()
    }

    func configure(_ configurations: [VesperSubtitleSideLoad]) async throws {
        guard configurations.count <= Self.maximumTrackCount else {
            throw Self.subtitleError(
                "Too many side-loaded subtitle tracks.",
                reason: "subtitleTrackLimitExceeded"
            )
        }
        guard !configurations.isEmpty else {
            reset()
            return
        }

        var loaded: [String: [Cue]] = [:]
        loaded.reserveCapacity(configurations.count)
        for (index, configuration) in configurations.enumerated() {
            try Task.checkCancellation()
            guard let url = URL(string: configuration.uri) else {
                throw Self.subtitleError(
                    "Invalid subtitle URI: \(configuration.uri)",
                    reason: "subtitleUriInvalid"
                )
            }
            let data = try await Self.readBoundedData(from: url)
            guard let text = String(data: data, encoding: .utf8) else {
                throw Self.subtitleError(
                    "Side-loaded subtitles must be UTF-8 encoded.",
                    reason: "subtitleEncodingUnsupported"
                )
            }
            let cues: [Cue]
            switch configuration.mimeType {
            case .subrip:
                cues = try Self.parseBlockCues(text, webVtt: false)
            case .webvtt:
                cues = try Self.parseBlockCues(text, webVtt: true)
            case .ssa:
                cues = try Self.parseSsaCues(text)
            }
            loaded[Self.trackId(for: index)] = cues
        }

        try Task.checkCancellation()
        cuesByTrackId = loaded
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

    static func trackId(for index: Int) -> String {
        "subtitle-side-load:\(index)"
    }

    private func applyRenderedText() {
        surfaceHost?.updateSubtitleOverlay(text: renderedText, style: style)
    }

    private static func readBoundedData(from url: URL) async throws -> Data {
        if url.isFileURL {
            let handle = try FileHandle(forReadingFrom: url)
            defer { try? handle.close() }
            let data = try handle.read(upToCount: maximumSubtitleBytes + 1) ?? Data()
            guard data.count <= maximumSubtitleBytes else {
                throw subtitleError(
                    "Subtitle file exceeds the 2 MiB limit.",
                    reason: "subtitleFileTooLarge"
                )
            }
            return data
        }

        guard let scheme = url.scheme?.lowercased(), scheme == "http" || scheme == "https" else {
            throw subtitleError(
                "Unsupported subtitle URI scheme.",
                reason: "subtitleUriSchemeUnsupported"
            )
        }
        var request = URLRequest(url: url)
        request.timeoutInterval = 10
        let (bytes, response) = try await URLSession.shared.bytes(for: request)
        if let response = response as? HTTPURLResponse,
           !(200...299).contains(response.statusCode)
        {
            throw subtitleError(
                "Subtitle request failed with HTTP \(response.statusCode).",
                reason: "subtitleHttpFailure"
            )
        }
        var data = Data()
        data.reserveCapacity(min(maximumSubtitleBytes, 64 * 1024))
        for try await byte in bytes {
            if data.count >= maximumSubtitleBytes {
                throw subtitleError(
                    "Subtitle response exceeds the 2 MiB limit.",
                    reason: "subtitleFileTooLarge"
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
                    "Subtitle cue text exceeds the supported limit.",
                    reason: "subtitleCueTooLarge"
                )
            }
            cues.append(Cue(startMs: startMs, endMs: endMs, text: text))
            if cues.count > maximumCueCount {
                throw subtitleError(
                    "Subtitle file exceeds the 10,000 cue limit.",
                    reason: "subtitleCueLimitExceeded"
                )
            }
        }
        return cues.sorted { left, right in
            left.startMs == right.startMs ? left.endMs < right.endMs : left.startMs < right.startMs
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
                    "Subtitle cue text exceeds the supported limit.",
                    reason: "subtitleCueTooLarge"
                )
            }
            cues.append(Cue(startMs: startMs, endMs: endMs, text: text))
            if cues.count > maximumCueCount {
                throw subtitleError(
                    "Subtitle file exceeds the 10,000 cue limit.",
                    reason: "subtitleCueLimitExceeded"
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
        guard components.count == 3,
              let hours = Int64(components[0]),
              let minutes = Int64(components[1])
        else { return nil }
        let secondsParts = components[2]
            .replacingOccurrences(of: ",", with: ".")
            .split(separator: ".", maxSplits: 1, omittingEmptySubsequences: false)
        guard let seconds = Int64(secondsParts[0]) else { return nil }
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

    private static func subtitleError(_ message: String, reason: String) -> VesperPlayerError {
        VesperPlayerError(
            message: message,
            code: .unsupported,
            category: .capability,
            retriable: false,
            details: ["reason": reason, "route": "subtitleOverlay"]
        )
    }
}
