mod adapter;
mod download;
mod error;
mod playlist;
mod preload;

use std::sync::OnceLock;
use std::time::{Duration, Instant};

use player_core::MediaSource;

pub use adapter::{
    PlayerRuntimeAdapter, PlayerRuntimeAdapterBootstrap, PlayerRuntimeAdapterFactory,
    PlayerRuntimeAdapterInitializer,
};
pub use download::{
    DownloadAssetId, DownloadAssetIndex, DownloadContentFormat, DownloadErrorSummary,
    DownloadEvent, DownloadExecutor, DownloadManager, DownloadManagerConfig, DownloadProfile,
    DownloadProgressSnapshot, DownloadResourceRecord, DownloadSegmentRecord, DownloadSnapshot,
    DownloadSource, DownloadStore, DownloadTaskId, DownloadTaskSnapshot, DownloadTaskState,
    DownloadTaskStatus, InMemoryDownloadExecutor, InMemoryDownloadStore,
};
pub use error::{
    PlayerRuntimeError, PlayerRuntimeErrorCategory, PlayerRuntimeErrorCode, PlayerRuntimeResult,
};
pub use player_core::{
    DecodedVideoFrame, MediaAbrMode, MediaAbrPolicy, MediaSourceKind, MediaSourceProtocol,
    MediaTrack, MediaTrackCatalog, MediaTrackKind, MediaTrackSelection, MediaTrackSelectionMode,
    MediaTrackSelectionSnapshot, PlaybackProgress, PresentationState, VideoPixelFormat,
};
pub use playlist::{
    PlaylistActivationReason, PlaylistActiveItem, PlaylistAdvanceDecision, PlaylistAdvanceOutcome,
    PlaylistAdvanceTrigger, PlaylistCoordinator, PlaylistCoordinatorConfig, PlaylistEvent,
    PlaylistFailureStrategy, PlaylistId, PlaylistItemPreloadProfile, PlaylistNeighborWindow,
    PlaylistPreloadWindow, PlaylistQueueItem, PlaylistQueueItemId, PlaylistQueueItemSnapshot,
    PlaylistRepeatMode, PlaylistSnapshot, PlaylistSwitchPolicy, PlaylistViewportHint,
    PlaylistViewportHintKind,
};
pub use preload::{
    InMemoryPreloadBudgetProvider, InMemoryPreloadExecutor, PreloadBudget, PreloadBudgetProvider,
    PreloadBudgetScope, PreloadCacheKey, PreloadCandidate, PreloadCandidateKind, PreloadConfig,
    PreloadErrorSummary, PreloadEvent, PreloadExecutor, PreloadPlanner, PreloadPriority,
    PreloadSelectionHint, PreloadSnapshot, PreloadSourceIdentity, PreloadTaskId,
    PreloadTaskSnapshot, PreloadTaskState, PreloadTaskStatus,
};

pub const DEFAULT_PLAYBACK_RATE: f32 = 1.0;
pub const MIN_PLAYBACK_RATE: f32 = 0.5;
pub const NATURAL_PLAYBACK_RATE_MAX: f32 = 2.0;
pub const MAX_PLAYBACK_RATE: f32 = 3.0;
pub const DEFAULT_VIDEO_PRESENT_EARLY_TOLERANCE: Duration = Duration::from_millis(4);
pub const DEFAULT_VIDEO_IDLE_POLL_INTERVAL: Duration = Duration::from_millis(16);
pub const DEFAULT_VIDEO_PREFETCH_CAPACITY: usize = 8;
pub const DEFAULT_RETRY_BASE_DELAY: Duration = Duration::from_millis(1_000);
pub const DEFAULT_RETRY_MAX_DELAY: Duration = Duration::from_millis(5_000);
pub const DEFAULT_PRELOAD_MAX_CONCURRENT_TASKS: u32 = 2;
pub const DEFAULT_PRELOAD_MAX_MEMORY_BYTES: u64 = 64 * 1024 * 1024;
pub const DEFAULT_PRELOAD_MAX_DISK_BYTES: u64 = 256 * 1024 * 1024;
pub const DEFAULT_PRELOAD_WARMUP_WINDOW: Duration = Duration::from_secs(30);

static DEFAULT_RUNTIME_ADAPTER_FACTORY: OnceLock<&'static dyn PlayerRuntimeAdapterFactory> =
    OnceLock::new();

