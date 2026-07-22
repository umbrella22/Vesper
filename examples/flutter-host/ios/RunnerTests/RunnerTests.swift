import Darwin
import Flutter
@testable import Runner
import UIKit
import XCTest

class RunnerTests: XCTestCase {

  func testBundledOptionalPluginFrameworkEntriesLoad() throws {
    let frameworksURL = try XCTUnwrap(Bundle.main.privateFrameworksURL)
    for frameworkName in [
      "VesperPlayerRemuxFfmpegPlugin",
      "VesperPlayerSourceNormalizerFfmpegPlugin",
      "VesperPlayerDecoderVideoToolboxPlugin",
      "VesperPlayerFrameProcessorDiagnosticPlugin",
    ] {
      let binaryURL = frameworksURL
        .appendingPathComponent("\(frameworkName).framework", isDirectory: true)
        .appendingPathComponent(frameworkName, isDirectory: false)
      XCTAssertTrue(
        FileManager.default.fileExists(atPath: binaryURL.path),
        "Missing bundled optional plugin framework binary: \(binaryURL.path)"
      )

      dlerror()
      guard let handle = dlopen(binaryURL.path, RTLD_NOW | RTLD_LOCAL) else {
        XCTFail("Failed to load \(frameworkName): \(dynamicLoaderMessage())")
        continue
      }
      defer { dlclose(handle) }

      dlerror()
      guard let symbol = dlsym(handle, "vesper_plugin_entry") else {
        XCTFail("Missing vesper_plugin_entry in \(frameworkName): \(dynamicLoaderMessage())")
        continue
      }
      typealias PluginEntry = @convention(c) () -> UnsafeRawPointer?
      let entry = unsafeBitCast(symbol, to: PluginEntry.self)
      XCTAssertNotNil(entry(), "The plugin descriptor must not be null: \(frameworkName)")
    }
  }

  func testBundledPluginResolverIgnoresFlatDylibAndUsesFramework() throws {
    let frameworksURL = FileManager.default.temporaryDirectory
      .appendingPathComponent(UUID().uuidString, isDirectory: true)
    try FileManager.default.createDirectory(at: frameworksURL, withIntermediateDirectories: true)
    defer { try? FileManager.default.removeItem(at: frameworksURL) }

    let dylibURL = frameworksURL.appendingPathComponent("libvesper_remux_ffmpeg.dylib")
    XCTAssertTrue(FileManager.default.createFile(atPath: dylibURL.path, contents: Data()))
    let frameworkBinaryURL = frameworksURL
      .appendingPathComponent("VesperPlayerRemuxFfmpegPlugin.framework", isDirectory: true)
      .appendingPathComponent("VesperPlayerRemuxFfmpegPlugin")
    try FileManager.default.createDirectory(
      at: frameworkBinaryURL.deletingLastPathComponent(),
      withIntermediateDirectories: true
    )
    XCTAssertTrue(FileManager.default.createFile(atPath: frameworkBinaryURL.path, contents: Data()))

    XCTAssertEqual(
      resolveBundledPluginLibraryPaths(
        frameworkName: "VesperPlayerRemuxFfmpegPlugin",
        binaryName: "VesperPlayerRemuxFfmpegPlugin",
        frameworksURL: frameworksURL
      ),
      [frameworkBinaryURL.standardizedFileURL.path]
    )
  }

  func testBundledPluginResolverUsesFramework() throws {
    let frameworksURL = FileManager.default.temporaryDirectory
      .appendingPathComponent(UUID().uuidString, isDirectory: true)
    try FileManager.default.createDirectory(at: frameworksURL, withIntermediateDirectories: true)
    defer { try? FileManager.default.removeItem(at: frameworksURL) }

    let frameworkBinaryURL = frameworksURL
      .appendingPathComponent("VesperPlayerRemuxFfmpegPlugin.framework", isDirectory: true)
      .appendingPathComponent("VesperPlayerRemuxFfmpegPlugin")
    try FileManager.default.createDirectory(
      at: frameworkBinaryURL.deletingLastPathComponent(),
      withIntermediateDirectories: true
    )
    XCTAssertTrue(FileManager.default.createFile(atPath: frameworkBinaryURL.path, contents: Data()))

    XCTAssertEqual(
      resolveBundledPluginLibraryPaths(
        frameworkName: "VesperPlayerRemuxFfmpegPlugin",
        binaryName: "VesperPlayerRemuxFfmpegPlugin",
        frameworksURL: frameworksURL
      ),
      [frameworkBinaryURL.standardizedFileURL.path]
    )
  }

  private func dynamicLoaderMessage() -> String {
    guard let message = dlerror() else {
      return "unknown dynamic loader error"
    }
    return String(cString: message)
  }

}
