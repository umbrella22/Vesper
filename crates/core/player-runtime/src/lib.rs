#![deny(unsafe_code)]
#![allow(
    clippy::result_large_err,
    reason = "PlayerError is a shared public API; boxing runtime errors would change public signatures"
)]
//! Core runtime facade and shared playback contracts.
//!
//! This crate exposes the runtime wrapper used by platform adapters, shared
//! policy types, command/event payloads, playlist and preload re-exports, and
//! capability snapshots. Concrete decoding, rendering, and platform ownership
//! stay behind [`PlayerRuntimeAdapter`] implementations.

mod adapter;
mod clock;
/// Shared policy resolution helpers.
pub mod policy;

use std::collections::{BTreeMap, VecDeque};
use std::fmt;
use std::path::PathBuf;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use player_model::MediaSource;
use player_plugin::{
    MAX_PLUGIN_PLATFORM_BYTES, PipelineEvent, PluginDiagnostic, PluginDiagnosticSeverity,
};
use serde::{Deserialize, Serialize};

pub use adapter::{
    PlayerRuntimeAdapter, PlayerRuntimeAdapterBootstrap, PlayerRuntimeAdapterFactory,
    PlayerRuntimeAdapterInitializer,
};
pub use clock::{MediaClock, PlaybackClock};
pub use player_download::{
    DownloadAssetId, DownloadAssetIndex, DownloadAssetStream, DownloadByteRange,
    DownloadContentFormat, DownloadErrorSummary, DownloadEvent, DownloadEventBatch,
    DownloadExecutor, DownloadExportPlan, DownloadManager, DownloadManagerConfig,
    DownloadPrepareResult, DownloadProfile, DownloadProgressSnapshot, DownloadResourceRecord,
    DownloadSegmentRecord, DownloadSnapshot, DownloadSource, DownloadStore, DownloadStreamKind,
    DownloadTaskId, DownloadTaskProgressPatch, DownloadTaskSnapshot, DownloadTaskState,
    DownloadTaskStatePatch, DownloadTaskStatus, InMemoryDownloadExecutor, InMemoryDownloadStore,
    MAX_PENDING_DOWNLOAD_EVENTS, MAX_PENDING_PIPELINE_EVENT_REPORTS, MAX_PENDING_PIPELINE_EVENTS,
    MAX_PIPELINE_EVENT_HOOKS, PipelineEventDispatcher, PipelineEventHookRegistration,
    PipelineEventHookReport, PipelineEventHookReportBatch, PostDownloadProcessorRegistration,
};
pub use player_model::{
    DecodedVideoFrame, FixedTrackSelectionErrorDetails, MediaAbrMode, MediaAbrPolicy,
    MediaSourceKind, MediaSourceProtocol, MediaTrack, MediaTrackCatalog, MediaTrackKind,
    MediaTrackSelection, MediaTrackSelectionMode, MediaTrackSelectionSnapshot, MediaTrackSupport,
    MediaTrackSupportDiagnostics, MediaTrackSupportReason, MediaTrackSupportSource,
    MediaTrackSupportStatus, PlaybackProgress, PlayerError, PlayerErrorCategory, PlayerErrorCode,
    PlayerResult, PresentationState, SubtitleErrorDetails, VideoPixelFormat,
};
pub use player_playlist::{
    MAX_PENDING_PLAYLIST_EVENTS, PlaylistActivationReason, PlaylistActiveItem,
    PlaylistAdvanceDecision, PlaylistAdvanceOutcome, PlaylistAdvanceTrigger, PlaylistCoordinator,
    PlaylistCoordinatorConfig, PlaylistEvent, PlaylistFailureStrategy, PlaylistId,
    PlaylistItemPreloadProfile, PlaylistNeighborWindow, PlaylistPreloadWindow, PlaylistQueueItem,
    PlaylistQueueItemId, PlaylistQueueItemSnapshot, PlaylistRepeatMode, PlaylistSnapshot,
    PlaylistSwitchPolicy, PlaylistViewportHint, PlaylistViewportHintKind,
};
pub use player_plugin::{PipelineEventHook, PipelineEventHookError};
pub use player_preload::{
    DEFAULT_PRELOAD_MAX_CONCURRENT_TASKS, DEFAULT_PRELOAD_MAX_DISK_BYTES,
    DEFAULT_PRELOAD_MAX_MEMORY_BYTES, DEFAULT_PRELOAD_WARMUP_WINDOW, InMemoryPreloadBudgetProvider,
    InMemoryPreloadExecutor, MAX_PENDING_PRELOAD_EVENTS, PlayerPreloadBudgetPolicy,
    PlayerResolvedPreloadBudgetPolicy, PreloadBudget, PreloadBudgetProvider, PreloadBudgetScope,
    PreloadCacheKey, PreloadCandidate, PreloadCandidateKind, PreloadConfig, PreloadErrorSummary,
    PreloadEvent, PreloadExecutor, PreloadPlanner, PreloadPriority, PreloadSelectionHint,
    PreloadSnapshot, PreloadSourceIdentity, PreloadTaskId, PreloadTaskSnapshot, PreloadTaskState,
    PreloadTaskStatus,
};

/// Download API re-exports.
pub mod download {
    pub use player_download::{
        DownloadAssetId, DownloadAssetIndex, DownloadAssetStream, DownloadByteRange,
        DownloadContentFormat, DownloadErrorSummary, DownloadEvent, DownloadEventBatch,
        DownloadExecutor, DownloadExportPlan, DownloadManager, DownloadManagerConfig,
        DownloadPrepareResult, DownloadProfile, DownloadProgressSnapshot, DownloadResourceRecord,
        DownloadSegmentRecord, DownloadSnapshot, DownloadSource, DownloadStore, DownloadStreamKind,
        DownloadTaskId, DownloadTaskSnapshot, DownloadTaskState, DownloadTaskStatus,
        InMemoryDownloadExecutor, InMemoryDownloadStore, MAX_PENDING_DOWNLOAD_EVENTS,
        MAX_PENDING_PIPELINE_EVENT_REPORTS, MAX_PENDING_PIPELINE_EVENTS, MAX_PIPELINE_EVENT_HOOKS,
        PipelineEventDispatcher, PipelineEventHookRegistration, PipelineEventHookReport,
        PipelineEventHookReportBatch, PostDownloadProcessorRegistration,
    };
}

/// Error API re-exports.
pub mod error {
    pub use player_model::{
        PlayerError, PlayerErrorCategory, PlayerErrorCode, PlayerResult, SubtitleErrorDetails,
    };
}

/// Preload API re-exports.
pub mod preload {
    pub use player_preload::{
        DEFAULT_PRELOAD_MAX_CONCURRENT_TASKS, DEFAULT_PRELOAD_MAX_DISK_BYTES,
        DEFAULT_PRELOAD_MAX_MEMORY_BYTES, DEFAULT_PRELOAD_WARMUP_WINDOW,
        InMemoryPreloadBudgetProvider, InMemoryPreloadExecutor, MAX_PENDING_PRELOAD_EVENTS,
        PlayerPreloadBudgetPolicy, PlayerResolvedPreloadBudgetPolicy, PreloadBudget,
        PreloadBudgetProvider, PreloadBudgetScope, PreloadCacheKey, PreloadCandidate,
        PreloadCandidateKind, PreloadConfig, PreloadErrorSummary, PreloadEvent, PreloadExecutor,
        PreloadPlanner, PreloadPriority, PreloadSelectionHint, PreloadSnapshot,
        PreloadSourceIdentity, PreloadTaskId, PreloadTaskSnapshot, PreloadTaskState,
        PreloadTaskStatus,
    };
}

/// Playlist API re-exports.
pub mod playlist {
    pub use player_playlist::{
        MAX_PENDING_PLAYLIST_EVENTS, PlaylistActivationReason, PlaylistActiveItem,
        PlaylistAdvanceDecision, PlaylistAdvanceOutcome, PlaylistAdvanceTrigger,
        PlaylistCoordinator, PlaylistCoordinatorConfig, PlaylistEvent, PlaylistFailureStrategy,
        PlaylistId, PlaylistItemPreloadProfile, PlaylistNeighborWindow, PlaylistPreloadWindow,
        PlaylistQueueItem, PlaylistQueueItemId, PlaylistQueueItemSnapshot, PlaylistRepeatMode,
        PlaylistSnapshot, PlaylistSwitchPolicy, PlaylistViewportHint, PlaylistViewportHintKind,
    };
}

/// Runtime default constants.
pub mod defaults {
    pub use super::{
        DEFAULT_PLAYBACK_RATE, DEFAULT_RETRY_BASE_DELAY, DEFAULT_RETRY_MAX_DELAY,
        DEFAULT_VIDEO_IDLE_POLL_INTERVAL, DEFAULT_VIDEO_PREFETCH_CAPACITY,
        DEFAULT_VIDEO_PRESENT_EARLY_TOLERANCE, MAX_PLAYBACK_RATE, MIN_PLAYBACK_RATE,
        NATURAL_PLAYBACK_RATE_MAX,
    };
    pub use player_preload::{
        DEFAULT_PRELOAD_MAX_CONCURRENT_TASKS, DEFAULT_PRELOAD_MAX_DISK_BYTES,
        DEFAULT_PRELOAD_MAX_MEMORY_BYTES, DEFAULT_PRELOAD_WARMUP_WINDOW,
    };
}

/// Default playback rate used by runtime options.
pub const DEFAULT_PLAYBACK_RATE: f32 = 1.0;
/// Minimum accepted playback rate.
pub const MIN_PLAYBACK_RATE: f32 = 0.5;
/// Upper bound for rates considered natural playback.
pub const NATURAL_PLAYBACK_RATE_MAX: f32 = 2.0;
/// Maximum accepted playback rate.
pub const MAX_PLAYBACK_RATE: f32 = 3.0;
/// Default tolerance for presenting video frames early.
pub const DEFAULT_VIDEO_PRESENT_EARLY_TOLERANCE: Duration = Duration::from_millis(4);
/// Default idle poll interval for video runtimes.
pub const DEFAULT_VIDEO_IDLE_POLL_INTERVAL: Duration = Duration::from_millis(16);
/// Default number of video frames allowed in the prefetch queue.
pub const DEFAULT_VIDEO_PREFETCH_CAPACITY: usize = 8;
/// Default base delay for retry policies.
pub const DEFAULT_RETRY_BASE_DELAY: Duration = Duration::from_millis(1_000);
/// Default maximum delay for retry policies.
pub const DEFAULT_RETRY_MAX_DELAY: Duration = Duration::from_millis(5_000);

/// Maximum runtime events forwarded to a pipeline hook during one drain call.
pub const MAX_PLAYBACK_PIPELINE_EVENTS_PER_DRAIN: usize = 256;
/// Maximum number of pending adapter events retained before new events are dropped.
pub const MAX_PENDING_RUNTIME_EVENTS: usize = 1_024;

static NEXT_PLAYBACK_PIPELINE_SESSION_ID: AtomicU64 = AtomicU64::new(1);

static DEFAULT_RUNTIME_ADAPTER_FACTORY: OnceLock<&'static dyn PlayerRuntimeAdapterFactory> =
    OnceLock::new();

/// Shared event identity and dispatch context for a playback host session.
///
/// `PlayerRuntime` owns this context internally for SDK-managed adapters, while
/// native host bridges can use it directly when platform callbacks are routed
/// around the runtime facade. Keeping the conversion here prevents Android and
/// iOS adapters from inventing different event names or resource identities.
#[derive(Debug, Clone)]
pub struct PipelineEventContext {
    dispatcher: PipelineEventDispatcher,
    run_id: String,
    session_id: String,
    resource_identity: String,
    platform: String,
    protocol: MediaSourceProtocol,
    started_at: Instant,
}

impl PipelineEventContext {
    /// Creates a context with a fresh playback session identity.
    pub fn new(
        dispatcher: PipelineEventDispatcher,
        platform: impl Into<String>,
        protocol: MediaSourceProtocol,
    ) -> PlayerResult<Self> {
        let platform = resolve_pipeline_event_platform(&platform.into(), "mobile")?;
        let session_number = NEXT_PLAYBACK_PIPELINE_SESSION_ID
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |value| {
                Some(value.saturating_add(1).max(1))
            })
            .unwrap_or(1);
        let session_id = format!("playback-session:{session_number}");
        Ok(Self {
            dispatcher,
            run_id: session_id.clone(),
            session_id: session_id.clone(),
            resource_identity: session_id,
            platform,
            protocol,
            started_at: Instant::now(),
        })
    }

    /// Creates a context using the protocol inferred from a media source.
    pub fn for_source(
        dispatcher: PipelineEventDispatcher,
        platform: impl Into<String>,
        source: &MediaSource,
    ) -> PlayerResult<Self> {
        Self::new(dispatcher, platform, source.protocol())
    }

    /// Enqueues one runtime event for all registered hooks.
    pub fn enqueue(&self, event: &PlayerRuntimeEvent) {
        self.dispatcher.enqueue(self.pipeline_event_for(event));
    }

    /// Records events dropped before they reached the dispatcher.
    pub fn record_dropped_events(&self, count: usize) {
        self.dispatcher
            .record_dropped_events(count.min(u64::MAX as usize) as u64);
    }

    /// Waits until all accepted events are processed, subject to `timeout`.
    pub fn flush(&self, timeout: Duration) -> bool {
        self.dispatcher.flush(timeout)
    }

    /// Closes the hook worker. This operation is idempotent.
    pub fn close(&self) -> bool {
        self.dispatcher.close()
    }

    /// Drains hook reports and dispatcher counters.
    pub fn drain_reports(&self) -> PipelineEventHookReportBatch {
        self.dispatcher.drain_reports()
    }

    fn pipeline_event_for(&self, event: &PlayerRuntimeEvent) -> PipelineEvent {
        pipeline_event_for_runtime_event(
            event,
            &self.run_id,
            &self.session_id,
            &self.resource_identity,
            &self.platform,
            self.protocol,
            self.started_at,
        )
    }
}

/// Options used while probing and opening a runtime.
#[derive(Debug, Clone)]
pub struct PlayerRuntimeOptions {
    /// Whether audio output should be enabled when the adapter supports it.
    pub enable_audio_output: bool,
    /// Optional host-owned video surface passed to the adapter.
    pub video_surface: Option<PlayerVideoSurfaceTarget>,
    /// Decoder plugin library paths considered during startup.
    pub decoder_plugin_library_paths: Vec<PathBuf>,
    /// Decoder plugin video rollout mode.
    pub decoder_plugin_video_mode: PlayerDecoderPluginVideoMode,
    /// Source normalizer plugin library paths considered during startup.
    pub source_normalizer_plugin_library_paths: Vec<PathBuf>,
    /// Source normalizer rollout mode.
    pub source_normalizer_mode: SourceNormalizerMode,
    /// Frame processor plugin library paths considered during startup.
    pub frame_processor_library_paths: Vec<PathBuf>,
    /// Policy required before raw native plugin library paths may be loaded.
    pub native_plugin_loading_policy: NativePluginLoadingPolicy,
    /// Frame processor rollout mode.
    pub frame_processor_mode: FrameProcessorMode,
    /// Frame processor scheduling policy.
    pub frame_processor_policy: FrameProcessorPolicy,
    /// Maximum number of decoded video frames to prefetch.
    pub video_prefetch_capacity: usize,
    /// Tolerance for presenting video frames before their target time.
    pub video_present_early_tolerance: Duration,
    /// Poll interval used while waiting for video work.
    pub video_idle_poll_interval: Duration,
    /// Buffering policy overrides.
    pub buffering_policy: PlayerBufferingPolicy,
    /// Retry policy overrides.
    pub retry_policy: PlayerRetryPolicy,
    /// Cache policy overrides.
    pub cache_policy: PlayerCachePolicy,
    /// Preload budget overrides.
    pub preload_budget: PlayerPreloadBudgetPolicy,
    /// Track selection preferences.
    pub track_preferences: PlayerTrackPreferencePolicy,
    /// Optional dispatcher for structured playback pipeline events.
    pub pipeline_event_dispatcher: Option<PipelineEventDispatcher>,
    /// Platform label attached to playback pipeline events.
    pub pipeline_event_platform: String,
}

/// Host policy for raw native plugin libraries supplied through runtime options.
///
/// Native libraries run in-process and are not sandboxed. The default rejects
/// raw paths so production hosts use signed packages or embedded registries;
/// desktop tooling must opt into development loading explicitly.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum NativePluginLoadingPolicy {
    /// Reject raw native plugin library paths.
    #[default]
    DenyRawPaths,
    /// Permit unsigned raw libraries for local development and diagnostics.
    DevelopmentRawPaths,
}

impl NativePluginLoadingPolicy {
    /// Returns whether this policy allows unsigned development library paths.
    pub const fn allows_development_raw_paths(self) -> bool {
        matches!(self, Self::DevelopmentRawPaths)
    }

