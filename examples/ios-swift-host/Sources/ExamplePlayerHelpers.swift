import Foundation
import VesperPlayerKit

enum ExampleLiveButtonState: Equatable {
    case goLive
    case live
    case liveBehind(Int64)
}

enum ExampleTimelineSummaryState: Equatable {
    case live
    case liveEdge(Int64)
    case window(positionMs: Int64, endMs: Int64)
}

func displayedTimelinePositionMs(_ timeline: TimelineUiState, pendingSeekRatio: Double?) -> Int64 {
    if let pendingSeekRatio {
        return timeline.position(forRatio: pendingSeekRatio)
    }
    return timeline.clampedPosition(timeline.positionMs)
}

func liveButtonState(_ timeline: TimelineUiState) -> ExampleLiveButtonState {
    guard let liveEdge = timeline.goLivePositionMs else { return .goLive }
    let behindMs = max(liveEdge - timeline.clampedPosition(timeline.positionMs), 0)
    if behindMs > 1_500 {
        return .liveBehind(behindMs)
    }
    return .live
}

func timelineSummaryState(_ timeline: TimelineUiState, pendingSeekRatio: Double?) -> ExampleTimelineSummaryState {
    let displayedPosition = displayedTimelinePositionMs(timeline, pendingSeekRatio: pendingSeekRatio)

    switch timeline.kind {
    case .live:
        if let liveEdge = timeline.goLivePositionMs {
            return .liveEdge(liveEdge)
        }
        return .live
    case .liveDvr:
        return liveDvrWindowSummary(timeline, displayedPosition: displayedPosition)
    case .vod:
        return .window(
            positionMs: displayedPosition,
            endMs: timeline.durationMs ?? 0
        )
    }
}

private func liveDvrWindowSummary(
    _ timeline: TimelineUiState,
    displayedPosition: Int64
) -> ExampleTimelineSummaryState {
    let rangeStart = timeline.seekableRange?.startMs ?? 0
    let windowEnd = timeline.goLivePositionMs ?? timeline.durationMs ?? 0
    return .window(
        positionMs: max(displayedPosition - rangeStart, 0),
        endMs: max(windowEnd - rangeStart, 0)
    )
}

func qualityButtonLabel(_ policy: VesperAbrPolicy) -> String {
    switch policy.mode {
    case .auto:
        return ExampleI18n.auto
    case .constrained:
        if let maxWidth = policy.maxWidth, let maxHeight = policy.maxHeight {
            let resolutionLabel = "\(maxWidth)x\(maxHeight)"
            if let maxBitRate = policy.maxBitRate {
                return "\(resolutionLabel) / \(formatBitRate(maxBitRate))"
            } else {
                return resolutionLabel
            }
        } else if let maxBitRate = policy.maxBitRate {
            return formatBitRate(maxBitRate)
        } else {
            return ExampleI18n.qualityButtonCapped
        }
    case .fixedTrack:
        return ExampleI18n.qualityButtonPinned
    }
}

func qualityButtonLabel(
    _ trackCatalog: VesperTrackCatalog,
    _ trackSelection: VesperTrackSelectionSnapshot,
    effectiveVideoTrackId: String?,
    fixedTrackStatus: VesperFixedTrackStatus?
) -> String {
    let requestedTrack = requestedFixedVideoTrack(trackCatalog, trackSelection)
    let effectiveTrack = effectiveVideoTrack(trackCatalog, effectiveVideoTrackId)
    let resolvedStatus = currentFixedTrackStatus(
        trackCatalog,
        trackSelection,
        effectiveVideoTrackId: effectiveVideoTrackId,
        fixedTrackStatus: fixedTrackStatus
    )

    switch trackSelection.abrPolicy.mode {
    case .fixedTrack:
        guard let requestedTrack else {
            return ExampleI18n.quality
        }
        switch resolvedStatus {
        case .pending, .fallback:
            return "\(ExampleI18n.qualityButtonLocking) · \(qualityLabel(requestedTrack))"
        case .locked, nil:
            return "\(ExampleI18n.qualityButtonPinned) · \(qualityLabel(requestedTrack))"
        }
    case .constrained, .auto:
        if let effectiveTrack {
            return "\(ExampleI18n.auto) · \(qualityLabel(effectiveTrack))"
        }
        return qualityButtonLabel(trackSelection.abrPolicy)
    }
}

func effectiveVideoTrack(
    _ trackCatalog: VesperTrackCatalog,
    _ effectiveVideoTrackId: String?
) -> VesperMediaTrack? {
    guard let effectiveVideoTrackId else { return nil }
    return trackCatalog.videoTracks.first { $0.id == effectiveVideoTrackId }
}

