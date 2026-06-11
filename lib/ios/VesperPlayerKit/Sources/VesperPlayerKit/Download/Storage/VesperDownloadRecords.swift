import Foundation

extension VesperDownloadResourceRecord {
    func compactedForPersistence() -> VesperDownloadResourceRecord {
        VesperDownloadResourceRecord(
            resourceId: resourceId,
            uri: uri,
            relativePath: relativePath,
            byteRange: byteRange,
            generatedText: nil,
            sizeBytes: sizeBytes,
            etag: etag,
            checksum: checksum
        )
    }

    func withSizeBytes(_ sizeBytes: UInt64) -> VesperDownloadResourceRecord {
        VesperDownloadResourceRecord(
            resourceId: resourceId,
            uri: uri,
            relativePath: relativePath,
            byteRange: byteRange,
            generatedText: generatedText,
            sizeBytes: sizeBytes,
            etag: etag,
            checksum: checksum
        )
    }

    func withMaterializedGeneratedText(uri: String, sizeBytes: UInt64) -> VesperDownloadResourceRecord {
        VesperDownloadResourceRecord(
            resourceId: resourceId,
            uri: uri,
            relativePath: relativePath,
            byteRange: byteRange,
            generatedText: nil,
            sizeBytes: sizeBytes,
            etag: etag,
            checksum: checksum
        )
    }

    func withGeneratedText(_ generatedText: String) -> VesperDownloadResourceRecord {
        VesperDownloadResourceRecord(
            resourceId: resourceId,
            uri: uri,
            relativePath: relativePath,
            byteRange: byteRange,
            generatedText: generatedText,
            sizeBytes: sizeBytes,
            etag: etag,
            checksum: checksum
        )
    }
}

extension VesperDownloadSegmentRecord {
    func withSizeBytes(_ sizeBytes: UInt64) -> VesperDownloadSegmentRecord {
        VesperDownloadSegmentRecord(
            segmentId: segmentId,
            uri: uri,
            relativePath: relativePath,
            sequence: sequence,
            byteRange: byteRange,
            sizeBytes: sizeBytes,
            checksum: checksum
        )
    }
}

extension VesperDownloadAssetIndex {
    func compactedForPersistence() -> VesperDownloadAssetIndex {
        VesperDownloadAssetIndex(
            contentFormat: contentFormat,
            version: version,
            etag: etag,
            checksum: checksum,
            totalSizeBytes: totalSizeBytes,
            resources: resources.map { $0.compactedForPersistence() },
            segments: segments,
            completedPath: completedPath
        )
    }

    func withResources(_ resources: [VesperDownloadResourceRecord]) -> VesperDownloadAssetIndex {
        VesperDownloadAssetIndex(
            contentFormat: contentFormat,
            version: version,
            etag: etag,
            checksum: checksum,
            totalSizeBytes: totalSizeBytes,
            resources: resources,
            segments: segments,
            completedPath: completedPath
        )
    }

    func withCompletedPath(_ completedPath: String?) -> VesperDownloadAssetIndex {
        VesperDownloadAssetIndex(
            contentFormat: contentFormat,
            version: version,
            etag: etag,
            checksum: checksum,
            totalSizeBytes: totalSizeBytes,
            resources: resources,
            segments: segments,
            completedPath: completedPath
        )
    }
}

extension VesperDownloadTaskSnapshot {
    func withAssetIndex(_ assetIndex: VesperDownloadAssetIndex) -> VesperDownloadTaskSnapshot {
        VesperDownloadTaskSnapshot(
            taskId: taskId,
            assetId: assetId,
            source: source,
            profile: profile,
            state: state,
            progress: progress,
            assetIndex: assetIndex,
            error: error
        )
    }

    func withStatePatch(_ patch: VesperDownloadTaskStatePatch) -> VesperDownloadTaskSnapshot {
        VesperDownloadTaskSnapshot(
            taskId: taskId,
            assetId: assetId,
            source: source,
            profile: profile,
            state: patch.state,
            progress: patch.progress,
            assetIndex: assetIndex.withCompletedPath(patch.completedPath ?? assetIndex.completedPath),
            error: patch.error
        )
    }

    func withProgress(_ progress: VesperDownloadProgressSnapshot) -> VesperDownloadTaskSnapshot {
        VesperDownloadTaskSnapshot(
            taskId: taskId,
            assetId: assetId,
            source: source,
            profile: profile,
            state: state,
            progress: progress,
            assetIndex: assetIndex,
            error: error
        )
    }
}

extension VesperDownloadSnapshot {
    func compactedForPersistence() -> VesperDownloadSnapshot {
        VesperDownloadSnapshot(
            tasks: tasks.map { $0.withAssetIndex($0.assetIndex.compactedForPersistence()) }
        )
    }
}
