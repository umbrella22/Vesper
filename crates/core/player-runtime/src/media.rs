use std::time::Duration;

use player_model::{
    MediaSourceKind, MediaSourceProtocol, MediaTrackCatalog, MediaTrackSelectionSnapshot,
    PlaybackProgress,
};

use crate::{PlayerError, PlayerErrorCode, PlayerResult};

/// Platform surface kinds accepted by runtime adapters.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlayerVideoSurfaceKind {
    /// macOS `NSView` pointer.
    NsView,
    /// iOS `UIView` pointer.
    UiView,
    /// Apple `AVPlayerLayer` pointer.
    PlayerLayer,
    /// Apple `CAMetalLayer` pointer.
    MetalLayer,
    /// Windows `HWND` value.
    Win32Hwnd,
}

/// Host-owned video surface handle passed through the runtime boundary.
///
/// The `handle` is an opaque platform pointer or integer value whose lifetime
/// remains owned by the host. Runtime adapters must validate that `kind` is
/// supported for their platform before using the handle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlayerVideoSurfaceTarget {
    /// Platform-specific handle kind.
    pub kind: PlayerVideoSurfaceKind,
    /// Opaque host-owned handle value.
    pub handle: usize,
}

/// Best-known video stream details.
#[derive(Debug, Clone)]
pub struct PlayerVideoInfo {
    /// Codec name or identifier.
    pub codec: String,
    /// Encoded width in pixels.
    pub width: u32,
    /// Encoded height in pixels.
    pub height: u32,
    /// Optional frame rate in frames per second.
    pub frame_rate: Option<f64>,
}

/// Best-known audio stream details.
#[derive(Debug, Clone)]
pub struct PlayerAudioInfo {
    /// Codec name or identifier.
    pub codec: String,
    /// Sample rate in hertz.
    pub sample_rate: u32,
    /// Channel count.
    pub channels: u16,
}

/// Media metadata discovered during probing or playback.
#[derive(Debug, Clone)]
pub struct PlayerMediaInfo {
    /// Source URI.
    pub source_uri: String,
    /// Source storage kind.
    pub source_kind: MediaSourceKind,
    /// Source protocol.
    pub source_protocol: MediaSourceProtocol,
    /// Known duration, if available.
    pub duration: Option<Duration>,
    /// Known aggregate bit rate, if available.
    pub bit_rate: Option<u64>,
    /// Number of audio streams.
    pub audio_streams: usize,
    /// Number of video streams.
    pub video_streams: usize,
    /// Best video stream summary, if available.
    pub best_video: Option<PlayerVideoInfo>,
    /// Best audio stream summary, if available.
    pub best_audio: Option<PlayerAudioInfo>,
    /// Available media tracks.
    pub track_catalog: MediaTrackCatalog,
    /// Current track selection snapshot.
    pub track_selection: MediaTrackSelectionSnapshot,
}

/// Timeline classification for playback progress.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlayerTimelineKind {
    /// Video-on-demand timeline.
    Vod,
    /// Live timeline without a seekable DVR window.
    Live,
    /// Live timeline with a seekable DVR window.
    LiveDvr,
}

/// Inclusive seekable media position range.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlayerSeekableRange {
    /// Start of the seekable range.
    pub start: Duration,
    /// End of the seekable range.
    pub end: Duration,
}

impl PlayerSeekableRange {
    /// Returns the range duration when `end >= start`.
    pub fn duration(&self) -> Option<Duration> {
        self.end.checked_sub(self.start)
    }

    /// Clamps a media position into this range.
    pub fn clamp_position(&self, position: Duration) -> Duration {
        position.clamp(self.start, self.end)
    }

    /// Returns whether a media position is inside this range.
    pub fn contains(&self, position: Duration) -> bool {
        position >= self.start && position <= self.end
    }
}

/// Runtime timeline snapshot derived from progress and media metadata.
#[derive(Debug, Clone)]
pub struct PlayerTimelineSnapshot {
    /// Timeline kind.
    pub kind: PlayerTimelineKind,
    /// Whether seeking is currently supported.
    pub is_seekable: bool,
    /// Seekable range when available.
    pub seekable_range: Option<PlayerSeekableRange>,
    /// Effective live edge when known.
    pub live_edge: Option<Duration>,
    /// Current media position.
    pub position: Duration,
    /// Current timeline duration when known.
    pub duration: Option<Duration>,
}

/// Audio output information reported by a runtime.
#[derive(Debug, Clone)]
pub struct PlayerAudioOutputInfo {
    /// Output device display name.
    pub device_name: Option<String>,
    /// Output channel count.
    pub channels: Option<u16>,
    /// Output sample rate in hertz.
    pub sample_rate: Option<u32>,
    /// Output sample format.
    pub sample_format: Option<String>,
}

impl PlayerTimelineSnapshot {
    /// Builds a VOD timeline from playback progress.
    pub fn vod(progress: PlaybackProgress, supports_seek: bool) -> Self {
        Self::vod_with_duration(progress, progress.duration(), supports_seek)
    }

    /// Builds a non-seekable live timeline from playback progress.
    pub fn live(progress: PlaybackProgress) -> Self {
        Self {
            kind: PlayerTimelineKind::Live,
            is_seekable: false,
            seekable_range: None,
            live_edge: None,
            position: progress.position(),
            duration: None,
        }
    }

