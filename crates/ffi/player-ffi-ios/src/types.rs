use std::ffi::{c_char, c_void};
use std::path::PathBuf;

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PlayerFfiCallStatus {
    #[default]
    Ok = 0,
    Error = 1,
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
#[derive(Debug, Default)]
pub struct PlayerFfiError {
    pub code: PlayerFfiErrorCode,
    pub category: PlayerFfiErrorCategory,
    pub retriable: bool,
    pub message: *mut c_char,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct PlayerFfiDownloadExportCallbacks {
    pub context: *mut c_void,
    pub on_progress: Option<unsafe extern "C" fn(context: *mut c_void, ratio: f32)>,
    pub is_cancelled: Option<unsafe extern "C" fn(context: *mut c_void) -> bool>,
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PlayerFfiPreloadScopeKind {
    #[default]
    App = 0,
    Session = 1,
    Playlist = 2,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PlayerFfiPreloadCandidateKind {
    #[default]
    Current = 0,
    Neighbor = 1,
    Recommended = 2,
    Background = 3,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PlayerFfiPreloadSelectionHint {
    #[default]
    None = 0,
    CurrentItem = 1,
    NeighborItem = 2,
    RecommendedItem = 3,
    BackgroundFill = 4,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PlayerFfiPreloadPriority {
    #[default]
    Critical = 0,
    High = 1,
    Normal = 2,
    Low = 3,
    Background = 4,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PlayerFfiPreloadTaskStatus {
    #[default]
    Planned = 0,
    Active = 1,
    Cancelled = 2,
    Completed = 3,
    Expired = 4,
    Failed = 5,
}

#[repr(C)]
#[derive(Debug, Default)]
pub struct PlayerFfiPreloadCandidate {
    pub source_uri: *const c_char,
    pub scope_kind: PlayerFfiPreloadScopeKind,
    pub scope_id: *const c_char,
    pub candidate_kind: PlayerFfiPreloadCandidateKind,
    pub selection_hint: PlayerFfiPreloadSelectionHint,
    pub priority: PlayerFfiPreloadPriority,
    pub expected_memory_bytes: u64,
    pub expected_disk_bytes: u64,
    pub has_ttl_ms: bool,
    pub ttl_ms: u64,
    pub has_warmup_window_ms: bool,
    pub warmup_window_ms: u64,
}

#[repr(C)]
#[derive(Debug, Default)]
pub struct PlayerFfiPreloadTask {
    pub task_id: u64,
    pub source_uri: *mut c_char,
    pub source_identity: *mut c_char,
    pub cache_key: *mut c_char,
    pub scope_kind: PlayerFfiPreloadScopeKind,
    pub scope_id: *mut c_char,
    pub candidate_kind: PlayerFfiPreloadCandidateKind,
    pub selection_hint: PlayerFfiPreloadSelectionHint,
    pub priority: PlayerFfiPreloadPriority,
    pub status: PlayerFfiPreloadTaskStatus,
    pub expected_memory_bytes: u64,
    pub expected_disk_bytes: u64,
    pub warmup_window_ms: u64,
    pub has_error: bool,
    pub error_code: PlayerFfiErrorCode,
    pub error_category: PlayerFfiErrorCategory,
    pub error_retriable: bool,
    pub error_message: *mut c_char,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PlayerFfiPreloadCommandKind {
    #[default]
    Start = 0,
    Cancel = 1,
}

#[repr(C)]
#[derive(Debug, Default)]
pub struct PlayerFfiPreloadCommand {
    pub kind: PlayerFfiPreloadCommandKind,
    pub task: PlayerFfiPreloadTask,
    pub task_id: u64,
}

#[repr(C)]
#[derive(Debug, Default)]
pub struct PlayerFfiPreloadCommandList {
    pub commands: *mut PlayerFfiPreloadCommand,
    pub len: usize,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PlayerFfiPlaylistRepeatMode {
    #[default]
    Off = 0,
    One = 1,
    All = 2,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PlayerFfiPlaylistFailureStrategy {
    Pause = 0,
    #[default]
    SkipToNext = 1,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PlayerFfiPlaylistViewportHintKind {
    Visible = 0,
    NearVisible = 1,
    PrefetchOnly = 2,
    #[default]
    Hidden = 3,
}

#[repr(C)]
#[derive(Debug, Default)]
pub struct PlayerFfiPlaylistConfig {
    pub playlist_id: *const c_char,
    pub neighbor_previous: u32,
    pub neighbor_next: u32,
    pub preload_near_visible: u32,
    pub preload_prefetch_only: u32,
    pub auto_advance: bool,
    pub repeat_mode: PlayerFfiPlaylistRepeatMode,
    pub failure_strategy: PlayerFfiPlaylistFailureStrategy,
}

#[repr(C)]
#[derive(Debug, Default)]
pub struct PlayerFfiPlaylistQueueItem {
    pub item_id: *const c_char,
    pub source_uri: *const c_char,
    pub expected_memory_bytes: u64,
    pub expected_disk_bytes: u64,
    pub has_ttl_ms: bool,
    pub ttl_ms: u64,
    pub has_warmup_window_ms: bool,
    pub warmup_window_ms: u64,
}

#[repr(C)]
#[derive(Debug, Default)]
pub struct PlayerFfiPlaylistViewportHint {
    pub item_id: *const c_char,
    pub kind: PlayerFfiPlaylistViewportHintKind,
    pub order: u32,
}

#[repr(C)]
#[derive(Debug, Default)]
pub struct PlayerFfiPlaylistActiveItem {
    pub item_id: *mut c_char,
    pub index: u32,
}

#[repr(C)]
#[derive(Debug, Default)]
pub struct PlayerFfiDownloadConfig {
    pub auto_start: bool,
    pub run_post_processors_on_completion: bool,
    pub plugin_library_paths: *mut *mut c_char,
    pub plugin_library_paths_len: usize,
}

#[derive(Debug, Default)]
pub(crate) struct ResolvedDownloadConfig {
    pub(crate) auto_start: bool,
    pub(crate) run_post_processors_on_completion: bool,
    pub(crate) plugin_library_paths: Vec<PathBuf>,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PlayerFfiDownloadContentFormat {
    HlsSegments = 0,
    DashSegments = 1,
    FlvSegments = 2,
    SingleFile = 3,
    #[default]
    Unknown = 4,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PlayerFfiDownloadOutputFormat {
    Mp4 = 0,
    Mkv = 1,
    #[default]
    Original = 2,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PlayerFfiDownloadStreamKind {
    #[default]
    Combined = 0,
    Video = 1,
    Audio = 2,
    SecondaryAudio = 3,
    Subtitle = 4,
    Auxiliary = 5,
}

#[repr(C)]
#[derive(Debug, Default)]
pub struct PlayerFfiDownloadSource {
    pub source_uri: *mut c_char,
    pub content_format: PlayerFfiDownloadContentFormat,
    pub manifest_uri: *mut c_char,
    pub header_names: *mut *mut c_char,
    pub header_values: *mut *mut c_char,
    pub headers_len: usize,
}

#[repr(C)]
#[derive(Debug, Default)]
pub struct PlayerFfiDownloadProfile {
    pub variant_id: *mut c_char,
    pub preferred_audio_language: *mut c_char,
    pub preferred_subtitle_language: *mut c_char,
    pub selected_track_ids: *mut *mut c_char,
    pub selected_track_ids_len: usize,
    pub has_target_output_format: bool,
    pub target_output_format: PlayerFfiDownloadOutputFormat,
    pub target_directory: *mut c_char,
    pub allow_metered_network: bool,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct PlayerFfiDownloadByteRange {
    pub offset: u64,
    pub length: u64,
}

#[repr(C)]
#[derive(Debug, Default)]
pub struct PlayerFfiDownloadResourceRecord {
    pub resource_id: *mut c_char,
    pub uri: *mut c_char,
    pub relative_path: *mut c_char,
    pub has_byte_range: bool,
    pub byte_range: PlayerFfiDownloadByteRange,
    pub generated_text: *mut c_char,
    pub has_size_bytes: bool,
    pub size_bytes: u64,
    pub etag: *mut c_char,
    pub checksum: *mut c_char,
}

#[repr(C)]
#[derive(Debug, Default)]
pub struct PlayerFfiDownloadSegmentRecord {
    pub segment_id: *mut c_char,
    pub uri: *mut c_char,
    pub relative_path: *mut c_char,
    pub has_sequence: bool,
    pub sequence: u64,
    pub has_byte_range: bool,
    pub byte_range: PlayerFfiDownloadByteRange,
    pub has_size_bytes: bool,
    pub size_bytes: u64,
    pub checksum: *mut c_char,
}

#[repr(C)]
#[derive(Debug, Default)]
pub struct PlayerFfiDownloadAssetStream {
    pub stream_id: *mut c_char,
    pub kind: PlayerFfiDownloadStreamKind,
    pub language: *mut c_char,
    pub codec: *mut c_char,
    pub label: *mut c_char,
    pub has_quality_rank: bool,
    pub quality_rank: u32,
    pub resource_ids: *mut *mut c_char,
    pub resource_ids_len: usize,
    pub segment_ids: *mut *mut c_char,
    pub segment_ids_len: usize,
    pub metadata_keys: *mut *mut c_char,
    pub metadata_values: *mut *mut c_char,
    pub metadata_len: usize,
}

#[repr(C)]
#[derive(Debug, Default)]
pub struct PlayerFfiDownloadAssetIndex {
    pub content_format: PlayerFfiDownloadContentFormat,
    pub version: *mut c_char,
    pub etag: *mut c_char,
    pub checksum: *mut c_char,
    pub has_total_size_bytes: bool,
    pub total_size_bytes: u64,
    pub resources: *mut PlayerFfiDownloadResourceRecord,
    pub resources_len: usize,
    pub segments: *mut PlayerFfiDownloadSegmentRecord,
    pub segments_len: usize,
    pub streams: *mut PlayerFfiDownloadAssetStream,
    pub streams_len: usize,
    pub completed_path: *mut c_char,
}

#[repr(C)]
#[derive(Debug, Default)]
pub struct PlayerFfiDownloadProgressSnapshot {
    pub received_bytes: u64,
    pub has_total_bytes: bool,
    pub total_bytes: u64,
    pub received_segments: u32,
    pub has_total_segments: bool,
    pub total_segments: u32,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PlayerFfiDownloadTaskStatus {
    #[default]
    Queued = 0,
    Preparing = 1,
    Downloading = 2,
    Paused = 3,
    Completed = 4,
    Failed = 5,
    Removed = 6,
}

#[repr(C)]
#[derive(Debug, Default)]
pub struct PlayerFfiDownloadTask {
    pub task_id: u64,
    pub asset_id: *mut c_char,
    pub source: PlayerFfiDownloadSource,
    pub profile: PlayerFfiDownloadProfile,
    pub status: PlayerFfiDownloadTaskStatus,
    pub progress: PlayerFfiDownloadProgressSnapshot,
    pub asset_index: PlayerFfiDownloadAssetIndex,
    pub has_error: bool,
    pub error_code: PlayerFfiErrorCode,
    pub error_category: PlayerFfiErrorCategory,
    pub error_retriable: bool,
    pub error_message: *mut c_char,
}

#[repr(C)]
#[derive(Debug, Default)]
pub struct PlayerFfiDownloadSnapshot {
    pub tasks: *mut PlayerFfiDownloadTask,
    pub len: usize,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PlayerFfiDownloadCommandKind {
    #[default]
    Prepare = 0,
    Start = 1,
    Pause = 2,
    Resume = 3,
    Remove = 4,
}

#[repr(C)]
#[derive(Debug, Default)]
pub struct PlayerFfiDownloadCommand {
    pub kind: PlayerFfiDownloadCommandKind,
    pub task: PlayerFfiDownloadTask,
    pub task_id: u64,
}

#[repr(C)]
#[derive(Debug, Default)]
pub struct PlayerFfiDownloadCommandList {
    pub commands: *mut PlayerFfiDownloadCommand,
    pub len: usize,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PlayerFfiDownloadEventKind {
    #[default]
    Created = 0,
    StateChanged = 1,
    AssetIndexUpdated = 2,
    ProgressUpdated = 3,
}

#[repr(C)]
#[derive(Debug, Default)]
pub struct PlayerFfiDownloadEvent {
    pub kind: PlayerFfiDownloadEventKind,
    pub task: PlayerFfiDownloadTask,
    pub task_id: u64,
    pub status: PlayerFfiDownloadTaskStatus,
    pub progress: PlayerFfiDownloadProgressSnapshot,
    pub has_error: bool,
    pub error_code: PlayerFfiErrorCode,
    pub error_category: PlayerFfiErrorCategory,
    pub error_retriable: bool,
    pub error_message: *mut c_char,
    pub completed_path: *mut c_char,
}

#[repr(C)]
#[derive(Debug, Default)]
pub struct PlayerFfiDownloadEventList {
    pub events: *mut PlayerFfiDownloadEvent,
    pub len: usize,
}
