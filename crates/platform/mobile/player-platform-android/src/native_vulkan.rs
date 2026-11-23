use std::ffi::c_void;
use std::io::Cursor;
use std::time::Duration;

use player_platform_mobile::MobileSourceNormalizerPacketOpen;
use player_platform_native_frame::{
    NativeFramePipelineStatusSnapshot, NativeMediaPipeline, NativeMediaVideoPoll,
    NormalizedVideoPackets,
};
use player_plugin_abi::{VESPER_HANDLE_AHB, VESPER_PIXEL_NV12, VesperVideoFormat};
use player_plugin_loader::native::{NativePlugin, NativeVideoFrame};
use player_render_vulkan::{AndroidVulkanPresenter, VulkanFrameLease};

use super::{
    AndroidNativeFramePipelineCounters, AndroidNativeFramePipelineFrame,
    AndroidNativeFramePipelineFrameResult, AndroidNativeFramePipelineFrameStatus,
    AndroidNativeFramePipelineOpenConfig, NativeFramePipelineLifecycleState, PlayerError,
    PlayerErrorCode, PlayerResult, PluginRegistry,
    hdr_programmable_processing_not_supported_reason, normalize_decoder_codec_identifier,
    pipeline_error_to_player_error, required_android_decoder_implementation_name,
    selected_video_track,
};

const MEDIACODEC_PLUGIN: &str = "io.github.umbrella22.vesper.decoder-mediacodec";
const DECODER_CALL_BUDGET: Duration = Duration::from_millis(700);
const SHADER: &[u8] = include_bytes!(
    "../../../../../plugins/video-filter-reference/prebuilt/android-combined-420.spv"
);

struct Lease(NativeVideoFrame);

impl VulkanFrameLease for Lease {
    fn complete(self, accepted: bool) {
        self.0.complete(accepted);
    }
}

struct TimedFrame {
    id: u64,
    lease: NativeVideoFrame,
    format: VesperVideoFormat,
}

pub(super) struct AndroidVulkanVideo {
    pipeline: NativeMediaPipeline<NormalizedVideoPackets>,
    presenter: Option<(usize, AndroidVulkanPresenter<Lease>)>,
    pending: Option<TimedFrame>,
    counters: AndroidNativeFramePipelineCounters,
    next_frame: u64,
    epoch: u64,
    seek_floor_us: i64,
    ended: bool,
    presenting: bool,
}

impl AndroidVulkanVideo {
    pub(super) fn select(
        config: &AndroidNativeFramePipelineOpenConfig,
        packet: MobileSourceNormalizerPacketOpen,
        select_decoder: impl FnOnce(&str, u32, u32) -> PlayerResult<Option<String>>,
    ) -> PlayerResult<Result<Self, MobileSourceNormalizerPacketOpen>> {
        let Some(plugin) = selected_decoder(config)? else {
            return Ok(Err(packet));
        };
        let stream = packet.session;
        if config
            .native_frame_pipeline
            .has_configured_frame_processors()
        {
            return Err(unsupported(
                "Android Vulkan uses the bundled Slang VideoFilter and cannot run legacy FrameProcessor plugins",
            ));
        }
        let track = selected_video_track(stream.stream_info())
            .ok_or_else(|| unsupported("Android Vulkan requires a video track"))?;
        if let Some(reason) = hdr_programmable_processing_not_supported_reason(track) {
            return Err(unsupported(reason));
        }
        player_render_vulkan::probe_android_vulkan_support().map_err(unsupported)?;
        let hardware_decoder_name =
            required_android_decoder_implementation_name(track, select_decoder)?;
        let fourcc = match normalize_decoder_codec_identifier(&track.codec).as_str() {
            "h264" | "avc" | "avc1" => *b"avc1",
            "avc3" => *b"avc3",
            "hevc" | "h265" | "hvc1" => *b"hvc1",
            "hev1" => *b"hev1",
            _ => {
                return Err(unsupported(
                    "the video codec is outside the Android Vulkan profile",
                ));
            }
        };
        let stream_index = track.stream_index;
        let params = serde_json::to_vec(&serde_json::json!({
            "width": track.width,
            "height": track.height,
            "hardwareDecoderName": hardware_decoder_name,
            "pixelFormat": VESPER_PIXEL_NV12,
        }))
        .map_err(unsupported)?;
        let decoder = plugin
            .open_video_decoder(
                u32::from_be_bytes(fourcc),
                &track.extradata,
                &params,
                DECODER_CALL_BUDGET,
            )
            .map_err(unsupported)?;
        Ok(Ok(Self {
            pipeline: NativeMediaPipeline::new(
                NormalizedVideoPackets::new(stream, stream_index),
                decoder,
            ),
            presenter: None,
            pending: None,
            counters: AndroidNativeFramePipelineCounters::default(),
            next_frame: 1,
            epoch: 0,
            seek_floor_us: i64::MIN,
            ended: false,
            presenting: false,
        }))
    }

