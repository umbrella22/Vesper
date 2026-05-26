use super::*;

pub(crate) fn open_macos_frame_processor_chain(
    stream_info: &VideoPacketStreamInfo,
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
        codec: stream_info.codec.clone(),
        pts_us: None,
        duration_us: None,
        width: stream_info.width.unwrap_or(0),
        height: stream_info.height.unwrap_or(0),
        coded_width: stream_info.width,
        coded_height: stream_info.height,
        visible_rect: None,
        handle_kind: NativeHandleKind::CvPixelBuffer,
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
        if !capabilities.supports_video_frames {
            anyhow::bail!(
                "frame processor `{}` does not support video frames",
                factory.name()
            );
        }
        if capabilities.may_change_dimensions {
            anyhow::bail!(
                "frame processor `{}` changes frame dimensions, which v1 does not allow",
                factory.name()
            );
        }
        let session = factory
            .open_session(&FrameProcessorSessionConfig {
                processor_index,
                input_metadata: input_metadata.clone(),
                max_in_flight_frames: Some(policy.max_in_flight_frames_per_processor),
            })
            .map_err(|error| anyhow::anyhow!(error.to_string()))?;
        processors.push(MacosFrameProcessorNode {
            plugin_name: factory.name().to_owned(),
            processor_index,
            session,
        });
    }
    if processors.is_empty() {
        return Ok(None);
    }
    Ok(Some(MacosFrameProcessorChain {
        processors,
        mode,
        policy,
        metrics: PlayerFrameProcessingMetrics::default(),
        pending_events: VecDeque::new(),
        debug: FrameProcessorDebugState::from_env(),
    }))
}

pub(crate) fn process_macos_native_frame(
    shared: &mut MacosNativeFrameDecoderState,
    frame: DecoderNativeFrame,
) -> Result<MacosFrameProcessorFrame, (anyhow::Error, DecoderNativeFrame)> {
    let Some(chain) = shared.frame_processor_chain.as_mut() else {
        return Ok(MacosFrameProcessorFrame {
            decoder_frame: frame.clone(),
            presentation_frame: frame,
            processor_outputs: Vec::new(),
        });
    };
    chain.process(frame)
}

pub(crate) fn decoder_frame_to_native_frame(frame: &DecoderNativeFrame) -> NativeFrame {
    NativeFrame {
        metadata: frame.metadata.clone().into(),
        handle: frame.handle,
    }
}

pub(crate) fn native_frame_to_decoder_frame(frame: &NativeFrame) -> DecoderNativeFrame {
    DecoderNativeFrame {
        metadata: frame.metadata.clone().into(),
        handle: frame.handle,
    }
}

pub(crate) fn output_frame_requires_processor_release(frame: &NativeFrame) -> bool {
    frame
        .metadata
        .release_tracking
        .as_ref()
        .is_none_or(|tracking| tracking.requires_release)
}

impl MacosFrameProcessorChain {
    pub(crate) fn process(
        &mut self,
        decoder_frame: DecoderNativeFrame,
    ) -> Result<MacosFrameProcessorFrame, (anyhow::Error, DecoderNativeFrame)> {
        let mut state = self.begin_process_state(&decoder_frame);
        for node_index in 0..self.processors.len() {
            self.process_node(node_index, &decoder_frame, &mut state)?;
        }

        Ok(self.finish_process_state(decoder_frame, state))
    }

    pub(crate) fn begin_process_state(
        &mut self,
        decoder_frame: &DecoderNativeFrame,
    ) -> MacosFrameProcessorProcessState {
        let current_frame = decoder_frame_to_native_frame(decoder_frame);
        let mut debug_sample = self.debug.begin_frame(current_frame.metadata.pts_us);
        debug_sample.node_count = self.processors.len();
        MacosFrameProcessorProcessState {
            current_frame,
            processor_outputs: Vec::new(),
            using_processor_output: false,
            debug_sample,
        }
    }

