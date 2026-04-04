import Photos
import PhotosUI
import SwiftUI
import UIKit
import UniformTypeIdentifiers
import VesperPlayerKit

struct PlayerHostView: View {
    @Environment(\.colorScheme) private var systemColorScheme
    @Environment(\.horizontalSizeClass) private var horizontalSizeClass

    @AppStorage("vesper.example.ios.theme_mode") private var themeModeRaw = ExampleThemeMode.system.rawValue
    @StateObject private var controller: VesperPlayerController
    @State private var pendingSeekRatio: Double?
    @State private var isVideoPickerPresented = false
    @State private var hostMessage: String?
    @State private var remoteStreamUrl = IOS_HLS_DEMO_URL
    @State private var controlsVisible = true
    @State private var activeSheet: ExamplePlayerSheet?
    @State private var isFullscreen = false
    @State private var controlsHideTask: Task<Void, Never>?

    init() {
        _controller = StateObject(
            wrappedValue: VesperPlayerControllerFactory.makeDefault(
                initialSource: iosHlsDemoSource()
            )
        )
    }

    private var themeMode: ExampleThemeMode {
        get { ExampleThemeMode(rawValue: themeModeRaw) ?? .system }
        set { themeModeRaw = newValue.rawValue }
    }

    private var useDarkTheme: Bool {
        switch themeMode {
        case .system:
            systemColorScheme == .dark
        case .light:
            false
        case .dark:
            true
        }
    }

    private var isCompactLayout: Bool {
        horizontalSizeClass != .regular
    }

    var body: some View {
        let palette = exampleHostPalette(useDarkTheme: useDarkTheme)
        let uiState = controller.uiState
        let trackCatalog = controller.trackCatalog
        let trackSelection = controller.trackSelection

        ZStack {
            LinearGradient(
                colors: [palette.pageTop, palette.pageBottom],
                startPoint: .top,
                endPoint: .bottom
            )
            .ignoresSafeArea()

            if isFullscreen {
                Color.black.ignoresSafeArea()

                playerStage(uiState: uiState)
                    .frame(maxWidth: .infinity, maxHeight: .infinity)
                    .background(Color.black)
                    .ignoresSafeArea()
            } else {
                ScrollView {
                    VStack(alignment: .leading, spacing: 18) {
                        ExamplePlayerHeader(
                            sourceLabel: uiState.sourceLabel,
                            subtitle: uiState.subtitle,
                            palette: palette
                        )

                        playerStage(uiState: uiState)
                            .frame(height: 248)

                        ExampleSourceSection(
                            palette: palette,
                            themeMode: themeMode,
                            remoteStreamUrl: $remoteStreamUrl,
                            hostMessage: hostMessage,
                            onThemeModeChange: { themeModeRaw = $0.rawValue },
                            onPickFromPhotos: {
                                requestPhotoLibraryAccessAndPresentPicker()
                            },
                            onUseHlsDemo: {
                                hostMessage = nil
                                controller.selectSource(iosHlsDemoSource())
                                controlsVisible = true
                            },
                            onOpenRemote: {
                                openRemoteSource()
                            }
                        )
                    }
                    .padding(20)
                }
            }
        }
        .preferredColorScheme(themeMode.preferredColorScheme)
        .statusBarHidden(isFullscreen)
        .persistentSystemOverlays(isFullscreen ? .hidden : .visible)
        .onAppear {
            controller.initialize()
            scheduleControlsAutoHide(for: uiState)
        }
        .onDisappear {
            controlsHideTask?.cancel()
            controller.dispose()
        }
        .onChange(of: uiState.playbackState) { _, _ in
            scheduleControlsAutoHide(for: controller.uiState)
        }
        .onChange(of: uiState.isBuffering) { _, _ in
            scheduleControlsAutoHide(for: controller.uiState)
        }
        .onChange(of: controlsVisible) { _, _ in
            scheduleControlsAutoHide(for: controller.uiState)
        }
        .onChange(of: activeSheet) { _, _ in
            scheduleControlsAutoHide(for: controller.uiState)
        }
        .onChange(of: pendingSeekRatio) { _, _ in
            scheduleControlsAutoHide(for: controller.uiState)
        }
        .sheet(isPresented: $isVideoPickerPresented) {
            ExampleVideoPicker { selection in
                isVideoPickerPresented = false
                guard let selection else { return }
                hostMessage = ExampleI18n.preparingVideoFromPhotos
                Task(priority: .userInitiated) {
                    try? await Task.sleep(for: .milliseconds(160))
                    await handlePickedVideoSelection(selection)
                }
            }
        }
        .sheet(item: $activeSheet) { sheet in
            ExampleSelectionSheetContent(
                sheet: sheet,
                uiState: uiState,
                trackCatalog: trackCatalog,
                trackSelection: trackSelection,
                onOpenSheet: { activeSheet = $0 },
                onSelectQuality: {
                    controller.setAbrPolicy($0)
                    activeSheet = nil
                },
                onSelectAudio: {
                    controller.setAudioTrackSelection($0)
                    activeSheet = nil
                },
                onSelectSubtitle: {
                    controller.setSubtitleTrackSelection($0)
                    activeSheet = nil
                },
                onSelectSpeed: {
                    controller.setPlaybackRate($0)
                    activeSheet = nil
                }
            )
            .presentationDetents([.height(sheetHeight(for: sheet))])
            .presentationDragIndicator(.hidden)
        }
    }

