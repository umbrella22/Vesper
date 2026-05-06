use std::borrow::Borrow;
use std::ffi::{CStr, CString, c_char, c_void};
use std::path::PathBuf;
use std::ptr;
use std::slice;
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use player_model::MediaSource;
use player_platform_ios::{
    IosDownloadBridgeSession, IosDownloadCommand, IosPlaylistBridgeSession,
    IosPreloadBridgeSession, IosPreloadCommand,
};
use player_plugin::ProcessorProgress;
use player_plugin_loader::BenchmarkSinkPluginSession;
use player_runtime::{
    DownloadAssetIndex, DownloadContentFormat, DownloadEvent, DownloadProfile,
    DownloadProgressSnapshot, DownloadResourceRecord, DownloadSegmentRecord, DownloadSource,
    DownloadTaskSnapshot, DownloadTaskStatus, MediaAbrMode, MediaAbrPolicy, MediaSourceKind,
    MediaSourceProtocol, MediaTrackSelection, MediaTrackSelectionMode, PlayerBufferingPolicy,
    PlayerBufferingPreset, PlayerCachePolicy, PlayerCachePreset, PlayerPreloadBudgetPolicy,
    PlayerRetryBackoff, PlayerRetryPolicy, PlayerRuntimeError, PlayerRuntimeErrorCategory,
    PlayerRuntimeErrorCode, PlayerTrackPreferencePolicy, PlaylistActiveItem,
    PlaylistCoordinatorConfig, PlaylistFailureStrategy, PlaylistNeighborWindow,
    PlaylistPreloadWindow, PlaylistQueueItem, PlaylistRepeatMode, PlaylistSwitchPolicy,
    PlaylistViewportHint, PlaylistViewportHintKind, PreloadBudget, PreloadBudgetScope,
    PreloadCandidate, PreloadCandidateKind, PreloadConfig, PreloadPriority, PreloadSelectionHint,
    PreloadTaskSnapshot,
    policy::{
        resolve_preload_budget as resolve_preload_budget_with_runtime,
        resolve_resilience_policy as resolve_resilience_policy_with_runtime,
        resolve_track_preferences as resolve_track_preferences_with_runtime,
    },
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
}

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PlayerFfiErrorCategory {
    #[default]
    Platform = 0,
    Input = 1,
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

#[derive(Debug)]
struct HandleRegistry<T> {
    slots: Vec<HandleSlot<T>>,
    free_slots: Vec<u32>,
}

#[derive(Debug)]
struct HandleSlot<T> {
    generation: u32,
    value: Option<T>,
}

impl<T> Default for HandleRegistry<T> {
    fn default() -> Self {
        Self {
            slots: Vec::new(),
            free_slots: Vec::new(),
        }
    }
}

impl<T> HandleRegistry<T> {
    fn insert(&mut self, value: T) -> u64 {
        if let Some(slot_index) = self.free_slots.pop() {
            let slot = &mut self.slots[slot_index as usize];
            slot.generation = next_generation(slot.generation);
            slot.value = Some(value);
            return encode_handle(slot_index, slot.generation);
        }

        let slot_index = self.slots.len() as u32;
        self.slots.push(HandleSlot {
            generation: 1,
            value: Some(value),
        });
        encode_handle(slot_index, 1)
    }

    fn get(&self, handle: impl Borrow<u64>) -> Option<&T> {
        let (slot_index, generation) = decode_handle(*handle.borrow())?;
        let slot = self.slots.get(slot_index as usize)?;
        (slot.generation == generation)
            .then_some(slot.value.as_ref())
            .flatten()
    }

    fn get_mut(&mut self, handle: impl Borrow<u64>) -> Option<&mut T> {
        let (slot_index, generation) = decode_handle(*handle.borrow())?;
        let slot = self.slots.get_mut(slot_index as usize)?;
        (slot.generation == generation)
            .then_some(slot.value.as_mut())
            .flatten()
    }

    fn remove(&mut self, handle: impl Borrow<u64>) -> Option<T> {
        let (slot_index, generation) = decode_handle(*handle.borrow())?;
        let slot = self.slots.get_mut(slot_index as usize)?;
        if slot.generation != generation {
            return None;
        }
        let value = slot.value.take()?;
        self.free_slots.push(slot_index);
        Some(value)
    }
}

fn encode_handle(slot_index: u32, generation: u32) -> u64 {
    let slot_id = u64::from(slot_index) + 1;
    (slot_id << 32) | u64::from(generation.max(1))
}

fn decode_handle(handle: u64) -> Option<(u32, u32)> {
    if handle == 0 {
        return None;
    }
    let slot_id = (handle >> 32) as u32;
    let generation = handle as u32;
    if slot_id == 0 || generation == 0 {
        return None;
    }
    Some((slot_id - 1, generation))
}

fn next_generation(generation: u32) -> u32 {
    generation.wrapping_add(1).max(1)
}

static PRELOAD_SESSIONS: OnceLock<Mutex<HandleRegistry<IosPreloadBridgeSession>>> = OnceLock::new();
static DOWNLOAD_SESSIONS: OnceLock<Mutex<HandleRegistry<IosDownloadBridgeSession>>> =
    OnceLock::new();
static PLAYLIST_SESSIONS: OnceLock<Mutex<HandleRegistry<IosPlaylistBridgeSession>>> =
    OnceLock::new();
static BENCHMARK_SESSIONS: OnceLock<Mutex<HandleRegistry<BenchmarkSinkPluginSession>>> =
    OnceLock::new();

fn preload_sessions() -> &'static Mutex<HandleRegistry<IosPreloadBridgeSession>> {
    PRELOAD_SESSIONS.get_or_init(|| Mutex::new(HandleRegistry::default()))
}

fn download_sessions() -> &'static Mutex<HandleRegistry<IosDownloadBridgeSession>> {
    DOWNLOAD_SESSIONS.get_or_init(|| Mutex::new(HandleRegistry::default()))
}

fn playlist_sessions() -> &'static Mutex<HandleRegistry<IosPlaylistBridgeSession>> {
    PLAYLIST_SESSIONS.get_or_init(|| Mutex::new(HandleRegistry::default()))
}

fn benchmark_sessions() -> &'static Mutex<HandleRegistry<BenchmarkSinkPluginSession>> {
    BENCHMARK_SESSIONS.get_or_init(|| Mutex::new(HandleRegistry::default()))
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
    pub error_code: u32,
    pub error_category: u32,
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
struct ResolvedDownloadConfig {
    auto_start: bool,
    run_post_processors_on_completion: bool,
    plugin_library_paths: Vec<PathBuf>,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PlayerFfiDownloadContentFormat {
    HlsSegments = 0,
    DashSegments = 1,
    SingleFile = 2,
    #[default]
    Unknown = 3,
}

#[repr(C)]
#[derive(Debug, Default)]
pub struct PlayerFfiDownloadSource {
    pub source_uri: *mut c_char,
    pub content_format: PlayerFfiDownloadContentFormat,
    pub manifest_uri: *mut c_char,
}

#[repr(C)]
#[derive(Debug, Default)]
pub struct PlayerFfiDownloadProfile {
    pub variant_id: *mut c_char,
    pub preferred_audio_language: *mut c_char,
    pub preferred_subtitle_language: *mut c_char,
    pub selected_track_ids: *mut *mut c_char,
    pub selected_track_ids_len: usize,
    pub target_directory: *mut c_char,
    pub allow_metered_network: bool,
}

#[repr(C)]
#[derive(Debug, Default)]
pub struct PlayerFfiDownloadResourceRecord {
    pub resource_id: *mut c_char,
    pub uri: *mut c_char,
    pub relative_path: *mut c_char,
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
    pub has_size_bytes: bool,
    pub size_bytes: u64,
    pub checksum: *mut c_char,
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
    pub error_code: u32,
    pub error_category: u32,
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
    Start = 0,
    Pause = 1,
    Resume = 2,
    Remove = 3,
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
    ProgressUpdated = 2,
}

#[repr(C)]
#[derive(Debug, Default)]
pub struct PlayerFfiDownloadEvent {
    pub kind: PlayerFfiDownloadEventKind,
    pub task: PlayerFfiDownloadTask,
}

#[repr(C)]
#[derive(Debug, Default)]
pub struct PlayerFfiDownloadEventList {
    pub events: *mut PlayerFfiDownloadEvent,
    pub len: usize,
}

/// # Safety
///
/// Raw pointers and opaque handles passed to this FFI entry point must either be null when
/// the parameter is documented as optional or point to valid objects allocated by the
/// matching Vesper FFI API for the duration of the call. Callers must serialize shared
/// handle access according to the host binding contract.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn player_ffi_resolve_resilience_policy(
    source_kind: PlayerFfiMediaSourceKind,
    source_protocol: PlayerFfiMediaSourceProtocol,
    buffering_policy: *const PlayerFfiBufferingPolicy,
    retry_policy: *const PlayerFfiRetryPolicy,
    cache_policy: *const PlayerFfiCachePolicy,
    out_policy: *mut PlayerFfiResolvedResiliencePolicy,
    out_error: *mut PlayerFfiError,
) -> PlayerFfiCallStatus {
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

    let resolved = resolve_resilience_policy_with_runtime(
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
}

/// # Safety
///
/// Raw pointers and opaque handles passed to this FFI entry point must either be null when
/// the parameter is documented as optional or point to valid objects allocated by the
/// matching Vesper FFI API for the duration of the call. Callers must serialize shared
/// handle access according to the host binding contract.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn player_ffi_resolve_preload_budget(
    preload_budget: *const PlayerFfiPreloadBudgetPolicy,
    out_budget: *mut PlayerFfiResolvedPreloadBudgetPolicy,
    out_error: *mut PlayerFfiError,
) -> PlayerFfiCallStatus {
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

    let resolved = resolve_preload_budget_with_runtime(preload_budget);
    unsafe {
        ptr::write(out_budget, resolved.into());
    }
    PlayerFfiCallStatus::Ok
}

/// # Safety
///
/// Raw pointers and opaque handles passed to this FFI entry point must either be null when
/// the parameter is documented as optional or point to valid objects allocated by the
/// matching Vesper FFI API for the duration of the call. Callers must serialize shared
/// handle access according to the host binding contract.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn player_ffi_preload_session_create(
    preload_budget: *const PlayerFfiResolvedPreloadBudgetPolicy,
    out_handle: *mut u64,
    out_error: *mut PlayerFfiError,
) -> PlayerFfiCallStatus {
    if out_handle.is_null() {
        write_error(
            out_error,
            owned_api_error(PlayerFfiErrorCode::NullPointer, "out_handle was null"),
        );
        return PlayerFfiCallStatus::Error;
    }

    let Some(preload_budget) = (unsafe { preload_budget.as_ref() }) else {
        write_error(
            out_error,
            owned_api_error(PlayerFfiErrorCode::NullPointer, "preload_budget was null"),
        );
        return PlayerFfiCallStatus::Error;
    };

    let budget_provider = player_runtime::InMemoryPreloadBudgetProvider::new(PreloadBudget {
        max_concurrent_tasks: preload_budget.max_concurrent_tasks,
        max_memory_bytes: preload_budget.max_memory_bytes,
        max_disk_bytes: preload_budget.max_disk_bytes,
        warmup_window: Duration::from_millis(preload_budget.warmup_window_ms),
    });
    let session = IosPreloadBridgeSession::new(budget_provider);

    let Ok(mut sessions) = preload_sessions().lock() else {
        write_error(
            out_error,
            owned_api_error(
                PlayerFfiErrorCode::InvalidArgument,
                "preload session registry lock failed",
            ),
        );
        return PlayerFfiCallStatus::Error;
    };
    let handle = sessions.insert(session);
    unsafe {
        ptr::write(out_handle, handle);
    }
    PlayerFfiCallStatus::Ok
}

/// # Safety
///
/// Raw pointers and opaque handles passed to this FFI entry point must either be null when
/// the parameter is documented as optional or point to valid objects allocated by the
/// matching Vesper FFI API for the duration of the call. Callers must serialize shared
/// handle access according to the host binding contract.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn player_ffi_preload_session_dispose(handle: u64) {
    if let Ok(mut sessions) = preload_sessions().lock() {
        sessions.remove(handle);
    }
}

/// # Safety
///
/// Raw pointers and opaque handles passed to this FFI entry point must either be null when
/// the parameter is documented as optional or point to valid objects allocated by the
/// matching Vesper FFI API for the duration of the call. Callers must serialize shared
/// handle access according to the host binding contract.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn player_ffi_preload_session_plan(
    handle: u64,
    candidates: *const PlayerFfiPreloadCandidate,
    candidates_len: usize,
    out_error: *mut PlayerFfiError,
) -> PlayerFfiCallStatus {
    let Ok(mut sessions) = preload_sessions().lock() else {
        write_error(
            out_error,
            owned_api_error(
                PlayerFfiErrorCode::InvalidArgument,
                "preload session registry lock failed",
            ),
        );
        return PlayerFfiCallStatus::Error;
    };
    let Some(session) = sessions.get_mut(handle) else {
        write_error(
            out_error,
            owned_api_error(
                PlayerFfiErrorCode::InvalidArgument,
                "invalid preload session handle",
            ),
        );
        return PlayerFfiCallStatus::Error;
    };

    let candidates = if candidates_len == 0 {
        &[][..]
    } else {
        if candidates.is_null() {
            write_error(
                out_error,
                owned_api_error(PlayerFfiErrorCode::NullPointer, "candidates was null"),
            );
            return PlayerFfiCallStatus::Error;
        }
        unsafe { slice::from_raw_parts(candidates, candidates_len) }
    };

    let rust_candidates = match candidates
        .iter()
        .map(read_preload_candidate)
        .collect::<Result<Vec<_>, _>>()
    {
        Ok(value) => value,
        Err(error) => {
            write_error(out_error, error);
            return PlayerFfiCallStatus::Error;
        }
    };

    session.plan(rust_candidates, std::time::Instant::now());
    PlayerFfiCallStatus::Ok
}

/// # Safety
///
/// Raw pointers and opaque handles passed to this FFI entry point must either be null when
/// the parameter is documented as optional or point to valid objects allocated by the
/// matching Vesper FFI API for the duration of the call. Callers must serialize shared
/// handle access according to the host binding contract.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn player_ffi_preload_session_drain_commands(
    handle: u64,
    out_commands: *mut PlayerFfiPreloadCommandList,
    out_error: *mut PlayerFfiError,
) -> PlayerFfiCallStatus {
    if out_commands.is_null() {
        write_error(
            out_error,
            owned_api_error(PlayerFfiErrorCode::NullPointer, "out_commands was null"),
        );
        return PlayerFfiCallStatus::Error;
    }

    let Ok(mut sessions) = preload_sessions().lock() else {
        write_error(
            out_error,
            owned_api_error(
                PlayerFfiErrorCode::InvalidArgument,
                "preload session registry lock failed",
            ),
        );
        return PlayerFfiCallStatus::Error;
    };
    let Some(session) = sessions.get_mut(handle) else {
        write_error(
            out_error,
            owned_api_error(
                PlayerFfiErrorCode::InvalidArgument,
                "invalid preload session handle",
            ),
        );
        return PlayerFfiCallStatus::Error;
    };

    let commands = session
        .drain_commands()
        .into_iter()
        .map(PlayerFfiPreloadCommand::from)
        .collect::<Vec<_>>();
    let len = commands.len();
    let ptr = if len == 0 {
        ptr::null_mut()
    } else {
        Box::into_raw(commands.into_boxed_slice()) as *mut PlayerFfiPreloadCommand
    };
    unsafe {
        ptr::write(
            out_commands,
            PlayerFfiPreloadCommandList { commands: ptr, len },
        );
    }
    PlayerFfiCallStatus::Ok
}