    pub(crate) fn process_node(
        &mut self,
        node_index: usize,
        decoder_frame: &DecoderNativeFrame,
        state: &mut MacosFrameProcessorProcessState,
    ) -> Result<(), (anyhow::Error, DecoderNativeFrame)> {
        let submit_result = match self.submit_to_node(node_index, &state.current_frame) {
            Ok(result) => result,
            Err(error) => {
                self.release_processor_outputs(std::mem::take(&mut state.processor_outputs));
                return Err((error, decoder_frame.clone()));
            }
        };

        if self.handle_submit_status(node_index, submit_result, decoder_frame, state)? {
            return Ok(());
        }

        let receive_output = match self.receive_from_node(node_index) {
            Ok(output) => output,
            Err(error) => {
                self.release_processor_outputs(std::mem::take(&mut state.processor_outputs));
                return Err((error, decoder_frame.clone()));
            }
        };
        self.handle_receive_output(node_index, receive_output, decoder_frame, state)
    }

    pub(crate) fn submit_to_node(
        &mut self,
        node_index: usize,
        current_frame: &NativeFrame,
    ) -> anyhow::Result<FrameProcessorSubmitResult> {
        let submit = FrameProcessorSubmitFrame {
            metadata: current_frame.metadata.clone(),
            present_deadline_us: current_frame
                .metadata
                .pts_us
                .map(|pts| pts.saturating_add(duration_us_i64(self.policy.frame_deadline))),
        };
        self.metrics.submitted_frame_count = self.metrics.submitted_frame_count.saturating_add(1);
        let node = &mut self.processors[node_index];
        node.session
            .submit_frame(current_frame, &submit)
            .map_err(|error| {
                frame_processor_runtime_error(
                    self.mode,
                    node.processor_index,
                    &node.plugin_name,
                    error,
                )
            })
    }

    pub(crate) fn handle_submit_status(
        &mut self,
        node_index: usize,
        submit_result: FrameProcessorSubmitResult,
        decoder_frame: &DecoderNativeFrame,
        state: &mut MacosFrameProcessorProcessState,
    ) -> Result<bool, (anyhow::Error, DecoderNativeFrame)> {
        self.debug
            .observe_submit(submit_result.queue_depth, submit_result.in_flight_frames);
        match submit_result.status {
            FrameProcessorSubmitStatus::Accepted => {
                state.debug_sample.submitted_nodes =
                    state.debug_sample.submitted_nodes.saturating_add(1);
                Ok(false)
            }
            FrameProcessorSubmitStatus::Bypassed | FrameProcessorSubmitStatus::Backpressure => {
                self.handle_submit_bypass(node_index, submit_result, decoder_frame, state)?;
                Ok(true)
            }
            FrameProcessorSubmitStatus::Rejected => {
                self.handle_submit_rejected(node_index, submit_result, decoder_frame, state)?;
                Ok(true)
            }
        }
    }

    pub(crate) fn handle_submit_bypass(
        &mut self,
        node_index: usize,
        submit_result: FrameProcessorSubmitResult,
        decoder_frame: &DecoderNativeFrame,
        state: &mut MacosFrameProcessorProcessState,
    ) -> Result<(), (anyhow::Error, DecoderNativeFrame)> {
        self.reset_to_decoder_frame(decoder_frame, state);
        self.metrics.bypassed_frame_count = self.metrics.bypassed_frame_count.saturating_add(1);
        self.debug.observe_bypass();
        state.debug_sample.bypassed = true;
        if submit_result.status == FrameProcessorSubmitStatus::Backpressure {
            self.metrics.backpressure_count = self.metrics.backpressure_count.saturating_add(1);
            self.debug.observe_backpressure();
        }
        let node_snapshot = self.node_snapshot(node_index);
        let warning_kind = if submit_result.status == FrameProcessorSubmitStatus::Backpressure {
            FrameProcessorWarningKind::Backpressure
        } else {
            FrameProcessorWarningKind::BypassActivated
        };
        self.push_warning(
            warning_kind,
            &node_snapshot,
            &state.current_frame,
            FrameProcessorWarningDetails {
                queue_depth: submit_result.queue_depth,
                in_flight_frames: submit_result.in_flight_frames,
                ..FrameProcessorWarningDetails::default()
            },
            FrameProcessorPolicyAction::BypassOriginalFrame,
            submit_result.message,
        );
        if self.mode == FrameProcessorMode::RequireProcessed {
            return Err((
                anyhow::anyhow!(
                    "frame processor `{}` bypassed a frame in strict mode",
                    node_snapshot.plugin_name
                ),
                decoder_frame.clone(),
            ));
        }
        Ok(())
    }

