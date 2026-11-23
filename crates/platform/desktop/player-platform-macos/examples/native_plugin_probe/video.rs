use std::{
    cell::Cell,
    path::Path,
    time::{Duration, Instant},
};

use anyhow::{Context, Result, bail, ensure};
use player_platform_native_frame::*;
use player_plugin::*;
use player_plugin_loader::LoadedNativePlugin;

use crate::{metal::Metal, metrics};

const FRAME_WAIT: Duration = Duration::from_secs(5);
const DECODE_STEPS: usize = 512;
const SEEK_COUNT: usize = 50;
const GPU_SLOTS: u32 = 3;
const BENCH_FRAMES: usize = 192;
const WARMUP_FRAMES: usize = 24;

thread_local! {
    static DECODER_SEND: Cell<Duration> = const { Cell::new(Duration::ZERO) };
}

pub fn verify(plugins: &Path, libmetal: &Path, source: &Path) -> Result<()> {
    let (mut core, width, height) = open(plugins, source)?;
    // SAFETY: libmetal is the trusted Objective-C++ fixture compiled by the runner.
    let mut metal = unsafe { Metal::open(libmetal) }?;
    let started = Instant::now();
    // Fill the bounded pool behind a real GPU event. The signal is sent only
    // after the decoder/session is destroyed, independently of GPU speed.
    for lease_id in 1..=u64::from(GPU_SLOTS) {
        ensure!(
            submit_next(&mut core, &mut metal, lease_id, true)?,
            "unexpected pool exhaustion"
        );
    }
    ensure!(
        !submit_next(&mut core, &mut metal, 4, true)?,
        "full pool accepted another frame"
    );
    metal.retire();
    for _ in 0..SEEK_COUNT {
        core.seek(Duration::ZERO)?;
    }
    core.close()?;
    drop(core);
    let pending = metal.stats();
    ensure!(
        pending.retired == GPU_SLOTS && pending.released == 0 && pending.active == 0,
        "seek and close recycled an in-flight slot {pending:?}"
    );
    metal.signal();
    let final_stats = metal.wait_idle(FRAME_WAIT)?;
    ensure!(
        final_stats.submitted == GPU_SLOTS && final_stats.released == GPU_SLOTS,
        "lost or duplicate frame release {final_stats:?}"
    );

    let (mut reopened, _, _) = open(plugins, source)?;
    ensure!(
        submit_next(&mut reopened, &mut metal, 5, false)?,
        "retired slots never recycled"
    );
    reopened.close()?;
    let reopened_stats = metal.wait_idle(FRAME_WAIT)?;
    ensure!(
        reopened_stats.released == GPU_SLOTS + 1,
        "post-seek release missing"
    );
    println!(
        "metal_late_completion=pass width={width} height={height} seeks={SEEK_COUNT} retired_before_signal={} released_after_signal={} pool_reused=pass elapsed_ms={}",
        pending.retired,
        final_stats.released,
        started.elapsed().as_millis()
    );
    verify_resource(plugins, source, &metal)?;
    let mut reference_pts = None;
    for round in 1..=4 {
        let pts = benchmark(plugins, libmetal, source, round)?;
        ensure!(
            reference_pts.get_or_insert(pts) == &pts,
            "decoded frame PTS differed"
        );
    }
    Ok(())
}

