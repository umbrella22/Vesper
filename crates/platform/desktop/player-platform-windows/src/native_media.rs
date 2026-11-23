use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::{Duration, Instant};

use anyhow::Context;
use player_backend_ffmpeg::{MediaProbe, VideoPacketSource};
use player_platform_desktop::{
    DesktopVideoFrame, DesktopVideoFramePoll, DesktopVideoSource, DesktopVideoSourceBootstrap,
};
use player_platform_native_frame::{
    MediaPacket, MediaPacketPoll, MediaPacketSource, NativeFramePipelineError, NativeMediaPipeline,
    NativeMediaVideoPoll,
};
use player_plugin::normalize_decoder_codec_identifier;
use player_plugin_abi::{
    VESPER_GPU_D3D11, VESPER_HANDLE_D3D11_TEXTURE2D, VESPER_PACKET_DISCONTINUITY,
    VESPER_PACKET_KEYFRAME, VesperD3D11Context, VesperGpuContext, VesperGpuContextData,
    VesperVideoFormat,
};
use player_plugin_loader::native::{NativePlugin, NativeVideoFrame};
use windows::Win32::Graphics::Direct3D11::{
    D3D11_ASYNC_GETDATA_DONOTFLUSH, D3D11_QUERY_DESC, D3D11_QUERY_EVENT, ID3D11Device,
    ID3D11DeviceContext, ID3D11Multithread, ID3D11Query,
};
use windows::core::{BOOL, Interface};

use super::{
    BackendVideoDecodeInfo, BackendVideoDecoderMode, WindowsNativeFramePresenter,
    WindowsSurfaceAttachTarget, d3d11_filter::ColorFilter, duration_from_micros,
    vulkan_presenter::VulkanPresenter, wait_for_windows_native_frame,
    windows_d3d11_presenter::WindowsD3D11NativeFramePresenter, windows_decoder_bitstream_format,
};

const PRODUCER_WAIT: Duration = Duration::from_millis(250);
const FRAME_SLOTS: usize = 3;
const RETIREMENT_WORKERS: usize = 8;
static WORKERS_IN_USE: AtomicUsize = AtomicUsize::new(0);

pub(super) fn open(
    packets: VideoPacketSource,
    probe: MediaProbe,
    plugin: NativePlugin,
    surface: WindowsSurfaceAttachTarget,
) -> anyhow::Result<DesktopVideoSourceBootstrap> {
    let stream = packets.stream_info();
    let codec = match normalize_decoder_codec_identifier(&stream.codec).as_str() {
        "h264" | "avc" | "avc1" | "avc3" => *b"avc1",
        "hevc" | "h265" | "hvc1" | "hev1" => *b"hvc1",
        codec => anyhow::bail!("unsupported Windows native video codec {codec}"),
    };
    let mut presenter = WindowsD3D11NativeFramePresenter::new()?;
    let vulkan = match VulkanPresenter::try_open(presenter.device(), surface) {
        Ok(vulkan) => vulkan,
        Err(error) => {
            tracing::info!(%error, "Windows Vulkan unavailable, selecting D3D11");
            None
        }
    };
    let options = serde_json::to_vec(&serde_json::json!({
        "width": stream.width,
        "height": stream.height,
        "codedWidth": stream.width,
        "codedHeight": stream.height,
        "reorderDepth": stream.reorder_depth,
        "bitstreamFormat": windows_decoder_bitstream_format(&stream.codec),
        "shareTextures": vulkan.is_some(),
    }))?;
    let color_filter = if vulkan.is_none() {
        presenter.attach(surface)?;
        ColorFilter::from_host_selection(presenter.device())?
    } else {
        None
    };
    let completion = CompletionQueue::new(presenter.device(), presenter.immediate_context())?;
    let gpu = VesperGpuContext {
        struct_size: size_of::<VesperGpuContext>() as u32,
        backend: VESPER_GPU_D3D11,
        data: VesperGpuContextData {
            d3d11: VesperD3D11Context {
                device: presenter.device().as_raw(),
                immediate_context: presenter.immediate_context().as_raw(),
            },
        },
    };
    // SAFETY: the admitted decoder receives the same device as the swapchain.
    // The pipeline field is dropped before the presenter and completion queue.
    let decoder = unsafe {
        plugin.open_video_decoder_with_device(
            &gpu,
            u32::from_be_bytes(codec),
            &stream.extradata,
            &options,
            Duration::from_millis(300),
        )
    }?;
    Ok(DesktopVideoSourceBootstrap {
        source: Box::new(WindowsDecoderSource {
            pipeline: NativeMediaPipeline::new(WindowsPackets(packets), decoder),
            presenter,
            completion,
            color_filter,
            vulkan,
            producer: None,
            surface,
        }),
        decode_info: BackendVideoDecodeInfo {
            selected_mode: BackendVideoDecoderMode::Hardware,
            hardware_available: true,
            hardware_backend: Some("d3d11".to_owned()),
            decoder_name: plugin.name().to_owned(),
            fallback_reason: None,
        },
        probe,
    })
}