    pub(crate) fn handle_submit_rejected(
        &mut self,
        node_index: usize,
        submit_result: FrameProcessorSubmitResult,
        decoder_frame: &DecoderNativeFrame,
        state: &mut MacosFrameProcessorProcessState,
    ) -> Result<(), (anyhow::Error, DecoderNativeFrame)> {
        self.reset_to_decoder_frame(decoder_frame, state);
        self.debug.observe_bypass();
        state.debug_sample.bypassed = true;
        let node_snapshot = self.node_snapshot(node_index);
        self.push_warning(
            FrameProcessorWarningKind::Unsupported,
            &node_snapshot,
            &state.current_frame,
            FrameProcessorWarningDetails {
                queue_depth: submit_result.queue_depth,
                in_flight_frames: submit_result.in_flight_frames,
                ..FrameProcessorWarningDetails::default()
            },
            if self.mode == FrameProcessorMode::RequireProcessed {
                FrameProcessorPolicyAction::FailPlayback
            } else {
                FrameProcessorPolicyAction::BypassOriginalFrame
            },
            submit_result.message,
        );
        if self.mode == FrameProcessorMode::RequireProcessed {
            return Err((
                anyhow::anyhow!(
                    "frame processor `{}` rejected a frame in strict mode",
                    node_snapshot.plugin_name
                ),
                decoder_frame.clone(),
            ));
        }
        Ok(())
    }

    pub(crate) fn receive_from_node(
        &mut self,
        node_index: usize,
    ) -> anyhow::Result<FrameProcessorReceiveOutput> {
        let node = &mut self.processors[node_index];
        node.session.receive_frame().map_err(|error| {
            frame_processor_runtime_error(self.mode, node.processor_index, &node.plugin_name, error)
        })
    }

    pub(crate) fn handle_receive_output(
        &mut self,
        node_index: usize,
        receive_output: FrameProcessorReceiveOutput,
        decoder_frame: &DecoderNativeFrame,
        state: &mut MacosFrameProcessorProcessState,
    ) -> Result<(), (anyhow::Error, DecoderNativeFrame)> {
        match receive_output {
            FrameProcessorReceiveOutput::Frame(output) => {
                self.handle_ready_output(node_index, output, decoder_frame, state)
            }
            FrameProcessorReceiveOutput::Pending | FrameProcessorReceiveOutput::EndOfStream => {
                self.handle_pending_output(node_index, decoder_frame, state)
            }
        }
    }

