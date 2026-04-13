use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use player_core::MediaSource;
use player_runtime::{
    DEFAULT_PLAYBACK_RATE, DecodedVideoFrame, FirstFrameReady, MAX_PLAYBACK_RATE,
    MIN_PLAYBACK_RATE, NATURAL_PLAYBACK_RATE_MAX, PlaybackProgress, PlayerMediaInfo,
    PlayerResilienceMetricsTracker, PlayerRuntimeAdapter, PlayerRuntimeAdapterBackendFamily,
    PlayerRuntimeAdapterBootstrap, PlayerRuntimeAdapterCapabilities, PlayerRuntimeAdapterFactory,
    PlayerRuntimeAdapterInitializer, PlayerRuntimeCommand, PlayerRuntimeCommandResult,
    PlayerRuntimeError, PlayerRuntimeErrorCode, PlayerRuntimeEvent, PlayerRuntimeOptions,
    PlayerRuntimeResult, PlayerRuntimeStartup, PlayerSnapshot, PlayerTimelineSnapshot,
    PlayerVideoInfo, PlayerVideoSurfaceKind, PlayerVideoSurfaceTarget, PresentationState,
};

pub const MACOS_NATIVE_PLAYER_RUNTIME_ADAPTER_ID: &str = "macos_native";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MacosAvFoundationBridgeContext {
    pub video_surface: Option<PlayerVideoSurfaceTarget>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MacosPlayerItemStatus {
    Unknown,
    ReadyToPlay,
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MacosTimeControlStatus {
    Paused,
    WaitingToPlay,
    Playing,
}

#[derive(Debug, Clone)]
pub struct MacosAvFoundationSnapshot {
    pub item_status: MacosPlayerItemStatus,
    pub time_control_status: MacosTimeControlStatus,
    pub playback_rate: f32,
    pub position: Duration,
    pub duration: Option<Duration>,
    pub reached_end: bool,
    pub error_message: Option<String>,
}

#[derive(Debug, Clone)]
pub struct MacosNativeObservation {
    pub presentation_state: PresentationState,
    pub is_buffering: bool,
    pub playback_rate: f32,
    pub progress: PlaybackProgress,
    pub emitted_events: Vec<PlayerRuntimeEvent>,
}

#[derive(Debug, Default, Clone)]
pub struct MacosAvFoundationStateTracker {
    has_started_playback: bool,
    playback_intent: bool,
    last_presentation_state: Option<PresentationState>,
    last_is_buffering: Option<bool>,
    last_playback_rate: Option<f32>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum MacosNativePlayerCommand {
    Play,
    Pause,
    SeekTo { position: Duration },
    Stop,
    SetPlaybackRate { rate: f32 },
}

pub trait MacosNativeCommandSink: Send {
    fn submit_command(&mut self, command: MacosNativePlayerCommand) -> PlayerRuntimeResult<()>;
    fn attach_video_surface(
        &mut self,
        _video_surface: PlayerVideoSurfaceTarget,
    ) -> PlayerRuntimeResult<()> {
        Err(PlayerRuntimeError::new(
            PlayerRuntimeErrorCode::Unsupported,
            "macos native command sink does not support attaching a video surface",
        ))
    }

    fn detach_video_surface(&mut self) -> PlayerRuntimeResult<()> {
        Err(PlayerRuntimeError::new(
            PlayerRuntimeErrorCode::Unsupported,
            "macos native command sink does not support detaching a video surface",
        ))
    }
}

impl<T> MacosNativeCommandSink for Box<T>
where
    T: MacosNativeCommandSink + ?Sized,
{
    fn submit_command(&mut self, command: MacosNativePlayerCommand) -> PlayerRuntimeResult<()> {
        (**self).submit_command(command)
    }

    fn attach_video_surface(
        &mut self,
        video_surface: PlayerVideoSurfaceTarget,
    ) -> PlayerRuntimeResult<()> {
        (**self).attach_video_surface(video_surface)
    }

    fn detach_video_surface(&mut self) -> PlayerRuntimeResult<()> {
        (**self).detach_video_surface()
    }
}

#[derive(Debug, Clone)]
pub enum MacosNativeSessionUpdate {
    Snapshot(MacosAvFoundationSnapshot),
    FirstFrameReady { position: Duration },
    InterruptionChanged { interrupted: bool },
    SeekCompleted { position: Duration },
    Error(PlayerRuntimeError),
}

#[derive(Debug, Clone, Default)]
pub struct MacosManagedNativeSessionController {
    updates: Arc<Mutex<VecDeque<MacosNativeSessionUpdate>>>,
}

pub struct MacosManagedNativeSession<C> {
    source_uri: String,
    media_info: PlayerMediaInfo,
    capabilities: PlayerRuntimeAdapterCapabilities,
    command_sink: C,
    controller: MacosManagedNativeSessionController,
    tracker: MacosAvFoundationStateTracker,
    presentation_state: PresentationState,
    video_surface: Option<PlayerVideoSurfaceTarget>,
    is_interrupted: bool,
    is_buffering: bool,
    playback_rate: f32,
    progress: PlaybackProgress,
    resilience_metrics: PlayerResilienceMetricsTracker,
    first_frame_emitted: bool,
    events: VecDeque<PlayerRuntimeEvent>,
}

pub trait MacosNativePlayerBridge: Send + Sync {
    fn probe_source(
        &self,
        source: &MediaSource,
        options: &PlayerRuntimeOptions,
    ) -> PlayerRuntimeResult<MacosNativePlayerProbe>;

    fn initialize_session(
        &self,
        source: MediaSource,
        options: PlayerRuntimeOptions,
        media_info: &PlayerMediaInfo,
        startup: &PlayerRuntimeStartup,
    ) -> PlayerRuntimeResult<MacosNativePlayerSessionBootstrap>;
}

pub trait MacosAvFoundationBridgeBindings: Send + Sync {
    fn probe_source(
        &self,
        context: &MacosAvFoundationBridgeContext,
        source: &MediaSource,
        options: &PlayerRuntimeOptions,
    ) -> PlayerRuntimeResult<MacosNativePlayerProbe>;

    fn create_command_sink(
        &self,
        context: MacosAvFoundationBridgeContext,
        source: &MediaSource,
        options: &PlayerRuntimeOptions,
        media_info: &PlayerMediaInfo,
        startup: &PlayerRuntimeStartup,
        controller: MacosManagedNativeSessionController,
    ) -> PlayerRuntimeResult<Box<dyn MacosNativeCommandSink>>;
}

pub trait MacosNativePlayerSession: Send {
    fn source_uri(&self) -> &str;
    fn capabilities(&self) -> PlayerRuntimeAdapterCapabilities;
    fn media_info(&self) -> &PlayerMediaInfo;
    fn presentation_state(&self) -> PresentationState;
    fn has_video_surface(&self) -> bool;
    fn is_interrupted(&self) -> bool;
    fn is_buffering(&self) -> bool;
    fn playback_rate(&self) -> f32;
    fn progress(&self) -> PlaybackProgress;
    fn drain_events(&mut self) -> Vec<PlayerRuntimeEvent>;
    fn dispatch(
        &mut self,
        command: PlayerRuntimeCommand,
    ) -> PlayerRuntimeResult<PlayerRuntimeCommandResult>;
    fn replace_video_surface(
        &mut self,
        video_surface: Option<PlayerVideoSurfaceTarget>,
    ) -> PlayerRuntimeResult<()>;
    fn advance(&mut self) -> PlayerRuntimeResult<Option<DecodedVideoFrame>>;
    fn next_deadline(&self) -> Option<Instant>;
}

#[derive(Debug, Clone)]
pub struct MacosNativePlayerProbe {
    pub media_info: PlayerMediaInfo,
    pub startup: PlayerRuntimeStartup,
}

pub struct MacosNativePlayerSessionBootstrap {
    pub runtime: Box<dyn MacosNativePlayerSession>,
    pub initial_frame: Option<DecodedVideoFrame>,
}

#[derive(Clone)]
pub struct MacosAvFoundationBridge {
    context: MacosAvFoundationBridgeContext,
    bindings: Arc<dyn MacosAvFoundationBridgeBindings>,
}

#[derive(Clone, Default)]
pub struct MacosNativePlayerRuntimeAdapterFactory {
    bridge: Option<Arc<dyn MacosNativePlayerBridge>>,
}

pub struct MacosNativePlayerRuntimeInitializer {
    bridge: Option<Arc<dyn MacosNativePlayerBridge>>,
    source: MediaSource,
    options: PlayerRuntimeOptions,
    media_info: PlayerMediaInfo,
    startup: PlayerRuntimeStartup,
}

pub struct MacosNativePlayerRuntime {
    inner: Box<dyn MacosNativePlayerSession>,
}

impl<C> std::fmt::Debug for MacosManagedNativeSession<C> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MacosManagedNativeSession")
            .field("source_uri", &self.source_uri)
            .field("state", &self.presentation_state)
            .field("playback_rate", &self.playback_rate)
            .finish()
    }
}

impl std::fmt::Debug for MacosNativePlayerRuntimeAdapterFactory {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MacosNativePlayerRuntimeAdapterFactory")
            .field("has_bridge", &self.bridge.is_some())
            .finish()
    }
}

