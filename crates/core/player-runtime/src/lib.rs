mod adapter;
mod error;

use std::sync::OnceLock;
use std::time::{Duration, Instant};

use player_core::MediaSource;

pub use adapter::{
    PlayerRuntimeAdapter, PlayerRuntimeAdapterBootstrap, PlayerRuntimeAdapterFactory,
    PlayerRuntimeAdapterInitializer,
};
pub use error::{PlayerRuntimeError, PlayerRuntimeErrorCode, PlayerRuntimeResult};
pub use player_core::{
    DecodedVideoFrame, MediaSourceKind, MediaSourceProtocol, PlaybackProgress, PresentationState,
    VideoPixelFormat,
};

pub const DEFAULT_PLAYBACK_RATE: f32 = 1.0;
pub const MIN_PLAYBACK_RATE: f32 = 0.5;
pub const NATURAL_PLAYBACK_RATE_MAX: f32 = 2.0;
pub const MAX_PLAYBACK_RATE: f32 = 3.0;
pub const DEFAULT_VIDEO_PRESENT_EARLY_TOLERANCE: Duration = Duration::from_millis(4);
pub const DEFAULT_VIDEO_IDLE_POLL_INTERVAL: Duration = Duration::from_millis(16);
pub const DEFAULT_VIDEO_PREFETCH_CAPACITY: usize = 8;

static DEFAULT_RUNTIME_ADAPTER_FACTORY: OnceLock<&'static dyn PlayerRuntimeAdapterFactory> =
    OnceLock::new();

