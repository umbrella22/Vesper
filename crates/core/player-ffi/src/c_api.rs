use std::any::Any;
use std::ffi::{CStr, CString, c_char};
use std::mem;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::ptr;
use std::sync::{Mutex, OnceLock};

use crate::{
    FfiAbrMode as BridgeAbrMode, FfiAbrPolicy as BridgeAbrPolicy, FfiAudioInfo, FfiAudioOutputInfo,
    FfiBufferingPolicy as BridgeBufferingPolicy, FfiBufferingPreset as BridgeBufferingPreset,
    FfiCachePolicy as BridgeCachePolicy, FfiCachePreset as BridgeCachePreset, FfiCommand,
    FfiDecodedAudioSummary, FfiError as BridgeError, FfiErrorCategory as BridgeErrorCategory,
    FfiErrorCode as BridgeErrorCode, FfiEvent as BridgeEvent, FfiFirstFrameReady,
    FfiMediaInfo as BridgeMediaInfo, FfiMediaSourceKind as BridgeMediaSourceKind,
    FfiMediaSourceProtocol as BridgeMediaSourceProtocol, FfiPixelFormat as BridgePixelFormat,
    FfiPlaybackState, FfiPlayer, FfiPlayerInitializer,
    FfiPreloadBudgetPolicy as BridgePreloadBudgetPolicy, FfiProgress as BridgeProgress,
    FfiResolvedPreloadBudgetPolicy as BridgeResolvedPreloadBudgetPolicy,
    FfiResolvedResiliencePolicy as BridgeResolvedResiliencePolicy,
    FfiRetryBackoff as BridgeRetryBackoff, FfiRetryPolicy as BridgeRetryPolicy,
    FfiSeekableRange as BridgeSeekableRange, FfiSnapshot as BridgeSnapshot,
    FfiStartup as BridgeStartup, FfiTimelineKind as BridgeTimelineKind,
    FfiTimelineSnapshot as BridgeTimelineSnapshot, FfiTrack as BridgeTrack,
    FfiTrackCatalog as BridgeTrackCatalog, FfiTrackKind as BridgeTrackKind,
    FfiTrackPreferences as BridgeTrackPreferences, FfiTrackSelection as BridgeTrackSelection,
    FfiTrackSelectionMode as BridgeTrackSelectionMode,
    FfiTrackSelectionSnapshot as BridgeTrackSelectionSnapshot,
    FfiVideoDecodeInfo as BridgeVideoDecodeInfo, FfiVideoDecodeMode as BridgeVideoDecodeMode,
    FfiVideoFrame as BridgeVideoFrame, FfiVideoInfo, resolve_preload_budget,
    resolve_resilience_policy, resolve_track_preferences,
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
pub enum PlayerFfiBufferingPreset {
    #[default]
    Default = 0,
    Balanced = 1,
    Streaming = 2,
    Resilient = 3,
    LowLatency = 4,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PlayerFfiRetryBackoff {
    Fixed = 0,
    #[default]
    Linear = 1,
    Exponential = 2,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PlayerFfiCachePreset {
    #[default]
    Default = 0,
    Disabled = 1,
    Streaming = 2,
    Resilient = 3,
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

/// Generation-checked initializer handle returned by `player_ffi_initializer_probe_uri`.
///
/// Handles are not thread-safe. The caller must serialize all `player_ffi_*`
/// calls that share the same handle. Concurrent calls on the same handle from
/// different threads are undefined behavior.
///
/// `raw == 0` is always invalid and may be used for zero-initialized storage.
/// Reusing a stale handle after `player_ffi_initializer_initialize` or
/// `player_ffi_initializer_destroy` returns `PlayerFfiCallStatus::Error` with
/// `PlayerFfiErrorCode::InvalidState`.
#[repr(C)]
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct PlayerFfiInitializerHandle {
    pub raw: u64,
}

/// Generation-checked player handle returned by `player_ffi_initializer_initialize`.
///
/// Handles are not thread-safe. The caller must serialize all `player_ffi_*`
/// calls that share the same handle. Concurrent calls on the same handle from
/// different threads are undefined behavior.
///
/// `raw == 0` is always invalid and may be used for zero-initialized storage.
/// Reusing a stale handle after `player_ffi_player_destroy` returns
/// `PlayerFfiCallStatus::Error` with `PlayerFfiErrorCode::InvalidState`.
#[repr(C)]
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct PlayerFfiHandle {
    pub raw: u64,
}

impl PlayerFfiInitializerHandle {
    fn is_invalid(self) -> bool {
        self.raw == 0
    }
}

impl PlayerFfiHandle {
    fn is_invalid(self) -> bool {
        self.raw == 0
    }
}

const _: [(); std::mem::size_of::<u64>()] = [(); std::mem::size_of::<PlayerFfiInitializerHandle>()];
const _: [(); std::mem::size_of::<u64>()] = [(); std::mem::size_of::<PlayerFfiHandle>()];

#[repr(C)]
#[derive(Debug, Default)]
/// Error payload written by status-returning `player_ffi_*` calls.
///
/// When a call returns `PlayerFfiCallStatus::Error`, the caller owns the
/// `message` buffer and must release it with `player_ffi_error_free` before
/// reusing the same storage for another error result.
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
pub struct PlayerFfiBufferingPolicy {
    pub preset: PlayerFfiBufferingPreset,
    pub has_min_buffer_ms: bool,
    pub min_buffer_ms: u64,
    pub has_max_buffer_ms: bool,
    pub max_buffer_ms: u64,
    pub has_buffer_for_playback_ms: bool,
    pub buffer_for_playback_ms: u64,
    pub has_buffer_for_rebuffer_ms: bool,
    pub buffer_for_rebuffer_ms: u64,
}

#[repr(C)]
#[derive(Debug, Default)]
pub struct PlayerFfiRetryPolicy {
    pub uses_default_max_attempts: bool,
    pub has_max_attempts: bool,
    pub max_attempts: u32,
    pub has_base_delay_ms: bool,
    pub base_delay_ms: u64,
    pub has_max_delay_ms: bool,
    pub max_delay_ms: u64,
    pub has_backoff: bool,
    pub backoff: PlayerFfiRetryBackoff,
}

#[repr(C)]
#[derive(Debug, Default)]
pub struct PlayerFfiCachePolicy {
    pub preset: PlayerFfiCachePreset,
    pub has_max_memory_bytes: bool,
    pub max_memory_bytes: u64,
    pub has_max_disk_bytes: bool,
    pub max_disk_bytes: u64,
}

#[repr(C)]
#[derive(Debug, Default)]
pub struct PlayerFfiResolvedResiliencePolicy {
    pub buffering: PlayerFfiBufferingPolicy,
    pub retry: PlayerFfiRetryPolicy,
    pub cache: PlayerFfiCachePolicy,
}

#[repr(C)]
#[derive(Debug, Default)]
pub struct PlayerFfiPreloadBudgetPolicy {
    pub has_max_concurrent_tasks: bool,
    pub max_concurrent_tasks: u32,
    pub has_max_memory_bytes: bool,
    pub max_memory_bytes: u64,
    pub has_max_disk_bytes: bool,
    pub max_disk_bytes: u64,
    pub has_warmup_window_ms: bool,
    pub warmup_window_ms: u64,
}

#[repr(C)]
#[derive(Debug, Default)]
pub struct PlayerFfiResolvedPreloadBudgetPolicy {
    pub max_concurrent_tasks: u32,
    pub max_memory_bytes: u64,
    pub max_disk_bytes: u64,
    pub warmup_window_ms: u64,
}

#[repr(C)]
#[derive(Debug, Default)]
pub struct PlayerFfiTrackPreferences {
    pub preferred_audio_language: *mut c_char,
    pub preferred_subtitle_language: *mut c_char,
    pub select_subtitles_by_default: bool,
    pub select_undetermined_subtitle_language: bool,
    pub audio_selection: PlayerFfiTrackSelection,
    pub subtitle_selection: PlayerFfiTrackSelection,
    pub abr_policy: PlayerFfiAbrPolicy,
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

impl From<PlayerFfiMediaSourceKind> for BridgeMediaSourceKind {
    fn from(value: PlayerFfiMediaSourceKind) -> Self {
        match value {
            PlayerFfiMediaSourceKind::Local => Self::Local,
            PlayerFfiMediaSourceKind::Remote => Self::Remote,
        }
    }
}

impl From<PlayerFfiMediaSourceProtocol> for BridgeMediaSourceProtocol {
    fn from(value: PlayerFfiMediaSourceProtocol) -> Self {
        match value {
            PlayerFfiMediaSourceProtocol::Unknown => Self::Unknown,
            PlayerFfiMediaSourceProtocol::File => Self::File,
            PlayerFfiMediaSourceProtocol::Content => Self::Content,
            PlayerFfiMediaSourceProtocol::Progressive => Self::Progressive,
            PlayerFfiMediaSourceProtocol::Hls => Self::Hls,
            PlayerFfiMediaSourceProtocol::Dash => Self::Dash,
        }
    }
}

impl From<BridgeBufferingPreset> for PlayerFfiBufferingPreset {
    fn from(value: BridgeBufferingPreset) -> Self {
        match value {
            BridgeBufferingPreset::Default => Self::Default,
            BridgeBufferingPreset::Balanced => Self::Balanced,
            BridgeBufferingPreset::Streaming => Self::Streaming,
            BridgeBufferingPreset::Resilient => Self::Resilient,
            BridgeBufferingPreset::LowLatency => Self::LowLatency,
        }
    }
}

impl From<PlayerFfiBufferingPreset> for BridgeBufferingPreset {
    fn from(value: PlayerFfiBufferingPreset) -> Self {
        match value {
            PlayerFfiBufferingPreset::Default => Self::Default,
            PlayerFfiBufferingPreset::Balanced => Self::Balanced,
            PlayerFfiBufferingPreset::Streaming => Self::Streaming,
            PlayerFfiBufferingPreset::Resilient => Self::Resilient,
            PlayerFfiBufferingPreset::LowLatency => Self::LowLatency,
        }
    }
}

impl From<BridgeBufferingPolicy> for PlayerFfiBufferingPolicy {
    fn from(value: BridgeBufferingPolicy) -> Self {
        Self {
            preset: value.preset.into(),
            has_min_buffer_ms: value.min_buffer_ms.is_some(),
            min_buffer_ms: value.min_buffer_ms.unwrap_or_default(),
            has_max_buffer_ms: value.max_buffer_ms.is_some(),
            max_buffer_ms: value.max_buffer_ms.unwrap_or_default(),
            has_buffer_for_playback_ms: value.buffer_for_playback_ms.is_some(),
            buffer_for_playback_ms: value.buffer_for_playback_ms.unwrap_or_default(),
            has_buffer_for_rebuffer_ms: value.buffer_for_rebuffer_ms.is_some(),
            buffer_for_rebuffer_ms: value.buffer_for_rebuffer_ms.unwrap_or_default(),
        }
    }
}

impl From<BridgeRetryBackoff> for PlayerFfiRetryBackoff {
    fn from(value: BridgeRetryBackoff) -> Self {
        match value {
            BridgeRetryBackoff::Fixed => Self::Fixed,
            BridgeRetryBackoff::Linear => Self::Linear,
            BridgeRetryBackoff::Exponential => Self::Exponential,
        }
    }
}

impl From<PlayerFfiRetryBackoff> for BridgeRetryBackoff {
    fn from(value: PlayerFfiRetryBackoff) -> Self {
        match value {
            PlayerFfiRetryBackoff::Fixed => Self::Fixed,
            PlayerFfiRetryBackoff::Linear => Self::Linear,
            PlayerFfiRetryBackoff::Exponential => Self::Exponential,
        }
    }
}

impl From<BridgeRetryPolicy> for PlayerFfiRetryPolicy {
    fn from(value: BridgeRetryPolicy) -> Self {
        Self {
            uses_default_max_attempts: false,
            has_max_attempts: value.max_attempts.is_some(),
            max_attempts: value.max_attempts.unwrap_or_default(),
            has_base_delay_ms: true,
            base_delay_ms: value.base_delay_ms,
            has_max_delay_ms: true,
            max_delay_ms: value.max_delay_ms,
            has_backoff: true,
            backoff: value.backoff.into(),
        }
    }
}

impl From<BridgeCachePreset> for PlayerFfiCachePreset {
    fn from(value: BridgeCachePreset) -> Self {
        match value {
            BridgeCachePreset::Default => Self::Default,
            BridgeCachePreset::Disabled => Self::Disabled,
            BridgeCachePreset::Streaming => Self::Streaming,
            BridgeCachePreset::Resilient => Self::Resilient,
        }
    }
}

impl From<PlayerFfiCachePreset> for BridgeCachePreset {
    fn from(value: PlayerFfiCachePreset) -> Self {
        match value {
            PlayerFfiCachePreset::Default => Self::Default,
            PlayerFfiCachePreset::Disabled => Self::Disabled,
            PlayerFfiCachePreset::Streaming => Self::Streaming,
            PlayerFfiCachePreset::Resilient => Self::Resilient,
        }
    }
}

impl From<BridgeCachePolicy> for PlayerFfiCachePolicy {
    fn from(value: BridgeCachePolicy) -> Self {
        Self {
            preset: value.preset.into(),
            has_max_memory_bytes: value.max_memory_bytes.is_some(),
            max_memory_bytes: value.max_memory_bytes.unwrap_or_default(),
            has_max_disk_bytes: value.max_disk_bytes.is_some(),
            max_disk_bytes: value.max_disk_bytes.unwrap_or_default(),
        }
    }
}

impl From<BridgeResolvedResiliencePolicy> for PlayerFfiResolvedResiliencePolicy {
    fn from(value: BridgeResolvedResiliencePolicy) -> Self {
        Self {
            buffering: value.buffering.into(),
            retry: value.retry.into(),
            cache: value.cache.into(),
        }
    }
}

impl From<BridgePreloadBudgetPolicy> for PlayerFfiPreloadBudgetPolicy {
    fn from(value: BridgePreloadBudgetPolicy) -> Self {
        Self {
            has_max_concurrent_tasks: value.max_concurrent_tasks.is_some(),
            max_concurrent_tasks: value.max_concurrent_tasks.unwrap_or_default(),
            has_max_memory_bytes: value.max_memory_bytes.is_some(),
            max_memory_bytes: value.max_memory_bytes.unwrap_or_default(),
            has_max_disk_bytes: value.max_disk_bytes.is_some(),
            max_disk_bytes: value.max_disk_bytes.unwrap_or_default(),
            has_warmup_window_ms: value.warmup_window_ms.is_some(),
            warmup_window_ms: value.warmup_window_ms.unwrap_or_default(),
        }
    }
}

impl From<BridgeResolvedPreloadBudgetPolicy> for PlayerFfiResolvedPreloadBudgetPolicy {
    fn from(value: BridgeResolvedPreloadBudgetPolicy) -> Self {
        Self {
            max_concurrent_tasks: value.max_concurrent_tasks,
            max_memory_bytes: value.max_memory_bytes,
            max_disk_bytes: value.max_disk_bytes,
            warmup_window_ms: value.warmup_window_ms,
        }
    }
}

impl From<BridgeTrackPreferences> for PlayerFfiTrackPreferences {
    fn from(value: BridgeTrackPreferences) -> Self {
        Self {
            preferred_audio_language: value
                .preferred_audio_language
                .map(into_c_string_ptr)
                .unwrap_or(ptr::null_mut()),
            preferred_subtitle_language: value
                .preferred_subtitle_language
                .map(into_c_string_ptr)
                .unwrap_or(ptr::null_mut()),
            select_subtitles_by_default: value.select_subtitles_by_default,
            select_undetermined_subtitle_language: value.select_undetermined_subtitle_language,
            audio_selection: value.audio_selection.into(),
            subtitle_selection: value.subtitle_selection.into(),
            abr_policy: value.abr_policy.into(),
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
    out_initializer: *mut PlayerFfiInitializerHandle,
    out_error: *mut PlayerFfiError,
) -> PlayerFfiCallStatus {
    ffi_call(out_error, || {
        if out_initializer.is_null() {
            write_error(
                out_error,
                owned_api_error(PlayerFfiErrorCode::NullPointer, "out_initializer was null"),
            );
            return PlayerFfiCallStatus::Error;
        }

        write_default_if_non_null(out_initializer);
        let uri = match read_uri(uri) {
            Ok(uri) => uri,
            Err(error) => {
                write_error(out_error, error);
                return PlayerFfiCallStatus::Error;
            }
        };

        match FfiPlayerInitializer::probe_uri(uri) {
            Ok(initializer) => {
                let Some(handle) = into_initializer_handle(initializer) else {
                    write_error(
                        out_error,
                        owned_api_error(
                            PlayerFfiErrorCode::BackendFailure,
                            "initializer handle registry overflow",
                        ),
                    );
                    return PlayerFfiCallStatus::Error;
                };
                write_handle(out_initializer, handle);
                PlayerFfiCallStatus::Ok
            }
            Err(error) => {
                write_error(out_error, owned_bridge_error(error));
                PlayerFfiCallStatus::Error
            }
        }
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn player_ffi_resolve_resilience_policy(
    source_kind: PlayerFfiMediaSourceKind,
    source_protocol: PlayerFfiMediaSourceProtocol,
    buffering_policy: *const PlayerFfiBufferingPolicy,
    retry_policy: *const PlayerFfiRetryPolicy,
    cache_policy: *const PlayerFfiCachePolicy,
    out_policy: *mut PlayerFfiResolvedResiliencePolicy,
    out_error: *mut PlayerFfiError,
) -> PlayerFfiCallStatus {
    ffi_call(out_error, || {
        if out_policy.is_null() {
            write_error(
                out_error,
                owned_api_error(PlayerFfiErrorCode::NullPointer, "out_policy was null"),
            );
            return PlayerFfiCallStatus::Error;
        }

        let buffering_policy = match read_buffering_policy(buffering_policy) {
            Ok(policy) => policy,
            Err(error) => {
                write_error(out_error, error);
                return PlayerFfiCallStatus::Error;
            }
        };
        let retry_policy = match read_retry_policy(retry_policy) {
            Ok(policy) => policy,
            Err(error) => {
                write_error(out_error, error);
                return PlayerFfiCallStatus::Error;
            }
        };
        let cache_policy = match read_cache_policy(cache_policy) {
            Ok(policy) => policy,
            Err(error) => {
                write_error(out_error, error);
                return PlayerFfiCallStatus::Error;
            }
        };

        let resolved = resolve_resilience_policy(
            source_kind.into(),
            source_protocol.into(),
            buffering_policy,
            retry_policy,
            cache_policy,
        );

        unsafe {
            ptr::write(out_policy, resolved.into());
        }
        PlayerFfiCallStatus::Ok
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn player_ffi_resolve_preload_budget(
    preload_budget: *const PlayerFfiPreloadBudgetPolicy,
    out_budget: *mut PlayerFfiResolvedPreloadBudgetPolicy,
    out_error: *mut PlayerFfiError,
) -> PlayerFfiCallStatus {
    ffi_call(out_error, || {
        if out_budget.is_null() {
            write_error(
                out_error,
                owned_api_error(PlayerFfiErrorCode::NullPointer, "out_budget was null"),
            );
            return PlayerFfiCallStatus::Error;
        }

        let preload_budget = match read_preload_budget(preload_budget) {
            Ok(preload_budget) => preload_budget,
            Err(error) => {
                write_error(out_error, error);
                return PlayerFfiCallStatus::Error;
            }
        };

        let resolved = resolve_preload_budget(preload_budget);
        unsafe {
            ptr::write(out_budget, resolved.into());
        }
        PlayerFfiCallStatus::Ok
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn player_ffi_resolve_track_preferences(
    track_preferences: *const PlayerFfiTrackPreferences,
    out_preferences: *mut PlayerFfiTrackPreferences,
    out_error: *mut PlayerFfiError,
) -> PlayerFfiCallStatus {
    ffi_call(out_error, || {
        if out_preferences.is_null() {
            write_error(
                out_error,
                owned_api_error(PlayerFfiErrorCode::NullPointer, "out_preferences was null"),
            );
            return PlayerFfiCallStatus::Error;
        }

        let track_preferences = match read_track_preferences(track_preferences) {
            Ok(track_preferences) => track_preferences,
            Err(error) => {
                write_error(out_error, error);
                return PlayerFfiCallStatus::Error;
            }
        };

        let resolved = resolve_track_preferences(track_preferences);
        unsafe {
            ptr::write(out_preferences, resolved.into());
        }
        PlayerFfiCallStatus::Ok
    })
}

#[unsafe(no_mangle)]
/// Destroys an initializer handle.
///
/// Passing a zero-initialized handle is a no-op. Passing a stale or already
/// consumed handle returns `PlayerFfiErrorCode::InvalidState`.
pub extern "C" fn player_ffi_initializer_destroy(
    handle: PlayerFfiInitializerHandle,
    out_error: *mut PlayerFfiError,
) -> PlayerFfiCallStatus {
    ffi_call(out_error, || {
        if handle.is_invalid() {
            return PlayerFfiCallStatus::Ok;
        }

        if destroy_initializer_handle(handle) {
            PlayerFfiCallStatus::Ok
        } else {
            write_error(out_error, invalid_initializer_handle_error());
            PlayerFfiCallStatus::Error
        }
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn player_ffi_initializer_media_info(
    handle: PlayerFfiInitializerHandle,
    out_media_info: *mut PlayerFfiMediaInfo,
    out_error: *mut PlayerFfiError,
) -> PlayerFfiCallStatus {
    ffi_call(out_error, || {
        if out_media_info.is_null() {
            write_error(
                out_error,
                owned_api_error(PlayerFfiErrorCode::NullPointer, "out_media_info was null"),
            );
            return PlayerFfiCallStatus::Error;
        }

        let Some(initializer) =
            with_initializer_ref(handle, |initializer| initializer.media_info())
        else {
            write_error(out_error, invalid_initializer_handle_error());
            return PlayerFfiCallStatus::Error;
        };

        unsafe {
            ptr::write(out_media_info, initializer.into());
        }
        PlayerFfiCallStatus::Ok
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn player_ffi_initializer_startup(
    handle: PlayerFfiInitializerHandle,
    out_startup: *mut PlayerFfiStartup,
    out_error: *mut PlayerFfiError,
) -> PlayerFfiCallStatus {
    ffi_call(out_error, || {
        if out_startup.is_null() {
            write_error(
                out_error,
                owned_api_error(PlayerFfiErrorCode::NullPointer, "out_startup was null"),
            );
            return PlayerFfiCallStatus::Error;
        }

        let Some(startup) = with_initializer_ref(handle, |initializer| initializer.startup())
        else {
            write_error(out_error, invalid_initializer_handle_error());
            return PlayerFfiCallStatus::Error;
        };

        unsafe {
            ptr::write(out_startup, startup.into());
        }
        PlayerFfiCallStatus::Ok
    })
}

#[unsafe(no_mangle)]
/// Consumes `handle` and initializes a player instance.
///
/// On both success and error, `handle` is consumed and must not be passed to
/// `player_ffi_initializer_destroy` or any other `player_ffi_initializer_*`
/// function afterwards. Reusing the consumed handle returns
/// `PlayerFfiErrorCode::InvalidState`.
pub extern "C" fn player_ffi_initializer_initialize(
    handle: PlayerFfiInitializerHandle,
    out_player: *mut PlayerFfiHandle,
    out_has_initial_frame: *mut bool,
    out_initial_frame: *mut PlayerFfiVideoFrame,
    out_startup: *mut PlayerFfiStartup,
    out_error: *mut PlayerFfiError,
) -> PlayerFfiCallStatus {
    ffi_call(out_error, || {
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

        write_default_if_non_null(out_player);
        let Some(initializer) = take_initializer(handle) else {
            write_error(out_error, invalid_initializer_handle_error());
            return PlayerFfiCallStatus::Error;
        };

        match initializer.initialize() {
            Ok(bootstrap) => {
                let has_initial_frame = bootstrap.initial_frame.is_some();
                let initial_frame = bootstrap
                    .initial_frame
                    .map(PlayerFfiVideoFrame::from)
                    .unwrap_or_default();
                let Some(player_handle) = into_player_handle(bootstrap.player) else {
                    write_error(
                        out_error,
                        owned_api_error(
                            PlayerFfiErrorCode::BackendFailure,
                            "player handle registry overflow",
                        ),
                    );
                    return PlayerFfiCallStatus::Error;
                };
                unsafe {
                    ptr::write(out_player, player_handle);
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
    })
}

#[unsafe(no_mangle)]
/// Destroys a player handle.
///
/// Passing a zero-initialized handle is a no-op. Passing a stale or already
/// destroyed handle returns `PlayerFfiErrorCode::InvalidState`.
pub extern "C" fn player_ffi_player_destroy(
    handle: PlayerFfiHandle,
    out_error: *mut PlayerFfiError,
) -> PlayerFfiCallStatus {
    ffi_call(out_error, || {
        if handle.is_invalid() {
            return PlayerFfiCallStatus::Ok;
        }

        if destroy_player_handle(handle) {
            PlayerFfiCallStatus::Ok
        } else {
            write_error(out_error, invalid_player_handle_error());
            PlayerFfiCallStatus::Error
        }
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn player_ffi_player_snapshot(
    handle: PlayerFfiHandle,
    out_snapshot: *mut PlayerFfiSnapshot,
    out_error: *mut PlayerFfiError,
) -> PlayerFfiCallStatus {
    ffi_call(out_error, || {
        if out_snapshot.is_null() {
            write_error(
                out_error,
                owned_api_error(PlayerFfiErrorCode::NullPointer, "out_snapshot was null"),
            );
            return PlayerFfiCallStatus::Error;
        }

        let Some(snapshot) = with_player_ref(handle, |player| player.snapshot()) else {
            write_error(out_error, invalid_player_handle_error());
            return PlayerFfiCallStatus::Error;
        };

        unsafe {
            ptr::write(out_snapshot, snapshot.into());
        }
        PlayerFfiCallStatus::Ok
    })
}

#[unsafe(no_mangle)]
/// Dispatches a player command and writes the resulting snapshot.
///
/// `out_frame` is optional. Pass `NULL` when the caller does not need an
/// immediate frame payload for this dispatch.
pub extern "C" fn player_ffi_player_dispatch(
    handle: PlayerFfiHandle,
    command: PlayerFfiCommandKind,
    position_ms: u64,
    out_applied: *mut bool,
    out_frame: *mut PlayerFfiVideoFrame,
    out_snapshot: *mut PlayerFfiSnapshot,
    out_error: *mut PlayerFfiError,
) -> PlayerFfiCallStatus {
    ffi_call(out_error, || {
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

        let Some(result) = with_player_mut(handle, |player| {
            player.dispatch(to_bridge_command(command, position_ms))
        }) else {
            write_error(out_error, invalid_player_handle_error());
            return PlayerFfiCallStatus::Error;
        };

        match result {
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
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn player_ffi_player_set_playback_rate(
    handle: PlayerFfiHandle,
    playback_rate: f32,
    out_applied: *mut bool,
    out_snapshot: *mut PlayerFfiSnapshot,
    out_error: *mut PlayerFfiError,
) -> PlayerFfiCallStatus {
    ffi_call(out_error, || {
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

        let Some(result) =
            with_player_mut(handle, |player| player.set_playback_rate(playback_rate))
        else {
            write_error(out_error, invalid_player_handle_error());
            return PlayerFfiCallStatus::Error;
        };

        match result {
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
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn player_ffi_player_set_video_track_selection(
    handle: PlayerFfiHandle,
    selection: *const PlayerFfiTrackSelection,
    out_applied: *mut bool,
    out_snapshot: *mut PlayerFfiSnapshot,
    out_error: *mut PlayerFfiError,
) -> PlayerFfiCallStatus {
    ffi_call(out_error, || {
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

        let selection = match read_track_selection(selection) {
            Ok(selection) => selection,
            Err(error) => {
                write_error(out_error, error);
                return PlayerFfiCallStatus::Error;
            }
        };

        let Some(result) =
            with_player_mut(handle, |player| player.set_video_track_selection(selection))
        else {
            write_error(out_error, invalid_player_handle_error());
            return PlayerFfiCallStatus::Error;
        };

        match result {
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
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn player_ffi_player_set_audio_track_selection(
    handle: PlayerFfiHandle,
    selection: *const PlayerFfiTrackSelection,
    out_applied: *mut bool,
    out_snapshot: *mut PlayerFfiSnapshot,
    out_error: *mut PlayerFfiError,
) -> PlayerFfiCallStatus {
    ffi_call(out_error, || {
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

        let selection = match read_track_selection(selection) {
            Ok(selection) => selection,
            Err(error) => {
                write_error(out_error, error);
                return PlayerFfiCallStatus::Error;
            }
        };

        let Some(result) =
            with_player_mut(handle, |player| player.set_audio_track_selection(selection))
        else {
            write_error(out_error, invalid_player_handle_error());
            return PlayerFfiCallStatus::Error;
        };

        match result {
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
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn player_ffi_player_set_subtitle_track_selection(
    handle: PlayerFfiHandle,
    selection: *const PlayerFfiTrackSelection,
    out_applied: *mut bool,
    out_snapshot: *mut PlayerFfiSnapshot,
    out_error: *mut PlayerFfiError,
) -> PlayerFfiCallStatus {
    ffi_call(out_error, || {
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

        let selection = match read_track_selection(selection) {
            Ok(selection) => selection,
            Err(error) => {
                write_error(out_error, error);
                return PlayerFfiCallStatus::Error;
            }
        };

        let Some(result) = with_player_mut(handle, |player| {
            player.set_subtitle_track_selection(selection)
        }) else {
            write_error(out_error, invalid_player_handle_error());
            return PlayerFfiCallStatus::Error;
        };

        match result {
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
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn player_ffi_player_set_abr_policy(
    handle: PlayerFfiHandle,
    policy: *const PlayerFfiAbrPolicy,
    out_applied: *mut bool,
    out_snapshot: *mut PlayerFfiSnapshot,
    out_error: *mut PlayerFfiError,
) -> PlayerFfiCallStatus {
    ffi_call(out_error, || {
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

        let policy = match read_abr_policy(policy) {
            Ok(policy) => policy,
            Err(error) => {
                write_error(out_error, error);
                return PlayerFfiCallStatus::Error;
            }
        };

        let Some(result) = with_player_mut(handle, |player| player.set_abr_policy(policy)) else {
            write_error(out_error, invalid_player_handle_error());
            return PlayerFfiCallStatus::Error;
        };

        match result {
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
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn player_ffi_player_drain_events(
    handle: PlayerFfiHandle,
    out_events: *mut PlayerFfiEventList,
    out_error: *mut PlayerFfiError,
) -> PlayerFfiCallStatus {
    ffi_call(out_error, || {
        if out_events.is_null() {
            write_error(
                out_error,
                owned_api_error(PlayerFfiErrorCode::NullPointer, "out_events was null"),
            );
            return PlayerFfiCallStatus::Error;
        }

        let Some(events) = with_player_mut(handle, |player| {
            player
                .drain_events()
                .into_iter()
                .map(PlayerFfiEvent::from)
                .collect::<Vec<_>>()
        }) else {
            write_error(out_error, invalid_player_handle_error());
            return PlayerFfiCallStatus::Error;
        };

        let (ptr, len) = into_owned_struct_array(events);

        unsafe {
            ptr::write(out_events, PlayerFfiEventList { ptr, len });
        }
        PlayerFfiCallStatus::Ok
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn player_ffi_player_advance(
    handle: PlayerFfiHandle,
    out_frame: *mut PlayerFfiVideoFrame,
    out_has_frame: *mut bool,
    out_error: *mut PlayerFfiError,
) -> PlayerFfiCallStatus {
    ffi_call(out_error, || {
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

        let Some(result) = with_player_mut(handle, |player| player.advance()) else {
            write_error(out_error, invalid_player_handle_error());
            return PlayerFfiCallStatus::Error;
        };

        match result {
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
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn player_ffi_player_next_deadline_delay_ms(
    handle: PlayerFfiHandle,
    out_has_deadline: *mut bool,
    out_delay_ms: *mut u64,
    out_error: *mut PlayerFfiError,
) -> PlayerFfiCallStatus {
    ffi_call(out_error, || {
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

        let Some(deadline) = with_player_ref(handle, |player| player.next_deadline_delay_ms())
        else {
            write_error(out_error, invalid_player_handle_error());
            return PlayerFfiCallStatus::Error;
        };

        unsafe {
            ptr::write(out_has_deadline, deadline.is_some());
            ptr::write(out_delay_ms, deadline.unwrap_or_default());
        }
        PlayerFfiCallStatus::Ok
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn player_ffi_error_free(error: *mut PlayerFfiError) {
    ffi_void(|| {
        let Some(error) = error_mut(error) else {
            return;
        };

        free_c_string(&mut error.message);
        *error = PlayerFfiError::default();
    });
}

#[unsafe(no_mangle)]
pub extern "C" fn player_ffi_media_info_free(media_info: *mut PlayerFfiMediaInfo) {
    ffi_void(|| {
        let Some(media_info) = media_info_mut(media_info) else {
            return;
        };

        free_media_info(media_info);
    });
}

#[unsafe(no_mangle)]
pub extern "C" fn player_ffi_track_preferences_free(
    track_preferences: *mut PlayerFfiTrackPreferences,
) {
    ffi_void(|| {
        let Some(track_preferences) = track_preferences_mut(track_preferences) else {
            return;
        };

        free_track_preferences(track_preferences);
    });
}

#[unsafe(no_mangle)]
pub extern "C" fn player_ffi_startup_free(startup: *mut PlayerFfiStartup) {
    ffi_void(|| {
        let Some(startup) = startup_mut(startup) else {
            return;
        };

        free_startup(startup);
    });
}

#[unsafe(no_mangle)]
pub extern "C" fn player_ffi_snapshot_free(snapshot: *mut PlayerFfiSnapshot) {
    ffi_void(|| {
        let Some(snapshot) = snapshot_mut(snapshot) else {
            return;
        };

        free_snapshot(snapshot);
    });
}

#[unsafe(no_mangle)]
pub extern "C" fn player_ffi_video_frame_free(frame: *mut PlayerFfiVideoFrame) {
    ffi_void(|| {
        let Some(frame) = video_frame_mut(frame) else {
            return;
        };

        free_video_frame(frame);
    });
}

#[unsafe(no_mangle)]
pub extern "C" fn player_ffi_event_list_free(events: *mut PlayerFfiEventList) {
    ffi_void(|| {
        let Some(events) = event_list_mut(events) else {
            return;
        };

        if !events.ptr.is_null() {
            unsafe {
                let mut boxed =
                    Box::from_raw(ptr::slice_from_raw_parts_mut(events.ptr, events.len));
                for event in boxed.iter_mut() {
                    free_event(event);
                }
            }
        }
        *events = PlayerFfiEventList::default();
    });
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

fn read_preload_budget(
    budget: *const PlayerFfiPreloadBudgetPolicy,
) -> Result<BridgePreloadBudgetPolicy, PlayerFfiError> {
    let Some(budget) = (unsafe { budget.as_ref() }) else {
        return Err(owned_api_error(
            PlayerFfiErrorCode::NullPointer,
            "preload budget pointer was null",
        ));
    };

    Ok(BridgePreloadBudgetPolicy {
        max_concurrent_tasks: budget
            .has_max_concurrent_tasks
            .then_some(budget.max_concurrent_tasks),
        max_memory_bytes: budget
            .has_max_memory_bytes
            .then_some(budget.max_memory_bytes),
        max_disk_bytes: budget.has_max_disk_bytes.then_some(budget.max_disk_bytes),
        warmup_window_ms: budget
            .has_warmup_window_ms
            .then_some(budget.warmup_window_ms),
    })
}

fn read_track_preferences(
    preferences: *const PlayerFfiTrackPreferences,
) -> Result<BridgeTrackPreferences, PlayerFfiError> {
    let Some(preferences) = (unsafe { preferences.as_ref() }) else {
        return Err(owned_api_error(
            PlayerFfiErrorCode::NullPointer,
            "track preferences pointer was null",
        ));
    };

    Ok(BridgeTrackPreferences {
        preferred_audio_language: read_optional_c_string(
            preferences.preferred_audio_language,
            "preferences.preferred_audio_language",
        )?,
        preferred_subtitle_language: read_optional_c_string(
            preferences.preferred_subtitle_language,
            "preferences.preferred_subtitle_language",
        )?,
        select_subtitles_by_default: preferences.select_subtitles_by_default,
        select_undetermined_subtitle_language: preferences.select_undetermined_subtitle_language,
        audio_selection: read_track_selection(&preferences.audio_selection)?,
        subtitle_selection: read_track_selection(&preferences.subtitle_selection)?,
        abr_policy: read_abr_policy(&preferences.abr_policy)?,
    })
}

fn read_buffering_policy(
    policy: *const PlayerFfiBufferingPolicy,
) -> Result<BridgeBufferingPolicy, PlayerFfiError> {
    let Some(policy) = (unsafe { policy.as_ref() }) else {
        return Err(owned_api_error(
            PlayerFfiErrorCode::NullPointer,
            "buffering policy pointer was null",
        ));
    };

    Ok(BridgeBufferingPolicy {
        preset: policy.preset.into(),
        min_buffer_ms: policy.has_min_buffer_ms.then_some(policy.min_buffer_ms),
        max_buffer_ms: policy.has_max_buffer_ms.then_some(policy.max_buffer_ms),
        buffer_for_playback_ms: policy
            .has_buffer_for_playback_ms
            .then_some(policy.buffer_for_playback_ms),
        buffer_for_rebuffer_ms: policy
            .has_buffer_for_rebuffer_ms
            .then_some(policy.buffer_for_rebuffer_ms),
    })
}

fn read_retry_policy(
    policy: *const PlayerFfiRetryPolicy,
) -> Result<BridgeRetryPolicy, PlayerFfiError> {
    let Some(policy) = (unsafe { policy.as_ref() }) else {
        return Err(owned_api_error(
            PlayerFfiErrorCode::NullPointer,
            "retry policy pointer was null",
        ));
    };

    Ok(BridgeRetryPolicy {
        max_attempts: if policy.uses_default_max_attempts {
            Some(3)
        } else if policy.has_max_attempts {
            Some(policy.max_attempts)
        } else {
            None
        },
        base_delay_ms: if policy.has_base_delay_ms {
            policy.base_delay_ms
        } else {
            1_000
        },
        max_delay_ms: if policy.has_max_delay_ms {
            policy.max_delay_ms
        } else {
            5_000
        },
        backoff: if policy.has_backoff {
            policy.backoff.into()
        } else {
            BridgeRetryBackoff::Linear
        },
    })
}

fn read_cache_policy(
    policy: *const PlayerFfiCachePolicy,
) -> Result<BridgeCachePolicy, PlayerFfiError> {
    let Some(policy) = (unsafe { policy.as_ref() }) else {
        return Err(owned_api_error(
            PlayerFfiErrorCode::NullPointer,
            "cache policy pointer was null",
        ));
    };

    Ok(BridgeCachePolicy {
        preset: policy.preset.into(),
        max_memory_bytes: policy
            .has_max_memory_bytes
            .then_some(policy.max_memory_bytes),
        max_disk_bytes: policy.has_max_disk_bytes.then_some(policy.max_disk_bytes),
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
    CString::new(sanitized).unwrap_or_default().into_raw()
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

fn free_track_preferences(track_preferences: &mut PlayerFfiTrackPreferences) {
    free_c_string(&mut track_preferences.preferred_audio_language);
    free_c_string(&mut track_preferences.preferred_subtitle_language);
    free_track_selection(&mut track_preferences.audio_selection);
    free_track_selection(&mut track_preferences.subtitle_selection);
    free_abr_policy(&mut track_preferences.abr_policy);
    *track_preferences = PlayerFfiTrackPreferences::default();
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

fn ffi_call(
    out_error: *mut PlayerFfiError,
    f: impl FnOnce() -> PlayerFfiCallStatus,
) -> PlayerFfiCallStatus {
    match catch_unwind(AssertUnwindSafe(f)) {
        Ok(status) => {
            if status == PlayerFfiCallStatus::Ok {
                write_success(out_error);
            }
            status
        }
        Err(payload) => {
            write_error(out_error, owned_panic_error(payload));
            PlayerFfiCallStatus::Error
        }
    }
}

fn ffi_void(f: impl FnOnce()) {
    let _ = catch_unwind(AssertUnwindSafe(f));
}

fn owned_panic_error(payload: Box<dyn Any + Send>) -> PlayerFfiError {
    let message = panic_payload_message(payload.as_ref());
    owned_api_error(
        PlayerFfiErrorCode::BackendFailure,
        &format!("player_ffi caught Rust panic: {message}"),
    )
}

fn panic_payload_message(payload: &(dyn Any + Send)) -> String {
    if let Some(message) = payload.downcast_ref::<&'static str>() {
        return (*message).to_owned();
    }

    if let Some(message) = payload.downcast_ref::<String>() {
        return message.clone();
    }

    "unknown panic payload".to_owned()
}

fn write_error(out_error: *mut PlayerFfiError, error: PlayerFfiError) {
    if out_error.is_null() {
        return;
    }

    unsafe {
        ptr::write(out_error, error);
    }
}

fn write_success(out_error: *mut PlayerFfiError) {
    if out_error.is_null() {
        return;
    }

    unsafe {
        ptr::write(out_error, PlayerFfiError::default());
    }
}

#[derive(Debug)]
struct HandleSlot<T> {
    generation: u32,
    value: T,
}

#[derive(Debug)]
struct HandleRegistry<T> {
    slots: Vec<Option<HandleSlot<T>>>,
    next_generation_seed: u32,
}

impl<T> HandleRegistry<T> {
    fn insert(&mut self, value: T) -> Option<u64> {
        let generation = self.allocate_generation();
        if let Some((slot_index, slot)) = self
            .slots
            .iter_mut()
            .enumerate()
            .find(|(_, slot)| slot.is_none())
        {
            *slot = Some(HandleSlot { generation, value });
            let slot_index = u32::try_from(slot_index).ok()?;
            return Some(encode_registry_handle(slot_index, generation));
        }

        let slot_index = u32::try_from(self.slots.len()).ok()?;
        self.slots.push(Some(HandleSlot { generation, value }));
        Some(encode_registry_handle(slot_index, generation))
    }

    fn get(&self, handle: u64) -> Option<&T> {
        let (slot_index, generation) = decode_registry_handle(handle)?;
        let slot = self.slots.get(slot_index as usize)?.as_ref()?;
        (slot.generation == generation).then_some(&slot.value)
    }

    fn get_mut(&mut self, handle: u64) -> Option<&mut T> {
        let (slot_index, generation) = decode_registry_handle(handle)?;
        let slot = self.slots.get_mut(slot_index as usize)?.as_mut()?;
        (slot.generation == generation).then_some(&mut slot.value)
    }

    fn remove(&mut self, handle: u64) -> Option<T> {
        let (slot_index, generation) = decode_registry_handle(handle)?;
        let slot = self.slots.get_mut(slot_index as usize)?;
        let existing = slot.as_ref()?;
        if existing.generation != generation {
            return None;
        }

        let value = slot.take().map(|entry| entry.value);
        self.compact_tail();
        value
    }

    fn allocate_generation(&mut self) -> u32 {
        let generation = next_generation(self.next_generation_seed);
        self.next_generation_seed = generation;
        generation
    }

    fn compact_tail(&mut self) {
        while matches!(self.slots.last(), Some(None)) {
            self.slots.pop();
        }
    }
}

impl<T> Default for HandleRegistry<T> {
    fn default() -> Self {
        Self {
            slots: Vec::new(),
            next_generation_seed: 0,
        }
    }
}

static INITIALIZER_HANDLE_REGISTRY: OnceLock<Mutex<HandleRegistry<usize>>> = OnceLock::new();
static PLAYER_HANDLE_REGISTRY: OnceLock<Mutex<HandleRegistry<usize>>> = OnceLock::new();

fn lock_initializer_registry() -> std::sync::MutexGuard<'static, HandleRegistry<usize>> {
    lock_registry(INITIALIZER_HANDLE_REGISTRY.get_or_init(|| Mutex::new(HandleRegistry::default())))
}

fn lock_player_registry() -> std::sync::MutexGuard<'static, HandleRegistry<usize>> {
    lock_registry(PLAYER_HANDLE_REGISTRY.get_or_init(|| Mutex::new(HandleRegistry::default())))
}

fn lock_registry<T>(
    registry: &'static Mutex<HandleRegistry<T>>,
) -> std::sync::MutexGuard<'static, HandleRegistry<T>> {
    match registry.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}

fn encode_registry_handle(slot_index: u32, generation: u32) -> u64 {
    ((u64::from(slot_index) + 1) << 32) | u64::from(generation.max(1))
}

fn decode_registry_handle(handle: u64) -> Option<(u32, u32)> {
    if handle == 0 {
        return None;
    }

    let slot_id = u32::try_from(handle >> 32).ok()?;
    let generation = handle as u32;
    if slot_id == 0 || generation == 0 {
        return None;
    }

    Some((slot_id - 1, generation))
}

fn next_generation(generation: u32) -> u32 {
    generation.wrapping_add(1).max(1)
}

fn into_initializer_handle(
    initializer: FfiPlayerInitializer,
) -> Option<PlayerFfiInitializerHandle> {
    let pointer = Box::into_raw(Box::new(initializer)) as usize;
    let Some(raw) = lock_initializer_registry().insert(pointer) else {
        unsafe {
            drop(Box::from_raw(pointer as *mut FfiPlayerInitializer));
        }
        return None;
    };
    Some(PlayerFfiInitializerHandle { raw })
}

fn into_player_handle(player: FfiPlayer) -> Option<PlayerFfiHandle> {
    let pointer = Box::into_raw(Box::new(player)) as usize;
    let Some(raw) = lock_player_registry().insert(pointer) else {
        unsafe {
            drop(Box::from_raw(pointer as *mut FfiPlayer));
        }
        return None;
    };
    Some(PlayerFfiHandle { raw })
}

fn with_initializer_ref<R>(
    handle: PlayerFfiInitializerHandle,
    f: impl FnOnce(&FfiPlayerInitializer) -> R,
) -> Option<R> {
    let registry = lock_initializer_registry();
    let pointer = registry.get(handle.raw).copied()?;
    unsafe { Some(f(&*(pointer as *const FfiPlayerInitializer))) }
}

fn take_initializer(handle: PlayerFfiInitializerHandle) -> Option<FfiPlayerInitializer> {
    let pointer = lock_initializer_registry().remove(handle.raw)?;
    unsafe { Some(*Box::from_raw(pointer as *mut FfiPlayerInitializer)) }
}

fn destroy_initializer_handle(handle: PlayerFfiInitializerHandle) -> bool {
    let Some(pointer) = lock_initializer_registry().remove(handle.raw) else {
        return false;
    };
    unsafe {
        drop(Box::from_raw(pointer as *mut FfiPlayerInitializer));
    }
    true
}

fn with_player_ref<R>(handle: PlayerFfiHandle, f: impl FnOnce(&FfiPlayer) -> R) -> Option<R> {
    let registry = lock_player_registry();
    let pointer = registry.get(handle.raw).copied()?;
    unsafe { Some(f(&*(pointer as *const FfiPlayer))) }
}

fn with_player_mut<R>(handle: PlayerFfiHandle, f: impl FnOnce(&mut FfiPlayer) -> R) -> Option<R> {
    let mut registry = lock_player_registry();
    let pointer = registry.get_mut(handle.raw).copied()?;
    unsafe { Some(f(&mut *(pointer as *mut FfiPlayer))) }
}

fn destroy_player_handle(handle: PlayerFfiHandle) -> bool {
    let Some(pointer) = lock_player_registry().remove(handle.raw) else {
        return false;
    };
    unsafe {
        drop(Box::from_raw(pointer as *mut FfiPlayer));
    }
    true
}

fn invalid_initializer_handle_error() -> PlayerFfiError {
    owned_api_error(
        PlayerFfiErrorCode::InvalidState,
        "initializer handle was invalid",
    )
}

fn invalid_player_handle_error() -> PlayerFfiError {
    owned_api_error(
        PlayerFfiErrorCode::InvalidState,
        "player handle was invalid",
    )
}

fn write_handle<T: Copy>(out_handle: *mut T, handle: T) {
    unsafe {
        ptr::write(out_handle, handle);
    }
}

fn write_default_if_non_null<T: Default>(out: *mut T) {
    if out.is_null() {
        return;
    }

    unsafe {
        ptr::write(out, T::default());
    }
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

fn track_preferences_mut(
    track_preferences: *mut PlayerFfiTrackPreferences,
) -> Option<&'static mut PlayerFfiTrackPreferences> {
    if track_preferences.is_null() {
        return None;
    }

    unsafe { Some(&mut *track_preferences) }
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

#[cfg(test)]
mod tests {
    use super::{
        FfiPlayerInitializer, PlayerFfiAbrMode, PlayerFfiCallStatus, PlayerFfiCommandKind,
        PlayerFfiError, PlayerFfiErrorCode, PlayerFfiEventKind, PlayerFfiHandle,
        PlayerFfiInitializerHandle, PlayerFfiMediaInfo, PlayerFfiPlaybackState, PlayerFfiSnapshot,
        PlayerFfiStartup, PlayerFfiTrackKind, PlayerFfiVideoFrame, into_initializer_handle,
        player_ffi_event_list_free, player_ffi_initializer_destroy,
        player_ffi_initializer_initialize, player_ffi_initializer_media_info,
        player_ffi_initializer_probe_uri, player_ffi_initializer_startup,
        player_ffi_media_info_free, player_ffi_player_destroy, player_ffi_player_dispatch,
        player_ffi_player_drain_events, player_ffi_player_set_playback_rate,
        player_ffi_snapshot_free, player_ffi_startup_free, player_ffi_video_frame_free,
    };
    use player_runtime::{
        DecodedVideoFrame, MediaAbrMode, MediaAbrPolicy, MediaSourceKind, MediaSourceProtocol,
        MediaTrack, MediaTrackCatalog, MediaTrackKind, MediaTrackSelection,
        MediaTrackSelectionSnapshot, PlaybackProgress, PlayerAudioInfo, PlayerMediaInfo,
        PlayerRuntimeAdapter, PlayerRuntimeAdapterBackendFamily, PlayerRuntimeAdapterBootstrap,
        PlayerRuntimeAdapterCapabilities, PlayerRuntimeAdapterFactory,
        PlayerRuntimeAdapterInitializer, PlayerRuntimeCommand, PlayerRuntimeCommandResult,
        PlayerRuntimeEvent, PlayerRuntimeInitializer, PlayerRuntimeOptions, PlayerRuntimeResult,
        PlayerRuntimeStartup, PlayerVideoInfo, PresentationState, VideoPixelFormat,
    };
    use std::ffi::{CStr, CString};
    use std::ptr;
    use std::time::Duration;

    #[test]
    fn initializer_probe_uri_rejects_null_output_pointer() {
        let uri = CString::new("https://example.com/master.m3u8").expect("valid uri");
        let mut error = PlayerFfiError::default();

        let status = player_ffi_initializer_probe_uri(uri.as_ptr(), ptr::null_mut(), &mut error);

        assert_eq!(status, PlayerFfiCallStatus::Error);
        assert_eq!(error.code, PlayerFfiErrorCode::NullPointer);
        assert_eq!(copy_c_string(error.message), "out_initializer was null");
        super::player_ffi_error_free(&mut error);
    }

    #[test]
    fn initializer_initialize_and_dispatch_accept_optional_frame_output() {
        let initializer = fake_initializer("https://example.com/master.m3u8");
        let handle = into_initializer_handle(initializer).expect("initializer handle should fit");
        let mut player_handle = PlayerFfiHandle::default();
        let mut has_initial_frame = false;
        let mut initial_frame = PlayerFfiVideoFrame::default();
        let mut startup = PlayerFfiStartup::default();
        let mut error = PlayerFfiError::default();

        let status = player_ffi_initializer_initialize(
            handle,
            &mut player_handle,
            &mut has_initial_frame,
            &mut initial_frame,
            &mut startup,
            &mut error,
        );

        assert_eq!(status, PlayerFfiCallStatus::Ok);
        assert_ne!(player_handle.raw, 0);
        assert!(has_initial_frame);
        assert_eq!(initial_frame.width, 2);
        assert!(startup.ffmpeg_initialized);
        assert_eq!(
            copy_c_string(startup.video_decode.hardware_backend),
            "stub-hw"
        );
        player_ffi_video_frame_free(&mut initial_frame);
        player_ffi_startup_free(&mut startup);

        let mut applied = false;
        let mut snapshot = PlayerFfiSnapshot::default();
        let dispatch_status = player_ffi_player_dispatch(
            player_handle,
            PlayerFfiCommandKind::Play,
            0,
            &mut applied,
            ptr::null_mut(),
            &mut snapshot,
            &mut error,
        );

        assert_eq!(dispatch_status, PlayerFfiCallStatus::Ok);
        assert!(applied);
        assert_eq!(snapshot.state, PlayerFfiPlaybackState::Playing);
        assert_eq!(
            copy_c_string(snapshot.source_uri),
            "https://example.com/master.m3u8"
        );
        assert_eq!(snapshot.media_info.track_catalog.len, 1);
        assert_eq!(
            unsafe { (*snapshot.media_info.track_catalog.tracks).kind },
            PlayerFfiTrackKind::Video
        );
        assert_eq!(
            snapshot.media_info.track_selection.abr_policy.mode,
            PlayerFfiAbrMode::FixedTrack
        );
        player_ffi_snapshot_free(&mut snapshot);

        let mut events = super::PlayerFfiEventList::default();
        let drain_status = player_ffi_player_drain_events(player_handle, &mut events, &mut error);
        assert_eq!(drain_status, PlayerFfiCallStatus::Ok);
        assert_eq!(events.len, 1);
        assert_eq!(
            unsafe { (*events.ptr).kind },
            PlayerFfiEventKind::PlaybackStateChanged
        );
        assert_eq!(
            unsafe { (*events.ptr).playback_state },
            PlayerFfiPlaybackState::Playing
        );
        player_ffi_event_list_free(&mut events);

        let destroy_status = player_ffi_player_destroy(player_handle, &mut error);
        assert_eq!(destroy_status, PlayerFfiCallStatus::Ok);
    }

    #[test]
    fn ffi_call_converts_panics_into_backend_failure() {
        let mut error = super::owned_api_error(PlayerFfiErrorCode::InvalidState, "stale error");

        let status = super::ffi_call(&mut error, || -> PlayerFfiCallStatus {
            panic!("ffi panic smoke");
        });

        assert_eq!(status, PlayerFfiCallStatus::Error);
        assert_eq!(error.code, PlayerFfiErrorCode::BackendFailure);
        assert_eq!(error.category, super::PlayerFfiErrorCategory::Platform);
        assert!(copy_c_string(error.message).contains("ffi panic smoke"));
        super::player_ffi_error_free(&mut error);
    }

    #[test]
    fn player_drain_events_preserves_order_and_is_one_shot() {
        let initializer = fake_initializer("https://example.com/master.m3u8");
        let handle = into_initializer_handle(initializer).expect("initializer handle should fit");
        let mut player_handle = PlayerFfiHandle::default();
        let mut has_initial_frame = false;
        let mut initial_frame = PlayerFfiVideoFrame::default();
        let mut startup = PlayerFfiStartup::default();
        let mut error = PlayerFfiError::default();

        let status = player_ffi_initializer_initialize(
            handle,
            &mut player_handle,
            &mut has_initial_frame,
            &mut initial_frame,
            &mut startup,
            &mut error,
        );
        assert_eq!(status, PlayerFfiCallStatus::Ok);
        assert_ne!(player_handle.raw, 0);
        player_ffi_video_frame_free(&mut initial_frame);
        player_ffi_startup_free(&mut startup);

        let mut applied = false;
        let mut snapshot = PlayerFfiSnapshot::default();
        let play_status = player_ffi_player_dispatch(
            player_handle,
            PlayerFfiCommandKind::Play,
            0,
            &mut applied,
            ptr::null_mut(),
            &mut snapshot,
            &mut error,
        );
        assert_eq!(play_status, PlayerFfiCallStatus::Ok);
        assert!(applied);
        player_ffi_snapshot_free(&mut snapshot);

        let rate_status = player_ffi_player_set_playback_rate(
            player_handle,
            1.25,
            &mut applied,
            &mut snapshot,
            &mut error,
        );
        assert_eq!(rate_status, PlayerFfiCallStatus::Ok);
        assert!(applied);
        player_ffi_snapshot_free(&mut snapshot);

        let mut events = super::PlayerFfiEventList::default();
        let drain_status = player_ffi_player_drain_events(player_handle, &mut events, &mut error);
        assert_eq!(drain_status, PlayerFfiCallStatus::Ok);
        assert_eq!(events.len, 2);
        assert_eq!(
            unsafe { (*events.ptr).kind },
            PlayerFfiEventKind::PlaybackStateChanged
        );
        assert_eq!(
            unsafe { (*events.ptr.add(1)).kind },
            PlayerFfiEventKind::PlaybackRateChanged
        );
        assert_eq!(unsafe { (*events.ptr.add(1)).playback_rate }, 1.25);
        player_ffi_event_list_free(&mut events);

        let second_drain_status =
            player_ffi_player_drain_events(player_handle, &mut events, &mut error);
        assert_eq!(second_drain_status, PlayerFfiCallStatus::Ok);
        assert_eq!(events.len, 0);
        assert!(events.ptr.is_null());
        player_ffi_event_list_free(&mut events);

        let destroy_status = player_ffi_player_destroy(player_handle, &mut error);
        assert_eq!(destroy_status, PlayerFfiCallStatus::Ok);
    }

    #[test]
    fn initializer_media_info_and_startup_round_trip_fake_runtime_payload() {
        let handle = into_initializer_handle(fake_initializer("https://example.com/video.mp4"))
            .expect("initializer handle should fit");
        let mut media_info = PlayerFfiMediaInfo::default();
        let mut startup = PlayerFfiStartup::default();
        let mut error = PlayerFfiError::default();

        let media_status = player_ffi_initializer_media_info(handle, &mut media_info, &mut error);
        assert_eq!(media_status, PlayerFfiCallStatus::Ok);
        assert_eq!(
            copy_c_string(media_info.source_uri),
            "https://example.com/video.mp4"
        );
        assert_eq!(
            media_info.source_kind,
            super::PlayerFfiMediaSourceKind::Remote
        );
        assert_eq!(
            media_info.source_protocol,
            super::PlayerFfiMediaSourceProtocol::Progressive
        );
        assert!(media_info.has_duration);
        assert_eq!(media_info.duration_ms, 60_000);
        assert!(media_info.has_best_video);
        assert_eq!(copy_c_string(media_info.best_video.codec), "h264");
        assert_eq!(media_info.track_catalog.len, 1);
        assert_eq!(
            unsafe { (*media_info.track_catalog.tracks).kind },
            PlayerFfiTrackKind::Video
        );
        player_ffi_media_info_free(&mut media_info);

        let startup_status = player_ffi_initializer_startup(handle, &mut startup, &mut error);
        assert_eq!(startup_status, PlayerFfiCallStatus::Ok);
        assert!(startup.ffmpeg_initialized);
        assert!(startup.has_audio_output);
        assert_eq!(
            copy_c_string(startup.audio_output.device_name),
            "Stub Speaker"
        );
        assert!(startup.has_video_decode);
        assert_eq!(
            copy_c_string(startup.video_decode.hardware_backend),
            "stub-hw"
        );
        player_ffi_startup_free(&mut startup);

        let destroy_status = player_ffi_initializer_destroy(handle, &mut error);
        assert_eq!(destroy_status, PlayerFfiCallStatus::Ok);
    }

    #[test]
    fn initializer_initialize_rejects_invalid_handle() {
        let mut player_handle = PlayerFfiHandle::default();
        let mut has_initial_frame = false;
        let mut initial_frame = PlayerFfiVideoFrame::default();
        let mut startup = PlayerFfiStartup::default();
        let mut error = PlayerFfiError::default();

        let status = player_ffi_initializer_initialize(
            PlayerFfiInitializerHandle::default(),
            &mut player_handle,
            &mut has_initial_frame,
            &mut initial_frame,
            &mut startup,
            &mut error,
        );

        assert_eq!(status, PlayerFfiCallStatus::Error);
        assert_eq!(error.code, PlayerFfiErrorCode::InvalidState);
        assert_eq!(
            copy_c_string(error.message),
            "initializer handle was invalid"
        );
        super::player_ffi_error_free(&mut error);
    }

    #[test]
    fn initializer_handle_becomes_invalid_after_initialize_consumes_it() {
        let handle = into_initializer_handle(fake_initializer("https://example.com/consumed.m3u8"))
            .expect("initializer handle should fit");
        let mut player_handle = PlayerFfiHandle::default();
        let mut has_initial_frame = false;
        let mut initial_frame = PlayerFfiVideoFrame::default();
        let mut startup = PlayerFfiStartup::default();
        let mut error = PlayerFfiError::default();

        let status = player_ffi_initializer_initialize(
            handle,
            &mut player_handle,
            &mut has_initial_frame,
            &mut initial_frame,
            &mut startup,
            &mut error,
        );
        assert_eq!(status, PlayerFfiCallStatus::Ok);
        player_ffi_video_frame_free(&mut initial_frame);
        player_ffi_startup_free(&mut startup);

        let mut consumed_startup = PlayerFfiStartup::default();
        let startup_status =
            player_ffi_initializer_startup(handle, &mut consumed_startup, &mut error);
        assert_eq!(startup_status, PlayerFfiCallStatus::Error);
        assert_eq!(error.code, PlayerFfiErrorCode::InvalidState);
        assert_eq!(
            copy_c_string(error.message),
            "initializer handle was invalid"
        );
        super::player_ffi_error_free(&mut error);

        let destroy_status = player_ffi_initializer_destroy(handle, &mut error);
        assert_eq!(destroy_status, PlayerFfiCallStatus::Error);
        assert_eq!(error.code, PlayerFfiErrorCode::InvalidState);
        assert_eq!(
            copy_c_string(error.message),
            "initializer handle was invalid"
        );
        super::player_ffi_error_free(&mut error);

        let player_destroy_status = player_ffi_player_destroy(player_handle, &mut error);
        assert_eq!(player_destroy_status, PlayerFfiCallStatus::Ok);
    }

    #[test]
    fn player_destroy_rejects_double_destroy_with_invalid_state() {
        let handle =
            into_initializer_handle(fake_initializer("https://example.com/double-destroy.m3u8"))
                .expect("initializer handle should fit");
        let mut player_handle = PlayerFfiHandle::default();
        let mut has_initial_frame = false;
        let mut initial_frame = PlayerFfiVideoFrame::default();
        let mut startup = PlayerFfiStartup::default();
        let mut error = PlayerFfiError::default();

        let status = player_ffi_initializer_initialize(
            handle,
            &mut player_handle,
            &mut has_initial_frame,
            &mut initial_frame,
            &mut startup,
            &mut error,
        );
        assert_eq!(status, PlayerFfiCallStatus::Ok);
        player_ffi_video_frame_free(&mut initial_frame);
        player_ffi_startup_free(&mut startup);

        let first_destroy_status = player_ffi_player_destroy(player_handle, &mut error);
        assert_eq!(first_destroy_status, PlayerFfiCallStatus::Ok);

        let second_destroy_status = player_ffi_player_destroy(player_handle, &mut error);
        assert_eq!(second_destroy_status, PlayerFfiCallStatus::Error);
        assert_eq!(error.code, PlayerFfiErrorCode::InvalidState);
        assert_eq!(copy_c_string(error.message), "player handle was invalid");
        super::player_ffi_error_free(&mut error);
    }

    fn copy_c_string(value: *mut std::ffi::c_char) -> String {
        if value.is_null() {
            return String::new();
        }
        unsafe { CStr::from_ptr(value).to_string_lossy().into_owned() }
    }

    fn fake_initializer(uri: &str) -> FfiPlayerInitializer {
        let factory = FakeRuntimeAdapterFactory;
        let inner = PlayerRuntimeInitializer::probe_uri_with_options_and_factory(
            uri,
            PlayerRuntimeOptions::default(),
            &factory,
        )
        .expect("fake initializer should probe");
        FfiPlayerInitializer { inner }
    }

    struct FakeRuntimeAdapterFactory;

    impl PlayerRuntimeAdapterFactory for FakeRuntimeAdapterFactory {
        fn adapter_id(&self) -> &'static str {
            "ffi-test-adapter"
        }

        fn probe_source_with_options(
            &self,
            source: player_core::MediaSource,
            _options: PlayerRuntimeOptions,
        ) -> PlayerRuntimeResult<Box<dyn PlayerRuntimeAdapterInitializer>> {
            Ok(Box::new(FakeRuntimeAdapterInitializer::new(
                source.uri().to_owned(),
            )))
        }
    }

    struct FakeRuntimeAdapterInitializer {
        source_uri: String,
        media_info: PlayerMediaInfo,
        startup: PlayerRuntimeStartup,
        initial_frame: DecodedVideoFrame,
        dispatch_frame: DecodedVideoFrame,
    }

    impl FakeRuntimeAdapterInitializer {
        fn new(source_uri: String) -> Self {
            let track_id = "video:1080p".to_owned();
            let media_info = PlayerMediaInfo {
                source_uri: source_uri.clone(),
                source_kind: MediaSourceKind::Remote,
                source_protocol: if source_uri.ends_with(".m3u8") {
                    MediaSourceProtocol::Hls
                } else {
                    MediaSourceProtocol::Progressive
                },
                duration: Some(Duration::from_secs(60)),
                bit_rate: Some(2_400_000),
                audio_streams: 1,
                video_streams: 1,
                best_video: Some(PlayerVideoInfo {
                    codec: "h264".to_owned(),
                    width: 1920,
                    height: 1080,
                    frame_rate: Some(30.0),
                }),
                best_audio: Some(PlayerAudioInfo {
                    codec: "aac".to_owned(),
                    sample_rate: 48_000,
                    channels: 2,
                }),
                track_catalog: MediaTrackCatalog {
                    tracks: vec![MediaTrack {
                        id: track_id.clone(),
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
                    }],
                    adaptive_video: true,
                    adaptive_audio: false,
                },
                track_selection: MediaTrackSelectionSnapshot {
                    video: MediaTrackSelection::auto(),
                    audio: MediaTrackSelection::auto(),
                    subtitle: MediaTrackSelection::disabled(),
                    abr_policy: MediaAbrPolicy {
                        mode: MediaAbrMode::FixedTrack,
                        track_id: Some(track_id),
                        max_bit_rate: Some(2_400_000),
                        max_width: Some(1920),
                        max_height: Some(1080),
                    },
                },
            };
            let startup = PlayerRuntimeStartup {
                ffmpeg_initialized: true,
                audio_output: Some(player_runtime::PlayerAudioOutputInfo {
                    device_name: Some("Stub Speaker".to_owned()),
                    channels: Some(2),
                    sample_rate: Some(48_000),
                    sample_format: Some("f32".to_owned()),
                }),
                decoded_audio: None,
                video_decode: Some(player_runtime::PlayerVideoDecodeInfo {
                    selected_mode: player_runtime::PlayerVideoDecodeMode::Hardware,
                    hardware_available: true,
                    hardware_backend: Some("stub-hw".to_owned()),
                    fallback_reason: None,
                }),
            };

            Self {
                source_uri,
                media_info,
                startup,
                initial_frame: fake_frame(10),
                dispatch_frame: fake_frame(20),
            }
        }
    }

    impl PlayerRuntimeAdapterInitializer for FakeRuntimeAdapterInitializer {
        fn capabilities(&self) -> PlayerRuntimeAdapterCapabilities {
            fake_capabilities()
        }

        fn media_info(&self) -> PlayerMediaInfo {
            self.media_info.clone()
        }

        fn startup(&self) -> PlayerRuntimeStartup {
            self.startup.clone()
        }

        fn initialize(self: Box<Self>) -> PlayerRuntimeResult<PlayerRuntimeAdapterBootstrap> {
            Ok(PlayerRuntimeAdapterBootstrap {
                runtime: Box::new(FakeRuntimeAdapter {
                    source_uri: self.source_uri,
                    media_info: self.media_info,
                    state: PresentationState::Ready,
                    playback_rate: 1.0,
                    progress: PlaybackProgress::new(
                        Duration::from_secs(12),
                        Some(Duration::from_secs(60)),
                    ),
                    pending_events: Vec::new(),
                    dispatch_frame: self.dispatch_frame,
                }),
                initial_frame: Some(self.initial_frame),
                startup: self.startup,
            })
        }
    }

    struct FakeRuntimeAdapter {
        source_uri: String,
        media_info: PlayerMediaInfo,
        state: PresentationState,
        playback_rate: f32,
        progress: PlaybackProgress,
        pending_events: Vec<PlayerRuntimeEvent>,
        dispatch_frame: DecodedVideoFrame,
    }

    impl PlayerRuntimeAdapter for FakeRuntimeAdapter {
        fn source_uri(&self) -> &str {
            &self.source_uri
        }

        fn capabilities(&self) -> PlayerRuntimeAdapterCapabilities {
            fake_capabilities()
        }

        fn media_info(&self) -> &PlayerMediaInfo {
            &self.media_info
        }

        fn presentation_state(&self) -> PresentationState {
            self.state
        }

        fn playback_rate(&self) -> f32 {
            self.playback_rate
        }

        fn progress(&self) -> PlaybackProgress {
            self.progress
        }

        fn drain_events(&mut self) -> Vec<PlayerRuntimeEvent> {
            self.pending_events.drain(..).collect()
        }

        fn dispatch(
            &mut self,
            command: PlayerRuntimeCommand,
        ) -> PlayerRuntimeResult<PlayerRuntimeCommandResult> {
            match command {
                PlayerRuntimeCommand::Play => {
                    self.state = PresentationState::Playing;
                    self.pending_events
                        .push(PlayerRuntimeEvent::PlaybackStateChanged(
                            PresentationState::Playing,
                        ));
                    Ok(PlayerRuntimeCommandResult {
                        applied: true,
                        frame: Some(self.dispatch_frame.clone()),
                        snapshot: PlayerRuntimeAdapter::snapshot(self),
                    })
                }
                PlayerRuntimeCommand::SetPlaybackRate { rate } => {
                    self.playback_rate = rate;
                    self.pending_events
                        .push(PlayerRuntimeEvent::PlaybackRateChanged { rate });
                    Ok(PlayerRuntimeCommandResult {
                        applied: true,
                        frame: None,
                        snapshot: PlayerRuntimeAdapter::snapshot(self),
                    })
                }
                _ => Ok(PlayerRuntimeCommandResult {
                    applied: false,
                    frame: None,
                    snapshot: PlayerRuntimeAdapter::snapshot(self),
                }),
            }
        }

        fn advance(&mut self) -> PlayerRuntimeResult<Option<DecodedVideoFrame>> {
            Ok(None)
        }

        fn next_deadline(&self) -> Option<std::time::Instant> {
            None
        }
    }

    fn fake_capabilities() -> PlayerRuntimeAdapterCapabilities {
        PlayerRuntimeAdapterCapabilities {
            adapter_id: "ffi-test-adapter",
            backend_family: PlayerRuntimeAdapterBackendFamily::Unknown,
            supports_audio_output: true,
            supports_frame_output: true,
            supports_external_video_surface: false,
            supports_seek: true,
            supports_stop: true,
            supports_playback_rate: true,
            playback_rate_min: Some(0.5),
            playback_rate_max: Some(3.0),
            natural_playback_rate_max: Some(2.0),
            supports_hardware_decode: true,
            supports_streaming: true,
            supports_hdr: false,
        }
    }

    fn fake_frame(presentation_time_ms: u64) -> DecodedVideoFrame {
        DecodedVideoFrame {
            presentation_time: Duration::from_millis(presentation_time_ms),
            width: 2,
            height: 2,
            bytes_per_row: 8,
            pixel_format: VideoPixelFormat::Rgba8888,
            bytes: vec![255; 16],
        }
    }
}