    pub(super) unsafe fn attach(&mut self, window: usize) -> PlayerResult<()> {
        if self
            .presenter
            .as_ref()
            .is_some_and(|(bound, _)| *bound == window)
        {
            return Ok(());
        }
        self.detach();
        let words = player_render_vulkan::read_shader_words(&mut Cursor::new(SHADER))
            .map_err(unsupported)?;
        // SAFETY: the host supplies the live clear playback Surface; the backend retains it.
        let mut presenter = unsafe { AndroidVulkanPresenter::open(window as *mut c_void, words) }
            .map_err(unsupported)?;
        if self.epoch > 0 {
            presenter.begin_epoch(self.epoch).map_err(unsupported)?;
        }
        self.presenter = Some((window, presenter));
        self.counters.presenter_attach_count += 1;
        Ok(())
    }

    pub(super) fn detach(&mut self) {
        self.pending = None;
        self.presenting = false;
        if self.presenter.take().is_some() {
            self.counters.presenter_detach_count += 1;
        }
    }

    pub(super) fn snapshot(&self) -> NativeFramePipelineStatusSnapshot {
        let attached = self.presenter.is_some();
        let lifecycle_state = if self.presenting {
            NativeFramePipelineLifecycleState::Presenting
        } else if attached {
            NativeFramePipelineLifecycleState::WarmingUp
        } else {
            NativeFramePipelineLifecycleState::WaitingForOutputTarget
        };
        NativeFramePipelineStatusSnapshot {
            lifecycle_state,
            output_target_attached: attached,
            presenter_configured: attached,
            decoder_configured: true,
            pending_frames: usize::from(self.pending.is_some()),
            pending_packet: false,
            end_of_stream: self.ended,
            epoch: self.epoch,
            generation: self.epoch,
            counters: self.counters.clone(),
        }
    }

    pub(super) fn advance(&mut self) -> PlayerResult<AndroidNativeFramePipelineFrameResult> {
        use AndroidNativeFramePipelineFrameStatus as Status;
        let mut result = AndroidNativeFramePipelineFrameResult {
            status: Status::Pending,
            handle: None,
            frame: None,
            requires_host_release: false,
            message: None,
        };
        let Some((_, presenter)) = self.presenter.as_mut() else {
            return Ok(result);
        };
        if self.ended {
            result.status = Status::EndOfStream;
            return Ok(result);
        }
        if !presenter.prepare().map_err(unsupported)? || self.pending.is_some() {
            return Ok(result);
        }
        match self
            .pipeline
            .advance()
            .map_err(pipeline_error_to_player_error)?
        {
            NativeMediaVideoPoll::Pending => {}
            NativeMediaVideoPoll::EndOfStream => {
                self.ended = true;
                result.status = Status::EndOfStream;
            }
            NativeMediaVideoPoll::Frame { lease, format } => {
                self.counters.decoded_frames += 1;
                let raw = lease.descriptor();
                if raw.handle_kind != VESPER_HANDLE_AHB || raw.sync_kind != 0 || raw.handles[0] == 0
                {
                    return Err(unsupported(
                        "MediaCodec did not supply a ready AHardwareBuffer",
                    ));
                }
                if format.pixel_format != VESPER_PIXEL_NV12
                    || matches!(format.color.transfer, 16 | 18)
                {
                    return Err(unsupported(
                        "the Android Vulkan Surface profile requires SDR NV12",
                    ));
                }
                if raw.pts_us < self.seek_floor_us {
                    self.counters.late_dropped += 1;
                    return Ok(result);
                }
                let id = self.next_frame;
                self.next_frame = id
                    .checked_add(1)
                    .ok_or_else(|| unsupported("Android Vulkan frame identifiers exhausted"))?;
                result.status = Status::Frame;
                result.handle = Some(id);
                result.requires_host_release = true;
                result.frame = Some(AndroidNativeFramePipelineFrame {
                    handle: 0,
                    presentation_time_us: raw.pts_us,
                    duration_us: (raw.duration_us != i64::MIN).then_some(raw.duration_us),
                    width: format.width,
                    height: format.height,
                    frame_id: Some(id),
                });
                self.pending = Some(TimedFrame { id, lease, format });
            }
        }
        Ok(result)
    }