/// # Safety
///
/// Raw pointers and opaque handles passed to this FFI entry point must either be null when
/// the parameter is documented as optional or point to valid objects allocated by the
/// matching Vesper FFI API for the duration of the call. Callers must serialize shared
/// handle access according to the host binding contract.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn player_ffi_preload_session_complete(
    handle: u64,
    task_id: u64,
    out_error: *mut PlayerFfiError,
) -> PlayerFfiCallStatus {
    let Ok(mut sessions) = preload_sessions().lock() else {
        write_error(
            out_error,
            owned_api_error(
                PlayerFfiErrorCode::InvalidArgument,
                "preload session registry lock failed",
            ),
        );
        return PlayerFfiCallStatus::Error;
    };
    let Some(session) = sessions.get_mut(handle) else {
        write_error(
            out_error,
            owned_api_error(
                PlayerFfiErrorCode::InvalidArgument,
                "invalid preload session handle",
            ),
        );
        return PlayerFfiCallStatus::Error;
    };
    if let Err(error) = session.complete(player_runtime::PreloadTaskId::from_raw(task_id)) {
        write_error(out_error, runtime_error_to_ffi(error));
        return PlayerFfiCallStatus::Error;
    }
    PlayerFfiCallStatus::Ok
}

/// # Safety
///
/// Raw pointers and opaque handles passed to this FFI entry point must either be null when
/// the parameter is documented as optional or point to valid objects allocated by the
/// matching Vesper FFI API for the duration of the call. Callers must serialize shared
/// handle access according to the host binding contract.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn player_ffi_preload_session_fail(
    handle: u64,
    task_id: u64,
    code: u32,
    category: u32,
    retriable: bool,
    message: *const c_char,
    out_error: *mut PlayerFfiError,
) -> PlayerFfiCallStatus {
    let message = match read_optional_c_string(message, "message") {
        Ok(Some(value)) => value,
        Ok(None) => String::new(),
        Err(error) => {
            write_error(out_error, error);
            return PlayerFfiCallStatus::Error;
        }
    };

    let Ok(mut sessions) = preload_sessions().lock() else {
        write_error(
            out_error,
            owned_api_error(
                PlayerFfiErrorCode::InvalidArgument,
                "preload session registry lock failed",
            ),
        );
        return PlayerFfiCallStatus::Error;
    };
    let Some(session) = sessions.get_mut(handle) else {
        write_error(
            out_error,
            owned_api_error(
                PlayerFfiErrorCode::InvalidArgument,
                "invalid preload session handle",
            ),
        );
        return PlayerFfiCallStatus::Error;
    };

    let error = PlayerRuntimeError::with_taxonomy(
        ffi_runtime_error_code(code),
        ffi_runtime_error_category(category),
        retriable,
        message,
    );
    if let Err(error) = session.fail(player_runtime::PreloadTaskId::from_raw(task_id), error) {
        write_error(out_error, runtime_error_to_ffi(error));
        return PlayerFfiCallStatus::Error;
    }
    PlayerFfiCallStatus::Ok
}

/// # Safety
///
/// Raw pointers and opaque handles passed to this FFI entry point must either be null when
/// the parameter is documented as optional or point to valid objects allocated by the
/// matching Vesper FFI API for the duration of the call. Callers must serialize shared
/// handle access according to the host binding contract.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn player_ffi_preload_command_list_free(
    list: *mut PlayerFfiPreloadCommandList,
) {
    let Some(list) = (unsafe { list.as_mut() }) else {
        return;
    };
    if !list.commands.is_null() && list.len > 0 {
        let commands = unsafe { Vec::from_raw_parts(list.commands, list.len, list.len) };
        for mut command in commands {
            preload_command_free(&mut command);
        }
    }
    *list = PlayerFfiPreloadCommandList::default();
}

/// # Safety
///
/// Raw pointers and opaque handles passed to this FFI entry point must either be null when
/// the parameter is documented as optional or point to valid objects allocated by the
/// matching Vesper FFI API for the duration of the call. Callers must serialize shared
/// handle access according to the host binding contract.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn player_ffi_download_session_create(
    config: *const PlayerFfiDownloadConfig,
    out_handle: *mut u64,
    out_error: *mut PlayerFfiError,
) -> PlayerFfiCallStatus {
    if out_handle.is_null() {
        write_error(
            out_error,
            owned_api_error(PlayerFfiErrorCode::NullPointer, "out_handle was null"),
        );
        return PlayerFfiCallStatus::Error;
    }

    let config = match read_download_config(config) {
        Ok(config) => config,
        Err(error) => {
            write_error(out_error, error);
            return PlayerFfiCallStatus::Error;
        }
    };

    let session = match IosDownloadBridgeSession::new_with_plugin_library_paths(
        config.auto_start,
        config.run_post_processors_on_completion,
        config.plugin_library_paths,
    ) {
        Ok(session) => session,
        Err(error) => {
            write_error(out_error, runtime_error_to_ffi(error));
            return PlayerFfiCallStatus::Error;
        }
    };

    let Ok(mut sessions) = download_sessions().lock() else {
        write_error(
            out_error,
            owned_api_error(
                PlayerFfiErrorCode::InvalidArgument,
                "download session registry lock failed",
            ),
        );
        return PlayerFfiCallStatus::Error;
    };
    let handle = sessions.insert(session);
    unsafe {
        ptr::write(out_handle, handle);
    }
    PlayerFfiCallStatus::Ok
}

/// # Safety
///
/// Raw pointers and opaque handles passed to this FFI entry point must either be null when
/// the parameter is documented as optional or point to valid objects allocated by the
/// matching Vesper FFI API for the duration of the call. Callers must serialize shared
/// handle access according to the host binding contract.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn player_ffi_download_session_dispose(handle: u64) {
    if let Ok(mut sessions) = download_sessions().lock() {
        sessions.remove(handle);
    }
}

#[derive(Debug, Clone, Copy)]
struct FfiDownloadExportProgress {
    callbacks: PlayerFfiDownloadExportCallbacks,
}

// SAFETY: this callback table is an FFI contract provided by the host for the
// duration of a single synchronous export call.
unsafe impl Send for FfiDownloadExportProgress {}
// SAFETY: same reasoning as above; the host-provided callback context is
// expected to be safe for shared access during the export call.
unsafe impl Sync for FfiDownloadExportProgress {}

impl ProcessorProgress for FfiDownloadExportProgress {
    fn on_progress(&self, ratio: f32) {
        if let Some(on_progress) = self.callbacks.on_progress {
            unsafe { on_progress(self.callbacks.context, ratio) };
        }
    }

    fn is_cancelled(&self) -> bool {
        self.callbacks
            .is_cancelled
            .map(|is_cancelled| unsafe { is_cancelled(self.callbacks.context) })
            .unwrap_or(false)
    }
}

/// # Safety
///
/// Raw pointers and opaque handles passed to this FFI entry point must either be null when
/// the parameter is documented as optional or point to valid objects allocated by the
/// matching Vesper FFI API for the duration of the call. Callers must serialize shared
/// handle access according to the host binding contract.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn player_ffi_download_session_create_task(
    handle: u64,
    asset_id: *const c_char,
    source: *const PlayerFfiDownloadSource,
    profile: *const PlayerFfiDownloadProfile,
    asset_index: *const PlayerFfiDownloadAssetIndex,
    out_task_id: *mut u64,
    out_error: *mut PlayerFfiError,
) -> PlayerFfiCallStatus {
    if out_task_id.is_null() {
        write_error(
            out_error,
            owned_api_error(PlayerFfiErrorCode::NullPointer, "out_task_id was null"),
        );
        return PlayerFfiCallStatus::Error;
    }

    let asset_id = match read_optional_c_string(asset_id, "asset_id") {
        Ok(Some(asset_id)) => asset_id,
        Ok(None) => {
            write_error(
                out_error,
                owned_api_error(PlayerFfiErrorCode::NullPointer, "asset_id was null"),
            );
            return PlayerFfiCallStatus::Error;
        }
        Err(error) => {
            write_error(out_error, error);
            return PlayerFfiCallStatus::Error;
        }
    };
    let source = match read_download_source(source) {
        Ok(source) => source,
        Err(error) => {
            write_error(out_error, error);
            return PlayerFfiCallStatus::Error;
        }
    };
    let profile = match read_download_profile(profile) {
        Ok(profile) => profile,
        Err(error) => {
            write_error(out_error, error);
            return PlayerFfiCallStatus::Error;
        }
    };
    let asset_index = match read_download_asset_index(asset_index) {
        Ok(asset_index) => asset_index,
        Err(error) => {
            write_error(out_error, error);
            return PlayerFfiCallStatus::Error;
        }
    };

    let Ok(mut sessions) = download_sessions().lock() else {
        write_error(
            out_error,
            owned_api_error(
                PlayerFfiErrorCode::InvalidArgument,
                "download session registry lock failed",
            ),
        );
        return PlayerFfiCallStatus::Error;
    };
    let Some(session) = sessions.get_mut(handle) else {
        write_error(
            out_error,
            owned_api_error(
                PlayerFfiErrorCode::InvalidArgument,
                "invalid download session handle",
            ),
        );
        return PlayerFfiCallStatus::Error;
    };

    let task_id = match session.create_task(
        asset_id,
        source,
        profile,
        asset_index,
        std::time::Instant::now(),
    ) {
        Ok(task_id) => task_id,
        Err(error) => {
            write_error(out_error, runtime_error_to_ffi(error));
            return PlayerFfiCallStatus::Error;
        }
    };
    unsafe {
        ptr::write(out_task_id, task_id.get());
    }
    PlayerFfiCallStatus::Ok
}

fn with_download_session_task_mutation(
    handle: u64,
    task_id: u64,
    out_error: *mut PlayerFfiError,
    mutate: impl FnOnce(
        &mut IosDownloadBridgeSession,
        player_runtime::DownloadTaskId,
        std::time::Instant,
    ) -> player_runtime::PlayerRuntimeResult<Option<DownloadTaskSnapshot>>,
) -> PlayerFfiCallStatus {
    let Ok(mut sessions) = download_sessions().lock() else {
        write_error(
            out_error,
            owned_api_error(
                PlayerFfiErrorCode::InvalidArgument,
                "download session registry lock failed",
            ),
        );
        return PlayerFfiCallStatus::Error;
    };
    let Some(session) = sessions.get_mut(handle) else {
        write_error(
            out_error,
            owned_api_error(
                PlayerFfiErrorCode::InvalidArgument,
                "invalid download session handle",
            ),
        );
        return PlayerFfiCallStatus::Error;
    };

    if let Err(error) = mutate(
        session,
        player_runtime::DownloadTaskId::from_raw(task_id),
        std::time::Instant::now(),
    ) {
        write_error(out_error, runtime_error_to_ffi(error));
        return PlayerFfiCallStatus::Error;
    }
    PlayerFfiCallStatus::Ok
}

/// # Safety
///
/// Raw pointers and opaque handles passed to this FFI entry point must either be null when
/// the parameter is documented as optional or point to valid objects allocated by the
/// matching Vesper FFI API for the duration of the call. Callers must serialize shared
/// handle access according to the host binding contract.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn player_ffi_download_session_start_task(
    handle: u64,
    task_id: u64,
    out_error: *mut PlayerFfiError,
) -> PlayerFfiCallStatus {
    with_download_session_task_mutation(handle, task_id, out_error, |session, task_id, now| {
        session.start_task(task_id, now)
    })
}

/// # Safety
///
/// Raw pointers and opaque handles passed to this FFI entry point must either be null when
/// the parameter is documented as optional or point to valid objects allocated by the
/// matching Vesper FFI API for the duration of the call. Callers must serialize shared
/// handle access according to the host binding contract.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn player_ffi_download_session_pause_task(
    handle: u64,
    task_id: u64,
    out_error: *mut PlayerFfiError,
) -> PlayerFfiCallStatus {
    with_download_session_task_mutation(handle, task_id, out_error, |session, task_id, now| {
        session.pause_task(task_id, now)
    })
}

/// # Safety
///
/// Raw pointers and opaque handles passed to this FFI entry point must either be null when
/// the parameter is documented as optional or point to valid objects allocated by the
/// matching Vesper FFI API for the duration of the call. Callers must serialize shared
/// handle access according to the host binding contract.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn player_ffi_download_session_resume_task(
    handle: u64,
    task_id: u64,
    out_error: *mut PlayerFfiError,
) -> PlayerFfiCallStatus {
    with_download_session_task_mutation(handle, task_id, out_error, |session, task_id, now| {
        session.resume_task(task_id, now)
    })
}

/// # Safety
///
/// Raw pointers and opaque handles passed to this FFI entry point must either be null when
/// the parameter is documented as optional or point to valid objects allocated by the
/// matching Vesper FFI API for the duration of the call. Callers must serialize shared
/// handle access according to the host binding contract.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn player_ffi_download_session_update_progress(
    handle: u64,
    task_id: u64,
    received_bytes: u64,
    received_segments: u32,
    out_error: *mut PlayerFfiError,
) -> PlayerFfiCallStatus {
    let Ok(mut sessions) = download_sessions().lock() else {
        write_error(
            out_error,
            owned_api_error(
                PlayerFfiErrorCode::InvalidArgument,
                "download session registry lock failed",
            ),
        );
        return PlayerFfiCallStatus::Error;
    };
    let Some(session) = sessions.get_mut(handle) else {
        write_error(
            out_error,
            owned_api_error(
                PlayerFfiErrorCode::InvalidArgument,
                "invalid download session handle",
            ),
        );
        return PlayerFfiCallStatus::Error;
    };

    if let Err(error) = session.update_progress(
        player_runtime::DownloadTaskId::from_raw(task_id),
        received_bytes,
        received_segments,
        std::time::Instant::now(),
    ) {
        write_error(out_error, runtime_error_to_ffi(error));
        return PlayerFfiCallStatus::Error;
    }
    PlayerFfiCallStatus::Ok
}