impl std::fmt::Debug for MacosNativePlayerRuntimeInitializer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MacosNativePlayerRuntimeInitializer")
            .field("source", &self.source.uri())
            .field("has_bridge", &self.bridge.is_some())
            .finish()
    }
}

impl std::fmt::Debug for MacosNativePlayerRuntime {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MacosNativePlayerRuntime")
            .field("source_uri", &self.inner.source_uri())
            .field("state", &self.inner.presentation_state())
            .finish()
    }
}

impl std::fmt::Debug for MacosAvFoundationBridge {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MacosAvFoundationBridge")
            .field("context", &self.context)
            .finish()
    }
}

impl MacosNativePlayerRuntimeAdapterFactory {
    pub const fn new() -> Self {
        Self { bridge: None }
    }

    pub fn with_bridge(bridge: Arc<dyn MacosNativePlayerBridge>) -> Self {
        Self {
            bridge: Some(bridge),
        }
    }
}

impl MacosAvFoundationBridge {
    pub fn new(
        context: MacosAvFoundationBridgeContext,
        bindings: Arc<dyn MacosAvFoundationBridgeBindings>,
    ) -> Self {
        Self { context, bindings }
    }
}

impl PlayerRuntimeAdapterFactory for MacosNativePlayerRuntimeAdapterFactory {
    fn adapter_id(&self) -> &'static str {
        MACOS_NATIVE_PLAYER_RUNTIME_ADAPTER_ID
    }

    fn probe_source_with_options(
        &self,
        source: MediaSource,
        options: PlayerRuntimeOptions,
    ) -> PlayerRuntimeResult<Box<dyn PlayerRuntimeAdapterInitializer>> {
        let (media_info, startup) = match &self.bridge {
            Some(bridge) => {
                let probe = bridge.probe_source(&source, &options)?;
                (
                    normalize_media_info(&source, probe.media_info),
                    probe.startup,
                )
            }
            None => (placeholder_media_info(&source), placeholder_startup()),
        };

        Ok(Box::new(MacosNativePlayerRuntimeInitializer {
            bridge: self.bridge.clone(),
            source,
            options,
            media_info,
            startup,
        }))
    }
}

impl PlayerRuntimeAdapterInitializer for MacosNativePlayerRuntimeInitializer {
    fn capabilities(&self) -> PlayerRuntimeAdapterCapabilities {
        macos_native_capabilities()
    }

    fn media_info(&self) -> PlayerMediaInfo {
        self.media_info.clone()
    }

    fn startup(&self) -> PlayerRuntimeStartup {
        self.startup.clone()
    }

    fn initialize(self: Box<Self>) -> PlayerRuntimeResult<PlayerRuntimeAdapterBootstrap> {
        let Self {
            bridge,
            source,
            options,
            media_info,
            startup,
        } = *self;

        let Some(bridge) = bridge else {
            return Err(PlayerRuntimeError::new(
                PlayerRuntimeErrorCode::Unsupported,
                macos_native_unavailable_message(),
            ));
        };

        let bootstrap = bridge.initialize_session(source, options, &media_info, &startup)?;
        Ok(PlayerRuntimeAdapterBootstrap {
            runtime: Box::new(MacosNativePlayerRuntime {
                inner: bootstrap.runtime,
            }),
            initial_frame: bootstrap.initial_frame,
            startup,
        })
    }
}

impl PlayerRuntimeAdapter for MacosNativePlayerRuntime {
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

    fn dispatch(
        &mut self,
        command: PlayerRuntimeCommand,
    ) -> PlayerRuntimeResult<PlayerRuntimeCommandResult> {
        self.inner.dispatch(command)
    }

    fn replace_video_surface(
        &mut self,
        video_surface: Option<PlayerVideoSurfaceTarget>,
    ) -> PlayerRuntimeResult<()> {
        self.inner.replace_video_surface(video_surface)
    }

    fn advance(&mut self) -> PlayerRuntimeResult<Option<DecodedVideoFrame>> {
        self.inner.advance()
    }

    fn next_deadline(&self) -> Option<Instant> {
        self.inner.next_deadline()
    }
}

impl MacosAvFoundationStateTracker {
    pub fn observe(&mut self, snapshot: &MacosAvFoundationSnapshot) -> MacosNativeObservation {
        let presentation_state = self.presentation_state(snapshot);
        let is_buffering = self.is_buffering(snapshot, presentation_state);
        let playback_rate = sanitize_native_playback_rate(snapshot.playback_rate);
        let progress = PlaybackProgress::new(snapshot.position, snapshot.duration);
        let mut emitted_events = Vec::new();

        if let Some(message) = snapshot.error_message.as_ref() {
            emitted_events.push(PlayerRuntimeEvent::Error(PlayerRuntimeError::new(
                PlayerRuntimeErrorCode::BackendFailure,
                message.clone(),
            )));
        }

        if self
            .last_presentation_state
            .map(|previous| previous != presentation_state)
            .unwrap_or(true)
        {
            if presentation_state == PresentationState::Finished {
                emitted_events.push(PlayerRuntimeEvent::Ended);
            }
            emitted_events.push(PlayerRuntimeEvent::PlaybackStateChanged(presentation_state));
        }

        if self
            .last_is_buffering
            .map(|previous| previous != is_buffering)
            .unwrap_or(is_buffering)
        {
            emitted_events.push(PlayerRuntimeEvent::BufferingChanged {
                buffering: is_buffering,
            });
        }

        if should_emit_playback_rate_change(self.last_playback_rate, playback_rate) {
            emitted_events.push(PlayerRuntimeEvent::PlaybackRateChanged {
                rate: playback_rate,
            });
        }

        if presentation_state == PresentationState::Playing {
            self.has_started_playback = true;
        }
        if presentation_state == PresentationState::Finished {
            self.playback_intent = false;
        }
        self.last_presentation_state = Some(presentation_state);
        self.last_is_buffering = Some(is_buffering);
        self.last_playback_rate = Some(playback_rate);

        MacosNativeObservation {
            presentation_state,
            is_buffering,
            playback_rate,
            progress,
            emitted_events,
        }
    }

    pub fn seed(&mut self, presentation_state: PresentationState, playback_rate: f32) {
        if presentation_state == PresentationState::Playing {
            self.has_started_playback = true;
        }
        self.playback_intent = presentation_state == PresentationState::Playing;
        self.last_presentation_state = Some(presentation_state);
        self.last_is_buffering = Some(false);
        self.last_playback_rate = Some(playback_rate);
    }

    fn presentation_state(&self, snapshot: &MacosAvFoundationSnapshot) -> PresentationState {
        if snapshot.reached_end {
            return PresentationState::Finished;
        }

        match snapshot.item_status {
            MacosPlayerItemStatus::Failed => PresentationState::Paused,
            MacosPlayerItemStatus::Unknown => {
                if self.playback_intent {
                    PresentationState::Playing
                } else if self.has_started_playback {
                    PresentationState::Paused
                } else {
                    PresentationState::Ready
                }
            }
            MacosPlayerItemStatus::ReadyToPlay => match snapshot.time_control_status {
                MacosTimeControlStatus::Playing => PresentationState::Playing,
                MacosTimeControlStatus::Paused | MacosTimeControlStatus::WaitingToPlay => {
                    if self.playback_intent {
                        PresentationState::Playing
                    } else if self.has_started_playback {
                        PresentationState::Paused
                    } else {
                        PresentationState::Ready
                    }
                }
            },
        }
    }

    fn is_buffering(
        &self,
        snapshot: &MacosAvFoundationSnapshot,
        presentation_state: PresentationState,
    ) -> bool {
        if presentation_state != PresentationState::Playing || snapshot.reached_end {
            return false;
        }

        snapshot.item_status != MacosPlayerItemStatus::Failed
            && snapshot.time_control_status == MacosTimeControlStatus::WaitingToPlay
            && self.playback_intent
    }
}

