use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use player_model::MediaSource;
use player_platform_mobile::{MobilePluginConfiguration, apply_mobile_plugin_diagnostics};
use player_runtime::{
    DEFAULT_PLAYBACK_RATE, DecodedVideoFrame, MAX_PLAYBACK_RATE, MIN_PLAYBACK_RATE, MediaAbrMode,
    MediaAbrPolicy, MediaTrackCatalog, MediaTrackKind, MediaTrackSelection,
    MediaTrackSelectionMode, MediaTrackSelectionSnapshot, PlaybackProgress, PlayerError,
    PlayerErrorCategory, PlayerErrorCode, PlayerMediaInfo, PlayerResilienceMetrics,
    PlayerResilienceMetricsTracker, PlayerResult, PlayerRuntimeAdapter,
    PlayerRuntimeAdapterBackendFamily, PlayerRuntimeAdapterBootstrap,
    PlayerRuntimeAdapterCapabilities, PlayerRuntimeAdapterFactory, PlayerRuntimeAdapterInitializer,
    PlayerRuntimeCommand, PlayerRuntimeCommandResult, PlayerRuntimeEvent, PlayerRuntimeOptions,
    PlayerRuntimeStartup, PlayerSeekableRange, PlayerSnapshot, PlayerTimelineKind,
    PlayerTimelineSnapshot, PresentationState,
};

