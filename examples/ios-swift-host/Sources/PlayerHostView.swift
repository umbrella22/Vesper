import AVFoundation
import MediaPlayer
import Photos
import SwiftUI
import UIKit
import UniformTypeIdentifiers
import VesperPlayerKit

struct PlayerHostView: View {
    @Environment(\.colorScheme) private var systemColorScheme
    @Environment(\.horizontalSizeClass) private var horizontalSizeClass

    @AppStorage("vesper.example.ios.theme_mode") private var themeModeRaw = ExampleThemeMode.system.rawValue
    @StateObject private var controller: VesperPlayerController
    @StateObject private var playlistCoordinator: VesperPlaylistCoordinator
    @StateObject private var downloadManager: VesperDownloadManager
    @StateObject private var deviceControls = ExampleIOSDeviceControls()
    @State private var pendingSeekRatio: Double?
    @State private var isVideoImporterPresented = false
    @State private var hostMessage: String?
    @State private var downloadMessage: String?
    @State private var downloadAlertMessage: String?
    @State private var remoteStreamUrl = IOS_HLS_DEMO_URL
    @State private var downloadRemoteUrl = IOS_HLS_DEMO_URL
    @State private var controlsVisible = true
    @State private var activeSheet: ExamplePlayerSheet?
    @State private var isFullscreen = false
    @State private var selectedTab: ExampleHostTab = .player
    @State private var selectedResilienceProfile: ExampleResilienceProfile = .balanced
    @State private var isApplyingResilienceProfile = false
    @State private var hasHandledFinishedPlayback = false
    @State private var controlsHideTask: Task<Void, Never>?
    @State private var queuedRemoteSource: VesperPlayerSource?
    @State private var queuedLocalSource: VesperPlayerSource?
    @State private var playlistItemIds: [String] = [IOS_HLS_PLAYLIST_ITEM_ID]
    @State private var pendingDownloadTasks: [ExamplePendingDownloadTask] = []
    @State private var savingTaskIds: Set<VesperDownloadTaskId> = []
    @State private var exportProgressByTaskId: [VesperDownloadTaskId: Float] = [:]

