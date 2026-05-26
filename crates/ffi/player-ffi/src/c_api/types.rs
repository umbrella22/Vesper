use super::*;

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
    CommandChannelClosed = 11,
    EventChannelClosed = 12,
    Cancelled = 13,
    Timeout = 14,
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
    Warning = 13,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PlayerFfiRuntimeWarningDomain {
    #[default]
    FrameProcessor = 0,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PlayerFfiFrameProcessorWarningKind {
    #[default]
    Slow = 0,
    DeadlineMissed = 1,
    Backpressure = 2,
    BypassActivated = 3,
    LateOutputDropped = 4,
    OutputDropped = 5,
    Disabled = 6,
    Recovered = 7,
    Unsupported = 8,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PlayerFfiFrameProcessorPolicyAction {
    #[default]
    Continue = 0,
    BypassOriginalFrame = 1,
    DropOutput = 2,
    DisableProcessor = 3,
    FailPlayback = 4,
    DiagnosticsOnly = 5,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PlayerFfiPluginDiagnosticStatus {
    #[default]
    Loaded = 0,
    LoadFailed = 1,
    UnsupportedKind = 2,
    DecoderSupported = 3,
    DecoderUnsupported = 4,
    FrameProcessorSupported = 5,
    FrameProcessorUnsupported = 6,
    SourceNormalizerSupported = 7,
    SourceNormalizerUnsupported = 8,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PlayerFfiPluginCapabilityKind {
    #[default]
    None = 0,
    Decoder = 1,
    FrameProcessor = 2,
    SourceNormalizer = 3,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PlayerFfiPluginParticipation {
    #[default]
    Unknown = 0,
    Available = 1,
    Selected = 2,
    Participated = 3,
    Bypassed = 4,
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
    pub(crate) fn is_invalid(self) -> bool {
        self.raw == 0
    }
}

impl PlayerFfiHandle {
    pub(crate) fn is_invalid(self) -> bool {
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
pub struct PlayerFfiPluginCodecCapability {
    pub media_kind: *mut c_char,
    pub codec: *mut c_char,
}

#[repr(C)]
#[derive(Debug, Default)]
pub struct PlayerFfiPluginDecoderCapabilitySummary {
    pub codecs: *mut PlayerFfiPluginCodecCapability,
    pub codecs_len: usize,
    pub legacy_codecs: *mut *mut c_char,
    pub legacy_codecs_len: usize,
    pub supports_native_frame_output: bool,
    pub supports_hardware_decode: bool,
    pub supports_cpu_video_frames: bool,
    pub supports_audio_frames: bool,
    pub supports_gpu_handles: bool,
    pub supports_flush: bool,
    pub supports_drain: bool,
    pub has_max_sessions: bool,
    pub max_sessions: u32,
}

#[repr(C)]
#[derive(Debug, Default)]
pub struct PlayerFfiPluginFrameProcessorCapabilitySummary {
    pub accepted_input_handle_kinds: *mut *mut c_char,
    pub accepted_input_handle_kinds_len: usize,
    pub output_handle_kinds: *mut *mut c_char,
    pub output_handle_kinds_len: usize,
    pub supports_video_frames: bool,
    pub supports_in_place_passthrough: bool,
    pub preserves_dimensions: bool,
    pub may_change_dimensions: bool,
    pub preserves_color_metadata: bool,
    pub preserves_hdr_metadata: bool,
    pub supports_flush: bool,
    pub has_max_sessions: bool,
    pub max_sessions: u32,
    pub has_max_in_flight_frames: bool,
    pub max_in_flight_frames: u32,
}

#[repr(C)]
#[derive(Debug, Default)]
pub struct PlayerFfiPluginSourceNormalizerCapabilitySummary {
    pub supported_runtime_profiles: *mut *mut c_char,
    pub supported_runtime_profiles_len: usize,
    pub supported_output_routes: *mut *mut c_char,
    pub supported_output_routes_len: usize,
    pub max_level: *mut c_char,
    pub media_kinds: *mut *mut c_char,
    pub media_kinds_len: usize,
    pub codecs: *mut *mut c_char,
    pub codecs_len: usize,
    pub bitstream_formats: *mut *mut c_char,
    pub bitstream_formats_len: usize,
    pub supports_seek: bool,
    pub supports_flush: bool,
    pub supports_growing_resources: bool,
    pub supports_range_reads: bool,
    pub supports_cancel: bool,
    pub content_types: *mut *mut c_char,
    pub content_types_len: usize,
    pub required_libraries: *mut *mut c_char,
    pub required_libraries_len: usize,
    pub required_demuxers: *mut *mut c_char,
    pub required_demuxers_len: usize,
    pub required_muxers: *mut *mut c_char,
    pub required_muxers_len: usize,
    pub required_protocols: *mut *mut c_char,
    pub required_protocols_len: usize,
    pub required_parsers: *mut *mut c_char,
    pub required_parsers_len: usize,
    pub required_bitstream_filters: *mut *mut c_char,
    pub required_bitstream_filters_len: usize,
    pub required_tls: *mut c_char,
    pub requires_network: bool,
    pub has_session_read_buffer_bytes: bool,
    pub session_read_buffer_bytes: u64,
    pub has_manifest_snapshot_bytes: bool,
    pub manifest_snapshot_bytes: u64,
    pub has_session_disk_soft_cap_bytes: bool,
    pub session_disk_soft_cap_bytes: u64,
    pub has_global_disk_soft_cap_bytes: bool,
    pub global_disk_soft_cap_bytes: u64,
    pub has_max_sessions: bool,
    pub max_sessions: u32,
}

#[repr(C)]
#[derive(Debug, Default)]
pub struct PlayerFfiPluginCapabilitySummary {
    pub kind: PlayerFfiPluginCapabilityKind,
    pub decoder: PlayerFfiPluginDecoderCapabilitySummary,
    pub frame_processor: PlayerFfiPluginFrameProcessorCapabilitySummary,
    pub source_normalizer: PlayerFfiPluginSourceNormalizerCapabilitySummary,
}

#[repr(C)]
#[derive(Debug, Default)]
pub struct PlayerFfiPluginDiagnostic {
    pub path: *mut c_char,
    pub plugin_name: *mut c_char,
    pub plugin_kind: *mut c_char,
    pub status: PlayerFfiPluginDiagnosticStatus,
    pub message: *mut c_char,
    pub capability: PlayerFfiPluginCapabilitySummary,
    pub participation: PlayerFfiPluginParticipation,
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
    pub plugin_diagnostics: *mut PlayerFfiPluginDiagnostic,
    pub plugin_diagnostics_len: usize,
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
pub struct PlayerFfiFrameProcessorWarning {
    pub kind: PlayerFfiFrameProcessorWarningKind,
    pub plugin_name: *mut c_char,
    pub processor_index: usize,
    pub has_frame_id: bool,
    pub frame_id: u64,
    pub has_frame_pts_us: bool,
    pub frame_pts_us: i64,
    pub has_frame_duration_us: bool,
    pub frame_duration_us: i64,
    pub input_handle_kind: *mut c_char,
    pub output_handle_kind: *mut c_char,
    pub has_queue_depth: bool,
    pub queue_depth: u32,
    pub has_in_flight_frames: bool,
    pub in_flight_frames: u32,
    pub has_queue_wait_us: bool,
    pub queue_wait_us: u64,
    pub has_process_time_us: bool,
    pub process_time_us: u64,
    pub has_submit_to_ready_us: bool,
    pub submit_to_ready_us: u64,
    pub has_present_deadline_us: bool,
    pub present_deadline_us: i64,
    pub has_deadline_overrun_us: bool,
    pub deadline_overrun_us: u64,
    pub has_consecutive_miss_count: bool,
    pub consecutive_miss_count: u32,
    pub policy_action: PlayerFfiFrameProcessorPolicyAction,
    pub message: *mut c_char,
}

#[repr(C)]
#[derive(Debug, Default)]
pub struct PlayerFfiRuntimeWarning {
    pub domain: PlayerFfiRuntimeWarningDomain,
    pub frame_processor: PlayerFfiFrameProcessorWarning,
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
    pub warning: PlayerFfiRuntimeWarning,
    pub error: PlayerFfiError,
}

#[repr(C)]
#[derive(Debug, Default)]
pub struct PlayerFfiEventList {
    pub ptr: *mut PlayerFfiEvent,
    pub len: usize,
}
