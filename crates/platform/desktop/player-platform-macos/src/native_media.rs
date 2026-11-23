use super::*;
use crossbeam_queue::ArrayQueue;
use player_platform_native_frame::{
    MediaPacket, MediaPacketPoll, MediaPacketSource, NativeFramePipelineError, NativeMediaPipeline,
    NativeMediaVideoPoll,
};
use player_plugin_abi::{
    VESPER_HANDLE_CV_PIXEL_BUFFER, VESPER_PACKET_DISCONTINUITY, VESPER_PACKET_KEYFRAME,
};
use player_plugin_loader::native::{LoadError, NativePlugin, NativeVideoDecoder, NativeVideoFrame};
use std::sync::mpsc::{SyncSender, TryRecvError, sync_channel};

// Leave independent capacity for GPU leases and the runtime's pending frame.
const VIDEO_PREFETCH_CAPACITY: usize = 48;
const FRAME_WAIT: Duration = Duration::from_secs(3);
const WORKER_IDLE: Duration = Duration::from_millis(2);

pub(crate) fn open_native_decoder(
    path: &std::path::Path,
    info: &VideoPacketStreamInfo,
) -> anyhow::Result<Option<NativeVideoDecoder>> {
    let fourcc = match player_plugin::normalize_decoder_codec_identifier(&info.codec).as_str() {
        "h264" | "avc" | "avc1" => *b"avc1",
        "avc3" => *b"avc3",
        "hevc" | "h265" | "hvc1" => *b"hvc1",
        "hev1" => *b"hev1",
        _ => return Ok(None),
    };
    // SAFETY: the caller verified desktop development policy and selected
    // the same trusted library for the native-frame route.
    let plugin = match unsafe { NativePlugin::load(path) } {
        Ok(plugin) => plugin,
        Err(LoadError::Library(_)) => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    if !plugin
        .interface_ids()
        .contains(&player_plugin_abi::VESPER_VIDEO_DECODER_ID)
    {
        return Ok(None);
    }
    let params = serde_json::to_vec(&serde_json::json!({ "reorderDepth": info.reorder_depth }))?;
    let decoder = plugin.open_video_decoder(
        u32::from_be_bytes(fourcc),
        &info.extradata,
        &params,
        Duration::from_millis(300),
    )?;
    tracing::info!(
        plugin_id = plugin.id(),
        "macOS selected a native VideoToolbox decoder plugin"
    );
    Ok(Some(decoder))
}

pub(crate) struct FfmpegPackets(pub(crate) VideoPacketSource);

impl MediaPacketSource for FfmpegPackets {
    type Storage = Vec<u8>;

    fn next_video_packet(
        &mut self,
    ) -> Result<MediaPacketPoll<Self::Storage>, NativeFramePipelineError> {
        self.0
            .next_packet()
            .map(|packet| {
                packet.map_or(MediaPacketPoll::EndOfStream, |packet| {
                    MediaPacketPoll::Packet(MediaPacket {
                        pts_us: packet.pts_us,
                        dts_us: packet.dts_us,
                        duration_us: packet.duration_us,
                        flags: if packet.key_frame {
                            VESPER_PACKET_KEYFRAME
                        } else {
                            0
                        } | if packet.discontinuity {
                            VESPER_PACKET_DISCONTINUITY
                        } else {
                            0
                        },
                        stream_index: packet.stream_index,
                        bytes: packet.data,
                    })
                })
            })
            .map_err(|error| NativeFramePipelineError::new("readVideoPacket", error.to_string()))
    }

    fn seek(&mut self, position: Duration) -> Result<(), NativeFramePipelineError> {
        self.0
            .seek_to(position)
            .map_err(|error| NativeFramePipelineError::new("seekVideoPackets", error.to_string()))
    }
}

type Output = (u64, Result<NativeMediaVideoPoll, NativeFramePipelineError>);

struct WorkerSignals {
    output: ArrayQueue<Output>,
    limit: AtomicUsize,
    cancelled: Arc<AtomicBool>,
}

pub(crate) struct MacosNativeMediaSource {
    signals: Arc<WorkerSignals>,
    commands: SyncSender<(u64, Duration)>,
    worker: Option<JoinHandle<()>>,
    presenter: Arc<Mutex<Option<MacosMetalLayerPresenter>>>,
    epoch: Arc<AtomicU64>,
    ended: bool,
}

impl MacosNativeMediaSource {
    pub(crate) fn new<S: MediaPacketSource + 'static>(
        decoder: NativeVideoDecoder,
        packets: S,
        presenter: MacosMetalLayerPresenter,
        buffer_capacity: usize,
        cancelled: Arc<AtomicBool>,
    ) -> anyhow::Result<Self> {
        let capacity = buffer_capacity.clamp(1, VIDEO_PREFETCH_CAPACITY);
        let signals = Arc::new(WorkerSignals {
            output: ArrayQueue::new(capacity),
            limit: AtomicUsize::new(1),
            cancelled,
        });
        let (commands, inbox) = sync_channel(1);
        let worker_signals = Arc::clone(&signals);
        let worker = thread::Builder::new()
            .name("vesper-native-media-macos".to_owned())
            .spawn(move || {
                run_worker(
                    NativeMediaPipeline::new(packets, decoder),
                    inbox,
                    worker_signals,
                );
            })?;
        Ok(Self {
            signals,
            commands,
            worker: Some(worker),
            presenter: Arc::new(Mutex::new(Some(presenter))),
            epoch: Arc::new(AtomicU64::new(0)),
            ended: false,
        })
    }

    fn unpark_worker(&self) {
        if let Some(worker) = &self.worker {
            worker.thread().unpark();
        }
    }
}