    init() {
        let playlistPreloadBudgetPolicy = VesperPreloadBudgetPolicy(
            maxConcurrentTasks: 2,
            maxMemoryBytes: 64 * 1024 * 1024,
            maxDiskBytes: 256 * 1024 * 1024,
            warmupWindowMs: 30_000
        )
        _controller = StateObject(
            wrappedValue: VesperPlayerControllerFactory.makeDefault(
                initialSource: nil,
                resiliencePolicy: ExampleResilienceProfile.balanced.policy,
                preloadBudgetPolicy: VesperPreloadBudgetPolicy(
                    maxConcurrentTasks: 0,
                    maxMemoryBytes: 0,
                    maxDiskBytes: 0,
                    warmupWindowMs: 0
                )
            )
        )
        _playlistCoordinator = StateObject(
            wrappedValue: VesperPlaylistCoordinator(
                configuration: VesperPlaylistConfiguration(
                    playlistId: "ios-swift-host",
                    neighborWindow: VesperPlaylistNeighborWindow(previous: 1, next: 1),
                    preloadWindow: VesperPlaylistPreloadWindow(nearVisible: 1, prefetchOnly: 2),
                    switchPolicy: examplePlaylistSwitchPolicy()
                ),
                preloadBudgetPolicy: playlistPreloadBudgetPolicy,
                resiliencePolicy: ExampleResilienceProfile.balanced.policy
            )
        )
        _downloadManager = StateObject(
            wrappedValue: VesperDownloadManager(
                configuration: VesperDownloadConfiguration(
                    runPostProcessorsOnCompletion: false,
                    pluginLibraryPaths: bundledDownloadPluginLibraryPaths()
                )
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

    private var isDownloadExportPluginInstalled: Bool {
        !bundledDownloadPluginLibraryPaths().isEmpty
    }

    var body: some View {
        let palette = exampleHostPalette(useDarkTheme: useDarkTheme)
        let uiState = controller.uiState
        let trackCatalog = controller.trackCatalog
        let trackSelection = controller.trackSelection
        let playlistSnapshot = playlistCoordinator.snapshot
        let downloadSnapshot = downloadManager.snapshot

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
                TabView(selection: $selectedTab) {
                    playerPage(
                        palette: palette,
                        uiState: uiState,
                        playlistSnapshot: playlistSnapshot
                    )
                    .tag(ExampleHostTab.player)
                    .tabItem {
                        Label(ExampleI18n.tabPlayer, systemImage: "play.rectangle.fill")
                    }

                    downloadPage(
                        palette: palette,
                        downloadSnapshot: downloadSnapshot
                    )
                    .tag(ExampleHostTab.downloads)
                    .tabItem {
                        Label(ExampleI18n.tabDownloads, systemImage: "arrow.down.circle.fill")
                    }
                }
                .tint(palette.primaryAction)
            }

            ExampleHiddenVolumeView(deviceControls: deviceControls)
                .frame(width: 1, height: 1)
                .opacity(0.01)
                .allowsHitTesting(false)
                .accessibilityHidden(true)
        }
        .preferredColorScheme(themeMode.preferredColorScheme)
        .statusBarHidden(isFullscreen)
        .persistentSystemOverlays(isFullscreen ? .hidden : .visible)
        .onAppear {
            controller.initialize()
            if playlistSnapshot.queue.isEmpty {
                applyPlaylistQueue(focusItemId: IOS_HLS_PLAYLIST_ITEM_ID)
            }
            scheduleControlsAutoHide(for: uiState)
        }
        .onDisappear {
            controlsHideTask?.cancel()
            downloadManager.dispose()
            playlistCoordinator.dispose()
            controller.dispose()
        }
        .onChange(of: playlistSnapshot.activeItem?.itemId) { _, activeItemId in
            guard
                let activeItemId,
                let source = playlistSnapshot.queue.first(where: { $0.item.itemId == activeItemId })?.item.source
            else {
                handlePlaybackCompletionIfNeeded(
                    playbackState: controller.uiState.playbackState,
                    activeItemId: activeItemId
                )
                return
            }
            controller.selectSource(source)
            controlsVisible = true
            handlePlaybackCompletionIfNeeded(
                playbackState: controller.uiState.playbackState,
                activeItemId: activeItemId
            )
        }
        .onChange(of: uiState.playbackState) { _, playbackState in
            scheduleControlsAutoHide(for: controller.uiState)
            handlePlaybackCompletionIfNeeded(
                playbackState: playbackState,
                activeItemId: playlistSnapshot.activeItem?.itemId
            )
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
        .fileImporter(
            isPresented: $isVideoImporterPresented,
            allowedContentTypes: [.movie, .video],
            allowsMultipleSelection: false
        ) { result in
            switch result {
            case let .success(urls):
                guard let url = urls.first else { return }
                hostMessage = ExampleI18n.preparingSelectedVideo
                Task(priority: .userInitiated) {
                    try? await Task.sleep(for: .milliseconds(120))
                    await handleImportedVideo(url)
                }
            case let .failure(error):
                let nsError = error as NSError
                guard nsError.code != NSUserCancelledError else { return }
                hostMessage = ExampleI18n.failedToLoadSelectedLocalVideo(error.localizedDescription)
            }
        }
        .sheet(item: $activeSheet) { sheet in
            ExampleSelectionSheetContent(
                sheet: sheet,
                uiState: uiState,
                trackCatalog: trackCatalog,
                trackSelection: trackSelection,
                effectiveVideoTrackId: controller.effectiveVideoTrackId,
                videoVariantObservation: controller.videoVariantObservation,
                fixedTrackStatus: controller.fixedTrackStatus,
                lastError: controller.lastError,
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
        .alert(
            ExampleI18n.downloadSaveToPhotosTitle,
            isPresented: Binding(
                get: { downloadAlertMessage != nil },
                set: { isPresented in
                    if !isPresented {
                        downloadAlertMessage = nil
                    }
                }
            )
        ) {
            Button("OK", role: .cancel) {
                downloadAlertMessage = nil
            }
        } message: {
            Text(downloadAlertMessage ?? "")
        }
    }

    @ViewBuilder
    private func playerPage(
        palette: ExampleHostPalette,
        uiState: PlayerHostUiState,
        playlistSnapshot: VesperPlaylistSnapshot
    ) -> some View {
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
                    dashDemoEnabled: true,
                    dashDemoNote: nil,
                    onThemeModeChange: { themeModeRaw = $0.rawValue },
                    onPickVideo: {
                        pickVideo()
                    },
                    onUseHlsDemo: {
                        hostMessage = nil
                        let nextPlaylistItemIds = enqueuePlaylistItem(
                            playlistItemIds,
                            itemId: IOS_HLS_PLAYLIST_ITEM_ID
                        )
                        applyPlaylistQueue(
                            focusItemId: IOS_HLS_PLAYLIST_ITEM_ID,
                            playlistItemIds: nextPlaylistItemIds
                        )
                        controlsVisible = true
                    },
                    onUseDashDemo: {
                        hostMessage = nil
                        let nextPlaylistItemIds = enqueuePlaylistItem(
                            playlistItemIds,
                            itemId: IOS_DASH_PLAYLIST_ITEM_ID
                        )
                        applyPlaylistQueue(
                            focusItemId: IOS_DASH_PLAYLIST_ITEM_ID,
                            playlistItemIds: nextPlaylistItemIds
                        )
                        controlsVisible = true
                    },
                    onUseLiveDvrAcceptance: {
                        hostMessage = nil
                        let nextPlaylistItemIds = enqueuePlaylistItem(
                            playlistItemIds,
                            itemId: IOS_LIVE_DVR_PLAYLIST_ITEM_ID
                        )
                        applyPlaylistQueue(
                            focusItemId: IOS_LIVE_DVR_PLAYLIST_ITEM_ID,
                            playlistItemIds: nextPlaylistItemIds
                        )
                        controlsVisible = true
                    },
                    onOpenRemote: {
                        openRemoteSource()
                    }
                )

                ExamplePlaylistSection(
                    palette: palette,
                    playlistQueue: playlistSnapshot.queue,
                    onFocusPlaylistItem: focusPlaylistItem
                )

                ExampleResilienceSection(
                    palette: palette,
                    selectedProfile: selectedResilienceProfile,
                    isApplyingProfile: isApplyingResilienceProfile,
                    onApplyProfile: applyResilienceProfile
                )
            }
            .padding(20)
        }
    }

    @ViewBuilder
    private func downloadPage(
        palette: ExampleHostPalette,
        downloadSnapshot: VesperDownloadSnapshot
    ) -> some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 18) {
                ExampleDownloadHeader(
                    palette: palette,
                    isDownloadExportPluginInstalled: isDownloadExportPluginInstalled
                )

                ExampleDownloadCreateSection(
                    palette: palette,
                    remoteUrl: $downloadRemoteUrl,
                    message: downloadMessage,
                    onUseHlsDemo: {
                        createDownloadTask(
                            assetIdPrefix: IOS_HLS_PLAYLIST_ITEM_ID,
                            source: iosHlsDemoSource()
                        )
                    },
                    onUseDashDemo: {
                        createDownloadTask(
                            assetIdPrefix: IOS_DASH_PLAYLIST_ITEM_ID,
                            source: iosDashDemoSource()
                        )
                    },
                    onCreateRemote: {
                        openRemoteDownloadSource()
                    }
                )

                ExampleDownloadTasksSection(
                    palette: palette,
                    tasks: downloadSnapshot.tasks,
                    pendingTasks: pendingDownloadTasks,
                    isDownloadExportPluginInstalled: isDownloadExportPluginInstalled,
                    savingTaskIds: savingTaskIds,
                    exportProgressByTaskId: exportProgressByTaskId,
                    onPrimaryAction: handleDownloadPrimaryAction,
                    onSaveToPhotos: saveDownloadToPhotos,
                    onRemoveTask: { task in
                        _ = downloadManager.removeTask(task.taskId)
                    }
                )
            }
            .padding(20)
        }
    }

    @ViewBuilder
    private func playerStage(uiState: PlayerHostUiState) -> some View {
        ExamplePlayerStage(
            surface: AnyView(PlayerSurfaceContainer(controller: controller)),
            uiState: uiState,
            trackCatalog: controller.trackCatalog,
            trackSelection: controller.trackSelection,
            effectiveVideoTrackId: controller.effectiveVideoTrackId,
            fixedTrackStatus: controller.fixedTrackStatus,
            controlsVisible: $controlsVisible,
            pendingSeekRatio: $pendingSeekRatio,
            isCompactLayout: isCompactLayout,
            isFullscreen: isFullscreen,
            onSeekBy: { controller.seek(by: $0) },
            onTogglePause: { controller.togglePause() },
            onSeekToRatio: { controller.seek(toRatio: $0) },
            onSeekToLiveEdge: { controller.seekToLiveEdge() },
            onSetPlaybackRate: { controller.setPlaybackRate($0) },
            onToggleFullscreen: {
                setFullscreen(!isFullscreen)
            },
            onOpenSheet: { activeSheet = $0 },
            currentBrightnessRatio: deviceControls.currentBrightnessRatio,
            onSetBrightnessRatio: deviceControls.setBrightnessRatio,
            currentVolumeRatio: deviceControls.currentVolumeRatio,
            onSetVolumeRatio: deviceControls.setVolumeRatio
        )
    }

    private func applyResilienceProfile(_ profile: ExampleResilienceProfile) {
        guard profile != selectedResilienceProfile, !isApplyingResilienceProfile else {
            return
        }

        selectedResilienceProfile = profile
        Task { @MainActor in
            isApplyingResilienceProfile = true
            await Task.yield()
            controller.setResiliencePolicy(profile.policy)
            playlistCoordinator.setResiliencePolicy(profile.policy)
            isApplyingResilienceProfile = false
        }
    }

    private func openRemoteSource() {
        let trimmed = remoteStreamUrl.trimmingCharacters(in: .whitespacesAndNewlines)
        guard let url = URL(string: trimmed), !trimmed.isEmpty else {
            hostMessage = ExampleI18n.invalidRemoteUrl
            return
        }
        let source = VesperPlayerSource.remoteUrl(url, label: ExampleI18n.customRemoteUrlLabel)
        hostMessage = nil
        queuedRemoteSource = source
        let nextPlaylistItemIds = enqueuePlaylistItem(
            playlistItemIds,
            itemId: IOS_REMOTE_PLAYLIST_ITEM_ID
        )
        applyPlaylistQueue(
            focusItemId: IOS_REMOTE_PLAYLIST_ITEM_ID,
            playlistItemIds: nextPlaylistItemIds
        )
        controlsVisible = true
    }

    private func openRemoteDownloadSource() {
        let trimmed = downloadRemoteUrl.trimmingCharacters(in: .whitespacesAndNewlines)
        guard let url = URL(string: trimmed), !trimmed.isEmpty else {
            downloadMessage = ExampleI18n.invalidRemoteUrl
            return
        }
        createDownloadTask(
            assetIdPrefix: IOS_REMOTE_PLAYLIST_ITEM_ID,
            source: .remoteUrl(url, label: exampleDraftDownloadLabel(for: url))
        )
    }

    private func createDownloadTask(
        assetIdPrefix: String,
        source: VesperPlayerSource
    ) {
        let assetId = "\(assetIdPrefix)-\(Int(Date().timeIntervalSince1970 * 1000.0))"
        if source.protocol == .dash {
            downloadMessage = ExampleI18n.dashNotSupportedOnIos
            return
        }

        pendingDownloadTasks.append(
            ExamplePendingDownloadTask(
                id: assetId,
                assetId: assetId,
                label: exampleDraftDownloadLabel(source),
                sourceUri: source.uri
            )
        )

        Task {
            do {
                let preparedTask = try await prepareExampleDownloadTask(assetId: assetId, source: source)
                await MainActor.run {
                    let taskId = downloadManager.createTask(
                        assetId: assetId,
                        source: preparedTask.source,
                        profile: preparedTask.profile,
                        assetIndex: preparedTask.assetIndex
                    )
                    pendingDownloadTasks.removeAll { $0.id == assetId }
                    downloadMessage = taskId == nil ? ExampleI18n.downloadCreateTaskFailed : nil
                }
            } catch {
                await MainActor.run {
                    pendingDownloadTasks.removeAll { $0.id == assetId }
                    downloadMessage = ExampleI18n.downloadCreateTaskFailed
                }
            }
        }
    }

    private func handleDownloadPrimaryAction(_ task: VesperDownloadTaskSnapshot) {
        let succeeded: Bool
        switch task.state {
        case .queued, .failed:
            succeeded = downloadManager.startTask(task.taskId)
        case .preparing, .downloading:
            succeeded = downloadManager.pauseTask(task.taskId)
        case .paused:
            succeeded = downloadManager.resumeTask(task.taskId)
        case .completed, .removed:
            succeeded = true
        }
        if !succeeded {
            downloadMessage = ExampleI18n.downloadActionFailed
        }
    }

    private func saveDownloadToPhotos(_ task: VesperDownloadTaskSnapshot) {
        guard
            let completedPath = task.assetIndex.completedPath?.trimmingCharacters(in: .whitespacesAndNewlines),
            !completedPath.isEmpty
        else {
            downloadAlertMessage = ExampleI18n.downloadSaveToPhotosMissingOutput
            return
        }
        guard !savingTaskIds.contains(task.taskId) else {
            return
        }

        let needsExport =
            task.source.contentFormat == .hlsSegments ||
            task.source.contentFormat == .dashSegments
        guard !needsExport || isDownloadExportPluginInstalled else {
            downloadAlertMessage = ExampleI18n.downloadExportPluginMissing
            return
        }

        Task {
            await MainActor.run {
                savingTaskIds.insert(task.taskId)
                if needsExport {
                    exportProgressByTaskId[task.taskId] = 0
                }
            }
            var exportURL: URL?
            do {
                let gallerySourcePath: String
                if needsExport {
                    exportURL = try createDownloadExportFile(for: task)
                    try? FileManager.default.removeItem(at: exportURL!)
                    try await downloadManager.exportTaskOutput(
                        taskId: task.taskId,
                        outputPath: exportURL!.path,
                        onProgress: { ratio in
                            Task { @MainActor in
                                exportProgressByTaskId[task.taskId] =
                                    max(Float(0), min(Float(1), ratio))
                            }
                        }
                    )
                    gallerySourcePath = exportURL!.path
                } else {
                    gallerySourcePath = completedPath
                }

                try await saveVideoToPhotoLibrary(completedPath: gallerySourcePath)
                await MainActor.run {
                    downloadAlertMessage = ExampleI18n.downloadSaveToPhotosSuccess
                }
            } catch {
                await MainActor.run {
                    downloadAlertMessage = ExampleI18n.downloadSaveToPhotosFailed(error.localizedDescription)
                }
            }
            if let exportURL {
                try? FileManager.default.removeItem(at: exportURL)
            }
            await MainActor.run {
                savingTaskIds.remove(task.taskId)
                exportProgressByTaskId.removeValue(forKey: task.taskId)
            }
        }
    }

    private func pickVideo() {
        hostMessage = nil
        isVideoImporterPresented = true
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

    private func applyPlaylistQueue(
        focusItemId: String? = nil,
        playlistItemIds: [String]? = nil
    ) {
        let queue = examplePlaylistQueue(
            playlistItemIds: playlistItemIds ?? self.playlistItemIds,
            remoteSource: queuedRemoteSource,
            localSource: queuedLocalSource
        )
        self.playlistItemIds = queue.map(\.itemId)
        playlistCoordinator.replaceQueue(queue)

        let requestedFocusItemId = focusItemId ?? playlistCoordinator.snapshot.activeItem?.itemId
        let resolvedFocusItemId = requestedFocusItemId.flatMap { itemId in
            queue.contains(where: { $0.itemId == itemId }) ? itemId : nil
        } ?? queue.first?.itemId

        guard let resolvedFocusItemId else {
            playlistCoordinator.clearViewportHints()
            return
        }

        playlistCoordinator.updateViewportHints(
            examplePlaylistViewportHints(
                queue: queue,
                focusedItemId: resolvedFocusItemId
            )
        )
    }

    private func focusPlaylistItem(_ itemId: String) {
        let queue = playlistCoordinator.snapshot.queue.map(\.item)
        playlistCoordinator.updateViewportHints(
            examplePlaylistViewportHints(
                queue: queue,
                focusedItemId: itemId
            )
        )
        controlsVisible = true
    }

    private func handlePlaybackCompletionIfNeeded(
        playbackState: PlaybackStateUi,
        activeItemId: String?
    ) {
        guard playbackState == .finished else {
            hasHandledFinishedPlayback = false
            return
        }
        guard !hasHandledFinishedPlayback, activeItemId != nil else {
            return
        }
        hasHandledFinishedPlayback = true
        playlistCoordinator.handlePlaybackCompleted()
    }

    private func handleImportedVideo(_ url: URL) async {
        do {
            let persisted = try persistImportedVideoFile(from: url)
            await MainActor.run {
                hostMessage = nil
                exampleIosHostLog("picked local video url=\(persisted.url.absoluteString)")
                queuedLocalSource = .localFile(url: persisted.url, label: persisted.label)
                let nextPlaylistItemIds = enqueuePlaylistItem(
                    playlistItemIds,
                    itemId: IOS_LOCAL_PLAYLIST_ITEM_ID
                )
                applyPlaylistQueue(
                    focusItemId: IOS_LOCAL_PLAYLIST_ITEM_ID,
                    playlistItemIds: nextPlaylistItemIds
                )
                controlsVisible = true
            }
        } catch {
            await MainActor.run {
                hostMessage = ExampleI18n.failedToLoadSelectedLocalVideo(error.localizedDescription)
                exampleIosHostLog("picked local video failed: \(error.localizedDescription)")
            }
        }
    }

    private func persistImportedVideoFile(from url: URL) throws -> (url: URL, label: String) {
        let startedSecurityScope = url.startAccessingSecurityScopedResource()
        defer {
            if startedSecurityScope {
                url.stopAccessingSecurityScopedResource()
            }
        }

        let fileExtension = url.pathExtension.isEmpty ? "mov" : url.pathExtension
        let destination = FileManager.default.temporaryDirectory
            .appendingPathComponent(UUID().uuidString)
            .appendingPathExtension(fileExtension)

        if FileManager.default.fileExists(atPath: destination.path) {
            try FileManager.default.removeItem(at: destination)
        }

        do {
            try FileManager.default.copyItem(at: url, to: destination)
        } catch {
            throw error
        }

        return (destination, url.lastPathComponent)
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
        case .denied, .restricted, .notDetermined:
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
        if completedPath.hasPrefix("file://"),
           let fileURL = URL(string: completedPath),
           fileURL.isFileURL {
            return fileURL
        }
        return URL(fileURLWithPath: completedPath)
    }
}

