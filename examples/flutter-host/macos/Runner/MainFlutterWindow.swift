import Cocoa
import FlutterMacOS

class MainFlutterWindow: NSWindow {
  private var mediaPickerChannel: FlutterMethodChannel?

  override func awakeFromNib() {
    let flutterViewController = FlutterViewController()
    let windowFrame = self.frame
    self.contentViewController = flutterViewController
    self.setFrame(windowFrame, display: true)

    RegisterGeneratedPlugins(registry: flutterViewController)
    let channel = FlutterMethodChannel(
      name: "io.github.ikaros.vesper.example.flutter_host/media_picker",
      binaryMessenger: flutterViewController.engine.binaryMessenger
    )
    channel.setMethodCallHandler { [weak self] call, result in
      switch call.method {
      case "pickVideo":
        self?.presentVideoPicker(result: result)
      case "bundledDownloadPluginLibraryPaths":
        result(self?.bundledDownloadPluginLibraryPaths() ?? [])
      case "saveVideoToGallery":
        result(
          FlutterError(
            code: "unsupported",
            message: "The macOS host does not export downloads to the system photo library.",
            details: nil
          )
        )
      default:
        result(FlutterMethodNotImplemented)
      }
    }
    mediaPickerChannel = channel

    super.awakeFromNib()
  }

  private func presentVideoPicker(result: @escaping FlutterResult) {
    let panel = NSOpenPanel()
    panel.allowedFileTypes = ["mp4", "mov", "m4v", "mkv", "avi", "webm", "ts", "m3u8", "mpd"]
    panel.canChooseFiles = true
    panel.canChooseDirectories = false
    panel.allowsMultipleSelection = false
    panel.beginSheetModal(for: self) { response in
      guard response == .OK, let url = panel.url else {
        result(nil)
        return
      }
      result(
        [
          "uri": url.absoluteString,
          "label": url.lastPathComponent,
        ]
      )
    }
  }

  private func bundledDownloadPluginLibraryPaths() -> [String] {
    let fileManager = FileManager.default
    let candidates = [
      Bundle.main.privateFrameworksPath?.appending("/libplayer_ffmpeg.dylib"),
      Bundle.main.bundlePath + "/Frameworks/libplayer_ffmpeg.dylib",
      Bundle.main.bundlePath + "/libplayer_ffmpeg.dylib",
    ]

    return candidates.compactMap { candidate in
      guard let candidate, fileManager.fileExists(atPath: candidate) else {
        return nil
      }
      return candidate
    }
  }
}