/// # Safety
///
/// Raw pointers and opaque handles passed to this FFI entry point must either be null when
/// the parameter is documented as optional or point to valid objects allocated by the
/// matching Vesper FFI API for the duration of the call. Callers must serialize shared
/// handle access according to the host binding contract.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn player_ffi_download_session_complete_task(
    handle: u64,
    task_id: u64,
    completed_path: *const c_char,
    out_error: *mut PlayerFfiError,
) -> PlayerFfiCallStatus {
    let completed_path = match read_optional_c_string(completed_path, "completed_path") {
        Ok(value) => value.map(PathBuf::from),
        Err(error) => {
            write_error(out_error, error);
            return PlayerFfiCallStatus::Error;
        }
    };

    let Ok(mut sessions) = download_sessions().lock() else {
        write_error(
            out_error,
            owned_api_error(
                PlayerFfiErrorCode::InvalidArgument,
                "download session registry lock failed",
            ),
        );
        return PlayerFfiCallStatus::Error;
    };
    let Some(session) = sessions.get_mut(handle) else {
        write_error(
            out_error,
            owned_api_error(
                PlayerFfiErrorCode::InvalidArgument,
                "invalid download session handle",
            ),
        );
        return PlayerFfiCallStatus::Error;
    };

    if let Err(error) = session.complete_task(
        player_runtime::DownloadTaskId::from_raw(task_id),
        completed_path,
        std::time::Instant::now(),
    ) {
        write_error(out_error, runtime_error_to_ffi(error));
        return PlayerFfiCallStatus::Error;
    }
    PlayerFfiCallStatus::Ok
}

/// # Safety
///
/// Raw pointers and opaque handles passed to this FFI entry point must either be null when
/// the parameter is documented as optional or point to valid objects allocated by the
/// matching Vesper FFI API for the duration of the call. Callers must serialize shared
/// handle access according to the host binding contract.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn player_ffi_download_session_export_task(
    handle: u64,
    task_id: u64,
    output_path: *const c_char,
    callbacks: PlayerFfiDownloadExportCallbacks,
    out_error: *mut PlayerFfiError,
) -> PlayerFfiCallStatus {
    let output_path = match read_optional_c_string(output_path, "output_path") {
        Ok(Some(path)) => path,
        Ok(None) => {
            write_error(
                out_error,
                owned_api_error(PlayerFfiErrorCode::NullPointer, "output_path was null"),
            );
            return PlayerFfiCallStatus::Error;
        }
        Err(error) => {
            write_error(out_error, error);
            return PlayerFfiCallStatus::Error;
        }
    };

    let progress = FfiDownloadExportProgress { callbacks };
    let Ok(mut sessions) = download_sessions().lock() else {
        write_error(
            out_error,
            owned_api_error(
                PlayerFfiErrorCode::InvalidArgument,
                "download session registry lock failed",
            ),
        );
        return PlayerFfiCallStatus::Error;
    };
    let Some(session) = sessions.get_mut(handle) else {
        write_error(
            out_error,
            owned_api_error(
                PlayerFfiErrorCode::InvalidArgument,
                "invalid download session handle",
            ),
        );
        return PlayerFfiCallStatus::Error;
    };

    if let Err(error) = session.export_task_output(
        player_runtime::DownloadTaskId::from_raw(task_id),
        Some(PathBuf::from(output_path)),
        &progress,
    ) {
        write_error(out_error, runtime_error_to_ffi(error));
        return PlayerFfiCallStatus::Error;
    }

    PlayerFfiCallStatus::Ok
}

/// # Safety
///
/// Raw pointers and opaque handles passed to this FFI entry point must either be null when
/// the parameter is documented as optional or point to valid objects allocated by the
/// matching Vesper FFI API for the duration of the call. Callers must serialize shared
/// handle access according to the host binding contract.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn player_ffi_download_session_fail_task(
    handle: u64,
    task_id: u64,
    code: u32,
    category: u32,
    retriable: bool,
    message: *const c_char,
    out_error: *mut PlayerFfiError,
) -> PlayerFfiCallStatus {
    let message = match read_optional_c_string(message, "message") {
        Ok(Some(value)) => value,
        Ok(None) => String::new(),
        Err(error) => {
            write_error(out_error, error);
            return PlayerFfiCallStatus::Error;
        }
    };

    let Ok(mut sessions) = download_sessions().lock() else {
        write_error(
            out_error,
            owned_api_error(
                PlayerFfiErrorCode::InvalidArgument,
                "download session registry lock failed",
            ),
        );
        return PlayerFfiCallStatus::Error;
    };
    let Some(session) = sessions.get_mut(handle) else {
        write_error(
            out_error,
            owned_api_error(
                PlayerFfiErrorCode::InvalidArgument,
                "invalid download session handle",
            ),
        );
        return PlayerFfiCallStatus::Error;
    };

    let error = PlayerRuntimeError::with_taxonomy(
        ffi_runtime_error_code(code),
        ffi_runtime_error_category(category),
        retriable,
        message,
    );
    if let Err(error) = session.fail_task(
        player_runtime::DownloadTaskId::from_raw(task_id),
        error,
        std::time::Instant::now(),
    ) {
        write_error(out_error, runtime_error_to_ffi(error));
        return PlayerFfiCallStatus::Error;
    }
    PlayerFfiCallStatus::Ok
}

/// # Safety
///
/// Raw pointers and opaque handles passed to this FFI entry point must either be null when
/// the parameter is documented as optional or point to valid objects allocated by the
/// matching Vesper FFI API for the duration of the call. Callers must serialize shared
/// handle access according to the host binding contract.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn player_ffi_download_session_remove_task(
    handle: u64,
    task_id: u64,
    out_error: *mut PlayerFfiError,
) -> PlayerFfiCallStatus {
    with_download_session_task_mutation(handle, task_id, out_error, |session, task_id, now| {
        session.remove_task(task_id, now)
    })
}

/// # Safety
///
/// Raw pointers and opaque handles passed to this FFI entry point must either be null when
/// the parameter is documented as optional or point to valid objects allocated by the
/// matching Vesper FFI API for the duration of the call. Callers must serialize shared
/// handle access according to the host binding contract.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn player_ffi_download_session_snapshot(
    handle: u64,
    out_snapshot: *mut PlayerFfiDownloadSnapshot,
    out_error: *mut PlayerFfiError,
) -> PlayerFfiCallStatus {
    if out_snapshot.is_null() {
        write_error(
            out_error,
            owned_api_error(PlayerFfiErrorCode::NullPointer, "out_snapshot was null"),
        );
        return PlayerFfiCallStatus::Error;
    }

    let Ok(sessions) = download_sessions().lock() else {
        write_error(
            out_error,
            owned_api_error(
                PlayerFfiErrorCode::InvalidArgument,
                "download session registry lock failed",
            ),
        );
        return PlayerFfiCallStatus::Error;
    };
    let Some(session) = sessions.get(handle) else {
        write_error(
            out_error,
            owned_api_error(
                PlayerFfiErrorCode::InvalidArgument,
                "invalid download session handle",
            ),
        );
        return PlayerFfiCallStatus::Error;
    };

    let tasks = session
        .snapshot()
        .tasks
        .into_iter()
        .map(download_task_to_ffi)
        .collect::<Vec<_>>();
    let len = tasks.len();
    let ptr = if len == 0 {
        ptr::null_mut()
    } else {
        Box::into_raw(tasks.into_boxed_slice()) as *mut PlayerFfiDownloadTask
    };
    unsafe {
        ptr::write(out_snapshot, PlayerFfiDownloadSnapshot { tasks: ptr, len });
    }
    PlayerFfiCallStatus::Ok
}

/// # Safety
///
/// Raw pointers and opaque handles passed to this FFI entry point must either be null when
/// the parameter is documented as optional or point to valid objects allocated by the
/// matching Vesper FFI API for the duration of the call. Callers must serialize shared
/// handle access according to the host binding contract.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn player_ffi_download_session_drain_commands(
    handle: u64,
    out_commands: *mut PlayerFfiDownloadCommandList,
    out_error: *mut PlayerFfiError,
) -> PlayerFfiCallStatus {
    if out_commands.is_null() {
        write_error(
            out_error,
            owned_api_error(PlayerFfiErrorCode::NullPointer, "out_commands was null"),
        );
        return PlayerFfiCallStatus::Error;
    }

    let Ok(mut sessions) = download_sessions().lock() else {
        write_error(
            out_error,
            owned_api_error(
                PlayerFfiErrorCode::InvalidArgument,
                "download session registry lock failed",
            ),
        );
        return PlayerFfiCallStatus::Error;
    };
    let Some(session) = sessions.get_mut(handle) else {
        write_error(
            out_error,
            owned_api_error(
                PlayerFfiErrorCode::InvalidArgument,
                "invalid download session handle",
            ),
        );
        return PlayerFfiCallStatus::Error;
    };

    let commands = session
        .drain_commands()
        .into_iter()
        .map(PlayerFfiDownloadCommand::from)
        .collect::<Vec<_>>();
    let len = commands.len();
    let ptr = if len == 0 {
        ptr::null_mut()
    } else {
        Box::into_raw(commands.into_boxed_slice()) as *mut PlayerFfiDownloadCommand
    };
    unsafe {
        ptr::write(
            out_commands,
            PlayerFfiDownloadCommandList { commands: ptr, len },
        );
    }
    PlayerFfiCallStatus::Ok
}

/// # Safety
///
/// Raw pointers and opaque handles passed to this FFI entry point must either be null when
/// the parameter is documented as optional or point to valid objects allocated by the
/// matching Vesper FFI API for the duration of the call. Callers must serialize shared
/// handle access according to the host binding contract.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn player_ffi_download_session_drain_events(
    handle: u64,
    out_events: *mut PlayerFfiDownloadEventList,
    out_error: *mut PlayerFfiError,
) -> PlayerFfiCallStatus {
    if out_events.is_null() {
        write_error(
            out_error,
            owned_api_error(PlayerFfiErrorCode::NullPointer, "out_events was null"),
        );
        return PlayerFfiCallStatus::Error;
    }

    let Ok(mut sessions) = download_sessions().lock() else {
        write_error(
            out_error,
            owned_api_error(
                PlayerFfiErrorCode::InvalidArgument,
                "download session registry lock failed",
            ),
        );
        return PlayerFfiCallStatus::Error;
    };
    let Some(session) = sessions.get_mut(handle) else {
        write_error(
            out_error,
            owned_api_error(
                PlayerFfiErrorCode::InvalidArgument,
                "invalid download session handle",
            ),
        );
        return PlayerFfiCallStatus::Error;
    };

    let events = session
        .drain_events()
        .into_iter()
        .map(PlayerFfiDownloadEvent::from)
        .collect::<Vec<_>>();
    let len = events.len();
    let ptr = if len == 0 {
        ptr::null_mut()
    } else {
        Box::into_raw(events.into_boxed_slice()) as *mut PlayerFfiDownloadEvent
    };
    unsafe {
        ptr::write(out_events, PlayerFfiDownloadEventList { events: ptr, len });
    }
    PlayerFfiCallStatus::Ok
}

/// # Safety
///
/// Raw pointers and opaque handles passed to this FFI entry point must either be null when
/// the parameter is documented as optional or point to valid objects allocated by the
/// matching Vesper FFI API for the duration of the call. Callers must serialize shared
/// handle access according to the host binding contract.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn player_ffi_download_snapshot_free(
    snapshot: *mut PlayerFfiDownloadSnapshot,
) {
    let Some(snapshot) = (unsafe { snapshot.as_mut() }) else {
        return;
    };
    if !snapshot.tasks.is_null() && snapshot.len > 0 {
        let tasks = unsafe { Vec::from_raw_parts(snapshot.tasks, snapshot.len, snapshot.len) };
        for mut task in tasks {
            download_task_free(&mut task);
        }
    }
    *snapshot = PlayerFfiDownloadSnapshot::default();
}

/// # Safety
///
/// Raw pointers and opaque handles passed to this FFI entry point must either be null when
/// the parameter is documented as optional or point to valid objects allocated by the
/// matching Vesper FFI API for the duration of the call. Callers must serialize shared
/// handle access according to the host binding contract.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn player_ffi_download_command_list_free(
    list: *mut PlayerFfiDownloadCommandList,
) {
    let Some(list) = (unsafe { list.as_mut() }) else {
        return;
    };
    if !list.commands.is_null() && list.len > 0 {
        let commands = unsafe { Vec::from_raw_parts(list.commands, list.len, list.len) };
        for mut command in commands {
            download_command_free(&mut command);
        }
    }
    *list = PlayerFfiDownloadCommandList::default();
}

/// # Safety
///
/// Raw pointers and opaque handles passed to this FFI entry point must either be null when
/// the parameter is documented as optional or point to valid objects allocated by the
/// matching Vesper FFI API for the duration of the call. Callers must serialize shared
/// handle access according to the host binding contract.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn player_ffi_download_event_list_free(
    list: *mut PlayerFfiDownloadEventList,
) {
    let Some(list) = (unsafe { list.as_mut() }) else {
        return;
    };
    if !list.events.is_null() && list.len > 0 {
        let events = unsafe { Vec::from_raw_parts(list.events, list.len, list.len) };
        for mut event in events {
            download_event_free(&mut event);
        }
    }
    *list = PlayerFfiDownloadEventList::default();
}

/// # Safety
///
/// Raw pointers and opaque handles passed to this FFI entry point must either be null when
/// the parameter is documented as optional or point to valid objects allocated by the
/// matching Vesper FFI API for the duration of the call. Callers must serialize shared
/// handle access according to the host binding contract.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn player_ffi_playlist_session_create(
    config: *const PlayerFfiPlaylistConfig,
    preload_budget: *const PlayerFfiResolvedPreloadBudgetPolicy,
    out_handle: *mut u64,
    out_error: *mut PlayerFfiError,
) -> PlayerFfiCallStatus {
    if out_handle.is_null() {
        write_error(
            out_error,
            owned_api_error(PlayerFfiErrorCode::NullPointer, "out_handle was null"),
        );
        return PlayerFfiCallStatus::Error;
    }

    let config = match read_playlist_config(config) {
        Ok(config) => config,
        Err(error) => {
            write_error(out_error, error);
            return PlayerFfiCallStatus::Error;
        }
    };

    let Some(preload_budget) = (unsafe { preload_budget.as_ref() }) else {
        write_error(
            out_error,
            owned_api_error(PlayerFfiErrorCode::NullPointer, "preload_budget was null"),
        );
        return PlayerFfiCallStatus::Error;
    };

    let session = IosPlaylistBridgeSession::new(
        config.0,
        config.1,
        PreloadBudget {
            max_concurrent_tasks: preload_budget.max_concurrent_tasks,
            max_memory_bytes: preload_budget.max_memory_bytes,
            max_disk_bytes: preload_budget.max_disk_bytes,
            warmup_window: Duration::from_millis(preload_budget.warmup_window_ms),
        },
    );

    let Ok(mut sessions) = playlist_sessions().lock() else {
        write_error(
            out_error,
            owned_api_error(
                PlayerFfiErrorCode::InvalidArgument,
                "playlist session registry lock failed",
            ),
        );
        return PlayerFfiCallStatus::Error;
    };
    let handle = sessions.insert(session);
    unsafe {
        ptr::write(out_handle, handle);
    }
    PlayerFfiCallStatus::Ok
}

