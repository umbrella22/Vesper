use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::thread;
use std::time::{Duration, Instant};

use player_audio_cpal::{
    AudioOutputConfig, AudioOutputDescriptor, AudioSink, AudioSinkController, detect_default_output,
};
use player_backend_ffmpeg::{
    AudioStreamProbe, BufferedFramePoll, BufferedVideoSource, BufferedVideoSourceBootstrap,
    DecodedAudioTrack, FfmpegBackend, MediaProbe, VideoDecodeInfo as BackendVideoDecodeInfo,
    VideoDecoderMode as BackendVideoDecoderMode, VideoStreamProbe,
};
use player_core::{
    MediaSource, MediaSourceKind, MediaSourceProtocol, PlaybackClock, PlaybackSessionModel,
};

use player_runtime::{
    DEFAULT_PLAYBACK_RATE, DecodedAudioSummary, DecodedVideoFrame, FirstFrameReady,
    MAX_PLAYBACK_RATE, MIN_PLAYBACK_RATE, NATURAL_PLAYBACK_RATE_MAX, PlaybackProgress,
    PlayerAudioInfo, PlayerAudioOutputInfo, PlayerBufferingPolicy, PlayerCachePolicy,
    PlayerMediaInfo, PlayerResilienceMetricsTracker, PlayerRetryBackoff, PlayerRetryPolicy,
    PlayerRuntimeAdapter, PlayerRuntimeAdapterBackendFamily, PlayerRuntimeAdapterBootstrap,
    PlayerRuntimeAdapterCapabilities, PlayerRuntimeAdapterFactory, PlayerRuntimeAdapterInitializer,
    PlayerRuntimeCommand, PlayerRuntimeCommandResult, PlayerRuntimeError, PlayerRuntimeErrorCode,
    PlayerRuntimeEvent, PlayerRuntimeOptions, PlayerRuntimeResult, PlayerRuntimeStartup,
    PlayerVideoInfo, PresentationState, register_default_runtime_adapter_factory,
};

pub const SOFTWARE_PLAYER_RUNTIME_ADAPTER_ID: &str = "software_desktop";
const AUDIO_STREAM_CHUNK_FRAMES: usize = 2_048;
const DEFAULT_AUDIO_STREAM_TARGET_BUFFER_DURATION: Duration = Duration::from_secs(2);
const DEFAULT_AUDIO_PLAYBACK_START_BUFFER_DURATION: Duration = Duration::from_millis(500);
const DEFAULT_AUDIO_REBUFFER_DURATION: Duration = Duration::from_millis(250);
const DEFAULT_AUDIO_BUFFER_HEADROOM_DURATION: Duration = Duration::from_millis(500);
const MAX_DESKTOP_AUDIO_BUFFER_DURATION: Duration = Duration::from_secs(4);
const DEFAULT_VIDEO_FRAME_RATE_ESTIMATE: f64 = 30.0;
const DEFAULT_VIDEO_FRAME_MEMORY_ESTIMATE_BYTES: usize = 1_382_400;
const DESKTOP_ACTIVE_VIDEO_PREFETCH_MEMORY_SCALE: u64 = 4;
const MAX_DESKTOP_VIDEO_PREFETCH_CAPACITY: usize = 96;
const DEFAULT_VIDEO_BUFFER_HEADROOM_DURATION: Duration = Duration::from_millis(500);
const AUDIO_STREAM_BACKPRESSURE_POLL_INTERVAL: Duration = Duration::from_millis(10);
const AUDIO_OUTPUT_POLL_INTERVAL: Duration = Duration::from_secs(1);
const AUDIO_CLOCK_STALL_TOLERANCE: Duration = Duration::from_millis(250);
const SOFTWARE_BUFFERING_GRACE_PERIOD: Duration = Duration::from_millis(120);

pub fn desktop_runtime_adapter_factory() -> &'static dyn PlayerRuntimeAdapterFactory {
    static FACTORY: SoftwarePlayerRuntimeAdapterFactory = SoftwarePlayerRuntimeAdapterFactory;
    &FACTORY
}

pub fn install_default_desktop_runtime_adapter_factory() -> PlayerRuntimeResult<()> {
    register_default_runtime_adapter_factory(desktop_runtime_adapter_factory())
}

pub fn probe_platform_desktop_source_with_options(
    adapter_id: &'static str,
    source: MediaSource,
    options: PlayerRuntimeOptions,
) -> PlayerRuntimeResult<Box<dyn PlayerRuntimeAdapterInitializer>> {
    Ok(Box::new(PlatformDesktopRuntimeAdapterInitializer {
        adapter_id,
        inner: Box::new(SoftwarePlayerRuntimeInitializer::probe_source_with_options(
            source, options,
        )?),
    }))
}

pub fn open_platform_desktop_source_with_options_and_interrupt(
    adapter_id: &'static str,
    source: MediaSource,
    options: PlayerRuntimeOptions,
    interrupt_flag: Arc<AtomicBool>,
) -> PlayerRuntimeResult<PlayerRuntimeAdapterBootstrap> {
    let initializer = SoftwarePlayerRuntimeInitializer::probe_source_with_options_and_interrupt(
        source,
        options,
        Some(interrupt_flag),
    )?;
    let PlayerRuntimeAdapterBootstrap {
        runtime,
        initial_frame,
        startup,
    } = Box::new(initializer).initialize()?;

    Ok(PlayerRuntimeAdapterBootstrap {
        runtime: Box::new(PlatformDesktopRuntimeAdapter {
            adapter_id,
            inner: runtime,
        }),
        initial_frame,
        startup,
    })
}

#[derive(Debug, Default, Clone, Copy)]
pub struct SoftwarePlayerRuntimeAdapterFactory;

#[derive(Debug)]
pub struct SoftwarePlayerRuntimeInitializer {
    backend: FfmpegBackend,
    source: MediaSource,
    probe: Option<player_backend_ffmpeg::MediaProbe>,
    audio_output: AudioOutputDescriptor,
    options: PlayerRuntimeOptions,
    interrupt_flag: Option<Arc<AtomicBool>>,
}

#[derive(Debug)]
struct SoftwareRuntimeConfig {
    backend: FfmpegBackend,
    source: MediaSource,
    probe: Option<player_backend_ffmpeg::MediaProbe>,
    buffering_policy: PlayerBufferingPolicy,
    retry_policy: PlayerRetryPolicy,
    cache_policy: PlayerCachePolicy,
    audio_output_descriptor: AudioOutputDescriptor,
    audio_output_config: Option<AudioOutputConfig>,
    source_audio_track: Option<DecodedAudioTrack>,
    interrupt_flag: Option<Arc<AtomicBool>>,
    video_prefetch_capacity: usize,
    video_present_early_tolerance: Duration,
    video_idle_poll_interval: Duration,
}

pub struct SoftwarePlayerRuntime {
    backend: FfmpegBackend,
    source: MediaSource,
    media_info: PlayerMediaInfo,
    session: PlaybackSessionModel,
    playback_rate: f32,
    initial_media_position: Duration,
    initial_video_position: Duration,
    audio_output_descriptor: AudioOutputDescriptor,
    audio_output_config: Option<AudioOutputConfig>,
    source_audio_track: Option<DecodedAudioTrack>,
    video_source: BufferedVideoSource,
    video_end_of_stream: bool,
    next_frame: Option<DecodedVideoFrame>,
    audio_sink: Option<AudioSink>,
    audio_sink_controller: Option<AudioSinkController>,
    playback_clock: Option<PlaybackClock>,
    video_playback_start_buffer_frames: usize,
    video_rebuffer_frames: usize,
    video_buffering_window: VideoBufferingWindow,
    audio_playback_start_buffer_duration: Duration,
    audio_stream_target_buffer_duration: Duration,
    audio_rebuffer_duration: Duration,
    audio_buffering_window: AudioBufferingWindow,
    video_present_early_tolerance: Duration,
    video_idle_poll_interval: Duration,
    pending_audio_metadata_worker: Option<PendingAudioMetadataWorker>,
    pending_audio_decode_worker: Option<PendingAudioDecodeWorker>,
    pending_audio_stream_worker: Option<PendingAudioStreamWorker>,
    pending_audio_metadata_retry: Option<ScheduledRetry>,
    pending_audio_stream_retry: Option<ScheduledAudioStreamRetry>,
    is_buffering: bool,
    buffering_candidate_since: Option<Instant>,
    last_audio_output_poll: Instant,
    retry_policy: PlayerRetryPolicy,
    resilience_metrics: PlayerResilienceMetricsTracker,
    events: VecDeque<PlayerRuntimeEvent>,
}

struct PendingAudioStreamWorker {
    generation: u64,
    retry_attempt: u32,
    receiver: Receiver<Result<AudioStreamWorkerEvent, String>>,
    interrupt_flag: Option<Arc<AtomicBool>>,
}

enum AudioStreamWorkerEvent {
    Metadata(MediaProbe),
    Finished,
}

struct PendingAudioDecodeWorker {
    receiver: Receiver<Result<DecodedAudioTrack, String>>,
}

struct PendingAudioMetadataWorker {
    retry_attempt: u32,
    receiver: Receiver<Result<MediaProbe, String>>,
}

#[derive(Debug, Clone, Copy)]
struct ScheduledRetry {
    attempt: u32,
    due_at: Instant,
}

#[derive(Debug, Clone, Copy)]
struct ScheduledAudioStreamRetry {
    attempt: u32,
    due_at: Instant,
    position: Duration,
    playback_rate: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AudioBufferingWindow {
    Startup,
    Rebuffer,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum VideoBufferingWindow {
    Startup,
    Rebuffer,
}

struct PlatformDesktopRuntimeAdapterInitializer {
    adapter_id: &'static str,
    inner: Box<dyn PlayerRuntimeAdapterInitializer>,
}

struct PlatformDesktopRuntimeAdapter {
    adapter_id: &'static str,
    inner: Box<dyn PlayerRuntimeAdapter>,
}

impl std::fmt::Debug for PlatformDesktopRuntimeAdapter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PlatformDesktopRuntimeAdapter")
            .field("adapter_id", &self.adapter_id)
            .field("source_uri", &self.inner.source_uri())
            .field("state", &self.inner.presentation_state())
            .finish()
    }
}

impl std::fmt::Debug for PlatformDesktopRuntimeAdapterInitializer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PlatformDesktopRuntimeAdapterInitializer")
            .field("adapter_id", &self.adapter_id)
            .finish()
    }
}

impl PlayerRuntimeAdapterFactory for SoftwarePlayerRuntimeAdapterFactory {
    fn adapter_id(&self) -> &'static str {
        SOFTWARE_PLAYER_RUNTIME_ADAPTER_ID
    }

    fn probe_source_with_options(
        &self,
        source: MediaSource,
        options: PlayerRuntimeOptions,
    ) -> PlayerRuntimeResult<Box<dyn PlayerRuntimeAdapterInitializer>> {
        Ok(Box::new(
            SoftwarePlayerRuntimeInitializer::probe_source_with_options(source, options)?,
        ))
    }
}

impl PlayerRuntimeAdapterInitializer for PlatformDesktopRuntimeAdapterInitializer {
    fn capabilities(&self) -> PlayerRuntimeAdapterCapabilities {
        with_adapter_id(self.inner.capabilities(), self.adapter_id)
    }