struct WindowsPackets(VideoPacketSource);

impl MediaPacketSource for WindowsPackets {
    type Storage = Vec<u8>;

    fn next_video_packet(
        &mut self,
    ) -> Result<MediaPacketPoll<Self::Storage>, NativeFramePipelineError> {
        let Some(packet) = self.0.next_packet().map_err(packet_error)? else {
            return Ok(MediaPacketPoll::EndOfStream);
        };
        let mut flags = 0;
        if packet.key_frame {
            flags |= VESPER_PACKET_KEYFRAME;
        }
        if packet.discontinuity {
            flags |= VESPER_PACKET_DISCONTINUITY;
        }
        Ok(MediaPacketPoll::Packet(MediaPacket {
            pts_us: packet.pts_us,
            dts_us: packet.dts_us,
            duration_us: packet.duration_us,
            flags,
            stream_index: packet.stream_index,
            bytes: packet.data,
        }))
    }

    fn seek(&mut self, position: Duration) -> Result<(), NativeFramePipelineError> {
        self.0.seek_to(position).map_err(packet_error)
    }
}

fn packet_error(error: anyhow::Error) -> NativeFramePipelineError {
    NativeFramePipelineError::new("windowsVideoPackets", error.to_string())
}

struct WindowsDecoderSource {
    pipeline: NativeMediaPipeline<WindowsPackets>,
    presenter: WindowsD3D11NativeFramePresenter,
    completion: CompletionQueue,
    color_filter: Option<ColorFilter>,
    vulkan: Option<VulkanPresenter>,
    producer: Option<ProducerFrame>,
    surface: WindowsSurfaceAttachTarget,
}

struct ProducerFrame {
    query: ID3D11Query,
    lease: NativeVideoFrame,
    format: VesperVideoFormat,
    deadline: Instant,
}

impl WindowsDecoderSource {
    fn fallback_to_d3d11(&mut self, error: &anyhow::Error) -> anyhow::Result<()> {
        if let Some(mut vulkan) = self.vulkan.take() {
            if let Err(drain_error) = vulkan.drain() {
                tracing::warn!(%drain_error, "Retiring the Vulkan presenter during D3D11 fallback");
            }
            drop(vulkan);
        }
        tracing::info!(%error, "Windows Vulkan texture presentation unavailable, selecting D3D11");
        self.presenter.attach(self.surface)?;
        self.color_filter = ColorFilter::from_host_selection(self.presenter.device())?;
        Ok(())
    }

    fn present_d3d11(
        &mut self,
        query: ID3D11Query,
        lease: NativeVideoFrame,
        format: VesperVideoFormat,
        handle: usize,
        slice: u32,
    ) -> anyhow::Result<DesktopVideoFramePoll> {
        let pts = lease.descriptor().pts_us;
        let result = (|| -> anyhow::Result<()> {
            if let Some(filter) = self.color_filter.as_mut() {
                // SAFETY: the decoded lease retains this texture and its device
                // through the completion query on the same immediate context.
                let output = unsafe {
                    filter.encode(self.presenter.immediate_context(), handle, slice, format)
                }?;
                self.presenter.present_filtered_texture(&output)?;
            } else {
                self.presenter.present(handle, slice)?;
            }
            Ok(())
        })();
        // A failed Present can follow a successful GPU copy or video blit.
        self.completion.end_query(&query);
        self.completion.retire(query, lease, result.is_ok());
        result?;
        Ok(presented_frame(pts, format))
    }