func requestedFixedVideoTrack(
    _ trackCatalog: VesperTrackCatalog,
    _ trackSelection: VesperTrackSelectionSnapshot
) -> VesperMediaTrack? {
    guard
        trackSelection.abrPolicy.mode == .fixedTrack,
        let trackId = trackSelection.abrPolicy.trackId
    else {
        return nil
    }
    return trackCatalog.videoTracks.first { $0.id == trackId }
}

func currentFixedTrackStatus(
    _ trackCatalog: VesperTrackCatalog,
    _ trackSelection: VesperTrackSelectionSnapshot,
    effectiveVideoTrackId: String?,
    fixedTrackStatus: VesperFixedTrackStatus?
) -> VesperFixedTrackStatus? {
    guard trackSelection.abrPolicy.mode == .fixedTrack else { return nil }
    if let fixedTrackStatus {
        return fixedTrackStatus
    }
    guard let requestedTrack = requestedFixedVideoTrack(trackCatalog, trackSelection) else {
        return .pending
    }
    guard let effectiveVideoTrackId else {
        return .pending
    }
    return effectiveVideoTrackId == requestedTrack.id ? .locked : .fallback
}

func qualityAutoRowSubtitle(
    _ trackCatalog: VesperTrackCatalog,
    _ trackSelection: VesperTrackSelectionSnapshot,
    effectiveVideoTrackId: String?,
    fixedTrackStatus: VesperFixedTrackStatus?,
    videoVariantObservation: VesperVideoVariantObservation?
) -> String {
    let effectiveTrack = effectiveVideoTrack(trackCatalog, effectiveVideoTrackId)
    let requestedTrack = requestedFixedVideoTrack(trackCatalog, trackSelection)
    let resolvedStatus = currentFixedTrackStatus(
        trackCatalog,
        trackSelection,
        effectiveVideoTrackId: effectiveVideoTrackId,
        fixedTrackStatus: fixedTrackStatus
    )
    let observation = videoVariantObservationSummary(videoVariantObservation)

    switch trackSelection.abrPolicy.mode {
    case .auto:
        if let effectiveTrack {
            return ExampleI18n.qualityAutoSubtitleWithEffective(qualityLabel(effectiveTrack))
        }
        if let observation {
            return ExampleI18n.qualityAutoSubtitleWithObservation(observation)
        }
        return ExampleI18n.qualityAutoSubtitle
    case .constrained:
        if let effectiveTrack {
            return ExampleI18n.qualityAutoSubtitleWithEffective(qualityLabel(effectiveTrack))
        }
        if let observation {
            return ExampleI18n.qualityAutoSubtitleWithObservation(observation)
        }
        return ExampleI18n.qualityAutoSubtitle
    case .fixedTrack:
        if
            let requestedTrack,
            resolvedStatus == .fallback,
            let effectiveTrack
        {
            return ExampleI18n.qualityFixedSubtitleFallback(
                qualityLabel(requestedTrack),
                qualityLabel(effectiveTrack)
            )
        }
        if let requestedTrack, resolvedStatus == .pending {
            return ExampleI18n.qualityFixedSubtitlePending(qualityLabel(requestedTrack))
        }
        if let requestedTrack {
            return ExampleI18n.qualityFixedSubtitleLocked(qualityLabel(requestedTrack))
        }
        if let observation {
            return ExampleI18n.qualityFixedSubtitleObservation(observation)
        }
        return ExampleI18n.qualityAutoSubtitle
    }
}

func qualityOptionBadgeLabel(
    trackId: String,
    trackCatalog: VesperTrackCatalog,
    trackSelection: VesperTrackSelectionSnapshot,
    effectiveVideoTrackId: String?,
    fixedTrackStatus: VesperFixedTrackStatus?
) -> String? {
    let isRequested =
        trackSelection.abrPolicy.mode == .fixedTrack &&
        trackSelection.abrPolicy.trackId == trackId
    let isEffective = effectiveVideoTrackId == trackId

    guard isRequested || isEffective else { return nil }
    let status = currentFixedTrackStatus(
        trackCatalog,
        trackSelection,
        effectiveVideoTrackId: effectiveVideoTrackId,
        fixedTrackStatus: fixedTrackStatus
    )
    if isRequested {
        switch status {
        case .pending:
            return ExampleI18n.qualityStatusPending
        case .locked:
            return ExampleI18n.qualityStatusLocked
        case .fallback:
            return ExampleI18n.qualityStatusFallback
        case nil:
            return ExampleI18n.qualityStatusRequested
        }
    }
    return ExampleI18n.qualityStatusLocked
}

