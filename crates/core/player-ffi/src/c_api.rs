use std::ffi::{c_char, CStr, CString};
use std::mem;
use std::ptr;

use crate::{
    FfiAbrMode as BridgeAbrMode, FfiAbrPolicy as BridgeAbrPolicy, FfiAudioInfo, FfiAudioOutputInfo,
    FfiCommand, FfiDecodedAudioSummary, FfiError as BridgeError,
    FfiErrorCategory as BridgeErrorCategory, FfiErrorCode as BridgeErrorCode,
    FfiEvent as BridgeEvent, FfiFirstFrameReady, FfiMediaInfo as BridgeMediaInfo,
    FfiMediaSourceKind as BridgeMediaSourceKind,
    FfiMediaSourceProtocol as BridgeMediaSourceProtocol, FfiPixelFormat as BridgePixelFormat,
    FfiPlaybackState, FfiPlayer, FfiPlayerInitializer, FfiProgress as BridgeProgress,
    FfiSeekableRange as BridgeSeekableRange, FfiSnapshot as BridgeSnapshot,
    FfiStartup as BridgeStartup, FfiTimelineKind as BridgeTimelineKind,
    FfiTimelineSnapshot as BridgeTimelineSnapshot, FfiTrack as BridgeTrack,
    FfiTrackCatalog as BridgeTrackCatalog, FfiTrackKind as BridgeTrackKind,
    FfiTrackSelection as BridgeTrackSelection, FfiTrackSelectionMode as BridgeTrackSelectionMode,
    FfiTrackSelectionSnapshot as BridgeTrackSelectionSnapshot,
    FfiVideoDecodeInfo as BridgeVideoDecodeInfo, FfiVideoDecodeMode as BridgeVideoDecodeMode,
    FfiVideoFrame as BridgeVideoFrame, FfiVideoInfo,
};

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PlayerFfiCallStatus {
    #[default]
    Ok = 0,
    Error = 1,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PlayerFfiPlaybackState {
    #[default]
    Ready = 0,
    Playing = 1,
    Paused = 2,
    Finished = 3,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PlayerFfiPixelFormat {
    #[default]
    Rgba8888 = 0,
    Yuv420p = 1,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PlayerFfiTimelineKind {
    #[default]
    Vod = 0,
    Live = 1,
    LiveDvr = 2,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PlayerFfiMediaSourceKind {
    Local = 0,
    #[default]
    Remote = 1,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PlayerFfiMediaSourceProtocol {
    #[default]
    Unknown = 0,
    File = 1,
    Content = 2,
    Progressive = 3,
    Hls = 4,
    Dash = 5,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PlayerFfiTrackKind {
    #[default]
    Video = 0,
    Audio = 1,
    Subtitle = 2,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PlayerFfiTrackSelectionMode {
    #[default]
    Auto = 0,
    Disabled = 1,
    Track = 2,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PlayerFfiAbrMode {
    #[default]
    Auto = 0,
    Constrained = 1,
    FixedTrack = 2,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PlayerFfiErrorCode {
    #[default]
    None = 0,
    NullPointer = 1,
    InvalidUtf8 = 2,
    InvalidArgument = 3,
    InvalidState = 4,
    InvalidSource = 5,
    BackendFailure = 6,
    AudioOutputUnavailable = 7,
    DecodeFailure = 8,
    SeekFailure = 9,
    Unsupported = 10,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PlayerFfiErrorCategory {
    Input = 0,
    Source = 1,
    Network = 2,
    Decode = 3,
    AudioOutput = 4,
    Playback = 5,
    Capability = 6,
    #[default]
    Platform = 7,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PlayerFfiCommandKind {
    #[default]
    Play = 0,
    Pause = 1,
    TogglePause = 2,
    SeekTo = 3,
    Stop = 4,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PlayerFfiEventKind {
    #[default]
    Initialized = 0,
    MetadataReady = 1,
    FirstFrameReady = 2,
    PlaybackStateChanged = 3,
    BufferingChanged = 4,
    VideoSurfaceChanged = 5,
    AudioOutputChanged = 6,
    PlaybackRateChanged = 7,
    SeekCompleted = 8,
    Error = 9,
    Ended = 10,
    InterruptionChanged = 11,
    RetryScheduled = 12,
}

#[repr(C)]
#[derive(Debug, Default)]
pub struct PlayerFfiInitializerHandle {
    _private: u8,
}

#[repr(C)]
#[derive(Debug, Default)]
pub struct PlayerFfiHandle {
    _private: u8,
}

#[repr(C)]
#[derive(Debug, Default)]
pub struct PlayerFfiError {
    pub code: PlayerFfiErrorCode,
    pub category: PlayerFfiErrorCategory,
    pub retriable: bool,
    pub message: *mut c_char,
}

#[repr(C)]
#[derive(Debug, Default)]
pub struct PlayerFfiVideoInfo {
    pub codec: *mut c_char,
    pub width: u32,
    pub height: u32,
    pub has_frame_rate: bool,
    pub frame_rate: f64,
}

#[repr(C)]
#[derive(Debug, Default)]
pub struct PlayerFfiAudioInfo {
    pub codec: *mut c_char,
    pub sample_rate: u32,
    pub channels: u16,
}

#[repr(C)]
#[derive(Debug, Default)]
pub struct PlayerFfiTrack {
    pub id: *mut c_char,
    pub kind: PlayerFfiTrackKind,
    pub label: *mut c_char,
    pub language: *mut c_char,
    pub codec: *mut c_char,
    pub has_bit_rate: bool,
    pub bit_rate: u64,
    pub has_width: bool,
    pub width: u32,
    pub has_height: bool,
    pub height: u32,
    pub has_frame_rate: bool,
    pub frame_rate: f64,
    pub has_channels: bool,
    pub channels: u16,
    pub has_sample_rate: bool,
    pub sample_rate: u32,
    pub is_default: bool,
    pub is_forced: bool,
}

#[repr(C)]
#[derive(Debug, Default)]
pub struct PlayerFfiTrackCatalog {
    pub tracks: *mut PlayerFfiTrack,
    pub len: usize,
    pub adaptive_video: bool,
    pub adaptive_audio: bool,
}

#[repr(C)]
#[derive(Debug, Default)]
pub struct PlayerFfiTrackSelection {
    pub mode: PlayerFfiTrackSelectionMode,
    pub track_id: *mut c_char,
}

#[repr(C)]
#[derive(Debug, Default)]
pub struct PlayerFfiAbrPolicy {
    pub mode: PlayerFfiAbrMode,
    pub track_id: *mut c_char,
    pub has_max_bit_rate: bool,
    pub max_bit_rate: u64,
    pub has_max_width: bool,
    pub max_width: u32,
    pub has_max_height: bool,
    pub max_height: u32,
}

#[repr(C)]
#[derive(Debug, Default)]
pub struct PlayerFfiTrackSelectionSnapshot {
    pub video: PlayerFfiTrackSelection,
    pub audio: PlayerFfiTrackSelection,
    pub subtitle: PlayerFfiTrackSelection,
    pub abr_policy: PlayerFfiAbrPolicy,
}

#[repr(C)]
#[derive(Debug, Default)]
pub struct PlayerFfiMediaInfo {
    pub source_uri: *mut c_char,
    pub source_kind: PlayerFfiMediaSourceKind,
    pub source_protocol: PlayerFfiMediaSourceProtocol,
    pub has_duration: bool,
    pub duration_ms: u64,
    pub has_bit_rate: bool,
    pub bit_rate: u64,
    pub audio_streams: usize,
    pub video_streams: usize,
    pub has_best_video: bool,
    pub best_video: PlayerFfiVideoInfo,
    pub has_best_audio: bool,
    pub best_audio: PlayerFfiAudioInfo,
    pub track_catalog: PlayerFfiTrackCatalog,
    pub track_selection: PlayerFfiTrackSelectionSnapshot,
}

#[repr(C)]
#[derive(Debug, Default)]
pub struct PlayerFfiAudioOutputInfo {
    pub device_name: *mut c_char,
    pub has_channels: bool,
    pub channels: u16,
    pub has_sample_rate: bool,
    pub sample_rate: u32,
    pub sample_format: *mut c_char,
}

#[repr(C)]
#[derive(Debug, Default)]
pub struct PlayerFfiDecodedAudioSummary {
    pub channels: u16,
    pub sample_rate: u32,
    pub duration_ms: u64,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PlayerFfiVideoDecodeMode {
    #[default]
    Software = 0,
    Hardware = 1,
}

#[repr(C)]
#[derive(Debug, Default)]
pub struct PlayerFfiVideoDecodeInfo {
    pub selected_mode: PlayerFfiVideoDecodeMode,
    pub hardware_available: bool,
    pub hardware_backend: *mut c_char,
    pub fallback_reason: *mut c_char,
}

#[repr(C)]
#[derive(Debug, Default)]
pub struct PlayerFfiStartup {
    pub ffmpeg_initialized: bool,
    pub has_audio_output: bool,
    pub audio_output: PlayerFfiAudioOutputInfo,
    pub has_decoded_audio: bool,
    pub decoded_audio: PlayerFfiDecodedAudioSummary,
    pub has_video_decode: bool,
    pub video_decode: PlayerFfiVideoDecodeInfo,
}

#[repr(C)]
#[derive(Debug, Default)]
pub struct PlayerFfiProgress {
    pub position_ms: u64,
    pub has_duration: bool,
    pub duration_ms: u64,
    pub has_ratio: bool,
    pub ratio: f64,
}

#[repr(C)]
#[derive(Debug, Default)]
pub struct PlayerFfiSeekableRange {
    pub start_ms: u64,
    pub end_ms: u64,
}

#[repr(C)]
#[derive(Debug, Default)]
pub struct PlayerFfiTimelineSnapshot {
    pub kind: PlayerFfiTimelineKind,
    pub is_seekable: bool,
    pub has_seekable_range: bool,
    pub seekable_range: PlayerFfiSeekableRange,
    pub has_live_edge: bool,
    pub live_edge_ms: u64,
    pub position_ms: u64,
    pub has_duration: bool,
    pub duration_ms: u64,
    pub has_ratio: bool,
    pub ratio: f64,
}

#[repr(C)]
#[derive(Debug, Default)]
pub struct PlayerFfiSnapshot {
    pub source_uri: *mut c_char,
    pub state: PlayerFfiPlaybackState,
    pub has_video_surface: bool,
    pub is_interrupted: bool,
    pub is_buffering: bool,
    pub playback_rate: f32,
    pub progress: PlayerFfiProgress,
    pub timeline: PlayerFfiTimelineSnapshot,
    pub media_info: PlayerFfiMediaInfo,
}

#[repr(C)]
#[derive(Debug, Default)]
pub struct PlayerFfiVideoFrame {
    pub presentation_time_ms: u64,
    pub width: u32,
    pub height: u32,
    pub bytes_per_row: u32,
    pub pixel_format: PlayerFfiPixelFormat,
    pub bytes: *mut u8,
    pub len: usize,
}

#[repr(C)]
#[derive(Debug, Default)]
pub struct PlayerFfiFirstFrameReady {
    pub presentation_time_ms: u64,
    pub width: u32,
    pub height: u32,
}

#[repr(C)]
#[derive(Debug, Default)]
pub struct PlayerFfiEvent {
    pub kind: PlayerFfiEventKind,
    pub initialized: PlayerFfiStartup,
    pub metadata_ready: PlayerFfiMediaInfo,
    pub first_frame_ready: PlayerFfiFirstFrameReady,
    pub playback_state: PlayerFfiPlaybackState,
    pub interrupted: bool,
    pub buffering: bool,
    pub surface_attached: bool,
    pub has_audio_output: bool,
    pub audio_output: PlayerFfiAudioOutputInfo,
    pub playback_rate: f32,
    pub seek_position_ms: u64,
    pub retry_attempt: u32,
    pub retry_delay_ms: u64,
    pub error: PlayerFfiError,
}

#[repr(C)]
#[derive(Debug, Default)]
pub struct PlayerFfiEventList {
    pub ptr: *mut PlayerFfiEvent,
    pub len: usize,
}

impl From<FfiPlaybackState> for PlayerFfiPlaybackState {
    fn from(value: FfiPlaybackState) -> Self {
        match value {
            FfiPlaybackState::Ready => Self::Ready,
            FfiPlaybackState::Playing => Self::Playing,
            FfiPlaybackState::Paused => Self::Paused,
            FfiPlaybackState::Finished => Self::Finished,
        }
    }
}

impl From<BridgePixelFormat> for PlayerFfiPixelFormat {
    fn from(value: BridgePixelFormat) -> Self {
        match value {
            BridgePixelFormat::Rgba8888 => Self::Rgba8888,
            BridgePixelFormat::Yuv420p => Self::Yuv420p,
        }
    }
}

impl From<BridgeTimelineKind> for PlayerFfiTimelineKind {
    fn from(value: BridgeTimelineKind) -> Self {
        match value {
            BridgeTimelineKind::Vod => Self::Vod,
            BridgeTimelineKind::Live => Self::Live,
            BridgeTimelineKind::LiveDvr => Self::LiveDvr,
        }
    }
}

impl From<BridgeMediaSourceKind> for PlayerFfiMediaSourceKind {
    fn from(value: BridgeMediaSourceKind) -> Self {
        match value {
            BridgeMediaSourceKind::Local => Self::Local,
            BridgeMediaSourceKind::Remote => Self::Remote,
        }
    }
}

impl From<BridgeMediaSourceProtocol> for PlayerFfiMediaSourceProtocol {
    fn from(value: BridgeMediaSourceProtocol) -> Self {
        match value {
            BridgeMediaSourceProtocol::Unknown => Self::Unknown,
            BridgeMediaSourceProtocol::File => Self::File,
            BridgeMediaSourceProtocol::Content => Self::Content,
            BridgeMediaSourceProtocol::Progressive => Self::Progressive,
            BridgeMediaSourceProtocol::Hls => Self::Hls,
            BridgeMediaSourceProtocol::Dash => Self::Dash,
        }
    }
}

impl From<BridgeTrackKind> for PlayerFfiTrackKind {
    fn from(value: BridgeTrackKind) -> Self {
        match value {
            BridgeTrackKind::Video => Self::Video,
            BridgeTrackKind::Audio => Self::Audio,
            BridgeTrackKind::Subtitle => Self::Subtitle,
        }
    }
}

impl From<BridgeTrackSelectionMode> for PlayerFfiTrackSelectionMode {
    fn from(value: BridgeTrackSelectionMode) -> Self {
        match value {
            BridgeTrackSelectionMode::Auto => Self::Auto,
            BridgeTrackSelectionMode::Disabled => Self::Disabled,
            BridgeTrackSelectionMode::Track => Self::Track,
        }
    }
}

impl From<BridgeAbrMode> for PlayerFfiAbrMode {
    fn from(value: BridgeAbrMode) -> Self {
        match value {
            BridgeAbrMode::Auto => Self::Auto,
            BridgeAbrMode::Constrained => Self::Constrained,
            BridgeAbrMode::FixedTrack => Self::FixedTrack,
        }
    }
}

impl From<BridgeErrorCode> for PlayerFfiErrorCode {
    fn from(value: BridgeErrorCode) -> Self {
        match value {
            BridgeErrorCode::InvalidArgument => Self::InvalidArgument,
            BridgeErrorCode::InvalidState => Self::InvalidState,
            BridgeErrorCode::InvalidSource => Self::InvalidSource,
            BridgeErrorCode::BackendFailure => Self::BackendFailure,
            BridgeErrorCode::AudioOutputUnavailable => Self::AudioOutputUnavailable,
            BridgeErrorCode::DecodeFailure => Self::DecodeFailure,
            BridgeErrorCode::SeekFailure => Self::SeekFailure,
            BridgeErrorCode::Unsupported => Self::Unsupported,
        }
    }
}

impl From<BridgeErrorCategory> for PlayerFfiErrorCategory {
    fn from(value: BridgeErrorCategory) -> Self {
        match value {
            BridgeErrorCategory::Input => Self::Input,
            BridgeErrorCategory::Source => Self::Source,
            BridgeErrorCategory::Network => Self::Network,
            BridgeErrorCategory::Decode => Self::Decode,
            BridgeErrorCategory::AudioOutput => Self::AudioOutput,
            BridgeErrorCategory::Playback => Self::Playback,
            BridgeErrorCategory::Capability => Self::Capability,
            BridgeErrorCategory::Platform => Self::Platform,
        }
    }
}

impl From<FfiVideoInfo> for PlayerFfiVideoInfo {
    fn from(value: FfiVideoInfo) -> Self {
        Self {
            codec: into_c_string_ptr(value.codec),
            width: value.width,
            height: value.height,
            has_frame_rate: value.frame_rate.is_some(),
            frame_rate: value.frame_rate.unwrap_or_default(),
        }
    }
}

impl From<FfiAudioInfo> for PlayerFfiAudioInfo {
    fn from(value: FfiAudioInfo) -> Self {
        Self {
            codec: into_c_string_ptr(value.codec),
            sample_rate: value.sample_rate,
            channels: value.channels,
        }
    }
}

impl From<BridgeTrack> for PlayerFfiTrack {
    fn from(value: BridgeTrack) -> Self {
        Self {
            id: into_c_string_ptr(value.id),
            kind: value.kind.into(),
            label: value
                .label
                .map(into_c_string_ptr)
                .unwrap_or(ptr::null_mut()),
            language: value
                .language
                .map(into_c_string_ptr)
                .unwrap_or(ptr::null_mut()),
            codec: value
                .codec
                .map(into_c_string_ptr)
                .unwrap_or(ptr::null_mut()),
            has_bit_rate: value.bit_rate.is_some(),
            bit_rate: value.bit_rate.unwrap_or_default(),
            has_width: value.width.is_some(),
            width: value.width.unwrap_or_default(),
            has_height: value.height.is_some(),
            height: value.height.unwrap_or_default(),
            has_frame_rate: value.frame_rate.is_some(),
            frame_rate: value.frame_rate.unwrap_or_default(),
            has_channels: value.channels.is_some(),
            channels: value.channels.unwrap_or_default(),
            has_sample_rate: value.sample_rate.is_some(),
            sample_rate: value.sample_rate.unwrap_or_default(),
            is_default: value.is_default,
            is_forced: value.is_forced,
        }
    }
}

impl From<BridgeTrackCatalog> for PlayerFfiTrackCatalog {
    fn from(value: BridgeTrackCatalog) -> Self {
        let tracks = value
            .tracks
            .into_iter()
            .map(PlayerFfiTrack::from)
            .collect::<Vec<_>>();
        let (tracks, len) = into_owned_struct_array(tracks);

        Self {
            tracks,
            len,
            adaptive_video: value.adaptive_video,
            adaptive_audio: value.adaptive_audio,
        }
    }
}

impl From<BridgeTrackSelection> for PlayerFfiTrackSelection {
    fn from(value: BridgeTrackSelection) -> Self {
        Self {
            mode: value.mode.into(),
            track_id: value
                .track_id
                .map(into_c_string_ptr)
                .unwrap_or(ptr::null_mut()),
        }
    }
}

impl From<BridgeAbrPolicy> for PlayerFfiAbrPolicy {
    fn from(value: BridgeAbrPolicy) -> Self {
        Self {
            mode: value.mode.into(),
            track_id: value
                .track_id
                .map(into_c_string_ptr)
                .unwrap_or(ptr::null_mut()),
            has_max_bit_rate: value.max_bit_rate.is_some(),
            max_bit_rate: value.max_bit_rate.unwrap_or_default(),
            has_max_width: value.max_width.is_some(),
            max_width: value.max_width.unwrap_or_default(),
            has_max_height: value.max_height.is_some(),
            max_height: value.max_height.unwrap_or_default(),
        }
    }
}

impl From<BridgeTrackSelectionSnapshot> for PlayerFfiTrackSelectionSnapshot {
    fn from(value: BridgeTrackSelectionSnapshot) -> Self {
        Self {
            video: value.video.into(),
            audio: value.audio.into(),
            subtitle: value.subtitle.into(),
            abr_policy: value.abr_policy.into(),
        }
    }
}

impl From<BridgeMediaInfo> for PlayerFfiMediaInfo {
    fn from(value: BridgeMediaInfo) -> Self {
        Self {
            source_uri: into_c_string_ptr(value.source_uri),
            source_kind: value.source_kind.into(),
            source_protocol: value.source_protocol.into(),
            has_duration: value.duration_ms.is_some(),
            duration_ms: value.duration_ms.unwrap_or_default(),
            has_bit_rate: value.bit_rate.is_some(),
            bit_rate: value.bit_rate.unwrap_or_default(),
            audio_streams: value.audio_streams,
            video_streams: value.video_streams,
            has_best_video: value.best_video.is_some(),
            best_video: value
                .best_video
                .map(PlayerFfiVideoInfo::from)
                .unwrap_or_default(),
            has_best_audio: value.best_audio.is_some(),
            best_audio: value
                .best_audio
                .map(PlayerFfiAudioInfo::from)
                .unwrap_or_default(),
            track_catalog: value.track_catalog.into(),
            track_selection: value.track_selection.into(),
        }
    }
}

impl From<FfiAudioOutputInfo> for PlayerFfiAudioOutputInfo {
    fn from(value: FfiAudioOutputInfo) -> Self {
        Self {
            device_name: value
                .device_name
                .map(into_c_string_ptr)
                .unwrap_or(ptr::null_mut()),
            has_channels: value.channels.is_some(),
            channels: value.channels.unwrap_or_default(),
            has_sample_rate: value.sample_rate.is_some(),
            sample_rate: value.sample_rate.unwrap_or_default(),
            sample_format: value
                .sample_format
                .map(into_c_string_ptr)
                .unwrap_or(ptr::null_mut()),
        }
    }
}

impl From<FfiDecodedAudioSummary> for PlayerFfiDecodedAudioSummary {
    fn from(value: FfiDecodedAudioSummary) -> Self {
        Self {
            channels: value.channels,
            sample_rate: value.sample_rate,
            duration_ms: value.duration_ms,
        }
    }
}

impl From<BridgeVideoDecodeMode> for PlayerFfiVideoDecodeMode {
    fn from(value: BridgeVideoDecodeMode) -> Self {
        match value {
            BridgeVideoDecodeMode::Software => Self::Software,
            BridgeVideoDecodeMode::Hardware => Self::Hardware,
        }
    }
}

impl From<BridgeVideoDecodeInfo> for PlayerFfiVideoDecodeInfo {
    fn from(value: BridgeVideoDecodeInfo) -> Self {
        Self {
            selected_mode: value.selected_mode.into(),
            hardware_available: value.hardware_available,
            hardware_backend: value
                .hardware_backend
                .map(into_c_string_ptr)
                .unwrap_or(ptr::null_mut()),
            fallback_reason: value
                .fallback_reason
                .map(into_c_string_ptr)
                .unwrap_or(ptr::null_mut()),
        }
    }
}

impl From<BridgeStartup> for PlayerFfiStartup {
    fn from(value: BridgeStartup) -> Self {
        Self {
            ffmpeg_initialized: value.ffmpeg_initialized,
            has_audio_output: value.audio_output.is_some(),
            audio_output: value
                .audio_output
                .map(PlayerFfiAudioOutputInfo::from)
                .unwrap_or_default(),
            has_decoded_audio: value.decoded_audio.is_some(),
            decoded_audio: value
                .decoded_audio
                .map(PlayerFfiDecodedAudioSummary::from)
                .unwrap_or_default(),
            has_video_decode: value.video_decode.is_some(),
            video_decode: value
                .video_decode
                .map(PlayerFfiVideoDecodeInfo::from)
                .unwrap_or_default(),
        }
    }
}

impl From<BridgeProgress> for PlayerFfiProgress {
    fn from(value: BridgeProgress) -> Self {
        Self {
            position_ms: value.position_ms,
            has_duration: value.duration_ms.is_some(),
            duration_ms: value.duration_ms.unwrap_or_default(),
            has_ratio: value.ratio.is_some(),
            ratio: value.ratio.unwrap_or_default(),
        }
    }
}

impl From<BridgeSeekableRange> for PlayerFfiSeekableRange {
    fn from(value: BridgeSeekableRange) -> Self {
        Self {
            start_ms: value.start_ms,
            end_ms: value.end_ms,
        }
    }
}

impl From<BridgeTimelineSnapshot> for PlayerFfiTimelineSnapshot {
    fn from(value: BridgeTimelineSnapshot) -> Self {
        Self {
            kind: value.kind.into(),
            is_seekable: value.is_seekable,
            has_seekable_range: value.seekable_range.is_some(),
            seekable_range: value
                .seekable_range
                .map(PlayerFfiSeekableRange::from)
                .unwrap_or_default(),
            has_live_edge: value.live_edge_ms.is_some(),
            live_edge_ms: value.live_edge_ms.unwrap_or_default(),
            position_ms: value.position_ms,
            has_duration: value.duration_ms.is_some(),
            duration_ms: value.duration_ms.unwrap_or_default(),
            has_ratio: value.ratio.is_some(),
            ratio: value.ratio.unwrap_or_default(),
        }
    }
}

impl From<BridgeSnapshot> for PlayerFfiSnapshot {
    fn from(value: BridgeSnapshot) -> Self {
        Self {
            source_uri: into_c_string_ptr(value.source_uri),
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

impl From<BridgeVideoFrame> for PlayerFfiVideoFrame {
    fn from(value: BridgeVideoFrame) -> Self {
        let (bytes, len) = into_owned_bytes(value.bytes);

        Self {
            presentation_time_ms: value.presentation_time_ms,
            width: value.width,
            height: value.height,
            bytes_per_row: value.bytes_per_row,
            pixel_format: value.pixel_format.into(),
            bytes,
            len,
        }
    }
}

impl From<FfiFirstFrameReady> for PlayerFfiFirstFrameReady {
    fn from(value: FfiFirstFrameReady) -> Self {
        Self {
            presentation_time_ms: value.presentation_time_ms,
            width: value.width,
            height: value.height,
        }
    }
}

impl From<BridgeEvent> for PlayerFfiEvent {
    fn from(value: BridgeEvent) -> Self {
        match value {
            BridgeEvent::Initialized(startup) => Self {
                kind: PlayerFfiEventKind::Initialized,
                initialized: startup.into(),
                ..Self::default()
            },
            BridgeEvent::MetadataReady(media_info) => Self {
                kind: PlayerFfiEventKind::MetadataReady,
                metadata_ready: media_info.into(),
                ..Self::default()
            },
            BridgeEvent::FirstFrameReady(frame) => Self {
                kind: PlayerFfiEventKind::FirstFrameReady,
                first_frame_ready: frame.into(),
                ..Self::default()
            },
            BridgeEvent::PlaybackStateChanged(state) => Self {
                kind: PlayerFfiEventKind::PlaybackStateChanged,
                playback_state: state.into(),
                ..Self::default()
            },
            BridgeEvent::InterruptionChanged { interrupted } => Self {
                kind: PlayerFfiEventKind::InterruptionChanged,
                interrupted,
                ..Self::default()
            },
            BridgeEvent::BufferingChanged { buffering } => Self {
                kind: PlayerFfiEventKind::BufferingChanged,
                buffering,
                ..Self::default()
            },
            BridgeEvent::VideoSurfaceChanged { attached } => Self {
                kind: PlayerFfiEventKind::VideoSurfaceChanged,
                surface_attached: attached,
                ..Self::default()
            },
            BridgeEvent::AudioOutputChanged(audio_output) => Self {
                kind: PlayerFfiEventKind::AudioOutputChanged,
                has_audio_output: audio_output.is_some(),
                audio_output: audio_output
                    .map(PlayerFfiAudioOutputInfo::from)
                    .unwrap_or_default(),
                ..Self::default()
            },
            BridgeEvent::PlaybackRateChanged { rate } => Self {
                kind: PlayerFfiEventKind::PlaybackRateChanged,
                playback_rate: rate,
                ..Self::default()
            },
            BridgeEvent::SeekCompleted { position_ms } => Self {
                kind: PlayerFfiEventKind::SeekCompleted,
                seek_position_ms: position_ms,
                ..Self::default()
            },
            BridgeEvent::RetryScheduled { attempt, delay_ms } => Self {
                kind: PlayerFfiEventKind::RetryScheduled,
                retry_attempt: attempt,
                retry_delay_ms: delay_ms,
                ..Self::default()
            },
            BridgeEvent::Error(error) => Self {
                kind: PlayerFfiEventKind::Error,
                error: owned_bridge_error(error),
                ..Self::default()
            },
            BridgeEvent::Ended => Self {
                kind: PlayerFfiEventKind::Ended,
                ..Self::default()
            },
        }
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn player_ffi_initializer_probe_uri(
    uri: *const c_char,
    out_initializer: *mut *mut PlayerFfiInitializerHandle,
    out_error: *mut PlayerFfiError,
) -> PlayerFfiCallStatus {
    if out_initializer.is_null() {
        write_error(
            out_error,
            owned_api_error(PlayerFfiErrorCode::NullPointer, "out_initializer was null"),
        );
        return PlayerFfiCallStatus::Error;
    }

    let uri = match read_uri(uri) {
        Ok(uri) => uri,
        Err(error) => {
            write_error(out_error, error);
            return PlayerFfiCallStatus::Error;
        }
    };

    match FfiPlayerInitializer::probe_uri(uri) {
        Ok(initializer) => {
            unsafe {
                ptr::write(out_initializer, into_initializer_handle(initializer));
            }
            PlayerFfiCallStatus::Ok
        }
        Err(error) => {
            write_error(out_error, owned_bridge_error(error));
            PlayerFfiCallStatus::Error
        }
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn player_ffi_initializer_destroy(handle: *mut PlayerFfiInitializerHandle) {
    if handle.is_null() {
        return;
    }

    unsafe {
        drop(Box::from_raw(handle as *mut FfiPlayerInitializer));
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn player_ffi_initializer_media_info(
    handle: *const PlayerFfiInitializerHandle,
    out_media_info: *mut PlayerFfiMediaInfo,
    out_error: *mut PlayerFfiError,
) -> PlayerFfiCallStatus {
    if out_media_info.is_null() {
        write_error(
            out_error,
            owned_api_error(PlayerFfiErrorCode::NullPointer, "out_media_info was null"),
        );
        return PlayerFfiCallStatus::Error;
    }

    let Some(initializer) = initializer_ref(handle) else {
        write_error(
            out_error,
            owned_api_error(
                PlayerFfiErrorCode::NullPointer,
                "initializer handle was null",
            ),
        );
        return PlayerFfiCallStatus::Error;
    };

    unsafe {
        ptr::write(out_media_info, initializer.media_info().into());
    }
    PlayerFfiCallStatus::Ok
}

#[unsafe(no_mangle)]
pub extern "C" fn player_ffi_initializer_startup(
    handle: *const PlayerFfiInitializerHandle,
    out_startup: *mut PlayerFfiStartup,
    out_error: *mut PlayerFfiError,
) -> PlayerFfiCallStatus {
    if out_startup.is_null() {
        write_error(
            out_error,
            owned_api_error(PlayerFfiErrorCode::NullPointer, "out_startup was null"),
        );
        return PlayerFfiCallStatus::Error;
    }

    let Some(initializer) = initializer_ref(handle) else {
        write_error(
            out_error,
            owned_api_error(
                PlayerFfiErrorCode::NullPointer,
                "initializer handle was null",
            ),
        );
        return PlayerFfiCallStatus::Error;
    };

    unsafe {
        ptr::write(out_startup, initializer.startup().into());
    }
    PlayerFfiCallStatus::Ok
}

#[unsafe(no_mangle)]
pub extern "C" fn player_ffi_initializer_initialize(
    handle: *mut PlayerFfiInitializerHandle,
    out_player: *mut *mut PlayerFfiHandle,
    out_has_initial_frame: *mut bool,
    out_initial_frame: *mut PlayerFfiVideoFrame,
    out_startup: *mut PlayerFfiStartup,
    out_error: *mut PlayerFfiError,
) -> PlayerFfiCallStatus {
    if out_player.is_null()
        || out_has_initial_frame.is_null()
        || out_initial_frame.is_null()
        || out_startup.is_null()
    {
        write_error(
            out_error,
            owned_api_error(
                PlayerFfiErrorCode::NullPointer,
                "one or more initialize output pointers were null",
            ),
        );
        return PlayerFfiCallStatus::Error;
    }

    let Some(initializer) = take_initializer(handle) else {
        write_error(
            out_error,
            owned_api_error(
                PlayerFfiErrorCode::NullPointer,
                "initializer handle was null",
            ),
        );
        return PlayerFfiCallStatus::Error;
    };

    match initializer.initialize() {
        Ok(bootstrap) => {
            let has_initial_frame = bootstrap.initial_frame.is_some();
            let initial_frame = bootstrap
                .initial_frame
                .map(PlayerFfiVideoFrame::from)
                .unwrap_or_default();
            unsafe {
                ptr::write(out_player, into_player_handle(bootstrap.player));
                ptr::write(out_has_initial_frame, has_initial_frame);
                ptr::write(out_initial_frame, initial_frame);
                ptr::write(out_startup, bootstrap.startup.into());
            }
            PlayerFfiCallStatus::Ok
        }
        Err(error) => {
            write_error(out_error, owned_bridge_error(error));
            PlayerFfiCallStatus::Error
        }
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn player_ffi_player_destroy(handle: *mut PlayerFfiHandle) {
    if handle.is_null() {
        return;
    }

    unsafe {
        drop(Box::from_raw(handle as *mut FfiPlayer));
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn player_ffi_player_snapshot(
    handle: *const PlayerFfiHandle,
    out_snapshot: *mut PlayerFfiSnapshot,
    out_error: *mut PlayerFfiError,
) -> PlayerFfiCallStatus {
    if out_snapshot.is_null() {
        write_error(
            out_error,
            owned_api_error(PlayerFfiErrorCode::NullPointer, "out_snapshot was null"),
        );
        return PlayerFfiCallStatus::Error;
    }

    let Some(player) = player_ref(handle) else {
        write_error(
            out_error,
            owned_api_error(PlayerFfiErrorCode::NullPointer, "player handle was null"),
        );
        return PlayerFfiCallStatus::Error;
    };

    unsafe {
        ptr::write(out_snapshot, player.snapshot().into());
    }
    PlayerFfiCallStatus::Ok
}

#[unsafe(no_mangle)]
pub extern "C" fn player_ffi_player_dispatch(
    handle: *mut PlayerFfiHandle,
    command: PlayerFfiCommandKind,
    position_ms: u64,
    out_applied: *mut bool,
    out_frame: *mut PlayerFfiVideoFrame,
    out_snapshot: *mut PlayerFfiSnapshot,
    out_error: *mut PlayerFfiError,
) -> PlayerFfiCallStatus {
    if out_applied.is_null() || out_snapshot.is_null() {
        write_error(
            out_error,
            owned_api_error(
                PlayerFfiErrorCode::NullPointer,
                "out_applied or out_snapshot was null",
            ),
        );
        return PlayerFfiCallStatus::Error;
    }

    let Some(player) = player_mut(handle) else {
        write_error(
            out_error,
            owned_api_error(PlayerFfiErrorCode::NullPointer, "player handle was null"),
        );
        return PlayerFfiCallStatus::Error;
    };

    match player.dispatch(to_bridge_command(command, position_ms)) {
        Ok(result) => {
            unsafe {
                ptr::write(out_applied, result.applied);
                ptr::write(out_snapshot, result.snapshot.into());
                if !out_frame.is_null() {
                    let frame = result
                        .frame
                        .map(PlayerFfiVideoFrame::from)
                        .unwrap_or_default();
                    ptr::write(out_frame, frame);
                }
            }
            PlayerFfiCallStatus::Ok
        }
        Err(error) => {
            write_error(out_error, owned_bridge_error(error));
            PlayerFfiCallStatus::Error
        }
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn player_ffi_player_set_playback_rate(
    handle: *mut PlayerFfiHandle,
    playback_rate: f32,
    out_applied: *mut bool,
    out_snapshot: *mut PlayerFfiSnapshot,
    out_error: *mut PlayerFfiError,
) -> PlayerFfiCallStatus {
    if out_applied.is_null() || out_snapshot.is_null() {
        write_error(
            out_error,
            owned_api_error(
                PlayerFfiErrorCode::NullPointer,
                "out_applied or out_snapshot was null",
            ),
        );
        return PlayerFfiCallStatus::Error;
    }

    let Some(player) = player_mut(handle) else {
        write_error(
            out_error,
            owned_api_error(PlayerFfiErrorCode::NullPointer, "player handle was null"),
        );
        return PlayerFfiCallStatus::Error;
    };

    match player.set_playback_rate(playback_rate) {
        Ok(result) => {
            unsafe {
                ptr::write(out_applied, result.applied);
                ptr::write(out_snapshot, result.snapshot.into());
            }
            PlayerFfiCallStatus::Ok
        }
        Err(error) => {
            write_error(out_error, owned_bridge_error(error));
            PlayerFfiCallStatus::Error
        }
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn player_ffi_player_set_video_track_selection(
    handle: *mut PlayerFfiHandle,
    selection: *const PlayerFfiTrackSelection,
    out_applied: *mut bool,
    out_snapshot: *mut PlayerFfiSnapshot,
    out_error: *mut PlayerFfiError,
) -> PlayerFfiCallStatus {
    if out_applied.is_null() || out_snapshot.is_null() {
        write_error(
            out_error,
            owned_api_error(
                PlayerFfiErrorCode::NullPointer,
                "out_applied or out_snapshot was null",
            ),
        );
        return PlayerFfiCallStatus::Error;
    }

    let Some(player) = player_mut(handle) else {
        write_error(
            out_error,
            owned_api_error(PlayerFfiErrorCode::NullPointer, "player handle was null"),
        );
        return PlayerFfiCallStatus::Error;
    };

    let selection = match read_track_selection(selection) {
        Ok(selection) => selection,
        Err(error) => {
            write_error(out_error, error);
            return PlayerFfiCallStatus::Error;
        }
    };

    match player.set_video_track_selection(selection) {
        Ok(result) => {
            unsafe {
                ptr::write(out_applied, result.applied);
                ptr::write(out_snapshot, result.snapshot.into());
            }
            PlayerFfiCallStatus::Ok
        }
        Err(error) => {
            write_error(out_error, owned_bridge_error(error));
            PlayerFfiCallStatus::Error
        }
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn player_ffi_player_set_audio_track_selection(
    handle: *mut PlayerFfiHandle,
    selection: *const PlayerFfiTrackSelection,
    out_applied: *mut bool,
    out_snapshot: *mut PlayerFfiSnapshot,
    out_error: *mut PlayerFfiError,
) -> PlayerFfiCallStatus {
    if out_applied.is_null() || out_snapshot.is_null() {
        write_error(
            out_error,
            owned_api_error(
                PlayerFfiErrorCode::NullPointer,
                "out_applied or out_snapshot was null",
            ),
        );
        return PlayerFfiCallStatus::Error;
    }

    let Some(player) = player_mut(handle) else {
        write_error(
            out_error,
            owned_api_error(PlayerFfiErrorCode::NullPointer, "player handle was null"),
        );
        return PlayerFfiCallStatus::Error;
    };

    let selection = match read_track_selection(selection) {
        Ok(selection) => selection,
        Err(error) => {
            write_error(out_error, error);
            return PlayerFfiCallStatus::Error;
        }
    };

    match player.set_audio_track_selection(selection) {
        Ok(result) => {
            unsafe {
                ptr::write(out_applied, result.applied);
                ptr::write(out_snapshot, result.snapshot.into());
            }
            PlayerFfiCallStatus::Ok
        }
        Err(error) => {
            write_error(out_error, owned_bridge_error(error));
            PlayerFfiCallStatus::Error
        }
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn player_ffi_player_set_subtitle_track_selection(
    handle: *mut PlayerFfiHandle,
    selection: *const PlayerFfiTrackSelection,
    out_applied: *mut bool,
    out_snapshot: *mut PlayerFfiSnapshot,
    out_error: *mut PlayerFfiError,
) -> PlayerFfiCallStatus {
    if out_applied.is_null() || out_snapshot.is_null() {
        write_error(
            out_error,
            owned_api_error(
                PlayerFfiErrorCode::NullPointer,
                "out_applied or out_snapshot was null",
            ),
        );
        return PlayerFfiCallStatus::Error;
    }

    let Some(player) = player_mut(handle) else {
        write_error(
            out_error,
            owned_api_error(PlayerFfiErrorCode::NullPointer, "player handle was null"),
        );
        return PlayerFfiCallStatus::Error;
    };

    let selection = match read_track_selection(selection) {
        Ok(selection) => selection,
        Err(error) => {
            write_error(out_error, error);
            return PlayerFfiCallStatus::Error;
        }
    };

    match player.set_subtitle_track_selection(selection) {
        Ok(result) => {
            unsafe {
                ptr::write(out_applied, result.applied);
                ptr::write(out_snapshot, result.snapshot.into());
            }
            PlayerFfiCallStatus::Ok
        }
        Err(error) => {
            write_error(out_error, owned_bridge_error(error));
            PlayerFfiCallStatus::Error
        }
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn player_ffi_player_set_abr_policy(
    handle: *mut PlayerFfiHandle,
    policy: *const PlayerFfiAbrPolicy,
    out_applied: *mut bool,
    out_snapshot: *mut PlayerFfiSnapshot,
    out_error: *mut PlayerFfiError,
) -> PlayerFfiCallStatus {
    if out_applied.is_null() || out_snapshot.is_null() {
        write_error(
            out_error,
            owned_api_error(
                PlayerFfiErrorCode::NullPointer,
                "out_applied or out_snapshot was null",
            ),
        );
        return PlayerFfiCallStatus::Error;
    }

    let Some(player) = player_mut(handle) else {
        write_error(
            out_error,
            owned_api_error(PlayerFfiErrorCode::NullPointer, "player handle was null"),
        );
        return PlayerFfiCallStatus::Error;
    };

    let policy = match read_abr_policy(policy) {
        Ok(policy) => policy,
        Err(error) => {
            write_error(out_error, error);
            return PlayerFfiCallStatus::Error;
        }
    };

    match player.set_abr_policy(policy) {
        Ok(result) => {
            unsafe {
                ptr::write(out_applied, result.applied);
                ptr::write(out_snapshot, result.snapshot.into());
            }
            PlayerFfiCallStatus::Ok
        }
        Err(error) => {
            write_error(out_error, owned_bridge_error(error));
            PlayerFfiCallStatus::Error
        }
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn player_ffi_player_drain_events(
    handle: *mut PlayerFfiHandle,
    out_events: *mut PlayerFfiEventList,
    out_error: *mut PlayerFfiError,
) -> PlayerFfiCallStatus {
    if out_events.is_null() {
        write_error(
            out_error,
            owned_api_error(PlayerFfiErrorCode::NullPointer, "out_events was null"),
        );
        return PlayerFfiCallStatus::Error;
    }

    let Some(player) = player_mut(handle) else {
        write_error(
            out_error,
            owned_api_error(PlayerFfiErrorCode::NullPointer, "player handle was null"),
        );
        return PlayerFfiCallStatus::Error;
    };

    let events = player
        .drain_events()
        .into_iter()
        .map(PlayerFfiEvent::from)
        .collect::<Vec<_>>();
    let (ptr, len) = into_owned_struct_array(events);

    unsafe {
        ptr::write(out_events, PlayerFfiEventList { ptr, len });
    }
    PlayerFfiCallStatus::Ok
}

#[unsafe(no_mangle)]
pub extern "C" fn player_ffi_player_advance(
    handle: *mut PlayerFfiHandle,
    out_frame: *mut PlayerFfiVideoFrame,
    out_has_frame: *mut bool,
    out_error: *mut PlayerFfiError,
) -> PlayerFfiCallStatus {
    if out_frame.is_null() || out_has_frame.is_null() {
        write_error(
            out_error,
            owned_api_error(
                PlayerFfiErrorCode::NullPointer,
                "out_frame or out_has_frame was null",
            ),
        );
        return PlayerFfiCallStatus::Error;
    }

    let Some(player) = player_mut(handle) else {
        write_error(
            out_error,
            owned_api_error(PlayerFfiErrorCode::NullPointer, "player handle was null"),
        );
        return PlayerFfiCallStatus::Error;
    };

    match player.advance() {
        Ok(frame) => {
            unsafe {
                ptr::write(out_has_frame, frame.is_some());
                ptr::write(
                    out_frame,
                    frame.map(PlayerFfiVideoFrame::from).unwrap_or_default(),
                );
            }
            PlayerFfiCallStatus::Ok
        }
        Err(error) => {
            write_error(out_error, owned_bridge_error(error));
            PlayerFfiCallStatus::Error
        }
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn player_ffi_player_next_deadline_delay_ms(
    handle: *const PlayerFfiHandle,
    out_has_deadline: *mut bool,
    out_delay_ms: *mut u64,
    out_error: *mut PlayerFfiError,
) -> PlayerFfiCallStatus {
    if out_has_deadline.is_null() || out_delay_ms.is_null() {
        write_error(
            out_error,
            owned_api_error(
                PlayerFfiErrorCode::NullPointer,
                "out_has_deadline or out_delay_ms was null",
            ),
        );
        return PlayerFfiCallStatus::Error;
    }

    let Some(player) = player_ref(handle) else {
        write_error(
            out_error,
            owned_api_error(PlayerFfiErrorCode::NullPointer, "player handle was null"),
        );
        return PlayerFfiCallStatus::Error;
    };

    let deadline = player.next_deadline_delay_ms();
    unsafe {
        ptr::write(out_has_deadline, deadline.is_some());
        ptr::write(out_delay_ms, deadline.unwrap_or_default());
    }
    PlayerFfiCallStatus::Ok
}

#[unsafe(no_mangle)]
pub extern "C" fn player_ffi_error_free(error: *mut PlayerFfiError) {
    let Some(error) = error_mut(error) else {
        return;
    };

    free_c_string(&mut error.message);
    *error = PlayerFfiError::default();
}

#[unsafe(no_mangle)]
pub extern "C" fn player_ffi_media_info_free(media_info: *mut PlayerFfiMediaInfo) {
    let Some(media_info) = media_info_mut(media_info) else {
        return;
    };

    free_media_info(media_info);
}

#[unsafe(no_mangle)]
pub extern "C" fn player_ffi_startup_free(startup: *mut PlayerFfiStartup) {
    let Some(startup) = startup_mut(startup) else {
        return;
    };

    free_startup(startup);
}

#[unsafe(no_mangle)]
pub extern "C" fn player_ffi_snapshot_free(snapshot: *mut PlayerFfiSnapshot) {
    let Some(snapshot) = snapshot_mut(snapshot) else {
        return;
    };

    free_snapshot(snapshot);
}

#[unsafe(no_mangle)]
pub extern "C" fn player_ffi_video_frame_free(frame: *mut PlayerFfiVideoFrame) {
    let Some(frame) = video_frame_mut(frame) else {
        return;
    };

    free_video_frame(frame);
}

#[unsafe(no_mangle)]
pub extern "C" fn player_ffi_event_list_free(events: *mut PlayerFfiEventList) {
    let Some(events) = event_list_mut(events) else {
        return;
    };

    if !events.ptr.is_null() {
        unsafe {
            let mut boxed = Box::from_raw(ptr::slice_from_raw_parts_mut(events.ptr, events.len));
            for event in boxed.iter_mut() {
                free_event(event);
            }
        }
    }

    *events = PlayerFfiEventList::default();
}

fn to_bridge_command(command: PlayerFfiCommandKind, position_ms: u64) -> FfiCommand {
    match command {
        PlayerFfiCommandKind::Play => FfiCommand::Play,
        PlayerFfiCommandKind::Pause => FfiCommand::Pause,
        PlayerFfiCommandKind::TogglePause => FfiCommand::TogglePause,
        PlayerFfiCommandKind::SeekTo => FfiCommand::SeekTo { position_ms },
        PlayerFfiCommandKind::Stop => FfiCommand::Stop,
    }
}

fn read_optional_c_string(
    value: *const c_char,
    field_name: &str,
) -> Result<Option<String>, PlayerFfiError> {
    if value.is_null() {
        return Ok(None);
    }

    let text = unsafe { CStr::from_ptr(value) };
    let text = text.to_str().map_err(|_| {
        owned_api_error(
            PlayerFfiErrorCode::InvalidUtf8,
            &format!("{field_name} was not valid UTF-8"),
        )
    })?;
    Ok(Some(text.to_owned()))
}

fn read_track_selection(
    selection: *const PlayerFfiTrackSelection,
) -> Result<BridgeTrackSelection, PlayerFfiError> {
    let Some(selection) = (unsafe { selection.as_ref() }) else {
        return Err(owned_api_error(
            PlayerFfiErrorCode::NullPointer,
            "selection pointer was null",
        ));
    };

    Ok(BridgeTrackSelection {
        mode: match selection.mode {
            PlayerFfiTrackSelectionMode::Auto => BridgeTrackSelectionMode::Auto,
            PlayerFfiTrackSelectionMode::Disabled => BridgeTrackSelectionMode::Disabled,
            PlayerFfiTrackSelectionMode::Track => BridgeTrackSelectionMode::Track,
        },
        track_id: read_optional_c_string(selection.track_id, "selection.track_id")?,
    })
}

fn read_abr_policy(policy: *const PlayerFfiAbrPolicy) -> Result<BridgeAbrPolicy, PlayerFfiError> {
    let Some(policy) = (unsafe { policy.as_ref() }) else {
        return Err(owned_api_error(
            PlayerFfiErrorCode::NullPointer,
            "policy pointer was null",
        ));
    };

    Ok(BridgeAbrPolicy {
        mode: match policy.mode {
            PlayerFfiAbrMode::Auto => BridgeAbrMode::Auto,
            PlayerFfiAbrMode::Constrained => BridgeAbrMode::Constrained,
            PlayerFfiAbrMode::FixedTrack => BridgeAbrMode::FixedTrack,
        },
        track_id: read_optional_c_string(policy.track_id, "policy.track_id")?,
        max_bit_rate: policy.has_max_bit_rate.then_some(policy.max_bit_rate),
        max_width: policy.has_max_width.then_some(policy.max_width),
        max_height: policy.has_max_height.then_some(policy.max_height),
    })
}

fn read_uri(uri: *const c_char) -> Result<String, PlayerFfiError> {
    if uri.is_null() {
        return Err(owned_api_error(
            PlayerFfiErrorCode::NullPointer,
            "uri pointer was null",
        ));
    }

    let uri = unsafe { CStr::from_ptr(uri) };
    let uri = uri
        .to_str()
        .map_err(|_| owned_api_error(PlayerFfiErrorCode::InvalidUtf8, "uri was not valid UTF-8"))?;

    Ok(uri.to_owned())
}

fn owned_bridge_error(error: BridgeError) -> PlayerFfiError {
    PlayerFfiError {
        code: error.code().into(),
        category: error.category().into(),
        retriable: error.is_retriable(),
        message: into_c_string_ptr(error.message().to_owned()),
    }
}

fn owned_api_error(code: PlayerFfiErrorCode, message: &str) -> PlayerFfiError {
    PlayerFfiError {
        code,
        category: api_error_category(code),
        retriable: false,
        message: into_c_string_ptr(message.to_owned()),
    }
}

fn api_error_category(code: PlayerFfiErrorCode) -> PlayerFfiErrorCategory {
    match code {
        PlayerFfiErrorCode::NullPointer
        | PlayerFfiErrorCode::InvalidUtf8
        | PlayerFfiErrorCode::InvalidArgument => PlayerFfiErrorCategory::Input,
        PlayerFfiErrorCode::InvalidState | PlayerFfiErrorCode::SeekFailure => {
            PlayerFfiErrorCategory::Playback
        }
        PlayerFfiErrorCode::InvalidSource => PlayerFfiErrorCategory::Source,
        PlayerFfiErrorCode::AudioOutputUnavailable => PlayerFfiErrorCategory::AudioOutput,
        PlayerFfiErrorCode::DecodeFailure => PlayerFfiErrorCategory::Decode,
        PlayerFfiErrorCode::Unsupported => PlayerFfiErrorCategory::Capability,
        PlayerFfiErrorCode::BackendFailure | PlayerFfiErrorCode::None => {
            PlayerFfiErrorCategory::Platform
        }
    }
}

fn into_c_string_ptr(text: String) -> *mut c_char {
    let sanitized = text.replace('\0', " ");
    CString::new(sanitized)
        .expect("sanitized CString")
        .into_raw()
}

fn into_owned_bytes(bytes: Vec<u8>) -> (*mut u8, usize) {
    if bytes.is_empty() {
        return (ptr::null_mut(), 0);
    }

    let mut boxed = bytes.into_boxed_slice();
    let len = boxed.len();
    let ptr = boxed.as_mut_ptr();
    mem::forget(boxed);
    (ptr, len)
}

fn into_owned_struct_array<T>(values: Vec<T>) -> (*mut T, usize) {
    if values.is_empty() {
        return (ptr::null_mut(), 0);
    }

    let mut boxed = values.into_boxed_slice();
    let len = boxed.len();
    let ptr = boxed.as_mut_ptr();
    mem::forget(boxed);
    (ptr, len)
}

fn free_c_string(ptr_ref: &mut *mut c_char) {
    if !ptr_ref.is_null() && !(*ptr_ref).is_null() {
        unsafe {
            drop(CString::from_raw(*ptr_ref));
        }
    }
    *ptr_ref = ptr::null_mut();
}

fn free_video_info(video: &mut PlayerFfiVideoInfo) {
    free_c_string(&mut video.codec);
    *video = PlayerFfiVideoInfo::default();
}

fn free_audio_info(audio: &mut PlayerFfiAudioInfo) {
    free_c_string(&mut audio.codec);
    *audio = PlayerFfiAudioInfo::default();
}

fn free_track(track: &mut PlayerFfiTrack) {
    free_c_string(&mut track.id);
    free_c_string(&mut track.label);
    free_c_string(&mut track.language);
    free_c_string(&mut track.codec);
    *track = PlayerFfiTrack::default();
}

fn free_track_catalog(track_catalog: &mut PlayerFfiTrackCatalog) {
    if !track_catalog.tracks.is_null() {
        unsafe {
            let mut boxed = Box::from_raw(ptr::slice_from_raw_parts_mut(
                track_catalog.tracks,
                track_catalog.len,
            ));
            for track in boxed.iter_mut() {
                free_track(track);
            }
        }
    }
    *track_catalog = PlayerFfiTrackCatalog::default();
}

fn free_track_selection(track_selection: &mut PlayerFfiTrackSelection) {
    free_c_string(&mut track_selection.track_id);
    *track_selection = PlayerFfiTrackSelection::default();
}

fn free_abr_policy(abr_policy: &mut PlayerFfiAbrPolicy) {
    free_c_string(&mut abr_policy.track_id);
    *abr_policy = PlayerFfiAbrPolicy::default();
}

fn free_track_selection_snapshot(track_selection: &mut PlayerFfiTrackSelectionSnapshot) {
    free_track_selection(&mut track_selection.video);
    free_track_selection(&mut track_selection.audio);
    free_track_selection(&mut track_selection.subtitle);
    free_abr_policy(&mut track_selection.abr_policy);
    *track_selection = PlayerFfiTrackSelectionSnapshot::default();
}

fn free_media_info(media_info: &mut PlayerFfiMediaInfo) {
    free_c_string(&mut media_info.source_uri);
    free_video_info(&mut media_info.best_video);
    free_audio_info(&mut media_info.best_audio);
    free_track_catalog(&mut media_info.track_catalog);
    free_track_selection_snapshot(&mut media_info.track_selection);
    *media_info = PlayerFfiMediaInfo::default();
}

fn free_audio_output(audio_output: &mut PlayerFfiAudioOutputInfo) {
    free_c_string(&mut audio_output.device_name);
    free_c_string(&mut audio_output.sample_format);
    *audio_output = PlayerFfiAudioOutputInfo::default();
}

fn free_video_decode(video_decode: &mut PlayerFfiVideoDecodeInfo) {
    free_c_string(&mut video_decode.hardware_backend);
    free_c_string(&mut video_decode.fallback_reason);
    *video_decode = PlayerFfiVideoDecodeInfo::default();
}

fn free_startup(startup: &mut PlayerFfiStartup) {
    free_audio_output(&mut startup.audio_output);
    free_video_decode(&mut startup.video_decode);
    *startup = PlayerFfiStartup::default();
}

fn free_snapshot(snapshot: &mut PlayerFfiSnapshot) {
    free_c_string(&mut snapshot.source_uri);
    free_media_info(&mut snapshot.media_info);
    *snapshot = PlayerFfiSnapshot::default();
}

fn free_video_frame(frame: &mut PlayerFfiVideoFrame) {
    if !frame.bytes.is_null() {
        unsafe {
            drop(Box::from_raw(ptr::slice_from_raw_parts_mut(
                frame.bytes,
                frame.len,
            )));
        }
    }
    *frame = PlayerFfiVideoFrame::default();
}

fn free_event(event: &mut PlayerFfiEvent) {
    free_startup(&mut event.initialized);
    free_media_info(&mut event.metadata_ready);
    free_audio_output(&mut event.audio_output);
    player_ffi_error_free(&mut event.error);
    *event = PlayerFfiEvent::default();
}

fn write_error(out_error: *mut PlayerFfiError, error: PlayerFfiError) {
    if out_error.is_null() {
        return;
    }

    unsafe {
        ptr::write(out_error, error);
    }
}

fn into_initializer_handle(initializer: FfiPlayerInitializer) -> *mut PlayerFfiInitializerHandle {
    Box::into_raw(Box::new(initializer)) as *mut PlayerFfiInitializerHandle
}

fn into_player_handle(player: FfiPlayer) -> *mut PlayerFfiHandle {
    Box::into_raw(Box::new(player)) as *mut PlayerFfiHandle
}

fn initializer_ref(
    handle: *const PlayerFfiInitializerHandle,
) -> Option<&'static FfiPlayerInitializer> {
    if handle.is_null() {
        return None;
    }

    unsafe { Some(&*(handle as *const FfiPlayerInitializer)) }
}

fn take_initializer(handle: *mut PlayerFfiInitializerHandle) -> Option<FfiPlayerInitializer> {
    if handle.is_null() {
        return None;
    }

    unsafe { Some(*Box::from_raw(handle as *mut FfiPlayerInitializer)) }
}

fn player_ref(handle: *const PlayerFfiHandle) -> Option<&'static FfiPlayer> {
    if handle.is_null() {
        return None;
    }

    unsafe { Some(&*(handle as *const FfiPlayer)) }
}

fn player_mut(handle: *mut PlayerFfiHandle) -> Option<&'static mut FfiPlayer> {
    if handle.is_null() {
        return None;
    }

    unsafe { Some(&mut *(handle as *mut FfiPlayer)) }
}

fn error_mut(error: *mut PlayerFfiError) -> Option<&'static mut PlayerFfiError> {
    if error.is_null() {
        return None;
    }

    unsafe { Some(&mut *error) }
}

fn media_info_mut(media_info: *mut PlayerFfiMediaInfo) -> Option<&'static mut PlayerFfiMediaInfo> {
    if media_info.is_null() {
        return None;
    }

    unsafe { Some(&mut *media_info) }
}

fn startup_mut(startup: *mut PlayerFfiStartup) -> Option<&'static mut PlayerFfiStartup> {
    if startup.is_null() {
        return None;
    }

    unsafe { Some(&mut *startup) }
}

fn snapshot_mut(snapshot: *mut PlayerFfiSnapshot) -> Option<&'static mut PlayerFfiSnapshot> {
    if snapshot.is_null() {
        return None;
    }

    unsafe { Some(&mut *snapshot) }
}

fn video_frame_mut(frame: *mut PlayerFfiVideoFrame) -> Option<&'static mut PlayerFfiVideoFrame> {
    if frame.is_null() {
        return None;
    }

    unsafe { Some(&mut *frame) }
}

fn event_list_mut(events: *mut PlayerFfiEventList) -> Option<&'static mut PlayerFfiEventList> {
    if events.is_null() {
        return None;
    }

    unsafe { Some(&mut *events) }
}