    fn media_info(&self) -> PlayerMediaInfo {
        self.inner.media_info()
    }

    fn startup(&self) -> PlayerRuntimeStartup {
        self.inner.startup()
    }

    fn initialize(self: Box<Self>) -> PlayerRuntimeResult<PlayerRuntimeAdapterBootstrap> {
        let Self { adapter_id, inner } = *self;
        let PlayerRuntimeAdapterBootstrap {
            runtime,
            initial_frame,
            startup,
        } = inner.initialize()?;

        Ok(PlayerRuntimeAdapterBootstrap {
            runtime: Box::new(PlatformDesktopRuntimeAdapter {
                adapter_id,
                inner: runtime,
            }),
            initial_frame,
            startup,
        })
    }
}

impl PlayerRuntimeAdapterInitializer for SoftwarePlayerRuntimeInitializer {
    fn capabilities(&self) -> PlayerRuntimeAdapterCapabilities {
        software_desktop_capabilities()
    }

    fn media_info(&self) -> PlayerMediaInfo {
        self.probe
            .as_ref()
            .map(player_media_info)
            .unwrap_or_else(|| unresolved_player_media_info(&self.source))
    }

    fn startup(&self) -> PlayerRuntimeStartup {
        PlayerRuntimeStartup {
            ffmpeg_initialized: self.backend.is_initialized(),
            audio_output: audio_output_info(&self.audio_output),
            decoded_audio: None,
            video_decode: None,
        }
    }

    fn initialize(self: Box<Self>) -> PlayerRuntimeResult<PlayerRuntimeAdapterBootstrap> {
        let Self {
            backend,
            source,
            probe,
            audio_output,
            options,
            interrupt_flag,
        } = *self;

        let decoded_audio = match audio_output.default_output_config.clone() {
            Some(output_config)
                if probe
                    .as_ref()
                    .is_some_and(|probe| probe.best_audio.is_some()) =>
            {
                if should_defer_audio_decode_for_source(&source) {
                    None
                } else {
                    Some(
                        backend
                            .decode_audio_track_with_interrupt(
                                source.clone(),
                                output_config.sample_rate,
                                output_config.channels,
                                interrupt_flag.clone(),
                            )
                            .map_err(|error| {
                                runtime_error(
                                    PlayerRuntimeErrorCode::DecodeFailure,
                                    "failed to decode audio track during initialization",
                                    error,
                                )
                            })?,
                    )
                }
            }
            _ => None,
        };
        let startup = PlayerRuntimeStartup {
            ffmpeg_initialized: backend.is_initialized(),
            audio_output: audio_output_info(&audio_output),
            decoded_audio: decoded_audio.as_ref().map(decoded_audio_summary),
            video_decode: None,
        };
        let resolved_resilience_policy =
            options.resolved_resilience_policy(source.kind(), source.protocol());
        let resolved_buffering_policy = resolved_resilience_policy.buffering_policy;
        let config = SoftwareRuntimeConfig {
            backend,
            source,
            probe,
            buffering_policy: resolved_buffering_policy.clone(),
            retry_policy: resolved_resilience_policy.retry_policy,
            cache_policy: resolved_resilience_policy.cache_policy,
            audio_output_descriptor: audio_output.clone(),
            audio_output_config: audio_output.default_output_config,
            source_audio_track: decoded_audio.clone(),
            interrupt_flag,
            video_prefetch_capacity: resolved_video_prefetch_capacity(
                options.video_prefetch_capacity,
                &resolved_buffering_policy,
            ),
            video_present_early_tolerance: options.video_present_early_tolerance,
            video_idle_poll_interval: options.video_idle_poll_interval,
        };

        SoftwarePlayerRuntime::open_with_startup(config, startup)
    }
}

impl SoftwarePlayerRuntimeInitializer {
    pub fn probe_source_with_options(
        source: MediaSource,
        options: PlayerRuntimeOptions,
    ) -> PlayerRuntimeResult<Self> {
        Self::probe_source_with_options_and_interrupt(source, options, None)
    }

    pub fn probe_source_with_options_and_interrupt(
        source: MediaSource,
        options: PlayerRuntimeOptions,
        interrupt_flag: Option<Arc<AtomicBool>>,
    ) -> PlayerRuntimeResult<Self> {
        let backend = FfmpegBackend::new().map_err(|error| {
            runtime_error(
                PlayerRuntimeErrorCode::BackendFailure,
                "failed to initialize ffmpeg backend",
                error,
            )
        })?;
        if let Some(reason) = backend.unsupported_source_reason(&source) {
            return Err(PlayerRuntimeError::new(
                PlayerRuntimeErrorCode::Unsupported,
                reason,
            ));
        }
        let audio_output = if options.enable_audio_output {
            detect_default_output()
        } else {
            AudioOutputDescriptor {
                default_output_device: None,
                default_output_config: None,
            }
        };
        let probe = if should_defer_media_probe_for_source(&source) {
            None
        } else {
            Some(
                backend
                    .probe_with_interrupt(source.clone(), interrupt_flag.clone())
                    .map_err(|error| {
                        runtime_error(
                            PlayerRuntimeErrorCode::InvalidSource,
                            "failed to probe media source",
                            error,
                        )
                    })?,
            )
        };

        Ok(Self {
            backend,
            source,
            probe,
            audio_output,
            options,
            interrupt_flag,
        })
    }
}

impl PlayerRuntimeAdapter for SoftwarePlayerRuntime {
    fn source_uri(&self) -> &str {
        self.source.uri()
    }

    fn capabilities(&self) -> PlayerRuntimeAdapterCapabilities {
        software_desktop_capabilities()
    }

    fn media_info(&self) -> &PlayerMediaInfo {
        &self.media_info
    }

    fn presentation_state(&self) -> PresentationState {
        self.session.presentation_state()
    }

    fn is_buffering(&self) -> bool {
        self.is_buffering
    }

    fn playback_rate(&self) -> f32 {
        self.playback_rate
    }

    fn progress(&self) -> PlaybackProgress {
        let position = self.playback_position().unwrap_or(Duration::ZERO);
        self.session.progress(position)
    }

    fn snapshot(&self) -> player_runtime::PlayerSnapshot {
        player_runtime::PlayerSnapshot {
            source_uri: self.source_uri().to_owned(),
            state: self.presentation_state(),
            has_video_surface: false,
            is_interrupted: false,
            is_buffering: self.is_buffering(),
            playback_rate: self.playback_rate(),
            progress: self.progress(),
            timeline: player_runtime::PlayerTimelineSnapshot::from_media_info(
                self.progress(),
                self.capabilities().supports_seek,
                self.media_info(),
            ),
            media_info: self.media_info().clone(),
            resilience_metrics: self.resilience_metrics.snapshot(),
        }
    }

    fn drain_events(&mut self) -> Vec<PlayerRuntimeEvent> {
        self.poll_scheduled_retries();
        self.poll_audio_output_device();
        self.poll_audio_metadata_worker();
        self.poll_audio_decode_worker();
        self.poll_audio_stream_worker();
        self.events.drain(..).collect()
    }

    fn dispatch(
        &mut self,
        command: PlayerRuntimeCommand,
    ) -> PlayerRuntimeResult<PlayerRuntimeCommandResult> {
        match self.try_dispatch(command) {
            Ok((applied, frame)) => Ok(PlayerRuntimeCommandResult {
                applied,
                frame,
                snapshot: self.snapshot(),
            }),
            Err(error) => self.fail(error),
        }
    }

    fn advance(&mut self) -> PlayerRuntimeResult<Option<DecodedVideoFrame>> {
        match self.try_advance() {
            Ok(frame) => Ok(frame),
            Err(error) => self.fail(error),
        }
    }

    fn next_deadline(&self) -> Option<Instant> {
        let retry_deadline = self.next_retry_deadline();
        if !self.session.is_started() || self.session.is_paused() || self.session.is_finished() {
            return retry_deadline;
        }

        if let Some(next_frame) = self.next_frame.as_ref() {
            let playback_position = self.playback_position()?;
            let scheduled_time = next_frame
                .presentation_time
                .saturating_sub(self.video_present_early_tolerance);
            if playback_position >= scheduled_time {
                return Some(
                    retry_deadline
                        .map_or_else(Instant::now, |deadline| deadline.min(Instant::now())),
                );
            }

            let frame_deadline = Instant::now() + scheduled_time.saturating_sub(playback_position);
            return Some(
                retry_deadline.map_or(frame_deadline, |deadline| deadline.min(frame_deadline)),
            );
        }

        if !self.video_end_of_stream {
            let idle_deadline = Instant::now() + self.video_idle_poll_interval;
            return Some(
                retry_deadline.map_or(idle_deadline, |deadline| deadline.min(idle_deadline)),
            );
        }

        if self
            .audio_sink
            .as_ref()
            .map(|sink| !sink.is_finished())
            .unwrap_or(false)
        {
            let idle_deadline = Instant::now() + self.video_idle_poll_interval;
            return Some(
                retry_deadline.map_or(idle_deadline, |deadline| deadline.min(idle_deadline)),
            );
        }

        retry_deadline
    }
}

impl PlayerRuntimeAdapter for PlatformDesktopRuntimeAdapter {
    fn source_uri(&self) -> &str {
        self.inner.source_uri()
    }

