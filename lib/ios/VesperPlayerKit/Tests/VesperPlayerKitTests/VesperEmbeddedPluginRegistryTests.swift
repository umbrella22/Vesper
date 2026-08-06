import Foundation
@testable import VesperPlayerKit
import XCTest

final class VesperEmbeddedPluginRegistryTests: XCTestCase {
    func testFrameworkCountDoesNotConsumeFragmentLimit() throws {
        let root = try makeTemporaryFrameworksDirectory()
        defer { try? FileManager.default.removeItem(at: root) }

        for index in 0..<257 {
            try createFramework(named: "Ordinary\(index)", under: root, fragment: nil)
        }
        try createFramework(named: "VesperPlugin", under: root, fragment: "{}")

        let fragments = try loadVesperIosPluginRegistryFragments(
            frameworksURL: root,
            fileManager: .default
        )

        XCTAssertEqual(fragments, ["{}"])
    }

    func testFragmentCountIsBoundedIndependentlyOfFrameworkCount() throws {
        let root = try makeTemporaryFrameworksDirectory()
        defer { try? FileManager.default.removeItem(at: root) }

        for index in 0..<257 {
            try createFramework(named: "Plugin\(index)", under: root, fragment: "{}")
        }

        XCTAssertThrowsError(
            try loadVesperIosPluginRegistryFragments(
                frameworksURL: root,
                fileManager: .default
            )
        ) { error in
            guard case let VesperEmbeddedPluginRegistryError.tooManyFragments(count) = error else {
                return XCTFail("unexpected error: \(error)")
            }
            XCTAssertEqual(count, 257)
        }
    }

    func testBinaryLocationDoesNotRequireHostPOSIXExecutePermission() throws {
        let root = try makeTemporaryFrameworksDirectory()
        defer { try? FileManager.default.removeItem(at: root) }
        let binary = root.appendingPathComponent("VesperPlugin")
        try Data("signed Mach-O fixture".utf8).write(to: binary)
        try FileManager.default.setAttributes(
            [.posixPermissions: 0o644],
            ofItemAtPath: binary.path
        )

        XCTAssertFalse(FileManager.default.isExecutableFile(atPath: binary.path))
        XCTAssertEqual(
            try validateVesperIosPluginBinaryLocation(
                binary,
                expectedBinary: binary,
                pluginId: "dev.vesper.fixture"
            ),
            binary
        )
    }

    func testBinaryLocationRejectsSymbolicLinks() throws {
        let root = try makeTemporaryFrameworksDirectory()
        defer { try? FileManager.default.removeItem(at: root) }
        let target = root.appendingPathComponent("PluginTarget")
        let binary = root.appendingPathComponent("VesperPlugin")
        try Data("signed Mach-O fixture".utf8).write(to: target)
        try FileManager.default.createSymbolicLink(at: binary, withDestinationURL: target)

        XCTAssertThrowsError(
            try validateVesperIosPluginBinaryLocation(
                binary,
                expectedBinary: target,
                pluginId: "dev.vesper.fixture"
            )
        )
    }

    private func makeTemporaryFrameworksDirectory() throws -> URL {
        let root = FileManager.default.temporaryDirectory.appendingPathComponent(
            "vesper-plugin-registry-tests-\(UUID().uuidString)",
            isDirectory: true
        )
        try FileManager.default.createDirectory(
            at: root,
            withIntermediateDirectories: false
        )
        return root
    }

    private func createFramework(
        named name: String,
        under root: URL,
        fragment: String?
    ) throws {
        let framework = root.appendingPathComponent("\(name).framework", isDirectory: true)
        try FileManager.default.createDirectory(
            at: framework,
            withIntermediateDirectories: false
        )
        guard let fragment else { return }
        try Data(fragment.utf8).write(
            to: framework.appendingPathComponent("vesper-plugin-registry.json")
        )
    }
}