    /// Returns a stable diagnostic label for the policy.
    pub const fn wire_name(self) -> &'static str {
        match self {
            Self::DenyRawPaths => "deny-raw-paths",
            Self::DevelopmentRawPaths => "development-raw-paths",
        }
    }

    /// Checks whether one runtime surface may load unsigned development
    /// library paths.
    pub fn validate_development_raw_paths(
        self,
        surface: &'static str,
    ) -> Result<(), NativePluginLoadingPolicyError> {
        if self.allows_development_raw_paths() {
            Ok(())
        } else {
            Err(NativePluginLoadingPolicyError::new(surface, self))
        }
    }
}

/// Error returned when raw native plugin paths are present without an explicit
/// loading policy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativePluginLoadingPolicyError {
    surface: &'static str,
    policy: NativePluginLoadingPolicy,
}

impl NativePluginLoadingPolicyError {
    fn new(surface: &'static str, policy: NativePluginLoadingPolicy) -> Self {
        Self { surface, policy }
    }

    /// Runtime surface that requested raw native plugin paths.
    pub const fn surface(&self) -> &'static str {
        self.surface
    }

    /// Policy that rejected the request.
    pub const fn policy(&self) -> NativePluginLoadingPolicy {
        self.policy
    }
}

impl fmt::Display for NativePluginLoadingPolicyError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "{} raw native plugin paths require explicit development loading policy; current policy is {}",
            self.surface,
            self.policy.wire_name()
        )
    }
}

impl std::error::Error for NativePluginLoadingPolicyError {}

/// Rollout mode for decoder plugin video output.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlayerDecoderPluginVideoMode {
    /// Load and report diagnostics without requiring native-frame video output.
    DiagnosticsOnly,
    /// Prefer plugin native-frame output when the adapter supports it.
    PreferNativeFrame,
}

/// Rust-internal source normalizer rollout mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SourceNormalizerMode {
    /// Source normalizer plugins are ignored.
    #[default]
    Disabled,
    /// Plugins are probed for diagnostics only.
    DiagnosticsOnly,
    /// Plugins may validate a source before native playback continues.
    PreflightOnly,
    /// Prefer normalized output but allow fallback.
    PreferNormalized,
    /// Require normalized output.
    RequireNormalized,
}

/// Rust-internal frame processor rollout mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FrameProcessorMode {
    /// Frame processors are ignored.
    #[default]
    Disabled,
    /// Frame processors are probed for diagnostics only.
    DiagnosticsOnly,
    /// Prefer processed output but allow fallback to original frames.
    PreferProcessed,
    /// Require processed output.
    RequireProcessed,
}

/// Rust-internal native-frame pipeline rollout mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum NativeFramePipelineMode {
    /// Native-frame pipeline is disabled.
    #[default]
    Disabled,
    /// Native-frame components are probed for diagnostics only.
    DiagnosticsOnly,
    /// Prefer the native-frame pipeline but allow fallback.
    PreferNativeFrame,
    /// Require the native-frame pipeline.
    RequireNativeFrame,
}

/// Shared playback route labels used by plugin diagnostics.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlayerPlaybackRoute {
    /// Playback is delegated to the platform or system player.
    SystemPlayer,
    /// Playback uses an SDK-managed native-frame route.
    SdkManagedNativeFrame,
    /// Playback uses a software decoder route.
    SoftwareDecoder,
}

impl PlayerPlaybackRoute {
    /// Returns the stable diagnostic wire name.
    pub const fn wire_name(self) -> &'static str {
        match self {
            Self::SystemPlayer => "systemPlayer",
            Self::SdkManagedNativeFrame => "sdkManagedNativeFrame",
            Self::SoftwareDecoder => "softwareDecoder",
        }
    }

    /// Parses a stable diagnostic wire name.
    pub const fn from_wire_name(value: &str) -> Option<Self> {
        match value.as_bytes() {
            b"systemPlayer" => Some(Self::SystemPlayer),
            b"sdkManagedNativeFrame" => Some(Self::SdkManagedNativeFrame),
            b"softwareDecoder" => Some(Self::SoftwareDecoder),
            _ => None,
        }
    }
}

/// Rust-internal frame processor scheduling policy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FrameProcessorPolicy {
    /// Target processing deadline for one frame.
    pub frame_deadline: Duration,
    /// Extra tolerance before late output is treated as over deadline.
    pub late_output_tolerance: Duration,
    /// Maximum number of processors in one chain.
    pub max_chain_depth: usize,
    /// Maximum queued or in-flight frames per processor.
    pub max_in_flight_frames_per_processor: u32,
}

impl Default for FrameProcessorPolicy {
    fn default() -> Self {
        Self {
            frame_deadline: Duration::from_millis(16),
            late_output_tolerance: Duration::from_millis(4),
            max_chain_depth: 8,
            max_in_flight_frames_per_processor: 1,
        }
    }
}

/// Named buffering presets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlayerBufferingPreset {
    /// Resolve from source kind and protocol.
    Default,
    /// Balanced default for ordinary playback.
    Balanced,
    /// Larger buffers for remote progressive playback.
    Streaming,
    /// Larger buffers for streaming protocols.
    Resilient,
    /// Smaller buffers for latency-sensitive playback.
    LowLatency,
}

/// Buffering policy with optional explicit duration overrides.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlayerBufferingPolicy {
    /// Preset used as the base policy.
    pub preset: PlayerBufferingPreset,
    /// Minimum desired buffered duration.
    pub min_buffer: Option<Duration>,
    /// Maximum desired buffered duration.
    pub max_buffer: Option<Duration>,
    /// Buffer required before initial playback.
    pub buffer_for_playback: Option<Duration>,
    /// Buffer required before resuming after rebuffering.
    pub buffer_for_rebuffer: Option<Duration>,
}

/// Retry backoff shape.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlayerRetryBackoff {
    /// Use the same delay for every retry.
    Fixed,
    /// Increase delay linearly.
    Linear,
    /// Increase delay exponentially.
    Exponential,
}

/// Retry policy used by runtimes that support recovery.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlayerRetryPolicy {
    /// Maximum retry attempts, or unlimited when `None`.
    pub max_attempts: Option<u32>,
    /// Initial retry delay.
    pub base_delay: Duration,
    /// Maximum retry delay.
    pub max_delay: Duration,
    /// Delay growth behavior.
    pub backoff: PlayerRetryBackoff,
}

/// Named cache presets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlayerCachePreset {
    /// Resolve from source kind and protocol.
    Default,
    /// Disable runtime cache use.
    Disabled,
    /// Cache budget for remote progressive playback.
    Streaming,
    /// Larger cache budget for streaming protocols.
    Resilient,
}

/// Cache policy with optional explicit budget overrides.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlayerCachePolicy {
    /// Preset used as the base policy.
    pub preset: PlayerCachePreset,
    /// Maximum memory bytes available for cache use.
    pub max_memory_bytes: Option<u64>,
    /// Maximum disk bytes available for cache use.
    pub max_disk_bytes: Option<u64>,
}

/// Resolved resilience policies for one source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlayerResolvedResiliencePolicy {
    /// Resolved buffering policy.
    pub buffering_policy: PlayerBufferingPolicy,
    /// Resolved retry policy.
    pub retry_policy: PlayerRetryPolicy,
    /// Resolved cache policy.
    pub cache_policy: PlayerCachePolicy,
}

/// Preferred track selection policy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlayerTrackPreferencePolicy {
    /// Preferred audio language tag.
    pub preferred_audio_language: Option<String>,
    /// Preferred subtitle language tag.
    pub preferred_subtitle_language: Option<String>,
    /// Whether subtitles should be selected by default.
    pub select_subtitles_by_default: bool,
    /// Whether undetermined-language subtitles may be selected by default.
    pub select_undetermined_subtitle_language: bool,
    /// Preferred audio selection.
    pub audio_selection: MediaTrackSelection,
    /// Preferred subtitle selection.
    pub subtitle_selection: MediaTrackSelection,
    /// Preferred adaptive bitrate policy.
    pub abr_policy: MediaAbrPolicy,
}

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

/// Broad backend family used by diagnostics and capability snapshots.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlayerRuntimeAdapterBackendFamily {
    /// Software desktop backend.
    SoftwareDesktop,
    /// Native macOS backend.
    NativeMacos,
    /// Native Android backend.
    NativeAndroid,
    /// Native iOS backend.
    NativeIos,
    #[deprecated(
        since = "0.1.0",
        note = "HarmonyOS backend is not implemented in this workspace."
    )]
    /// Deprecated placeholder for an unimplemented HarmonyOS backend.
    NativeHarmony,
    /// Unknown or custom backend family.
    Unknown,
}

/// Runtime capability snapshot.
#[derive(Debug, Clone)]
pub struct PlayerRuntimeAdapterCapabilities {
    /// Stable adapter id.
    pub adapter_id: &'static str,
    /// Broad backend family.
    pub backend_family: PlayerRuntimeAdapterBackendFamily,
    /// Whether the adapter can output audio.
    pub supports_audio_output: bool,
    /// Whether the adapter can return decoded frames from `advance`.
    pub supports_frame_output: bool,
    /// Whether the adapter accepts an external video surface.
    pub supports_external_video_surface: bool,
    /// Whether seeking is supported.
    pub supports_seek: bool,
    /// Whether stopping is supported.
    pub supports_stop: bool,
    /// Whether playback-rate changes are supported.
    pub supports_playback_rate: bool,
    /// Minimum supported playback rate.
    pub playback_rate_min: Option<f32>,
    /// Maximum supported playback rate.
    pub playback_rate_max: Option<f32>,
    /// Maximum playback rate considered natural by the backend.
    pub natural_playback_rate_max: Option<f32>,
    /// Whether hardware decoding is available.
    pub supports_hardware_decode: bool,
    /// Whether remote streaming protocols are supported.
    pub supports_streaming: bool,
    /// Whether HDR playback is supported.
    pub supports_hdr: bool,
}

/// Probed runtime that can be initialized into a [`PlayerRuntime`].
pub struct PlayerRuntimeInitializer {
    adapter_id: &'static str,
    inner: Box<dyn PlayerRuntimeAdapterInitializer>,
    pipeline_event_dispatcher: Option<PipelineEventDispatcher>,
    pipeline_event_platform: String,
}

/// Summary of decoded audio collected during startup.
#[derive(Debug, Clone)]
pub struct DecodedAudioSummary {
    /// Audio channel count.
    pub channels: u16,
    /// Sample rate in hertz.
    pub sample_rate: u32,
    /// Decoded audio duration.
    pub duration: Duration,
}

/// Selected video decode mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlayerVideoDecodeMode {
    /// CPU/software decode.
    Software,
    /// Hardware-accelerated decode.
    Hardware,
}

/// Video decode startup summary.
#[derive(Debug, Clone)]
pub struct PlayerVideoDecodeInfo {
    /// Decode mode selected by the runtime.
    pub selected_mode: PlayerVideoDecodeMode,
    /// Whether hardware decode was available.
    pub hardware_available: bool,
    /// Backend name for hardware decode when available.
    pub hardware_backend: Option<String>,
    /// Reason hardware decode was not selected, if any.
    pub fallback_reason: Option<String>,
}

/// Plugin diagnostic status reported during runtime startup.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlayerPluginDiagnosticStatus {
    Loaded,
    LoadFailed,
    UnsupportedKind,
    DecoderSupported,
    DecoderUnsupported,
    FrameProcessorSupported,
    FrameProcessorUnsupported,
    SourceNormalizerSupported,
    SourceNormalizerUnsupported,
}

impl PlayerPluginDiagnosticStatus {
    /// Returns the stable diagnostic wire name.
    pub const fn wire_name(self) -> &'static str {
        match self {
            Self::Loaded => "loaded",
            Self::LoadFailed => "loadFailed",
            Self::UnsupportedKind => "unsupportedKind",
            Self::DecoderSupported => "decoderSupported",
            Self::DecoderUnsupported => "decoderUnsupported",
            Self::FrameProcessorSupported => "frameProcessorSupported",
            Self::FrameProcessorUnsupported => "frameProcessorUnsupported",
            Self::SourceNormalizerSupported => "sourceNormalizerSupported",
            Self::SourceNormalizerUnsupported => "sourceNormalizerUnsupported",
        }
    }

    /// Parses a stable diagnostic wire name.
    pub const fn from_wire_name(value: &str) -> Option<Self> {
        match value.as_bytes() {
            b"loaded" => Some(Self::Loaded),
            b"loadFailed" => Some(Self::LoadFailed),
            b"unsupportedKind" => Some(Self::UnsupportedKind),
            b"decoderSupported" => Some(Self::DecoderSupported),
            b"decoderUnsupported" => Some(Self::DecoderUnsupported),
            b"frameProcessorSupported" => Some(Self::FrameProcessorSupported),
            b"frameProcessorUnsupported" => Some(Self::FrameProcessorUnsupported),
            b"sourceNormalizerSupported" => Some(Self::SourceNormalizerSupported),
            b"sourceNormalizerUnsupported" => Some(Self::SourceNormalizerUnsupported),
            _ => None,
        }
    }
}

/// Rust-side codec capability summary emitted by decoder plugin diagnostics.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlayerPluginCodecCapability {
    /// Media kind reported by the plugin.
    pub media_kind: String,
    /// Codec identifier reported by the plugin.
    pub codec: String,
}

/// Rust-side decoder capability summary emitted by desktop runtime probes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlayerPluginDecoderCapabilitySummary {
    /// Structured codec capabilities.
    pub codecs: Vec<PlayerPluginCodecCapability>,
    /// Legacy codec names.
    pub legacy_codecs: Vec<String>,
    /// Whether native-frame video output is supported.
    pub supports_native_frame_output: bool,
    /// Whether hardware decode is supported.
    pub supports_hardware_decode: bool,
    /// Whether CPU video frames are supported.
    pub supports_cpu_video_frames: bool,
    /// Whether compressed audio packets are supported.
    pub supports_audio_packets: bool,
    /// Whether decoded audio frames are supported.
    pub supports_audio_frames: bool,
    /// Whether PCM frames are supported.
    pub supports_pcm_frames: bool,
    /// Whether GPU handles are supported.
    pub supports_gpu_handles: bool,
    /// Whether the decoder can flush.
    pub supports_flush: bool,
    /// Whether the decoder can drain.
    pub supports_drain: bool,
    /// Maximum simultaneous sessions.
    pub max_sessions: Option<u32>,
}

/// Rust-side frame processor capability summary emitted by desktop runtime probes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlayerPluginFrameProcessorCapabilitySummary {
    /// Accepted input handle kinds.
    pub accepted_input_handle_kinds: Vec<String>,
    /// Produced output handle kinds.
    pub output_handle_kinds: Vec<String>,
    /// Accepted input pipeline profiles.
    pub accepted_input_pipeline_profiles: Vec<String>,
    /// Produced output pipeline profiles.
    pub output_pipeline_profiles: Vec<String>,
    /// Whether video frames are supported.
    pub supports_video_frames: bool,
    /// Whether in-place passthrough is supported.
    pub supports_in_place_passthrough: bool,
    /// Whether dimensions are preserved.
    pub preserves_dimensions: bool,
    /// Whether dimensions may change.
    pub may_change_dimensions: bool,
    /// Whether color metadata is preserved.
    pub preserves_color_metadata: bool,
    /// Whether HDR metadata is preserved.
    pub preserves_hdr_metadata: bool,
    /// Whether the processor can flush.
    pub supports_flush: bool,
    /// Maximum simultaneous sessions.
    pub max_sessions: Option<u32>,
    /// Maximum in-flight frames accepted.
    pub max_in_flight_frames: Option<u32>,
}

/// Rust-side source normalizer capability summary emitted by runtime probes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlayerPluginSourceNormalizerCapabilitySummary {
    /// Supported runtime profile names.
    pub supported_runtime_profiles: Vec<String>,
    /// Supported output route names.
    pub supported_output_routes: Vec<String>,
    /// Maximum normalization level.
    pub max_level: String,
    /// Supported media kind names.
    pub media_kinds: Vec<String>,
    /// Supported codec names.
    pub codecs: Vec<String>,
    /// Supported bitstream format names.
    pub bitstream_formats: Vec<String>,
    /// Whether seeking is supported.
    pub supports_seek: bool,
    /// Whether flushing is supported.
    pub supports_flush: bool,
    /// Whether growing resources are supported.
    pub supports_growing_resources: bool,
    /// Whether range reads are supported.
    pub supports_range_reads: bool,
    /// Whether cancellation is supported.
    pub supports_cancel: bool,
    /// Supported content types.
    pub content_types: Vec<String>,
    /// Required external library names.
    pub required_libraries: Vec<String>,
    /// Required demuxer names.
    pub required_demuxers: Vec<String>,
    /// Required muxer names.
    pub required_muxers: Vec<String>,
    /// Required protocol names.
    pub required_protocols: Vec<String>,
    /// Required parser names.
    pub required_parsers: Vec<String>,
    /// Required bitstream filter names.
    pub required_bitstream_filters: Vec<String>,
    /// Required TLS backend.
    pub required_tls: Option<String>,
    /// Whether network access is required.
    pub requires_network: bool,
    /// Suggested session read buffer size.
    pub session_read_buffer_bytes: Option<u64>,
    /// Suggested manifest snapshot size.
    pub manifest_snapshot_bytes: Option<u64>,
    /// Suggested per-session disk soft cap.
    pub session_disk_soft_cap_bytes: Option<u64>,
    /// Suggested global disk soft cap.
    pub global_disk_soft_cap_bytes: Option<u64>,
    /// Maximum simultaneous sessions.
    pub max_sessions: Option<u32>,
}

