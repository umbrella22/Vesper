//! Pure data playback controls shared by desktop renderers.
//!
//! The component intentionally knows nothing about a runtime, a windowing
//! toolkit, or a media backend. Hosts turn the explicit actions into their
//! own commands and may use the layout/primitives to draw an overlay.

use std::time::Duration;

pub const PLAYBACK_RATES: &[(f32, &str)] = &[
    (0.5, "0.5X"),
    (1.0, "1X"),
    (1.5, "1.5X"),
    (2.0, "2X"),
    (3.0, "3X"),
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlaybackTimelineKind {
    Vod,
    Live,
    LiveDvr,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PlaybackUiAction {
    SeekStart,
    SeekBack,
    TogglePause,
    Stop,
    SeekForward,
    SeekEnd,
    SetRate(f32),
    SeekToRatio(f32),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlaybackRect {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

impl PlaybackRect {
    pub fn contains(self, x: u32, y: u32) -> bool {
        x >= self.x
            && x < self.x.saturating_add(self.width)
            && y >= self.y
            && y < self.y.saturating_add(self.height)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlaybackLayoutMetrics {
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

impl PlaybackLayoutMetrics {
    pub fn for_surface(frame_width: u32, frame_height: u32) -> Option<Self> {
        if frame_width == 0 || frame_height == 0 {
            return None;
        }

        let bar_height = (frame_height / 5).clamp(24, 88).min(frame_height);
        let padding = (bar_height / 5)
            .max(1)
            .min(bar_height.saturating_sub(1) / 2);
        let gap = (padding / 2).max(1);
        let icon_size = bar_height.saturating_sub(padding.saturating_mul(2)).max(1);
        let rate_width = icon_size.saturating_add(20).max(24);

        Some(Self {
            bar_height,
            padding,
            gap,
            icon_size,
            rate_width,
            progress_height: 4.min(bar_height),
            progress_hit_slop_top: 8,
            progress_hit_slop_bottom: 4,
            time_label_height: 14.min(bar_height),
        })
    }

    pub fn overlay_origin_y(self, frame_height: u32) -> u32 {
        frame_height.saturating_sub(self.bar_height)
    }

    pub fn button_origin_y(self, frame_height: u32) -> u32 {
        self.overlay_origin_y(frame_height)
            .saturating_add(self.padding)
    }

    pub fn progress_rect(self, frame_width: u32, frame_height: u32) -> PlaybackRect {
        PlaybackRect {
            x: 0,
            y: self.overlay_origin_y(frame_height),
            width: frame_width,
            height: self.progress_height,
        }
    }

    pub fn progress_hit_rect(self, frame_width: u32, frame_height: u32) -> PlaybackRect {
        let progress_rect = self.progress_rect(frame_width, frame_height);
        let y = progress_rect.y.saturating_sub(self.progress_hit_slop_top);
        let bottom = progress_rect
            .y
            .saturating_add(progress_rect.height)
            .saturating_add(self.progress_hit_slop_bottom)
            .min(frame_height);

        PlaybackRect {
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
pub struct PlaybackSeekRange {
    pub start: Duration,
    pub end: Duration,
}

impl PlaybackSeekRange {
    pub fn normalized(self) -> Option<Self> {
        if self.end <= self.start {
            None
        } else {
            Some(self)
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PlaybackSeekPreview {
    pub position: Duration,
    pub ratio: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PlaybackUiViewModel {
    pub timeline_kind: PlaybackTimelineKind,
    pub is_playing: bool,
    pub is_seekable: bool,
    pub can_scrub: bool,
    pub controls_visible: bool,
    pub playback_rate: f32,
    pub displayed_position: Duration,
    pub duration: Option<Duration>,
    pub seekable_range: Option<PlaybackSeekRange>,
    pub displayed_progress_ratio: Option<f64>,
}

impl PlaybackUiViewModel {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        timeline_kind: PlaybackTimelineKind,
        is_playing: bool,
        is_seekable: bool,
        controls_visible: bool,
        playback_rate: f32,
        position: Duration,
        duration: Option<Duration>,
        seekable_range: Option<PlaybackSeekRange>,
        displayed_progress_ratio: Option<f64>,
    ) -> Self {
        let seekable_range = seekable_range.and_then(PlaybackSeekRange::normalized);
        let can_scrub = is_seekable
            && matches!(
                timeline_kind,
                PlaybackTimelineKind::Vod | PlaybackTimelineKind::LiveDvr
            )
            && seekable_range.is_some();
        let displayed_progress_ratio = displayed_progress_ratio
            .filter(|ratio| ratio.is_finite())
            .map(|ratio| ratio.clamp(0.0, 1.0));

        Self {
            timeline_kind,
            is_playing,
            is_seekable,
            can_scrub,
            controls_visible,
            playback_rate,
            displayed_position: position,
            duration,
            seekable_range,
            displayed_progress_ratio,
        }
    }

    pub fn is_rate_active(&self, rate: f32) -> bool {
        (self.playback_rate - rate).abs() < 0.05
    }

    pub fn seek_preview(&self, ratio: f64) -> Option<PlaybackSeekPreview> {
        if !self.can_scrub {
            return None;
        }
        let ratio = ratio.clamp(0.0, 1.0);
        let range = self.seekable_range?;
        let (start, end) = (range.start, range.end);
        let span = end.checked_sub(start)?;
        let seconds = span.as_secs_f64() * ratio;
        let offset = Duration::try_from_secs_f64(seconds).ok()?;
        let position = start.checked_add(offset).unwrap_or(end).min(end);
        Some(PlaybackSeekPreview { position, ratio })
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PlaybackControl {
    pub rect: PlaybackRect,
    pub action: PlaybackUiAction,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PlaybackUiLayout {
    pub metrics: PlaybackLayoutMetrics,
    pub bar_rect: PlaybackRect,
    pub progress_rect: PlaybackRect,
    pub progress_hit_rect: PlaybackRect,
    pub controls: Vec<PlaybackControl>,
}

impl PlaybackUiLayout {
    /// Builds a playback layout from host-owned geometry.
    ///
    /// Desktop hosts may have a sidebar, floating panel, or platform-specific
    /// control sizing. They can retain that geometry while still delegating
    /// playback hit testing and primitive generation to this component.
    pub fn from_parts(
        metrics: PlaybackLayoutMetrics,
        bar_rect: PlaybackRect,
        progress_rect: PlaybackRect,
        progress_hit_rect: PlaybackRect,
        controls: Vec<PlaybackControl>,
    ) -> Self {
        Self {
            metrics,
            bar_rect,
            progress_rect,
            progress_hit_rect,
            controls,
        }
    }

    pub fn for_surface(frame_width: u32, frame_height: u32) -> Option<Self> {
        let metrics = PlaybackLayoutMetrics::for_surface(frame_width, frame_height)?;
        let bar_rect = PlaybackRect {
            x: 0,
            y: metrics.overlay_origin_y(frame_height),
            width: frame_width,
            height: metrics.bar_height,
        };
        let progress_rect = metrics.progress_rect(frame_width, frame_height);
        let progress_hit_rect = metrics.progress_hit_rect(frame_width, frame_height);
        let y = metrics.button_origin_y(frame_height);

        let mut controls = Vec::with_capacity(6 + PLAYBACK_RATES.len());
        let mut x = metrics.padding;
        for action in [
            PlaybackUiAction::SeekStart,
            PlaybackUiAction::SeekBack,
            PlaybackUiAction::TogglePause,
            PlaybackUiAction::Stop,
            PlaybackUiAction::SeekForward,
            PlaybackUiAction::SeekEnd,
        ] {
            controls.push(PlaybackControl {
                rect: PlaybackRect {
                    x,
                    y,
                    width: metrics.icon_size,
                    height: metrics.icon_size,
                },
                action,
            });
            x = x.saturating_add(metrics.icon_size.saturating_add(metrics.gap));
        }

        let total_rate_width = PLAYBACK_RATES
            .len()
            .saturating_mul(metrics.rate_width as usize)
            .saturating_add(
                PLAYBACK_RATES
                    .len()
                    .saturating_sub(1)
                    .saturating_mul(metrics.gap as usize),
            ) as u32;
        let mut rate_x =
            frame_width.saturating_sub(metrics.padding.saturating_add(total_rate_width));
        for &(rate, _) in PLAYBACK_RATES {
            controls.push(PlaybackControl {
                rect: PlaybackRect {
                    x: rate_x,
                    y,
                    width: metrics.rate_width,
                    height: metrics.icon_size,
                },
                action: PlaybackUiAction::SetRate(rate),
            });
            rate_x = rate_x.saturating_add(metrics.rate_width.saturating_add(metrics.gap));
        }

        Some(Self::from_parts(
            metrics,
            bar_rect,
            progress_rect,
            progress_hit_rect,
            controls,
        ))
    }

    pub fn hit_test(
        &self,
        cursor_x: f64,
        cursor_y: f64,
        view_model: &PlaybackUiViewModel,
    ) -> Option<PlaybackUiAction> {
        let x = coordinate(cursor_x);
        let y = coordinate(cursor_y);
        if !view_model.controls_visible {
            return None;
        }
        self.controls
            .iter()
            .find(|control| control.rect.contains(x, y))
            .map(|control| control.action)
            .or_else(|| {
                if self.progress_hit_rect.contains(x, y) {
                    let ratio = if self.progress_rect.width == 0 {
                        0.0
                    } else {
                        (cursor_x - f64::from(self.progress_rect.x))
                            / f64::from(self.progress_rect.width)
                    };
                    view_model
                        .seek_preview(ratio)
                        .map(|preview| PlaybackUiAction::SeekToRatio(preview.ratio as f32))
                } else {
                    None
                }
            })
    }

    pub fn primitives(&self, view_model: &PlaybackUiViewModel) -> Vec<PlaybackOverlayPrimitive> {
        let mut primitives = Vec::with_capacity(self.controls.len() + 2);
        primitives.push(PlaybackOverlayPrimitive::ProgressTrack {
            rect: self.progress_rect,
            ratio: view_model.displayed_progress_ratio,
        });
        if view_model.controls_visible {
            primitives.extend(self.controls.iter().copied().map(|control| {
                PlaybackOverlayPrimitive::Control {
                    rect: control.rect,
                    action: control.action,
                }
            }));
        }
        primitives.push(PlaybackOverlayPrimitive::TimeLabel {
            rect: self.bar_rect,
        });
        primitives
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PlaybackOverlayPrimitive {
    ProgressTrack {
        rect: PlaybackRect,
        ratio: Option<f64>,
    },
    Control {
        rect: PlaybackRect,
        action: PlaybackUiAction,
    },
    TimeLabel {
        rect: PlaybackRect,
    },
}

fn coordinate(value: f64) -> u32 {
    if !value.is_finite() {
        return 0;
    }
    value.round().clamp(0.0, f64::from(u32::MAX)) as u32
}

#[cfg(test)]
mod tests {
    use super::*;

    fn model(duration: Option<Duration>, ratio: Option<f64>) -> PlaybackUiViewModel {
        let seekable_range = duration.map(|end| PlaybackSeekRange {
            start: Duration::ZERO,
            end,
        });
        PlaybackUiViewModel::new(
            PlaybackTimelineKind::Vod,
            true,
            true,
            true,
            1.0,
            Duration::from_secs(10),
            duration,
            seekable_range,
            ratio,
        )
    }

    #[test]
    fn zero_surface_has_no_layout() {
        assert!(PlaybackLayoutMetrics::for_surface(0, 400).is_none());
        assert!(PlaybackUiLayout::for_surface(400, 0).is_none());
    }

    #[test]
    fn tiny_surface_stays_inside_surface() {
        let metrics = PlaybackLayoutMetrics::for_surface(2, 2).expect("tiny metrics");
        assert!(metrics.bar_height <= 2);
        assert!(metrics.progress_rect(2, 2).y <= 2);
        assert!(metrics.progress_hit_rect(2, 2).y <= 2);
    }

    #[test]
    fn ratio_and_preview_are_clamped() {
        let view_model = model(Some(Duration::from_secs(100)), Some(3.0));
        assert_eq!(view_model.displayed_progress_ratio, Some(1.0));
        assert_eq!(view_model.seek_preview(-1.0).expect("preview").ratio, 0.0);
        assert_eq!(
            view_model.seek_preview(2.0).expect("preview").position,
            Duration::from_secs(100)
        );
    }

    #[test]
    fn no_duration_disables_scrubbing() {
        let view_model = model(None, Some(0.4));
        assert!(!view_model.can_scrub);
        assert!(view_model.seek_preview(0.4).is_none());
    }

    #[test]
    fn missing_seekable_range_disables_scrubbing_even_with_duration() {
        let view_model = PlaybackUiViewModel::new(
            PlaybackTimelineKind::Vod,
            true,
            true,
            true,
            1.0,
            Duration::from_secs(10),
            Some(Duration::from_secs(100)),
            None,
            Some(0.4),
        );
        assert!(!view_model.can_scrub);
        assert!(view_model.seek_preview(0.4).is_none());
    }

    #[test]
    fn live_dvr_range_allows_scrubbing_without_duration() {
        let view_model = PlaybackUiViewModel::new(
            PlaybackTimelineKind::LiveDvr,
            true,
            true,
            true,
            1.0,
            Duration::from_secs(20),
            None,
            Some(PlaybackSeekRange {
                start: Duration::from_secs(10),
                end: Duration::from_secs(110),
            }),
            None,
        );
        assert!(view_model.can_scrub);
        assert_eq!(
            view_model.seek_preview(0.5).expect("preview").position,
            Duration::from_secs(60)
        );
    }

    #[test]
    fn hit_test_returns_transport_and_rate_actions() {
        let layout = PlaybackUiLayout::for_surface(1280, 720).expect("layout");
        let view_model = model(Some(Duration::from_secs(100)), None);
        assert_eq!(
            layout.hit_test(20.0, 650.0, &view_model),
            Some(PlaybackUiAction::SeekStart)
        );
        let rate = layout
            .controls
            .iter()
            .find(|control| control.action == PlaybackUiAction::SetRate(1.5))
            .expect("rate control");
        assert_eq!(
            layout.hit_test(rate.rect.x as f64, rate.rect.y as f64, &view_model),
            Some(rate.action)
        );
    }

    #[test]
    fn hidden_controls_do_not_hit_test() {
        let layout = PlaybackUiLayout::for_surface(800, 600).expect("layout");
        let mut view_model = model(Some(Duration::from_secs(100)), None);
        view_model.controls_visible = false;
        assert!(layout.hit_test(10.0, 550.0, &view_model).is_none());
    }
}
