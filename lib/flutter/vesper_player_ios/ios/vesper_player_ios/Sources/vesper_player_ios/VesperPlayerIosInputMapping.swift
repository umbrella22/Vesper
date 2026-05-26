import Foundation
import VesperPlayerKit

extension Dictionary where Key == String, Value == Any {
    func toVesperPlayerSource() throws -> VesperPlayerSource {
        guard let uri = self["uri"] as? String, !uri.isEmpty else {
            throw PluginError.invalidSource("Missing source uri.")
        }
        let label = self["label"] as? String ?? uri
        let kind = (self["kind"] as? String) == "remote"
            ? VesperPlayerSourceKind.remote
            : VesperPlayerSourceKind.local
        let `protocol`: VesperPlayerSourceProtocol
        switch self["protocol"] as? String {
        case "file":
            `protocol` = .file
        case "content":
            `protocol` = .content
        case "progressive":
            `protocol` = .progressive
        case "hls":
            `protocol` = .hls
        case "dash":
            `protocol` = .dash
        default:
            `protocol` = .unknown
        }
        let headers = stringMap(self["headers"])
        return try VesperPlayerSource(
            uri: uri,
            label: label,
            kind: kind,
            protocol: `protocol`,
            headers: headers
        )
        .validatedForIosBackend()
    }

    func toRawVesperPlayerSource() throws -> VesperPlayerSource {
        guard let uri = self["uri"] as? String, !uri.isEmpty else {
            throw PluginError.invalidSource("Missing source uri.")
        }
        let label = self["label"] as? String ?? uri
        let kind = (self["kind"] as? String) == "remote"
            ? VesperPlayerSourceKind.remote
            : VesperPlayerSourceKind.local
        let `protocol`: VesperPlayerSourceProtocol
        switch self["protocol"] as? String {
        case "file":
            `protocol` = .file
        case "content":
            `protocol` = .content
        case "progressive":
            `protocol` = .progressive
        case "hls":
            `protocol` = .hls
        case "dash":
            `protocol` = .dash
        default:
            `protocol` = .unknown
        }
        let headers = stringMap(self["headers"])
        return try VesperPlayerSource(
            uri: uri,
            label: label,
            kind: kind,
            protocol: `protocol`,
            headers: headers
        )
    }

    func toSystemPlaybackConfiguration() -> VesperSystemPlaybackConfiguration {
        let backgroundMode: VesperBackgroundPlaybackMode =
            (self["backgroundMode"] as? String) == "disabled" ? .disabled : .continueAudio
        return VesperSystemPlaybackConfiguration(
            enabled: self["enabled"] as? Bool ?? true,
            backgroundMode: backgroundMode,
            showSystemControls: self["showSystemControls"] as? Bool ?? true,
            showSeekActions: self["showSeekActions"] as? Bool ?? true,
            metadata: (try? nestedMap(self["metadata"]))?.toSystemPlaybackMetadata(),
            controls: (try? nestedMap(self["controls"]))?.toSystemPlaybackControls()
                ?? .videoDefault()
        )
    }

    func toSystemPlaybackMetadata() -> VesperSystemPlaybackMetadata {
        VesperSystemPlaybackMetadata(
            title: self["title"] as? String ?? "",
            artist: self["artist"] as? String,
            albumTitle: self["albumTitle"] as? String,
            artworkUri: self["artworkUri"] as? String,
            contentUri: self["contentUri"] as? String,
            durationMs: (self["durationMs"] as? NSNumber)?.int64Value,
            isLive: self["isLive"] as? Bool ?? false
        )
    }

    func toSystemPlaybackControls() -> VesperSystemPlaybackControls {
        let buttons = (self["compactButtons"] as? [Any])?
            .compactMap { stringKeyedMap($0)?.toSystemPlaybackControlButton() } ?? []
        return VesperSystemPlaybackControls(compactButtons: buttons)
    }

    func toSystemPlaybackControlButton() -> VesperSystemPlaybackControlButton {
        let kind: VesperSystemPlaybackControlKind
        switch self["kind"] as? String {
        case "seekBack":
            kind = .seekBack
        case "seekForward":
            kind = .seekForward
        default:
            kind = .playPause
        }
        return VesperSystemPlaybackControlButton(
            kind: kind,
            seekOffsetMs: (self["seekOffsetMs"] as? NSNumber)?.int64Value
        )
    }

    func toTrackSelection() throws -> VesperTrackSelection {
        switch self["mode"] as? String {
        case "disabled":
            return .disabled()
        case "track":
            guard let trackId = self["trackId"] as? String, !trackId.isEmpty else {
                throw PluginError.invalidTrackSelection("Missing trackId for track selection.")
            }
            return .track(trackId)
        default:
            return .auto()
        }
    }