/// Rust-side capability summary emitted by plugin diagnostics.
#[allow(
    clippy::large_enum_variant,
    reason = "boxing capability summaries would break the public diagnostics API"
)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlayerPluginCapabilitySummary {
    /// Decoder plugin capabilities.
    Decoder(PlayerPluginDecoderCapabilitySummary),
    /// Frame processor plugin capabilities.
    FrameProcessor(PlayerPluginFrameProcessorCapabilitySummary),
    /// Source normalizer plugin capabilities.
    SourceNormalizer(PlayerPluginSourceNormalizerCapabilitySummary),
}

/// Runtime participation state for plugin diagnostics.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PlayerPluginParticipation {
    /// Participation is not known.
    #[default]
    Unknown,
    /// Plugin was available for selection.
    Available,
    /// Plugin was selected for the route.
    Selected,
    /// Plugin participated in playback.
    Participated,
    /// Plugin was bypassed by policy or runtime conditions.
    Bypassed,
    /// Playback fell back away from the plugin.
    Fallback,
}

impl PlayerPluginParticipation {
    /// Returns the stable diagnostic wire name.
    pub const fn wire_name(self) -> &'static str {
        match self {
            Self::Unknown => "unknown",
            Self::Available => "available",
            Self::Selected => "selected",
            Self::Participated => "participated",
            Self::Bypassed => "bypassed",
            Self::Fallback => "fallback",
        }
    }

    /// Parses a stable diagnostic wire name.
    pub const fn from_wire_name(value: &str) -> Option<Self> {
        match value.as_bytes() {
            b"unknown" => Some(Self::Unknown),
            b"available" => Some(Self::Available),
            b"selected" => Some(Self::Selected),
            b"participated" => Some(Self::Participated),
            b"bypassed" => Some(Self::Bypassed),
            b"fallback" => Some(Self::Fallback),
            _ => None,
        }
    }
}

/// Resource limits applied to a plugin to defend the host against misbehaving
/// or slow plugins. This is the "circuit breaker" tier of plugin defense
/// (plan: `plugin-defense-budget-circuit-breaker-2026-07-09.md`).
///
/// Vesper deliberately does **not** do signature verification or sandboxing:
/// the host that `dlopen`s a plugin already has full process rights, so
/// signature checks add little, and true sandboxing is incompatible with the
/// native-decode resource access (GPU textures / CVPixelBuffer / D3D11) that
/// decoder and frame-processor plugins require. Instead Vesper caps the
/// observable resource consumption and trips a breaker when a plugin exceeds
/// the budget, downgrading to bypass / fallback rather than crashing playback.
///
/// All limits are optional; missing fields fall back to the associated
/// `DEFAULT_PLUGIN_*` constant.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PluginBudgetPolicy {
    /// Maximum frames a processor may queue before backpressure is reported.
    pub max_queue_depth: Option<u32>,
    /// Maximum frames a processor may have in flight simultaneously.
    pub max_in_flight_frames: Option<u32>,
    /// Per-frame processing time budget in microseconds. Exceeding it counts
    /// as a deadline miss.
    pub max_process_time_us: Option<u64>,
    /// Number of consecutive failures before the processor is disabled and the
    /// chain falls back to bypass / system decode.
    pub max_consecutive_failures: Option<u32>,
}

impl PluginBudgetPolicy {
    /// Effective queue-depth limit, falling back to the default when unset.
    pub fn effective_max_queue_depth(self) -> u32 {
        self.max_queue_depth
            .unwrap_or(DEFAULT_PLUGIN_MAX_QUEUE_DEPTH)
    }

    /// Effective in-flight frame limit, falling back to the default when unset.
    pub fn effective_max_in_flight_frames(self) -> u32 {
        self.max_in_flight_frames
            .unwrap_or(DEFAULT_PLUGIN_MAX_IN_FLIGHT_FRAMES)
    }

    /// Effective per-frame processing budget in microseconds.
    pub fn effective_max_process_time_us(self) -> u64 {
        self.max_process_time_us
            .unwrap_or(DEFAULT_PLUGIN_MAX_PROCESS_TIME_US)
    }

    /// Effective consecutive-failure threshold before the breaker trips.
    pub fn effective_max_consecutive_failures(self) -> u32 {
        self.max_consecutive_failures
            .unwrap_or(DEFAULT_PLUGIN_MAX_CONSECUTIVE_FAILURES)
    }
}

/// Default frame queue depth before backpressure is reported.
pub const DEFAULT_PLUGIN_MAX_QUEUE_DEPTH: u32 = 16;
/// Default in-flight frame cap.
pub const DEFAULT_PLUGIN_MAX_IN_FLIGHT_FRAMES: u32 = 4;
/// Default per-frame processing budget (roughly 1.5 frames at 60fps).
pub const DEFAULT_PLUGIN_MAX_PROCESS_TIME_US: u64 = 16_600;
/// Default consecutive-failure threshold before a processor is disabled.
pub const DEFAULT_PLUGIN_MAX_CONSECUTIVE_FAILURES: u32 = 5;

/// Tracks per-plugin breaker state so a plugin call site can decide whether to
/// keep invoking a plugin, downgrade it to bypass, or disable it outright.
///
/// The state machine is intentionally small and synchronous: each observation
/// (`record_success` / `record_deadline_miss` / `record_failure`) updates the
/// internal counters and returns the policy action the caller should apply.
/// Callers must not hold any lock while invoking the plugin itself.
#[derive(Debug, Clone)]
pub struct PluginBreakerState {
    budget: PluginBudgetPolicy,
    consecutive_failures: u32,
    consecutive_deadline_misses: u32,
    disabled: bool,
}

impl PluginBreakerState {
    /// Creates a breaker tracking the given budget.
    pub fn new(budget: PluginBudgetPolicy) -> Self {
        Self {
            budget,
            consecutive_failures: 0,
            consecutive_deadline_misses: 0,
            disabled: false,
        }
    }

    /// Whether the breaker has tripped and the plugin should be skipped.
    pub fn is_disabled(&self) -> bool {
        self.disabled
    }

    /// Current consecutive failure counter (for diagnostics).
    pub fn consecutive_failures(&self) -> u32 {
        self.consecutive_failures
    }

    /// Current consecutive deadline-miss counter (for warning diagnostics).
    pub fn consecutive_deadline_misses(&self) -> u32 {
        self.consecutive_deadline_misses
    }

    /// Records a successful plugin invocation; clears failure counters.
    pub fn record_success(&mut self) {
        self.consecutive_failures = 0;
        self.consecutive_deadline_misses = 0;
    }

    /// Records a deadline miss. Returns the policy action to apply; once the
    /// miss counter reaches the budget the breaker trips.
    pub fn record_deadline_miss(&mut self) -> FrameProcessorPolicyAction {
        if self.disabled {
            return FrameProcessorPolicyAction::BypassOriginalFrame;
        }
        self.consecutive_deadline_misses = self.consecutive_deadline_misses.saturating_add(1);
        self.consecutive_failures = self.consecutive_failures.saturating_add(1);
        if self.consecutive_failures >= self.budget.effective_max_consecutive_failures() {
            self.disabled = true;
            FrameProcessorPolicyAction::DisableProcessor
        } else {
            FrameProcessorPolicyAction::Continue
        }
    }

    /// Records an outright plugin failure (panic-mapped error, bad output, ...).
    pub fn record_failure(&mut self) -> FrameProcessorPolicyAction {
        if self.disabled {
            return FrameProcessorPolicyAction::BypassOriginalFrame;
        }
        self.consecutive_failures = self.consecutive_failures.saturating_add(1);
        if self.consecutive_failures >= self.budget.effective_max_consecutive_failures() {
            self.disabled = true;
            FrameProcessorPolicyAction::DisableProcessor
        } else {
            FrameProcessorPolicyAction::DropOutput
        }
    }

    /// Evaluates an observed queue depth against the budget, returning the
    /// policy action (backpressure -> drop / bypass) without touching failure
    /// counters. Backpressure alone does not trip the breaker.
    pub fn evaluate_queue_depth(&self, queue_depth: u32) -> FrameProcessorPolicyAction {
        if self.disabled {
            return FrameProcessorPolicyAction::BypassOriginalFrame;
        }
        if queue_depth > self.budget.effective_max_queue_depth() {
            FrameProcessorPolicyAction::DropOutput
        } else {
            FrameProcessorPolicyAction::Continue
        }
    }

    /// Evaluates observed in-flight frames against the budget without touching
    /// failure counters. In-flight pressure alone does not trip the breaker.
    pub fn evaluate_in_flight_frames(&self, in_flight_frames: u32) -> FrameProcessorPolicyAction {
        if self.disabled {
            return FrameProcessorPolicyAction::BypassOriginalFrame;
        }
        if in_flight_frames > self.budget.effective_max_in_flight_frames() {
            FrameProcessorPolicyAction::DropOutput
        } else {
            FrameProcessorPolicyAction::Continue
        }
    }

    /// Evaluates queue depth and in-flight pressure together. Missing values
    /// are ignored so call sites can pass only the counters they observed.
    pub fn evaluate_load(
        &self,
        queue_depth: Option<u32>,
        in_flight_frames: Option<u32>,
    ) -> FrameProcessorPolicyAction {
        if self.disabled {
            return FrameProcessorPolicyAction::BypassOriginalFrame;
        }
        if let Some(queue_depth) = queue_depth
            && self.evaluate_queue_depth(queue_depth) == FrameProcessorPolicyAction::DropOutput
        {
            return FrameProcessorPolicyAction::DropOutput;
        }
        if let Some(in_flight_frames) = in_flight_frames
            && self.evaluate_in_flight_frames(in_flight_frames)
                == FrameProcessorPolicyAction::DropOutput
        {
            return FrameProcessorPolicyAction::DropOutput;
        }
        FrameProcessorPolicyAction::Continue
    }

    /// Resets the breaker after a manual recovery (for diagnostics / host
    /// override). The breaker starts counting failures from zero again.
    pub fn reset(&mut self) {
        self.consecutive_failures = 0;
        self.consecutive_deadline_misses = 0;
        self.disabled = false;
    }
}

/// Stable key/value detail attached to a plugin diagnostic.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlayerPluginDiagnosticDetail {
    /// Stable detail key.
    pub key: String,
    /// Detail value.
    pub value: String,
}

/// Rust-side plugin diagnostic record emitted by desktop runtime probes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlayerPluginDiagnostic {
    /// Plugin path.
    pub path: String,
    /// Plugin name when available.
    pub plugin_name: Option<String>,
    /// Plugin kind when available.
    pub plugin_kind: Option<String>,
    /// Diagnostic status.
    pub status: PlayerPluginDiagnosticStatus,
    /// Human-readable diagnostic message.
    pub message: Option<String>,
    /// Capability summary when probing produced one.
    pub capability: Option<PlayerPluginCapabilitySummary>,
    /// Runtime participation state.
    pub participation: PlayerPluginParticipation,
    /// Additional stable diagnostic details.
    pub details: Vec<PlayerPluginDiagnosticDetail>,
}

/// Startup diagnostics and discovered output capabilities.
#[derive(Debug, Clone)]
pub struct PlayerRuntimeStartup {
    /// Whether FFmpeg initialization completed.
    pub ffmpeg_initialized: bool,
    /// Audio output information discovered during startup.
    pub audio_output: Option<PlayerAudioOutputInfo>,
    /// Decoded audio summary collected during startup.
    pub decoded_audio: Option<DecodedAudioSummary>,
    /// Video decode summary collected during startup.
    pub video_decode: Option<PlayerVideoDecodeInfo>,
    /// Plugin diagnostics collected during startup.
    pub plugin_diagnostics: Vec<PlayerPluginDiagnostic>,
}

/// Command sent to a runtime adapter.
#[derive(Debug, Clone)]
pub enum PlayerRuntimeCommand {
    /// Start or resume playback.
    Play,
    /// Pause playback.
    Pause,
    /// Toggle between playing and paused states.
    TogglePause,
    /// Seek to an absolute media position.
    SeekTo { position: Duration },
    /// Set playback rate.
    SetPlaybackRate { rate: f32 },
    /// Set video track selection.
    SetVideoTrackSelection { selection: MediaTrackSelection },
    /// Set audio track selection.
    SetAudioTrackSelection { selection: MediaTrackSelection },
    /// Set subtitle track selection.
    SetSubtitleTrackSelection { selection: MediaTrackSelection },
    /// Set adaptive bitrate policy with an optional catalog revision precondition.
    SetAbrPolicy {
        policy: MediaAbrPolicy,
        expected_catalog_revision: Option<u64>,
    },
    /// Stop playback when the adapter supports it.
    Stop,
}

/// Result returned after a runtime command is dispatched.
#[derive(Debug)]
pub struct PlayerRuntimeCommandResult {
    /// Whether the adapter applied the command.
    pub applied: bool,
    /// Optional frame produced while applying the command.
    pub frame: Option<DecodedVideoFrame>,
    /// Snapshot after command handling.
    pub snapshot: PlayerSnapshot,
}

/// Aggregate resilience counters reported by runtimes.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PlayerResilienceMetrics {
    /// Number of buffering transitions into buffering state.
    pub buffering_event_count: u32,
    /// Number of buffering events after playback had already started.
    pub rebuffer_count: u32,
    /// Highest retry attempt observed.
    pub retry_count: u32,
    /// Total time spent buffering.
    pub total_buffering_duration: Duration,
    /// Last retry delay that was scheduled.
    pub last_retry_delay: Option<Duration>,
}

/// Aggregate frame processing counters reported by frame-processing runtimes.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct PlayerFrameProcessingMetrics {
    /// Frames submitted to frame processors.
    pub submitted_frame_count: u64,
    /// Frames that produced processed output.
    pub processed_frame_count: u64,
    /// Frames that bypassed processing.
    pub bypassed_frame_count: u64,
    /// Processor outputs dropped by policy.
    pub dropped_output_count: u64,
    /// Frame deadline misses.
    pub deadline_miss_count: u64,
    /// Late outputs dropped by policy.
    pub late_output_drop_count: u64,
    /// Backpressure events.
    pub backpressure_count: u64,
    /// Processors disabled by policy.
    pub disabled_processor_count: u32,
    /// Maximum observed queue depth.
    pub max_queue_depth: Option<u32>,
    /// Maximum observed in-flight frames.
    pub max_in_flight_frames: Option<u32>,
    /// Last queue wait duration in microseconds.
    pub last_queue_wait_us: Option<u64>,
    /// Last processing duration in microseconds.
    pub last_process_time_us: Option<u64>,
    /// Last submit-to-ready duration in microseconds.
    pub last_submit_to_ready_us: Option<u64>,
}

/// Helper for tracking resilience metrics from runtime observations.
#[derive(Debug, Default)]
pub struct PlayerResilienceMetricsTracker {
    metrics: PlayerResilienceMetrics,
    buffering_started_at: Option<Instant>,
    has_started_playback: bool,
}

/// Point-in-time runtime state.
#[derive(Debug, Clone)]
pub struct PlayerSnapshot {
    /// Source URI.
    pub source_uri: String,
    /// Current presentation state.
    pub state: PresentationState,
    /// Whether a video surface is attached.
    pub has_video_surface: bool,
    /// Whether playback is interrupted by the host platform.
    pub is_interrupted: bool,
    /// Whether the runtime is buffering.
    pub is_buffering: bool,
    /// Current playback rate.
    pub playback_rate: f32,
    /// Current playback progress.
    pub progress: PlaybackProgress,
    /// Current timeline information.
    pub timeline: PlayerTimelineSnapshot,
    /// Current media information.
    pub media_info: PlayerMediaInfo,
    /// Current resilience metrics.
    pub resilience_metrics: PlayerResilienceMetrics,
}

/// Metadata attached to a first-frame-ready event.
#[derive(Debug, Clone)]
pub struct FirstFrameReady {
    /// Presentation timestamp of the first frame.
    pub presentation_time: Duration,
    /// Frame width in pixels.
    pub width: u32,
    /// Frame height in pixels.
    pub height: u32,
}

/// Warning domain identifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PlayerRuntimeWarningDomain {
    /// Warning produced by frame processing.
    FrameProcessor,
}

