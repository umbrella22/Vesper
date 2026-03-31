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
            onThemeModeChange: { _ in },
            onPickFromPhotos: {},
            onUseHlsDemo: {},
            onOpenRemote: {}
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
