use std::collections::VecDeque;
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::thread;
use std::time::{Duration, Instant};

use player_audio_cpal::{
    AudioOutputConfig, AudioOutputDescriptor, AudioSink, AudioSinkController, detect_default_output,
};
use player_backend_ffmpeg::{
    AudioStreamProbe, BufferedFramePoll, BufferedVideoSource, BufferedVideoSourceBootstrap,
    DecodedAudioTrack, FfmpegBackend, VideoDecodeInfo as BackendVideoDecodeInfo,
    VideoDecoderMode as BackendVideoDecoderMode, VideoStreamProbe,
};
use player_core::{MediaSource, PlaybackClock, PlaybackSessionModel};

use player_runtime::{
    DEFAULT_PLAYBACK_RATE, DecodedAudioSummary, DecodedVideoFrame, FirstFrameReady,
    MAX_PLAYBACK_RATE, MIN_PLAYBACK_RATE, NATURAL_PLAYBACK_RATE_MAX, PlaybackProgress,
    PlayerAudioInfo, PlayerAudioOutputInfo, PlayerMediaInfo, PlayerRuntimeAdapter,
    PlayerRuntimeAdapterBackendFamily, PlayerRuntimeAdapterBootstrap,
    PlayerRuntimeAdapterCapabilities, PlayerRuntimeAdapterFactory, PlayerRuntimeAdapterInitializer,
    PlayerRuntimeCommand, PlayerRuntimeCommandResult, PlayerRuntimeError, PlayerRuntimeErrorCode,
    PlayerRuntimeEvent, PlayerRuntimeOptions, PlayerRuntimeResult, PlayerRuntimeStartup,
    PlayerVideoInfo, PresentationState, register_default_runtime_adapter_factory,
};

pub const SOFTWARE_PLAYER_RUNTIME_ADAPTER_ID: &str = "software_desktop";
const AUDIO_STREAM_CHUNK_FRAMES: usize = 2_048;
const AUDIO_STREAM_TARGET_BUFFER_DURATION: Duration = Duration::from_secs(2);
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

#[derive(Debug, Default, Clone, Copy)]
pub struct SoftwarePlayerRuntimeAdapterFactory;

#[derive(Debug)]
pub struct SoftwarePlayerRuntimeInitializer {
    backend: FfmpegBackend,
    source: MediaSource,
    probe: player_backend_ffmpeg::MediaProbe,
    audio_output: AudioOutputDescriptor,
    options: PlayerRuntimeOptions,
}

#[derive(Debug)]
struct SoftwareRuntimeConfig {
    backend: FfmpegBackend,
    source: MediaSource,
    probe: player_backend_ffmpeg::MediaProbe,
    audio_output_descriptor: AudioOutputDescriptor,
    audio_output_config: Option<AudioOutputConfig>,
    source_audio_track: Option<DecodedAudioTrack>,
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
    audio_output_descriptor: AudioOutputDescriptor,
    audio_output_config: Option<AudioOutputConfig>,
    source_audio_track: Option<DecodedAudioTrack>,
    video_source: BufferedVideoSource,
    video_end_of_stream: bool,
    next_frame: Option<DecodedVideoFrame>,
    audio_sink: Option<AudioSink>,
    audio_sink_controller: Option<AudioSinkController>,
    playback_clock: Option<PlaybackClock>,
    video_present_early_tolerance: Duration,
    video_idle_poll_interval: Duration,
    pending_audio_stream_worker: Option<PendingAudioStreamWorker>,
    is_buffering: bool,
    buffering_candidate_since: Option<Instant>,
    last_audio_output_poll: Instant,
    events: VecDeque<PlayerRuntimeEvent>,
}

