import SwiftUI
import VesperPlayerKit

@MainActor
public struct VesperPlayerStage: View {
    let surface: AnyView
    let contentOverlay: AnyView?
    let landscapeControlBarLeading: AnyView?
    let uiState: PlayerHostUiState
    let trackCatalog: VesperTrackCatalog
    let trackSelection: VesperTrackSelectionSnapshot
    let effectiveVideoTrackId: String?
    let fixedTrackStatus: VesperFixedTrackStatus?
    @Binding var controlsVisible: Bool
    @Binding var pendingSeekRatio: Double?
    let isCompactLayout: Bool
    let isFullscreen: Bool
    let pictureInPicturePresentation: Bool
    let onSeekBy: (Int64) -> Void
    let onTogglePause: () -> Void
    let onSeekToRatio: (Double) -> Void
    let onSeekToLiveEdge: () -> Void
    let onSetPlaybackRate: (Float) -> Void
    let onToggleFullscreen: () -> Void
    let onNavigateBack: (() -> Void)?
    let navigateBackAccessibilityLabel: String?
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

    /// Creates a Stage with optional host content, landscape controls, and a
    /// state-labelled navigation action.
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
        pictureInPicturePresentation: Bool = false,
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
        onSetVolumeRatio: @escaping (Double) -> Double? = { _ in nil },
        contentOverlay: AnyView? = nil,
        landscapeControlBarLeading: AnyView? = nil,
        onNavigateBack: (() -> Void)? = nil,
        navigateBackAccessibilityLabel: String? = nil
    ) {
        self.surface = surface
        self.contentOverlay = contentOverlay
        self.landscapeControlBarLeading = landscapeControlBarLeading
        self.uiState = uiState
        self.trackCatalog = trackCatalog
        self.trackSelection = trackSelection
        self.effectiveVideoTrackId = effectiveVideoTrackId
        self.fixedTrackStatus = fixedTrackStatus
        _controlsVisible = controlsVisible
        _pendingSeekRatio = pendingSeekRatio
        self.isCompactLayout = isCompactLayout
        self.isFullscreen = isFullscreen
        self.pictureInPicturePresentation = pictureInPicturePresentation
        self.onSeekBy = onSeekBy
        self.onTogglePause = onTogglePause
        self.onSeekToRatio = onSeekToRatio
        self.onSeekToLiveEdge = onSeekToLiveEdge
        self.onSetPlaybackRate = onSetPlaybackRate
        self.onToggleFullscreen = onToggleFullscreen
        self.onNavigateBack = onNavigateBack
        self.navigateBackAccessibilityLabel = navigateBackAccessibilityLabel
        self.onOpenSheet = onOpenSheet
        self.currentBrightnessRatio = currentBrightnessRatio
        self.onSetBrightnessRatio = onSetBrightnessRatio
        self.currentVolumeRatio = currentVolumeRatio
        self.onSetVolumeRatio = onSetVolumeRatio
    }
}