/// # Safety
///
/// Raw pointers and opaque handles passed to this FFI entry point must either be null when
/// the parameter is documented as optional or point to valid objects allocated by the
/// matching Vesper FFI API for the duration of the call. Callers must serialize shared
/// handle access according to the host binding contract.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn player_ffi_playlist_session_dispose(handle: u64) {
    if let Ok(mut sessions) = playlist_sessions().lock() {
        sessions.remove(handle);
    }
}

/// # Safety
///
/// Raw pointers and opaque handles passed to this FFI entry point must either be null when
/// the parameter is documented as optional or point to valid objects allocated by the
/// matching Vesper FFI API for the duration of the call. Callers must serialize shared
/// handle access according to the host binding contract.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn player_ffi_playlist_session_replace_queue(
    handle: u64,
    queue: *const PlayerFfiPlaylistQueueItem,
    queue_len: usize,
    out_error: *mut PlayerFfiError,
) -> PlayerFfiCallStatus {
    let Ok(mut sessions) = playlist_sessions().lock() else {
        write_error(
            out_error,
            owned_api_error(
                PlayerFfiErrorCode::InvalidArgument,
                "playlist session registry lock failed",
            ),
        );
        return PlayerFfiCallStatus::Error;
    };
    let Some(session) = sessions.get_mut(handle) else {
        write_error(
            out_error,
            owned_api_error(
                PlayerFfiErrorCode::InvalidArgument,
                "invalid playlist session handle",
            ),
        );
        return PlayerFfiCallStatus::Error;
    };

    let queue = if queue_len == 0 {
        &[][..]
    } else {
        if queue.is_null() {
            write_error(
                out_error,
                owned_api_error(PlayerFfiErrorCode::NullPointer, "queue was null"),
            );
            return PlayerFfiCallStatus::Error;
        }
        unsafe { slice::from_raw_parts(queue, queue_len) }
    };

    let rust_queue = match queue
        .iter()
        .map(read_playlist_queue_item)
        .collect::<Result<Vec<_>, _>>()
    {
        Ok(value) => value,
        Err(error) => {
            write_error(out_error, error);
            return PlayerFfiCallStatus::Error;
        }
    };

    session.replace_queue(rust_queue, std::time::Instant::now());
    PlayerFfiCallStatus::Ok
}

/// # Safety
///
/// Raw pointers and opaque handles passed to this FFI entry point must either be null when
/// the parameter is documented as optional or point to valid objects allocated by the
/// matching Vesper FFI API for the duration of the call. Callers must serialize shared
/// handle access according to the host binding contract.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn player_ffi_playlist_session_update_viewport_hints(
    handle: u64,
    hints: *const PlayerFfiPlaylistViewportHint,
    hints_len: usize,
    out_error: *mut PlayerFfiError,
) -> PlayerFfiCallStatus {
    let Ok(mut sessions) = playlist_sessions().lock() else {
        write_error(
            out_error,
            owned_api_error(
                PlayerFfiErrorCode::InvalidArgument,
                "playlist session registry lock failed",
            ),
        );
        return PlayerFfiCallStatus::Error;
    };
    let Some(session) = sessions.get_mut(handle) else {
        write_error(
            out_error,
            owned_api_error(
                PlayerFfiErrorCode::InvalidArgument,
                "invalid playlist session handle",
            ),
        );
        return PlayerFfiCallStatus::Error;
    };

    let hints = if hints_len == 0 {
        &[][..]
    } else {
        if hints.is_null() {
            write_error(
                out_error,
                owned_api_error(PlayerFfiErrorCode::NullPointer, "hints was null"),
            );
            return PlayerFfiCallStatus::Error;
        }
        unsafe { slice::from_raw_parts(hints, hints_len) }
    };

    let rust_hints = match hints
        .iter()
        .map(read_playlist_viewport_hint)
        .collect::<Result<Vec<_>, _>>()
    {
        Ok(value) => value,
        Err(error) => {
            write_error(out_error, error);
            return PlayerFfiCallStatus::Error;
        }
    };

    session.update_viewport_hints(rust_hints, std::time::Instant::now());
    PlayerFfiCallStatus::Ok
}

/// # Safety
///
/// Raw pointers and opaque handles passed to this FFI entry point must either be null when
/// the parameter is documented as optional or point to valid objects allocated by the
/// matching Vesper FFI API for the duration of the call. Callers must serialize shared
/// handle access according to the host binding contract.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn player_ffi_playlist_session_clear_viewport_hints(
    handle: u64,
    out_error: *mut PlayerFfiError,
) -> PlayerFfiCallStatus {
    let Ok(mut sessions) = playlist_sessions().lock() else {
        write_error(
            out_error,
            owned_api_error(
                PlayerFfiErrorCode::InvalidArgument,
                "playlist session registry lock failed",
            ),
        );
        return PlayerFfiCallStatus::Error;
    };
    let Some(session) = sessions.get_mut(handle) else {
        write_error(
            out_error,
            owned_api_error(
                PlayerFfiErrorCode::InvalidArgument,
                "invalid playlist session handle",
            ),
        );
        return PlayerFfiCallStatus::Error;
    };

    session.clear_viewport_hints(std::time::Instant::now());
    PlayerFfiCallStatus::Ok
}

fn with_playlist_session_advance(
    handle: u64,
    out_error: *mut PlayerFfiError,
    advance: impl FnOnce(&mut IosPlaylistBridgeSession, std::time::Instant),
) -> PlayerFfiCallStatus {
    let Ok(mut sessions) = playlist_sessions().lock() else {
        write_error(
            out_error,
            owned_api_error(
                PlayerFfiErrorCode::InvalidArgument,
                "playlist session registry lock failed",
            ),
        );
        return PlayerFfiCallStatus::Error;
    };
    let Some(session) = sessions.get_mut(handle) else {
        write_error(
            out_error,
            owned_api_error(
                PlayerFfiErrorCode::InvalidArgument,
                "invalid playlist session handle",
            ),
        );
        return PlayerFfiCallStatus::Error;
    };

    advance(session, std::time::Instant::now());
    PlayerFfiCallStatus::Ok
}

/// # Safety
///
/// Raw pointers and opaque handles passed to this FFI entry point must either be null when
/// the parameter is documented as optional or point to valid objects allocated by the
/// matching Vesper FFI API for the duration of the call. Callers must serialize shared
/// handle access according to the host binding contract.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn player_ffi_playlist_session_advance_to_next(
    handle: u64,
    out_error: *mut PlayerFfiError,
) -> PlayerFfiCallStatus {
    with_playlist_session_advance(handle, out_error, |session, now| {
        let _ = session.advance_to_next(now);
    })
}

/// # Safety
///
/// Raw pointers and opaque handles passed to this FFI entry point must either be null when
/// the parameter is documented as optional or point to valid objects allocated by the
/// matching Vesper FFI API for the duration of the call. Callers must serialize shared
/// handle access according to the host binding contract.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn player_ffi_playlist_session_advance_to_previous(
    handle: u64,
    out_error: *mut PlayerFfiError,
) -> PlayerFfiCallStatus {
    with_playlist_session_advance(handle, out_error, |session, now| {
        let _ = session.advance_to_previous(now);
    })
}

/// # Safety
///
/// Raw pointers and opaque handles passed to this FFI entry point must either be null when
/// the parameter is documented as optional or point to valid objects allocated by the
/// matching Vesper FFI API for the duration of the call. Callers must serialize shared
/// handle access according to the host binding contract.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn player_ffi_playlist_session_handle_playback_completed(
    handle: u64,
    out_error: *mut PlayerFfiError,
) -> PlayerFfiCallStatus {
    with_playlist_session_advance(handle, out_error, |session, now| {
        let _ = session.handle_playback_completed(now);
    })
}

/// # Safety
///
/// Raw pointers and opaque handles passed to this FFI entry point must either be null when
/// the parameter is documented as optional or point to valid objects allocated by the
/// matching Vesper FFI API for the duration of the call. Callers must serialize shared
/// handle access according to the host binding contract.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn player_ffi_playlist_session_handle_playback_failed(
    handle: u64,
    out_error: *mut PlayerFfiError,
) -> PlayerFfiCallStatus {
    with_playlist_session_advance(handle, out_error, |session, now| {
        let _ = session.handle_playback_failed(now);
    })
}

/// # Safety
///
/// Raw pointers and opaque handles passed to this FFI entry point must either be null when
/// the parameter is documented as optional or point to valid objects allocated by the
/// matching Vesper FFI API for the duration of the call. Callers must serialize shared
/// handle access according to the host binding contract.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn player_ffi_playlist_session_current_active_item(
    handle: u64,
    out_active_item: *mut PlayerFfiPlaylistActiveItem,
    out_error: *mut PlayerFfiError,
) -> PlayerFfiCallStatus {
    if out_active_item.is_null() {
        write_error(
            out_error,
            owned_api_error(PlayerFfiErrorCode::NullPointer, "out_active_item was null"),
        );
        return PlayerFfiCallStatus::Error;
    }

    let Ok(sessions) = playlist_sessions().lock() else {
        write_error(
            out_error,
            owned_api_error(
                PlayerFfiErrorCode::InvalidArgument,
                "playlist session registry lock failed",
            ),
        );
        return PlayerFfiCallStatus::Error;
    };
    let Some(session) = sessions.get(handle) else {
        write_error(
            out_error,
            owned_api_error(
                PlayerFfiErrorCode::InvalidArgument,
                "invalid playlist session handle",
            ),
        );
        return PlayerFfiCallStatus::Error;
    };

    let active_item = session
        .active_item()
        .map(playlist_active_item_to_ffi)
        .unwrap_or_default();
    unsafe {
        ptr::write(out_active_item, active_item);
    }
    PlayerFfiCallStatus::Ok
}

/// # Safety
///
/// Raw pointers and opaque handles passed to this FFI entry point must either be null when
/// the parameter is documented as optional or point to valid objects allocated by the
/// matching Vesper FFI API for the duration of the call. Callers must serialize shared
/// handle access according to the host binding contract.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn player_ffi_playlist_active_item_free(
    item: *mut PlayerFfiPlaylistActiveItem,
) {
    let Some(item) = (unsafe { item.as_mut() }) else {
        return;
    };
    free_c_string(&mut item.item_id);
    *item = PlayerFfiPlaylistActiveItem::default();
}

/// # Safety
///
/// Raw pointers and opaque handles passed to this FFI entry point must either be null when
/// the parameter is documented as optional or point to valid objects allocated by the
/// matching Vesper FFI API for the duration of the call. Callers must serialize shared
/// handle access according to the host binding contract.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn player_ffi_playlist_session_drain_preload_commands(
    handle: u64,
    out_commands: *mut PlayerFfiPreloadCommandList,
    out_error: *mut PlayerFfiError,
) -> PlayerFfiCallStatus {
    if out_commands.is_null() {
        write_error(
            out_error,
            owned_api_error(PlayerFfiErrorCode::NullPointer, "out_commands was null"),
        );
        return PlayerFfiCallStatus::Error;
    }

    let Ok(mut sessions) = playlist_sessions().lock() else {
        write_error(
            out_error,
            owned_api_error(
                PlayerFfiErrorCode::InvalidArgument,
                "playlist session registry lock failed",
            ),
        );
        return PlayerFfiCallStatus::Error;
    };
    let Some(session) = sessions.get_mut(handle) else {
        write_error(
            out_error,
            owned_api_error(
                PlayerFfiErrorCode::InvalidArgument,
                "invalid playlist session handle",
            ),
        );
        return PlayerFfiCallStatus::Error;
    };

    let commands = session
        .drain_commands()
        .into_iter()
        .map(PlayerFfiPreloadCommand::from)
        .collect::<Vec<_>>();
    let len = commands.len();
    let ptr = if len == 0 {
        ptr::null_mut()
    } else {
        Box::into_raw(commands.into_boxed_slice()) as *mut PlayerFfiPreloadCommand
    };
    unsafe {
        ptr::write(
            out_commands,
            PlayerFfiPreloadCommandList { commands: ptr, len },
        );
    }
    PlayerFfiCallStatus::Ok
}

/// # Safety
///
/// Raw pointers and opaque handles passed to this FFI entry point must either be null when
/// the parameter is documented as optional or point to valid objects allocated by the
/// matching Vesper FFI API for the duration of the call. Callers must serialize shared
/// handle access according to the host binding contract.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn player_ffi_playlist_session_complete_preload_task(
    handle: u64,
    task_id: u64,
    out_error: *mut PlayerFfiError,
) -> PlayerFfiCallStatus {
    let Ok(mut sessions) = playlist_sessions().lock() else {
        write_error(
            out_error,
            owned_api_error(
                PlayerFfiErrorCode::InvalidArgument,
                "playlist session registry lock failed",
            ),
        );
        return PlayerFfiCallStatus::Error;
    };
    let Some(session) = sessions.get_mut(handle) else {
        write_error(
            out_error,
            owned_api_error(
                PlayerFfiErrorCode::InvalidArgument,
                "invalid playlist session handle",
            ),
        );
        return PlayerFfiCallStatus::Error;
    };

    if let Err(error) =
        session.complete_preload_task(player_runtime::PreloadTaskId::from_raw(task_id))
    {
        write_error(out_error, runtime_error_to_ffi(error));
        return PlayerFfiCallStatus::Error;
    }
    PlayerFfiCallStatus::Ok
}

/// # Safety
///
/// Raw pointers and opaque handles passed to this FFI entry point must either be null when
/// the parameter is documented as optional or point to valid objects allocated by the
/// matching Vesper FFI API for the duration of the call. Callers must serialize shared
/// handle access according to the host binding contract.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn player_ffi_playlist_session_fail_preload_task(
    handle: u64,
    task_id: u64,
    code: u32,
    category: u32,
    retriable: bool,
    message: *const c_char,
    out_error: *mut PlayerFfiError,
) -> PlayerFfiCallStatus {
    let message = match read_optional_c_string(message, "message") {
        Ok(Some(value)) => value,
        Ok(None) => String::new(),
        Err(error) => {
            write_error(out_error, error);
            return PlayerFfiCallStatus::Error;
        }
    };

    let Ok(mut sessions) = playlist_sessions().lock() else {
        write_error(
            out_error,
            owned_api_error(
                PlayerFfiErrorCode::InvalidArgument,
                "playlist session registry lock failed",
            ),
        );
        return PlayerFfiCallStatus::Error;
    };
    let Some(session) = sessions.get_mut(handle) else {
        write_error(
            out_error,
            owned_api_error(
                PlayerFfiErrorCode::InvalidArgument,
                "invalid playlist session handle",
            ),
        );
        return PlayerFfiCallStatus::Error;
    };

    let error = PlayerRuntimeError::with_taxonomy(
        ffi_runtime_error_code(code),
        ffi_runtime_error_category(category),
        retriable,
        message,
    );
    if let Err(error) =
        session.fail_preload_task(player_runtime::PreloadTaskId::from_raw(task_id), error)
    {
        write_error(out_error, runtime_error_to_ffi(error));
        return PlayerFfiCallStatus::Error;
    }
    PlayerFfiCallStatus::Ok
}

