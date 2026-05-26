use super::*;

#[derive(Debug)]
pub(crate) struct MacosNativeFrameVideoSourceFactory {
    pub(crate) plugin_path: PathBuf,
    pub(crate) video_surface: PlayerVideoSurfaceTarget,
    pub(crate) frame_processor_paths: Vec<PathBuf>,
    pub(crate) frame_processor_mode: FrameProcessorMode,
    pub(crate) frame_processor_policy: FrameProcessorPolicy,
}

pub(crate) struct MacosSourceNormalizerPacketVideoSourceFactory {
    pub(crate) decoder_plugin_path: PathBuf,
    pub(crate) decoder_plugin_name: Option<String>,
    pub(crate) video_surface: PlayerVideoSurfaceTarget,
    pub(crate) frame_processor_paths: Vec<PathBuf>,
    pub(crate) frame_processor_mode: FrameProcessorMode,
    pub(crate) frame_processor_policy: FrameProcessorPolicy,
    pub(crate) packet_session: Arc<Mutex<Option<Box<dyn SourceNormalizerPacketSession>>>>,
}

impl std::fmt::Debug for MacosSourceNormalizerPacketVideoSourceFactory {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MacosSourceNormalizerPacketVideoSourceFactory")
            .field("decoder_plugin_path", &self.decoder_plugin_path)
            .field("decoder_plugin_name", &self.decoder_plugin_name)
            .field("video_surface", &self.video_surface)
            .field("frame_processor_paths", &self.frame_processor_paths)
            .field("frame_processor_mode", &self.frame_processor_mode)
            .field("frame_processor_policy", &self.frame_processor_policy)
            .finish_non_exhaustive()
    }
}

pub(crate) struct MacosNativeFrameVideoSource {
    pub(crate) stream_info: VideoPacketStreamInfo,
    pub(crate) session: Arc<Mutex<Box<dyn NativeDecoderSession>>>,
    pub(crate) shared: Arc<Mutex<MacosNativeFrameDecoderState>>,
    pub(crate) outstanding_frames: Arc<AtomicUsize>,
    pub(crate) command_tx: Sender<MacosNativeFrameWorkerCommand>,
    pub(crate) frame_rx: Receiver<MacosNativeFrameWorkerEvent>,
    pub(crate) generation: u64,
    pub(crate) current_generation: Arc<AtomicU64>,
    pub(crate) buffered_frame_count: Arc<AtomicUsize>,
    pub(crate) prefetch_limit: Arc<AtomicUsize>,
    pub(crate) prefetch_wakeup: Arc<MacosNativeFramePrefetchWakeup>,
    pub(crate) end_of_input_sent: bool,
    pub(crate) end_of_stream_received: bool,
    pub(crate) worker: Option<JoinHandle<()>>,
}

// Lock ordering for native-frame playback: acquire `session` before `shared` whenever both are
// needed. Holding `shared` while taking `session` can deadlock with decoder receive/release paths.
pub(crate) struct MacosNativeFrameDecoderState {
    pub(crate) frame_processor_chain: Option<MacosFrameProcessorChain>,
    pub(crate) presenter: Option<MacosMetalLayerPresenter>,
    pub(crate) presentation_epoch: u64,
}

#[derive(Debug, Default)]
pub(crate) struct MacosNativeFramePrefetchWakeup {
    pub(crate) state: Mutex<MacosNativeFramePrefetchWakeupState>,
    pub(crate) changed: Condvar,
}

#[derive(Debug, Default)]
pub(crate) struct MacosNativeFramePrefetchWakeupState {
    pub(crate) sequence: u64,
}

impl MacosNativeFramePrefetchWakeup {
    pub(crate) fn notify(&self) {
        match self.state.lock() {
            Ok(mut state) => {
                state.sequence = state.sequence.wrapping_add(1);
                self.changed.notify_all();
            }
            Err(_) => {
                tracing::error!("macOS native frame prefetch wakeup state mutex was poisoned");
            }
        }
    }

    pub(crate) fn wait_for_change(&self, observed_sequence: &mut u64) {
        let state = match self.state.lock() {
            Ok(state) => state,
            Err(_) => {
                tracing::error!("macOS native frame prefetch wakeup state mutex was poisoned");
                return;
            }
        };
        let sequence = state.sequence;
        if sequence != *observed_sequence {
            *observed_sequence = sequence;
            return;
        }
        if let Ok((state_after_wait, _)) = self
            .changed
            .wait_timeout(state, MACOS_NATIVE_FRAME_PREFETCH_COMMAND_POLL_INTERVAL)
        {
            *observed_sequence = state_after_wait.sequence;
        }
    }
}

#[derive(Debug)]
pub(crate) struct MacosFrameProcessorChain {
    pub(crate) processors: Vec<MacosFrameProcessorNode>,
    pub(crate) mode: FrameProcessorMode,
    pub(crate) policy: FrameProcessorPolicy,
    pub(crate) metrics: PlayerFrameProcessingMetrics,
    pub(crate) pending_events: VecDeque<PlayerRuntimeEvent>,
    pub(crate) debug: FrameProcessorDebugState,
}

pub(crate) struct MacosFrameProcessorNode {
    pub(crate) plugin_name: String,
    pub(crate) processor_index: usize,
    pub(crate) session: Box<dyn FrameProcessorSession>,
}

impl std::fmt::Debug for MacosFrameProcessorNode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MacosFrameProcessorNode")
            .field("plugin_name", &self.plugin_name)
            .field("processor_index", &self.processor_index)
            .finish()
    }
}

#[derive(Debug)]
pub(crate) struct MacosFrameProcessorFrame {
    pub(crate) decoder_frame: DecoderNativeFrame,
    pub(crate) presentation_frame: DecoderNativeFrame,
    pub(crate) processor_outputs: Vec<ProcessorOwnedNativeFrame>,
}

#[derive(Debug)]
pub(crate) struct ProcessorOwnedNativeFrame {
    pub(crate) processor_index: usize,
    pub(crate) frame: NativeFrame,
}

#[derive(Debug)]
pub(crate) struct FrameProcessorDebugState {
    pub(crate) enabled: bool,
    pub(crate) trace_frames: bool,
    pub(crate) window_frames: u64,
    pub(crate) frame_count: u64,
    pub(crate) window_start: Instant,
    pub(crate) last_pts_us: Option<i64>,
    pub(crate) last_frame_started_at: Option<Instant>,
    pub(crate) max_wall_us: u128,
    pub(crate) total_wall_us: u128,
    pub(crate) max_pts_delta_us: Option<i64>,
    pub(crate) total_pts_delta_us: i128,
    pub(crate) pts_delta_count: u64,
    pub(crate) bypassed_frames: u64,
    pub(crate) dropped_outputs: u64,
    pub(crate) deadline_misses: u64,
    pub(crate) backpressure_count: u64,
    pub(crate) pending_count: u64,
    pub(crate) presented_processed: u64,
    pub(crate) presented_original: u64,
    pub(crate) max_queue_depth: Option<u32>,
    pub(crate) max_in_flight_frames: Option<u32>,
}

