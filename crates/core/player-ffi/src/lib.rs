mod c_api;

use std::time::{Duration, Instant};

use player_runtime::{
    DecodedAudioSummary, DecodedVideoFrame, FirstFrameReady, MediaAbrMode, MediaAbrPolicy,
    MediaSourceKind, MediaSourceProtocol, MediaTrack, MediaTrackCatalog, MediaTrackKind,
    MediaTrackSelection, MediaTrackSelectionMode, MediaTrackSelectionSnapshot, PlaybackProgress,
    PlayerAudioInfo, PlayerAudioOutputInfo, PlayerMediaInfo, PlayerRuntime, PlayerRuntimeBootstrap,
    PlayerRuntimeCommand, PlayerRuntimeCommandResult, PlayerRuntimeError, PlayerRuntimeErrorCode,
    PlayerRuntimeEvent, PlayerRuntimeInitializer, PlayerRuntimeStartup, PlayerSeekableRange,
    PlayerSnapshot, PlayerTimelineKind, PlayerTimelineSnapshot, PlayerVideoDecodeInfo,
    PlayerVideoDecodeMode, PlayerVideoInfo, PresentationState, VideoPixelFormat,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FfiTrackKind {
    Video,
    Audio,
    Subtitle,
}

#[derive(Debug, Clone)]
pub struct FfiTrack {
    pub id: String,
    pub kind: FfiTrackKind,
    pub label: Option<String>,
    pub language: Option<String>,
    pub codec: Option<String>,
    pub bit_rate: Option<u64>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub frame_rate: Option<f64>,
    pub channels: Option<u16>,
    pub sample_rate: Option<u32>,
    pub is_default: bool,
    pub is_forced: bool,
}

#[derive(Debug, Clone, Default)]
pub struct FfiTrackCatalog {
    pub tracks: Vec<FfiTrack>,
    pub adaptive_video: bool,
    pub adaptive_audio: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FfiTrackSelectionMode {
    Auto,
    Disabled,
    Track,
}

#[derive(Debug, Clone)]
pub struct FfiTrackSelection {
    pub mode: FfiTrackSelectionMode,
    pub track_id: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FfiAbrMode {
    Auto,
    Constrained,
    FixedTrack,
}

#[derive(Debug, Clone)]
pub struct FfiAbrPolicy {
    pub mode: FfiAbrMode,
    pub track_id: Option<String>,
    pub max_bit_rate: Option<u64>,
    pub max_width: Option<u32>,
    pub max_height: Option<u32>,
}

#[derive(Debug, Clone)]
pub struct FfiTrackSelectionSnapshot {
    pub video: FfiTrackSelection,
    pub audio: FfiTrackSelection,
    pub subtitle: FfiTrackSelection,
    pub abr_policy: FfiAbrPolicy,
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
    pub track_catalog: FfiTrackCatalog,
    pub track_selection: FfiTrackSelectionSnapshot,
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

impl From<MediaTrackKind> for FfiTrackKind {
    fn from(value: MediaTrackKind) -> Self {
        match value {
            MediaTrackKind::Video => Self::Video,
            MediaTrackKind::Audio => Self::Audio,
            MediaTrackKind::Subtitle => Self::Subtitle,
        }
    }
}

impl From<MediaTrack> for FfiTrack {
    fn from(value: MediaTrack) -> Self {
        Self {
            id: value.id,
            kind: value.kind.into(),
            label: value.label,
            language: value.language,
            codec: value.codec,
            bit_rate: value.bit_rate,
            width: value.width,
            height: value.height,
            frame_rate: value.frame_rate,
            channels: value.channels,
            sample_rate: value.sample_rate,
            is_default: value.is_default,
            is_forced: value.is_forced,
        }
    }
}

impl From<&MediaTrack> for FfiTrack {
    fn from(value: &MediaTrack) -> Self {
        Self::from(value.clone())
    }
}

impl From<MediaTrackCatalog> for FfiTrackCatalog {
    fn from(value: MediaTrackCatalog) -> Self {
        Self {
            tracks: value.tracks.into_iter().map(FfiTrack::from).collect(),
            adaptive_video: value.adaptive_video,
            adaptive_audio: value.adaptive_audio,
        }
    }
}

impl From<&MediaTrackCatalog> for FfiTrackCatalog {
    fn from(value: &MediaTrackCatalog) -> Self {
        Self::from(value.clone())
    }
}

impl From<MediaTrackSelectionMode> for FfiTrackSelectionMode {
    fn from(value: MediaTrackSelectionMode) -> Self {
        match value {
            MediaTrackSelectionMode::Auto => Self::Auto,
            MediaTrackSelectionMode::Disabled => Self::Disabled,
            MediaTrackSelectionMode::Track => Self::Track,
        }
    }
}

impl From<MediaTrackSelection> for FfiTrackSelection {
    fn from(value: MediaTrackSelection) -> Self {
        Self {
            mode: value.mode.into(),
            track_id: value.track_id,
        }
    }
}

impl From<FfiTrackSelection> for MediaTrackSelection {
    fn from(value: FfiTrackSelection) -> Self {
        Self {
            mode: value.mode.into(),
            track_id: value.track_id,
        }
    }
}

impl From<&MediaTrackSelection> for FfiTrackSelection {
    fn from(value: &MediaTrackSelection) -> Self {
        Self::from(value.clone())
    }
}

impl From<MediaAbrMode> for FfiAbrMode {
    fn from(value: MediaAbrMode) -> Self {
        match value {
            MediaAbrMode::Auto => Self::Auto,
            MediaAbrMode::Constrained => Self::Constrained,
            MediaAbrMode::FixedTrack => Self::FixedTrack,
        }
    }
}

impl From<FfiTrackSelectionMode> for MediaTrackSelectionMode {
    fn from(value: FfiTrackSelectionMode) -> Self {
        match value {
            FfiTrackSelectionMode::Auto => Self::Auto,
            FfiTrackSelectionMode::Disabled => Self::Disabled,
            FfiTrackSelectionMode::Track => Self::Track,
        }
    }
}

impl From<MediaAbrPolicy> for FfiAbrPolicy {
    fn from(value: MediaAbrPolicy) -> Self {
        Self {
            mode: value.mode.into(),
            track_id: value.track_id,
            max_bit_rate: value.max_bit_rate,
            max_width: value.max_width,
            max_height: value.max_height,
        }
    }
}

impl From<FfiAbrMode> for MediaAbrMode {
    fn from(value: FfiAbrMode) -> Self {
        match value {
            FfiAbrMode::Auto => Self::Auto,
            FfiAbrMode::Constrained => Self::Constrained,
            FfiAbrMode::FixedTrack => Self::FixedTrack,
        }
    }
}

impl From<FfiAbrPolicy> for MediaAbrPolicy {
    fn from(value: FfiAbrPolicy) -> Self {
        Self {
            mode: value.mode.into(),
            track_id: value.track_id,
            max_bit_rate: value.max_bit_rate,
            max_width: value.max_width,
            max_height: value.max_height,
        }
    }
}

impl From<&MediaAbrPolicy> for FfiAbrPolicy {
    fn from(value: &MediaAbrPolicy) -> Self {
        Self::from(value.clone())
    }
}

impl From<MediaTrackSelectionSnapshot> for FfiTrackSelectionSnapshot {
    fn from(value: MediaTrackSelectionSnapshot) -> Self {
        Self {
            video: value.video.into(),
            audio: value.audio.into(),
            subtitle: value.subtitle.into(),
            abr_policy: value.abr_policy.into(),
        }
    }
}

impl From<&MediaTrackSelectionSnapshot> for FfiTrackSelectionSnapshot {
    fn from(value: &MediaTrackSelectionSnapshot) -> Self {
        Self::from(value.clone())
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
            track_catalog: value.track_catalog.into(),
            track_selection: value.track_selection.into(),
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

    pub fn set_video_track_selection(
        &mut self,
        selection: FfiTrackSelection,
    ) -> FfiResult<FfiCommandResult> {
        self.inner
            .set_video_track_selection(selection.into())
            .map(FfiCommandResult::from)
            .map_err(FfiError::from)
    }

    pub fn set_audio_track_selection(
        &mut self,
        selection: FfiTrackSelection,
    ) -> FfiResult<FfiCommandResult> {
        self.inner
            .set_audio_track_selection(selection.into())
            .map(FfiCommandResult::from)
            .map_err(FfiError::from)
    }

    pub fn set_subtitle_track_selection(
        &mut self,
        selection: FfiTrackSelection,
    ) -> FfiResult<FfiCommandResult> {
        self.inner
            .set_subtitle_track_selection(selection.into())
            .map(FfiCommandResult::from)
            .map_err(FfiError::from)
    }

    pub fn set_abr_policy(&mut self, policy: FfiAbrPolicy) -> FfiResult<FfiCommandResult> {
        self.inner
            .set_abr_policy(policy.into())
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

impl Default for FfiTrackSelection {
    fn default() -> Self {
        Self {
            mode: FfiTrackSelectionMode::Auto,
            track_id: None,
        }
    }
}

impl Default for FfiAbrPolicy {
    fn default() -> Self {
        Self {
            mode: FfiAbrMode::Auto,
            track_id: None,
            max_bit_rate: None,
            max_width: None,
            max_height: None,
        }
    }
}

impl Default for FfiTrackSelectionSnapshot {
    fn default() -> Self {
        Self {
            video: FfiTrackSelection::default(),
            audio: FfiTrackSelection::default(),
            subtitle: FfiTrackSelection {
                mode: FfiTrackSelectionMode::Disabled,
                track_id: None,
            },
            abr_policy: FfiAbrPolicy::default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        FfiAbrMode, FfiMediaInfo, FfiTrackKind, FfiTrackSelectionMode, MediaAbrMode,
        MediaAbrPolicy, MediaSourceKind, MediaSourceProtocol, MediaTrack, MediaTrackCatalog,
        MediaTrackKind, MediaTrackSelection, MediaTrackSelectionSnapshot, PlayerMediaInfo,
    };
    use std::time::Duration;

    #[test]
    fn media_info_to_ffi_preserves_track_catalog_and_selection() {
        let media_info = PlayerMediaInfo {
            source_uri: "https://example.com/master.m3u8".to_owned(),
            source_kind: MediaSourceKind::Remote,
            source_protocol: MediaSourceProtocol::Hls,
            duration: Some(Duration::from_secs(60)),
            bit_rate: Some(2_400_000),
            audio_streams: 2,
            video_streams: 1,
            best_video: None,
            best_audio: None,
            track_catalog: MediaTrackCatalog {
                tracks: vec![
                    MediaTrack {
                        id: "video-1080p".to_owned(),
                        kind: MediaTrackKind::Video,
                        label: Some("1080p".to_owned()),
                        language: None,
                        codec: Some("avc1".to_owned()),
                        bit_rate: Some(2_400_000),
                        width: Some(1920),
                        height: Some(1080),
                        frame_rate: Some(30.0),
                        channels: None,
                        sample_rate: None,
                        is_default: true,
                        is_forced: false,
                    },
                    MediaTrack {
                        id: "audio-en".to_owned(),
                        kind: MediaTrackKind::Audio,
                        label: Some("English".to_owned()),
                        language: Some("en".to_owned()),
                        codec: Some("aac".to_owned()),
                        bit_rate: Some(128_000),
                        width: None,
                        height: None,
                        frame_rate: None,
                        channels: Some(2),
                        sample_rate: Some(48_000),
                        is_default: true,
                        is_forced: false,
                    },
                ],
                adaptive_video: true,
                adaptive_audio: false,
            },
            track_selection: MediaTrackSelectionSnapshot {
                video: MediaTrackSelection::track("video-1080p"),
                audio: MediaTrackSelection::track("audio-en"),
                subtitle: MediaTrackSelection::disabled(),
                abr_policy: MediaAbrPolicy {
                    mode: MediaAbrMode::FixedTrack,
                    track_id: Some("video-1080p".to_owned()),
                    max_bit_rate: Some(2_400_000),
                    max_width: Some(1920),
                    max_height: Some(1080),
                },
            },
        };

        let ffi = FfiMediaInfo::from(media_info);

        assert_eq!(ffi.track_catalog.tracks.len(), 2);
        assert!(ffi.track_catalog.adaptive_video);
        assert_eq!(ffi.track_catalog.tracks[0].kind, FfiTrackKind::Video);
        assert_eq!(ffi.track_catalog.tracks[0].bit_rate, Some(2_400_000));
        assert_eq!(ffi.track_catalog.tracks[1].kind, FfiTrackKind::Audio);
        assert_eq!(ffi.track_catalog.tracks[1].language.as_deref(), Some("en"));
        assert_eq!(ffi.track_selection.video.mode, FfiTrackSelectionMode::Track);
        assert_eq!(
            ffi.track_selection.video.track_id.as_deref(),
            Some("video-1080p")
        );
        assert_eq!(ffi.track_selection.abr_policy.mode, FfiAbrMode::FixedTrack);
        assert_eq!(
            ffi.track_selection.abr_policy.track_id.as_deref(),
            Some("video-1080p")
        );
    }
}