    pub(super) fn release(&mut self, id: u64, present: bool) -> PlayerResult<()> {
        let pending = self
            .pending
            .take_if(|frame| frame.id == id)
            .ok_or_else(|| {
                PlayerError::new(
                    PlayerErrorCode::InvalidArgument,
                    "stale Android Vulkan frame handle",
                )
            })?;
        self.counters.released_frames += 1;
        if !present {
            return Ok(());
        }
        let Some((_, presenter)) = &mut self.presenter else {
            return Err(PlayerError::new(
                PlayerErrorCode::InvalidState,
                "Android Vulkan Surface is detached",
            ));
        };
        let buffer = pending.lease.descriptor().handles[0] as *const c_void;
        self.counters.presenter_submit_count += 1;
        // SAFETY: advance accepted only a live clear AHardwareBuffer with no pending producer fence.
        if unsafe {
            presenter.present(
                self.epoch,
                Lease(pending.lease),
                buffer,
                pending.format,
                1.0,
            )
        }
        .map_err(unsupported)?
        {
            self.presenting = true;
            self.counters.processed_frames += 1;
            self.counters.presented_frames += 1;
        } else {
            self.counters.presenter_backpressure_count += 1;
        }
        Ok(())
    }

    fn invalidate(&mut self) -> PlayerResult<()> {
        self.pending = None;
        self.ended = false;
        self.presenting = false;
        self.epoch = self
            .epoch
            .checked_add(1)
            .ok_or_else(|| unsupported("Android Vulkan epochs exhausted"))?;
        if let Some((_, presenter)) = &mut self.presenter {
            presenter.begin_epoch(self.epoch).map_err(unsupported)?;
        }
        Ok(())
    }

    pub(super) fn seek(&mut self, position: Duration) -> PlayerResult<()> {
        self.invalidate()?;
        self.pipeline
            .seek(position)
            .map_err(pipeline_error_to_player_error)?;
        self.seek_floor_us = i64::try_from(position.as_micros()).unwrap_or(i64::MAX);
        self.counters.seek_count += 1;
        Ok(())
    }

    pub(super) fn flush(&mut self) -> PlayerResult<()> {
        self.invalidate()?;
        self.pipeline
            .flush()
            .map_err(pipeline_error_to_player_error)?;
        self.counters.flush_count += 1;
        Ok(())
    }
}

fn selected_decoder(
    config: &AndroidNativeFramePipelineOpenConfig,
) -> PlayerResult<Option<NativePlugin>> {
    let Some(artifact) = config
        .native_frame_pipeline
        .decoder_plugin_artifacts
        .iter()
        .find(|artifact| artifact.reference.plugin_id() == MEDIACODEC_PLUGIN)
    else {
        return Ok(None);
    };
    let loader = artifact.loader_artifact().map_err(unsupported)?;
    let registry = PluginRegistry::load_native_artifacts([loader.clone()]).map_err(unsupported)?;
    let resolved = registry
        .resolve_native_decoder(&artifact.reference)
        .map_err(unsupported)?;
    let Some(plugin) = loader.native_plugin() else {
        return Ok(None);
    };
    let factory = resolved.capability();
    let requirements = factory.native_requirements();
    if !factory.capabilities().supports_hardware_decode
        || !requirements
            .output_handle_kinds
            .contains(&player_plugin::DecoderNativeHandleKind::MediaCodecHardwareBuffer)
        || !requirements
            .output_pipeline_profiles
            .contains(&player_plugin::NativeFramePipelineProfile::MediaCodecHardwareBuffer)
        || requirements.requires_native_device_context
    {
        return Err(unsupported(
            "the admitted decoder does not declare a host-independent MediaCodec hardware buffer",
        ));
    }
    Ok(Some(plugin))
}

fn unsupported(error: impl std::fmt::Display) -> PlayerError {
    PlayerError::new(PlayerErrorCode::Unsupported, error.to_string())
}