pub const ANDROID_NATIVE_PLAYER_RUNTIME_ADAPTER_ID: &str = "android_native";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AndroidHostTimelineKind {
    Vod,
    Live,
    LiveDvr,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AndroidHostSeekableRange {
    pub start_ms: u64,
    pub end_ms: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AndroidHostSnapshot {
    pub playback_state: PresentationState,
    pub playback_rate: f32,
    pub is_buffering: bool,
    pub is_interrupted: bool,
    pub timeline_kind: AndroidHostTimelineKind,
    pub is_seekable: bool,
    pub seekable_range: Option<AndroidHostSeekableRange>,
    pub live_edge_ms: Option<u64>,
    pub position_ms: u64,
    pub duration_ms: Option<u64>,
    pub resilience_metrics: PlayerResilienceMetrics,
}

#[derive(Debug, Clone, PartialEq)]
pub enum AndroidHostEvent {
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
        code: PlayerErrorCode,
        category: PlayerErrorCategory,
        retriable: bool,
        message: String,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub enum AndroidHostCommand {
    Play,
    Pause,
    SeekTo { position_ms: u64 },
    Stop,
    SetPlaybackRate { rate: f32 },
    SetVideoTrackSelection { selection: MediaTrackSelection },
    SetAudioTrackSelection { selection: MediaTrackSelection },
    SetSubtitleTrackSelection { selection: MediaTrackSelection },
    SetAbrPolicy { policy: MediaAbrPolicy },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AndroidOpaqueHandle(pub usize);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AndroidVideoSurfaceKind {
    Surface,
    SurfaceView,
    SurfaceTexture,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AndroidVideoSurfaceTarget {
    pub kind: AndroidVideoSurfaceKind,
    pub handle: AndroidOpaqueHandle,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AndroidExoPlayerBridgeContext {
    pub java_vm: AndroidOpaqueHandle,
    pub exo_player: AndroidOpaqueHandle,
    pub video_surface: Option<AndroidVideoSurfaceTarget>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AndroidExoPlaybackState {
    Idle,
    Buffering,
    Ready,
    Ended,
}

#[derive(Debug, Clone)]
pub struct AndroidExoPlaybackSnapshot {
    pub playback_state: AndroidExoPlaybackState,
    pub play_when_ready: bool,
    pub playback_rate: f32,
    pub position: Duration,
    pub duration: Option<Duration>,
    pub is_live: bool,
    pub is_seekable: bool,
    pub seekable_range: Option<AndroidExoSeekableRange>,
    pub live_edge: Option<Duration>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AndroidExoSeekableRange {
    pub start: Duration,
    pub end: Duration,
}

#[derive(Debug, Clone)]
pub struct AndroidNativeObservation {
    pub presentation_state: PresentationState,
    pub is_buffering: bool,
    pub playback_rate: f32,
    pub progress: PlaybackProgress,
    pub emitted_events: Vec<PlayerRuntimeEvent>,
}

#[derive(Debug, Default, Clone)]
pub struct AndroidExoStateTracker {
    has_started_playback: bool,
    last_presentation_state: Option<PresentationState>,
    last_is_buffering: Option<bool>,
    last_playback_rate: Option<f32>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum AndroidNativePlayerCommand {
    Play,
    Pause,
    SeekTo { position: Duration },
    Stop,
    SetPlaybackRate { rate: f32 },
    SetVideoTrackSelection { selection: MediaTrackSelection },
    SetAudioTrackSelection { selection: MediaTrackSelection },
    SetSubtitleTrackSelection { selection: MediaTrackSelection },
    SetAbrPolicy { policy: MediaAbrPolicy },
}

pub trait AndroidNativeCommandSink: Send {
    fn submit_command(&mut self, command: AndroidNativePlayerCommand) -> PlayerResult<()>;
}

impl<T> AndroidNativeCommandSink for Box<T>
where
    T: AndroidNativeCommandSink + ?Sized,
{
    fn submit_command(&mut self, command: AndroidNativePlayerCommand) -> PlayerResult<()> {
        (**self).submit_command(command)
    }
}

#[derive(Debug, Clone)]
pub enum AndroidNativeSessionUpdate {
    Snapshot(AndroidExoPlaybackSnapshot),
    MediaInfo {
        track_catalog: MediaTrackCatalog,
        track_selection: MediaTrackSelectionSnapshot,
    },
    SeekCompleted {
        position: Duration,
    },
    RetryScheduled {
        attempt: u32,
        delay: Duration,
    },
    Error(PlayerError),
}

#[derive(Debug, Clone, Default)]
pub struct AndroidManagedNativeSessionController {
    updates: Arc<Mutex<VecDeque<AndroidNativeSessionUpdate>>>,
}

pub struct AndroidManagedNativeSession<C> {
    source_uri: String,
    media_info: PlayerMediaInfo,
    capabilities: PlayerRuntimeAdapterCapabilities,
    command_sink: C,
    controller: AndroidManagedNativeSessionController,
    tracker: AndroidExoStateTracker,
    presentation_state: PresentationState,
    is_buffering: bool,
    playback_rate: f32,
    progress: PlaybackProgress,
    timeline_metadata: Option<AndroidLiveTimelineMetadata>,
    resilience_metrics: PlayerResilienceMetricsTracker,
    events: VecDeque<PlayerRuntimeEvent>,
}

#[derive(Debug, Clone, Copy)]
struct AndroidLiveTimelineMetadata {
    kind: PlayerTimelineKind,
    seekable_range: Option<PlayerSeekableRange>,
    live_edge: Option<Duration>,
}

pub trait AndroidNativePlayerBridge: Send + Sync {
    fn probe_source(
        &self,
        source: &MediaSource,
        options: &PlayerRuntimeOptions,
    ) -> PlayerResult<AndroidNativePlayerProbe>;

    fn initialize_session(
        &self,
        source: MediaSource,
        options: PlayerRuntimeOptions,
        media_info: &PlayerMediaInfo,
        startup: &PlayerRuntimeStartup,
    ) -> PlayerResult<AndroidNativePlayerSessionBootstrap>;
}

pub trait AndroidExoPlayerBridgeBindings: Send + Sync {
    fn probe_source(
        &self,
        context: &AndroidExoPlayerBridgeContext,
        source: &MediaSource,
        options: &PlayerRuntimeOptions,
    ) -> PlayerResult<AndroidNativePlayerProbe>;

    fn create_command_sink(
        &self,
        context: AndroidExoPlayerBridgeContext,
        source: &MediaSource,
        options: &PlayerRuntimeOptions,
        media_info: &PlayerMediaInfo,
        startup: &PlayerRuntimeStartup,
        controller: AndroidManagedNativeSessionController,
    ) -> PlayerResult<Box<dyn AndroidNativeCommandSink>>;
}

pub trait AndroidNativePlayerSession: Send {
    fn source_uri(&self) -> &str;
    fn capabilities(&self) -> PlayerRuntimeAdapterCapabilities;
    fn media_info(&self) -> &PlayerMediaInfo;
    fn presentation_state(&self) -> PresentationState;
    fn is_buffering(&self) -> bool {
        false
    }
    fn playback_rate(&self) -> f32;
    fn progress(&self) -> PlaybackProgress;
    fn drain_events(&mut self) -> Vec<PlayerRuntimeEvent>;
    fn dispatch(
        &mut self,
        command: PlayerRuntimeCommand,
    ) -> PlayerResult<PlayerRuntimeCommandResult>;
    fn advance(&mut self) -> PlayerResult<Option<DecodedVideoFrame>>;
    fn next_deadline(&self) -> Option<Instant>;
}

#[derive(Debug, Clone)]
pub struct AndroidNativePlayerProbe {
    pub media_info: PlayerMediaInfo,
    pub startup: PlayerRuntimeStartup,
}

pub struct AndroidNativePlayerSessionBootstrap {
    pub runtime: Box<dyn AndroidNativePlayerSession>,
    pub initial_frame: Option<DecodedVideoFrame>,
}

pub struct AndroidHostBridgeSession {
    session: AndroidManagedNativeSession<AndroidHostCommandSink>,
    command_queue: Arc<Mutex<VecDeque<AndroidNativePlayerCommand>>>,
    surface_attached: bool,
    extra_events: VecDeque<PlayerRuntimeEvent>,
}

#[derive(Clone)]
pub struct AndroidExoPlayerBridge {
    context: AndroidExoPlayerBridgeContext,
    bindings: Arc<dyn AndroidExoPlayerBridgeBindings>,
}

#[derive(Clone, Default)]
pub struct AndroidNativePlayerRuntimeAdapterFactory {
    bridge: Option<Arc<dyn AndroidNativePlayerBridge>>,
}

pub struct AndroidNativePlayerRuntimeInitializer {
    bridge: Option<Arc<dyn AndroidNativePlayerBridge>>,
    source: MediaSource,
    options: PlayerRuntimeOptions,
    media_info: PlayerMediaInfo,
    startup: PlayerRuntimeStartup,
}

pub struct AndroidNativePlayerRuntime {
    inner: Box<dyn AndroidNativePlayerSession>,
}

impl<C> std::fmt::Debug for AndroidManagedNativeSession<C> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AndroidManagedNativeSession")
            .field("source_uri", &self.source_uri)
            .field("state", &self.presentation_state)
            .field("playback_rate", &self.playback_rate)
            .finish()
    }
}

impl std::fmt::Debug for AndroidNativePlayerRuntimeAdapterFactory {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AndroidNativePlayerRuntimeAdapterFactory")
            .field("has_bridge", &self.bridge.is_some())
            .finish()
    }
}

impl std::fmt::Debug for AndroidNativePlayerRuntimeInitializer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AndroidNativePlayerRuntimeInitializer")
            .field("source", &self.source.uri())
            .field("has_bridge", &self.bridge.is_some())
            .finish()
    }
}

impl std::fmt::Debug for AndroidNativePlayerRuntime {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AndroidNativePlayerRuntime")
            .field("source_uri", &self.inner.source_uri())
            .field("state", &self.inner.presentation_state())
            .finish()
    }
}

impl std::fmt::Debug for AndroidExoPlayerBridge {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AndroidExoPlayerBridge")
            .field("context", &self.context)
            .finish()
    }
}

#[derive(Debug, Clone)]
struct AndroidHostCommandSink {
    queue: Arc<Mutex<VecDeque<AndroidNativePlayerCommand>>>,
}

impl AndroidHostCommandSink {
    fn new(queue: Arc<Mutex<VecDeque<AndroidNativePlayerCommand>>>) -> Self {
        Self { queue }
    }
}

impl AndroidNativeCommandSink for AndroidHostCommandSink {
    fn submit_command(&mut self, command: AndroidNativePlayerCommand) -> PlayerResult<()> {
        match self.queue.lock() {
            Ok(mut queue) => {
                queue.push_back(command);
            }
            Err(_) => {
                tracing::error!("android native command queue mutex was poisoned");
            }
        }
        Ok(())
    }
}

impl AndroidNativePlayerRuntimeAdapterFactory {
    pub fn with_bridge(bridge: Arc<dyn AndroidNativePlayerBridge>) -> Self {
        Self {
            bridge: Some(bridge),
        }
    }
}

impl AndroidExoPlayerBridge {
    pub fn new(
        context: AndroidExoPlayerBridgeContext,
        bindings: Arc<dyn AndroidExoPlayerBridgeBindings>,
    ) -> Self {
        Self { context, bindings }
    }
}

impl AndroidHostSnapshot {
    pub fn from_player_snapshot(snapshot: &PlayerSnapshot) -> Self {
        Self {
            playback_state: snapshot.state,
            playback_rate: snapshot.playback_rate,
            is_buffering: snapshot.is_buffering,
            is_interrupted: snapshot.is_interrupted,
            timeline_kind: host_timeline_kind(snapshot.timeline.kind),
            is_seekable: snapshot.timeline.is_seekable,
            seekable_range: snapshot.timeline.seekable_range.map(|range| {
                AndroidHostSeekableRange {
                    start_ms: duration_to_millis(range.start),
                    end_ms: duration_to_millis(range.end),
                }
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

impl AndroidHostEvent {
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
            | PlayerRuntimeEvent::AudioOutputChanged(_)
            | PlayerRuntimeEvent::Warning(_) => None,
        }
    }
}

impl AndroidHostCommand {
    pub fn from_native_command(command: &AndroidNativePlayerCommand) -> Self {
        match command {
            AndroidNativePlayerCommand::Play => Self::Play,
            AndroidNativePlayerCommand::Pause => Self::Pause,
            AndroidNativePlayerCommand::SeekTo { position } => Self::SeekTo {
                position_ms: duration_to_millis(*position),
            },
            AndroidNativePlayerCommand::Stop => Self::Stop,
            AndroidNativePlayerCommand::SetPlaybackRate { rate } => {
                Self::SetPlaybackRate { rate: *rate }
            }
            AndroidNativePlayerCommand::SetVideoTrackSelection { selection } => {
                Self::SetVideoTrackSelection {
                    selection: selection.clone(),
                }
            }
            AndroidNativePlayerCommand::SetAudioTrackSelection { selection } => {
                Self::SetAudioTrackSelection {
                    selection: selection.clone(),
                }
            }
            AndroidNativePlayerCommand::SetSubtitleTrackSelection { selection } => {
                Self::SetSubtitleTrackSelection {
                    selection: selection.clone(),
                }
            }
            AndroidNativePlayerCommand::SetAbrPolicy { policy } => Self::SetAbrPolicy {
                policy: policy.clone(),
            },
        }
    }
}

impl AndroidHostBridgeSession {
    pub fn new(source_uri: impl Into<String>) -> Self {
        Self::new_with_plugin_configuration(source_uri, MobilePluginConfiguration::default())
    }

    pub fn new_with_plugin_configuration(
        source_uri: impl Into<String>,
        _plugin_configuration: MobilePluginConfiguration,
    ) -> Self {
        let source_uri = source_uri.into();
        let command_queue = Arc::new(Mutex::new(VecDeque::new()));
        let source = MediaSource::new(source_uri.clone());
        let media_info = placeholder_media_info(&source);
        let sink = AndroidHostCommandSink::new(command_queue.clone());
        let session = AndroidManagedNativeSession::new(source_uri, media_info, sink);

        Self {
            session,
            command_queue,
            surface_attached: false,
            extra_events: VecDeque::new(),
        }
    }

    pub fn snapshot(&mut self) -> AndroidHostSnapshot {
        AndroidHostSnapshot::from_player_snapshot(&self.session.snapshot())
    }

    pub fn drain_events(&mut self) -> Vec<AndroidHostEvent> {
        let mut raw_events: Vec<PlayerRuntimeEvent> = self.extra_events.drain(..).collect();
        raw_events.extend(self.session.drain_events());
        raw_events
            .iter()
            .filter_map(AndroidHostEvent::from_runtime_event)
            .collect()
    }

    pub fn drain_native_commands(&mut self) -> Vec<AndroidHostCommand> {
        self.command_queue
            .lock()
            .map(|mut queue| {
                queue
                    .drain(..)
                    .map(|command| AndroidHostCommand::from_native_command(&command))
                    .collect()
            })
            .unwrap_or_default()
    }

    pub fn dispatch_command(
        &mut self,
        command: PlayerRuntimeCommand,
    ) -> PlayerResult<PlayerRuntimeCommandResult> {
        self.session.dispatch(command)
    }

    pub fn set_surface_attached(&mut self, attached: bool) {
        if self.surface_attached != attached {
            self.surface_attached = attached;
            self.extra_events
                .push_back(PlayerRuntimeEvent::VideoSurfaceChanged { attached });
        }
    }

    pub fn apply_exo_snapshot(&mut self, snapshot: AndroidExoPlaybackSnapshot) {
        self.session.apply_snapshot(&snapshot);
    }

    pub fn report_media_info(
        &mut self,
        track_catalog: MediaTrackCatalog,
        track_selection: MediaTrackSelectionSnapshot,
    ) {
        self.session
            .controller()
            .report_media_info(track_catalog, track_selection);
    }

    pub fn report_seek_completed(&mut self, position: Duration) {
        self.session.controller().report_seek_completed(position);
    }

    pub fn report_retry_scheduled(&mut self, attempt: u32, delay: Duration) {
        self.session
            .controller()
            .report_retry_scheduled(attempt, delay);
    }

    pub fn report_error(&mut self, code: PlayerErrorCode, message: impl Into<String>) {
        self.session.controller().report_error(code, message);
    }

    pub fn report_player_error(&mut self, error: PlayerError) {
        self.session.controller().report_player_error(error);
    }
}

impl PlayerRuntimeAdapterFactory for AndroidNativePlayerRuntimeAdapterFactory {
    fn adapter_id(&self) -> &'static str {
        ANDROID_NATIVE_PLAYER_RUNTIME_ADAPTER_ID
    }

    fn probe_source_with_options(
        &self,
        source: MediaSource,
        options: PlayerRuntimeOptions,
    ) -> PlayerResult<Box<dyn PlayerRuntimeAdapterInitializer>> {
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
        let startup = apply_mobile_plugin_diagnostics(
            startup,
            &source,
            &MobilePluginConfiguration::from_runtime_options(&options),
        );

        Ok(Box::new(AndroidNativePlayerRuntimeInitializer {
            bridge: self.bridge.clone(),
            source,
            options,
            media_info,
            startup,
        }))
    }
}

impl PlayerRuntimeAdapterInitializer for AndroidNativePlayerRuntimeInitializer {
    fn capabilities(&self) -> PlayerRuntimeAdapterCapabilities {
        android_native_capabilities()
    }

    fn media_info(&self) -> PlayerMediaInfo {
        self.media_info.clone()
    }

    fn startup(&self) -> PlayerRuntimeStartup {
        self.startup.clone()
    }

    fn initialize(self: Box<Self>) -> PlayerResult<PlayerRuntimeAdapterBootstrap> {
        let Self {
            bridge,
            source,
            options,
            media_info,
            startup,
        } = *self;

        let Some(bridge) = bridge else {
            return Err(PlayerError::new(
                PlayerErrorCode::Unsupported,
                android_native_unavailable_message(),
            ));
        };

        let bootstrap = bridge.initialize_session(source, options, &media_info, &startup)?;

        Ok(PlayerRuntimeAdapterBootstrap {
            runtime: Box::new(AndroidNativePlayerRuntime {
                inner: bootstrap.runtime,
            }),
            initial_frame: bootstrap.initial_frame,
            startup,
        })
    }
}

impl PlayerRuntimeAdapter for AndroidNativePlayerRuntime {
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
    ) -> PlayerResult<PlayerRuntimeCommandResult> {
        self.inner.dispatch(command)
    }

    fn advance(&mut self) -> PlayerResult<Option<DecodedVideoFrame>> {
        self.inner.advance()
    }

    fn next_deadline(&self) -> Option<Instant> {
        self.inner.next_deadline()
    }
}

impl AndroidExoStateTracker {
    pub fn observe(&mut self, snapshot: &AndroidExoPlaybackSnapshot) -> AndroidNativeObservation {
        let presentation_state = self.presentation_state(snapshot);
        let is_buffering = snapshot.playback_state == AndroidExoPlaybackState::Buffering;
        let playback_rate = sanitize_native_playback_rate(snapshot.playback_rate);
        let progress = PlaybackProgress::new(snapshot.position, snapshot.duration);
        let mut emitted_events = Vec::new();

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

        if should_emit_playback_rate_change(self.last_playback_rate, playback_rate) {
            emitted_events.push(PlayerRuntimeEvent::PlaybackRateChanged {
                rate: playback_rate,
            });
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

        if presentation_state == PresentationState::Playing {
            self.has_started_playback = true;
        }
        self.last_presentation_state = Some(presentation_state);
        self.last_is_buffering = Some(is_buffering);
        self.last_playback_rate = Some(playback_rate);

        AndroidNativeObservation {
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
        self.last_presentation_state = Some(presentation_state);
        self.last_is_buffering = Some(false);
        self.last_playback_rate = Some(playback_rate);
    }

    fn presentation_state(&self, snapshot: &AndroidExoPlaybackSnapshot) -> PresentationState {
        match snapshot.playback_state {
            AndroidExoPlaybackState::Ended => PresentationState::Finished,
            AndroidExoPlaybackState::Ready if snapshot.play_when_ready => {
                PresentationState::Playing
            }
            AndroidExoPlaybackState::Buffering if snapshot.play_when_ready => {
                PresentationState::Playing
            }
            AndroidExoPlaybackState::Idle | AndroidExoPlaybackState::Buffering => {
                if self.has_started_playback {
                    PresentationState::Paused
                } else {
                    PresentationState::Ready
                }
            }
            AndroidExoPlaybackState::Ready => {
                if self.has_started_playback {
                    PresentationState::Paused
                } else {
                    PresentationState::Ready
                }
            }
        }
    }
}

impl AndroidManagedNativeSessionController {
    pub fn apply_snapshot(&self, snapshot: AndroidExoPlaybackSnapshot) {
        self.push_update(AndroidNativeSessionUpdate::Snapshot(snapshot));
    }

    pub fn report_media_info(
        &self,
        track_catalog: MediaTrackCatalog,
        track_selection: MediaTrackSelectionSnapshot,
    ) {
        self.push_update(AndroidNativeSessionUpdate::MediaInfo {
            track_catalog,
            track_selection,
        });
    }

    pub fn report_seek_completed(&self, position: Duration) {
        self.push_update(AndroidNativeSessionUpdate::SeekCompleted { position });
    }

    pub fn report_retry_scheduled(&self, attempt: u32, delay: Duration) {
        self.push_update(AndroidNativeSessionUpdate::RetryScheduled { attempt, delay });
    }

    pub fn report_error(&self, code: PlayerErrorCode, message: impl Into<String>) {
        self.push_update(AndroidNativeSessionUpdate::Error(PlayerError::new(
            code,
            message.into(),
        )));
    }

    pub fn report_player_error(&self, error: PlayerError) {
        self.push_update(AndroidNativeSessionUpdate::Error(error));
    }

    pub fn push_update(&self, update: AndroidNativeSessionUpdate) {
        match self.updates.lock() {
            Ok(mut updates) => {
                updates.push_back(update);
            }
            Err(_) => {
                tracing::error!("android native session update mutex was poisoned");
            }
        }
    }

    fn take_pending(&self) -> Vec<AndroidNativeSessionUpdate> {
        self.updates
            .lock()
            .map(|mut updates| updates.drain(..).collect())
            .unwrap_or_default()
    }
}

impl<C: AndroidNativeCommandSink> AndroidManagedNativeSession<C> {
    pub fn new(
        source_uri: impl Into<String>,
        media_info: PlayerMediaInfo,
        command_sink: C,
    ) -> Self {
        Self::with_capabilities(
            source_uri,
            media_info,
            android_native_capabilities(),
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
    ) -> (Self, AndroidManagedNativeSessionController) {
        Self::with_capabilities_and_controller(
            source_uri,
            media_info,
            android_native_capabilities(),
            command_sink,
        )
    }

    pub fn with_capabilities_and_controller(
        source_uri: impl Into<String>,
        media_info: PlayerMediaInfo,
        capabilities: PlayerRuntimeAdapterCapabilities,
        command_sink: C,
    ) -> (Self, AndroidManagedNativeSessionController) {
        let controller = AndroidManagedNativeSessionController::default();
        let session = Self::with_existing_controller(
            source_uri,
            media_info,
            capabilities,
            command_sink,
            controller.clone(),
        );
        (session, controller)
    }

    pub fn with_existing_controller(
        source_uri: impl Into<String>,
        media_info: PlayerMediaInfo,
        capabilities: PlayerRuntimeAdapterCapabilities,
        command_sink: C,
        controller: AndroidManagedNativeSessionController,
    ) -> Self {
        Self {
            source_uri: source_uri.into(),
            media_info,
            capabilities,
            command_sink,
            controller,
            tracker: AndroidExoStateTracker::default(),
            presentation_state: PresentationState::Ready,
            is_buffering: false,
            playback_rate: DEFAULT_PLAYBACK_RATE,
            progress: PlaybackProgress::new(Duration::ZERO, None),
            timeline_metadata: None,
            resilience_metrics: PlayerResilienceMetricsTracker::default(),
            events: VecDeque::new(),
        }
    }

    pub fn controller(&self) -> AndroidManagedNativeSessionController {
        self.controller.clone()
    }

    fn pump_pending_updates(&mut self) {
        for update in self.controller.take_pending() {
            match update {
                AndroidNativeSessionUpdate::Snapshot(snapshot) => self.apply_snapshot(&snapshot),
                AndroidNativeSessionUpdate::MediaInfo {
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
                AndroidNativeSessionUpdate::SeekCompleted { position } => {
                    self.progress = PlaybackProgress::new(position, self.progress.duration());
                    if self.presentation_state == PresentationState::Finished {
                        self.presentation_state = PresentationState::Ready;
                        self.tracker
                            .seed(self.presentation_state, self.playback_rate);
                    }
                    self.events
                        .push_back(PlayerRuntimeEvent::SeekCompleted { position });
                }
                AndroidNativeSessionUpdate::RetryScheduled { attempt, delay } => {
                    self.resilience_metrics
                        .observe_retry_scheduled(attempt, delay);
                    self.events
                        .push_back(PlayerRuntimeEvent::RetryScheduled { attempt, delay });
                }
                AndroidNativeSessionUpdate::Error(error) => {
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

    pub fn apply_snapshot(&mut self, snapshot: &AndroidExoPlaybackSnapshot) {
        let observation = self.tracker.observe(snapshot);
        self.timeline_metadata = live_timeline_metadata(snapshot);
        self.apply_observation(observation);
    }

    fn apply_observation(&mut self, observation: AndroidNativeObservation) {
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
        let timeline = self
            .timeline_metadata
            .map(|metadata| player_timeline_from_android_live_metadata(self.progress, metadata))
            .unwrap_or_else(|| {
                PlayerTimelineSnapshot::from_media_info(
                    self.progress,
                    self.capabilities.supports_seek,
                    &self.media_info,
                )
            });

        PlayerSnapshot {
            source_uri: self.source_uri.clone(),
            state: self.presentation_state,
            has_video_surface: false,
            is_interrupted: false,
            is_buffering: self.is_buffering,
            playback_rate: self.playback_rate,
            progress: self.progress,
            timeline,
            media_info: self.media_info.clone(),
            resilience_metrics: self.resilience_metrics.snapshot(),
        }
    }

    fn validate_playback_rate(&self, rate: f32) -> PlayerResult<f32> {
        if !rate.is_finite() {
            return Err(PlayerError::new(
                PlayerErrorCode::InvalidArgument,
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
            return Err(PlayerError::new(
                PlayerErrorCode::InvalidArgument,
                format!("playback rate must be within {min:.1}x..={max:.1}x"),
            ));
        }

        Ok(rate)
    }

    fn submit_commands(&mut self, commands: Vec<AndroidNativePlayerCommand>) -> PlayerResult<()> {
        for command in commands {
            self.command_sink.submit_command(command)?;
        }
        Ok(())
    }

    fn validate_track_selection_request(
        &self,
        kind: MediaTrackKind,
        selection: &MediaTrackSelection,
    ) -> PlayerResult<MediaTrackSelection> {
        match selection.mode {
            MediaTrackSelectionMode::Auto => Ok(MediaTrackSelection::auto()),
            MediaTrackSelectionMode::Disabled => Ok(MediaTrackSelection::disabled()),
            MediaTrackSelectionMode::Track => {
                let Some(track_id) = selection.track_id.as_deref() else {
                    return Err(PlayerError::new(
                        PlayerErrorCode::InvalidArgument,
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
                        PlayerError::new(
                            PlayerErrorCode::InvalidArgument,
                            format!(
                                "track '{track_id}' is not present in the current track catalog"
                            ),
                        )
                    })?;

                if track.kind != kind {
                    return Err(PlayerError::new(
                        PlayerErrorCode::InvalidArgument,
                        format!("track '{track_id}' is not a {:?} track", kind),
                    ));
                }

                Ok(MediaTrackSelection::track(track_id))
            }
        }
    }

    fn validate_abr_policy_request(&self, policy: &MediaAbrPolicy) -> PlayerResult<MediaAbrPolicy> {
        match policy.mode {
            MediaAbrMode::Auto => Ok(MediaAbrPolicy::default()),
            MediaAbrMode::Constrained => {
                if policy.max_bit_rate.is_none()
                    && policy.max_width.is_none()
                    && policy.max_height.is_none()
                {
                    return Err(PlayerError::new(
                        PlayerErrorCode::InvalidArgument,
                        "constrained ABR requires at least one bitrate or size constraint",
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
            MediaAbrMode::FixedTrack => {
                let Some(track_id) = policy.track_id.as_deref() else {
                    return Err(PlayerError::new(
                        PlayerErrorCode::InvalidArgument,
                        "fixed-track ABR requires a video track id",
                    ));
                };

                let track = self
                    .media_info
                    .track_catalog
                    .tracks
                    .iter()
                    .find(|track| track.id == track_id)
                    .ok_or_else(|| {
                        PlayerError::new(
                            PlayerErrorCode::InvalidArgument,
                            format!(
                                "track '{track_id}' is not present in the current track catalog"
                            ),
                        )
                    })?;

                if track.kind != MediaTrackKind::Video {
                    return Err(PlayerError::new(
                        PlayerErrorCode::InvalidArgument,
                        format!("track '{track_id}' is not a video track"),
                    ));
                }

                Ok(MediaAbrPolicy {
                    mode: MediaAbrMode::FixedTrack,
                    track_id: Some(track_id.to_owned()),
                    max_bit_rate: None,
                    max_width: None,
                    max_height: None,
                })
            }
        }
    }

    fn translate_command(
        &self,
        command: &PlayerRuntimeCommand,
    ) -> PlayerResult<(bool, Vec<AndroidNativePlayerCommand>)> {
        match command {
            PlayerRuntimeCommand::Play => match self.presentation_state {
                PresentationState::Playing => Ok((false, Vec::new())),
                PresentationState::Finished => Ok((
                    true,
                    vec![
                        AndroidNativePlayerCommand::SeekTo {
                            position: Duration::ZERO,
                        },
                        AndroidNativePlayerCommand::Play,
                    ],
                )),
                PresentationState::Ready | PresentationState::Paused => {
                    Ok((true, vec![AndroidNativePlayerCommand::Play]))
                }
            },
            PlayerRuntimeCommand::Pause => match self.presentation_state {
                PresentationState::Playing => Ok((true, vec![AndroidNativePlayerCommand::Pause])),
                PresentationState::Paused => Ok((false, Vec::new())),
                PresentationState::Ready | PresentationState::Finished => Err(PlayerError::new(
                    PlayerErrorCode::InvalidState,
                    "pause is only valid after playback has started",
                )),
            },
            PlayerRuntimeCommand::TogglePause => match self.presentation_state {
                PresentationState::Playing => Ok((true, vec![AndroidNativePlayerCommand::Pause])),
                PresentationState::Ready | PresentationState::Paused => {
                    Ok((true, vec![AndroidNativePlayerCommand::Play]))
                }
                PresentationState::Finished => Ok((
                    true,
                    vec![
                        AndroidNativePlayerCommand::SeekTo {
                            position: Duration::ZERO,
                        },
                        AndroidNativePlayerCommand::Play,
                    ],
                )),
            },
            PlayerRuntimeCommand::SeekTo { position } => Ok((
                true,
                vec![AndroidNativePlayerCommand::SeekTo {
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
                    vec![AndroidNativePlayerCommand::SetPlaybackRate { rate }],
                ))
            }
            PlayerRuntimeCommand::SetVideoTrackSelection { selection } => {
                let selection =
                    self.validate_track_selection_request(MediaTrackKind::Video, selection)?;
                if self.media_info.track_selection.video == selection {
                    return Ok((false, Vec::new()));
                }
                Ok((
                    true,
                    vec![AndroidNativePlayerCommand::SetVideoTrackSelection { selection }],
                ))
            }
            PlayerRuntimeCommand::SetAudioTrackSelection { selection } => {
                let selection =
                    self.validate_track_selection_request(MediaTrackKind::Audio, selection)?;
                if self.media_info.track_selection.audio == selection {
                    return Ok((false, Vec::new()));
                }
                Ok((
                    true,
                    vec![AndroidNativePlayerCommand::SetAudioTrackSelection { selection }],
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
                    vec![AndroidNativePlayerCommand::SetSubtitleTrackSelection { selection }],
                ))
            }
            PlayerRuntimeCommand::SetAbrPolicy { policy } => {
                let policy = self.validate_abr_policy_request(policy)?;
                if self.media_info.track_selection.abr_policy == policy {
                    return Ok((false, Vec::new()));
                }
                Ok((
                    true,
                    vec![AndroidNativePlayerCommand::SetAbrPolicy { policy }],
                ))
            }
            PlayerRuntimeCommand::Stop => {
                if self.presentation_state == PresentationState::Ready
                    && self.progress.position().is_zero()
                {
                    return Ok((false, Vec::new()));
                }
                Ok((true, vec![AndroidNativePlayerCommand::Stop]))
            }
        }
    }
}

impl AndroidNativePlayerBridge for AndroidExoPlayerBridge {
    fn probe_source(
        &self,
        source: &MediaSource,
        options: &PlayerRuntimeOptions,
    ) -> PlayerResult<AndroidNativePlayerProbe> {
        self.bindings.probe_source(&self.context, source, options)
    }

    fn initialize_session(
        &self,
        source: MediaSource,
        options: PlayerRuntimeOptions,
        media_info: &PlayerMediaInfo,
        startup: &PlayerRuntimeStartup,
    ) -> PlayerResult<AndroidNativePlayerSessionBootstrap> {
        let capabilities = android_native_capabilities();
        let controller = AndroidManagedNativeSessionController::default();
        let command_sink = self.bindings.create_command_sink(
            self.context,
            &source,
            &options,
            media_info,
            startup,
            controller.clone(),
        )?;
        let session = AndroidManagedNativeSession::with_existing_controller(
            source.uri(),
            media_info.clone(),
            capabilities,
            command_sink,
            controller,
        );

        Ok(AndroidNativePlayerSessionBootstrap {
            runtime: Box::new(session),
            initial_frame: None,
        })
    }
}

impl<C: AndroidNativeCommandSink> AndroidNativePlayerSession for AndroidManagedNativeSession<C> {
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
    ) -> PlayerResult<PlayerRuntimeCommandResult> {
        self.pump_pending_updates();
        let previous_state = self.presentation_state;
        let previous_media_info = self.media_info.clone();
        let (applied, native_commands) = self.translate_command(&command)?;
        self.submit_commands(native_commands)?;

        if applied {
            match command {
                PlayerRuntimeCommand::Play => {
                    self.presentation_state = PresentationState::Playing;
                    if previous_state == PresentationState::Finished {
                        self.progress =
                            PlaybackProgress::new(Duration::ZERO, self.progress.duration());
                    }
                }
                PlayerRuntimeCommand::Pause => {
                    self.presentation_state = PresentationState::Paused;
                }
                PlayerRuntimeCommand::TogglePause => {
                    self.presentation_state =
                        if self.presentation_state == PresentationState::Playing {
                            PresentationState::Paused
                        } else {
                            PresentationState::Playing
                        };
                    if previous_state == PresentationState::Finished
                        && self.presentation_state == PresentationState::Playing
                    {
                        self.progress =
                            PlaybackProgress::new(Duration::ZERO, self.progress.duration());
                    }
                }
                PlayerRuntimeCommand::SeekTo { position } => {
                    self.progress = PlaybackProgress::new(position, self.progress.duration());
                    if self.presentation_state == PresentationState::Finished {
                        self.presentation_state = PresentationState::Ready;
                    }
                }
                PlayerRuntimeCommand::SetPlaybackRate { rate } => {
                    self.playback_rate = rate;
                }
                PlayerRuntimeCommand::SetVideoTrackSelection { selection } => {
                    let selected_track_id = selection.track_id.clone();
                    self.media_info.track_selection.video = selection;
                    match self.media_info.track_selection.video.mode {
                        MediaTrackSelectionMode::Track => {
                            self.media_info.track_selection.abr_policy = MediaAbrPolicy {
                                mode: MediaAbrMode::FixedTrack,
                                track_id: selected_track_id,
                                max_bit_rate: None,
                                max_width: None,
                                max_height: None,
                            };
                        }
                        MediaTrackSelectionMode::Auto | MediaTrackSelectionMode::Disabled => {
                            if self.media_info.track_selection.abr_policy.mode
                                == MediaAbrMode::FixedTrack
                            {
                                self.media_info.track_selection.abr_policy =
                                    MediaAbrPolicy::default();
                            }
                        }
                    }
                }
                PlayerRuntimeCommand::SetAudioTrackSelection { selection } => {
                    self.media_info.track_selection.audio = selection;
                }
                PlayerRuntimeCommand::SetSubtitleTrackSelection { selection } => {
                    self.media_info.track_selection.subtitle = selection;
                }
                PlayerRuntimeCommand::SetAbrPolicy { policy } => {
                    let policy_mode = policy.mode;
                    let policy_track_id = policy.track_id.clone();
                    self.media_info.track_selection.abr_policy = policy;
                    match policy_mode {
                        MediaAbrMode::FixedTrack => {
                            if let Some(track_id) = policy_track_id {
                                self.media_info.track_selection.video =
                                    MediaTrackSelection::track(track_id);
                            }
                        }
                        MediaAbrMode::Auto | MediaAbrMode::Constrained => {
                            if self.media_info.track_selection.video.mode
                                == MediaTrackSelectionMode::Track
                            {
                                self.media_info.track_selection.video = MediaTrackSelection::auto();
                            }
                        }
                    }
                }
                PlayerRuntimeCommand::Stop => {
                    self.presentation_state = PresentationState::Ready;
                    self.progress = PlaybackProgress::new(Duration::ZERO, self.progress.duration());
                }
            }
            if self.media_info.track_selection != previous_media_info.track_selection {
                self.events
                    .push_back(PlayerRuntimeEvent::MetadataReady(self.media_info.clone()));
            }
            self.tracker
                .seed(self.presentation_state, self.playback_rate);
        }

        Ok(PlayerRuntimeCommandResult {
            applied,
            frame: None,
            snapshot: self.snapshot(),
        })
    }

    fn advance(&mut self) -> PlayerResult<Option<DecodedVideoFrame>> {
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

fn duration_to_millis(duration: Duration) -> u64 {
    duration.as_millis().min(u128::from(u64::MAX)) as u64
}

fn live_timeline_metadata(
    snapshot: &AndroidExoPlaybackSnapshot,
) -> Option<AndroidLiveTimelineMetadata> {
    if !snapshot.is_live {
        return None;
    }

    let seekable_range = if snapshot.is_seekable {
        snapshot.seekable_range.map(|range| PlayerSeekableRange {
            start: range.start,
            end: range.end,
        })
    } else {
        None
    };
    let live_edge = snapshot
        .live_edge
        .or_else(|| seekable_range.map(|range| range.end));
    let kind = if seekable_range.is_some() {
        PlayerTimelineKind::LiveDvr
    } else {
        PlayerTimelineKind::Live
    };

    Some(AndroidLiveTimelineMetadata {
        kind,
        seekable_range,
        live_edge,
    })
}

fn player_timeline_from_android_live_metadata(
    progress: PlaybackProgress,
    metadata: AndroidLiveTimelineMetadata,
) -> PlayerTimelineSnapshot {
    match metadata.kind {
        PlayerTimelineKind::LiveDvr => {
            if let Some(seekable_range) = metadata.seekable_range {
                PlayerTimelineSnapshot::live_dvr(progress, seekable_range, metadata.live_edge)
            } else {
                PlayerTimelineSnapshot {
                    kind: PlayerTimelineKind::Live,
                    is_seekable: false,
                    seekable_range: None,
                    live_edge: metadata.live_edge,
                    position: progress.position(),
                    duration: None,
                }
            }
        }
        PlayerTimelineKind::Live => PlayerTimelineSnapshot {
            kind: PlayerTimelineKind::Live,
            is_seekable: false,
            seekable_range: None,
            live_edge: metadata.live_edge,
            position: progress.position(),
            duration: None,
        },
        PlayerTimelineKind::Vod => PlayerTimelineSnapshot::vod(progress, true),
    }
}

fn host_timeline_kind(kind: player_runtime::PlayerTimelineKind) -> AndroidHostTimelineKind {
    match kind {
        player_runtime::PlayerTimelineKind::Vod => AndroidHostTimelineKind::Vod,
        player_runtime::PlayerTimelineKind::Live => AndroidHostTimelineKind::Live,
        player_runtime::PlayerTimelineKind::LiveDvr => AndroidHostTimelineKind::LiveDvr,
    }
}

fn placeholder_startup() -> PlayerRuntimeStartup {
    PlayerRuntimeStartup {
        ffmpeg_initialized: false,
        audio_output: None,
        decoded_audio: None,
        video_decode: None,
        plugin_diagnostics: Vec::new(),
    }
}

fn normalize_media_info(source: &MediaSource, mut media_info: PlayerMediaInfo) -> PlayerMediaInfo {
    media_info.source_uri = source.uri().to_owned();
    media_info.source_kind = source.kind();
    media_info.source_protocol = source.protocol();
    media_info
}

fn android_native_capabilities() -> PlayerRuntimeAdapterCapabilities {
    PlayerRuntimeAdapterCapabilities {
        adapter_id: ANDROID_NATIVE_PLAYER_RUNTIME_ADAPTER_ID,
        backend_family: PlayerRuntimeAdapterBackendFamily::NativeAndroid,
        supports_audio_output: true,
        supports_frame_output: false,
        supports_external_video_surface: true,
        supports_seek: true,
        supports_stop: true,
        supports_playback_rate: true,
        playback_rate_min: Some(0.5),
        playback_rate_max: Some(3.0),
        natural_playback_rate_max: Some(2.0),
        supports_hardware_decode: true,
        supports_streaming: true,
        supports_hdr: true,
    }
}

fn android_native_unavailable_message() -> &'static str {
    if cfg!(target_os = "android") {
        "android native adapter skeleton exists, but the platform player bridge is not implemented yet"
    } else {
        "android native adapter can be probed as a skeleton on desktop builds, but initialization is only planned for Android targets"
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
#[path = "native_tests.rs"]
mod tests;
