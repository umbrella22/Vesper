import AVFoundation
import Darwin
import Flutter
import MediaPlayer
import Photos
import UIKit
import VesperPlayerKit

@main
@objc class AppDelegate: FlutterAppDelegate, FlutterImplicitEngineDelegate, UIDocumentPickerDelegate {
  private var mediaPickerChannel: FlutterMethodChannel?
  private var deviceControlsChannel: FlutterMethodChannel?
  private var pendingVideoPickerResult: FlutterResult?
  private let volumeView = MPVolumeView(frame: CGRect(x: -1000, y: -1000, width: 1, height: 1))
  private weak var volumeSlider: UISlider?

  override func application(
    _ application: UIApplication,
    didFinishLaunchingWithOptions launchOptions: [UIApplication.LaunchOptionsKey: Any]?
  ) -> Bool {
    return super.application(application, didFinishLaunchingWithOptions: launchOptions)
  }

  func didInitializeImplicitFlutterEngine(_ engineBridge: FlutterImplicitEngineBridge) {
    let channel = FlutterMethodChannel(
      name: "io.github.umbrella22.vesper.example.flutter_host/media_picker",
      binaryMessenger: engineBridge.applicationRegistrar.messenger()
    )
    channel.setMethodCallHandler { [weak self] call, result in
      switch call.method {
      case "pickVideo":
        self?.presentVideoPicker(result: result)
      case "saveVideoToGallery":
        self?.handleSaveVideoToGallery(call: call, result: result)
      case "hdrEvidenceOutputRoot":
        result(self?.hdrEvidenceOutputRoot().path)
      case "hdrEvidenceDevice":
        result(self?.hdrEvidenceDevice())
      default:
        result(FlutterMethodNotImplemented)
      }
    }
    mediaPickerChannel = channel
    let deviceChannel = FlutterMethodChannel(
      name: "io.github.umbrella22.vesper.example.flutter_host/device_controls",
      binaryMessenger: engineBridge.applicationRegistrar.messenger()
    )
    deviceChannel.setMethodCallHandler { [weak self] call, result in
      self?.handleDeviceControl(call: call, result: result)
    }
    deviceControlsChannel = deviceChannel
    GeneratedPluginRegistrant.register(with: engineBridge.pluginRegistry)
  }

  func documentPicker(_ controller: UIDocumentPickerViewController, didPickDocumentsAt urls: [URL]) {
    guard let url = urls.first else {
      finishVideoPicker(with: nil)
      return
    }
    finishVideoPicker(
      with: [
        "uri": url.absoluteString,
        "label": url.lastPathComponent,
      ]
    )
  }

  func documentPickerWasCancelled(_ controller: UIDocumentPickerViewController) {
    finishVideoPicker(with: nil)
  }

  private func presentVideoPicker(result: @escaping FlutterResult) {
    guard pendingVideoPickerResult == nil else {
      result(
        FlutterError(
          code: "busy",
          message: "A media picker request is already active.",
          details: nil
        )
      )
      return
    }
    guard let presenter = topViewController() else {
      result(
        FlutterError(
          code: "picker_unavailable",
          message: "Unable to locate a presenter for the video picker.",
          details: nil
        )
      )
      return
    }

    pendingVideoPickerResult = result
    let picker = UIDocumentPickerViewController(
      documentTypes: ["public.movie", "public.video"],
      in: .import
    )
    picker.delegate = self
    picker.allowsMultipleSelection = false
    presenter.present(picker, animated: true)
  }

  private func finishVideoPicker(with value: Any?) {
    let result = pendingVideoPickerResult
    pendingVideoPickerResult = nil
    result?(value)
  }

  private func topViewController(base: UIViewController? = nil) -> UIViewController? {
    let rootController = base ?? activeRootViewController()
    if let navigationController = rootController as? UINavigationController {
      return topViewController(base: navigationController.visibleViewController)
    }
    if let tabBarController = rootController as? UITabBarController,
      let selectedViewController = tabBarController.selectedViewController
    {
      return topViewController(base: selectedViewController)
    }
    if let presentedViewController = rootController?.presentedViewController {
      return topViewController(base: presentedViewController)
    }
    return rootController
  }

  private func handleDeviceControl(call: FlutterMethodCall, result: @escaping FlutterResult) {
    Task { @MainActor [weak self] in
      self?.handleDeviceControlOnMain(call: call, result: result)
    }
  }

