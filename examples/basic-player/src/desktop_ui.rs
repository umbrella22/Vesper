#![cfg_attr(target_os = "macos", allow(dead_code))]

use std::time::Duration;

use player_render_wgpu::{
    PLAYBACK_RATES, PlaybackLayoutMetrics, PlaybackRect, PlaybackSeekPreview, PlaybackSeekRange,
    PlaybackTimelineKind, PlaybackUiAction, PlaybackUiViewModel,
};
use player_runtime::{PlayerSeekableRange, PlayerSnapshot, PlayerTimelineKind, PresentationState};

pub const CONTROL_RATES: &[(f32, &str)] = PLAYBACK_RATES;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ControlAction {
    SeekStart,
    SeekBack,
    TogglePause,
    Stop,
    SeekForward,
    SeekEnd,
    SetRate(f32),
    SeekToRatio(f32),
    OpenLocalFile,
    OpenHlsDemo,
    OpenDashDemo,
    SelectSidebarTab(DesktopSidebarTab),
    FocusPlaylistItem(usize),
    CreateDownloadHlsDemo,
    CreateDownloadDashDemo,
    CreateDownloadCurrentSource,
    DownloadPrimaryAction(u64),
    DownloadExport(u64),
    DownloadRemove(u64),
}

impl ControlAction {
    /// Converts a renderer playback action into the example's broader action
    /// space. Sidebar, download, and source actions remain example-owned.
    pub fn from_playback_action(action: PlaybackUiAction) -> Self {
        match action {
            PlaybackUiAction::SeekStart => Self::SeekStart,
            PlaybackUiAction::SeekBack => Self::SeekBack,
            PlaybackUiAction::TogglePause => Self::TogglePause,
            PlaybackUiAction::Stop => Self::Stop,
            PlaybackUiAction::SeekForward => Self::SeekForward,
            PlaybackUiAction::SeekEnd => Self::SeekEnd,
            PlaybackUiAction::SetRate(rate) => Self::SetRate(rate),
            PlaybackUiAction::SeekToRatio(ratio) => Self::SeekToRatio(ratio),
        }
    }

    pub fn as_playback_action(self) -> Option<PlaybackUiAction> {
        match self {
            Self::SeekStart => Some(PlaybackUiAction::SeekStart),
            Self::SeekBack => Some(PlaybackUiAction::SeekBack),
            Self::TogglePause => Some(PlaybackUiAction::TogglePause),
            Self::Stop => Some(PlaybackUiAction::Stop),
            Self::SeekForward => Some(PlaybackUiAction::SeekForward),
            Self::SeekEnd => Some(PlaybackUiAction::SeekEnd),
            Self::SetRate(rate) => Some(PlaybackUiAction::SetRate(rate)),
            Self::SeekToRatio(ratio) => Some(PlaybackUiAction::SeekToRatio(ratio)),
            Self::OpenLocalFile
            | Self::OpenHlsDemo
            | Self::OpenDashDemo
            | Self::SelectSidebarTab(_)
            | Self::FocusPlaylistItem(_)
            | Self::CreateDownloadHlsDemo
            | Self::CreateDownloadDashDemo
            | Self::CreateDownloadCurrentSource
            | Self::DownloadPrimaryAction(_)
            | Self::DownloadExport(_)
            | Self::DownloadRemove(_) => None,
        }
    }
}

