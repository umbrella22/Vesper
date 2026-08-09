use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use player_model::MediaSource;
use player_platform_mobile::{
    MobileCommandQueue, MobileNativeFramePipelineConfiguration, MobilePluginConfiguration,
    MobileSourceNormalizerConfiguration, apply_mobile_plugin_diagnostics,
    hdr_programmable_processing_not_supported_reason, open_mobile_source_normalizer_packet_session,
    push_video_surface_event,
};
use player_platform_native_frame::{
    NativeFrameDecoderAdapter, NativeFramePacketRead, NativeFramePacketSourceAdapter,
    NativeFramePipelineCore, NativeFramePipelineCoreConfig, NativeFramePipelineCounters,
    NativeFramePipelineError, NativeFramePipelineFrame, NativeFramePipelineFrameResult,
    NativeFramePipelineFrameStatus, NativeFramePipelineLifecycleState, NativeFramePresenterAdapter,
    NativeFramePresenterFrame, NativeFramePresenterSubmitResult, NativeFrameProcessorChainCore,
    NativeFrameProcessorError, NativeFrameProcessorMetricsDelta, NativeFrameProcessorNode,
    NativeFrameProcessorOwnedFrame, NativeFrameProcessorPipelineAdapter,
    NativeFrameProcessorProcessedFrame, NativeFrameProcessorReleaseError,
    NativeFrameProcessorReleaseResult, NoopNativeFrameProcessorObserver,
};
use player_plugin::{
    DecoderMediaKind, DecoderNativeDeviceContext, DecoderNativeDeviceContextKind,
    DecoderNativeFrame, DecoderNativeHandleKind, DecoderPacket, DecoderPacketResult,
    DecoderReceiveNativeFrameOutput, DecoderSessionConfig, DecoderSessionRequirements,
    FrameProcessorCapabilities, FrameProcessorSessionConfig, FrameProcessorSessionRequirements,
    NativeDecoderPluginFactory, NativeDecoderSession, NativeFrameMetadata,
    NativeFramePipelineProfile, NativeHandleKind, SourceNormalizerPacket,
    SourceNormalizerPacketMediaKind, SourceNormalizerPacketSeek, SourceNormalizerPacketSession,
    SourceNormalizerPacketStreamInfo, SourceNormalizerPacketTrackInfo,
    SourceNormalizerReadPacketStatus, normalize_decoder_codec_identifier,
};
use player_plugin_loader::{
    DecoderPluginMatchRequest, NativePluginArtifact, PluginCapabilitySummary, PluginRegistry,
};
use player_runtime::{
    DEFAULT_PLAYBACK_RATE, DecodedVideoFrame, FirstFrameReady, FixedTrackSelectionErrorDetails,
    FrameProcessorMode, FrameProcessorPolicy, MAX_PENDING_RUNTIME_EVENTS, MAX_PLAYBACK_RATE,
    MIN_PLAYBACK_RATE, MediaAbrMode, MediaAbrPolicy, MediaSourceKind, MediaSourceProtocol,
    MediaTrackCatalog, MediaTrackKind, MediaTrackSelection, MediaTrackSelectionMode,
    MediaTrackSelectionSnapshot, MediaTrackSupportStatus, NativeFramePipelineMode,
    PipelineEventContext, PipelineEventDispatcher, PipelineEventHookRegistration,
    PipelineEventHookReportBatch, PlaybackProgress, PlayerError, PlayerErrorCategory,
    PlayerErrorCode, PlayerMediaInfo, PlayerPlaybackRoute, PlayerResilienceMetrics,
    PlayerResilienceMetricsTracker, PlayerResult, PlayerRuntimeAdapter,
    PlayerRuntimeAdapterBackendFamily, PlayerRuntimeAdapterBootstrap,
    PlayerRuntimeAdapterCapabilities, PlayerRuntimeAdapterFactory, PlayerRuntimeAdapterInitializer,
    PlayerRuntimeCommand, PlayerRuntimeCommandResult, PlayerRuntimeEvent, PlayerRuntimeOptions,
    PlayerRuntimeStartup, PlayerSeekableRange, PlayerSnapshot, PlayerTimelineKind,
    PlayerTimelineSnapshot, PresentationState, SourceNormalizerMode, SubtitleErrorDetails,
    extend_runtime_events_bounded, push_runtime_event_bounded,
};
use serde::Serialize;

pub const ANDROID_NATIVE_PLAYER_RUNTIME_ADAPTER_ID: &str = "android_native";
const ANDROID_NATIVE_FRAME_ADVANCE_PACKET_BUDGET: usize = 8;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AndroidNativeFramePipelineProfile {
    HostTimedSurface,
    #[allow(dead_code)]
    SdkOwnedHardwareBuffer,
}

impl AndroidNativeFramePipelineProfile {
    fn wire_name(self) -> &'static str {
        match self {
            Self::HostTimedSurface => "hostTimedSurface",
            Self::SdkOwnedHardwareBuffer => "sdkOwnedHardwareBuffer",
        }
    }

    fn pipeline_profile(self) -> NativeFramePipelineProfile {
        match self {
            Self::HostTimedSurface => NativeFramePipelineProfile::MediaCodecSurfaceTexture,
            Self::SdkOwnedHardwareBuffer => NativeFramePipelineProfile::MediaCodecHardwareBuffer,
        }
    }

    fn pipeline_profile_label(self) -> String {
        self.pipeline_profile().label()
    }

    fn processor_handle_kind(self) -> NativeHandleKind {
        match self {
            Self::HostTimedSurface => NativeHandleKind::MediaCodecSurfaceTexture,
            Self::SdkOwnedHardwareBuffer => NativeHandleKind::MediaCodecHardwareBuffer,
        }
    }

    fn processor_format(self) -> player_plugin::DecoderFrameFormat {
        match self {
            Self::HostTimedSurface => {
                player_plugin::DecoderFrameFormat::Unknown("mediacodec_surface_texture".to_owned())
            }
            Self::SdkOwnedHardwareBuffer => {
                player_plugin::DecoderFrameFormat::Unknown("mediacodec_hardware_buffer".to_owned())
            }
        }
    }

    fn decoder_requirements(self, codec: impl Into<String>) -> DecoderSessionRequirements {
        match self {
            Self::HostTimedSurface => DecoderSessionRequirements {
                native_device_context_kind: Some(
                    DecoderNativeDeviceContextKind::AndroidNativeWindow,
                ),
                require_presentation_release: true,
                ..DecoderSessionRequirements::native_video(
                    codec,
                    DecoderNativeHandleKind::MediaCodecSurfaceTexture,
                    NativeFramePipelineProfile::MediaCodecSurfaceTexture,
                )
            },
            Self::SdkOwnedHardwareBuffer => DecoderSessionRequirements {
                native_device_context_kind: None,
                require_presentation_release: false,
                ..DecoderSessionRequirements::native_video(
                    codec,
                    DecoderNativeHandleKind::MediaCodecHardwareBuffer,
                    NativeFramePipelineProfile::MediaCodecHardwareBuffer,
                )
            },
        }
    }
}

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
        subtitle_details: Option<SubtitleErrorDetails>,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub enum AndroidHostCommand {
    Play,
    Pause,
    SeekTo {
        position_ms: u64,
    },
    Stop,
    SetPlaybackRate {
        rate: f32,
    },
    SetVideoTrackSelection {
        selection: MediaTrackSelection,
    },
    SetAudioTrackSelection {
        selection: MediaTrackSelection,
    },
    SetSubtitleTrackSelection {
        selection: MediaTrackSelection,
    },
    SetAbrPolicy {
        policy: MediaAbrPolicy,
        expected_catalog_revision: Option<u64>,
    },
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
    SeekTo {
        position: Duration,
    },
    Stop,
    SetPlaybackRate {
        rate: f32,
    },
    SetVideoTrackSelection {
        selection: MediaTrackSelection,
    },
    SetAudioTrackSelection {
        selection: MediaTrackSelection,
    },
    SetSubtitleTrackSelection {
        selection: MediaTrackSelection,
    },
    SetAbrPolicy {
        policy: MediaAbrPolicy,
        expected_catalog_revision: Option<u64>,
    },
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
    FirstFrameReady(FirstFrameReady),
    Error(PlayerError),
}

#[derive(Debug, Clone)]
pub struct AndroidManagedNativeSessionController {
    updates: Arc<Mutex<VecDeque<AndroidNativeSessionUpdate>>>,
    dropped_updates: Arc<AtomicU64>,
}

impl Default for AndroidManagedNativeSessionController {
    fn default() -> Self {
        Self {
            updates: Arc::new(Mutex::new(VecDeque::new())),
            dropped_updates: Arc::new(AtomicU64::new(0)),
        }
    }
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
    dropped_events: u64,
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
    fn take_dropped_event_count(&mut self) -> u64 {
        0
    }
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
    command_queue: MobileCommandQueue<AndroidNativePlayerCommand>,
    surface_attached: bool,
    extra_events: VecDeque<PlayerRuntimeEvent>,
    dropped_events: u64,
    pipeline_event_context: Option<PipelineEventContext>,
    // Keep the registry owner alive for as long as the playback session. The
    // hook adapters also retain their capability Arcs, but retaining the
    // registry here makes the native owner lifetime explicit at this boundary.
    _plugin_registry: Option<Arc<PluginRegistry>>,
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AndroidNativeFramePipelineOpenConfig {
    pub source_uri: String,
    pub source_normalizer: MobileSourceNormalizerConfiguration,
    pub native_frame_pipeline: MobileNativeFramePipelineConfiguration,
    pub avc_decoder_implementation_name: Option<String>,
    pub hevc_decoder_implementation_name: Option<String>,
    pub presenter_profile: AndroidNativeFramePresenterProfile,
}

pub struct AndroidNativeFramePipelinePacketSource {
    plugin_name: Option<String>,
    plugin_path: String,
    session: Box<dyn SourceNormalizerPacketSession>,
    stream_info: SourceNormalizerPacketStreamInfo,
    selected_video_stream_index: Option<u32>,
    pending_packet: Option<AndroidNativeFramePipelinePendingPacket>,
    closed: bool,
}

#[derive(Debug, Clone)]
struct AndroidNativeFramePipelinePendingPacket {
    packet: SourceNormalizerPacket,
    data: Vec<u8>,
}

pub(crate) trait AndroidNativeFrameDecoderSink: Send {
    fn send_packet(
        &mut self,
        packet: &DecoderPacket,
        data: &[u8],
    ) -> PlayerResult<DecoderPacketResult>;

    fn receive_native_frame(&mut self) -> PlayerResult<DecoderReceiveNativeFrameOutput>;

    fn release_native_frame(
        &mut self,
        frame: DecoderNativeFrame,
        presented: bool,
    ) -> PlayerResult<()>;