@MainActor
private final class ExampleIOSDeviceControls: ObservableObject {
    fileprivate let volumeView: MPVolumeView
    private weak var volumeSlider: UISlider?

    init() {
        volumeView = MPVolumeView(frame: .zero)
        volumeView.showsVolumeSlider = true
    }

    func currentBrightnessRatio() -> Double? {
        Double(UIScreen.main.brightness).clamped(to: 0...1)
    }

    func setBrightnessRatio(_ ratio: Double) -> Double? {
        let nextRatio = CGFloat(ratio.clamped(to: 0.02...1))
        UIScreen.main.brightness = nextRatio
        return Double(UIScreen.main.brightness).clamped(to: 0...1)
    }

    func currentVolumeRatio() -> Double? {
        prepareAudioSessionIfNeeded()
        refreshVolumeSlider()
        if let volumeSlider {
            return Double(volumeSlider.value).clamped(to: 0...1)
        }
        return Double(AVAudioSession.sharedInstance().outputVolume).clamped(to: 0...1)
    }

    func setVolumeRatio(_ ratio: Double) -> Double? {
        prepareAudioSessionIfNeeded()
        refreshVolumeSlider()
        guard let volumeSlider else {
            return currentVolumeRatio()
        }
        let nextRatio = Float(ratio.clamped(to: 0...1))
        volumeSlider.setValue(nextRatio, animated: false)
        volumeSlider.sendActions(for: .valueChanged)
        volumeSlider.sendActions(for: .touchUpInside)
        return Double(volumeSlider.value).clamped(to: 0...1)
    }

