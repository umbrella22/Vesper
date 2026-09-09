//! Runtime option types and plugin rollout configuration.

use std::fmt;
use std::path::PathBuf;
use std::time::Duration;

use player_download::{PipelineEventDispatcher, PipelineEventHookRegistration};
use player_model::{MediaSourceKind, MediaSourceProtocol};
use player_plugin::AudioPitchMode as PitchMode;
use player_preload::{PlayerPreloadBudgetPolicy, PlayerResolvedPreloadBudgetPolicy};

use crate::policy::{
    PlayerBufferingPolicy, PlayerCachePolicy, PlayerResolvedResiliencePolicy, PlayerRetryPolicy,
    PlayerTrackPreferencePolicy,
};
use crate::{
    DEFAULT_VIDEO_IDLE_POLL_INTERVAL, DEFAULT_VIDEO_PREFETCH_CAPACITY,
    DEFAULT_VIDEO_PRESENT_EARLY_TOLERANCE, PlayerVideoSurfaceTarget,
};

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
    /// Native audio processor plugin paths used only by explicit desktop processing routes.
    pub audio_processor_library_paths: Vec<PathBuf>,
    /// Pitch behavior for desktop audio processing and FFmpeg fallback.
    pub audio_pitch_mode: PitchMode,
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
            audio_processor_library_paths: Vec::new(),
            audio_pitch_mode: PitchMode::PreservePitch,
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

    /// Replaces Native audio processor paths for the experimental desktop route.
    pub fn with_audio_processor_library_paths(
        mut self,
        paths: impl IntoIterator<Item = PathBuf>,
    ) -> Self {
        self.audio_processor_library_paths = paths.into_iter().collect();
        self
    }

    /// Sets desktop audio pitch behavior.
    pub fn with_audio_pitch_mode(mut self, mode: PitchMode) -> Self {
        self.audio_pitch_mode = mode;
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