    fn flush(&mut self) -> PlayerResult<()>;

    fn close(&mut self) -> PlayerResult<()>;
}

struct AndroidNativeFrameDecoderSessionSink {
    plugin_name: Option<String>,
    plugin_path: String,
    session: Box<dyn NativeDecoderSession>,
    closed: bool,
}

#[derive(Clone)]
struct AndroidNativeFrameDecoderOpenPlan {
    plugin_name: Option<String>,
    plugin_path: PathBuf,
    factory: Arc<dyn NativeDecoderPluginFactory>,
    video_track: SourceNormalizerPacketTrackInfo,
    selected_profile: AndroidNativeFramePipelineProfile,
    required_decoder_implementation_name: String,
    requires_android_native_window: bool,
}

pub(crate) trait AndroidNativeFrameProcessorChain: Send {
    fn process_frame(
        &mut self,
        frame: DecoderNativeFrame,
        counters: &mut AndroidNativeFramePipelineCounters,
    ) -> PlayerResult<AndroidNativeFramePipelineProcessedFrame>;

    fn release_processor_outputs(
        &mut self,
        outputs: Vec<AndroidNativeFrameProcessorOwnedFrame>,
    ) -> Result<NativeFrameProcessorReleaseResult, NativeFrameProcessorReleaseError>;

    fn flush(&mut self) -> PlayerResult<()>;