    fn capabilities(&self) -> PlayerRuntimeAdapterCapabilities {
        with_adapter_id(self.inner.capabilities(), self.adapter_id)
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

    fn snapshot(&self) -> player_runtime::PlayerSnapshot {
        self.inner.snapshot()
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

    fn advance(&mut self) -> PlayerRuntimeResult<Option<DecodedVideoFrame>> {
        self.inner.advance()
    }

    fn next_deadline(&self) -> Option<Instant> {
        self.inner.next_deadline()
    }
}

impl SoftwarePlayerRuntime {
    fn open_with_startup(
        config: SoftwareRuntimeConfig,
        mut startup: PlayerRuntimeStartup,
    ) -> PlayerRuntimeResult<PlayerRuntimeAdapterBootstrap> {
        let BufferedVideoSourceBootstrap {
            source: mut video_source,
            decode_info,
            probe: opened_probe,
        } = BufferedVideoSource::new_with_interrupt(
            config.source.clone(),
            config.video_prefetch_capacity,
            config.interrupt_flag.clone(),
        )
        .map_err(|error| {
            runtime_error(
                PlayerRuntimeErrorCode::BackendFailure,
                "failed to create buffered video source",
                error,
            )
        })?;
        let probe = config.probe.unwrap_or(opened_probe);
        let initial_frame = video_source
            .recv_frame()
            .map_err(|error| {
                runtime_error(
                    PlayerRuntimeErrorCode::DecodeFailure,
                    "failed to receive initial video frame from the predecode worker",
                    error,
                )
            })?
            .ok_or_else(|| {
                PlayerRuntimeError::new(
                    PlayerRuntimeErrorCode::DecodeFailure,
                    "video stream did not produce any frames during initialization",
                )
            })?;
        startup.video_decode = Some(player_video_decode_info(&decode_info));
        let session = PlaybackSessionModel::new(
            probe.duration,
            probe.best_video.as_ref().and_then(|video| video.frame_rate),
        );
        let media_info = player_media_info(&probe);
        let video_prefetch_limit = resolved_video_prefetch_limit(
            &media_info,
            config.video_prefetch_capacity,
            &config.cache_policy,
        );
        video_source.set_prefetch_limit(video_prefetch_limit);
        let video_playback_start_buffer_frames = resolved_video_playback_start_buffer_frames(
            &media_info,
            video_prefetch_limit,
            &config.buffering_policy,
        );
        let video_rebuffer_frames = resolved_video_rebuffer_frames(
            &media_info,
            video_prefetch_limit,
            &config.buffering_policy,
        );
        let audio_playback_start_buffer_duration =
            resolved_audio_playback_start_buffer_duration(&config.buffering_policy);
        let audio_stream_target_buffer_duration =
            resolved_audio_stream_target_buffer_duration(&config.buffering_policy);
        let audio_rebuffer_duration = resolved_audio_rebuffer_duration(&config.buffering_policy);

        let mut runtime = Self {
            backend: config.backend,
            source: config.source,
            media_info,
            session,
            playback_rate: DEFAULT_PLAYBACK_RATE,
            initial_media_position: Duration::ZERO,
            initial_video_position: Duration::ZERO,
            audio_output_descriptor: config.audio_output_descriptor,
            audio_output_config: config.audio_output_config,
            source_audio_track: config.source_audio_track,
            video_source,
            video_end_of_stream: false,
            next_frame: None,
            audio_sink: None,
            audio_sink_controller: None,
            playback_clock: None,
            video_playback_start_buffer_frames,
            video_rebuffer_frames,
            video_buffering_window: VideoBufferingWindow::Startup,
            audio_playback_start_buffer_duration,
            audio_stream_target_buffer_duration,
            audio_rebuffer_duration,
            audio_buffering_window: AudioBufferingWindow::Startup,
            video_present_early_tolerance: config.video_present_early_tolerance,
            video_idle_poll_interval: config.video_idle_poll_interval,
            pending_audio_metadata_worker: None,
            pending_audio_decode_worker: None,
            pending_audio_stream_worker: None,
            pending_audio_metadata_retry: None,
            pending_audio_stream_retry: None,
            is_buffering: false,
            buffering_candidate_since: None,
            last_audio_output_poll: Instant::now(),
            retry_policy: config.retry_policy,
            resilience_metrics: PlayerResilienceMetricsTracker::default(),
            events: VecDeque::new(),
        };

        let (initial_media_position, initial_video_position) = initial_restart_positions(
            runtime.source_audio_track.as_ref(),
            initial_frame.presentation_time,
        );
        runtime.initial_media_position = initial_media_position;
        runtime.initial_video_position = initial_video_position;
        runtime.set_playback_clock(initial_frame.presentation_time);
        runtime.maybe_start_audio_metadata_probe_worker()?;
        runtime.maybe_start_audio_decode_worker()?;
        runtime.ensure_audio_output(initial_media_position, runtime.playback_rate)?;
        runtime.fill_next_frame()?;
        runtime.refresh_playback_finished();
        runtime.emit_event(PlayerRuntimeEvent::Initialized(startup.clone()));
        runtime.emit_event(PlayerRuntimeEvent::MetadataReady(
            runtime.media_info.clone(),
        ));
        runtime.emit_event(PlayerRuntimeEvent::FirstFrameReady(FirstFrameReady {
            presentation_time: initial_frame.presentation_time,
            width: initial_frame.width,
            height: initial_frame.height,
        }));
        runtime.emit_event(PlayerRuntimeEvent::PlaybackStateChanged(
            runtime.presentation_state(),
        ));

        Ok(PlayerRuntimeAdapterBootstrap {
            runtime: Box::new(runtime),
            initial_frame: Some(initial_frame),
            startup,
        })
    }

    fn try_dispatch(
        &mut self,
        command: PlayerRuntimeCommand,
    ) -> PlayerRuntimeResult<(bool, Option<DecodedVideoFrame>)> {
        self.poll_scheduled_retries();
        self.poll_audio_output_device();
        self.poll_audio_metadata_worker();
        self.poll_audio_decode_worker();
        self.poll_audio_stream_worker();

        match command {
            PlayerRuntimeCommand::Play => self.play(),
            PlayerRuntimeCommand::Pause => Ok((self.pause()?, None)),
            PlayerRuntimeCommand::TogglePause => self.toggle_pause(),
            PlayerRuntimeCommand::SeekTo { position } => Ok((true, self.seek_to(position)?)),
            PlayerRuntimeCommand::SetPlaybackRate { rate } => {
                Ok((self.set_playback_rate(rate)?, None))
            }
            PlayerRuntimeCommand::SetVideoTrackSelection { .. }
            | PlayerRuntimeCommand::SetAudioTrackSelection { .. }
            | PlayerRuntimeCommand::SetSubtitleTrackSelection { .. }
            | PlayerRuntimeCommand::SetAbrPolicy { .. } => Err(PlayerRuntimeError::new(
                PlayerRuntimeErrorCode::Unsupported,
                "track selection and ABR control are not implemented for the software desktop runtime",
            )),
            PlayerRuntimeCommand::Stop => self.stop(),
        }
    }

    fn play(&mut self) -> PlayerRuntimeResult<(bool, Option<DecodedVideoFrame>)> {
        match self.presentation_state() {
            PresentationState::Playing => Ok((false, None)),
            PresentationState::Finished => {
                let frame = self.rewind_to_ready(PresentationState::Finished)?;
                let previous_state = self.presentation_state();
                self.session.start_or_resume();
                if let Some(clock) = self.playback_clock.as_mut() {
                    clock.resume();
                }
                if let Some(audio_sink) = self.audio_sink.as_mut() {
                    audio_sink.play();
                }
                self.emit_state_change_if_needed(previous_state);
                self.update_buffering_state();
                Ok((true, frame))
            }
            PresentationState::Ready | PresentationState::Paused => {
                let previous_state = self.presentation_state();
                self.session.start_or_resume();
                if let Some(clock) = self.playback_clock.as_mut() {
                    clock.resume();
                }
                if let Some(audio_sink) = self.audio_sink.as_mut() {
                    audio_sink.play();
                }
                self.emit_state_change_if_needed(previous_state);
                self.update_buffering_state();
                Ok((true, None))
            }
        }
    }

    fn pause(&mut self) -> PlayerRuntimeResult<bool> {
        match self.presentation_state() {
            PresentationState::Playing => {
                let previous_state = self.presentation_state();
                self.session.pause_playback();
                if let Some(clock) = self.playback_clock.as_mut() {
                    clock.pause();
                }
                if let Some(audio_sink) = self.audio_sink.as_mut() {
                    audio_sink.pause();
                }
                self.emit_state_change_if_needed(previous_state);
                self.update_buffering_state();
                Ok(true)
            }
            PresentationState::Paused => Ok(false),
            PresentationState::Ready => {
                Err(self.invalid_state("pause is only valid after playback has started"))
            }
            PresentationState::Finished => {
                Err(self.invalid_state("pause is not valid after playback has finished"))
            }
        }
    }

    fn try_advance(&mut self) -> PlayerRuntimeResult<Option<DecodedVideoFrame>> {
        self.poll_scheduled_retries();
        self.poll_audio_output_device();
        self.poll_audio_metadata_worker();
        self.poll_audio_decode_worker();
        self.poll_audio_stream_worker();

        if !self.session.is_started() || self.session.is_paused() || self.session.is_finished() {
            return Ok(None);
        }

        self.fill_next_frame()?;
        if self.playback_position().is_none() {
            return Ok(None);
        }

        let mut latest_frame = None;
        loop {
            let Some(next_frame) = self.next_frame.as_ref() else {
                break;
            };
            if !self.should_present_frame(next_frame.presentation_time) {
                break;
            }

            latest_frame = self.next_frame.take();
            self.fill_next_frame()?;
        }

        self.refresh_playback_finished();
        self.update_buffering_state();
        Ok(latest_frame)
    }

    fn toggle_pause(&mut self) -> PlayerRuntimeResult<(bool, Option<DecodedVideoFrame>)> {
        if matches!(
            self.presentation_state(),
            PresentationState::Ready | PresentationState::Paused | PresentationState::Finished
        ) {
            self.play()
        } else {
            Ok((self.pause()?, None))
        }
    }

    fn seek_to(&mut self, position: Duration) -> PlayerRuntimeResult<Option<DecodedVideoFrame>> {
        self.try_seek_to(position)
    }

    fn try_seek_to(
        &mut self,
        position: Duration,
    ) -> PlayerRuntimeResult<Option<DecodedVideoFrame>> {
        let previous_state = self.presentation_state();
        let target_position = self.session.clamp_seek_position(position);
        let seeked_frame = self
            .video_source
            .seek_to(target_position)
            .map_err(|error| {
                runtime_error(
                    PlayerRuntimeErrorCode::SeekFailure,
                    "failed to seek video source",
                    error,
                )
            })?;

        let Some(first_frame) = seeked_frame else {
            self.next_frame = None;
            self.video_end_of_stream = true;
            self.restore_seek_state(previous_state);
            self.ensure_audio_output(target_position, self.playback_rate)?;
            self.set_playback_clock(target_position);
            self.refresh_playback_finished();
            self.emit_state_change_if_needed(previous_state);
            self.emit_event(PlayerRuntimeEvent::SeekCompleted {
                position: target_position,
            });
            self.update_buffering_state();
            return Ok(None);
        };

        self.video_end_of_stream = false;
        self.next_frame = None;
        self.video_buffering_window = VideoBufferingWindow::Startup;
        self.audio_buffering_window = AudioBufferingWindow::Startup;
        self.fill_next_frame()?;
        self.restore_seek_state(previous_state);
        self.ensure_audio_output(target_position, self.playback_rate)?;
        self.set_playback_clock(first_frame.presentation_time);
        self.refresh_playback_finished();
        self.emit_state_change_if_needed(previous_state);
        self.emit_event(PlayerRuntimeEvent::SeekCompleted {
            position: first_frame.presentation_time,
        });
        self.update_buffering_state();

        Ok(Some(first_frame))
    }

    fn stop(&mut self) -> PlayerRuntimeResult<(bool, Option<DecodedVideoFrame>)> {
        self.try_stop()
    }

    fn try_stop(&mut self) -> PlayerRuntimeResult<(bool, Option<DecodedVideoFrame>)> {
        if self.presentation_state() == PresentationState::Ready
            && self.progress().position().is_zero()
        {
            return Ok((false, None));
        }

        let previous_state = self.presentation_state();
        let frame = self.rewind_to_ready(previous_state)?;

        Ok((true, frame))
    }

    fn ensure_audio_output(
        &mut self,
        position: Duration,
        playback_rate: f32,
    ) -> PlayerRuntimeResult<()> {
        let Some(output_config) = self.audio_output_config.clone() else {
            self.audio_sink = None;
            self.audio_sink_controller = None;
            self.cancel_pending_audio_stream_worker();
            return Ok(());
        };
        let source_audio_track = self.source_audio_track.as_ref();
        if source_audio_track.is_none() && !should_stream_audio_source_directly(&self.source) {
            self.audio_sink = None;
            self.audio_sink_controller = None;
            self.cancel_pending_audio_stream_worker();
            return Ok(());
        }

        if self.audio_sink.is_none() {
            let media_start = source_audio_track
                .map(|track| {
                    let sample_offset = track.sample_offset_for_position(position);
                    track.media_time_for_sample_offset(sample_offset)
                })
                .unwrap_or(position);
            let sink = AudioSink::new_default(
                output_config,
                media_start,
                playback_rate,
                self.session.should_hold_output(),
            )
            .map_err(|error| {
                runtime_error(
                    PlayerRuntimeErrorCode::AudioOutputUnavailable,
                    "failed to open default audio output",
                    error,
                )
            })?;
            self.audio_sink_controller = Some(sink.controller());
            self.audio_sink = Some(sink);
        }

        if source_audio_track.is_some() {
            self.start_audio_stream(position, playback_rate)?;
        } else if should_stream_audio_source_directly(&self.source) {
            self.start_remote_audio_stream(position, playback_rate)?;
        }
        self.sync_audio_output_state();
        Ok(())
    }

    fn maybe_start_audio_decode_worker(&mut self) -> PlayerRuntimeResult<()> {
        if should_stream_audio_source_directly(&self.source) {
            return Ok(());
        }

        if self.source_audio_track.is_some()
            || self.pending_audio_decode_worker.is_some()
            || self.audio_output_config.is_none()
            || self.media_info.best_audio.is_none()
        {
            return Ok(());
        }

        let Some(output_config) = self.audio_output_config.clone() else {
            return Ok(());
        };
        let backend = self.backend;
        let source = self.source.clone();
        let (sender, receiver) = mpsc::channel();

        thread::Builder::new()
            .name("player-audio-decode".to_owned())
            .spawn(move || {
                let result = backend
                    .decode_audio_track(source, output_config.sample_rate, output_config.channels)
                    .map_err(|error| error.to_string());
                let _ = sender.send(result);
            })
            .map_err(|error| {
                runtime_error(
                    PlayerRuntimeErrorCode::BackendFailure,
                    "failed to spawn deferred audio decode worker",
                    error,
                )
            })?;

        self.pending_audio_decode_worker = Some(PendingAudioDecodeWorker { receiver });
        Ok(())
    }

    fn maybe_start_audio_metadata_probe_worker(&mut self) -> PlayerRuntimeResult<()> {
        if !should_defer_media_probe_for_source(&self.source)
            || should_stream_audio_source_directly(&self.source)
            || self.pending_audio_metadata_worker.is_some()
            || self.media_info.best_audio.is_some()
        {
            return Ok(());
        }

        self.start_audio_metadata_probe_worker(0)
    }

    fn start_audio_metadata_probe_worker(&mut self, retry_attempt: u32) -> PlayerRuntimeResult<()> {
        if self.pending_audio_metadata_worker.is_some() {
            return Ok(());
        }

        let backend = self.backend;
        let source = self.source.clone();
        let (sender, receiver) = mpsc::channel();

        thread::Builder::new()
            .name("player-audio-metadata-probe".to_owned())
            .spawn(move || {
                let result = backend
                    .probe_audio_decode_source_with_interrupt(source, None)
                    .map_err(|error| error.to_string());
                let _ = sender.send(result);
            })
            .map_err(|error| {
                runtime_error(
                    PlayerRuntimeErrorCode::BackendFailure,
                    "failed to spawn deferred audio metadata probe worker",
                    error,
                )
            })?;

        self.pending_audio_metadata_retry = None;
        self.pending_audio_metadata_worker = Some(PendingAudioMetadataWorker {
            retry_attempt,
            receiver,
        });
        Ok(())
    }

    fn set_playback_clock(&mut self, media_start: Duration) {
        let mut clock = PlaybackClock::new(media_start, self.playback_rate);
        if self.session.should_hold_output() {
            clock.pause();
        }
        self.playback_clock = Some(clock);
    }

    fn playback_position(&self) -> Option<Duration> {
        select_playback_position(
            self.audio_sink.as_ref().map(AudioSink::playback_position),
            self.playback_clock
                .as_ref()
                .map(PlaybackClock::playback_position),
        )
    }

    fn should_present_frame(&self, media_time: Duration) -> bool {
        let Some(playback_position) = self.playback_position() else {
            return false;
        };
        let scheduled_time = media_time.saturating_sub(self.video_present_early_tolerance);

        playback_position >= scheduled_time
    }

    fn refresh_playback_finished(&mut self) {
        let previous_state = self.presentation_state();
        let video_finished = self.video_end_of_stream && self.next_frame.is_none();
        let audio_finished = self
            .audio_sink
            .as_ref()
            .map(AudioSink::is_finished)
            .unwrap_or(true);
        self.session.sync_finished(video_finished, audio_finished);
        if self.session.is_finished() && previous_state != PresentationState::Finished {
            self.emit_event(PlayerRuntimeEvent::Ended);
        }
        self.emit_state_change_if_needed(previous_state);
    }

    fn fill_next_frame(&mut self) -> PlayerRuntimeResult<()> {
        if self.next_frame.is_some() || self.video_end_of_stream {
            return Ok(());
        }

        match self.video_source.try_recv_frame().map_err(|error| {
            runtime_error(
                PlayerRuntimeErrorCode::DecodeFailure,
                "failed to fetch decoded video frame from buffer",
                error,
            )
        })? {
            BufferedFramePoll::Ready(frame) => {
                self.next_frame = Some(frame);
            }
            BufferedFramePoll::Pending => {}
            BufferedFramePoll::EndOfStream => {
                self.video_end_of_stream = true;
            }
        }

        Ok(())
    }

    fn emit_event(&mut self, event: PlayerRuntimeEvent) {
        self.observe_resilience_event(&event);
        self.events.push_back(event);
    }

    fn emit_state_change_if_needed(&mut self, previous_state: PresentationState) {
        let current_state = self.presentation_state();
        if current_state != previous_state {
            self.emit_event(PlayerRuntimeEvent::PlaybackStateChanged(current_state));
        }
    }

    fn rewind_to_ready(
        &mut self,
        previous_state: PresentationState,
    ) -> PlayerRuntimeResult<Option<DecodedVideoFrame>> {
        self.session.reset_to_ready();

        let Some(first_frame) = self
            .video_source
            .seek_to(self.initial_video_position)
            .map_err(|error| {
                runtime_error(
                    PlayerRuntimeErrorCode::SeekFailure,
                    "failed to seek media source to the beginning",
                    error,
                )
            })?
        else {
            return Err(PlayerRuntimeError::new(
                PlayerRuntimeErrorCode::DecodeFailure,
                "rewind did not produce an initial frame",
            ));
        };

        self.video_end_of_stream = false;
        self.next_frame = None;
        self.video_buffering_window = VideoBufferingWindow::Startup;
        self.audio_buffering_window = AudioBufferingWindow::Startup;
        self.ensure_audio_output(self.initial_media_position, self.playback_rate)?;
        self.set_playback_clock(first_frame.presentation_time);
        self.fill_next_frame()?;
        self.refresh_playback_finished();
        self.emit_state_change_if_needed(previous_state);
        self.update_buffering_state();

        Ok(Some(first_frame))
    }

    fn restore_seek_state(&mut self, previous_state: PresentationState) {
        match previous_state {
            PresentationState::Playing => {
                self.session.start_or_resume();
                self.session.set_finished(false);
            }
            PresentationState::Paused => {
                self.session.start_or_resume();
                self.session.pause_playback();
                self.session.set_finished(false);
            }
            PresentationState::Ready | PresentationState::Finished => {
                self.session.reset_to_ready();
            }
        }
    }

    fn fail<T>(&mut self, error: PlayerRuntimeError) -> PlayerRuntimeResult<T> {
        self.emit_event(PlayerRuntimeEvent::Error(error.clone()));
        Err(error)
    }

    fn invalid_state(&self, message: &str) -> PlayerRuntimeError {
        PlayerRuntimeError::new(PlayerRuntimeErrorCode::InvalidState, message)
    }

    fn set_playback_rate(&mut self, rate: f32) -> PlayerRuntimeResult<bool> {
        let rate = validate_playback_rate(rate)?;
        if (self.playback_rate - rate).abs() < 0.001 {
            return Ok(false);
        }

        let current_position = self
            .playback_position()
            .unwrap_or_else(|| self.progress().position());
        self.ensure_audio_output(current_position, rate)?;
        self.playback_rate = rate;
        self.set_playback_clock(current_position);
        self.refresh_playback_finished();
        self.emit_event(PlayerRuntimeEvent::PlaybackRateChanged { rate });
        self.update_buffering_state();

        Ok(true)
    }

    fn poll_audio_stream_worker(&mut self) {
        let Some(worker) = self.pending_audio_stream_worker.take() else {
            return;
        };

        let is_active_generation = self
            .audio_sink_controller
            .as_ref()
            .map(|controller| controller.is_generation_active(worker.generation))
            .unwrap_or(false);

        match worker.receiver.try_recv() {
            Ok(Ok(AudioStreamWorkerEvent::Metadata(probe))) => {
                if merge_audio_probe_into_media_info(&mut self.media_info, &probe) {
                    self.emit_event(PlayerRuntimeEvent::MetadataReady(self.media_info.clone()));
                }
                self.pending_audio_stream_worker = Some(worker);
            }
            Ok(Ok(AudioStreamWorkerEvent::Finished)) => {}
            Ok(Err(error)) => {
                if is_active_generation {
                    if let Some(controller) = self.audio_sink_controller.as_ref() {
                        controller.finish_generation(worker.generation);
                    }
                    let retry_position = self
                        .playback_position()
                        .unwrap_or_else(|| self.progress().position());
                    if !self.schedule_remote_audio_stream_retry(
                        worker.retry_attempt.saturating_add(1),
                        retry_position,
                        self.playback_rate,
                    ) {
                        self.emit_event(PlayerRuntimeEvent::Error(PlayerRuntimeError::new(
                            PlayerRuntimeErrorCode::DecodeFailure,
                            format!("failed to stream retimed audio for playback: {error}"),
                        )));
                    }
                    self.refresh_playback_finished();
                }
            }
            Err(TryRecvError::Empty) => {
                self.pending_audio_stream_worker = Some(worker);
            }
            Err(TryRecvError::Disconnected) => {
                if is_active_generation {
                    if let Some(controller) = self.audio_sink_controller.as_ref() {
                        controller.finish_generation(worker.generation);
                    }
                    let retry_position = self
                        .playback_position()
                        .unwrap_or_else(|| self.progress().position());
                    if !self.schedule_remote_audio_stream_retry(
                        worker.retry_attempt.saturating_add(1),
                        retry_position,
                        self.playback_rate,
                    ) {
                        self.emit_event(PlayerRuntimeEvent::Error(PlayerRuntimeError::new(
                            PlayerRuntimeErrorCode::BackendFailure,
                            "audio stream worker disconnected before completing playback",
                        )));
                    }
                    self.refresh_playback_finished();
                }
            }
        }

        self.update_buffering_state();
    }

    fn poll_audio_metadata_worker(&mut self) {
        let Some(worker) = self.pending_audio_metadata_worker.take() else {
            return;
        };

        match worker.receiver.try_recv() {
            Ok(Ok(probe)) => {
                if merge_audio_probe_into_media_info(&mut self.media_info, &probe) {
                    self.emit_event(PlayerRuntimeEvent::MetadataReady(self.media_info.clone()));
                }
            }
            Ok(Err(error)) => {
                if !self.schedule_audio_metadata_retry(worker.retry_attempt.saturating_add(1)) {
                    self.emit_event(PlayerRuntimeEvent::Error(PlayerRuntimeError::new(
                        PlayerRuntimeErrorCode::BackendFailure,
                        format!("failed to probe remote audio metadata for playback: {error}"),
                    )));
                }
            }
            Err(TryRecvError::Empty) => {
                self.pending_audio_metadata_worker = Some(worker);
            }
            Err(TryRecvError::Disconnected) => {
                if !self.schedule_audio_metadata_retry(worker.retry_attempt.saturating_add(1)) {
                    self.emit_event(PlayerRuntimeEvent::Error(PlayerRuntimeError::new(
                        PlayerRuntimeErrorCode::BackendFailure,
                        "audio metadata probe worker disconnected before producing playback metadata",
                    )));
                }
            }
        }
    }

    fn poll_audio_decode_worker(&mut self) {
        let Some(worker) = self.pending_audio_decode_worker.take() else {
            return;
        };

        match worker.receiver.try_recv() {
            Ok(Ok(track)) => {
                self.source_audio_track = Some(track);
                let current_position = self
                    .playback_position()
                    .unwrap_or_else(|| self.progress().position());
                if let Err(error) = self.ensure_audio_output(current_position, self.playback_rate) {
                    self.emit_event(PlayerRuntimeEvent::Error(error));
                }
                self.refresh_playback_finished();
            }
            Ok(Err(error)) => {
                self.emit_event(PlayerRuntimeEvent::Error(PlayerRuntimeError::new(
                    PlayerRuntimeErrorCode::DecodeFailure,
                    format!("failed to decode audio track for playback: {error}"),
                )));
                self.refresh_playback_finished();
            }
            Err(TryRecvError::Empty) => {
                self.pending_audio_decode_worker = Some(worker);
            }
            Err(TryRecvError::Disconnected) => {
                self.emit_event(PlayerRuntimeEvent::Error(PlayerRuntimeError::new(
                    PlayerRuntimeErrorCode::BackendFailure,
                    "audio decode worker disconnected before producing playback data",
                )));
                self.refresh_playback_finished();
            }
        }

        self.update_buffering_state();
    }

    fn start_audio_stream(
        &mut self,
        position: Duration,
        playback_rate: f32,
    ) -> PlayerRuntimeResult<()> {
        self.cancel_pending_audio_stream_worker();

        let Some(source_track) = self.source_audio_track.clone() else {
            return Ok(());
        };
        let Some(controller) = self.audio_sink_controller.clone() else {
            return Ok(());
        };

        let sample_offset = source_track.sample_offset_for_position(position);
        let media_start = source_track.media_time_for_sample_offset(sample_offset);
        let generation = controller.begin_generation(media_start, playback_rate);
        let backend = self.backend;
        let (sender, receiver) = mpsc::channel();
        let target_buffer_samples = buffered_sample_target(
            source_track.sample_rate,
            source_track.channels,
            self.audio_stream_target_buffer_duration,
        );

        thread::Builder::new()
            .name("player-audio-stream".to_owned())
            .spawn(move || {
                let range = sample_offset..source_track.samples.len();
                let emit_chunk = |chunk: Vec<f32>| -> anyhow::Result<bool> {
                    if !wait_for_audio_buffer_window(&controller, generation, target_buffer_samples)
                    {
                        return Ok(false);
                    }

                    controller.append_samples(generation, chunk)
                };
                let result: Result<AudioStreamWorkerEvent, String> =
                    if (playback_rate - DEFAULT_PLAYBACK_RATE).abs() < 0.000_001 {
                        stream_direct_audio_track_range(&source_track, range, emit_chunk)
                    } else {
                        backend.stream_retime_audio_track_range(
                            &source_track,
                            playback_rate,
                            range,
                            emit_chunk,
                        )
                    }
                    .and_then(|_| {
                        if controller.is_generation_active(generation) {
                            controller.finish_generation(generation);
                        }
                        Ok(())
                    })
                    .map(|_| AudioStreamWorkerEvent::Finished)
                    .map_err(|error| error.to_string());
                let _ = sender.send(result);
            })
            .map_err(|error| {
                runtime_error(
                    PlayerRuntimeErrorCode::BackendFailure,
                    "failed to spawn streaming audio worker",
                    error,
                )
            })?;

        self.pending_audio_stream_worker = Some(PendingAudioStreamWorker {
            generation,
            retry_attempt: 0,
            receiver,
            interrupt_flag: None,
        });
        self.pending_audio_stream_retry = None;
        self.audio_buffering_window = AudioBufferingWindow::Startup;
        self.update_buffering_state();
        Ok(())
    }

    fn start_remote_audio_stream(
        &mut self,
        position: Duration,
        playback_rate: f32,
    ) -> PlayerRuntimeResult<()> {
        self.start_remote_audio_stream_with_retry_attempt(position, playback_rate, 0)
    }

    fn start_remote_audio_stream_with_retry_attempt(
        &mut self,
        position: Duration,
        playback_rate: f32,
        retry_attempt: u32,
    ) -> PlayerRuntimeResult<()> {
        self.cancel_pending_audio_stream_worker();

        let Some(controller) = self.audio_sink_controller.clone() else {
            return Ok(());
        };
        let Some(output_config) = self.audio_output_config.clone() else {
            return Ok(());
        };

        let generation = controller.begin_generation(position, playback_rate);
        let backend = self.backend;
        let source = self.source.clone();
        let (sender, receiver) = mpsc::channel();
        let interrupt_flag = Arc::new(AtomicBool::new(false));
        let worker_interrupt_flag = interrupt_flag.clone();
        let target_buffer_samples = buffered_sample_target(
            output_config.sample_rate,
            output_config.channels,
            self.audio_stream_target_buffer_duration,
        );

        thread::Builder::new()
            .name("player-remote-audio-stream".to_owned())
            .spawn(move || {
                let emit_metadata = |probe: MediaProbe| -> anyhow::Result<()> {
                    let _ = sender.send(Ok(AudioStreamWorkerEvent::Metadata(probe)));
                    Ok(())
                };
                let emit_chunk = |chunk: Vec<f32>| -> anyhow::Result<bool> {
                    if !wait_for_audio_buffer_window(&controller, generation, target_buffer_samples)
                    {
                        return Ok(false);
                    }

                    controller.append_samples(generation, chunk)
                };
                let result = backend
                    .stream_audio_source_with_playback_rate_and_interrupt(
                        source,
                        output_config.sample_rate,
                        output_config.channels,
                        playback_rate,
                        position,
                        Some(worker_interrupt_flag),
                        emit_metadata,
                        emit_chunk,
                    )
                    .and_then(|_| {
                        if controller.is_generation_active(generation) {
                            controller.finish_generation(generation);
                        }
                        Ok(())
                    })
                    .map(|_| AudioStreamWorkerEvent::Finished)
                    .map_err(|error| error.to_string());
                let _ = sender.send(result);
            })
            .map_err(|error| {
                runtime_error(
                    PlayerRuntimeErrorCode::BackendFailure,
                    "failed to spawn remote audio stream worker",
                    error,
                )
            })?;

        self.pending_audio_stream_worker = Some(PendingAudioStreamWorker {
            generation,
            retry_attempt,
            receiver,
            interrupt_flag: Some(interrupt_flag),
        });
        self.pending_audio_stream_retry = None;
        self.audio_buffering_window = AudioBufferingWindow::Startup;
        self.update_buffering_state();
        Ok(())
    }

    fn cancel_pending_audio_stream_worker(&mut self) {
        if let Some(worker) = self.pending_audio_stream_worker.take() {
            if let Some(interrupt_flag) = worker.interrupt_flag {
                interrupt_flag.store(true, Ordering::SeqCst);
            }
        }
        self.pending_audio_stream_retry = None;
    }

    fn sync_audio_output_state(&mut self) {
        let Some(audio_sink) = self.audio_sink.as_mut() else {
            return;
        };

        if self.session.should_hold_output() {
            audio_sink.pause();
        } else {
            audio_sink.play();
        }
    }

    fn poll_audio_output_device(&mut self) {
        if self.last_audio_output_poll.elapsed() < AUDIO_OUTPUT_POLL_INTERVAL {
            return;
        }
        self.last_audio_output_poll = Instant::now();

        if self.source_audio_track.is_none() && !should_stream_audio_source_directly(&self.source) {
            return;
        }

        let descriptor = detect_default_output();
        if !audio_output_descriptor_changed(&self.audio_output_descriptor, &descriptor) {
            return;
        }

        self.handle_audio_output_change(descriptor);
    }

    fn handle_audio_output_change(&mut self, descriptor: AudioOutputDescriptor) {
        let current_position = self
            .playback_position()
            .unwrap_or_else(|| self.progress().position());
        let current_rate = self.playback_rate;

        self.audio_output_descriptor = descriptor.clone();
        self.audio_output_config = descriptor.default_output_config.clone();
        self.audio_sink = None;
        self.audio_sink_controller = None;
        self.cancel_pending_audio_stream_worker();
        self.set_playback_clock(current_position);

        if self.audio_output_config.is_some() {
            if let Err(error) = self.maybe_start_audio_decode_worker() {
                self.emit_event(PlayerRuntimeEvent::Error(error));
            }
        }

        match self.ensure_audio_output(current_position, current_rate) {
            Ok(()) => {
                self.emit_event(PlayerRuntimeEvent::AudioOutputChanged(audio_output_info(
                    &self.audio_output_descriptor,
                )));
                self.refresh_playback_finished();
                self.update_buffering_state();
            }
            Err(error) => {
                self.audio_output_config = None;
                self.audio_sink = None;
                self.audio_sink_controller = None;
                self.cancel_pending_audio_stream_worker();
                self.emit_event(PlayerRuntimeEvent::AudioOutputChanged(None));
                self.emit_event(PlayerRuntimeEvent::Error(error));
                self.refresh_playback_finished();
                self.update_buffering_state();
            }
        }
    }

    fn raw_is_buffering(&self) -> bool {
        if !self.session.is_started() || self.session.is_paused() || self.session.is_finished() {
            return false;
        }

        let waiting_for_video = !self.video_end_of_stream
            && match self.video_buffering_window {
                VideoBufferingWindow::Startup => {
                    self.buffered_video_frame_count()
                        < self.video_playback_start_buffer_frames.max(1)
                }
                VideoBufferingWindow::Rebuffer => {
                    self.buffered_video_frame_count() < self.video_rebuffer_frames.max(1)
                }
            };
        let waiting_for_audio = self
            .pending_audio_stream_worker
            .as_ref()
            .and_then(|worker| {
                self.audio_sink_controller.as_ref().and_then(|controller| {
                    let required_samples = self
                        .audio_output_config
                        .as_ref()
                        .map(|output_config| {
                            buffered_sample_target(
                                output_config.sample_rate,
                                output_config.channels,
                                self.current_audio_buffer_requirement(),
                            )
                        })
                        .unwrap_or(0);
                    controller
                        .buffered_samples(worker.generation)
                        .map(|buffered_samples| buffered_samples < required_samples.max(1))
                })
            })
            .unwrap_or(false);
        let waiting_for_audio_retry = should_stream_audio_source_directly(&self.source)
            && self.pending_audio_stream_retry.is_some();

        waiting_for_video || waiting_for_audio || waiting_for_audio_retry
    }

    fn update_buffering_state(&mut self) {
        let raw_is_buffering = self.raw_is_buffering();
        if !raw_is_buffering {
            if self.video_buffering_window == VideoBufferingWindow::Startup {
                self.video_buffering_window = VideoBufferingWindow::Rebuffer;
            }
            if self.audio_buffering_window == AudioBufferingWindow::Startup {
                self.audio_buffering_window = AudioBufferingWindow::Rebuffer;
            }
            self.buffering_candidate_since = None;
            if self.is_buffering {
                self.is_buffering = false;
                self.emit_event(PlayerRuntimeEvent::BufferingChanged { buffering: false });
            }
            return;
        }

        if self.is_buffering {
            return;
        }

        let now = Instant::now();
        let candidate_since = self.buffering_candidate_since.get_or_insert(now);
        if now.saturating_duration_since(*candidate_since) >= SOFTWARE_BUFFERING_GRACE_PERIOD {
            self.is_buffering = true;
            self.emit_event(PlayerRuntimeEvent::BufferingChanged { buffering: true });
        }
    }

    fn current_audio_buffer_requirement(&self) -> Duration {
        match self.audio_buffering_window {
            AudioBufferingWindow::Startup => self.audio_playback_start_buffer_duration,
            AudioBufferingWindow::Rebuffer => self.audio_rebuffer_duration,
        }
    }

    fn buffered_video_frame_count(&self) -> usize {
        self.video_source.buffered_frame_count() + usize::from(self.next_frame.is_some())
    }

    fn next_retry_deadline(&self) -> Option<Instant> {
        match (
            self.pending_audio_metadata_retry,
            self.pending_audio_stream_retry,
        ) {
            (Some(metadata), Some(stream)) => Some(metadata.due_at.min(stream.due_at)),
            (Some(metadata), None) => Some(metadata.due_at),
            (None, Some(stream)) => Some(stream.due_at),
            (None, None) => None,
        }
    }

    fn poll_scheduled_retries(&mut self) {
        let now = Instant::now();

        if let Some(retry) = self.pending_audio_metadata_retry {
            if now >= retry.due_at
                && self.pending_audio_metadata_worker.is_none()
                && self.media_info.best_audio.is_none()
            {
                self.pending_audio_metadata_retry = None;
                if let Err(error) = self.start_audio_metadata_probe_worker(retry.attempt) {
                    self.emit_event(PlayerRuntimeEvent::Error(error));
                }
            }
        }

        if let Some(retry) = self.pending_audio_stream_retry {
            if now >= retry.due_at
                && self.pending_audio_stream_worker.is_none()
                && should_stream_audio_source_directly(&self.source)
                && self.audio_output_config.is_some()
                && self.audio_sink_controller.is_some()
            {
                self.pending_audio_stream_retry = None;
                if let Err(error) = self.start_remote_audio_stream_with_retry_attempt(
                    retry.position,
                    retry.playback_rate,
                    retry.attempt,
                ) {
                    self.emit_event(PlayerRuntimeEvent::Error(error));
                }
            }
        }
    }

    fn schedule_audio_metadata_retry(&mut self, attempt: u32) -> bool {
        if !should_defer_media_probe_for_source(&self.source) {
            return false;
        }
        let Some(delay) = retry_delay_for_attempt(&self.retry_policy, attempt) else {
            return false;
        };
        self.pending_audio_metadata_retry = Some(ScheduledRetry {
            attempt,
            due_at: Instant::now() + delay,
        });
        self.emit_event(PlayerRuntimeEvent::RetryScheduled { attempt, delay });
        true
    }

    fn schedule_remote_audio_stream_retry(
        &mut self,
        attempt: u32,
        position: Duration,
        playback_rate: f32,
    ) -> bool {
        if !should_stream_audio_source_directly(&self.source) {
            return false;
        }
        let Some(delay) = retry_delay_for_attempt(&self.retry_policy, attempt) else {
            return false;
        };
        self.pending_audio_stream_retry = Some(ScheduledAudioStreamRetry {
            attempt,
            due_at: Instant::now() + delay,
            position,
            playback_rate,
        });
        self.emit_event(PlayerRuntimeEvent::RetryScheduled { attempt, delay });
        true
    }

    fn observe_resilience_event(&mut self, event: &PlayerRuntimeEvent) {
        observe_resilience_metrics_for_event(&mut self.resilience_metrics, event);
    }
}

fn buffered_sample_target(sample_rate: u32, channels: u16, duration: Duration) -> usize {
    let frames = (duration.as_secs_f64() * f64::from(sample_rate.max(1))).ceil() as usize;
    frames.saturating_mul(usize::from(channels.max(1)))
}

fn resolved_video_prefetch_capacity(base_capacity: usize, policy: &PlayerBufferingPolicy) -> usize {
    let Some(target_duration) = policy
        .buffer_for_rebuffer
        .or(policy.buffer_for_playback)
        .map(|duration| duration.saturating_add(DEFAULT_VIDEO_BUFFER_HEADROOM_DURATION))
    else {
        return base_capacity.max(1);
    };

    let estimated_frames =
        (target_duration.as_secs_f64() * DEFAULT_VIDEO_FRAME_RATE_ESTIMATE).ceil() as usize;
    estimated_frames
        .max(base_capacity.max(1))
        .min(MAX_DESKTOP_VIDEO_PREFETCH_CAPACITY)
}

fn resolved_video_prefetch_limit(
    media_info: &PlayerMediaInfo,
    buffering_capacity: usize,
    cache_policy: &PlayerCachePolicy,
) -> usize {
    if media_info.source_kind != MediaSourceKind::Remote {
        return buffering_capacity.max(1);
    }

    let Some(max_memory_bytes) = cache_policy.max_memory_bytes else {
        return buffering_capacity.max(1);
    };

    if max_memory_bytes == 0 {
        return 1;
    }

    let estimated_frame_bytes = estimated_video_frame_memory_bytes(media_info).max(1) as u64;
    let effective_memory_budget = max_memory_bytes
        .saturating_mul(DESKTOP_ACTIVE_VIDEO_PREFETCH_MEMORY_SCALE)
        .max(estimated_frame_bytes);
    let frames_by_budget = (effective_memory_budget / estimated_frame_bytes).max(1);
    let frames_by_budget = usize::try_from(frames_by_budget).unwrap_or(usize::MAX);

    frames_by_budget.clamp(1, buffering_capacity.max(1))
}

fn resolved_video_playback_start_buffer_frames(
    media_info: &PlayerMediaInfo,
    video_prefetch_capacity: usize,
    policy: &PlayerBufferingPolicy,
) -> usize {
    let Some(target_duration) = policy.buffer_for_playback.or(policy.buffer_for_rebuffer) else {
        return 1;
    };

    let frame_rate = media_info
        .best_video
        .as_ref()
        .and_then(|video| video.frame_rate)
        .filter(|rate| rate.is_finite() && *rate > 0.0)
        .unwrap_or(DEFAULT_VIDEO_FRAME_RATE_ESTIMATE);
    let required_frames = (target_duration.as_secs_f64() * frame_rate).ceil() as usize;

    required_frames.clamp(1, video_prefetch_capacity.max(1))
}

fn resolved_video_rebuffer_frames(
    media_info: &PlayerMediaInfo,
    video_prefetch_capacity: usize,
    policy: &PlayerBufferingPolicy,
) -> usize {
    let Some(target_duration) = policy.buffer_for_rebuffer.or(policy.buffer_for_playback) else {
        return 1;
    };

    let frame_rate = media_info
        .best_video
        .as_ref()
        .and_then(|video| video.frame_rate)
        .filter(|rate| rate.is_finite() && *rate > 0.0)
        .unwrap_or(DEFAULT_VIDEO_FRAME_RATE_ESTIMATE);
    let required_frames = (target_duration.as_secs_f64() * frame_rate).ceil() as usize;

    required_frames.clamp(1, video_prefetch_capacity.max(1))
}

fn estimated_video_frame_memory_bytes(media_info: &PlayerMediaInfo) -> usize {
    media_info
        .best_video
        .as_ref()
        .and_then(|video| {
            let width = usize::try_from(video.width).ok()?;
            let height = usize::try_from(video.height).ok()?;
            let pixels = width.checked_mul(height)?;
            pixels.checked_mul(3)?.checked_div(2)
        })
        .filter(|bytes| *bytes > 0)
        .unwrap_or(DEFAULT_VIDEO_FRAME_MEMORY_ESTIMATE_BYTES)
}

fn retry_delay_for_attempt(policy: &PlayerRetryPolicy, attempt: u32) -> Option<Duration> {
    if attempt == 0 {
        return None;
    }

    if policy
        .max_attempts
        .is_some_and(|max_attempts| attempt > max_attempts)
    {
        return None;
    }

    let multiplier = match policy.backoff {
        PlayerRetryBackoff::Fixed => 1,
        PlayerRetryBackoff::Linear => attempt,
        PlayerRetryBackoff::Exponential => 1u32
            .checked_shl(attempt.saturating_sub(1))
            .unwrap_or(u32::MAX),
    };

    Some(
        policy
            .base_delay
            .saturating_mul(multiplier)
            .min(policy.max_delay),
    )
}

fn resolved_audio_playback_start_buffer_duration(policy: &PlayerBufferingPolicy) -> Duration {
    clamp_audio_buffer_duration(
        policy
            .buffer_for_playback
            .or(policy.buffer_for_rebuffer)
            .unwrap_or(DEFAULT_AUDIO_PLAYBACK_START_BUFFER_DURATION),
    )
}

fn resolved_audio_stream_target_buffer_duration(policy: &PlayerBufferingPolicy) -> Duration {
    clamp_audio_buffer_duration(
        policy
            .buffer_for_rebuffer
            .or(policy.buffer_for_playback)
            .map(|duration| duration.saturating_add(DEFAULT_AUDIO_BUFFER_HEADROOM_DURATION))
            .unwrap_or(DEFAULT_AUDIO_STREAM_TARGET_BUFFER_DURATION),
    )
}

fn resolved_audio_rebuffer_duration(policy: &PlayerBufferingPolicy) -> Duration {
    clamp_audio_buffer_duration(
        policy
            .buffer_for_rebuffer
            .unwrap_or(DEFAULT_AUDIO_REBUFFER_DURATION),
    )
}

fn clamp_audio_buffer_duration(duration: Duration) -> Duration {
    duration.clamp(
        DEFAULT_AUDIO_REBUFFER_DURATION,
        MAX_DESKTOP_AUDIO_BUFFER_DURATION,
    )
}

fn wait_for_audio_buffer_window(
    controller: &AudioSinkController,
    generation: u64,
    target_buffer_samples: usize,
) -> bool {
    loop {
        if !controller.is_generation_active(generation) {
            return false;
        }

        let buffered_samples = controller.buffered_samples(generation).unwrap_or(0);
        if buffered_samples <= target_buffer_samples {
            return true;
        }

        thread::sleep(AUDIO_STREAM_BACKPRESSURE_POLL_INTERVAL);
    }
}

fn stream_direct_audio_track_range<F>(
    source_track: &DecodedAudioTrack,
    sample_range: std::ops::Range<usize>,
    mut emit_chunk: F,
) -> anyhow::Result<()>
where
    F: FnMut(Vec<f32>) -> anyhow::Result<bool>,
{
    let channels = usize::from(source_track.channels.max(1));
    let start_sample =
        (sample_range.start - (sample_range.start % channels)).min(source_track.samples.len());
    let end_sample =
        (sample_range.end - (sample_range.end % channels)).min(source_track.samples.len());

    if end_sample <= start_sample {
        return Ok(());
    }

    let chunk_samples = AUDIO_STREAM_CHUNK_FRAMES.saturating_mul(channels.max(1));
    let mut chunk_start = start_sample;
    while chunk_start < end_sample {
        let chunk_end = chunk_start.saturating_add(chunk_samples).min(end_sample);
        if !emit_chunk(source_track.samples[chunk_start..chunk_end].to_vec())? {
            return Ok(());
        }
        chunk_start = chunk_end;
    }

    Ok(())
}

fn validate_playback_rate(rate: f32) -> PlayerRuntimeResult<f32> {
    if !rate.is_finite() {
        return Err(PlayerRuntimeError::new(
            PlayerRuntimeErrorCode::InvalidArgument,
            format!(
                "playback rate must be a finite number between {MIN_PLAYBACK_RATE:.1}x and {MAX_PLAYBACK_RATE:.1}x"
            ),
        ));
    }

    if !(MIN_PLAYBACK_RATE..=MAX_PLAYBACK_RATE).contains(&rate) {
        return Err(PlayerRuntimeError::new(
            PlayerRuntimeErrorCode::InvalidArgument,
            format!(
                "playback rate {rate:.2}x is out of range; this player accepts {MIN_PLAYBACK_RATE:.1}x to {MAX_PLAYBACK_RATE:.1}x, and {MIN_PLAYBACK_RATE:.1}x to {NATURAL_PLAYBACK_RATE_MAX:.1}x is the most natural-sounding range"
            ),
        ));
    }

    Ok(rate)
}

fn audio_output_descriptor_changed(
    current: &AudioOutputDescriptor,
    next: &AudioOutputDescriptor,
) -> bool {
    if current.default_output_device != next.default_output_device {
        return true;
    }

    audio_output_config_signature(current.default_output_config.as_ref())
        != audio_output_config_signature(next.default_output_config.as_ref())
}

fn audio_output_config_signature(config: Option<&AudioOutputConfig>) -> Option<(u16, u32, String)> {
    config.map(|config| {
        (
            config.channels,
            config.sample_rate,
            format!("{:?}", config.sample_format),
        )
    })
}

fn audio_output_info(descriptor: &AudioOutputDescriptor) -> Option<PlayerAudioOutputInfo> {
    let device_name = descriptor.default_output_device.clone();
    let channels = descriptor
        .default_output_config
        .as_ref()
        .map(|config| config.channels);
    let sample_rate = descriptor
        .default_output_config
        .as_ref()
        .map(|config| config.sample_rate);
    let sample_format = descriptor
        .default_output_config
        .as_ref()
        .map(|config| format!("{:?}", config.sample_format));

    if device_name.is_none()
        && channels.is_none()
        && sample_rate.is_none()
        && sample_format.is_none()
    {
        return None;
    }

    Some(PlayerAudioOutputInfo {
        device_name,
        channels,
        sample_rate,
        sample_format,
    })
}

fn should_defer_audio_decode_for_source(source: &MediaSource) -> bool {
    matches!(source.kind(), MediaSourceKind::Remote)
}

fn should_defer_media_probe_for_source(source: &MediaSource) -> bool {
    source.kind() == MediaSourceKind::Remote && source.protocol() == MediaSourceProtocol::Hls
}

fn should_stream_audio_source_directly(source: &MediaSource) -> bool {
    source.kind() == MediaSourceKind::Remote && source.protocol() == MediaSourceProtocol::Hls
}

fn initial_restart_positions(
    source_audio_track: Option<&DecodedAudioTrack>,
    initial_video_position: Duration,
) -> (Duration, Duration) {
    let initial_media_position = source_audio_track
        .map(|track| track.presentation_time.min(initial_video_position))
        .unwrap_or(initial_video_position);
    (initial_media_position, initial_video_position)
}

fn decoded_audio_summary(track: &DecodedAudioTrack) -> DecodedAudioSummary {
    DecodedAudioSummary {
        channels: track.channels,
        sample_rate: track.sample_rate,
        duration: track.duration(),
    }
}

fn player_video_info(video: &VideoStreamProbe) -> PlayerVideoInfo {
    PlayerVideoInfo {
        codec: video.codec.clone(),
        width: video.width,
        height: video.height,
        frame_rate: video.frame_rate,
    }
}

fn player_audio_info(audio: &AudioStreamProbe) -> PlayerAudioInfo {
    PlayerAudioInfo {
        codec: audio.codec.clone(),
        sample_rate: audio.sample_rate,
        channels: audio.channels,
    }
}

fn player_video_decode_info(
    decode_info: &BackendVideoDecodeInfo,
) -> player_runtime::PlayerVideoDecodeInfo {
    player_runtime::PlayerVideoDecodeInfo {
        selected_mode: match decode_info.selected_mode {
            BackendVideoDecoderMode::Software => player_runtime::PlayerVideoDecodeMode::Software,
            BackendVideoDecoderMode::Hardware => player_runtime::PlayerVideoDecodeMode::Hardware,
        },
        hardware_available: decode_info.hardware_available,
        hardware_backend: decode_info.hardware_backend.clone(),
        fallback_reason: decode_info.fallback_reason.clone(),
    }
}

fn player_media_info(probe: &player_backend_ffmpeg::MediaProbe) -> PlayerMediaInfo {
    PlayerMediaInfo {
        source_uri: probe.source.uri().to_owned(),
        source_kind: probe.source.kind(),
        source_protocol: probe.source.protocol(),
        duration: probe.duration,
        bit_rate: probe.bit_rate,
        audio_streams: probe.audio_streams,
        video_streams: probe.video_streams,
        best_video: probe.best_video.as_ref().map(player_video_info),
        best_audio: probe.best_audio.as_ref().map(player_audio_info),
        track_catalog: Default::default(),
        track_selection: Default::default(),
    }
}

fn merge_audio_probe_into_media_info(media_info: &mut PlayerMediaInfo, probe: &MediaProbe) -> bool {
    let mut changed = false;
    let audio_streams = probe.audio_streams.max(media_info.audio_streams);
    if media_info.audio_streams != audio_streams {
        media_info.audio_streams = audio_streams;
        changed = true;
    }

    if let Some(best_audio) = probe.best_audio.as_ref().map(player_audio_info)
        && !player_audio_info_matches(media_info.best_audio.as_ref(), &best_audio)
    {
        media_info.best_audio = Some(best_audio);
        changed = true;
    }

    changed
}

fn observe_resilience_metrics_for_event(
    tracker: &mut PlayerResilienceMetricsTracker,
    event: &PlayerRuntimeEvent,
) {
    match event {
        PlayerRuntimeEvent::PlaybackStateChanged(state) => {
            tracker.observe_playback_state(*state);
        }
        PlayerRuntimeEvent::BufferingChanged { buffering } => {
            tracker.observe_buffering(*buffering);
        }
        PlayerRuntimeEvent::RetryScheduled { attempt, delay } => {
            tracker.observe_retry_scheduled(*attempt, *delay);
        }
        _ => {}
    }
}

fn player_audio_info_matches(current: Option<&PlayerAudioInfo>, next: &PlayerAudioInfo) -> bool {
    current.is_some_and(|current| {
        current.codec == next.codec
            && current.sample_rate == next.sample_rate
            && current.channels == next.channels
    })
}

fn unresolved_player_media_info(source: &MediaSource) -> PlayerMediaInfo {
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

fn software_desktop_capabilities() -> PlayerRuntimeAdapterCapabilities {
    PlayerRuntimeAdapterCapabilities {
        adapter_id: SOFTWARE_PLAYER_RUNTIME_ADAPTER_ID,
        backend_family: PlayerRuntimeAdapterBackendFamily::SoftwareDesktop,
        supports_audio_output: true,
        supports_frame_output: true,
        supports_external_video_surface: false,
        supports_seek: true,
        supports_stop: true,
        supports_playback_rate: true,
        playback_rate_min: Some(MIN_PLAYBACK_RATE),
        playback_rate_max: Some(MAX_PLAYBACK_RATE),
        natural_playback_rate_max: Some(NATURAL_PLAYBACK_RATE_MAX),
        supports_hardware_decode: false,
        supports_streaming: true,
        supports_hdr: false,
    }
}

fn with_adapter_id(
    mut capabilities: PlayerRuntimeAdapterCapabilities,
    adapter_id: &'static str,
) -> PlayerRuntimeAdapterCapabilities {
    capabilities.adapter_id = adapter_id;
    capabilities
}

fn runtime_error(
    code: PlayerRuntimeErrorCode,
    context: &str,
    error: impl std::fmt::Display,
) -> PlayerRuntimeError {
    PlayerRuntimeError::new(code, format!("{context}: {error}"))
}

fn select_playback_position(
    audio_position: Option<Duration>,
    clock_position: Option<Duration>,
) -> Option<Duration> {
    match (audio_position, clock_position) {
        (Some(audio_position), Some(clock_position)) => {
            if audio_position.saturating_add(AUDIO_CLOCK_STALL_TOLERANCE) < clock_position {
                Some(clock_position)
            } else {
                Some(audio_position)
            }
        }
        (Some(audio_position), None) => Some(audio_position),
        (None, Some(clock_position)) => Some(clock_position),
        (None, None) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use player_core::MediaSource;
    use player_runtime::PlayerRuntimeOptions;

    #[test]
    fn audio_output_descriptor_change_detects_device_name_switch() {
        let current = AudioOutputDescriptor {
            default_output_device: Some("MacBook Pro Speakers".to_owned()),
            default_output_config: None,
        };
        let next = AudioOutputDescriptor {
            default_output_device: Some("AirPods Pro".to_owned()),
            default_output_config: None,
        };

        assert!(audio_output_descriptor_changed(&current, &next));
    }

    #[test]
    fn audio_output_descriptor_change_ignores_identical_empty_descriptors() {
        let current = AudioOutputDescriptor {
            default_output_device: None,
            default_output_config: None,
        };
        let next = current.clone();

        assert!(!audio_output_descriptor_changed(&current, &next));
        assert!(audio_output_info(&current).is_none());
    }

    #[test]
    fn dash_probe_reports_unsupported_when_ffmpeg_lacks_dash_demuxer() {
        let backend = FfmpegBackend::new().expect("ffmpeg backend should initialize");
        let source = MediaSource::new("https://example.com/manifest.mpd");
        if backend.unsupported_source_reason(&source).is_none() {
            return;
        }

        let error = SoftwarePlayerRuntimeInitializer::probe_source_with_options(
            source,
            PlayerRuntimeOptions::default(),
        )
        .expect_err("dash probe should fail when ffmpeg lacks dash demuxer");

        assert_eq!(error.code(), PlayerRuntimeErrorCode::Unsupported);
        assert!(error.message().contains("'dash' demuxer"));
    }

    #[test]
    fn playback_position_falls_back_to_clock_when_audio_stalls() {
        let selected = select_playback_position(
            Some(Duration::from_millis(0)),
            Some(Duration::from_millis(600)),
        );

        assert_eq!(selected, Some(Duration::from_millis(600)));
    }

    #[test]
    fn playback_position_keeps_audio_clock_when_it_is_close() {
        let selected = select_playback_position(
            Some(Duration::from_millis(480)),
            Some(Duration::from_millis(600)),
        );

        assert_eq!(selected, Some(Duration::from_millis(480)));
    }

    #[test]
    fn remote_sources_defer_audio_decode_to_protect_first_frame() {
        assert!(should_defer_audio_decode_for_source(&MediaSource::new(
            "https://example.com/video.m3u8"
        )));
        assert!(should_defer_audio_decode_for_source(&MediaSource::new(
            "https://example.com/video.mp4"
        )));
        assert!(!should_defer_audio_decode_for_source(&MediaSource::new(
            "/tmp/video.mp4"
        )));
    }

    #[test]
    fn buffering_policy_shapes_desktop_audio_buffer_windows() {
        let resilient_playback_start =
            resolved_audio_playback_start_buffer_duration(&PlayerBufferingPolicy::resilient());
        let resilient_target =
            resolved_audio_stream_target_buffer_duration(&PlayerBufferingPolicy::resilient());
        let resilient_rebuffer =
            resolved_audio_rebuffer_duration(&PlayerBufferingPolicy::resilient());
        let low_latency_playback_start =
            resolved_audio_playback_start_buffer_duration(&PlayerBufferingPolicy::low_latency());
        let low_latency_target =
            resolved_audio_stream_target_buffer_duration(&PlayerBufferingPolicy::low_latency());
        let low_latency_rebuffer =
            resolved_audio_rebuffer_duration(&PlayerBufferingPolicy::low_latency());

        assert!(resilient_playback_start > low_latency_playback_start);
        assert!(resilient_target > low_latency_target);
        assert!(resilient_rebuffer > low_latency_rebuffer);
        assert_eq!(low_latency_playback_start, Duration::from_millis(500));
        assert_eq!(resilient_playback_start, Duration::from_millis(1_500));
        assert_eq!(low_latency_target, Duration::from_millis(1_500));
        assert_eq!(resilient_target, Duration::from_millis(3_500));
        assert!(low_latency_target > low_latency_rebuffer);
        assert!(resilient_target > resilient_rebuffer);
    }

    #[test]
    fn buffering_policy_expands_video_prefetch_capacity_for_resilient_streams() {
        let low_latency_capacity =
            resolved_video_prefetch_capacity(8, &PlayerBufferingPolicy::low_latency());
        let resilient_capacity =
            resolved_video_prefetch_capacity(8, &PlayerBufferingPolicy::resilient());

        assert!(low_latency_capacity > 8);
        assert!(resilient_capacity >= low_latency_capacity);
        assert_eq!(resilient_capacity, MAX_DESKTOP_VIDEO_PREFETCH_CAPACITY);
    }

    #[test]
    fn buffering_policy_derives_video_startup_requirement_from_frame_rate() {
        let media_info = PlayerMediaInfo {
            source_uri: "https://example.com/live/master.m3u8".to_owned(),
            source_kind: MediaSourceKind::Remote,
            source_protocol: MediaSourceProtocol::Hls,
            duration: Some(Duration::from_secs(600)),
            bit_rate: Some(4_000_000),
            audio_streams: 0,
            video_streams: 1,
            best_video: Some(PlayerVideoInfo {
                codec: "H264".to_owned(),
                width: 960,
                height: 540,
                frame_rate: Some(60.0),
            }),
            best_audio: None,
            track_catalog: Default::default(),
            track_selection: Default::default(),
        };

        let startup_frames = resolved_video_playback_start_buffer_frames(
            &media_info,
            48,
            &PlayerBufferingPolicy::resilient(),
        );

        assert_eq!(startup_frames, 48);
    }

    #[test]
    fn buffering_policy_derives_video_rebuffer_requirement_from_frame_rate() {
        let media_info = PlayerMediaInfo {
            source_uri: "https://example.com/live/master.m3u8".to_owned(),
            source_kind: MediaSourceKind::Remote,
            source_protocol: MediaSourceProtocol::Hls,
            duration: Some(Duration::from_secs(600)),
            bit_rate: Some(4_000_000),
            audio_streams: 0,
            video_streams: 1,
            best_video: Some(PlayerVideoInfo {
                codec: "H264".to_owned(),
                width: 960,
                height: 540,
                frame_rate: Some(60.0),
            }),
            best_audio: None,
            track_catalog: Default::default(),
            track_selection: Default::default(),
        };

        let rebuffer_frames =
            resolved_video_rebuffer_frames(&media_info, 48, &PlayerBufferingPolicy::resilient());

        assert_eq!(rebuffer_frames, 48);
    }

    #[test]
    fn cache_policy_caps_remote_video_prefetch_capacity_by_memory_budget() {
        let media_info = PlayerMediaInfo {
            source_uri: "https://example.com/live/master.m3u8".to_owned(),
            source_kind: MediaSourceKind::Remote,
            source_protocol: MediaSourceProtocol::Hls,
            duration: Some(Duration::from_secs(600)),
            bit_rate: Some(4_000_000),
            audio_streams: 0,
            video_streams: 1,
            best_video: Some(PlayerVideoInfo {
                codec: "H264".to_owned(),
                width: 960,
                height: 540,
                frame_rate: Some(60.0),
            }),
            best_audio: None,
            track_catalog: Default::default(),
            track_selection: Default::default(),
        };

        let limited = resolved_video_prefetch_limit(
            &media_info,
            96,
            &PlayerCachePolicy {
                preset: player_runtime::PlayerCachePreset::Resilient,
                max_memory_bytes: Some(8 * 1024 * 1024),
                max_disk_bytes: Some(128 * 1024 * 1024),
            },
        );

        assert!(limited < 96);
        assert_eq!(limited, 43);
    }

    #[test]
    fn desktop_resilience_metrics_track_runtime_events() {
        let mut tracker = PlayerResilienceMetricsTracker::default();
        observe_resilience_metrics_for_event(
            &mut tracker,
            &PlayerRuntimeEvent::PlaybackStateChanged(PresentationState::Playing),
        );
        observe_resilience_metrics_for_event(
            &mut tracker,
            &PlayerRuntimeEvent::BufferingChanged { buffering: true },
        );
        observe_resilience_metrics_for_event(
            &mut tracker,
            &PlayerRuntimeEvent::BufferingChanged { buffering: false },
        );
        observe_resilience_metrics_for_event(
            &mut tracker,
            &PlayerRuntimeEvent::RetryScheduled {
                attempt: 2,
                delay: Duration::from_millis(1_500),
            },
        );

        let snapshot = tracker.snapshot();
        assert_eq!(snapshot.buffering_event_count, 1);
        assert_eq!(snapshot.rebuffer_count, 1);
        assert_eq!(snapshot.retry_count, 2);
        assert_eq!(
            snapshot.last_retry_delay,
            Some(Duration::from_millis(1_500))
        );
    }

    #[test]
    fn retry_policy_delay_respects_backoff_and_max_attempts() {
        let linear = PlayerRetryPolicy::default();
        assert_eq!(
            retry_delay_for_attempt(&linear, 1),
            Some(Duration::from_secs(1))
        );
        assert_eq!(
            retry_delay_for_attempt(&linear, 3),
            Some(Duration::from_secs(3))
        );
        assert_eq!(retry_delay_for_attempt(&linear, 4), None);

        let exponential = PlayerRetryPolicy {
            max_attempts: Some(6),
            base_delay: Duration::from_millis(500),
            max_delay: Duration::from_secs(2),
            backoff: PlayerRetryBackoff::Exponential,
        };
        assert_eq!(
            retry_delay_for_attempt(&exponential, 1),
            Some(Duration::from_millis(500))
        );
        assert_eq!(
            retry_delay_for_attempt(&exponential, 3),
            Some(Duration::from_secs(2))
        );
        assert_eq!(
            retry_delay_for_attempt(&exponential, 6),
            Some(Duration::from_secs(2))
        );
    }

    #[test]
    fn deferred_audio_probe_merges_missing_audio_metadata() {
        let mut media_info =
            unresolved_player_media_info(&MediaSource::new("https://example.com/live/master.m3u8"));
        media_info.video_streams = 1;
        media_info.best_video = Some(PlayerVideoInfo {
            codec: "H264".to_owned(),
            width: 960,
            height: 540,
            frame_rate: Some(60.0),
        });

        let changed = merge_audio_probe_into_media_info(
            &mut media_info,
            &MediaProbe {
                source: MediaSource::new("https://example.com/live/master.m3u8"),
                duration: Some(Duration::from_secs(600)),
                bit_rate: Some(128_000),
                audio_streams: 1,
                video_streams: 0,
                best_video: None,
                best_audio: Some(AudioStreamProbe {
                    index: 0,
                    codec: "AAC".to_owned(),
                    sample_rate: 48_000,
                    channels: 2,
                }),
            },
        );

        assert!(changed);
        assert_eq!(media_info.audio_streams, 1);
        let best_audio = media_info
            .best_audio
            .expect("best audio should be restored");
        assert_eq!(best_audio.codec, "AAC");
        assert_eq!(best_audio.sample_rate, 48_000);
        assert_eq!(best_audio.channels, 2);
    }

    #[test]
    fn only_remote_hls_sources_defer_metadata_probe() {
        assert!(should_defer_media_probe_for_source(&MediaSource::new(
            "https://example.com/video.m3u8"
        )));
        assert!(!should_defer_media_probe_for_source(&MediaSource::new(
            "https://example.com/video.mp4"
        )));
        assert!(!should_defer_media_probe_for_source(&MediaSource::new(
            "/tmp/video.m3u8"
        )));
    }

    #[test]
    fn only_remote_hls_sources_stream_audio_directly() {
        assert!(should_stream_audio_source_directly(&MediaSource::new(
            "https://example.com/video.m3u8"
        )));
        assert!(!should_stream_audio_source_directly(&MediaSource::new(
            "https://example.com/video.mp4"
        )));
        assert!(!should_stream_audio_source_directly(&MediaSource::new(
            "/tmp/video.m3u8"
        )));
    }

    #[test]
    fn initial_restart_positions_preserve_non_zero_video_start() {
        let audio_track = DecodedAudioTrack {
            presentation_time: Duration::from_secs(10),
            sample_rate: 48_000,
            channels: 2,
            playback_rate: 1.0,
            samples: Arc::from(Vec::<f32>::new()),
        };

        assert_eq!(
            initial_restart_positions(Some(&audio_track), Duration::from_millis(10_016)),
            (Duration::from_secs(10), Duration::from_millis(10_016))
        );
        assert_eq!(
            initial_restart_positions(None, Duration::from_millis(10_016)),
            (Duration::from_millis(10_016), Duration::from_millis(10_016))
        );
    }
}