pub type SeekPreview = PlaybackSeekPreview;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DesktopSidebarTab {
    Playlist,
    Downloads,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DesktopPlaylistItemViewData {
    pub label: String,
    pub status: String,
    pub is_active: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DesktopPendingDownloadTaskViewData {
    pub asset_id: String,
    pub label: String,
    pub source_uri: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DesktopDownloadTaskViewData {
    pub task_id: u64,
    pub label: String,
    pub status: String,
    pub progress_summary: String,
    pub progress_ratio: Option<f32>,
    pub completed_path: Option<String>,
    pub error_message: Option<String>,
    pub primary_action_label: Option<String>,
    pub export_action_label: Option<String>,
    pub is_export_enabled: bool,
    pub is_remove_enabled: bool,
    pub is_exporting: bool,
    pub export_progress: Option<f32>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DesktopOverlayViewModel {
    pub source_label: String,
    pub playback_state_label: String,
    pub subtitle: String,
    pub controls_opacity: f32,
    pub cursor_position: Option<(u32, u32)>,
    pub sidebar_tab: DesktopSidebarTab,
    pub playlist_items: Vec<DesktopPlaylistItemViewData>,
    pub pending_downloads: Vec<DesktopPendingDownloadTaskViewData>,
    pub download_tasks: Vec<DesktopDownloadTaskViewData>,
    pub host_message: Option<String>,
    pub download_message: Option<String>,
    pub export_plugin_installed: bool,
}

pub type DesktopUiRect = PlaybackRect;
pub type DesktopUiLayoutMetrics = PlaybackLayoutMetrics;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DesktopUiTimelineKind {
    Vod,
    Live,
    LiveDvr,
}

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
impl DesktopUiTimelineKind {
    pub fn from_player(kind: PlayerTimelineKind) -> Self {
        match kind {
            PlayerTimelineKind::Vod => Self::Vod,
            PlayerTimelineKind::Live => Self::Live,
            PlayerTimelineKind::LiveDvr => Self::LiveDvr,
        }
    }

    pub fn as_raw(self) -> u32 {
        match self {
            Self::Vod => 0,
            Self::Live => 1,
            Self::LiveDvr => 2,
        }
    }
}

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
#[derive(Debug, Clone)]
pub struct DesktopUiViewModel {
    /// Renderer-owned playback model; the surrounding fields keep the
    /// example's existing text and drawing adapters source-compatible.
    pub playback_ui: PlaybackUiViewModel,
    pub timeline_kind: DesktopUiTimelineKind,
    pub is_playing: bool,
    pub is_seekable: bool,
    pub can_scrub: bool,
    pub controls_visible: bool,
    pub has_duration: bool,
    pub playback_rate: f32,
    pub displayed_position: Duration,
    pub duration: Option<Duration>,
    pub seekable_range: Option<PlayerSeekableRange>,
    pub displayed_progress_ratio: Option<f64>,
    pub play_pause_label: &'static str,
    pub time_label: String,
}

impl DesktopUiViewModel {
    pub fn from_snapshot(
        snapshot: &PlayerSnapshot,
        controls_visible: bool,
        seek_preview: Option<SeekPreview>,
    ) -> Self {
        let timeline_kind = DesktopUiTimelineKind::from_player(snapshot.timeline.kind);
        let displayed_position = seek_preview
            .map(|preview| preview.position)
            .unwrap_or(snapshot.timeline.position);
        let duration = snapshot.timeline.duration.or(snapshot.progress.duration());
        let has_duration = duration.is_some();
        let displayed_progress_ratio = seek_preview
            .map(|preview| preview.ratio)
            .or_else(|| snapshot.timeline.displayed_ratio());
        let play_pause_label = play_pause_label(snapshot.state);
        let can_scrub = is_scrubbable_timeline(snapshot);
        let playback_ui = PlaybackUiViewModel::new(
            playback_timeline_kind(snapshot.timeline.kind),
            snapshot.state == PresentationState::Playing,
            snapshot.timeline.is_seekable,
            controls_visible,
            snapshot.playback_rate,
            displayed_position,
            duration,
            snapshot
                .timeline
                .seekable_range
                .map(|range| PlaybackSeekRange {
                    start: range.start,
                    end: range.end,
                }),
            displayed_progress_ratio,
        );
        let time_label = duration
            .map(|duration| {
                format!(
                    "{}/{}",
                    format_duration(displayed_position),
                    format_duration(duration)
                )
            })
            .unwrap_or_else(|| format_duration(displayed_position));

        Self {
            playback_ui,
            timeline_kind,
            is_playing: snapshot.state == PresentationState::Playing,
            is_seekable: snapshot.timeline.is_seekable,
            can_scrub,
            controls_visible,
            has_duration,
            playback_rate: snapshot.playback_rate,
            displayed_position,
            duration,
            seekable_range: snapshot.timeline.seekable_range,
            displayed_progress_ratio,
            play_pause_label,
            time_label,
        }
    }

    pub fn renderer_action(&self, action: PlaybackUiAction) -> ControlAction {
        ControlAction::from_playback_action(action)
    }

    pub fn is_rate_active(&self, rate: f32) -> bool {
        (self.playback_rate - rate).abs() < 0.05
    }
}

pub fn playback_state_label(state: PresentationState) -> &'static str {
    match state {
        PresentationState::Ready => "READY",
        PresentationState::Playing => "PLAYING",
        PresentationState::Paused => "PAUSED",
        PresentationState::Finished => "FINISHED",
    }
}

pub fn is_scrubbable_timeline(snapshot: &PlayerSnapshot) -> bool {
    is_scrubbable(
        snapshot.timeline.kind,
        snapshot.timeline.is_seekable,
        snapshot.timeline.seekable_range.is_some(),
    )
}

pub fn format_duration(duration: Duration) -> String {
    let total_seconds = duration.as_secs();
    let minutes = total_seconds / 60;
    let seconds = total_seconds % 60;

    format!("{minutes:02}:{seconds:02}")
}

fn play_pause_label(state: PresentationState) -> &'static str {
    if matches!(state, PresentationState::Playing) {
        "||"
    } else {
        "|>"
    }
}

fn is_scrubbable(kind: PlayerTimelineKind, is_seekable: bool, has_seekable_range: bool) -> bool {
    is_seekable
        && has_seekable_range
        && matches!(kind, PlayerTimelineKind::Vod | PlayerTimelineKind::LiveDvr)
}

fn playback_timeline_kind(kind: PlayerTimelineKind) -> PlaybackTimelineKind {
    match kind {
        PlayerTimelineKind::Vod => PlaybackTimelineKind::Vod,
        PlayerTimelineKind::Live => PlaybackTimelineKind::Live,
        PlayerTimelineKind::LiveDvr => PlaybackTimelineKind::LiveDvr,
    }
}

#[cfg(test)]
mod tests {
    use super::{
        ControlAction, DesktopUiLayoutMetrics, DesktopUiTimelineKind, DesktopUiViewModel,
        SeekPreview,
    };
    use player_render_wgpu::PlaybackUiAction;
    use player_runtime::{
        MediaSourceKind, MediaSourceProtocol, MediaTrackCatalog, MediaTrackSelectionSnapshot,
        PlaybackProgress, PlayerMediaInfo, PlayerResilienceMetrics, PlayerSnapshot,
        PlayerTimelineKind, PlayerTimelineSnapshot, PresentationState,
    };
    use std::time::Duration;

    #[test]
    fn view_model_prefers_seek_preview_for_vod() {
        let snapshot = test_snapshot(
            PlayerTimelineSnapshot {
                kind: PlayerTimelineKind::Vod,
                is_seekable: true,
                seekable_range: Some(player_runtime::PlayerSeekableRange {
                    start: Duration::ZERO,
                    end: Duration::from_secs(120),
                }),
                live_edge: None,
                position: Duration::from_secs(30),
                duration: Some(Duration::from_secs(120)),
            },
            PresentationState::Playing,
            PlaybackProgress::new(Duration::from_secs(30), Some(Duration::from_secs(120))),
        );

        let view_model = DesktopUiViewModel::from_snapshot(
            &snapshot,
            true,
            Some(SeekPreview {
                position: Duration::from_secs(45),
                ratio: 0.375,
            }),
        );

        assert_eq!(view_model.timeline_kind, DesktopUiTimelineKind::Vod);
        assert_eq!(view_model.displayed_position, Duration::from_secs(45));
        assert_eq!(view_model.displayed_progress_ratio, Some(0.375));
        assert_eq!(view_model.play_pause_label, "||");
        assert_eq!(view_model.time_label, "00:45/02:00");
        assert!(view_model.can_scrub);
        assert!(view_model.controls_visible);
        assert_eq!(
            view_model.renderer_action(PlaybackUiAction::SeekForward),
            ControlAction::SeekForward
        );
    }

    #[test]
    fn renderer_playback_actions_round_trip_through_host_adapter() {
        let actions = [
            PlaybackUiAction::SeekStart,
            PlaybackUiAction::SeekBack,
            PlaybackUiAction::TogglePause,
            PlaybackUiAction::Stop,
            PlaybackUiAction::SeekForward,
            PlaybackUiAction::SeekEnd,
            PlaybackUiAction::SetRate(1.5),
            PlaybackUiAction::SeekToRatio(0.25),
        ];

        for action in actions {
            let host_action = ControlAction::from_playback_action(action);
            assert_eq!(host_action.as_playback_action(), Some(action));
        }
    }

    #[test]
    fn view_model_formats_live_without_placeholder_duration() {
        let snapshot = test_snapshot(
            PlayerTimelineSnapshot {
                kind: PlayerTimelineKind::Live,
                is_seekable: false,
                seekable_range: None,
                live_edge: None,
                position: Duration::from_secs(15),
                duration: None,
            },
            PresentationState::Paused,
            PlaybackProgress::new(Duration::from_secs(15), None),
        );

        let view_model = DesktopUiViewModel::from_snapshot(&snapshot, false, None);

        assert_eq!(view_model.timeline_kind, DesktopUiTimelineKind::Live);
        assert_eq!(view_model.displayed_progress_ratio, None);
        assert_eq!(view_model.play_pause_label, "|>");
        assert_eq!(view_model.time_label, "00:15");
        assert!(!view_model.can_scrub);
        assert!(!view_model.has_duration);
        assert!(!view_model.controls_visible);
    }

    #[test]
    fn layout_metrics_match_desktop_baseline() {
        let metrics = DesktopUiLayoutMetrics::for_surface(1280, 720).expect("layout metrics");

        assert_eq!(metrics.bar_height, 88);
        assert_eq!(metrics.padding, 17);
        assert_eq!(metrics.gap, 8);
        assert_eq!(metrics.icon_size, 54);
        assert_eq!(metrics.rate_width, 74);
        assert_eq!(metrics.progress_height, 4);
        assert_eq!(metrics.progress_hit_slop_top, 8);
        assert_eq!(metrics.progress_hit_slop_bottom, 4);
        assert_eq!(metrics.time_label_height, 14);
    }

    #[test]
    fn progress_hit_rect_expands_track_touch_area() {
        let metrics = DesktopUiLayoutMetrics::for_surface(1280, 720).expect("layout metrics");
        let hit_rect = metrics.progress_hit_rect(1280, 720);

        assert_eq!(hit_rect.x, 0);
        assert_eq!(hit_rect.y, 624);
        assert_eq!(hit_rect.width, 1280);
        assert_eq!(hit_rect.height, 16);
        assert!(hit_rect.contains(32, 624));
        assert!(hit_rect.contains(32, 639));
    }

    fn test_snapshot(
        timeline: PlayerTimelineSnapshot,
        state: PresentationState,
        progress: PlaybackProgress,
    ) -> PlayerSnapshot {
        PlayerSnapshot {
            source_uri: "test://media".to_owned(),
            state,
            has_video_surface: true,
            is_interrupted: false,
            is_buffering: false,
            playback_rate: 1.0,
            progress,
            timeline,
            media_info: PlayerMediaInfo {
                source_uri: "test://media".to_owned(),
                source_kind: MediaSourceKind::Local,
                source_protocol: MediaSourceProtocol::File,
                duration: progress.duration(),
                bit_rate: None,
                audio_streams: 0,
                video_streams: 0,
                best_video: None,
                best_audio: None,
                track_catalog: MediaTrackCatalog::default(),
                track_selection: MediaTrackSelectionSnapshot::default(),
            },
            resilience_metrics: PlayerResilienceMetrics::default(),
        }
    }
}