    fn close(&mut self) -> PlayerResult<()>;
}

type AndroidNativeFramePipelineProcessedFrame = NativeFrameProcessorProcessedFrame;
type AndroidNativeFrameProcessorOwnedFrame = NativeFrameProcessorOwnedFrame;
pub type AndroidNativeFramePipelineFrame = NativeFramePipelineFrame;
pub type AndroidNativeFramePipelineFrameResult = NativeFramePipelineFrameResult;
pub type AndroidNativeFramePipelineFrameStatus = NativeFramePipelineFrameStatus;
pub type AndroidNativeFramePipelineCounters = NativeFramePipelineCounters;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AndroidNativeFramePresenterFrame {
    pub frame_handle: u64,
    pub frame: AndroidNativeFramePipelineFrame,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AndroidNativeFramePresenterSubmitResult {
    pub accepted: bool,
    pub requires_host_release: bool,
    pub message: Option<String>,
}

pub trait AndroidNativeFramePresenterSink: Send {
    fn submit_frame(
        &mut self,
        frame: &AndroidNativeFramePresenterFrame,
    ) -> PlayerResult<AndroidNativeFramePresenterSubmitResult>;

    fn decoder_device_context(&self) -> Option<DecoderNativeDeviceContext> {
        None
    }

    fn flush(&mut self) -> PlayerResult<()>;

    fn close(&mut self) -> PlayerResult<()>;
}

struct AndroidNativeFrameProcessorSessionChain {
    core: NativeFrameProcessorChainCore,
    closed: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum AndroidNativeFramePresenterProfile {
    SurfaceView,
    Surface,
    SurfaceTexture,
}

pub struct AndroidNativeFramePipelineSession {
    source_uri: String,
    source_kind: MediaSourceKind,
    source_protocol: MediaSourceProtocol,
    source_normalizer_mode: SourceNormalizerMode,
    decoder_plugin_library_paths: Vec<PathBuf>,
    frame_processor_plugin_library_paths: Vec<PathBuf>,
    max_in_flight_frames: u32,
    presenter_profile: AndroidNativeFramePresenterProfile,
    selected_pipeline_profile: AndroidNativeFramePipelineProfile,
    presenter_surface_profile: Option<AndroidNativeFramePresenterProfile>,
    decoder_open_plan_template: Option<AndroidNativeFrameDecoderOpenPlan>,
    decoder_open_plan: Option<AndroidNativeFrameDecoderOpenPlan>,
    core: NativeFramePipelineCore,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AndroidNativeFramePipelineOpenWire {
    pub handle: u64,
    pub route: &'static str,
    pub participation: &'static str,
    pub source_input: &'static str,
    pub decoder_adapter: &'static str,
    pub selected_profile: String,
    pub presenter_profile: &'static str,
    pub presenter_ready: bool,
    pub presenter_configured: bool,
    pub presenter_state: &'static str,
    pub surface_attached: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub surface_profile: Option<&'static str>,
    pub pipeline_profile: String,
    pub source_uri: String,
    pub source_kind: String,
    pub source_protocol: String,
    pub source_normalizer_mode: String,
    pub decoder_plugin_count: usize,
    pub frame_processor_count: usize,
    pub max_in_flight_frames: u32,
    pub counters: AndroidNativeFramePipelineCounters,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AndroidNativeFramePipelineStatusWire {
    pub handle: u64,
    pub route: &'static str,
    pub participation: &'static str,
    pub source_input: &'static str,
    pub decoder_adapter: &'static str,
    pub selected_profile: String,
    pub presenter_profile: &'static str,
    pub presenter_ready: bool,
    pub presenter_configured: bool,
    pub presenter_state: &'static str,
    pub surface_attached: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub surface_profile: Option<&'static str>,
    pub pipeline_profile: String,
    pub pending_frames: usize,
    pub end_of_stream: bool,
    pub counters: AndroidNativeFramePipelineCounters,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AndroidNativeFramePipelineFrameWire {
    pub status: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub handle: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub native_handle: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub presentation_time_us: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration_us: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub width: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub height: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub frame_id: Option<u64>,
    pub requires_host_release: bool,
    pub counters: AndroidNativeFramePipelineCounters,
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
    queue: MobileCommandQueue<AndroidNativePlayerCommand>,
}

impl AndroidHostCommandSink {
    fn new(queue: MobileCommandQueue<AndroidNativePlayerCommand>) -> Self {
        Self { queue }
    }
}

impl AndroidNativeCommandSink for AndroidHostCommandSink {
    fn submit_command(&mut self, command: AndroidNativePlayerCommand) -> PlayerResult<()> {
        self.queue.push(command)
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
                subtitle_details: error.subtitle_details().cloned(),
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
            AndroidNativePlayerCommand::SetAbrPolicy {
                policy,
                expected_catalog_revision,
            } => Self::SetAbrPolicy {
                policy: policy.clone(),
                expected_catalog_revision: *expected_catalog_revision,
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
        let command_queue = MobileCommandQueue::new("android native");
        let source = MediaSource::new(source_uri.clone());
        let media_info = placeholder_media_info(&source);
        let sink = AndroidHostCommandSink::new(command_queue.clone());
        let session = AndroidManagedNativeSession::new(source_uri, media_info, sink);

        Self {
            session,
            command_queue,
            surface_attached: false,
            extra_events: VecDeque::new(),
            dropped_events: 0,
            pipeline_event_context: None,
            _plugin_registry: None,
        }
    }

    /// Creates an Android host session from explicitly selected native hook
    /// references held by a host-owned plugin registry.
    pub fn new_with_plugin_registry(
        source_uri: impl Into<String>,
        registry: Arc<PluginRegistry>,
        references: impl IntoIterator<Item = player_plugin::PluginReference>,
    ) -> PlayerResult<Self> {
        let registrations = references
            .into_iter()
            .map(|reference| {
                let resolved =
                    registry
                        .resolve_pipeline_event_hook(&reference)
                        .map_err(|error| {
                            PlayerError::new(
                                player_runtime::PlayerErrorCode::Unsupported,
                                format!(
                                    "failed to resolve Android PipelineEventHook {}: {error}",
                                    reference.plugin_id()
                                ),
                            )
                        })?;
                PipelineEventHookRegistration::new(reference, resolved.capability())
            })
            .collect::<PlayerResult<Vec<_>>>()?;
        let mut session = Self::new_with_pipeline_event_hooks(source_uri, registrations)?;
        session._plugin_registry = Some(registry);
        Ok(session)
    }

    /// Creates an Android host session with structured playback event hooks.
    pub fn new_with_pipeline_event_dispatcher(
        source_uri: impl Into<String>,
        dispatcher: PipelineEventDispatcher,
        platform: impl Into<String>,
    ) -> PlayerResult<Self> {
        let source_uri = source_uri.into();
        let source = MediaSource::new(source_uri.clone());
        let mut session = Self::new(source_uri);
        session.pipeline_event_context = Some(PipelineEventContext::for_source(
            dispatcher, platform, &source,
        )?);
        Ok(session)
    }

    /// Creates an Android host session with resolved event-hook registrations.
    pub fn new_with_pipeline_event_hooks(
        source_uri: impl Into<String>,
        registrations: Vec<PipelineEventHookRegistration>,
    ) -> PlayerResult<Self> {
        Self::new_with_pipeline_event_dispatcher(
            source_uri,
            PipelineEventDispatcher::new(registrations),
            "android",
        )
    }

    pub fn snapshot(&mut self) -> AndroidHostSnapshot {
        AndroidHostSnapshot::from_player_snapshot(&self.session.snapshot())
    }

    pub fn sample_timeline(&self, snapshot: &AndroidExoPlaybackSnapshot) -> PlayerTimelineSnapshot {
        self.session.sample_timeline(snapshot)
    }

    pub fn drain_events(&mut self) -> Vec<AndroidHostEvent> {
        let mut events = self.extra_events.drain(..).collect::<Vec<_>>();
        events.extend(self.session.drain_events());
        let dropped = self.take_dropped_event_count();
        if let Some(context) = &self.pipeline_event_context {
            for event in &events {
                context.enqueue(event);
            }
            context.record_dropped_events(dropped.min(usize::MAX as u64) as usize);
        }
        events
            .iter()
            .filter_map(AndroidHostEvent::from_runtime_event)
            .collect()
    }

    pub fn drain_native_commands(&mut self) -> Vec<AndroidHostCommand> {
        self.command_queue
            .drain_map(|command| AndroidHostCommand::from_native_command(&command))
    }

    pub fn dispatch_command(
        &mut self,
        command: PlayerRuntimeCommand,
    ) -> PlayerResult<PlayerRuntimeCommandResult> {
        self.session.dispatch(command)
    }

    pub fn set_surface_attached(&mut self, attached: bool) {
        push_video_surface_event(
            &mut self.extra_events,
            &mut self.dropped_events,
            &mut self.surface_attached,
            attached,
        );
    }

    pub fn take_dropped_event_count(&mut self) -> u64 {
        let dropped = self
            .dropped_events
            .saturating_add(self.session.take_dropped_event_count());
        self.dropped_events = 0;
        dropped
    }

    /// Flushes accepted playback hook events before a lifecycle transition.
    pub fn flush_pipeline_event_hooks(&self, timeout: Duration) -> bool {
        self.pipeline_event_context
            .as_ref()
            .map(|context| context.flush(timeout))
            .unwrap_or(true)
    }

    /// Closes the playback hook worker. This operation is idempotent.
    pub fn close_pipeline_event_hooks(&self) -> bool {
        self.pipeline_event_context
            .as_ref()
            .map(PipelineEventContext::close)
            .unwrap_or(true)
    }

    /// Drains structured reports emitted by playback hooks.
    pub fn drain_pipeline_event_hook_reports(&self) -> PipelineEventHookReportBatch {
        self.pipeline_event_context
            .as_ref()
            .map(PipelineEventContext::drain_reports)
            .unwrap_or_default()
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

    /// Reports the first rendered frame observed by the native player.
    pub fn report_first_frame(&mut self, presentation_time: Duration, width: u32, height: u32) {
        self.session
            .controller()
            .report_first_frame(FirstFrameReady {
                presentation_time,
                width,
                height,
            });
    }

    pub fn report_error(&mut self, code: PlayerErrorCode, message: impl Into<String>) {
        self.session.controller().report_error(code, message);
    }

    pub fn report_player_error(&mut self, error: PlayerError) {
        self.session.controller().report_player_error(error);
    }
}

impl AndroidNativeFramePresenterProfile {
    pub const fn wire_name(self) -> &'static str {
        match self {
            Self::SurfaceView => "SurfaceView",
            Self::Surface => "Surface",
            Self::SurfaceTexture => "SurfaceTexture",
        }
    }
}

fn validate_native_frame_pipeline_open_config(
    config: &AndroidNativeFramePipelineOpenConfig,
) -> PlayerResult<()> {
    if !matches!(
        config.native_frame_pipeline.mode,
        NativeFramePipelineMode::PreferNativeFrame | NativeFramePipelineMode::RequireNativeFrame
    ) {
        return Err(PlayerError::new(
            PlayerErrorCode::InvalidArgument,
            "Android native-frame pipeline must be explicitly preferred or required",
        ));
    }
    config
        .source_normalizer
        .resolved_plugin_library_paths()
        .map_err(|message| PlayerError::new(PlayerErrorCode::InvalidArgument, message))?;
    config
        .native_frame_pipeline
        .resolved_decoder_plugin_library_paths()
        .map_err(|message| PlayerError::new(PlayerErrorCode::InvalidArgument, message))?;
    config
        .native_frame_pipeline
        .resolved_frame_processor_plugin_library_paths()
        .map_err(|message| PlayerError::new(PlayerErrorCode::InvalidArgument, message))?;
    if !config.source_normalizer.has_configured_plugins() {
        return Err(PlayerError::new(
            PlayerErrorCode::Unsupported,
            "Android native-frame pipeline requires a SourceNormalizer packet-stream plugin path",
        ));
    }
    if !config
        .native_frame_pipeline
        .has_configured_decoder_plugins()
    {
        return Err(PlayerError::new(
            PlayerErrorCode::Unsupported,
            "Android native-frame pipeline requires a MediaCodec decoder plugin path",
        ));
    }
    Ok(())
}

fn prepare_android_native_frame_decoder(
    config: &AndroidNativeFramePipelineOpenConfig,
    stream_info: &SourceNormalizerPacketStreamInfo,
) -> PlayerResult<AndroidNativeFrameDecoderOpenPlan> {
    let video_track = selected_video_track(stream_info).ok_or_else(|| {
        PlayerError::new(
            PlayerErrorCode::Unsupported,
            "Android native-frame pipeline requires a video stream from SourceNormalizer",
        )
    })?;
    if video_track.codec.trim().is_empty() {
        return Err(PlayerError::new(
            PlayerErrorCode::Unsupported,
            "Android native-frame pipeline SourceNormalizer video stream did not report a codec",
        ));
    }
    if let Some(reason) = hdr_programmable_processing_not_supported_reason(video_track) {
        return Err(PlayerError::new(PlayerErrorCode::Unsupported, reason));
    }
    let required_decoder_implementation_name =
        required_android_decoder_implementation_name(config, &video_track.codec)?;
    let request = DecoderPluginMatchRequest::video(video_track.codec.clone());
    if !config
        .native_frame_pipeline
        .decoder_plugin_artifacts
        .is_empty()
    {
        let mut last_rejection = None;
        for artifact in &config.native_frame_pipeline.decoder_plugin_artifacts {
            let native_artifact =
                NativePluginArtifact::new(artifact.reference.plugin_id(), &artifact.library_path)
                    .map_err(|error| {
                    PlayerError::new(
                        PlayerErrorCode::Unsupported,
                        format!("Android native-frame decoder reference is invalid: {error}"),
                    )
                })?;
            let registry =
                PluginRegistry::load_native_artifacts([native_artifact]).map_err(|error| {
                    PlayerError::new(
                        PlayerErrorCode::Unsupported,
                        format!(
                            "Android native-frame decoder artifact {} failed to load: {error}",
                            artifact.library_path.display()
                        ),
                    )
                })?;
            let resolved = registry
                .resolve_native_decoder(&artifact.reference)
                .map_err(|error| {
                    PlayerError::new(
                        PlayerErrorCode::Unsupported,
                        format!(
                            "Android native-frame decoder reference {} at {} could not be resolved: {error}",
                            artifact.reference.plugin_id(),
                            artifact.library_path.display()
                        ),
                    )
                })?;
            let factory = resolved.capability();
            if !factory
                .capabilities()
                .supports_codec(&request.codec, request.media_kind)
            {
                last_rejection = Some(PlayerError::new(
                    PlayerErrorCode::Unsupported,
                    format!(
                        "Android native-frame decoder `{}` does not support {:?} {}",
                        factory.name(),
                        request.media_kind,
                        request.codec
                    ),
                ));
                continue;
            }
            match android_native_frame_decoder_plan(
                factory,
                Some(factory_name(&resolved.capability())),
                artifact.library_path.clone(),
                video_track,
                required_decoder_implementation_name.clone(),
            ) {
                Ok(plan) => return Ok(plan),
                Err(error) => last_rejection = Some(error),
            }
        }
        return Err(last_rejection.unwrap_or_else(|| {
            PlayerError::new(
                PlayerErrorCode::Unsupported,
                format!(
                    "Android native-frame pipeline found no selected decoder for video codec {}",
                    request.codec
                ),
            )
        }));
    }
    let registry = PluginRegistry::inspect_decoder_support_development(
        &config.native_frame_pipeline.decoder_plugin_library_paths,
        request.clone(),
    );
    let record = registry.best_native_decoder_for(&request).ok_or_else(|| {
        PlayerError::new(
            PlayerErrorCode::Unsupported,
            format!(
                "Android native-frame pipeline found no native decoder plugin for video codec {}{}",
                request.codec,
                android_decoder_registry_notes(&registry)
            ),
        )
    })?;
    validate_android_native_decoder_record(record)?;
    let reference = registry.reference_for_record(record).ok_or_else(|| {
        PlayerError::new(
            PlayerErrorCode::Unsupported,
            format!(
                "Android native-frame decoder selection at {} has no plugin capability reference",
                record.path.display()
            ),
        )
    })?;
    let factory = registry
        .resolve_native_decoder(reference)
        .map_err(|error| {
            PlayerError::new(
                PlayerErrorCode::Unsupported,
                format!(
                    "Android native-frame decoder selection failed at {}: {error}",
                    record.path.display()
                ),
            )
        })?
        .capability();
    android_native_frame_decoder_plan(
        factory,
        record.plugin_name.clone(),
        record.path.clone(),
        video_track,
        required_decoder_implementation_name,
    )
}

fn factory_name(factory: &Arc<dyn NativeDecoderPluginFactory>) -> String {
    factory.name().to_owned()
}

fn android_native_frame_decoder_plan(
    factory: Arc<dyn NativeDecoderPluginFactory>,
    plugin_name: Option<String>,
    plugin_path: PathBuf,
    video_track: &SourceNormalizerPacketTrackInfo,
    required_decoder_implementation_name: String,
) -> PlayerResult<AndroidNativeFrameDecoderOpenPlan> {
    let capabilities = factory.capabilities();
    if !capabilities.supports_hardware_decode {
        return Err(PlayerError::new(
            PlayerErrorCode::Unsupported,
            format!(
                "Android native-frame decoder `{}` does not advertise hardware decode",
                factory.name()
            ),
        ));
    }
    if !capabilities.supports_gpu_handles {
        return Err(PlayerError::new(
            PlayerErrorCode::Unsupported,
            format!(
                "Android native-frame decoder `{}` does not advertise native GPU handles",
                factory.name()
            ),
        ));
    }
    let selected_profile = select_android_native_frame_pipeline_profile(video_track);
    let requirements_for_session = selected_profile.decoder_requirements(video_track.codec.clone());
    let missing_capabilities = requirements_for_session
        .missing_capabilities(&capabilities, &factory.native_requirements());
    if !missing_capabilities.is_empty() {
        return Err(PlayerError::new(
            PlayerErrorCode::Unsupported,
            format!(
                "Android native-frame decoder `{}` does not satisfy session requirements: missing {}",
                factory.name(),
                missing_capabilities.join(", ")
            ),
        ));
    }
    let requirements = factory.native_requirements();
    validate_android_native_decoder_requirements(
        plugin_name.as_deref().unwrap_or_else(|| factory.name()),
        &requirements,
    )?;
    Ok(AndroidNativeFrameDecoderOpenPlan {
        plugin_name: plugin_name.or_else(|| Some(factory.name().to_owned())),
        plugin_path,
        factory,
        video_track: video_track.clone(),
        selected_profile,
        required_decoder_implementation_name,
        requires_android_native_window: requirements.requires_native_device_context
            || requirements
                .required_device_context_kinds
                .contains(&DecoderNativeDeviceContextKind::AndroidNativeWindow),
    })
}

fn required_android_decoder_implementation_name(
    config: &AndroidNativeFramePipelineOpenConfig,
    codec: &str,
) -> PlayerResult<String> {
    let normalized = normalize_decoder_codec_identifier(codec);
    let selected = match normalized.as_str() {
        "h264" | "avc" | "avc1" | "avc3" => config.avc_decoder_implementation_name.as_deref(),
        "hevc" | "h265" | "hvc1" | "hev1" => config.hevc_decoder_implementation_name.as_deref(),
        _ => None,
    };
    selected
        .filter(|name| !name.trim().is_empty())
        .map(str::to_owned)
        .ok_or_else(|| {
            PlayerError::new(
                PlayerErrorCode::Unsupported,
                format!(
                    "Android native-frame pipeline found no host-selected hardware decoder implementation for codec {codec}"
                ),
            )
        })
}

fn open_android_native_frame_decoder_session(
    factory: &dyn NativeDecoderPluginFactory,
    video_track: &SourceNormalizerPacketTrackInfo,
    required_decoder_implementation_name: &str,
    native_device_context: Option<DecoderNativeDeviceContext>,
) -> PlayerResult<Box<dyn NativeDecoderSession>> {
    let config = DecoderSessionConfig {
        codec: video_track.codec.clone(),
        media_kind: DecoderMediaKind::Video,
        extradata: video_track.extradata.clone(),
        bitstream_format: video_track.bitstream_format.clone(),
        width: video_track.width,
        height: video_track.height,
        coded_width: video_track.coded_width,
        coded_height: video_track.coded_height,
        reorder_depth: video_track.reorder_depth,
        prefer_hardware: true,
        require_cpu_output: false,
        required_decoder_implementation_name: Some(required_decoder_implementation_name.to_owned()),
        native_device_context,
        color: video_track.color.clone(),
        hdr: video_track.hdr.clone(),
        ..DecoderSessionConfig::default()
    };
    factory.open_native_session(&config).map_err(|error| {
        PlayerError::new(
            PlayerErrorCode::Unsupported,
            format!(
                "Android native-frame decoder `{}` open_native_session failed: {error}",
                factory.name()
            ),
        )
    })
}

fn open_android_native_frame_processor_chain(
    configuration: &MobileNativeFramePipelineConfiguration,
    stream_info: &SourceNormalizerPacketStreamInfo,
    max_in_flight_frames: u32,
    selected_profile: AndroidNativeFramePipelineProfile,
) -> PlayerResult<Option<Box<dyn AndroidNativeFrameProcessorChain>>> {
    if !configuration.has_configured_frame_processors() {
        return Ok(None);
    }
    let video_track = selected_video_track(stream_info).ok_or_else(|| {
        PlayerError::new(
            PlayerErrorCode::Unsupported,
            "Android native-frame processor chain requires a video stream from SourceNormalizer",
        )
    })?;
    let mode = FrameProcessorMode::PreferProcessed;
    let policy = FrameProcessorPolicy {
        max_in_flight_frames_per_processor: max_in_flight_frames.max(1),
        ..FrameProcessorPolicy::default()
    };
    let input_metadata = android_frame_processor_input_metadata(video_track, selected_profile);
    let mut nodes = Vec::new();
    let bindings = if configuration.frame_processor_plugin_artifacts.is_empty() {
        configuration
            .frame_processor_plugin_library_paths
            .iter()
            .map(|path| (path, None))
            .collect::<Vec<_>>()
    } else {
        configuration
            .frame_processor_plugin_artifacts
            .iter()
            .map(|artifact| (&artifact.library_path, Some(&artifact.reference)))
            .collect::<Vec<_>>()
    };
    for (processor_index, (path, requested_reference)) in bindings
        .into_iter()
        .enumerate()
        .take(policy.max_chain_depth)
    {
        let registry = match requested_reference {
            Some(reference) => {
                let artifact =
                    NativePluginArtifact::new(reference.plugin_id(), path).map_err(|error| {
                        PlayerError::new(
                            PlayerErrorCode::Unsupported,
                            format!("Android frame processor reference is invalid: {error}"),
                        )
                    })?;
                PluginRegistry::load_native_artifacts([artifact])
            }
            None => PluginRegistry::load_native_development([path]),
        }
        .map_err(|error| {
            PlayerError::new(
                PlayerErrorCode::Unsupported,
                format!(
                    "Android native-frame processor plugin load failed at {}: {error}",
                    path.display()
                ),
            )
        })?;
        let implicit_references;
        let reference = match requested_reference {
            Some(reference) => reference,
            None => {
                implicit_references = registry.frame_processor_references().map_err(|error| {
                    PlayerError::new(
                        PlayerErrorCode::Unsupported,
                        format!(
                            "Android native-frame processor selection failed at {}: {error}",
                            path.display()
                        ),
                    )
                })?;
                match implicit_references.as_slice() {
                    [reference] => reference,
                    [] => {
                        return Err(PlayerError::new(
                            PlayerErrorCode::Unsupported,
                            format!(
                                "Android native-frame processor artifact {} does not expose FrameProcessor",
                                path.display()
                            ),
                        ));
                    }
                    _ => {
                        return Err(PlayerError::new(
                            PlayerErrorCode::InvalidArgument,
                            format!(
                                "Android native-frame processor artifact {} exposes {} FrameProcessor instances; an explicit PluginReference is required",
                                path.display(),
                                implicit_references.len()
                            ),
                        ));
                    }
                }
            }
        };
        let factory = registry
            .resolve_frame_processor(reference)
            .map_err(|error| {
                PlayerError::new(
                    PlayerErrorCode::Unsupported,
                    format!(
                        "Android native-frame processor selection failed at {}: {error}",
                        path.display()
                    ),
                )
            })?
            .capability();
        let capabilities = factory.capabilities();
        validate_android_frame_processor_capabilities(
            factory.name(),
            &capabilities,
            &input_metadata,
        )?;
        let session = factory
            .open_session(&FrameProcessorSessionConfig {
                processor_index,
                input_metadata: input_metadata.clone(),
                max_in_flight_frames: Some(policy.max_in_flight_frames_per_processor),
            })
            .map_err(|error| {
                PlayerError::new(
                    PlayerErrorCode::Unsupported,
                    format!(
                        "Android native-frame processor `{}` open_session failed: {error}",
                        factory.name()
                    ),
                )
            })?;
        nodes.push(NativeFrameProcessorNode::new(
            factory.name(),
            processor_index,
            session,
        ));
    }
    if nodes.is_empty() {
        Ok(None)
    } else {
        Ok(Some(Box::new(AndroidNativeFrameProcessorSessionChain {
            core: NativeFrameProcessorChainCore::new(nodes, mode, policy),
            closed: false,
        })))
    }
}

fn android_frame_processor_input_metadata(
    video_track: &SourceNormalizerPacketTrackInfo,
    selected_profile: AndroidNativeFramePipelineProfile,
) -> NativeFrameMetadata {
    NativeFrameMetadata {
        media_kind: DecoderMediaKind::Video,
        // Android MediaCodec native outputs are opaque at session-open time.
        // Capability gating must use the selected handle/profile rather than a
        // pretend CPU-readable pixel format.
        format: selected_profile.processor_format(),
        codec: video_track.codec.clone(),
        pts_us: None,
        duration_us: None,
        width: video_track.width.unwrap_or(0),
        height: video_track.height.unwrap_or(0),
        coded_width: video_track.coded_width.or(video_track.width),
        coded_height: video_track.coded_height.or(video_track.height),
        visible_rect: None,
        handle_kind: selected_profile.processor_handle_kind(),
        pipeline_profile: Some(selected_profile.pipeline_profile()),
        color_space: video_track
            .color
            .as_ref()
            .and_then(|color| color.primaries.clone()),
        hdr_metadata: video_track.hdr.as_ref().map(|hdr| hdr.kind.clone()),
        color: video_track.color.clone(),
        hdr: video_track.hdr.clone(),
        sync_info: None,
        transform: None,
        frame_id: None,
        release_tracking: None,
    }
}

fn validate_android_frame_processor_capabilities(
    processor_name: &str,
    capabilities: &FrameProcessorCapabilities,
    input_metadata: &NativeFrameMetadata,
) -> PlayerResult<()> {
    let requirements = FrameProcessorSessionRequirements {
        output_handle_kind: Some(input_metadata.handle_kind.clone()),
        output_pipeline_profile: Some(input_metadata.effective_pipeline_profile()),
        require_explicit_native_input: true,
        require_flush: false,
        ..FrameProcessorSessionRequirements::native_video(input_metadata.clone())
    };
    let missing = requirements.missing_capabilities(capabilities);
    if !missing.is_empty() {
        return Err(PlayerError::new(
            PlayerErrorCode::Unsupported,
            format!(
                "Android frame processor `{processor_name}` does not satisfy session requirements: missing {}",
                missing.join(", ")
            ),
        ));
    }
    Ok(())
}

fn validate_android_native_decoder_record(
    record: &player_plugin_loader::PluginDiagnosticRecord,
) -> PlayerResult<()> {
    let requirements = match record.capability_summary.as_ref() {
        Some(PluginCapabilitySummary::Decoder(summary)) => summary.native_requirements.as_ref(),
        _ => None,
    };
    if let Some(requirements) = requirements {
        validate_android_native_decoder_requirements(
            record
                .plugin_name
                .as_deref()
                .unwrap_or_else(|| record.path.to_str().unwrap_or("decoder plugin")),
            requirements,
        )?;
    }
    Ok(())
}

fn select_android_native_frame_pipeline_profile(
    _video_track: &SourceNormalizerPacketTrackInfo,
) -> AndroidNativeFramePipelineProfile {
    AndroidNativeFramePipelineProfile::HostTimedSurface
}

fn validate_android_native_decoder_requirements(
    decoder_name: &str,
    requirements: &player_plugin::DecoderNativeRequirements,
) -> PlayerResult<()> {
    let unsupported_contexts: Vec<String> = requirements
        .required_device_context_kinds
        .iter()
        .filter(|kind| **kind != DecoderNativeDeviceContextKind::AndroidNativeWindow)
        .map(|kind| format!("{kind:?}"))
        .collect();
    if !unsupported_contexts.is_empty() {
        return Err(PlayerError::new(
            PlayerErrorCode::Unsupported,
            format!(
                "Android native-frame decoder plugin {decoder_name} requires unsupported native device context(s): {}",
                unsupported_contexts.join(", ")
            ),
        ));
    }
    Ok(())
}

fn selected_video_track(
    stream_info: &SourceNormalizerPacketStreamInfo,
) -> Option<&SourceNormalizerPacketTrackInfo> {
    stream_info
        .selected_track_index
        .and_then(|selected| {
            stream_info.tracks.iter().find(|track| {
                track.media_kind == SourceNormalizerPacketMediaKind::Video
                    && track.stream_index == selected
            })
        })
        .or_else(|| {
            stream_info
                .tracks
                .iter()
                .find(|track| track.media_kind == SourceNormalizerPacketMediaKind::Video)
        })
}

fn android_decoder_registry_notes(registry: &PluginRegistry) -> String {
    let notes = registry.diagnostic_notes();
    if notes.is_empty() {
        String::new()
    } else {
        format!("; {}", notes.join("; "))
    }
}

impl AndroidNativeFramePipelinePacketSource {
    pub fn new(
        plugin_name: Option<String>,
        plugin_path: String,
        session: Box<dyn SourceNormalizerPacketSession>,
    ) -> Self {
        let stream_info = session.stream_info();
        let selected_video_stream_index =
            selected_video_track(&stream_info).map(|track| track.stream_index);
        Self {
            plugin_name,
            plugin_path,
            session,
            stream_info,
            selected_video_stream_index,
            pending_packet: None,
            closed: false,
        }
    }

    pub fn plugin_name(&self) -> Option<&str> {
        self.plugin_name.as_deref()
    }

    pub fn plugin_path(&self) -> &str {
        &self.plugin_path
    }

    pub fn has_pending_packet(&self) -> bool {
        self.pending_packet.is_some()
    }

    pub fn pending_packet_data_len(&self) -> Option<usize> {
        self.pending_packet
            .as_ref()
            .map(|pending| pending.data.len())
    }

    pub fn pending_packet_stream_index(&self) -> Option<u32> {
        self.pending_packet
            .as_ref()
            .map(|pending| pending.packet.stream_index)
    }

    fn stream_info(&self) -> &SourceNormalizerPacketStreamInfo {
        &self.stream_info
    }

    fn clear_pending_packet(&mut self) {
        self.pending_packet = None;
    }

    fn flush(&mut self) -> PlayerResult<()> {
        self.clear_pending_packet();
        self.session.flush().map(|_| ()).map_err(|error| {
            PlayerError::new(
                PlayerErrorCode::DecodeFailure,
                format!("source normalizer flush failed: {error}"),
            )
        })
    }

    fn seek(&mut self, position: Duration) -> PlayerResult<()> {
        self.clear_pending_packet();
        self.session
            .seek(&SourceNormalizerPacketSeek {
                position_millis: duration_to_millis(position),
                exact: false,
            })
            .map(|_| ())
            .map_err(|error| {
                PlayerError::new(
                    PlayerErrorCode::DecodeFailure,
                    format!("source normalizer seek failed: {error}"),
                )
            })
    }

    fn close(&mut self) -> PlayerResult<()> {
        if self.closed {
            return Ok(());
        }
        self.clear_pending_packet();
        self.session.close().map_err(|error| {
            PlayerError::new(
                PlayerErrorCode::DecodeFailure,
                format!("source normalizer close failed: {error}"),
            )
        })?;
        self.closed = true;
        Ok(())
    }

    fn release_packet(&mut self, handle: usize) -> PlayerResult<()> {
        self.session.release_packet(handle).map_err(|error| {
            PlayerError::new(
                PlayerErrorCode::DecodeFailure,
                format!("source normalizer release_packet failed: {error}"),
            )
        })
    }
}

impl std::fmt::Debug for AndroidNativeFramePipelinePacketSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AndroidNativeFramePipelinePacketSource")
            .field("plugin_name", &self.plugin_name)
            .field("plugin_path", &self.plugin_path)
            .field(
                "selected_video_stream_index",
                &self.selected_video_stream_index,
            )
            .field("has_pending_packet", &self.pending_packet.is_some())
            .field("closed", &self.closed)
            .finish()
    }
}

impl Drop for AndroidNativeFramePipelinePacketSource {
    fn drop(&mut self) {
        if let Err(error) = self.close() {
            tracing::warn!(error = %error, "Android native-frame packet source close during drop failed");
        }
    }
}

impl NativeFramePacketSourceAdapter for AndroidNativeFramePipelinePacketSource {
    fn selected_video_stream_index(&self) -> Option<u32> {
        self.selected_video_stream_index
    }

    fn read_packet(&mut self) -> Result<NativeFramePacketRead, NativeFramePipelineError> {
        loop {
            let lease = self.session.read_packet().map_err(|error| {
                NativeFramePipelineError::new(
                    "readPacket",
                    format!("source normalizer read_packet failed: {error}"),
                )
            })?;
            match lease.metadata.status {
                SourceNormalizerReadPacketStatus::NeedMoreData => {
                    return Ok(NativeFramePacketRead::NeedMoreData {
                        message: lease.metadata.message,
                    });
                }
                SourceNormalizerReadPacketStatus::EndOfStream => {
                    return Ok(NativeFramePacketRead::EndOfStream {
                        message: lease.metadata.message,
                    });
                }
                SourceNormalizerReadPacketStatus::Packet => {}
            }

            let Some(packet) = lease.metadata.packet.clone() else {
                let handle = lease.handle;
                drop(lease);
                self.release_packet(handle)
                    .map_err(player_error_to_pipeline_error)?;
                continue;
            };
            let data = lease.data.to_vec();
            let handle = lease.handle;
            drop(lease);
            self.release_packet(handle)
                .map_err(player_error_to_pipeline_error)?;
            return Ok(NativeFramePacketRead::Packet {
                packet,
                data,
                message: None,
            });
        }
    }

    fn flush(&mut self) -> Result<(), NativeFramePipelineError> {
        AndroidNativeFramePipelinePacketSource::flush(self).map_err(player_error_to_pipeline_error)
    }

    fn seek(&mut self, position: Duration) -> Result<(), NativeFramePipelineError> {
        AndroidNativeFramePipelinePacketSource::seek(self, position)
            .map_err(player_error_to_pipeline_error)
    }

    fn close(&mut self) -> Result<(), NativeFramePipelineError> {
        AndroidNativeFramePipelinePacketSource::close(self).map_err(player_error_to_pipeline_error)
    }
}

impl AndroidNativeFrameDecoderSessionSink {
    fn new(
        plugin_name: Option<String>,
        plugin_path: String,
        session: Box<dyn NativeDecoderSession>,
    ) -> Self {
        Self {
            plugin_name,
            plugin_path,
            session,
            closed: false,
        }
    }
}

impl AndroidNativeFrameDecoderOpenPlan {
    fn open(
        &self,
        native_device_context: Option<DecoderNativeDeviceContext>,
    ) -> PlayerResult<Box<dyn AndroidNativeFrameDecoderSink>> {
        let plugin_name = self.plugin_name.clone();
        let plugin_path = self.plugin_path.display().to_string();
        let session = open_android_native_frame_decoder_session(
            &*self.factory,
            &self.video_track,
            &self.required_decoder_implementation_name,
            native_device_context,
        )?;
        Ok(Box::new(AndroidNativeFrameDecoderSessionSink::new(
            plugin_name,
            plugin_path,
            session,
        )))
    }
}

impl AndroidNativeFrameDecoderSink for AndroidNativeFrameDecoderSessionSink {
    fn send_packet(
        &mut self,
        packet: &DecoderPacket,
        data: &[u8],
    ) -> PlayerResult<DecoderPacketResult> {
        self.session.send_packet(packet, data).map_err(|error| {
            PlayerError::new(
                PlayerErrorCode::DecodeFailure,
                format!(
                    "Android native-frame decoder `{}` send_packet failed: {error}",
                    self.plugin_name.as_deref().unwrap_or(&self.plugin_path)
                ),
            )
        })
    }

    fn receive_native_frame(&mut self) -> PlayerResult<DecoderReceiveNativeFrameOutput> {
        self.session.receive_native_frame().map_err(|error| {
            PlayerError::new(
                PlayerErrorCode::DecodeFailure,
                format!(
                    "Android native-frame decoder `{}` receive_native_frame failed: {error}",
                    self.plugin_name.as_deref().unwrap_or(&self.plugin_path)
                ),
            )
        })
    }

    fn release_native_frame(
        &mut self,
        frame: DecoderNativeFrame,
        presented: bool,
    ) -> PlayerResult<()> {
        self.session
            .release_native_frame_with_presentation(frame, presented)
            .map_err(|error| {
                PlayerError::new(
                    PlayerErrorCode::DecodeFailure,
                    format!(
                        "Android native-frame decoder `{}` release_native_frame failed: {error}",
                        self.plugin_name.as_deref().unwrap_or(&self.plugin_path)
                    ),
                )
            })
    }

    fn flush(&mut self) -> PlayerResult<()> {
        self.session.flush().map_err(|error| {
            PlayerError::new(
                PlayerErrorCode::DecodeFailure,
                format!(
                    "Android native-frame decoder `{}` flush failed: {error}",
                    self.plugin_name.as_deref().unwrap_or(&self.plugin_path)
                ),
            )
        })
    }

    fn close(&mut self) -> PlayerResult<()> {
        if self.closed {
            return Ok(());
        }
        self.session.close().map_err(|error| {
            PlayerError::new(
                PlayerErrorCode::DecodeFailure,
                format!(
                    "Android native-frame decoder `{}` close failed: {error}",
                    self.plugin_name.as_deref().unwrap_or(&self.plugin_path)
                ),
            )
        })?;
        self.closed = true;
        Ok(())
    }
}

impl std::fmt::Debug for AndroidNativeFrameDecoderSessionSink {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AndroidNativeFrameDecoderSessionSink")
            .field("plugin_name", &self.plugin_name)
            .field("plugin_path", &self.plugin_path)
            .field("session_info", &self.session.session_info())
            .field("closed", &self.closed)
            .finish()
    }
}

impl Drop for AndroidNativeFrameDecoderSessionSink {
    fn drop(&mut self) {
        if let Err(error) = AndroidNativeFrameDecoderSink::close(self) {
            tracing::warn!(error = %error, "Android native-frame decoder close during drop failed");
        }
    }
}

impl NativeFrameDecoderAdapter for AndroidNativeFrameDecoderSessionSink {
    fn send_packet(
        &mut self,
        packet: &DecoderPacket,
        data: &[u8],
    ) -> Result<DecoderPacketResult, NativeFramePipelineError> {
        AndroidNativeFrameDecoderSink::send_packet(self, packet, data)
            .map_err(player_error_to_pipeline_error)
    }

    fn receive_native_frame(
        &mut self,
    ) -> Result<DecoderReceiveNativeFrameOutput, NativeFramePipelineError> {
        AndroidNativeFrameDecoderSink::receive_native_frame(self)
            .map_err(player_error_to_pipeline_error)
    }

    fn release_native_frame(
        &mut self,
        frame: DecoderNativeFrame,
        presented: bool,
    ) -> Result<(), NativeFramePipelineError> {
        AndroidNativeFrameDecoderSink::release_native_frame(self, frame, presented)
            .map_err(player_error_to_pipeline_error)
    }

    fn flush(&mut self) -> Result<(), NativeFramePipelineError> {
        AndroidNativeFrameDecoderSink::flush(self).map_err(player_error_to_pipeline_error)
    }

    fn close(&mut self) -> Result<(), NativeFramePipelineError> {
        AndroidNativeFrameDecoderSink::close(self).map_err(player_error_to_pipeline_error)
    }
}

impl AndroidNativeFrameProcessorChain for AndroidNativeFrameProcessorSessionChain {
    fn process_frame(
        &mut self,
        frame: DecoderNativeFrame,
        counters: &mut AndroidNativeFramePipelineCounters,
    ) -> PlayerResult<AndroidNativeFramePipelineProcessedFrame> {
        let before = self.core.metrics().clone();
        let processed = self
            .core
            .process(frame, &mut NoopNativeFrameProcessorObserver)
            .map_err(|error| android_frame_processor_error(error.error))?;
        let after = self.core.metrics();
        counters.processed_frames = counters.processed_frames.saturating_add(
            after
                .processed_frame_count
                .saturating_sub(before.processed_frame_count),
        );
        counters.deadline_misses = counters.deadline_misses.saturating_add(
            after
                .deadline_miss_count
                .saturating_sub(before.deadline_miss_count),
        );
        counters.late_dropped = counters.late_dropped.saturating_add(
            after
                .late_output_drop_count
                .saturating_sub(before.late_output_drop_count),
        );
        counters.backpressure_count = counters.backpressure_count.saturating_add(
            after
                .backpressure_count
                .saturating_sub(before.backpressure_count),
        );
        Ok(processed)
    }

    fn release_processor_outputs(
        &mut self,
        outputs: Vec<AndroidNativeFrameProcessorOwnedFrame>,
    ) -> Result<NativeFrameProcessorReleaseResult, NativeFrameProcessorReleaseError> {
        self.core.release_processor_outputs_tracked(outputs)
    }

    fn flush(&mut self) -> PlayerResult<()> {
        self.core.flush().map_err(android_frame_processor_error)
    }

    fn close(&mut self) -> PlayerResult<()> {
        if self.closed {
            return Ok(());
        }
        self.core.close().map_err(android_frame_processor_error)?;
        self.closed = true;
        Ok(())
    }
}

fn android_frame_processor_error(error: NativeFrameProcessorError) -> PlayerError {
    PlayerError::new(PlayerErrorCode::DecodeFailure, error.to_string())
}

impl Drop for AndroidNativeFrameProcessorSessionChain {
    fn drop(&mut self) {
        if let Err(error) = self.close() {
            tracing::warn!(error = %error, "Android native-frame processor chain close during drop failed");
        }
    }
}

struct AndroidDecoderAdapter {
    inner: Box<dyn AndroidNativeFrameDecoderSink>,
}

impl NativeFrameDecoderAdapter for AndroidDecoderAdapter {
    fn send_packet(
        &mut self,
        packet: &DecoderPacket,
        data: &[u8],
    ) -> Result<DecoderPacketResult, NativeFramePipelineError> {
        self.inner
            .send_packet(packet, data)
            .map_err(player_error_to_pipeline_error)
    }

    fn receive_native_frame(
        &mut self,
    ) -> Result<DecoderReceiveNativeFrameOutput, NativeFramePipelineError> {
        self.inner
            .receive_native_frame()
            .map_err(player_error_to_pipeline_error)
    }

    fn release_native_frame(
        &mut self,
        frame: DecoderNativeFrame,
        presented: bool,
    ) -> Result<(), NativeFramePipelineError> {
        self.inner
            .release_native_frame(frame, presented)
            .map_err(player_error_to_pipeline_error)
    }

    fn flush(&mut self) -> Result<(), NativeFramePipelineError> {
        self.inner.flush().map_err(player_error_to_pipeline_error)
    }

    fn close(&mut self) -> Result<(), NativeFramePipelineError> {
        self.inner.close().map_err(player_error_to_pipeline_error)
    }
}

struct AndroidPresenterAdapter {
    inner: Box<dyn AndroidNativeFramePresenterSink>,
}

impl NativeFramePresenterAdapter for AndroidPresenterAdapter {
    fn submit_frame(
        &mut self,
        frame: &NativeFramePresenterFrame,
    ) -> Result<NativeFramePresenterSubmitResult, NativeFramePipelineError> {
        let android_frame = AndroidNativeFramePresenterFrame {
            frame_handle: frame.frame_handle,
            frame: frame.frame.clone(),
        };
        self.inner
            .submit_frame(&android_frame)
            .map(|result| NativeFramePresenterSubmitResult {
                accepted: result.accepted,
                requires_host_release: result.requires_host_release,
                message: result.message,
            })
            .map_err(player_error_to_pipeline_error)
    }

    fn decoder_device_context(&self) -> Option<DecoderNativeDeviceContext> {
        self.inner.decoder_device_context()
    }

    fn flush(&mut self) -> Result<(), NativeFramePipelineError> {
        self.inner.flush().map_err(player_error_to_pipeline_error)
    }

    fn close(&mut self) -> Result<(), NativeFramePipelineError> {
        self.inner.close().map_err(player_error_to_pipeline_error)
    }
}

struct AndroidProcessorChainAdapter {
    inner: Box<dyn AndroidNativeFrameProcessorChain>,
}

impl NativeFrameProcessorPipelineAdapter for AndroidProcessorChainAdapter {
    fn process_frame(
        &mut self,
        frame: DecoderNativeFrame,
    ) -> Result<
        (
            NativeFrameProcessorProcessedFrame,
            NativeFrameProcessorMetricsDelta,
        ),
        NativeFramePipelineError,
    > {
        let mut counters = AndroidNativeFramePipelineCounters::default();
        let processed = self
            .inner
            .process_frame(frame, &mut counters)
            .map_err(player_error_to_pipeline_error)?;
        Ok((
            processed,
            NativeFrameProcessorMetricsDelta {
                processed_frames: counters.processed_frames,
                deadline_misses: counters.deadline_misses,
                late_dropped: counters.late_dropped,
                backpressure_count: counters.backpressure_count,
            },
        ))
    }

    fn release_processor_outputs(
        &mut self,
        outputs: Vec<NativeFrameProcessorOwnedFrame>,
    ) -> Result<NativeFrameProcessorReleaseResult, NativeFrameProcessorReleaseError> {
        self.inner.release_processor_outputs(outputs)
    }

    fn flush(&mut self) -> Result<(), NativeFramePipelineError> {
        self.inner.flush().map_err(player_error_to_pipeline_error)
    }

    fn close(&mut self) -> Result<(), NativeFramePipelineError> {
        self.inner.close().map_err(player_error_to_pipeline_error)
    }
}

fn android_processor_chain_to_shared_core(
    chain: Box<dyn AndroidNativeFrameProcessorChain>,
) -> Box<dyn NativeFrameProcessorPipelineAdapter> {
    Box::new(AndroidProcessorChainAdapter { inner: chain })
}

fn player_error_to_pipeline_error(error: PlayerError) -> NativeFramePipelineError {
    NativeFramePipelineError::new("androidNativeFrame", error.message().to_owned())
}

fn pipeline_error_to_player_error(error: NativeFramePipelineError) -> PlayerError {
    PlayerError::new(PlayerErrorCode::DecodeFailure, error.to_string())
}

impl std::fmt::Debug for AndroidNativeFramePipelineSession {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AndroidNativeFramePipelineSession")
            .field("source_uri", &self.source_uri)
            .field("source_kind", &self.source_kind)
            .field("source_protocol", &self.source_protocol)
            .field("source_normalizer_mode", &self.source_normalizer_mode)
            .field(
                "decoder_plugin_library_paths",
                &self.decoder_plugin_library_paths,
            )
            .field(
                "frame_processor_plugin_library_paths",
                &self.frame_processor_plugin_library_paths,
            )
            .field("max_in_flight_frames", &self.max_in_flight_frames)
            .field("presenter_profile", &self.presenter_profile)
            .field("presenter_surface_profile", &self.presenter_surface_profile)
            .field(
                "presenter_surface_attached",
                &self.core.output_target_attached(),
            )
            .field("has_presenter_sink", &self.core.has_presenter())
            .field("has_packet_source", &self.core.has_packet_source())
            .field(
                "has_decoder_open_plan_template",
                &self.decoder_open_plan_template.is_some(),
            )
            .field("has_decoder_open_plan", &self.decoder_open_plan.is_some())
            .field("has_decoder_sink", &self.core.has_decoder())
            .field("pending_frames", &self.core.pending_frame_count())
            .field("end_of_stream", &self.core.end_of_stream())
            .field("counters", self.core.counters())
            .finish()
    }
}

impl AndroidNativeFramePipelineSession {
    pub fn open(config: AndroidNativeFramePipelineOpenConfig) -> PlayerResult<Self> {
        validate_native_frame_pipeline_open_config(&config)?;
        let source = MediaSource::new(config.source_uri.clone());
        let packet_open =
            open_mobile_source_normalizer_packet_session(&source, &config.source_normalizer)
                .map_err(|message| {
                    PlayerError::new(
                        PlayerErrorCode::Unsupported,
                        format!("Android native-frame packet source open failed: {message}"),
                    )
                })?;
        let packet_source = AndroidNativeFramePipelinePacketSource::new(
            packet_open.plugin_name,
            packet_open.plugin_path,
            packet_open.session,
        );
        let decoder_open_plan =
            prepare_android_native_frame_decoder(&config, packet_source.stream_info())?;
        let processor_chain = open_android_native_frame_processor_chain(
            &config.native_frame_pipeline,
            packet_source.stream_info(),
            config
                .native_frame_pipeline
                .max_in_flight_frames
                .unwrap_or(3)
                .max(1),
            decoder_open_plan.selected_profile,
        )?;
        Self::open_with_components(
            config,
            source,
            Some(packet_source),
            Some(decoder_open_plan),
            None,
            processor_chain,
        )
    }

    pub fn open_with_packet_source(
        config: AndroidNativeFramePipelineOpenConfig,
        source: MediaSource,
        packet_source: Option<AndroidNativeFramePipelinePacketSource>,
    ) -> PlayerResult<Self> {
        let (decoder_open_plan, processor_chain) = match packet_source.as_ref() {
            Some(packet_source) => {
                let decoder_open_plan =
                    prepare_android_native_frame_decoder(&config, packet_source.stream_info())?;
                let processor_chain = open_android_native_frame_processor_chain(
                    &config.native_frame_pipeline,
                    packet_source.stream_info(),
                    config
                        .native_frame_pipeline
                        .max_in_flight_frames
                        .unwrap_or(3)
                        .max(1),
                    decoder_open_plan.selected_profile,
                )?;
                (Some(decoder_open_plan), processor_chain)
            }
            None => (None, None),
        };
        Self::open_with_components(
            config,
            source,
            packet_source,
            decoder_open_plan,
            None,
            processor_chain,
        )
    }

    #[cfg(test)]
    pub(crate) fn open_with_all_components(
        config: AndroidNativeFramePipelineOpenConfig,
        source: MediaSource,
        packet_source: Option<AndroidNativeFramePipelinePacketSource>,
        decoder_sink: Option<Box<dyn AndroidNativeFrameDecoderSink>>,
        processor_chain: Option<Box<dyn AndroidNativeFrameProcessorChain>>,
        presenter_sink: Option<Box<dyn AndroidNativeFramePresenterSink>>,
    ) -> PlayerResult<Self> {
        let mut session = Self::open_with_components(
            config,
            source,
            packet_source,
            None,
            decoder_sink,
            processor_chain,
        )?;
        if let Some(presenter_sink) = presenter_sink {
            session
                .core
                .set_presenter(Box::new(AndroidPresenterAdapter {
                    inner: presenter_sink,
                }));
        }
        Ok(session)
    }

    #[cfg(test)]
    pub(crate) fn open_with_packet_source_without_decoder_sink(
        config: AndroidNativeFramePipelineOpenConfig,
        source: MediaSource,
        packet_source: Option<AndroidNativeFramePipelinePacketSource>,
    ) -> PlayerResult<Self> {
        Self::open_with_components(config, source, packet_source, None, None, None)
    }

    fn open_with_components(
        config: AndroidNativeFramePipelineOpenConfig,
        source: MediaSource,
        packet_source: Option<AndroidNativeFramePipelinePacketSource>,
        decoder_open_plan: Option<AndroidNativeFrameDecoderOpenPlan>,
        decoder_sink: Option<Box<dyn AndroidNativeFrameDecoderSink>>,
        processor_chain: Option<Box<dyn AndroidNativeFrameProcessorChain>>,
    ) -> PlayerResult<Self> {
        validate_native_frame_pipeline_open_config(&config)?;
        let max_in_flight_frames = config
            .native_frame_pipeline
            .max_in_flight_frames
            .unwrap_or(3)
            .max(1);
        let decoder_plugin_library_paths = config
            .native_frame_pipeline
            .resolved_decoder_plugin_library_paths()
            .map_err(|message| PlayerError::new(PlayerErrorCode::InvalidArgument, message))?;
        let frame_processor_plugin_library_paths = config
            .native_frame_pipeline
            .resolved_frame_processor_plugin_library_paths()
            .map_err(|message| PlayerError::new(PlayerErrorCode::InvalidArgument, message))?;
        let core = NativeFramePipelineCore::with_components(
            NativeFramePipelineCoreConfig {
                max_in_flight_frames,
                packet_budget: ANDROID_NATIVE_FRAME_ADVANCE_PACKET_BUDGET,
                pending_presenter_message: "Android native-frame presenter is waiting".to_owned(),
                missing_packet_source_message:
                    "Android native-frame packet source is not configured".to_owned(),
                decoder_warmup_message: "Android native-frame decoder is warming up".to_owned(),
            },
            packet_source.map(|source| Box::new(source) as Box<dyn NativeFramePacketSourceAdapter>),
            decoder_sink.map(|sink| {
                Box::new(AndroidDecoderAdapter { inner: sink })
                    as Box<dyn NativeFrameDecoderAdapter>
            }),
            processor_chain.map(android_processor_chain_to_shared_core),
            None,
        );
        Ok(Self {
            source_uri: source.uri().to_owned(),
            source_kind: source.kind(),
            source_protocol: source.protocol(),
            source_normalizer_mode: config.source_normalizer.mode,
            decoder_plugin_library_paths,
            frame_processor_plugin_library_paths,
            max_in_flight_frames,
            presenter_profile: config.presenter_profile,
            selected_pipeline_profile: decoder_open_plan
                .as_ref()
                .map(|plan| plan.selected_profile)
                .unwrap_or(AndroidNativeFramePipelineProfile::HostTimedSurface),
            presenter_surface_profile: None,
            decoder_open_plan_template: decoder_open_plan.clone(),
            decoder_open_plan,
            core,
        })
    }

    pub fn open_wire(&self, handle: u64) -> AndroidNativeFramePipelineOpenWire {
        let snapshot = self.core.status_snapshot();
        AndroidNativeFramePipelineOpenWire {
            handle,
            route: PlayerPlaybackRoute::SdkManagedNativeFrame.wire_name(),
            participation: "selected",
            source_input: "sourceNormalizerPacket",
            decoder_adapter: "MediaCodec",
            selected_profile: self.selected_pipeline_profile.wire_name().to_owned(),
            presenter_profile: self.presenter_profile.wire_name(),
            presenter_ready: self.presenter_ready(),
            presenter_configured: snapshot.presenter_configured,
            presenter_state: self.presenter_state_wire_name(),
            surface_attached: snapshot.output_target_attached,
            surface_profile: self
                .presenter_surface_profile
                .map(AndroidNativeFramePresenterProfile::wire_name),
            pipeline_profile: self.selected_pipeline_profile.pipeline_profile_label(),
            source_uri: self.source_uri.clone(),
            source_kind: media_source_kind_wire_name(self.source_kind).to_owned(),
            source_protocol: media_source_protocol_wire_name(self.source_protocol).to_owned(),
            source_normalizer_mode: source_normalizer_mode_wire_name(self.source_normalizer_mode)
                .to_owned(),
            decoder_plugin_count: self.decoder_plugin_library_paths.len(),
            frame_processor_count: self.frame_processor_plugin_library_paths.len(),
            max_in_flight_frames: self.max_in_flight_frames,
            counters: snapshot.counters,
        }
    }

    pub fn status_wire(
        &self,
        handle: u64,
        message: Option<String>,
    ) -> AndroidNativeFramePipelineStatusWire {
        let snapshot = self.core.status_snapshot();
        AndroidNativeFramePipelineStatusWire {
            handle,
            route: PlayerPlaybackRoute::SdkManagedNativeFrame.wire_name(),
            participation: "selected",
            source_input: "sourceNormalizerPacket",
            decoder_adapter: "MediaCodec",
            selected_profile: self.selected_pipeline_profile.wire_name().to_owned(),
            presenter_profile: self.presenter_profile.wire_name(),
            presenter_ready: self.presenter_ready(),
            presenter_configured: snapshot.presenter_configured,
            presenter_state: self.presenter_state_wire_name(),
            surface_attached: snapshot.output_target_attached,
            surface_profile: self
                .presenter_surface_profile
                .map(AndroidNativeFramePresenterProfile::wire_name),
            pipeline_profile: self.selected_pipeline_profile.pipeline_profile_label(),
            pending_frames: snapshot.pending_frames,
            end_of_stream: snapshot.end_of_stream,
            counters: snapshot.counters,
            message,
        }
    }

    pub fn advance(&mut self) -> PlayerResult<AndroidNativeFramePipelineFrameResult> {
        self.ensure_decoder_open()?;
        self.core.advance().map_err(pipeline_error_to_player_error)
    }

    pub fn flush(&mut self) -> PlayerResult<()> {
        self.core.flush().map_err(pipeline_error_to_player_error)
    }

    pub fn seek(&mut self, position: Duration) -> PlayerResult<()> {
        self.core
            .seek(position)
            .map_err(pipeline_error_to_player_error)
    }

    pub fn release_frame(&mut self, frame_handle: u64, presented: bool) -> PlayerResult<()> {
        self.core
            .release_frame(frame_handle, presented)
            .map_err(pipeline_error_to_player_error)
    }

    pub fn attach_presenter_surface(
        &mut self,
        surface_profile: AndroidNativeFramePresenterProfile,
    ) -> PlayerResult<()> {
        if surface_profile != self.presenter_profile {
            return Err(PlayerError::new(
                PlayerErrorCode::InvalidArgument,
                format!(
                    "Android native-frame presenter expected {} surface but received {}",
                    self.presenter_profile.wire_name(),
                    surface_profile.wire_name()
                ),
            ));
        }
        self.presenter_surface_profile = Some(surface_profile);
        self.core.set_output_target_attached(true);
        Ok(())
    }

    pub fn set_presenter_sink(&mut self, sink: Box<dyn AndroidNativeFramePresenterSink>) {
        self.close_decoder_for_presenter_rebind();
        self.core
            .set_presenter(Box::new(AndroidPresenterAdapter { inner: sink }));
    }

    pub fn configure_presenter_sink(
        &mut self,
        sink: Box<dyn AndroidNativeFramePresenterSink>,
    ) -> PlayerResult<()> {
        self.set_presenter_sink(sink);
        self.ensure_decoder_open()
    }

    pub fn detach_presenter_surface(&mut self) {
        if let Err(error) = self.core.clear_presenter_for_detach() {
            tracing::warn!(error = %error, "Android native-frame presenter detach failed");
        }
        self.presenter_surface_profile = None;
        if self.decoder_open_plan.is_none() {
            self.decoder_open_plan = self.decoder_open_plan_template.clone();
        }
    }

    fn close_decoder_for_presenter_rebind(&mut self) {
        if let Err(error) = self.core.close_decoder_for_rebind() {
            tracing::warn!(error = %error, "Android native-frame decoder close before presenter rebind failed");
        }
        if self.decoder_open_plan.is_none() {
            self.decoder_open_plan = self.decoder_open_plan_template.clone();
        }
    }

    pub fn presenter_ready(&self) -> bool {
        let snapshot = self.core.status_snapshot();
        snapshot.output_target_attached
            && snapshot.presenter_configured
            && snapshot.decoder_configured
    }

    pub fn pending_frame_count(&self) -> usize {
        self.core.pending_frame_count()
    }

    pub fn has_pending_packet(&self) -> bool {
        self.core.has_pending_packet()
    }

    pub fn pending_packet_data_len(&self) -> Option<usize> {
        self.core.pending_packet_data_len()
    }

    pub fn pending_packet_stream_index(&self) -> Option<u32> {
        self.core.pending_packet_stream_index()
    }

    fn ensure_decoder_open(&mut self) -> PlayerResult<()> {
        if self.core.has_decoder() {
            return Ok(());
        }
        let Some(plan) = self.decoder_open_plan.take() else {
            return Ok(());
        };
        let native_device_context = if plan.requires_android_native_window {
            let context = self.core.presenter_decoder_device_context();
            if context.is_none() {
                self.decoder_open_plan = Some(plan);
                return Ok(());
            }
            context
        } else {
            self.core.presenter_decoder_device_context()
        };
        match plan.open(native_device_context) {
            Ok(decoder_sink) => {
                self.core.set_decoder(Box::new(AndroidDecoderAdapter {
                    inner: decoder_sink,
                }));
                self.decoder_open_plan_template = Some(plan);
                Ok(())
            }
            Err(error) => {
                self.decoder_open_plan = Some(plan);
                Err(error)
            }
        }
    }

    fn presenter_state_wire_name(&self) -> &'static str {
        if !self.core.output_target_attached() {
            "waitingForSurface"
        } else if !self.core.has_presenter() {
            "waitingForPresenter"
        } else if !self.core.has_decoder() && self.decoder_open_plan.is_some() {
            "waitingForDecoder"
        } else if self.core.status_snapshot().lifecycle_state
            == NativeFramePipelineLifecycleState::Presenting
        {
            "presenting"
        } else {
            "ready"
        }
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

    fn take_dropped_event_count(&mut self) -> u64 {
        self.inner.take_dropped_event_count()
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

    pub fn report_first_frame(&self, frame: FirstFrameReady) {
        self.push_update(AndroidNativeSessionUpdate::FirstFrameReady(frame));
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
                if updates.len() >= MAX_PENDING_RUNTIME_EVENTS {
                    self.dropped_updates.fetch_add(1, Ordering::Relaxed);
                    return;
                }
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

    fn take_dropped_update_count(&self) -> u64 {
        self.dropped_updates.swap(0, Ordering::Relaxed)
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
            dropped_events: 0,
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
                        push_runtime_event_bounded(
                            &mut self.events,
                            &mut self.dropped_events,
                            PlayerRuntimeEvent::MetadataReady(self.media_info.clone()),
                        );
                    }
                }
                AndroidNativeSessionUpdate::SeekCompleted { position } => {
                    self.progress = PlaybackProgress::new(position, self.progress.duration());
                    if self.presentation_state == PresentationState::Finished {
                        self.presentation_state = PresentationState::Ready;
                        self.tracker
                            .seed(self.presentation_state, self.playback_rate);
                    }
                    push_runtime_event_bounded(
                        &mut self.events,
                        &mut self.dropped_events,
                        PlayerRuntimeEvent::SeekCompleted { position },
                    );
                }
                AndroidNativeSessionUpdate::RetryScheduled { attempt, delay } => {
                    self.resilience_metrics
                        .observe_retry_scheduled(attempt, delay);
                    push_runtime_event_bounded(
                        &mut self.events,
                        &mut self.dropped_events,
                        PlayerRuntimeEvent::RetryScheduled { attempt, delay },
                    );
                }
                AndroidNativeSessionUpdate::FirstFrameReady(frame) => {
                    push_runtime_event_bounded(
                        &mut self.events,
                        &mut self.dropped_events,
                        PlayerRuntimeEvent::FirstFrameReady(frame),
                    );
                }
                AndroidNativeSessionUpdate::Error(error) => {
                    push_runtime_event_bounded(
                        &mut self.events,
                        &mut self.dropped_events,
                        PlayerRuntimeEvent::Error(error),
                    );
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

    pub fn sample_timeline(&self, snapshot: &AndroidExoPlaybackSnapshot) -> PlayerTimelineSnapshot {
        let progress = PlaybackProgress::new(snapshot.position, snapshot.duration);
        live_timeline_metadata(snapshot)
            .map(|metadata| player_timeline_from_android_live_metadata(progress, metadata))
            .unwrap_or_else(|| {
                PlayerTimelineSnapshot::from_media_info(
                    progress,
                    self.capabilities.supports_seek,
                    &self.media_info,
                )
            })
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
        extend_runtime_events_bounded(
            &mut self.events,
            &mut self.dropped_events,
            observation.emitted_events,
        );
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
                    return Err(Self::track_selection_error(
                        kind,
                        "subtitle_track_not_found",
                        None,
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
                        Self::track_selection_error(
                            kind,
                            "subtitle_track_not_found",
                            Some(track_id),
                            format!(
                                "track '{track_id}' is not present in the current track catalog"
                            ),
                        )
                    })?;

                if track.kind != kind {
                    return Err(Self::track_selection_error(
                        kind,
                        "subtitle_track_not_found",
                        Some(track_id),
                        format!("track '{track_id}' is not a {:?} track", kind),
                    ));
                }

                Ok(MediaTrackSelection::track(track_id))
            }
        }
    }

    fn track_selection_error(
        kind: MediaTrackKind,
        code: &str,
        track_id: Option<&str>,
        message: impl Into<String>,
    ) -> PlayerError {
        let message = message.into();
        let error = PlayerError::new(PlayerErrorCode::InvalidArgument, message.clone());
        if kind == MediaTrackKind::Subtitle {
            error.with_subtitle_details(SubtitleErrorDetails::new(
                code,
                "selection",
                track_id.map(str::to_owned),
                false,
                message,
            ))
        } else {
            error
        }
    }

    fn validate_abr_policy_request(
        &self,
        policy: &MediaAbrPolicy,
        expected_catalog_revision: Option<u64>,
    ) -> PlayerResult<MediaAbrPolicy> {
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
                    return Err(Self::fixed_track_selection_error(
                        PlayerErrorCode::InvalidArgument,
                        "trackUnavailable",
                        None,
                        expected_catalog_revision,
                        Some(self.media_info.track_catalog.catalog_revision),
                        "fixed-track ABR requires a video track id",
                    ));
                };

                if let Some(expected_catalog_revision) = expected_catalog_revision
                    && expected_catalog_revision != self.media_info.track_catalog.catalog_revision
                {
                    return Err(Self::fixed_track_selection_error(
                        PlayerErrorCode::InvalidState,
                        "staleCatalog",
                        Some(track_id),
                        Some(expected_catalog_revision),
                        Some(self.media_info.track_catalog.catalog_revision),
                        "the track catalog changed before the fixed-track command was applied",
                    ));
                }

                let track = self
                    .media_info
                    .track_catalog
                    .tracks
                    .iter()
                    .find(|track| track.id == track_id)
                    .ok_or_else(|| {
                        Self::fixed_track_selection_error(
                            PlayerErrorCode::InvalidArgument,
                            "trackUnavailable",
                            Some(track_id),
                            expected_catalog_revision,
                            Some(self.media_info.track_catalog.catalog_revision),
                            format!(
                                "track '{track_id}' is not present in the current track catalog"
                            ),
                        )
                    })?;

                if track.kind != MediaTrackKind::Video {
                    return Err(Self::fixed_track_selection_error(
                        PlayerErrorCode::InvalidArgument,
                        "trackUnavailable",
                        Some(track_id),
                        expected_catalog_revision,
                        Some(self.media_info.track_catalog.catalog_revision),
                        format!("track '{track_id}' is not a video track"),
                    ));
                }

                let (code, error_code, message) = match track.support.status {
                    MediaTrackSupportStatus::ExceedsCapabilities => (
                        "trackExceedsCapabilities",
                        PlayerErrorCode::Unsupported,
                        "the requested track exceeds current playback capabilities",
                    ),
                    MediaTrackSupportStatus::Unsupported => (
                        "trackUnsupported",
                        PlayerErrorCode::Unsupported,
                        "the requested track is unsupported by the active playback path",
                    ),
                    MediaTrackSupportStatus::Supported | MediaTrackSupportStatus::Unknown => {
                        ("", PlayerErrorCode::InvalidArgument, "")
                    }
                };
                if !code.is_empty() {
                    return Err(Self::fixed_track_selection_error(
                        error_code,
                        code,
                        Some(track_id),
                        expected_catalog_revision,
                        Some(self.media_info.track_catalog.catalog_revision),
                        message,
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

    fn fixed_track_selection_error(
        error_code: PlayerErrorCode,
        code: &str,
        track_id: Option<&str>,
        expected_catalog_revision: Option<u64>,
        actual_catalog_revision: Option<u64>,
        message: impl Into<String>,
    ) -> PlayerError {
        let message = message.into();
        PlayerError::with_taxonomy(
            error_code,
            PlayerErrorCategory::Capability,
            false,
            message.clone(),
        )
        .with_fixed_track_selection_details(FixedTrackSelectionErrorDetails::new(
            code,
            track_id.map(str::to_owned),
            expected_catalog_revision,
            actual_catalog_revision,
            message,
        ))
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
            PlayerRuntimeCommand::SetAbrPolicy {
                policy,
                expected_catalog_revision,
            } => {
                let policy =
                    self.validate_abr_policy_request(policy, *expected_catalog_revision)?;
                if self.media_info.track_selection.abr_policy == policy {
                    return Ok((false, Vec::new()));
                }
                Ok((
                    true,
                    vec![AndroidNativePlayerCommand::SetAbrPolicy {
                        policy,
                        expected_catalog_revision: *expected_catalog_revision,
                    }],
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

    fn take_dropped_event_count(&mut self) -> u64 {
        let dropped = self
            .dropped_events
            .saturating_add(self.controller.take_dropped_update_count());
        self.dropped_events = 0;
        dropped
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
                PlayerRuntimeCommand::SetAbrPolicy { policy, .. } => {
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

pub fn android_native_frame_pipeline_open_json(
    handle: u64,
    session: &AndroidNativeFramePipelineSession,
) -> Result<String, serde_json::Error> {
    serde_json::to_string(&session.open_wire(handle))
}

pub fn android_native_frame_pipeline_status_json(
    handle: u64,
    session: &AndroidNativeFramePipelineSession,
    message: Option<String>,
) -> Result<String, serde_json::Error> {
    serde_json::to_string(&session.status_wire(handle, message))
}

pub fn android_native_frame_pipeline_frame_json(
    result: AndroidNativeFramePipelineFrameResult,
    counters: AndroidNativeFramePipelineCounters,
) -> Result<String, serde_json::Error> {
    let wire = match result.frame {
        Some(frame) => AndroidNativeFramePipelineFrameWire {
            status: "frame",
            message: result.message,
            handle: result.handle,
            native_handle: Some(frame.handle),
            presentation_time_us: Some(frame.presentation_time_us),
            duration_us: frame.duration_us,
            width: Some(frame.width),
            height: Some(frame.height),
            frame_id: frame.frame_id,
            requires_host_release: result.requires_host_release,
            counters,
        },
        None => AndroidNativeFramePipelineFrameWire {
            status: match result.status {
                AndroidNativeFramePipelineFrameStatus::Pending => "pending",
                AndroidNativeFramePipelineFrameStatus::Frame => "frame",
                AndroidNativeFramePipelineFrameStatus::Presented => "presented",
                AndroidNativeFramePipelineFrameStatus::EndOfStream => "endOfStream",
            },
            message: result.message,
            handle: result.handle,
            native_handle: None,
            presentation_time_us: None,
            duration_us: None,
            width: None,
            height: None,
            frame_id: None,
            requires_host_release: result.requires_host_release,
            counters,
        },
    };
    serde_json::to_string(&wire)
}

fn normalize_media_info(source: &MediaSource, mut media_info: PlayerMediaInfo) -> PlayerMediaInfo {
    media_info.source_uri = source.uri().to_owned();
    media_info.source_kind = source.kind();
    media_info.source_protocol = source.protocol();
    media_info
}

fn media_source_kind_wire_name(kind: MediaSourceKind) -> &'static str {
    match kind {
        MediaSourceKind::Local => "local",
        MediaSourceKind::Remote => "remote",
    }
}

fn media_source_protocol_wire_name(protocol: MediaSourceProtocol) -> &'static str {
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

fn source_normalizer_mode_wire_name(mode: SourceNormalizerMode) -> &'static str {
    match mode {
        SourceNormalizerMode::Disabled => "disabled",
        SourceNormalizerMode::DiagnosticsOnly => "diagnosticsOnly",
        SourceNormalizerMode::PreflightOnly => "preflightOnly",
        SourceNormalizerMode::PreferNormalized => "preferNormalized",
        SourceNormalizerMode::RequireNormalized => "requireNormalized",
    }
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
        "android native adapter requires a platform player bridge before initialization"
    } else {
        "android native adapter can be probed on desktop builds, but initialization is Android-target only"
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