impl FrameProcessorDebugState {
    // Debug environment variables are snapshotted when the processor chain is opened. Changing
    // them during playback requires recreating the player/runtime.
    pub(crate) fn from_env() -> Self {
        Self {
            enabled: env_flag(FRAME_PROCESSOR_DEBUG_ENV),
            trace_frames: env_flag(FRAME_PROCESSOR_DEBUG_TRACE_ENV),
            window_frames: env_u64(FRAME_PROCESSOR_DEBUG_WINDOW_ENV)
                .unwrap_or(DEFAULT_FRAME_PROCESSOR_DEBUG_WINDOW)
                .max(1),
            frame_count: 0,
            window_start: Instant::now(),
            last_pts_us: None,
            last_frame_started_at: None,
            max_wall_us: 0,
            total_wall_us: 0,
            max_pts_delta_us: None,
            total_pts_delta_us: 0,
            pts_delta_count: 0,
            bypassed_frames: 0,
            dropped_outputs: 0,
            deadline_misses: 0,
            backpressure_count: 0,
            pending_count: 0,
            presented_processed: 0,
            presented_original: 0,
            max_queue_depth: None,
            max_in_flight_frames: None,
        }
    }

    pub(crate) fn begin_frame(&mut self, pts_us: Option<i64>) -> FrameProcessorFrameDebugSample {
        if !self.enabled {
            return FrameProcessorFrameDebugSample::default();
        }
        self.frame_count = self.frame_count.saturating_add(1);
        let started_at = Instant::now();
        self.last_frame_started_at = Some(started_at);
        let pts_delta_us = pts_us.and_then(|pts| self.last_pts_us.map(|previous| pts - previous));
        if let Some(delta) = pts_delta_us {
            self.max_pts_delta_us = Some(
                self.max_pts_delta_us
                    .map(|current| current.max(delta.abs()))
                    .unwrap_or(delta.abs()),
            );
            self.total_pts_delta_us = self.total_pts_delta_us.saturating_add(delta as i128);
            self.pts_delta_count = self.pts_delta_count.saturating_add(1);
        }
        if pts_us.is_some() {
            self.last_pts_us = pts_us;
        }
        FrameProcessorFrameDebugSample {
            sequence: self.frame_count,
            started_at: Some(started_at),
            input_pts_us: pts_us,
            pts_delta_us,
            ..FrameProcessorFrameDebugSample::default()
        }
    }

    pub(crate) fn observe_submit(
        &mut self,
        queue_depth: Option<u32>,
        in_flight_frames: Option<u32>,
    ) {
        if !self.enabled {
            return;
        }
        self.max_queue_depth = max_option_u32(self.max_queue_depth, queue_depth);
        self.max_in_flight_frames = max_option_u32(self.max_in_flight_frames, in_flight_frames);
    }

    pub(crate) fn observe_bypass(&mut self) {
        if self.enabled {
            self.bypassed_frames = self.bypassed_frames.saturating_add(1);
        }
    }

    pub(crate) fn observe_backpressure(&mut self) {
        if self.enabled {
            self.backpressure_count = self.backpressure_count.saturating_add(1);
        }
    }

    pub(crate) fn observe_pending(&mut self) {
        if self.enabled {
            self.pending_count = self.pending_count.saturating_add(1);
        }
    }

    pub(crate) fn observe_deadline_miss(&mut self) {
        if self.enabled {
            self.deadline_misses = self.deadline_misses.saturating_add(1);
        }
    }

    pub(crate) fn observe_dropped_output(&mut self) {
        if self.enabled {
            self.dropped_outputs = self.dropped_outputs.saturating_add(1);
        }
    }

    pub(crate) fn finish_frame(&mut self, sample: FrameProcessorFrameDebugSample) {
        if !self.enabled {
            return;
        }
        let wall_us = sample
            .started_at
            .map(|started_at| started_at.elapsed().as_micros())
            .unwrap_or(0);
        self.max_wall_us = self.max_wall_us.max(wall_us);
        self.total_wall_us = self.total_wall_us.saturating_add(wall_us);
        if sample.presented_processed {
            self.presented_processed = self.presented_processed.saturating_add(1);
        } else {
            self.presented_original = self.presented_original.saturating_add(1);
        }
        if self.trace_frames {
            info!(
                sequence = sample.sequence,
                input_pts_us = sample.input_pts_us,
                pts_delta_us = sample.pts_delta_us,
                wall_us,
                node_count = sample.node_count,
                submitted_nodes = sample.submitted_nodes,
                processed_nodes = sample.processed_nodes,
                bypassed = sample.bypassed,
                pending = sample.pending,
                dropped_output = sample.dropped_output,
                deadline_missed = sample.deadline_missed,
                presented_processed = sample.presented_processed,
                output_pts_us = sample.output_pts_us,
                "macOS frame processor debug frame"
            );
        }
        if self.frame_count.is_multiple_of(self.window_frames) {
            self.log_summary();
            self.reset_window();
        }
    }

    pub(crate) fn log_summary(&self) {
        let avg_wall_us = if self.window_frames == 0 {
            0
        } else {
            self.total_wall_us / u128::from(self.window_frames)
        };
        let avg_pts_delta_us = if self.pts_delta_count == 0 {
            None
        } else {
            Some(self.total_pts_delta_us / i128::from(self.pts_delta_count))
        };
        info!(
            frames = self.window_frames,
            elapsed_ms = self.window_start.elapsed().as_millis(),
            avg_wall_us,
            max_wall_us = self.max_wall_us,
            avg_pts_delta_us,
            max_pts_delta_us = self.max_pts_delta_us,
            bypassed_frames = self.bypassed_frames,
            dropped_outputs = self.dropped_outputs,
            deadline_misses = self.deadline_misses,
            backpressure_count = self.backpressure_count,
            pending_count = self.pending_count,
            presented_processed = self.presented_processed,
            presented_original = self.presented_original,
            max_queue_depth = self.max_queue_depth,
            max_in_flight_frames = self.max_in_flight_frames,
            "macOS frame processor debug summary"
        );
    }

    pub(crate) fn reset_window(&mut self) {
        self.window_start = Instant::now();
        self.max_wall_us = 0;
        self.total_wall_us = 0;
        self.max_pts_delta_us = None;
        self.total_pts_delta_us = 0;
        self.pts_delta_count = 0;
        self.bypassed_frames = 0;
        self.dropped_outputs = 0;
        self.deadline_misses = 0;
        self.backpressure_count = 0;
        self.pending_count = 0;
        self.presented_processed = 0;
        self.presented_original = 0;
        self.max_queue_depth = None;
        self.max_in_flight_frames = None;
    }
}