func qualityOptionSubtitle(
    _ track: VesperMediaTrack,
    trackCatalog: VesperTrackCatalog,
    trackSelection: VesperTrackSelectionSnapshot,
    effectiveVideoTrackId: String?,
    fixedTrackStatus: VesperFixedTrackStatus?
) -> String {
    let base = qualitySubtitle(track)
    guard
        trackSelection.abrPolicy.mode == .fixedTrack,
        trackSelection.abrPolicy.trackId == track.id
    else {
        return base
    }
    let status = currentFixedTrackStatus(
        trackCatalog,
        trackSelection,
        effectiveVideoTrackId: effectiveVideoTrackId,
        fixedTrackStatus: fixedTrackStatus
    )
    switch status {
    case .pending:
        return "\(base) · \(ExampleI18n.qualityStatusPending)"
    case .locked:
        return "\(base) · \(ExampleI18n.qualityStatusLocked)"
    case .fallback:
        return "\(base) · \(ExampleI18n.qualityStatusFallback)"
    case nil:
        return base
    }
}

func qualityRuntimeNotice(_ error: VesperPlayerError?) -> String? {
    guard let error, error.message.contains("fixedTrack") else {
        return nil
    }
    return error.message
}

func videoVariantObservationSummary(_ observation: VesperVideoVariantObservation?) -> String? {
    guard let observation else { return nil }
    var parts: [String] = []
    if let width = observation.width, let height = observation.height {
        parts.append("\(width)x\(height)")
    }
    if let bitRate = observation.bitRate {
        parts.append(formatBitRate(bitRate))
    }
    return parts.isEmpty ? nil : parts.joined(separator: " · ")
}

func audioButtonLabel(
    _ trackCatalog: VesperTrackCatalog,
    _ trackSelection: VesperTrackSelectionSnapshot
) -> String {
    guard trackSelection.audio.mode == .track else { return ExampleI18n.audio }
    return trackCatalog.audioTracks.first { $0.id == trackSelection.audio.trackId }.map(audioLabel) ?? ExampleI18n.audio
}

func subtitleButtonLabel(
    _ trackCatalog: VesperTrackCatalog,
    _ trackSelection: VesperTrackSelectionSnapshot
) -> String {
    switch trackSelection.subtitle.mode {
    case .disabled:
        return ExampleI18n.captionsOff
    case .auto:
        return ExampleI18n.captionsAuto
    case .track:
        return trackCatalog.subtitleTracks.first { $0.id == trackSelection.subtitle.trackId }.map(subtitleLabel) ?? ExampleI18n.subtitles
    }
}

func stageBadgeText(_ timeline: TimelineUiState) -> String {
    switch timeline.kind {
    case .vod:
        return ExampleI18n.stageVideoOnDemand
    case .live:
        return ExampleI18n.stageLiveStream
    case .liveDvr:
        return ExampleI18n.stageLiveWithDvrWindow
    }
}

func playlistHintLabel(_ kind: VesperPlaylistViewportHintKind) -> String {
    switch kind {
    case .visible:
        return ExampleI18n.playlistStatusVisible
    case .nearVisible:
        return ExampleI18n.playlistStatusNearVisible
    case .prefetchOnly:
        return ExampleI18n.playlistStatusPrefetch
    case .hidden:
        return ExampleI18n.playlistStatusHidden
    }
}

func downloadStateLabel(_ state: VesperDownloadState) -> String {
    switch state {
    case .queued:
        return ExampleI18n.downloadStateQueued
    case .preparing:
        return ExampleI18n.downloadStatePreparing
    case .downloading:
        return ExampleI18n.downloadStateDownloading
    case .paused:
        return ExampleI18n.downloadStatePaused
    case .completed:
        return ExampleI18n.downloadStateCompleted
    case .failed:
        return ExampleI18n.downloadStateFailed
    case .removed:
        return ExampleI18n.downloadStateRemoved
    }
}

func downloadPrimaryActionLabel(_ state: VesperDownloadState) -> String? {
    switch state {
    case .queued, .failed:
        return ExampleI18n.downloadActionStart
    case .preparing, .downloading:
        return ExampleI18n.downloadActionPause
    case .paused:
        return ExampleI18n.downloadActionResume
    case .completed, .removed:
        return nil
    }
}

func downloadProgressSummary(_ task: VesperDownloadTaskSnapshot) -> String {
    let ratioText = task.progress.completionRatio
        .map { "\(Int($0 * 100.0))%" }
        ?? ExampleI18n.downloadProgressUnknown
    let bytesText = ExampleI18n.downloadProgressBytes(
        formatDownloadBytes(task.progress.receivedBytes),
        formatDownloadBytes(task.progress.totalBytes)
    )
    return "\(ratioText) · \(bytesText)"
}

