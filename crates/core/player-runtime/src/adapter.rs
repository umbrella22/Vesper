use std::time::Instant;

use player_model::MediaSource;

use crate::{
    DecodedVideoFrame, PlaybackProgress, PlayerMediaInfo, PlayerResilienceMetrics,
    PlayerRuntimeAdapterCapabilities, PlayerRuntimeCommand, PlayerRuntimeCommandResult,
    PlayerRuntimeError, PlayerRuntimeErrorCode, PlayerRuntimeEvent, PlayerRuntimeOptions,
    PlayerRuntimeResult, PlayerRuntimeStartup, PlayerSnapshot, PlayerTimelineSnapshot,
    PlayerVideoSurfaceTarget, PresentationState,
};

pub struct PlayerRuntimeAdapterBootstrap {
    pub runtime: Box<dyn PlayerRuntimeAdapter>,
    pub initial_frame: Option<DecodedVideoFrame>,
    pub startup: PlayerRuntimeStartup,
}

pub trait PlayerRuntimeAdapterInitializer: Send {
    fn capabilities(&self) -> PlayerRuntimeAdapterCapabilities;
    fn media_info(&self) -> PlayerMediaInfo;
    fn startup(&self) -> PlayerRuntimeStartup;
    /// Performs any blocking backend startup required to create the adapter.
    ///
    /// Callers that run on a UI thread are responsible for moving this work to a
    /// background thread before invoking it.
    fn initialize(self: Box<Self>) -> PlayerRuntimeResult<PlayerRuntimeAdapterBootstrap>;
}

pub trait PlayerRuntimeAdapterFactory: Sync + Send {
    fn adapter_id(&self) -> &'static str;
    /// Probes a source and returns an initializer for a concrete runtime.
    ///
    /// This method may perform blocking media or network probing. UI hosts
    /// should call it from a background thread.
    fn probe_source_with_options(
        &self,
        source: MediaSource,
        options: PlayerRuntimeOptions,
    ) -> PlayerRuntimeResult<Box<dyn PlayerRuntimeAdapterInitializer>>;
}

pub trait PlayerRuntimeAdapter: Send {
    fn source_uri(&self) -> &str;
    fn capabilities(&self) -> PlayerRuntimeAdapterCapabilities;
    fn media_info(&self) -> &PlayerMediaInfo;
    fn presentation_state(&self) -> PresentationState;
    fn has_video_surface(&self) -> bool {
        false
    }
    fn is_interrupted(&self) -> bool {
        false
    }
    fn is_buffering(&self) -> bool {
        false
    }
    fn playback_rate(&self) -> f32;
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
    ) -> PlayerRuntimeResult<PlayerRuntimeCommandResult>;
    fn replace_video_surface(
        &mut self,
        _video_surface: Option<PlayerVideoSurfaceTarget>,
    ) -> PlayerRuntimeResult<()> {
        Err(PlayerRuntimeError::new(
            PlayerRuntimeErrorCode::Unsupported,
            "this runtime adapter does not support replacing external video surfaces",
        ))
    }
    /// Advances decoding/presentation state.
    ///
    /// This method must be called serially from one adapter-owner thread. It may
    /// poll worker channels and should not be invoked concurrently with command
    /// dispatch or event draining.
    fn advance(&mut self) -> PlayerRuntimeResult<Option<DecodedVideoFrame>>;
    fn next_deadline(&self) -> Option<Instant>;

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
