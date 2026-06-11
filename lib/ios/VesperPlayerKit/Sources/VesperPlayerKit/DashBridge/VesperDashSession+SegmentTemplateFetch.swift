@preconcurrency import AVFoundation
import Foundation
import VesperPlayerKitBridgeShim

extension VesperDashSession {
    func fetchSegmentTemplatePayload(
        manifest: VesperDashManifest,
        playable: VesperDashPlayableRepresentation,
        segmentTemplate: VesperDashSegmentTemplate,
        segment: VesperDashSegmentRequest,
        cacheURL: URL,
        key: VesperDashSegmentCacheKey,
        allowSkippingLargeMediaEntry: Bool,
        contentType: String
    ) async throws -> VesperDashSegmentPayloadResult {
        if case .media = segment {
            let payload = try await fetchSegmentTemplateFile(
                manifest: manifest,
                playable: playable,
                segmentTemplate: segmentTemplate,
                segment: segment,
                cacheURL: cacheURL,
                key: key,
                allowSkippingLargeMediaEntry: allowSkippingLargeMediaEntry,
                contentType: contentType
            )
            return VesperDashSegmentPayloadResult(
                payload: payload,
                cacheHit: false,
                segmentType: "template",
                byteRange: nil,
                delivery: payload.isTemporaryFile ? "temporaryFile" : "cacheFile"
            )
        }

        let data = try await fetchSegmentTemplateData(
            manifest: manifest,
            playable: playable,
            segmentTemplate: segmentTemplate,
            segment: segment
        )
        try Task.checkCancellation()
        if try writeSegmentTemplateCache(
            data,
            to: cacheURL,
            key: key,
            allowSkippingLargeMediaEntry: allowSkippingLargeMediaEntry
        ) {
            let payload = cachedSegmentFilePayload(for: key, at: cacheURL, contentType: contentType)
                ?? .data(data, contentType: contentType)
            return VesperDashSegmentPayloadResult(
                payload: payload,
                cacheHit: false,
                segmentType: "template",
                byteRange: nil,
                delivery: "cacheFile"
            )
        }
        return VesperDashSegmentPayloadResult(
            payload: .data(data, contentType: contentType),
            cacheHit: false,
            segmentType: "template",
            byteRange: nil,
            delivery: "networkData"
        )
    }
    func fetchSegmentTemplateFile(
        manifest: VesperDashManifest,
        playable: VesperDashPlayableRepresentation,
        segmentTemplate: VesperDashSegmentTemplate,
        segment: VesperDashSegmentRequest,
        cacheURL: URL,
        key: VesperDashSegmentCacheKey,
        allowSkippingLargeMediaEntry: Bool,
        contentType: String
    ) async throws -> VesperDashSegmentPayload {
        let url = try templateSegmentURL(
            manifest: manifest,
            playable: playable,
            segmentTemplate: segmentTemplate,
            segment: segment
        )
        let temporaryURL = temporarySegmentDownloadURL(renditionId: playable.renditionId, segment: segment)
        let size = try await networkClient.download(for: url, to: temporaryURL)
#if DEBUG
        logTopLevelBoxes(
            fileURL: temporaryURL,
            totalBytes: size,
            label: "dashSegmentTemplate",
            renditionId: playable.renditionId,
            segment: segment
        )
#endif
        return try materializeSegmentTemplateFile(
            from: temporaryURL,
            to: cacheURL,
            size: size,
            key: key,
            allowSkippingLargeMediaEntry: allowSkippingLargeMediaEntry,
            contentType: contentType
        )
    }

    func materializeSegmentTemplateFile(
        from temporaryURL: URL,
        to cacheURL: URL,
        size: UInt64,
        key: VesperDashSegmentCacheKey,
        allowSkippingLargeMediaEntry: Bool,
        contentType: String
    ) throws -> VesperDashSegmentPayload {
        if allowSkippingLargeMediaEntry,
           case .media = key.segment,
           size > Self.segmentCacheMaxSingleMediaBytes {
#if DEBUG
            iosHostLog(
                "dashSegmentCache streamLarge rendition=\(key.renditionId) segment=\(key.segment) bytes=\(size)"
            )
#endif
            return .file(
                url: temporaryURL,
                offset: 0,
                size: size,
                removeAfterServing: true,
                contentType: contentType
            )
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
        removeFileIfPresent(cacheURL, context: "existing DASH segment cache file")
        try FileManager.default.moveItem(at: temporaryURL, to: cacheURL)
        cachedSegmentFiles[key] = VesperDashCachedSegmentFile(
            url: cacheURL,
            size: size,
            segment: key.segment,
            lastAccessedAt: Date()
        )
        segmentCacheTotalBytes = segmentCacheTotalBytes.dashSaturatingAdd(size)
        try trimSegmentCache(reserving: 0, addingEntry: false, protecting: key)
        return .file(
            url: cacheURL,
            offset: 0,
            size: size,
            removeAfterServing: false,
            contentType: contentType
        )
    }

    func temporarySegmentDownloadURL(
        renditionId: String,
        segment: VesperDashSegmentRequest
    ) -> URL {
        let encodedId = renditionId.addingPercentEncoding(withAllowedCharacters: dashPathComponentAllowedCharacters)
            ?? renditionId
        let segmentName: String
        switch segment {
        case .initialization:
            segmentName = "init"
        case let .media(index):
            segmentName = "\(index)"
        }
        return segmentCacheDirectory
            .appendingPathComponent("tmp-\(encodedId)-\(segmentName)-\(UUID().uuidString)", isDirectory: false)
    }
}
