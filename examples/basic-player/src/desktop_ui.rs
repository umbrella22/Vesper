#![cfg_attr(target_os = "macos", allow(dead_code))]

use std::time::Duration;

use player_runtime::{PlayerSeekableRange, PlayerSnapshot, PlayerTimelineKind, PresentationState};

pub const CONTROL_RATES: &[(f32, &str)] = &[
    (0.5, "0.5X"),
    (1.0, "1X"),
    (1.5, "1.5X"),
    (2.0, "2X"),
    (3.0, "3X"),
];

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

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SeekPreview {
    pub position: Duration,
    pub ratio: f64,
}

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DesktopUiRect {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

impl DesktopUiRect {
    pub fn contains(self, x: u32, y: u32) -> bool {
        x >= self.x
            && x < self.x.saturating_add(self.width)
            && y >= self.y
            && y < self.y.saturating_add(self.height)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DesktopUiLayoutMetrics {
    pub bar_height: u32,
    pub padding: u32,
    pub gap: u32,
    pub icon_size: u32,
    pub rate_width: u32,
    pub progress_height: u32,
    pub progress_hit_slop_top: u32,
    pub progress_hit_slop_bottom: u32,
    pub time_label_height: u32,
}

impl DesktopUiLayoutMetrics {
    pub fn for_surface(frame_width: u32, frame_height: u32) -> Option<Self> {
        if frame_width == 0 || frame_height == 0 {
            return None;
        }

        let bar_height = (frame_height / 5).clamp(60, 88);
        let padding = (bar_height / 5).max(8);
        let gap = (padding / 2).max(8);
        let icon_size = bar_height.saturating_sub(padding * 2);
        let rate_width = (icon_size + 20).max(58);

        Some(Self {
            bar_height,
            padding,
            gap,
            icon_size,
            rate_width,
            progress_height: 4,
            progress_hit_slop_top: 8,
            progress_hit_slop_bottom: 4,
            time_label_height: 14,
        })
    }

    pub fn overlay_origin_y(self, frame_height: u32) -> u32 {
        frame_height.saturating_sub(self.bar_height)
    }

    pub fn button_origin_y(self, frame_height: u32) -> u32 {
        self.overlay_origin_y(frame_height)
            .saturating_add(self.padding)
    }

    pub fn progress_rect(self, frame_width: u32, frame_height: u32) -> DesktopUiRect {
        DesktopUiRect {
            x: 0,
            y: self.overlay_origin_y(frame_height),
            width: frame_width,
            height: self.progress_height,
        }
    }

    pub fn progress_hit_rect(self, frame_width: u32, frame_height: u32) -> DesktopUiRect {
        let progress_rect = self.progress_rect(frame_width, frame_height);
        let y = progress_rect.y.saturating_sub(self.progress_hit_slop_top);
        let bottom = progress_rect
            .y
            .saturating_add(progress_rect.height)
            .saturating_add(self.progress_hit_slop_bottom)
            .min(frame_height);

        DesktopUiRect {
            x: progress_rect.x,
            y,
            width: progress_rect.width,
            height: bottom.saturating_sub(y),
        }
    }

    pub fn time_label_offset_y(self) -> u32 {
        self.bar_height.saturating_sub(self.time_label_height) / 2
    }
}

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
        let can_scrub = is_scrubbable(snapshot.timeline.kind, snapshot.timeline.is_seekable);
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
    is_scrubbable(snapshot.timeline.kind, snapshot.timeline.is_seekable)
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

fn is_scrubbable(kind: PlayerTimelineKind, is_seekable: bool) -> bool {
    is_seekable && matches!(kind, PlayerTimelineKind::Vod | PlayerTimelineKind::LiveDvr)
}

#[cfg(test)]
mod tests {
    use super::{DesktopUiLayoutMetrics, DesktopUiTimelineKind, DesktopUiViewModel, SeekPreview};
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