  @MainActor
  private func handleDeviceControlOnMain(
    call: FlutterMethodCall,
    result: @escaping FlutterResult
  ) {
    switch call.method {
    case "getBrightness":
      result(Double(UIScreen.main.brightness).clampedToUnit())
    case "setBrightness":
      guard let ratio = ratioArgument(from: call) else {
        result(
          FlutterError(
            code: "invalid_argument",
            message: "Missing brightness ratio.",
            details: nil
          )
        )
        return
      }
      let nextRatio = CGFloat(ratio.clamped(min: 0.02, max: 1))
      UIScreen.main.brightness = nextRatio
      result(Double(UIScreen.main.brightness).clampedToUnit())
    case "getVolume":
      result(currentVolumeRatio())
    case "setVolume":
      guard let ratio = ratioArgument(from: call) else {
        result(
          FlutterError(
            code: "invalid_argument",
            message: "Missing volume ratio.",
            details: nil
          )
        )
        return
      }
      result(setVolumeRatio(ratio))
    case "subtitleOverlaySnapshot":
      handleSubtitleOverlayEvidence(call: call, captureImage: false, result: result)
    case "captureSubtitleOverlayEvidence":
      handleSubtitleOverlayEvidence(call: call, captureImage: true, result: result)
    default:
      result(FlutterMethodNotImplemented)
    }
  }

  private func handleSubtitleOverlayEvidence(
    call: FlutterMethodCall,
    captureImage: Bool,
    result: @escaping FlutterResult
  ) {
    guard
      let arguments = call.arguments as? [String: Any],
      let playerId = arguments["playerId"] as? String,
      !playerId.isEmpty
    else {
      result(
        FlutterError(
          code: "invalid_argument",
          message: "Missing playerId for subtitle overlay evidence.",
          details: nil
        )
      )
      return
    }
    let windows = applicationWindows()
    let surfaceIdentifier = "io.github.umbrella22.vesper.player.surface.\(playerId)"
    let surfaceMarkerIdentifier =
      "io.github.umbrella22.vesper.player.surface-marker.\(playerId)"
    let directSurface = windows.lazy.compactMap({ window in
      self.descendantView(in: window, accessibilityIdentifier: surfaceIdentifier)
    }).first
    let markedSurface = windows.lazy.compactMap({ window in
      self.descendantView(in: window, accessibilityIdentifier: surfaceMarkerIdentifier)?
        .superview
    }).first
    let fallbackSurfaces = windows.flatMap { window in
      self.descendantViews(in: window) { $0 is PlayerSurfaceView }
    }
    let uniqueFallbackSurface = fallbackSurfaces.count == 1 ? fallbackSurfaces[0] : nil
    guard
      let surface = directSurface ?? markedSurface ?? uniqueFallbackSurface,
      let subtitleLabel = descendantView(
        in: surface,
        accessibilityIdentifier: "io.github.umbrella22.vesper.player.subtitle-overlay"
      ) as? UILabel
    else {
      result(
        FlutterError(
          code: "subtitle_overlay_unavailable",
          message: "Unable to locate the subtitle overlay for playerId=\(playerId).",
          details: [
            "windowCount": windows.count,
            "surfaceIdentifier": surfaceIdentifier,
            "surfaceMarkerIdentifier": surfaceMarkerIdentifier,
            "visibleAccessibilityIdentifiers": windows.flatMap { window in
              self.descendantAccessibilityIdentifiers(in: window, remaining: 64)
            },
          ]
        )
      )
      return
    }

    surface.layoutIfNeeded()
    subtitleLabel.layoutIfNeeded()
    let snapshot = subtitleOverlaySnapshot(from: subtitleLabel)
    guard captureImage else {
      result(snapshot)
      return
    }
    guard surface.bounds.width > 0, surface.bounds.height > 0 else {
      result(
        FlutterError(
          code: "subtitle_overlay_unavailable",
          message: "The subtitle surface has an empty frame.",
          details: snapshot
        )
      )
      return
    }

    var didDrawHierarchy = false
    let renderer = UIGraphicsImageRenderer(bounds: surface.bounds)
    let image = renderer.image { _ in
      didDrawHierarchy = surface.drawHierarchy(
        in: surface.bounds,
        afterScreenUpdates: true
      )
    }
    guard didDrawHierarchy, let png = image.pngData() else {
      result(
        FlutterError(
          code: "subtitle_overlay_capture_failed",
          message: "UIKit could not capture the subtitle surface.",
          details: snapshot
        )
      )
      return
    }
    result([
      "snapshot": snapshot,
      "png": FlutterStandardTypedData(bytes: png),
    ])
  }

  private func descendantView(
    in view: UIView,
    accessibilityIdentifier: String
  ) -> UIView? {
    if view.accessibilityIdentifier == accessibilityIdentifier {
      return view
    }
    for subview in view.subviews {
      if let match = descendantView(
        in: subview,
        accessibilityIdentifier: accessibilityIdentifier
      ) {
        return match
      }
    }
    return nil
  }

