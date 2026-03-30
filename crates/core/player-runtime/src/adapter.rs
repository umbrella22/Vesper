use std::time::Instant;

use player_core::MediaSource;

use crate::{
    DecodedVideoFrame, PlaybackProgress, PlayerMediaInfo, PlayerRuntimeAdapterCapabilities,
    PlayerRuntimeCommand, PlayerRuntimeCommandResult, PlayerRuntimeError, PlayerRuntimeErrorCode,
    PlayerRuntimeEvent, PlayerRuntimeOptions, PlayerRuntimeResult, PlayerRuntimeStartup,
    PlayerSnapshot, PlayerTimelineSnapshot, PlayerVideoSurfaceTarget, PresentationState,
};

pub struct PlayerRuntimeAdapterBootstrap {
    pub runtime: Box<dyn PlayerRuntimeAdapter>,
    pub initial_frame: Option<DecodedVideoFrame>,
    pub startup: PlayerRuntimeStartup,
}

pub trait PlayerRuntimeAdapterInitializer {
    fn capabilities(&self) -> PlayerRuntimeAdapterCapabilities;
    fn media_info(&self) -> PlayerMediaInfo;
    fn startup(&self) -> PlayerRuntimeStartup;
    fn initialize(self: Box<Self>) -> PlayerRuntimeResult<PlayerRuntimeAdapterBootstrap>;
}

pub trait PlayerRuntimeAdapterFactory: Sync {
    fn adapter_id(&self) -> &'static str;
    fn probe_source_with_options(
        &self,
        source: MediaSource,
        options: PlayerRuntimeOptions,
    ) -> PlayerRuntimeResult<Box<dyn PlayerRuntimeAdapterInitializer>>;
}

pub trait PlayerRuntimeAdapter {
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
    fn drain_events(&mut self) -> Vec<PlayerRuntimeEvent>;
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
        }
    }
}