/// # Safety
///
/// Raw pointers and opaque handles passed to this FFI entry point must either be null when
/// the parameter is documented as optional or point to valid objects allocated by the
/// matching Vesper FFI API for the duration of the call. Callers must serialize shared
/// handle access according to the host binding contract.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn player_ffi_resolve_track_preferences(
    track_preferences: *const PlayerFfiTrackPreferences,
    out_preferences: *mut PlayerFfiTrackPreferences,
    out_error: *mut PlayerFfiError,
) -> PlayerFfiCallStatus {
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

    let resolved = resolve_track_preferences_with_runtime(track_preferences);
    unsafe {
        ptr::write(out_preferences, resolved.into());
    }
    PlayerFfiCallStatus::Ok
}

/// # Safety
///
/// Raw pointers and opaque handles passed to this FFI entry point must either be null when
/// the parameter is documented as optional or point to valid objects allocated by the
/// matching Vesper FFI API for the duration of the call. Callers must serialize shared
/// handle access according to the host binding contract.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn player_ffi_benchmark_session_create(
    plugin_library_paths: *mut *mut c_char,
    plugin_library_paths_len: usize,
    out_handle: *mut u64,
    out_error: *mut PlayerFfiError,
) -> PlayerFfiCallStatus {
    if out_handle.is_null() {
        write_error(
            out_error,
            owned_api_error(PlayerFfiErrorCode::NullPointer, "out_handle was null"),
        );
        return PlayerFfiCallStatus::Error;
    }

    let plugin_library_paths = match read_string_list(
        plugin_library_paths,
        plugin_library_paths_len,
        "plugin_library_paths",
    ) {
        Ok(paths) => paths.into_iter().map(PathBuf::from).collect::<Vec<_>>(),
        Err(error) => {
            write_error(out_error, error);
            return PlayerFfiCallStatus::Error;
        }
    };

    let session = match BenchmarkSinkPluginSession::load_paths(plugin_library_paths) {
        Ok(session) => session,
        Err(error) => {
            write_error(
                out_error,
                owned_api_error(PlayerFfiErrorCode::InvalidArgument, &error.to_string()),
            );
            return PlayerFfiCallStatus::Error;
        }
    };

    let Ok(mut sessions) = benchmark_sessions().lock() else {
        write_error(
            out_error,
            owned_api_error(
                PlayerFfiErrorCode::InvalidArgument,
                "benchmark session registry lock failed",
            ),
        );
        return PlayerFfiCallStatus::Error;
    };
    let handle = sessions.insert(session);
    unsafe {
        ptr::write(out_handle, handle);
    }
    PlayerFfiCallStatus::Ok
}

/// # Safety
///
/// Raw pointers and opaque handles passed to this FFI entry point must either be null when
/// the parameter is documented as optional or point to valid objects allocated by the
/// matching Vesper FFI API for the duration of the call. Callers must serialize shared
/// handle access according to the host binding contract.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn player_ffi_benchmark_session_dispose(handle: u64) {
    if let Ok(mut sessions) = benchmark_sessions().lock() {
        sessions.remove(handle);
    }
}

/// # Safety
///
/// Raw pointers and opaque handles passed to this FFI entry point must either be null when
/// the parameter is documented as optional or point to valid objects allocated by the
/// matching Vesper FFI API for the duration of the call. Callers must serialize shared
/// handle access according to the host binding contract.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn player_ffi_benchmark_session_on_event_batch_json(
    handle: u64,
    batch_json: *const c_char,
    out_report_json: *mut *mut c_char,
    out_error: *mut PlayerFfiError,
) -> PlayerFfiCallStatus {
    if batch_json.is_null() {
        write_error(
            out_error,
            owned_api_error(PlayerFfiErrorCode::NullPointer, "batch_json was null"),
        );
        return PlayerFfiCallStatus::Error;
    }
    if out_report_json.is_null() {
        write_error(
            out_error,
            owned_api_error(PlayerFfiErrorCode::NullPointer, "out_report_json was null"),
        );
        return PlayerFfiCallStatus::Error;
    }

    let batch_json = match unsafe { CStr::from_ptr(batch_json) }.to_str() {
        Ok(value) => value,
        Err(_) => {
            write_error(
                out_error,
                owned_api_error(
                    PlayerFfiErrorCode::InvalidUtf8,
                    "batch_json was not valid UTF-8",
                ),
            );
            return PlayerFfiCallStatus::Error;
        }
    };

    let Ok(sessions) = benchmark_sessions().lock() else {
        write_error(
            out_error,
            owned_api_error(
                PlayerFfiErrorCode::InvalidArgument,
                "benchmark session registry lock failed",
            ),
        );
        return PlayerFfiCallStatus::Error;
    };
    let Some(session) = sessions.get(handle) else {
        write_error(
            out_error,
            owned_api_error(
                PlayerFfiErrorCode::InvalidArgument,
                "invalid benchmark session handle",
            ),
        );
        return PlayerFfiCallStatus::Error;
    };

    let report_json = match session.on_event_batch_report_json(batch_json) {
        Ok(value) => value,
        Err(error) => {
            write_error(
                out_error,
                owned_api_error(PlayerFfiErrorCode::InvalidArgument, &error.to_string()),
            );
            return PlayerFfiCallStatus::Error;
        }
    };

    unsafe {
        ptr::write(out_report_json, into_c_string_ptr(report_json));
    }
    PlayerFfiCallStatus::Ok
}

/// # Safety
///
/// Raw pointers and opaque handles passed to this FFI entry point must either be null when
/// the parameter is documented as optional or point to valid objects allocated by the
/// matching Vesper FFI API for the duration of the call. Callers must serialize shared
/// handle access according to the host binding contract.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn player_ffi_benchmark_session_flush_json(
    handle: u64,
    out_report_json: *mut *mut c_char,
    out_error: *mut PlayerFfiError,
) -> PlayerFfiCallStatus {
    if out_report_json.is_null() {
        write_error(
            out_error,
            owned_api_error(PlayerFfiErrorCode::NullPointer, "out_report_json was null"),
        );
        return PlayerFfiCallStatus::Error;
    }

    let Ok(sessions) = benchmark_sessions().lock() else {
        write_error(
            out_error,
            owned_api_error(
                PlayerFfiErrorCode::InvalidArgument,
                "benchmark session registry lock failed",
            ),
        );
        return PlayerFfiCallStatus::Error;
    };
    let Some(session) = sessions.get(handle) else {
        write_error(
            out_error,
            owned_api_error(
                PlayerFfiErrorCode::InvalidArgument,
                "invalid benchmark session handle",
            ),
        );
        return PlayerFfiCallStatus::Error;
    };

    let report_json = match session.flush_json() {
        Ok(value) => value,
        Err(error) => {
            write_error(
                out_error,
                owned_api_error(PlayerFfiErrorCode::InvalidArgument, &error.to_string()),
            );
            return PlayerFfiCallStatus::Error;
        }
    };

    unsafe {
        ptr::write(out_report_json, into_c_string_ptr(report_json));
    }
    PlayerFfiCallStatus::Ok
}

/// # Safety
///
/// Raw pointers and opaque handles passed to this FFI entry point must either be null when
/// the parameter is documented as optional or point to valid objects allocated by the
/// matching Vesper FFI API for the duration of the call. Callers must serialize shared
/// handle access according to the host binding contract.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn player_ffi_benchmark_report_string_free(value: *mut c_char) {
    let mut value = value;
    free_c_string(&mut value);
}

/// # Safety
///
/// Raw pointers and opaque handles passed to this FFI entry point must either be null when
/// the parameter is documented as optional or point to valid objects allocated by the
/// matching Vesper FFI API for the duration of the call. Callers must serialize shared
/// handle access according to the host binding contract.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn player_ffi_dash_bridge_execute_json(
    request_json: *const c_char,
    out_json: *mut *mut c_char,
    out_error: *mut PlayerFfiError,
) -> PlayerFfiCallStatus {
    if request_json.is_null() {
        write_error(
            out_error,
            owned_api_error(PlayerFfiErrorCode::NullPointer, "request_json was null"),
        );
        return PlayerFfiCallStatus::Error;
    }
    if out_json.is_null() {
        write_error(
            out_error,
            owned_api_error(PlayerFfiErrorCode::NullPointer, "out_json was null"),
        );
        return PlayerFfiCallStatus::Error;
    }

    let request_json = match unsafe { CStr::from_ptr(request_json) }.to_str() {
        Ok(value) => value,
        Err(_) => {
            write_error(
                out_error,
                owned_api_error(
                    PlayerFfiErrorCode::InvalidUtf8,
                    "request_json was not valid UTF-8",
                ),
            );
            return PlayerFfiCallStatus::Error;
        }
    };

    let response_json = match player_dash_hls_bridge::ops::execute_json(request_json) {
        Ok(value) => value,
        Err(error) => {
            write_error(
                out_error,
                owned_api_error(PlayerFfiErrorCode::InvalidArgument, &error.to_string()),
            );
            return PlayerFfiCallStatus::Error;
        }
    };

    unsafe {
        ptr::write(out_json, into_c_string_ptr(response_json));
    }
    PlayerFfiCallStatus::Ok
}

/// # Safety
///
/// Raw pointers and opaque handles passed to this FFI entry point must either be null when
/// the parameter is documented as optional or point to valid objects allocated by the
/// matching Vesper FFI API for the duration of the call. Callers must serialize shared
/// handle access according to the host binding contract.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn player_ffi_dash_bridge_string_free(value: *mut c_char) {
    let mut value = value;
    free_c_string(&mut value);
}

/// # Safety
///
/// Raw pointers and opaque handles passed to this FFI entry point must either be null when
/// the parameter is documented as optional or point to valid objects allocated by the
/// matching Vesper FFI API for the duration of the call. Callers must serialize shared
/// handle access according to the host binding contract.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn player_ffi_error_free(error: *mut PlayerFfiError) {
    let Some(error) = (unsafe { error.as_mut() }) else {
        return;
    };

    free_c_string(&mut error.message);
    *error = PlayerFfiError::default();
}

/// # Safety
///
/// Raw pointers and opaque handles passed to this FFI entry point must either be null when
/// the parameter is documented as optional or point to valid objects allocated by the
/// matching Vesper FFI API for the duration of the call. Callers must serialize shared
/// handle access according to the host binding contract.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn player_ffi_track_preferences_free(
    track_preferences: *mut PlayerFfiTrackPreferences,
) {
    let Some(track_preferences) = (unsafe { track_preferences.as_mut() }) else {
        return;
    };

    free_c_string(&mut track_preferences.preferred_audio_language);
    free_c_string(&mut track_preferences.preferred_subtitle_language);
    free_c_string(&mut track_preferences.audio_selection.track_id);
    free_c_string(&mut track_preferences.subtitle_selection.track_id);
    free_c_string(&mut track_preferences.abr_policy.track_id);
    *track_preferences = PlayerFfiTrackPreferences::default();
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
    selection: &PlayerFfiTrackSelection,
) -> Result<MediaTrackSelection, PlayerFfiError> {
    Ok(MediaTrackSelection {
        mode: selection.mode.into(),
        track_id: read_optional_c_string(selection.track_id, "selection.track_id")?,
    })
}

fn read_abr_policy(policy: &PlayerFfiAbrPolicy) -> Result<MediaAbrPolicy, PlayerFfiError> {
    Ok(MediaAbrPolicy {
        mode: policy.mode.into(),
        track_id: read_optional_c_string(policy.track_id, "policy.track_id")?,
        max_bit_rate: policy.has_max_bit_rate.then_some(policy.max_bit_rate),
        max_width: policy.has_max_width.then_some(policy.max_width),
        max_height: policy.has_max_height.then_some(policy.max_height),
    })
}

fn read_preload_budget(
    budget: *const PlayerFfiPreloadBudgetPolicy,
) -> Result<PlayerPreloadBudgetPolicy, PlayerFfiError> {
    let Some(budget) = (unsafe { budget.as_ref() }) else {
        return Err(owned_api_error(
            PlayerFfiErrorCode::NullPointer,
            "preload budget pointer was null",
        ));
    };

    Ok(PlayerPreloadBudgetPolicy {
        max_concurrent_tasks: budget
            .has_max_concurrent_tasks
            .then_some(budget.max_concurrent_tasks),
        max_memory_bytes: budget
            .has_max_memory_bytes
            .then_some(budget.max_memory_bytes),
        max_disk_bytes: budget.has_max_disk_bytes.then_some(budget.max_disk_bytes),
        warmup_window: budget
            .has_warmup_window_ms
            .then_some(Duration::from_millis(budget.warmup_window_ms)),
    })
}

fn read_preload_candidate(
    candidate: &PlayerFfiPreloadCandidate,
) -> Result<PreloadCandidate, PlayerFfiError> {
    let source_uri = read_optional_c_string(candidate.source_uri, "candidate.source_uri")?
        .ok_or_else(|| {
            owned_api_error(
                PlayerFfiErrorCode::NullPointer,
                "candidate.source_uri was null",
            )
        })?;
    let scope_id = read_optional_c_string(candidate.scope_id, "candidate.scope_id")?;
    let scope = match candidate.scope_kind {
        PlayerFfiPreloadScopeKind::App => PreloadBudgetScope::App,
        PlayerFfiPreloadScopeKind::Session => {
            PreloadBudgetScope::Session(scope_id.unwrap_or_default())
        }
        PlayerFfiPreloadScopeKind::Playlist => {
            PreloadBudgetScope::Playlist(scope_id.unwrap_or_default())
        }
    };

    Ok(PreloadCandidate {
        source: MediaSource::new(source_uri),
        scope,
        kind: candidate.candidate_kind.into(),
        selection_hint: candidate.selection_hint.into(),
        config: PreloadConfig {
            priority: candidate.priority.into(),
            ttl: candidate
                .has_ttl_ms
                .then_some(Duration::from_millis(candidate.ttl_ms)),
            expected_memory_bytes: candidate.expected_memory_bytes,
            expected_disk_bytes: candidate.expected_disk_bytes,
            warmup_window: candidate
                .has_warmup_window_ms
                .then_some(Duration::from_millis(candidate.warmup_window_ms)),
        },
    })
}

