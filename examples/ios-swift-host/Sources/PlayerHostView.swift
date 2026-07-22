import AVFoundation
import AVKit
import Combine
import CoreTransferable
import MediaPlayer
import Photos
import PhotosUI
import SwiftUI
import UIKit
import UniformTypeIdentifiers
import VesperPlayerKit
import VesperPlayerKitUI

@MainActor
private func makeExampleController(
    sourceNormalizerSetting: ExampleSourceNormalizerSetting,
    nativeFramePipelineSetting: ExampleNativeFramePipelineSetting,
    initialSource: VesperPlayerSource?,
    resiliencePolicy: VesperPlaybackResiliencePolicy,
    directNativePlaybackRequired: Bool = false
) -> VesperPlayerController {
    let pluginConfiguration = makeExamplePlaybackPluginConfiguration(
        sourceNormalizerSetting: sourceNormalizerSetting,
        nativeFramePipelineSetting: nativeFramePipelineSetting,
        sourceNormalizerPluginLibraryPaths: bundledSourceNormalizerPluginLibraryPaths(),
        decoderPluginLibraryPaths: bundledDecoderPluginLibraryPaths(),
        frameProcessorPluginLibraryPaths: bundledFrameProcessorPluginLibraryPaths(),
        directNativePlaybackRequired: directNativePlaybackRequired
    )
    return VesperPlayerControllerFactory.makeDefault(
        initialSource: initialSource,
        resiliencePolicy: resiliencePolicy,
        preloadBudgetPolicy: VesperPreloadBudgetPolicy(
            maxConcurrentTasks: 0,
            maxMemoryBytes: 0,
            maxDiskBytes: 0,
            warmupWindowMs: 0
        ),
        sourceNormalizerConfiguration: pluginConfiguration.sourceNormalizerConfiguration,
        frameProcessorConfiguration: pluginConfiguration.frameProcessorConfiguration,
        nativeFramePipelineConfiguration: pluginConfiguration.nativeFramePipelineConfiguration
    )
}

private func playbackSafeNativeFrameSetting(
    _ setting: ExampleNativeFramePipelineSetting
) -> ExampleNativeFramePipelineSetting {
    switch setting {
    case .requireNativeFrame:
        return .preferNativeFrame
    case .disabled, .diagnosticsOnly, .preferNativeFrame:
        return setting
    }
}

private func nativeFrameSettingForSourceNormalizer(
    sourceNormalizerSetting: ExampleSourceNormalizerSetting,
    nativeFramePipelineSetting: ExampleNativeFramePipelineSetting
) -> ExampleNativeFramePipelineSetting {
    guard !sourceNormalizerSetting.supportsNativeFramePacketInput,
          nativeFramePipelineSetting == .requireNativeFrame else {
        return nativeFramePipelineSetting
    }
    return .preferNativeFrame
}

@MainActor
private final class ExamplePlayerControllerStore: ObservableObject {
    @Published private(set) var controller: VesperPlayerController

    private var controllerObservation: AnyCancellable?

    init(controller: VesperPlayerController) {
        self.controller = controller
        observe(controller)
    }

    func replace(with nextController: VesperPlayerController) -> VesperPlayerController {
        let previousController = controller
        controller = nextController
        observe(nextController)
        return previousController
    }

    func dispose() {
        controllerObservation?.cancel()
        controller.dispose()
    }

    private func observe(_ controller: VesperPlayerController) {
        controllerObservation = controller.objectWillChange.sink { [weak self] _ in
            Task { @MainActor in
                self?.objectWillChange.send()
            }
        }
    }
}

private final class ExamplePictureInPictureDelegate: NSObject, AVPictureInPictureControllerDelegate {
    var onWillStart: (() -> Void)?
    var onDidStart: (() -> Void)?
    var onWillStop: (() -> Void)?
    var onStop: (() -> Void)?
    var onFailure: ((Error) -> Void)?

    func pictureInPictureControllerWillStartPictureInPicture(
        _ pictureInPictureController: AVPictureInPictureController
    ) {
        onWillStart?()
    }

    func pictureInPictureControllerDidStartPictureInPicture(
        _ pictureInPictureController: AVPictureInPictureController
    ) {
        onDidStart?()
    }

    func pictureInPictureController(
        _ pictureInPictureController: AVPictureInPictureController,
        failedToStartPictureInPictureWithError error: Error
    ) {
        onFailure?(error)
    }

    func pictureInPictureControllerWillStopPictureInPicture(
        _ pictureInPictureController: AVPictureInPictureController
    ) {
        onWillStop?()
    }

    func pictureInPictureControllerDidStopPictureInPicture(
        _ pictureInPictureController: AVPictureInPictureController
    ) {
        onStop?()
    }

