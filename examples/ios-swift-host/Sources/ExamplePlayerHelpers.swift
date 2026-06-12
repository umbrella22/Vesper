import AVFoundation
import Darwin
import Foundation
import UIKit
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

func nativeFrameDiagnosticDetails(_ diagnostic: [String: Any]) -> String {
    let values = [
        diagnosticLabel("clock", diagnostic["clockSource"]),
        diagnosticLabel("audioDecoder", diagnostic["audioDecoder"]),
        diagnosticLabel("audio", diagnostic["audioOutput"]),
        diagnosticLabel("audioPipeline", diagnostic["audioPipeline"]),
        diagnosticLabel("rateControl", diagnostic["audioRateControl"]),
        diagnosticStream(
            "video",
            kind: diagnostic["selectedVideoMediaKind"],
            index: diagnostic["selectedVideoStreamIndex"]
        ),
        diagnosticStream(
            "audioTrack",
            kind: diagnostic["audioMediaKind"],
            index: diagnostic["audioStreamIndex"]
        ),
        diagnosticFlag("seekable", diagnostic["seekable"]),
        diagnosticLabel("fallbackTarget", diagnostic["fallbackTargetRoute"]),
        diagnosticLabel("issue", diagnostic["issueKind"] ?? diagnostic["failureKind"] ?? diagnostic["pendingKind"]),
    ].filter { !$0.isEmpty }
    return values.joined(separator: " · ")
}

func pluginDiagnosticCounters(_ diagnostic: [String: Any]) -> String {
    let values = [
        diagnosticCounter("processed", diagnostic["processedFrames"]),
        diagnosticCounter("presented", diagnostic["presentedFrames"]),
        diagnosticCounter("deadline", diagnostic["deadlineMisses"]),
        diagnosticCounter("backpressure", diagnostic["backpressureCount"]),
        diagnosticCounter("late", diagnostic["lateDropped"]),
        diagnosticCounter("skipAudio", diagnostic["skippedAudioPackets"]),
        diagnosticCounter("skipVideo", diagnostic["skippedVideoPackets"]),
        diagnosticCounter("skipOther", diagnostic["skippedOtherPackets"]),
    ].filter { !$0.isEmpty }
    return values.joined(separator: " · ")
}

private func diagnosticLabel(_ label: String, _ rawValue: Any?) -> String {
    guard let value = rawValue as? String, !value.isEmpty else {
        return ""
    }
    return "\(label)=\(value)"
}

private func diagnosticFlag(_ label: String, _ rawValue: Any?) -> String {
    if let value = rawValue as? Bool {
        return "\(label)=\(value)"
    }
    if let value = rawValue as? NSNumber {
        return "\(label)=\(value.boolValue)"
    }
    return ""
}

private func diagnosticStream(_ label: String, kind: Any?, index: Any?) -> String {
    guard let indexValue = diagnosticInt(index) else {
        return ""
    }
    let kindValue = (kind as? String).flatMap { $0.isEmpty || $0 == "pending" ? nil : $0 } ?? "unknown"
    return "\(label)=\(kindValue)#\(indexValue)"
}

private func diagnosticInt(_ rawValue: Any?) -> Int? {
    if let value = rawValue as? NSNumber {
        return value.intValue
    }
    return rawValue as? Int
}