    fileprivate func refreshVolumeSlider() {
        volumeSlider = volumeView.subviews.compactMap { $0 as? UISlider }.first
    }

    private func prepareAudioSessionIfNeeded() {
        try? AVAudioSession.sharedInstance().setActive(true)
    }
}

private struct ExampleHiddenVolumeView: UIViewRepresentable {
    let deviceControls: ExampleIOSDeviceControls

    func makeUIView(context: Context) -> MPVolumeView {
        DispatchQueue.main.async {
            deviceControls.refreshVolumeSlider()
        }
        return deviceControls.volumeView
    }

    func updateUIView(_ uiView: MPVolumeView, context: Context) {
        DispatchQueue.main.async {
            deviceControls.refreshVolumeSlider()
        }
    }
}

private extension UIWindowScene {
    var keyWindow: UIWindow? {
        windows.first(where: \.isKeyWindow)
    }
}

private enum ExamplePhotoLibraryExportError: LocalizedError {
    case missingCompletedFile
    case accessDenied
    case saveFailed

    var errorDescription: String? {
        switch self {
        case .missingCompletedFile:
            return ExampleI18n.downloadSaveToPhotosMissingOutput
        case .accessDenied:
            return ExampleI18n.photoLibraryAddAccessRequired
        case .saveFailed:
            return ExampleI18n.downloadSaveToPhotosFailed(ExampleI18n.downloadSaveToPhotosFailedUnknown)
        }
    }
}
