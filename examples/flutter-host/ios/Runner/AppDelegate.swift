import Flutter
import Photos
import UIKit

@main
@objc class AppDelegate: FlutterAppDelegate, FlutterImplicitEngineDelegate, UIDocumentPickerDelegate {
  private var mediaPickerChannel: FlutterMethodChannel?
  private var pendingVideoPickerResult: FlutterResult?

  override func application(
    _ application: UIApplication,
    didFinishLaunchingWithOptions launchOptions: [UIApplication.LaunchOptionsKey: Any]?
  ) -> Bool {
    return super.application(application, didFinishLaunchingWithOptions: launchOptions)
  }

  func didInitializeImplicitFlutterEngine(_ engineBridge: FlutterImplicitEngineBridge) {
    let channel = FlutterMethodChannel(
      name: "io.github.ikaros.vesper.example.flutter_host/media_picker",
      binaryMessenger: engineBridge.applicationRegistrar.messenger()
    )
    channel.setMethodCallHandler { [weak self] call, result in
      switch call.method {
      case "pickVideo":
        self?.presentVideoPicker(result: result)
      case "bundledDownloadPluginLibraryPaths":
        result(self?.bundledDownloadPluginLibraryPaths() ?? [])
      case "saveVideoToGallery":
        self?.handleSaveVideoToGallery(call: call, result: result)
      default:
        result(FlutterMethodNotImplemented)
      }
    }
    mediaPickerChannel = channel
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

  private func activeRootViewController() -> UIViewController? {
    let scenes = UIApplication.shared.connectedScenes.compactMap { $0 as? UIWindowScene }
    let activeScene = scenes.first { $0.activationState == .foregroundActive } ?? scenes.first
    let keyWindow = activeScene?.windows.first { $0.isKeyWindow }
    return keyWindow?.rootViewController
  }

  private func bundledDownloadPluginLibraryPaths() -> [String] {
    let fileManager = FileManager.default
    let frameworksPath = Bundle.main.privateFrameworksPath ?? Bundle.main.bundlePath + "/Frameworks"
    let candidates = [
      frameworksPath + "/vesper_player_ios.framework/libplayer_remux_ffmpeg.dylib",
      frameworksPath + "/VesperPlayerKit.framework/libplayer_remux_ffmpeg.dylib",
      frameworksPath + "/libplayer_remux_ffmpeg.dylib",
      Bundle.main.bundlePath + "/libplayer_remux_ffmpeg.dylib",
    ]

    return candidates.compactMap { candidate in
      guard fileManager.fileExists(atPath: candidate) else {
        return nil
      }
      return candidate
    }
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