    func pictureInPictureController(
        _ pictureInPictureController: AVPictureInPictureController,
        restoreUserInterfaceForPictureInPictureStopWithCompletionHandler
            completionHandler: @escaping (Bool) -> Void
    ) {
        completionHandler(true)
    }
}

struct PlayerHostView: View {
    @Environment(\.colorScheme) private var systemColorScheme
    @Environment(\.horizontalSizeClass) private var horizontalSizeClass

    @AppStorage("vesper.example.ios.theme_mode") private var themeModeRaw = ExampleThemeMode.system.rawValue
    @StateObject private var controllerStore: ExamplePlayerControllerStore
    @StateObject private var playlistCoordinator: VesperPlaylistCoordinator
    @StateObject private var downloadManager: VesperDownloadManager
    @StateObject private var deviceControls = ExampleIOSDeviceControls()
    @State private var pendingSeekRatio: Double?
    @State private var isVideoPickerPresented = false
    @State private var selectedVideoItem: PhotosPickerItem?
    @State private var hostMessage: String?
    @State private var downloadMessage: String?
    @State private var downloadAlertMessage: String?
    @State private var remoteStreamUrl = IOS_HLS_DEMO_URL
    @State private var downloadRemoteUrl = IOS_HLS_DEMO_URL
    @State private var controlsVisible = true
    @State private var activeSheet: ExamplePlayerSheet?
    @State private var isFullscreen = false
    @State private var pictureInPictureEnabled = false
    @State private var pictureInPicturePresentation = false
    @State private var pictureInPictureController: AVPictureInPictureController?
    @State private var pictureInPictureDelegate = ExamplePictureInPictureDelegate()
    @State private var currentSurfaceView: PlayerSurfaceView?
    @State private var selectedTab: ExampleHostTab = .play
    @State private var isManageQueuePresented = false
    @State private var selectedResilienceProfile: ExampleResilienceProfile = .balanced
    @State private var sourceNormalizerSetting: ExampleSourceNormalizerSetting = .preflightOnly
    @State private var nativeFramePipelineSetting: ExampleNativeFramePipelineSetting = .disabled
    @State private var isApplyingResilienceProfile = false
    @State private var selectedHdrEvidencePreset = exampleHdrEvidenceP0Presets[1]
    @State private var selectedDolbyDrmKind: ExampleDolbyAcceptanceDrmKind = .clear
    @State private var selectedDolbyProfile: ExampleDolbyAcceptanceProfile?
    @State private var selectedDolbyFps: Int?
    @State private var isCapturingHdrEvidence = false
    @State private var hasHandledFinishedPlayback = false
    @State private var controlsHideTask: Task<Void, Never>?
    @State private var activeDirectSource: VesperPlayerSource?
    @State private var playbackOrigin: ExamplePlaybackOrigin?
    @State private var hostLogEntries: [ExampleHostLogEntry] = []
    @State private var hostLogNextId: Int64 = 1
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
        _controllerStore = StateObject(
            wrappedValue: ExamplePlayerControllerStore(
                controller: makeExampleController(
                    sourceNormalizerSetting: .preflightOnly,
                    nativeFramePipelineSetting: .disabled,
                    initialSource: nil,
                    resiliencePolicy: ExampleResilienceProfile.balanced.policy
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

    private var controller: VesperPlayerController {
        controllerStore.controller
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

    private var sourceNormalizerPluginLibraryPaths: [String] {
        bundledSourceNormalizerPluginLibraryPaths()
    }

    private var decoderPluginLibraryPaths: [String] {
        bundledDecoderPluginLibraryPaths()
    }

    private var frameProcessorPluginLibraryPaths: [String] {
        bundledFrameProcessorPluginLibraryPaths()
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
                    playPage(
                        palette: palette,
                        uiState: uiState,
                        playlistSnapshot: playlistSnapshot
                    )
                    .tag(ExampleHostTab.play)
                    .tabItem {
                        Label(ExampleI18n.tabPlay, systemImage: "play.rectangle.fill")
                    }

                    diagnosticsPage(
                        palette: palette,
                        uiState: uiState
                    )
                    .tag(ExampleHostTab.diagnostics)
                    .tabItem {
                        Label(ExampleI18n.tabDiagnostics, systemImage: "stethoscope")
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
            controllerStore.dispose()
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
            if let presetId = dolbyPresetIdFromPlaylistItemId(activeItemId),
               let preset = exampleDolbyAcceptancePreset(id: presetId) {
                activateDolbyAcceptancePreset(
                    preset,
                    origin: .queue(itemId: activeItemId)
                )
            } else {
                selectSourceForPlayback(source, origin: .queue(itemId: activeItemId))
            }
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
        .onChange(of: pictureInPictureEnabled) { _, enabled in
            pictureInPictureController?.canStartPictureInPictureAutomaticallyFromInline = enabled
            if !enabled {
                setPictureInPicturePresentation(false)
            }
        }
        .photosPicker(
            isPresented: $isVideoPickerPresented,
            selection: $selectedVideoItem,
            matching: .videos,
            preferredItemEncoding: .current,
            photoLibrary: .shared()
        )
        .onChange(of: selectedVideoItem) { _, item in
            guard let item else {
                return
            }
            hostMessage = ExampleI18n.preparingVideoFromPhotos
            Task(priority: .userInitiated) {
                await handlePickedVideo(item)
                await MainActor.run {
                    selectedVideoItem = nil
                }
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
                onSelectSubtitle: { selection in
                    Task { @MainActor in
                        do {
                            try await controller.setSubtitleTrackSelection(selection)
                            activeSheet = nil
                        } catch {
                            hostMessage = error.localizedDescription
                            appendHostLog(
                                severity: .error,
                                title: ExampleI18n.subtitles,
                                detail: error.localizedDescription
                            )
                        }
                    }
                },
                onSelectSpeed: {
                    controller.setPlaybackRate($0)
                    activeSheet = nil
                }
            )
            .presentationDetents([.height(sheetHeight(for: sheet))])
            .presentationDragIndicator(.hidden)
        }
        .sheet(isPresented: $isManageQueuePresented) {
            ExampleQueueManagementSheet(
                palette: palette,
                playlistQueue: playlistSnapshot.queue,
                onFocusPlaylistItem: { itemId in
                    focusPlaylistItem(itemId)
                    isManageQueuePresented = false
                }
            )
            .presentationDetents([.medium, .large])
            .presentationDragIndicator(.visible)
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
    private func playPage(
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

                ExampleThemeModeControl(
                    palette: palette,
                    themeMode: themeMode,
                    onThemeModeChange: { themeModeRaw = $0.rawValue }
                )

                playerStage(uiState: uiState)
                    .frame(height: 248)

                ExampleQuickSourcePanel(
                    palette: palette,
                    remoteStreamUrl: $remoteStreamUrl,
                    hostMessage: hostMessage,
                    dashDemoEnabled: true,
                    dashDemoNote: nil,
                    onPickVideo: {
                        pickVideo()
                    },
                    onUseHlsDemo: {
                        enqueueAndFocusPlaylistItem(
                            IOS_HLS_PLAYLIST_ITEM_ID,
                            logTitle: ExampleI18n.logSourceSelected,
                            logDetail: ExampleI18n.hlsDemoLabel
                        )
                    },
                    onUseDashDemo: {
                        enqueueAndFocusPlaylistItem(
                            IOS_DASH_PLAYLIST_ITEM_ID,
                            logTitle: ExampleI18n.logSourceSelected,
                            logDetail: ExampleI18n.dashDemoLabel
                        )
                    },
                    onUseLiveDvrAcceptance: {
                        enqueueAndFocusPlaylistItem(
                            IOS_LIVE_DVR_PLAYLIST_ITEM_ID,
                            logTitle: ExampleI18n.logSourceSelected,
                            logDetail: ExampleI18n.liveDvrAcceptanceLabel
                        )
                    },
                    onOpenRemote: {
                        openRemoteSource()
                    }
                )

                ExampleQueuePanel(
                    palette: palette,
                    playlistQueue: playlistSnapshot.queue,
                    onFocusPlaylistItem: focusPlaylistItem,
                    onManageQueue: { isManageQueuePresented = true }
                )

                ExamplePictureInPictureSection(
                    palette: palette,
                    enabled: $pictureInPictureEnabled,
                    onRequestPictureInPicture: requestPictureInPicture
                )

            }
            .padding(20)
        }
    }

    @ViewBuilder
    private func diagnosticsPage(
        palette: ExampleHostPalette,
        uiState: PlayerHostUiState
    ) -> some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 18) {
                ExampleDiagnosticsSummarySection(
                    palette: palette,
                    sourceLabel: uiState.sourceLabel.isEmpty ? ExampleI18n.diagnosticsNone : uiState.sourceLabel,
                    sourceProtocol: activePlaybackSource()?.protocol.rawValue ?? ExampleI18n.diagnosticsNone,
                    routeLabel: diagnosticsRouteLabel(),
                    playbackOrigin: playbackOrigin,
                    sourceNormalizerSetting: sourceNormalizerSetting,
                    nativeFramePipelineSetting: nativeFramePipelineSetting
                )

                ExampleEventLogSection(
                    palette: palette,
                    entries: hostLogEntries
                )

                ExampleDolbyCatalogPanel(
                    palette: palette,
                    presets: exampleDolbyAcceptanceCatalog,
                    selectedDrmKind: selectedDolbyDrmKind,
                    selectedProfile: selectedDolbyProfile,
                    selectedFps: selectedDolbyFps,
                    onDrmKindChange: { selectedDolbyDrmKind = $0 },
                    onProfileChange: { selectedDolbyProfile = $0 },
                    onFpsChange: { selectedDolbyFps = $0 },
                    onPresetPlayNow: { preset in
                        activateDolbyAcceptancePreset(
                            preset,
                            origin: .dolbyAdHoc(presetId: preset.id)
                        )
                    },
                    onPresetAddToQueue: addDolbyPresetToQueue
                )

                ExamplePluginDiagnosticsSection(
                    palette: palette,
                    sourceNormalizerSetting: sourceNormalizerSetting,
                    nativeFramePipelineSetting: nativeFramePipelineSetting,
                    sourceNormalizerPluginLibraryPaths: sourceNormalizerPluginLibraryPaths,
                    decoderPluginLibraryPaths: decoderPluginLibraryPaths,
                    frameProcessorPluginLibraryPaths: frameProcessorPluginLibraryPaths,
                    pluginDiagnostics: controller.pluginDiagnostics,
                    hdrEvidencePresets: exampleHdrEvidenceP0Presets + exampleDolbyAcceptanceHdrEvidencePresets(),
                    selectedHdrEvidencePreset: selectedHdrEvidencePreset,
                    isCapturingHdrEvidence: isCapturingHdrEvidence,
                    hdrEvidenceActiveSourceAvailable: activePlaybackSource() != nil,
                    onSourceNormalizerSettingChange: applySourceNormalizerSetting,
                    onNativeFramePipelineSettingChange: applyNativeFramePipelineSetting,
                    onHdrEvidencePresetChange: { preset in
                        selectedHdrEvidencePreset = preset
                    },
                    onCaptureHdrEvidence: captureHdrEvidence
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
                    onShareOutput: shareDownloadOutput,
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
            surface: AnyView(
                PlayerSurfaceContainer(
                    controller: controller,
                    onSurfaceReady: { view in
                        guard currentSurfaceView !== view else { return }
                        Task { @MainActor in
                            if currentSurfaceView !== view {
                                currentSurfaceView = view
                            }
                        }
                    },
                    onSurfaceRemoved: { view in
                        guard currentSurfaceView === view else { return }
                        Task { @MainActor in
                            if currentSurfaceView === view {
                                currentSurfaceView = nil
                            }
                        }
                    }
                )
            ),
            uiState: uiState,
            trackCatalog: controller.trackCatalog,
            trackSelection: controller.trackSelection,
            effectiveVideoTrackId: controller.effectiveVideoTrackId,
            fixedTrackStatus: controller.fixedTrackStatus,
            controlsVisible: $controlsVisible,
            pendingSeekRatio: $pendingSeekRatio,
            isCompactLayout: isCompactLayout,
            isFullscreen: isFullscreen,
            pictureInPicturePresentation: pictureInPicturePresentation,
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
            onSetVolumeRatio: deviceControls.setVolumeRatio,
            airPlayRouteButton: AnyView(
                VesperAirPlayRouteButton(
                    controller: controller,
                    tintColor: .white,
                    activeTintColor: .systemBlue
                )
            )
        )
    }

    private func appendHostLog(
        severity: ExampleHostLogSeverity = .info,
        title: String,
        detail: String? = nil
    ) {
        let entry = ExampleHostLogEntry(
            id: hostLogNextId,
            atMillis: Int64(Date().timeIntervalSince1970 * 1000.0),
            severity: severity,
            title: title,
            detail: detail
        )
        hostLogNextId += 1
        hostLogEntries = appendExampleHostLogEntry(hostLogEntries, entry: entry)
    }

    private func enqueueAndFocusPlaylistItem(
        _ itemId: String,
        logTitle: String,
        logDetail: String
    ) {
        hostMessage = nil
        let nextPlaylistItemIds = enqueuePlaylistItem(
            playlistItemIds,
            itemId: itemId
        )
        applyPlaylistQueue(
            focusItemId: itemId,
            playlistItemIds: nextPlaylistItemIds
        )
        appendHostLog(title: logTitle, detail: logDetail)
        controlsVisible = true
    }

    private func addDolbyPresetToQueue(_ preset: ExampleDolbyAcceptancePreset) {
        guard canQueueDolbyAcceptancePreset(preset) else {
            hostMessage = ExampleI18n.dolbyAcceptancePendingMessage
            appendHostLog(
                severity: .warning,
                title: ExampleI18n.logDolbyAddedToQueue,
                detail: hostMessage
            )
            return
        }
        let itemId = dolbyPlaylistItemId(preset.id)
        let nextPlaylistItemIds = enqueuePlaylistItem(
            playlistItemIds,
            itemId: itemId
        )
        applyPlaylistQueue(playlistItemIds: nextPlaylistItemIds)
        appendHostLog(
            title: ExampleI18n.logDolbyAddedToQueue,
            detail: preset.label
        )
    }

    private func diagnosticsRouteLabel() -> String {
        switch nativeFramePipelineSetting {
        case .preferNativeFrame, .requireNativeFrame:
            return nativeFramePipelineSetting.title
        case .disabled, .diagnosticsOnly:
            return "AVPlayer"
        }
    }

    private func applyResilienceProfile(_ profile: ExampleResilienceProfile) {
        guard profile != selectedResilienceProfile, !isApplyingResilienceProfile else {
            return
        }

        selectedResilienceProfile = profile
        appendHostLog(
            title: ExampleI18n.logPluginModeChange,
            detail: profile.title
        )
        Task { @MainActor in
            isApplyingResilienceProfile = true
            await Task.yield()
            controller.setResiliencePolicy(profile.policy)
            playlistCoordinator.setResiliencePolicy(profile.policy)
            isApplyingResilienceProfile = false
        }
    }

    private func applySourceNormalizerSetting(_ setting: ExampleSourceNormalizerSetting) {
        guard setting != sourceNormalizerSetting else {
            return
        }

        let previousController = controller
        let activeSource = activePlaybackSource()
        let previousUiState = previousController.uiState
        sourceNormalizerSetting = setting
        appendHostLog(
            title: ExampleI18n.logPluginModeChange,
            detail: setting.title
        )
        let resolvedNativeFrameSetting = nativeFrameSettingForSourceNormalizer(
            sourceNormalizerSetting: setting,
            nativeFramePipelineSetting: nativeFramePipelineSetting
        )
        if resolvedNativeFrameSetting != nativeFramePipelineSetting {
            nativeFramePipelineSetting = resolvedNativeFrameSetting
            hostMessage = ExampleI18n.nativeFrameRequireDowngradedForPlayback
        }
        let nextController = makeExampleController(
            sourceNormalizerSetting: setting,
            nativeFramePipelineSetting: resolvedNativeFrameSetting,
            initialSource: activeSource,
            resiliencePolicy: selectedResilienceProfile.policy
        )
        _ = controllerStore.replace(with: nextController)
        previousController.dispose()
        controlsVisible = true
        initializeReplacementController(
            nextController,
            activeSource: activeSource,
            previousUiState: previousUiState,
            nativeFramePipelineSetting: resolvedNativeFrameSetting
        )
    }

    private func applyNativeFramePipelineSetting(_ setting: ExampleNativeFramePipelineSetting) {
        guard setting != nativeFramePipelineSetting else {
            return
        }

        let previousController = controller
        let activeSource = activePlaybackSource()
        let previousUiState = previousController.uiState
        let resolvedSetting = nativeFrameSettingForSourceNormalizer(
            sourceNormalizerSetting: sourceNormalizerSetting,
            nativeFramePipelineSetting: setting
        )
        nativeFramePipelineSetting = resolvedSetting
        appendHostLog(
            title: ExampleI18n.logPluginModeChange,
            detail: resolvedSetting.title
        )
        if resolvedSetting != setting {
            hostMessage = ExampleI18n.nativeFrameRequireDowngradedForPlayback
        }
        let nextController = makeExampleController(
            sourceNormalizerSetting: sourceNormalizerSetting,
            nativeFramePipelineSetting: resolvedSetting,
            initialSource: activeSource,
            resiliencePolicy: selectedResilienceProfile.policy
        )
        _ = controllerStore.replace(with: nextController)
        previousController.dispose()
        controlsVisible = true
        initializeReplacementController(
            nextController,
            activeSource: activeSource,
            previousUiState: previousUiState,
            nativeFramePipelineSetting: resolvedSetting
        )
    }

    private func captureHdrEvidence() {
        guard !isCapturingHdrEvidence else {
            return
        }

        let preset = selectedHdrEvidencePreset
        let source: VesperPlayerSource
        if preset.sampleId == "NETWORK-FAILURE-CONTROL" {
            guard let url = URL(string: IOS_HDR_EVIDENCE_NETWORK_CONTROL_URL) else {
                hostMessage = ExampleI18n.invalidRemoteUrl
                return
            }
            source = .remoteUrl(
                url,
                label: ExampleI18n.hdrEvidenceNetworkControlLabel,
                protocol: .progressive
            )
        } else if let dolbyPreset = exampleDolbyAcceptancePreset(id: preset.sampleId) {
            source = dolbyPreset.source
        } else if let activeSource = activePlaybackSource() {
            source = activeSource
        } else {
            hostMessage = ExampleI18n.hdrEvidenceSelectSource
            return
        }

        isCapturingHdrEvidence = true
        hostMessage = ExampleI18n.hdrEvidenceCapturing
        Task { @MainActor in
            await Task.yield()
            do {
                let directory = try await captureExampleHdrEvidenceBundle(
                    ExampleHdrEvidenceCaptureContext(
                        preset: preset,
                        source: source,
                        controller: controller,
                        sourceNormalizerSetting: sourceNormalizerSetting,
                        nativeFramePipelineSetting: nativeFramePipelineSetting,
                        sourceNormalizerPluginLibraryPaths: sourceNormalizerPluginLibraryPaths,
                        decoderPluginLibraryPaths: decoderPluginLibraryPaths,
                        frameProcessorPluginLibraryPaths: frameProcessorPluginLibraryPaths
                    )
                )
                hostMessage = ExampleI18n.hdrEvidenceWritten(directory.path)
                appendHostLog(
                    title: ExampleI18n.logHdrEvidenceResult,
                    detail: directory.path
                )
            } catch {
                hostMessage = ExampleI18n.hdrEvidenceFailed(error.localizedDescription)
                appendHostLog(
                    severity: .error,
                    title: ExampleI18n.logHdrEvidenceResult,
                    detail: error.localizedDescription
                )
            }
            isCapturingHdrEvidence = false
        }
    }

    private func activateDolbyAcceptancePreset(
        _ preset: ExampleDolbyAcceptancePreset,
        origin: ExamplePlaybackOrigin
    ) {
        guard preset.isPlayable else {
            hostMessage = preset.drmKind == .fairPlay
                ? ExampleI18n.dolbyAcceptanceFairPlayConfigRequired
                : ExampleI18n.dolbyAcceptancePendingMessage
            appendHostLog(
                severity: .warning,
                title: ExampleI18n.logDolbyPlayNow,
                detail: hostMessage
            )
            return
        }

        let previousController = controller
        let previousUiState = previousController.uiState
        activeDirectSource = preset.source
        playbackOrigin = origin
        sourceNormalizerSetting = .disabled
        nativeFramePipelineSetting = .disabled
        let nextController = makeExampleController(
            sourceNormalizerSetting: .disabled,
            nativeFramePipelineSetting: .disabled,
            initialSource: preset.source,
            resiliencePolicy: selectedResilienceProfile.policy,
            directNativePlaybackRequired: true
        )
        _ = controllerStore.replace(with: nextController)
        previousController.dispose()
        configureSystemPlayback(for: preset.source, controller: nextController)
        initializeReplacementController(
            nextController,
            activeSource: preset.source,
            previousUiState: previousUiState,
            nativeFramePipelineSetting: .disabled
        )
        hostMessage = ExampleI18n.dolbyAcceptanceDirectRouteMessage
        selectedHdrEvidencePreset = preset.toHdrEvidencePreset()
        controlsVisible = true
        appendHostLog(
            title: ExampleI18n.logDolbyPlayNow,
            detail: preset.label
        )
        exampleIosHostLog(
            "dolby acceptance preset=\(preset.id) route=directNative sourceNormalizer=\(sourceNormalizerSetting.rawValue) nativeFrame=\(nativeFramePipelineSetting.rawValue)"
        )
    }

    private func initializeReplacementController(
        _ nextController: VesperPlayerController,
        activeSource: VesperPlayerSource?,
        previousUiState: PlayerHostUiState,
        nativeFramePipelineSetting: ExampleNativeFramePipelineSetting
    ) {
        Task { @MainActor in
            await Task.yield()
            guard controller === nextController else {
                return
            }
            if let activeSource {
                configureSystemPlayback(for: activeSource, controller: nextController)
            }
            nextController.initialize()
            if shouldRestorePosition(
                for: nativeFramePipelineSetting,
                controller: nextController
            ) {
                let restorePositionMs = previousUiState.timeline.positionMs
                if restorePositionMs > 0 {
                    nextController.seek(by: restorePositionMs)
                }
            }
            if previousUiState.playbackState == .playing {
                nextController.play()
            }
        }
    }

    private func shouldRestorePosition(
        for nativeFramePipelineSetting: ExampleNativeFramePipelineSetting,
        controller: VesperPlayerController
    ) -> Bool {
        switch nativeFramePipelineSetting {
        case .disabled, .diagnosticsOnly:
            return true
        case .preferNativeFrame:
            return controller.pluginDiagnostics.contains { diagnostic in
                diagnostic["pluginKind"] as? String == "native_frame_pipeline" &&
                    diagnostic["route"] as? String == "systemPlayer"
            }
        case .requireNativeFrame:
            return false
        }
    }

    private func selectSourceForPlayback(
        _ source: VesperPlayerSource,
        origin: ExamplePlaybackOrigin?
    ) {
        ensurePlaybackSafeNativeFrameSetting()
        activeDirectSource = source
        playbackOrigin = origin
        controller.selectSource(source)
        configureSystemPlayback(for: source)
    }

    private func ensurePlaybackSafeNativeFrameSetting() {
        let resolvedSetting = playbackSafeNativeFrameSetting(nativeFramePipelineSetting)
        guard resolvedSetting != nativeFramePipelineSetting else {
            return
        }
        nativeFramePipelineSetting = resolvedSetting
        hostMessage = ExampleI18n.nativeFrameRequireDowngradedForPlayback
    }

    private func configureSystemPlayback(
        for source: VesperPlayerSource,
        controller targetController: VesperPlayerController? = nil
    ) {
        let targetController = targetController ?? controller
        targetController.configureSystemPlayback(
            VesperSystemPlaybackConfiguration(
                metadata: VesperSystemPlaybackMetadata(
                    title: systemPlaybackTitle(for: source),
                    contentUri: source.uri
                ),
                controls: .videoDefault()
            )
        )
    }

    private func systemPlaybackTitle(for source: VesperPlayerSource) -> String {
        let label = source.label.trimmingCharacters(in: .whitespacesAndNewlines)
        if !label.isEmpty {
            return label
        }
        if let lastPathComponent = URL(string: source.uri)?.lastPathComponent,
           !lastPathComponent.isEmpty {
            return lastPathComponent
        }
        return source.uri
    }

    private func requestPictureInPicture() {
        guard pictureInPictureEnabled else {
            hostMessage = ExampleI18n.pictureInPictureUnavailable
            return
        }
        guard AVPictureInPictureController.isPictureInPictureSupported() else {
            hostMessage = ExampleI18n.pictureInPictureUnavailable
            return
        }
        guard let surfaceView = currentSurfaceView,
              !surfaceView.isNativeFramePresentationActive,
              let playerLayer = surfaceView.pictureInPicturePlayerLayer else {
            hostMessage = ExampleI18n.pictureInPictureUnavailable
            return
        }

        if pictureInPictureController?.playerLayer !== playerLayer {
            guard let nextController = AVPictureInPictureController(playerLayer: playerLayer) else {
                setPictureInPicturePresentation(false)
                hostMessage = ExampleI18n.pictureInPictureUnavailable
                pictureInPictureController = nil
                return
            }
            nextController.canStartPictureInPictureAutomaticallyFromInline =
                pictureInPictureEnabled
            pictureInPictureDelegate.onWillStart = {
                Task { @MainActor in
                    setPictureInPicturePresentation(true)
                }
            }
            pictureInPictureDelegate.onDidStart = {
                Task { @MainActor in
                    setPictureInPicturePresentation(true)
                }
            }
            pictureInPictureDelegate.onWillStop = {
                Task { @MainActor in
                    setPictureInPicturePresentation(true)
                }
            }
            pictureInPictureDelegate.onStop = {
                Task { @MainActor in
                    setPictureInPicturePresentation(false)
                    pictureInPictureController = nil
                }
            }
            pictureInPictureDelegate.onFailure = { _ in
                Task { @MainActor in
                    setPictureInPicturePresentation(false)
                    hostMessage = ExampleI18n.pictureInPictureUnavailable
                    pictureInPictureController = nil
                }
            }
            nextController.delegate = pictureInPictureDelegate
            pictureInPictureController = nextController
        }
        setPictureInPicturePresentation(true)
        pictureInPictureController?.startPictureInPicture()
    }

    private func activePlaylistSource() -> VesperPlayerSource? {
        guard let activeItemId = playlistCoordinator.snapshot.activeItem?.itemId else {
            return nil
        }
        return playlistCoordinator.snapshot.queue
            .first(where: { $0.item.itemId == activeItemId })?
            .item
            .source
    }

    private func activePlaybackSource() -> VesperPlayerSource? {
        activeDirectSource ?? activePlaylistSource()
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
        enqueueAndFocusPlaylistItem(
            IOS_REMOTE_PLAYLIST_ITEM_ID,
            logTitle: ExampleI18n.logSourceSelected,
            logDetail: trimmed
        )
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
                try await MainActor.run {
                    let taskId = try downloadManager.createTask(
                        assetId: assetId,
                        source: preparedTask.source,
                        profile: preparedTask.profile,
                        assetIndex: preparedTask.assetIndex
                    )
                    pendingDownloadTasks.removeAll { $0.id == assetId }
                    downloadMessage = taskId == nil ? ExampleI18n.downloadCreateTaskFailed : nil
                    if taskId == nil {
                        appendHostLog(
                            severity: .error,
                            title: ExampleI18n.logDownloadCreateFailure,
                            detail: assetId
                        )
                    }
                }
            } catch {
                await MainActor.run {
                    pendingDownloadTasks.removeAll { $0.id == assetId }
                    downloadMessage = ExampleI18n.downloadCreateTaskFailed
                    appendHostLog(
                        severity: .error,
                        title: ExampleI18n.logDownloadCreateFailure,
                        detail: error.localizedDescription
                    )
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
            task.source.contentFormat == .dashSegments ||
            task.source.contentFormat == .flvSegments
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

    private func shareDownloadOutput(_ task: VesperDownloadTaskSnapshot) {
        guard let presenter = topViewController() else {
            downloadAlertMessage = ExampleI18n.downloadSaveToPhotosFailed(
                ExampleI18n.downloadSaveToPhotosFailedUnknown
            )
            return
        }
        do {
            try downloadManager.shareTaskOutput(
                taskId: task.taskId,
                fileName: nil,
                mimeType: nil,
                from: presenter
            )
        } catch {
            downloadAlertMessage = ExampleI18n.downloadSaveToPhotosFailed(error.localizedDescription)
        }
    }

    private func pickVideo() {
        hostMessage = nil
        isVideoPickerPresented = true
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
        guard !pictureInPicturePresentation else {
            return
        }
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

    private func setPictureInPicturePresentation(_ enabled: Bool) {
        guard pictureInPicturePresentation != enabled else {
            return
        }
        pictureInPicturePresentation = enabled
        if enabled {
            controlsHideTask?.cancel()
            activeSheet = nil
            pendingSeekRatio = nil
            controlsVisible = false
        } else {
            controlsVisible = true
            scheduleControlsAutoHide(for: controller.uiState)
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
        guard
            !hasHandledFinishedPlayback,
            shouldAdvancePlaylistOnFinished(
                origin: playbackOrigin,
                activeItemId: activeItemId
            )
        else {
            return
        }
        hasHandledFinishedPlayback = true
        playlistCoordinator.handlePlaybackCompleted()
    }

    private func handlePickedVideo(_ item: PhotosPickerItem) async {
        do {
            guard let imported = try await item.loadTransferable(type: ImportedVideoTransferable.self) else {
                throw ExampleVideoImportError.noVideoFile
            }
            await MainActor.run {
                hostMessage = nil
                exampleIosHostLog("picked local video url=\(imported.url.absoluteString)")
                ensurePlaybackSafeNativeFrameSetting()
                queuedLocalSource = .localFile(url: imported.url, label: imported.label)
                let localItemId = iosLocalPlaylistItemId()
                let playlistWithoutPreviousLocalItems = playlistItemIds.filter {
                    !isIosLocalPlaylistItemId($0)
                }
                let nextPlaylistItemIds = enqueuePlaylistItem(
                    playlistWithoutPreviousLocalItems,
                    itemId: localItemId
                )
                applyPlaylistQueue(
                    focusItemId: localItemId,
                    playlistItemIds: nextPlaylistItemIds
                )
                appendHostLog(
                    title: ExampleI18n.logSourceSelected,
                    detail: imported.label
                )
                controlsVisible = true
            }
        } catch {
            await MainActor.run {
                hostMessage = ExampleI18n.failedToLoadSelectedPhotoVideo(error.localizedDescription)
                exampleIosHostLog("picked local video failed: \(error.localizedDescription)")
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

    private func topViewController() -> UIViewController? {
        let scenes = UIApplication.shared.connectedScenes.compactMap { $0 as? UIWindowScene }
        let window = scenes
            .flatMap(\.windows)
            .first(where: { $0.isKeyWindow })
        var controller = window?.rootViewController
        while let presented = controller?.presentedViewController {
            controller = presented
        }
        return controller
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

private struct ImportedVideoTransferable: Transferable {
    let url: URL
    let label: String

    static var transferRepresentation: some TransferRepresentation {
        FileRepresentation(contentType: .movie) { video in
            SentTransferredFile(video.url)
        } importing: { received in
            let fileExtension = received.file.pathExtension.isEmpty ? "mov" : received.file.pathExtension
            let destination = FileManager.default.temporaryDirectory
                .appendingPathComponent(UUID().uuidString)
                .appendingPathExtension(fileExtension)

            if FileManager.default.fileExists(atPath: destination.path) {
                try FileManager.default.removeItem(at: destination)
            }
            try FileManager.default.copyItem(at: received.file, to: destination)

            let label = received.file.lastPathComponent.isEmpty
                ? destination.lastPathComponent
                : received.file.lastPathComponent
            return ImportedVideoTransferable(url: destination, label: label)
        }
    }
}

private enum ExampleVideoImportError: LocalizedError {
    case noVideoFile

    var errorDescription: String? {
        switch self {
        case .noVideoFile:
            return ExampleI18n.failedToLoadSelectedVideoFromPhotos
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
            return ExampleI18n.downloadSaveToPhotosMissingOutput
        case .accessDenied:
            return ExampleI18n.photoLibraryAddAccessRequired
        case .saveFailed:
            return ExampleI18n.downloadSaveToPhotosFailed(ExampleI18n.downloadSaveToPhotosFailedUnknown)
        }
    }
}