    pub(crate) fn handle_ready_output(
        &mut self,
        node_index: usize,
        output: FrameProcessorOutputFrame,
        decoder_frame: &DecoderNativeFrame,
        state: &mut MacosFrameProcessorProcessState,
    ) -> Result<(), (anyhow::Error, DecoderNativeFrame)> {
        state.debug_sample.processed_nodes = state.debug_sample.processed_nodes.saturating_add(1);
        let node_snapshot = self.node_snapshot(node_index);
        let timing_decision =
            self.record_output_timing(&node_snapshot, &state.current_frame, &output);
        state.debug_sample.deadline_missed |= timing_decision.deadline_missed;
        state.debug_sample.dropped_output |= timing_decision.should_drop_output;
        if timing_decision.should_drop_output || timing_decision.should_fail_playback {
            self.release_processor_outputs(vec![ProcessorOwnedNativeFrame {
                processor_index: node_snapshot.processor_index,
                frame: output.frame.clone(),
            }]);
        }
        if timing_decision.should_fail_playback && self.mode == FrameProcessorMode::RequireProcessed
        {
            self.release_processor_outputs(std::mem::take(&mut state.processor_outputs));
            return Err((
                anyhow::anyhow!(
                    "frame processor `{}` missed frame deadline in strict mode",
                    node_snapshot.plugin_name
                ),
                decoder_frame.clone(),
            ));
        }
        if timing_decision.should_drop_output {
            self.reset_to_decoder_frame(decoder_frame, state);
            return Ok(());
        }
        self.accept_processor_output(output.frame, &node_snapshot, decoder_frame, state);
        Ok(())
    }

    pub(crate) fn accept_processor_output(
        &mut self,
        output_frame: NativeFrame,
        node_snapshot: &MacosFrameProcessorNodeSnapshot,
        decoder_frame: &DecoderNativeFrame,
        state: &mut MacosFrameProcessorProcessState,
    ) {
        if output_frame_requires_processor_release(&output_frame) {
            state.processor_outputs.push(ProcessorOwnedNativeFrame {
                processor_index: node_snapshot.processor_index,
                frame: output_frame.clone(),
            });
        }
        state.current_frame = output_frame;
        if self.mode == FrameProcessorMode::DiagnosticsOnly {
            state.current_frame = decoder_frame_to_native_frame(decoder_frame);
            state.using_processor_output = false;
        } else {
            state.using_processor_output = true;
        }
    }

    pub(crate) fn handle_pending_output(
        &mut self,
        node_index: usize,
        decoder_frame: &DecoderNativeFrame,
        state: &mut MacosFrameProcessorProcessState,
    ) -> Result<(), (anyhow::Error, DecoderNativeFrame)> {
        self.reset_to_decoder_frame(decoder_frame, state);
        self.metrics.bypassed_frame_count = self.metrics.bypassed_frame_count.saturating_add(1);
        self.debug.observe_bypass();
        self.debug.observe_pending();
        state.debug_sample.bypassed = true;
        state.debug_sample.pending = true;
        let node_snapshot = self.node_snapshot(node_index);
        self.push_warning(
            FrameProcessorWarningKind::BypassActivated,
            &node_snapshot,
            &state.current_frame,
            FrameProcessorWarningDetails::default(),
            FrameProcessorPolicyAction::BypassOriginalFrame,
            Some("processor did not return a ready frame".to_owned()),
        );
        if self.mode == FrameProcessorMode::RequireProcessed {
            return Err((
                anyhow::anyhow!(
                    "frame processor `{}` did not return a ready frame in strict mode",
                    node_snapshot.plugin_name
                ),
                decoder_frame.clone(),
            ));
        }
        Ok(())
    }

    pub(crate) fn reset_to_decoder_frame(
        &mut self,
        decoder_frame: &DecoderNativeFrame,
        state: &mut MacosFrameProcessorProcessState,
    ) {
        self.release_processor_outputs(std::mem::take(&mut state.processor_outputs));
        state.current_frame = decoder_frame_to_native_frame(decoder_frame);
        state.using_processor_output = false;
    }

    pub(crate) fn finish_process_state(
        &mut self,
        decoder_frame: DecoderNativeFrame,
        mut state: MacosFrameProcessorProcessState,
    ) -> MacosFrameProcessorFrame {
        let presentation_frame = if self.mode == FrameProcessorMode::PreferProcessed
            || self.mode == FrameProcessorMode::RequireProcessed
        {
            native_frame_to_decoder_frame(&state.current_frame)
        } else {
            decoder_frame.clone()
        };
        state.debug_sample.output_pts_us = presentation_frame.metadata.pts_us;
        state.debug_sample.presented_processed = state.using_processor_output;
        self.debug.finish_frame(state.debug_sample);
        MacosFrameProcessorFrame {
            decoder_frame,
            presentation_frame,
            processor_outputs: state.processor_outputs,
        }
    }