  private func descendantAccessibilityIdentifiers(
    in view: UIView,
    remaining: Int
  ) -> [String] {
    guard remaining > 0 else { return [] }
    var identifiers: [String] = []
    if let identifier = view.accessibilityIdentifier, !identifier.isEmpty {
      identifiers.append(identifier)
    }
    for subview in view.subviews where identifiers.count < remaining {
      identifiers.append(
        contentsOf: descendantAccessibilityIdentifiers(
          in: subview,
          remaining: remaining - identifiers.count
        )
      )
    }
    return identifiers
  }

  private func descendantViews(
    in view: UIView,
    matching predicate: (UIView) -> Bool
  ) -> [UIView] {
    var matches: [UIView] = []
    if predicate(view) {
      matches.append(view)
    }
    for subview in view.subviews {
      matches.append(contentsOf: descendantViews(in: subview, matching: predicate))
    }
    return matches
  }

  private func applicationWindows() -> [UIWindow] {
    let scenes = UIApplication.shared.connectedScenes.compactMap { $0 as? UIWindowScene }
    return scenes.flatMap(\.windows).sorted { lhs, rhs in
      if lhs.isKeyWindow != rhs.isKeyWindow {
        return lhs.isKeyWindow
      }
      return lhs.windowLevel.rawValue < rhs.windowLevel.rawValue
    }
  }

  private func subtitleOverlaySnapshot(from label: UILabel) -> [String: Any] {
    let text = label.text ?? ""
    let frame = label.frame
    let windowAttached = label.window != nil
    return [
      "text": text,
      "hidden": label.isHidden,
      "alpha": Double(label.alpha),
      "windowAttached": windowAttached,
      "frame": [
        "x": Double(frame.origin.x),
        "y": Double(frame.origin.y),
        "width": Double(frame.width),
        "height": Double(frame.height),
      ],
      "visible": !text.isEmpty
        && !label.isHidden
        && label.alpha > 0
        && windowAttached
        && frame.width > 0
        && frame.height > 0,
    ]
  }

  private func ratioArgument(from call: FlutterMethodCall) -> Double? {
    guard
      let arguments = call.arguments as? [String: Any],
      let ratio = arguments["ratio"] as? NSNumber
    else {
      return nil
    }
    return ratio.doubleValue
  }

  private func currentVolumeRatio() -> Double? {
    prepareVolumeViewIfNeeded()
    if let volumeSlider {
      return Double(volumeSlider.value).clampedToUnit()
    }
    return Double(AVAudioSession.sharedInstance().outputVolume).clampedToUnit()
  }

  private func setVolumeRatio(_ ratio: Double) -> Double? {
    prepareVolumeViewIfNeeded()
    guard let volumeSlider else {
      return currentVolumeRatio()
    }
    let nextRatio = Float(ratio.clampedToUnit())
    volumeSlider.setValue(nextRatio, animated: false)
    volumeSlider.sendActions(for: .valueChanged)
    volumeSlider.sendActions(for: .touchUpInside)
    return Double(volumeSlider.value).clampedToUnit()
  }

  private func prepareVolumeViewIfNeeded() {
    try? AVAudioSession.sharedInstance().setActive(true)
    if volumeView.superview == nil {
      volumeView.showsVolumeSlider = true
      volumeView.alpha = 0.01
      activeRootViewController()?.view.addSubview(volumeView)
    }
    volumeSlider = volumeView.subviews.compactMap { $0 as? UISlider }.first
  }

  private func activeRootViewController() -> UIViewController? {
    let scenes = UIApplication.shared.connectedScenes.compactMap { $0 as? UIWindowScene }
    let activeScene = scenes.first { $0.activationState == .foregroundActive } ?? scenes.first
    let keyWindow = activeScene?.windows.first { $0.isKeyWindow }
    return keyWindow?.rootViewController
  }

  private func handleSaveVideoToGallery(call: FlutterMethodCall, result: @escaping FlutterResult) {
    guard
      let arguments = call.arguments as? [String: Any],
      let completedPath = (arguments["completedPath"] as? String)?
        .trimmingCharacters(in: .whitespacesAndNewlines),
      !completedPath.isEmpty
    else {
      result(
        FlutterError(
          code: "invalid_argument",
          message: "The completed download output is unavailable.",
          details: nil
        )
      )
      return
    }

    Task {
      do {
        try await saveVideoToPhotoLibrary(completedPath: completedPath)
        await MainActor.run {
          result(nil)
        }
      } catch {
        await MainActor.run {
          result(
            FlutterError(
              code: "save_failed",
              message: error.localizedDescription,
              details: nil
            )
          )
        }
      }
    }
  }

