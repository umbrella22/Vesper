mod c_api;

use std::time::{Duration, Instant};

use player_runtime::{
    DecodedAudioSummary, DecodedVideoFrame, FirstFrameReady, MediaSourceKind, MediaSourceProtocol,
    PlaybackProgress, PlayerAudioInfo, PlayerAudioOutputInfo, PlayerMediaInfo, PlayerRuntime,
    PlayerRuntimeBootstrap, PlayerRuntimeCommand, PlayerRuntimeCommandResult, PlayerRuntimeError,
    PlayerRuntimeErrorCode, PlayerRuntimeEvent, PlayerRuntimeInitializer, PlayerRuntimeStartup,
    PlayerSeekableRange, PlayerSnapshot, PlayerTimelineKind, PlayerTimelineSnapshot,
    PlayerVideoDecodeInfo, PlayerVideoDecodeMode, PlayerVideoInfo, PresentationState,
    VideoPixelFormat,
};

pub type FfiResult<T> = Result<T, FfiError>;

pub use c_api::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FfiPlaybackState {
    Ready,
    Playing,
    Paused,
    Finished,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FfiErrorCode {
    InvalidArgument,
    InvalidState,
    InvalidSource,
    BackendFailure,
    AudioOutputUnavailable,
    DecodeFailure,
    SeekFailure,
    Unsupported,
}

#[derive(Debug, Clone)]
pub struct FfiError {
    code: FfiErrorCode,
    message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FfiPixelFormat {
    Rgba8888,
    Yuv420p,
}

#[derive(Debug, Clone)]
pub struct FfiVideoInfo {
    pub codec: String,
    pub width: u32,
    pub height: u32,
    pub frame_rate: Option<f64>,
}

#[derive(Debug, Clone)]
pub struct FfiAudioInfo {
    pub codec: String,
    pub sample_rate: u32,
    pub channels: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FfiMediaSourceKind {
    Local,
    Remote,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FfiMediaSourceProtocol {
    Unknown,
    File,
    Content,
    Progressive,
    Hls,
    Dash,
}

#[derive(Debug, Clone)]
pub struct FfiMediaInfo {
    pub source_uri: String,
    pub source_kind: FfiMediaSourceKind,
    pub source_protocol: FfiMediaSourceProtocol,
    pub duration_ms: Option<u64>,
    pub bit_rate: Option<u64>,
    pub audio_streams: usize,
    pub video_streams: usize,
    pub best_video: Option<FfiVideoInfo>,
    pub best_audio: Option<FfiAudioInfo>,
}

#[derive(Debug, Clone)]
pub struct FfiAudioOutputInfo {
    pub device_name: Option<String>,
    pub channels: Option<u16>,
    pub sample_rate: Option<u32>,
    pub sample_format: Option<String>,
}

#[derive(Debug, Clone)]
pub struct FfiDecodedAudioSummary {
    pub channels: u16,
    pub sample_rate: u32,
    pub duration_ms: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FfiVideoDecodeMode {
    Software,
    Hardware,
}

#[derive(Debug, Clone)]
pub struct FfiVideoDecodeInfo {
    pub selected_mode: FfiVideoDecodeMode,
    pub hardware_available: bool,
    pub hardware_backend: Option<String>,
    pub fallback_reason: Option<String>,
}

#[derive(Debug, Clone)]
pub struct FfiStartup {
    pub ffmpeg_initialized: bool,
    pub audio_output: Option<FfiAudioOutputInfo>,
    pub decoded_audio: Option<FfiDecodedAudioSummary>,
    pub video_decode: Option<FfiVideoDecodeInfo>,
}

#[derive(Debug, Clone)]
pub struct FfiProgress {
    pub position_ms: u64,
    pub duration_ms: Option<u64>,
    pub ratio: Option<f64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FfiTimelineKind {
    Vod,
    Live,
    LiveDvr,
}

#[derive(Debug, Clone)]
pub struct FfiSeekableRange {
    pub start_ms: u64,
    pub end_ms: u64,
}

#[derive(Debug, Clone)]
pub struct FfiTimelineSnapshot {
    pub kind: FfiTimelineKind,
    pub is_seekable: bool,
    pub seekable_range: Option<FfiSeekableRange>,
    pub live_edge_ms: Option<u64>,
    pub position_ms: u64,
    pub duration_ms: Option<u64>,
    pub ratio: Option<f64>,
}

#[derive(Debug, Clone)]
pub struct FfiSnapshot {
    pub source_uri: String,
    pub state: FfiPlaybackState,
    pub has_video_surface: bool,
    pub is_interrupted: bool,
    pub is_buffering: bool,
    pub playback_rate: f32,
    pub progress: FfiProgress,
    pub timeline: FfiTimelineSnapshot,
    pub media_info: FfiMediaInfo,
}

#[derive(Debug, Clone)]
pub struct FfiVideoFrame {
    pub presentation_time_ms: u64,
    pub width: u32,
    pub height: u32,
    pub bytes_per_row: u32,
    pub pixel_format: FfiPixelFormat,
    pub bytes: Vec<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FfiCommand {
    Play,
    Pause,
    TogglePause,
    SeekTo { position_ms: u64 },
    Stop,
}

#[derive(Debug, Clone)]
pub struct FfiCommandResult {
    pub applied: bool,
    pub frame: Option<FfiVideoFrame>,
    pub snapshot: FfiSnapshot,
}

#[derive(Debug, Clone)]
pub struct FfiFirstFrameReady {
    pub presentation_time_ms: u64,
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone)]
pub enum FfiEvent {
    Initialized(FfiStartup),
    MetadataReady(FfiMediaInfo),
    FirstFrameReady(FfiFirstFrameReady),
    PlaybackStateChanged(FfiPlaybackState),
    InterruptionChanged { interrupted: bool },
    BufferingChanged { buffering: bool },
    VideoSurfaceChanged { attached: bool },
    AudioOutputChanged(Option<FfiAudioOutputInfo>),
    PlaybackRateChanged { rate: f32 },
    SeekCompleted { position_ms: u64 },
    Error(FfiError),
    Ended,
}

#[derive(Debug)]
pub struct FfiPlayerInitializer {
    inner: PlayerRuntimeInitializer,
}

#[derive(Debug)]
pub struct FfiPlayerBootstrap {
    pub player: FfiPlayer,
    pub initial_frame: Option<FfiVideoFrame>,
    pub startup: FfiStartup,
}

#[cfg(target_os = "linux")]
use player_platform_linux::install_default_linux_runtime_adapter_factory as install_host_desktop_runtime_adapter_factory;
#[cfg(target_os = "macos")]
use player_platform_macos::install_default_macos_runtime_adapter_factory as install_host_desktop_runtime_adapter_factory;
#[cfg(target_os = "windows")]
use player_platform_windows::install_default_windows_runtime_adapter_factory as install_host_desktop_runtime_adapter_factory;

pub struct FfiPlayer {
    inner: PlayerRuntime,
}

impl std::fmt::Debug for FfiPlayer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FfiPlayer")
            .field("source_uri", &self.inner.source_uri())
            .field("state", &self.inner.presentation_state())
            .finish()
    }
}

impl FfiError {
    pub fn code(&self) -> FfiErrorCode {
        self.code
    }

    pub fn message(&self) -> &str {
        &self.message
    }
}

impl From<PresentationState> for FfiPlaybackState {
    fn from(value: PresentationState) -> Self {
        match value {
            PresentationState::Ready => Self::Ready,
            PresentationState::Playing => Self::Playing,
            PresentationState::Paused => Self::Paused,
            PresentationState::Finished => Self::Finished,
        }
    }
}

impl From<PlayerRuntimeErrorCode> for FfiErrorCode {
    fn from(value: PlayerRuntimeErrorCode) -> Self {
        match value {
            PlayerRuntimeErrorCode::InvalidArgument => Self::InvalidArgument,
            PlayerRuntimeErrorCode::InvalidState => Self::InvalidState,
            PlayerRuntimeErrorCode::InvalidSource => Self::InvalidSource,
            PlayerRuntimeErrorCode::BackendFailure => Self::BackendFailure,
            PlayerRuntimeErrorCode::AudioOutputUnavailable => Self::AudioOutputUnavailable,
            PlayerRuntimeErrorCode::DecodeFailure => Self::DecodeFailure,
            PlayerRuntimeErrorCode::SeekFailure => Self::SeekFailure,
            PlayerRuntimeErrorCode::Unsupported => Self::Unsupported,
        }
    }
}

impl From<PlayerRuntimeError> for FfiError {
    fn from(value: PlayerRuntimeError) -> Self {
        Self {
            code: value.code().into(),
            message: value.message().to_owned(),
        }
    }
}

impl From<PlayerVideoInfo> for FfiVideoInfo {
    fn from(value: PlayerVideoInfo) -> Self {
        Self {
            codec: value.codec,
            width: value.width,
            height: value.height,
            frame_rate: value.frame_rate,
        }
    }
}

impl From<&PlayerVideoInfo> for FfiVideoInfo {
    fn from(value: &PlayerVideoInfo) -> Self {
        Self::from(value.clone())
    }
}

impl From<PlayerAudioInfo> for FfiAudioInfo {
    fn from(value: PlayerAudioInfo) -> Self {
        Self {
            codec: value.codec,
            sample_rate: value.sample_rate,
            channels: value.channels,
        }
    }
}

impl From<&PlayerAudioInfo> for FfiAudioInfo {
    fn from(value: &PlayerAudioInfo) -> Self {
        Self::from(value.clone())
    }
}

impl From<MediaSourceKind> for FfiMediaSourceKind {
    fn from(value: MediaSourceKind) -> Self {
        match value {
            MediaSourceKind::Local => Self::Local,
            MediaSourceKind::Remote => Self::Remote,
        }
    }
}

impl From<MediaSourceProtocol> for FfiMediaSourceProtocol {
    fn from(value: MediaSourceProtocol) -> Self {
        match value {
            MediaSourceProtocol::Unknown => Self::Unknown,
            MediaSourceProtocol::File => Self::File,
            MediaSourceProtocol::Content => Self::Content,
            MediaSourceProtocol::Progressive => Self::Progressive,
            MediaSourceProtocol::Hls => Self::Hls,
            MediaSourceProtocol::Dash => Self::Dash,
        }
    }
}

impl From<PlayerMediaInfo> for FfiMediaInfo {
    fn from(value: PlayerMediaInfo) -> Self {
        Self {
            source_uri: value.source_uri,
            source_kind: value.source_kind.into(),
            source_protocol: value.source_protocol.into(),
            duration_ms: value.duration.map(duration_to_millis),
            bit_rate: value.bit_rate,
            audio_streams: value.audio_streams,
            video_streams: value.video_streams,
            best_video: value.best_video.map(FfiVideoInfo::from),
            best_audio: value.best_audio.map(FfiAudioInfo::from),
        }
    }
}

impl From<&PlayerMediaInfo> for FfiMediaInfo {
    fn from(value: &PlayerMediaInfo) -> Self {
        Self::from(value.clone())
    }
}

impl From<PlayerAudioOutputInfo> for FfiAudioOutputInfo {
    fn from(value: PlayerAudioOutputInfo) -> Self {
        Self {
            device_name: value.device_name,
            channels: value.channels,
            sample_rate: value.sample_rate,
            sample_format: value.sample_format,
        }
    }
}

impl From<&PlayerAudioOutputInfo> for FfiAudioOutputInfo {
    fn from(value: &PlayerAudioOutputInfo) -> Self {
        Self::from(value.clone())
    }
}

impl From<DecodedAudioSummary> for FfiDecodedAudioSummary {
    fn from(value: DecodedAudioSummary) -> Self {
        Self {
            channels: value.channels,
            sample_rate: value.sample_rate,
            duration_ms: duration_to_millis(value.duration),
        }
    }
}

impl From<&DecodedAudioSummary> for FfiDecodedAudioSummary {
    fn from(value: &DecodedAudioSummary) -> Self {
        Self::from(value.clone())
    }
}

impl From<PlayerVideoDecodeMode> for FfiVideoDecodeMode {
    fn from(value: PlayerVideoDecodeMode) -> Self {
        match value {
            PlayerVideoDecodeMode::Software => Self::Software,
            PlayerVideoDecodeMode::Hardware => Self::Hardware,
        }
    }
}

impl From<PlayerVideoDecodeInfo> for FfiVideoDecodeInfo {
    fn from(value: PlayerVideoDecodeInfo) -> Self {
        Self {
            selected_mode: value.selected_mode.into(),
            hardware_available: value.hardware_available,
            hardware_backend: value.hardware_backend,
            fallback_reason: value.fallback_reason,
        }
    }
}

impl From<&PlayerVideoDecodeInfo> for FfiVideoDecodeInfo {
    fn from(value: &PlayerVideoDecodeInfo) -> Self {
        Self::from(value.clone())
    }
}

impl From<PlayerRuntimeStartup> for FfiStartup {
    fn from(value: PlayerRuntimeStartup) -> Self {
        Self {
            ffmpeg_initialized: value.ffmpeg_initialized,
            audio_output: value.audio_output.map(FfiAudioOutputInfo::from),
            decoded_audio: value.decoded_audio.map(FfiDecodedAudioSummary::from),
            video_decode: value.video_decode.map(FfiVideoDecodeInfo::from),
        }
    }
}

impl From<&PlayerRuntimeStartup> for FfiStartup {
    fn from(value: &PlayerRuntimeStartup) -> Self {
        Self::from(value.clone())
    }
}

impl From<PlaybackProgress> for FfiProgress {
    fn from(value: PlaybackProgress) -> Self {
        Self {
            position_ms: duration_to_millis(value.position()),
            duration_ms: value.duration().map(duration_to_millis),
            ratio: value.ratio(),
        }
    }
}

impl From<PlayerTimelineKind> for FfiTimelineKind {
    fn from(value: PlayerTimelineKind) -> Self {
        match value {
            PlayerTimelineKind::Vod => Self::Vod,
            PlayerTimelineKind::Live => Self::Live,
            PlayerTimelineKind::LiveDvr => Self::LiveDvr,
        }
    }
}

impl From<PlayerSeekableRange> for FfiSeekableRange {
    fn from(value: PlayerSeekableRange) -> Self {
        Self {
            start_ms: duration_to_millis(value.start),
            end_ms: duration_to_millis(value.end),
        }
    }
}

impl From<PlayerTimelineSnapshot> for FfiTimelineSnapshot {
    fn from(value: PlayerTimelineSnapshot) -> Self {
        Self {
            kind: value.kind.into(),
            is_seekable: value.is_seekable,
            seekable_range: value.seekable_range.map(FfiSeekableRange::from),
            live_edge_ms: value.live_edge.map(duration_to_millis),
            position_ms: duration_to_millis(value.position),
            duration_ms: value.duration.map(duration_to_millis),
            ratio: value.displayed_ratio(),
        }
    }
}

impl From<PlayerSnapshot> for FfiSnapshot {
    fn from(value: PlayerSnapshot) -> Self {
        Self {
            source_uri: value.source_uri,
            state: value.state.into(),
            has_video_surface: value.has_video_surface,
            is_interrupted: value.is_interrupted,
            is_buffering: value.is_buffering,
            playback_rate: value.playback_rate,
            progress: value.progress.into(),
            timeline: value.timeline.into(),
            media_info: value.media_info.into(),
        }
    }
}

impl From<&PlayerSnapshot> for FfiSnapshot {
    fn from(value: &PlayerSnapshot) -> Self {
        Self::from(value.clone())
    }
}

impl From<DecodedVideoFrame> for FfiVideoFrame {
    fn from(value: DecodedVideoFrame) -> Self {
        Self {
            presentation_time_ms: duration_to_millis(value.presentation_time),
            width: value.width,
            height: value.height,
            bytes_per_row: value.bytes_per_row,
            pixel_format: value.pixel_format.into(),
            bytes: value.bytes,
        }
    }
}

impl From<&DecodedVideoFrame> for FfiVideoFrame {
    fn from(value: &DecodedVideoFrame) -> Self {
        Self {
            presentation_time_ms: duration_to_millis(value.presentation_time),
            width: value.width,
            height: value.height,
            bytes_per_row: value.bytes_per_row,
            pixel_format: value.pixel_format.into(),
            bytes: value.bytes.clone(),
        }
    }
}

impl From<VideoPixelFormat> for FfiPixelFormat {
    fn from(value: VideoPixelFormat) -> Self {
        match value {
            VideoPixelFormat::Rgba8888 => Self::Rgba8888,
            VideoPixelFormat::Yuv420p => Self::Yuv420p,
        }
    }
}

impl From<FirstFrameReady> for FfiFirstFrameReady {
    fn from(value: FirstFrameReady) -> Self {
        Self {
            presentation_time_ms: duration_to_millis(value.presentation_time),
            width: value.width,
            height: value.height,
        }
    }
}

impl From<&FirstFrameReady> for FfiFirstFrameReady {
    fn from(value: &FirstFrameReady) -> Self {
        Self::from(value.clone())
    }
}

impl From<PlayerRuntimeEvent> for FfiEvent {
    fn from(value: PlayerRuntimeEvent) -> Self {
        match value {
            PlayerRuntimeEvent::Initialized(startup) => Self::Initialized(startup.into()),
            PlayerRuntimeEvent::MetadataReady(media_info) => Self::MetadataReady(media_info.into()),
            PlayerRuntimeEvent::FirstFrameReady(frame) => Self::FirstFrameReady(frame.into()),
            PlayerRuntimeEvent::PlaybackStateChanged(state) => {
                Self::PlaybackStateChanged(state.into())
            }
            PlayerRuntimeEvent::InterruptionChanged { interrupted } => {
                Self::InterruptionChanged { interrupted }
            }
            PlayerRuntimeEvent::BufferingChanged { buffering } => {
                Self::BufferingChanged { buffering }
            }
            PlayerRuntimeEvent::VideoSurfaceChanged { attached } => {
                Self::VideoSurfaceChanged { attached }
            }
            PlayerRuntimeEvent::AudioOutputChanged(audio_output) => {
                Self::AudioOutputChanged(audio_output.map(FfiAudioOutputInfo::from))
            }
            PlayerRuntimeEvent::PlaybackRateChanged { rate } => Self::PlaybackRateChanged { rate },
            PlayerRuntimeEvent::SeekCompleted { position } => Self::SeekCompleted {
                position_ms: duration_to_millis(position),
            },
            PlayerRuntimeEvent::Error(error) => Self::Error(error.into()),
            PlayerRuntimeEvent::Ended => Self::Ended,
        }
    }
}

impl From<FfiCommand> for PlayerRuntimeCommand {
    fn from(value: FfiCommand) -> Self {
        match value {
            FfiCommand::Play => Self::Play,
            FfiCommand::Pause => Self::Pause,
            FfiCommand::TogglePause => Self::TogglePause,
            FfiCommand::SeekTo { position_ms } => Self::SeekTo {
                position: Duration::from_millis(position_ms),
            },
            FfiCommand::Stop => Self::Stop,
        }
    }
}

impl From<PlayerRuntimeCommandResult> for FfiCommandResult {
    fn from(value: PlayerRuntimeCommandResult) -> Self {
        Self {
            applied: value.applied,
            frame: value.frame.map(FfiVideoFrame::from),
            snapshot: value.snapshot.into(),
        }
    }
}

impl FfiPlayerInitializer {
    pub fn probe_uri(uri: impl Into<String>) -> FfiResult<Self> {
        install_host_desktop_runtime_adapter_factory().map_err(FfiError::from)?;
        Ok(Self {
            inner: PlayerRuntimeInitializer::probe_uri(uri).map_err(FfiError::from)?,
        })
    }

    pub fn media_info(&self) -> FfiMediaInfo {
        self.inner.media_info().into()
    }

    pub fn startup(&self) -> FfiStartup {
        self.inner.startup().into()
    }

    pub fn initialize(self) -> FfiResult<FfiPlayerBootstrap> {
        let bootstrap = self.inner.initialize().map_err(FfiError::from)?;
        Ok(FfiPlayerBootstrap::from(bootstrap))
    }
}

impl From<PlayerRuntimeBootstrap> for FfiPlayerBootstrap {
    fn from(value: PlayerRuntimeBootstrap) -> Self {
        Self {
            player: FfiPlayer {
                inner: value.runtime,
            },
            initial_frame: value.initial_frame.map(FfiVideoFrame::from),
            startup: value.startup.into(),
        }
    }
}

impl FfiPlayer {
    pub fn source_uri(&self) -> &str {
        self.inner.source_uri()
    }

    pub fn snapshot(&self) -> FfiSnapshot {
        self.inner.snapshot().into()
    }

    pub fn dispatch(&mut self, command: FfiCommand) -> FfiResult<FfiCommandResult> {
        self.inner
            .dispatch(command.into())
            .map(FfiCommandResult::from)
            .map_err(FfiError::from)
    }

    pub fn set_playback_rate(&mut self, rate: f32) -> FfiResult<FfiCommandResult> {
        self.inner
            .set_playback_rate(rate)
            .map(FfiCommandResult::from)
            .map_err(FfiError::from)
    }

    pub fn drain_events(&mut self) -> Vec<FfiEvent> {
        self.inner
            .drain_events()
            .into_iter()
            .map(FfiEvent::from)
            .collect()
    }

    pub fn advance(&mut self) -> FfiResult<Option<FfiVideoFrame>> {
        self.inner
            .advance()
            .map(|frame| frame.map(FfiVideoFrame::from))
            .map_err(FfiError::from)
    }

    pub fn next_deadline_delay_ms(&self) -> Option<u64> {
        let now = Instant::now();
        self.inner
            .next_deadline()
            .map(|deadline| duration_to_millis(deadline.saturating_duration_since(now)))
    }
}

fn duration_to_millis(duration: Duration) -> u64 {
    u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
}