    func toAbrPolicy() throws -> VesperAbrPolicy {
        switch self["mode"] as? String {
        case "constrained":
            return .constrained(
                maxBitRate: (self["maxBitRate"] as? NSNumber)?.int64Value,
                maxWidth: (self["maxWidth"] as? NSNumber)?.intValue,
                maxHeight: (self["maxHeight"] as? NSNumber)?.intValue
            )
        case "fixedTrack":
            guard let trackId = self["trackId"] as? String, !trackId.isEmpty else {
                throw PluginError.invalidAbrPolicy("Missing trackId for fixed track policy.")
            }
            return .fixedTrack(trackId)
        default:
            return .auto()
        }
    }

    func toResiliencePolicy() throws -> VesperPlaybackResiliencePolicy {
        let buffering = try (nestedMap(self["buffering"])?.toBufferingPolicy()) ?? VesperBufferingPolicy()
        let retry = try (nestedMap(self["retry"])?.toRetryPolicy()) ?? VesperRetryPolicy()
        let cache = try (nestedMap(self["cache"])?.toCachePolicy()) ?? VesperCachePolicy()
        return VesperPlaybackResiliencePolicy(
            buffering: buffering,
            retry: retry,
            cache: cache
        )
    }

    func toTrackPreferencePolicy() throws -> VesperTrackPreferencePolicy {
        let audioSelection = try (nestedMap(self["audioSelection"])?.toTrackSelection()) ?? .auto()
        let subtitleSelection =
            try (nestedMap(self["subtitleSelection"])?.toTrackSelection()) ?? .disabled()
        let abrPolicy = try (nestedMap(self["abrPolicy"])?.toAbrPolicy()) ?? .auto()
        return VesperTrackPreferencePolicy(
            preferredAudioLanguage: self["preferredAudioLanguage"] as? String,
            preferredSubtitleLanguage: self["preferredSubtitleLanguage"] as? String,
            selectSubtitlesByDefault: self["selectSubtitlesByDefault"] as? Bool ?? false,
            selectUndeterminedSubtitleLanguage:
                self["selectUndeterminedSubtitleLanguage"] as? Bool ?? false,
            audioSelection: audioSelection,
            subtitleSelection: subtitleSelection,
            abrPolicy: abrPolicy
        )
    }

    func toPreloadBudgetPolicy() -> VesperPreloadBudgetPolicy {
        VesperPreloadBudgetPolicy(
            maxConcurrentTasks: (self["maxConcurrentTasks"] as? NSNumber)?.intValue,
            maxMemoryBytes: (self["maxMemoryBytes"] as? NSNumber)?.int64Value,
            maxDiskBytes: (self["maxDiskBytes"] as? NSNumber)?.int64Value,
            warmupWindowMs: (self["warmupWindowMs"] as? NSNumber)?.int64Value
        )
    }

    func toBenchmarkConfiguration() -> VesperBenchmarkConfiguration {
        VesperBenchmarkConfiguration(
            enabled: self["enabled"] as? Bool ?? false,
            maxBufferedEvents: (self["maxBufferedEvents"] as? NSNumber)?.intValue ?? 2_048,
            includeRawEvents: self["includeRawEvents"] as? Bool ?? true,
            consoleLogging: self["consoleLogging"] as? Bool ?? false,
            pluginLibraryPaths:
                (self["pluginLibraryPaths"] as? [Any])?.compactMap { value in
                    value as? String
                } ?? []
        )
    }

    func toSourceNormalizerConfiguration() -> VesperSourceNormalizerConfiguration {
        let mode: VesperSourceNormalizerMode
        switch self["mode"] as? String {
        case "diagnosticsOnly":
            mode = .diagnosticsOnly
        case "preflightOnly":
            mode = .preflightOnly
        case "preferNormalized":
            mode = .preferNormalized
        case "requireNormalized":
            mode = .requireNormalized
        default:
            mode = .disabled
        }
        return VesperSourceNormalizerConfiguration(
            mode: mode,
            pluginLibraryPaths:
                (self["pluginLibraryPaths"] as? [Any])?.compactMap { value in
                    value as? String
                } ?? [],
            runtimeProfile: self["runtimeProfile"] as? String
        )
    }

    func toFrameProcessorConfiguration() -> VesperFrameProcessorConfiguration {
        let mode: VesperFrameProcessorMode =
            (self["mode"] as? String) == "diagnosticsOnly" ? .diagnosticsOnly : .disabled
        return VesperFrameProcessorConfiguration(
            mode: mode,
            pluginLibraryPaths:
                (self["pluginLibraryPaths"] as? [Any])?.compactMap { value in
                    value as? String
                } ?? []
        )
    }