func liveButtonLabel(_ timeline: TimelineUiState) -> String {
    switch liveButtonState(timeline) {
    case .goLive:
        return ExampleI18n.goLive
    case .live:
        return ExampleI18n.live
    case let .liveBehind(behindMs):
        return ExampleI18n.liveBehind(formatMillis(behindMs))
    }
}

func timelineSummary(_ timeline: TimelineUiState, pendingSeekRatio: Double?) -> String {
    switch timelineSummaryState(timeline, pendingSeekRatio: pendingSeekRatio) {
    case .live:
        return ExampleI18n.live
    case let .liveEdge(liveEdge):
        return ExampleI18n.liveEdge(formatMillis(liveEdge))
    case let .window(positionMs, endMs):
        return "\(formatMillis(positionMs)) / \(formatMillis(endMs))"
    }
}

func speedBadge(_ value: Float) -> String {
    return ExampleI18n.playbackRate(Double(value))
}

func resilienceBufferingValue(_ policy: VesperBufferingPolicy) -> String {
    return "\(bufferingPresetLabel(policy.preset)) · \(bufferWindowLabel(policy))"
}

func resilienceRetryValue(_ policy: VesperRetryPolicy) -> String {
    let attempts = policy.maxAttempts.map(ExampleI18n.resilienceRetryAttempts) ?? ExampleI18n.resilienceRetryUnlimited
    return ExampleI18n.resilienceRetryValue(attempts, retryBackoffLabel(policy.backoff))
}

func resilienceCacheValue(_ policy: VesperCachePolicy) -> String {
    return ExampleI18n.resilienceCacheValue(
        cachePresetLabel(policy.preset),
        formatStorageBytes(policy.maxMemoryBytes),
        formatStorageBytes(policy.maxDiskBytes)
    )
}

func bufferingPresetLabel(_ preset: VesperBufferingPreset) -> String {
    switch preset {
    case .default:
        return ExampleI18n.resiliencePresetDefault
    case .balanced:
        return ExampleI18n.resiliencePresetBalanced
    case .streaming:
        return ExampleI18n.resiliencePresetStreaming
    case .resilient:
        return ExampleI18n.resiliencePresetResilient
    case .lowLatency:
        return ExampleI18n.resiliencePresetLowLatency
    }
}

func cachePresetLabel(_ preset: VesperCachePreset) -> String {
    switch preset {
    case .default:
        return ExampleI18n.resiliencePresetDefault
    case .disabled:
        return ExampleI18n.resiliencePresetDisabled
    case .streaming:
        return ExampleI18n.resiliencePresetStreaming
    case .resilient:
        return ExampleI18n.resiliencePresetResilient
    }
}

func retryBackoffLabel(_ backoff: VesperRetryBackoff) -> String {
    switch backoff {
    case .fixed:
        return ExampleI18n.resilienceBackoffFixed
    case .linear:
        return ExampleI18n.resilienceBackoffLinear
    case .exponential:
        return ExampleI18n.resilienceBackoffExponential
    }
}

func bufferWindowLabel(_ policy: VesperBufferingPolicy) -> String {
    guard let minBufferMs = policy.minBufferMs, let maxBufferMs = policy.maxBufferMs else {
        return ExampleI18n.resilienceWindowDefault
    }
    return ExampleI18n.resilienceWindowRange(minBufferMs, maxBufferMs)
}

func formatStorageBytes(_ value: Int64?) -> String {
    guard let value else {
        return ExampleI18n.resilienceWindowDefault
    }
    if value == 0 {
        return "0 B"
    }
    if value >= 1024 * 1024 * 1024 {
        return String(format: "%.1f GB", Double(value) / (1024.0 * 1024.0 * 1024.0))
    }
    if value >= 1024 * 1024 {
        return String(format: "%.0f MB", Double(value) / (1024.0 * 1024.0))
    }
    if value >= 1024 {
        return String(format: "%.0f KB", Double(value) / 1024.0)
    }
    return "\(value) B"
}

func bundledDownloadPluginLibraryPaths() -> [String] {
    let fileManager = FileManager.default
    let frameworksPath = Bundle.main.privateFrameworksPath ?? "\(Bundle.main.bundlePath)/Frameworks"
    let candidates = [
        "\(frameworksPath)/VesperPlayerKit.framework/libplayer_ffmpeg.dylib",
        "\(frameworksPath)/libplayer_ffmpeg.dylib",
        "\(Bundle.main.bundlePath)/libplayer_ffmpeg.dylib",
    ]

    return candidates.compactMap { candidate in
        guard fileManager.fileExists(atPath: candidate) else {
            return nil
        }
        return candidate
    }
}