impl MacosManagedNativeSessionController {
    pub fn apply_snapshot(&self, snapshot: MacosAvFoundationSnapshot) {
        self.push_update(MacosNativeSessionUpdate::Snapshot(snapshot));
    }

    pub fn report_seek_completed(&self, position: Duration) {
        self.push_update(MacosNativeSessionUpdate::SeekCompleted { position });
    }

    pub fn report_first_frame_ready(&self, position: Duration) {
        self.push_update(MacosNativeSessionUpdate::FirstFrameReady { position });
    }

    pub fn report_interruption_changed(&self, interrupted: bool) {
        self.push_update(MacosNativeSessionUpdate::InterruptionChanged { interrupted });
    }

    pub fn report_error(&self, code: PlayerRuntimeErrorCode, message: impl Into<String>) {
        self.push_update(MacosNativeSessionUpdate::Error(PlayerRuntimeError::new(
            code,
            message.into(),
        )));
    }

    pub fn push_update(&self, update: MacosNativeSessionUpdate) {
        if let Ok(mut updates) = self.updates.lock() {
            updates.push_back(update);
        }
    }

    fn take_pending(&self) -> Vec<MacosNativeSessionUpdate> {
        self.updates
            .lock()
            .map(|mut updates| updates.drain(..).collect())
            .unwrap_or_default()
    }
}

impl<C: MacosNativeCommandSink> MacosManagedNativeSession<C> {
    #[cfg(test)]
    pub fn new(
        source_uri: impl Into<String>,
        media_info: PlayerMediaInfo,
        command_sink: C,
    ) -> Self {
        Self::with_existing_controller(
            source_uri,
            media_info,
            macos_native_capabilities(),
            None,
            command_sink,
            MacosManagedNativeSessionController::default(),
        )
    }

    pub fn with_existing_controller(
        source_uri: impl Into<String>,
        media_info: PlayerMediaInfo,
        capabilities: PlayerRuntimeAdapterCapabilities,
        video_surface: Option<PlayerVideoSurfaceTarget>,
        command_sink: C,
        controller: MacosManagedNativeSessionController,
    ) -> Self {
        Self {
            source_uri: source_uri.into(),
            media_info,
            capabilities,
            command_sink,
            controller,
            tracker: MacosAvFoundationStateTracker::default(),
            presentation_state: PresentationState::Ready,
            video_surface,
            is_interrupted: false,
            is_buffering: false,
            playback_rate: DEFAULT_PLAYBACK_RATE,
            progress: PlaybackProgress::new(Duration::ZERO, None),
            resilience_metrics: PlayerResilienceMetricsTracker::default(),
            first_frame_emitted: false,
            events: VecDeque::new(),
        }
    }

    fn pump_pending_updates(&mut self) {
        for update in self.controller.take_pending() {
            match update {
                MacosNativeSessionUpdate::Snapshot(snapshot) => self.apply_snapshot(&snapshot),
                MacosNativeSessionUpdate::FirstFrameReady { position } => {
                    self.emit_first_frame_ready(position);
                }
                MacosNativeSessionUpdate::InterruptionChanged { interrupted } => {
                    self.apply_interruption(interrupted);
                }
                MacosNativeSessionUpdate::SeekCompleted { position } => {
                    self.progress = PlaybackProgress::new(position, self.progress.duration());
                    if self.presentation_state == PresentationState::Finished {
                        self.presentation_state = PresentationState::Ready;
                        self.is_buffering = false;
                        self.tracker
                            .seed(self.presentation_state, self.playback_rate);
                    }
                    self.events
                        .push_back(PlayerRuntimeEvent::SeekCompleted { position });
                }
                MacosNativeSessionUpdate::Error(error) => {
                    self.events.push_back(PlayerRuntimeEvent::Error(error));
                }
            }
        }
    }

    pub fn apply_snapshot(&mut self, snapshot: &MacosAvFoundationSnapshot) {
        let observation = self.tracker.observe(snapshot);
        self.apply_observation(observation);
    }

    fn apply_observation(&mut self, observation: MacosNativeObservation) {
        self.resilience_metrics
            .observe_playback_state(observation.presentation_state);
        self.resilience_metrics
            .observe_buffering(observation.is_buffering);
        self.presentation_state = observation.presentation_state;
        self.is_buffering = observation.is_buffering;
        self.playback_rate = observation.playback_rate;
        self.progress = observation.progress;
        self.events.extend(observation.emitted_events);
    }

    fn emit_first_frame_ready(&mut self, position: Duration) {
        if self.first_frame_emitted {
            return;
        }

        let Some(video) = self.media_info.best_video.as_ref() else {
            return;
        };

        self.first_frame_emitted = true;
        self.events
            .push_back(PlayerRuntimeEvent::FirstFrameReady(FirstFrameReady {
                presentation_time: position,
                width: video.width,
                height: video.height,
            }));
    }

    fn snapshot(&self) -> PlayerSnapshot {
        PlayerSnapshot {
            source_uri: self.source_uri.clone(),
            state: self.presentation_state,
            has_video_surface: self.video_surface.is_some(),
            is_interrupted: self.is_interrupted,
            is_buffering: self.is_buffering,
            playback_rate: self.playback_rate,
            progress: self.progress,
            timeline: PlayerTimelineSnapshot::vod(self.progress, self.capabilities.supports_seek),
            media_info: self.media_info.clone(),
            resilience_metrics: self.resilience_metrics.snapshot(),
        }
    }

    fn emit_initial_runtime_events(&mut self, startup: PlayerRuntimeStartup) {
        self.events
            .push_back(PlayerRuntimeEvent::Initialized(startup));
        self.events
            .push_back(PlayerRuntimeEvent::MetadataReady(self.media_info.clone()));
        self.events
            .push_back(PlayerRuntimeEvent::PlaybackStateChanged(
                self.presentation_state,
            ));
        if self.is_interrupted {
            self.events
                .push_back(PlayerRuntimeEvent::InterruptionChanged { interrupted: true });
        }
        if self.is_buffering {
            self.events
                .push_back(PlayerRuntimeEvent::BufferingChanged { buffering: true });
        }
        self.tracker
            .seed(self.presentation_state, self.playback_rate);
    }

    fn emit_state_change_if_needed(&mut self, previous_state: PresentationState) {
        if self.presentation_state != previous_state {
            self.resilience_metrics
                .observe_playback_state(self.presentation_state);
            self.events
                .push_back(PlayerRuntimeEvent::PlaybackStateChanged(
                    self.presentation_state,
                ));
        }
    }

    fn emit_playback_rate_change_if_needed(&mut self, previous_rate: f32) {
        if (self.playback_rate - previous_rate).abs() > f32::EPSILON {
            self.events
                .push_back(PlayerRuntimeEvent::PlaybackRateChanged {
                    rate: self.playback_rate,
                });
        }
    }

    fn emit_buffering_change_if_needed(&mut self, previous_buffering: bool) {
        if self.is_buffering != previous_buffering {
            self.resilience_metrics.observe_buffering(self.is_buffering);
            self.events.push_back(PlayerRuntimeEvent::BufferingChanged {
                buffering: self.is_buffering,
            });
        }
    }

    fn emit_interruption_change_if_needed(&mut self, previous_interrupted: bool) {
        if self.is_interrupted != previous_interrupted {
            self.events
                .push_back(PlayerRuntimeEvent::InterruptionChanged {
                    interrupted: self.is_interrupted,
                });
        }
    }

    fn apply_interruption(&mut self, interrupted: bool) {
        let previous_interrupted = self.is_interrupted;
        let previous_buffering = self.is_buffering;
        self.is_interrupted = interrupted;
        if interrupted {
            self.is_buffering = false;
        }
        self.emit_interruption_change_if_needed(previous_interrupted);
        if interrupted {
            self.emit_buffering_change_if_needed(previous_buffering);
        }
    }

    fn emit_replay_from_finished_state(&mut self) {
        let previous_state = self.presentation_state;
        self.presentation_state = PresentationState::Ready;
        self.emit_state_change_if_needed(previous_state);
        self.presentation_state = PresentationState::Playing;
        self.emit_state_change_if_needed(PresentationState::Ready);
    }

    fn validate_playback_rate(&self, rate: f32) -> PlayerRuntimeResult<f32> {
        if !rate.is_finite() {
            return Err(PlayerRuntimeError::new(
                PlayerRuntimeErrorCode::InvalidArgument,
                "playback rate must be a finite number",
            ));
        }

        let min = self
            .capabilities
            .playback_rate_min
            .unwrap_or(MIN_PLAYBACK_RATE);
        let max = self
            .capabilities
            .playback_rate_max
            .unwrap_or(MAX_PLAYBACK_RATE);
        if !(min..=max).contains(&rate) {
            return Err(PlayerRuntimeError::new(
                PlayerRuntimeErrorCode::InvalidArgument,
                format!("playback rate must be within {min:.1}x..={max:.1}x"),
            ));
        }

        Ok(rate)
    }