#[derive(Debug, Clone)]
pub struct PlayerRuntimeOptions {
    pub enable_audio_output: bool,
    pub video_surface: Option<PlayerVideoSurfaceTarget>,
    pub video_prefetch_capacity: usize,
    pub video_present_early_tolerance: Duration,
    pub video_idle_poll_interval: Duration,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlayerVideoSurfaceKind {
    NsView,
    UiView,
    PlayerLayer,
    MetalLayer,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlayerVideoSurfaceTarget {
    pub kind: PlayerVideoSurfaceKind,
    pub handle: usize,
}

#[derive(Debug, Clone)]
pub struct PlayerVideoInfo {
    pub codec: String,
    pub width: u32,
    pub height: u32,
    pub frame_rate: Option<f64>,
}

#[derive(Debug, Clone)]
pub struct PlayerAudioInfo {
    pub codec: String,
    pub sample_rate: u32,
    pub channels: u16,
}

#[derive(Debug, Clone)]
pub struct PlayerMediaInfo {
    pub source_uri: String,
    pub source_kind: MediaSourceKind,
    pub source_protocol: MediaSourceProtocol,
    pub duration: Option<Duration>,
    pub bit_rate: Option<u64>,
    pub audio_streams: usize,
    pub video_streams: usize,
    pub best_video: Option<PlayerVideoInfo>,
    pub best_audio: Option<PlayerAudioInfo>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlayerTimelineKind {
    Vod,
    Live,
    LiveDvr,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlayerSeekableRange {
    pub start: Duration,
    pub end: Duration,
}

#[derive(Debug, Clone)]
pub struct PlayerTimelineSnapshot {
    pub kind: PlayerTimelineKind,
    pub is_seekable: bool,
    pub seekable_range: Option<PlayerSeekableRange>,
    pub live_edge: Option<Duration>,
    pub position: Duration,
    pub duration: Option<Duration>,
}

#[derive(Debug, Clone)]
pub struct PlayerAudioOutputInfo {
    pub device_name: Option<String>,
    pub channels: Option<u16>,
    pub sample_rate: Option<u32>,
    pub sample_format: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlayerRuntimeAdapterBackendFamily {
    SoftwareDesktop,
    NativeMacos,
    NativeAndroid,
    NativeIos,
    NativeHarmony,
    Unknown,
}

#[derive(Debug, Clone)]
pub struct PlayerRuntimeAdapterCapabilities {
    pub adapter_id: &'static str,
    pub backend_family: PlayerRuntimeAdapterBackendFamily,
    pub supports_audio_output: bool,
    pub supports_frame_output: bool,
    pub supports_external_video_surface: bool,
    pub supports_seek: bool,
    pub supports_stop: bool,
    pub supports_playback_rate: bool,
    pub playback_rate_min: Option<f32>,
    pub playback_rate_max: Option<f32>,
    pub natural_playback_rate_max: Option<f32>,
    pub supports_hardware_decode: bool,
    pub supports_streaming: bool,
    pub supports_hdr: bool,
}

pub struct PlayerRuntimeInitializer {
    adapter_id: &'static str,
    inner: Box<dyn PlayerRuntimeAdapterInitializer>,
}

#[derive(Debug, Clone)]
pub struct DecodedAudioSummary {
    pub channels: u16,
    pub sample_rate: u32,
    pub duration: Duration,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlayerVideoDecodeMode {
    Software,
    Hardware,
}

#[derive(Debug, Clone)]
pub struct PlayerVideoDecodeInfo {
    pub selected_mode: PlayerVideoDecodeMode,
    pub hardware_available: bool,
    pub hardware_backend: Option<String>,
    pub fallback_reason: Option<String>,
}

#[derive(Debug, Clone)]
pub struct PlayerRuntimeStartup {
    pub ffmpeg_initialized: bool,
    pub audio_output: Option<PlayerAudioOutputInfo>,
    pub decoded_audio: Option<DecodedAudioSummary>,
    pub video_decode: Option<PlayerVideoDecodeInfo>,
}

#[derive(Debug, Clone, Copy)]
pub enum PlayerRuntimeCommand {
    Play,
    Pause,
    TogglePause,
    SeekTo { position: Duration },
    SetPlaybackRate { rate: f32 },
    Stop,
}

#[derive(Debug)]
pub struct PlayerRuntimeCommandResult {
    pub applied: bool,
    pub frame: Option<DecodedVideoFrame>,
    pub snapshot: PlayerSnapshot,
}

#[derive(Debug, Clone)]
pub struct PlayerSnapshot {
    pub source_uri: String,
    pub state: PresentationState,
    pub has_video_surface: bool,
    pub is_interrupted: bool,
    pub is_buffering: bool,
    pub playback_rate: f32,
    pub progress: PlaybackProgress,
    pub timeline: PlayerTimelineSnapshot,
    pub media_info: PlayerMediaInfo,
}

#[derive(Debug, Clone)]
pub struct FirstFrameReady {
    pub presentation_time: Duration,
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone)]
pub enum PlayerRuntimeEvent {
    Initialized(PlayerRuntimeStartup),
    MetadataReady(PlayerMediaInfo),
    FirstFrameReady(FirstFrameReady),
    PlaybackStateChanged(PresentationState),
    InterruptionChanged { interrupted: bool },
    BufferingChanged { buffering: bool },
    VideoSurfaceChanged { attached: bool },
    AudioOutputChanged(Option<PlayerAudioOutputInfo>),
    PlaybackRateChanged { rate: f32 },
    SeekCompleted { position: Duration },
    Error(PlayerRuntimeError),
    Ended,
}

pub struct PlayerRuntimeBootstrap {
    pub runtime: PlayerRuntime,
    pub initial_frame: Option<DecodedVideoFrame>,
    pub startup: PlayerRuntimeStartup,
}

pub struct PlayerRuntime {
    adapter_id: &'static str,
    inner: Box<dyn PlayerRuntimeAdapter>,
}

impl std::fmt::Debug for PlayerRuntimeInitializer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PlayerRuntimeInitializer")
            .field("adapter_id", &self.adapter_id)
            .finish()
    }
}

impl std::fmt::Debug for PlayerRuntime {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PlayerRuntime")
            .field("adapter_id", &self.adapter_id)
            .field("source_uri", &self.source_uri())
            .field("state", &self.presentation_state())
            .finish()
    }
}

impl Default for PlayerRuntimeOptions {
    fn default() -> Self {
        Self {
            enable_audio_output: true,
            video_surface: None,
            video_prefetch_capacity: DEFAULT_VIDEO_PREFETCH_CAPACITY,
            video_present_early_tolerance: DEFAULT_VIDEO_PRESENT_EARLY_TOLERANCE,
            video_idle_poll_interval: DEFAULT_VIDEO_IDLE_POLL_INTERVAL,
        }
    }
}

impl PlayerRuntimeOptions {
    pub fn with_video_surface(mut self, video_surface: PlayerVideoSurfaceTarget) -> Self {
        self.video_surface = Some(video_surface);
        self
    }
}

impl PlayerTimelineSnapshot {
    pub fn vod(progress: PlaybackProgress, supports_seek: bool) -> Self {
        Self::vod_with_duration(progress, progress.duration(), supports_seek)
    }

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

    pub fn live_dvr(
        progress: PlaybackProgress,
        seekable_range: PlayerSeekableRange,
        live_edge: Option<Duration>,
    ) -> Self {
        let duration = seekable_range.end.checked_sub(seekable_range.start);
        Self {
            kind: PlayerTimelineKind::LiveDvr,
            is_seekable: true,
            seekable_range: Some(seekable_range),
            live_edge: live_edge.or(Some(seekable_range.end)),
            position: progress.position(),
            duration,
        }
    }

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

    pub fn from_media_info(
        progress: PlaybackProgress,
        supports_seek: bool,
        media_info: &PlayerMediaInfo,
    ) -> Self {
        let inferred_duration = progress.duration().or(media_info.duration);

        match (media_info.source_kind, media_info.source_protocol) {
            // Without an explicit live window from the platform/backend, treat remote HLS/DASH
            // with a known duration as VOD and duration-less streams as baseline LIVE.
            (MediaSourceKind::Remote, MediaSourceProtocol::Hls | MediaSourceProtocol::Dash) =>
                inferred_duration
                    .map(|duration| Self::vod_with_duration(progress, Some(duration), supports_seek))
                    .unwrap_or_else(|| Self::live(progress)),
            _ => Self::vod_with_duration(progress, inferred_duration, supports_seek),
        }
    }

    pub fn displayed_ratio(&self) -> Option<f64> {
        self.ratio_for_position(self.position)
    }

    pub fn ratio_for_position(&self, position: Duration) -> Option<f64> {
        let range = self.seekable_range?;
        let total = range.end.checked_sub(range.start)?;
        if total.is_zero() {
            return Some(1.0);
        }

        let clamped_position = position.clamp(range.start, range.end);
        let offset = clamped_position.checked_sub(range.start)?;
        Some((offset.as_secs_f64() / total.as_secs_f64()).clamp(0.0, 1.0))
    }

    pub fn position_for_ratio(&self, ratio: f64) -> Option<Duration> {
        let range = self.seekable_range?;
        let total = range.end.checked_sub(range.start)?;
        if total.is_zero() {
            return Some(range.start);
        }

        let clamped_ratio = ratio.clamp(0.0, 1.0);
        let target_offset = Duration::from_secs_f64(total.as_secs_f64() * clamped_ratio);
        Some((range.start + target_offset).clamp(range.start, range.end))
    }
}

impl PlayerRuntimeInitializer {
    pub fn probe_uri(uri: impl Into<String>) -> PlayerRuntimeResult<Self> {
        Self::probe_source(MediaSource::new(uri))
    }

    pub fn probe_uri_with_options_and_factory(
        uri: impl Into<String>,
        options: PlayerRuntimeOptions,
        factory: &dyn PlayerRuntimeAdapterFactory,
    ) -> PlayerRuntimeResult<Self> {
        Self::probe_source_with_factory(MediaSource::new(uri), options, factory)
    }

    pub fn probe_source(source: MediaSource) -> PlayerRuntimeResult<Self> {
        Self::probe_source_with_options(source, PlayerRuntimeOptions::default())
    }

    pub fn probe_source_with_options(
        source: MediaSource,
        options: PlayerRuntimeOptions,
    ) -> PlayerRuntimeResult<Self> {
        Self::probe_source_with_factory(source, options, default_runtime_adapter_factory()?)
    }

    pub fn probe_source_with_factory(
        source: MediaSource,
        options: PlayerRuntimeOptions,
        factory: &dyn PlayerRuntimeAdapterFactory,
    ) -> PlayerRuntimeResult<Self> {
        Ok(Self {
            adapter_id: factory.adapter_id(),
            inner: factory.probe_source_with_options(source, options)?,
        })
    }

    pub fn adapter_id(&self) -> &str {
        self.adapter_id
    }

    pub fn capabilities(&self) -> PlayerRuntimeAdapterCapabilities {
        self.inner.capabilities()
    }

    pub fn media_info(&self) -> PlayerMediaInfo {
        self.inner.media_info()
    }

    pub fn startup(&self) -> PlayerRuntimeStartup {
        self.inner.startup()
    }

    pub fn initialize(self) -> PlayerRuntimeResult<PlayerRuntimeBootstrap> {
        let Self { adapter_id, inner } = self;
        let PlayerRuntimeAdapterBootstrap {
            runtime,
            initial_frame,
            startup,
        } = inner.initialize()?;

        Ok(PlayerRuntimeBootstrap {
            runtime: PlayerRuntime {
                adapter_id,
                inner: runtime,
            },
            initial_frame,
            startup,
        })
    }
}

impl PlayerRuntime {
    pub fn open_uri(uri: impl Into<String>) -> PlayerRuntimeResult<PlayerRuntimeBootstrap> {
        Self::open_source(MediaSource::new(uri))
    }

    pub fn open_uri_with_options_and_factory(
        uri: impl Into<String>,
        options: PlayerRuntimeOptions,
        factory: &dyn PlayerRuntimeAdapterFactory,
    ) -> PlayerRuntimeResult<PlayerRuntimeBootstrap> {
        Self::open_source_with_factory(MediaSource::new(uri), options, factory)
    }

    pub fn open_source(source: MediaSource) -> PlayerRuntimeResult<PlayerRuntimeBootstrap> {
        Self::open_source_with_options(source, PlayerRuntimeOptions::default())
    }

    pub fn open_source_with_options(
        source: MediaSource,
        options: PlayerRuntimeOptions,
    ) -> PlayerRuntimeResult<PlayerRuntimeBootstrap> {
        Self::open_source_with_factory(source, options, default_runtime_adapter_factory()?)
    }

    pub fn open_source_with_factory(
        source: MediaSource,
        options: PlayerRuntimeOptions,
        factory: &dyn PlayerRuntimeAdapterFactory,
    ) -> PlayerRuntimeResult<PlayerRuntimeBootstrap> {
        PlayerRuntimeInitializer::probe_source_with_factory(source, options, factory)?.initialize()
    }

    pub fn adapter_id(&self) -> &str {
        self.adapter_id
    }

    pub fn source_uri(&self) -> &str {
        self.inner.source_uri()
    }

    pub fn capabilities(&self) -> PlayerRuntimeAdapterCapabilities {
        self.inner.capabilities()
    }

    pub fn media_info(&self) -> &PlayerMediaInfo {
        self.inner.media_info()
    }

    pub fn presentation_state(&self) -> PresentationState {
        self.inner.presentation_state()
    }

    pub fn progress(&self) -> PlaybackProgress {
        self.inner.progress()
    }

    pub fn has_video_surface(&self) -> bool {
        self.inner.has_video_surface()
    }

    pub fn is_interrupted(&self) -> bool {
        self.inner.is_interrupted()
    }

    pub fn playback_rate(&self) -> f32 {
        self.inner.playback_rate()
    }

    pub fn is_buffering(&self) -> bool {
        self.inner.is_buffering()
    }

    pub fn snapshot(&self) -> PlayerSnapshot {
        self.inner.snapshot()
    }

    pub fn drain_events(&mut self) -> Vec<PlayerRuntimeEvent> {
        self.inner.drain_events()
    }

    pub fn dispatch(
        &mut self,
        command: PlayerRuntimeCommand,
    ) -> PlayerRuntimeResult<PlayerRuntimeCommandResult> {
        self.inner.dispatch(command)
    }

    pub fn set_playback_rate(
        &mut self,
        rate: f32,
    ) -> PlayerRuntimeResult<PlayerRuntimeCommandResult> {
        self.dispatch(PlayerRuntimeCommand::SetPlaybackRate { rate })
    }

    pub fn replace_video_surface(
        &mut self,
        video_surface: Option<PlayerVideoSurfaceTarget>,
    ) -> PlayerRuntimeResult<()> {
        self.inner.replace_video_surface(video_surface)
    }

    pub fn advance(&mut self) -> PlayerRuntimeResult<Option<DecodedVideoFrame>> {
        self.inner.advance()
    }

    pub fn next_deadline(&self) -> Option<Instant> {
        self.inner.next_deadline()
    }
}

pub fn register_default_runtime_adapter_factory(
    factory: &'static dyn PlayerRuntimeAdapterFactory,
) -> PlayerRuntimeResult<()> {
    match DEFAULT_RUNTIME_ADAPTER_FACTORY.set(factory) {
        Ok(()) => Ok(()),
        Err(existing) if existing.adapter_id() == factory.adapter_id() => Ok(()),
        Err(existing) => Err(PlayerRuntimeError::new(
            PlayerRuntimeErrorCode::InvalidState,
            format!(
                "default runtime adapter factory is already registered as '{}'; cannot replace it with '{}'",
                existing.adapter_id(),
                factory.adapter_id()
            ),
        )),
    }
}

fn default_runtime_adapter_factory() -> PlayerRuntimeResult<&'static dyn PlayerRuntimeAdapterFactory>
{
    DEFAULT_RUNTIME_ADAPTER_FACTORY.get().copied().ok_or_else(|| {
        PlayerRuntimeError::new(
            PlayerRuntimeErrorCode::Unsupported,
            "no default runtime adapter factory is registered; use probe_source_with_factory/open_source_with_factory or install a platform adapter factory",
        )
    })
}

#[cfg(test)]
mod tests {
    use super::{
        MediaSourceKind, MediaSourceProtocol, PlaybackProgress, PlayerMediaInfo,
        PlayerSeekableRange, PlayerTimelineKind, PlayerTimelineSnapshot,
    };
    use std::time::Duration;

    fn test_media_info(
        source_kind: MediaSourceKind,
        source_protocol: MediaSourceProtocol,
        duration: Option<Duration>,
    ) -> PlayerMediaInfo {
        PlayerMediaInfo {
            source_uri: "placeholder".to_owned(),
            source_kind,
            source_protocol,
            duration,
            bit_rate: None,
            audio_streams: 1,
            video_streams: 1,
            best_video: None,
            best_audio: None,
        }
    }

    #[test]
    fn timeline_from_media_info_uses_media_duration_for_streaming_vod() {
        let media_info = test_media_info(
            MediaSourceKind::Remote,
            MediaSourceProtocol::Hls,
            Some(Duration::from_secs(18)),
        );
        let timeline = PlayerTimelineSnapshot::from_media_info(
            PlaybackProgress::new(Duration::from_secs(3), None),
            true,
            &media_info,
        );

        assert_eq!(timeline.kind, PlayerTimelineKind::Vod);
        assert!(timeline.is_seekable);
        assert_eq!(timeline.duration, Some(Duration::from_secs(18)));
        assert_eq!(
            timeline.seekable_range.expect("seekable range").end,
            Duration::from_secs(18)
        );
    }

    #[test]
    fn timeline_from_media_info_promotes_unknown_streaming_duration_to_live() {
        let media_info = test_media_info(MediaSourceKind::Remote, MediaSourceProtocol::Dash, None);
        let timeline = PlayerTimelineSnapshot::from_media_info(
            PlaybackProgress::new(Duration::from_secs(1), None),
            true,
            &media_info,
        );

        assert_eq!(timeline.kind, PlayerTimelineKind::Live);
        assert!(!timeline.is_seekable);
        assert!(timeline.seekable_range.is_none());
        assert!(timeline.duration.is_none());
        assert!(timeline.live_edge.is_none());
    }

    #[test]
    fn timeline_from_media_info_keeps_progressive_unknown_duration_as_vod() {
        let media_info =
            test_media_info(MediaSourceKind::Remote, MediaSourceProtocol::Progressive, None);
        let timeline = PlayerTimelineSnapshot::from_media_info(
            PlaybackProgress::new(Duration::from_secs(1), None),
            true,
            &media_info,
        );

        assert_eq!(timeline.kind, PlayerTimelineKind::Vod);
        assert!(!timeline.is_seekable);
        assert!(timeline.seekable_range.is_none());
        assert!(timeline.duration.is_none());
    }

    #[test]
    fn live_dvr_uses_seekable_window_and_live_edge() {
        let timeline = PlayerTimelineSnapshot::live_dvr(
            PlaybackProgress::new(Duration::from_secs(90), None),
            PlayerSeekableRange {
                start: Duration::from_secs(30),
                end: Duration::from_secs(120),
            },
            Some(Duration::from_secs(120)),
        );

        assert_eq!(timeline.kind, PlayerTimelineKind::LiveDvr);
        assert!(timeline.is_seekable);
        assert_eq!(
            timeline.seekable_range.expect("seekable range").start,
            Duration::from_secs(30)
        );
        assert_eq!(timeline.live_edge, Some(Duration::from_secs(120)));
        assert_eq!(timeline.duration, Some(Duration::from_secs(90)));
    }
}
