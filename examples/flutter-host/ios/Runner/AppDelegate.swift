import Flutter
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
      guard call.method == "pickVideo" else {
        result(FlutterMethodNotImplemented)
        return
      }
      self?.presentVideoPicker(result: result)
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
}