    pub(crate) fn record_output_timing(
        &mut self,
        node: &MacosFrameProcessorNodeSnapshot,
        input: &NativeFrame,
        output: &FrameProcessorOutputFrame,
    ) -> MacosFrameProcessorTimingDecision {
        self.metrics.processed_frame_count = self.metrics.processed_frame_count.saturating_add(1);
        self.metrics.last_queue_wait_us = output.timings.queue_wait_us;
        self.metrics.last_process_time_us = output.timings.process_time_us;
        self.metrics.last_submit_to_ready_us = output.timings.submit_to_ready_us;
        let mut decision = MacosFrameProcessorTimingDecision::default();
        if output
            .timings
            .submit_to_ready_us
            .is_some_and(|elapsed| elapsed > self.policy.frame_deadline.as_micros() as u64)
        {
            self.metrics.deadline_miss_count = self.metrics.deadline_miss_count.saturating_add(1);
            self.debug.observe_deadline_miss();
            decision.deadline_missed = true;
            let action = if self.mode == FrameProcessorMode::RequireProcessed {
                FrameProcessorPolicyAction::FailPlayback
            } else {
                FrameProcessorPolicyAction::BypassOriginalFrame
            };
            self.push_warning(
                FrameProcessorWarningKind::DeadlineMissed,
                node,
                input,
                FrameProcessorWarningDetails::from_output_timing(
                    output,
                    self.policy.frame_deadline,
                ),
                action,
                Some("processor output missed frame deadline".to_owned()),
            );
            if self.mode == FrameProcessorMode::RequireProcessed {
                decision.should_fail_playback = true;
            }
        }
        if output.timings.submit_to_ready_us.is_some_and(|elapsed| {
            elapsed
                > (self.policy.frame_deadline + self.policy.late_output_tolerance).as_micros()
                    as u64
        }) {
            decision.should_drop_output = true;
            self.metrics.dropped_output_count = self.metrics.dropped_output_count.saturating_add(1);
            self.metrics.late_output_drop_count =
                self.metrics.late_output_drop_count.saturating_add(1);
            self.debug.observe_dropped_output();
            self.push_warning(
                FrameProcessorWarningKind::LateOutputDropped,
                node,
                input,
                FrameProcessorWarningDetails::from_output_timing(
                    output,
                    self.policy.frame_deadline,
                ),
                FrameProcessorPolicyAction::DropOutput,
                Some("processor output was later than tolerance".to_owned()),
            );
        }
        decision
    }

    pub(crate) fn release_processor_outputs(
        &mut self,
        mut outputs: Vec<ProcessorOwnedNativeFrame>,
    ) {
        while let Some(output) = outputs.pop() {
            if let Some(node) = self
                .processors
                .iter_mut()
                .find(|node| node.processor_index == output.processor_index)
            {
                let _ = node.session.release_frame(output.frame);
            }
        }
    }

    pub(crate) fn drain_events(&mut self) -> Vec<PlayerRuntimeEvent> {
        self.pending_events.drain(..).collect()
    }

    pub(crate) fn flush(&mut self) {
        for node in &mut self.processors {
            let _ = node.session.flush();
        }
    }

