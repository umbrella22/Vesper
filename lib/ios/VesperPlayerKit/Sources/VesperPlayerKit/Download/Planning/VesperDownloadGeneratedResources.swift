import Foundation

struct VesperGeneratedDownloadResourceMaterializer {
    let fileManager: FileManager
    let baseDirectory: URL?

    init(fileManager: FileManager = .default, baseDirectory: URL?) {
        self.fileManager = fileManager
        self.baseDirectory = baseDirectory
    }

    func materialize(
        assetId: VesperDownloadAssetId,
        taskId: VesperDownloadTaskId?,
        profile: VesperDownloadProfile,
        assetIndex: VesperDownloadAssetIndex
    ) throws -> VesperDownloadAssetIndex {
        guard assetIndex.resources.contains(where: { $0.generatedText != nil }) else {
            return assetIndex.compactedForPersistence()
        }

        let taskDirectory = taskBaseDirectory(assetId: assetId, taskId: taskId, profile: profile)
        let generatedDirectory = taskDirectory.appendingPathComponent(".generated", isDirectory: true)
        try fileManager.createDirectory(at: generatedDirectory, withIntermediateDirectories: true)
        excludeDownloadItemFromBackup(taskDirectory, fileManager: fileManager)
        excludeDownloadItemFromBackup(generatedDirectory, fileManager: fileManager)

        var usedNames = Set<String>()
        let resources = try assetIndex.resources.map { resource in
            guard let generatedText = resource.generatedText else {
                return resource
            }
            let data = Data(generatedText.utf8)
            let fileName = uniqueGeneratedFileName(for: resource, usedNames: &usedNames)
            let destinationURL = generatedDirectory.appendingPathComponent(fileName, isDirectory: false)
            do {
                try data.write(to: destinationURL, options: .atomic)
                excludeDownloadItemFromBackup(destinationURL, fileManager: fileManager)
            } catch {
                throw VesperForegroundDownloadPreparationError.invalidSource(
                    "failed to persist generated download resource \(resource.resourceId): \(error.localizedDescription)"
                )
            }
            return resource.withMaterializedGeneratedText(
                uri: destinationURL.absoluteString,
                sizeBytes: UInt64(data.count)
            )
        }

        return VesperDownloadAssetIndex(
            contentFormat: assetIndex.contentFormat,
            version: assetIndex.version,
            etag: assetIndex.etag,
            checksum: assetIndex.checksum,
            totalSizeBytes: recomputedTotalSizeBytes(
                original: assetIndex.totalSizeBytes,
                resources: resources,
                segments: assetIndex.segments
            ),
            resources: resources,
            segments: assetIndex.segments,
            completedPath: assetIndex.completedPath
        )
    }

    private func taskBaseDirectory(
        assetId: VesperDownloadAssetId,
        taskId: VesperDownloadTaskId?,
        profile: VesperDownloadProfile
    ) -> URL {
        if let targetDirectory = profile.targetDirectory {
            return targetDirectory
        }
        let root = baseDirectory
            ?? fileManager.urls(for: .documentDirectory, in: .userDomainMask).first!
                .appendingPathComponent("vesper-downloads", isDirectory: true)
        let assetComponent = assetId.isEmpty ? taskId.map(String.init) ?? "asset" : assetId
        return root.appendingPathComponent(assetComponent, isDirectory: true)
    }

    private func uniqueGeneratedFileName(
        for resource: VesperDownloadResourceRecord,
        usedNames: inout Set<String>
    ) -> String {
        let baseName = generatedBaseName(for: resource)
        if usedNames.insert(baseName).inserted {
            return baseName
        }
        let hashed = appendStableHash(
            to: baseName,
            hash: stableShortHash("\(resource.resourceId)|\(resource.relativePath ?? "")|\(resource.uri)")
        )
        _ = usedNames.insert(hashed)
        return hashed
    }

    private func generatedBaseName(for resource: VesperDownloadResourceRecord) -> String {
        let raw = resource.relativePath.flatMap(lastPathComponent) ?? resource.resourceId
        let sanitized = raw
            .replacingOccurrences(of: "[^A-Za-z0-9._-]+", with: "_", options: .regularExpression)
            .trimmingCharacters(in: CharacterSet(charactersIn: ". "))
        if sanitized.isEmpty || sanitized == ".." {
            return "resource-\(stableShortHash(resource.resourceId.isEmpty ? resource.uri : resource.resourceId))"
        }
        return sanitized
    }

    private func lastPathComponent(_ value: String) -> String? {
        value.split(whereSeparator: { $0 == "/" || $0 == "\\" }).last.map(String.init)
    }

    private func appendStableHash(to fileName: String, hash: String) -> String {
        let nsName = fileName as NSString
        let ext = nsName.pathExtension
        let stem = nsName.deletingPathExtension
        return ext.isEmpty ? "\(stem)-\(hash)" : "\(stem)-\(hash).\(ext)"
    }

    private func stableShortHash(_ value: String) -> String {
        var hash: UInt64 = 0xcbf29ce484222325
        for byte in value.utf8 {
            hash ^= UInt64(byte)
            hash &*= 0x100000001b3
        }
        let text = String(hash, radix: 16)
        return String(text.suffix(8))
    }

    private func recomputedTotalSizeBytes(
        original: UInt64?,
        resources: [VesperDownloadResourceRecord],
        segments: [VesperDownloadSegmentRecord]
    ) -> UInt64? {
        var total: UInt64 = 0
        for resource in resources {
            guard let sizeBytes = resource.sizeBytes else {
                return original
            }
            total += sizeBytes
        }
        for segment in segments {
            guard let sizeBytes = segment.sizeBytes else {
                return original
            }
            total += sizeBytes
        }
        return total
    }
}
