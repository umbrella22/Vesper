import Foundation
@_implementationOnly import VesperPlayerKitBridgeShim

private let vesperPluginRegistryFileName = "vesper-plugin-registry.json"
private let maxVesperPluginRegistryFragments = 256
private let maxVesperPluginRegistryFragmentBytes = 1024 * 1024
private let maxVesperPluginRegistrySetBytes = 4 * 1024 * 1024

enum VesperEmbeddedPluginRegistryError: LocalizedError {
    case missingFrameworksDirectory
    case invalidFrameworksDirectory(String)
    case tooManyFragments(Int)
    case invalidFragment(String)
    case oversizedFragment(String)
    case oversizedFragmentSet
    case invalidResolutionPayload(String)
    case missingFramework(String)
    case invalidFramework(String)
    case bridge(String)

    var errorDescription: String? {
        switch self {
        case .missingFrameworksDirectory:
            "The app has no private Frameworks directory for the selected Vesper plugins."
        case let .invalidFrameworksDirectory(message):
            "The app Frameworks directory is invalid: \(message)"
        case let .tooManyFragments(count):
            "The app contains \(count) Vesper plugin registry fragments; the maximum is \(maxVesperPluginRegistryFragments)."
        case let .invalidFragment(message):
            "A Vesper plugin registry fragment is invalid: \(message)"
        case let .oversizedFragment(path):
            "The Vesper plugin registry fragment exceeds \(maxVesperPluginRegistryFragmentBytes) bytes: \(path)"
        case .oversizedFragmentSet:
            "The Vesper plugin registry fragments exceed \(maxVesperPluginRegistrySetBytes) bytes."
        case let .invalidResolutionPayload(message):
            "The Vesper plugin resolution payload is invalid: \(message)"
        case let .missingFramework(pluginId):
            "No embedded iOS plugin framework was found for `\(pluginId)`."
        case let .invalidFramework(message):
            "An embedded iOS plugin framework is invalid: \(message)"
        case let .bridge(message):
            message
        }
    }
}

private struct VesperIosPluginResolution: Decodable {
    let pluginId: String
    let frameworkName: String
    let bundleIdentifier: String
    let validation: String
}

private struct VesperIosResolvedFrameworkSet: Encodable {
    let frameworksRoot: String
    let frameworks: [VesperIosResolvedFramework]
}

private struct VesperIosResolvedFramework: Encodable {
    let pluginId: String
    let frameworkName: String
    let bundleIdentifier: String
    let frameworkPath: String
    let binaryPath: String
}

/// Owns one generation-safe Rust registry for explicitly selected iOS plugins.
final class VesperEmbeddedPluginRegistry {
    private(set) var handle: UInt64

    private init(handle: UInt64) {
        self.handle = handle
    }

    deinit {
        close()
    }

    func close() {
        guard handle != 0 else { return }
        vesper_runtime_ios_plugin_registry_dispose(handle)
        handle = 0
    }