fn read_download_config(
    config: *const PlayerFfiDownloadConfig,
) -> Result<ResolvedDownloadConfig, PlayerFfiError> {
    let Some(config) = (unsafe { config.as_ref() }) else {
        return Err(owned_api_error(
            PlayerFfiErrorCode::NullPointer,
            "download config pointer was null",
        ));
    };
    Ok(ResolvedDownloadConfig {
        auto_start: config.auto_start,
        run_post_processors_on_completion: config.run_post_processors_on_completion,
        plugin_library_paths: read_string_list(
            config.plugin_library_paths,
            config.plugin_library_paths_len,
            "config.plugin_library_paths",
        )?
        .into_iter()
        .map(PathBuf::from)
        .collect(),
    })
}

fn read_download_source(
    source: *const PlayerFfiDownloadSource,
) -> Result<DownloadSource, PlayerFfiError> {
    let Some(source) = (unsafe { source.as_ref() }) else {
        return Err(owned_api_error(
            PlayerFfiErrorCode::NullPointer,
            "download source pointer was null",
        ));
    };
    let source_uri =
        read_optional_c_string(source.source_uri, "source.source_uri")?.ok_or_else(|| {
            owned_api_error(
                PlayerFfiErrorCode::NullPointer,
                "source.source_uri was null",
            )
        })?;

    let mut download_source =
        DownloadSource::new(MediaSource::new(source_uri), source.content_format.into());
    if let Some(manifest_uri) = read_optional_c_string(source.manifest_uri, "source.manifest_uri")?
        && !manifest_uri.is_empty()
    {
        download_source = download_source.with_manifest_uri(manifest_uri);
    }
    Ok(download_source)
}

fn read_string_list(
    values: *mut *mut c_char,
    len: usize,
    field_name: &str,
) -> Result<Vec<String>, PlayerFfiError> {
    if len == 0 {
        return Ok(Vec::new());
    }
    if values.is_null() {
        return Err(owned_api_error(
            PlayerFfiErrorCode::NullPointer,
            &format!("{field_name} was null"),
        ));
    }

    let values = unsafe { slice::from_raw_parts(values, len) };
    values
        .iter()
        .map(|value| read_optional_c_string(*value as *const c_char, field_name))
        .collect::<Result<Vec<_>, _>>()
        .map(|values| values.into_iter().flatten().collect())
}

fn read_download_profile(
    profile: *const PlayerFfiDownloadProfile,
) -> Result<DownloadProfile, PlayerFfiError> {
    let Some(profile) = (unsafe { profile.as_ref() }) else {
        return Err(owned_api_error(
            PlayerFfiErrorCode::NullPointer,
            "download profile pointer was null",
        ));
    };

    Ok(DownloadProfile {
        variant_id: read_optional_c_string(profile.variant_id, "profile.variant_id")?,
        preferred_audio_language: read_optional_c_string(
            profile.preferred_audio_language,
            "profile.preferred_audio_language",
        )?,
        preferred_subtitle_language: read_optional_c_string(
            profile.preferred_subtitle_language,
            "profile.preferred_subtitle_language",
        )?,
        selected_track_ids: read_string_list(
            profile.selected_track_ids,
            profile.selected_track_ids_len,
            "profile.selected_track_ids",
        )?,
        target_directory: read_optional_c_string(
            profile.target_directory,
            "profile.target_directory",
        )?
        .map(PathBuf::from),
        allow_metered_network: profile.allow_metered_network,
    })
}

fn read_download_resource_record(
    resource: &PlayerFfiDownloadResourceRecord,
) -> Result<DownloadResourceRecord, PlayerFfiError> {
    Ok(DownloadResourceRecord {
        resource_id: read_optional_c_string(resource.resource_id, "resource.resource_id")?
            .ok_or_else(|| {
                owned_api_error(
                    PlayerFfiErrorCode::NullPointer,
                    "resource.resource_id was null",
                )
            })?,
        uri: read_optional_c_string(resource.uri, "resource.uri")?.ok_or_else(|| {
            owned_api_error(PlayerFfiErrorCode::NullPointer, "resource.uri was null")
        })?,
        relative_path: read_optional_c_string(resource.relative_path, "resource.relative_path")?
            .map(PathBuf::from),
        size_bytes: resource.has_size_bytes.then_some(resource.size_bytes),
        etag: read_optional_c_string(resource.etag, "resource.etag")?,
        checksum: read_optional_c_string(resource.checksum, "resource.checksum")?,
    })
}

fn read_download_segment_record(
    segment: &PlayerFfiDownloadSegmentRecord,
) -> Result<DownloadSegmentRecord, PlayerFfiError> {
    Ok(DownloadSegmentRecord {
        segment_id: read_optional_c_string(segment.segment_id, "segment.segment_id")?.ok_or_else(
            || {
                owned_api_error(
                    PlayerFfiErrorCode::NullPointer,
                    "segment.segment_id was null",
                )
            },
        )?,
        uri: read_optional_c_string(segment.uri, "segment.uri")?.ok_or_else(|| {
            owned_api_error(PlayerFfiErrorCode::NullPointer, "segment.uri was null")
        })?,
        relative_path: read_optional_c_string(segment.relative_path, "segment.relative_path")?
            .map(PathBuf::from),
        sequence: segment.has_sequence.then_some(segment.sequence),
        size_bytes: segment.has_size_bytes.then_some(segment.size_bytes),
        checksum: read_optional_c_string(segment.checksum, "segment.checksum")?,
    })
}

fn read_download_asset_index(
    asset_index: *const PlayerFfiDownloadAssetIndex,
) -> Result<DownloadAssetIndex, PlayerFfiError> {
    let Some(asset_index) = (unsafe { asset_index.as_ref() }) else {
        return Err(owned_api_error(
            PlayerFfiErrorCode::NullPointer,
            "download asset_index pointer was null",
        ));
    };

    let resources = if asset_index.resources_len == 0 {
        Vec::new()
    } else {
        if asset_index.resources.is_null() {
            return Err(owned_api_error(
                PlayerFfiErrorCode::NullPointer,
                "asset_index.resources was null",
            ));
        }
        unsafe { slice::from_raw_parts(asset_index.resources, asset_index.resources_len) }
            .iter()
            .map(read_download_resource_record)
            .collect::<Result<Vec<_>, _>>()?
    };

    let segments = if asset_index.segments_len == 0 {
        Vec::new()
    } else {
        if asset_index.segments.is_null() {
            return Err(owned_api_error(
                PlayerFfiErrorCode::NullPointer,
                "asset_index.segments was null",
            ));
        }
        unsafe { slice::from_raw_parts(asset_index.segments, asset_index.segments_len) }
            .iter()
            .map(read_download_segment_record)
            .collect::<Result<Vec<_>, _>>()?
    };

    Ok(DownloadAssetIndex {
        content_format: asset_index.content_format.into(),
        version: read_optional_c_string(asset_index.version, "asset_index.version")?,
        etag: read_optional_c_string(asset_index.etag, "asset_index.etag")?,
        checksum: read_optional_c_string(asset_index.checksum, "asset_index.checksum")?,
        total_size_bytes: asset_index
            .has_total_size_bytes
            .then_some(asset_index.total_size_bytes),
        resources,
        segments,
        completed_path: read_optional_c_string(
            asset_index.completed_path,
            "asset_index.completed_path",
        )?
        .map(PathBuf::from),
    })
}

fn read_playlist_config(
    config: *const PlayerFfiPlaylistConfig,
) -> Result<(String, PlaylistCoordinatorConfig), PlayerFfiError> {
    let Some(config) = (unsafe { config.as_ref() }) else {
        return Err(owned_api_error(
            PlayerFfiErrorCode::NullPointer,
            "playlist config pointer was null",
        ));
    };

    let playlist_id = read_optional_c_string(config.playlist_id, "config.playlist_id")?
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| "ios-host-playlist".to_owned());

    Ok((
        playlist_id,
        PlaylistCoordinatorConfig {
            neighbor_window: PlaylistNeighborWindow {
                previous: config.neighbor_previous as usize,
                next: config.neighbor_next as usize,
            },
            preload_window: PlaylistPreloadWindow {
                near_visible: config.preload_near_visible as usize,
                prefetch_only: config.preload_prefetch_only as usize,
            },
            switch_policy: PlaylistSwitchPolicy {
                auto_advance: config.auto_advance,
                repeat_mode: match config.repeat_mode {
                    PlayerFfiPlaylistRepeatMode::Off => PlaylistRepeatMode::Off,
                    PlayerFfiPlaylistRepeatMode::One => PlaylistRepeatMode::One,
                    PlayerFfiPlaylistRepeatMode::All => PlaylistRepeatMode::All,
                },
                failure_strategy: match config.failure_strategy {
                    PlayerFfiPlaylistFailureStrategy::Pause => PlaylistFailureStrategy::Pause,
                    PlayerFfiPlaylistFailureStrategy::SkipToNext => {
                        PlaylistFailureStrategy::SkipToNext
                    }
                },
            },
        },
    ))
}

fn read_playlist_queue_item(
    item: &PlayerFfiPlaylistQueueItem,
) -> Result<PlaylistQueueItem, PlayerFfiError> {
    let item_id = read_optional_c_string(item.item_id, "item.item_id")?
        .ok_or_else(|| owned_api_error(PlayerFfiErrorCode::NullPointer, "item.item_id was null"))?;
    let source_uri =
        read_optional_c_string(item.source_uri, "item.source_uri")?.ok_or_else(|| {
            owned_api_error(PlayerFfiErrorCode::NullPointer, "item.source_uri was null")
        })?;

    Ok(
        PlaylistQueueItem::new(item_id, MediaSource::new(source_uri)).with_preload_profile(
            player_runtime::PlaylistItemPreloadProfile {
                expected_memory_bytes: item.expected_memory_bytes,
                expected_disk_bytes: item.expected_disk_bytes,
                ttl: item
                    .has_ttl_ms
                    .then_some(Duration::from_millis(item.ttl_ms)),
                warmup_window: item
                    .has_warmup_window_ms
                    .then_some(Duration::from_millis(item.warmup_window_ms)),
            },
        ),
    )
}

fn read_playlist_viewport_hint(
    hint: &PlayerFfiPlaylistViewportHint,
) -> Result<PlaylistViewportHint, PlayerFfiError> {
    let item_id = read_optional_c_string(hint.item_id, "hint.item_id")?
        .ok_or_else(|| owned_api_error(PlayerFfiErrorCode::NullPointer, "hint.item_id was null"))?;
    let kind = match hint.kind {
        PlayerFfiPlaylistViewportHintKind::Visible => PlaylistViewportHintKind::Visible,
        PlayerFfiPlaylistViewportHintKind::NearVisible => PlaylistViewportHintKind::NearVisible,
        PlayerFfiPlaylistViewportHintKind::PrefetchOnly => PlaylistViewportHintKind::PrefetchOnly,
        PlayerFfiPlaylistViewportHintKind::Hidden => PlaylistViewportHintKind::Hidden,
    };

    Ok(PlaylistViewportHint::new(item_id, kind).with_order(hint.order))
}

fn runtime_error_to_ffi(error: PlayerRuntimeError) -> PlayerFfiError {
    PlayerFfiError {
        code: PlayerFfiErrorCode::InvalidArgument,
        category: PlayerFfiErrorCategory::Platform,
        retriable: error.is_retriable(),
        message: into_c_string_ptr(error.message().to_owned()),
    }
}

fn ffi_runtime_error_code(value: u32) -> PlayerRuntimeErrorCode {
    match value {
        1 => PlayerRuntimeErrorCode::InvalidState,
        2 => PlayerRuntimeErrorCode::InvalidSource,
        3 => PlayerRuntimeErrorCode::BackendFailure,
        4 => PlayerRuntimeErrorCode::AudioOutputUnavailable,
        5 => PlayerRuntimeErrorCode::DecodeFailure,
        6 => PlayerRuntimeErrorCode::SeekFailure,
        7 => PlayerRuntimeErrorCode::Unsupported,
        _ => PlayerRuntimeErrorCode::InvalidArgument,
    }
}

fn ffi_runtime_error_category(value: u32) -> PlayerRuntimeErrorCategory {
    match value {
        1 => PlayerRuntimeErrorCategory::Source,
        2 => PlayerRuntimeErrorCategory::Network,
        3 => PlayerRuntimeErrorCategory::Decode,
        4 => PlayerRuntimeErrorCategory::AudioOutput,
        5 => PlayerRuntimeErrorCategory::Playback,
        6 => PlayerRuntimeErrorCategory::Capability,
        7 => PlayerRuntimeErrorCategory::Platform,
        _ => PlayerRuntimeErrorCategory::Input,
    }
}

fn preload_task_to_ffi(task: PreloadTaskSnapshot) -> PlayerFfiPreloadTask {
    let (scope_kind, scope_id) = match task.scope {
        PreloadBudgetScope::App => (PlayerFfiPreloadScopeKind::App, ptr::null_mut()),
        PreloadBudgetScope::Session(value) => {
            (PlayerFfiPreloadScopeKind::Session, into_c_string_ptr(value))
        }
        PreloadBudgetScope::Playlist(value) => (
            PlayerFfiPreloadScopeKind::Playlist,
            into_c_string_ptr(value),
        ),
    };
    let (has_error, error_code, error_category, error_retriable, error_message) =
        match task.error_summary {
            Some(error) => (
                true,
                error.code as u32,
                error.category as u32,
                error.retriable,
                into_c_string_ptr(error.message),
            ),
            None => (false, 0, 0, false, ptr::null_mut()),
        };

    PlayerFfiPreloadTask {
        task_id: task.task_id.get(),
        source_uri: into_c_string_ptr(task.source.uri().to_owned()),
        source_identity: into_c_string_ptr(task.source_identity.as_str().to_owned()),
        cache_key: into_c_string_ptr(task.cache_key.as_str().to_owned()),
        scope_kind,
        scope_id,
        candidate_kind: task.kind.into(),
        selection_hint: task.selection_hint.into(),
        priority: task.priority.into(),
        status: task.status.into(),
        expected_memory_bytes: task.expected_memory_bytes,
        expected_disk_bytes: task.expected_disk_bytes,
        warmup_window_ms: duration_to_millis_u64(task.warmup_window),
        has_error,
        error_code,
        error_category,
        error_retriable,
        error_message,
    }
}

fn into_c_string_list(values: Vec<String>) -> (*mut *mut c_char, usize) {
    let len = values.len();
    if len == 0 {
        return (ptr::null_mut(), 0);
    }

    let ptrs = values
        .into_iter()
        .map(into_c_string_ptr)
        .collect::<Vec<_>>()
        .into_boxed_slice();
    (Box::into_raw(ptrs) as *mut *mut c_char, len)
}

fn download_source_to_ffi(source: DownloadSource) -> PlayerFfiDownloadSource {
    PlayerFfiDownloadSource {
        source_uri: into_c_string_ptr(source.source.uri().to_owned()),
        content_format: source.content_format.into(),
        manifest_uri: source
            .manifest_uri
            .map(into_c_string_ptr)
            .unwrap_or(ptr::null_mut()),
    }
}