/// Frame processor warning kind.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum FrameProcessorWarningKind {
    /// Processing was slow but playback may continue.
    Slow,
    /// Processing missed a presentation deadline.
    DeadlineMissed,
    /// Processor reported backpressure.
    Backpressure,
    /// Original frame bypass was activated.
    BypassActivated,
    /// Late output was dropped.
    LateOutputDropped,
    /// Output was dropped by policy.
    OutputDropped,
    /// Processor was disabled by policy.
    Disabled,
    /// Processor recovered after a warning condition.
    Recovered,
    /// Processor did not support the frame or configuration.
    Unsupported,
}

/// Policy action associated with a frame processor warning.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum FrameProcessorPolicyAction {
    /// Continue using the output.
    Continue,
    /// Bypass and present the original frame.
    BypassOriginalFrame,
    /// Drop the processor output.
    DropOutput,
    /// Disable the processor.
    DisableProcessor,
    /// Fail playback.
    FailPlayback,
    /// Record diagnostics only.
    DiagnosticsOnly,
}

/// Structured diagnostics for one frame processor warning.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FrameProcessorWarning {
    /// Warning kind.
    pub kind: FrameProcessorWarningKind,
    /// Plugin name.
    pub plugin_name: String,
    /// Processor index in the chain.
    pub processor_index: usize,
    /// Frame id when available.
    pub frame_id: Option<u64>,
    /// Frame presentation timestamp in microseconds.
    pub frame_pts_us: Option<i64>,
    /// Frame duration in microseconds.
    pub frame_duration_us: Option<i64>,
    /// Input handle kind.
    pub input_handle_kind: Option<String>,
    /// Output handle kind.
    pub output_handle_kind: Option<String>,
    /// Processor queue depth.
    pub queue_depth: Option<u32>,
    /// Processor in-flight frame count.
    pub in_flight_frames: Option<u32>,
    /// Queue wait duration in microseconds.
    pub queue_wait_us: Option<u64>,
    /// Processing duration in microseconds.
    pub process_time_us: Option<u64>,
    /// Submit-to-ready duration in microseconds.
    pub submit_to_ready_us: Option<u64>,
    /// Presentation deadline in microseconds.
    pub present_deadline_us: Option<i64>,
    /// Amount by which the deadline was overrun.
    pub deadline_overrun_us: Option<u64>,
    /// Consecutive deadline miss count.
    pub consecutive_miss_count: Option<u32>,
    /// Policy action chosen for the warning.
    pub policy_action: FrameProcessorPolicyAction,
    /// Human-readable warning message.
    pub message: Option<String>,
}

/// Runtime warning payloads emitted by adapters while playback continues.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "domain", content = "payload")]
pub enum PlayerRuntimeWarning {
    /// Frame processor warning payload.
    FrameProcessor(FrameProcessorWarning),
}

impl PlayerRuntimeWarning {
    /// Returns the warning domain.
    pub fn domain(&self) -> PlayerRuntimeWarningDomain {
        match self {
            Self::FrameProcessor(_) => PlayerRuntimeWarningDomain::FrameProcessor,
        }
    }
}

/// Event emitted by a runtime adapter.
#[derive(Debug, Clone)]
pub enum PlayerRuntimeEvent {
    /// Runtime finished initialization.
    Initialized(PlayerRuntimeStartup),
    /// Metadata became available or changed.
    MetadataReady(PlayerMediaInfo),
    /// First frame became available.
    FirstFrameReady(FirstFrameReady),
    /// Presentation state changed.
    PlaybackStateChanged(PresentationState),
    /// Host interruption state changed.
    InterruptionChanged { interrupted: bool },
    /// Buffering state changed.
    BufferingChanged { buffering: bool },
    /// Video surface attachment changed.
    VideoSurfaceChanged { attached: bool },
    /// Audio output changed.
    AudioOutputChanged(Option<PlayerAudioOutputInfo>),
    /// Playback rate changed.
    PlaybackRateChanged { rate: f32 },
    /// Seek completed.
    SeekCompleted { position: Duration },
    /// Retry was scheduled.
    RetryScheduled { attempt: u32, delay: Duration },
    /// Non-fatal runtime warning.
    Warning(PlayerRuntimeWarning),
    /// Runtime error.
    Error(PlayerError),
    /// Playback reached the end.
    Ended,
}

/// Pushes one runtime event while enforcing the shared adapter queue bound.
pub fn push_runtime_event_bounded(
    queue: &mut VecDeque<PlayerRuntimeEvent>,
    dropped_events: &mut u64,
    event: PlayerRuntimeEvent,
) {
    if queue.len() >= MAX_PENDING_RUNTIME_EVENTS {
        *dropped_events = dropped_events.saturating_add(1);
        return;
    }
    queue.push_back(event);
}

/// Extends a runtime event queue while enforcing the shared adapter queue bound.
pub fn extend_runtime_events_bounded(
    queue: &mut VecDeque<PlayerRuntimeEvent>,
    dropped_events: &mut u64,
    events: impl IntoIterator<Item = PlayerRuntimeEvent>,
) {
    for event in events {
        push_runtime_event_bounded(queue, dropped_events, event);
    }
}

/// Runtime returned after opening a source.
pub struct PlayerRuntimeBootstrap {
    /// Runtime wrapper.
    pub runtime: PlayerRuntime,
    /// Optional frame available immediately after opening.
    pub initial_frame: Option<DecodedVideoFrame>,
    /// Startup diagnostics collected while opening.
    pub startup: PlayerRuntimeStartup,
}

/// Runtime facade that serializes access to a concrete adapter.
pub struct PlayerRuntime {
    adapter_id: &'static str,
    inner: Box<dyn PlayerRuntimeAdapter>,
    pipeline_event_dispatcher: Option<PipelineEventDispatcher>,
    pipeline_event_run_id: String,
    pipeline_event_session_id: String,
    pipeline_event_resource_identity: String,
    pipeline_event_platform: String,
    pipeline_event_started_at: Instant,
    dropped_runtime_events: u64,
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
            decoder_plugin_library_paths: Vec::new(),
            decoder_plugin_video_mode: PlayerDecoderPluginVideoMode::DiagnosticsOnly,
            source_normalizer_plugin_library_paths: Vec::new(),
            source_normalizer_mode: SourceNormalizerMode::Disabled,
            frame_processor_library_paths: Vec::new(),
            native_plugin_loading_policy: NativePluginLoadingPolicy::default(),
            frame_processor_mode: FrameProcessorMode::Disabled,
            frame_processor_policy: FrameProcessorPolicy::default(),
            video_prefetch_capacity: DEFAULT_VIDEO_PREFETCH_CAPACITY,
            video_present_early_tolerance: DEFAULT_VIDEO_PRESENT_EARLY_TOLERANCE,
            video_idle_poll_interval: DEFAULT_VIDEO_IDLE_POLL_INTERVAL,
            buffering_policy: PlayerBufferingPolicy::default(),
            retry_policy: PlayerRetryPolicy::default(),
            cache_policy: PlayerCachePolicy::default(),
            preload_budget: PlayerPreloadBudgetPolicy::default(),
            track_preferences: PlayerTrackPreferencePolicy::default(),
            pipeline_event_dispatcher: None,
            pipeline_event_platform: "unknown".to_owned(),
        }
    }
}

impl PlayerRuntimeOptions {
    /// Installs a shared dispatcher for structured playback pipeline events.
    pub fn with_pipeline_event_dispatcher(mut self, dispatcher: PipelineEventDispatcher) -> Self {
        self.pipeline_event_dispatcher = Some(dispatcher);
        self
    }

    /// Creates and installs a dispatcher for structured playback pipeline events.
    pub fn with_pipeline_event_hooks(
        mut self,
        registrations: Vec<PipelineEventHookRegistration>,
        platform: impl Into<String>,
    ) -> Self {
        self.pipeline_event_dispatcher = Some(PipelineEventDispatcher::new(registrations));
        self.pipeline_event_platform = platform.into();
        self
    }

    /// Sets the platform label attached to playback pipeline events.
    pub fn with_pipeline_event_platform(mut self, platform: impl Into<String>) -> Self {
        self.pipeline_event_platform = platform.into();
        self
    }

    /// Sets the initial host-owned video surface.
    pub fn with_video_surface(mut self, video_surface: PlayerVideoSurfaceTarget) -> Self {
        self.video_surface = Some(video_surface);
        self
    }

    /// Replaces decoder plugin library paths.
    pub fn with_decoder_plugin_library_paths(
        mut self,
        paths: impl IntoIterator<Item = PathBuf>,
    ) -> Self {
        self.decoder_plugin_library_paths = paths.into_iter().collect();
        self
    }

    /// Sets decoder plugin video mode.
    pub fn with_decoder_plugin_video_mode(mut self, mode: PlayerDecoderPluginVideoMode) -> Self {
        self.decoder_plugin_video_mode = mode;
        self
    }

    /// Replaces source normalizer plugin library paths.
    pub fn with_source_normalizer_plugin_library_paths(
        mut self,
        paths: impl IntoIterator<Item = PathBuf>,
    ) -> Self {
        self.source_normalizer_plugin_library_paths = paths.into_iter().collect();
        self
    }

    /// Sets source normalizer mode.
    pub fn with_source_normalizer_mode(mut self, mode: SourceNormalizerMode) -> Self {
        self.source_normalizer_mode = mode;
        self
    }

    /// Sets frame processor mode.
    pub fn with_frame_processor_mode(mut self, mode: FrameProcessorMode) -> Self {
        self.frame_processor_mode = mode;
        self
    }

    /// Replaces frame processor plugin library paths.
    pub fn with_frame_processor_library_paths(
        mut self,
        paths: impl IntoIterator<Item = PathBuf>,
    ) -> Self {
        self.frame_processor_library_paths = paths.into_iter().collect();
        self
    }

    /// Sets the policy used when runtime options contain raw native plugin
    /// library paths.
    pub fn with_native_plugin_loading_policy(mut self, policy: NativePluginLoadingPolicy) -> Self {
        self.native_plugin_loading_policy = policy;
        self
    }

    /// Enables raw native plugin paths for local development and diagnostics.
    pub fn with_development_native_plugin_loading(self) -> Self {
        self.with_native_plugin_loading_policy(NativePluginLoadingPolicy::DevelopmentRawPaths)
    }

    /// Checks whether raw native plugin paths are allowed for a runtime surface.
    pub fn validate_native_plugin_loading_policy(
        &self,
        surface: &'static str,
    ) -> Result<(), NativePluginLoadingPolicyError> {
        self.native_plugin_loading_policy
            .validate_development_raw_paths(surface)
    }

    /// Sets frame processor scheduling policy.
    pub fn with_frame_processor_policy(mut self, policy: FrameProcessorPolicy) -> Self {
        self.frame_processor_policy = policy;
        self
    }

    /// Sets buffering policy overrides.
    pub fn with_buffering_policy(mut self, buffering_policy: PlayerBufferingPolicy) -> Self {
        self.buffering_policy = buffering_policy;
        self
    }

    /// Sets retry policy overrides.
    pub fn with_retry_policy(mut self, retry_policy: PlayerRetryPolicy) -> Self {
        self.retry_policy = retry_policy;
        self
    }

    /// Sets cache policy overrides.
    pub fn with_cache_policy(mut self, cache_policy: PlayerCachePolicy) -> Self {
        self.cache_policy = cache_policy;
        self
    }

    /// Sets preload budget overrides.
    pub fn with_preload_budget(mut self, preload_budget: PlayerPreloadBudgetPolicy) -> Self {
        self.preload_budget = preload_budget;
        self
    }

    /// Sets track preference policy.
    pub fn with_track_preferences(
        mut self,
        track_preferences: PlayerTrackPreferencePolicy,
    ) -> Self {
        self.track_preferences = track_preferences;
        self
    }

    /// Resolves buffering, retry, and cache policies for a source.
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

    /// Returns normalized track preferences.
    pub fn resolved_track_preferences(&self) -> PlayerTrackPreferencePolicy {
        self.track_preferences.resolved()
    }

    /// Returns preload budget policy with defaults applied.
    pub fn resolved_preload_budget(&self) -> PlayerResolvedPreloadBudgetPolicy {
        self.preload_budget.resolved()
    }
}

impl PlayerBufferingPolicy {
    /// Returns the buffering policy implied by a source, if any.
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

    /// Resolves this policy against source-specific defaults.
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

    /// Returns the balanced buffering preset.
    pub fn balanced() -> Self {
        Self {
            preset: PlayerBufferingPreset::Balanced,
            min_buffer: Some(Duration::from_millis(10_000)),
            max_buffer: Some(Duration::from_millis(30_000)),
            buffer_for_playback: Some(Duration::from_millis(1_000)),
            buffer_for_rebuffer: Some(Duration::from_millis(2_000)),
        }
    }

    /// Returns the streaming buffering preset.
    pub fn streaming() -> Self {
        Self {
            preset: PlayerBufferingPreset::Streaming,
            min_buffer: Some(Duration::from_millis(12_000)),
            max_buffer: Some(Duration::from_millis(36_000)),
            buffer_for_playback: Some(Duration::from_millis(1_200)),
            buffer_for_rebuffer: Some(Duration::from_millis(2_500)),
        }
    }

    /// Returns the resilient buffering preset.
    pub fn resilient() -> Self {
        Self {
            preset: PlayerBufferingPreset::Resilient,
            min_buffer: Some(Duration::from_millis(20_000)),
            max_buffer: Some(Duration::from_millis(50_000)),
            buffer_for_playback: Some(Duration::from_millis(1_500)),
            buffer_for_rebuffer: Some(Duration::from_millis(3_000)),
        }
    }

    /// Returns the low-latency buffering preset.
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
    /// Returns this retry policy with runtime defaults applied.
    pub fn resolved(&self) -> Self {
        self.clone()
    }

    /// Returns a short retry policy for quick failure.
    pub fn aggressive() -> Self {
        Self {
            max_attempts: Some(2),
            base_delay: Duration::from_millis(500),
            max_delay: Duration::from_millis(2_000),
            backoff: PlayerRetryBackoff::Fixed,
        }
    }

    /// Returns a longer retry policy for unstable networks.
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
    /// Returns the cache policy implied by a source.
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

    /// Resolves this policy against source-specific defaults.
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

    /// Returns a policy that disables cache budgets.
    pub fn disabled() -> Self {
        Self {
            preset: PlayerCachePreset::Disabled,
            max_memory_bytes: Some(0),
            max_disk_bytes: Some(0),
        }
    }

    /// Returns the streaming cache preset.
    pub fn streaming() -> Self {
        Self {
            preset: PlayerCachePreset::Streaming,
            max_memory_bytes: Some(8 * 1024 * 1024),
            max_disk_bytes: Some(128 * 1024 * 1024),
        }
    }

    /// Returns the resilient cache preset.
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

impl PlayerTrackPreferencePolicy {
    /// Returns track preferences with empty text and invalid selections normalized.
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
    /// Observes presentation state changes for rebuffer classification.
    pub fn observe_playback_state(&mut self, state: PresentationState) {
        if state == PresentationState::Playing {
            self.has_started_playback = true;
        }
    }

    /// Observes buffering transitions.
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

    /// Observes a scheduled retry.
    pub fn observe_retry_scheduled(&mut self, attempt: u32, delay: Duration) {
        self.metrics.retry_count = self.metrics.retry_count.max(attempt);
        self.metrics.last_retry_delay = Some(delay);
    }

    /// Returns current resilience metrics.
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

impl PlayerRuntimeInitializer {
    /// Probes a URI with default options and the registered default factory.
    pub fn probe_uri(uri: impl Into<String>) -> PlayerResult<Self> {
        Self::probe_source(MediaSource::new(uri))
    }

    /// Probes a URI with explicit options and factory.
    pub fn probe_uri_with_options_and_factory(
        uri: impl Into<String>,
        options: PlayerRuntimeOptions,
        factory: &dyn PlayerRuntimeAdapterFactory,
    ) -> PlayerResult<Self> {
        Self::probe_source_with_factory(MediaSource::new(uri), options, factory)
    }

    /// Probes a source with default options and the registered default factory.
    pub fn probe_source(source: MediaSource) -> PlayerResult<Self> {
        Self::probe_source_with_options(source, PlayerRuntimeOptions::default())
    }

    /// Probes a source with explicit options and the registered default factory.
    pub fn probe_source_with_options(
        source: MediaSource,
        options: PlayerRuntimeOptions,
    ) -> PlayerResult<Self> {
        Self::probe_source_with_factory(source, options, default_runtime_adapter_factory()?)
    }

    /// Probes a source with explicit options and factory.
    pub fn probe_source_with_factory(
        source: MediaSource,
        options: PlayerRuntimeOptions,
        factory: &dyn PlayerRuntimeAdapterFactory,
    ) -> PlayerResult<Self> {
        let pipeline_event_platform = resolve_pipeline_event_platform(
            &options.pipeline_event_platform,
            factory.adapter_id(),
        )?;
        let pipeline_event_dispatcher = options.pipeline_event_dispatcher.clone();
        Ok(Self {
            adapter_id: factory.adapter_id(),
            inner: factory.probe_source_with_options(source, options)?,
            pipeline_event_dispatcher,
            pipeline_event_platform,
        })
    }