    static func create(
        references: [VesperPluginReference],
        frameworksURL: URL? = Bundle.main.privateFrameworksURL,
        fileManager: FileManager = .default
    ) throws -> VesperEmbeddedPluginRegistry {
        let fragmentStrings: [String]
        if references.isEmpty {
            fragmentStrings = []
        } else {
            guard let frameworksURL else {
                throw VesperEmbeddedPluginRegistryError.missingFrameworksDirectory
            }
            fragmentStrings = try loadVesperIosPluginRegistryFragments(
                frameworksURL: frameworksURL,
                fileManager: fileManager
            )
        }

        let fragmentSetData = try JSONEncoder().encode(fragmentStrings)
        let referencesData = Data(try encodeVesperPluginReferencesJSON(references).utf8)
        var planHandle: UInt64 = 0
        var planErrorMessage: UnsafeMutablePointer<CChar>?
        let planned = fragmentSetData.withUnsafeBytes { fragmentBuffer in
            referencesData.withUnsafeBytes { referenceBuffer in
                withUnsafeMutablePointer(to: &planHandle) { handlePointer in
                    withUnsafeMutablePointer(to: &planErrorMessage) { errorPointer in
                        vesper_runtime_ios_plugin_plan_create(
                            fragmentBuffer.baseAddress?.assumingMemoryBound(to: UInt8.self),
                            UInt(fragmentBuffer.count),
                            referenceBuffer.baseAddress?.assumingMemoryBound(to: UInt8.self),
                            UInt(referenceBuffer.count),
                            handlePointer,
                            errorPointer
                        )
                    }
                }
            }
        }
        defer { freeVesperIosPluginString(planErrorMessage) }
        guard planned, planHandle != 0 else {
            throw VesperEmbeddedPluginRegistryError.bridge(
                stringFromVesperIosPluginString(planErrorMessage)
                    ?? "iOS plugin plan creation failed"
            )
        }
        defer { vesper_runtime_ios_plugin_plan_dispose(planHandle) }

        var resolutionsPointer: UnsafeMutablePointer<CChar>?
        var resolutionErrorMessage: UnsafeMutablePointer<CChar>?
        let resolved = withUnsafeMutablePointer(to: &resolutionsPointer) { jsonPointer in
            withUnsafeMutablePointer(to: &resolutionErrorMessage) { errorPointer in
                vesper_runtime_ios_plugin_plan_resolutions_json(
                    planHandle,
                    jsonPointer,
                    errorPointer
                )
            }
        }
        defer {
            freeVesperIosPluginString(resolutionsPointer)
            freeVesperIosPluginString(resolutionErrorMessage)
        }
        guard resolved, let resolutionsPointer else {
            throw VesperEmbeddedPluginRegistryError.bridge(
                stringFromVesperIosPluginString(resolutionErrorMessage)
                    ?? "iOS plugin resolution failed"
            )
        }
        let resolutionsData = Data(String(cString: resolutionsPointer).utf8)
        let resolutions: [VesperIosPluginResolution]
        do {
            resolutions = try JSONDecoder().decode(
                [VesperIosPluginResolution].self,
                from: resolutionsData
            )
        } catch {
            throw VesperEmbeddedPluginRegistryError.invalidResolutionPayload(
                error.localizedDescription
            )
        }
        let resolvedFrameworks = try resolveVesperIosPluginFrameworks(
            resolutions,
            frameworksURL: frameworksURL
        )
        let resolvedSet = VesperIosResolvedFrameworkSet(
            frameworksRoot: references.isEmpty ? "" : frameworksURL?.path ?? "",
            frameworks: resolvedFrameworks
        )
        let resolvedSetData = try JSONEncoder().encode(resolvedSet)

        var registryHandle: UInt64 = 0
        var registryErrorMessage: UnsafeMutablePointer<CChar>?
        let loaded = resolvedSetData.withUnsafeBytes { buffer in
            withUnsafeMutablePointer(to: &registryHandle) { handlePointer in
                withUnsafeMutablePointer(to: &registryErrorMessage) { errorPointer in
                    vesper_runtime_ios_plugin_registry_load(
                        planHandle,
                        buffer.baseAddress?.assumingMemoryBound(to: UInt8.self),
                        UInt(buffer.count),
                        handlePointer,
                        errorPointer
                    )
                }
            }
        }
        defer { freeVesperIosPluginString(registryErrorMessage) }
        guard loaded, registryHandle != 0 else {
            throw VesperEmbeddedPluginRegistryError.bridge(
                stringFromVesperIosPluginString(registryErrorMessage)
                    ?? "iOS plugin registry loading failed"
            )
        }
        return VesperEmbeddedPluginRegistry(handle: registryHandle)
    }
}

func loadVesperIosPluginRegistryFragments(
    frameworksURL: URL,
    fileManager: FileManager
) throws -> [String] {
    let rootValues = try frameworksURL.resourceValues(forKeys: [
        .isDirectoryKey,
        .isSymbolicLinkKey,
    ])
    guard rootValues.isDirectory == true, rootValues.isSymbolicLink != true else {
        throw VesperEmbeddedPluginRegistryError.invalidFrameworksDirectory(frameworksURL.path)
    }
    let frameworkURLs = try fileManager.contentsOfDirectory(
        at: frameworksURL,
        includingPropertiesForKeys: [.isDirectoryKey, .isSymbolicLinkKey],
        options: [.skipsHiddenFiles]
    )
    .filter { $0.pathExtension == "framework" }
    .sorted { $0.lastPathComponent < $1.lastPathComponent }

    var fragments: [String] = []
    var totalBytes = 0
    for frameworkURL in frameworkURLs {
        let frameworkValues = try frameworkURL.resourceValues(forKeys: [
            .isDirectoryKey,
            .isSymbolicLinkKey,
        ])
        guard frameworkValues.isDirectory == true, frameworkValues.isSymbolicLink != true else {
            throw VesperEmbeddedPluginRegistryError.invalidFramework(frameworkURL.path)
        }
        let fragmentURL = frameworkURL.appendingPathComponent(
            vesperPluginRegistryFileName,
            isDirectory: false
        )
        guard fileManager.fileExists(atPath: fragmentURL.path) else { continue }
        guard fragments.count < maxVesperPluginRegistryFragments else {
            throw VesperEmbeddedPluginRegistryError.tooManyFragments(fragments.count + 1)
        }
        let values = try fragmentURL.resourceValues(forKeys: [
            .fileSizeKey,
            .isRegularFileKey,
            .isSymbolicLinkKey,
        ])
        guard values.isRegularFile == true, values.isSymbolicLink != true else {
            throw VesperEmbeddedPluginRegistryError.invalidFragment(fragmentURL.path)
        }
        guard (values.fileSize ?? maxVesperPluginRegistryFragmentBytes + 1)
            <= maxVesperPluginRegistryFragmentBytes
        else {
            throw VesperEmbeddedPluginRegistryError.oversizedFragment(fragmentURL.path)
        }
        let handle = try FileHandle(forReadingFrom: fragmentURL)
        defer { try? handle.close() }
        let data = try handle.read(upToCount: maxVesperPluginRegistryFragmentBytes + 1) ?? Data()
        guard data.count <= maxVesperPluginRegistryFragmentBytes else {
            throw VesperEmbeddedPluginRegistryError.oversizedFragment(fragmentURL.path)
        }
        totalBytes += data.count
        guard totalBytes <= maxVesperPluginRegistrySetBytes else {
            throw VesperEmbeddedPluginRegistryError.oversizedFragmentSet
        }
        guard let fragment = String(data: data, encoding: .utf8) else {
            throw VesperEmbeddedPluginRegistryError.invalidFragment(
                "fragment is not valid UTF-8: \(fragmentURL.path)"
            )
        }
        fragments.append(fragment)
    }
    return fragments
}