fn download_profile_to_ffi(profile: DownloadProfile) -> PlayerFfiDownloadProfile {
    let (selected_track_ids, selected_track_ids_len) =
        into_c_string_list(profile.selected_track_ids);
    PlayerFfiDownloadProfile {
        variant_id: profile
            .variant_id
            .map(into_c_string_ptr)
            .unwrap_or(ptr::null_mut()),
        preferred_audio_language: profile
            .preferred_audio_language
            .map(into_c_string_ptr)
            .unwrap_or(ptr::null_mut()),
        preferred_subtitle_language: profile
            .preferred_subtitle_language
            .map(into_c_string_ptr)
            .unwrap_or(ptr::null_mut()),
        selected_track_ids,
        selected_track_ids_len,
        target_directory: profile
            .target_directory
            .map(|path| into_c_string_ptr(path.to_string_lossy().into_owned()))
            .unwrap_or(ptr::null_mut()),
        allow_metered_network: profile.allow_metered_network,
    }
}

fn download_resource_record_to_ffi(
    resource: DownloadResourceRecord,
) -> PlayerFfiDownloadResourceRecord {
    PlayerFfiDownloadResourceRecord {
        resource_id: into_c_string_ptr(resource.resource_id),
        uri: into_c_string_ptr(resource.uri),
        relative_path: resource
            .relative_path
            .map(|path| into_c_string_ptr(path.to_string_lossy().into_owned()))
            .unwrap_or(ptr::null_mut()),
        has_size_bytes: resource.size_bytes.is_some(),
        size_bytes: resource.size_bytes.unwrap_or_default(),
        etag: resource
            .etag
            .map(into_c_string_ptr)
            .unwrap_or(ptr::null_mut()),
        checksum: resource
            .checksum
            .map(into_c_string_ptr)
            .unwrap_or(ptr::null_mut()),
    }
}

fn download_segment_record_to_ffi(
    segment: DownloadSegmentRecord,
) -> PlayerFfiDownloadSegmentRecord {
    PlayerFfiDownloadSegmentRecord {
        segment_id: into_c_string_ptr(segment.segment_id),
        uri: into_c_string_ptr(segment.uri),
        relative_path: segment
            .relative_path
            .map(|path| into_c_string_ptr(path.to_string_lossy().into_owned()))
            .unwrap_or(ptr::null_mut()),
        has_sequence: segment.sequence.is_some(),
        sequence: segment.sequence.unwrap_or_default(),
        has_size_bytes: segment.size_bytes.is_some(),
        size_bytes: segment.size_bytes.unwrap_or_default(),
        checksum: segment
            .checksum
            .map(into_c_string_ptr)
            .unwrap_or(ptr::null_mut()),
    }
}

fn download_asset_index_to_ffi(asset_index: DownloadAssetIndex) -> PlayerFfiDownloadAssetIndex {
    let resources = asset_index
        .resources
        .into_iter()
        .map(download_resource_record_to_ffi)
        .collect::<Vec<_>>();
    let resources_len = resources.len();
    let resources = if resources_len == 0 {
        ptr::null_mut()
    } else {
        Box::into_raw(resources.into_boxed_slice()) as *mut PlayerFfiDownloadResourceRecord
    };

    let segments = asset_index
        .segments
        .into_iter()
        .map(download_segment_record_to_ffi)
        .collect::<Vec<_>>();
    let segments_len = segments.len();
    let segments = if segments_len == 0 {
        ptr::null_mut()
    } else {
        Box::into_raw(segments.into_boxed_slice()) as *mut PlayerFfiDownloadSegmentRecord
    };

    PlayerFfiDownloadAssetIndex {
        content_format: asset_index.content_format.into(),
        version: asset_index
            .version
            .map(into_c_string_ptr)
            .unwrap_or(ptr::null_mut()),
        etag: asset_index
            .etag
            .map(into_c_string_ptr)
            .unwrap_or(ptr::null_mut()),
        checksum: asset_index
            .checksum
            .map(into_c_string_ptr)
            .unwrap_or(ptr::null_mut()),
        has_total_size_bytes: asset_index.total_size_bytes.is_some(),
        total_size_bytes: asset_index.total_size_bytes.unwrap_or_default(),
        resources,
        resources_len,
        segments,
        segments_len,
        completed_path: asset_index
            .completed_path
            .map(|path| into_c_string_ptr(path.to_string_lossy().into_owned()))
            .unwrap_or(ptr::null_mut()),
    }
}

fn download_progress_to_ffi(
    progress: DownloadProgressSnapshot,
) -> PlayerFfiDownloadProgressSnapshot {
    PlayerFfiDownloadProgressSnapshot {
        received_bytes: progress.received_bytes,
        has_total_bytes: progress.total_bytes.is_some(),
        total_bytes: progress.total_bytes.unwrap_or_default(),
        received_segments: progress.received_segments,
        has_total_segments: progress.total_segments.is_some(),
        total_segments: progress.total_segments.unwrap_or_default(),
    }
}

fn download_task_to_ffi(task: DownloadTaskSnapshot) -> PlayerFfiDownloadTask {
    let (has_error, error_code, error_category, error_retriable, error_message) =
        match task.error_summary {
            Some(error) => (
                true,
                error.code as u32,
                error.category as u32,
                error.retriable,
                into_c_string_ptr(error.message),
            ),
            None => (false, 0, 0, false, ptr::null_mut()),
        };

    PlayerFfiDownloadTask {
        task_id: task.task_id.get(),
        asset_id: into_c_string_ptr(task.asset_id.as_str().to_owned()),
        source: download_source_to_ffi(task.source),
        profile: download_profile_to_ffi(task.profile),
        status: task.status.into(),
        progress: download_progress_to_ffi(task.progress),
        asset_index: download_asset_index_to_ffi(task.asset_index),
        has_error,
        error_code,
        error_category,
        error_retriable,
        error_message,
    }
}

fn playlist_active_item_to_ffi(item: PlaylistActiveItem) -> PlayerFfiPlaylistActiveItem {
    PlayerFfiPlaylistActiveItem {
        item_id: into_c_string_ptr(item.item_id.as_str().to_owned()),
        index: item.index.min(u32::MAX as usize) as u32,
    }
}

fn preload_command_free(command: &mut PlayerFfiPreloadCommand) {
    preload_task_free(&mut command.task);
    *command = PlayerFfiPreloadCommand::default();
}

fn download_command_free(command: &mut PlayerFfiDownloadCommand) {
    download_task_free(&mut command.task);
    *command = PlayerFfiDownloadCommand::default();
}

fn download_event_free(event: &mut PlayerFfiDownloadEvent) {
    download_task_free(&mut event.task);
    *event = PlayerFfiDownloadEvent::default();
}

fn preload_task_free(task: &mut PlayerFfiPreloadTask) {
    free_c_string(&mut task.source_uri);
    free_c_string(&mut task.source_identity);
    free_c_string(&mut task.cache_key);
    free_c_string(&mut task.scope_id);
    free_c_string(&mut task.error_message);
    *task = PlayerFfiPreloadTask::default();
}

fn free_c_string_list(values: &mut *mut *mut c_char, len: &mut usize) {
    if !(*values).is_null() && *len > 0 {
        let items = unsafe { Vec::from_raw_parts(*values, *len, *len) };
        for mut value in items {
            free_c_string(&mut value);
        }
    }
    *values = ptr::null_mut();
    *len = 0;
}

fn download_profile_free(profile: &mut PlayerFfiDownloadProfile) {
    free_c_string(&mut profile.variant_id);
    free_c_string(&mut profile.preferred_audio_language);
    free_c_string(&mut profile.preferred_subtitle_language);
    free_c_string_list(
        &mut profile.selected_track_ids,
        &mut profile.selected_track_ids_len,
    );
    free_c_string(&mut profile.target_directory);
    *profile = PlayerFfiDownloadProfile::default();
}

fn download_source_free(source: &mut PlayerFfiDownloadSource) {
    free_c_string(&mut source.source_uri);
    free_c_string(&mut source.manifest_uri);
    *source = PlayerFfiDownloadSource::default();
}

fn download_resource_record_free(resource: &mut PlayerFfiDownloadResourceRecord) {
    free_c_string(&mut resource.resource_id);
    free_c_string(&mut resource.uri);
    free_c_string(&mut resource.relative_path);
    free_c_string(&mut resource.etag);
    free_c_string(&mut resource.checksum);
    *resource = PlayerFfiDownloadResourceRecord::default();
}

fn download_segment_record_free(segment: &mut PlayerFfiDownloadSegmentRecord) {
    free_c_string(&mut segment.segment_id);
    free_c_string(&mut segment.uri);
    free_c_string(&mut segment.relative_path);
    free_c_string(&mut segment.checksum);
    *segment = PlayerFfiDownloadSegmentRecord::default();
}

fn download_asset_index_free(asset_index: &mut PlayerFfiDownloadAssetIndex) {
    free_c_string(&mut asset_index.version);
    free_c_string(&mut asset_index.etag);
    free_c_string(&mut asset_index.checksum);
    free_c_string(&mut asset_index.completed_path);

    if !asset_index.resources.is_null() && asset_index.resources_len > 0 {
        let resources = unsafe {
            Vec::from_raw_parts(
                asset_index.resources,
                asset_index.resources_len,
                asset_index.resources_len,
            )
        };
        for mut resource in resources {
            download_resource_record_free(&mut resource);
        }
    }
    if !asset_index.segments.is_null() && asset_index.segments_len > 0 {
        let segments = unsafe {
            Vec::from_raw_parts(
                asset_index.segments,
                asset_index.segments_len,
                asset_index.segments_len,
            )
        };
        for mut segment in segments {
            download_segment_record_free(&mut segment);
        }
    }
    *asset_index = PlayerFfiDownloadAssetIndex::default();
}

fn download_task_free(task: &mut PlayerFfiDownloadTask) {
    free_c_string(&mut task.asset_id);
    download_source_free(&mut task.source);
    download_profile_free(&mut task.profile);
    download_asset_index_free(&mut task.asset_index);
    free_c_string(&mut task.error_message);
    *task = PlayerFfiDownloadTask::default();
}