#[derive(Debug, Default)]
pub(crate) struct FrameProcessorFrameDebugSample {
    pub(crate) sequence: u64,
    pub(crate) started_at: Option<Instant>,
    pub(crate) input_pts_us: Option<i64>,
    pub(crate) output_pts_us: Option<i64>,
    pub(crate) pts_delta_us: Option<i64>,
    pub(crate) node_count: usize,
    pub(crate) submitted_nodes: usize,
    pub(crate) processed_nodes: usize,
    pub(crate) bypassed: bool,
    pub(crate) pending: bool,
    pub(crate) dropped_output: bool,
    pub(crate) deadline_missed: bool,
    pub(crate) presented_processed: bool,
}

#[derive(Debug)]
pub(crate) struct MacosFrameProcessorProcessState {
    pub(crate) current_frame: NativeFrame,
    pub(crate) processor_outputs: Vec<ProcessorOwnedNativeFrame>,
    pub(crate) using_processor_output: bool,
    pub(crate) debug_sample: FrameProcessorFrameDebugSample,
}

#[derive(Debug)]
pub(crate) enum MacosNativeFramePoll {
    Frame(MacosFrameProcessorFrame),
    Decoder(DecoderReceiveNativeFrameOutput),
}

#[derive(Debug)]
pub(crate) enum MacosNativeFrameWorkerCommand {
    Seek { generation: u64, position: Duration },
    Shutdown,
}

#[derive(Debug)]
pub(crate) enum MacosNativeFrameWorkerEvent {
    Frame {
        generation: u64,
        frame: MacosFrameProcessorFrame,
    },
    EndOfStream {
        generation: u64,
    },
    Error {
        generation: u64,
        message: String,
    },
}

pub(crate) struct MacosDeferredNativeFramePresentation {
    pub(crate) session: Arc<Mutex<Box<dyn NativeDecoderSession>>>,
    pub(crate) shared: Arc<Mutex<MacosNativeFrameDecoderState>>,
    pub(crate) outstanding_frames: Arc<AtomicUsize>,
    pub(crate) frame: Option<MacosFrameProcessorFrame>,
    pub(crate) presentation_epoch: u64,
}

impl std::fmt::Debug for MacosDeferredNativeFramePresentation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MacosDeferredNativeFramePresentation")
            .field("has_frame", &self.frame.is_some())
            .field("presentation_epoch", &self.presentation_epoch)
            .finish()
    }
}

impl std::fmt::Debug for MacosNativeFrameVideoSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MacosNativeFrameVideoSource")
            .field("codec", &self.stream_info.codec)
            .field("end_of_input_sent", &self.end_of_input_sent)
            .finish()
    }
}

impl std::fmt::Debug for MacosNativeFrameDecoderState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MacosNativeFrameDecoderState").finish()
    }
}