    /// Returns the adapter id selected by probing.
    pub fn adapter_id(&self) -> &str {
        self.adapter_id
    }

    /// Returns capabilities discovered during probing.
    pub fn capabilities(&self) -> PlayerRuntimeAdapterCapabilities {
        self.inner.capabilities()
    }

    /// Returns media information discovered during probing.
    pub fn media_info(&self) -> PlayerMediaInfo {
        self.inner.media_info()
    }

    /// Returns startup diagnostics available before full initialization.
    pub fn startup(&self) -> PlayerRuntimeStartup {
        self.inner.startup()
    }

    /// Initializes the probed runtime.
    pub fn initialize(self) -> PlayerResult<PlayerRuntimeBootstrap> {
        let Self {
            adapter_id,
            inner,
            pipeline_event_dispatcher,
            pipeline_event_platform,
        } = self;
        Ok(PlayerRuntime::from_adapter_bootstrap_with_pipeline(
            adapter_id,
            inner.initialize()?,
            pipeline_event_dispatcher,
            pipeline_event_platform,
        ))
    }
}

impl PlayerRuntime {
    /// Wraps an adapter bootstrap with a runtime facade.
    pub fn from_adapter_bootstrap(
        adapter_id: &'static str,
        bootstrap: PlayerRuntimeAdapterBootstrap,
    ) -> PlayerRuntimeBootstrap {
        Self::from_adapter_bootstrap_with_pipeline(
            adapter_id,
            bootstrap,
            None,
            adapter_id.to_owned(),
        )
    }

    /// Wraps an adapter bootstrap and attaches an optional playback event dispatcher.
    pub fn from_adapter_bootstrap_with_pipeline(
        adapter_id: &'static str,
        bootstrap: PlayerRuntimeAdapterBootstrap,
        pipeline_event_dispatcher: Option<PipelineEventDispatcher>,
        pipeline_event_platform: String,
    ) -> PlayerRuntimeBootstrap {
        let PlayerRuntimeAdapterBootstrap {
            runtime,
            initial_frame,
            startup,
        } = bootstrap;

        let session_id = NEXT_PLAYBACK_PIPELINE_SESSION_ID
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |value| {
                Some(value.saturating_add(1).max(1))
            })
            .unwrap_or(1);
        let pipeline_event_session_id = format!("playback-session:{session_id}");
        let pipeline_event_resource_identity = pipeline_event_session_id.clone();
        let pipeline_event_run_id = pipeline_event_session_id.clone();
        PlayerRuntimeBootstrap {
            runtime: PlayerRuntime {
                adapter_id,
                inner: runtime,
                pipeline_event_dispatcher,
                pipeline_event_run_id,
                pipeline_event_session_id,
                pipeline_event_resource_identity,
                pipeline_event_platform: resolve_pipeline_event_platform(
                    &pipeline_event_platform,
                    adapter_id,
                )
                .unwrap_or_else(|_| adapter_id.to_owned()),
                pipeline_event_started_at: Instant::now(),
                dropped_runtime_events: 0,
            },
            initial_frame,
            startup,
        }
    }

    /// Replaces the adapter while preserving the outer runtime's pipeline
    /// identity, dispatcher, and dropped-event accounting. Platform wrappers
    /// must forward dropped-event counts from the supplied inner runtime.
    pub fn wrap_adapter(
        self,
        wrapper: impl FnOnce(PlayerRuntime) -> Box<dyn PlayerRuntimeAdapter>,
    ) -> PlayerRuntime {
        let Self {
            adapter_id,
            inner,
            pipeline_event_dispatcher,
            pipeline_event_run_id,
            pipeline_event_session_id,
            pipeline_event_resource_identity,
            pipeline_event_platform,
            pipeline_event_started_at,
            dropped_runtime_events,
        } = self;
        let runtime = Self {
            adapter_id,
            inner,
            pipeline_event_dispatcher: None,
            pipeline_event_run_id: pipeline_event_run_id.clone(),
            pipeline_event_session_id: pipeline_event_session_id.clone(),
            pipeline_event_resource_identity: pipeline_event_resource_identity.clone(),
            pipeline_event_platform: pipeline_event_platform.clone(),
            pipeline_event_started_at,
            dropped_runtime_events: 0,
        };
        let inner = wrapper(runtime);
        Self {
            adapter_id,
            inner,
            pipeline_event_dispatcher,
            pipeline_event_run_id,
            pipeline_event_session_id,
            pipeline_event_resource_identity,
            pipeline_event_platform,
            pipeline_event_started_at,
            dropped_runtime_events,
        }
    }

    /// Opens a URI with default options and the registered default factory.
    pub fn open_uri(uri: impl Into<String>) -> PlayerResult<PlayerRuntimeBootstrap> {
        Self::open_source(MediaSource::new(uri))
    }

    /// Opens a URI with explicit options and factory.
    pub fn open_uri_with_options_and_factory(
        uri: impl Into<String>,
        options: PlayerRuntimeOptions,
        factory: &dyn PlayerRuntimeAdapterFactory,
    ) -> PlayerResult<PlayerRuntimeBootstrap> {
        Self::open_source_with_factory(MediaSource::new(uri), options, factory)
    }

    /// Opens a source with default options and the registered default factory.
    pub fn open_source(source: MediaSource) -> PlayerResult<PlayerRuntimeBootstrap> {
        Self::open_source_with_options(source, PlayerRuntimeOptions::default())
    }

    /// Opens a source with explicit options and the registered default factory.
    pub fn open_source_with_options(
        source: MediaSource,
        options: PlayerRuntimeOptions,
    ) -> PlayerResult<PlayerRuntimeBootstrap> {
        Self::open_source_with_factory(source, options, default_runtime_adapter_factory()?)
    }

    /// Opens a source with explicit options and factory.
    pub fn open_source_with_factory(
        source: MediaSource,
        options: PlayerRuntimeOptions,
        factory: &dyn PlayerRuntimeAdapterFactory,
    ) -> PlayerResult<PlayerRuntimeBootstrap> {
        PlayerRuntimeInitializer::probe_source_with_factory(source, options, factory)?.initialize()
    }

    /// Returns the adapter id.
    pub fn adapter_id(&self) -> &str {
        self.adapter_id
    }

    /// Returns the current source URI.
    pub fn source_uri(&self) -> &str {
        self.inner.source_uri()
    }

    /// Returns current runtime capabilities.
    pub fn capabilities(&self) -> PlayerRuntimeAdapterCapabilities {
        self.inner.capabilities()
    }

    /// Returns current media information.
    pub fn media_info(&self) -> &PlayerMediaInfo {
        self.inner.media_info()
    }

    /// Returns current presentation state.
    pub fn presentation_state(&self) -> PresentationState {
        self.inner.presentation_state()
    }

    /// Returns current playback progress.
    pub fn progress(&self) -> PlaybackProgress {
        self.inner.progress()
    }

    /// Returns whether a video surface is attached.
    pub fn has_video_surface(&self) -> bool {
        self.inner.has_video_surface()
    }

    /// Returns whether playback is interrupted by the host platform.
    pub fn is_interrupted(&self) -> bool {
        self.inner.is_interrupted()
    }

    /// Returns the current playback rate.
    pub fn playback_rate(&self) -> f32 {
        self.inner.playback_rate()
    }

    /// Returns whether the runtime is buffering.
    pub fn is_buffering(&self) -> bool {
        self.inner.is_buffering()
    }

    /// Returns a point-in-time runtime snapshot.
    pub fn snapshot(&self) -> PlayerSnapshot {
        self.inner.snapshot()
    }

    /// Drains pending runtime events.
    pub fn drain_events(&mut self) -> Vec<PlayerRuntimeEvent> {
        let events = self.inner.drain_events();
        let adapter_dropped_events = self.inner.take_dropped_event_count();
        self.dropped_runtime_events = self
            .dropped_runtime_events
            .saturating_add(adapter_dropped_events);
        if let Some(dispatcher) = &self.pipeline_event_dispatcher {
            let forwarded_events = events.len().min(MAX_PLAYBACK_PIPELINE_EVENTS_PER_DRAIN);
            for event in events.iter().take(forwarded_events) {
                dispatcher.enqueue(self.pipeline_event_for(event));
            }
            dispatcher.record_dropped_events(events.len().saturating_sub(forwarded_events) as u64);
            dispatcher.record_dropped_events(adapter_dropped_events);
        }
        events
    }

    /// Returns and clears runtime events dropped by an adapter queue or the
    /// runtime-to-hook batch bound.
    pub fn take_dropped_event_count(&mut self) -> u64 {
        let dropped = self.dropped_runtime_events;
        self.dropped_runtime_events = 0;
        dropped
    }

    /// Flushes accepted playback pipeline events within the supplied deadline.
    pub fn flush_pipeline_event_hooks(&self, timeout: Duration) -> bool {
        self.pipeline_event_dispatcher
            .as_ref()
            .map(|dispatcher| dispatcher.flush(timeout))
            .unwrap_or(true)
    }

    /// Closes the playback pipeline event dispatcher, if configured.
    pub fn close_pipeline_event_hooks(&self) -> bool {
        self.pipeline_event_dispatcher
            .as_ref()
            .map(PipelineEventDispatcher::close)
            .unwrap_or(true)
    }

    /// Drains playback pipeline hook reports, if configured.
    pub fn drain_pipeline_event_hook_reports(&self) -> PipelineEventHookReportBatch {
        self.pipeline_event_dispatcher
            .as_ref()
            .map(PipelineEventDispatcher::drain_reports)
            .unwrap_or_default()
    }

    fn pipeline_event_for(&self, event: &PlayerRuntimeEvent) -> PipelineEvent {
        pipeline_event_for_runtime_event(
            event,
            &self.pipeline_event_run_id,
            &self.pipeline_event_session_id,
            &self.pipeline_event_resource_identity,
            &self.pipeline_event_platform,
            self.inner.media_info().source_protocol,
            self.pipeline_event_started_at,
        )
    }

    #[allow(dead_code)]
    fn pipeline_event_for_legacy(&self, event: &PlayerRuntimeEvent) -> PipelineEvent {
        let mut attributes = BTreeMap::new();
        let (event_name, diagnostic) = match event {
            PlayerRuntimeEvent::Initialized(startup) => {
                attributes.insert(
                    "ffmpegInitialized".to_owned(),
                    startup.ffmpeg_initialized.to_string(),
                );
                ("playback.initialized", None)
            }
            PlayerRuntimeEvent::MetadataReady(media_info) => {
                attributes.insert(
                    "sourceKind".to_owned(),
                    playback_source_kind(media_info.source_kind).to_owned(),
                );
                attributes.insert(
                    "sourceProtocol".to_owned(),
                    playback_protocol(media_info.source_protocol).to_owned(),
                );
                attributes.insert(
                    "audioStreams".to_owned(),
                    media_info.audio_streams.to_string(),
                );
                attributes.insert(
                    "videoStreams".to_owned(),
                    media_info.video_streams.to_string(),
                );
                ("playback.metadata_ready", None)
            }
            PlayerRuntimeEvent::FirstFrameReady(frame) => {
                attributes.insert("width".to_owned(), frame.width.to_string());
                attributes.insert("height".to_owned(), frame.height.to_string());
                attributes.insert(
                    "presentationTimeMs".to_owned(),
                    frame.presentation_time.as_millis().to_string(),
                );
                ("playback.first_frame_ready", None)
            }
            PlayerRuntimeEvent::PlaybackStateChanged(state) => {
                attributes.insert("state".to_owned(), playback_state(*state).to_owned());
                ("playback.state_changed", None)
            }
            PlayerRuntimeEvent::InterruptionChanged { interrupted } => {
                attributes.insert("interrupted".to_owned(), interrupted.to_string());
                ("playback.interruption_changed", None)
            }
            PlayerRuntimeEvent::BufferingChanged { buffering } => {
                attributes.insert("buffering".to_owned(), buffering.to_string());
                ("playback.buffering_changed", None)
            }
            PlayerRuntimeEvent::VideoSurfaceChanged { attached } => {
                attributes.insert("attached".to_owned(), attached.to_string());
                ("playback.video_surface_changed", None)
            }
            PlayerRuntimeEvent::AudioOutputChanged(output) => {
                attributes.insert("available".to_owned(), output.is_some().to_string());
                ("playback.audio_output_changed", None)
            }
            PlayerRuntimeEvent::PlaybackRateChanged { rate } => {
                attributes.insert("rate".to_owned(), rate.to_string());
                ("playback.rate_changed", None)
            }
            PlayerRuntimeEvent::SeekCompleted { position } => {
                attributes.insert("positionMs".to_owned(), position.as_millis().to_string());
                ("playback.seek_completed", None)
            }
            PlayerRuntimeEvent::RetryScheduled { attempt, delay } => {
                attributes.insert("attempt".to_owned(), attempt.to_string());
                attributes.insert("delayMs".to_owned(), delay.as_millis().to_string());
                ("playback.retry_scheduled", None)
            }
            PlayerRuntimeEvent::Warning(warning) => {
                attributes.insert(
                    "domain".to_owned(),
                    playback_warning_domain(warning.domain()).to_owned(),
                );
                ("playback.warning", None)
            }
            PlayerRuntimeEvent::Error(error) => {
                attributes.insert(
                    "code".to_owned(),
                    playback_error_code(error.code()).to_owned(),
                );
                attributes.insert(
                    "category".to_owned(),
                    playback_error_category(error.category()).to_owned(),
                );
                attributes.insert("retriable".to_owned(), error.is_retriable().to_string());
                (
                    "playback.error",
                    Some(PluginDiagnostic {
                        code: "playback.error".to_owned(),
                        severity: PluginDiagnosticSeverity::Error,
                        message: "playback runtime error".to_owned(),
                        attributes: BTreeMap::new(),
                    }),
                )
            }
            PlayerRuntimeEvent::Ended => ("playback.ended", None),
        };
        let timestamp_ns = self
            .pipeline_event_started_at
            .elapsed()
            .as_nanos()
            .min(u128::from(u64::MAX)) as u64;
        PipelineEvent {
            run_id: self.pipeline_event_run_id.clone(),
            session_id: self.pipeline_event_session_id.clone(),
            platform: self.pipeline_event_platform.clone(),
            protocol: Some(playback_protocol(self.inner.media_info().source_protocol).to_owned()),
            event_name: event_name.to_owned(),
            timestamp_ns,
            thread: None,
            resource_identity: Some(self.pipeline_event_resource_identity.clone()),
            attributes,
            diagnostic,
        }
    }

    /// Dispatches a command to the underlying adapter.
    pub fn dispatch(
        &mut self,
        command: PlayerRuntimeCommand,
    ) -> PlayerResult<PlayerRuntimeCommandResult> {
        self.inner.dispatch(command)
    }

    /// Sets playback rate through command dispatch.
    pub fn set_playback_rate(&mut self, rate: f32) -> PlayerResult<PlayerRuntimeCommandResult> {
        self.dispatch(PlayerRuntimeCommand::SetPlaybackRate { rate })
    }

    /// Sets video track selection through command dispatch.
    pub fn set_video_track_selection(
        &mut self,
        selection: MediaTrackSelection,
    ) -> PlayerResult<PlayerRuntimeCommandResult> {
        self.dispatch(PlayerRuntimeCommand::SetVideoTrackSelection { selection })
    }

    /// Sets audio track selection through command dispatch.
    pub fn set_audio_track_selection(
        &mut self,
        selection: MediaTrackSelection,
    ) -> PlayerResult<PlayerRuntimeCommandResult> {
        self.dispatch(PlayerRuntimeCommand::SetAudioTrackSelection { selection })
    }

    /// Sets subtitle track selection through command dispatch.
    pub fn set_subtitle_track_selection(
        &mut self,
        selection: MediaTrackSelection,
    ) -> PlayerResult<PlayerRuntimeCommandResult> {
        self.dispatch(PlayerRuntimeCommand::SetSubtitleTrackSelection { selection })
    }

    /// Sets ABR policy through command dispatch.
    pub fn set_abr_policy(
        &mut self,
        policy: MediaAbrPolicy,
    ) -> PlayerResult<PlayerRuntimeCommandResult> {
        self.set_abr_policy_with_catalog_revision(policy, None)
    }

    /// Sets ABR policy with a revision from the catalog consumed by the caller.
    pub fn set_abr_policy_with_catalog_revision(
        &mut self,
        policy: MediaAbrPolicy,
        expected_catalog_revision: Option<u64>,
    ) -> PlayerResult<PlayerRuntimeCommandResult> {
        self.dispatch(PlayerRuntimeCommand::SetAbrPolicy {
            policy,
            expected_catalog_revision,
        })
    }

    /// Replaces the host-owned video surface.
    pub fn replace_video_surface(
        &mut self,
        video_surface: Option<PlayerVideoSurfaceTarget>,
    ) -> PlayerResult<()> {
        self.inner.replace_video_surface(video_surface)
    }