private func diagnosticCounter(_ label: String, _ rawValue: Any?) -> String {
    if let value = rawValue as? NSNumber {
        return "\(label) \(value.intValue)"
    }
    if let value = rawValue as? Int {
        return "\(label) \(value)"
    }
    return ""
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

func compactTimelineSummary(_ timeline: TimelineUiState, pendingSeekRatio: Double?) -> String {
    switch timelineSummaryState(timeline, pendingSeekRatio: pendingSeekRatio) {
    case .live, .liveEdge:
        return ExampleI18n.live
    case let .window(positionMs, endMs):
        return "\(formatMillis(positionMs))/\(formatMillis(endMs))"
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
    bundledPluginLibraryPaths(
        dylibName: "libvesper_remux_ffmpeg.dylib",
        frameworkName: "VesperPlayerRemuxFfmpegPlugin",
        binaryName: "VesperPlayerRemuxFfmpegPlugin"
    )
}

func bundledSourceNormalizerPluginLibraryPaths() -> [String] {
    bundledPluginLibraryPaths(
        dylibName: "libvesper_source_normalizer_ffmpeg.dylib",
        frameworkName: "VesperPlayerSourceNormalizerFfmpegPlugin",
        binaryName: "VesperPlayerSourceNormalizerFfmpegPlugin"
    )
}

func bundledDecoderPluginLibraryPaths() -> [String] {
    bundledPluginLibraryPaths(
        dylibName: "libvesper_decoder_videotoolbox.dylib",
        frameworkName: "VesperPlayerDecoderVideoToolboxPlugin",
        binaryName: "VesperPlayerDecoderVideoToolboxPlugin"
    )
}

func bundledFrameProcessorPluginLibraryPaths() -> [String] {
    bundledPluginLibraryPaths(
        dylibName: "libvesper_frame_processor_diagnostic.dylib",
        frameworkName: "VesperPlayerFrameProcessorDiagnosticPlugin",
        binaryName: "VesperPlayerFrameProcessorDiagnosticPlugin"
    )
}

private func bundledPluginLibraryPaths(
    dylibName: String,
    frameworkName: String,
    binaryName: String
) -> [String] {
    let fileManager = FileManager.default
    let frameworksPath = Bundle.main.privateFrameworksPath ?? "\(Bundle.main.bundlePath)/Frameworks"
    let candidates = [
        "\(frameworksPath)/\(dylibName)",
        "\(frameworksPath)/\(frameworkName).framework/\(binaryName)",
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
    let downloadSource = VesperDownloadSource(source: source)
    let targetOutputFormat: VesperDownloadOutputFormat? =
        switch downloadSource.contentFormat {
        case .hlsSegments, .dashSegments, .flvSegments:
            .mp4
        case .singleFile, .unknown:
            nil
        }

    return ExamplePreparedDownloadTask(
        source: downloadSource,
        profile: VesperDownloadProfile(
            targetOutputFormat: targetOutputFormat,
            targetDirectory: exampleDownloadTargetDirectory(assetId: assetId)
        ),
        assetIndex: VesperDownloadAssetIndex()
    )
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

let IOS_HDR_EVIDENCE_NETWORK_CONTROL_URL =
    "https://127.0.0.1:9/vesper-hdr-network-control.mp4"

struct ExampleHdrEvidenceSamplePreset: Identifiable, Equatable {
    let sampleId: String
    let label: String
    let expectedAxis: String
    let sourceMetadata: [String: Any]

    var id: String { sampleId }

    static func == (lhs: ExampleHdrEvidenceSamplePreset, rhs: ExampleHdrEvidenceSamplePreset) -> Bool {
        lhs.sampleId == rhs.sampleId
    }
}

let exampleHdrEvidenceP0Presets: [ExampleHdrEvidenceSamplePreset] = [
    ExampleHdrEvidenceSamplePreset(
        sampleId: "HDR10-HEVC-MAIN10-2160P60-PQ",
        label: "HDR10 4K60 PQ",
        expectedAxis: "display",
        sourceMetadata: [
            "container": "mov",
            "codec": "hvc1",
            "sampleMimeType": "video/hevc",
            "width": 3840,
            "height": 2160,
            "frameRate": 60.0,
            "bitDepth": 10,
            "hdrKind": "hdr10",
            "colorPrimaries": "BT.2020",
            "transferFunction": "SMPTE_ST_2084_PQ",
            "yCbCrMatrix": "BT.2020_NCL",
            "controlPurpose": "none",
        ]
    ),
    ExampleHdrEvidenceSamplePreset(
        sampleId: "HEVC-SDR-CONTROL",
        label: "HEVC SDR control",
        expectedAxis: "none",
        sourceMetadata: [
            "container": "mp4",
            "codec": "hvc1",
            "sampleMimeType": "video/hevc",
            "width": 1920,
            "height": 1080,
            "frameRate": 30.0,
            "bitDepth": 8,
            "hdrKind": "none",
            "colorPrimaries": "BT.709",
            "transferFunction": "BT.709",
            "yCbCrMatrix": "BT.709",
            "controlPurpose": "hevcSdrFalsePositive",
        ]
    ),
    ExampleHdrEvidenceSamplePreset(
        sampleId: "NETWORK-FAILURE-CONTROL",
        label: "Network failure control",
        expectedAxis: "network",
        sourceMetadata: [
            "sourceKind": "progressive",
            "container": "mp4",
            "codec": "none",
            "sampleMimeType": "video/mp4",
            "hdrKind": "none",
            "sourceUri": IOS_HDR_EVIDENCE_NETWORK_CONTROL_URL,
            "manifestKind": "none",
            "controlPurpose": "networkFailure",
        ]
    ),
]

struct ExampleHdrEvidenceCaptureContext {
    let preset: ExampleHdrEvidenceSamplePreset
    let source: VesperPlayerSource
    let controller: VesperPlayerController
    let sourceNormalizerSetting: ExampleSourceNormalizerSetting
    let nativeFramePipelineSetting: ExampleNativeFramePipelineSetting
    let sourceNormalizerPluginLibraryPaths: [String]
    let decoderPluginLibraryPaths: [String]
    let frameProcessorPluginLibraryPaths: [String]
}

enum ExampleHdrEvidenceCaptureError: LocalizedError {
    case missingActiveSource

    var errorDescription: String? {
        switch self {
        case .missingActiveSource:
            return "Select a local file or remote URL before capturing this HDR evidence preset."
        }
    }
}

@MainActor
func captureExampleHdrEvidenceBundle(
    _ context: ExampleHdrEvidenceCaptureContext
) async throws -> URL {
    let sourceMetadata = exampleHdrEvidenceSourceMetadata(
        preset: context.preset,
        source: context.source
    )
    let probe = VesperPlayerControllerFactory.probePlaybackCapability(
        VesperPlaybackCapabilityProbeRequest(
            source: context.source,
            codec: exampleHdrEvidenceProbeCodec(sourceMetadata),
            width: sourceMetadata["width"] as? Int,
            height: sourceMetadata["height"] as? Int,
            frameRate: sourceMetadata["frameRate"] as? Double,
            sourceNormalizerConfiguration: VesperSourceNormalizerConfiguration(
                mode: context.sourceNormalizerSetting.mode,
                pluginLibraryPaths: context.sourceNormalizerPluginLibraryPaths
            ),
            frameProcessorConfiguration: VesperFrameProcessorConfiguration(
                mode: context.frameProcessorPluginLibraryPaths.isEmpty ? .disabled : .diagnosticsOnly,
                pluginLibraryPaths: context.frameProcessorPluginLibraryPaths
            ),
            nativeFramePipelineConfiguration: VesperNativeFramePipelineConfiguration(
                mode: context.nativeFramePipelineSetting.mode,
                decoderPluginLibraryPaths: context.decoderPluginLibraryPaths,
                frameProcessorPluginLibraryPaths: context.frameProcessorPluginLibraryPaths,
                maxInFlightFrames: 2
            )
        )
    )
    let controlledNetworkFailureEvidence =
        await exampleHdrEvidenceControlledNetworkFailureEvidence(
            preset: context.preset,
            source: context.source
        )
    let captureDate = exampleHdrEvidenceCaptureDate()
    let deviceId = "ios-example-host"
    let bundle = ExampleHdrEvidenceBundle(
        sampleId: context.preset.sampleId,
        deviceId: deviceId,
        platform: "ios",
        captureDate: captureDate,
        sdkCommit: "local-debug",
        sourceMetadata: sourceMetadata,
        device: exampleHdrEvidenceDevice(
            deviceId: deviceId,
            captureDate: captureDate,
            sdkCommit: "local-debug"
        ),
        probe: probe.wireMap,
        playbackOutcome: exampleHdrEvidencePlaybackOutcome(
            controller: context.controller,
            probe: probe,
            preset: context.preset,
            controlledNetworkFailureEvidence: controlledNetworkFailureEvidence
        ),
        runtimeWarning: nil,
        runtimeError: context.controller.lastError,
        controlledNetworkFailureEvidence: controlledNetworkFailureEvidence,
        expectedAxis: context.preset.expectedAxis,
        missingEvidence: exampleHdrEvidenceMissingEvidence(
            for: context.preset,
            controlledNetworkFailureEvidence: controlledNetworkFailureEvidence
        ),
        platformLog: exampleHdrEvidencePlatformLog(
            source: context.source,
            controller: context.controller,
            probe: probe,
            controlledNetworkFailureEvidence: controlledNetworkFailureEvidence
        ),
        notes: nil
    )
    return try ExampleHdrEvidenceBundleWriter(
        outputRoot: exampleHdrEvidenceOutputRoot()
    ).write(bundle, overwrite: true)
}

private struct ExampleHdrEvidenceBundle {
    let sampleId: String
    let deviceId: String
    let platform: String
    let captureDate: String
    let sdkCommit: String
    let sourceMetadata: [String: Any]
    let device: [String: Any]
    let probe: [String: Any]
    let playbackOutcome: String
    let runtimeWarning: [String: Any]?
    let runtimeError: VesperPlayerError?
    let controlledNetworkFailureEvidence: ExampleControlledNetworkFailureEvidence?
    let expectedAxis: String
    let missingEvidence: [String]
    let platformLog: String
    let notes: String?
}

private struct ExampleControlledNetworkFailureEvidence {
    let observed: Bool
    let sourceUri: String
    let errorDomain: String?
    let errorCode: Int?
    let errorDescription: String?
    let durationMs: Int
    let timedOut: Bool

    var details: [String: String] {
        var values: [String: String] = [
            "sourceUri": sourceUri,
            "durationMs": "\(durationMs)",
            "timedOut": timedOut ? "true" : "false",
            "iosRuntimeEvidenceSource": "ios-swift-host-controlled-url",
        ]
        if let errorDomain {
            values["nsErrorDomain"] = errorDomain
        }
        if let errorCode {
            values["nsErrorCode"] = "\(errorCode)"
        }
        if let errorDescription {
            values["networkFailureMessage"] = errorDescription
        }
        return values
    }
}

private struct ExampleHdrEvidenceBundleWriter {
    let outputRoot: URL

    func write(
        _ bundle: ExampleHdrEvidenceBundle,
        overwrite: Bool
    ) throws -> URL {
        let directory = outputRoot
            .appendingPathComponent(bundle.captureDate, isDirectory: true)
            .appendingPathComponent(bundle.deviceId, isDirectory: true)
            .appendingPathComponent(bundle.sampleId, isDirectory: true)
        if FileManager.default.fileExists(atPath: directory.path), overwrite {
            try FileManager.default.removeItem(at: directory)
        }
        try FileManager.default.createDirectory(
            at: directory,
            withIntermediateDirectories: true
        )
        try writeJson(bundle.device, to: directory.appendingPathComponent("device.json"))
        try writeJson(
            exampleHdrEvidenceSourceMetadataJson(bundle),
            to: directory.appendingPathComponent("source-metadata.json")
        )
        try writeJson(
            exampleHdrEvidenceProbeJson(bundle, schema: "vesper-hdr-dv-probe-host-v1"),
            to: directory.appendingPathComponent("probe-host.json")
        )
        try writeJson(
            exampleHdrEvidenceFlutterProbeJson(bundle),
            to: directory.appendingPathComponent("probe-flutter.json")
        )
        try writeJson(
            exampleHdrEvidenceRuntimeWarningJson(bundle),
            to: directory.appendingPathComponent("runtime-warning.json")
        )
        try writeJson(
            exampleHdrEvidenceRuntimeErrorJson(bundle),
            to: directory.appendingPathComponent("runtime-error.json")
        )
        try writeJson(
            exampleHdrEvidenceTypedEvidenceJson(bundle),
            to: directory.appendingPathComponent("typed-evidence.json")
        )
        try bundle.platformLog.write(
            to: directory.appendingPathComponent("platform-log.txt"),
            atomically: true,
            encoding: .utf8
        )
        try exampleHdrEvidenceNotes(bundle, bundlePath: directory.path).write(
            to: directory.appendingPathComponent("notes.md"),
            atomically: true,
            encoding: .utf8
        )
        return directory
    }

    private func writeJson(_ value: [String: Any], to url: URL) throws {
        let data = try JSONSerialization.data(
            withJSONObject: exampleHdrEvidenceJsonValue(value),
            options: [.prettyPrinted, .sortedKeys]
        )
        var output = data
        output.append(0x0A)
        try output.write(to: url, options: .atomic)
    }
}

private func exampleHdrEvidenceOutputRoot() throws -> URL {
    let documents = FileManager.default.urls(
        for: .documentDirectory,
        in: .userDomainMask
    ).first ?? URL(fileURLWithPath: NSTemporaryDirectory())
    let root = documents.appendingPathComponent("hdr-dv-evidence", isDirectory: true)
    try FileManager.default.createDirectory(at: root, withIntermediateDirectories: true)
    return root
}

private func exampleHdrEvidenceDevice(
    deviceId: String,
    captureDate: String,
    sdkCommit: String
) -> [String: Any] {
    let screen = UIScreen.main
    return [
        "schema": "vesper-hdr-dv-device-v1",
        "deviceId": deviceId,
        "platform": "ios",
        "captureDate": captureDate,
        "sdkCommit": sdkCommit,
        "hostApp": [
            "name": "ios-swift-host",
            "version": Bundle.main.object(
                forInfoDictionaryKey: "CFBundleShortVersionString"
            ) as? String ?? "debug",
            "displayPath": "AVPlayer",
        ],
        "android": [
            "manufacturer": "TBD",
            "model": "TBD",
            "apiLevel": "TBD",
            "buildFingerprint": "TBD",
            "displayHdrTypes": [],
            "displayRefreshRate": NSNull(),
            "displayModes": [],
            "media3Version": "TBD",
            "decoderCandidates": [
                "hevc": [],
                "dolbyVision": [],
            ],
        ],
        "ios": [
            "model": exampleDeviceModelIdentifier(),
            "iosVersion": UIDevice.current.systemVersion,
            "avPlayerEligibleForHdrPlayback": AVPlayer.eligibleForHDRPlayback,
            "displayGamut": exampleDisplayGamutName(screen.traitCollection.displayGamut),
            "nativeDisplaySize": [
                "width": Int(screen.nativeBounds.width.rounded()),
                "height": Int(screen.nativeBounds.height.rounded()),
            ],
            "maximumFramesPerSecond": screen.maximumFramesPerSecond,
        ],
        "knownCaveats": [
            "Captured through ios-swift-host debug helper; native-host probe-flutter.json mirrors the host probe and is not Flutter parity evidence.",
        ],
    ]
}

private func exampleHdrEvidenceSourceMetadata(
    preset: ExampleHdrEvidenceSamplePreset,
    source: VesperPlayerSource
) -> [String: Any] {
    var metadata = preset.sourceMetadata
    metadata["sourceUri"] = source.uri
    metadata["sourceKind"] = exampleHdrEvidenceSourceKind(source)
    metadata["manifestKind"] = exampleHdrEvidenceManifestKind(source)
    return metadata
}

private func exampleHdrEvidenceSourceMetadataJson(
    _ bundle: ExampleHdrEvidenceBundle
) -> [String: Any] {
    exampleMergeMaps(
        [
            "schema": "vesper-hdr-dv-source-metadata-v1",
            "sampleId": bundle.sampleId,
            "sourceKind": "TBD",
            "sourceUri": "TBD",
            "container": "TBD",
            "manifestKind": "none",
            "codec": "TBD",
            "sampleMimeType": "TBD",
            "width": NSNull(),
            "height": NSNull(),
            "frameRate": NSNull(),
            "bitDepth": NSNull(),
            "hdrKind": "none",
            "colorPrimaries": "TBD",
            "transferFunction": "TBD",
            "yCbCrMatrix": "TBD",
            "maxContentLightLevelNits": NSNull(),
            "maxFrameAverageLightLevelNits": NSNull(),
            "masteringDisplay": [
                "present": NSNull(),
                "primary0": NSNull(),
                "primary1": NSNull(),
                "primary2": NSNull(),
                "whitePoint": NSNull(),
                "maxLuminanceNits": NSNull(),
                "minLuminanceNits": NSNull(),
            ],
            "dolbyVision": [
                "codec": bundle.probe.pathValue("hdrMetadata", "dolbyVisionCodec"),
                "profile": bundle.probe.pathValue("hdrMetadata", "dolbyVisionProfile"),
                "level": bundle.probe.pathValue("hdrMetadata", "dolbyVisionLevel"),
                "compatibility": bundle.probe.pathValue("hdrMetadata", "dolbyVisionCompatibility"),
                "profileFamily": bundle.probe.pathValue("hdrMetadata", "dolbyVisionProfileFamily"),
                "baseLayer": bundle.probe.pathValue("hdrMetadata", "dolbyVisionBaseLayer"),
                "fallbackTarget": bundle.probe.pathValue("hdrMetadata", "dolbyVisionFallbackTarget"),
                "baseLayerEvidence": bundle.probe.pathValue("hdrMetadata", "dolbyVisionBaseLayerEvidence"),
                "baseLayerTransferFunction": bundle.probe.pathValue(
                    "hdrMetadata",
                    "dolbyVisionBaseLayerTransferFunction"
                ),
                "containerEvidence": NSNull(),
            ],
            "controlPurpose": "none",
            "metadataTool": [
                "name": "ios-swift-host-preset",
                "version": "debug",
                "command": "example native HDR evidence capture",
            ],
            "notes": [],
        ],
        bundle.sourceMetadata
    )
}

private func exampleHdrEvidenceProbeJson(
    _ bundle: ExampleHdrEvidenceBundle,
    schema: String
) -> [String: Any] {
    [
        "schema": schema,
        "sampleId": bundle.sampleId,
        "deviceId": bundle.deviceId,
        "platform": bundle.platform,
        "captureDate": bundle.captureDate,
        "request": [
            "codec": bundle.sourceMetadata["codec"] ?? NSNull(),
            "width": bundle.sourceMetadata["width"] ?? NSNull(),
            "height": bundle.sourceMetadata["height"] ?? NSNull(),
            "frameRate": bundle.sourceMetadata["frameRate"] ?? NSNull(),
            "hdrKind": bundle.sourceMetadata["hdrKind"] ?? NSNull(),
            "sourceKind": bundle.sourceMetadata["sourceKind"] ?? NSNull(),
            "manifestKind": bundle.sourceMetadata["manifestKind"] ?? NSNull(),
        ],
        "result": exampleHdrEvidenceProbeResult(bundle.probe),
        "diagnosticGroups": exampleHdrEvidenceProbeDiagnosticGroups(bundle.probe),
        "capturedVia": "ios-swift-host",
    ]
}

private func exampleHdrEvidenceFlutterProbeJson(
    _ bundle: ExampleHdrEvidenceBundle
) -> [String: Any] {
    var json = exampleHdrEvidenceProbeJson(
        bundle,
        schema: "vesper-hdr-dv-probe-flutter-v1"
    )
    json["capturedVia"] = "ios-swift-host-native-mirror"
    json["matchesHostProbe"] = true
    return json
}

private func exampleHdrEvidenceProbeResult(_ probe: [String: Any]) -> [String: Any] {
    [
        "status": probe["status"] ?? "unknown",
        "recommendedPlaybackPath": probe["recommendedPlaybackPath"] ?? "systemPlayer",
        "confidence": probe["confidence"] ?? "codecOnly",
        "hdrKind": probe["hdrKind"] ?? "none",
        "missingCapabilities": probe["missingCapabilities"] ?? [],
        "hdrMetadata": probe["hdrMetadata"] as? [String: Any] ?? [:],
    ]
}

private func exampleHdrEvidenceProbeDiagnosticGroups(_ probe: [String: Any]) -> [String: Any] {
    let diagnostics = probe["diagnostics"] as? [String: String] ?? [:]
    return [
        "display": diagnostics.filter {
            $0.key.hasPrefix("display") ||
                $0.key.hasPrefix("avPlayer") ||
                $0.key.hasPrefix("requestedFrameRate")
        },
        "codecFormat": diagnostics.filter { $0.key.hasPrefix("codecFormat") },
        "asset": diagnostics.filter { $0.key.hasPrefix("asset") },
        "dolbyVision": diagnostics.filter { $0.key.hasPrefix("dolbyVision") },
        "other": diagnostics.filter { entry in
            !entry.key.hasPrefix("display") &&
                !entry.key.hasPrefix("avPlayer") &&
                !entry.key.hasPrefix("requestedFrameRate") &&
                !entry.key.hasPrefix("codecFormat") &&
                !entry.key.hasPrefix("asset") &&
                !entry.key.hasPrefix("dolbyVision")
        },
    ]
}

private func exampleHdrEvidenceRuntimeWarningJson(
    _ bundle: ExampleHdrEvidenceBundle
) -> [String: Any] {
    [
        "schema": "vesper-hdr-dv-runtime-warning-v1",
        "sampleId": bundle.sampleId,
        "deviceId": bundle.deviceId,
        "platform": bundle.platform,
        "captureDate": bundle.captureDate,
        "observed": false,
        "warning": exampleEmptyCapabilityEvidence(probe: bundle.probe),
    ]
}

private func exampleHdrEvidenceRuntimeErrorJson(
    _ bundle: ExampleHdrEvidenceBundle
) -> [String: Any] {
    let controlledNetworkFailureEvidence = bundle.controlledNetworkFailureEvidence
    let error = bundle.runtimeError
    let details = controlledNetworkFailureEvidence?.details ?? error?.details ?? [:]
    let errorJson: [String: Any] = [
        "code": controlledNetworkFailureEvidence?.observed == true
            ? VesperPlayerErrorCode.backendFailure.rawValue
            : error?.code.rawValue as Any,
        "category": controlledNetworkFailureEvidence?.observed == true
            ? VesperPlayerErrorCategory.network.rawValue
            : error?.category.rawValue as Any,
        "message": controlledNetworkFailureEvidence?.errorDescription as Any ?? error?.message as Any,
        "retriable": controlledNetworkFailureEvidence?.observed == true ? true : error?.retriable as Any,
        "details": details,
    ]
    let iosJson: [String: Any] = [
        "avErrorCode": details["avErrorCode"] ?? NSNull(),
        "nsErrorDomain": details["nsErrorDomain"] ?? NSNull(),
        "nsErrorCode": details["nsErrorCode"] ?? NSNull(),
        "iosRuntimeEvidenceSource": details["iosRuntimeEvidenceSource"] ?? NSNull(),
        "iosRuntimeFailureCategory": details["iosRuntimeFailureCategory"] ?? NSNull(),
        "iosRuntimeFailureRetriable": details["iosRuntimeFailureRetriable"] ?? NSNull(),
        "iosRuntimeFailureCode": details["iosRuntimeFailureCode"] ?? NSNull(),
        "capabilityFailureCause": details["capabilityFailureCause"] ?? NSNull(),
        "missingCapabilities": details["missingCapabilities"] ?? NSNull(),
        "sessionProbe": details["sessionProbe"] ?? NSNull(),
        "displayHdrProbeAvailable": details["displayHdrProbeAvailable"] ?? NSNull(),
        "displayHdrSupported": details["displayHdrSupported"] ?? NSNull(),
        "displayGamut": details["displayGamut"] ?? NSNull(),
        "avPlayerEligibleForHDRPlayback": details["avPlayerEligibleForHDRPlayback"] ?? NSNull(),
        "hdrKindSupportBasis": details["hdrKindSupportBasis"] ?? NSNull(),
        "displayFrameRateSupported": details["displayFrameRateSupported"] ?? NSNull(),
        "displayMaximumFramesPerSecond": details["displayMaximumFramesPerSecond"] ?? NSNull(),
        "displayNativeWidth": details["displayNativeWidth"] ?? NSNull(),
        "displayNativeHeight": details["displayNativeHeight"] ?? NSNull(),
        "requestedWidth": details["requestedWidth"] ?? NSNull(),
        "requestedHeight": details["requestedHeight"] ?? NSNull(),
        "requestedFrameRate": details["requestedFrameRate"] ?? NSNull(),
        "avPlayerItemStatusEvidenceSource": details["avPlayerItemStatusEvidenceSource"] ?? NSNull(),
        "avPlayerItemStatus": details["avPlayerItemStatus"] ?? NSNull(),
        "avPlayerItemErrorLogEvidenceSource": details["avPlayerItemErrorLogEvidenceSource"] ?? NSNull(),
        "avPlayerItemErrorLogEventCount": details["avPlayerItemErrorLogEventCount"] ?? NSNull(),
        "avPlayerItemErrorLogRecentEventCount": details[
            "avPlayerItemErrorLogRecentEventCount"
        ] ?? NSNull(),
        "avPlayerItemErrorLogEvents": details["avPlayerItemErrorLogEvents"] ?? NSNull(),
        "avPlayerItemErrorStatusCode": details["avPlayerItemErrorStatusCode"] ?? NSNull(),
        "avPlayerItemErrorDomain": details["avPlayerItemErrorDomain"] ?? NSNull(),
        "avPlayerItemErrorComment": details["avPlayerItemErrorComment"] ?? NSNull(),
        "networkEvidenceSource": controlledNetworkFailureEvidence?.observed == true
            ? "ios-swift-host-controlled-url"
            : NSNull(),
        "networkFailureMessage": controlledNetworkFailureEvidence?.errorDescription as Any,
        "networkFailureDurationMs": controlledNetworkFailureEvidence?.durationMs as Any,
    ]
    return [
        "schema": "vesper-hdr-dv-runtime-error-v1",
        "sampleId": bundle.sampleId,
        "deviceId": bundle.deviceId,
        "platform": bundle.platform,
        "captureDate": bundle.captureDate,
        "playbackOutcome": bundle.playbackOutcome,
        "observed": controlledNetworkFailureEvidence?.observed == true || error != nil,
        "error": errorJson,
        "android": [:],
        "ios": iosJson,
        "expectedAxis": bundle.expectedAxis,
        "axisSupportedByEvidence": exampleHdrEvidenceAxisSupportedByEvidence(bundle),
        "missingEvidence": bundle.missingEvidence,
        "matchesHostEvidence": true,
        "evidenceMismatches": [],
    ]
}

private func exampleHdrEvidenceTypedEvidenceJson(
    _ bundle: ExampleHdrEvidenceBundle
) -> [String: Any] {
    let warningPresent = bundle.runtimeWarning != nil
    let errorPresent = bundle.runtimeError != nil
    return [
        "schema": "vesper-hdr-dv-typed-evidence-v1",
        "sampleId": bundle.sampleId,
        "deviceId": bundle.deviceId,
        "platform": bundle.platform,
        "captureDate": bundle.captureDate,
        "flutter": [
            "vesperCapabilityWarning": exampleCapabilityEvidence(
                present: warningPresent,
                probe: bundle.probe
            ),
            "vesperHdrCapabilityEvidence": exampleCapabilityEvidence(
                present: errorPresent,
                probe: bundle.probe
            ),
        ],
        "matchesHostEvidence": true,
        "probeMismatches": [],
        "evidenceMismatches": [],
    ]
}

private func exampleCapabilityEvidence(
    present: Bool,
    probe: [String: Any]
) -> [String: Any] {
    var evidence = exampleEmptyCapabilityEvidence(probe: probe)
    evidence["present"] = present
    return evidence
}

private func exampleEmptyCapabilityEvidence(probe: [String: Any]) -> [String: Any] {
    [
        "reason": NSNull(),
        "recommendedPlaybackPath": NSNull(),
        "hdrKind": probe["hdrKind"] ?? "none",
        "likelyHdrCapabilityIssue": false,
        "confidence": probe["confidence"] ?? "codecOnly",
        "errorCode": NSNull(),
        "capabilityFailureCause": NSNull(),
        "capabilityFailureAxis": NSNull(),
        "hdrMetadata": probe["hdrMetadata"] as? [String: Any] ?? [:],
        "diagnostics": probe["diagnostics"] as? [String: Any] ?? [:],
        "message": NSNull(),
    ]
}

private func exampleHdrEvidenceNotes(
    _ bundle: ExampleHdrEvidenceBundle,
    bundlePath: String
) -> String {
    bundle.notes ?? """
    # HDR / Dolby Vision Evidence Notes

    - Bundle path: `\(bundlePath)`
    - Sample ID: `\(bundle.sampleId)`
    - Device ID: `\(bundle.deviceId)`
    - Platform: `\(bundle.platform)`
    - Capture date: `\(bundle.captureDate)`
    - Host app: `ios-swift-host`
    - Playback outcome: `\(bundle.playbackOutcome)`
    - Expected axis: `\(bundle.expectedAxis)`

    ## Evidence Summary

    - Native host probe captured in `probe-host.json`.
    - `probe-flutter.json` mirrors the native host probe so the existing validator can compare route policy; it is not Flutter parity evidence.
    - HDR/DV policy remains systemPlayer-only for this capture path.
    - Source metadata uses the selected P0 preset plus the active source URI.
    - Missing evidence: `\(bundle.missingEvidence.isEmpty ? "none" : bundle.missingEvidence.joined(separator: "; "))`
    """
}

@MainActor
private func exampleHdrEvidencePlatformLog(
    source: VesperPlayerSource,
    controller: VesperPlayerController,
    probe: VesperPlaybackCapabilityProbeResult,
    controlledNetworkFailureEvidence: ExampleControlledNetworkFailureEvidence?
) -> String {
    """
    ios-swift-host HDR evidence capture
    source=\(source.uri)
    playbackState=\(controller.uiState.playbackState.rawValue)
    route=\(probe.recommendedPlaybackPath.rawValue)
    status=\(probe.status.rawValue)
    hdrKind=\(probe.hdrKind.rawValue)
    missingCapabilities=\(probe.missingCapabilities.joined(separator: ","))
    controlledNetworkFailureObserved=\(controlledNetworkFailureEvidence?.observed == true)
    controlledNetworkFailureDomain=\(controlledNetworkFailureEvidence?.errorDomain ?? "none")
    controlledNetworkFailureCode=\(controlledNetworkFailureEvidence.map { "\($0.errorCode ?? 0)" } ?? "none")
    controlledNetworkFailureDurationMs=\(controlledNetworkFailureEvidence.map { "\($0.durationMs)" } ?? "none")
    pluginDiagnostics=\(controller.pluginDiagnostics)
    """
}

private func exampleHdrEvidenceProbeCodec(_ metadata: [String: Any]) -> String? {
    let codec = (metadata["codec"] as? String)?.trimmingCharacters(in: .whitespacesAndNewlines)
    let hdrKind = (metadata["hdrKind"] as? String)?.trimmingCharacters(in: .whitespacesAndNewlines)
    guard let codec, !codec.isEmpty, codec != "none" else {
        return nil
    }
    guard let hdrKind, hdrKind != "none", hdrKind != "unknown" else {
        return codec
    }
    return "\(codec),\(hdrKind)"
}

@MainActor
private func exampleHdrEvidencePlaybackOutcome(
    controller: VesperPlayerController,
    probe: VesperPlaybackCapabilityProbeResult,
    preset: ExampleHdrEvidenceSamplePreset,
    controlledNetworkFailureEvidence: ExampleControlledNetworkFailureEvidence?
) -> String {
    if preset.sampleId == "NETWORK-FAILURE-CONTROL",
        controlledNetworkFailureEvidence?.observed == true {
        return "failure"
    }
    if controller.lastError != nil {
        return "failure"
    }
    if preset.sampleId == "NETWORK-FAILURE-CONTROL" {
        return "notRun"
    }
    if probe.recommendedPlaybackPath == .systemPlayer,
        probe.missingCapabilities.contains("hdrProgrammableProcessingNotSupported") {
        return "fallback"
    }
    return "success"
}

private func exampleHdrEvidenceMissingEvidence(
    for preset: ExampleHdrEvidenceSamplePreset,
    controlledNetworkFailureEvidence: ExampleControlledNetworkFailureEvidence?
) -> [String] {
    if preset.sampleId == "NETWORK-FAILURE-CONTROL" {
        if controlledNetworkFailureEvidence?.observed == true {
            return []
        }
        return ["controlled network failure must be observed in runtime-error.json"]
    }
    return []
}

private func exampleHdrEvidenceAxisSupportedByEvidence(
    _ bundle: ExampleHdrEvidenceBundle
) -> Any {
    if bundle.expectedAxis == "none" {
        return true
    }
    if bundle.sampleId == "NETWORK-FAILURE-CONTROL" {
        return bundle.controlledNetworkFailureEvidence?.observed == true
    }
    return NSNull()
}

private func exampleHdrEvidenceControlledNetworkFailureEvidence(
    preset: ExampleHdrEvidenceSamplePreset,
    source: VesperPlayerSource
) async -> ExampleControlledNetworkFailureEvidence? {
    guard preset.sampleId == "NETWORK-FAILURE-CONTROL",
        let url = URL(string: source.uri)
    else {
        return nil
    }

    let startedAt = Date()
    let configuration = URLSessionConfiguration.ephemeral
    configuration.timeoutIntervalForRequest = 3
    configuration.timeoutIntervalForResource = 3
    let session = URLSession(configuration: configuration)
    defer {
        session.invalidateAndCancel()
    }

    do {
        _ = try await session.data(from: url)
        let durationMs = Int(Date().timeIntervalSince(startedAt) * 1000)
        return ExampleControlledNetworkFailureEvidence(
            observed: false,
            sourceUri: source.uri,
            errorDomain: nil,
            errorCode: nil,
            errorDescription: "controlled URL unexpectedly returned data",
            durationMs: durationMs,
            timedOut: false
        )
    } catch {
        let durationMs = Int(Date().timeIntervalSince(startedAt) * 1000)
        let nsError = error as NSError
        return ExampleControlledNetworkFailureEvidence(
            observed: true,
            sourceUri: source.uri,
            errorDomain: nsError.domain,
            errorCode: nsError.code,
            errorDescription: nsError.localizedDescription,
            durationMs: durationMs,
            timedOut: nsError.domain == NSURLErrorDomain && nsError.code == NSURLErrorTimedOut
        )
    }
}

private func exampleHdrEvidenceCaptureDate() -> String {
    let formatter = DateFormatter()
    formatter.calendar = Calendar(identifier: .gregorian)
    formatter.locale = Locale(identifier: "en_US_POSIX")
    formatter.timeZone = TimeZone.current
    formatter.dateFormat = "yyyy-MM-dd"
    return formatter.string(from: Date())
}

private func exampleDeviceModelIdentifier() -> String {
    var systemInfo = utsname()
    uname(&systemInfo)
    return withUnsafePointer(to: &systemInfo.machine) { pointer in
        pointer.withMemoryRebound(to: CChar.self, capacity: 1) {
            String(cString: $0)
        }
    }
}

private func exampleDisplayGamutName(_ gamut: UIDisplayGamut) -> String {
    switch gamut {
    case .P3:
        return "P3"
    case .SRGB:
        return "SRGB"
    case .unspecified:
        return "unspecified"
    @unknown default:
        return "unknown"
    }
}

private func exampleHdrEvidenceSourceKind(_ source: VesperPlayerSource) -> String {
    switch source.protocol {
    case .file, .content:
        return "file"
    case .hls:
        return "hls"
    case .dash, .progressive:
        return "progressive"
    case .unknown:
        return source.kind == .local ? "file" : "progressive"
    }
}

private func exampleHdrEvidenceManifestKind(_ source: VesperPlayerSource) -> String {
    switch source.protocol {
    case .hls:
        return "hls"
    case .dash:
        return "dash"
    case .unknown, .file, .content, .progressive:
        return "none"
    }
}

private func exampleHdrEvidenceJsonValue(_ value: Any) -> Any {
    if value is NSNull {
        return value
    }
    if let dictionary = value as? [String: Any] {
        var output: [String: Any] = [:]
        dictionary.forEach { key, value in
            output[key] = exampleHdrEvidenceJsonValue(value)
        }
        return output
    }
    if let array = value as? [Any] {
        return array.map(exampleHdrEvidenceJsonValue)
    }
    let mirror = Mirror(reflecting: value)
    if mirror.displayStyle == .optional {
        guard let child = mirror.children.first else {
            return NSNull()
        }
        return exampleHdrEvidenceJsonValue(child.value)
    }
    return value
}

private func exampleMergeMaps(
    _ defaults: [String: Any],
    _ overrides: [String: Any]
) -> [String: Any] {
    var result = defaults
    for (key, override) in overrides {
        if let base = result[key] as? [String: Any],
            let overrideMap = override as? [String: Any] {
            result[key] = exampleMergeMaps(base, overrideMap)
        } else {
            result[key] = override
        }
    }
    return result
}

private extension Dictionary where Key == String, Value == Any {
    func pathValue(_ first: String, _ second: String) -> Any {
        guard let child = self[first] as? [String: Any] else {
            return NSNull()
        }
        return child[second] ?? NSNull()
    }
}