    @ViewBuilder
    private func playerStage(uiState: PlayerHostUiState) -> some View {
        ExamplePlayerStage(
            surface: AnyView(PlayerSurfaceContainer(controller: controller)),
            uiState: uiState,
            controlsVisible: $controlsVisible,
            pendingSeekRatio: $pendingSeekRatio,
            isCompactLayout: isCompactLayout,
            isFullscreen: isFullscreen,
            onSeekBy: { controller.seek(by: $0) },
            onTogglePause: { controller.togglePause() },
            onSeekToRatio: { controller.seek(toRatio: $0) },
            onSeekToLiveEdge: { controller.seekToLiveEdge() },
            onToggleFullscreen: {
                setFullscreen(!isFullscreen)
            },
            onOpenSheet: { activeSheet = $0 }
        )
    }

    private func openRemoteSource() {
        let trimmed = remoteStreamUrl.trimmingCharacters(in: .whitespacesAndNewlines)
        guard let url = URL(string: trimmed), !trimmed.isEmpty else {
            hostMessage = ExampleI18n.invalidRemoteUrl
            return
        }
        hostMessage = nil
        controller.selectSource(VesperPlayerSource.remoteUrl(url, label: ExampleI18n.customRemoteUrlLabel))
        controlsVisible = true
    }

    private func setFullscreen(_ fullscreen: Bool) {
        withAnimation(.easeInOut(duration: 0.2)) {
            isFullscreen = fullscreen
        }

        Task { @MainActor in
            updateInterfaceOrientation(forFullscreen: fullscreen)
        }
    }

    @MainActor
    private func updateInterfaceOrientation(forFullscreen fullscreen: Bool) {
        let requestedOrientations: UIInterfaceOrientationMask = fullscreen ? .landscapeRight : .portrait

        guard
            let windowScene = UIApplication.shared.connectedScenes
                .compactMap({ $0 as? UIWindowScene })
                .first(where: { $0.activationState == .foregroundActive || $0.activationState == .foregroundInactive })
        else {
            return
        }

        windowScene.keyWindow?.rootViewController?.setNeedsUpdateOfSupportedInterfaceOrientations()

        windowScene.requestGeometryUpdate(.iOS(interfaceOrientations: requestedOrientations)) { error in
            exampleIosHostLog("interface orientation update failed: \(error.localizedDescription)")
        }
    }

