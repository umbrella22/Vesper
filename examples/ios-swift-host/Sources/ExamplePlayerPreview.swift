import SwiftUI
import VesperPlayerKit

#Preview("Player Stage Dark") {
    ZStack {
        LinearGradient(
            colors: [Color(red: 0.047, green: 0.063, blue: 0.098), Color(red: 0.023, green: 0.027, blue: 0.043)],
            startPoint: .top,
            endPoint: .bottom
        )
        .ignoresSafeArea()

        ExamplePlayerStage(
            surface: AnyView(
                LinearGradient(
                    colors: [Color.black, Color(red: 0.11, green: 0.12, blue: 0.18)],
                    startPoint: .topLeading,
                    endPoint: .bottomTrailing
                )
            ),
            uiState: previewPlayerUiState(),
            trackCatalog: previewTrackCatalog(),
            trackSelection: previewTrackSelection(),
            effectiveVideoTrackId: "video:hls:cavc1:b1500000:w1280:h720:f3000",
            fixedTrackStatus: nil,
            controlsVisible: .constant(true),
            pendingSeekRatio: .constant(nil),
            isCompactLayout: true,
            isFullscreen: false,
            onSeekBy: { _ in },
            onTogglePause: {},
            onSeekToRatio: { _ in },
            onSeekToLiveEdge: {},
            onToggleFullscreen: {},
            onOpenSheet: { _ in }
        )
        .frame(height: 248)
        .padding(20)
    }
}

#Preview("Player Stage Fullscreen Dark") {
    ExamplePlayerStage(
        surface: AnyView(
            LinearGradient(
                colors: [Color.black, Color(red: 0.11, green: 0.12, blue: 0.18)],
                startPoint: .topLeading,
                endPoint: .bottomTrailing
            )
        ),
        uiState: previewPlayerUiState(),
        trackCatalog: previewTrackCatalog(),
        trackSelection: previewTrackSelection(),
        effectiveVideoTrackId: "video:hls:cavc1:b1500000:w1280:h720:f3000",
        fixedTrackStatus: nil,
        controlsVisible: .constant(true),
        pendingSeekRatio: .constant(nil),
        isCompactLayout: true,
        isFullscreen: true,
        onSeekBy: { _ in },
        onTogglePause: {},
        onSeekToRatio: { _ in },
        onSeekToLiveEdge: {},
        onToggleFullscreen: {},
        onOpenSheet: { _ in }
    )
    .background(Color.black)
}

#Preview("Sources Light") {
    let palette = exampleHostPalette(useDarkTheme: false)
    ZStack {
        LinearGradient(
            colors: [palette.pageTop, palette.pageBottom],
            startPoint: .top,
            endPoint: .bottom
        )
        .ignoresSafeArea()

        ExampleSourceSection(
            palette: palette,
            themeMode: .system,
            remoteStreamUrl: .constant(IOS_HLS_DEMO_URL),
            hostMessage: nil,
            dashDemoEnabled: false,
            dashDemoNote: ExampleI18n.dashDemoUnavailableNote,
            onThemeModeChange: { _ in },
            onPickVideo: {},
            onUseHlsDemo: {},
            onUseDashDemo: {},
            onUseLiveDvrAcceptance: {},
            onOpenRemote: {}
        )
        .padding(20)
    }
}

#Preview("Playlist Light") {
    let palette = exampleHostPalette(useDarkTheme: false)
    ZStack {
        LinearGradient(
            colors: [palette.pageTop, palette.pageBottom],
            startPoint: .top,
            endPoint: .bottom
        )
        .ignoresSafeArea()

        ExamplePlaylistSection(
            palette: palette,
            playlistQueue: previewPlaylistQueue(),
            onFocusPlaylistItem: { _ in }
        )
        .padding(20)
    }
}

#Preview("Resilience Light") {
    let palette = exampleHostPalette(useDarkTheme: false)
    ZStack {
        LinearGradient(
            colors: [palette.pageTop, palette.pageBottom],
            startPoint: .top,
            endPoint: .bottom
        )
        .ignoresSafeArea()

        ExampleResilienceSection(
            palette: palette,
            selectedProfile: .balanced,
            isApplyingProfile: true,
            onApplyProfile: { _ in }
        )
        .padding(20)
    }
}