    func toDownloadConfiguration() -> VesperDownloadConfiguration {
        VesperDownloadConfiguration(
            autoStart: self["autoStart"] as? Bool ?? true,
            runPostProcessorsOnCompletion:
                self["runPostProcessorsOnCompletion"] as? Bool ?? true,
            resumePartialDownloads: self["resumePartialDownloads"] as? Bool ?? true,
            restoreTasksOnStartup: self["restoreTasksOnStartup"] as? Bool ?? true,
            baseDirectory: (self["baseDirectory"] as? String).map {
                URL(fileURLWithPath: $0, isDirectory: true)
            },
            pluginLibraryPaths:
                (self["pluginLibraryPaths"] as? [Any])?.compactMap { value in
                    value as? String
                } ?? [],
            rangeChunkBytes: (self["rangeChunkBytes"] as? NSNumber)?.uint64Value,
            minProgressBytes: (self["minProgressBytes"] as? NSNumber)?.uint64Value ?? 512 * 1024,
            minProgressIntervalMs: (self["minProgressIntervalMs"] as? NSNumber)?.uint64Value ?? 250
        )
    }

    func toDownloadRecoveredTaskPlan() throws -> VesperDownloadRecoveredTaskPlan {
        VesperDownloadRecoveredTaskPlan(
            source: try requireNestedMap(arguments: self, key: "source").toDownloadSource(),
            profile: try requireNestedMap(arguments: self, key: "profile").toDownloadProfile(),
            assetIndex: try requireNestedMap(arguments: self, key: "assetIndex").toDownloadAssetIndex()
        )
    }

    func toDownloadSource() throws -> VesperDownloadSource {
        let contentFormat: VesperDownloadContentFormat
        switch self["contentFormat"] as? String {
        case "hlsSegments":
            contentFormat = .hlsSegments
        case "dashSegments":
            contentFormat = .dashSegments
        case "flvSegments":
            contentFormat = .flvSegments
        case "singleFile":
            contentFormat = .singleFile
        default:
            contentFormat = .unknown
        }
        return VesperDownloadSource(
            source: try requireNestedMap(arguments: self, key: "source").toRawVesperPlayerSource(),
            contentFormat: contentFormat,
            manifestUri: self["manifestUri"] as? String
        )
    }

    func toDownloadProfile() -> VesperDownloadProfile {
        VesperDownloadProfile(
            variantId: self["variantId"] as? String,
            preferredAudioLanguage: self["preferredAudioLanguage"] as? String,
            preferredSubtitleLanguage: self["preferredSubtitleLanguage"] as? String,
            selectedTrackIds:
                (self["selectedTrackIds"] as? [Any])?.compactMap { value in
                    value as? String
                } ?? [],
            targetOutputFormat: downloadOutputFormat(
                from: self["targetOutputFormat"] as? String
            ),
            targetDirectory: (self["targetDirectory"] as? String).map {
                URL(fileURLWithPath: $0, isDirectory: true)
            },
            allowMeteredNetwork: self["allowMeteredNetwork"] as? Bool ?? false
        )
    }

    func toDownloadAssetIndex() -> VesperDownloadAssetIndex {
        let contentFormat: VesperDownloadContentFormat
        switch self["contentFormat"] as? String {
        case "hlsSegments":
            contentFormat = .hlsSegments
        case "dashSegments":
            contentFormat = .dashSegments
        case "flvSegments":
            contentFormat = .flvSegments
        case "singleFile":
            contentFormat = .singleFile
        default:
            contentFormat = .unknown
        }
        return VesperDownloadAssetIndex(
            contentFormat: contentFormat,
            version: self["version"] as? String,
            etag: self["etag"] as? String,
            checksum: self["checksum"] as? String,
            totalSizeBytes: (self["totalSizeBytes"] as? NSNumber)?.uint64Value,
            resources:
                (self["resources"] as? [Any])?.compactMap { value in
                    stringKeyedMap(value)?.toDownloadResourceRecord()
                } ?? [],
            segments:
                (self["segments"] as? [Any])?.compactMap { value in
                    stringKeyedMap(value)?.toDownloadSegmentRecord()
                } ?? [],
            streams:
                (self["streams"] as? [Any])?.compactMap { value in
                    stringKeyedMap(value)?.toDownloadAssetStream()
                } ?? [],
            completedPath: self["completedPath"] as? String
        )
    }

    func toDownloadResourceRecord() -> VesperDownloadResourceRecord {
        VesperDownloadResourceRecord(
            resourceId: self["resourceId"] as? String ?? "",
            uri: self["uri"] as? String ?? "",
            relativePath: self["relativePath"] as? String,
            byteRange: stringKeyedMap(self["byteRange"])?.toDownloadByteRange(),
            generatedText: self["generatedText"] as? String,
            sizeBytes: (self["sizeBytes"] as? NSNumber)?.uint64Value,
            etag: self["etag"] as? String,
            checksum: self["checksum"] as? String
        )
    }