    private func scheduleControlsAutoHide(for uiState: PlayerHostUiState) {
        controlsHideTask?.cancel()
        guard
            uiState.playbackState == .playing,
            !uiState.isBuffering,
            controlsVisible,
            activeSheet == nil,
            pendingSeekRatio == nil
        else {
            return
        }

        controlsHideTask = Task { @MainActor in
            try? await Task.sleep(for: .seconds(3))
            guard
                !Task.isCancelled,
                controller.uiState.playbackState == .playing,
                !controller.uiState.isBuffering,
                activeSheet == nil,
                pendingSeekRatio == nil
            else {
                return
            }
            controlsVisible = false
        }
    }

    private func requestPhotoLibraryAccessAndPresentPicker() {
        let status = PHPhotoLibrary.authorizationStatus(for: .readWrite)
        switch status {
        case .authorized, .limited:
            hostMessage = nil
            exampleIosHostLog("photo library access available: \(status.rawValue)")
            isVideoPickerPresented = true
        case .notDetermined:
            exampleIosHostLog("requesting photo library access")
            Task {
                let result = await PHPhotoLibrary.requestAuthorization(for: .readWrite)
                await MainActor.run {
                    handlePhotoAuthorizationStatus(result)
                }
            }
        case .denied, .restricted:
            hostMessage = ExampleI18n.photoLibraryAccessRequired
            exampleIosHostLog("photo library access denied: \(status.rawValue)")
        @unknown default:
            hostMessage = ExampleI18n.unknownPhotoAuthorizationState
            exampleIosHostLog("photo library access unknown")
        }
    }

    private func handlePhotoAuthorizationStatus(_ status: PHAuthorizationStatus) {
        switch status {
        case .authorized, .limited:
            hostMessage = nil
            exampleIosHostLog("photo library access granted: \(status.rawValue)")
            isVideoPickerPresented = true
        case .denied, .restricted:
            hostMessage = ExampleI18n.photoLibraryAccessRequired
            exampleIosHostLog("photo library access denied after prompt: \(status.rawValue)")
        case .notDetermined:
            exampleIosHostLog("photo library access still not determined")
        @unknown default:
            hostMessage = ExampleI18n.unknownPhotoAuthorizationState
            exampleIosHostLog("photo library access unknown after prompt")
        }
    }

    private func handlePickedVideoSelection(_ selection: ExamplePickedVideoSelection) async {
        do {
            if let resolved = try await resolvePickedVideo(selection) {
                await MainActor.run {
                    hostMessage = nil
                    controlsVisible = true
                }
                try? await Task.sleep(for: .milliseconds(120))
                await MainActor.run {
                    hostMessage = nil
                    exampleIosHostLog("picked photo video url=\(resolved.url.absoluteString)")
                    controller.selectSource(.localFile(url: resolved.url, label: resolved.label))
                    controlsVisible = true
                }
            } else {
                await MainActor.run {
                    hostMessage = ExampleI18n.failedToLoadSelectedVideoFromPhotos
                    exampleIosHostLog("picked photo video returned nil provider payload")
                }
            }
        } catch {
            await MainActor.run {
                hostMessage = ExampleI18n.failedToLoadSelectedPhotoVideo(error.localizedDescription)
                exampleIosHostLog("picked photo video failed: \(error.localizedDescription)")
            }
        }
    }

    private func resolvePickedVideo(_ selection: ExamplePickedVideoSelection) async throws -> (url: URL, label: String)? {
        if let file = try await loadProviderVideoFile(selection.itemProvider) {
            return file
        }

        if let identifier = selection.assetIdentifier,
           let original = await resolveOriginalPhotoVideo(identifier: identifier) {
            return original
        }

        return nil
    }

    private func loadProviderVideoFile(_ itemProvider: NSItemProvider) async throws -> (url: URL, label: String)? {
        let typeIdentifier: String
        if itemProvider.hasItemConformingToTypeIdentifier(UTType.movie.identifier) {
            typeIdentifier = UTType.movie.identifier
        } else if itemProvider.hasItemConformingToTypeIdentifier(UTType.video.identifier) {
            typeIdentifier = UTType.video.identifier
        } else {
            return nil
        }

        return try await withCheckedThrowingContinuation { continuation in
            itemProvider.loadFileRepresentation(forTypeIdentifier: typeIdentifier) { url, error in
                if let error {
                    continuation.resume(throwing: error)
                    return
                }

                guard let url else {
                    continuation.resume(returning: nil)
                    return
                }

                do {
                    let persistedUrl = try persistPickedVideoFile(from: url)
                    continuation.resume(returning: (persistedUrl, persistedUrl.lastPathComponent))
                } catch {
                    continuation.resume(throwing: error)
                }
            }
        }
    }