fn benchmark(
    plugins: &Path,
    libmetal: &Path,
    source: &Path,
    round: u32,
) -> Result<[i64; BENCH_FRAMES]> {
    let (mut core, width, height) = open(plugins, source)?;
    // SAFETY: the runner compiles this trusted probe backend.
    let mut metal = unsafe { Metal::open(libmetal) }?;
    let mut elapsed = [Duration::ZERO; BENCH_FRAMES];
    let mut offscreen = [Duration::ZERO; BENCH_FRAMES];
    let mut advance = [Duration::ZERO; BENCH_FRAMES];
    let mut submit = [Duration::ZERO; BENCH_FRAMES];
    let mut decoder_send = [Duration::ZERO; BENCH_FRAMES];
    let mut allocations = 0;
    let mut pts = [0; BENCH_FRAMES];
    DECODER_SEND.set(Duration::ZERO);
    for index in 0..WARMUP_FRAMES + BENCH_FRAMES {
        let frame_started = Instant::now();
        // Account for CPU work between successive decoded frames. Waiting for
        // GPU completion stays outside this interval for both routes.
        let (frame, advance_time, advance_allocs) = metrics::measure(|| next_frame(&mut core));
        let (handle, frame) = frame?;
        let send_time = DECODER_SEND.replace(Duration::ZERO);
        let (accepted, submit_time, submit_allocs) = metrics::measure(|| {
            submit_frame(
                &mut core,
                &mut metal,
                handle,
                &frame,
                index as u64 + 1,
                false,
            )
        });
        ensure!(accepted?, "benchmark frame could not acquire a Metal slot");
        metal.wait_idle(FRAME_WAIT)?;
        let completion_time = frame_started.elapsed();
        if let Some(sample) = index.checked_sub(WARMUP_FRAMES) {
            pts[sample] = frame.presentation_time_us;
            elapsed[sample] = advance_time + submit_time;
            offscreen[sample] = completion_time;
            advance[sample] = advance_time;
            submit[sample] = submit_time;
            decoder_send[sample] = send_time;
            allocations += advance_allocs + submit_allocs;
        }
    }
    core.close()?;
    println!(
        "frame_benchmark mode=native_packet_decode_and_metal width={width} height={height} round={round} frames={BENCH_FRAMES} rust_host_allocs_per_frame={:.3} host_frame_p99_us={:.2} offscreen_frame_p99_us={:.2} decode_and_filter_p99_us={:.2} send_decoder_packet_p99_us={:.2} submit_and_release_p99_us={:.2}",
        allocations as f64 / BENCH_FRAMES as f64,
        metrics::p99_us(&mut elapsed),
        metrics::p99_us(&mut offscreen),
        metrics::p99_us(&mut advance),
        metrics::p99_us(&mut decoder_send),
        metrics::p99_us(&mut submit),
    );
    Ok(pts)
}

pub fn open(plugins: &Path, source: &Path) -> Result<(NativeFramePipelineCore, u32, u32)> {
    let config = crate::packets::config(source)?;
    let inner = crate::packets::open(plugins, &config)?;
    let info = inner.stream_info();
    let stream = info
        .selected_track_index
        .context("normalizer selected no track")?;
    let track = info
        .tracks
        .iter()
        .find(|t| {
            t.stream_index == stream && t.media_kind == SourceNormalizerPacketMediaKind::Video
        })
        .context("selected track is not video")?;
    let width = track.width.context("missing video width")?;
    let height = track.height.context("missing video height")?;
    ensure!(
        info.seekable && !track.extradata.is_empty() && track.bitstream_format.is_some(),
        "normalizer did not provide seekable decoder setup"
    );
    let decoder_config = DecoderSessionConfig {
        codec: track.codec.clone(),
        media_kind: DecoderMediaKind::Video,
        extradata: track.extradata.clone(),
        bitstream_format: track.bitstream_format.clone(),
        width: track.width,
        height: track.height,
        coded_width: track.coded_width,
        coded_height: track.coded_height,
        reorder_depth: track.reorder_depth,
        color: track.color.clone(),
        hdr: track.hdr.clone(),
        prefer_hardware: true,
        ..DecoderSessionConfig::default()
    };
    let plugin =
        LoadedNativePlugin::load_development(plugins.join("libvesper_decoder_videotoolbox.dylib"))?;
    let reference = PluginReference::new(plugin.plugin_id(), None, PluginTransport::Native)?;
    let decoder = plugin
        .resolve_native_decoder(&reference)?
        .open_native_session(&decoder_config)?;
    let mut core = NativeFramePipelineCore::with_components(
        NativeFramePipelineCoreConfig::default(),
        Some(Box::new(Packets {
            inner,
            stream: Some(stream),
        })),
        Some(Box::new(Decoder(decoder))),
        None,
        Some(Box::new(HostFrame)),
    );
    core.set_output_target_attached(true);
    Ok((core, width, height))
}