struct ExamplePreparedDownloadTask {
    let source: VesperDownloadSource
    let profile: VesperDownloadProfile
    let assetIndex: VesperDownloadAssetIndex
}

func exampleDraftDownloadLabel(_ source: VesperPlayerSource) -> String {
    if !source.label.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
        return source.label
    }
    if let sourceURL = URL(string: source.uri) {
        return exampleDraftDownloadLabel(for: sourceURL)
    }
    return source.uri
}

func exampleDraftDownloadLabel(for url: URL) -> String {
    let fileName = url.lastPathComponent.trimmingCharacters(in: .whitespacesAndNewlines)
    let parentDirectory = url.deletingLastPathComponent().lastPathComponent
        .trimmingCharacters(in: .whitespacesAndNewlines)
    let normalizedFileName = fileName.lowercased()
    let rawCandidate: String
    if fileName.isEmpty {
        rawCandidate = url.host ?? url.absoluteString
    } else if genericManifestFileNames.contains(normalizedFileName), !parentDirectory.isEmpty {
        rawCandidate = parentDirectory
    } else if let dotIndex = fileName.lastIndex(of: "."), dotIndex > fileName.startIndex {
        rawCandidate = String(fileName[..<dotIndex])
    } else {
        rawCandidate = fileName
    }
    let cleaned = rawCandidate
        .replacingOccurrences(of: "_", with: " ")
        .replacingOccurrences(of: "-", with: " ")
        .trimmingCharacters(in: .whitespacesAndNewlines)
    return cleaned.isEmpty ? (url.host ?? url.absoluteString) : cleaned
}

func prepareExampleDownloadTask(
    assetId: String,
    source: VesperPlayerSource
) async throws -> ExamplePreparedDownloadTask {
    switch source.protocol {
    case .hls:
        return try await prepareHlsDownloadTask(assetId: assetId, source: source)
    default:
        return ExamplePreparedDownloadTask(
            source: VesperDownloadSource(source: source),
            profile: VesperDownloadProfile(),
            assetIndex: VesperDownloadAssetIndex()
        )
    }
}

private struct HlsMasterSelection {
    let variantPlaylistURL: URL
    let audioPlaylistURL: URL?
}

private enum HlsPlaylistEntryKind {
    case resource
    case segment
}

private struct HlsPlaylistEntry {
    let kind: HlsPlaylistEntryKind
    let url: URL
    let sequence: UInt64?
}

private func prepareHlsDownloadTask(
    assetId: String,
    source: VesperPlayerSource
) async throws -> ExamplePreparedDownloadTask {
    guard let manifestURL = URL(string: source.uri) else {
        throw CocoaError(.fileReadInvalidFileName)
    }
    let manifestText = try await fetchRemoteText(manifestURL)
    let targetDirectory = exampleDownloadTargetDirectory(assetId: assetId)

    var resourceRecords: [String: VesperDownloadResourceRecord] = [:]
    var segmentRecords: [String: VesperDownloadSegmentRecord] = [:]

    func addResource(_ url: URL) {
        let relativePath = relativePathForRemoteURL(url)
        resourceRecords[relativePath] = resourceRecords[relativePath] ?? VesperDownloadResourceRecord(
            resourceId: relativePath,
            uri: url.absoluteString,
            relativePath: relativePath
        )
    }

    func addSegment(_ url: URL, sequence: UInt64?) {
        let relativePath = relativePathForRemoteURL(url)
        segmentRecords[relativePath] = segmentRecords[relativePath] ?? VesperDownloadSegmentRecord(
            segmentId: relativePath,
            uri: url.absoluteString,
            relativePath: relativePath,
            sequence: sequence
        )
    }

    func addPlaylistEntry(_ entry: HlsPlaylistEntry) {
        switch entry.kind {
        case .resource:
            addResource(entry.url)
        case .segment:
            addSegment(entry.url, sequence: entry.sequence)
        }
    }

    addResource(manifestURL)

    var primaryPlaylistText: String? = nil
    if let masterSelection = parseHlsMasterManifest(manifestText, manifestURL: manifestURL) {
        addResource(masterSelection.variantPlaylistURL)
        if let audioPlaylistURL = masterSelection.audioPlaylistURL {
            addResource(audioPlaylistURL)
        }

        let videoPlaylistText = try await fetchRemoteText(masterSelection.variantPlaylistURL)
        primaryPlaylistText = videoPlaylistText
        parseHlsMediaPlaylist(videoPlaylistText, playlistURL: masterSelection.variantPlaylistURL)
            .forEach(addPlaylistEntry(_:))

        if let audioPlaylistURL = masterSelection.audioPlaylistURL {
            let audioPlaylistText = try await fetchRemoteText(audioPlaylistURL)
            parseHlsMediaPlaylist(audioPlaylistText, playlistURL: audioPlaylistURL)
                .forEach(addPlaylistEntry(_:))
        }
    } else {
        primaryPlaylistText = manifestText
        parseHlsMediaPlaylist(manifestText, playlistURL: manifestURL)
            .forEach(addPlaylistEntry(_:))
    }

    let preparedLabel =
        resolvePreparedHlsLabel(
            originalSource: source,
            manifestURL: manifestURL,
            manifestText: manifestText,
            primaryPlaylistText: primaryPlaylistText
        )

    return ExamplePreparedDownloadTask(
        source: VesperDownloadSource(
            source: VesperPlayerSource.remoteUrl(manifestURL, label: preparedLabel),
            contentFormat: .hlsSegments,
            manifestUri: manifestURL.absoluteString
        ),
        profile: VesperDownloadProfile(targetDirectory: targetDirectory),
        assetIndex: VesperDownloadAssetIndex(
            contentFormat: .hlsSegments,
            resources: Array(resourceRecords.values),
            segments: Array(segmentRecords.values)
        )
    )
}

