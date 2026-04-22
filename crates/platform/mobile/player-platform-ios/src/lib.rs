mod download;
mod playlist;
mod preload;

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use player_core::MediaSource;
use player_runtime::{
    DEFAULT_PLAYBACK_RATE, DecodedVideoFrame, MAX_PLAYBACK_RATE, MIN_PLAYBACK_RATE, MediaAbrMode,
    MediaAbrPolicy, MediaTrackCatalog, MediaTrackKind, MediaTrackSelection,
    MediaTrackSelectionMode, MediaTrackSelectionSnapshot, PlaybackProgress, PlayerMediaInfo,
    PlayerResilienceMetrics, PlayerResilienceMetricsTracker, PlayerRuntimeAdapter,
    PlayerRuntimeAdapterBackendFamily, PlayerRuntimeAdapterBootstrap,
    PlayerRuntimeAdapterCapabilities, PlayerRuntimeAdapterFactory, PlayerRuntimeAdapterInitializer,
    PlayerRuntimeCommand, PlayerRuntimeCommandResult, PlayerRuntimeError,
    PlayerRuntimeErrorCategory, PlayerRuntimeErrorCode, PlayerRuntimeEvent, PlayerRuntimeOptions,
    PlayerRuntimeResult, PlayerRuntimeStartup, PlayerSnapshot, PlayerTimelineKind,
    PlayerTimelineSnapshot, PlayerVideoSurfaceKind, PlayerVideoSurfaceTarget, PresentationState,
};

pub use download::{IosDownloadBridgeSession, IosDownloadCommand};
pub use playlist::IosPlaylistBridgeSession;
pub use preload::{IosPreloadBridgeSession, IosPreloadCommand};