fn verify_resource(plugins: &Path, source: &Path, metal: &Metal) -> Result<()> {
    let root = source
        .parent()
        .context("fixture has no parent")?
        .join("normalized");
    let plugin = LoadedNativePlugin::load_development(
        plugins.join("libvesper_source_normalizer_ffmpeg.dylib"),
    )?;
    let reference = PluginReference::new(plugin.plugin_id(), None, PluginTransport::Native)?;
    let factory = plugin.resolve_source_resource(&reference)?;
    let mut session = factory.open_resource_session(&SourceNormalizerResourceSessionConfig {
        runtime_profile: "generic-fallback".into(),
        input: source
            .canonicalize()?
            .to_str()
            .context("fixture path is not UTF-8")?
            .into(),
        output_root: root.to_str().context("output root is not UTF-8")?.into(),
        headers: Vec::new(),
        cache_policy: SourceNormalizerResourceCachePolicy::default(),
        preferred_route: Some(SourceNormalizerOutputRoute::Fmp4LocalStream),
        startup_timeout_ms: Some(5_000),
        read_idle_timeout_ms: Some(5_000),
    })?;
    let result = (|| {
        let deadline = Instant::now() + FRAME_WAIT;
        loop {
            let status = session.poll()?;
            match status.state {
                SourceNormalizerResourceSessionState::Ready
                | SourceNormalizerResourceSessionState::Completed => {
                    let info = status.info.unwrap_or_else(|| session.session_info());
                    ensure!(
                        info.output_route == SourceNormalizerOutputRoute::Fmp4LocalStream,
                        "unexpected normalizer resource route"
                    );
                    let resource = info
                        .primary_resource_path
                        .context("normalizer emitted no resource path")?;
                    metal.check_avplayer_resource(Path::new(&resource))?;
                    return Ok(());
                }
                SourceNormalizerResourceSessionState::Failed
                | SourceNormalizerResourceSessionState::Cancelled => {
                    bail!(
                        "normalizer resource worker ended as {:?}: {:?}",
                        status.state,
                        status.message
                    );
                }
                _ => {}
            }
            ensure!(
                Instant::now() < deadline,
                "normalizer resource worker timed out"
            );
            session.wait_for_update(Duration::from_millis(25))?;
        }
    })();
    let closed = session.close();
    result?;
    closed?;
    println!("source_normalizer_packet_metadata=pass avplayer_resource_ready=pass");
    Ok(())
}

fn submit_next(
    core: &mut NativeFramePipelineCore,
    metal: &mut Metal,
    lease_id: u64,
    gated: bool,
) -> Result<bool> {
    let (handle, frame) = next_frame(core)?;
    submit_frame(core, metal, handle, &frame, lease_id, gated)
}

fn submit_frame(
    core: &mut NativeFramePipelineCore,
    metal: &mut Metal,
    handle: u64,
    frame: &NativeFramePipelineFrame,
    lease_id: u64,
    gated: bool,
) -> Result<bool> {
    // SAFETY: the decoder adapter admits only CVPixelBuffer frames.
    // The decoder lease remains live through submit's independent retain.
    let accepted = unsafe { metal.submit(frame, lease_id, gated) };
    core.release_frame(handle, false)?;
    accepted
}

fn next_frame(core: &mut NativeFramePipelineCore) -> Result<(u64, NativeFramePipelineFrame)> {
    let deadline = Instant::now() + FRAME_WAIT;
    for _ in 0..DECODE_STEPS {
        let result = core.advance()?;
        match result.status {
            NativeFramePipelineFrameStatus::Frame => {
                let handle = result.handle.context("missing host frame lease")?;
                let frame = result.frame.context("missing decoded frame")?;
                return Ok((handle, frame));
            }
            NativeFramePipelineFrameStatus::EndOfStream => {
                bail!("fixture ended before the requested frame")
            }
            NativeFramePipelineFrameStatus::Presented => {
                bail!("host frame was prematurely released")
            }
            NativeFramePipelineFrameStatus::Pending => {}
        }
        ensure!(
            Instant::now() < deadline,
            "decoder made no progress in time"
        );
    }
    bail!("decoder exceeded the packet budget for one frame")
}

type FrameResult<T> = Result<T, NativeFramePipelineError>;

struct Packets {
    inner: Box<dyn SourceNormalizerPacketSession>,
    stream: Option<u32>,
}

impl NativeFramePacketSourceAdapter for Packets {
    fn selected_video_stream_index(&self) -> Option<u32> {
        self.stream
    }