  private func saveVideoToPhotoLibrary(completedPath: String) async throws {
    let fileURL = resolveCompletedFileURL(from: completedPath)
    guard FileManager.default.fileExists(atPath: fileURL.path) else {
      throw ExamplePhotoLibraryExportError.missingCompletedFile
    }

    let authorizationStatus = await requestPhotoLibraryAuthorization()
    switch authorizationStatus {
    case .authorized, .limited:
      break
    case .denied, .restricted:
      throw ExamplePhotoLibraryExportError.accessDenied
    case .notDetermined:
      throw ExamplePhotoLibraryExportError.accessDenied
    @unknown default:
      throw ExamplePhotoLibraryExportError.accessDenied
    }

    try await withCheckedThrowingContinuation { (continuation: CheckedContinuation<Void, Error>) in
      PHPhotoLibrary.shared().performChanges {
        let request = PHAssetCreationRequest.forAsset()
        request.addResource(with: .video, fileURL: fileURL, options: nil)
      } completionHandler: { success, error in
        if let error {
          continuation.resume(throwing: error)
          return
        }
        guard success else {
          continuation.resume(throwing: ExamplePhotoLibraryExportError.saveFailed)
          return
        }
        continuation.resume(returning: ())
      }
    }
  }

  private func requestPhotoLibraryAuthorization() async -> PHAuthorizationStatus {
    if #available(iOS 14, *) {
      return await withCheckedContinuation { continuation in
        PHPhotoLibrary.requestAuthorization(for: .addOnly) { status in
          continuation.resume(returning: status)
        }
      }
    }

    return await withCheckedContinuation { continuation in
      PHPhotoLibrary.requestAuthorization { status in
        continuation.resume(returning: status)
      }
    }
  }

  private func resolveCompletedFileURL(from completedPath: String) -> URL {
    if completedPath.hasPrefix("file://"), let fileURL = URL(string: completedPath), fileURL.isFileURL {
      return fileURL
    }
    return URL(fileURLWithPath: completedPath)
  }

  private func hdrEvidenceOutputRoot() -> URL {
    let documents =
      FileManager.default.urls(for: .documentDirectory, in: .userDomainMask).first
      ?? URL(fileURLWithPath: NSTemporaryDirectory())
    let root = documents.appendingPathComponent("hdr-dv-evidence", isDirectory: true)
    try? FileManager.default.createDirectory(at: root, withIntermediateDirectories: true)
    return root
  }

  private func hdrEvidenceDevice() -> [String: Any] {
    let screen = UIScreen.main
    return [
      "ios": [
        "model": deviceModelIdentifier(),
        "iosVersion": UIDevice.current.systemVersion,
        "avPlayerEligibleForHdrPlayback": AVPlayer.eligibleForHDRPlayback,
        "displayGamut": displayGamutName(screen.traitCollection.displayGamut),
        "nativeDisplaySize": [
          "width": Int(screen.nativeBounds.width.rounded()),
          "height": Int(screen.nativeBounds.height.rounded()),
        ],
        "maximumFramesPerSecond": screen.maximumFramesPerSecond,
      ],
    ]
  }

  private func displayGamutName(_ gamut: UIDisplayGamut) -> String {
    switch gamut {
    case .P3:
      return "P3"
    case .SRGB:
      return "SRGB"
    case .unspecified:
      return "unspecified"
    @unknown default:
      return "unknown"
    }
  }

  private func deviceModelIdentifier() -> String {
    var systemInfo = utsname()
    uname(&systemInfo)
    return withUnsafePointer(to: &systemInfo.machine) { pointer in
      pointer.withMemoryRebound(to: CChar.self, capacity: 1) { String(cString: $0) }
    }
  }
}

private enum ExamplePhotoLibraryExportError: LocalizedError {
  case missingCompletedFile
  case accessDenied
  case saveFailed

  var errorDescription: String? {
    switch self {
    case .missingCompletedFile:
      return "The completed download output is unavailable."
    case .accessDenied:
      return "Photo Library add access is required to save videos."
    case .saveFailed:
      return "Failed to save the downloaded video to Photos."
    }
  }
}

private extension Double {
  func clamped(min lowerBound: Double, max upperBound: Double) -> Double {
    Swift.min(Swift.max(self, lowerBound), upperBound)
  }

  func clampedToUnit() -> Double {
    clamped(min: 0, max: 1)
  }
}