#Preview("Sheet Menu Dark") {
    ExampleSelectionSheetContent(
        sheet: .menu,
        uiState: previewPlayerUiState(),
        trackCatalog: previewTrackCatalog(),
        trackSelection: previewTrackSelection(),
        effectiveVideoTrackId: "video:hls:cavc1:b1500000:w1280:h720:f3000",
        videoVariantObservation: VesperVideoVariantObservation(
            bitRate: 1_500_000,
            width: 1280,
            height: 720
        ),
        fixedTrackStatus: nil,
        lastError: nil,
        onOpenSheet: { _ in },
        onSelectQuality: { _ in },
        onSelectAudio: { _ in },
        onSelectSubtitle: { _ in },
        onSelectSpeed: { _ in }
    )
}

#Preview("Sheet Quality Dark") {
    ExampleSelectionSheetContent(
        sheet: .quality,
        uiState: previewPlayerUiState(),
        trackCatalog: previewTrackCatalog(),
        trackSelection: previewTrackSelection(),
        effectiveVideoTrackId: "video:hls:cavc1:b1500000:w1280:h720:f3000",
        videoVariantObservation: VesperVideoVariantObservation(
            bitRate: 1_500_000,
            width: 1280,
            height: 720
        ),
        fixedTrackStatus: nil,
        lastError: nil,
        onOpenSheet: { _ in },
        onSelectQuality: { _ in },
        onSelectAudio: { _ in },
        onSelectSubtitle: { _ in },
        onSelectSpeed: { _ in }
    )
}

private func previewPlayerUiState() -> PlayerHostUiState {
    PlayerHostUiState(
        title: "Vesper",
        subtitle: "iOS native player host",
        sourceLabel: "VID_20260216_223628.mp4",
        playbackState: .playing,
        playbackRate: 1.0,
        isBuffering: false,
        isInterrupted: false,
        timeline: TimelineUiState(
            kind: .vod,
            isSeekable: true,
            seekableRange: SeekableRangeUi(startMs: 0, endMs: 48_000),
            liveEdgeMs: nil,
            positionMs: 2_000,
            durationMs: 48_000
        )
    )
}

private func previewTrackCatalog() -> VesperTrackCatalog {
    VesperTrackCatalog(
        tracks: [
            VesperMediaTrack(
                id: "video:hls:cavc1:b1500000:w1280:h720:f3000",
                kind: .video,
                label: "720p",
                codec: "avc1",
                bitRate: 1_500_000,
                width: 1280,
                height: 720,
                frameRate: 30,
                isDefault: true
            ),
            VesperMediaTrack(
                id: "video:hls:cavc1:b2500000:w1920:h1080:f3000",
                kind: .video,
                label: "1080p",
                codec: "avc1",
                bitRate: 2_500_000,
                width: 1920,
                height: 1080,
                frameRate: 30
            ),
            VesperMediaTrack(
                id: "audio-ja",
                kind: .audio,
                label: "Japanese",
                language: "ja",
                channels: 2,
                sampleRate: 48_000
            ),
            VesperMediaTrack(
                id: "subtitle-zh",
                kind: .subtitle,
                label: "简体中文",
                language: "zh",
                isDefault: true
            ),
        ],
        adaptiveVideo: true,
        adaptiveAudio: false
    )
}

private func previewTrackSelection() -> VesperTrackSelectionSnapshot {
    VesperTrackSelectionSnapshot(
        video: .auto(),
        audio: .track("audio-ja"),
        subtitle: .track("subtitle-zh"),
        abrPolicy: .auto()
    )
}

private func previewPlaylistQueue() -> [VesperPlaylistQueueItemState] {
    let queue = examplePlaylistQueue(
        playlistItemIds: [
            IOS_HLS_PLAYLIST_ITEM_ID,
            IOS_LOCAL_PLAYLIST_ITEM_ID,
            IOS_REMOTE_PLAYLIST_ITEM_ID,
        ],
        remoteSource: .remoteUrl(
            URL(string: "https://example.com/preview.mp4")!,
            label: ExampleI18n.customRemoteUrlLabel
        ),
        localSource: .localFile(
            url: URL(fileURLWithPath: "/tmp/preview.mov"),
            label: "Preview.mov"
        )
    )
    let hints = examplePlaylistViewportHints(queue: queue, focusedItemId: IOS_HLS_PLAYLIST_ITEM_ID)
    let hintByItemId = Dictionary(uniqueKeysWithValues: hints.map { ($0.itemId, $0.kind) })

    return queue.enumerated().map { index, item in
        VesperPlaylistQueueItemState(
            item: item,
            index: index,
            viewportHint: hintByItemId[item.itemId] ?? .hidden,
            isActive: item.itemId == IOS_HLS_PLAYLIST_ITEM_ID
        )
    }
}
