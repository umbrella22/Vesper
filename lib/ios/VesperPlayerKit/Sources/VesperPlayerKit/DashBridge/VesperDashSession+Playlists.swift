@preconcurrency import AVFoundation
import Foundation
internal import VesperPlayerKitBridgeShim

extension VesperDashSession {
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
            let videoDecodeCapabilities = try videoDecodeCapabilities(for: manifest)
            let playlist = try VesperDashHlsBuilder.buildMasterPlaylist(
                manifest: manifest,
                variantPolicy: variantPolicy,
                videoDecodeCapabilities: videoDecodeCapabilities,
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
            startStartupPrefetch(for: startupSelected.audio + startupSelected.video, manifest: manifest)
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
                    cacheHit: false,
                    extra: masterPlaylistDecodeSelectionAttributes(startupSelected: startupSelected)
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
                    extra: [
                        "renditionId": renditionId,
                        "coalesced": "false",
                    ]
                )
            )
            return cached
        }

        if let inFlightTask = mediaPlaylistTasksByRenditionId[renditionId] {
            do {
                let data = try await inFlightTask.value
                await recordBenchmarkEvent(
                    "dash_media_playlist_request_end",
                    attributes: playlistBenchmarkEndAttributes(
                        startedAt: startedAt,
                        bytes: data.count,
                        cacheHit: false,
                        extra: [
                            "renditionId": renditionId,
                            "coalesced": "true",
                        ]
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
                        extra: [
                            "renditionId": renditionId,
                            "coalesced": "true",
                        ]
                    )
                )
                throw error
            }
        }

        let buildTask = Task { try await self.buildMediaPlaylistData(renditionId: renditionId) }
        mediaPlaylistTasksByRenditionId[renditionId] = buildTask
        do {
            let data = try await buildTask.value
            mediaPlaylistTasksByRenditionId[renditionId] = nil
            await recordBenchmarkEvent(
                "dash_media_playlist_request_end",
                attributes: playlistBenchmarkEndAttributes(
                    startedAt: startedAt,
                    bytes: data.count,
                    cacheHit: false,
                    extra: [
                        "renditionId": renditionId,
                        "coalesced": "false",
                    ]
                )
            )
            return data
        } catch {
            mediaPlaylistTasksByRenditionId[renditionId] = nil
            await recordBenchmarkEvent(
                "dash_media_playlist_request_end",
                attributes: playlistBenchmarkEndAttributes(
                    startedAt: startedAt,
                    bytes: nil,
                    cacheHit: false,
                    error: error,
                    extra: [
                        "renditionId": renditionId,
                        "coalesced": "false",
                    ]
                )
            )
            throw error
        }
    }

    func buildMediaPlaylistData(renditionId: String) async throws -> Data {
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
        startBackgroundSegmentPrefetch(
            renditionId: playable.renditionId,
            segmentCount: segments.count,
            prefetchMediaSegments: shouldPrefetchTemplateMediaSegments(
                playable: playable,
                segments: segments
            )
        )
        // Point EXT-X-MAP and media segments at the vesper-dash:// scheme so
        // every DASH-derived HLS resource goes through AVAssetResourceLoaderDelegate.
        // Missing init bytes surface as 'frmt', so the custom scheme keeps
        // delivery deterministic and visible to benchmark events.
        //
        // WebVTT subtitle renditions use the `.vtt` extension so AVPlayer
        // sees a MIME-aware URL instead of a misleading `.m4s`. The flag is
        // derived from the rendition content type (text/vtt family); audio
        // and video renditions keep `.m4s`/`init.mp4` for backward
        // compatibility with the existing SegmentTemplate golden tests.
        let subtitleFileExtension = subtitleSegmentFileExtension(for: playable)
        let initializationURL = segmentTemplate.initialization.map { _ in
            self.segmentURL(
                for: playable.renditionId,
                segment: .initialization,
                fileExtension: subtitleFileExtension
            ).absoluteString
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
                let segmentURL = self.segmentURL(
                    for: playable.renditionId,
                    segment: .media(segmentIndex),
                    fileExtension: subtitleFileExtension
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
            "dashMediaPlaylist rendition=\(playable.renditionId) resourceLoaderSegments=true count=\(segments.count) init=\(initializationURL ?? "none")"
        )
        // Log the first playlist lines to diagnose HLS tag concatenation
        // regressions, such as EXT-X-PLAYLIST-TYPE and EXT-X-MAP being glued
        // onto one line by a missing trailing newline in a multiline string.
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

    func hlsSegmentIndex(
        manifest: VesperDashManifest,
        segment: VesperDashTemplateSegment,
        fallbackIndex: Int
    ) throws -> Int {
        guard manifest.type == .dynamic else {
            return fallbackIndex
        }
        return try checkedInt(segment.number, field: "DASH live segment number")
    }
}