impl Drop for MacosNativeFrameVideoSource {
    fn drop(&mut self) {
        let _ = self
            .command_tx
            .send(MacosNativeFrameWorkerCommand::Shutdown);
        self.prefetch_wakeup.notify();
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
        self.release_queued_prefetch_events();
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MacosNativeFramePacketSendStatus {
    Sent,
    NeedMoreData,
    EndOfStream,
}

pub(crate) trait MacosNativeFramePacketSource: Send {
    fn send_next_packet(
        &mut self,
        decoder_session: &Arc<Mutex<Box<dyn NativeDecoderSession>>>,
    ) -> anyhow::Result<MacosNativeFramePacketSendStatus>;
    fn seek_to(&mut self, position: Duration) -> anyhow::Result<()>;
}

impl MacosNativeFramePacketSource for VideoPacketSource {
    fn send_next_packet(
        &mut self,
        decoder_session: &Arc<Mutex<Box<dyn NativeDecoderSession>>>,
    ) -> anyhow::Result<MacosNativeFramePacketSendStatus> {
        match VideoPacketSource::next_packet(self)? {
            Some(packet) => {
                send_macos_native_frame_packet(decoder_session, packet)?;
                Ok(MacosNativeFramePacketSendStatus::Sent)
            }
            None => Ok(MacosNativeFramePacketSendStatus::EndOfStream),
        }
    }

    fn seek_to(&mut self, position: Duration) -> anyhow::Result<()> {
        VideoPacketSource::seek_to(self, position)
    }
}

pub(crate) struct SourceNormalizerPacketSource {
    pub(crate) session: Arc<Mutex<Option<Box<dyn SourceNormalizerPacketSession>>>>,
    pub(crate) pending: Option<SourceNormalizerPendingPacket>,
}

#[derive(Debug)]
pub(crate) struct SourceNormalizerPendingPacket {
    pub(crate) packet: DecoderPacket,
    pub(crate) data: Vec<u8>,
}

impl SourceNormalizerPacketSource {
    pub(crate) fn new(session: Arc<Mutex<Option<Box<dyn SourceNormalizerPacketSession>>>>) -> Self {
        Self {
            session,
            pending: None,
        }
    }

    pub(crate) fn send_pending_packet(
        &mut self,
        decoder_session: &Arc<Mutex<Box<dyn NativeDecoderSession>>>,
        pending: SourceNormalizerPendingPacket,
    ) -> anyhow::Result<MacosNativeFramePacketSendStatus> {
        let send_result = send_macos_native_frame_packet_bytes(
            decoder_session,
            pending.packet.clone(),
            &pending.data,
        );
        match send_result {
            Ok(result) if result.accepted => Ok(MacosNativeFramePacketSendStatus::Sent),
            Ok(_) => {
                self.pending = Some(pending);
                Ok(MacosNativeFramePacketSendStatus::NeedMoreData)
            }
            Err(error) => Err(error),
        }
    }
}

impl Drop for SourceNormalizerPacketSource {
    fn drop(&mut self) {
        self.pending = None;
    }
}

impl MacosNativeFramePacketSource for SourceNormalizerPacketSource {
    fn send_next_packet(
        &mut self,
        decoder_session: &Arc<Mutex<Box<dyn NativeDecoderSession>>>,
    ) -> anyhow::Result<MacosNativeFramePacketSendStatus> {
        if let Some(pending) = self.pending.take() {
            return self.send_pending_packet(decoder_session, pending);
        }

        let session_arc = self.session.clone();
        let mut guard = session_arc
            .lock()
            .map_err(|_| anyhow::anyhow!("source normalizer packet session is poisoned"))?;
        let session = guard
            .as_mut()
            .ok_or_else(|| anyhow::anyhow!("source normalizer packet session is not configured"))?;
        let lease = session
            .read_packet()
            .map_err(|error| anyhow::anyhow!("source normalizer read_packet failed: {error}"))?;
        if lease.metadata.status == SourceNormalizerReadPacketStatus::EndOfStream {
            return Ok(MacosNativeFramePacketSendStatus::EndOfStream);
        }
        if lease.metadata.status == SourceNormalizerReadPacketStatus::NeedMoreData {
            return Ok(MacosNativeFramePacketSendStatus::NeedMoreData);
        }
        let metadata = source_normalizer_packet_metadata(&lease.metadata);
        let data = lease.data.to_vec();
        let handle = lease.handle;
        drop(lease);
        session
            .release_packet(handle)
            .map_err(|error| anyhow::anyhow!(error.to_string()))?;
        let metadata = metadata?;
        let pending = SourceNormalizerPendingPacket {
            packet: metadata,
            data,
        };
        self.send_pending_packet(decoder_session, pending)
    }

    fn seek_to(&mut self, position: Duration) -> anyhow::Result<()> {
        self.pending = None;
        let mut guard = self
            .session
            .lock()
            .map_err(|_| anyhow::anyhow!("source normalizer packet session is poisoned"))?;
        let session = guard
            .as_mut()
            .ok_or_else(|| anyhow::anyhow!("source normalizer packet session is not configured"))?;
        session
            .seek(&SourceNormalizerPacketSeek {
                position_millis: position.as_millis().min(u64::MAX as u128) as u64,
                exact: false,
            })
            .map(|_| ())
            .map_err(|error| anyhow::anyhow!(error.to_string()))
    }
}

impl DesktopVideoSourceFactory for MacosSourceNormalizerPacketVideoSourceFactory {
    fn open_video_source(
        &self,
        source: MediaSource,
        _buffer_capacity: usize,
        _interrupt_flag: Option<Arc<AtomicBool>>,
    ) -> anyhow::Result<DesktopVideoSourceBootstrap> {
        let stream_info = {
            let guard = self
                .packet_session
                .lock()
                .map_err(|_| anyhow::anyhow!("source normalizer packet session is poisoned"))?;
            let session = guard.as_ref().ok_or_else(|| {
                anyhow::anyhow!("source normalizer packet session is not configured")
            })?;
            macos_packet_stream_info_from_source_normalizer(&session.stream_info())?
        };
        let plugin = LoadedDynamicPlugin::load(&self.decoder_plugin_path).with_context(|| {
            format!(
                "failed to load native-frame decoder plugin {}",
                self.decoder_plugin_path.display()
            )
        })?;
        let factory = plugin.native_decoder_plugin_factory().ok_or_else(|| {
            anyhow::anyhow!("decoder plugin does not export a v2 native-frame API")
        })?;
        if !factory
            .capabilities()
            .supports_codec(&stream_info.codec, DecoderMediaKind::Video)
        {
            anyhow::bail!(
                "native-frame decoder plugin `{}` does not support {} video",
                factory.name(),
                stream_info.codec
            );
        }

        let session = factory
            .open_native_session(&DecoderSessionConfig {
                codec: stream_info.codec.clone(),
                media_kind: DecoderMediaKind::Video,
                extradata: stream_info.extradata.clone(),
                bitstream_format: Some(macos_decoder_bitstream_format(&stream_info.codec)),
                width: stream_info.width,
                height: stream_info.height,
                coded_width: stream_info.width,
                coded_height: stream_info.height,
                prefer_hardware: true,
                require_cpu_output: false,
                ..DecoderSessionConfig::default()
            })
            .map_err(|error| anyhow::anyhow!(error.to_string()))?;
        let session_info = session.session_info();
        let presenter = MacosMetalLayerPresenter::new(self.video_surface)
            .map_err(|error| anyhow::anyhow!(error.message().to_owned()))?;
        let frame_processor_chain = open_macos_frame_processor_chain(
            &stream_info,
            &self.frame_processor_paths,
            self.frame_processor_mode,
            self.frame_processor_policy.clone(),
        )?;
        let decode_info = BackendVideoDecodeInfo {
            selected_mode: BackendVideoDecoderMode::Hardware,
            hardware_available: true,
            hardware_backend: session_info
                .selected_hardware_backend
                .or_else(|| Some(VIDEOTOOLBOX_BACKEND_NAME.to_owned())),
            decoder_name: session_info.decoder_name.unwrap_or_else(|| {
                self.decoder_plugin_name
                    .clone()
                    .unwrap_or_else(|| factory.name().to_owned())
            }),
            fallback_reason: None,
        };
        let probe = player_backend_ffmpeg::MediaProbe {
            source: source.clone(),
            duration: None,
            bit_rate: None,
            audio_streams: 0,
            video_streams: 1,
            best_video: Some(player_backend_ffmpeg::VideoStreamProbe {
                index: stream_info.stream_index,
                codec: stream_info.codec.clone(),
                width: stream_info.width.unwrap_or_default(),
                height: stream_info.height.unwrap_or_default(),
                frame_rate: stream_info.frame_rate,
            }),
            best_audio: None,
        };
        let outstanding_frames = Arc::new(AtomicUsize::new(0));
        let session = Arc::new(Mutex::new(session));
        let shared = Arc::new(Mutex::new(MacosNativeFrameDecoderState {
            frame_processor_chain,
            presenter: Some(presenter),
            presentation_epoch: 0,
        }));
        let (command_tx, command_rx) = mpsc::channel();
        let (frame_tx, frame_rx) = mpsc::channel();
        let current_generation = Arc::new(AtomicU64::new(0));
        let buffered_frame_count = Arc::new(AtomicUsize::new(0));
        let prefetch_limit = Arc::new(AtomicUsize::new(1));
        let prefetch_wakeup = Arc::new(MacosNativeFramePrefetchWakeup::default());
        let worker = spawn_macos_native_frame_prefetch_worker(
            Box::new(SourceNormalizerPacketSource::new(
                self.packet_session.clone(),
            )),
            session.clone(),
            shared.clone(),
            outstanding_frames.clone(),
            command_rx,
            frame_tx,
            current_generation.clone(),
            buffered_frame_count.clone(),
            prefetch_limit.clone(),
            prefetch_wakeup.clone(),
        )?;

        Ok(DesktopVideoSourceBootstrap {
            source: Box::new(MacosNativeFrameVideoSource {
                stream_info,
                session,
                shared,
                outstanding_frames,
                command_tx,
                frame_rx,
                generation: 0,
                current_generation,
                buffered_frame_count,
                prefetch_limit,
                prefetch_wakeup,
                end_of_input_sent: false,
                end_of_stream_received: false,
                worker: Some(worker),
            }),
            decode_info,
            probe,
        })
    }
}

impl DesktopVideoSourceFactory for MacosNativeFrameVideoSourceFactory {
    fn open_video_source(
        &self,
        source: MediaSource,
        _buffer_capacity: usize,
        interrupt_flag: Option<Arc<AtomicBool>>,
    ) -> anyhow::Result<DesktopVideoSourceBootstrap> {
        let backend = FfmpegBackend::new().context("failed to initialize FFmpeg backend")?;
        let probe = backend
            .probe_with_interrupt(source.clone(), interrupt_flag.clone())
            .context("failed to probe media source for native-frame decoder")?;
        let packet_source = backend
            .open_video_packet_source_with_interrupt(source, interrupt_flag)
            .context("failed to open FFmpeg packet source for native-frame decoder")?;
        let stream_info = packet_source.stream_info().clone();
        let plugin = LoadedDynamicPlugin::load(&self.plugin_path).with_context(|| {
            format!(
                "failed to load native-frame decoder plugin {}",
                self.plugin_path.display()
            )
        })?;
        let factory = plugin.native_decoder_plugin_factory().ok_or_else(|| {
            anyhow::anyhow!("decoder plugin does not export a v2 native-frame API")
        })?;
        if !factory
            .capabilities()
            .supports_codec(&stream_info.codec, DecoderMediaKind::Video)
        {
            anyhow::bail!(
                "native-frame decoder plugin `{}` does not support {} video",
                factory.name(),
                stream_info.codec
            );
        }

        let session = factory
            .open_native_session(&DecoderSessionConfig {
                codec: stream_info.codec.clone(),
                media_kind: DecoderMediaKind::Video,
                extradata: stream_info.extradata.clone(),
                bitstream_format: Some(macos_decoder_bitstream_format(&stream_info.codec)),
                width: stream_info.width,
                height: stream_info.height,
                coded_width: stream_info.width,
                coded_height: stream_info.height,
                prefer_hardware: true,
                require_cpu_output: false,
                ..DecoderSessionConfig::default()
            })
            .map_err(|error| anyhow::anyhow!(error.to_string()))?;
        let session_info = session.session_info();
        let presenter = MacosMetalLayerPresenter::new(self.video_surface)
            .map_err(|error| anyhow::anyhow!(error.message().to_owned()))?;
        let frame_processor_chain = open_macos_frame_processor_chain(
            &stream_info,
            &self.frame_processor_paths,
            self.frame_processor_mode,
            self.frame_processor_policy.clone(),
        )?;
        let decode_info = BackendVideoDecodeInfo {
            selected_mode: BackendVideoDecoderMode::Hardware,
            hardware_available: true,
            hardware_backend: session_info
                .selected_hardware_backend
                .or_else(|| Some(VIDEOTOOLBOX_BACKEND_NAME.to_owned())),
            decoder_name: session_info
                .decoder_name
                .unwrap_or_else(|| factory.name().to_owned()),
            fallback_reason: None,
        };
        let outstanding_frames = Arc::new(AtomicUsize::new(0));
        let session = Arc::new(Mutex::new(session));
        let shared = Arc::new(Mutex::new(MacosNativeFrameDecoderState {
            frame_processor_chain,
            presenter: Some(presenter),
            presentation_epoch: 0,
        }));
        let (command_tx, command_rx) = mpsc::channel();
        let (frame_tx, frame_rx) = mpsc::channel();
        let current_generation = Arc::new(AtomicU64::new(0));
        let buffered_frame_count = Arc::new(AtomicUsize::new(0));
        let prefetch_limit = Arc::new(AtomicUsize::new(1));
        let prefetch_wakeup = Arc::new(MacosNativeFramePrefetchWakeup::default());
        let worker = spawn_macos_native_frame_prefetch_worker(
            Box::new(packet_source),
            session.clone(),
            shared.clone(),
            outstanding_frames.clone(),
            command_rx,
            frame_tx,
            current_generation.clone(),
            buffered_frame_count.clone(),
            prefetch_limit.clone(),
            prefetch_wakeup.clone(),
        )?;

        Ok(DesktopVideoSourceBootstrap {
            source: Box::new(MacosNativeFrameVideoSource {
                stream_info,
                session,
                shared,
                outstanding_frames,
                command_tx,
                frame_rx,
                generation: 0,
                current_generation,
                buffered_frame_count,
                prefetch_limit,
                prefetch_wakeup,
                end_of_input_sent: false,
                end_of_stream_received: false,
                worker: Some(worker),
            }),
            decode_info,
            probe,
        })
    }
}

impl DesktopVideoSource for MacosNativeFrameVideoSource {
    fn recv_frame(&mut self) -> anyhow::Result<Option<DesktopVideoFrame>> {
        self.recv_prefetched_frame()
    }

    fn try_recv_frame(&mut self) -> anyhow::Result<DesktopVideoFramePoll> {
        self.try_recv_prefetched_frame()
    }

    fn seek_to(&mut self, position: Duration) -> anyhow::Result<Option<DesktopVideoFrame>> {
        {
            let mut shared = self
                .shared
                .lock()
                .map_err(|_| anyhow::anyhow!("native-frame decoder state is poisoned"))?;
            shared.presentation_epoch = shared.presentation_epoch.saturating_add(1);
        }
        self.generation = self.generation.wrapping_add(1);
        self.current_generation
            .store(self.generation, Ordering::SeqCst);
        self.buffered_frame_count.store(0, Ordering::SeqCst);
        self.end_of_input_sent = false;
        self.end_of_stream_received = false;
        self.command_tx
            .send(MacosNativeFrameWorkerCommand::Seek {
                generation: self.generation,
                position,
            })
            .context("failed to send seek request to macOS native-frame prefetch worker")?;
        self.prefetch_wakeup.notify();
        self.recv_prefetched_frame()
    }

    fn buffered_frame_count(&self) -> usize {
        self.buffered_frame_count.load(Ordering::SeqCst)
    }

    fn set_prefetch_limit(&self, limit: usize) {
        self.prefetch_limit.store(limit.max(1), Ordering::SeqCst);
        self.prefetch_wakeup.notify();
    }

    fn drain_events(&mut self) -> Vec<PlayerRuntimeEvent> {
        self.shared
            .lock()
            .ok()
            .and_then(|mut shared| {
                shared
                    .frame_processor_chain
                    .as_mut()
                    .map(MacosFrameProcessorChain::drain_events)
            })
            .unwrap_or_default()
    }
}

impl MacosNativeFrameVideoSource {
    pub(crate) fn recv_prefetched_frame(&mut self) -> anyhow::Result<Option<DesktopVideoFrame>> {
        loop {
            if self.end_of_input_sent {
                return Ok(None);
            }

            let event = self
                .frame_rx
                .recv()
                .context("macOS native-frame prefetch worker disconnected")?;
            if let Some(frame) = self.handle_prefetch_event(event)? {
                return Ok(Some(frame));
            }

            if self.end_of_input_sent {
                return Ok(None);
            }
        }
    }

    pub(crate) fn try_recv_prefetched_frame(&mut self) -> anyhow::Result<DesktopVideoFramePoll> {
        if self.end_of_input_sent {
            return Ok(DesktopVideoFramePoll::EndOfStream);
        }

        loop {
            match self.frame_rx.try_recv() {
                Ok(event) => {
                    if let Some(frame) = self.handle_prefetch_event(event)? {
                        return Ok(DesktopVideoFramePoll::Ready(frame));
                    }
                    if self.end_of_input_sent {
                        return Ok(DesktopVideoFramePoll::EndOfStream);
                    }
                }
                Err(TryRecvError::Empty) => return Ok(DesktopVideoFramePoll::Pending),
                Err(TryRecvError::Disconnected) => {
                    anyhow::bail!("macOS native-frame prefetch worker disconnected")
                }
            }
        }
    }

    pub(crate) fn handle_prefetch_event(
        &mut self,
        event: MacosNativeFrameWorkerEvent,
    ) -> anyhow::Result<Option<DesktopVideoFrame>> {
        match event {
            MacosNativeFrameWorkerEvent::Frame { generation, frame }
                if generation == self.generation =>
            {
                decrement_macos_native_frame_buffered_count(
                    &self.buffered_frame_count,
                    &self.prefetch_wakeup,
                );
                self.deferred_desktop_frame(frame).map(Some)
            }
            MacosNativeFrameWorkerEvent::Frame { frame, .. } => {
                decrement_macos_native_frame_buffered_count(
                    &self.buffered_frame_count,
                    &self.prefetch_wakeup,
                );
                if let (Ok(mut session), Ok(mut shared)) = (self.session.lock(), self.shared.lock())
                {
                    let _ = release_macos_processor_frame_and_track(
                        session.as_mut(),
                        &mut shared,
                        self.outstanding_frames.as_ref(),
                        frame,
                    );
                }
                Ok(None)
            }
            MacosNativeFrameWorkerEvent::EndOfStream { generation }
                if generation == self.generation =>
            {
                self.end_of_input_sent = true;
                self.end_of_stream_received = true;
                Ok(None)
            }
            MacosNativeFrameWorkerEvent::Error {
                generation,
                message,
            } if generation == self.generation => Err(anyhow::anyhow!(message)),
            _ => Ok(None),
        }
    }

    pub(crate) fn release_queued_prefetch_events(&mut self) {
        while let Ok(event) = self.frame_rx.try_recv() {
            if let MacosNativeFrameWorkerEvent::Frame { frame, .. } = event {
                decrement_macos_native_frame_buffered_count(
                    &self.buffered_frame_count,
                    &self.prefetch_wakeup,
                );
                if let (Ok(mut session), Ok(mut shared)) = (self.session.lock(), self.shared.lock())
                {
                    let _ = release_macos_processor_frame_and_track(
                        session.as_mut(),
                        &mut shared,
                        self.outstanding_frames.as_ref(),
                        frame,
                    );
                }
            }
        }
    }

    pub(crate) fn deferred_desktop_frame(
        &self,
        frame: MacosFrameProcessorFrame,
    ) -> anyhow::Result<DesktopVideoFrame> {
        if frame.presentation_frame.metadata.handle_kind != DecoderNativeHandleKind::CvPixelBuffer {
            let mut session = self
                .session
                .lock()
                .map_err(|_| anyhow::anyhow!("native-frame decoder session is poisoned"))?;
            let mut shared = self
                .shared
                .lock()
                .map_err(|_| anyhow::anyhow!("native-frame decoder state is poisoned"))?;
            let _ = release_macos_processor_frame_and_track(
                session.as_mut(),
                &mut shared,
                self.outstanding_frames.as_ref(),
                frame,
            );
            anyhow::bail!("macOS native-frame presenter only accepts CVPixelBuffer handles");
        }
        let presentation_time = frame
            .presentation_frame
            .metadata
            .pts_us
            .and_then(duration_from_micros)
            .unwrap_or(Duration::ZERO);
        let width = frame.presentation_frame.metadata.width;
        let height = frame.presentation_frame.metadata.height;
        Ok(DesktopVideoFrame::native_deferred(
            presentation_time,
            width,
            height,
            Box::new(MacosDeferredNativeFramePresentation {
                session: self.session.clone(),
                shared: self.shared.clone(),
                outstanding_frames: self.outstanding_frames.clone(),
                frame: Some(frame),
                presentation_epoch: shared_presentation_epoch(&self.shared)?,
            }),
        ))
    }
}

pub(crate) fn spawn_macos_native_frame_prefetch_worker(
    packet_source: Box<dyn MacosNativeFramePacketSource>,
    session: Arc<Mutex<Box<dyn NativeDecoderSession>>>,
    shared: Arc<Mutex<MacosNativeFrameDecoderState>>,
    outstanding_frames: Arc<AtomicUsize>,
    command_rx: Receiver<MacosNativeFrameWorkerCommand>,
    frame_tx: Sender<MacosNativeFrameWorkerEvent>,
    current_generation: Arc<AtomicU64>,
    buffered_frame_count: Arc<AtomicUsize>,
    prefetch_limit: Arc<AtomicUsize>,
    prefetch_wakeup: Arc<MacosNativeFramePrefetchWakeup>,
) -> anyhow::Result<JoinHandle<()>> {
    thread::Builder::new()
        .name("macos-native-frame-prefetch".to_owned())
        .spawn(move || {
            macos_native_frame_prefetch_worker_loop(
                packet_source,
                session,
                shared,
                outstanding_frames,
                command_rx,
                frame_tx,
                current_generation,
                buffered_frame_count,
                prefetch_limit,
                prefetch_wakeup,
            );
        })
        .context("failed to spawn macOS native-frame prefetch worker")
}

pub(crate) fn macos_native_frame_prefetch_worker_loop(
    mut packet_source: Box<dyn MacosNativeFramePacketSource>,
    session: Arc<Mutex<Box<dyn NativeDecoderSession>>>,
    shared: Arc<Mutex<MacosNativeFrameDecoderState>>,
    outstanding_frames: Arc<AtomicUsize>,
    command_rx: Receiver<MacosNativeFrameWorkerCommand>,
    frame_tx: Sender<MacosNativeFrameWorkerEvent>,
    current_generation: Arc<AtomicU64>,
    buffered_frame_count: Arc<AtomicUsize>,
    prefetch_limit: Arc<AtomicUsize>,
    prefetch_wakeup: Arc<MacosNativeFramePrefetchWakeup>,
) {
    let mut generation = 0u64;
    let mut end_of_input_sent = false;
    let mut end_of_stream_received = false;
    let mut pending_event = None;
    let mut wakeup_sequence = 0u64;

    loop {
        match latest_macos_native_frame_worker_command(&command_rx) {
            Some(MacosNativeFrameWorkerCommand::Shutdown) => break,
            Some(MacosNativeFrameWorkerCommand::Seek {
                generation: new_generation,
                position,
            }) => {
                generation = new_generation;
                pending_event = None;
                end_of_input_sent = false;
                end_of_stream_received = false;
                let seek_result = flush_and_seek_macos_native_frame_source(
                    &session,
                    &shared,
                    packet_source.as_mut(),
                    position,
                );
                if let Err(error) = seek_result {
                    pending_event = Some(MacosNativeFrameWorkerEvent::Error {
                        generation,
                        message: error.to_string(),
                    });
                }
            }
            None => {}
        }

        if pending_event.is_none() {
            if end_of_stream_received {
                wait_for_macos_native_frame_prefetch_work(&prefetch_wakeup, &mut wakeup_sequence);
                continue;
            }
            let limit = prefetch_limit.load(Ordering::SeqCst).max(1);
            if buffered_frame_count.load(Ordering::SeqCst) >= limit {
                wait_for_macos_native_frame_prefetch_work(&prefetch_wakeup, &mut wakeup_sequence);
                continue;
            }
            pending_event = Some(
                match decode_next_macos_native_frame_worker_event(
                    &shared,
                    &session,
                    &outstanding_frames,
                    packet_source.as_mut(),
                    generation,
                    &mut end_of_input_sent,
                    &mut end_of_stream_received,
                ) {
                    Ok(event) => event,
                    Err(error) => MacosNativeFrameWorkerEvent::Error {
                        generation,
                        message: error.to_string(),
                    },
                },
            );
        }

        let Some(event) = pending_event.take() else {
            continue;
        };
        let frame_generation = macos_native_frame_worker_frame_generation(&event);
        if let Some(event_generation) = frame_generation
            && event_generation == current_generation.load(Ordering::SeqCst)
        {
            buffered_frame_count.fetch_add(1, Ordering::SeqCst);
        }
        match frame_tx.send(event) {
            Ok(()) => {}
            Err(event) => {
                if let Some(event_generation) = frame_generation
                    && event_generation == current_generation.load(Ordering::SeqCst)
                {
                    decrement_macos_native_frame_buffered_count(
                        &buffered_frame_count,
                        &prefetch_wakeup,
                    );
                }
                if let MacosNativeFrameWorkerEvent::Frame { frame, .. } = event.0
                    && let (Ok(mut session), Ok(mut shared)) = (session.lock(), shared.lock())
                {
                    let _ = release_macos_processor_frame_and_track(
                        session.as_mut(),
                        &mut shared,
                        outstanding_frames.as_ref(),
                        frame,
                    );
                }
                break;
            }
        }
    }
}

pub(crate) fn latest_macos_native_frame_worker_command(
    command_rx: &Receiver<MacosNativeFrameWorkerCommand>,
) -> Option<MacosNativeFrameWorkerCommand> {
    let mut latest = None;
    loop {
        match command_rx.try_recv() {
            Ok(MacosNativeFrameWorkerCommand::Shutdown) => {
                return Some(MacosNativeFrameWorkerCommand::Shutdown);
            }
            Ok(command) => latest = Some(command),
            Err(TryRecvError::Empty) => return latest,
            Err(TryRecvError::Disconnected) => {
                return Some(MacosNativeFrameWorkerCommand::Shutdown);
            }
        }
    }
}

pub(crate) fn wait_for_macos_native_frame_prefetch_work(
    wakeup: &MacosNativeFramePrefetchWakeup,
    observed_sequence: &mut u64,
) {
    wakeup.wait_for_change(observed_sequence);
}

pub(crate) fn macos_native_frame_worker_frame_generation(
    event: &MacosNativeFrameWorkerEvent,
) -> Option<u64> {
    match event {
        MacosNativeFrameWorkerEvent::Frame { generation, .. } => Some(*generation),
        _ => None,
    }
}

pub(crate) fn decrement_macos_native_frame_buffered_count(
    buffered_frame_count: &AtomicUsize,
    wakeup: &MacosNativeFramePrefetchWakeup,
) {
    let _ = buffered_frame_count.fetch_update(Ordering::SeqCst, Ordering::SeqCst, |count| {
        Some(count.saturating_sub(1))
    });
    wakeup.notify();
}

pub(crate) fn flush_and_seek_macos_native_frame_source(
    session: &Arc<Mutex<Box<dyn NativeDecoderSession>>>,
    shared: &Arc<Mutex<MacosNativeFrameDecoderState>>,
    packet_source: &mut dyn MacosNativeFramePacketSource,
    position: Duration,
) -> anyhow::Result<()> {
    {
        let mut session = session
            .lock()
            .map_err(|_| anyhow::anyhow!("native-frame decoder session is poisoned"))?;
        session
            .flush()
            .map_err(|error| anyhow::anyhow!(error.to_string()))?;
    }
    {
        let mut shared = shared
            .lock()
            .map_err(|_| anyhow::anyhow!("native-frame decoder state is poisoned"))?;
        if let Some(chain) = shared.frame_processor_chain.as_mut() {
            chain.flush();
        }
    }
    packet_source.seek_to(position)
}

pub(crate) fn decode_next_macos_native_frame_worker_event(
    shared: &Arc<Mutex<MacosNativeFrameDecoderState>>,
    session: &Arc<Mutex<Box<dyn NativeDecoderSession>>>,
    outstanding_frames: &AtomicUsize,
    packet_source: &mut dyn MacosNativeFramePacketSource,
    generation: u64,
    end_of_input_sent: &mut bool,
    end_of_stream_received: &mut bool,
) -> anyhow::Result<MacosNativeFrameWorkerEvent> {
    loop {
        match receive_macos_native_frame_from_decoder(shared, session, outstanding_frames)? {
            MacosNativeFramePoll::Frame(frame) => {
                return Ok(MacosNativeFrameWorkerEvent::Frame { generation, frame });
            }
            MacosNativeFramePoll::Decoder(DecoderReceiveNativeFrameOutput::Eof) => {
                *end_of_stream_received = true;
                return Ok(MacosNativeFrameWorkerEvent::EndOfStream { generation });
            }
            MacosNativeFramePoll::Decoder(DecoderReceiveNativeFrameOutput::NeedMoreInput) => {}
            MacosNativeFramePoll::Decoder(DecoderReceiveNativeFrameOutput::Frame(_)) => {}
        }

        if *end_of_input_sent {
            thread::sleep(MACOS_NATIVE_FRAME_DECODER_DRAIN_RETRY_INTERVAL);
            continue;
        }

        match packet_source.send_next_packet(session)? {
            MacosNativeFramePacketSendStatus::Sent => {}
            MacosNativeFramePacketSendStatus::NeedMoreData => {
                thread::sleep(MACOS_NATIVE_FRAME_DECODER_DRAIN_RETRY_INTERVAL);
            }
            MacosNativeFramePacketSendStatus::EndOfStream => {
                send_macos_native_frame_end_of_stream(session)?;
                *end_of_input_sent = true;
            }
        }
    }
}

pub(crate) fn receive_macos_native_frame_from_decoder(
    shared: &Arc<Mutex<MacosNativeFrameDecoderState>>,
    session: &Arc<Mutex<Box<dyn NativeDecoderSession>>>,
    outstanding_frames: &AtomicUsize,
) -> anyhow::Result<MacosNativeFramePoll> {
    let mut session = session
        .lock()
        .map_err(|_| anyhow::anyhow!("native-frame decoder session is poisoned"))?;
    let result = session
        .receive_native_frame()
        .map_err(|error| anyhow::anyhow!(error.to_string()))?;
    let DecoderReceiveNativeFrameOutput::Frame(frame) = result else {
        return Ok(MacosNativeFramePoll::Decoder(result));
    };
    outstanding_frames.fetch_add(1, Ordering::SeqCst);
    let mut shared = shared
        .lock()
        .map_err(|_| anyhow::anyhow!("native-frame decoder state is poisoned"))?;
    let frame = match process_macos_native_frame(&mut shared, frame) {
        Ok(frame) => frame,
        Err((error, frame_for_release)) => {
            let _ = release_native_frame_with_counter(
                session.as_mut(),
                outstanding_frames,
                frame_for_release,
            );
            return Err(error);
        }
    };
    Ok(MacosNativeFramePoll::Frame(frame))
}

pub(crate) fn send_macos_native_frame_packet(
    session: &Arc<Mutex<Box<dyn NativeDecoderSession>>>,
    packet: CompressedVideoPacket,
) -> anyhow::Result<()> {
    send_macos_native_frame_packet_bytes(
        session,
        DecoderPacket {
            pts_us: packet.pts_us,
            dts_us: packet.dts_us,
            duration_us: packet.duration_us,
            stream_index: packet.stream_index,
            key_frame: packet.key_frame,
            discontinuity: packet.discontinuity,
            end_of_stream: false,
        },
        &packet.data,
    )
    .map(|_| ())
}

pub(crate) fn source_normalizer_packet_metadata(
    metadata: &SourceNormalizerReadPacketMetadata,
) -> anyhow::Result<DecoderPacket> {
    let packet = metadata
        .packet
        .clone()
        .ok_or_else(|| anyhow::anyhow!("source normalizer packet metadata was missing"))?;
    Ok(DecoderPacket {
        pts_us: packet.pts_us,
        dts_us: packet.dts_us,
        duration_us: packet.duration_us,
        stream_index: packet.stream_index,
        key_frame: packet.key_frame,
        discontinuity: packet.discontinuity,
        end_of_stream: packet.end_of_stream,
    })
}

pub(crate) fn send_macos_native_frame_packet_bytes(
    session: &Arc<Mutex<Box<dyn NativeDecoderSession>>>,
    packet: DecoderPacket,
    data: &[u8],
) -> anyhow::Result<player_plugin::DecoderPacketResult> {
    let mut session = session
        .lock()
        .map_err(|_| anyhow::anyhow!("native-frame decoder session is poisoned"))?;
    session
        .send_packet(&packet, data)
        .map_err(|error| anyhow::anyhow!(error.to_string()))
}

pub(crate) fn send_macos_native_frame_end_of_stream(
    session: &Arc<Mutex<Box<dyn NativeDecoderSession>>>,
) -> anyhow::Result<()> {
    let mut session = session
        .lock()
        .map_err(|_| anyhow::anyhow!("native-frame decoder session is poisoned"))?;
    session
        .send_packet(
            &DecoderPacket {
                end_of_stream: true,
                ..DecoderPacket::default()
            },
            &[],
        )
        .map(|_| ())
        .map_err(|error| anyhow::anyhow!(error.to_string()))
}

impl DesktopVideoFramePresentation for MacosDeferredNativeFramePresentation {
    fn present(mut self: Box<Self>) -> anyhow::Result<()> {
        let Some(frame) = self.frame.take() else {
            return Ok(());
        };
        present_and_release_macos_processor_frame(
            &self.session,
            &self.shared,
            self.outstanding_frames.as_ref(),
            frame,
            self.presentation_epoch,
        )
    }
}

impl Drop for MacosDeferredNativeFramePresentation {
    fn drop(&mut self) {
        if let Some(frame) = self.frame.take()
            && let (Ok(mut session), Ok(mut shared)) = (self.session.lock(), self.shared.lock())
        {
            let _ = release_macos_processor_frame_and_track(
                session.as_mut(),
                &mut shared,
                self.outstanding_frames.as_ref(),
                frame,
            );
        }
    }
}

pub(crate) fn release_macos_processor_frame_and_track(
    session: &mut dyn NativeDecoderSession,
    shared: &mut MacosNativeFrameDecoderState,
    outstanding_frames: &AtomicUsize,
    frame: MacosFrameProcessorFrame,
) -> anyhow::Result<()> {
    if let Some(chain) = shared.frame_processor_chain.as_mut() {
        chain.release_processor_outputs(frame.processor_outputs);
    }
    release_native_frame_with_counter(session, outstanding_frames, frame.decoder_frame)
        .map_err(|error| anyhow::anyhow!(error.to_string()))
}

pub(crate) fn release_native_frame_with_counter(
    session: &mut dyn NativeDecoderSession,
    outstanding_frames: &AtomicUsize,
    frame: DecoderNativeFrame,
) -> Result<(), player_plugin::DecoderError> {
    let result = session.release_native_frame(frame);
    if result.is_ok() {
        outstanding_frames.fetch_sub(1, Ordering::SeqCst);
    }
    result
}

pub(crate) fn present_and_release_macos_processor_frame(
    session: &Arc<Mutex<Box<dyn NativeDecoderSession>>>,
    shared: &Arc<Mutex<MacosNativeFrameDecoderState>>,
    outstanding_frames: &AtomicUsize,
    frame: MacosFrameProcessorFrame,
    presentation_epoch: u64,
) -> anyhow::Result<()> {
    let mut session = session
        .lock()
        .map_err(|_| anyhow::anyhow!("native-frame decoder session is poisoned"))?;
    let mut shared = shared
        .lock()
        .map_err(|_| anyhow::anyhow!("native-frame decoder state is poisoned"))?;
    if shared.presentation_epoch != presentation_epoch {
        return release_macos_processor_frame_and_track(
            session.as_mut(),
            &mut shared,
            outstanding_frames,
            frame,
        );
    }
    let presenter = shared
        .presenter
        .as_mut()
        .ok_or_else(|| anyhow::anyhow!("macOS native-frame presenter is not configured"))?;
    let present_result = presenter
        .present_cv_pixel_buffer_handle(frame.presentation_frame.handle)
        .map_err(|error| anyhow::anyhow!(error.message().to_owned()));
    let release_result = release_macos_processor_frame_and_track(
        session.as_mut(),
        &mut shared,
        outstanding_frames,
        frame,
    );
    present_result.and(release_result)
}

#[cfg(test)]
pub(crate) fn present_if_current_epoch_and_release(
    session: &mut dyn NativeDecoderSession,
    outstanding_frames: &AtomicUsize,
    current_epoch: u64,
    presentation_epoch: u64,
    frame: DecoderNativeFrame,
    present: impl FnOnce(DecoderNativeFrame) -> anyhow::Result<()>,
) -> anyhow::Result<()> {
    if current_epoch != presentation_epoch {
        return release_native_frame_with_counter(session, outstanding_frames, frame)
            .map_err(|error| anyhow::anyhow!(error.to_string()));
    }
    present(frame)
}

pub(crate) fn shared_presentation_epoch(
    shared: &Arc<Mutex<MacosNativeFrameDecoderState>>,
) -> anyhow::Result<u64> {
    shared
        .lock()
        .map(|state| state.presentation_epoch)
        .map_err(|_| anyhow::anyhow!("native-frame decoder state is poisoned"))
}

#[cfg(test)]
pub(crate) fn present_and_release_native_frame_with_presenter(
    session: &mut dyn NativeDecoderSession,
    outstanding_frames: &AtomicUsize,
    frame: DecoderNativeFrame,
    present: impl FnOnce(usize) -> Result<(), String>,
) -> anyhow::Result<()> {
    let present_result = present(frame.handle).map_err(|error| anyhow::anyhow!(error));
    let release_result = release_native_frame_with_counter(session, outstanding_frames, frame)
        .map_err(|error| anyhow::anyhow!(error.to_string()));

    present_result.and(release_result)
}