pub const IOS_NATIVE_PLAYER_RUNTIME_ADAPTER_ID: &str = "ios_native";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IosOpaqueHandle(pub usize);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IosVideoSurfaceKind {
    UiView,
    PlayerLayer,
    MetalLayer,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IosVideoSurfaceTarget {
    pub kind: IosVideoSurfaceKind,
    pub handle: IosOpaqueHandle,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IosAvPlayerBridgeContext {
    pub av_player: IosOpaqueHandle,
    pub video_surface: Option<IosVideoSurfaceTarget>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IosPlayerItemStatus {
    Unknown,
    ReadyToPlay,
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IosTimeControlStatus {
    Paused,
    WaitingToPlay,
    Playing,
}

#[derive(Debug, Clone)]
pub struct IosAvPlayerSnapshot {
    pub item_status: IosPlayerItemStatus,
    pub time_control_status: IosTimeControlStatus,
    pub playback_rate: f32,
    pub position: Duration,
    pub duration: Option<Duration>,
    pub reached_end: bool,
    pub error_message: Option<String>,
}

#[derive(Debug, Clone)]
pub struct IosNativeObservation {
    pub presentation_state: PresentationState,
    pub is_buffering: bool,
    pub playback_rate: f32,
    pub progress: PlaybackProgress,
    pub emitted_events: Vec<PlayerRuntimeEvent>,
}

#[derive(Debug, Default, Clone)]
pub struct IosAvPlayerStateTracker {
    has_started_playback: bool,
    playback_intent: bool,
    last_presentation_state: Option<PresentationState>,
    last_is_buffering: Option<bool>,
    last_playback_rate: Option<f32>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum IosNativePlayerCommand {
    Play,
    Pause,
    SeekTo { position: Duration },
    Stop,
    SetPlaybackRate { rate: f32 },
    SetAudioTrackSelection { selection: MediaTrackSelection },
    SetSubtitleTrackSelection { selection: MediaTrackSelection },
    SetAbrPolicy { policy: MediaAbrPolicy },
}

pub trait IosNativeCommandSink: Send {
    fn submit_command(&mut self, command: IosNativePlayerCommand) -> PlayerRuntimeResult<()>;
    fn attach_video_surface(
        &mut self,
        _video_surface: PlayerVideoSurfaceTarget,
    ) -> PlayerRuntimeResult<()> {
        Err(PlayerRuntimeError::new(
            PlayerRuntimeErrorCode::Unsupported,
            "ios native command sink does not support attaching a video surface",
        ))
    }

    fn detach_video_surface(&mut self) -> PlayerRuntimeResult<()> {
        Err(PlayerRuntimeError::new(
            PlayerRuntimeErrorCode::Unsupported,
            "ios native command sink does not support detaching a video surface",
        ))
    }
}

impl<T> IosNativeCommandSink for Box<T>
where
    T: IosNativeCommandSink + ?Sized,
{
    fn submit_command(&mut self, command: IosNativePlayerCommand) -> PlayerRuntimeResult<()> {
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
pub enum IosNativeSessionUpdate {
    Snapshot(IosAvPlayerSnapshot),
    MediaInfo {
        track_catalog: MediaTrackCatalog,
        track_selection: MediaTrackSelectionSnapshot,
    },
    InterruptionChanged {
        interrupted: bool,
    },
    SeekCompleted {
        position: Duration,
    },
    RetryScheduled {
        attempt: u32,
        delay: Duration,
    },
    Error(PlayerRuntimeError),
}

#[derive(Debug, Clone, Default)]
pub struct IosManagedNativeSessionController {
    updates: Arc<Mutex<VecDeque<IosNativeSessionUpdate>>>,
}

pub struct IosManagedNativeSession<C> {
    source_uri: String,
    media_info: PlayerMediaInfo,
    capabilities: PlayerRuntimeAdapterCapabilities,
    command_sink: C,
    controller: IosManagedNativeSessionController,
    tracker: IosAvPlayerStateTracker,
    presentation_state: PresentationState,
    video_surface: Option<PlayerVideoSurfaceTarget>,
    is_interrupted: bool,
    is_buffering: bool,
    playback_rate: f32,
    progress: PlaybackProgress,
    resilience_metrics: PlayerResilienceMetricsTracker,
    events: VecDeque<PlayerRuntimeEvent>,
}

pub trait IosNativePlayerBridge: Send + Sync {
    fn probe_source(
        &self,
        source: &MediaSource,
        options: &PlayerRuntimeOptions,
    ) -> PlayerRuntimeResult<IosNativePlayerProbe>;

    fn initialize_session(
        &self,
        source: MediaSource,
        options: PlayerRuntimeOptions,
        media_info: &PlayerMediaInfo,
        startup: &PlayerRuntimeStartup,
    ) -> PlayerRuntimeResult<IosNativePlayerSessionBootstrap>;
}

pub trait IosAvPlayerBridgeBindings: Send + Sync {
    fn probe_source(
        &self,
        context: &IosAvPlayerBridgeContext,
        source: &MediaSource,
        options: &PlayerRuntimeOptions,
    ) -> PlayerRuntimeResult<IosNativePlayerProbe>;

    fn create_command_sink(
        &self,
        context: IosAvPlayerBridgeContext,
        source: &MediaSource,
        options: &PlayerRuntimeOptions,
        media_info: &PlayerMediaInfo,
        startup: &PlayerRuntimeStartup,
        controller: IosManagedNativeSessionController,
    ) -> PlayerRuntimeResult<Box<dyn IosNativeCommandSink>>;
}

pub trait IosNativePlayerSession: Send {
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
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IosHostTimelineKind {
    Vod,
    Live,
    LiveDvr,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IosHostSeekableRange {
    pub start_ms: u64,
    pub end_ms: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct IosHostSnapshot {
    pub playback_state: PresentationState,
    pub playback_rate: f32,
    pub is_buffering: bool,
    pub is_interrupted: bool,
    pub has_video_surface: bool,
    pub timeline_kind: IosHostTimelineKind,
    pub is_seekable: bool,
    pub seekable_range: Option<IosHostSeekableRange>,
    pub live_edge_ms: Option<u64>,
    pub position_ms: u64,
    pub duration_ms: Option<u64>,
    pub resilience_metrics: PlayerResilienceMetrics,
}

#[derive(Debug, Clone, PartialEq)]
pub enum IosHostEvent {
    PlaybackStateChanged {
        state: PresentationState,
    },
    PlaybackRateChanged {
        rate: f32,
    },
    BufferingChanged {
        buffering: bool,
    },
    InterruptionChanged {
        interrupted: bool,
    },
    VideoSurfaceChanged {
        attached: bool,
    },
    SeekCompleted {
        position_ms: u64,
    },
    RetryScheduled {
        attempt: u32,
        delay_ms: u64,
    },
    Ended,
    Error {
        code: PlayerRuntimeErrorCode,
        category: PlayerRuntimeErrorCategory,
        retriable: bool,
        message: String,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub enum IosHostCommand {
    Play,
    Pause,
    SeekTo { position_ms: u64 },
    Stop,
    SetPlaybackRate { rate: f32 },
    SetAudioTrackSelection { selection: MediaTrackSelection },
    SetSubtitleTrackSelection { selection: MediaTrackSelection },
    SetAbrPolicy { policy: MediaAbrPolicy },
}

pub struct IosHostBridgeSession {
    session: IosManagedNativeSession<IosHostCommandSink>,
    command_queue: Arc<Mutex<VecDeque<IosNativePlayerCommand>>>,
    surface_attached: bool,
    extra_events: VecDeque<PlayerRuntimeEvent>,
}

#[derive(Debug, Clone)]
pub struct IosNativePlayerProbe {
    pub media_info: PlayerMediaInfo,
    pub startup: PlayerRuntimeStartup,
}

pub struct IosNativePlayerSessionBootstrap {
    pub runtime: Box<dyn IosNativePlayerSession>,
    pub initial_frame: Option<DecodedVideoFrame>,
}

#[derive(Clone)]
pub struct IosAvPlayerBridge {
    context: IosAvPlayerBridgeContext,
    bindings: Arc<dyn IosAvPlayerBridgeBindings>,
}

#[derive(Clone, Default)]
pub struct IosNativePlayerRuntimeAdapterFactory {
    bridge: Option<Arc<dyn IosNativePlayerBridge>>,
}

pub struct IosNativePlayerRuntimeInitializer {
    bridge: Option<Arc<dyn IosNativePlayerBridge>>,
    source: MediaSource,
    options: PlayerRuntimeOptions,
    media_info: PlayerMediaInfo,
    startup: PlayerRuntimeStartup,
}

pub struct IosNativePlayerRuntime {
    inner: Box<dyn IosNativePlayerSession>,
}

#[derive(Debug, Clone)]
struct IosHostCommandSink {
    queue: Arc<Mutex<VecDeque<IosNativePlayerCommand>>>,
}

impl IosHostCommandSink {
    fn new(queue: Arc<Mutex<VecDeque<IosNativePlayerCommand>>>) -> Self {
        Self { queue }
    }
}

impl IosNativeCommandSink for IosHostCommandSink {
    fn submit_command(&mut self, command: IosNativePlayerCommand) -> PlayerRuntimeResult<()> {
        if let Ok(mut queue) = self.queue.lock() {
            queue.push_back(command);
        }
        Ok(())
    }
}

impl<C> std::fmt::Debug for IosManagedNativeSession<C> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("IosManagedNativeSession")
            .field("source_uri", &self.source_uri)
            .field("state", &self.presentation_state)
            .field("playback_rate", &self.playback_rate)
            .finish()
    }
}

impl std::fmt::Debug for IosNativePlayerRuntimeAdapterFactory {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("IosNativePlayerRuntimeAdapterFactory")
            .field("has_bridge", &self.bridge.is_some())
            .finish()
    }
}

impl std::fmt::Debug for IosNativePlayerRuntimeInitializer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("IosNativePlayerRuntimeInitializer")
            .field("source", &self.source.uri())
            .field("has_bridge", &self.bridge.is_some())
            .finish()
    }
}

impl std::fmt::Debug for IosNativePlayerRuntime {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("IosNativePlayerRuntime")
            .field("source_uri", &self.inner.source_uri())
            .field("state", &self.inner.presentation_state())
            .finish()
    }
}

impl std::fmt::Debug for IosAvPlayerBridge {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("IosAvPlayerBridge")
            .field("context", &self.context)
            .finish()
    }
}

impl IosNativePlayerRuntimeAdapterFactory {
    pub fn with_bridge(bridge: Arc<dyn IosNativePlayerBridge>) -> Self {
        Self {
            bridge: Some(bridge),
        }
    }
}

impl IosAvPlayerBridge {
    pub fn new(
        context: IosAvPlayerBridgeContext,
        bindings: Arc<dyn IosAvPlayerBridgeBindings>,
    ) -> Self {
        Self { context, bindings }
    }
}

impl IosHostSnapshot {
    pub fn from_player_snapshot(snapshot: &PlayerSnapshot) -> Self {
        Self {
            playback_state: snapshot.state,
            playback_rate: snapshot.playback_rate,
            is_buffering: snapshot.is_buffering,
            is_interrupted: snapshot.is_interrupted,
            has_video_surface: snapshot.has_video_surface,
            timeline_kind: host_timeline_kind(snapshot.timeline.kind),
            is_seekable: snapshot.timeline.is_seekable,
            seekable_range: snapshot
                .timeline
                .seekable_range
                .map(|range| IosHostSeekableRange {
                    start_ms: duration_to_millis(range.start),
                    end_ms: duration_to_millis(range.end),
                }),
            live_edge_ms: snapshot
                .timeline
                .effective_live_edge()
                .map(duration_to_millis),
            position_ms: duration_to_millis(snapshot.timeline.position),
            duration_ms: snapshot.timeline.duration.map(duration_to_millis),
            resilience_metrics: snapshot.resilience_metrics.clone(),
        }
    }
}

impl IosHostEvent {
    pub fn from_runtime_event(event: &PlayerRuntimeEvent) -> Option<Self> {
        match event {
            PlayerRuntimeEvent::PlaybackStateChanged(state) => {
                Some(Self::PlaybackStateChanged { state: *state })
            }
            PlayerRuntimeEvent::PlaybackRateChanged { rate } => {
                Some(Self::PlaybackRateChanged { rate: *rate })
            }
            PlayerRuntimeEvent::BufferingChanged { buffering } => Some(Self::BufferingChanged {
                buffering: *buffering,
            }),
            PlayerRuntimeEvent::InterruptionChanged { interrupted } => {
                Some(Self::InterruptionChanged {
                    interrupted: *interrupted,
                })
            }
            PlayerRuntimeEvent::VideoSurfaceChanged { attached } => {
                Some(Self::VideoSurfaceChanged {
                    attached: *attached,
                })
            }
            PlayerRuntimeEvent::SeekCompleted { position } => Some(Self::SeekCompleted {
                position_ms: duration_to_millis(*position),
            }),
            PlayerRuntimeEvent::RetryScheduled { attempt, delay } => Some(Self::RetryScheduled {
                attempt: *attempt,
                delay_ms: duration_to_millis(*delay),
            }),
            PlayerRuntimeEvent::Ended => Some(Self::Ended),
            PlayerRuntimeEvent::Error(error) => Some(Self::Error {
                code: error.code(),
                category: error.category(),
                retriable: error.is_retriable(),
                message: error.message().to_owned(),
            }),
            PlayerRuntimeEvent::Initialized(_)
            | PlayerRuntimeEvent::MetadataReady(_)
            | PlayerRuntimeEvent::FirstFrameReady(_)
            | PlayerRuntimeEvent::AudioOutputChanged(_) => None,
        }
    }
}

impl IosHostCommand {
    pub fn from_native_command(command: &IosNativePlayerCommand) -> Self {
        match command {
            IosNativePlayerCommand::Play => Self::Play,
            IosNativePlayerCommand::Pause => Self::Pause,
            IosNativePlayerCommand::SeekTo { position } => Self::SeekTo {
                position_ms: duration_to_millis(*position),
            },
            IosNativePlayerCommand::Stop => Self::Stop,
            IosNativePlayerCommand::SetPlaybackRate { rate } => {
                Self::SetPlaybackRate { rate: *rate }
            }
            IosNativePlayerCommand::SetAudioTrackSelection { selection } => {
                Self::SetAudioTrackSelection {
                    selection: selection.clone(),
                }
            }
            IosNativePlayerCommand::SetSubtitleTrackSelection { selection } => {
                Self::SetSubtitleTrackSelection {
                    selection: selection.clone(),
                }
            }
            IosNativePlayerCommand::SetAbrPolicy { policy } => Self::SetAbrPolicy {
                policy: policy.clone(),
            },
        }
    }
}

impl IosHostBridgeSession {
    pub fn new(source_uri: impl Into<String>) -> Self {
        let source_uri = source_uri.into();
        let command_queue = Arc::new(Mutex::new(VecDeque::new()));
        let source = MediaSource::new(source_uri.clone());
        let media_info = placeholder_media_info(&source);
        let sink = IosHostCommandSink::new(command_queue.clone());
        let session = IosManagedNativeSession::new(source_uri, media_info, sink);

        Self {
            session,
            command_queue,
            surface_attached: false,
            extra_events: VecDeque::new(),
        }
    }

    pub fn snapshot(&mut self) -> IosHostSnapshot {
        IosHostSnapshot::from_player_snapshot(&self.session.snapshot())
    }

    pub fn drain_events(&mut self) -> Vec<IosHostEvent> {
        let mut raw_events: Vec<PlayerRuntimeEvent> = self.extra_events.drain(..).collect();
        raw_events.extend(self.session.drain_events());
        raw_events
            .iter()
            .filter_map(IosHostEvent::from_runtime_event)
            .collect()
    }

    pub fn drain_native_commands(&mut self) -> Vec<IosHostCommand> {
        self.command_queue
            .lock()
            .map(|mut queue| {
                queue
                    .drain(..)
                    .map(|command| IosHostCommand::from_native_command(&command))
                    .collect()
            })
            .unwrap_or_default()
    }

    pub fn dispatch_command(
        &mut self,
        command: PlayerRuntimeCommand,
    ) -> PlayerRuntimeResult<PlayerRuntimeCommandResult> {
        self.session.dispatch(command)
    }

    pub fn set_surface_attached(&mut self, attached: bool) {
        if self.surface_attached == attached {
            return;
        }

        self.surface_attached = attached;
        self.session.video_surface = attached.then_some(host_video_surface_target());
        self.extra_events
            .push_back(PlayerRuntimeEvent::VideoSurfaceChanged { attached });
    }

    pub fn apply_avplayer_snapshot(&mut self, snapshot: IosAvPlayerSnapshot) {
        self.session.apply_snapshot(&snapshot);
    }

    pub fn report_seek_completed(&mut self, position: Duration) {
        self.session.controller().report_seek_completed(position);
    }

    pub fn report_retry_scheduled(&mut self, attempt: u32, delay: Duration) {
        self.session
            .controller()
            .report_retry_scheduled(attempt, delay);
    }

    pub fn report_interruption_changed(&mut self, interrupted: bool) {
        self.session
            .controller()
            .report_interruption_changed(interrupted);
    }

    pub fn report_error(&mut self, code: PlayerRuntimeErrorCode, message: impl Into<String>) {
        self.session.controller().report_error(code, message);
    }

    pub fn report_runtime_error(&mut self, error: PlayerRuntimeError) {
        self.session.controller().report_runtime_error(error);
    }
}

impl PlayerRuntimeAdapterFactory for IosNativePlayerRuntimeAdapterFactory {
    fn adapter_id(&self) -> &'static str {
        IOS_NATIVE_PLAYER_RUNTIME_ADAPTER_ID
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

        Ok(Box::new(IosNativePlayerRuntimeInitializer {
            bridge: self.bridge.clone(),
            source,
            options,
            media_info,
            startup,
        }))
    }
}

impl PlayerRuntimeAdapterInitializer for IosNativePlayerRuntimeInitializer {
    fn capabilities(&self) -> PlayerRuntimeAdapterCapabilities {
        ios_native_capabilities()
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
                ios_native_unavailable_message(),
            ));
        };

        let bootstrap = bridge.initialize_session(source, options, &media_info, &startup)?;

        Ok(PlayerRuntimeAdapterBootstrap {
            runtime: Box::new(IosNativePlayerRuntime {
                inner: bootstrap.runtime,
            }),
            initial_frame: bootstrap.initial_frame,
            startup,
        })
    }
}

impl PlayerRuntimeAdapter for IosNativePlayerRuntime {
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

impl IosAvPlayerStateTracker {
    pub fn observe(&mut self, snapshot: &IosAvPlayerSnapshot) -> IosNativeObservation {
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

        IosNativeObservation {
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

    fn presentation_state(&self, snapshot: &IosAvPlayerSnapshot) -> PresentationState {
        if snapshot.reached_end {
            return PresentationState::Finished;
        }

        match snapshot.item_status {
            IosPlayerItemStatus::Failed => PresentationState::Paused,
            IosPlayerItemStatus::Unknown => {
                if self.playback_intent {
                    PresentationState::Playing
                } else if self.has_started_playback {
                    PresentationState::Paused
                } else {
                    PresentationState::Ready
                }
            }
            IosPlayerItemStatus::ReadyToPlay => match snapshot.time_control_status {
                IosTimeControlStatus::Playing => PresentationState::Playing,
                IosTimeControlStatus::Paused | IosTimeControlStatus::WaitingToPlay => {
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
        snapshot: &IosAvPlayerSnapshot,
        presentation_state: PresentationState,
    ) -> bool {
        if presentation_state != PresentationState::Playing || snapshot.reached_end {
            return false;
        }

        snapshot.item_status != IosPlayerItemStatus::Failed
            && snapshot.time_control_status == IosTimeControlStatus::WaitingToPlay
            && self.playback_intent
    }
}

impl IosManagedNativeSessionController {
    pub fn apply_snapshot(&self, snapshot: IosAvPlayerSnapshot) {
        self.push_update(IosNativeSessionUpdate::Snapshot(snapshot));
    }

    pub fn report_media_info(
        &self,
        track_catalog: MediaTrackCatalog,
        track_selection: MediaTrackSelectionSnapshot,
    ) {
        self.push_update(IosNativeSessionUpdate::MediaInfo {
            track_catalog,
            track_selection,
        });
    }

    pub fn report_interruption_changed(&self, interrupted: bool) {
        self.push_update(IosNativeSessionUpdate::InterruptionChanged { interrupted });
    }

    pub fn report_seek_completed(&self, position: Duration) {
        self.push_update(IosNativeSessionUpdate::SeekCompleted { position });
    }

    pub fn report_retry_scheduled(&self, attempt: u32, delay: Duration) {
        self.push_update(IosNativeSessionUpdate::RetryScheduled { attempt, delay });
    }

    pub fn report_error(&self, code: PlayerRuntimeErrorCode, message: impl Into<String>) {
        self.push_update(IosNativeSessionUpdate::Error(PlayerRuntimeError::new(
            code,
            message.into(),
        )));
    }

    pub fn report_runtime_error(&self, error: PlayerRuntimeError) {
        self.push_update(IosNativeSessionUpdate::Error(error));
    }

    pub fn push_update(&self, update: IosNativeSessionUpdate) {
        if let Ok(mut updates) = self.updates.lock() {
            updates.push_back(update);
        }
    }

    fn take_pending(&self) -> Vec<IosNativeSessionUpdate> {
        self.updates
            .lock()
            .map(|mut updates| updates.drain(..).collect())
            .unwrap_or_default()
    }
}

impl<C: IosNativeCommandSink> IosManagedNativeSession<C> {
    pub fn new(
        source_uri: impl Into<String>,
        media_info: PlayerMediaInfo,
        command_sink: C,
    ) -> Self {
        Self::with_capabilities(
            source_uri,
            media_info,
            ios_native_capabilities(),
            command_sink,
        )
    }

    pub fn with_capabilities(
        source_uri: impl Into<String>,
        media_info: PlayerMediaInfo,
        capabilities: PlayerRuntimeAdapterCapabilities,
        command_sink: C,
    ) -> Self {
        let (session, _) = Self::with_capabilities_and_controller(
            source_uri,
            media_info,
            capabilities,
            command_sink,
        );
        session
    }

    pub fn with_controller(
        source_uri: impl Into<String>,
        media_info: PlayerMediaInfo,
        command_sink: C,
    ) -> (Self, IosManagedNativeSessionController) {
        Self::with_capabilities_and_controller(
            source_uri,
            media_info,
            ios_native_capabilities(),
            command_sink,
        )
    }

    pub fn with_capabilities_and_controller(
        source_uri: impl Into<String>,
        media_info: PlayerMediaInfo,
        capabilities: PlayerRuntimeAdapterCapabilities,
        command_sink: C,
    ) -> (Self, IosManagedNativeSessionController) {
        let controller = IosManagedNativeSessionController::default();
        let session = Self::with_existing_controller(
            source_uri,
            media_info,
            capabilities,
            None,
            command_sink,
            controller.clone(),
        );
        (session, controller)
    }

    pub fn with_existing_controller(
        source_uri: impl Into<String>,
        media_info: PlayerMediaInfo,
        capabilities: PlayerRuntimeAdapterCapabilities,
        video_surface: Option<PlayerVideoSurfaceTarget>,
        command_sink: C,
        controller: IosManagedNativeSessionController,
    ) -> Self {
        Self {
            source_uri: source_uri.into(),
            media_info,
            capabilities,
            command_sink,
            controller,
            tracker: IosAvPlayerStateTracker::default(),
            presentation_state: PresentationState::Ready,
            video_surface,
            is_interrupted: false,
            is_buffering: false,
            playback_rate: DEFAULT_PLAYBACK_RATE,
            progress: PlaybackProgress::new(Duration::ZERO, None),
            resilience_metrics: PlayerResilienceMetricsTracker::default(),
            events: VecDeque::new(),
        }
    }

    pub fn controller(&self) -> IosManagedNativeSessionController {
        self.controller.clone()
    }

    fn pump_pending_updates(&mut self) {
        for update in self.controller.take_pending() {
            match update {
                IosNativeSessionUpdate::Snapshot(snapshot) => self.apply_snapshot(&snapshot),
                IosNativeSessionUpdate::MediaInfo {
                    track_catalog,
                    track_selection,
                } => {
                    if self.media_info.track_catalog != track_catalog
                        || self.media_info.track_selection != track_selection
                    {
                        self.media_info.track_catalog = track_catalog;
                        self.media_info.track_selection = track_selection;
                        self.events
                            .push_back(PlayerRuntimeEvent::MetadataReady(self.media_info.clone()));
                    }
                }
                IosNativeSessionUpdate::InterruptionChanged { interrupted } => {
                    self.apply_interruption(interrupted);
                }
                IosNativeSessionUpdate::SeekCompleted { position } => {
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
                IosNativeSessionUpdate::RetryScheduled { attempt, delay } => {
                    self.resilience_metrics
                        .observe_retry_scheduled(attempt, delay);
                    self.events
                        .push_back(PlayerRuntimeEvent::RetryScheduled { attempt, delay });
                }
                IosNativeSessionUpdate::Error(error) => {
                    self.events.push_back(PlayerRuntimeEvent::Error(error));
                }
            }
        }
    }

    pub fn pending_update_count(&self) -> usize {
        self.controller
            .updates
            .lock()
            .map(|updates| updates.len())
            .unwrap_or_default()
    }

    pub fn apply_snapshot(&mut self, snapshot: &IosAvPlayerSnapshot) {
        let observation = self.tracker.observe(snapshot);
        self.apply_observation(observation);
    }

    fn apply_observation(&mut self, observation: IosNativeObservation) {
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

    fn snapshot(&mut self) -> PlayerSnapshot {
        self.pump_pending_updates();
        PlayerSnapshot {
            source_uri: self.source_uri.clone(),
            state: self.presentation_state,
            has_video_surface: self.video_surface.is_some(),
            is_interrupted: self.is_interrupted,
            is_buffering: self.is_buffering,
            playback_rate: self.playback_rate,
            progress: self.progress,
            timeline: PlayerTimelineSnapshot::from_media_info(
                self.progress,
                self.capabilities.supports_seek,
                &self.media_info,
            ),
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
        commands: Vec<IosNativePlayerCommand>,
    ) -> PlayerRuntimeResult<()> {
        for command in commands {
            self.command_sink.submit_command(command)?;
        }
        Ok(())
    }

    fn validate_track_selection_request(
        &self,
        kind: MediaTrackKind,
        selection: &MediaTrackSelection,
    ) -> PlayerRuntimeResult<MediaTrackSelection> {
        match selection.mode {
            MediaTrackSelectionMode::Auto => Ok(MediaTrackSelection::auto()),
            MediaTrackSelectionMode::Disabled => Ok(MediaTrackSelection::disabled()),
            MediaTrackSelectionMode::Track => {
                let Some(track_id) = selection.track_id.as_deref() else {
                    return Err(PlayerRuntimeError::new(
                        PlayerRuntimeErrorCode::InvalidArgument,
                        "track selection mode=Track requires a track id",
                    ));
                };

                let track = self
                    .media_info
                    .track_catalog
                    .tracks
                    .iter()
                    .find(|track| track.id == track_id)
                    .ok_or_else(|| {
                        PlayerRuntimeError::new(
                            PlayerRuntimeErrorCode::InvalidArgument,
                            format!(
                                "track '{track_id}' is not present in the current track catalog"
                            ),
                        )
                    })?;

                if track.kind != kind {
                    return Err(PlayerRuntimeError::new(
                        PlayerRuntimeErrorCode::InvalidArgument,
                        format!("track '{track_id}' is not a {:?} track", kind),
                    ));
                }

                Ok(MediaTrackSelection::track(track_id))
            }
        }
    }

    fn validate_abr_policy_request(
        &self,
        policy: &MediaAbrPolicy,
    ) -> PlayerRuntimeResult<MediaAbrPolicy> {
        match policy.mode {
            MediaAbrMode::Auto => Ok(MediaAbrPolicy::default()),
            MediaAbrMode::Constrained => {
                let has_resolution_limit =
                    policy.max_width.is_some() || policy.max_height.is_some();
                if policy.max_bit_rate.is_none() && !has_resolution_limit {
                    return Err(PlayerRuntimeError::new(
                        PlayerRuntimeErrorCode::InvalidArgument,
                        "iOS constrained ABR requires at least one max_bit_rate or max_width/max_height limit",
                    ));
                }

                if has_resolution_limit
                    && (policy.max_width.is_none() || policy.max_height.is_none())
                {
                    return Err(PlayerRuntimeError::new(
                        PlayerRuntimeErrorCode::InvalidArgument,
                        "iOS constrained ABR resolution limits require both max_width and max_height",
                    ));
                }

                Ok(MediaAbrPolicy {
                    mode: MediaAbrMode::Constrained,
                    track_id: None,
                    max_bit_rate: policy.max_bit_rate,
                    max_width: policy.max_width,
                    max_height: policy.max_height,
                })
            }
            MediaAbrMode::FixedTrack => Err(PlayerRuntimeError::new(
                PlayerRuntimeErrorCode::Unsupported,
                "fixed-track ABR is not implemented for the iOS AVPlayer runtime",
            )),
        }
    }

    fn translate_command(
        &self,
        command: &PlayerRuntimeCommand,
    ) -> PlayerRuntimeResult<(bool, Vec<IosNativePlayerCommand>)> {
        match command {
            PlayerRuntimeCommand::Play => match self.presentation_state {
                PresentationState::Playing => Ok((false, Vec::new())),
                PresentationState::Finished => Ok((
                    true,
                    vec![
                        IosNativePlayerCommand::SeekTo {
                            position: Duration::ZERO,
                        },
                        IosNativePlayerCommand::Play,
                    ],
                )),
                PresentationState::Ready | PresentationState::Paused => {
                    Ok((true, vec![IosNativePlayerCommand::Play]))
                }
            },
            PlayerRuntimeCommand::Pause => match self.presentation_state {
                PresentationState::Playing => Ok((true, vec![IosNativePlayerCommand::Pause])),
                PresentationState::Paused => Ok((false, Vec::new())),
                PresentationState::Ready | PresentationState::Finished => {
                    Err(PlayerRuntimeError::new(
                        PlayerRuntimeErrorCode::InvalidState,
                        "pause is only valid after playback has started",
                    ))
                }
            },
            PlayerRuntimeCommand::TogglePause => match self.presentation_state {
                PresentationState::Playing => Ok((true, vec![IosNativePlayerCommand::Pause])),
                PresentationState::Ready | PresentationState::Paused => {
                    Ok((true, vec![IosNativePlayerCommand::Play]))
                }
                PresentationState::Finished => Ok((
                    true,
                    vec![
                        IosNativePlayerCommand::SeekTo {
                            position: Duration::ZERO,
                        },
                        IosNativePlayerCommand::Play,
                    ],
                )),
            },
            PlayerRuntimeCommand::SeekTo { position } => Ok((
                true,
                vec![IosNativePlayerCommand::SeekTo {
                    position: *position,
                }],
            )),
            PlayerRuntimeCommand::SetPlaybackRate { rate } => {
                let rate = self.validate_playback_rate(*rate)?;
                if (self.playback_rate - rate).abs() <= f32::EPSILON {
                    return Ok((false, Vec::new()));
                }
                Ok((true, vec![IosNativePlayerCommand::SetPlaybackRate { rate }]))
            }
            PlayerRuntimeCommand::SetVideoTrackSelection { .. } => Err(PlayerRuntimeError::new(
                PlayerRuntimeErrorCode::Unsupported,
                "fixed video-track selection is not implemented for the iOS AVPlayer runtime",
            )),
            PlayerRuntimeCommand::SetAudioTrackSelection { selection } => {
                let selection =
                    self.validate_track_selection_request(MediaTrackKind::Audio, selection)?;
                if self.media_info.track_selection.audio == selection {
                    return Ok((false, Vec::new()));
                }
                Ok((
                    true,
                    vec![IosNativePlayerCommand::SetAudioTrackSelection { selection }],
                ))
            }
            PlayerRuntimeCommand::SetSubtitleTrackSelection { selection } => {
                let selection =
                    self.validate_track_selection_request(MediaTrackKind::Subtitle, selection)?;
                if self.media_info.track_selection.subtitle == selection {
                    return Ok((false, Vec::new()));
                }
                Ok((
                    true,
                    vec![IosNativePlayerCommand::SetSubtitleTrackSelection { selection }],
                ))
            }
            PlayerRuntimeCommand::SetAbrPolicy { policy } => {
                let policy = self.validate_abr_policy_request(policy)?;
                if self.media_info.track_selection.abr_policy == policy {
                    return Ok((false, Vec::new()));
                }
                Ok((true, vec![IosNativePlayerCommand::SetAbrPolicy { policy }]))
            }
            PlayerRuntimeCommand::Stop => {
                if self.presentation_state == PresentationState::Ready
                    && self.progress.position().is_zero()
                {
                    return Ok((false, Vec::new()));
                }
                Ok((true, vec![IosNativePlayerCommand::Stop]))
            }
        }
    }
}

impl IosNativePlayerBridge for IosAvPlayerBridge {
    fn probe_source(
        &self,
        source: &MediaSource,
        options: &PlayerRuntimeOptions,
    ) -> PlayerRuntimeResult<IosNativePlayerProbe> {
        let context = resolve_bridge_context(&self.context, options)?;
        self.bindings.probe_source(&context, source, options)
    }

    fn initialize_session(
        &self,
        source: MediaSource,
        options: PlayerRuntimeOptions,
        media_info: &PlayerMediaInfo,
        startup: &PlayerRuntimeStartup,
    ) -> PlayerRuntimeResult<IosNativePlayerSessionBootstrap> {
        let context = resolve_bridge_context(&self.context, &options)?;
        let capabilities = ios_native_capabilities();
        let controller = IosManagedNativeSessionController::default();
        let command_sink = self.bindings.create_command_sink(
            context,
            &source,
            &options,
            media_info,
            startup,
            controller.clone(),
        )?;
        let session = IosManagedNativeSession::with_existing_controller(
            source.uri(),
            media_info.clone(),
            capabilities,
            context.video_surface.map(runtime_surface_from_ios_surface),
            command_sink,
            controller,
        );
        let mut session = session;
        session.emit_initial_runtime_events(startup.clone());

        Ok(IosNativePlayerSessionBootstrap {
            runtime: Box::new(session),
            initial_frame: None,
        })
    }
}

impl<C: IosNativeCommandSink> IosNativePlayerSession for IosManagedNativeSession<C> {
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
        let previous_media_info = self.media_info.clone();
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
                PlayerRuntimeCommand::SetVideoTrackSelection { .. } => {}
                PlayerRuntimeCommand::SetAudioTrackSelection { selection } => {
                    self.media_info.track_selection.audio = selection;
                }
                PlayerRuntimeCommand::SetSubtitleTrackSelection { selection } => {
                    self.media_info.track_selection.subtitle = selection;
                }
                PlayerRuntimeCommand::SetAbrPolicy { policy } => {
                    self.media_info.track_selection.abr_policy = policy;
                }
                PlayerRuntimeCommand::Stop => {
                    self.presentation_state = PresentationState::Ready;
                    self.is_buffering = false;
                    self.progress = PlaybackProgress::new(Duration::ZERO, self.progress.duration());
                    self.emit_state_change_if_needed(previous_state);
                }
            }
            if self.media_info.track_selection != previous_media_info.track_selection {
                self.events
                    .push_back(PlayerRuntimeEvent::MetadataReady(self.media_info.clone()));
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
                    validate_ios_video_surface(surface, best_video)?;
                }
                self.command_sink.attach_video_surface(surface)?;
                self.video_surface = Some(surface);
            }
            None => {
                self.command_sink.detach_video_surface()?;
                self.video_surface = None;
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
    base_context: &IosAvPlayerBridgeContext,
    options: &PlayerRuntimeOptions,
) -> PlayerRuntimeResult<IosAvPlayerBridgeContext> {
    let resolved_surface = match options.video_surface {
        Some(surface) => Some(ios_surface_from_runtime_surface(surface)?),
        None => base_context.video_surface,
    };

    Ok(IosAvPlayerBridgeContext {
        av_player: base_context.av_player,
        video_surface: resolved_surface,
    })
}

fn ios_surface_from_runtime_surface(
    surface: PlayerVideoSurfaceTarget,
) -> PlayerRuntimeResult<IosVideoSurfaceTarget> {
    let kind = match surface.kind {
        PlayerVideoSurfaceKind::UiView => IosVideoSurfaceKind::UiView,
        PlayerVideoSurfaceKind::PlayerLayer => IosVideoSurfaceKind::PlayerLayer,
        PlayerVideoSurfaceKind::MetalLayer => IosVideoSurfaceKind::MetalLayer,
        PlayerVideoSurfaceKind::NsView => {
            return Err(PlayerRuntimeError::new(
                PlayerRuntimeErrorCode::InvalidArgument,
                "ios native backend does not support NsView as a video surface target",
            ));
        }
    };

    Ok(IosVideoSurfaceTarget {
        kind,
        handle: IosOpaqueHandle(surface.handle),
    })
}

fn runtime_surface_from_ios_surface(surface: IosVideoSurfaceTarget) -> PlayerVideoSurfaceTarget {
    PlayerVideoSurfaceTarget {
        kind: match surface.kind {
            IosVideoSurfaceKind::UiView => PlayerVideoSurfaceKind::UiView,
            IosVideoSurfaceKind::PlayerLayer => PlayerVideoSurfaceKind::PlayerLayer,
            IosVideoSurfaceKind::MetalLayer => PlayerVideoSurfaceKind::MetalLayer,
        },
        handle: surface.handle.0,
    }
}

fn validate_ios_video_surface(
    surface: PlayerVideoSurfaceTarget,
    best_video: &player_runtime::PlayerVideoInfo,
) -> PlayerRuntimeResult<()> {
    match surface.kind {
        PlayerVideoSurfaceKind::UiView
        | PlayerVideoSurfaceKind::PlayerLayer
        | PlayerVideoSurfaceKind::MetalLayer => Ok(()),
        PlayerVideoSurfaceKind::NsView => Err(PlayerRuntimeError::new(
            PlayerRuntimeErrorCode::InvalidArgument,
            format!(
                "ios native backend does not support NsView as a video surface for {} playback",
                best_video.codec
            ),
        )),
    }
}

fn host_video_surface_target() -> PlayerVideoSurfaceTarget {
    PlayerVideoSurfaceTarget {
        kind: PlayerVideoSurfaceKind::PlayerLayer,
        handle: 0,
    }
}

fn ios_native_capabilities() -> PlayerRuntimeAdapterCapabilities {
    PlayerRuntimeAdapterCapabilities {
        adapter_id: IOS_NATIVE_PLAYER_RUNTIME_ADAPTER_ID,
        backend_family: PlayerRuntimeAdapterBackendFamily::NativeIos,
        supports_audio_output: true,
        supports_frame_output: false,
        supports_external_video_surface: true,
        supports_seek: true,
        supports_stop: true,
        supports_playback_rate: true,
        playback_rate_min: Some(0.5),
        playback_rate_max: Some(3.0),
        natural_playback_rate_max: Some(player_runtime::NATURAL_PLAYBACK_RATE_MAX),
        supports_hardware_decode: true,
        supports_streaming: true,
        supports_hdr: true,
    }
}

fn ios_native_unavailable_message() -> &'static str {
    if cfg!(target_os = "ios") {
        "ios native adapter is available, but no AVPlayer bridge has been installed"
    } else {
        "ios native adapter can be probed on non-iOS hosts as a skeleton, but initialization is only planned for Apple mobile targets"
    }
}

fn duration_to_millis(duration: Duration) -> u64 {
    duration.as_millis().min(u128::from(u64::MAX)) as u64
}

fn host_timeline_kind(kind: PlayerTimelineKind) -> IosHostTimelineKind {
    match kind {
        PlayerTimelineKind::Vod => IosHostTimelineKind::Vod,
        PlayerTimelineKind::Live => IosHostTimelineKind::Live,
        PlayerTimelineKind::LiveDvr => IosHostTimelineKind::LiveDvr,
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
    use std::time::{Duration, Instant};

    use super::{
        IOS_NATIVE_PLAYER_RUNTIME_ADAPTER_ID, IosAvPlayerBridge, IosAvPlayerBridgeBindings,
        IosAvPlayerBridgeContext, IosAvPlayerSnapshot, IosAvPlayerStateTracker,
        IosHostBridgeSession, IosHostCommand, IosHostEvent, IosHostSnapshot, IosHostTimelineKind,
        IosManagedNativeSession, IosNativeCommandSink, IosNativePlayerBridge,
        IosNativePlayerCommand, IosNativePlayerProbe, IosNativePlayerRuntimeAdapterFactory,
        IosNativePlayerSession, IosNativePlayerSessionBootstrap, IosOpaqueHandle,
        IosPlayerItemStatus, IosTimeControlStatus, IosVideoSurfaceKind, host_video_surface_target,
        resolve_bridge_context,
    };
    use player_core::MediaSource;
    use player_runtime::{
        DecodedVideoFrame, MediaAbrMode, MediaAbrPolicy, MediaTrack, MediaTrackCatalog,
        MediaTrackKind, MediaTrackSelection, MediaTrackSelectionSnapshot, PlaybackProgress,
        PlayerMediaInfo, PlayerResilienceMetrics, PlayerRuntimeAdapterBackendFamily,
        PlayerRuntimeAdapterCapabilities, PlayerRuntimeAdapterFactory, PlayerRuntimeCommand,
        PlayerRuntimeCommandResult, PlayerRuntimeErrorCode, PlayerRuntimeEvent,
        PlayerRuntimeOptions, PlayerRuntimeResult, PlayerRuntimeStartup, PlayerSnapshot,
        PlayerTimelineSnapshot, PlayerVideoSurfaceKind, PlayerVideoSurfaceTarget,
        PresentationState,
    };

    #[test]
    fn ios_factory_exposes_native_capabilities() {
        let factory = IosNativePlayerRuntimeAdapterFactory::default();
        let initializer = factory
            .probe_source_with_options(
                MediaSource::new("placeholder.mp4"),
                PlayerRuntimeOptions::default(),
            )
            .expect("ios skeleton probe should succeed");

        let capabilities = initializer.capabilities();
        assert_eq!(
            capabilities.adapter_id,
            IOS_NATIVE_PLAYER_RUNTIME_ADAPTER_ID
        );
        assert!(capabilities.supports_external_video_surface);
        assert!(capabilities.supports_hardware_decode);
    }

    #[test]
    fn ios_factory_is_initialize_unsupported_without_bridge() {
        let factory = IosNativePlayerRuntimeAdapterFactory::default();
        let initializer = factory
            .probe_source_with_options(
                MediaSource::new("placeholder.mp4"),
                PlayerRuntimeOptions::default(),
            )
            .expect("ios skeleton probe should succeed");

        let error = match initializer.initialize() {
            Ok(_) => panic!("ios skeleton initialize should be unsupported"),
            Err(error) => error,
        };
        assert_eq!(error.code(), PlayerRuntimeErrorCode::Unsupported);
    }

    #[test]
    fn ios_factory_can_initialize_with_bridge() {
        let factory = IosNativePlayerRuntimeAdapterFactory::with_bridge(Arc::new(FakeIosBridge));
        let initializer = factory
            .probe_source_with_options(
                MediaSource::new("placeholder.mp4"),
                PlayerRuntimeOptions::default(),
            )
            .expect("ios bridge probe should succeed");

        let bootstrap = initializer
            .initialize()
            .expect("ios bridge initialize should succeed");
        assert!(bootstrap.initial_frame.is_none());
        assert_eq!(
            bootstrap.runtime.capabilities().backend_family,
            PlayerRuntimeAdapterBackendFamily::NativeIos
        );
    }

    #[test]
    fn ios_state_tracker_maps_ready_play_pause_and_end() {
        let mut tracker = IosAvPlayerStateTracker::default();

        let ready = tracker.observe(&IosAvPlayerSnapshot {
            item_status: IosPlayerItemStatus::ReadyToPlay,
            time_control_status: IosTimeControlStatus::Paused,
            playback_rate: 1.0,
            position: Duration::ZERO,
            duration: Some(Duration::from_secs(8)),
            reached_end: false,
            error_message: None,
        });
        assert_eq!(ready.presentation_state, PresentationState::Ready);

        let playing = tracker.observe(&IosAvPlayerSnapshot {
            item_status: IosPlayerItemStatus::ReadyToPlay,
            time_control_status: IosTimeControlStatus::Playing,
            playback_rate: 1.0,
            position: Duration::from_secs(1),
            duration: Some(Duration::from_secs(8)),
            reached_end: false,
            error_message: None,
        });
        assert_eq!(playing.presentation_state, PresentationState::Playing);

        let paused = tracker.observe(&IosAvPlayerSnapshot {
            item_status: IosPlayerItemStatus::ReadyToPlay,
            time_control_status: IosTimeControlStatus::Paused,
            playback_rate: 1.0,
            position: Duration::from_secs(2),
            duration: Some(Duration::from_secs(8)),
            reached_end: false,
            error_message: None,
        });
        assert_eq!(paused.presentation_state, PresentationState::Paused);

        let finished = tracker.observe(&IosAvPlayerSnapshot {
            item_status: IosPlayerItemStatus::ReadyToPlay,
            time_control_status: IosTimeControlStatus::Paused,
            playback_rate: 1.0,
            position: Duration::from_secs(8),
            duration: Some(Duration::from_secs(8)),
            reached_end: true,
            error_message: None,
        });
        assert_eq!(finished.presentation_state, PresentationState::Finished);
        assert!(
            finished
                .emitted_events
                .iter()
                .any(|event| matches!(event, player_runtime::PlayerRuntimeEvent::Ended))
        );
    }

    #[test]
    fn ios_state_tracker_keeps_playing_while_native_player_is_waiting() {
        let mut tracker = IosAvPlayerStateTracker::default();
        tracker.seed(PresentationState::Playing, 1.0);

        let observation = tracker.observe(&IosAvPlayerSnapshot {
            item_status: IosPlayerItemStatus::ReadyToPlay,
            time_control_status: IosTimeControlStatus::WaitingToPlay,
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
    fn ios_state_tracker_emits_backend_error_event() {
        let mut tracker = IosAvPlayerStateTracker::default();

        let observation = tracker.observe(&IosAvPlayerSnapshot {
            item_status: IosPlayerItemStatus::Failed,
            time_control_status: IosTimeControlStatus::Paused,
            playback_rate: 1.0,
            position: Duration::ZERO,
            duration: None,
            reached_end: false,
            error_message: Some("avplayer item failed".to_owned()),
        });

        assert!(observation.emitted_events.iter().any(|event| matches!(
            event,
            player_runtime::PlayerRuntimeEvent::Error(error)
            if error.code() == PlayerRuntimeErrorCode::BackendFailure
        )));
    }

    #[test]
    fn ios_managed_session_replays_from_start_when_finished() {
        let commands = Arc::new(Mutex::new(Vec::new()));
        let sink = RecordingIosCommandSink::new(commands.clone());
        let mut session = IosManagedNativeSession::new("placeholder.mp4", test_media_info(), sink);

        session.apply_snapshot(&IosAvPlayerSnapshot {
            item_status: IosPlayerItemStatus::ReadyToPlay,
            time_control_status: IosTimeControlStatus::Paused,
            playback_rate: 1.0,
            position: Duration::from_secs(6),
            duration: Some(Duration::from_secs(6)),
            reached_end: true,
            error_message: None,
        });

        let result = session
            .dispatch(PlayerRuntimeCommand::Play)
            .expect("play from finished should be bridged");

        assert!(result.applied);
        assert_eq!(result.snapshot.state, PresentationState::Playing);
        assert_eq!(
            *commands.lock().expect("commands lock"),
            vec![
                IosNativePlayerCommand::SeekTo {
                    position: Duration::ZERO,
                },
                IosNativePlayerCommand::Play,
            ]
        );
    }

    #[test]
    fn ios_managed_session_validates_pause_and_playback_rate() {
        let commands = Arc::new(Mutex::new(Vec::new()));
        let sink = RecordingIosCommandSink::new(commands.clone());
        let mut session = IosManagedNativeSession::new("placeholder.mp4", test_media_info(), sink);

        let pause_error = session
            .dispatch(PlayerRuntimeCommand::Pause)
            .expect_err("pause before play should be invalid");
        assert_eq!(pause_error.code(), PlayerRuntimeErrorCode::InvalidState);

        let rate_error = session
            .dispatch(PlayerRuntimeCommand::SetPlaybackRate { rate: 4.0 })
            .expect_err("out-of-range playback rate should fail");
        assert_eq!(rate_error.code(), PlayerRuntimeErrorCode::InvalidArgument);
        assert!(commands.lock().expect("commands lock").is_empty());
    }

    #[test]
    fn ios_managed_session_dispatches_audio_track_selection() {
        let commands = Arc::new(Mutex::new(Vec::new()));
        let sink = RecordingIosCommandSink::new(commands.clone());
        let mut session = IosManagedNativeSession::new("placeholder.mp4", test_media_info(), sink);

        let result = session
            .dispatch(PlayerRuntimeCommand::SetAudioTrackSelection {
                selection: MediaTrackSelection::track("audio-en"),
            })
            .expect("audio track selection should dispatch");

        assert!(result.applied);
        assert_eq!(
            *commands.lock().expect("commands lock"),
            vec![IosNativePlayerCommand::SetAudioTrackSelection {
                selection: MediaTrackSelection::track("audio-en"),
            }]
        );
        assert_eq!(
            result.snapshot.media_info.track_selection.audio,
            MediaTrackSelection::track("audio-en")
        );
        let events = session.drain_events();
        assert!(events.iter().any(|event| matches!(
            event,
            PlayerRuntimeEvent::MetadataReady(media_info)
                if media_info.track_selection.audio == MediaTrackSelection::track("audio-en")
        )));
    }

    #[test]
    fn ios_managed_session_dispatches_constrained_abr_by_bitrate() {
        let commands = Arc::new(Mutex::new(Vec::new()));
        let sink = RecordingIosCommandSink::new(commands.clone());
        let mut session = IosManagedNativeSession::new("placeholder.mp4", test_media_info(), sink);
        let policy = MediaAbrPolicy {
            mode: MediaAbrMode::Constrained,
            track_id: None,
            max_bit_rate: Some(1_500_000),
            max_width: None,
            max_height: None,
        };

        let result = session
            .dispatch(PlayerRuntimeCommand::SetAbrPolicy {
                policy: policy.clone(),
            })
            .expect("constrained abr should dispatch");

        assert!(result.applied);
        assert_eq!(
            *commands.lock().expect("commands lock"),
            vec![IosNativePlayerCommand::SetAbrPolicy {
                policy: policy.clone(),
            }]
        );
        assert_eq!(
            result.snapshot.media_info.track_selection.abr_policy,
            policy
        );
    }

    #[test]
    fn ios_managed_session_dispatches_constrained_abr_by_resolution() {
        let commands = Arc::new(Mutex::new(Vec::new()));
        let sink = RecordingIosCommandSink::new(commands.clone());
        let mut session = IosManagedNativeSession::new("placeholder.mp4", test_media_info(), sink);
        let policy = MediaAbrPolicy {
            mode: MediaAbrMode::Constrained,
            track_id: None,
            max_bit_rate: None,
            max_width: Some(1280),
            max_height: Some(720),
        };

        let result = session
            .dispatch(PlayerRuntimeCommand::SetAbrPolicy {
                policy: policy.clone(),
            })
            .expect("resolution-constrained abr should dispatch");

        assert!(result.applied);
        assert_eq!(
            *commands.lock().expect("commands lock"),
            vec![IosNativePlayerCommand::SetAbrPolicy {
                policy: policy.clone(),
            }]
        );
        assert_eq!(
            result.snapshot.media_info.track_selection.abr_policy,
            policy
        );
    }

    #[test]
    fn ios_managed_session_rejects_partial_resolution_abr_limit() {
        let commands = Arc::new(Mutex::new(Vec::new()));
        let sink = RecordingIosCommandSink::new(commands.clone());
        let mut session = IosManagedNativeSession::new("placeholder.mp4", test_media_info(), sink);

        let error = session
            .dispatch(PlayerRuntimeCommand::SetAbrPolicy {
                policy: MediaAbrPolicy {
                    mode: MediaAbrMode::Constrained,
                    track_id: None,
                    max_bit_rate: None,
                    max_width: Some(1280),
                    max_height: None,
                },
            })
            .expect_err("partial resolution abr limit should be rejected");

        assert_eq!(error.code(), PlayerRuntimeErrorCode::InvalidArgument);
        assert!(commands.lock().expect("commands lock").is_empty());
    }

    #[test]
    fn ios_managed_session_rejects_video_track_selection() {
        let commands = Arc::new(Mutex::new(Vec::new()));
        let sink = RecordingIosCommandSink::new(commands.clone());
        let mut session = IosManagedNativeSession::new("placeholder.mp4", test_media_info(), sink);

        let error = session
            .dispatch(PlayerRuntimeCommand::SetVideoTrackSelection {
                selection: MediaTrackSelection::track("video-main"),
            })
            .expect_err("video track selection should be unsupported");

        assert_eq!(error.code(), PlayerRuntimeErrorCode::Unsupported);
        assert!(commands.lock().expect("commands lock").is_empty());
    }

    #[test]
    fn ios_managed_session_updates_from_native_snapshot() {
        let commands = Arc::new(Mutex::new(Vec::new()));
        let sink = RecordingIosCommandSink::new(commands);
        let mut session = IosManagedNativeSession::new("placeholder.mp4", test_media_info(), sink);

        session.apply_snapshot(&IosAvPlayerSnapshot {
            item_status: IosPlayerItemStatus::ReadyToPlay,
            time_control_status: IosTimeControlStatus::Playing,
            playback_rate: 1.25,
            position: Duration::from_millis(900),
            duration: Some(Duration::from_secs(5)),
            reached_end: false,
            error_message: None,
        });

        assert_eq!(session.presentation_state(), PresentationState::Playing);
        assert!((session.playback_rate() - 1.25).abs() < f32::EPSILON);
        assert_eq!(session.progress().position(), Duration::from_millis(900));
        let events = session.drain_events();
        assert!(events.iter().any(|event| matches!(
            event,
            player_runtime::PlayerRuntimeEvent::PlaybackRateChanged { rate }
            if (*rate - 1.25).abs() < f32::EPSILON
        )));
    }

    #[test]
    fn ios_managed_session_controller_delivers_async_updates() {
        let commands = Arc::new(Mutex::new(Vec::new()));
        let sink = RecordingIosCommandSink::new(commands);
        let (mut session, controller) =
            IosManagedNativeSession::with_controller("placeholder.mp4", test_media_info(), sink);

        controller.apply_snapshot(IosAvPlayerSnapshot {
            item_status: IosPlayerItemStatus::ReadyToPlay,
            time_control_status: IosTimeControlStatus::Playing,
            playback_rate: 1.5,
            position: Duration::from_secs(2),
            duration: Some(Duration::from_secs(12)),
            reached_end: false,
            error_message: None,
        });
        controller.report_seek_completed(Duration::from_secs(3));
        controller.report_retry_scheduled(2, Duration::from_millis(1_500));
        controller.report_error(
            PlayerRuntimeErrorCode::BackendFailure,
            "avplayer callback failed",
        );

        let events = session.drain_events();
        assert_eq!(session.presentation_state(), PresentationState::Playing);
        assert!((session.playback_rate() - 1.5).abs() < f32::EPSILON);
        assert_eq!(session.progress().position(), Duration::from_secs(3));
        assert!(events.iter().any(|event| matches!(
            event,
            PlayerRuntimeEvent::SeekCompleted { position } if *position == Duration::from_secs(3)
        )));
        assert!(events.iter().any(|event| matches!(
            event,
            PlayerRuntimeEvent::RetryScheduled { attempt: 2, delay }
            if *delay == Duration::from_millis(1_500)
        )));
        assert!(events.iter().any(|event| matches!(
            event,
            PlayerRuntimeEvent::Error(error)
            if error.code() == PlayerRuntimeErrorCode::BackendFailure
        )));
        assert_eq!(session.snapshot().resilience_metrics.retry_count, 2);
    }

    #[test]
    fn ios_managed_session_snapshot_pumps_pending_media_info_updates() {
        let commands = Arc::new(Mutex::new(Vec::new()));
        let sink = RecordingIosCommandSink::new(commands);
        let (mut session, controller) =
            IosManagedNativeSession::with_controller("placeholder.mp4", test_media_info(), sink);
        let track_selection = MediaTrackSelectionSnapshot {
            video: MediaTrackSelection::auto(),
            audio: MediaTrackSelection::track("audio-en"),
            subtitle: MediaTrackSelection::track("subtitle-en"),
            abr_policy: MediaAbrPolicy {
                mode: MediaAbrMode::Constrained,
                track_id: None,
                max_bit_rate: Some(900_000),
                max_width: None,
                max_height: None,
            },
        };

        controller.report_media_info(test_track_catalog(), track_selection.clone());

        let snapshot = session.snapshot();
        assert_eq!(snapshot.media_info.track_catalog, test_track_catalog());
        assert_eq!(snapshot.media_info.track_selection, track_selection);
    }

    #[test]
    fn ios_managed_session_emits_initial_and_interruption_events() {
        let mut session = IosManagedNativeSession::new(
            "placeholder.mp4",
            test_media_info(),
            RecordingIosCommandSink::new(Arc::new(Mutex::new(Vec::new()))),
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

        session
            .dispatch(PlayerRuntimeCommand::Play)
            .expect("play should seed playback intent");
        let _ = session.drain_events();

        session.apply_snapshot(&IosAvPlayerSnapshot {
            item_status: IosPlayerItemStatus::ReadyToPlay,
            time_control_status: IosTimeControlStatus::WaitingToPlay,
            playback_rate: 1.0,
            position: Duration::from_millis(250),
            duration: Some(Duration::from_secs(12)),
            reached_end: false,
            error_message: None,
        });
        let buffering_events = session.drain_events();
        assert!(buffering_events.iter().any(|event| matches!(
            event,
            PlayerRuntimeEvent::BufferingChanged { buffering: true }
        )));

        session.controller().report_interruption_changed(true);
        let interrupted_events = session.drain_events();
        assert!(interrupted_events.iter().any(|event| matches!(
            event,
            PlayerRuntimeEvent::InterruptionChanged { interrupted: true }
        )));
        assert!(interrupted_events.iter().any(|event| matches!(
            event,
            PlayerRuntimeEvent::BufferingChanged { buffering: false }
        )));
        assert!(session.snapshot().is_interrupted);
        assert!(!session.snapshot().is_buffering);
    }

    #[test]
    fn ios_managed_session_replace_video_surface_emits_event() {
        let commands = Arc::new(Mutex::new(Vec::new()));
        let surfaces = Arc::new(Mutex::new(Vec::new()));
        let mut session = IosManagedNativeSession::new(
            "placeholder.mp4",
            test_media_info(),
            SurfaceRecordingIosCommandSink::new(commands, surfaces.clone()),
        );

        session
            .replace_video_surface(Some(host_video_surface_target()))
            .expect("surface attachment should succeed");
        assert!(session.snapshot().has_video_surface);
        assert_eq!(surfaces.lock().expect("surface lock").len(), 1);

        let attach_events = session.drain_events();
        assert!(attach_events.iter().any(|event| matches!(
            event,
            PlayerRuntimeEvent::VideoSurfaceChanged { attached: true }
        )));

        session
            .replace_video_surface(None)
            .expect("surface detach should succeed");
        assert!(!session.snapshot().has_video_surface);
        let detach_events = session.drain_events();
        assert!(detach_events.iter().any(|event| matches!(
            event,
            PlayerRuntimeEvent::VideoSurfaceChanged { attached: false }
        )));
    }

    #[test]
    fn ios_avplayer_bridge_bindings_can_initialize_managed_session() {
        let bridge = IosAvPlayerBridge::new(
            IosAvPlayerBridgeContext {
                av_player: IosOpaqueHandle(2),
                video_surface: None,
            },
            Arc::new(FakeIosAvBindings::default()),
        );
        let factory = IosNativePlayerRuntimeAdapterFactory::with_bridge(Arc::new(bridge));
        let initializer = factory
            .probe_source_with_options(
                MediaSource::new("placeholder.mp4"),
                PlayerRuntimeOptions::default(),
            )
            .expect("ios av bridge probe should succeed");

        let bootstrap = initializer
            .initialize()
            .expect("ios av bridge initialize should succeed");
        assert!(bootstrap.initial_frame.is_none());
        assert_eq!(
            bootstrap.runtime.capabilities().backend_family,
            PlayerRuntimeAdapterBackendFamily::NativeIos
        );
    }

    #[test]
    fn ios_resolve_bridge_context_prefers_runtime_surface_option() {
        let context = resolve_bridge_context(
            &IosAvPlayerBridgeContext {
                av_player: IosOpaqueHandle(7),
                video_surface: None,
            },
            &PlayerRuntimeOptions::default().with_video_surface(PlayerVideoSurfaceTarget {
                kind: PlayerVideoSurfaceKind::PlayerLayer,
                handle: 42,
            }),
        )
        .expect("runtime surface should resolve");

        let resolved_surface = context.video_surface.expect("resolved surface");
        assert_eq!(resolved_surface.kind, IosVideoSurfaceKind::PlayerLayer);
        assert_eq!(resolved_surface.handle, IosOpaqueHandle(42));
    }

    #[test]
    fn ios_host_snapshot_conversion_preserves_timeline_shape() {
        let snapshot = PlayerSnapshot {
            source_uri: "placeholder.mp4".to_owned(),
            state: PresentationState::Playing,
            has_video_surface: true,
            is_interrupted: true,
            is_buffering: true,
            playback_rate: 1.5,
            progress: PlaybackProgress::new(Duration::from_secs(5), Some(Duration::from_secs(20))),
            timeline: PlayerTimelineSnapshot::vod(
                PlaybackProgress::new(Duration::from_secs(5), Some(Duration::from_secs(20))),
                true,
            ),
            media_info: test_media_info(),
            resilience_metrics: PlayerResilienceMetrics::default(),
        };

        let host = IosHostSnapshot::from_player_snapshot(&snapshot);
        assert_eq!(host.playback_state, PresentationState::Playing);
        assert!(host.has_video_surface);
        assert!(host.is_interrupted);
        assert!(host.is_buffering);
        assert_eq!(host.position_ms, 5_000);
        assert_eq!(host.duration_ms, Some(20_000));
        assert_eq!(host.seekable_range.expect("seekable range").end_ms, 20_000);
    }

    #[test]
    fn ios_host_snapshot_conversion_uses_effective_live_edge_for_live_dvr() {
        let snapshot = PlayerSnapshot {
            source_uri: "https://example.com/live.m3u8".to_owned(),
            state: PresentationState::Playing,
            has_video_surface: true,
            is_interrupted: false,
            is_buffering: false,
            playback_rate: 1.0,
            progress: PlaybackProgress::new(Duration::from_secs(84), None),
            timeline: PlayerTimelineSnapshot::live_dvr(
                PlaybackProgress::new(Duration::from_secs(84), None),
                player_runtime::PlayerSeekableRange {
                    start: Duration::ZERO,
                    end: Duration::from_secs(120),
                },
                None,
            ),
            media_info: test_media_info(),
            resilience_metrics: PlayerResilienceMetrics::default(),
        };

        let host = IosHostSnapshot::from_player_snapshot(&snapshot);
        assert_eq!(host.timeline_kind, IosHostTimelineKind::LiveDvr);
        assert_eq!(host.live_edge_ms, Some(120_000));
        assert_eq!(host.position_ms, 84_000);
    }

    #[test]
    fn ios_host_event_conversion_maps_runtime_events() {
        let rate = IosHostEvent::from_runtime_event(&PlayerRuntimeEvent::PlaybackRateChanged {
            rate: 1.25,
        });
        assert!(matches!(
            rate,
            Some(IosHostEvent::PlaybackRateChanged { rate })
            if (rate - 1.25).abs() < f32::EPSILON
        ));

        let interruption =
            IosHostEvent::from_runtime_event(&PlayerRuntimeEvent::InterruptionChanged {
                interrupted: true,
            });
        assert!(matches!(
            interruption,
            Some(IosHostEvent::InterruptionChanged { interrupted: true })
        ));

        let retry = IosHostEvent::from_runtime_event(&PlayerRuntimeEvent::RetryScheduled {
            attempt: 3,
            delay: Duration::from_secs(2),
        });
        assert!(matches!(
            retry,
            Some(IosHostEvent::RetryScheduled {
                attempt: 3,
                delay_ms: 2_000,
            })
        ));
    }

    #[test]
    fn ios_host_bridge_session_drains_native_commands() {
        let mut session = IosHostBridgeSession::new("placeholder.mp4");
        session
            .dispatch_command(PlayerRuntimeCommand::Play)
            .expect("play should dispatch");
        session
            .dispatch_command(PlayerRuntimeCommand::SetPlaybackRate { rate: 1.5 })
            .expect("rate should dispatch");

        let commands = session.drain_native_commands();
        assert_eq!(
            commands,
            vec![
                IosHostCommand::Play,
                IosHostCommand::SetPlaybackRate { rate: 1.5 },
            ]
        );
    }

    #[test]
    fn ios_host_bridge_session_reports_surface_and_interruption_events() {
        let mut session = IosHostBridgeSession::new("placeholder.mp4");
        session.set_surface_attached(true);
        session.report_interruption_changed(true);
        session.report_seek_completed(Duration::from_millis(900));

        let events = session.drain_events();
        assert!(
            events
                .iter()
                .any(|event| matches!(event, IosHostEvent::VideoSurfaceChanged { attached: true }))
        );
        assert!(events.iter().any(|event| matches!(
            event,
            IosHostEvent::InterruptionChanged { interrupted: true }
        )));
        assert!(
            events
                .iter()
                .any(|event| matches!(event, IosHostEvent::SeekCompleted { position_ms: 900 }))
        );
        assert!(session.snapshot().has_video_surface);
    }

    #[test]
    fn ios_host_bridge_session_uses_media_info_duration_for_hls_vod_snapshot() {
        let mut session = IosHostBridgeSession::new("https://example.com/master.m3u8");
        session.session.media_info.duration = Some(Duration::from_secs(24));

        let snapshot = session.snapshot();
        assert_eq!(snapshot.timeline_kind, IosHostTimelineKind::Vod);
        assert!(snapshot.is_seekable);
        assert_eq!(snapshot.duration_ms, Some(24_000));
        assert_eq!(
            snapshot.seekable_range.expect("seekable range").end_ms,
            24_000
        );
    }

    #[test]
    fn ios_host_bridge_session_promotes_unknown_hls_duration_to_live_snapshot() {
        let mut session = IosHostBridgeSession::new("https://example.com/master.m3u8");

        let snapshot = session.snapshot();
        assert_eq!(snapshot.timeline_kind, IosHostTimelineKind::Live);
        assert!(!snapshot.is_seekable);
        assert!(snapshot.seekable_range.is_none());
        assert_eq!(snapshot.duration_ms, None);
        assert_eq!(snapshot.live_edge_ms, None);
    }

    struct FakeIosBridge;

    #[derive(Default)]
    struct FakeIosAvBindings {
        commands: Arc<Mutex<Vec<IosNativePlayerCommand>>>,
    }

    struct RecordingIosCommandSink {
        commands: Arc<Mutex<Vec<IosNativePlayerCommand>>>,
    }

    struct SurfaceRecordingIosCommandSink {
        commands: Arc<Mutex<Vec<IosNativePlayerCommand>>>,
        surfaces: Arc<Mutex<Vec<Option<PlayerVideoSurfaceTarget>>>>,
    }

    impl RecordingIosCommandSink {
        fn new(commands: Arc<Mutex<Vec<IosNativePlayerCommand>>>) -> Self {
            Self { commands }
        }
    }

    impl SurfaceRecordingIosCommandSink {
        fn new(
            commands: Arc<Mutex<Vec<IosNativePlayerCommand>>>,
            surfaces: Arc<Mutex<Vec<Option<PlayerVideoSurfaceTarget>>>>,
        ) -> Self {
            Self { commands, surfaces }
        }
    }

    impl IosNativeCommandSink for RecordingIosCommandSink {
        fn submit_command(&mut self, command: IosNativePlayerCommand) -> PlayerRuntimeResult<()> {
            self.commands.lock().expect("commands lock").push(command);
            Ok(())
        }
    }

    impl IosNativeCommandSink for SurfaceRecordingIosCommandSink {
        fn submit_command(&mut self, command: IosNativePlayerCommand) -> PlayerRuntimeResult<()> {
            self.commands.lock().expect("commands lock").push(command);
            Ok(())
        }

        fn attach_video_surface(
            &mut self,
            video_surface: PlayerVideoSurfaceTarget,
        ) -> PlayerRuntimeResult<()> {
            self.surfaces
                .lock()
                .expect("surface lock")
                .push(Some(video_surface));
            Ok(())
        }

        fn detach_video_surface(&mut self) -> PlayerRuntimeResult<()> {
            self.surfaces.lock().expect("surface lock").push(None);
            Ok(())
        }
    }

    impl IosAvPlayerBridgeBindings for FakeIosAvBindings {
        fn probe_source(
            &self,
            _context: &IosAvPlayerBridgeContext,
            source: &MediaSource,
            _options: &PlayerRuntimeOptions,
        ) -> PlayerRuntimeResult<IosNativePlayerProbe> {
            Ok(IosNativePlayerProbe {
                media_info: PlayerMediaInfo {
                    source_uri: source.uri().to_owned(),
                    source_kind: source.kind(),
                    source_protocol: source.protocol(),
                    duration: Some(Duration::from_secs(1)),
                    bit_rate: None,
                    audio_streams: 1,
                    video_streams: 1,
                    best_video: None,
                    best_audio: None,
                    track_catalog: test_track_catalog(),
                    track_selection: test_track_selection(),
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
            _context: IosAvPlayerBridgeContext,
            _source: &MediaSource,
            _options: &PlayerRuntimeOptions,
            _media_info: &PlayerMediaInfo,
            _startup: &PlayerRuntimeStartup,
            controller: super::IosManagedNativeSessionController,
        ) -> PlayerRuntimeResult<Box<dyn IosNativeCommandSink>> {
            controller.apply_snapshot(IosAvPlayerSnapshot {
                item_status: IosPlayerItemStatus::ReadyToPlay,
                time_control_status: IosTimeControlStatus::Paused,
                playback_rate: 1.0,
                position: Duration::ZERO,
                duration: Some(Duration::from_secs(1)),
                reached_end: false,
                error_message: None,
            });
            Ok(Box::new(RecordingIosCommandSink::new(
                self.commands.clone(),
            )))
        }
    }

    fn test_media_info() -> PlayerMediaInfo {
        PlayerMediaInfo {
            source_uri: "placeholder.mp4".to_owned(),
            source_kind: player_runtime::MediaSourceKind::Local,
            source_protocol: player_runtime::MediaSourceProtocol::File,
            duration: Some(Duration::from_secs(12)),
            bit_rate: None,
            audio_streams: 1,
            video_streams: 1,
            best_video: None,
            best_audio: None,
            track_catalog: test_track_catalog(),
            track_selection: test_track_selection(),
        }
    }

    impl IosNativePlayerBridge for FakeIosBridge {
        fn probe_source(
            &self,
            source: &MediaSource,
            _options: &PlayerRuntimeOptions,
        ) -> PlayerRuntimeResult<IosNativePlayerProbe> {
            Ok(IosNativePlayerProbe {
                media_info: PlayerMediaInfo {
                    source_uri: source.uri().to_owned(),
                    source_kind: source.kind(),
                    source_protocol: source.protocol(),
                    duration: Some(Duration::from_secs(1)),
                    bit_rate: None,
                    audio_streams: 1,
                    video_streams: 1,
                    best_video: None,
                    best_audio: None,
                    track_catalog: test_track_catalog(),
                    track_selection: test_track_selection(),
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
        ) -> PlayerRuntimeResult<IosNativePlayerSessionBootstrap> {
            Ok(IosNativePlayerSessionBootstrap {
                runtime: Box::new(FakeIosSession {
                    source_uri: source.uri().to_owned(),
                    media_info: media_info.clone(),
                }),
                initial_frame: None,
            })
        }
    }

    fn test_track_catalog() -> MediaTrackCatalog {
        MediaTrackCatalog {
            tracks: vec![
                MediaTrack {
                    id: "video-main".to_owned(),
                    kind: MediaTrackKind::Video,
                    label: Some("Main Video".to_owned()),
                    language: None,
                    codec: Some("h264".to_owned()),
                    bit_rate: Some(2_000_000),
                    width: Some(1280),
                    height: Some(720),
                    frame_rate: Some(30.0),
                    channels: None,
                    sample_rate: None,
                    is_default: true,
                    is_forced: false,
                },
                MediaTrack {
                    id: "audio-en".to_owned(),
                    kind: MediaTrackKind::Audio,
                    label: Some("English".to_owned()),
                    language: Some("en".to_owned()),
                    codec: Some("aac".to_owned()),
                    bit_rate: Some(128_000),
                    width: None,
                    height: None,
                    frame_rate: None,
                    channels: Some(2),
                    sample_rate: Some(48_000),
                    is_default: true,
                    is_forced: false,
                },
                MediaTrack {
                    id: "subtitle-en".to_owned(),
                    kind: MediaTrackKind::Subtitle,
                    label: Some("English CC".to_owned()),
                    language: Some("en".to_owned()),
                    codec: None,
                    bit_rate: None,
                    width: None,
                    height: None,
                    frame_rate: None,
                    channels: None,
                    sample_rate: None,
                    is_default: false,
                    is_forced: false,
                },
            ],
            adaptive_video: true,
            adaptive_audio: true,
        }
    }

    fn test_track_selection() -> MediaTrackSelectionSnapshot {
        MediaTrackSelectionSnapshot {
            video: MediaTrackSelection::auto(),
            audio: MediaTrackSelection::auto(),
            subtitle: MediaTrackSelection::disabled(),
            abr_policy: MediaAbrPolicy::default(),
        }
    }

    struct FakeIosSession {
        source_uri: String,
        media_info: PlayerMediaInfo,
    }

    impl IosNativePlayerSession for FakeIosSession {
        fn source_uri(&self) -> &str {
            &self.source_uri
        }

        fn capabilities(&self) -> PlayerRuntimeAdapterCapabilities {
            super::ios_native_capabilities()
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

        fn drain_events(&mut self) -> Vec<player_runtime::PlayerRuntimeEvent> {
            Vec::new()
        }

        fn dispatch(
            &mut self,
            _command: PlayerRuntimeCommand,
        ) -> PlayerRuntimeResult<PlayerRuntimeCommandResult> {
            Err(player_runtime::PlayerRuntimeError::new(
                PlayerRuntimeErrorCode::Unsupported,
                "fake ios session does not implement commands",
            ))
        }

        fn advance(&mut self) -> PlayerRuntimeResult<Option<DecodedVideoFrame>> {
            Ok(None)
        }

        fn next_deadline(&self) -> Option<Instant> {
            None
        }
    }
}