    pub(crate) fn push_warning(
        &mut self,
        kind: FrameProcessorWarningKind,
        node: &MacosFrameProcessorNodeSnapshot,
        input: &NativeFrame,
        details: FrameProcessorWarningDetails,
        policy_action: FrameProcessorPolicyAction,
        message: Option<String>,
    ) {
        self.pending_events.push_back(PlayerRuntimeEvent::Warning(
            PlayerRuntimeWarning::FrameProcessor(FrameProcessorWarning {
                kind,
                plugin_name: node.plugin_name.clone(),
                processor_index: node.processor_index,
                frame_id: input.metadata.frame_id,
                frame_pts_us: input.metadata.pts_us,
                frame_duration_us: input.metadata.duration_us,
                input_handle_kind: Some(format!("{:?}", input.metadata.handle_kind)),
                output_handle_kind: details.output_handle_kind,
                queue_depth: details.queue_depth,
                in_flight_frames: details.in_flight_frames,
                queue_wait_us: details.queue_wait_us.or(self.metrics.last_queue_wait_us),
                process_time_us: details
                    .process_time_us
                    .or(self.metrics.last_process_time_us),
                submit_to_ready_us: details
                    .submit_to_ready_us
                    .or(self.metrics.last_submit_to_ready_us),
                present_deadline_us: input
                    .metadata
                    .pts_us
                    .map(|pts| pts.saturating_add(duration_us_i64(self.policy.frame_deadline))),
                deadline_overrun_us: details.deadline_overrun_us,
                consecutive_miss_count: None,
                policy_action,
                message,
            }),
        ));
    }

    pub(crate) fn node_snapshot(&self, node_index: usize) -> MacosFrameProcessorNodeSnapshot {
        let node = &self.processors[node_index];
        MacosFrameProcessorNodeSnapshot {
            plugin_name: node.plugin_name.clone(),
            processor_index: node.processor_index,
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct MacosFrameProcessorNodeSnapshot {
    pub(crate) plugin_name: String,
    pub(crate) processor_index: usize,
}

#[derive(Debug, Default)]
pub(crate) struct FrameProcessorWarningDetails {
    pub(crate) output_handle_kind: Option<String>,
    pub(crate) queue_depth: Option<u32>,
    pub(crate) in_flight_frames: Option<u32>,
    pub(crate) queue_wait_us: Option<u64>,
    pub(crate) process_time_us: Option<u64>,
    pub(crate) submit_to_ready_us: Option<u64>,
    pub(crate) deadline_overrun_us: Option<u64>,
}

impl FrameProcessorWarningDetails {
    pub(crate) fn from_output_timing(
        output: &FrameProcessorOutputFrame,
        deadline: Duration,
    ) -> Self {
        let deadline_us = deadline.as_micros() as u64;
        Self {
            output_handle_kind: Some(format!("{:?}", output.frame.metadata.handle_kind)),
            queue_wait_us: output.timings.queue_wait_us,
            process_time_us: output.timings.process_time_us,
            submit_to_ready_us: output.timings.submit_to_ready_us,
            deadline_overrun_us: output
                .timings
                .submit_to_ready_us
                .and_then(|elapsed| elapsed.checked_sub(deadline_us)),
            ..Self::default()
        }
    }
}

#[derive(Debug, Default)]
pub(crate) struct MacosFrameProcessorTimingDecision {
    pub(crate) should_drop_output: bool,
    pub(crate) should_fail_playback: bool,
    pub(crate) deadline_missed: bool,
}

pub(crate) fn frame_processor_runtime_error(
    mode: FrameProcessorMode,
    processor_index: usize,
    plugin_name: &str,
    error: FrameProcessorError,
) -> anyhow::Error {
    if mode == FrameProcessorMode::RequireProcessed {
        anyhow::anyhow!(
            "frame processor `{plugin_name}` at index {processor_index} failed in strict mode: {error}"
        )
    } else {
        anyhow::anyhow!(
            "frame processor `{plugin_name}` at index {processor_index} failed: {error}"
        )
    }
}

pub(crate) fn duration_us_i64(duration: Duration) -> i64 {
    i64::try_from(duration.as_micros()).unwrap_or(i64::MAX)
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