    /// Builds a live DVR timeline with an explicit seekable range.
    pub fn live_dvr(
        progress: PlaybackProgress,
        seekable_range: PlayerSeekableRange,
        live_edge: Option<Duration>,
    ) -> Self {
        let duration = seekable_range.duration();
        Self {
            kind: PlayerTimelineKind::LiveDvr,
            is_seekable: true,
            seekable_range: Some(seekable_range),
            live_edge: live_edge.or(Some(seekable_range.end)),
            position: progress.position(),
            duration,
        }
    }

    /// Builds a VOD timeline with an explicit duration override.
    pub fn vod_with_duration(
        progress: PlaybackProgress,
        duration: Option<Duration>,
        supports_seek: bool,
    ) -> Self {
        let seekable_range = duration.map(|end| PlayerSeekableRange {
            start: Duration::ZERO,
            end,
        });
        let is_seekable = supports_seek && seekable_range.is_some();
        Self {
            kind: PlayerTimelineKind::Vod,
            is_seekable,
            seekable_range: if is_seekable { seekable_range } else { None },
            live_edge: None,
            position: progress.position(),
            duration,
        }
    }

    /// Infers a timeline from playback progress and media metadata.
    pub fn from_media_info(
        progress: PlaybackProgress,
        supports_seek: bool,
        media_info: &PlayerMediaInfo,
    ) -> Self {
        let inferred_duration = progress.duration().or(media_info.duration);
        match (media_info.source_kind, media_info.source_protocol) {
            // RTMP and RTSP are live transports unless a platform/backend
            // explicitly supplies a different timeline.
            (_, MediaSourceProtocol::Rtmp | MediaSourceProtocol::Rtsp) => Self::live(progress),
            // Without an explicit live window from the platform/backend, treat remote HLS/DASH
            // with a known duration as VOD and duration-less streams as baseline LIVE.
            (MediaSourceKind::Remote, MediaSourceProtocol::Hls | MediaSourceProtocol::Dash) => {
                inferred_duration
                    .map(|duration| {
                        Self::vod_with_duration(progress, Some(duration), supports_seek)
                    })
                    .unwrap_or_else(|| Self::live(progress))
            }
            _ => Self::vod_with_duration(progress, inferred_duration, supports_seek),
        }
    }

    /// Returns the displayed position as a ratio of the seekable range.
    pub fn displayed_ratio(&self) -> Option<f64> {
        self.ratio_for_position(self.position)
    }

    /// Returns the effective live edge for live timelines.
    pub fn effective_live_edge(&self) -> Option<Duration> {
        match self.kind {
            PlayerTimelineKind::Vod => None,
            PlayerTimelineKind::Live => self.live_edge,
            PlayerTimelineKind::LiveDvr => self
                .live_edge
                .or_else(|| self.seekable_range.map(|range| range.end)),
        }
    }

    /// Returns the position used by a go-live action.
    pub fn go_live_position(&self) -> Option<Duration> {
        self.effective_live_edge()
    }

    /// Clamps a position into the current timeline window.
    pub fn clamp_position(&self, position: Duration) -> Duration {
        if let Some(range) = self.seekable_range {
            return range.clamp_position(position);
        }
        if let Some(duration) = self.duration {
            return position.clamp(Duration::ZERO, duration);
        }
        position
    }

    /// Returns whether a position is outside the current timeline window.
    pub fn is_position_out_of_range(&self, position: Duration) -> bool {
        if let Some(range) = self.seekable_range {
            return !range.contains(position);
        }
        if let Some(duration) = self.duration {
            return position > duration;
        }
        false
    }

    /// Validates a position and returns the original position on success.
    pub fn validate_position(&self, position: Duration) -> PlayerResult<Duration> {
        if self.is_position_out_of_range(position) {
            return Err(PlayerError::new(
                PlayerErrorCode::SeekFailure,
                format!(
                    "seek position {}ms is outside the current timeline window",
                    position.as_millis()
                ),
            ));
        }
        Ok(position)
    }

    /// Returns distance from current position to live edge.
    pub fn live_offset(&self) -> Option<Duration> {
        let live_edge = self.effective_live_edge()?;
        Some(live_edge.saturating_sub(self.clamp_position(self.position)))
    }

    /// Returns whether the current position is within tolerance of live edge.
    pub fn is_at_live_edge(&self, tolerance: Duration) -> bool {
        self.live_offset().is_some_and(|offset| offset <= tolerance)
    }

    /// Converts a position into a seekable-range ratio.
    pub fn ratio_for_position(&self, position: Duration) -> Option<f64> {
        let range = self.seekable_range?;
        let total = range.duration()?;
        if total.is_zero() {
            return Some(1.0);
        }
        let clamped_position = range.clamp_position(position);
        let offset = clamped_position.checked_sub(range.start)?;
        Some((offset.as_secs_f64() / total.as_secs_f64()).clamp(0.0, 1.0))
    }

    /// Converts a seekable-range ratio into a position.
    pub fn position_for_ratio(&self, ratio: f64) -> Option<Duration> {
        if !ratio.is_finite() {
            return None;
        }
        let range = self.seekable_range?;
        let total = range.duration()?;
        if total.is_zero() {
            return Some(range.start);
        }
        let clamped_ratio = ratio.clamp(0.0, 1.0);
        let target_offset = Duration::from_secs_f64(total.as_secs_f64() * clamped_ratio);
        Some(range.clamp_position(range.start + target_offset))
    }
}
