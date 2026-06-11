@preconcurrency import AVFoundation
import Foundation
import VesperPlayerKitBridgeShim

extension VesperDashSession {
    func cachedSegmentFilePayload(
        for key: VesperDashSegmentCacheKey,
        at url: URL,
        contentType: String
    ) -> VesperDashSegmentPayload? {
        guard FileManager.default.fileExists(atPath: url.path) else {
            if let existing = cachedSegmentFiles.removeValue(forKey: key) {
                segmentCacheTotalBytes = segmentCacheTotalBytes.dashSaturatingSubtract(existing.size)
            }
            return nil
        }
        let size = fileSize(at: url) ?? cachedSegmentFiles[key]?.size ?? 0
        touchCachedSegmentFile(key: key, url: url, size: size)
        return .file(
            url: url,
            offset: 0,
            size: size,
            removeAfterServing: false,
            contentType: contentType
        )
    }

    func cachedSegmentFileExists(
        for key: VesperDashSegmentCacheKey,
        at url: URL
    ) -> Bool {
        guard FileManager.default.fileExists(atPath: url.path) else {
            if let existing = cachedSegmentFiles.removeValue(forKey: key) {
                segmentCacheTotalBytes = segmentCacheTotalBytes.dashSaturatingSubtract(existing.size)
            }
            return false
        }
        let size = fileSize(at: url) ?? cachedSegmentFiles[key]?.size ?? 0
        touchCachedSegmentFile(key: key, url: url, size: size)
        return true
    }

    @discardableResult
    func writeSegmentTemplateCache(
        _ data: Data,
        to url: URL,
        key: VesperDashSegmentCacheKey,
        allowSkippingLargeMediaEntry: Bool
    ) throws -> Bool {
        let size = UInt64(data.count)
        if allowSkippingLargeMediaEntry,
           case .media = key.segment,
           size > Self.segmentCacheMaxSingleMediaBytes {
#if DEBUG
            iosHostLog(
                "dashSegmentCache skipLarge rendition=\(key.renditionId) segment=\(key.segment) bytes=\(size)"
            )
#endif
            return false
        }

        try FileManager.default.createDirectory(
            at: segmentCacheDirectory,
            withIntermediateDirectories: true
        )
        let addsEntry = cachedSegmentFiles[key] == nil
        if let existing = cachedSegmentFiles[key] {
            segmentCacheTotalBytes = segmentCacheTotalBytes.dashSaturatingSubtract(existing.size)
        }
        try trimSegmentCache(reserving: size, addingEntry: addsEntry, protecting: key)
        try data.write(to: url, options: .atomic)
        cachedSegmentFiles[key] = VesperDashCachedSegmentFile(
            url: url,
            size: size,
            segment: key.segment,
            lastAccessedAt: Date()
        )
        segmentCacheTotalBytes = segmentCacheTotalBytes.dashSaturatingAdd(size)
        try trimSegmentCache(reserving: 0, addingEntry: false, protecting: key)
        return true
    }

    func touchCachedSegmentFile(
        key: VesperDashSegmentCacheKey,
        url: URL,
        size: UInt64
    ) {
        if let existing = cachedSegmentFiles[key] {
            segmentCacheTotalBytes = segmentCacheTotalBytes
                .dashSaturatingSubtract(existing.size)
                .dashSaturatingAdd(size)
            cachedSegmentFiles[key] = VesperDashCachedSegmentFile(
                url: url,
                size: size,
                segment: key.segment,
                lastAccessedAt: Date()
            )
            return
        }
        cachedSegmentFiles[key] = VesperDashCachedSegmentFile(
            url: url,
            size: size,
            segment: key.segment,
            lastAccessedAt: Date()
        )
        segmentCacheTotalBytes = segmentCacheTotalBytes.dashSaturatingAdd(size)
    }

    func fileSize(at url: URL) -> UInt64? {
        guard
            let attributes = try? FileManager.default.attributesOfItem(atPath: url.path),
            let value = attributes[.size] as? NSNumber
        else {
            return nil
        }
        return value.uint64Value
    }

    func trimSegmentCache(
        reserving additionalBytes: UInt64,
        addingEntry: Bool,
        protecting protectedKey: VesperDashSegmentCacheKey
    ) throws {
        var projectedBytes = segmentCacheTotalBytes.dashSaturatingAdd(additionalBytes)
        while
            cachedSegmentFiles.count + (addingEntry ? 1 : 0) > Self.segmentCacheMaxEntryCount ||
            projectedBytes > Self.segmentCacheMaxBytes
        {
            guard let eviction = nextSegmentCacheEviction(protecting: protectedKey) else {
                return
            }
            cachedSegmentFiles[eviction.key] = nil
            segmentCacheTotalBytes = segmentCacheTotalBytes.dashSaturatingSubtract(eviction.file.size)
            projectedBytes = projectedBytes.dashSaturatingSubtract(eviction.file.size)
            removeFileIfPresent(eviction.file.url, context: "evicted DASH segment cache file")
#if DEBUG
            iosHostLog(
                "dashSegmentCache evict rendition=\(eviction.key.renditionId) segment=\(eviction.key.segment) bytes=\(eviction.file.size)"
            )
#endif
        }
    }

    func nextSegmentCacheEviction(
        protecting protectedKey: VesperDashSegmentCacheKey
    ) -> (key: VesperDashSegmentCacheKey, file: VesperDashCachedSegmentFile)? {
        let candidate = cachedSegmentFiles
            .filter { key, _ in key != protectedKey }
            .min { lhs, rhs in
                let lhsInit = lhs.value.isInitialization
                let rhsInit = rhs.value.isInitialization
                if lhsInit != rhsInit {
                    return !lhsInit
                }
                return lhs.value.lastAccessedAt < rhs.value.lastAccessedAt
            }
        return candidate.map { (key: $0.key, file: $0.value) }
    }

    func segmentRedirectURL(renditionId: String, segment: VesperDashSegmentRequest) async throws -> URL {
        let key = VesperDashSegmentCacheKey(renditionId: renditionId, segment: segment)
        let url = segmentCacheURL(renditionId: renditionId, segment: segment)
        if cachedSegmentFileExists(for: key, at: url) {
            return url
        }

        let manifest = try await loadManifest()
        let playable = try await playableRepresentation(renditionId: renditionId)
        if let segmentTemplate = playable.representation.segmentTemplate {
            let payloadResult = try await coalescedSegmentTemplatePayload(
                manifest: manifest,
                playable: playable,
                segmentTemplate: segmentTemplate,
                segment: segment,
                cacheURL: url,
                key: key,
                allowSkippingLargeMediaEntry: false,
                contentType: dashSegmentContentType(for: playable, segment: segment)
            )
            guard case let .file(fileURL, 0, _, false, _) = payloadResult.payload else {
                throw VesperDashBridgeError.network("DASH segment redirect requires a persistent local file")
            }
            return fileURL
        }

        let data = try await segmentData(renditionId: renditionId, segment: segment)
        _ = try writeSegmentTemplateCache(
            data,
            to: url,
            key: key,
            allowSkippingLargeMediaEntry: false
        )
        return url
    }

    func segmentCacheURL(renditionId: String, segment: VesperDashSegmentRequest) -> URL {
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
}