    fn submit_commands(
        &mut self,
        commands: Vec<MacosNativePlayerCommand>,
    ) -> PlayerRuntimeResult<()> {
        for command in commands {
            self.command_sink.submit_command(command)?;
        }
        Ok(())
    }

    fn translate_command(
        &self,
        command: &PlayerRuntimeCommand,
    ) -> PlayerRuntimeResult<(bool, Vec<MacosNativePlayerCommand>)> {
        match command {
            PlayerRuntimeCommand::Play => match self.presentation_state {
                PresentationState::Playing => Ok((false, Vec::new())),
                PresentationState::Finished => Ok((
                    true,
                    vec![
                        MacosNativePlayerCommand::SeekTo {
                            position: Duration::ZERO,
                        },
                        MacosNativePlayerCommand::Play,
                    ],
                )),
                PresentationState::Ready | PresentationState::Paused => {
                    Ok((true, vec![MacosNativePlayerCommand::Play]))
                }
            },
            PlayerRuntimeCommand::Pause => match self.presentation_state {
                PresentationState::Playing => Ok((true, vec![MacosNativePlayerCommand::Pause])),
                PresentationState::Paused => Ok((false, Vec::new())),
                PresentationState::Ready | PresentationState::Finished => {
                    Err(PlayerRuntimeError::new(
                        PlayerRuntimeErrorCode::InvalidState,
                        "pause is only valid after playback has started",
                    ))
                }
            },
            PlayerRuntimeCommand::TogglePause => match self.presentation_state {
                PresentationState::Playing => Ok((true, vec![MacosNativePlayerCommand::Pause])),
                PresentationState::Ready | PresentationState::Paused => {
                    Ok((true, vec![MacosNativePlayerCommand::Play]))
                }
                PresentationState::Finished => Ok((
                    true,
                    vec![
                        MacosNativePlayerCommand::SeekTo {
                            position: Duration::ZERO,
                        },
                        MacosNativePlayerCommand::Play,
                    ],
                )),
            },
            PlayerRuntimeCommand::SeekTo { position } => Ok((
                true,
                vec![MacosNativePlayerCommand::SeekTo {
                    position: *position,
                }],
            )),
            PlayerRuntimeCommand::SetPlaybackRate { rate } => {
                let rate = self.validate_playback_rate(*rate)?;
                if (self.playback_rate - rate).abs() <= f32::EPSILON {
                    return Ok((false, Vec::new()));
                }
                Ok((
                    true,
                    vec![MacosNativePlayerCommand::SetPlaybackRate { rate }],
                ))
            }
            PlayerRuntimeCommand::SetVideoTrackSelection { .. }
            | PlayerRuntimeCommand::SetAudioTrackSelection { .. }
            | PlayerRuntimeCommand::SetSubtitleTrackSelection { .. }
            | PlayerRuntimeCommand::SetAbrPolicy { .. } => Err(PlayerRuntimeError::new(
                PlayerRuntimeErrorCode::Unsupported,
                "track selection and ABR control are not implemented for the macOS native runtime yet",
            )),
            PlayerRuntimeCommand::Stop => {
                if self.presentation_state == PresentationState::Ready
                    && self.progress.position().is_zero()
                {
                    return Ok((false, Vec::new()));
                }
                Ok((true, vec![MacosNativePlayerCommand::Stop]))
            }
        }
    }
}

impl MacosNativePlayerBridge for MacosAvFoundationBridge {
    fn probe_source(
        &self,
        source: &MediaSource,
        options: &PlayerRuntimeOptions,
    ) -> PlayerRuntimeResult<MacosNativePlayerProbe> {
        self.bindings.probe_source(&self.context, source, options)
    }

    fn initialize_session(
        &self,
        source: MediaSource,
        options: PlayerRuntimeOptions,
        media_info: &PlayerMediaInfo,
        startup: &PlayerRuntimeStartup,
    ) -> PlayerRuntimeResult<MacosNativePlayerSessionBootstrap> {
        let context = resolve_bridge_context(&self.context, &options, media_info)?;
        let capabilities = macos_native_capabilities();
        let controller = MacosManagedNativeSessionController::default();
        let command_sink = self.bindings.create_command_sink(
            context,
            &source,
            &options,
            media_info,
            startup,
            controller.clone(),
        )?;
        let session = MacosManagedNativeSession::with_existing_controller(
            source.uri(),
            media_info.clone(),
            capabilities,
            context.video_surface,
            command_sink,
            controller,
        );
        let mut session = session;
        session.emit_initial_runtime_events(startup.clone());

        Ok(MacosNativePlayerSessionBootstrap {
            runtime: Box::new(session),
            initial_frame: None,
        })
    }
}

impl<C: MacosNativeCommandSink> MacosNativePlayerSession for MacosManagedNativeSession<C> {
    fn source_uri(&self) -> &str {
        &self.source_uri
    }

    fn capabilities(&self) -> PlayerRuntimeAdapterCapabilities {
        self.capabilities.clone()
    }

    fn media_info(&self) -> &PlayerMediaInfo {
        &self.media_info
    }

    fn presentation_state(&self) -> PresentationState {
        self.presentation_state
    }

    fn has_video_surface(&self) -> bool {
        self.video_surface.is_some()
    }

    fn is_interrupted(&self) -> bool {
        self.is_interrupted
    }

    fn is_buffering(&self) -> bool {
        self.is_buffering
    }

    fn playback_rate(&self) -> f32 {
        self.playback_rate
    }

    fn progress(&self) -> PlaybackProgress {
        self.progress
    }

    fn drain_events(&mut self) -> Vec<PlayerRuntimeEvent> {
        self.pump_pending_updates();
        self.events.drain(..).collect()
    }

    fn dispatch(
        &mut self,
        command: PlayerRuntimeCommand,
    ) -> PlayerRuntimeResult<PlayerRuntimeCommandResult> {
        self.pump_pending_updates();
        let previous_state = self.presentation_state;
        let previous_buffering = self.is_buffering;
        let previous_rate = self.playback_rate;
        let (applied, native_commands) = self.translate_command(&command)?;
        self.submit_commands(native_commands)?;

        if applied {
            match command {
                PlayerRuntimeCommand::Play => {
                    if previous_state == PresentationState::Finished {
                        self.progress =
                            PlaybackProgress::new(Duration::ZERO, self.progress.duration());
                        self.is_buffering = false;
                        self.emit_replay_from_finished_state();
                    } else {
                        self.presentation_state = PresentationState::Playing;
                        self.emit_state_change_if_needed(previous_state);
                    }
                }
                PlayerRuntimeCommand::Pause => {
                    self.presentation_state = PresentationState::Paused;
                    self.is_buffering = false;
                    self.emit_state_change_if_needed(previous_state);
                }
                PlayerRuntimeCommand::TogglePause => {
                    if previous_state == PresentationState::Finished {
                        self.progress =
                            PlaybackProgress::new(Duration::ZERO, self.progress.duration());
                        self.is_buffering = false;
                        self.emit_replay_from_finished_state();
                    } else {
                        self.presentation_state =
                            if self.presentation_state == PresentationState::Playing {
                                PresentationState::Paused
                            } else {
                                PresentationState::Playing
                            };
                        if self.presentation_state != PresentationState::Playing {
                            self.is_buffering = false;
                        }
                        self.emit_state_change_if_needed(previous_state);
                    }
                }
                PlayerRuntimeCommand::SeekTo { position } => {
                    self.progress = PlaybackProgress::new(position, self.progress.duration());
                    if self.presentation_state == PresentationState::Finished {
                        self.presentation_state = PresentationState::Ready;
                    }
                    self.is_buffering = false;
                    self.emit_state_change_if_needed(previous_state);
                }
                PlayerRuntimeCommand::SetPlaybackRate { rate } => {
                    self.playback_rate = rate;
                }
                PlayerRuntimeCommand::SetVideoTrackSelection { .. }
                | PlayerRuntimeCommand::SetAudioTrackSelection { .. }
                | PlayerRuntimeCommand::SetSubtitleTrackSelection { .. }
                | PlayerRuntimeCommand::SetAbrPolicy { .. } => {}
                PlayerRuntimeCommand::Stop => {
                    self.presentation_state = PresentationState::Ready;
                    self.is_buffering = false;
                    self.progress = PlaybackProgress::new(Duration::ZERO, self.progress.duration());
                    self.emit_state_change_if_needed(previous_state);
                }
            }
            self.emit_buffering_change_if_needed(previous_buffering);
            self.emit_playback_rate_change_if_needed(previous_rate);
            self.tracker
                .seed(self.presentation_state, self.playback_rate);
        }

        Ok(PlayerRuntimeCommandResult {
            applied,
            frame: None,
            snapshot: self.snapshot(),
        })
    }

