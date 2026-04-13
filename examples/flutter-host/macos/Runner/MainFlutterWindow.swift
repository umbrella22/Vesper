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
      guard call.method == "pickVideo" else {
        result(FlutterMethodNotImplemented)
        return
      }
      self?.presentVideoPicker(result: result)
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
}