struct PendingAudioStreamWorker {
    generation: u64,
    receiver: Receiver<Result<(), String>>,
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
        player_media_info(&self.probe)
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
        } = *self;

        let decoded_audio = match audio_output.default_output_config.clone() {
            Some(output_config) if probe.best_audio.is_some() => Some(
                backend
                    .decode_audio_track(
                        source.clone(),
                        output_config.sample_rate,
                        output_config.channels,
                    )
                    .map_err(|error| {
                        runtime_error(
                            PlayerRuntimeErrorCode::DecodeFailure,
                            "failed to decode audio track during initialization",
                            error,
                        )
                    })?,
            ),
            _ => None,
        };
        let startup = PlayerRuntimeStartup {
            ffmpeg_initialized: backend.is_initialized(),
            audio_output: audio_output_info(&audio_output),
            decoded_audio: decoded_audio.as_ref().map(decoded_audio_summary),
            video_decode: None,
        };
        let config = SoftwareRuntimeConfig {
            backend,
            source,
            probe,
            audio_output_descriptor: audio_output.clone(),
            audio_output_config: audio_output.default_output_config,
            source_audio_track: decoded_audio.clone(),
            video_prefetch_capacity: options.video_prefetch_capacity,
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
        let probe = backend.probe(source.clone()).map_err(|error| {
            runtime_error(
                PlayerRuntimeErrorCode::InvalidSource,
                "failed to probe media source",
                error,
            )
        })?;

        Ok(Self {
            backend,
            source,
            probe,
            audio_output,
            options,
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

    fn drain_events(&mut self) -> Vec<PlayerRuntimeEvent> {
        self.poll_audio_output_device();
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
        if !self.session.is_started() || self.session.is_paused() || self.session.is_finished() {
            return None;
        }

        if let Some(next_frame) = self.next_frame.as_ref() {
            let playback_position = self.playback_position()?;
            let scheduled_time = next_frame
                .presentation_time
                .saturating_sub(self.video_present_early_tolerance);
            if playback_position >= scheduled_time {
                return Some(Instant::now());
            }

            return Some(Instant::now() + scheduled_time.saturating_sub(playback_position));
        }

        if !self.video_end_of_stream {
            return Some(Instant::now() + self.video_idle_poll_interval);
        }

        if self
            .audio_sink
            .as_ref()
            .map(|sink| !sink.is_finished())
            .unwrap_or(false)
        {
            return Some(Instant::now() + self.video_idle_poll_interval);
        }

        None
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
        } = BufferedVideoSource::new(config.source.clone(), config.video_prefetch_capacity)
            .map_err(|error| {
                runtime_error(
                    PlayerRuntimeErrorCode::BackendFailure,
                    "failed to create buffered video source",
                    error,
                )
            })?;
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
            config.probe.duration,
            config
                .probe
                .best_video
                .as_ref()
                .and_then(|video| video.frame_rate),
        );
        let media_info = player_media_info(&config.probe);

        let mut runtime = Self {
            backend: config.backend,
            source: config.source,
            media_info,
            session,
            playback_rate: DEFAULT_PLAYBACK_RATE,
            audio_output_descriptor: config.audio_output_descriptor,
            audio_output_config: config.audio_output_config,
            source_audio_track: config.source_audio_track,
            video_source,
            video_end_of_stream: false,
            next_frame: None,
            audio_sink: None,
            audio_sink_controller: None,
            playback_clock: None,
            video_present_early_tolerance: config.video_present_early_tolerance,
            video_idle_poll_interval: config.video_idle_poll_interval,
            pending_audio_stream_worker: None,
            is_buffering: false,
            buffering_candidate_since: None,
            last_audio_output_poll: Instant::now(),
            events: VecDeque::new(),
        };

        let initial_position = runtime
            .source_audio_track
            .as_ref()
            .map(|track| track.presentation_time.min(initial_frame.presentation_time))
            .unwrap_or(initial_frame.presentation_time);
        runtime.set_playback_clock(initial_frame.presentation_time);
        runtime.ensure_audio_output(initial_position, runtime.playback_rate)?;
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
        self.poll_audio_output_device();
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
        self.poll_audio_output_device();
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
            self.pending_audio_stream_worker = None;
            return Ok(());
        };
        let Some(source_audio_track) = self.source_audio_track.as_ref() else {
            self.audio_sink = None;
            self.audio_sink_controller = None;
            self.pending_audio_stream_worker = None;
            return Ok(());
        };

        if self.audio_sink.is_none() {
            let sample_offset = source_audio_track.sample_offset_for_position(position);
            let media_start = source_audio_track.media_time_for_sample_offset(sample_offset);
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

        self.start_audio_stream(position, playback_rate)?;
        self.sync_audio_output_state();
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

        let Some(first_frame) = self.video_source.seek_to(Duration::ZERO).map_err(|error| {
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
        self.ensure_audio_output(Duration::ZERO, self.playback_rate)?;
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
            Ok(Ok(())) => {}
            Ok(Err(error)) => {
                if is_active_generation {
                    if let Some(controller) = self.audio_sink_controller.as_ref() {
                        controller.finish_generation(worker.generation);
                    }
                    self.emit_event(PlayerRuntimeEvent::Error(PlayerRuntimeError::new(
                        PlayerRuntimeErrorCode::DecodeFailure,
                        format!("failed to stream retimed audio for playback: {error}"),
                    )));
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
                    self.emit_event(PlayerRuntimeEvent::Error(PlayerRuntimeError::new(
                        PlayerRuntimeErrorCode::BackendFailure,
                        "audio stream worker disconnected before completing playback",
                    )));
                    self.refresh_playback_finished();
                }
            }
        }

        self.update_buffering_state();
    }

    fn start_audio_stream(
        &mut self,
        position: Duration,
        playback_rate: f32,
    ) -> PlayerRuntimeResult<()> {
        let Some(source_track) = self.source_audio_track.clone() else {
            self.pending_audio_stream_worker = None;
            return Ok(());
        };
        let Some(controller) = self.audio_sink_controller.clone() else {
            self.pending_audio_stream_worker = None;
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
            AUDIO_STREAM_TARGET_BUFFER_DURATION,
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
                let result: Result<(), String> =
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
            receiver,
        });
        self.update_buffering_state();
        Ok(())
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

        if self.source_audio_track.is_none() {
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
        self.pending_audio_stream_worker = None;
        self.set_playback_clock(current_position);

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
                self.pending_audio_stream_worker = None;
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

        let waiting_for_video = !self.video_end_of_stream && self.next_frame.is_none();
        let waiting_for_audio = self
            .pending_audio_stream_worker
            .as_ref()
            .and_then(|worker| {
                self.audio_sink_controller
                    .as_ref()
                    .and_then(|controller| controller.buffered_samples(worker.generation))
            })
            .map(|buffered_samples| buffered_samples == 0)
            .unwrap_or(false);

        waiting_for_video || waiting_for_audio
    }

    fn update_buffering_state(&mut self) {
        let raw_is_buffering = self.raw_is_buffering();
        if !raw_is_buffering {
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
}

fn buffered_sample_target(sample_rate: u32, channels: u16, duration: Duration) -> usize {
    let frames = (duration.as_secs_f64() * f64::from(sample_rate.max(1))).ceil() as usize;
    frames.saturating_mul(usize::from(channels.max(1)))
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
}
