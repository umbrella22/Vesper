import AVFoundation
import Photos
import PhotosUI
import VesperPlayerKit
import SwiftUI
import UniformTypeIdentifiers

struct PlayerHostView: View {
    @StateObject private var controller: VesperPlayerController
    @State private var pendingSeekRatio: Double?
    @State private var isImporterPresented = false
    @State private var isPhotoPickerPresented = false
    @State private var selectedPhotoItem: PhotosPickerItem?
    @State private var hostMessage: String?
    @State private var remoteHlsUrl = IOS_HLS_DEMO_URL

    init() {
        _controller = StateObject(
            wrappedValue: VesperPlayerControllerFactory.makeDefault(
                initialSource: iosHlsDemoSource()
            )
        )
    }

    var body: some View {
        let uiState = controller.uiState

        ScrollView {
            VStack(alignment: .leading, spacing: 18) {
                Text(uiState.title)
                    .font(.largeTitle.weight(.bold))
                Text(uiState.subtitle)
                    .font(.body)
                    .foregroundStyle(.secondary)
                if let hostMessage {
                    Text(hostMessage)
                        .font(.caption)
                        .foregroundStyle(.red)
                }
                Text("Source: \(uiState.sourceLabel)")
                    .font(.caption)
                    .foregroundStyle(.purple)

                PlayerSurfaceContainer(controller: controller)
                    .frame(height: 240)

                HStack(spacing: 8) {
                    pill(uiState.playbackState.rawValue)
                    pill(controller.backend.rawValue)
                    pill("rate \(formatRate(uiState.playbackRate))x")
                    if uiState.isBuffering {
                        pill("buffering")
                    }
                    if uiState.isInterrupted {
                        pill("interrupted")
                    }
                }

                HStack(spacing: 10) {
                    Button("Pick from Photos") {
                        requestPhotoLibraryAccessAndPresentPicker()
                    }
                    Button("Import File") {
                        isImporterPresented = true
                    }
                    Button("Use HLS Demo") {
                        hostMessage = nil
                        controller.selectSource(iosHlsDemoSource())
                    }
                }
                .buttonStyle(.bordered)

                TextField("HLS URL", text: $remoteHlsUrl)
                    .textInputAutocapitalization(.never)
                    .autocorrectionDisabled()
                    .textFieldStyle(.roundedBorder)

                Button("Open HLS URL") {
                    let trimmed = remoteHlsUrl.trimmingCharacters(in: .whitespacesAndNewlines)
                    guard let url = URL(string: trimmed), !trimmed.isEmpty else {
                        hostMessage = "Please enter a valid HLS URL."
                        return
                    }
                    hostMessage = nil
                    controller.selectSource(VesperPlayerSource.hls(url: url, label: "Custom HLS URL"))
                }
                .buttonStyle(.bordered)

                if uiState.timeline.isSeekable && (uiState.timeline.kind == .vod || uiState.timeline.kind == .liveDvr) {
                    Slider(
                        value: Binding(
                            get: { pendingSeekRatio ?? (uiState.timeline.displayedRatio ?? 0.0) },
                            set: { pendingSeekRatio = $0 }
                        ),
                        in: 0...1,
                        onEditingChanged: { editing in
                            if !editing {
                                controller.seek(toRatio: pendingSeekRatio ?? (uiState.timeline.displayedRatio ?? 0.0))
                                pendingSeekRatio = nil
                            } else {
                                pendingSeekRatio = uiState.timeline.displayedRatio ?? 0.0
                            }
                        }
                    )

                    if uiState.timeline.kind == .liveDvr {
                        HStack(spacing: 8) {
                            pill(liveBadgeText(for: uiState.timeline))
                            Button("Go Live") {
                                controller.seekToLiveEdge()
                            }
                            .buttonStyle(.bordered)
                        }

                        Text(
                            "DVR \(formatMillis(uiState.timeline.positionMs)) / \(formatMillis(uiState.timeline.liveEdgeMs ?? uiState.timeline.durationMs ?? 0))"
                        )
                        .font(.subheadline)
                        .foregroundStyle(.secondary)
                    } else {
                        Text("\(formatMillis(uiState.timeline.positionMs)) / \(formatMillis(uiState.timeline.durationMs ?? 0))")
                            .font(.subheadline)
                            .foregroundStyle(.secondary)
                    }
                } else if uiState.timeline.kind == .live {
                    HStack(spacing: 8) {
                        pill("LIVE")
                        if let liveEdgeMs = uiState.timeline.liveEdgeMs {
                            Text("Edge \(formatMillis(liveEdgeMs))")
                                .font(.subheadline)
                                .foregroundStyle(.secondary)
                        }
                    }
                } else {
                    Text("Live timeline UI will be shown here when live/DVR backends land.")
                        .font(.subheadline)
                        .foregroundStyle(.secondary)
                }

                HStack(spacing: 10) {
                    Button("<< 5s") { controller.seek(by: -5_000) }
                    Button(uiState.playbackState == .playing ? "Pause" : "Play") { controller.togglePause() }
                    Button("Stop") { controller.stop() }
                    Button("5s >>") { controller.seek(by: 5_000) }
                }
                .buttonStyle(.borderedProminent)

                HStack(spacing: 10) {
                    ForEach(VesperPlayerController.supportedPlaybackRates, id: \.self) { rate in
                        Button("\(formatRate(rate))x") {
                            controller.setPlaybackRate(rate)
                        }
                    }
                }
                .buttonStyle(.bordered)
            }
            .padding(20)
        }
        .background(Color(red: 0.956, green: 0.945, blue: 0.918))
        .onAppear { controller.initialize() }
        .onDisappear { controller.dispose() }
        .photosPicker(
            isPresented: $isPhotoPickerPresented,
            selection: $selectedPhotoItem,
            matching: .videos,
            preferredItemEncoding: .current
        )
        .onChange(of: selectedPhotoItem) { _, item in
            guard let item else { return }
            Task {
                await handlePickedPhotoItem(item)
            }
        }
        .fileImporter(
            isPresented: $isImporterPresented,
            allowedContentTypes: [.movie, .mpeg4Movie, .video],
            allowsMultipleSelection: false
        ) { result in
            guard case let .success(urls) = result, let url = urls.first else { return }
            hostMessage = nil
            _ = url.startAccessingSecurityScopedResource()
            controller.selectSource(.localFile(url: url))
        }
    }

