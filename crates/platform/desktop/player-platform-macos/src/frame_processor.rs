use super::*;

pub(crate) fn open_macos_frame_processor_chain(
    stream_info: &MacosNativeFrameStreamInfo,
    paths: &[PathBuf],
    mode: FrameProcessorMode,
    policy: FrameProcessorPolicy,
) -> anyhow::Result<Option<MacosFrameProcessorChain>> {
    if mode == FrameProcessorMode::Disabled || paths.is_empty() {
        return Ok(None);
    }
    let input_metadata = NativeFrameMetadata {
        media_kind: DecoderMediaKind::Video,
        format: player_plugin::DecoderFrameFormat::Nv12,
        codec: stream_info.packet.codec.clone(),
        pts_us: None,
        duration_us: None,
        width: stream_info.packet.width.unwrap_or(0),
        height: stream_info.packet.height.unwrap_or(0),
        coded_width: stream_info.packet.width,
        coded_height: stream_info.packet.height,
        visible_rect: None,
        handle_kind: NativeHandleKind::CvPixelBuffer,
        pipeline_profile: Some(
            player_plugin::NativeFramePipelineProfile::VideoToolboxCvPixelBuffer,
        ),
        color_space: stream_info
            .color
            .as_ref()
            .and_then(|color| color.primaries.clone()),
        hdr_metadata: stream_info.hdr.as_ref().map(|hdr| hdr.kind.clone()),
        color: stream_info.color.clone(),
        hdr: stream_info.hdr.clone(),
        sync_info: None,
        transform: None,
        frame_id: None,
        release_tracking: None,
    };
    let mut processors = Vec::new();
    for (processor_index, path) in paths.iter().enumerate().take(policy.max_chain_depth) {
        let plugin = LoadedDynamicPlugin::load(path)
            .with_context(|| format!("failed to load frame processor plugin {}", path.display()))?;
        let factory = plugin.frame_processor_plugin_factory().ok_or_else(|| {
            anyhow::anyhow!(
                "plugin `{}` does not export a frame processor API",
                plugin.plugin_name()
            )
        })?;
        let capabilities = factory.capabilities();
        let requirements =
            player_plugin::FrameProcessorSessionRequirements::native_video(input_metadata.clone());
        let missing_capabilities = requirements.missing_capabilities(&capabilities);
        if !missing_capabilities.is_empty() {
            anyhow::bail!(
                "frame processor `{}` does not satisfy session requirements for {:?} input handles with {:?} pipeline profile: missing {}",
                factory.name(),
                input_metadata.handle_kind,
                input_metadata.effective_pipeline_profile(),
                missing_capabilities.join(", ")
            );
        }
        let session = factory
            .open_session(&FrameProcessorSessionConfig {
                processor_index,
                input_metadata: input_metadata.clone(),
                max_in_flight_frames: Some(policy.max_in_flight_frames_per_processor),
            })
            .map_err(|error| anyhow::anyhow!(error.to_string()))?;
        processors.push(NativeFrameProcessorNode::new(
            factory.name(),
            processor_index,
            session,
        ));
    }
    if processors.is_empty() {
        return Ok(None);
    }
    Ok(Some(MacosFrameProcessorChain {
        core: NativeFrameProcessorChainCore::new(processors, mode, policy),
        debug: FrameProcessorDebugState::from_env(),
    }))
}

pub(crate) fn process_macos_native_frame(
    shared: &mut MacosNativeFrameDecoderState,
    frame: DecoderNativeFrame,
) -> Result<MacosFrameProcessorFrame, (anyhow::Error, DecoderNativeFrame)> {
    let Some(chain) = shared.frame_processor_chain.as_mut() else {
        return Ok(MacosFrameProcessorFrame(
            NativeFrameProcessorProcessedFrame {
                decoder_frame: frame.clone(),
                presentation_frame: frame,
                processor_outputs: Vec::new(),
            },
        ));
    };
    chain.process(frame)
}