    private func persistPickedVideoFile(from url: URL) throws -> URL {
        let fileExtension = url.pathExtension.isEmpty ? "mov" : url.pathExtension
        let destination = FileManager.default.temporaryDirectory
            .appendingPathComponent(UUID().uuidString)
            .appendingPathExtension(fileExtension)

        if FileManager.default.fileExists(atPath: destination.path) {
            try FileManager.default.removeItem(at: destination)
        }

        do {
            try FileManager.default.moveItem(at: url, to: destination)
        } catch {
            try FileManager.default.copyItem(at: url, to: destination)
        }

        return destination
    }

    private func resolveOriginalPhotoVideo(identifier: String) async -> (url: URL, label: String)? {
        let fetchResult = PHAsset.fetchAssets(withLocalIdentifiers: [identifier], options: nil)
        guard let asset = fetchResult.firstObject else {
            exampleIosHostLog("failed to fetch PHAsset for identifier=\(identifier)")
            return nil
        }

        let filename = PHAssetResource.assetResources(for: asset).first?.originalFilename ?? identifier
        exampleIosHostLog("requesting original PHAsset video identifier=\(identifier) filename=\(filename)")

        return await withCheckedContinuation { continuation in
            let options = PHVideoRequestOptions()
            options.version = .original
            options.deliveryMode = .highQualityFormat
            options.isNetworkAccessAllowed = true

            PHImageManager.default().requestAVAsset(forVideo: asset, options: options) { avAsset, _, info in
                if let error = info?[PHImageErrorKey] as? NSError {
                    exampleIosHostLog("requestAVAsset failed: \(error.localizedDescription)")
                }
                if let urlAsset = avAsset as? AVURLAsset {
                    exampleIosHostLog("resolved original PHAsset video url=\(urlAsset.url.absoluteString)")
                    continuation.resume(returning: (urlAsset.url, filename))
                } else {
                    if avAsset != nil {
                        exampleIosHostLog("requestAVAsset returned non-URL asset, falling back to transferable copy")
                    }
                    continuation.resume(returning: nil)
                }
            }
        }
    }
}

private extension UIWindowScene {
    var keyWindow: UIWindow? {
        windows.first(where: \.isKeyWindow)
    }
}

private struct ExamplePickedVideoSelection {
    let assetIdentifier: String?
    let itemProvider: NSItemProvider
}

private struct ExampleVideoPicker: UIViewControllerRepresentable {
    let onPick: (ExamplePickedVideoSelection?) -> Void

    func makeCoordinator() -> Coordinator {
        Coordinator(onPick: onPick)
    }

    func makeUIViewController(context: Context) -> PHPickerViewController {
        var configuration = PHPickerConfiguration(photoLibrary: .shared())
        configuration.filter = .videos
        configuration.selectionLimit = 1
        configuration.preferredAssetRepresentationMode = .current

        let picker = PHPickerViewController(configuration: configuration)
        picker.delegate = context.coordinator
        return picker
    }

    func updateUIViewController(_ uiViewController: PHPickerViewController, context: Context) {}

    final class Coordinator: NSObject, PHPickerViewControllerDelegate {
        let onPick: (ExamplePickedVideoSelection?) -> Void

        init(onPick: @escaping (ExamplePickedVideoSelection?) -> Void) {
            self.onPick = onPick
        }

        func picker(_ picker: PHPickerViewController, didFinishPicking results: [PHPickerResult]) {
            let selection = results.first.map {
                ExamplePickedVideoSelection(
                    assetIdentifier: $0.assetIdentifier,
                    itemProvider: $0.itemProvider
                )
            }

            picker.dismiss(animated: true) {
                self.onPick(selection)
            }
        }
    }
}