    @ViewBuilder
    private func pill(_ title: String) -> some View {
        Text(title)
            .font(.caption.weight(.semibold))
            .padding(.horizontal, 10)
            .padding(.vertical, 6)
            .background(Color.black.opacity(0.08), in: Capsule())
    }

    private func requestPhotoLibraryAccessAndPresentPicker() {
        let status = PHPhotoLibrary.authorizationStatus(for: .readWrite)
        switch status {
        case .authorized, .limited:
            hostMessage = nil
            exampleIosHostLog("photo library access available: \(status.rawValue)")
            isPhotoPickerPresented = true
        case .notDetermined:
            exampleIosHostLog("requesting photo library access")
            Task {
                let result = await PHPhotoLibrary.requestAuthorization(for: .readWrite)
                await MainActor.run {
                    handlePhotoAuthorizationStatus(result)
                }
            }
        case .denied, .restricted:
            hostMessage = "Photo Library access is required to pick videos from Photos."
            exampleIosHostLog("photo library access denied: \(status.rawValue)")
        @unknown default:
            hostMessage = "Unknown Photo Library authorization state."
            exampleIosHostLog("photo library access unknown")
        }
    }

    private func liveBadgeText(for timeline: TimelineUiState) -> String {
        guard let liveEdgeMs = timeline.liveEdgeMs else {
            return "LIVE"
        }
        let behindMs = max(liveEdgeMs - timeline.positionMs, 0)
        if behindMs > 1_500 {
            return "LIVE -\(formatMillis(behindMs))"
        }
        return "LIVE"
    }

    private func handlePhotoAuthorizationStatus(_ status: PHAuthorizationStatus) {
        switch status {
        case .authorized, .limited:
            hostMessage = nil
            exampleIosHostLog("photo library access granted: \(status.rawValue)")
            isPhotoPickerPresented = true
        case .denied, .restricted:
            hostMessage = "Photo Library access is required to pick videos from Photos."
            exampleIosHostLog("photo library access denied after prompt: \(status.rawValue)")
        case .notDetermined:
            exampleIosHostLog("photo library access still not determined")
        @unknown default:
            hostMessage = "Unknown Photo Library authorization state."
            exampleIosHostLog("photo library access unknown after prompt")
        }
    }

    private func handlePickedPhotoItem(_ item: PhotosPickerItem) async {
        do {
            if let resolved = try await resolvePickedVideo(item) {
                await MainActor.run {
                    hostMessage = nil
                    exampleIosHostLog("picked photo video url=\(resolved.url.absoluteString)")
                    controller.selectSource(.localFile(url: resolved.url, label: resolved.label))
                    selectedPhotoItem = nil
                }
            } else {
                await MainActor.run {
                    hostMessage = "Failed to load the selected video from Photos."
                    exampleIosHostLog("picked photo video returned nil transferable")
                    selectedPhotoItem = nil
                }
            }
        } catch {
            await MainActor.run {
                hostMessage = "Failed to load selected photo video: \(error.localizedDescription)"
                exampleIosHostLog("picked photo video failed: \(error.localizedDescription)")
                selectedPhotoItem = nil
            }
        }
    }

    private func resolvePickedVideo(_ item: PhotosPickerItem) async throws -> (url: URL, label: String)? {
        if let identifier = item.itemIdentifier,
           let original = await resolveOriginalPhotoVideo(identifier: identifier) {
            return original
        }

        if let movie = try await item.loadTransferable(type: PickedVideo.self) {
            return (movie.url, movie.url.lastPathComponent)
        }

        return nil
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

private func iosHlsDemoSource() -> VesperPlayerSource {
    VesperPlayerSource.hls(
        url: URL(string: IOS_HLS_DEMO_URL)!,
        label: "HLS Demo (BipBop)"
    )
}

private func exampleIosHostLog(_ message: String) {
    print("[VesperPlayerIOSExample] \(message)")
}

private let IOS_HLS_DEMO_URL =
    "https://devstreaming-cdn.apple.com/videos/streaming/examples/img_bipbop_adv_example_ts/master.m3u8"

private struct PickedVideo: Transferable {
    let url: URL

    static var transferRepresentation: some TransferRepresentation {
        FileRepresentation(importedContentType: .movie) { received in
            let destination = FileManager.default.temporaryDirectory
                .appendingPathComponent(UUID().uuidString)
                .appendingPathExtension(received.file.pathExtension)
            if FileManager.default.fileExists(atPath: destination.path) {
                try FileManager.default.removeItem(at: destination)
            }
            try FileManager.default.copyItem(at: received.file, to: destination)
            return Self(url: destination)
        }
    }
}

private func formatMillis(_ value: Int64) -> String {
    let totalSeconds = value / 1000
    let minutes = totalSeconds / 60
    let seconds = totalSeconds % 60
    return String(format: "%02d:%02d", minutes, seconds)
}

private func formatRate(_ value: Float) -> String {
    String(format: "%.1f", value)
}
