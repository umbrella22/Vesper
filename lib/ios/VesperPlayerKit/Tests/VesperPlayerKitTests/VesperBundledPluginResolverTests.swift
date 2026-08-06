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

    func testEmptySelectionDoesNotAutoDiscoverBundledPlugins() throws {
        let root = try makeTemporaryDirectory()
        _ = try makeFrameworkBinary(named: "VesperPlayerSourceNormalizerFfmpegPlugin", in: root)

        let resolved = try VesperBundledPluginResolver.resolvePluginArtifacts(
            [],
            frameworkSearchURLs: [root]
        )

        XCTAssertTrue(resolved.libraryPaths.isEmpty)
    }

    func testExplicitSourceNormalizerReferenceResolvesBundledArtifact() throws {
        let root = try makeTemporaryDirectory()
        let frameworkPath = try makeFrameworkBinary(
            named: "VesperPlayerSourceNormalizerFfmpegPlugin",
            in: root
        )

        let resolved = try VesperBundledPluginResolver.resolvePluginArtifacts(
            [VesperBundledPluginReferences.sourceNormalizerFfmpeg],
            frameworkSearchURLs: [root]
        )

        XCTAssertEqual(resolved.libraryPaths, [frameworkPath.path])
    }

    func testExplicitSourceNormalizerReferenceIgnoresFlatDylibAndUsesFramework() throws {
        let root = try makeTemporaryDirectory()
        _ = try makeFlatBinary(
            named: "libvesper_source_normalizer_ffmpeg.dylib",
            in: root
        )
        let frameworkPath = try makeFrameworkBinary(
            named: "VesperPlayerSourceNormalizerFfmpegPlugin",
            in: root
        )

        let resolved = try VesperBundledPluginResolver.resolvePluginArtifacts(
            [VesperBundledPluginReferences.sourceNormalizerFfmpeg],
            frameworkSearchURLs: [root]
        )

        XCTAssertEqual(resolved.libraryPaths, [frameworkPath.path])
    }

    func testCanonicalMobileReferencesUseNativeTransport() {
        XCTAssertEqual(VesperBundledPluginReferences.sourceNormalizerFfmpeg.transport, .native)
        XCTAssertEqual(VesperBundledPluginReferences.decoderVideoToolbox.transport, .native)
        XCTAssertEqual(VesperBundledPluginReferences.frameProcessorDiagnostic.transport, .native)
    }

    func testPluginReferencesResolveOneArtifactPerPluginRoot() throws {
        let root = try makeTemporaryDirectory()
        let remuxPath = try makeFrameworkBinary(
            named: "VesperPlayerRemuxFfmpegPlugin",
            in: root
        )
        let postDownload = try VesperPluginReference(
            pluginId: "io.github.ikaros.vesper.remux-ffmpeg",
            capabilityInstanceId: "io.github.ikaros.vesper.remux-ffmpeg.post-download",
            transport: .native
        )
        let eventHook = try VesperPluginReference(
            pluginId: "io.github.ikaros.vesper.remux-ffmpeg",
            capabilityInstanceId: "io.github.ikaros.vesper.remux-ffmpeg.event-hook",
            transport: .native
        )

        let artifacts = try VesperBundledPluginResolver.resolvePluginArtifacts(
            [postDownload, postDownload, eventHook],
            frameworkSearchURLs: [root]
        )

        XCTAssertEqual(artifacts.libraryPaths, [remuxPath.path])
        XCTAssertEqual(artifacts.artifacts.map(\.reference), [postDownload, eventHook])
        XCTAssertEqual(
            artifacts.artifacts.map(\.libraryPath),
            [remuxPath.path, remuxPath.path]
        )

        let json = try encodeVesperResolvedPluginArtifactsJSON(artifacts)
        let values = try XCTUnwrap(
            JSONSerialization.jsonObject(with: Data(json.utf8)) as? [[String: Any]]
        )
        let capabilityInstanceIds = values.compactMap { value in
            (value["reference"] as? [String: Any])?["capabilityInstanceId"] as? String
        }
        XCTAssertEqual(
            capabilityInstanceIds,
            [
                "io.github.ikaros.vesper.remux-ffmpeg.post-download",
                "io.github.ikaros.vesper.remux-ffmpeg.event-hook",
            ]
        )
    }

    func testDiagnosticSanitizationUsesCanonicalInstanceForSharedArtifact() throws {
        let pluginId = "io.github.ikaros.vesper.remux-ffmpeg"
        let first = try VesperPluginReference(
            pluginId: pluginId,
            capabilityInstanceId: "io.github.ikaros.vesper.remux-ffmpeg.first",
            transport: .native
        )
        let second = try VesperPluginReference(
            pluginId: pluginId,
            capabilityInstanceId: "io.github.ikaros.vesper.remux-ffmpeg.second",
            transport: .native
        )
        let secondInstanceId = try XCTUnwrap(second.capabilityInstanceId)
        let path = "/Frameworks/VesperPlayerRemuxFfmpegPlugin"
        let artifacts = VesperResolvedPluginArtifacts(
            artifacts: [
                .init(reference: first, libraryPath: path),
                .init(reference: second, libraryPath: path),
            ]
        )

        let canonical = try XCTUnwrap(
            pluginDiagnosticsReplacingArtifactPaths(
                [
                    [
                        "path": path,
                        "details": [
                            "pluginId": pluginId,
                            "capabilityInstanceId": secondInstanceId,
                            "transport": "native",
                        ],
                    ]
                ],
                artifacts: [artifacts]
            ).first
        )
        XCTAssertNil(canonical["path"])
        XCTAssertEqual(canonical["pluginId"] as? String, pluginId)
        XCTAssertEqual(
            canonical["capabilityInstanceId"] as? String,
            secondInstanceId
        )
        XCTAssertEqual(
            (canonical["pluginReference"] as? [String: Any])?["capabilityInstanceId"] as? String,
            secondInstanceId
        )

        let candidates = try XCTUnwrap(
            pluginDiagnosticsReplacingArtifactPaths(
                [["path": path]],
                artifacts: [artifacts]
            ).first
        )
        XCTAssertNil(candidates["capabilityInstanceId"])
        let candidateReferences = try XCTUnwrap(
            candidates["pluginReferences"] as? [[String: Any]]
        )
        XCTAssertEqual(
            candidateReferences.compactMap { $0["capabilityInstanceId"] as? String },
            [first.capabilityInstanceId, second.capabilityInstanceId].compactMap { $0 }
        )
    }

    func testPluginReferenceResolutionRejectsUnsupportedTransportAndMissingArtifact() throws {
        let wasm = try VesperPluginReference(
            pluginId: "io.github.ikaros.vesper.remux-ffmpeg",
            transport: .wasm
        )
        XCTAssertThrowsError(
            try VesperBundledPluginResolver.resolvePluginArtifacts(
                [wasm],
                frameworkSearchURLs: []
            )
        ) { error in
            XCTAssertEqual(
                error as? VesperBundledPluginResolutionError,
                .unsupportedTransport("wasm")
            )
        }

        let missing = try VesperPluginReference(
            pluginId: "io.github.ikaros.vesper.remux-ffmpeg",
            transport: .native
        )
        XCTAssertThrowsError(
            try VesperBundledPluginResolver.resolvePluginArtifacts(
                [missing],
                frameworkSearchURLs: []
            )
        ) { error in
            XCTAssertEqual(
                error as? VesperBundledPluginResolutionError,
                .missingArtifact("io.github.ikaros.vesper.remux-ffmpeg")
            )
        }
    }

    func testPluginReferenceResolutionCapsUniqueReferences() throws {
        let references = try (0...256).map { index in
            try VesperPluginReference(
                pluginId: "io.github.ikaros.vesper.remux-ffmpeg",
                capabilityInstanceId: "io.github.ikaros.vesper.remux-ffmpeg.capability-\(index)",
                transport: .native
            )
        }

        XCTAssertThrowsError(
            try VesperBundledPluginResolver.resolvePluginArtifacts(
                references,
                frameworkSearchURLs: []
            )
        ) { error in
            XCTAssertEqual(
                error as? VesperBundledPluginResolutionError,
                .tooManyReferences(257)
            )
        }
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

    private func makeFlatBinary(named binaryName: String, in root: URL) throws -> URL {
        let binaryURL = root.appendingPathComponent(binaryName)
        try Data().write(to: binaryURL)
        return binaryURL.standardizedFileURL
    }
}