    /// Advances decoding or presentation state.
    pub fn advance(&mut self) -> PlayerResult<Option<DecodedVideoFrame>> {
        self.inner.advance()
    }

    /// Returns the next time the host should call [`advance`](Self::advance).
    pub fn next_deadline(&self) -> Option<Instant> {
        self.inner.next_deadline()
    }
}

fn pipeline_event_for_runtime_event(
    event: &PlayerRuntimeEvent,
    run_id: &str,
    session_id: &str,
    resource_identity: &str,
    platform: &str,
    protocol: MediaSourceProtocol,
    started_at: Instant,
) -> PipelineEvent {
    let mut attributes = BTreeMap::new();
    let (event_name, diagnostic) = match event {
        PlayerRuntimeEvent::Initialized(startup) => {
            attributes.insert(
                "ffmpegInitialized".to_owned(),
                startup.ffmpeg_initialized.to_string(),
            );
            ("playback.initialized", None)
        }
        PlayerRuntimeEvent::MetadataReady(media_info) => {
            attributes.insert(
                "sourceKind".to_owned(),
                playback_source_kind(media_info.source_kind).to_owned(),
            );
            attributes.insert(
                "sourceProtocol".to_owned(),
                playback_protocol(media_info.source_protocol).to_owned(),
            );
            attributes.insert(
                "audioStreams".to_owned(),
                media_info.audio_streams.to_string(),
            );
            attributes.insert(
                "videoStreams".to_owned(),
                media_info.video_streams.to_string(),
            );
            ("playback.metadata_ready", None)
        }
        PlayerRuntimeEvent::FirstFrameReady(frame) => {
            attributes.insert("width".to_owned(), frame.width.to_string());
            attributes.insert("height".to_owned(), frame.height.to_string());
            attributes.insert(
                "presentationTimeMs".to_owned(),
                frame.presentation_time.as_millis().to_string(),
            );
            ("playback.first_frame_ready", None)
        }
        PlayerRuntimeEvent::PlaybackStateChanged(state) => {
            attributes.insert("state".to_owned(), playback_state(*state).to_owned());
            ("playback.state_changed", None)
        }
        PlayerRuntimeEvent::InterruptionChanged { interrupted } => {
            attributes.insert("interrupted".to_owned(), interrupted.to_string());
            ("playback.interruption_changed", None)
        }
        PlayerRuntimeEvent::BufferingChanged { buffering } => {
            attributes.insert("buffering".to_owned(), buffering.to_string());
            ("playback.buffering_changed", None)
        }
        PlayerRuntimeEvent::VideoSurfaceChanged { attached } => {
            attributes.insert("attached".to_owned(), attached.to_string());
            ("playback.video_surface_changed", None)
        }
        PlayerRuntimeEvent::AudioOutputChanged(output) => {
            attributes.insert("available".to_owned(), output.is_some().to_string());
            ("playback.audio_output_changed", None)
        }
        PlayerRuntimeEvent::PlaybackRateChanged { rate } => {
            attributes.insert("rate".to_owned(), rate.to_string());
            ("playback.rate_changed", None)
        }
        PlayerRuntimeEvent::SeekCompleted { position } => {
            attributes.insert("positionMs".to_owned(), position.as_millis().to_string());
            ("playback.seek_completed", None)
        }
        PlayerRuntimeEvent::RetryScheduled { attempt, delay } => {
            attributes.insert("attempt".to_owned(), attempt.to_string());
            attributes.insert("delayMs".to_owned(), delay.as_millis().to_string());
            ("playback.retry_scheduled", None)
        }
        PlayerRuntimeEvent::Warning(warning) => {
            attributes.insert(
                "domain".to_owned(),
                playback_warning_domain(warning.domain()).to_owned(),
            );
            ("playback.warning", None)
        }
        PlayerRuntimeEvent::Error(error) => {
            attributes.insert(
                "code".to_owned(),
                playback_error_code(error.code()).to_owned(),
            );
            attributes.insert(
                "category".to_owned(),
                playback_error_category(error.category()).to_owned(),
            );
            attributes.insert("retriable".to_owned(), error.is_retriable().to_string());
            (
                "playback.error",
                Some(PluginDiagnostic {
                    code: "playback.error".to_owned(),
                    severity: PluginDiagnosticSeverity::Error,
                    message: "playback runtime error".to_owned(),
                    attributes: BTreeMap::new(),
                }),
            )
        }
        PlayerRuntimeEvent::Ended => ("playback.ended", None),
    };
    let timestamp_ns = started_at.elapsed().as_nanos().min(u128::from(u64::MAX)) as u64;
    PipelineEvent {
        run_id: run_id.to_owned(),
        session_id: session_id.to_owned(),
        platform: platform.to_owned(),
        protocol: Some(playback_protocol(protocol).to_owned()),
        event_name: event_name.to_owned(),
        timestamp_ns,
        thread: None,
        resource_identity: Some(resource_identity.to_owned()),
        attributes,
        diagnostic,
    }
}

fn playback_protocol(protocol: MediaSourceProtocol) -> &'static str {
    match protocol {
        MediaSourceProtocol::Unknown => "unknown",
        MediaSourceProtocol::File => "file",
        MediaSourceProtocol::Content => "content",
        MediaSourceProtocol::Progressive => "progressive",
        MediaSourceProtocol::Hls => "hls",
        MediaSourceProtocol::Dash => "dash",
        MediaSourceProtocol::Rtmp => "rtmp",
        MediaSourceProtocol::Rtsp => "rtsp",
        MediaSourceProtocol::Flv => "flv",
    }
}

fn playback_source_kind(kind: MediaSourceKind) -> &'static str {
    match kind {
        MediaSourceKind::Local => "local",
        MediaSourceKind::Remote => "remote",
    }
}

fn playback_state(state: PresentationState) -> &'static str {
    match state {
        PresentationState::Ready => "ready",
        PresentationState::Playing => "playing",
        PresentationState::Paused => "paused",
        PresentationState::Finished => "finished",
    }
}

fn playback_warning_domain(domain: PlayerRuntimeWarningDomain) -> &'static str {
    match domain {
        PlayerRuntimeWarningDomain::FrameProcessor => "frameProcessor",
    }
}

fn playback_error_code(code: PlayerErrorCode) -> &'static str {
    match code {
        PlayerErrorCode::InvalidArgument => "invalidArgument",
        PlayerErrorCode::InvalidState => "invalidState",
        PlayerErrorCode::InvalidSource => "invalidSource",
        PlayerErrorCode::BackendFailure => "backendFailure",
        PlayerErrorCode::AudioOutputUnavailable => "audioOutputUnavailable",
        PlayerErrorCode::DecodeFailure => "decodeFailure",
        PlayerErrorCode::SeekFailure => "seekFailure",
        PlayerErrorCode::Unsupported => "unsupported",
        PlayerErrorCode::CommandChannelClosed => "commandChannelClosed",
        PlayerErrorCode::EventChannelClosed => "eventChannelClosed",
        PlayerErrorCode::Cancelled => "cancelled",
        PlayerErrorCode::Timeout => "timeout",
    }
}

fn playback_error_category(category: PlayerErrorCategory) -> &'static str {
    match category {
        PlayerErrorCategory::Input => "input",
        PlayerErrorCategory::Source => "source",
        PlayerErrorCategory::Network => "network",
        PlayerErrorCategory::Decode => "decode",
        PlayerErrorCategory::AudioOutput => "audioOutput",
        PlayerErrorCategory::Playback => "playback",
        PlayerErrorCategory::Capability => "capability",
        PlayerErrorCategory::Platform => "platform",
    }
}

fn resolve_pipeline_event_platform(configured: &str, adapter_id: &str) -> PlayerResult<String> {
    let platform = if configured.trim().is_empty() || configured == "unknown" {
        adapter_id
    } else {
        configured
    };
    if platform.trim().is_empty() {
        return Err(PlayerError::with_category(
            PlayerErrorCode::InvalidArgument,
            PlayerErrorCategory::Input,
            "pipeline event platform must not be empty",
        ));
    }
    if platform.len() > MAX_PLUGIN_PLATFORM_BYTES {
        return Err(PlayerError::with_category(
            PlayerErrorCode::InvalidArgument,
            PlayerErrorCategory::Input,
            format!("pipeline event platform exceeds {MAX_PLUGIN_PLATFORM_BYTES} bytes"),
        ));
    }
    Ok(platform.to_owned())
}

/// Registers the process-wide default runtime adapter factory.
pub fn register_default_runtime_adapter_factory(
    factory: &'static dyn PlayerRuntimeAdapterFactory,
) -> PlayerResult<()> {
    match DEFAULT_RUNTIME_ADAPTER_FACTORY.set(factory) {
        Ok(()) => Ok(()),
        Err(existing) if existing.adapter_id() == factory.adapter_id() => Ok(()),
        Err(existing) => Err(PlayerError::new(
            PlayerErrorCode::InvalidState,
            format!(
                "default runtime adapter factory is already registered as '{}'; cannot replace it with '{}'",
                existing.adapter_id(),
                factory.adapter_id()
            ),
        )),
    }
}

fn default_runtime_adapter_factory() -> PlayerResult<&'static dyn PlayerRuntimeAdapterFactory> {
    DEFAULT_RUNTIME_ADAPTER_FACTORY.get().copied().ok_or_else(|| {
        PlayerError::new(
            PlayerErrorCode::Unsupported,
            "no default runtime adapter factory is registered; use probe_source_with_factory/open_source_with_factory or install a platform adapter factory",
        )
    })
}

#[cfg(test)]
mod tests {
    use super::{
        DEFAULT_PLUGIN_MAX_CONSECUTIVE_FAILURES, DEFAULT_PLUGIN_MAX_IN_FLIGHT_FRAMES,
        DEFAULT_PLUGIN_MAX_PROCESS_TIME_US, DEFAULT_PLUGIN_MAX_QUEUE_DEPTH, PluginBreakerState,
        PluginBudgetPolicy,
    };
    use super::{
        DEFAULT_PRELOAD_MAX_CONCURRENT_TASKS, DEFAULT_PRELOAD_MAX_DISK_BYTES,
        DEFAULT_PRELOAD_MAX_MEMORY_BYTES, DEFAULT_PRELOAD_WARMUP_WINDOW, FirstFrameReady,
        FrameProcessorMode, FrameProcessorPolicy, FrameProcessorPolicyAction,
        FrameProcessorWarning, FrameProcessorWarningKind, MAX_PENDING_RUNTIME_EVENTS,
        MAX_PLAYBACK_PIPELINE_EVENTS_PER_DRAIN, MediaAbrMode, MediaAbrPolicy, MediaSourceKind,
        MediaSourceProtocol, MediaTrackSelection, MediaTrackSelectionMode,
        NativePluginLoadingPolicy, PlaybackProgress, PlayerAudioOutputInfo, PlayerBufferingPolicy,
        PlayerBufferingPreset, PlayerCachePolicy, PlayerCachePreset, PlayerError,
        PlayerErrorCategory, PlayerErrorCode, PlayerFrameProcessingMetrics, PlayerMediaInfo,
        PlayerPlaybackRoute, PlayerPluginDiagnosticStatus, PlayerPluginParticipation,
        PlayerPreloadBudgetPolicy, PlayerResilienceMetricsTracker,
        PlayerResolvedPreloadBudgetPolicy, PlayerResult, PlayerRetryBackoff, PlayerRetryPolicy,
        PlayerRuntimeAdapter, PlayerRuntimeAdapterBackendFamily, PlayerRuntimeAdapterBootstrap,
        PlayerRuntimeAdapterCapabilities, PlayerRuntimeAdapterFactory,
        PlayerRuntimeAdapterInitializer, PlayerRuntimeCommand, PlayerRuntimeCommandResult,
        PlayerRuntimeEvent, PlayerRuntimeInitializer, PlayerRuntimeOptions, PlayerRuntimeStartup,
        PlayerRuntimeWarning, PlayerRuntimeWarningDomain, PlayerSeekableRange, PlayerTimelineKind,
        PlayerTimelineSnapshot, PlayerTrackPreferencePolicy, PlayerVideoSurfaceTarget,
        PresentationState, SourceNormalizerMode, extend_runtime_events_bounded,
        push_runtime_event_bounded,
    };
    use player_plugin::{
        PipelineEvent, PipelineEventHook, PipelineEventHookOutcome, PluginReference,
        PluginTransport,
    };
    use std::collections::VecDeque;
    use std::path::PathBuf;
    use std::sync::{Arc, Mutex};
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

    fn test_startup() -> PlayerRuntimeStartup {
        PlayerRuntimeStartup {
            ffmpeg_initialized: true,
            audio_output: None,
            decoded_audio: None,
            video_decode: None,
            plugin_diagnostics: Vec::new(),
        }
    }

    fn fake_capabilities() -> PlayerRuntimeAdapterCapabilities {
        PlayerRuntimeAdapterCapabilities {
            adapter_id: "fake-adapter",
            backend_family: PlayerRuntimeAdapterBackendFamily::Unknown,
            supports_audio_output: false,
            supports_frame_output: false,
            supports_external_video_surface: false,
            supports_seek: true,
            supports_stop: true,
            supports_playback_rate: true,
            playback_rate_min: Some(0.5),
            playback_rate_max: Some(3.0),
            natural_playback_rate_max: Some(2.0),
            supports_hardware_decode: false,
            supports_streaming: true,
            supports_hdr: false,
        }
    }

    struct FakeAdapter {
        media_info: PlayerMediaInfo,
        events: Vec<PlayerRuntimeEvent>,
        dropped_events: u64,
    }

    impl PlayerRuntimeAdapter for FakeAdapter {
        fn source_uri(&self) -> &str {
            &self.media_info.source_uri
        }

        fn capabilities(&self) -> PlayerRuntimeAdapterCapabilities {
            fake_capabilities()
        }

        fn media_info(&self) -> &PlayerMediaInfo {
            &self.media_info
        }

        fn presentation_state(&self) -> PresentationState {
            PresentationState::Ready
        }

        fn playback_rate(&self) -> f32 {
            1.0
        }

        fn progress(&self) -> PlaybackProgress {
            PlaybackProgress::new(Duration::ZERO, self.media_info.duration)
        }

        fn drain_events(&mut self) -> Vec<PlayerRuntimeEvent> {
            std::mem::take(&mut self.events)
        }

        fn take_dropped_event_count(&mut self) -> u64 {
            std::mem::take(&mut self.dropped_events)
        }

        fn dispatch(
            &mut self,
            _command: PlayerRuntimeCommand,
        ) -> PlayerResult<PlayerRuntimeCommandResult> {
            Err(PlayerError::new(
                PlayerErrorCode::Unsupported,
                "fake adapter does not dispatch commands",
            ))
        }

        fn advance(&mut self) -> PlayerResult<Option<super::DecodedVideoFrame>> {
            Ok(None)
        }

        fn next_deadline(&self) -> Option<std::time::Instant> {
            None
        }
    }

    struct ForwardingRuntimeAdapter {
        inner: super::PlayerRuntime,
    }

    impl PlayerRuntimeAdapter for ForwardingRuntimeAdapter {
        fn source_uri(&self) -> &str {
            self.inner.source_uri()
        }

        fn capabilities(&self) -> PlayerRuntimeAdapterCapabilities {
            self.inner.capabilities()
        }

        fn media_info(&self) -> &PlayerMediaInfo {
            self.inner.media_info()
        }

        fn presentation_state(&self) -> PresentationState {
            self.inner.presentation_state()
        }

        fn has_video_surface(&self) -> bool {
            self.inner.has_video_surface()
        }

        fn is_interrupted(&self) -> bool {
            self.inner.is_interrupted()
        }

        fn is_buffering(&self) -> bool {
            self.inner.is_buffering()
        }

        fn playback_rate(&self) -> f32 {
            self.inner.playback_rate()
        }

        fn progress(&self) -> PlaybackProgress {
            self.inner.progress()
        }

        fn drain_events(&mut self) -> Vec<PlayerRuntimeEvent> {
            self.inner.drain_events()
        }

        fn take_dropped_event_count(&mut self) -> u64 {
            self.inner.take_dropped_event_count()
        }

        fn dispatch(
            &mut self,
            command: PlayerRuntimeCommand,
        ) -> PlayerResult<PlayerRuntimeCommandResult> {
            self.inner.dispatch(command)
        }

        fn replace_video_surface(
            &mut self,
            video_surface: Option<PlayerVideoSurfaceTarget>,
        ) -> PlayerResult<()> {
            self.inner.replace_video_surface(video_surface)
        }

        fn advance(&mut self) -> PlayerResult<Option<super::DecodedVideoFrame>> {
            self.inner.advance()
        }

        fn next_deadline(&self) -> Option<std::time::Instant> {
            self.inner.next_deadline()
        }
    }

    struct FakeInitializer {
        media_info: PlayerMediaInfo,
        events: Vec<PlayerRuntimeEvent>,
    }