    fn replace_video_surface(
        &mut self,
        video_surface: Option<PlayerVideoSurfaceTarget>,
    ) -> PlayerRuntimeResult<()> {
        if self.video_surface == video_surface {
            return Ok(());
        }

        match video_surface {
            Some(surface) => {
                if let Some(best_video) = self.media_info.best_video.as_ref() {
                    validate_macos_video_surface(surface, best_video)?;
                }
                self.command_sink.attach_video_surface(surface)?;
                self.video_surface = Some(surface);
                self.first_frame_emitted = false;
            }
            None => {
                self.command_sink.detach_video_surface()?;
                self.video_surface = None;
                self.first_frame_emitted = false;
            }
        }

        self.events
            .push_back(PlayerRuntimeEvent::VideoSurfaceChanged {
                attached: self.video_surface.is_some(),
            });
        Ok(())
    }

    fn advance(&mut self) -> PlayerRuntimeResult<Option<DecodedVideoFrame>> {
        self.pump_pending_updates();
        Ok(None)
    }

    fn next_deadline(&self) -> Option<Instant> {
        None
    }
}

fn placeholder_media_info(source: &MediaSource) -> PlayerMediaInfo {
    PlayerMediaInfo {
        source_uri: source.uri().to_owned(),
        source_kind: source.kind(),
        source_protocol: source.protocol(),
        duration: None,
        bit_rate: None,
        audio_streams: 0,
        video_streams: 0,
        best_video: None,
        best_audio: None,
        track_catalog: Default::default(),
        track_selection: Default::default(),
    }
}

fn placeholder_startup() -> PlayerRuntimeStartup {
    PlayerRuntimeStartup {
        ffmpeg_initialized: false,
        audio_output: None,
        decoded_audio: None,
        video_decode: None,
    }
}

fn normalize_media_info(source: &MediaSource, mut media_info: PlayerMediaInfo) -> PlayerMediaInfo {
    media_info.source_uri = source.uri().to_owned();
    media_info.source_kind = source.kind();
    media_info.source_protocol = source.protocol();
    media_info
}

fn resolve_bridge_context(
    base_context: &MacosAvFoundationBridgeContext,
    options: &PlayerRuntimeOptions,
    media_info: &PlayerMediaInfo,
) -> PlayerRuntimeResult<MacosAvFoundationBridgeContext> {
    let resolved_surface = options.video_surface.or(base_context.video_surface);

    if let Some(best_video) = media_info.best_video.as_ref() {
        let surface = resolved_surface.ok_or_else(|| {
            PlayerRuntimeError::new(
                PlayerRuntimeErrorCode::InvalidArgument,
                format!(
                    "macos native backend requires a video surface target for {} video playback",
                    best_video.codec
                ),
            )
        })?;
        validate_macos_video_surface(surface, best_video)?;
    }

    Ok(MacosAvFoundationBridgeContext {
        video_surface: resolved_surface,
    })
}

fn validate_macos_video_surface(
    surface: PlayerVideoSurfaceTarget,
    best_video: &PlayerVideoInfo,
) -> PlayerRuntimeResult<()> {
    match surface.kind {
        PlayerVideoSurfaceKind::NsView
        | PlayerVideoSurfaceKind::PlayerLayer
        | PlayerVideoSurfaceKind::MetalLayer => Ok(()),
        PlayerVideoSurfaceKind::UiView => Err(PlayerRuntimeError::new(
            PlayerRuntimeErrorCode::InvalidArgument,
            format!(
                "macos native backend does not support UiView as a video surface for {} playback",
                best_video.codec
            ),
        )),
    }
}

fn macos_native_capabilities() -> PlayerRuntimeAdapterCapabilities {
    PlayerRuntimeAdapterCapabilities {
        adapter_id: MACOS_NATIVE_PLAYER_RUNTIME_ADAPTER_ID,
        backend_family: PlayerRuntimeAdapterBackendFamily::NativeMacos,
        supports_audio_output: true,
        supports_frame_output: false,
        supports_external_video_surface: true,
        supports_seek: true,
        supports_stop: true,
        supports_playback_rate: true,
        playback_rate_min: Some(MIN_PLAYBACK_RATE),
        playback_rate_max: Some(MAX_PLAYBACK_RATE),
        natural_playback_rate_max: Some(NATURAL_PLAYBACK_RATE_MAX),
        supports_hardware_decode: true,
        supports_streaming: true,
        supports_hdr: true,
    }
}

fn macos_native_unavailable_message() -> &'static str {
    if cfg!(target_os = "macos") {
        "macos native adapter skeleton exists, but the AVFoundation/VideoToolbox bridge is not implemented yet"
    } else {
        "macos native adapter can be probed on non-macOS hosts as a skeleton, but initialization is only planned for macOS"
    }
}

fn sanitize_native_playback_rate(playback_rate: f32) -> f32 {
    if playback_rate.is_finite() && playback_rate > 0.0 {
        playback_rate
    } else {
        DEFAULT_PLAYBACK_RATE
    }
}