private func resolveVesperIosPluginFrameworks(
    _ resolutions: [VesperIosPluginResolution],
    frameworksURL: URL?
) throws -> [VesperIosResolvedFramework] {
    guard !resolutions.isEmpty else { return [] }
    guard let frameworksURL else {
        throw VesperEmbeddedPluginRegistryError.missingFrameworksDirectory
    }
    let root = frameworksURL.standardizedFileURL.resolvingSymlinksInPath()
    var seenPluginIds = Set<String>()
    var seenFrameworkPaths = Set<String>()

    return try resolutions.map { resolution in
        guard resolution.validation == "same-team-as-host-or-simulator-ad-hoc" else {
            throw VesperEmbeddedPluginRegistryError.invalidResolutionPayload(
                "unsupported integrity policy for `\(resolution.pluginId)`"
            )
        }
        guard seenPluginIds.insert(resolution.pluginId).inserted else {
            throw VesperEmbeddedPluginRegistryError.invalidResolutionPayload(
                "duplicate plugin `\(resolution.pluginId)`"
            )
        }
        let frameworkURL = root
            .appendingPathComponent("\(resolution.frameworkName).framework", isDirectory: true)
            .standardizedFileURL
        let frameworkValues = try frameworkURL.resourceValues(forKeys: [
            .isDirectoryKey,
            .isSymbolicLinkKey,
        ])
        guard frameworkValues.isDirectory == true, frameworkValues.isSymbolicLink != true else {
            throw VesperEmbeddedPluginRegistryError.missingFramework(resolution.pluginId)
        }
        let canonicalFramework = frameworkURL.resolvingSymlinksInPath()
        guard canonicalFramework.deletingLastPathComponent() == root,
              seenFrameworkPaths.insert(canonicalFramework.path).inserted
        else {
            throw VesperEmbeddedPluginRegistryError.invalidFramework(canonicalFramework.path)
        }
        guard let bundle = Bundle(url: canonicalFramework),
              bundle.bundleIdentifier == resolution.bundleIdentifier,
              let executableURL = bundle.executableURL
        else {
            throw VesperEmbeddedPluginRegistryError.invalidFramework(
                "bundle identity mismatch for `\(resolution.pluginId)`"
            )
        }
        let expectedBinary = canonicalFramework
            .appendingPathComponent(resolution.frameworkName, isDirectory: false)
            .standardizedFileURL
            .resolvingSymlinksInPath()
        let binary = try validateVesperIosPluginBinaryLocation(
            executableURL,
            expectedBinary: expectedBinary,
            pluginId: resolution.pluginId
        )
        return VesperIosResolvedFramework(
            pluginId: resolution.pluginId,
            frameworkName: resolution.frameworkName,
            bundleIdentifier: resolution.bundleIdentifier,
            frameworkPath: canonicalFramework.path,
            binaryPath: binary.path
        )
    }
}

func validateVesperIosPluginBinaryLocation(
    _ executableURL: URL,
    expectedBinary: URL,
    pluginId: String
) throws -> URL {
    let binary = executableURL.standardizedFileURL.resolvingSymlinksInPath()
    let binaryValues = try executableURL.resourceValues(forKeys: [
        .isRegularFileKey,
        .isSymbolicLinkKey,
    ])
    guard binary == expectedBinary,
          binaryValues.isRegularFile == true,
          binaryValues.isSymbolicLink != true
    else {
        throw VesperEmbeddedPluginRegistryError.invalidFramework(
            "invalid executable for `\(pluginId)`"
        )
    }
    return binary
}

private func freeVesperIosPluginString(_ pointer: UnsafeMutablePointer<CChar>?) {
    guard let pointer else { return }
    vesper_runtime_ios_plugin_string_free(pointer)
}

private func stringFromVesperIosPluginString(
    _ pointer: UnsafeMutablePointer<CChar>?
) -> String? {
    pointer.map { String(cString: $0) }
}