fn read_track_preferences(
    preferences: *const PlayerFfiTrackPreferences,
) -> Result<PlayerTrackPreferencePolicy, PlayerFfiError> {
    let Some(preferences) = (unsafe { preferences.as_ref() }) else {
        return Err(owned_api_error(
            PlayerFfiErrorCode::NullPointer,
            "track preferences pointer was null",
        ));
    };

    Ok(PlayerTrackPreferencePolicy {
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
) -> Result<PlayerBufferingPolicy, PlayerFfiError> {
    let Some(policy) = (unsafe { policy.as_ref() }) else {
        return Err(owned_api_error(
            PlayerFfiErrorCode::NullPointer,
            "buffering policy pointer was null",
        ));
    };

    Ok(PlayerBufferingPolicy {
        preset: policy.preset.into(),
        min_buffer: policy
            .has_min_buffer_ms
            .then_some(Duration::from_millis(policy.min_buffer_ms)),
        max_buffer: policy
            .has_max_buffer_ms
            .then_some(Duration::from_millis(policy.max_buffer_ms)),
        buffer_for_playback: policy
            .has_buffer_for_playback_ms
            .then_some(Duration::from_millis(policy.buffer_for_playback_ms)),
        buffer_for_rebuffer: policy
            .has_buffer_for_rebuffer_ms
            .then_some(Duration::from_millis(policy.buffer_for_rebuffer_ms)),
    })
}

fn read_retry_policy(
    policy: *const PlayerFfiRetryPolicy,
) -> Result<PlayerRetryPolicy, PlayerFfiError> {
    let Some(policy) = (unsafe { policy.as_ref() }) else {
        return Err(owned_api_error(
            PlayerFfiErrorCode::NullPointer,
            "retry policy pointer was null",
        ));
    };

    Ok(PlayerRetryPolicy {
        max_attempts: if policy.uses_default_max_attempts {
            Some(3)
        } else if policy.has_max_attempts {
            Some(policy.max_attempts)
        } else {
            None
        },
        base_delay: if policy.has_base_delay_ms {
            Duration::from_millis(policy.base_delay_ms)
        } else {
            Duration::from_millis(1_000)
        },
        max_delay: if policy.has_max_delay_ms {
            Duration::from_millis(policy.max_delay_ms)
        } else {
            Duration::from_millis(5_000)
        },
        backoff: if policy.has_backoff {
            policy.backoff.into()
        } else {
            PlayerRetryBackoff::Linear
        },
    })
}

fn read_cache_policy(
    policy: *const PlayerFfiCachePolicy,
) -> Result<PlayerCachePolicy, PlayerFfiError> {
    let Some(policy) = (unsafe { policy.as_ref() }) else {
        return Err(owned_api_error(
            PlayerFfiErrorCode::NullPointer,
            "cache policy pointer was null",
        ));
    };

    Ok(PlayerCachePolicy {
        preset: policy.preset.into(),
        max_memory_bytes: policy
            .has_max_memory_bytes
            .then_some(policy.max_memory_bytes),
        max_disk_bytes: policy.has_max_disk_bytes.then_some(policy.max_disk_bytes),
    })
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
        PlayerFfiErrorCode::None => PlayerFfiErrorCategory::Platform,
    }
}

fn into_c_string_ptr(value: String) -> *mut c_char {
    CString::new(value).unwrap_or_default().into_raw()
}

fn free_c_string(value: &mut *mut c_char) {
    if value.is_null() {
        return;
    }

    unsafe {
        let raw = ptr::replace(value, ptr::null_mut());
        if !raw.is_null() {
            let _ = CString::from_raw(raw);
        }
    }
}

fn write_error(out_error: *mut PlayerFfiError, mut error: PlayerFfiError) {
    if out_error.is_null() {
        free_c_string(&mut error.message);
        return;
    }

    unsafe {
        ptr::write(out_error, error);
    }
}

impl From<PlayerFfiMediaSourceKind> for MediaSourceKind {
    fn from(value: PlayerFfiMediaSourceKind) -> Self {
        match value {
            PlayerFfiMediaSourceKind::Local => Self::Local,
            PlayerFfiMediaSourceKind::Remote => Self::Remote,
        }
    }
}

impl From<PlayerFfiMediaSourceProtocol> for MediaSourceProtocol {
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

impl From<PlayerFfiBufferingPreset> for PlayerBufferingPreset {
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

impl From<PlayerBufferingPreset> for PlayerFfiBufferingPreset {
    fn from(value: PlayerBufferingPreset) -> Self {
        match value {
            PlayerBufferingPreset::Default => Self::Default,
            PlayerBufferingPreset::Balanced => Self::Balanced,
            PlayerBufferingPreset::Streaming => Self::Streaming,
            PlayerBufferingPreset::Resilient => Self::Resilient,
            PlayerBufferingPreset::LowLatency => Self::LowLatency,
        }
    }
}

impl From<PlayerFfiRetryBackoff> for PlayerRetryBackoff {
    fn from(value: PlayerFfiRetryBackoff) -> Self {
        match value {
            PlayerFfiRetryBackoff::Fixed => Self::Fixed,
            PlayerFfiRetryBackoff::Linear => Self::Linear,
            PlayerFfiRetryBackoff::Exponential => Self::Exponential,
        }
    }
}

impl From<PlayerRetryBackoff> for PlayerFfiRetryBackoff {
    fn from(value: PlayerRetryBackoff) -> Self {
        match value {
            PlayerRetryBackoff::Fixed => Self::Fixed,
            PlayerRetryBackoff::Linear => Self::Linear,
            PlayerRetryBackoff::Exponential => Self::Exponential,
        }
    }
}

impl From<PlayerFfiCachePreset> for PlayerCachePreset {
    fn from(value: PlayerFfiCachePreset) -> Self {
        match value {
            PlayerFfiCachePreset::Default => Self::Default,
            PlayerFfiCachePreset::Disabled => Self::Disabled,
            PlayerFfiCachePreset::Streaming => Self::Streaming,
            PlayerFfiCachePreset::Resilient => Self::Resilient,
        }
    }
}

impl From<PlayerCachePreset> for PlayerFfiCachePreset {
    fn from(value: PlayerCachePreset) -> Self {
        match value {
            PlayerCachePreset::Default => Self::Default,
            PlayerCachePreset::Disabled => Self::Disabled,
            PlayerCachePreset::Streaming => Self::Streaming,
            PlayerCachePreset::Resilient => Self::Resilient,
        }
    }
}

impl From<PlayerFfiTrackSelectionMode> for MediaTrackSelectionMode {
    fn from(value: PlayerFfiTrackSelectionMode) -> Self {
        match value {
            PlayerFfiTrackSelectionMode::Auto => Self::Auto,
            PlayerFfiTrackSelectionMode::Disabled => Self::Disabled,
            PlayerFfiTrackSelectionMode::Track => Self::Track,
        }
    }
}

impl From<MediaTrackSelectionMode> for PlayerFfiTrackSelectionMode {
    fn from(value: MediaTrackSelectionMode) -> Self {
        match value {
            MediaTrackSelectionMode::Auto => Self::Auto,
            MediaTrackSelectionMode::Disabled => Self::Disabled,
            MediaTrackSelectionMode::Track => Self::Track,
        }
    }
}

impl From<PlayerFfiAbrMode> for MediaAbrMode {
    fn from(value: PlayerFfiAbrMode) -> Self {
        match value {
            PlayerFfiAbrMode::Auto => Self::Auto,
            PlayerFfiAbrMode::Constrained => Self::Constrained,
            PlayerFfiAbrMode::FixedTrack => Self::FixedTrack,
        }
    }
}

impl From<PlayerFfiPreloadCandidateKind> for PreloadCandidateKind {
    fn from(value: PlayerFfiPreloadCandidateKind) -> Self {
        match value {
            PlayerFfiPreloadCandidateKind::Current => Self::Current,
            PlayerFfiPreloadCandidateKind::Neighbor => Self::Neighbor,
            PlayerFfiPreloadCandidateKind::Recommended => Self::Recommended,
            PlayerFfiPreloadCandidateKind::Background => Self::Background,
        }
    }
}

impl From<PlayerFfiPreloadSelectionHint> for PreloadSelectionHint {
    fn from(value: PlayerFfiPreloadSelectionHint) -> Self {
        match value {
            PlayerFfiPreloadSelectionHint::None => Self::None,
            PlayerFfiPreloadSelectionHint::CurrentItem => Self::CurrentItem,
            PlayerFfiPreloadSelectionHint::NeighborItem => Self::NeighborItem,
            PlayerFfiPreloadSelectionHint::RecommendedItem => Self::RecommendedItem,
            PlayerFfiPreloadSelectionHint::BackgroundFill => Self::BackgroundFill,
        }
    }
}

impl From<PlayerFfiPreloadPriority> for PreloadPriority {
    fn from(value: PlayerFfiPreloadPriority) -> Self {
        match value {
            PlayerFfiPreloadPriority::Critical => Self::Critical,
            PlayerFfiPreloadPriority::High => Self::High,
            PlayerFfiPreloadPriority::Normal => Self::Normal,
            PlayerFfiPreloadPriority::Low => Self::Low,
            PlayerFfiPreloadPriority::Background => Self::Background,
        }
    }
}

impl From<player_runtime::PreloadCandidateKind> for PlayerFfiPreloadCandidateKind {
    fn from(value: player_runtime::PreloadCandidateKind) -> Self {
        match value {
            player_runtime::PreloadCandidateKind::Current => Self::Current,
            player_runtime::PreloadCandidateKind::Neighbor => Self::Neighbor,
            player_runtime::PreloadCandidateKind::Recommended => Self::Recommended,
            player_runtime::PreloadCandidateKind::Background => Self::Background,
        }
    }
}

impl From<player_runtime::PreloadSelectionHint> for PlayerFfiPreloadSelectionHint {
    fn from(value: player_runtime::PreloadSelectionHint) -> Self {
        match value {
            player_runtime::PreloadSelectionHint::None => Self::None,
            player_runtime::PreloadSelectionHint::CurrentItem => Self::CurrentItem,
            player_runtime::PreloadSelectionHint::NeighborItem => Self::NeighborItem,
            player_runtime::PreloadSelectionHint::RecommendedItem => Self::RecommendedItem,
            player_runtime::PreloadSelectionHint::BackgroundFill => Self::BackgroundFill,
        }
    }
}

impl From<player_runtime::PreloadPriority> for PlayerFfiPreloadPriority {
    fn from(value: player_runtime::PreloadPriority) -> Self {
        match value {
            player_runtime::PreloadPriority::Critical => Self::Critical,
            player_runtime::PreloadPriority::High => Self::High,
            player_runtime::PreloadPriority::Normal => Self::Normal,
            player_runtime::PreloadPriority::Low => Self::Low,
            player_runtime::PreloadPriority::Background => Self::Background,
        }
    }
}

impl From<player_runtime::PreloadTaskStatus> for PlayerFfiPreloadTaskStatus {
    fn from(value: player_runtime::PreloadTaskStatus) -> Self {
        match value {
            player_runtime::PreloadTaskStatus::Planned => Self::Planned,
            player_runtime::PreloadTaskStatus::Active => Self::Active,
            player_runtime::PreloadTaskStatus::Cancelled => Self::Cancelled,
            player_runtime::PreloadTaskStatus::Completed => Self::Completed,
            player_runtime::PreloadTaskStatus::Expired => Self::Expired,
            player_runtime::PreloadTaskStatus::Failed => Self::Failed,
        }
    }
}

impl From<PlayerFfiDownloadContentFormat> for DownloadContentFormat {
    fn from(value: PlayerFfiDownloadContentFormat) -> Self {
        match value {
            PlayerFfiDownloadContentFormat::HlsSegments => Self::HlsSegments,
            PlayerFfiDownloadContentFormat::DashSegments => Self::DashSegments,
            PlayerFfiDownloadContentFormat::SingleFile => Self::SingleFile,
            PlayerFfiDownloadContentFormat::Unknown => Self::Unknown,
        }
    }
}

impl From<DownloadContentFormat> for PlayerFfiDownloadContentFormat {
    fn from(value: DownloadContentFormat) -> Self {
        match value {
            DownloadContentFormat::HlsSegments => Self::HlsSegments,
            DownloadContentFormat::DashSegments => Self::DashSegments,
            DownloadContentFormat::SingleFile => Self::SingleFile,
            DownloadContentFormat::Unknown => Self::Unknown,
        }
    }
}

impl From<DownloadTaskStatus> for PlayerFfiDownloadTaskStatus {
    fn from(value: DownloadTaskStatus) -> Self {
        match value {
            DownloadTaskStatus::Queued => Self::Queued,
            DownloadTaskStatus::Preparing => Self::Preparing,
            DownloadTaskStatus::Downloading => Self::Downloading,
            DownloadTaskStatus::Paused => Self::Paused,
            DownloadTaskStatus::Completed => Self::Completed,
            DownloadTaskStatus::Failed => Self::Failed,
            DownloadTaskStatus::Removed => Self::Removed,
        }
    }
}

impl From<IosPreloadCommand> for PlayerFfiPreloadCommand {
    fn from(value: IosPreloadCommand) -> Self {
        match value {
            IosPreloadCommand::Start { task } => Self {
                kind: PlayerFfiPreloadCommandKind::Start,
                task: preload_task_to_ffi(task),
                task_id: 0,
            },
            IosPreloadCommand::Cancel { task_id } => Self {
                kind: PlayerFfiPreloadCommandKind::Cancel,
                task: PlayerFfiPreloadTask::default(),
                task_id: task_id.get(),
            },
        }
    }
}

impl From<IosDownloadCommand> for PlayerFfiDownloadCommand {
    fn from(value: IosDownloadCommand) -> Self {
        match value {
            IosDownloadCommand::Start { task } => Self {
                kind: PlayerFfiDownloadCommandKind::Start,
                task: download_task_to_ffi(task),
                task_id: 0,
            },
            IosDownloadCommand::Pause { task_id } => Self {
                kind: PlayerFfiDownloadCommandKind::Pause,
                task: PlayerFfiDownloadTask::default(),
                task_id: task_id.get(),
            },
            IosDownloadCommand::Resume { task } => Self {
                kind: PlayerFfiDownloadCommandKind::Resume,
                task: download_task_to_ffi(task),
                task_id: 0,
            },
            IosDownloadCommand::Remove { task_id } => Self {
                kind: PlayerFfiDownloadCommandKind::Remove,
                task: PlayerFfiDownloadTask::default(),
                task_id: task_id.get(),
            },
        }
    }
}

impl From<DownloadEvent> for PlayerFfiDownloadEvent {
    fn from(value: DownloadEvent) -> Self {
        match value {
            DownloadEvent::Created(task) => Self {
                kind: PlayerFfiDownloadEventKind::Created,
                task: download_task_to_ffi(task),
            },
            DownloadEvent::StateChanged(task) => Self {
                kind: PlayerFfiDownloadEventKind::StateChanged,
                task: download_task_to_ffi(task),
            },
            DownloadEvent::ProgressUpdated(task) => Self {
                kind: PlayerFfiDownloadEventKind::ProgressUpdated,
                task: download_task_to_ffi(task),
            },
        }
    }
}

impl From<MediaAbrMode> for PlayerFfiAbrMode {
    fn from(value: MediaAbrMode) -> Self {
        match value {
            MediaAbrMode::Auto => Self::Auto,
            MediaAbrMode::Constrained => Self::Constrained,
            MediaAbrMode::FixedTrack => Self::FixedTrack,
        }
    }
}

impl From<player_runtime::PlayerResolvedResiliencePolicy> for PlayerFfiResolvedResiliencePolicy {
    fn from(value: player_runtime::PlayerResolvedResiliencePolicy) -> Self {
        Self {
            buffering: PlayerFfiBufferingPolicy {
                preset: value.buffering_policy.preset.into(),
                has_min_buffer_ms: value.buffering_policy.min_buffer.is_some(),
                min_buffer_ms: value
                    .buffering_policy
                    .min_buffer
                    .map(duration_to_millis_u64)
                    .unwrap_or_default(),
                has_max_buffer_ms: value.buffering_policy.max_buffer.is_some(),
                max_buffer_ms: value
                    .buffering_policy
                    .max_buffer
                    .map(duration_to_millis_u64)
                    .unwrap_or_default(),
                has_buffer_for_playback_ms: value.buffering_policy.buffer_for_playback.is_some(),
                buffer_for_playback_ms: value
                    .buffering_policy
                    .buffer_for_playback
                    .map(duration_to_millis_u64)
                    .unwrap_or_default(),
                has_buffer_for_rebuffer_ms: value.buffering_policy.buffer_for_rebuffer.is_some(),
                buffer_for_rebuffer_ms: value
                    .buffering_policy
                    .buffer_for_rebuffer
                    .map(duration_to_millis_u64)
                    .unwrap_or_default(),
            },
            retry: PlayerFfiRetryPolicy {
                uses_default_max_attempts: value.retry_policy.max_attempts == Some(3),
                has_max_attempts: value.retry_policy.max_attempts.is_some(),
                max_attempts: value.retry_policy.max_attempts.unwrap_or_default(),
                has_base_delay_ms: true,
                base_delay_ms: duration_to_millis_u64(value.retry_policy.base_delay),
                has_max_delay_ms: true,
                max_delay_ms: duration_to_millis_u64(value.retry_policy.max_delay),
                has_backoff: true,
                backoff: value.retry_policy.backoff.into(),
            },
            cache: PlayerFfiCachePolicy {
                preset: value.cache_policy.preset.into(),
                has_max_memory_bytes: value.cache_policy.max_memory_bytes.is_some(),
                max_memory_bytes: value.cache_policy.max_memory_bytes.unwrap_or_default(),
                has_max_disk_bytes: value.cache_policy.max_disk_bytes.is_some(),
                max_disk_bytes: value.cache_policy.max_disk_bytes.unwrap_or_default(),
            },
        }
    }
}

impl From<player_runtime::PlayerResolvedPreloadBudgetPolicy>
    for PlayerFfiResolvedPreloadBudgetPolicy
{
    fn from(value: player_runtime::PlayerResolvedPreloadBudgetPolicy) -> Self {
        Self {
            max_concurrent_tasks: value.max_concurrent_tasks,
            max_memory_bytes: value.max_memory_bytes,
            max_disk_bytes: value.max_disk_bytes,
            warmup_window_ms: duration_to_millis_u64(value.warmup_window),
        }
    }
}

impl From<PlayerTrackPreferencePolicy> for PlayerFfiTrackPreferences {
    fn from(value: PlayerTrackPreferencePolicy) -> Self {
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

impl From<MediaTrackSelection> for PlayerFfiTrackSelection {
    fn from(value: MediaTrackSelection) -> Self {
        Self {
            mode: value.mode.into(),
            track_id: value
                .track_id
                .map(into_c_string_ptr)
                .unwrap_or(ptr::null_mut()),
        }
    }
}

impl From<MediaAbrPolicy> for PlayerFfiAbrPolicy {
    fn from(value: MediaAbrPolicy) -> Self {
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

fn duration_to_millis_u64(duration: Duration) -> u64 {
    duration.as_millis().min(u128::from(u64::MAX)) as u64
}

#[cfg(test)]
mod tests {
    use super::HandleRegistry;

    #[test]
    fn ffi_handle_registry_reuses_slot_with_new_generation_and_rejects_stale_handle() {
        let mut registry = HandleRegistry::default();
        let first = registry.insert(7_u32);

        assert_eq!(registry.get(first), Some(&7));
        assert_eq!(registry.remove(first), Some(7));

        let second = registry.insert(9_u32);
        assert_ne!(first, second);
        assert!(registry.get(first).is_none());
        assert_eq!(registry.get(second), Some(&9));
    }
}