private func resolvePreparedHlsLabel(
    originalSource: VesperPlayerSource,
    manifestURL: URL,
    manifestText: String,
    primaryPlaylistText: String?
) -> String {
    let draftLabel = exampleDraftDownloadLabel(for: manifestURL)
    let originalLabel = originalSource.label.trimmingCharacters(in: .whitespacesAndNewlines)
    if !originalLabel.isEmpty, originalLabel != draftLabel {
        return originalLabel
    }
    return extractHlsManifestTitle(manifestText)
        ?? primaryPlaylistText.flatMap(extractHlsManifestTitle(_:))
        ?? draftLabel
}

private func extractHlsManifestTitle(_ manifestText: String) -> String? {
    return hlsSessionDataTitle(manifestText)
}

private func hlsSessionDataTitle(_ manifestText: String) -> String? {
    for line in manifestText.components(separatedBy: .newlines) {
        let trimmed = line.trimmingCharacters(in: .whitespacesAndNewlines)
        guard trimmed.uppercased().hasPrefix("#EXT-X-SESSION-DATA") else {
            continue
        }
        let attributes = parseAttributeList(trimmed.components(separatedBy: ":").dropFirst().joined(separator: ":"))
        let dataId = attributes["DATA-ID"]?.lowercased() ?? ""
        if dataId.contains("title"), let title = attributes["VALUE"]?.trimmingCharacters(in: .whitespacesAndNewlines), !title.isEmpty {
            return title
        }
    }
    return nil
}

private func parseHlsMasterManifest(
    _ manifestText: String,
    manifestURL: URL
) -> HlsMasterSelection? {
    var audioPlaylists: [String: [URL]] = [:]
    var variants: [(UInt64, URL, String?)] = []
    var pendingVariantBandwidth: UInt64?
    var pendingAudioGroupId: String?

    for rawLine in manifestText.components(separatedBy: .newlines) {
        let line = rawLine.trimmingCharacters(in: .whitespacesAndNewlines)
        if line.uppercased().hasPrefix("#EXT-X-MEDIA") {
            let attributes = parseAttributeList(line.components(separatedBy: ":").dropFirst().joined(separator: ":"))
            guard
                attributes["TYPE"] == "AUDIO",
                let groupId = attributes["GROUP-ID"],
                let uriValue = attributes["URI"],
                let url = URL(string: uriValue, relativeTo: manifestURL)?.absoluteURL
            else {
                continue
            }
            audioPlaylists[groupId, default: []].append(url)
            continue
        }
        if line.uppercased().hasPrefix("#EXT-X-STREAM-INF") {
            let attributes = parseAttributeList(line.components(separatedBy: ":").dropFirst().joined(separator: ":"))
            pendingVariantBandwidth = UInt64(attributes["BANDWIDTH"] ?? "")
            pendingAudioGroupId = attributes["AUDIO"]
            continue
        }
        if let bandwidth = pendingVariantBandwidth, !line.isEmpty, !line.hasPrefix("#"),
           let variantURL = URL(string: line, relativeTo: manifestURL)?.absoluteURL {
            variants.append((bandwidth, variantURL, pendingAudioGroupId))
            pendingVariantBandwidth = nil
            pendingAudioGroupId = nil
        }
    }

    guard let selectedVariant = variants.first else {
        return nil
    }
    let audioPlaylistURL = selectedVariant.2.flatMap { audioPlaylists[$0]?.first }
    return HlsMasterSelection(
        variantPlaylistURL: selectedVariant.1,
        audioPlaylistURL: audioPlaylistURL
    )
}