    fn read_packet(&mut self) -> FrameResult<NativeFramePacketRead> {
        let lease = self.inner.read_packet().map_err(boundary("readPacket"))?;
        let message = lease.metadata.message;
        match lease.metadata.status {
            SourceNormalizerReadPacketStatus::Packet => {
                let packet = lease.metadata.packet;
                let data = lease.data.to_vec();
                let handle = lease.handle;
                self.inner
                    .release_packet(handle)
                    .map_err(boundary("releasePacket"))?;
                let packet = packet.ok_or_else(|| {
                    NativeFramePipelineError::new("readPacket", "packet has no metadata")
                })?;
                Ok(NativeFramePacketRead::Packet {
                    packet,
                    data,
                    message,
                })
            }
            SourceNormalizerReadPacketStatus::NeedMoreData => {
                Ok(NativeFramePacketRead::NeedMoreData { message })
            }
            SourceNormalizerReadPacketStatus::EndOfStream => {
                Ok(NativeFramePacketRead::EndOfStream { message })
            }
        }
    }

    fn flush(&mut self) -> FrameResult<()> {
        self.inner
            .flush()
            .map(|_| ())
            .map_err(boundary("flushPackets"))
    }

    fn seek(&mut self, position: Duration) -> FrameResult<()> {
        let position_millis =
            u64::try_from(position.as_millis()).map_err(boundary("seekPackets"))?;
        self.inner
            .seek(&SourceNormalizerPacketSeek {
                position_millis,
                exact: false,
            })
            .map(|_| ())
            .map_err(boundary("seekPackets"))
    }

    fn close(&mut self) -> FrameResult<()> {
        self.inner.close().map_err(boundary("closePackets"))
    }
}

struct Decoder(Box<dyn NativeDecoderSession>);

impl NativeFrameDecoderAdapter for Decoder {
    fn send_packet(
        &mut self,
        packet: &DecoderPacket,
        data: &[u8],
    ) -> FrameResult<DecoderPacketResult> {
        let started = Instant::now();
        let result = self
            .0
            .send_packet(packet, data)
            .map_err(boundary("sendPacket"));
        DECODER_SEND.set(DECODER_SEND.get() + started.elapsed());
        result
    }

    fn receive_native_frame(&mut self) -> FrameResult<DecoderReceiveNativeFrameOutput> {
        let frame = self
            .0
            .receive_native_frame()
            .map_err(boundary("receiveFrame"))?;
        if let DecoderReceiveNativeFrameOutput::Frame(frame) = &frame
            && frame.metadata.handle_kind != DecoderNativeHandleKind::CvPixelBuffer
        {
            self.0
                .release_native_frame(frame.clone())
                .map_err(boundary("releaseFrame"))?;
            return Err(NativeFramePipelineError::new(
                "receiveFrame",
                "expected a CVPixelBuffer",
            ));
        }
        Ok(frame)
    }

    fn release_native_frame(
        &mut self,
        frame: DecoderNativeFrame,
        _presented: bool,
    ) -> FrameResult<()> {
        self.0
            .release_native_frame(frame)
            .map_err(boundary("releaseFrame"))
    }

    fn flush(&mut self) -> FrameResult<()> {
        self.0.flush().map_err(boundary("flushDecoder"))
    }

    fn close(&mut self) -> FrameResult<()> {
        self.0.close().map_err(boundary("closeDecoder"))
    }
}

struct HostFrame;

impl NativeFramePresenterAdapter for HostFrame {
    fn submit_frame(
        &mut self,
        _: &NativeFramePresenterFrame,
    ) -> FrameResult<NativeFramePresenterSubmitResult> {
        Ok(NativeFramePresenterSubmitResult {
            accepted: true,
            requires_host_release: true,
            message: None,
        })
    }

    fn flush(&mut self) -> FrameResult<()> {
        Ok(())
    }

    fn close(&mut self) -> FrameResult<()> {
        Ok(())
    }
}

fn boundary<E: std::fmt::Display>(
    operation: &'static str,
) -> impl FnOnce(E) -> NativeFramePipelineError {
    move |error| NativeFramePipelineError::new(operation, error.to_string())
}