fn run_worker<S: MediaPacketSource>(
    mut pipeline: NativeMediaPipeline<S>,
    inbox: Receiver<(u64, Duration)>,
    signals: Arc<WorkerSignals>,
) {
    let mut epoch = 0;
    let mut ended = false;
    while !signals.cancelled.load(Ordering::Acquire) {
        match inbox.try_recv() {
            Ok((next, position)) => {
                epoch = next;
                ended = false;
                while signals.output.pop().is_some() {}
                if let Err(error) = pipeline.seek(position) {
                    let _ = signals.output.push((epoch, Err(error)));
                    ended = true;
                }
            }
            Err(TryRecvError::Disconnected) => break,
            Err(TryRecvError::Empty) => {}
        }
        if ended || signals.output.len() >= signals.limit.load(Ordering::Relaxed) {
            thread::park_timeout(WORKER_IDLE);
            continue;
        }
        let result = pipeline.advance();
        if matches!(result, Ok(NativeMediaVideoPoll::Pending)) {
            thread::park_timeout(WORKER_IDLE);
            continue;
        }
        ended = result.is_err() || matches!(result, Ok(NativeMediaVideoPoll::EndOfStream));
        // This worker is the sole producer and checked the bounded capacity.
        let _ = signals.output.push((epoch, result));
    }
}

impl DesktopVideoSource for MacosNativeMediaSource {
    fn recv_frame(&mut self) -> anyhow::Result<Option<DesktopVideoFrame>> {
        let deadline = Instant::now() + FRAME_WAIT;
        loop {
            match self.try_recv_frame()? {
                DesktopVideoFramePoll::Ready(frame) => return Ok(Some(frame)),
                DesktopVideoFramePoll::EndOfStream => return Ok(None),
                DesktopVideoFramePoll::Pending => {}
            }
            if self.signals.cancelled.load(Ordering::Acquire) || Instant::now() >= deadline {
                anyhow::bail!(
                    "macOS native media pipeline did not produce a frame before its deadline"
                );
            }
            thread::sleep(WORKER_IDLE);
        }
    }