    fn poll_vulkan_producer(
        &mut self,
        producer: ProducerFrame,
    ) -> anyhow::Result<DesktopVideoFramePoll> {
        let ProducerFrame {
            query,
            lease,
            format,
            deadline,
        } = producer;
        let done = query_completed(self.presenter.immediate_context(), &query);
        if !matches!(done, Ok(true)) {
            if matches!(done, Ok(false)) && Instant::now() < deadline {
                self.producer = Some(ProducerFrame {
                    query,
                    lease,
                    format,
                    deadline,
                });
                return Ok(DesktopVideoFramePoll::Pending);
            }
            self.completion.retire(query, lease, false);
            let error = match done {
                Err(error) => error.into(),
                _ => anyhow::anyhow!("D3D11 producer completion timed out"),
            };
            self.fallback_to_d3d11(&error)?;
            return Ok(DesktopVideoFramePoll::Pending);
        }
        let frame = *lease.descriptor();
        let handle = usize::try_from(frame.handles[0])?;
        let slice = u32::try_from(frame.handles[1])?;
        let vulkan = self.vulkan.as_mut().context("missing Vulkan presenter")?;
        let image = match vulkan.import(self.presenter.device(), handle, slice, format) {
            Ok(image) => image,
            Err(error) => {
                self.fallback_to_d3d11(&error)?;
                return self.present_d3d11(query, lease, format, handle, slice);
            }
        };
        self.completion.available.push(query);
        let first = !vulkan.submitted();
        match vulkan.present(lease, format, image) {
            Ok(true) => {
                if first {
                    tracing::info!(
                        "Windows same-adapter Vulkan texture import and presentation active"
                    );
                }
                Ok(presented_frame(frame.pts_us, format))
            }
            Ok(false) => Ok(DesktopVideoFramePoll::Pending),
            Err(error) => {
                self.fallback_to_d3d11(&error)?;
                Ok(DesktopVideoFramePoll::Pending)
            }
        }
    }

    fn retire_producer(&mut self) {
        if let Some(producer) = self.producer.take() {
            self.completion
                .retire(producer.query, producer.lease, false);
        }
    }
}

impl Drop for WindowsDecoderSource {
    fn drop(&mut self) {
        self.retire_producer();
    }
}

fn presented_frame(pts: i64, format: VesperVideoFormat) -> DesktopVideoFramePoll {
    DesktopVideoFramePoll::Ready(DesktopVideoFrame::native_presented(
        duration_from_micros(pts).unwrap_or_default(),
        format.width,
        format.height,
    ))
}

impl DesktopVideoSource for WindowsDecoderSource {
    fn recv_frame(&mut self) -> anyhow::Result<Option<DesktopVideoFrame>> {
        wait_for_windows_native_frame(Duration::from_secs(2), || self.try_recv_frame())
    }

    fn try_recv_frame(&mut self) -> anyhow::Result<DesktopVideoFramePoll> {
        if let Some(producer) = self.producer.take() {
            return self.poll_vulkan_producer(producer);
        }
        self.completion.reclaim_completed();
        if self.completion.available.is_empty() {
            return Ok(DesktopVideoFramePoll::Pending);
        }
        match self.pipeline.advance()? {
            NativeMediaVideoPoll::Pending => Ok(DesktopVideoFramePoll::Pending),
            NativeMediaVideoPoll::EndOfStream => Ok(DesktopVideoFramePoll::EndOfStream),
            NativeMediaVideoPoll::Frame { lease, format } => {
                let frame = *lease.descriptor();
                anyhow::ensure!(
                    frame.handle_kind == VESPER_HANDLE_D3D11_TEXTURE2D && frame.plane_count == 1,
                    "windows D3D11 native-frame presenter requires one D3D11 texture"
                );
                let handle = usize::try_from(frame.handles[0])?;
                let slice = u32::try_from(frame.handles[1])?;
                let query = self
                    .completion
                    .available
                    .pop()
                    .context("missing D3D11 frame slot")?;
                if self.vulkan.is_some() {
                    self.completion.end_query(&query);
                    let producer = ProducerFrame {
                        query,
                        lease,
                        format,
                        deadline: Instant::now() + PRODUCER_WAIT,
                    };
                    return self.poll_vulkan_producer(producer);
                }
                self.present_d3d11(query, lease, format, handle, slice)
            }
        }
    }

    fn seek_to(&mut self, position: Duration) -> anyhow::Result<Option<DesktopVideoFrame>> {
        self.retire_producer();
        if let Some(vulkan) = self.vulkan.as_mut() {
            vulkan.begin_epoch()?;
        }
        self.pipeline.seek(position)?;
        self.recv_frame()
    }

    fn buffered_frame_count(&self) -> usize {
        0
    }

    fn set_prefetch_limit(&self, _limit: usize) {}
}

struct WorkerPermit;

