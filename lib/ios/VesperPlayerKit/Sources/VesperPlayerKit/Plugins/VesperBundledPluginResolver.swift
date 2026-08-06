import Foundation

enum VesperBundledPluginResolutionError: LocalizedError, Equatable {
    case unsupportedTransport(String)
    case missingArtifact(String)
    case tooManyReferences(Int)

    var errorDescription: String? {
        switch self {
        case let .unsupportedTransport(transport):
            "iOS build-time plugins do not support transport `\(transport)`."
        case let .missingArtifact(pluginId):
            "No embedded iOS plugin artifact was found for `\(pluginId)`."
        case let .tooManyReferences(count):
            "iOS plugin reference count \(count) exceeds the limit of 256."
        }
    }
}

/// Internal artifact locators resolved from one explicit public selection.
struct VesperResolvedPluginArtifacts: Equatable {
    struct Artifact: Equatable {
        let reference: VesperPluginReference
        let libraryPath: String
    }

    let artifacts: [Artifact]

    init(artifacts: [Artifact]) {
        self.artifacts = artifacts
    }

    var libraryPaths: [String] {
        var seen = Set<String>()
        return artifacts.compactMap { artifact in
            seen.insert(artifact.libraryPath).inserted ? artifact.libraryPath : nil
        }
    }

    func references(forLibraryPath path: String) -> [VesperPluginReference] {
        artifacts.compactMap { artifact in
            artifact.libraryPath == path ? artifact.reference : nil
        }
    }
}

enum VesperBundledPluginResolver {
    private static let knownFrameworkNames: [String: String] = [
        "io.github.ikaros.vesper.source-normalizer-ffmpeg":
            "VesperPlayerSourceNormalizerFfmpegPlugin",
        "io.github.ikaros.vesper.remux-ffmpeg": "VesperPlayerRemuxFfmpegPlugin",
        "io.github.ikaros.vesper.decoder-videotoolbox":
            "VesperPlayerDecoderVideoToolboxPlugin",
        "dev.vesper.frame-processor-diagnostic":
            "VesperPlayerFrameProcessorDiagnosticPlugin",
    ]

    static func resolvePluginArtifacts(
        _ references: [VesperPluginReference],
        frameworkSearchURLs: [URL] = defaultFrameworkSearchURLs(),
        fileManager: FileManager = .default
    ) throws -> VesperResolvedPluginArtifacts {
        var uniqueReferences: [VesperPluginReference] = []
        uniqueReferences.reserveCapacity(references.count)
        for reference in references where !uniqueReferences.contains(reference) {
            uniqueReferences.append(reference)
        }
        guard uniqueReferences.count <= 256 else {
            throw VesperBundledPluginResolutionError.tooManyReferences(uniqueReferences.count)
        }

        var resolvedArtifacts: [VesperResolvedPluginArtifacts.Artifact] = []
        for reference in uniqueReferences {
            let resolvedPath: String
            switch reference.transport {
            case .native:
                guard
                    let frameworkName = knownFrameworkNames[reference.pluginId],
                    let path = findPluginBinary(
                        frameworkName: frameworkName,
                        searchURLs: frameworkSearchURLs,
                        fileManager: fileManager
                    )
                else {
                    throw VesperBundledPluginResolutionError.missingArtifact(reference.pluginId)
                }
                resolvedPath = path
            case .wasm:
                throw VesperBundledPluginResolutionError.unsupportedTransport("wasm")
            case let .unknown(rawValue):
                throw VesperBundledPluginResolutionError.unsupportedTransport(rawValue)
            }
            resolvedArtifacts.append(
                VesperResolvedPluginArtifacts.Artifact(
                    reference: reference,
                    libraryPath: resolvedPath
                )
            )
        }
        return VesperResolvedPluginArtifacts(artifacts: resolvedArtifacts)
    }

    static func isRegisteredNativeReference(
        _ reference: VesperPluginReference,
        pluginId: String
    ) -> Bool {
        reference.transport == .native &&
            reference.pluginId == pluginId &&
            knownFrameworkNames[reference.pluginId] != nil
    }

    static func findPluginBinary(
        frameworkName: String,
        searchURLs: [URL],
        fileManager: FileManager = .default
    ) -> String? {
        return findFrameworkBinary(
            frameworkName: frameworkName,
            searchURLs: searchURLs,
            fileManager: fileManager
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
        // Optional plugin frameworks are build-time embedded under the host
        // application's Frameworks directory. Avoid `Bundle.allFrameworks`
        // here: it performs a process-wide bundle scan and can block when
        // this resolver runs on the bounded utility queue during startup.

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
