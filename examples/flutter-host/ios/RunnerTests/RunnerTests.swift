import Darwin
import Flutter
@testable import Runner
@testable import vesper_player_ios
import UIKit
import VesperPlayerKit
import XCTest

class RunnerTests: XCTestCase {

  func testPendingDownloadSnapshotResyncEmitsNoFlutterEvent() {
    let payloads = flutterDownloadEventPayloads(
      downloadId: "downloads",
      snapshot: ["tasks": []],
      batch: VesperDownloadEventBatch(
        events: [downloadProgressEvent()],
        droppedEvents: 2,
        requiresSnapshotResync: true,
        snapshotIsAuthoritative: false
      )
    )

    XCTAssertTrue(payloads.isEmpty)
  }

  func testAuthoritativeDownloadSnapshotResyncSuppressesRetainedEvents() throws {
    let payloads = flutterDownloadEventPayloads(
      downloadId: "downloads",
      snapshot: ["tasks": [["taskId": NSNumber(value: 7)]]],
      batch: VesperDownloadEventBatch(
        events: [downloadProgressEvent()],
        droppedEvents: 3,
        requiresSnapshotResync: true,
        snapshotIsAuthoritative: true
      )
    )

    let payload = try XCTUnwrap(payloads.first)
    XCTAssertEqual(payloads.count, 1)
    XCTAssertEqual(payload["downloadId"] as? String, "downloads")
    XCTAssertEqual(payload["type"] as? String, "downloadResync")
    XCTAssertEqual((payload["droppedEvents"] as? NSNumber)?.uint64Value, 3)
    let snapshot = try XCTUnwrap(payload["snapshot"] as? [String: Any])
    XCTAssertEqual((snapshot["tasks"] as? [[String: Any]])?.count, 1)
  }

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
      guard dlsym(handle, "vesper_plugin_entry") != nil else {
        XCTFail("Missing vesper_plugin_entry in \(frameworkName): \(dynamicLoaderMessage())")
        continue
      }
    }
  }

  private func dynamicLoaderMessage() -> String {
    guard let message = dlerror() else {
      return "unknown dynamic loader error"
    }
    return String(cString: message)
  }

  private func downloadProgressEvent() -> VesperDownloadEvent {
    .progressUpdated(
      VesperDownloadTaskProgressPatch(
        taskId: 7,
        progress: VesperDownloadProgressSnapshot(receivedBytes: 512)
      )
    )
  }

}
