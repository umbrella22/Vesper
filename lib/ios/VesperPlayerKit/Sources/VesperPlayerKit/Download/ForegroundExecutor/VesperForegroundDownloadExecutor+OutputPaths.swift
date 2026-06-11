import Foundation

extension VesperForegroundDownloadExecutor {
    func resolveURL(_ value: String) throws -> URL {
        if let url = URL(string: value) {
            try rejectInsecureHTTPURL(url)
            return url
        }
        throw CocoaError(.fileReadInvalidFileName)
    }

    func outputURL(
        for task: VesperDownloadTaskSnapshot,
        entry: ForegroundDownloadEntry,
        index: Int
    ) throws -> URL {
        let baseDirectory = defaultBaseDirectory(for: task)
        if let relativePath = entry.relativePath, !relativePath.isEmpty {
            if relativePath.hasPrefix("/") {
                return URL(fileURLWithPath: relativePath)
            }
            let components = relativePath.split(separator: "/", omittingEmptySubsequences: false)
            if components.contains(where: { $0 == ".." }) {
                throw VesperForegroundDownloadPreparationError.invalidSource(
                    "download output path escapes the task directory: \(relativePath)"
                )
            }
            let candidate = baseDirectory.appendingPathComponent(relativePath).standardizedFileURL
            let standardizedBase = baseDirectory.standardizedFileURL
            guard candidate.path == standardizedBase.path || candidate.path.hasPrefix(standardizedBase.path + "/") else {
                throw VesperForegroundDownloadPreparationError.invalidSource(
                    "download output path escapes the task directory: \(relativePath)"
                )
            }
            return candidate
        }

        let filename =
            entry.url.lastPathComponent.isEmpty
            ? "\(entry.fallbackName)-\(index + 1).bin"
            : entry.url.lastPathComponent
        return baseDirectory.appendingPathComponent(filename)
    }

    func completedPath(
        for task: VesperDownloadTaskSnapshot,
        plan: [ForegroundDownloadEntry]
    ) -> String {
        guard plan.count == 1, let first = try? outputURL(for: task, entry: plan[0], index: 0) else {
            return defaultBaseDirectory(for: task).path
        }
        return first.path
    }

    func defaultBaseDirectory(for task: VesperDownloadTaskSnapshot) -> URL {
        if let targetDirectory = task.profile.targetDirectory {
            return targetDirectory
        }
        return defaultAssetDirectory(for: task)
    }

    func defaultAssetDirectory(for task: VesperDownloadTaskSnapshot) -> URL {
        let root = baseDirectory
            ?? fileManager.urls(for: .documentDirectory, in: .userDomainMask).first!
                .appendingPathComponent("vesper-downloads", isDirectory: true)
        return root.appendingPathComponent(task.assetId.isEmpty ? String(task.taskId) : task.assetId)
    }
}