impl WorkerPermit {
    fn acquire() -> anyhow::Result<Self> {
        WORKERS_IN_USE
            .try_update(Ordering::AcqRel, Ordering::Acquire, |count| {
                (count < RETIREMENT_WORKERS).then_some(count + 1)
            })
            .map_err(|_| anyhow::anyhow!("Windows D3D11 retirement capacity is exhausted"))?;
        Ok(Self)
    }
}

impl Drop for WorkerPermit {
    fn drop(&mut self) {
        WORKERS_IN_USE.fetch_sub(1, Ordering::Release);
    }
}

struct Submission {
    query: ID3D11Query,
    lease: NativeVideoFrame,
    rendered: bool,
}

struct CompletionQueue {
    context: ID3D11DeviceContext,
    available: Vec<ID3D11Query>,
    submit: Sender<Submission>,
    completed: Receiver<ID3D11Query>,
}

impl CompletionQueue {
    fn new(device: &ID3D11Device, context: &ID3D11DeviceContext) -> anyhow::Result<Self> {
        let permit = WorkerPermit::acquire()?;
        let protection: ID3D11Multithread = context
            .cast()
            .context("D3D11 context multithread protection")?;
        // SAFETY: this is the live immediate context. Enable API serialization
        // before handing clones to the completion worker or the decoder.
        let _ = unsafe { protection.SetMultithreadProtected(true) };
        let mut available = Vec::with_capacity(FRAME_SLOTS);
        let desc = D3D11_QUERY_DESC {
            Query: D3D11_QUERY_EVENT,
            MiscFlags: 0,
        };
        for _ in 0..FRAME_SLOTS {
            let mut query = None;
            // SAFETY: the query descriptor and output slot are valid for this device.
            unsafe { device.CreateQuery(&desc, Some(&mut query)) }?;
            available.push(query.context("D3D11 returned an empty event query")?);
        }
        // Each in-flight message carries one pooled query. Both channels combined
        // can hold at most FRAME_SLOTS messages, even after the source closes.
        let (submit, pending) = mpsc::channel::<Submission>();
        let (complete, completed) = mpsc::channel();
        let worker_device = device.clone();
        let worker_context = context.clone();
        std::thread::Builder::new()
            .name("vesper-d3d11-retirement".to_owned())
            .spawn(move || {
                let _permit = permit;
                while let Ok(submission) = pending.recv() {
                    let gpu_completed =
                        wait_for_query(&worker_device, &worker_context, &submission.query);
                    submission
                        .lease
                        .complete(submission.rendered && gpu_completed);
                    let _ = complete.send(submission.query);
                }
            })
            .context("start D3D11 completion worker")?;
        Ok(Self {
            context: context.clone(),
            available,
            submit,
            completed,
        })
    }

    fn reclaim_completed(&mut self) {
        self.available
            .extend(self.completed.try_iter().take(FRAME_SLOTS));
    }

    fn end_query(&self, query: &ID3D11Query) {
        // SAFETY: the query and the protected immediate context share one device.
        // End orders the event behind all previously submitted reads of the texture.
        unsafe {
            self.context.End(query);
            self.context.Flush();
        }
    }

    fn retire(&self, query: ID3D11Query, lease: NativeVideoFrame, rendered: bool) {
        if let Err(error) = self.submit.send(Submission {
            query,
            lease,
            rendered,
        }) {
            // The receiver lives until this sender is dropped. If the worker
            // exits unexpectedly, keep this sample alive rather than recycle it.
            std::mem::forget(error.0.lease);
        }
    }
}

fn query_completed(
    context: &ID3D11DeviceContext,
    query: &ID3D11Query,
) -> windows::core::Result<bool> {
    let mut done = BOOL(0);
    // SAFETY: the query is ended, the context is multithread protected, and
    // D3D11_QUERY_EVENT writes a BOOL into the sized output slot.
    unsafe {
        context.GetData(
            query,
            Some((&mut done as *mut BOOL).cast()),
            size_of::<BOOL>() as u32,
            D3D11_ASYNC_GETDATA_DONOTFLUSH.0 as u32,
        )?;
    }
    Ok(done.as_bool())
}

fn wait_for_query(
    device: &ID3D11Device,
    context: &ID3D11DeviceContext,
    query: &ID3D11Query,
) -> bool {
    loop {
        if matches!(query_completed(context, query), Ok(true)) {
            return true;
        }
        // SAFETY: the worker retains the live producer device.
        if unsafe { device.GetDeviceRemovedReason() }.is_err() {
            return false;
        }
        // A stalled device retains a bounded set of samples off the host thread
        // until the query completes or D3D11 reports device removal.
        std::thread::sleep(Duration::from_millis(2));
    }
}