    fn try_recv_frame(&mut self) -> anyhow::Result<DesktopVideoFramePoll> {
        if self.ended {
            return Ok(DesktopVideoFramePoll::EndOfStream);
        }
        for _ in 0..self.signals.output.capacity() {
            let Some((epoch, result)) = self.signals.output.pop() else {
                break;
            };
            self.unpark_worker();
            if epoch != self.epoch.load(Ordering::Acquire) {
                continue;
            }
            match result? {
                NativeMediaVideoPoll::Frame { lease, format } => {
                    let raw = lease.descriptor();
                    if raw.handle_kind != VESPER_HANDLE_CV_PIXEL_BUFFER {
                        anyhow::bail!(
                            "Metal presenter expected a CVPixelBuffer lease, received handle kind {}",
                            raw.handle_kind
                        );
                    }
                    let pts =
                        u64::try_from(raw.pts_us).map_or(Duration::ZERO, Duration::from_micros);
                    return Ok(DesktopVideoFramePoll::Ready(
                        DesktopVideoFrame::native_deferred(
                            pts,
                            format.width,
                            format.height,
                            Box::new(Presentation {
                                lease,
                                epoch,
                                current_epoch: Arc::clone(&self.epoch),
                                presenter: Arc::clone(&self.presenter),
                            }),
                        ),
                    ));
                }
                NativeMediaVideoPoll::EndOfStream => {
                    self.ended = true;
                    return Ok(DesktopVideoFramePoll::EndOfStream);
                }
                NativeMediaVideoPoll::Pending => {}
            }
        }
        if self.worker.as_ref().is_some_and(JoinHandle::is_finished) {
            anyhow::bail!("macOS native media worker stopped");
        }
        Ok(DesktopVideoFramePoll::Pending)
    }

    fn seek_to(&mut self, position: Duration) -> anyhow::Result<Option<DesktopVideoFrame>> {
        let epoch = self
            .epoch
            .load(Ordering::Relaxed)
            .checked_add(1)
            .context("presentation epoch exhausted")?;
        self.epoch.store(epoch, Ordering::Release);
        self.ended = false;
        self.commands
            .try_send((epoch, position))
            .context("macOS native media seek mailbox is busy")?;
        self.unpark_worker();
        self.recv_frame()
    }

    fn buffered_frame_count(&self) -> usize {
        self.signals.output.len()
    }

    fn set_prefetch_limit(&self, limit: usize) {
        self.signals.limit.store(
            limit.clamp(1, self.signals.output.capacity()),
            Ordering::Relaxed,
        );
        self.unpark_worker();
    }
}

impl Drop for MacosNativeMediaSource {
    fn drop(&mut self) {
        self.epoch.store(u64::MAX, Ordering::Release);
        self.signals.cancelled.store(true, Ordering::Release);
        self.unpark_worker();
        if let Some(worker) = self.worker.take()
            && wait_for_macos_native_frame_worker_shutdown(&worker)
        {
            let _ = worker.join();
        }
    }
}

struct Presentation {
    lease: NativeVideoFrame,
    epoch: u64,
    current_epoch: Arc<AtomicU64>,
    presenter: Arc<Mutex<Option<MacosMetalLayerPresenter>>>,
}

impl std::fmt::Debug for Presentation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NativeMediaMetalPresentation")
            .field("epoch", &self.epoch)
            .finish_non_exhaustive()
    }
}

impl DesktopVideoFramePresentation for Presentation {
    fn present(self: Box<Self>) -> anyhow::Result<()> {
        if self.epoch != self.current_epoch.load(Ordering::Acquire) {
            return Ok(());
        }
        // Desktop runtime schedules presentation and seek on one owner thread.
        // Move the presenter out while encoding, so no mutex spans the GPU call.
        let mut presenter = self
            .presenter
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take()
            .context("Metal presenter is busy")?;
        let result = presenter.present_leased_cv_pixel_buffer(self.lease);
        *self.presenter.lock().unwrap_or_else(|e| e.into_inner()) = Some(presenter);
        result.map_err(|error| anyhow::anyhow!(error.message().to_owned()))
    }
}