private func parseHlsMediaPlaylist(
    _ playlistText: String,
    playlistURL: URL
) -> [HlsPlaylistEntry] {
    var entries: [HlsPlaylistEntry] = []
    var nextSequence: UInt64 = 0

    for rawLine in playlistText.components(separatedBy: .newlines) {
        let line = rawLine.trimmingCharacters(in: .whitespacesAndNewlines)
        if line.uppercased().hasPrefix("#EXT-X-MEDIA-SEQUENCE") {
            let value = line.components(separatedBy: ":").dropFirst().joined(separator: ":")
            nextSequence = UInt64(value) ?? nextSequence
            continue
        }
        if line.uppercased().hasPrefix("#EXT-X-KEY") || line.uppercased().hasPrefix("#EXT-X-MAP") {
            let attributes = parseAttributeList(line.components(separatedBy: ":").dropFirst().joined(separator: ":"))
            guard let uriValue = attributes["URI"], let url = URL(string: uriValue, relativeTo: playlistURL)?.absoluteURL else {
                continue
            }
            entries.append(HlsPlaylistEntry(kind: .resource, url: url, sequence: nil))
            continue
        }
        if !line.isEmpty, !line.hasPrefix("#"), let url = URL(string: line, relativeTo: playlistURL)?.absoluteURL {
            entries.append(HlsPlaylistEntry(kind: .segment, url: url, sequence: nextSequence))
            nextSequence += 1
        }
    }

    return entries
}

private func parseAttributeList(_ line: String) -> [String: String] {
    var result: [String: String] = [:]
    let nsLine = line as NSString
    attributePattern.enumerateMatches(in: line, range: NSRange(location: 0, length: nsLine.length)) { match, _, _ in
        guard let match else { return }
        let key = nsLine.substring(with: match.range(at: 1))
        let quotedValueRange = match.range(at: 3)
        let unquotedValueRange = match.range(at: 2)
        let valueRange = quotedValueRange.location != NSNotFound ? quotedValueRange : unquotedValueRange
        guard valueRange.location != NSNotFound else { return }
        result[key] = nsLine.substring(with: valueRange).trimmingCharacters(in: .whitespacesAndNewlines)
    }
    return result
}

private func relativePathForRemoteURL(_ url: URL) -> String {
    let path = url.path.trimmingCharacters(in: CharacterSet(charactersIn: "/"))
    if !path.isEmpty {
        return path
    }
    let fallback = url.lastPathComponent.trimmingCharacters(in: .whitespacesAndNewlines)
    return fallback.isEmpty ? "download.bin" : fallback
}

private func exampleDownloadTargetDirectory(assetId: String) -> URL {
    let root = FileManager.default.urls(for: .documentDirectory, in: .userDomainMask).first!
        .appendingPathComponent("vesper-downloads", isDirectory: true)
    let directory = root.appendingPathComponent(assetId, isDirectory: true)
    try? FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
    return directory
}

private func fetchRemoteText(_ url: URL) async throws -> String {
    let (data, _) = try await URLSession.shared.data(from: url)
    guard let text = String(data: data, encoding: .utf8) else {
        throw CocoaError(.fileReadCorruptFile)
    }
    return text
}