fn should_emit_playback_rate_change(last_playback_rate: Option<f32>, playback_rate: f32) -> bool {
    match last_playback_rate {
        Some(previous) => (previous - playback_rate).abs() > f32::EPSILON,
        None => (playback_rate - DEFAULT_PLAYBACK_RATE).abs() > f32::EPSILON,
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    use super::{
        MACOS_NATIVE_PLAYER_RUNTIME_ADAPTER_ID, MacosAvFoundationBridge,
        MacosAvFoundationBridgeBindings, MacosAvFoundationBridgeContext, MacosAvFoundationSnapshot,
        MacosAvFoundationStateTracker, MacosManagedNativeSession,
        MacosManagedNativeSessionController, MacosNativeCommandSink, MacosNativePlayerBridge,
        MacosNativePlayerCommand, MacosNativePlayerProbe, MacosNativePlayerRuntimeAdapterFactory,
        MacosNativePlayerSession, MacosNativePlayerSessionBootstrap, MacosPlayerItemStatus,
        MacosTimeControlStatus, macos_native_capabilities,
    };
    use player_core::MediaSource;
    use player_runtime::{
        PlayerMediaInfo, PlayerRuntimeAdapterBackendFamily, PlayerRuntimeAdapterFactory,
        PlayerRuntimeCommand, PlayerRuntimeErrorCode, PlayerRuntimeEvent, PlayerRuntimeOptions,
        PlayerRuntimeResult, PlayerRuntimeStartup, PlayerVideoInfo, PlayerVideoSurfaceKind,
        PlayerVideoSurfaceTarget, PresentationState,
    };

    #[test]
    fn macos_native_factory_exposes_native_capabilities() {
        let factory = MacosNativePlayerRuntimeAdapterFactory::default();
        let initializer = factory
            .probe_source_with_options(
                MediaSource::new("placeholder.mp4"),
                PlayerRuntimeOptions::default(),
            )
            .expect("macos native skeleton probe should succeed");

        let capabilities = initializer.capabilities();
        let startup = initializer.startup();

        assert_eq!(
            capabilities.adapter_id,
            MACOS_NATIVE_PLAYER_RUNTIME_ADAPTER_ID
        );
        assert_eq!(
            capabilities.backend_family,
            PlayerRuntimeAdapterBackendFamily::NativeMacos
        );
        assert!(capabilities.supports_external_video_surface);
        assert!(capabilities.supports_hardware_decode);
        assert!(!startup.ffmpeg_initialized);
    }

    #[test]
    fn macos_native_factory_without_bridge_rejects_initialize() {
        let factory = MacosNativePlayerRuntimeAdapterFactory::default();
        let initializer = factory
            .probe_source_with_options(
                MediaSource::new("placeholder.mp4"),
                PlayerRuntimeOptions::default(),
            )
            .expect("macos native skeleton probe should succeed");

        let error = match initializer.initialize() {
            Ok(_) => panic!("macos native skeleton should not initialize without a bridge"),
            Err(error) => error,
        };

        assert_eq!(error.code(), PlayerRuntimeErrorCode::Unsupported);
    }

    #[test]
    fn macos_factory_can_initialize_with_bridge() {
        let factory =
            MacosNativePlayerRuntimeAdapterFactory::with_bridge(Arc::new(FakeMacosBridge));
        let initializer = factory
            .probe_source_with_options(
                MediaSource::new("placeholder.mp4"),
                PlayerRuntimeOptions::default(),
            )
            .expect("bridge-backed macos probe should succeed");

        let bootstrap = initializer
            .initialize()
            .expect("bridge-backed macos initializer should initialize");
        let capabilities = bootstrap.runtime.capabilities();

        assert_eq!(
            capabilities.backend_family,
            PlayerRuntimeAdapterBackendFamily::NativeMacos
        );
        assert!(bootstrap.initial_frame.is_none());
    }

    #[test]
    fn macos_state_tracker_emits_ended_and_rate_change() {
        let mut tracker = MacosAvFoundationStateTracker::default();
        let observation = tracker.observe(&MacosAvFoundationSnapshot {
            item_status: MacosPlayerItemStatus::ReadyToPlay,
            time_control_status: MacosTimeControlStatus::Playing,
            playback_rate: 1.5,
            position: Duration::from_secs(12),
            duration: Some(Duration::from_secs(30)),
            reached_end: true,
            error_message: None,
        });

        assert_eq!(observation.presentation_state, PresentationState::Finished);
        assert_eq!(observation.playback_rate, 1.5);
        assert!(
            observation
                .emitted_events
                .iter()
                .any(|event| matches!(event, PlayerRuntimeEvent::Ended))
        );
        assert!(observation.emitted_events.iter().any(|event| matches!(
            event,
            PlayerRuntimeEvent::PlaybackRateChanged { rate } if (*rate - 1.5).abs() < f32::EPSILON
        )));
    }

    #[test]
    fn macos_state_tracker_keeps_playing_while_native_player_is_waiting() {
        let mut tracker = MacosAvFoundationStateTracker::default();
        tracker.seed(PresentationState::Playing, 1.0);

        let observation = tracker.observe(&MacosAvFoundationSnapshot {
            item_status: MacosPlayerItemStatus::ReadyToPlay,
            time_control_status: MacosTimeControlStatus::WaitingToPlay,
            playback_rate: 1.0,
            position: Duration::from_millis(250),
            duration: Some(Duration::from_secs(10)),
            reached_end: false,
            error_message: None,
        });

        assert_eq!(observation.presentation_state, PresentationState::Playing);
        assert!(observation.is_buffering);
        assert!(
            !observation
                .emitted_events
                .iter()
                .any(|event| matches!(event, PlayerRuntimeEvent::PlaybackStateChanged(_)))
        );
        assert!(observation.emitted_events.iter().any(|event| matches!(
            event,
            PlayerRuntimeEvent::BufferingChanged { buffering: true }
        )));
    }

    #[test]
    fn managed_session_emits_initial_and_dispatch_events() {
        let controller = MacosManagedNativeSessionController::default();
        let mut session = MacosManagedNativeSession::with_existing_controller(
            "fixture.mp4",
            demo_media_info(),
            macos_native_capabilities(),
            None,
            FakeCommandSink::default(),
            controller.clone(),
        );

        session.emit_initial_runtime_events(PlayerRuntimeStartup {
            ffmpeg_initialized: false,
            audio_output: None,
            decoded_audio: None,
            video_decode: None,
        });

        let initial_events = session.drain_events();
        assert!(matches!(
            initial_events.first(),
            Some(PlayerRuntimeEvent::Initialized(_))
        ));
        assert!(
            initial_events
                .iter()
                .any(|event| matches!(event, PlayerRuntimeEvent::MetadataReady(_)))
        );
        assert!(initial_events.iter().any(|event| matches!(
            event,
            PlayerRuntimeEvent::PlaybackStateChanged(PresentationState::Ready)
        )));

        let result = session
            .dispatch(PlayerRuntimeCommand::Play)
            .expect("play should be accepted");
        assert!(result.applied);

        let play_events = session.drain_events();
        assert!(play_events.iter().any(|event| matches!(
            event,
            PlayerRuntimeEvent::PlaybackStateChanged(PresentationState::Playing)
        )));

        let result = session
            .dispatch(PlayerRuntimeCommand::SetPlaybackRate { rate: 1.5 })
            .expect("playback rate change should be accepted");
        assert!(result.applied);

        let rate_events = session.drain_events();
        assert!(rate_events.iter().any(|event| matches!(
            event,
            PlayerRuntimeEvent::PlaybackRateChanged { rate } if (rate - 1.5).abs() < f32::EPSILON
        )));

        session.apply_snapshot(&MacosAvFoundationSnapshot {
            item_status: MacosPlayerItemStatus::ReadyToPlay,
            time_control_status: MacosTimeControlStatus::WaitingToPlay,
            playback_rate: 1.5,
            position: Duration::ZERO,
            duration: Some(Duration::from_secs(42)),
            reached_end: false,
            error_message: None,
        });
        controller.report_first_frame_ready(Duration::ZERO);

        let first_frame_events = session.drain_events();
        assert!(first_frame_events.iter().any(|event| matches!(
            event,
            PlayerRuntimeEvent::FirstFrameReady(frame)
                if frame.width == 960 && frame.height == 432
        )));
        assert!(!first_frame_events.iter().any(|event| matches!(
            event,
            PlayerRuntimeEvent::PlaybackStateChanged(PresentationState::Paused)
        )));
        assert!(first_frame_events.iter().any(|event| matches!(
            event,
            PlayerRuntimeEvent::BufferingChanged { buffering: true }
        )));
        assert!(session.snapshot().is_buffering);
    }

    #[test]
    fn managed_session_snapshot_tracks_resilience_metrics() {
        let mut session = MacosManagedNativeSession::new(
            "fixture.mp4",
            demo_media_info(),
            FakeCommandSink::default(),
        );

        session.emit_initial_runtime_events(PlayerRuntimeStartup {
            ffmpeg_initialized: false,
            audio_output: None,
            decoded_audio: None,
            video_decode: None,
        });
        let _ = session.drain_events();

        let result = session
            .dispatch(PlayerRuntimeCommand::Play)
            .expect("play should be accepted");
        assert!(result.applied);
        let _ = session.drain_events();

        session.apply_snapshot(&MacosAvFoundationSnapshot {
            item_status: MacosPlayerItemStatus::ReadyToPlay,
            time_control_status: MacosTimeControlStatus::WaitingToPlay,
            playback_rate: 1.0,
            position: Duration::from_secs(1),
            duration: Some(Duration::from_secs(42)),
            reached_end: false,
            error_message: None,
        });

        let snapshot = session.snapshot();
        assert_eq!(snapshot.resilience_metrics.buffering_event_count, 1);
        assert_eq!(snapshot.resilience_metrics.rebuffer_count, 1);
        assert_eq!(snapshot.resilience_metrics.retry_count, 0);
    }

    #[test]
    fn managed_session_play_replays_from_finished_via_ready_then_playing() {
        let mut session = MacosManagedNativeSession::new(
            "fixture.mp4",
            demo_media_info(),
            FakeCommandSink::default(),
        );

        session.emit_initial_runtime_events(PlayerRuntimeStartup {
            ffmpeg_initialized: false,
            audio_output: None,
            decoded_audio: None,
            video_decode: None,
        });
        let _ = session.drain_events();

        session.apply_snapshot(&MacosAvFoundationSnapshot {
            item_status: MacosPlayerItemStatus::ReadyToPlay,
            time_control_status: MacosTimeControlStatus::Paused,
            playback_rate: 1.0,
            position: Duration::from_secs(42),
            duration: Some(Duration::from_secs(42)),
            reached_end: true,
            error_message: None,
        });
        let _ = session.drain_events();

        let result = session
            .dispatch(PlayerRuntimeCommand::Play)
            .expect("play should replay from finished");
        assert!(result.applied);
        assert_eq!(result.snapshot.state, PresentationState::Playing);
        assert_eq!(result.snapshot.progress.position(), Duration::ZERO);

        let events = session.drain_events();
        assert!(matches!(
            events.as_slice(),
            [
                PlayerRuntimeEvent::PlaybackStateChanged(PresentationState::Ready),
                PlayerRuntimeEvent::PlaybackStateChanged(PresentationState::Playing),
            ]
        ));
    }

    #[test]
    fn managed_session_toggle_pause_replays_from_finished_via_ready_then_playing() {
        let mut session = MacosManagedNativeSession::new(
            "fixture.mp4",
            demo_media_info(),
            FakeCommandSink::default(),
        );

        session.emit_initial_runtime_events(PlayerRuntimeStartup {
            ffmpeg_initialized: false,
            audio_output: None,
            decoded_audio: None,
            video_decode: None,
        });
        let _ = session.drain_events();

        session.apply_snapshot(&MacosAvFoundationSnapshot {
            item_status: MacosPlayerItemStatus::ReadyToPlay,
            time_control_status: MacosTimeControlStatus::Paused,
            playback_rate: 1.0,
            position: Duration::from_secs(42),
            duration: Some(Duration::from_secs(42)),
            reached_end: true,
            error_message: None,
        });
        let _ = session.drain_events();

        let result = session
            .dispatch(PlayerRuntimeCommand::TogglePause)
            .expect("toggle pause should replay from finished");
        assert!(result.applied);
        assert_eq!(result.snapshot.state, PresentationState::Playing);
        assert_eq!(result.snapshot.progress.position(), Duration::ZERO);

        let events = session.drain_events();
        assert!(matches!(
            events.as_slice(),
            [
                PlayerRuntimeEvent::PlaybackStateChanged(PresentationState::Ready),
                PlayerRuntimeEvent::PlaybackStateChanged(PresentationState::Playing),
            ]
        ));
    }

    #[test]
    fn managed_session_replaces_video_surface_and_emits_surface_events() {
        let sink = FakeCommandSink::default();
        let attached_surfaces = sink.attached_surfaces.clone();
        let mut session = MacosManagedNativeSession::with_existing_controller(
            "fixture.mp4",
            demo_media_info(),
            macos_native_capabilities(),
            None,
            sink,
            MacosManagedNativeSessionController::default(),
        );
        let surface = PlayerVideoSurfaceTarget {
            kind: PlayerVideoSurfaceKind::PlayerLayer,
            handle: 0x1234,
        };

        session
            .replace_video_surface(Some(surface))
            .expect("attaching a video surface should succeed");

        assert!(session.snapshot().has_video_surface);
        assert!(session.drain_events().iter().any(|event| matches!(
            event,
            PlayerRuntimeEvent::VideoSurfaceChanged { attached: true }
        )));
        assert_eq!(
            attached_surfaces.lock().expect("surface ops").as_slice(),
            &[Some(surface)]
        );

        session
            .replace_video_surface(None)
            .expect("detaching a video surface should succeed");

        assert!(!session.snapshot().has_video_surface);
        assert!(session.drain_events().iter().any(|event| matches!(
            event,
            PlayerRuntimeEvent::VideoSurfaceChanged { attached: false }
        )));
        assert_eq!(
            attached_surfaces.lock().expect("surface ops").as_slice(),
            &[Some(surface), None]
        );
    }

    #[test]
    fn managed_session_emits_interruption_events_and_clears_buffering() {
        let controller = MacosManagedNativeSessionController::default();
        let mut session = MacosManagedNativeSession::with_existing_controller(
            "fixture.mp4",
            demo_media_info(),
            macos_native_capabilities(),
            None,
            FakeCommandSink::default(),
            controller.clone(),
        );

        session.emit_initial_runtime_events(PlayerRuntimeStartup {
            ffmpeg_initialized: false,
            audio_output: None,
            decoded_audio: None,
            video_decode: None,
        });
        let _ = session.drain_events();

        let result = session
            .dispatch(PlayerRuntimeCommand::Play)
            .expect("play should establish playback intent before interruption");
        assert!(result.applied);
        let _ = session.drain_events();

        session.apply_snapshot(&MacosAvFoundationSnapshot {
            item_status: MacosPlayerItemStatus::ReadyToPlay,
            time_control_status: MacosTimeControlStatus::WaitingToPlay,
            playback_rate: 1.0,
            position: Duration::from_secs(1),
            duration: Some(Duration::from_secs(42)),
            reached_end: false,
            error_message: None,
        });
        assert!(session.snapshot().is_buffering);
        let _ = session.drain_events();

        controller.report_interruption_changed(true);
        let interrupt_events = session.drain_events();
        assert!(session.snapshot().is_interrupted);
        assert!(!session.snapshot().is_buffering);
        assert!(interrupt_events.iter().any(|event| matches!(
            event,
            PlayerRuntimeEvent::InterruptionChanged { interrupted: true }
        )));
        assert!(interrupt_events.iter().any(|event| matches!(
            event,
            PlayerRuntimeEvent::BufferingChanged { buffering: false }
        )));

        controller.report_interruption_changed(false);
        let resume_events = session.drain_events();
        assert!(!session.snapshot().is_interrupted);
        assert!(resume_events.iter().any(|event| matches!(
            event,
            PlayerRuntimeEvent::InterruptionChanged { interrupted: false }
        )));
    }

    #[test]
    fn managed_session_pause_during_interruption_keeps_interrupted_and_clears_buffering() {
        let controller = MacosManagedNativeSessionController::default();
        let mut session = MacosManagedNativeSession::with_existing_controller(
            "fixture.mp4",
            demo_media_info(),
            macos_native_capabilities(),
            None,
            FakeCommandSink::default(),
            controller.clone(),
        );

        session.emit_initial_runtime_events(PlayerRuntimeStartup {
            ffmpeg_initialized: false,
            audio_output: None,
            decoded_audio: None,
            video_decode: None,
        });
        let _ = session.drain_events();
        let _ = session
            .dispatch(PlayerRuntimeCommand::Play)
            .expect("play should succeed");
        let _ = session.drain_events();

        session.apply_snapshot(&MacosAvFoundationSnapshot {
            item_status: MacosPlayerItemStatus::ReadyToPlay,
            time_control_status: MacosTimeControlStatus::WaitingToPlay,
            playback_rate: 1.0,
            position: Duration::from_secs(2),
            duration: Some(Duration::from_secs(42)),
            reached_end: false,
            error_message: None,
        });
        let _ = session.drain_events();
        controller.report_interruption_changed(true);
        let _ = session.drain_events();

        let result = session
            .dispatch(PlayerRuntimeCommand::Pause)
            .expect("pause should still succeed while interrupted");
        assert!(result.applied);
        assert_eq!(result.snapshot.state, PresentationState::Paused);
        assert!(result.snapshot.is_interrupted);
        assert!(!result.snapshot.is_buffering);

        let events = session.drain_events();
        assert!(events.iter().any(|event| matches!(
            event,
            PlayerRuntimeEvent::PlaybackStateChanged(PresentationState::Paused)
        )));
        assert!(!events.iter().any(|event| matches!(
            event,
            PlayerRuntimeEvent::InterruptionChanged { interrupted: false }
        )));
    }

    #[test]
    fn managed_session_stop_during_interruption_rewinds_but_preserves_interrupted_flag() {
        let controller = MacosManagedNativeSessionController::default();
        let mut session = MacosManagedNativeSession::with_existing_controller(
            "fixture.mp4",
            demo_media_info(),
            macos_native_capabilities(),
            None,
            FakeCommandSink::default(),
            controller.clone(),
        );

        session.emit_initial_runtime_events(PlayerRuntimeStartup {
            ffmpeg_initialized: false,
            audio_output: None,
            decoded_audio: None,
            video_decode: None,
        });
        let _ = session.drain_events();
        let _ = session
            .dispatch(PlayerRuntimeCommand::Play)
            .expect("play should succeed");
        let _ = session.drain_events();
        controller.report_interruption_changed(true);
        let _ = session.drain_events();

        let result = session
            .dispatch(PlayerRuntimeCommand::Stop)
            .expect("stop should succeed while interrupted");
        assert!(result.applied);
        assert_eq!(result.snapshot.state, PresentationState::Ready);
        assert_eq!(result.snapshot.progress.position(), Duration::ZERO);
        assert!(result.snapshot.is_interrupted);

        let events = session.drain_events();
        assert!(events.iter().any(|event| matches!(
            event,
            PlayerRuntimeEvent::PlaybackStateChanged(PresentationState::Ready)
        )));
        assert!(!events.iter().any(|event| matches!(
            event,
            PlayerRuntimeEvent::InterruptionChanged { interrupted: false }
        )));
    }

    fn demo_media_info() -> PlayerMediaInfo {
        PlayerMediaInfo {
            source_uri: "fixture.mp4".to_owned(),
            source_kind: player_runtime::MediaSourceKind::Local,
            source_protocol: player_runtime::MediaSourceProtocol::File,
            duration: Some(Duration::from_secs(42)),
            bit_rate: Some(800_000),
            audio_streams: 1,
            video_streams: 1,
            best_video: Some(PlayerVideoInfo {
                codec: "H264".to_owned(),
                width: 960,
                height: 432,
                frame_rate: Some(30.0),
            }),
            best_audio: None,
            track_catalog: Default::default(),
            track_selection: Default::default(),
        }
    }

    struct FakeMacosBridge;

    impl MacosNativePlayerBridge for FakeMacosBridge {
        fn probe_source(
            &self,
            source: &MediaSource,
            _options: &PlayerRuntimeOptions,
        ) -> PlayerRuntimeResult<MacosNativePlayerProbe> {
            Ok(MacosNativePlayerProbe {
                media_info: PlayerMediaInfo {
                    source_uri: source.uri().to_owned(),
                    source_kind: source.kind(),
                    source_protocol: source.protocol(),
                    duration: Some(Duration::from_secs(60)),
                    bit_rate: Some(1_000_000),
                    audio_streams: 1,
                    video_streams: 1,
                    best_video: None,
                    best_audio: None,
                    track_catalog: Default::default(),
                    track_selection: Default::default(),
                },
                startup: PlayerRuntimeStartup {
                    ffmpeg_initialized: false,
                    audio_output: None,
                    decoded_audio: None,
                    video_decode: None,
                },
            })
        }

        fn initialize_session(
            &self,
            source: MediaSource,
            _options: PlayerRuntimeOptions,
            media_info: &PlayerMediaInfo,
            _startup: &PlayerRuntimeStartup,
        ) -> PlayerRuntimeResult<MacosNativePlayerSessionBootstrap> {
            Ok(MacosNativePlayerSessionBootstrap {
                runtime: Box::new(MacosManagedNativeSession::new(
                    source.uri(),
                    media_info.clone(),
                    FakeCommandSink::default(),
                )),
                initial_frame: None,
            })
        }
    }

    #[derive(Default)]
    struct FakeCommandSink {
        commands: Arc<Mutex<Vec<MacosNativePlayerCommand>>>,
        attached_surfaces: Arc<Mutex<Vec<Option<PlayerVideoSurfaceTarget>>>>,
    }

    impl MacosNativeCommandSink for FakeCommandSink {
        fn submit_command(&mut self, command: MacosNativePlayerCommand) -> PlayerRuntimeResult<()> {
            self.commands
                .lock()
                .expect("fake command list should stay lockable")
                .push(command);
            Ok(())
        }

        fn attach_video_surface(
            &mut self,
            video_surface: PlayerVideoSurfaceTarget,
        ) -> PlayerRuntimeResult<()> {
            self.attached_surfaces
                .lock()
                .expect("fake surface ops should stay lockable")
                .push(Some(video_surface));
            Ok(())
        }

        fn detach_video_surface(&mut self) -> PlayerRuntimeResult<()> {
            self.attached_surfaces
                .lock()
                .expect("fake surface ops should stay lockable")
                .push(None);
            Ok(())
        }
    }

    struct FakeBindings {
        commands: Arc<Mutex<Vec<MacosNativePlayerCommand>>>,
        contexts: Arc<Mutex<Vec<MacosAvFoundationBridgeContext>>>,
    }

    impl MacosAvFoundationBridgeBindings for FakeBindings {
        fn probe_source(
            &self,
            _context: &MacosAvFoundationBridgeContext,
            source: &MediaSource,
            _options: &PlayerRuntimeOptions,
        ) -> PlayerRuntimeResult<MacosNativePlayerProbe> {
            Ok(MacosNativePlayerProbe {
                media_info: PlayerMediaInfo {
                    source_uri: source.uri().to_owned(),
                    source_kind: source.kind(),
                    source_protocol: source.protocol(),
                    duration: Some(Duration::from_secs(42)),
                    bit_rate: Some(800_000),
                    audio_streams: 1,
                    video_streams: 1,
                    best_video: Some(PlayerVideoInfo {
                        codec: "H264".to_owned(),
                        width: 960,
                        height: 432,
                        frame_rate: Some(30.0),
                    }),
                    best_audio: None,
                    track_catalog: Default::default(),
                    track_selection: Default::default(),
                },
                startup: PlayerRuntimeStartup {
                    ffmpeg_initialized: false,
                    audio_output: None,
                    decoded_audio: None,
                    video_decode: None,
                },
            })
        }

        fn create_command_sink(
            &self,
            context: MacosAvFoundationBridgeContext,
            _source: &MediaSource,
            _options: &PlayerRuntimeOptions,
            _media_info: &PlayerMediaInfo,
            _startup: &PlayerRuntimeStartup,
            _controller: super::MacosManagedNativeSessionController,
        ) -> PlayerRuntimeResult<Box<dyn MacosNativeCommandSink>> {
            self.contexts
                .lock()
                .expect("context list should stay lockable")
                .push(context);
            Ok(Box::new(FakeCommandSink {
                commands: self.commands.clone(),
                attached_surfaces: Arc::new(Mutex::new(Vec::new())),
            }))
        }
    }

    #[test]
    fn avfoundation_bridge_builds_managed_session() {
        let commands = Arc::new(Mutex::new(Vec::new()));
        let contexts = Arc::new(Mutex::new(Vec::new()));
        let bridge = MacosAvFoundationBridge::new(
            MacosAvFoundationBridgeContext {
                video_surface: Some(PlayerVideoSurfaceTarget {
                    kind: PlayerVideoSurfaceKind::PlayerLayer,
                    handle: 7,
                }),
            },
            Arc::new(FakeBindings {
                commands: commands.clone(),
                contexts: contexts.clone(),
            }),
        );

        let probe = bridge
            .probe_source(
                &MediaSource::new("fixture.mp4"),
                &PlayerRuntimeOptions::default(),
            )
            .expect("probe should succeed");
        let bootstrap = bridge
            .initialize_session(
                MediaSource::new("fixture.mp4"),
                PlayerRuntimeOptions::default(),
                &probe.media_info,
                &probe.startup,
            )
            .expect("session bootstrap should succeed");
        let mut runtime = bootstrap.runtime;

        let result = runtime
            .dispatch(PlayerRuntimeCommand::Play)
            .expect("play should translate to native command");
        assert!(result.applied);
        assert_eq!(runtime.presentation_state(), PresentationState::Playing);
        assert_eq!(commands.lock().expect("command list lock").len(), 1);
        assert_eq!(contexts.lock().expect("context list lock").len(), 1);
    }

    #[test]
    fn avfoundation_bridge_requires_surface_for_video_playback() {
        let bridge = MacosAvFoundationBridge::new(
            MacosAvFoundationBridgeContext {
                video_surface: None,
            },
            Arc::new(FakeBindings {
                commands: Arc::new(Mutex::new(Vec::new())),
                contexts: Arc::new(Mutex::new(Vec::new())),
            }),
        );

        let probe = bridge
            .probe_source(
                &MediaSource::new("fixture.mp4"),
                &PlayerRuntimeOptions::default(),
            )
            .expect("probe should succeed");
        let error = match bridge.initialize_session(
            MediaSource::new("fixture.mp4"),
            PlayerRuntimeOptions::default(),
            &probe.media_info,
            &probe.startup,
        ) {
            Ok(_) => panic!("video native playback should require a surface"),
            Err(error) => error,
        };

        assert_eq!(error.code(), PlayerRuntimeErrorCode::InvalidArgument);
    }

    #[test]
    fn avfoundation_bridge_prefers_surface_from_runtime_options() {
        let commands = Arc::new(Mutex::new(Vec::new()));
        let contexts = Arc::new(Mutex::new(Vec::new()));
        let bridge = MacosAvFoundationBridge::new(
            MacosAvFoundationBridgeContext {
                video_surface: Some(PlayerVideoSurfaceTarget {
                    kind: PlayerVideoSurfaceKind::PlayerLayer,
                    handle: 7,
                }),
            },
            Arc::new(FakeBindings {
                commands,
                contexts: contexts.clone(),
            }),
        );

        let options =
            PlayerRuntimeOptions::default().with_video_surface(PlayerVideoSurfaceTarget {
                kind: PlayerVideoSurfaceKind::NsView,
                handle: 42,
            });
        let probe = bridge
            .probe_source(&MediaSource::new("fixture.mp4"), &options)
            .expect("probe should succeed");
        let _bootstrap = bridge
            .initialize_session(
                MediaSource::new("fixture.mp4"),
                options,
                &probe.media_info,
                &probe.startup,
            )
            .expect("session bootstrap should succeed");

        let context = contexts
            .lock()
            .expect("context list lock")
            .last()
            .copied()
            .expect("binding should receive resolved context");
        assert_eq!(
            context.video_surface,
            Some(PlayerVideoSurfaceTarget {
                kind: PlayerVideoSurfaceKind::NsView,
                handle: 42,
            })
        );
    }
}