#[derive(Debug, Clone)]
pub struct PlayerRuntimeOptions {
    pub enable_audio_output: bool,
    pub video_surface: Option<PlayerVideoSurfaceTarget>,
    pub video_prefetch_capacity: usize,
    pub video_present_early_tolerance: Duration,
    pub video_idle_poll_interval: Duration,
    pub buffering_policy: PlayerBufferingPolicy,
    pub retry_policy: PlayerRetryPolicy,
    pub cache_policy: PlayerCachePolicy,
    pub preload_budget: PlayerPreloadBudgetPolicy,
    pub track_preferences: PlayerTrackPreferencePolicy,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlayerBufferingPreset {
    Default,
    Balanced,
    Streaming,
    Resilient,
    LowLatency,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlayerBufferingPolicy {
    pub preset: PlayerBufferingPreset,
    pub min_buffer: Option<Duration>,
    pub max_buffer: Option<Duration>,
    pub buffer_for_playback: Option<Duration>,
    pub buffer_for_rebuffer: Option<Duration>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlayerRetryBackoff {
    Fixed,
    Linear,
    Exponential,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlayerRetryPolicy {
    pub max_attempts: Option<u32>,
    pub base_delay: Duration,
    pub max_delay: Duration,
    pub backoff: PlayerRetryBackoff,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlayerCachePreset {
    Default,
    Disabled,
    Streaming,
    Resilient,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlayerCachePolicy {
    pub preset: PlayerCachePreset,
    pub max_memory_bytes: Option<u64>,
    pub max_disk_bytes: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlayerResolvedResiliencePolicy {
    pub buffering_policy: PlayerBufferingPolicy,
    pub retry_policy: PlayerRetryPolicy,
    pub cache_policy: PlayerCachePolicy,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PlayerPreloadBudgetPolicy {
    pub max_concurrent_tasks: Option<u32>,
    pub max_memory_bytes: Option<u64>,
    pub max_disk_bytes: Option<u64>,
    pub warmup_window: Option<Duration>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlayerResolvedPreloadBudgetPolicy {
    pub max_concurrent_tasks: u32,
    pub max_memory_bytes: u64,
    pub max_disk_bytes: u64,
    pub warmup_window: Duration,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlayerTrackPreferencePolicy {
    pub preferred_audio_language: Option<String>,
    pub preferred_subtitle_language: Option<String>,
    pub select_subtitles_by_default: bool,
    pub select_undetermined_subtitle_language: bool,
    pub audio_selection: MediaTrackSelection,
    pub subtitle_selection: MediaTrackSelection,
    pub abr_policy: MediaAbrPolicy,
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
    pub track_catalog: MediaTrackCatalog,
    pub track_selection: MediaTrackSelectionSnapshot,
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

impl PlayerSeekableRange {
    pub fn duration(&self) -> Option<Duration> {
        self.end.checked_sub(self.start)
    }

    pub fn clamp_position(&self, position: Duration) -> Duration {
        position.clamp(self.start, self.end)
    }

    pub fn contains(&self, position: Duration) -> bool {
        position >= self.start && position <= self.end
    }
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

#[derive(Debug, Clone)]
pub enum PlayerRuntimeCommand {
    Play,
    Pause,
    TogglePause,
    SeekTo { position: Duration },
    SetPlaybackRate { rate: f32 },
    SetVideoTrackSelection { selection: MediaTrackSelection },
    SetAudioTrackSelection { selection: MediaTrackSelection },
    SetSubtitleTrackSelection { selection: MediaTrackSelection },
    SetAbrPolicy { policy: MediaAbrPolicy },
    Stop,
}

#[derive(Debug)]
pub struct PlayerRuntimeCommandResult {
    pub applied: bool,
    pub frame: Option<DecodedVideoFrame>,
    pub snapshot: PlayerSnapshot,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PlayerResilienceMetrics {
    pub buffering_event_count: u32,
    pub rebuffer_count: u32,
    pub retry_count: u32,
    pub total_buffering_duration: Duration,
    pub last_retry_delay: Option<Duration>,
}

#[derive(Debug, Default)]
pub struct PlayerResilienceMetricsTracker {
    metrics: PlayerResilienceMetrics,
    buffering_started_at: Option<Instant>,
    has_started_playback: bool,
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
    pub resilience_metrics: PlayerResilienceMetrics,
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
    RetryScheduled { attempt: u32, delay: Duration },
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
            buffering_policy: PlayerBufferingPolicy::default(),
            retry_policy: PlayerRetryPolicy::default(),
            cache_policy: PlayerCachePolicy::default(),
            preload_budget: PlayerPreloadBudgetPolicy::default(),
            track_preferences: PlayerTrackPreferencePolicy::default(),
        }
    }
}

impl PlayerRuntimeOptions {
    pub fn with_video_surface(mut self, video_surface: PlayerVideoSurfaceTarget) -> Self {
        self.video_surface = Some(video_surface);
        self
    }

    pub fn with_buffering_policy(mut self, buffering_policy: PlayerBufferingPolicy) -> Self {
        self.buffering_policy = buffering_policy;
        self
    }

    pub fn with_retry_policy(mut self, retry_policy: PlayerRetryPolicy) -> Self {
        self.retry_policy = retry_policy;
        self
    }

    pub fn with_cache_policy(mut self, cache_policy: PlayerCachePolicy) -> Self {
        self.cache_policy = cache_policy;
        self
    }

    pub fn with_preload_budget(mut self, preload_budget: PlayerPreloadBudgetPolicy) -> Self {
        self.preload_budget = preload_budget;
        self
    }

    pub fn with_track_preferences(
        mut self,
        track_preferences: PlayerTrackPreferencePolicy,
    ) -> Self {
        self.track_preferences = track_preferences;
        self
    }

    pub fn resolved_resilience_policy(
        &self,
        source_kind: MediaSourceKind,
        source_protocol: MediaSourceProtocol,
    ) -> PlayerResolvedResiliencePolicy {
        PlayerResolvedResiliencePolicy {
            buffering_policy: self
                .buffering_policy
                .resolved_for_source(source_kind, source_protocol),
            retry_policy: self.retry_policy.resolved(),
            cache_policy: self
                .cache_policy
                .resolved_for_source(source_kind, source_protocol),
        }
    }

    pub fn resolved_track_preferences(&self) -> PlayerTrackPreferencePolicy {
        self.track_preferences.resolved()
    }

    pub fn resolved_preload_budget(&self) -> PlayerResolvedPreloadBudgetPolicy {
        self.preload_budget.resolved()
    }
}

impl PlayerBufferingPolicy {
    pub fn source_default(
        source_kind: MediaSourceKind,
        source_protocol: MediaSourceProtocol,
    ) -> Option<Self> {
        match source_kind {
            MediaSourceKind::Local => None,
            MediaSourceKind::Remote => match source_protocol {
                MediaSourceProtocol::Hls | MediaSourceProtocol::Dash => Some(Self::resilient()),
                _ => Some(Self::streaming()),
            },
        }
    }

    fn merge_onto(&self, base: Option<&Self>) -> Self {
        Self {
            preset: if self.preset == PlayerBufferingPreset::Default {
                base.map(|policy| policy.preset).unwrap_or(self.preset)
            } else {
                self.preset
            },
            min_buffer: self
                .min_buffer
                .or(base.and_then(|policy| policy.min_buffer)),
            max_buffer: self
                .max_buffer
                .or(base.and_then(|policy| policy.max_buffer)),
            buffer_for_playback: self
                .buffer_for_playback
                .or(base.and_then(|policy| policy.buffer_for_playback)),
            buffer_for_rebuffer: self
                .buffer_for_rebuffer
                .or(base.and_then(|policy| policy.buffer_for_rebuffer)),
        }
    }

    pub fn resolved_for_source(
        &self,
        source_kind: MediaSourceKind,
        source_protocol: MediaSourceProtocol,
    ) -> Self {
        let base = match self.preset {
            PlayerBufferingPreset::Default => Self::source_default(source_kind, source_protocol),
            PlayerBufferingPreset::Balanced => Some(Self::balanced()),
            PlayerBufferingPreset::Streaming => Some(Self::streaming()),
            PlayerBufferingPreset::Resilient => Some(Self::resilient()),
            PlayerBufferingPreset::LowLatency => Some(Self::low_latency()),
        };

        self.merge_onto(base.as_ref())
    }

    pub fn balanced() -> Self {
        Self {
            preset: PlayerBufferingPreset::Balanced,
            min_buffer: Some(Duration::from_millis(10_000)),
            max_buffer: Some(Duration::from_millis(30_000)),
            buffer_for_playback: Some(Duration::from_millis(1_000)),
            buffer_for_rebuffer: Some(Duration::from_millis(2_000)),
        }
    }

    pub fn streaming() -> Self {
        Self {
            preset: PlayerBufferingPreset::Streaming,
            min_buffer: Some(Duration::from_millis(12_000)),
            max_buffer: Some(Duration::from_millis(36_000)),
            buffer_for_playback: Some(Duration::from_millis(1_200)),
            buffer_for_rebuffer: Some(Duration::from_millis(2_500)),
        }
    }

    pub fn resilient() -> Self {
        Self {
            preset: PlayerBufferingPreset::Resilient,
            min_buffer: Some(Duration::from_millis(20_000)),
            max_buffer: Some(Duration::from_millis(50_000)),
            buffer_for_playback: Some(Duration::from_millis(1_500)),
            buffer_for_rebuffer: Some(Duration::from_millis(3_000)),
        }
    }

    pub fn low_latency() -> Self {
        Self {
            preset: PlayerBufferingPreset::LowLatency,
            min_buffer: Some(Duration::from_millis(4_000)),
            max_buffer: Some(Duration::from_millis(12_000)),
            buffer_for_playback: Some(Duration::from_millis(500)),
            buffer_for_rebuffer: Some(Duration::from_millis(1_000)),
        }
    }
}

impl Default for PlayerBufferingPolicy {
    fn default() -> Self {
        Self {
            preset: PlayerBufferingPreset::Default,
            min_buffer: None,
            max_buffer: None,
            buffer_for_playback: None,
            buffer_for_rebuffer: None,
        }
    }
}

impl PlayerRetryPolicy {
    pub fn resolved(&self) -> Self {
        self.clone()
    }

    pub fn aggressive() -> Self {
        Self {
            max_attempts: Some(2),
            base_delay: Duration::from_millis(500),
            max_delay: Duration::from_millis(2_000),
            backoff: PlayerRetryBackoff::Fixed,
        }
    }

    pub fn resilient() -> Self {
        Self {
            max_attempts: Some(6),
            base_delay: Duration::from_millis(1_000),
            max_delay: Duration::from_millis(8_000),
            backoff: PlayerRetryBackoff::Exponential,
        }
    }
}

impl Default for PlayerRetryPolicy {
    fn default() -> Self {
        Self {
            max_attempts: Some(3),
            base_delay: DEFAULT_RETRY_BASE_DELAY,
            max_delay: DEFAULT_RETRY_MAX_DELAY,
            backoff: PlayerRetryBackoff::Linear,
        }
    }
}

impl PlayerCachePolicy {
    pub fn source_default(
        source_kind: MediaSourceKind,
        source_protocol: MediaSourceProtocol,
    ) -> Self {
        match source_kind {
            MediaSourceKind::Local => Self::disabled(),
            MediaSourceKind::Remote => match source_protocol {
                MediaSourceProtocol::Hls | MediaSourceProtocol::Dash => Self::resilient(),
                _ => Self::streaming(),
            },
        }
    }

    fn merge_onto(&self, base: &Self) -> Self {
        Self {
            preset: if self.preset == PlayerCachePreset::Default {
                base.preset
            } else {
                self.preset
            },
            max_memory_bytes: self.max_memory_bytes.or(base.max_memory_bytes),
            max_disk_bytes: self.max_disk_bytes.or(base.max_disk_bytes),
        }
    }

    pub fn resolved_for_source(
        &self,
        source_kind: MediaSourceKind,
        source_protocol: MediaSourceProtocol,
    ) -> Self {
        if source_kind == MediaSourceKind::Local {
            return Self::disabled();
        }

        let base = match self.preset {
            PlayerCachePreset::Default => Self::source_default(source_kind, source_protocol),
            PlayerCachePreset::Disabled => Self::disabled(),
            PlayerCachePreset::Streaming => Self::streaming(),
            PlayerCachePreset::Resilient => Self::resilient(),
        };

        self.merge_onto(&base)
    }

    pub fn disabled() -> Self {
        Self {
            preset: PlayerCachePreset::Disabled,
            max_memory_bytes: Some(0),
            max_disk_bytes: Some(0),
        }
    }

    pub fn streaming() -> Self {
        Self {
            preset: PlayerCachePreset::Streaming,
            max_memory_bytes: Some(8 * 1024 * 1024),
            max_disk_bytes: Some(128 * 1024 * 1024),
        }
    }

    pub fn resilient() -> Self {
        Self {
            preset: PlayerCachePreset::Resilient,
            max_memory_bytes: Some(16 * 1024 * 1024),
            max_disk_bytes: Some(384 * 1024 * 1024),
        }
    }
}

impl Default for PlayerCachePolicy {
    fn default() -> Self {
        Self {
            preset: PlayerCachePreset::Default,
            max_memory_bytes: None,
            max_disk_bytes: None,
        }
    }
}

impl PlayerPreloadBudgetPolicy {
    pub fn resolved(&self) -> PlayerResolvedPreloadBudgetPolicy {
        PlayerResolvedPreloadBudgetPolicy {
            max_concurrent_tasks: self
                .max_concurrent_tasks
                .unwrap_or(DEFAULT_PRELOAD_MAX_CONCURRENT_TASKS),
            max_memory_bytes: self
                .max_memory_bytes
                .unwrap_or(DEFAULT_PRELOAD_MAX_MEMORY_BYTES),
            max_disk_bytes: self
                .max_disk_bytes
                .unwrap_or(DEFAULT_PRELOAD_MAX_DISK_BYTES),
            warmup_window: self.warmup_window.unwrap_or(DEFAULT_PRELOAD_WARMUP_WINDOW),
        }
    }
}

impl PlayerTrackPreferencePolicy {
    pub fn resolved(&self) -> Self {
        Self {
            preferred_audio_language: normalize_optional_text(
                self.preferred_audio_language.as_deref(),
            ),
            preferred_subtitle_language: normalize_optional_text(
                self.preferred_subtitle_language.as_deref(),
            ),
            select_subtitles_by_default: self.select_subtitles_by_default,
            select_undetermined_subtitle_language: self.select_undetermined_subtitle_language,
            audio_selection: normalize_track_selection(
                &self.audio_selection,
                MediaTrackSelection::auto(),
            ),
            subtitle_selection: normalize_track_selection(
                &self.subtitle_selection,
                MediaTrackSelection::disabled(),
            ),
            abr_policy: normalize_abr_policy(&self.abr_policy),
        }
    }
}

impl Default for PlayerTrackPreferencePolicy {
    fn default() -> Self {
        Self {
            preferred_audio_language: None,
            preferred_subtitle_language: None,
            select_subtitles_by_default: false,
            select_undetermined_subtitle_language: false,
            audio_selection: MediaTrackSelection::auto(),
            subtitle_selection: MediaTrackSelection::disabled(),
            abr_policy: MediaAbrPolicy::default(),
        }
    }
}

fn normalize_optional_text(value: Option<&str>) -> Option<String> {
    let normalized = value?.trim();
    if normalized.is_empty() {
        None
    } else {
        Some(normalized.to_owned())
    }
}

fn normalize_track_selection(
    selection: &MediaTrackSelection,
    fallback: MediaTrackSelection,
) -> MediaTrackSelection {
    match selection.mode {
        MediaTrackSelectionMode::Auto => MediaTrackSelection::auto(),
        MediaTrackSelectionMode::Disabled => MediaTrackSelection::disabled(),
        MediaTrackSelectionMode::Track => normalize_optional_text(selection.track_id.as_deref())
            .map(MediaTrackSelection::track)
            .unwrap_or(fallback),
    }
}

fn normalize_abr_policy(policy: &MediaAbrPolicy) -> MediaAbrPolicy {
    match policy.mode {
        MediaAbrMode::Auto => MediaAbrPolicy::default(),
        MediaAbrMode::Constrained => {
            let normalized = MediaAbrPolicy {
                mode: MediaAbrMode::Constrained,
                track_id: None,
                max_bit_rate: policy.max_bit_rate,
                max_width: policy.max_width,
                max_height: policy.max_height,
            };
            if normalized.max_bit_rate.is_none()
                && normalized.max_width.is_none()
                && normalized.max_height.is_none()
            {
                MediaAbrPolicy::default()
            } else {
                normalized
            }
        }
        MediaAbrMode::FixedTrack => normalize_optional_text(policy.track_id.as_deref())
            .map(|track_id| MediaAbrPolicy {
                mode: MediaAbrMode::FixedTrack,
                track_id: Some(track_id),
                max_bit_rate: None,
                max_width: None,
                max_height: None,
            })
            .unwrap_or_default(),
    }
}

impl PlayerResilienceMetricsTracker {
    pub fn observe_playback_state(&mut self, state: PresentationState) {
        if state == PresentationState::Playing {
            self.has_started_playback = true;
        }
    }

    pub fn observe_buffering(&mut self, buffering: bool) {
        let now = Instant::now();
        match (buffering, self.buffering_started_at) {
            (true, None) => {
                self.metrics.buffering_event_count += 1;
                if self.has_started_playback {
                    self.metrics.rebuffer_count += 1;
                }
                self.buffering_started_at = Some(now);
            }
            (false, Some(started_at)) => {
                self.metrics.total_buffering_duration += now.saturating_duration_since(started_at);
                self.buffering_started_at = None;
            }
            _ => {}
        }
    }

    pub fn observe_retry_scheduled(&mut self, attempt: u32, delay: Duration) {
        self.metrics.retry_count = self.metrics.retry_count.max(attempt);
        self.metrics.last_retry_delay = Some(delay);
    }

    pub fn snapshot(&self) -> PlayerResilienceMetrics {
        let mut metrics = self.metrics.clone();
        if let Some(started_at) = self.buffering_started_at {
            metrics.total_buffering_duration +=
                Instant::now().saturating_duration_since(started_at);
        }
        metrics
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

    pub fn displayed_ratio(&self) -> Option<f64> {
        self.ratio_for_position(self.position)
    }

    pub fn effective_live_edge(&self) -> Option<Duration> {
        match self.kind {
            PlayerTimelineKind::Vod => None,
            PlayerTimelineKind::Live => self.live_edge,
            PlayerTimelineKind::LiveDvr => self
                .live_edge
                .or_else(|| self.seekable_range.map(|range| range.end)),
        }
    }

    pub fn go_live_position(&self) -> Option<Duration> {
        self.effective_live_edge()
    }

    pub fn clamp_position(&self, position: Duration) -> Duration {
        if let Some(range) = self.seekable_range {
            return range.clamp_position(position);
        }

        if let Some(duration) = self.duration {
            return position.clamp(Duration::ZERO, duration);
        }

        position
    }

    pub fn is_position_out_of_range(&self, position: Duration) -> bool {
        if let Some(range) = self.seekable_range {
            return !range.contains(position);
        }

        if let Some(duration) = self.duration {
            return position > duration;
        }

        false
    }

    pub fn validate_position(&self, position: Duration) -> PlayerRuntimeResult<Duration> {
        if self.is_position_out_of_range(position) {
            return Err(PlayerRuntimeError::new(
                PlayerRuntimeErrorCode::SeekFailure,
                format!(
                    "seek position {}ms is outside the current timeline window",
                    position.as_millis()
                ),
            ));
        }

        Ok(position)
    }

    pub fn live_offset(&self) -> Option<Duration> {
        let live_edge = self.effective_live_edge()?;
        Some(live_edge.saturating_sub(self.clamp_position(self.position)))
    }

    pub fn is_at_live_edge(&self, tolerance: Duration) -> bool {
        self.live_offset().is_some_and(|offset| offset <= tolerance)
    }

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

    pub fn position_for_ratio(&self, ratio: f64) -> Option<Duration> {
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

    pub fn set_video_track_selection(
        &mut self,
        selection: MediaTrackSelection,
    ) -> PlayerRuntimeResult<PlayerRuntimeCommandResult> {
        self.dispatch(PlayerRuntimeCommand::SetVideoTrackSelection { selection })
    }

    pub fn set_audio_track_selection(
        &mut self,
        selection: MediaTrackSelection,
    ) -> PlayerRuntimeResult<PlayerRuntimeCommandResult> {
        self.dispatch(PlayerRuntimeCommand::SetAudioTrackSelection { selection })
    }

    pub fn set_subtitle_track_selection(
        &mut self,
        selection: MediaTrackSelection,
    ) -> PlayerRuntimeResult<PlayerRuntimeCommandResult> {
        self.dispatch(PlayerRuntimeCommand::SetSubtitleTrackSelection { selection })
    }

    pub fn set_abr_policy(
        &mut self,
        policy: MediaAbrPolicy,
    ) -> PlayerRuntimeResult<PlayerRuntimeCommandResult> {
        self.dispatch(PlayerRuntimeCommand::SetAbrPolicy { policy })
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
        DEFAULT_PRELOAD_MAX_CONCURRENT_TASKS, DEFAULT_PRELOAD_MAX_DISK_BYTES,
        DEFAULT_PRELOAD_MAX_MEMORY_BYTES, DEFAULT_PRELOAD_WARMUP_WINDOW, MediaAbrMode,
        MediaAbrPolicy, MediaSourceKind, MediaSourceProtocol, MediaTrackSelection,
        MediaTrackSelectionMode, PlaybackProgress, PlayerBufferingPolicy, PlayerBufferingPreset,
        PlayerCachePolicy, PlayerCachePreset, PlayerMediaInfo, PlayerPreloadBudgetPolicy,
        PlayerResilienceMetricsTracker, PlayerResolvedPreloadBudgetPolicy, PlayerRetryBackoff,
        PlayerRetryPolicy, PlayerRuntimeErrorCategory, PlayerRuntimeErrorCode,
        PlayerRuntimeOptions, PlayerSeekableRange, PlayerTimelineKind, PlayerTimelineSnapshot,
        PlayerTrackPreferencePolicy, PresentationState,
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
            track_catalog: Default::default(),
            track_selection: Default::default(),
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
        let media_info = test_media_info(
            MediaSourceKind::Remote,
            MediaSourceProtocol::Progressive,
            None,
        );
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

    #[test]
    fn live_dvr_go_live_position_defaults_to_window_end() {
        let timeline = PlayerTimelineSnapshot::live_dvr(
            PlaybackProgress::new(Duration::from_secs(84), None),
            PlayerSeekableRange {
                start: Duration::from_secs(30),
                end: Duration::from_secs(120),
            },
            None,
        );

        assert_eq!(timeline.go_live_position(), Some(Duration::from_secs(120)));
        assert_eq!(
            timeline.effective_live_edge(),
            Some(Duration::from_secs(120))
        );
    }

    #[test]
    fn live_dvr_live_offset_and_live_edge_detection_follow_tolerance() {
        let timeline = PlayerTimelineSnapshot::live_dvr(
            PlaybackProgress::new(Duration::from_millis(118_800), None),
            PlayerSeekableRange {
                start: Duration::from_secs(30),
                end: Duration::from_secs(120),
            },
            Some(Duration::from_secs(120)),
        );

        assert_eq!(timeline.live_offset(), Some(Duration::from_millis(1_200)));
        assert!(timeline.is_at_live_edge(Duration::from_millis(1_500)));
        assert!(!timeline.is_at_live_edge(Duration::from_millis(1_000)));
    }

    #[test]
    fn timeline_clamps_positions_against_seekable_window() {
        let timeline = PlayerTimelineSnapshot::live_dvr(
            PlaybackProgress::new(Duration::from_secs(90), None),
            PlayerSeekableRange {
                start: Duration::from_secs(30),
                end: Duration::from_secs(120),
            },
            Some(Duration::from_secs(120)),
        );

        assert_eq!(
            timeline.clamp_position(Duration::from_secs(20)),
            Duration::from_secs(30)
        );
        assert_eq!(
            timeline.clamp_position(Duration::from_secs(150)),
            Duration::from_secs(120)
        );
    }

    #[test]
    fn timeline_validate_position_rejects_live_dvr_out_of_range_seek() {
        let timeline = PlayerTimelineSnapshot::live_dvr(
            PlaybackProgress::new(Duration::from_secs(90), None),
            PlayerSeekableRange {
                start: Duration::from_secs(30),
                end: Duration::from_secs(120),
            },
            Some(Duration::from_secs(120)),
        );

        let error = timeline
            .validate_position(Duration::from_secs(10))
            .expect_err("position before live window should fail");
        assert_eq!(error.code(), PlayerRuntimeErrorCode::SeekFailure);
        assert_eq!(error.category(), PlayerRuntimeErrorCategory::Playback);
    }

    #[test]
    fn runtime_options_default_to_shared_resilience_baseline() {
        let options = PlayerRuntimeOptions::default();

        assert_eq!(options.buffering_policy, PlayerBufferingPolicy::default());
        assert_eq!(
            options.retry_policy,
            PlayerRetryPolicy {
                max_attempts: Some(3),
                base_delay: Duration::from_millis(1_000),
                max_delay: Duration::from_millis(5_000),
                backoff: PlayerRetryBackoff::Linear,
            }
        );
        assert_eq!(options.cache_policy, PlayerCachePolicy::default());
        assert_eq!(
            options.track_preferences,
            PlayerTrackPreferencePolicy::default()
        );
    }

    #[test]
    fn runtime_options_resolve_preload_budget_to_runtime_defaults() {
        let resolved = PlayerRuntimeOptions::default().resolved_preload_budget();

        assert_eq!(
            resolved,
            PlayerResolvedPreloadBudgetPolicy {
                max_concurrent_tasks: DEFAULT_PRELOAD_MAX_CONCURRENT_TASKS,
                max_memory_bytes: DEFAULT_PRELOAD_MAX_MEMORY_BYTES,
                max_disk_bytes: DEFAULT_PRELOAD_MAX_DISK_BYTES,
                warmup_window: DEFAULT_PRELOAD_WARMUP_WINDOW,
            }
        );
    }

    #[test]
    fn runtime_preload_budget_preserves_explicit_zero_and_override_values() {
        let resolved = PlayerRuntimeOptions::default()
            .with_preload_budget(PlayerPreloadBudgetPolicy {
                max_concurrent_tasks: Some(0),
                max_memory_bytes: Some(0),
                max_disk_bytes: Some(512 * 1024 * 1024),
                warmup_window: Some(Duration::ZERO),
            })
            .resolved_preload_budget();

        assert_eq!(
            resolved,
            PlayerResolvedPreloadBudgetPolicy {
                max_concurrent_tasks: 0,
                max_memory_bytes: 0,
                max_disk_bytes: 512 * 1024 * 1024,
                warmup_window: Duration::ZERO,
            }
        );
    }

    #[test]
    fn runtime_options_resolve_track_preferences_to_runtime_defaults() {
        let resolved = PlayerRuntimeOptions::default().resolved_track_preferences();

        assert_eq!(resolved, PlayerTrackPreferencePolicy::default());
    }

    #[test]
    fn runtime_track_preferences_normalize_blank_values_and_invalid_overrides() {
        let resolved = PlayerRuntimeOptions::default()
            .with_track_preferences(PlayerTrackPreferencePolicy {
                preferred_audio_language: Some("  en-US  ".to_owned()),
                preferred_subtitle_language: Some("   ".to_owned()),
                select_subtitles_by_default: true,
                select_undetermined_subtitle_language: true,
                audio_selection: MediaTrackSelection {
                    mode: MediaTrackSelectionMode::Track,
                    track_id: Some("   ".to_owned()),
                },
                subtitle_selection: MediaTrackSelection {
                    mode: MediaTrackSelectionMode::Track,
                    track_id: Some(" subtitle:eng-main ".to_owned()),
                },
                abr_policy: MediaAbrPolicy {
                    mode: MediaAbrMode::FixedTrack,
                    track_id: Some("  ".to_owned()),
                    max_bit_rate: Some(2_000_000),
                    max_width: Some(1_920),
                    max_height: Some(1_080),
                },
            })
            .resolved_track_preferences();

        assert_eq!(resolved.preferred_audio_language.as_deref(), Some("en-US"));
        assert_eq!(resolved.preferred_subtitle_language, None);
        assert_eq!(resolved.audio_selection, MediaTrackSelection::auto());
        assert_eq!(
            resolved.subtitle_selection,
            MediaTrackSelection::track("subtitle:eng-main")
        );
        assert_eq!(resolved.abr_policy, MediaAbrPolicy::default());
    }

    #[test]
    fn runtime_track_preferences_preserve_valid_constraints() {
        let resolved = PlayerRuntimeOptions::default()
            .with_track_preferences(PlayerTrackPreferencePolicy {
                preferred_audio_language: Some("ja".to_owned()),
                preferred_subtitle_language: Some("zh-Hans".to_owned()),
                select_subtitles_by_default: true,
                select_undetermined_subtitle_language: false,
                audio_selection: MediaTrackSelection::auto(),
                subtitle_selection: MediaTrackSelection::disabled(),
                abr_policy: MediaAbrPolicy {
                    mode: MediaAbrMode::Constrained,
                    track_id: Some("ignored-track-id".to_owned()),
                    max_bit_rate: Some(4_000_000),
                    max_width: None,
                    max_height: Some(1_080),
                },
            })
            .resolved_track_preferences();

        assert_eq!(resolved.preferred_audio_language.as_deref(), Some("ja"));
        assert_eq!(
            resolved.preferred_subtitle_language.as_deref(),
            Some("zh-Hans")
        );
        assert_eq!(resolved.audio_selection, MediaTrackSelection::auto());
        assert_eq!(resolved.subtitle_selection, MediaTrackSelection::disabled());
        assert_eq!(
            resolved.abr_policy,
            MediaAbrPolicy {
                mode: MediaAbrMode::Constrained,
                track_id: None,
                max_bit_rate: Some(4_000_000),
                max_width: None,
                max_height: Some(1_080),
            }
        );
    }

    #[test]
    fn runtime_options_resolve_remote_unknown_to_streaming_defaults() {
        let resolved = PlayerRuntimeOptions::default()
            .resolved_resilience_policy(MediaSourceKind::Remote, MediaSourceProtocol::Unknown);

        assert_eq!(
            resolved.buffering_policy,
            PlayerBufferingPolicy::streaming()
        );
        assert_eq!(resolved.retry_policy, PlayerRetryPolicy::default());
        assert_eq!(resolved.cache_policy, PlayerCachePolicy::streaming());
    }

    #[test]
    fn runtime_options_resolve_manifest_sources_to_resilient_defaults() {
        let resolved = PlayerRuntimeOptions::default()
            .resolved_resilience_policy(MediaSourceKind::Remote, MediaSourceProtocol::Hls);

        assert_eq!(
            resolved.buffering_policy,
            PlayerBufferingPolicy::resilient()
        );
        assert_eq!(resolved.retry_policy, PlayerRetryPolicy::default());
        assert_eq!(resolved.cache_policy, PlayerCachePolicy::resilient());
    }

    #[test]
    fn buffering_policy_resolution_merges_explicit_overrides_onto_source_defaults() {
        let resolved = PlayerBufferingPolicy {
            preset: PlayerBufferingPreset::Default,
            min_buffer: None,
            max_buffer: Some(Duration::from_secs(40)),
            buffer_for_playback: None,
            buffer_for_rebuffer: None,
        }
        .resolved_for_source(MediaSourceKind::Remote, MediaSourceProtocol::Progressive);

        assert_eq!(
            resolved,
            PlayerBufferingPolicy {
                preset: PlayerBufferingPreset::Streaming,
                min_buffer: Some(Duration::from_millis(12_000)),
                max_buffer: Some(Duration::from_secs(40)),
                buffer_for_playback: Some(Duration::from_millis(1_200)),
                buffer_for_rebuffer: Some(Duration::from_millis(2_500)),
            }
        );
    }

    #[test]
    fn cache_policy_resolution_disables_local_sources() {
        let resolved = PlayerCachePolicy {
            preset: PlayerCachePreset::Default,
            max_memory_bytes: Some(32 * 1024 * 1024),
            max_disk_bytes: Some(512 * 1024 * 1024),
        }
        .resolved_for_source(MediaSourceKind::Local, MediaSourceProtocol::File);

        assert_eq!(resolved, PlayerCachePolicy::disabled());
    }

    #[test]
    fn buffering_presets_offer_distinct_profiles() {
        assert_eq!(
            PlayerBufferingPolicy::streaming().preset,
            PlayerBufferingPreset::Streaming
        );
        assert_eq!(
            PlayerBufferingPolicy::resilient().min_buffer,
            Some(Duration::from_millis(20_000))
        );
        assert_eq!(
            PlayerBufferingPolicy::low_latency().max_buffer,
            Some(Duration::from_millis(12_000))
        );
    }

    #[test]
    fn cache_presets_offer_distinct_profiles() {
        assert_eq!(
            PlayerCachePolicy::disabled().preset,
            PlayerCachePreset::Disabled
        );
        assert_eq!(
            PlayerCachePolicy::streaming().max_disk_bytes,
            Some(128 * 1024 * 1024)
        );
        assert_eq!(
            PlayerCachePolicy::resilient().max_memory_bytes,
            Some(16 * 1024 * 1024)
        );
    }

    #[test]
    fn resilience_metrics_tracker_counts_buffering_and_retry() {
        let mut tracker = PlayerResilienceMetricsTracker::default();

        tracker.observe_buffering(true);
        std::thread::sleep(Duration::from_millis(2));
        tracker.observe_buffering(false);
        tracker.observe_playback_state(PresentationState::Playing);
        tracker.observe_buffering(true);
        tracker.observe_buffering(false);
        tracker.observe_retry_scheduled(2, Duration::from_millis(1_500));

        let metrics = tracker.snapshot();
        assert_eq!(metrics.buffering_event_count, 2);
        assert_eq!(metrics.rebuffer_count, 1);
        assert_eq!(metrics.retry_count, 2);
        assert_eq!(metrics.last_retry_delay, Some(Duration::from_millis(1_500)));
        assert!(metrics.total_buffering_duration >= Duration::from_millis(2));
    }
}