private let attributePattern = try! NSRegularExpression(pattern: #"([A-Z0-9-]+)=("([^"]*)"|[^,]*)"#)
private let genericManifestFileNames: Set<String> = [
    "master.m3u8",
    "playlist.m3u8",
    "index.m3u8",
    "prog_index.m3u8",
    "manifest.mpd",
    "stream.mpd",
]

func createDownloadExportFile(for task: VesperDownloadTaskSnapshot) throws -> URL {
    let directory = FileManager.default.temporaryDirectory
        .appendingPathComponent("vesper-exported-videos", isDirectory: true)
    try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
    let safeStem = task.assetId
        .trimmingCharacters(in: .whitespacesAndNewlines)
        .ifEmpty("download-\(task.taskId)")
        .replacingOccurrences(
            of: "[^A-Za-z0-9._-]",
            with: "_",
            options: .regularExpression
        )
    return directory.appendingPathComponent(safeStem).appendingPathExtension("mp4")
}

func formatDownloadBytes(_ value: UInt64?) -> String {
    guard let value, value > 0 else {
        return "-"
    }
    if value >= 1024 * 1024 * 1024 {
        return String(format: "%.1f GB", Double(value) / (1024.0 * 1024.0 * 1024.0))
    }
    if value >= 1024 * 1024 {
        return String(format: "%.1f MB", Double(value) / (1024.0 * 1024.0))
    }
    if value >= 1024 {
        return String(format: "%.0f KB", Double(value) / 1024.0)
    }
    return "\(value) B"
}

func audioLabel(_ track: VesperMediaTrack) -> String {
    track.label ?? track.language?.uppercased() ?? ExampleI18n.audioTrack
}

func audioSubtitle(_ track: VesperMediaTrack) -> String {
    let parts = [
        track.language?.uppercased(),
        track.channels.map(ExampleI18n.audioChannels),
        track.sampleRate.map { ExampleI18n.audioSampleRateKhz($0 / 1000) },
        track.codec,
    ].compactMap { $0 }
    return parts.isEmpty ? ExampleI18n.audioProgram : parts.joined(separator: " • ")
}

func subtitleLabel(_ track: VesperMediaTrack) -> String {
    track.label ?? track.language?.uppercased() ?? ExampleI18n.subtitleTrack
}

func subtitleSubtitle(_ track: VesperMediaTrack) -> String {
    let parts = [
        track.language?.uppercased(),
        track.isForced ? ExampleI18n.subtitleForced : nil,
        track.isDefault ? ExampleI18n.subtitleDefault : nil,
    ].compactMap { $0 }
    return parts.isEmpty ? ExampleI18n.subtitleOption : parts.joined(separator: " • ")
}

func qualityLabel(_ track: VesperMediaTrack) -> String {
    if let height = track.height {
        return "\(height)p"
    }
    if let width = track.width {
        return "\(width)w"
    }
    if let label = track.label, !label.isEmpty {
        return label
    }
    if let bitRate = track.bitRate {
        return formatBitRate(bitRate)
    }
    return track.id
}

func qualitySubtitle(_ track: VesperMediaTrack) -> String {
    let parts = [
        track.width.flatMap { width in
            track.height.map { "\(width)x\($0)" }
        },
        track.bitRate.map(formatBitRate),
        track.frameRate.map { String(format: "%.0f fps", $0) },
        track.codec,
    ].compactMap { $0 }
    return parts.isEmpty ? track.id : parts.joined(separator: " • ")
}

func formatBitRate(_ value: Int64) -> String {
    if value >= 1_000_000 {
        return ExampleI18n.bitRateMbps(Double(value) / 1_000_000.0)
    }
    if value >= 1_000 {
        return ExampleI18n.bitRateKbps(Double(value) / 1_000.0)
    }
    return ExampleI18n.bitRateBps(value)
}

func formatMillis(_ value: Int64) -> String {
    let totalSeconds = value / 1000
    let minutes = totalSeconds / 60
    let seconds = totalSeconds % 60
    return String(format: "%02d:%02d", minutes, seconds)
}

private extension String {
    func ifEmpty(_ fallback: @autoclosure () -> String) -> String {
        isEmpty ? fallback() : self
    }
}

func abrPresets() -> [AbrPreset] {
    [
        AbrPreset(
            id: "data-saver",
            title: ExampleI18n.abrPresetDataSaverTitle,
            subtitle: ExampleI18n.abrPresetDataSaverSubtitle,
            policy: .constrained(maxBitRate: 800_000, maxWidth: 854, maxHeight: 480)
        ),
        AbrPreset(
            id: "balanced",
            title: ExampleI18n.abrPresetBalancedTitle,
            subtitle: ExampleI18n.abrPresetBalancedSubtitle,
            policy: .constrained(maxBitRate: 2_000_000, maxWidth: 1280, maxHeight: 720)
        ),
        AbrPreset(
            id: "high",
            title: ExampleI18n.abrPresetHighTitle,
            subtitle: ExampleI18n.abrPresetHighSubtitle,
            policy: .constrained(maxBitRate: 5_000_000, maxWidth: 1920, maxHeight: 1080)
        ),
    ]
}

func sheetTitle(_ sheet: ExamplePlayerSheet) -> String {
    return ExampleI18n.sheetTitle(sheet)
}

func sheetSubtitle(_ sheet: ExamplePlayerSheet) -> String {
    return ExampleI18n.sheetSubtitle(sheet)
}

func sheetHeight(for sheet: ExamplePlayerSheet) -> CGFloat {
    switch sheet {
    case .menu:
        return 360
    case .quality:
        return 620
    case .audio:
        return 440
    case .subtitle:
        return 470
    case .speed:
        return 360
    }
}

func exampleIosHostLog(_ message: String) {
    print("[VesperPlayerIOSExample] \(message)")
}

extension Comparable {
    func clamped(to limits: ClosedRange<Self>) -> Self {
        min(max(self, limits.lowerBound), limits.upperBound)
    }
}
