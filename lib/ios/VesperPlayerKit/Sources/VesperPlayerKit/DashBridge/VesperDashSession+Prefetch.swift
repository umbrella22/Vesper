@preconcurrency import AVFoundation
import Foundation
@_implementationOnly import VesperPlayerKitBridgeShim

extension VesperDashSession {
    func startBackgroundSegmentPrefetch(
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

    func startStartupPrefetch(
        for playables: [VesperDashPlayableRepresentation],
        manifest: VesperDashManifest
    ) {
        guard !playables.isEmpty else {
            return
        }
        startBackgroundPrefetch(for: playables, manifest: manifest)
        let renditionIds = playables.map(\.renditionId)
        Task(priority: .userInitiated) { [weak self] in
            await self?.runStartupMediaPlaylistPrefetch(renditionIds: renditionIds)
        }
    }

    func runStartupMediaPlaylistPrefetch(renditionIds: [String]) async {
        await recordBenchmarkEvent(
            "dash_startup_prefetch_start",
            attributes: ["renditionIds": renditionIds.joined(separator: ",")]
        )
        let succeeded = await withTaskGroup(of: Bool.self, returning: Int.self) { group in
            for renditionId in renditionIds {
                group.addTask { [weak self] in
                    guard let self else {
                        return false
                    }
                    do {
                        _ = try await self.mediaPlaylistData(renditionId: renditionId)
                        return true
                    } catch {
                        iosHostLog(
                            "dashStartupPrefetch failed rendition=\(renditionId) error=\(error.localizedDescription)"
                        )
                        return false
                    }
                }
            }

            var count = 0
            for await ok in group where ok {
                count += 1
            }
            return count
        }
        await recordBenchmarkEvent(
            "dash_startup_prefetch_end",
            attributes: [
                "requested": "\(renditionIds.count)",
                "succeeded": "\(succeeded)",
            ]
        )
    }

    func startBackgroundPrefetch(
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

    func shouldPrefetchTemplateMediaSegments(
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

    func runBackgroundSegmentPrefetch(
        renditionId: String,
        segmentCount: Int,
        prefetchMediaSegments: Bool
    ) async {
        let prefetchLimit = prefetchMediaSegments
            ? min(segmentCount, Self.startupMediaSegmentPrefetchLimit)
            : 0
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

    func prefetchIgnoringFailure(
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
}