    impl PlayerRuntimeAdapterInitializer for FakeInitializer {
        fn capabilities(&self) -> PlayerRuntimeAdapterCapabilities {
            fake_capabilities()
        }

        fn media_info(&self) -> PlayerMediaInfo {
            self.media_info.clone()
        }

        fn startup(&self) -> PlayerRuntimeStartup {
            test_startup()
        }

        fn initialize(self: Box<Self>) -> PlayerResult<PlayerRuntimeAdapterBootstrap> {
            let FakeInitializer { media_info, events } = *self;
            Ok(PlayerRuntimeAdapterBootstrap {
                runtime: Box::new(FakeAdapter {
                    media_info,
                    events,
                    dropped_events: 0,
                }),
                initial_frame: None,
                startup: test_startup(),
            })
        }
    }

    struct FakeFactory {
        seen_platform: Arc<Mutex<Option<String>>>,
        media_info: PlayerMediaInfo,
        events: Vec<PlayerRuntimeEvent>,
    }

    impl PlayerRuntimeAdapterFactory for FakeFactory {
        fn adapter_id(&self) -> &'static str {
            "fake-adapter"
        }

        fn probe_source_with_options(
            &self,
            _source: player_model::MediaSource,
            options: PlayerRuntimeOptions,
        ) -> PlayerResult<Box<dyn PlayerRuntimeAdapterInitializer>> {
            *self
                .seen_platform
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner()) =
                Some(options.pipeline_event_platform.clone());
            Ok(Box::new(FakeInitializer {
                media_info: self.media_info.clone(),
                events: self.events.clone(),
            }))
        }
    }

    struct CapturingHook {
        events: Arc<Mutex<Vec<PipelineEvent>>>,
    }

    impl PipelineEventHook for CapturingHook {
        fn on_event(
            &self,
            event: &PipelineEvent,
        ) -> Result<PipelineEventHookOutcome, player_plugin::PipelineEventHookError> {
            self.events
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .push(event.clone());
            Ok(PipelineEventHookOutcome::accepted())
        }
    }

    fn capturing_dispatcher() -> (
        super::PipelineEventDispatcher,
        Arc<Mutex<Vec<PipelineEvent>>>,
    ) {
        let events = Arc::new(Mutex::new(Vec::new()));
        let registration = super::PipelineEventHookRegistration::new(
            PluginReference::new(
                "dev.vesper.runtime-test",
                Some("dev.vesper.runtime-test.primary".to_owned()),
                PluginTransport::Native,
            )
            .expect("valid test reference"),
            Arc::new(CapturingHook {
                events: events.clone(),
            }),
        )
        .expect("valid test registration");
        (
            super::PipelineEventDispatcher::new(vec![registration]),
            events,
        )
    }

    fn fake_bootstrap(
        media_info: PlayerMediaInfo,
        events: Vec<PlayerRuntimeEvent>,
    ) -> PlayerRuntimeAdapterBootstrap {
        PlayerRuntimeAdapterBootstrap {
            runtime: Box::new(FakeAdapter {
                media_info,
                events,
                dropped_events: 0,
            }),
            initial_frame: None,
            startup: test_startup(),
        }
    }

    fn playback_warning_event() -> PlayerRuntimeEvent {
        PlayerRuntimeEvent::Warning(PlayerRuntimeWarning::FrameProcessor(
            FrameProcessorWarning {
                kind: FrameProcessorWarningKind::DeadlineMissed,
                plugin_name: "fixture".to_owned(),
                processor_index: 0,
                frame_id: None,
                frame_pts_us: None,
                frame_duration_us: None,
                input_handle_kind: None,
                output_handle_kind: None,
                queue_depth: None,
                in_flight_frames: None,
                queue_wait_us: None,
                process_time_us: None,
                submit_to_ready_us: None,
                present_deadline_us: None,
                deadline_overrun_us: None,
                consecutive_miss_count: None,
                policy_action: FrameProcessorPolicyAction::Continue,
                message: Some("internal warning".to_owned()),
            },
        ))
    }

    fn valid_pipeline_event() -> PipelineEvent {
        PipelineEvent {
            run_id: "run".to_owned(),
            session_id: "session".to_owned(),
            platform: "test".to_owned(),
            protocol: Some("hls".to_owned()),
            event_name: "playback.test".to_owned(),
            timestamp_ns: 0,
            thread: None,
            resource_identity: Some("playback-session:test".to_owned()),
            attributes: Default::default(),
            diagnostic: None,
        }
    }

    #[test]
    fn runtime_event_queue_keeps_capacity_and_drop_accounting_stable() {
        let mut queue = VecDeque::new();
        let mut dropped = 0;
        for _ in 0..(MAX_PENDING_RUNTIME_EVENTS + 3) {
            push_runtime_event_bounded(&mut queue, &mut dropped, PlayerRuntimeEvent::Ended);
        }
        extend_runtime_events_bounded(
            &mut queue,
            &mut dropped,
            [PlayerRuntimeEvent::Ended, PlayerRuntimeEvent::Ended],
        );

        assert_eq!(queue.len(), MAX_PENDING_RUNTIME_EVENTS);
        assert_eq!(dropped, 5);
    }

    #[test]
    fn playback_events_forward_stable_wire_values_and_redact_sensitive_text() {
        let mut media_info = test_media_info(
            MediaSourceKind::Remote,
            MediaSourceProtocol::Hls,
            Some(Duration::from_secs(30)),
        );
        media_info.source_uri = "https://secret.example/video.m3u8?token=secret".to_owned();
        let error_message = "decode failed for https://secret.example/video.m3u8?token=secret";
        let events = vec![
            PlayerRuntimeEvent::Initialized(test_startup()),
            PlayerRuntimeEvent::MetadataReady(media_info.clone()),
            PlayerRuntimeEvent::FirstFrameReady(FirstFrameReady {
                presentation_time: Duration::from_millis(33),
                width: 1920,
                height: 1080,
            }),
            PlayerRuntimeEvent::PlaybackStateChanged(PresentationState::Playing),
            PlayerRuntimeEvent::InterruptionChanged { interrupted: true },
            PlayerRuntimeEvent::BufferingChanged { buffering: true },
            PlayerRuntimeEvent::VideoSurfaceChanged { attached: true },
            PlayerRuntimeEvent::AudioOutputChanged(Some(PlayerAudioOutputInfo {
                device_name: Some("secret-device".to_owned()),
                channels: Some(2),
                sample_rate: Some(48_000),
                sample_format: Some("float".to_owned()),
            })),
            PlayerRuntimeEvent::PlaybackRateChanged { rate: 1.25 },
            PlayerRuntimeEvent::SeekCompleted {
                position: Duration::from_secs(4),
            },
            PlayerRuntimeEvent::RetryScheduled {
                attempt: 2,
                delay: Duration::from_millis(500),
            },
            playback_warning_event(),
            PlayerRuntimeEvent::Error(PlayerError::new(
                PlayerErrorCode::DecodeFailure,
                error_message,
            )),
            PlayerRuntimeEvent::Ended,
        ];
        let (dispatcher, captured) = capturing_dispatcher();
        let mut runtime = super::PlayerRuntime::from_adapter_bootstrap_with_pipeline(
            "fake-adapter",
            fake_bootstrap(media_info, events.clone()),
            Some(dispatcher),
            "host.test".to_owned(),
        )
        .runtime;

        assert_eq!(runtime.drain_events().len(), events.len());
        assert!(runtime.flush_pipeline_event_hooks(Duration::from_secs(1)));
        let captured = captured
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone();
        let names = captured
            .iter()
            .map(|event| event.event_name.as_str())
            .collect::<Vec<_>>();
        assert_eq!(
            names,
            vec![
                "playback.initialized",
                "playback.metadata_ready",
                "playback.first_frame_ready",
                "playback.state_changed",
                "playback.interruption_changed",
                "playback.buffering_changed",
                "playback.video_surface_changed",
                "playback.audio_output_changed",
                "playback.rate_changed",
                "playback.seek_completed",
                "playback.retry_scheduled",
                "playback.warning",
                "playback.error",
                "playback.ended",
            ]
        );
        for event in &captured {
            assert!(event.run_id.starts_with("playback-session:"));
            assert_eq!(event.run_id, event.session_id);
            assert_eq!(
                event.resource_identity.as_deref(),
                Some(event.session_id.as_str())
            );
            assert_eq!(event.platform, "host.test");
            assert_eq!(event.protocol.as_deref(), Some("hls"));
            assert!(
                event
                    .attributes
                    .values()
                    .all(|value| !value.contains("secret"))
            );
        }
        let metadata = &captured[1];
        assert_eq!(
            metadata.attributes.get("sourceKind"),
            Some(&"remote".to_owned())
        );
        assert_eq!(
            metadata.attributes.get("sourceProtocol"),
            Some(&"hls".to_owned())
        );
        assert_eq!(
            captured[3].attributes.get("state"),
            Some(&"playing".to_owned())
        );
        assert_eq!(
            captured[11].attributes.get("domain"),
            Some(&"frameProcessor".to_owned())
        );
        let error = &captured[12];
        assert_eq!(
            error.attributes.get("code"),
            Some(&"decodeFailure".to_owned())
        );
        assert_eq!(error.attributes.get("category"), Some(&"decode".to_owned()));
        assert_eq!(
            error
                .diagnostic
                .as_ref()
                .map(|diagnostic| diagnostic.message.as_str()),
            Some("playback runtime error")
        );
        assert!(!format!("{captured:?}").contains("secret.example"));

        let reports = runtime.drain_pipeline_event_hook_reports();
        assert_eq!(reports.reports.len(), events.len());
        assert_eq!(reports.dropped_events, 0);
        assert!(reports.reports.iter().all(|report| report.result.is_ok()));
        assert!(runtime.close_pipeline_event_hooks());
        assert!(runtime.close_pipeline_event_hooks());
    }

    #[test]
    fn playback_event_batch_cap_reports_omitted_events() {
        let media_info = test_media_info(
            MediaSourceKind::Remote,
            MediaSourceProtocol::Progressive,
            None,
        );
        let events = (0..(MAX_PLAYBACK_PIPELINE_EVENTS_PER_DRAIN + 7))
            .map(|_| PlayerRuntimeEvent::Ended)
            .collect::<Vec<_>>();
        let (dispatcher, captured) = capturing_dispatcher();
        let mut runtime = super::PlayerRuntime::from_adapter_bootstrap_with_pipeline(
            "fake-adapter",
            fake_bootstrap(media_info, events.clone()),
            Some(dispatcher),
            "host.test".to_owned(),
        )
        .runtime;

        assert_eq!(runtime.drain_events().len(), events.len());
        assert!(runtime.flush_pipeline_event_hooks(Duration::from_secs(1)));
        assert_eq!(
            captured
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .len(),
            MAX_PLAYBACK_PIPELINE_EVENTS_PER_DRAIN
        );
        let reports = runtime.drain_pipeline_event_hook_reports();
        assert_eq!(reports.dropped_events, 7);
        assert_eq!(
            reports.reports.len(),
            MAX_PLAYBACK_PIPELINE_EVENTS_PER_DRAIN
        );
    }

    #[test]
    fn dropping_runtime_does_not_close_a_shared_dispatcher() {
        let (dispatcher, captured) = capturing_dispatcher();
        let media_info = test_media_info(
            MediaSourceKind::Remote,
            MediaSourceProtocol::Hls,
            Some(Duration::from_secs(1)),
        );
        let runtime = super::PlayerRuntime::from_adapter_bootstrap_with_pipeline(
            "fake-adapter",
            fake_bootstrap(media_info, Vec::new()),
            Some(dispatcher.clone()),
            "host.test".to_owned(),
        )
        .runtime;
        drop(runtime);

        dispatcher.enqueue(valid_pipeline_event());
        assert!(dispatcher.flush(Duration::from_secs(1)));
        assert_eq!(
            captured
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .len(),
            1
        );
        assert!(dispatcher.close());
    }

    #[test]
    fn wrapping_runtime_adapter_preserves_pipeline_dispatcher() {
        let (dispatcher, captured) = capturing_dispatcher();
        let media_info = test_media_info(
            MediaSourceKind::Remote,
            MediaSourceProtocol::Hls,
            Some(Duration::from_secs(1)),
        );
        let event = PlayerRuntimeEvent::Ended;
        let mut runtime = super::PlayerRuntime::from_adapter_bootstrap_with_pipeline(
            "fake-adapter",
            PlayerRuntimeAdapterBootstrap {
                runtime: Box::new(FakeAdapter {
                    media_info,
                    events: vec![event],
                    dropped_events: 3,
                }),
                initial_frame: None,
                startup: test_startup(),
            },
            Some(dispatcher),
            "host.test".to_owned(),
        )
        .runtime
        .wrap_adapter(|inner| Box::new(ForwardingRuntimeAdapter { inner }));

        assert_eq!(runtime.drain_events().len(), 1);
        assert_eq!(runtime.take_dropped_event_count(), 3);
        assert!(runtime.flush_pipeline_event_hooks(Duration::from_secs(1)));
        assert_eq!(
            captured
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .len(),
            1
        );
        assert_eq!(
            runtime.drain_pipeline_event_hook_reports().dropped_events,
            3
        );
    }

    #[test]
    fn probing_preserves_platform_option_for_factory_and_rejects_oversized_labels() {
        let seen_platform = Arc::new(Mutex::new(None));
        let factory = FakeFactory {
            seen_platform: seen_platform.clone(),
            media_info: test_media_info(
                MediaSourceKind::Remote,
                MediaSourceProtocol::Hls,
                Some(Duration::from_secs(1)),
            ),
            events: Vec::new(),
        };
        let initializer = PlayerRuntimeInitializer::probe_source_with_factory(
            player_model::MediaSource::new("https://example.test/video.m3u8"),
            PlayerRuntimeOptions::default().with_pipeline_event_platform("custom.test"),
            &factory,
        )
        .expect("factory probe should succeed");
        assert_eq!(
            *seen_platform
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner()),
            Some("custom.test".to_owned())
        );
        let _ = initializer
            .initialize()
            .expect("fake initializer should initialize");

        let oversized = "x".repeat(player_plugin::MAX_PLUGIN_PLATFORM_BYTES + 1);
        let error = PlayerRuntimeInitializer::probe_source_with_factory(
            player_model::MediaSource::new("https://example.test/video.m3u8"),
            PlayerRuntimeOptions::default().with_pipeline_event_platform(oversized),
            &factory,
        )
        .expect_err("oversized platform should be rejected at the runtime boundary");
        assert_eq!(error.code(), PlayerErrorCode::InvalidArgument);
        assert_eq!(error.category(), PlayerErrorCategory::Input);
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
    fn timeline_from_media_info_defaults_rtmp_and_rtsp_to_live() {
        for protocol in [MediaSourceProtocol::Rtmp, MediaSourceProtocol::Rtsp] {
            let media_info = test_media_info(
                MediaSourceKind::Remote,
                protocol,
                Some(Duration::from_secs(600)),
            );
            let timeline = PlayerTimelineSnapshot::from_media_info(
                PlaybackProgress::new(Duration::from_secs(30), Some(Duration::from_secs(600))),
                true,
                &media_info,
            );

            assert_eq!(timeline.kind, PlayerTimelineKind::Live, "{protocol:?}");
            assert!(!timeline.is_seekable, "{protocol:?}");
            assert_eq!(timeline.duration, None, "{protocol:?}");
            assert_eq!(timeline.seekable_range, None, "{protocol:?}");
        }
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
        assert_eq!(error.code(), PlayerErrorCode::SeekFailure);
        assert_eq!(error.category(), PlayerErrorCategory::Playback);
    }

    #[test]
    fn runtime_options_default_to_shared_resilience_baseline() {
        let options = PlayerRuntimeOptions::default();

        assert_eq!(
            options.native_plugin_loading_policy,
            NativePluginLoadingPolicy::DenyRawPaths
        );
        assert!(
            options
                .validate_native_plugin_loading_policy("runtime")
                .is_err()
        );
        assert_eq!(
            options.source_normalizer_mode,
            SourceNormalizerMode::Disabled
        );
        assert!(options.source_normalizer_plugin_library_paths.is_empty());
        assert_eq!(options.frame_processor_mode, FrameProcessorMode::Disabled);
        assert!(options.frame_processor_library_paths.is_empty());
        assert_eq!(
            options.frame_processor_policy,
            FrameProcessorPolicy::default()
        );
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
    fn native_plugin_loading_requires_an_explicit_policy() {
        let options = PlayerRuntimeOptions::default()
            .with_decoder_plugin_library_paths([PathBuf::from("/tmp/untrusted-plugin.dylib")]);
        let error = options
            .validate_native_plugin_loading_policy("decoder")
            .expect_err("raw paths must be denied by default");
        assert_eq!(error.surface(), "decoder");
        assert_eq!(error.policy(), NativePluginLoadingPolicy::DenyRawPaths);

        let development = options.clone().with_development_native_plugin_loading();
        assert!(
            development
                .validate_native_plugin_loading_policy("decoder")
                .is_ok()
        );
        assert_eq!(
            development.native_plugin_loading_policy.wire_name(),
            "development-raw-paths"
        );
    }

    #[test]
    fn playback_route_wire_names_match_apple_shared_contract() {
        assert_eq!(
            PlayerPlaybackRoute::SystemPlayer.wire_name(),
            "systemPlayer"
        );
        assert_eq!(
            PlayerPlaybackRoute::from_wire_name("systemPlayer"),
            Some(PlayerPlaybackRoute::SystemPlayer)
        );
        assert_eq!(
            PlayerPlaybackRoute::SdkManagedNativeFrame.wire_name(),
            "sdkManagedNativeFrame"
        );
        assert_eq!(
            PlayerPlaybackRoute::from_wire_name("sdkManagedNativeFrame"),
            Some(PlayerPlaybackRoute::SdkManagedNativeFrame)
        );
        assert_eq!(
            PlayerPlaybackRoute::SoftwareDecoder.wire_name(),
            "softwareDecoder"
        );
        assert_eq!(
            PlayerPlaybackRoute::from_wire_name("softwareDecoder"),
            Some(PlayerPlaybackRoute::SoftwareDecoder)
        );
        assert_eq!(PlayerPlaybackRoute::from_wire_name("nativeFrame"), None);
    }

    #[test]
    fn plugin_diagnostic_status_wire_names_match_shared_contract() {
        assert_eq!(PlayerPluginDiagnosticStatus::Loaded.wire_name(), "loaded");
        assert_eq!(
            PlayerPluginDiagnosticStatus::from_wire_name("loaded"),
            Some(PlayerPluginDiagnosticStatus::Loaded)
        );
        assert_eq!(
            PlayerPluginDiagnosticStatus::LoadFailed.wire_name(),
            "loadFailed"
        );
        assert_eq!(
            PlayerPluginDiagnosticStatus::from_wire_name("loadFailed"),
            Some(PlayerPluginDiagnosticStatus::LoadFailed)
        );
        assert_eq!(
            PlayerPluginDiagnosticStatus::UnsupportedKind.wire_name(),
            "unsupportedKind"
        );
        assert_eq!(
            PlayerPluginDiagnosticStatus::from_wire_name("unsupportedKind"),
            Some(PlayerPluginDiagnosticStatus::UnsupportedKind)
        );
        assert_eq!(
            PlayerPluginDiagnosticStatus::DecoderSupported.wire_name(),
            "decoderSupported"
        );
        assert_eq!(
            PlayerPluginDiagnosticStatus::from_wire_name("decoderSupported"),
            Some(PlayerPluginDiagnosticStatus::DecoderSupported)
        );
        assert_eq!(
            PlayerPluginDiagnosticStatus::DecoderUnsupported.wire_name(),
            "decoderUnsupported"
        );
        assert_eq!(
            PlayerPluginDiagnosticStatus::from_wire_name("decoderUnsupported"),
            Some(PlayerPluginDiagnosticStatus::DecoderUnsupported)
        );
        assert_eq!(
            PlayerPluginDiagnosticStatus::FrameProcessorSupported.wire_name(),
            "frameProcessorSupported"
        );
        assert_eq!(
            PlayerPluginDiagnosticStatus::from_wire_name("frameProcessorSupported"),
            Some(PlayerPluginDiagnosticStatus::FrameProcessorSupported)
        );
        assert_eq!(
            PlayerPluginDiagnosticStatus::FrameProcessorUnsupported.wire_name(),
            "frameProcessorUnsupported"
        );
        assert_eq!(
            PlayerPluginDiagnosticStatus::from_wire_name("frameProcessorUnsupported"),
            Some(PlayerPluginDiagnosticStatus::FrameProcessorUnsupported)
        );
        assert_eq!(
            PlayerPluginDiagnosticStatus::SourceNormalizerSupported.wire_name(),
            "sourceNormalizerSupported"
        );
        assert_eq!(
            PlayerPluginDiagnosticStatus::from_wire_name("sourceNormalizerSupported"),
            Some(PlayerPluginDiagnosticStatus::SourceNormalizerSupported)
        );
        assert_eq!(
            PlayerPluginDiagnosticStatus::SourceNormalizerUnsupported.wire_name(),
            "sourceNormalizerUnsupported"
        );
        assert_eq!(
            PlayerPluginDiagnosticStatus::from_wire_name("sourceNormalizerUnsupported"),
            Some(PlayerPluginDiagnosticStatus::SourceNormalizerUnsupported)
        );
        assert_eq!(
            PlayerPluginDiagnosticStatus::from_wire_name("decoder-supported"),
            None
        );
    }

    #[test]
    fn plugin_participation_wire_names_match_shared_contract() {
        assert_eq!(PlayerPluginParticipation::Unknown.wire_name(), "unknown");
        assert_eq!(
            PlayerPluginParticipation::from_wire_name("unknown"),
            Some(PlayerPluginParticipation::Unknown)
        );
        assert_eq!(
            PlayerPluginParticipation::Available.wire_name(),
            "available"
        );
        assert_eq!(
            PlayerPluginParticipation::from_wire_name("available"),
            Some(PlayerPluginParticipation::Available)
        );
        assert_eq!(PlayerPluginParticipation::Selected.wire_name(), "selected");
        assert_eq!(
            PlayerPluginParticipation::from_wire_name("selected"),
            Some(PlayerPluginParticipation::Selected)
        );
        assert_eq!(
            PlayerPluginParticipation::Participated.wire_name(),
            "participated"
        );
        assert_eq!(
            PlayerPluginParticipation::from_wire_name("participated"),
            Some(PlayerPluginParticipation::Participated)
        );
        assert_eq!(PlayerPluginParticipation::Bypassed.wire_name(), "bypassed");
        assert_eq!(
            PlayerPluginParticipation::from_wire_name("bypassed"),
            Some(PlayerPluginParticipation::Bypassed)
        );
        assert_eq!(PlayerPluginParticipation::Fallback.wire_name(), "fallback");
        assert_eq!(
            PlayerPluginParticipation::from_wire_name("fallback"),
            Some(PlayerPluginParticipation::Fallback)
        );
        assert_eq!(
            PlayerPluginParticipation::from_wire_name("participating"),
            None
        );
    }

    #[test]
    fn runtime_options_builder_sets_frame_processor_mode() {
        let options = PlayerRuntimeOptions::default()
            .with_source_normalizer_mode(SourceNormalizerMode::PreflightOnly)
            .with_source_normalizer_plugin_library_paths([std::path::PathBuf::from(
                "/tmp/source-normalizer",
            )])
            .with_frame_processor_mode(FrameProcessorMode::RequireProcessed)
            .with_frame_processor_library_paths([std::path::PathBuf::from("/tmp/frame-processor")])
            .with_frame_processor_policy(FrameProcessorPolicy {
                frame_deadline: Duration::from_millis(8),
                late_output_tolerance: Duration::from_millis(2),
                max_chain_depth: 2,
                max_in_flight_frames_per_processor: 1,
            });

        assert_eq!(
            options.source_normalizer_mode,
            SourceNormalizerMode::PreflightOnly
        );
        assert_eq!(
            options.source_normalizer_plugin_library_paths,
            vec![std::path::PathBuf::from("/tmp/source-normalizer")]
        );
        assert_eq!(
            options.frame_processor_mode,
            FrameProcessorMode::RequireProcessed
        );
        assert_eq!(
            options.frame_processor_library_paths,
            vec![std::path::PathBuf::from("/tmp/frame-processor")]
        );
        assert_eq!(
            options.frame_processor_policy.frame_deadline,
            Duration::from_millis(8)
        );
    }

    #[test]
    fn frame_processor_warning_round_trips_through_json() {
        let warning = PlayerRuntimeWarning::FrameProcessor(FrameProcessorWarning {
            kind: FrameProcessorWarningKind::DeadlineMissed,
            plugin_name: "fixture-denoise".to_owned(),
            processor_index: 1,
            frame_id: Some(123),
            frame_pts_us: Some(66_000),
            frame_duration_us: Some(33_333),
            input_handle_kind: Some("CvPixelBuffer".to_owned()),
            output_handle_kind: Some("CvPixelBuffer".to_owned()),
            queue_depth: Some(2),
            in_flight_frames: Some(1),
            queue_wait_us: Some(1_200),
            process_time_us: Some(9_800),
            submit_to_ready_us: Some(11_000),
            present_deadline_us: Some(75_000),
            deadline_overrun_us: Some(700),
            consecutive_miss_count: Some(3),
            policy_action: FrameProcessorPolicyAction::BypassOriginalFrame,
            message: Some("processed frame missed presenter deadline".to_owned()),
        });

        let encoded = serde_json::to_string(&warning).expect("serialize warning");
        let decoded: PlayerRuntimeWarning =
            serde_json::from_str(&encoded).expect("deserialize warning");

        assert_eq!(decoded, warning);
        assert_eq!(decoded.domain(), PlayerRuntimeWarningDomain::FrameProcessor);
    }

    #[test]
    fn runtime_event_can_carry_frame_processor_warning() {
        let event = PlayerRuntimeEvent::Warning(PlayerRuntimeWarning::FrameProcessor(
            FrameProcessorWarning {
                kind: FrameProcessorWarningKind::BypassActivated,
                plugin_name: "fixture-upscale".to_owned(),
                processor_index: 0,
                frame_id: Some(7),
                frame_pts_us: Some(42_000),
                frame_duration_us: None,
                input_handle_kind: None,
                output_handle_kind: None,
                queue_depth: None,
                in_flight_frames: None,
                queue_wait_us: None,
                process_time_us: None,
                submit_to_ready_us: None,
                present_deadline_us: None,
                deadline_overrun_us: None,
                consecutive_miss_count: Some(5),
                policy_action: FrameProcessorPolicyAction::BypassOriginalFrame,
                message: None,
            },
        ));

        match event {
            PlayerRuntimeEvent::Warning(PlayerRuntimeWarning::FrameProcessor(warning)) => {
                assert_eq!(warning.processor_index, 0);
                assert_eq!(warning.kind, FrameProcessorWarningKind::BypassActivated);
            }
            other => panic!("expected frame processor warning event, got {other:?}"),
        }
    }

    #[test]
    fn frame_processing_metrics_default_is_empty() {
        assert_eq!(
            PlayerFrameProcessingMetrics::default(),
            PlayerFrameProcessingMetrics {
                submitted_frame_count: 0,
                processed_frame_count: 0,
                bypassed_frame_count: 0,
                dropped_output_count: 0,
                deadline_miss_count: 0,
                late_output_drop_count: 0,
                backpressure_count: 0,
                disabled_processor_count: 0,
                max_queue_depth: None,
                max_in_flight_frames: None,
                last_queue_wait_us: None,
                last_process_time_us: None,
                last_submit_to_ready_us: None,
            }
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

    #[test]
    fn plugin_budget_defaults_kick_in_when_limits_are_unset() {
        let policy = PluginBudgetPolicy::default();
        assert_eq!(
            policy.effective_max_queue_depth(),
            DEFAULT_PLUGIN_MAX_QUEUE_DEPTH
        );
        assert_eq!(
            policy.effective_max_in_flight_frames(),
            DEFAULT_PLUGIN_MAX_IN_FLIGHT_FRAMES
        );
        assert_eq!(
            policy.effective_max_process_time_us(),
            DEFAULT_PLUGIN_MAX_PROCESS_TIME_US
        );
        assert_eq!(
            policy.effective_max_consecutive_failures(),
            DEFAULT_PLUGIN_MAX_CONSECUTIVE_FAILURES
        );
    }

    #[test]
    fn plugin_budget_respects_explicit_overrides() {
        let policy = PluginBudgetPolicy {
            max_queue_depth: Some(32),
            max_in_flight_frames: Some(8),
            max_process_time_us: Some(5_000),
            max_consecutive_failures: Some(2),
        };
        assert_eq!(policy.effective_max_queue_depth(), 32);
        assert_eq!(policy.effective_max_in_flight_frames(), 8);
        assert_eq!(policy.effective_max_process_time_us(), 5_000);
        assert_eq!(policy.effective_max_consecutive_failures(), 2);
    }

    #[test]
    fn breaker_trips_after_consecutive_failures() {
        let mut breaker = PluginBreakerState::new(PluginBudgetPolicy {
            max_consecutive_failures: Some(3),
            ..PluginBudgetPolicy::default()
        });

        // First two failures drop output but keep the chain alive.
        assert_eq!(
            breaker.record_failure(),
            FrameProcessorPolicyAction::DropOutput
        );
        assert!(!breaker.is_disabled());
        assert_eq!(
            breaker.record_failure(),
            FrameProcessorPolicyAction::DropOutput
        );
        assert!(!breaker.is_disabled());

        // Third failure trips the breaker.
        assert_eq!(
            breaker.record_failure(),
            FrameProcessorPolicyAction::DisableProcessor
        );
        assert!(breaker.is_disabled());
        assert_eq!(breaker.consecutive_failures(), 3);

        // Subsequent observations bypass the plugin instead of re-tripping.
        assert_eq!(
            breaker.record_failure(),
            FrameProcessorPolicyAction::BypassOriginalFrame
        );
        assert_eq!(
            breaker.evaluate_queue_depth(100),
            FrameProcessorPolicyAction::BypassOriginalFrame
        );
    }

    #[test]
    fn breaker_counts_deadline_misses_towards_failure_threshold() {
        let mut breaker = PluginBreakerState::new(PluginBudgetPolicy {
            max_consecutive_failures: Some(2),
            ..PluginBudgetPolicy::default()
        });

        assert_eq!(
            breaker.record_deadline_miss(),
            FrameProcessorPolicyAction::Continue
        );
        assert_eq!(breaker.consecutive_deadline_misses(), 1);
        // Deadline misses count towards consecutive failures and trip too.
        assert_eq!(
            breaker.record_deadline_miss(),
            FrameProcessorPolicyAction::DisableProcessor
        );
        assert!(breaker.is_disabled());
        assert_eq!(breaker.consecutive_deadline_misses(), 2);
    }

    #[test]
    fn breaker_record_success_clears_failure_counters() {
        let mut breaker = PluginBreakerState::new(PluginBudgetPolicy {
            max_consecutive_failures: Some(4),
            ..PluginBudgetPolicy::default()
        });

        breaker.record_failure();
        breaker.record_failure();
        breaker.record_deadline_miss();
        breaker.record_success();
        assert_eq!(breaker.consecutive_failures(), 0);
        assert_eq!(breaker.consecutive_deadline_misses(), 0);
        assert!(!breaker.is_disabled());

        // Two more failures after reset should not trip yet (threshold is 4).
        breaker.record_failure();
        breaker.record_failure();
        assert!(!breaker.is_disabled());
    }

    #[test]
    fn breaker_load_evaluates_pressure_without_tripping() {
        let breaker = PluginBreakerState::new(PluginBudgetPolicy {
            max_queue_depth: Some(8),
            max_in_flight_frames: Some(2),
            max_consecutive_failures: Some(3),
            ..PluginBudgetPolicy::default()
        });

        // Within budget: continue.
        assert_eq!(
            breaker.evaluate_queue_depth(8),
            FrameProcessorPolicyAction::Continue
        );
        // Over budget: drop output but do not touch failure counters.
        assert_eq!(
            breaker.evaluate_queue_depth(9),
            FrameProcessorPolicyAction::DropOutput
        );
        // In-flight pressure uses its own cap.
        assert_eq!(
            breaker.evaluate_in_flight_frames(3),
            FrameProcessorPolicyAction::DropOutput
        );
        assert_eq!(
            breaker.evaluate_load(Some(8), Some(2)),
            FrameProcessorPolicyAction::Continue
        );
        assert_eq!(
            breaker.evaluate_load(Some(9), Some(2)),
            FrameProcessorPolicyAction::DropOutput
        );
        assert_eq!(
            breaker.evaluate_load(Some(8), Some(3)),
            FrameProcessorPolicyAction::DropOutput
        );
        assert_eq!(breaker.consecutive_failures(), 0);
        assert!(!breaker.is_disabled());

        // Backpressure alone does not trip the breaker even when sustained.
        for _ in 0..10 {
            assert_eq!(
                breaker.evaluate_queue_depth(100),
                FrameProcessorPolicyAction::DropOutput
            );
        }
        assert!(!breaker.is_disabled());
    }

    #[test]
    fn breaker_reset_recovers_a_disabled_plugin() {
        let mut breaker = PluginBreakerState::new(PluginBudgetPolicy {
            max_consecutive_failures: Some(1),
            ..PluginBudgetPolicy::default()
        });

        breaker.record_failure();
        assert!(breaker.is_disabled());

        breaker.reset();
        assert!(!breaker.is_disabled());
        assert_eq!(breaker.consecutive_failures(), 0);
        assert_eq!(
            breaker.evaluate_queue_depth(1),
            FrameProcessorPolicyAction::Continue
        );
    }
}