impl MacosFrameProcessorChain {
    pub(crate) fn process(
        &mut self,
        decoder_frame: DecoderNativeFrame,
    ) -> Result<MacosFrameProcessorFrame, (anyhow::Error, DecoderNativeFrame)> {
        let mut observer = MacosFrameProcessorDebugObserver::new(&mut self.debug);
        self.core
            .process(decoder_frame, &mut observer)
            .map(MacosFrameProcessorFrame)
            .map_err(|error| {
                (
                    macos_frame_processor_error(error.error),
                    error.decoder_frame,
                )
            })
    }

    pub(crate) fn release_processor_outputs(&mut self, outputs: Vec<ProcessorOwnedNativeFrame>) {
        let _ = self.core.release_processor_outputs(outputs);
    }

    pub(crate) fn drain_events(&mut self) -> Vec<PlayerRuntimeEvent> {
        self.core.drain_events()
    }

    pub(crate) fn flush(&mut self) {
        let _ = self.core.flush();
    }

    pub(crate) fn close(&mut self) {
        let _ = self.core.close();
    }

    #[cfg(test)]
    pub(crate) fn metrics(&self) -> &PlayerFrameProcessingMetrics {
        self.core.metrics()
    }
}

fn macos_frame_processor_error(error: NativeFrameProcessorError) -> anyhow::Error {
    anyhow::anyhow!(error.to_string())
}

struct MacosFrameProcessorDebugObserver<'a> {
    debug: &'a mut FrameProcessorDebugState,
    sample: FrameProcessorFrameDebugSample,
}

impl<'a> MacosFrameProcessorDebugObserver<'a> {
    fn new(debug: &'a mut FrameProcessorDebugState) -> Self {
        Self {
            debug,
            sample: FrameProcessorFrameDebugSample::default(),
        }
    }
}

impl NativeFrameProcessorObserver for MacosFrameProcessorDebugObserver<'_> {
    fn begin_frame(&mut self, pts_us: Option<i64>, node_count: usize) {
        self.sample = self.debug.begin_frame(pts_us);
        self.sample.node_count = node_count;
    }

    fn observe_submit(&mut self, queue_depth: Option<u32>, in_flight_frames: Option<u32>) {
        self.debug.observe_submit(queue_depth, in_flight_frames);
    }

    fn observe_submitted_node(&mut self) {
        self.sample.submitted_nodes = self.sample.submitted_nodes.saturating_add(1);
    }

    fn observe_processed_node(&mut self) {
        self.sample.processed_nodes = self.sample.processed_nodes.saturating_add(1);
    }

    fn observe_bypass(&mut self) {
        self.debug.observe_bypass();
        self.sample.bypassed = true;
    }

    fn observe_backpressure(&mut self) {
        self.debug.observe_backpressure();
    }

    fn observe_pending(&mut self) {
        self.debug.observe_pending();
        self.sample.pending = true;
    }

    fn observe_timing(&mut self, deadline_missed: bool, dropped_output: bool) {
        if deadline_missed {
            self.debug.observe_deadline_miss();
            self.sample.deadline_missed = true;
        }
        if dropped_output {
            self.debug.observe_dropped_output();
            self.sample.dropped_output = true;
        }
    }

    fn finish_frame(&mut self, output_pts_us: Option<i64>, presented_processed: bool) {
        self.sample.output_pts_us = output_pts_us;
        self.sample.presented_processed = presented_processed;
        self.debug.finish_frame(std::mem::take(&mut self.sample));
    }
}

pub(crate) fn env_flag(name: &str) -> bool {
    std::env::var(name)
        .ok()
        .map(|value| {
            matches!(
                value.to_ascii_lowercase().as_str(),
                "1" | "true" | "yes" | "on"
            )
        })
        .unwrap_or(false)
}

pub(crate) fn env_u64(name: &str) -> Option<u64> {
    std::env::var(name)
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
}

pub(crate) fn max_option_u32(current: Option<u32>, next: Option<u32>) -> Option<u32> {
    current.max(next)
}
