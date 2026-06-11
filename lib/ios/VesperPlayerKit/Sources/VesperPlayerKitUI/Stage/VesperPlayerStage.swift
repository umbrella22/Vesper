import SwiftUI
import VesperPlayerKit

@MainActor
public struct VesperPlayerStage: View {
    let surface: AnyView
    let uiState: PlayerHostUiState
    let trackCatalog: VesperTrackCatalog
    let trackSelection: VesperTrackSelectionSnapshot
    let effectiveVideoTrackId: String?
    let fixedTrackStatus: VesperFixedTrackStatus?
    @Binding var controlsVisible: Bool
    @Binding var pendingSeekRatio: Double?
    let isCompactLayout: Bool
    let isFullscreen: Bool
    let onSeekBy: (Int64) -> Void
    let onTogglePause: () -> Void
    let onSeekToRatio: (Double) -> Void
    let onSeekToLiveEdge: () -> Void
    let onSetPlaybackRate: (Float) -> Void
    let onToggleFullscreen: () -> Void
    let onOpenSheet: (VesperPlayerStageSheet) -> Void
    let currentBrightnessRatio: () -> Double?
    let onSetBrightnessRatio: (Double) -> Double?
    let currentVolumeRatio: () -> Double?
    let onSetVolumeRatio: (Double) -> Double?
    @State var stageGestureKind: StageAreaGestureKind?
    @State var deviceGestureStartRatio = 0.0
    @State var seekGestureRatio = 0.0
    @State var gestureFeedback: StageGestureFeedback?
    @State var gestureFeedbackTask: Task<Void, Never>?
    @State var speedGestureRestoreRate: Float?

    public init(
        surface: AnyView,
        uiState: PlayerHostUiState,
        trackCatalog: VesperTrackCatalog,
        trackSelection: VesperTrackSelectionSnapshot,
        effectiveVideoTrackId: String?,
        fixedTrackStatus: VesperFixedTrackStatus?,
        controlsVisible: Binding<Bool>,
        pendingSeekRatio: Binding<Double?>,
        isCompactLayout: Bool,
        isFullscreen: Bool,
        onSeekBy: @escaping (Int64) -> Void,
        onTogglePause: @escaping () -> Void,
        onSeekToRatio: @escaping (Double) -> Void,
        onSeekToLiveEdge: @escaping () -> Void,
        onSetPlaybackRate: @escaping (Float) -> Void = { _ in },
        onToggleFullscreen: @escaping () -> Void,
        onOpenSheet: @escaping (VesperPlayerStageSheet) -> Void,
        currentBrightnessRatio: @escaping () -> Double? = { nil },
        onSetBrightnessRatio: @escaping (Double) -> Double? = { _ in nil },
        currentVolumeRatio: @escaping () -> Double? = { nil },
        onSetVolumeRatio: @escaping (Double) -> Double? = { _ in nil }
    ) {
        self.surface = surface
        self.uiState = uiState
        self.trackCatalog = trackCatalog
        self.trackSelection = trackSelection
        self.effectiveVideoTrackId = effectiveVideoTrackId
        self.fixedTrackStatus = fixedTrackStatus
        _controlsVisible = controlsVisible
        _pendingSeekRatio = pendingSeekRatio
        self.isCompactLayout = isCompactLayout
        self.isFullscreen = isFullscreen
        self.onSeekBy = onSeekBy
        self.onTogglePause = onTogglePause
        self.onSeekToRatio = onSeekToRatio
        self.onSeekToLiveEdge = onSeekToLiveEdge
        self.onSetPlaybackRate = onSetPlaybackRate
        self.onToggleFullscreen = onToggleFullscreen
        self.onOpenSheet = onOpenSheet
        self.currentBrightnessRatio = currentBrightnessRatio
        self.onSetBrightnessRatio = onSetBrightnessRatio
        self.currentVolumeRatio = currentVolumeRatio
        self.onSetVolumeRatio = onSetVolumeRatio
    }