    func toDownloadSegmentRecord() -> VesperDownloadSegmentRecord {
        VesperDownloadSegmentRecord(
            segmentId: self["segmentId"] as? String ?? "",
            uri: self["uri"] as? String ?? "",
            relativePath: self["relativePath"] as? String,
            sequence: (self["sequence"] as? NSNumber)?.uint64Value,
            byteRange: stringKeyedMap(self["byteRange"])?.toDownloadByteRange(),
            sizeBytes: (self["sizeBytes"] as? NSNumber)?.uint64Value,
            checksum: self["checksum"] as? String
        )
    }

    func toDownloadAssetStream() -> VesperDownloadAssetStream {
        VesperDownloadAssetStream(
            streamId: self["streamId"] as? String ?? "",
            kind: downloadStreamKind(from: self["kind"] as? String),
            language: self["language"] as? String,
            codec: self["codec"] as? String,
            label: self["label"] as? String,
            qualityRank: (self["qualityRank"] as? NSNumber)?.uint32Value,
            resourceIds: (self["resourceIds"] as? [String]) ?? [],
            segmentIds: (self["segmentIds"] as? [String]) ?? [],
            metadata: (self["metadata"] as? [String: String]) ?? [:]
        )
    }

    func toDownloadByteRange() -> VesperDownloadByteRange {
        VesperDownloadByteRange(
            offset: (self["offset"] as? NSNumber)?.uint64Value ?? 0,
            length: (self["length"] as? NSNumber)?.uint64Value ?? 0
        )
    }

    func toFlutterViewport() -> FlutterViewport {
        FlutterViewport(
            left: (self["left"] as? NSNumber)?.doubleValue ?? 0,
            top: (self["top"] as? NSNumber)?.doubleValue ?? 0,
            width: (self["width"] as? NSNumber)?.doubleValue ?? 0,
            height: (self["height"] as? NSNumber)?.doubleValue ?? 0
        )
    }

    func toFlutterViewportHint() -> FlutterViewportHint {
        let kind: String
        switch self["kind"] as? String {
        case "visible":
            kind = "visible"
        case "nearVisible":
            kind = "nearVisible"
        case "prefetchOnly":
            kind = "prefetchOnly"
        default:
            kind = "hidden"
        }

        let visibleFraction = Swift.max(
            0,
            Swift.min((self["visibleFraction"] as? NSNumber)?.doubleValue ?? 0, 1)
        )
        return FlutterViewportHint(kind: kind, visibleFraction: visibleFraction)
    }

    func toBufferingPolicy() -> VesperBufferingPolicy {
        let preset: VesperBufferingPreset
        switch self["preset"] as? String {
        case "balanced":
            preset = .balanced
        case "streaming":
            preset = .streaming
        case "resilient":
            preset = .resilient
        case "lowLatency":
            preset = .lowLatency
        default:
            preset = .default
        }
        return VesperBufferingPolicy(
            preset: preset,
            minBufferMs: (self["minBufferMs"] as? NSNumber)?.int64Value,
            maxBufferMs: (self["maxBufferMs"] as? NSNumber)?.int64Value,
            bufferForPlaybackMs: (self["bufferForPlaybackMs"] as? NSNumber)?.int64Value,
            bufferForPlaybackAfterRebufferMs:
                (self["bufferForPlaybackAfterRebufferMs"] as? NSNumber)?.int64Value
        )
    }

    func toRetryPolicy() -> VesperRetryPolicy {
        let backoff: VesperRetryBackoff?
        switch self["backoff"] as? String {
        case "fixed":
            backoff = .fixed
        case "linear":
            backoff = .linear
        case "exponential":
            backoff = .exponential
        default:
            backoff = nil
        }
        let maxAttempts: Int?
        if keys.contains("maxAttempts") {
            if self["maxAttempts"] is NSNull {
                maxAttempts = nil
            } else {
                maxAttempts = (self["maxAttempts"] as? NSNumber)?.intValue
            }
        } else {
            maxAttempts = 3
        }
        return VesperRetryPolicy(
            maxAttempts: maxAttempts,
            baseDelayMs: (self["baseDelayMs"] as? NSNumber)?.uint64Value,
            maxDelayMs: (self["maxDelayMs"] as? NSNumber)?.uint64Value,
            backoff: backoff
        )
    }

    func toCachePolicy() -> VesperCachePolicy {
        let preset: VesperCachePreset
        switch self["preset"] as? String {
        case "disabled":
            preset = .disabled
        case "streaming":
            preset = .streaming
        case "resilient":
            preset = .resilient
        default:
            preset = .default
        }
        return VesperCachePolicy(
            preset: preset,
            maxMemoryBytes: (self["maxMemoryBytes"] as? NSNumber)?.int64Value,
            maxDiskBytes: (self["maxDiskBytes"] as? NSNumber)?.int64Value
        )
    }
}
