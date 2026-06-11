import Foundation

enum VesperBundledPluginResolver {
    private static let sourceNormalizerFrameworkName = "VesperPlayerSourceNormalizerFfmpegPlugin"

    static func resolveSourceNormalizerConfiguration(
        _ configuration: VesperSourceNormalizerConfiguration
    ) -> VesperSourceNormalizerConfiguration {
        resolveSourceNormalizerConfiguration(
            configuration,
            frameworkSearchURLs: defaultFrameworkSearchURLs()
        )
    }

    static func resolveSourceNormalizerConfiguration(
        _ configuration: VesperSourceNormalizerConfiguration,
        frameworkSearchURLs: [URL],
        fileManager: FileManager = .default
    ) -> VesperSourceNormalizerConfiguration {
        if configuration.mode == .disabled || !configuration.pluginLibraryPaths.isEmpty {
            return configuration
        }

        guard
            let pluginPath = findFrameworkBinary(
                frameworkName: sourceNormalizerFrameworkName,
                searchURLs: frameworkSearchURLs,
                fileManager: fileManager
            )
        else {
            return configuration
        }

        return VesperSourceNormalizerConfiguration(
            mode: configuration.mode,
            pluginLibraryPaths: [pluginPath],
            runtimeProfile: configuration.runtimeProfile
        )
    }

    static func findFrameworkBinary(
        frameworkName: String,
        searchURLs: [URL],
        fileManager: FileManager = .default
    ) -> String? {
        for searchURL in searchURLs {
            let frameworkURL =
                searchURL
                .appendingPathComponent("\(frameworkName).framework", isDirectory: true)
                .standardizedFileURL
            let binaryURL =
                frameworkURL
                .appendingPathComponent(frameworkName, isDirectory: false)
                .standardizedFileURL
            if fileManager.isExecutableFile(atPath: binaryURL.path) ||
                fileManager.fileExists(atPath: binaryURL.path)
            {
                return binaryURL.path
            }
        }
        return nil
    }

    private static func defaultFrameworkSearchURLs() -> [URL] {
        var urls: [URL] = []
        if let privateFrameworksURL = Bundle.main.privateFrameworksURL {
            urls.append(privateFrameworksURL)
        }
        if let builtInPlugInsURL = Bundle.main.builtInPlugInsURL {
            urls.append(builtInPlugInsURL)
        }
        urls.append(Bundle.main.bundleURL)
        urls.append(contentsOf: Bundle.allFrameworks.map(\.bundleURL).map { $0.deletingLastPathComponent() })

        var seen = Set<String>()
        return urls.compactMap { url in
            let path = url.standardizedFileURL.path
            guard seen.insert(path).inserted else {
                return nil
            }
            return URL(fileURLWithPath: path, isDirectory: true)
        }
    }
}
