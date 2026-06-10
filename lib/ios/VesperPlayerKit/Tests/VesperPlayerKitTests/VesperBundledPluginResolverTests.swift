import Foundation
@testable import VesperPlayerKit
import XCTest

final class VesperBundledPluginResolverTests: XCTestCase {
    private var temporaryDirectories: [URL] = []

    override func tearDownWithError() throws {
        for directory in temporaryDirectories {
            try? FileManager.default.removeItem(at: directory)
        }
        temporaryDirectories.removeAll()
        try super.tearDownWithError()
    }

    func testDisabledSourceNormalizerDoesNotResolveBundledPlugins() throws {
        let root = try makeTemporaryDirectory()
        _ = try makeFrameworkBinary(named: "VesperPlayerSourceNormalizerFfmpegPlugin", in: root)

        let resolved =
            VesperBundledPluginResolver.resolveSourceNormalizerConfiguration(
                VesperSourceNormalizerConfiguration(),
                frameworkSearchURLs: [root]
            )

        XCTAssertEqual(resolved.mode, .disabled)
        XCTAssertTrue(resolved.pluginLibraryPaths.isEmpty)
    }

    func testExplicitSourceNormalizerPluginPathsOverrideBundledDiscovery() throws {
        let root = try makeTemporaryDirectory()
        _ = try makeFrameworkBinary(named: "VesperPlayerSourceNormalizerFfmpegPlugin", in: root)
        let explicitPath = "/custom/VesperPlayerSourceNormalizerFfmpegPlugin"

        let resolved =
            VesperBundledPluginResolver.resolveSourceNormalizerConfiguration(
                VesperSourceNormalizerConfiguration(
                    mode: .preferNormalized,
                    pluginLibraryPaths: [explicitPath]
                ),
                frameworkSearchURLs: [root]
            )

        XCTAssertEqual(resolved.pluginLibraryPaths, [explicitPath])
    }

    func testEnabledSourceNormalizerUsesBundledPluginWhenAvailable() throws {
        let root = try makeTemporaryDirectory()
        let bundledPath =
            try makeFrameworkBinary(named: "VesperPlayerSourceNormalizerFfmpegPlugin", in: root)

        let resolved =
            VesperBundledPluginResolver.resolveSourceNormalizerConfiguration(
                VesperSourceNormalizerConfiguration(mode: .preferNormalized),
                frameworkSearchURLs: [root]
            )

        XCTAssertEqual(resolved.mode, .preferNormalized)
        XCTAssertEqual(resolved.pluginLibraryPaths, [bundledPath.path])
    }

    func testPreferNormalizedWithoutBundledPluginLeavesConfigurationNonFatal() throws {
        let root = try makeTemporaryDirectory()

        let resolved =
            VesperBundledPluginResolver.resolveSourceNormalizerConfiguration(
                VesperSourceNormalizerConfiguration(mode: .preferNormalized),
                frameworkSearchURLs: [root]
            )

        XCTAssertEqual(resolved.mode, .preferNormalized)
        XCTAssertTrue(resolved.pluginLibraryPaths.isEmpty)
    }

    func testRequireNormalizedWithoutBundledPluginKeepsRequiredModeForNativeFailure() throws {
        let root = try makeTemporaryDirectory()

        let resolved =
            VesperBundledPluginResolver.resolveSourceNormalizerConfiguration(
                VesperSourceNormalizerConfiguration(mode: .requireNormalized),
                frameworkSearchURLs: [root]
            )

        XCTAssertEqual(resolved.mode, .requireNormalized)
        XCTAssertTrue(resolved.pluginLibraryPaths.isEmpty)
    }

    private func makeTemporaryDirectory() throws -> URL {
        let directory =
            FileManager.default.temporaryDirectory
            .appendingPathComponent(UUID().uuidString, isDirectory: true)
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        temporaryDirectories.append(directory)
        return directory
    }

    private func makeFrameworkBinary(named frameworkName: String, in root: URL) throws -> URL {
        let frameworkDirectory =
            root.appendingPathComponent("\(frameworkName).framework", isDirectory: true)
        try FileManager.default.createDirectory(
            at: frameworkDirectory,
            withIntermediateDirectories: true
        )
        let binaryURL = frameworkDirectory.appendingPathComponent(frameworkName)
        try Data().write(to: binaryURL)
        return binaryURL.standardizedFileURL
    }
}
