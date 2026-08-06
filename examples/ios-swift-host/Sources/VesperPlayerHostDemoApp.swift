import SwiftUI
import VesperPlayerKit

@main
struct VesperPlayerHostDemoApp: App {
    var body: some Scene {
        WindowGroup {
            VesperPlayerHostRootView()
        }
    }
}

private struct VesperPlayerHostRootView: View {
    @State private var downloadManager: VesperDownloadManager?
    @State private var isDownloadExportPluginInstalled = false
    @State private var startupError: String?

    var body: some View {
        Group {
            if let downloadManager {
                PlayerHostView(
                    downloadManager: downloadManager,
                    isDownloadExportPluginInstalled: isDownloadExportPluginInstalled
                )
            } else if let startupError {
                ContentUnavailableView(
                    "Player unavailable",
                    systemImage: "exclamationmark.triangle",
                    description: Text(startupError)
                )
            } else {
                ProgressView()
            }
        }
        .task {
            guard downloadManager == nil, startupError == nil else {
                return
            }
            do {
                do {
                    downloadManager = try VesperDownloadManager(
                        configuration: VesperDownloadConfiguration(
                            runPostProcessorsOnCompletion: false,
                            postDownloadPluginReferences: bundledDownloadPluginReferences()
                        )
                    )
                    isDownloadExportPluginInstalled = true
                } catch {
                    downloadManager = try VesperDownloadManager(
                        configuration: VesperDownloadConfiguration(
                            runPostProcessorsOnCompletion: false
                        )
                    )
                    isDownloadExportPluginInstalled = false
                }
            } catch {
                startupError = error.localizedDescription
            }
        }
    }
}
