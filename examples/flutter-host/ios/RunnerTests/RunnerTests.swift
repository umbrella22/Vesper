import AVKit
import Combine
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
      "VesperPlayerPerformanceDiagnosticsPlugin",
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

  func testPerformanceDiagnosticsBooleanMappingRejectsNumericNSNumber() throws {
    XCTAssertThrowsError(
      try ["includeRawEvents": NSNumber(value: 1)].toPerformanceDiagnosticsConfiguration()
    ) { error in
      XCTAssertEqual(
        (error as? VesperPerformanceDiagnosticsError)?.code,
        .invalidConfiguration
      )
    }

    XCTAssertThrowsError(
      try ["expectedOverlayActive": NSNumber(value: 1)]
        .optionalPerformanceExpectedOverlayActive()
    ) { error in
      XCTAssertEqual(
        (error as? VesperPerformanceDiagnosticsError)?.code,
        .protocolViolation
      )
    }
  }

  func testPerformanceDiagnosticsBooleanMappingAcceptsCfBoolean() throws {
    let configuration = try ["includeRawEvents": NSNumber(value: true)]
      .toPerformanceDiagnosticsConfiguration()
    XCTAssertTrue(configuration.includeRawEvents)

    XCTAssertEqual(
      try ["expectedOverlayActive": NSNumber(value: false)]
        .optionalPerformanceExpectedOverlayActive(),
      false
    )
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

final class PictureInPictureRegressionTests: XCTestCase {
  @MainActor
  func testConfigurationCreatesAndUpdatesAutomaticPictureInPicture() throws {
    let fixture = PictureInPictureFixture()
    defer { fixture.dispose() }
    fixture.attachPlayer()

    fixture.configure(enabled: true, autoEnter: true)
    let system = try XCTUnwrap(fixture.controllers.last)
    XCTAssertEqual(fixture.controllers.count, 1)
    XCTAssertTrue(system.canStartPictureInPictureAutomaticallyFromInline)

    fixture.configure(enabled: true, autoEnter: false)
    XCTAssertFalse(system.canStartPictureInPictureAutomaticallyFromInline)
    XCTAssertEqual(system.invalidationCount, 1)
    XCTAssertNil(system.delegate)
    XCTAssertEqual(fixture.controllers.count, 1)
    fixture.configure(enabled: true, autoEnter: false)
    XCTAssertEqual(fixture.controllers.count, 1)
    fixture.configure(enabled: true, autoEnter: true)
    let automatic = try XCTUnwrap(fixture.controllers.last)
    XCTAssertEqual(fixture.controllers.count, 2)
    XCTAssertTrue(automatic.canStartPictureInPictureAutomaticallyFromInline)
    fixture.configure(enabled: false, autoEnter: true)
    XCTAssertFalse(automatic.canStartPictureInPictureAutomaticallyFromInline)
    let stopCount = automatic.stopCount
    fixture.configure(enabled: false, autoEnter: true)
    XCTAssertEqual(automatic.stopCount, stopCount)
    XCTAssertEqual(automatic.invalidationCount, 1)
    XCTAssertNil(automatic.delegate)
    XCTAssertEqual(fixture.controllers.count, 2)
    fixture.configure(enabled: true, autoEnter: true)
    XCTAssertEqual(fixture.controllers.count, 3)
    XCTAssertTrue(try XCTUnwrap(fixture.controllers.last).canStartPictureInPictureAutomaticallyFromInline)
  }

  @MainActor
  func testDisablingAutomaticEntryPreservesPendingManualStart() async throws {
    let fixture = PictureInPictureFixture()
    defer { fixture.dispose() }
    fixture.attachPlayer()
    fixture.configure(enabled: true, autoEnter: true)
    let system = try XCTUnwrap(fixture.controllers.last)
    fixture.session.pictureInPictureState = "entering"
    fixture.coordinator.start()

    fixture.configure(enabled: true, autoEnter: false)
    system.possibility.send(true)
    try await Task.sleep(for: .milliseconds(30))

    XCTAssertEqual(fixture.controllers.count, 1)
    XCTAssertFalse(system.canStartPictureInPictureAutomaticallyFromInline)
    XCTAssertEqual(system.invalidationCount, 0)
    XCTAssertEqual(system.startCount, 1)
  }

  @MainActor
  func testConfigurationWaitsForPlayerAndFollowsReplacementSurface() async throws {
    let fixture = PictureInPictureFixture()
    defer { fixture.dispose() }
    fixture.configure(enabled: true, autoEnter: true)
    XCTAssertTrue(fixture.controllers.isEmpty)
    fixture.attachPlayer()
    let snapshot = expectation(description: "snapshot after player attachment")
    _ = fixture.plugin.onListen(withArguments: nil) { _ in snapshot.fulfill() }
    await fulfillment(of: [snapshot], timeout: 2)
    fixture.plugin.eventSink = nil
    let first = try XCTUnwrap(fixture.controllers.last)
    XCTAssertTrue(first.canStartPictureInPictureAutomaticallyFromInline)

    let replacement = PlayerSurfaceView()
    fixture.plugin.bindSessionHost(playerId: fixture.session.id, host: replacement)
    XCTAssertFalse(first.canStartPictureInPictureAutomaticallyFromInline)
    XCTAssertNil(first.delegate)
    fixture.attachPlayer(to: replacement)
    fixture.configure(enabled: true, autoEnter: true)
    XCTAssertEqual(fixture.controllers.count, 2)
    XCTAssertTrue(try XCTUnwrap(fixture.controllers.last).canStartPictureInPictureAutomaticallyFromInline)
  }

  @MainActor
  func testExitCancelsStartWaitingForPossibility() async throws {
    let fixture = PictureInPictureFixture()
    defer { fixture.dispose() }
    fixture.attachPlayer()
    fixture.configure(enabled: true, autoEnter: true)
    let system = try XCTUnwrap(fixture.controllers.last)
    fixture.session.pictureInPictureState = "entering"
    fixture.coordinator.start()

    fixture.command("exitPictureInPicture")
    system.possibility.send(true)
    try await Task.sleep(for: .milliseconds(30))

    XCTAssertEqual(fixture.session.pictureInPictureState, "inactive")
    XCTAssertEqual(system.startCount, 0)
    XCTAssertGreaterThan(system.stopCount, 0)
  }

  @MainActor
  func testExitRejectsAlreadyQueuedStartAndLaterRequestCanStart() async throws {
    let fixture = PictureInPictureFixture()
    defer { fixture.dispose() }
    fixture.attachPlayer()
    fixture.configure(enabled: true, autoEnter: true)
    let system = try XCTUnwrap(fixture.controllers.last)
    fixture.coordinator.start()
    system.possibility.send(true)
    fixture.command("exitPictureInPicture")
    try await Task.sleep(for: .milliseconds(30))
    XCTAssertEqual(system.startCount, 0)

    fixture.coordinator.start()
    system.possibility.send(true)
    system.possibility.send(true)
    try await Task.sleep(for: .milliseconds(30))
    XCTAssertEqual(system.startCount, 1)
  }

  @MainActor
  func testStartWaitsAgainWhenQueuedPossibilityBecomesFalse() async throws {
    let fixture = PictureInPictureFixture()
    defer { fixture.dispose() }
    fixture.attachPlayer()
    fixture.configure(enabled: true, autoEnter: true)
    let system = try XCTUnwrap(fixture.controllers.last)
    fixture.coordinator.start()
    system.possibility.send(true)
    system.possibility.send(false)
    try await Task.sleep(for: .milliseconds(30))
    XCTAssertEqual(system.startCount, 0)

    system.possibility.send(true)
    try await Task.sleep(for: .milliseconds(30))
    XCTAssertEqual(system.startCount, 1)
  }

  @MainActor
  func testReplacementRejectsQueuedStartFromPreviousLayer() async throws {
    let fixture = PictureInPictureFixture()
    defer { fixture.dispose() }
    fixture.attachPlayer()
    fixture.configure(enabled: true, autoEnter: true)
    let first = try XCTUnwrap(fixture.controllers.last)
    fixture.coordinator.start()
    first.possibility.send(true)
    let layer = AVPlayerLayer(player: AVPlayer())
    XCTAssertTrue(fixture.coordinator.configure(with: layer))
    try await Task.sleep(for: .milliseconds(30))
    XCTAssertEqual(first.startCount, 0)
    XCTAssertNil(first.delegate)
    XCTAssertFalse(first.canStartPictureInPictureAutomaticallyFromInline)
  }

  @MainActor
  func testDisablingPendingStartClearsStateAndRejectsQueuedCallback() async throws {
    let fixture = PictureInPictureFixture()
    defer { fixture.dispose() }
    fixture.attachPlayer()
    fixture.configure(enabled: true, autoEnter: true)
    let system = try XCTUnwrap(fixture.controllers.last)
    fixture.session.pictureInPictureState = "entering"
    fixture.coordinator.start()
    system.possibility.send(true)

    fixture.configure(enabled: false, autoEnter: false)
    try await Task.sleep(for: .milliseconds(30))

    XCTAssertEqual(system.startCount, 0)
    XCTAssertEqual(fixture.session.pictureInPictureState, "inactive")
  }

  @MainActor
  func testDisablingActiveControllerWaitsForStopAndReenableCreatesNewController() throws {
    let fixture = PictureInPictureFixture()
    defer { fixture.dispose() }
    fixture.attachPlayer()
    fixture.configure(enabled: true, autoEnter: true)
    let system = try XCTUnwrap(fixture.controllers.last)
    system.isPictureInPictureActive = true
    fixture.session.pictureInPictureState = "active"
    fixture.session.pictureInPictureActive = true

    fixture.configure(enabled: false, autoEnter: false)
    XCTAssertEqual(system.stopCount, 1)
    XCTAssertNotNil(system.delegate)
    XCTAssertTrue(fixture.session.pictureInPictureActive)
    // AVKit may clear isActive before delivering its final didStop callback.
    system.isPictureInPictureActive = false
    fixture.configure(enabled: false, autoEnter: false)
    XCTAssertEqual(system.stopCount, 1)
    XCTAssertNotNil(system.delegate)
    XCTAssertEqual(system.invalidationCount, 0)

    fixture.configure(enabled: true, autoEnter: true)
    XCTAssertEqual(system.invalidationCount, 1)
    XCTAssertNil(system.delegate)
    XCTAssertEqual(fixture.controllers.count, 2)
    XCTAssertFalse(fixture.session.pictureInPictureActive)
    XCTAssertTrue(try XCTUnwrap(fixture.controllers.last).canStartPictureInPictureAutomaticallyFromInline)
  }

  @MainActor
  func testRejectedRequestStillAppliesDisabledConfiguration() throws {
    let fixture = PictureInPictureFixture()
    defer { fixture.dispose() }
    fixture.attachPlayer()
    fixture.configure(enabled: true, autoEnter: true)
    let system = try XCTUnwrap(fixture.controllers.last)
    var receivedError = false
    fixture.plugin.handle(FlutterMethodCall(methodName: "requestPictureInPicture", arguments: [
      "playerId": fixture.session.id,
      "configuration": ["enabled": false, "autoEnter": false],
    ])) { result in receivedError = result is FlutterError }

    XCTAssertTrue(receivedError)
    XCTAssertFalse(system.canStartPictureInPictureAutomaticallyFromInline)
    XCTAssertEqual(fixture.session.pictureInPictureState, "failed")
    fixture.configure(enabled: false, autoEnter: false)
    XCTAssertEqual(fixture.session.pictureInPictureState, "failed")
  }

  @MainActor
  func testReplacingAnActiveLayerClearsThePreviousSessionState() throws {
    let fixture = PictureInPictureFixture()
    defer { fixture.dispose() }
    fixture.attachPlayer()
    fixture.configure(enabled: true, autoEnter: true)
    let system = try XCTUnwrap(fixture.controllers.last)
    system.isPictureInPictureActive = true
    fixture.session.pictureInPictureActive = true
    fixture.session.pictureInPictureState = "active"

    fixture.plugin.bindSessionHost(playerId: fixture.session.id, host: PlayerSurfaceView())

    XCTAssertFalse(fixture.session.pictureInPictureActive)
    XCTAssertEqual(fixture.session.pictureInPictureState, "inactive")
    XCTAssertNil(system.delegate)
  }
}

@MainActor
private final class PictureInPictureFixture {
  let plugin = VesperPlayerIosPlugin()
  let session = PlayerSession(id: "pip-regression", controller: VesperPlayerControllerFactory.makeDefault())
  let host = PlayerSurfaceView()
  var controllers: [PictureInPictureControllerStub] = []
  var coordinator: VesperIosPictureInPictureCoordinator { session.pictureInPictureCoordinator! }

  init() {
    session.pictureInPictureCoordinator = VesperIosPictureInPictureCoordinator(
      plugin: plugin, session: session,
      makeController: { [weak self] layer in
        let controller = PictureInPictureControllerStub(layer: layer)
        self?.controllers.append(controller)
        return controller
      }
    )
    plugin.sessions[session.id] = session
    plugin.bindSessionHost(playerId: session.id, host: host)
  }

  func attachPlayer(to surface: PlayerSurfaceView? = nil) {
    let surface = surface ?? host
    let layer = surface.layer.sublayers?.compactMap { $0 as? AVPlayerLayer }.first
    XCTAssertNotNil(layer)
    layer?.player = AVPlayer()
  }

  func configure(enabled: Bool, autoEnter: Bool) {
    command("setPictureInPictureConfiguration", extra: [
      "configuration": ["enabled": enabled, "autoEnter": autoEnter]
    ])
  }

  func command(_ name: String, extra: [String: Any] = [:]) {
    var arguments = extra
    arguments["playerId"] = session.id
    var completed = false
    plugin.handle(FlutterMethodCall(methodName: name, arguments: arguments)) { result in
      XCTAssertFalse(result is FlutterError, "\(String(describing: result))")
      completed = true
    }
    XCTAssertTrue(completed)
  }

  func dispose() {
    session.pictureInPictureCoordinator?.reset()
    session.controller.dispose()
  }
}

@MainActor
private final class PictureInPictureControllerStub: VesperIosPictureInPictureController {
  let playerLayer: AVPlayerLayer
  var isPictureInPicturePossible: Bool { possibility.value }
  var isPictureInPictureActive = false
  var canStartPictureInPictureAutomaticallyFromInline = false
  weak var delegate: AVPictureInPictureControllerDelegate?
  let possibility = CurrentValueSubject<Bool, Never>(false)
  var startCount = 0
  var stopCount = 0
  var invalidationCount = 0

  init(layer: AVPlayerLayer) { playerLayer = layer }
  func startPictureInPicture() { startCount += 1 }
  func stopPictureInPicture() { stopCount += 1 }
  func invalidateContentSource() { invalidationCount += 1 }
  func observePossibility(_ changed: @escaping (Bool) -> Void) -> AnyCancellable {
    possibility.sink(receiveValue: changed)
  }
}

/// Opt-in system acceptance. Copy a portrait movie to Documents/vesper-portrait-pip.mov
/// and activate another app at READY_* markers, then return at *_VERIFIED markers.
final class PictureInPictureDeviceTests: XCTestCase {
  @MainActor
  func testPortraitAutomaticEntryAndDisablingSystemPictureInPicture() async throws {
    guard ProcessInfo.processInfo.environment["VESPER_IOS_PIP_DEVICE_TESTS"] == "1" else {
      throw XCTSkip("Set VESPER_IOS_PIP_DEVICE_TESTS=1 and supply the portrait fixture and app-switch driver")
    }
    let movie = try FileManager.default.url(for: .documentDirectory, in: .userDomainMask,
                                            appropriateFor: nil, create: false)
      .appendingPathComponent("vesper-portrait-pip.mov")
    XCTAssertTrue(FileManager.default.fileExists(atPath: movie.path))
    let plugin = VesperPlayerIosPlugin()
    let controller = VesperPlayerControllerFactory.makeDefault(initialSource: .localFile(url: movie))
    let session = PlayerSession(id: "pip-device", controller: controller)
    plugin.sessions[session.id] = session
    let scene = try XCTUnwrap(UIApplication.shared.connectedScenes.compactMap { $0 as? UIWindowScene }.first)
    let originalWindow = scene.windows.first { $0.isKeyWindow }
    let window = UIWindow(windowScene: scene)
    let host = PlayerSurfaceView()
    window.rootViewController = UIViewController()
    window.rootViewController?.view = host
    window.makeKeyAndVisible()
    plugin.bindSessionHost(playerId: session.id, host: host)
    let backgroundTask = UIApplication.shared.beginBackgroundTask(withName: "PiP acceptance")
    defer {
      session.pictureInPictureCoordinator?.reset()
      controller.dispose()
      window.isHidden = true
      window.rootViewController = nil
      originalWindow?.makeKeyAndVisible()
      if backgroundTask != .invalid { UIApplication.shared.endBackgroundTask(backgroundTask) }
    }
    weak var systemController: AVPictureInPictureController?
    var checkpoints: [[String: Any]] = []
    func checkpoint(_ name: String) {
      checkpoints.append([
        "step": name, "time": Date().timeIntervalSince1970,
        "state": session.pictureInPictureState, "active": session.pictureInPictureActive,
        "systemPossible": systemController?.isPictureInPicturePossible == true,
        "systemActive": systemController?.isPictureInPictureActive == true,
        "systemAutomatic": systemController?.canStartPictureInPictureAutomaticallyFromInline == true,
        "controller": systemController.map { String(describing: ObjectIdentifier($0)) } ?? "nil",
        "playerRate": host.pictureInPicturePlayerLayer?.player?.rate ?? 0,
        "timeControlStatus": host.pictureInPicturePlayerLayer?.player?.timeControlStatus.rawValue ?? -1,
        "waitingReason": host.pictureInPicturePlayerLayer?.player?.reasonForWaitingToPlay?.rawValue ?? "none",
        "layerReady": host.pictureInPicturePlayerLayer?.isReadyForDisplay == true,
        "hostAttached": host.window === window,
        "windowKey": window.isKeyWindow, "windowHidden": window.isHidden,
        "applicationState": UIApplication.shared.applicationState.rawValue,
        "sceneState": scene.activationState.rawValue,
      ])
    }
    session.pictureInPictureCoordinator = VesperIosPictureInPictureCoordinator(
      plugin: plugin, session: session,
      makeController: { layer in
        guard AVPictureInPictureController.isPictureInPictureSupported() else { return nil }
        let next = AVPictureInPictureController(playerLayer: layer)
        systemController = next
        return next
      }
    )
    defer {
      checkpoint("finished")
      if let data = try? JSONSerialization.data(withJSONObject: checkpoints) {
        let attachment = XCTAttachment(data: data, uniformTypeIdentifier: "public.json")
        attachment.name = "system-pip-checkpoints.json"
        attachment.lifetime = .keepAlways
        add(attachment)
      }
    }
    controller.initialize()
    try await waitFor("Portrait playback did not start") {
      host.pictureInPicturePlayerLayer?.isReadyForDisplay == true
        && (host.pictureInPicturePlayerLayer?.player?.rate ?? 0) > 0
    }
    let layer = try XCTUnwrap(host.pictureInPicturePlayerLayer)
    layer.player?.isMuted = true
    let size = try XCTUnwrap(layer.player?.currentItem).presentationSize
    XCTAssertEqual(size.width / size.height, 9.0 / 16, accuracy: 0.002)
    var activeEvents = 0
    plugin.eventSink = { payload in
      guard let event = payload as? [String: Any], event["type"] as? String == "pictureInPicture" else { return }
      if event["state"] as? String == "active" { activeEvents += 1 }
      checkpoint("event")
    }
    func command(_ method: String, configuration: [String: Bool]? = nil) {
      var arguments: [String: Any] = ["playerId": session.id]
      if let configuration { arguments["configuration"] = configuration }
      plugin.handle(FlutterMethodCall(methodName: method, arguments: arguments)) { result in
        XCTAssertFalse(result is FlutterError, "\(String(describing: result))")
      }
      checkpoint(method)
    }

    let initialPosition = layer.player?.currentTime().seconds ?? 0
    command("setPictureInPictureConfiguration", configuration: ["enabled": true, "autoEnter": true])
    try await waitFor("System PiP did not become ready for automatic entry") {
      systemController?.isPictureInPicturePossible == true && layer.player?.timeControlStatus == .playing
        && (layer.player?.currentTime().seconds ?? 0) >= initialPosition + 3
    }
    checkpoint("readyForAutomaticEntry")
    marker("READY_AUTO_PIP")
    try await waitFor("Automatic system PiP did not start") {
      UIApplication.shared.applicationState == .background && session.pictureInPictureActive
    }
    XCTAssertEqual(activeEvents, 1)
    marker("AUTO_PIP_VERIFIED")
    try await waitFor("Host did not return to foreground") { UIApplication.shared.applicationState == .active }
    command("exitPictureInPicture")
    try await waitFor("System PiP did not exit") { session.pictureInPictureState == "inactive" }
    XCTAssertTrue(layer.isReadyForDisplay)

    command("requestPictureInPicture")
    try await waitFor("Manual system PiP did not start") { session.pictureInPictureActive }
    command("setPictureInPictureConfiguration", configuration: ["enabled": false, "autoEnter": false])
    try await waitFor("Disabling PiP did not stop the active system window") {
      session.pictureInPictureState == "inactive" && !session.pictureInPictureActive
    }
    controller.play()
    let previousActiveEvents = activeEvents
    marker("READY_DISABLED_PIP")
    try await waitFor("Host did not enter background") { UIApplication.shared.applicationState == .background }
    try await Task.sleep(for: .seconds(3))
    XCTAssertFalse(session.pictureInPictureActive)
    XCTAssertEqual(activeEvents, previousActiveEvents)
    marker("DISABLED_PIP_VERIFIED")
    try await waitFor("Host did not return to foreground") { UIApplication.shared.applicationState == .active }
    XCTAssertTrue(layer.isReadyForDisplay)

    controller.play()
    try await waitFor("Re-enabled inline playback did not resume") {
      layer.isReadyForDisplay && (layer.player?.rate ?? 0) > 0
    }
    command("setPictureInPictureConfiguration", configuration: ["enabled": true, "autoEnter": true])
    command("requestPictureInPicture")
    try await waitFor("Re-enabled manual PiP did not start") {
      session.pictureInPictureActive || session.pictureInPictureState == "failed"
    }
    guard session.pictureInPictureActive else {
      XCTFail("Re-enabled manual PiP failed")
      return
    }
    command("setPictureInPictureConfiguration", configuration: ["enabled": true, "autoEnter": false])
    command("exitPictureInPicture")
    try await waitFor("Manual PiP did not exit after disabling automatic entry") {
      session.pictureInPictureState == "inactive"
    }
    controller.play()
    let activeEventsBeforeAutoDisabled = activeEvents
    marker("READY_AUTO_DISABLED_PIP")
    try await waitFor("Host did not enter background") { UIApplication.shared.applicationState == .background }
    try await Task.sleep(for: .seconds(3))
    XCTAssertFalse(session.pictureInPictureActive)
    XCTAssertEqual(activeEvents, activeEventsBeforeAutoDisabled)
    marker("AUTO_DISABLED_PIP_VERIFIED")
    try await waitFor("Host did not return to foreground") { UIApplication.shared.applicationState == .active }
    try await waitFor("Inline picture did not resume") { layer.isReadyForDisplay }

    controller.play()
    command("setPictureInPictureConfiguration", configuration: ["enabled": true, "autoEnter": true])
    command("requestPictureInPicture")
    try await waitFor("Manual PiP did not restart") { session.pictureInPictureActive }
    command("exitPictureInPicture")
    try await waitFor("Manual PiP did not exit") { session.pictureInPictureState == "inactive" }
    command("setPictureInPictureConfiguration", configuration: ["enabled": true, "autoEnter": false])
    controller.play()
    try await waitFor("Inline playback did not resume after disabling automatic entry") {
      layer.isReadyForDisplay && (layer.player?.rate ?? 0) > 0
    }
    let activeEventsBeforeIdleDisable = activeEvents
    marker("READY_IDLE_AUTO_DISABLED_PIP")
    try await waitFor("Host did not enter background") { UIApplication.shared.applicationState == .background }
    try await Task.sleep(for: .seconds(3))
    XCTAssertFalse(session.pictureInPictureActive)
    XCTAssertEqual(activeEvents, activeEventsBeforeIdleDisable)
    marker("IDLE_AUTO_DISABLED_PIP_VERIFIED")
    try await waitFor("Host did not return to foreground") { UIApplication.shared.applicationState == .active }
    controller.play()
    command("requestPictureInPicture")
    try await waitFor("Manual PiP did not start with automatic entry disabled") { session.pictureInPictureActive }
    command("exitPictureInPicture")
    try await waitFor("Manual-only PiP did not exit") { session.pictureInPictureState == "inactive" }
    XCTAssertNil(controller.lastError)
  }

  private func marker(_ value: String) {
    print("[PiPDeviceAcceptance] \(value)")
    fflush(stdout)
  }

  @MainActor
  private func waitFor(_ message: String, condition: () -> Bool) async throws {
    let deadline = ContinuousClock.now.advanced(by: .seconds(45))
    while !condition(), ContinuousClock.now < deadline { try await Task.sleep(for: .milliseconds(100)) }
    guard condition() else {
      throw NSError(domain: "PictureInPictureDeviceTest", code: 1, userInfo: [NSLocalizedDescriptionKey: message])
    }
  }
}
