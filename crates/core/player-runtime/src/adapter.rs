use std::time::Instant;

use player_model::MediaSource;

use crate::{
    DecodedVideoFrame, PlaybackProgress, PlayerError, PlayerErrorCode, PlayerMediaInfo,
    PlayerResilienceMetrics, PlayerResult, PlayerRuntimeAdapterCapabilities, PlayerRuntimeCommand,
    PlayerRuntimeCommandResult, PlayerRuntimeEvent, PlayerRuntimeOptions, PlayerRuntimeStartup,
    PlayerSnapshot, PlayerTimelineSnapshot, PlayerVideoSurfaceTarget, PresentationState,
};

/// Result of creating a concrete runtime adapter.
pub struct PlayerRuntimeAdapterBootstrap {
    /// Runtime adapter that owns backend playback state.
    pub runtime: Box<dyn PlayerRuntimeAdapter>,
    /// Optional decoded frame available immediately after initialization.
    pub initial_frame: Option<DecodedVideoFrame>,
    /// Startup diagnostics collected while creating the adapter.
    pub startup: PlayerRuntimeStartup,
}

/// Deferred adapter initialization returned after source probing.
pub trait PlayerRuntimeAdapterInitializer: Send {
    /// Capabilities discovered during probing.
    fn capabilities(&self) -> PlayerRuntimeAdapterCapabilities;
    /// Media information discovered during probing.
    fn media_info(&self) -> PlayerMediaInfo;
    /// Startup diagnostics available before full initialization.
    fn startup(&self) -> PlayerRuntimeStartup;
    /// Performs any blocking backend startup required to create the adapter.
    ///
    /// Callers that run on a UI thread are responsible for moving this work to a
    /// background thread before invoking it.
    fn initialize(self: Box<Self>) -> PlayerResult<PlayerRuntimeAdapterBootstrap>;
}

/// Factory that probes media sources and creates runtime initializers.
pub trait PlayerRuntimeAdapterFactory: Sync + Send {
    /// Stable adapter id used in diagnostics and runtime wrappers.
    fn adapter_id(&self) -> &'static str;
    /// Probes a source and returns an initializer for a concrete runtime.
    ///
    /// This method may perform blocking media or network probing. UI hosts
    /// should call it from a background thread.
    fn probe_source_with_options(
        &self,
        source: MediaSource,
        options: PlayerRuntimeOptions,
    ) -> PlayerResult<Box<dyn PlayerRuntimeAdapterInitializer>>;
}

/// Backend playback contract used by [`crate::PlayerRuntime`].
pub trait PlayerRuntimeAdapter: Send {
    /// Returns the source URI currently owned by this adapter.
    fn source_uri(&self) -> &str;
    /// Returns current adapter capabilities.
    fn capabilities(&self) -> PlayerRuntimeAdapterCapabilities;
    /// Returns current media information.
    fn media_info(&self) -> &PlayerMediaInfo;
    /// Returns current presentation state.
    fn presentation_state(&self) -> PresentationState;
    /// Returns whether an external video surface is attached.
    fn has_video_surface(&self) -> bool {
        false
    }
    /// Returns whether playback is interrupted by the host platform.
    fn is_interrupted(&self) -> bool {
        false
    }
    /// Returns whether the adapter is currently buffering.
    fn is_buffering(&self) -> bool {
        false
    }
    /// Returns the current playback rate.
    fn playback_rate(&self) -> f32;
    /// Returns current playback progress.
    fn progress(&self) -> PlaybackProgress;
    /// Drains pending adapter events.
    ///
    /// This must be called serially on the same adapter-owner thread that calls
    /// [`advance`](Self::advance). Implementations may mutate internal event
    /// queues and do not need to be reentrant.
    fn drain_events(&mut self) -> Vec<PlayerRuntimeEvent>;
    /// Applies a runtime command.
    ///
    /// Calls must be serialized with [`advance`](Self::advance) and
    /// [`drain_events`](Self::drain_events). Hosts may dispatch commands from
    /// any thread only after funneling them through the adapter-owner thread.
    fn dispatch(
        &mut self,
        command: PlayerRuntimeCommand,
    ) -> PlayerResult<PlayerRuntimeCommandResult>;
    /// Replaces the host-owned video surface when the adapter supports it.
    fn replace_video_surface(
        &mut self,
        _video_surface: Option<PlayerVideoSurfaceTarget>,
    ) -> PlayerResult<()> {
        Err(PlayerError::new(
            PlayerErrorCode::Unsupported,
            "this runtime adapter does not support replacing external video surfaces",
        ))
    }
    /// Advances decoding/presentation state.
    ///
    /// This method must be called serially from one adapter-owner thread. It may
    /// poll worker channels and should not be invoked concurrently with command
    /// dispatch or event draining.
    fn advance(&mut self) -> PlayerResult<Option<DecodedVideoFrame>>;
    /// Returns the next time the host should call [`advance`](Self::advance).
    fn next_deadline(&self) -> Option<Instant>;

    /// Builds a snapshot from the adapter's current query methods.
    fn snapshot(&self) -> PlayerSnapshot {
        PlayerSnapshot {
            source_uri: self.source_uri().to_owned(),
            state: self.presentation_state(),
            has_video_surface: self.has_video_surface(),
            is_interrupted: self.is_interrupted(),
            is_buffering: self.is_buffering(),
            playback_rate: self.playback_rate(),
            progress: self.progress(),
            timeline: PlayerTimelineSnapshot::from_media_info(
                self.progress(),
                self.capabilities().supports_seek,
                self.media_info(),
            ),
            media_info: self.media_info().clone(),
            resilience_metrics: PlayerResilienceMetrics::default(),
        }
    }
}
