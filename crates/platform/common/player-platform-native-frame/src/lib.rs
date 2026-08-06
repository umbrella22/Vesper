#![deny(unsafe_code)]

use std::collections::{HashMap, VecDeque};
use std::time::Duration;

use player_plugin::{
    DecoderNativeDeviceContext, DecoderNativeFrame, DecoderPacket, DecoderPacketResult,
    DecoderReceiveNativeFrameOutput, FrameProcessorError, FrameProcessorFrameTimings,
    FrameProcessorInputFrame, FrameProcessorOutputFrame, FrameProcessorReceiveOutput,
    FrameProcessorSession, FrameProcessorSubmitFrame, FrameProcessorSubmitResult,
    FrameProcessorSubmitStatus, NativeFrame, NativeFrameReleaseTracking, SourceNormalizerPacket,
    SourceNormalizerPacketMediaKind,
};
use player_runtime::{
    FrameProcessorMode, FrameProcessorPolicy, FrameProcessorPolicyAction, FrameProcessorWarning,
    FrameProcessorWarningKind, PlayerFrameProcessingMetrics, PlayerRuntimeEvent,
    PlayerRuntimeWarning, PluginBreakerState, PluginBudgetPolicy,
};

/// Maximum number of pending runtime events the frame processor chain buffers
/// before discarding oldest events. Hosts must call `drain_events()` regularly
/// to consume events and stay within this bound.
const MAX_PENDING_EVENTS: usize = 256;

fn plugin_budget_from_frame_processor_policy(policy: &FrameProcessorPolicy) -> PluginBudgetPolicy {
    PluginBudgetPolicy {
        max_queue_depth: Some(policy.max_in_flight_frames_per_processor),
        max_in_flight_frames: Some(policy.max_in_flight_frames_per_processor),
        max_process_time_us: Some(
            u64::try_from(policy.frame_deadline.as_micros()).unwrap_or(u64::MAX),
        ),
        max_consecutive_failures: None,
    }
}

#[derive(Debug)]
pub struct NativeFrameProcessorChainCore {
    processors: Vec<NativeFrameProcessorNode>,
    mode: FrameProcessorMode,
    policy: FrameProcessorPolicy,
    metrics: PlayerFrameProcessingMetrics,
    pending_events: VecDeque<PlayerRuntimeEvent>,
    closed: bool,
}

impl NativeFrameProcessorChainCore {
    pub fn new(
        processors: Vec<NativeFrameProcessorNode>,
        mode: FrameProcessorMode,
        policy: FrameProcessorPolicy,
    ) -> Self {
        let budget = plugin_budget_from_frame_processor_policy(&policy);
        let processors = processors
            .into_iter()
            .map(|node| node.with_budget(budget))
            .collect();
        Self {
            processors,
            mode,
            policy,
            metrics: PlayerFrameProcessingMetrics::default(),
            pending_events: VecDeque::new(),
            closed: false,
        }
    }

    pub fn mode(&self) -> FrameProcessorMode {
        self.mode
    }

    pub fn policy(&self) -> &FrameProcessorPolicy {
        &self.policy
    }

    pub fn metrics(&self) -> &PlayerFrameProcessingMetrics {
        &self.metrics
    }

    pub fn metrics_mut(&mut self) -> &mut PlayerFrameProcessingMetrics {
        &mut self.metrics
    }

    pub fn drain_events(&mut self) -> Vec<PlayerRuntimeEvent> {
        self.pending_events.drain(..).collect()
    }

    pub fn process(
        &mut self,
        decoder_frame: DecoderNativeFrame,
        observer: &mut impl NativeFrameProcessorObserver,
    ) -> Result<NativeFrameProcessorProcessedFrame, NativeFrameProcessorProcessError> {
        let mut state = NativeFrameProcessorProcessState {
            current_frame: decoder_frame_to_native_frame(&decoder_frame),
            processor_outputs: Vec::new(),
            using_processor_output: false,
        };
        observer.begin_frame(decoder_frame.metadata.pts_us, self.processors.len());
        for node_index in 0..self.processors.len() {
            if let Err(error) = self.process_node(node_index, &decoder_frame, &mut state, observer)
            {
                let _ = self.release_processor_outputs_best_effort(state.processor_outputs);
                return Err(error);
            }
        }
        let presentation_frame = if matches!(
            self.mode,
            FrameProcessorMode::PreferProcessed | FrameProcessorMode::RequireProcessed
        ) {
            native_frame_to_decoder_frame(&state.current_frame)
        } else {
            decoder_frame.clone()
        };
        observer.finish_frame(
            presentation_frame.metadata.pts_us,
            state.using_processor_output,
        );
        Ok(NativeFrameProcessorProcessedFrame {
            decoder_frame,
            presentation_frame,
            processor_outputs: state.processor_outputs,
        })
    }

    fn process_node(
        &mut self,
        node_index: usize,
        decoder_frame: &DecoderNativeFrame,
        state: &mut NativeFrameProcessorProcessState,
        observer: &mut impl NativeFrameProcessorObserver,
    ) -> Result<(), NativeFrameProcessorProcessError> {
        if self.processors[node_index].breaker.is_disabled() {
            return self.handle_disabled_node(node_index, decoder_frame, state, observer);
        }

        let submit_result = match self.submit_to_node(node_index, &state.current_frame) {
            Ok(result) => result,
            Err(error) => {
                return self.handle_processor_failure(
                    node_index,
                    error,
                    decoder_frame,
                    state,
                    observer,
                );
            }
        };

        observer.observe_submit(submit_result.queue_depth, submit_result.in_flight_frames);
        let load_action = self.observe_node_load(
            node_index,
            submit_result.queue_depth,
            submit_result.in_flight_frames,
        );
        match submit_result.status {
            FrameProcessorSubmitStatus::Accepted => observer.observe_submitted_node(),
            FrameProcessorSubmitStatus::Bypassed | FrameProcessorSubmitStatus::Backpressure => {
                self.handle_submit_bypass(
                    node_index,
                    submit_result,
                    decoder_frame,
                    state,
                    observer,
                )?;
                return Ok(());
            }
            FrameProcessorSubmitStatus::Rejected => {
                self.handle_submit_rejected(
                    node_index,
                    submit_result,
                    decoder_frame,
                    state,
                    observer,
                )?;
                return Ok(());
            }
        }

        let receive = match self.receive_from_node(node_index) {
            Ok(output) => output,
            Err(error) => {
                return self.handle_processor_failure(
                    node_index,
                    error,
                    decoder_frame,
                    state,
                    observer,
                );
            }
        };
        match receive {
            FrameProcessorReceiveOutput::Frame(output) => {
                if load_action == FrameProcessorPolicyAction::DropOutput {
                    self.handle_over_budget_output(
                        node_index,
                        output,
                        submit_result.queue_depth,
                        submit_result.in_flight_frames,
                        decoder_frame,
                        state,
                        observer,
                    )
                } else {
                    self.handle_ready_output(node_index, output, decoder_frame, state, observer)
                }
            }
            FrameProcessorReceiveOutput::Pending | FrameProcessorReceiveOutput::EndOfStream => {
                self.handle_pending_output(node_index, decoder_frame, state, observer)
            }
        }
    }

    fn handle_over_budget_output(
        &mut self,
        node_index: usize,
        output: FrameProcessorOutputFrame,
        queue_depth: Option<u32>,
        in_flight_frames: Option<u32>,
        decoder_frame: &DecoderNativeFrame,
        state: &mut NativeFrameProcessorProcessState,
        observer: &mut impl NativeFrameProcessorObserver,
    ) -> Result<(), NativeFrameProcessorProcessError> {
        self.handle_ready_output(node_index, output, decoder_frame, state, observer)?;
        self.reset_to_decoder_frame(decoder_frame, state);
        self.metrics.bypassed_frame_count = self.metrics.bypassed_frame_count.saturating_add(1);
        self.metrics.backpressure_count = self.metrics.backpressure_count.saturating_add(1);
        self.metrics.dropped_output_count = self.metrics.dropped_output_count.saturating_add(1);
        observer.observe_bypass();
        observer.observe_backpressure();
        observer.observe_timing(false, true);
        let node_snapshot = self.node_snapshot(node_index);
        self.push_warning(
            FrameProcessorWarningKind::OutputDropped,
            &node_snapshot,
            &state.current_frame,
            FrameProcessorWarningDetails {
                queue_depth,
                in_flight_frames,
                ..FrameProcessorWarningDetails::default()
            },
            FrameProcessorPolicyAction::DropOutput,
            Some("processor output dropped because queue or in-flight load exceeded the configured budget".to_owned()),
        );
        if self.mode == FrameProcessorMode::RequireProcessed {
            return Err(NativeFrameProcessorProcessError {
                error: NativeFrameProcessorError::strict(
                    node_snapshot.processor_index,
                    &node_snapshot.plugin_name,
                    "exceeded the configured queue or in-flight budget",
                ),
                decoder_frame: decoder_frame.clone(),
            });
        }
        Ok(())
    }

    fn handle_disabled_node(
        &mut self,
        node_index: usize,
        decoder_frame: &DecoderNativeFrame,
        state: &mut NativeFrameProcessorProcessState,
        observer: &mut impl NativeFrameProcessorObserver,
    ) -> Result<(), NativeFrameProcessorProcessError> {
        self.reset_to_decoder_frame(decoder_frame, state);
        self.metrics.bypassed_frame_count = self.metrics.bypassed_frame_count.saturating_add(1);
        observer.observe_bypass();
        let node_snapshot = self.node_snapshot(node_index);
        if self.mode == FrameProcessorMode::RequireProcessed {
            return Err(NativeFrameProcessorProcessError {
                error: NativeFrameProcessorError::strict(
                    node_snapshot.processor_index,
                    &node_snapshot.plugin_name,
                    "is disabled by policy",
                ),
                decoder_frame: decoder_frame.clone(),
            });
        }
        Ok(())
    }

    fn handle_processor_failure(
        &mut self,
        node_index: usize,
        error: NativeFrameProcessorError,
        decoder_frame: &DecoderNativeFrame,
        state: &mut NativeFrameProcessorProcessState,
        observer: &mut impl NativeFrameProcessorObserver,
    ) -> Result<(), NativeFrameProcessorProcessError> {
        self.reset_to_decoder_frame(decoder_frame, state);
        self.metrics.bypassed_frame_count = self.metrics.bypassed_frame_count.saturating_add(1);
        observer.observe_bypass();

        let breaker_action = self.processors[node_index].breaker.record_failure();
        if breaker_action == FrameProcessorPolicyAction::DisableProcessor {
            self.emit_disabled_warning_once(
                node_index,
                &state.current_frame,
                FrameProcessorWarningDetails::default(),
                Some(format!(
                    "processor failed repeatedly and was disabled: {}",
                    error.message
                )),
            );
        } else {
            let node_snapshot = self.node_snapshot(node_index);
            self.push_warning(
                FrameProcessorWarningKind::OutputDropped,
                &node_snapshot,
                &state.current_frame,
                FrameProcessorWarningDetails::default(),
                if self.mode == FrameProcessorMode::RequireProcessed {
                    FrameProcessorPolicyAction::FailPlayback
                } else {
                    breaker_action
                },
                Some(format!("processor call failed: {}", error.message)),
            );
        }

        if self.mode == FrameProcessorMode::RequireProcessed {
            return Err(NativeFrameProcessorProcessError {
                error,
                decoder_frame: decoder_frame.clone(),
            });
        }
        Ok(())
    }

    fn submit_to_node(
        &mut self,
        node_index: usize,
        current_frame: &NativeFrame,
    ) -> Result<FrameProcessorSubmitResult, NativeFrameProcessorError> {
        let submit = FrameProcessorSubmitFrame {
            metadata: current_frame.metadata.clone(),
            present_deadline_us: present_deadline_us(
                current_frame.metadata.pts_us,
                self.policy.frame_deadline,
            ),
        };
        self.metrics.submitted_frame_count = self.metrics.submitted_frame_count.saturating_add(1);
        let node = &mut self.processors[node_index];
        node.session
            .submit_frame(FrameProcessorInputFrame::new(current_frame), &submit)
            .map_err(|error| {
                NativeFrameProcessorError::from_plugin(
                    self.mode,
                    node.processor_index,
                    &node.plugin_name,
                    "submit_frame",
                    error,
                )
            })
    }

    fn receive_from_node(
        &mut self,
        node_index: usize,
    ) -> Result<FrameProcessorReceiveOutput, NativeFrameProcessorError> {
        let node = &mut self.processors[node_index];
        node.session.receive_frame().map_err(|error| {
            NativeFrameProcessorError::from_plugin(
                self.mode,
                node.processor_index,
                &node.plugin_name,
                "receive_frame",
                error,
            )
        })
    }

    fn handle_submit_bypass(
        &mut self,
        node_index: usize,
        submit_result: FrameProcessorSubmitResult,
        decoder_frame: &DecoderNativeFrame,
        state: &mut NativeFrameProcessorProcessState,
        observer: &mut impl NativeFrameProcessorObserver,
    ) -> Result<(), NativeFrameProcessorProcessError> {
        self.reset_to_decoder_frame(decoder_frame, state);
        self.metrics.bypassed_frame_count = self.metrics.bypassed_frame_count.saturating_add(1);
        observer.observe_bypass();
        if submit_result.status == FrameProcessorSubmitStatus::Backpressure {
            self.metrics.backpressure_count = self.metrics.backpressure_count.saturating_add(1);
            observer.observe_backpressure();
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
            return Err(NativeFrameProcessorProcessError {
                error: NativeFrameProcessorError::strict(
                    node_snapshot.processor_index,
                    &node_snapshot.plugin_name,
                    "bypassed a frame",
                ),
                decoder_frame: decoder_frame.clone(),
            });
        }
        Ok(())
    }

    fn handle_submit_rejected(
        &mut self,
        node_index: usize,
        submit_result: FrameProcessorSubmitResult,
        decoder_frame: &DecoderNativeFrame,
        state: &mut NativeFrameProcessorProcessState,
        observer: &mut impl NativeFrameProcessorObserver,
    ) -> Result<(), NativeFrameProcessorProcessError> {
        self.reset_to_decoder_frame(decoder_frame, state);
        observer.observe_bypass();
        let node_snapshot = self.node_snapshot(node_index);
        let details = FrameProcessorWarningDetails {
            queue_depth: submit_result.queue_depth,
            in_flight_frames: submit_result.in_flight_frames,
            ..FrameProcessorWarningDetails::default()
        };
        let breaker_action = self.processors[node_index].breaker.record_failure();
        if breaker_action == FrameProcessorPolicyAction::DisableProcessor {
            self.emit_disabled_warning_once(
                node_index,
                &state.current_frame,
                details,
                submit_result
                    .message
                    .or_else(|| Some("processor rejected frames repeatedly".to_owned())),
            );
        } else {
            self.push_warning(
                FrameProcessorWarningKind::Unsupported,
                &node_snapshot,
                &state.current_frame,
                details,
                if self.mode == FrameProcessorMode::RequireProcessed {
                    FrameProcessorPolicyAction::FailPlayback
                } else {
                    FrameProcessorPolicyAction::BypassOriginalFrame
                },
                submit_result.message,
            );
        }
        if self.mode == FrameProcessorMode::RequireProcessed {
            return Err(NativeFrameProcessorProcessError {
                error: NativeFrameProcessorError::strict(
                    node_snapshot.processor_index,
                    &node_snapshot.plugin_name,
                    "rejected a frame",
                ),
                decoder_frame: decoder_frame.clone(),
            });
        }
        Ok(())
    }

    fn handle_ready_output(
        &mut self,
        node_index: usize,
        output: FrameProcessorOutputFrame,
        decoder_frame: &DecoderNativeFrame,
        state: &mut NativeFrameProcessorProcessState,
        observer: &mut impl NativeFrameProcessorObserver,
    ) -> Result<(), NativeFrameProcessorProcessError> {
        observer.observe_processed_node();
        let node_snapshot = self.node_snapshot(node_index);
        let timing = self.record_output_timing(node_index, &state.current_frame, &output);
        observer.observe_timing(timing.deadline_missed, timing.should_drop_output);
        if timing.should_drop_output || timing.should_fail_playback {
            if output_frame_requires_processor_release(&output.frame)
                && let Err(error) = self.release_processor_outputs_best_effort(vec![
                    NativeFrameProcessorOwnedFrame {
                        processor_index: node_snapshot.processor_index,
                        frame: output.frame.clone(),
                    },
                ])
            {
                return Err(NativeFrameProcessorProcessError {
                    error: error.error,
                    decoder_frame: decoder_frame.clone(),
                });
            }
        }
        if timing.should_fail_playback && self.mode == FrameProcessorMode::RequireProcessed {
            let _ = self.release_processor_outputs_best_effort(std::mem::take(
                &mut state.processor_outputs,
            ));
            return Err(NativeFrameProcessorProcessError {
                error: NativeFrameProcessorError::strict(
                    node_snapshot.processor_index,
                    &node_snapshot.plugin_name,
                    "missed frame deadline",
                ),
                decoder_frame: decoder_frame.clone(),
            });
        }
        if timing.should_drop_output {
            self.reset_to_decoder_frame(decoder_frame, state);
            return Ok(());
        }
        self.accept_processor_output(node_index, output.frame, decoder_frame, state);
        Ok(())
    }

    fn handle_pending_output(
        &mut self,
        node_index: usize,
        decoder_frame: &DecoderNativeFrame,
        state: &mut NativeFrameProcessorProcessState,
        observer: &mut impl NativeFrameProcessorObserver,
    ) -> Result<(), NativeFrameProcessorProcessError> {
        self.reset_to_decoder_frame(decoder_frame, state);
        self.metrics.bypassed_frame_count = self.metrics.bypassed_frame_count.saturating_add(1);
        observer.observe_bypass();
        observer.observe_pending();
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
            return Err(NativeFrameProcessorProcessError {
                error: NativeFrameProcessorError::strict(
                    node_snapshot.processor_index,
                    &node_snapshot.plugin_name,
                    "did not return a ready frame",
                ),
                decoder_frame: decoder_frame.clone(),
            });
        }
        Ok(())
    }

    fn accept_processor_output(
        &mut self,
        node_index: usize,
        output_frame: NativeFrame,
        decoder_frame: &DecoderNativeFrame,
        state: &mut NativeFrameProcessorProcessState,
    ) {
        if output_frame_requires_processor_release(&output_frame) {
            state
                .processor_outputs
                .push(NativeFrameProcessorOwnedFrame {
                    processor_index: self.processors[node_index].processor_index,
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

    fn reset_to_decoder_frame(
        &mut self,
        decoder_frame: &DecoderNativeFrame,
        state: &mut NativeFrameProcessorProcessState,
    ) {
        let _ = self
            .release_processor_outputs_best_effort(std::mem::take(&mut state.processor_outputs));
        state.current_frame = decoder_frame_to_native_frame(decoder_frame);
        state.using_processor_output = false;
    }

    pub fn release_processor_outputs(
        &mut self,
        outputs: Vec<NativeFrameProcessorOwnedFrame>,
    ) -> Result<(), NativeFrameProcessorError> {
        self.release_processor_outputs_best_effort(outputs)
            .map(|_| ())
            .map_err(|failure| failure.error)
    }

    pub fn release_processor_outputs_tracked(
        &mut self,
        outputs: Vec<NativeFrameProcessorOwnedFrame>,
    ) -> Result<NativeFrameProcessorReleaseResult, NativeFrameProcessorReleaseError> {
        match self.release_processor_outputs_best_effort(outputs) {
            Ok(unreleased_outputs) => Ok(NativeFrameProcessorReleaseResult { unreleased_outputs }),
            Err(failure) => Err(NativeFrameProcessorReleaseError {
                error: NativeFramePipelineError::new(
                    "releaseProcessorFrame",
                    failure.error.to_string(),
                ),
                unreleased_outputs: failure.unreleased_outputs,
            }),
        }
    }

    fn release_processor_outputs_best_effort(
        &mut self,
        mut outputs: Vec<NativeFrameProcessorOwnedFrame>,
    ) -> Result<Vec<NativeFrameProcessorOwnedFrame>, NativeFrameProcessorReleaseFailure> {
        let mut first_error = None;
        let mut unreleased_outputs = Vec::new();
        while let Some(output) = outputs.pop() {
            let Some(node) = self
                .processors
                .iter_mut()
                .find(|node| node.processor_index == output.processor_index)
            else {
                if first_error.is_none() {
                    first_error = Some(NativeFrameProcessorError {
                        processor_index: output.processor_index,
                        plugin_name: "<missing>".to_owned(),
                        operation: "release_frame".to_owned(),
                        message: "frame processor is missing for owned output release".to_owned(),
                        strict: self.mode == FrameProcessorMode::RequireProcessed,
                    });
                }
                unreleased_outputs.push(output);
                continue;
            };
            if let Err(error) = node.session.release_frame(output.frame.clone()) {
                if first_error.is_none() {
                    first_error = Some(NativeFrameProcessorError::from_plugin(
                        self.mode,
                        node.processor_index,
                        &node.plugin_name,
                        "release_frame",
                        error,
                    ));
                }
                unreleased_outputs.push(output);
            }
        }
        match first_error {
            Some(error) => Err(NativeFrameProcessorReleaseFailure {
                error,
                unreleased_outputs,
            }),
            None => Ok(Vec::new()),
        }
    }

    pub fn flush(&mut self) -> Result<(), NativeFrameProcessorError> {
        for node in &mut self.processors {
            node.session.flush().map_err(|error| {
                NativeFrameProcessorError::from_plugin(
                    self.mode,
                    node.processor_index,
                    &node.plugin_name,
                    "flush",
                    error,
                )
            })?;
        }
        Ok(())
    }

    pub fn close(&mut self) -> Result<(), NativeFrameProcessorError> {
        if self.closed {
            return Ok(());
        }
        self.closed = true;
        let mut first_error = None;
        for node in &mut self.processors {
            if let Err(error) = node.session.close()
                && first_error.is_none()
            {
                first_error = Some(NativeFrameProcessorError::from_plugin(
                    self.mode,
                    node.processor_index,
                    &node.plugin_name,
                    "close",
                    error,
                ));
            }
        }
        if let Some(error) = first_error {
            return Err(error);
        }
        Ok(())
    }

    fn record_output_timing(
        &mut self,
        node_index: usize,
        input: &NativeFrame,
        output: &FrameProcessorOutputFrame,
    ) -> NativeFrameProcessorTimingDecision {
        self.metrics.processed_frame_count = self.metrics.processed_frame_count.saturating_add(1);
        self.metrics.last_queue_wait_us = output.timings.queue_wait_us;
        self.metrics.last_process_time_us = output.timings.process_time_us;
        self.metrics.last_submit_to_ready_us = output.timings.submit_to_ready_us;
        let mut decision = NativeFrameProcessorTimingDecision::default();
        let node = self.node_snapshot(node_index);
        if output
            .timings
            .submit_to_ready_us
            .is_some_and(|elapsed| elapsed > self.policy.frame_deadline.as_micros() as u64)
        {
            self.metrics.deadline_miss_count = self.metrics.deadline_miss_count.saturating_add(1);
            decision.deadline_missed = true;
            let breaker_action = self.processors[node_index].breaker.record_deadline_miss();
            let consecutive_miss_count = self.processors[node_index]
                .breaker
                .consecutive_deadline_misses();
            let action = if self.mode == FrameProcessorMode::RequireProcessed {
                FrameProcessorPolicyAction::FailPlayback
            } else if breaker_action == FrameProcessorPolicyAction::DisableProcessor {
                FrameProcessorPolicyAction::DisableProcessor
            } else {
                FrameProcessorPolicyAction::BypassOriginalFrame
            };
            let details = FrameProcessorWarningDetails {
                consecutive_miss_count: Some(consecutive_miss_count),
                ..FrameProcessorWarningDetails::from_output_timing(
                    output,
                    self.policy.frame_deadline,
                )
            };
            self.push_warning(
                FrameProcessorWarningKind::DeadlineMissed,
                &node,
                input,
                details.clone(),
                action,
                Some("processor output missed frame deadline".to_owned()),
            );
            if breaker_action == FrameProcessorPolicyAction::DisableProcessor {
                self.emit_disabled_warning_once(
                    node_index,
                    input,
                    details,
                    Some("processor missed frame deadlines repeatedly".to_owned()),
                );
            }
            if self.mode == FrameProcessorMode::RequireProcessed {
                decision.should_fail_playback = true;
            } else if breaker_action == FrameProcessorPolicyAction::DisableProcessor {
                decision.should_drop_output = true;
                self.metrics.dropped_output_count =
                    self.metrics.dropped_output_count.saturating_add(1);
            }
        } else {
            self.processors[node_index].breaker.record_success();
        }
        if output.timings.submit_to_ready_us.is_some_and(|elapsed| {
            elapsed
                > (self.policy.frame_deadline + self.policy.late_output_tolerance).as_micros()
                    as u64
        }) {
            let was_already_dropping = decision.should_drop_output;
            decision.should_drop_output = true;
            if !was_already_dropping {
                self.metrics.dropped_output_count =
                    self.metrics.dropped_output_count.saturating_add(1);
            }
            self.metrics.late_output_drop_count =
                self.metrics.late_output_drop_count.saturating_add(1);
            self.push_warning(
                FrameProcessorWarningKind::LateOutputDropped,
                &node,
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

    fn push_warning(
        &mut self,
        kind: FrameProcessorWarningKind,
        node: &NativeFrameProcessorNodeSnapshot,
        input: &NativeFrame,
        details: FrameProcessorWarningDetails,
        policy_action: FrameProcessorPolicyAction,
        message: Option<String>,
    ) {
        if self.pending_events.len() >= MAX_PENDING_EVENTS {
            self.pending_events.pop_front();
        }
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
                present_deadline_us: present_deadline_us(
                    input.metadata.pts_us,
                    self.policy.frame_deadline,
                ),
                deadline_overrun_us: details.deadline_overrun_us,
                consecutive_miss_count: details.consecutive_miss_count,
                policy_action,
                message,
            }),
        ));
    }

    fn observe_node_load(
        &mut self,
        node_index: usize,
        queue_depth: Option<u32>,
        in_flight_frames: Option<u32>,
    ) -> FrameProcessorPolicyAction {
        if let Some(queue_depth) = queue_depth {
            self.metrics.max_queue_depth = Some(
                self.metrics
                    .max_queue_depth
                    .map_or(queue_depth, |current| current.max(queue_depth)),
            );
        }
        if let Some(in_flight_frames) = in_flight_frames {
            self.metrics.max_in_flight_frames = Some(
                self.metrics
                    .max_in_flight_frames
                    .map_or(in_flight_frames, |current| current.max(in_flight_frames)),
            );
        }
        self.processors[node_index]
            .breaker
            .evaluate_load(queue_depth, in_flight_frames)
    }

    fn emit_disabled_warning_once(
        &mut self,
        node_index: usize,
        input: &NativeFrame,
        details: FrameProcessorWarningDetails,
        message: Option<String>,
    ) {
        if self.processors[node_index].disabled_warning_emitted {
            return;
        }
        self.processors[node_index].disabled_warning_emitted = true;
        self.metrics.disabled_processor_count =
            self.metrics.disabled_processor_count.saturating_add(1);
        let node_snapshot = self.node_snapshot(node_index);
        self.push_warning(
            FrameProcessorWarningKind::Disabled,
            &node_snapshot,
            input,
            details,
            FrameProcessorPolicyAction::DisableProcessor,
            message,
        );
    }

    fn node_snapshot(&self, node_index: usize) -> NativeFrameProcessorNodeSnapshot {
        let node = &self.processors[node_index];
        NativeFrameProcessorNodeSnapshot {
            plugin_name: node.plugin_name.clone(),
            processor_index: node.processor_index,
        }
    }
}

impl Drop for NativeFrameProcessorChainCore {
    fn drop(&mut self) {
        let _ = self.close();
    }
}

pub trait NativeFrameProcessorObserver {
    fn begin_frame(&mut self, _pts_us: Option<i64>, _node_count: usize) {}
    fn observe_submit(&mut self, _queue_depth: Option<u32>, _in_flight_frames: Option<u32>) {}
    fn observe_submitted_node(&mut self) {}
    fn observe_processed_node(&mut self) {}
    fn observe_bypass(&mut self) {}
    fn observe_backpressure(&mut self) {}
    fn observe_pending(&mut self) {}
    fn observe_timing(&mut self, _deadline_missed: bool, _dropped_output: bool) {}
    fn finish_frame(&mut self, _output_pts_us: Option<i64>, _presented_processed: bool) {}
}

#[derive(Debug, Default)]
pub struct NoopNativeFrameProcessorObserver;

impl NativeFrameProcessorObserver for NoopNativeFrameProcessorObserver {}

pub struct NativeFrameProcessorNode {
    pub plugin_name: String,
    pub processor_index: usize,
    pub session: Box<dyn FrameProcessorSession>,
    breaker: PluginBreakerState,
    disabled_warning_emitted: bool,
}

impl std::fmt::Debug for NativeFrameProcessorNode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NativeFrameProcessorNode")
            .field("plugin_name", &self.plugin_name)
            .field("processor_index", &self.processor_index)
            .finish()
    }
}

impl NativeFrameProcessorNode {
    pub fn new(
        plugin_name: impl Into<String>,
        processor_index: usize,
        session: Box<dyn FrameProcessorSession>,
    ) -> Self {
        Self {
            plugin_name: plugin_name.into(),
            processor_index,
            session,
            breaker: PluginBreakerState::new(PluginBudgetPolicy::default()),
            disabled_warning_emitted: false,
        }
    }

    pub fn with_budget(mut self, budget: PluginBudgetPolicy) -> Self {
        self.breaker = PluginBreakerState::new(budget);
        self.disabled_warning_emitted = false;
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeFrameProcessorProcessedFrame {
    pub decoder_frame: DecoderNativeFrame,
    pub presentation_frame: DecoderNativeFrame,
    pub processor_outputs: Vec<NativeFrameProcessorOwnedFrame>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeFrameProcessorOwnedFrame {
    pub processor_index: usize,
    pub frame: NativeFrame,
}

#[derive(Debug)]
pub struct NativeFrameProcessorProcessError {
    pub error: NativeFrameProcessorError,
    pub decoder_frame: DecoderNativeFrame,
}

#[derive(Debug)]
pub struct NativeFrameProcessorReleaseFailure {
    pub error: NativeFrameProcessorError,
    pub unreleased_outputs: Vec<NativeFrameProcessorOwnedFrame>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeFrameProcessorError {
    pub processor_index: usize,
    pub plugin_name: String,
    pub operation: String,
    pub message: String,
    pub strict: bool,
}

impl NativeFrameProcessorError {
    fn from_plugin(
        mode: FrameProcessorMode,
        processor_index: usize,
        plugin_name: &str,
        operation: &str,
        error: FrameProcessorError,
    ) -> Self {
        let strict = mode == FrameProcessorMode::RequireProcessed;
        let message = if strict {
            format!(
                "frame processor `{plugin_name}` at index {processor_index} {operation} failed in strict mode: {error}"
            )
        } else {
            format!(
                "frame processor `{plugin_name}` at index {processor_index} {operation} failed: {error}"
            )
        };
        Self {
            processor_index,
            plugin_name: plugin_name.to_owned(),
            operation: operation.to_owned(),
            message,
            strict,
        }
    }

    fn strict(processor_index: usize, plugin_name: &str, reason: &str) -> Self {
        Self {
            processor_index,
            plugin_name: plugin_name.to_owned(),
            operation: "process".to_owned(),
            message: format!("frame processor `{plugin_name}` {reason} in strict mode"),
            strict: true,
        }
    }
}

impl std::fmt::Display for NativeFrameProcessorError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for NativeFrameProcessorError {}

#[derive(Debug, Clone)]
struct NativeFrameProcessorProcessState {
    current_frame: NativeFrame,
    processor_outputs: Vec<NativeFrameProcessorOwnedFrame>,
    using_processor_output: bool,
}

#[derive(Debug, Clone)]
struct NativeFrameProcessorNodeSnapshot {
    plugin_name: String,
    processor_index: usize,
}

#[derive(Debug, Default, Clone)]
struct FrameProcessorWarningDetails {
    output_handle_kind: Option<String>,
    queue_depth: Option<u32>,
    in_flight_frames: Option<u32>,
    queue_wait_us: Option<u64>,
    process_time_us: Option<u64>,
    submit_to_ready_us: Option<u64>,
    deadline_overrun_us: Option<u64>,
    consecutive_miss_count: Option<u32>,
}

impl FrameProcessorWarningDetails {
    fn from_output_timing(output: &FrameProcessorOutputFrame, deadline: Duration) -> Self {
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
struct NativeFrameProcessorTimingDecision {
    should_drop_output: bool,
    should_fail_playback: bool,
    deadline_missed: bool,
}

pub fn decoder_frame_to_native_frame(frame: &DecoderNativeFrame) -> NativeFrame {
    NativeFrame {
        metadata: frame.metadata.clone().into(),
        handle: frame.handle,
        lease_token: frame.lease_token,
    }
}

pub fn native_frame_to_decoder_frame(frame: &NativeFrame) -> DecoderNativeFrame {
    DecoderNativeFrame {
        metadata: frame.metadata.clone().into(),
        handle: frame.handle,
        lease_token: frame.lease_token,
    }
}

pub fn output_frame_requires_processor_release(frame: &NativeFrame) -> bool {
    frame
        .metadata
        .release_tracking
        .as_ref()
        .is_none_or(|tracking| tracking.requires_release)
}

pub fn present_deadline_us(pts_us: Option<i64>, frame_deadline: Duration) -> Option<i64> {
    pts_us.map(|pts| pts.saturating_add(duration_us_i64(frame_deadline)))
}

pub fn duration_us_i64(duration: Duration) -> i64 {
    i64::try_from(duration.as_micros()).unwrap_or(i64::MAX)
}

pub fn native_frame_release_tracking(requires_release: bool) -> NativeFrameReleaseTracking {
    NativeFrameReleaseTracking {
        frame_id: None,
        requires_release,
    }
}

pub fn timing_with_submit_to_ready(submit_to_ready_us: u64) -> FrameProcessorFrameTimings {
    FrameProcessorFrameTimings {
        queue_wait_us: None,
        process_time_us: None,
        submit_to_ready_us: Some(submit_to_ready_us),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeFramePipelineCoreConfig {
    pub max_in_flight_frames: u32,
    pub packet_budget: usize,
    pub pending_presenter_message: String,
    pub missing_packet_source_message: String,
    pub decoder_warmup_message: String,
}

impl Default for NativeFramePipelineCoreConfig {
    fn default() -> Self {
        Self {
            max_in_flight_frames: 3,
            packet_budget: 8,
            pending_presenter_message: "native-frame presenter is waiting".to_owned(),
            missing_packet_source_message: "native-frame packet source is not configured"
                .to_owned(),
            decoder_warmup_message: "native-frame decoder is warming up".to_owned(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NativeFramePipelineLifecycleState {
    WaitingForSource,
    WaitingForOutputTarget,
    OpeningDecoder,
    WarmingUp,
    Presenting,
    Failed,
}

impl NativeFramePipelineLifecycleState {
    pub fn wire_name(self) -> &'static str {
        match self {
            Self::WaitingForSource => "waitingForSource",
            Self::WaitingForOutputTarget => "waitingForOutputTarget",
            Self::OpeningDecoder => "openingDecoder",
            Self::WarmingUp => "warmingUp",
            Self::Presenting => "presenting",
            Self::Failed => "failed",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NativeFramePipelineFrameStatus {
    Pending,
    Frame,
    Presented,
    EndOfStream,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeFramePipelineFrame {
    pub handle: usize,
    pub presentation_time_us: i64,
    pub duration_us: Option<i64>,
    pub width: u32,
    pub height: u32,
    pub frame_id: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeFramePipelineFrameResult {
    pub status: NativeFramePipelineFrameStatus,
    pub handle: Option<u64>,
    pub frame: Option<NativeFramePipelineFrame>,
    pub requires_host_release: bool,
    pub message: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NativeFramePipelinePacketStatus {
    PacketQueued,
    NeedMoreData,
    EndOfStream,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeFramePipelinePacketResult {
    pub status: NativeFramePipelinePacketStatus,
    pub message: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NativeFramePipelineDecoderPacketStatus {
    Sent,
    Backpressure,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeFramePipelineDecoderPacketResult {
    pub status: NativeFramePipelineDecoderPacketStatus,
    pub message: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NativeFramePipelineCounters {
    pub decoded_frames: u64,
    pub processed_frames: u64,
    pub presented_frames: u64,
    pub presenter_submit_count: u64,
    pub presenter_backpressure_count: u64,
    pub presenter_attach_count: u64,
    pub presenter_detach_count: u64,
    pub skipped_audio_packets: u64,
    pub skipped_video_packets: u64,
    pub skipped_other_packets: u64,
    pub seek_count: u64,
    pub flush_count: u64,
    pub deadline_misses: u64,
    pub backpressure_count: u64,
    pub late_dropped: u64,
    pub released_frames: u64,
    pub source_packets_read: u64,
    pub source_packet_bytes: u64,
    pub decoder_packets_sent: u64,
    pub decoder_packet_bytes: u64,
    pub decoder_backpressure_count: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NativeFrameProcessorMetricsDelta {
    pub processed_frames: u64,
    pub deadline_misses: u64,
    pub late_dropped: u64,
    pub backpressure_count: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeFramePipelineStatusSnapshot {
    pub lifecycle_state: NativeFramePipelineLifecycleState,
    pub output_target_attached: bool,
    pub presenter_configured: bool,
    pub decoder_configured: bool,
    pub pending_frames: usize,
    pub pending_packet: bool,
    pub end_of_stream: bool,
    pub epoch: u64,
    pub counters: NativeFramePipelineCounters,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeFramePipelineError {
    pub operation: &'static str,
    pub message: String,
}

impl NativeFramePipelineError {
    pub fn new(operation: &'static str, message: impl Into<String>) -> Self {
        Self {
            operation,
            message: message.into(),
        }
    }
}

impl std::fmt::Display for NativeFramePipelineError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} failed: {}", self.operation, self.message)
    }
}

impl std::error::Error for NativeFramePipelineError {}

#[derive(Debug, Clone)]
pub enum NativeFramePacketRead {
    Packet {
        packet: SourceNormalizerPacket,
        data: Vec<u8>,
        message: Option<String>,
    },
    NeedMoreData {
        message: Option<String>,
    },
    EndOfStream {
        message: Option<String>,
    },
}

pub trait NativeFramePacketSourceAdapter: Send {
    fn selected_video_stream_index(&self) -> Option<u32>;
    fn read_packet(&mut self) -> Result<NativeFramePacketRead, NativeFramePipelineError>;
    fn flush(&mut self) -> Result<(), NativeFramePipelineError>;
    fn seek(&mut self, position: Duration) -> Result<(), NativeFramePipelineError>;
    fn close(&mut self) -> Result<(), NativeFramePipelineError>;
}

pub trait NativeFrameDecoderAdapter: Send {
    fn send_packet(
        &mut self,
        packet: &DecoderPacket,
        data: &[u8],
    ) -> Result<DecoderPacketResult, NativeFramePipelineError>;

    fn receive_native_frame(
        &mut self,
    ) -> Result<DecoderReceiveNativeFrameOutput, NativeFramePipelineError>;

    fn release_native_frame(
        &mut self,
        frame: DecoderNativeFrame,
        presented: bool,
    ) -> Result<(), NativeFramePipelineError>;

    fn flush(&mut self) -> Result<(), NativeFramePipelineError>;
    fn close(&mut self) -> Result<(), NativeFramePipelineError>;
}

pub fn protect_native_frame_packet_source_adapter(
    inner: Box<dyn NativeFramePacketSourceAdapter>,
) -> Box<dyn NativeFramePacketSourceAdapter> {
    Box::new(ProtectedNativeFramePacketSourceAdapter::new(inner))
}

pub fn protect_native_frame_decoder_adapter(
    inner: Box<dyn NativeFrameDecoderAdapter>,
) -> Box<dyn NativeFrameDecoderAdapter> {
    Box::new(ProtectedNativeFrameDecoderAdapter::new(inner))
}

struct ProtectedNativeFramePacketSourceAdapter {
    inner: Box<dyn NativeFramePacketSourceAdapter>,
    breaker: PluginBreakerState,
}

impl ProtectedNativeFramePacketSourceAdapter {
    fn new(inner: Box<dyn NativeFramePacketSourceAdapter>) -> Self {
        Self {
            inner,
            breaker: PluginBreakerState::new(PluginBudgetPolicy::default()),
        }
    }

    fn disabled_error(operation: &'static str) -> NativeFramePipelineError {
        NativeFramePipelineError::new(
            operation,
            "native-frame packet source disabled after repeated adapter failures",
        )
    }

    fn ensure_enabled(&self, operation: &'static str) -> Result<(), NativeFramePipelineError> {
        if self.breaker.is_disabled() {
            Err(Self::disabled_error(operation))
        } else {
            Ok(())
        }
    }

    fn record_failure(&mut self, error: NativeFramePipelineError) -> NativeFramePipelineError {
        let _ = self.breaker.record_failure();
        error
    }
}

impl NativeFramePacketSourceAdapter for ProtectedNativeFramePacketSourceAdapter {
    fn selected_video_stream_index(&self) -> Option<u32> {
        self.inner.selected_video_stream_index()
    }

    fn read_packet(&mut self) -> Result<NativeFramePacketRead, NativeFramePipelineError> {
        self.ensure_enabled("readPacket")?;
        let read = self
            .inner
            .read_packet()
            .map_err(|error| self.record_failure(error))?;
        self.breaker.record_success();
        Ok(read)
    }

    fn flush(&mut self) -> Result<(), NativeFramePipelineError> {
        self.ensure_enabled("flushPacketSource")?;
        self.inner
            .flush()
            .map(|()| {
                self.breaker.record_success();
            })
            .map_err(|error| self.record_failure(error))
    }

    fn seek(&mut self, position: Duration) -> Result<(), NativeFramePipelineError> {
        self.ensure_enabled("seekPacketSource")?;
        self.inner
            .seek(position)
            .map(|()| {
                self.breaker.record_success();
            })
            .map_err(|error| self.record_failure(error))
    }

    fn close(&mut self) -> Result<(), NativeFramePipelineError> {
        self.inner.close()
    }
}

struct ProtectedNativeFrameDecoderAdapter {
    inner: Box<dyn NativeFrameDecoderAdapter>,
    breaker: PluginBreakerState,
}

impl ProtectedNativeFrameDecoderAdapter {
    fn new(inner: Box<dyn NativeFrameDecoderAdapter>) -> Self {
        Self {
            inner,
            breaker: PluginBreakerState::new(PluginBudgetPolicy::default()),
        }
    }

    fn disabled_error(operation: &'static str) -> NativeFramePipelineError {
        NativeFramePipelineError::new(
            operation,
            "native-frame decoder disabled after repeated adapter failures",
        )
    }

    fn ensure_enabled(&self, operation: &'static str) -> Result<(), NativeFramePipelineError> {
        if self.breaker.is_disabled() {
            Err(Self::disabled_error(operation))
        } else {
            Ok(())
        }
    }

    fn record_failure(&mut self, error: NativeFramePipelineError) -> NativeFramePipelineError {
        let _ = self.breaker.record_failure();
        error
    }
}

impl NativeFrameDecoderAdapter for ProtectedNativeFrameDecoderAdapter {
    fn send_packet(
        &mut self,
        packet: &DecoderPacket,
        data: &[u8],
    ) -> Result<DecoderPacketResult, NativeFramePipelineError> {
        self.ensure_enabled("sendDecoderPacket")?;
        self.inner
            .send_packet(packet, data)
            .map(|result| {
                self.breaker.record_success();
                result
            })
            .map_err(|error| self.record_failure(error))
    }

    fn receive_native_frame(
        &mut self,
    ) -> Result<DecoderReceiveNativeFrameOutput, NativeFramePipelineError> {
        self.ensure_enabled("receiveDecoderFrame")?;
        let output = self
            .inner
            .receive_native_frame()
            .map_err(|error| self.record_failure(error))?;
        self.breaker.record_success();
        Ok(output)
    }

    fn release_native_frame(
        &mut self,
        frame: DecoderNativeFrame,
        presented: bool,
    ) -> Result<(), NativeFramePipelineError> {
        self.inner
            .release_native_frame(frame, presented)
            .map(|()| {
                self.breaker.record_success();
            })
            .map_err(|error| self.record_failure(error))
    }

    fn flush(&mut self) -> Result<(), NativeFramePipelineError> {
        self.inner
            .flush()
            .map(|()| {
                self.breaker.record_success();
            })
            .map_err(|error| self.record_failure(error))
    }

    fn close(&mut self) -> Result<(), NativeFramePipelineError> {
        self.inner.close()
    }
}

pub trait NativeFramePresenterAdapter: Send {
    fn submit_frame(
        &mut self,
        frame: &NativeFramePresenterFrame,
    ) -> Result<NativeFramePresenterSubmitResult, NativeFramePipelineError>;

    fn decoder_device_context(&self) -> Option<DecoderNativeDeviceContext> {
        None
    }

    fn flush(&mut self) -> Result<(), NativeFramePipelineError>;
    fn close(&mut self) -> Result<(), NativeFramePipelineError>;
}

pub trait NativeFramePipelineObserver {
    fn lifecycle_state_changed(&mut self, _state: NativeFramePipelineLifecycleState) {}
    fn epoch_changed(&mut self, _epoch: u64) {}
}

pub trait NativeFrameProcessorPipelineAdapter: Send {
    fn process_frame(
        &mut self,
        frame: DecoderNativeFrame,
    ) -> Result<
        (
            NativeFrameProcessorProcessedFrame,
            NativeFrameProcessorMetricsDelta,
        ),
        NativeFramePipelineError,
    >;

    fn release_processor_outputs(
        &mut self,
        outputs: Vec<NativeFrameProcessorOwnedFrame>,
    ) -> Result<NativeFrameProcessorReleaseResult, NativeFrameProcessorReleaseError>;

    fn flush(&mut self) -> Result<(), NativeFramePipelineError>;
    fn close(&mut self) -> Result<(), NativeFramePipelineError>;
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NativeFrameProcessorReleaseResult {
    pub unreleased_outputs: Vec<NativeFrameProcessorOwnedFrame>,
}

#[derive(Debug)]
pub struct NativeFrameProcessorReleaseError {
    pub error: NativeFramePipelineError,
    pub unreleased_outputs: Vec<NativeFrameProcessorOwnedFrame>,
}

impl NativeFrameProcessorPipelineAdapter for NativeFrameProcessorChainCore {
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
        let before = self.metrics().clone();
        let processed = self
            .process(frame, &mut NoopNativeFrameProcessorObserver)
            .map_err(|error| {
                NativeFramePipelineError::new("processFrame", error.error.to_string())
            })?;
        let after = self.metrics();
        Ok((
            processed,
            NativeFrameProcessorMetricsDelta {
                processed_frames: after
                    .processed_frame_count
                    .saturating_sub(before.processed_frame_count),
                deadline_misses: after
                    .deadline_miss_count
                    .saturating_sub(before.deadline_miss_count),
                late_dropped: after
                    .late_output_drop_count
                    .saturating_sub(before.late_output_drop_count),
                backpressure_count: after
                    .backpressure_count
                    .saturating_sub(before.backpressure_count),
            },
        ))
    }

    fn release_processor_outputs(
        &mut self,
        outputs: Vec<NativeFrameProcessorOwnedFrame>,
    ) -> Result<NativeFrameProcessorReleaseResult, NativeFrameProcessorReleaseError> {
        self.release_processor_outputs_tracked(outputs)
    }

    fn flush(&mut self) -> Result<(), NativeFramePipelineError> {
        NativeFrameProcessorChainCore::flush(self)
            .map_err(|error| NativeFramePipelineError::new("flushProcessor", error.to_string()))
    }

    fn close(&mut self) -> Result<(), NativeFramePipelineError> {
        NativeFrameProcessorChainCore::close(self)
            .map_err(|error| NativeFramePipelineError::new("closeProcessor", error.to_string()))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeFramePresenterFrame {
    pub frame_handle: u64,
    pub frame: NativeFramePipelineFrame,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeFramePresenterSubmitResult {
    pub accepted: bool,
    pub requires_host_release: bool,
    pub message: Option<String>,
}

#[derive(Debug, Clone)]
struct NativeFramePipelinePendingPacket {
    packet: SourceNormalizerPacket,
    data: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct NativeFramePipelinePendingFrame {
    frame: NativeFramePipelineFrame,
    decoder_frame: Option<DecoderNativeFrame>,
    processor_outputs: Vec<NativeFrameProcessorOwnedFrame>,
    epoch: u64,
}

pub struct NativeFramePipelineCore {
    config: NativeFramePipelineCoreConfig,
    packet_source: Option<Box<dyn NativeFramePacketSourceAdapter>>,
    decoder: Option<Box<dyn NativeFrameDecoderAdapter>>,
    processor_chain: Option<Box<dyn NativeFrameProcessorPipelineAdapter>>,
    presenter: Option<Box<dyn NativeFramePresenterAdapter>>,
    pending_packet: Option<NativeFramePipelinePendingPacket>,
    pending_frames: HashMap<u64, NativeFramePipelinePendingFrame>,
    next_frame_handle: u64,
    end_of_stream: bool,
    output_target_attached: bool,
    lifecycle_state: NativeFramePipelineLifecycleState,
    epoch: u64,
    counters: NativeFramePipelineCounters,
    closed: bool,
}

impl NativeFramePipelineCore {
    pub fn new(config: NativeFramePipelineCoreConfig) -> Self {
        Self {
            config: NativeFramePipelineCoreConfig {
                max_in_flight_frames: config.max_in_flight_frames.max(1),
                packet_budget: config.packet_budget.max(1),
                ..config
            },
            packet_source: None,
            decoder: None,
            processor_chain: None,
            presenter: None,
            pending_packet: None,
            pending_frames: HashMap::new(),
            next_frame_handle: 1,
            end_of_stream: false,
            output_target_attached: false,
            lifecycle_state: NativeFramePipelineLifecycleState::WaitingForSource,
            epoch: 0,
            counters: NativeFramePipelineCounters::default(),
            closed: false,
        }
    }

    pub fn with_components(
        config: NativeFramePipelineCoreConfig,
        packet_source: Option<Box<dyn NativeFramePacketSourceAdapter>>,
        decoder: Option<Box<dyn NativeFrameDecoderAdapter>>,
        processor_chain: Option<Box<dyn NativeFrameProcessorPipelineAdapter>>,
        presenter: Option<Box<dyn NativeFramePresenterAdapter>>,
    ) -> Self {
        let mut core = Self::new(config);
        core.packet_source = packet_source.map(protect_native_frame_packet_source_adapter);
        core.decoder = decoder.map(protect_native_frame_decoder_adapter);
        core.processor_chain = processor_chain;
        core.presenter = presenter;
        core.refresh_lifecycle_state();
        core
    }

    pub fn counters(&self) -> &NativeFramePipelineCounters {
        &self.counters
    }

    pub fn counters_mut(&mut self) -> &mut NativeFramePipelineCounters {
        &mut self.counters
    }

    pub fn epoch(&self) -> u64 {
        self.epoch
    }

    pub fn lifecycle_state(&self) -> NativeFramePipelineLifecycleState {
        self.lifecycle_state
    }

    pub fn status_snapshot(&self) -> NativeFramePipelineStatusSnapshot {
        NativeFramePipelineStatusSnapshot {
            lifecycle_state: self.lifecycle_state,
            output_target_attached: self.output_target_attached,
            presenter_configured: self.presenter.is_some(),
            decoder_configured: self.decoder.is_some(),
            pending_frames: self.pending_frames.len(),
            pending_packet: self.pending_packet.is_some(),
            end_of_stream: self.end_of_stream,
            epoch: self.epoch,
            counters: self.counters.clone(),
        }
    }

    pub fn has_packet_source(&self) -> bool {
        self.packet_source.is_some()
    }

    pub fn has_decoder(&self) -> bool {
        self.decoder.is_some()
    }

    pub fn has_presenter(&self) -> bool {
        self.presenter.is_some()
    }

    pub fn output_target_attached(&self) -> bool {
        self.output_target_attached
    }

    pub fn pending_frame_count(&self) -> usize {
        self.pending_frames.len()
    }

    pub fn has_pending_packet(&self) -> bool {
        self.pending_packet.is_some()
    }

    pub fn pending_packet_data_len(&self) -> Option<usize> {
        self.pending_packet.as_ref().map(|packet| packet.data.len())
    }

    pub fn pending_packet_stream_index(&self) -> Option<u32> {
        self.pending_packet
            .as_ref()
            .map(|packet| packet.packet.stream_index)
    }

    pub fn end_of_stream(&self) -> bool {
        self.end_of_stream
    }

    pub fn set_packet_source(&mut self, packet_source: Box<dyn NativeFramePacketSourceAdapter>) {
        self.packet_source = Some(protect_native_frame_packet_source_adapter(packet_source));
        self.refresh_lifecycle_state();
    }

    pub fn set_decoder(&mut self, decoder: Box<dyn NativeFrameDecoderAdapter>) {
        self.decoder = Some(protect_native_frame_decoder_adapter(decoder));
        self.refresh_lifecycle_state();
    }

    pub fn set_processor_chain(
        &mut self,
        processor_chain: Box<dyn NativeFrameProcessorPipelineAdapter>,
    ) {
        self.processor_chain = Some(processor_chain);
    }

    pub fn set_presenter(&mut self, presenter: Box<dyn NativeFramePresenterAdapter>) {
        if let Some(mut previous) = self.presenter.take() {
            let _ = previous.close();
        }
        self.presenter = Some(presenter);
        self.refresh_lifecycle_state();
    }

    pub fn set_output_target_attached(&mut self, attached: bool) {
        if self.output_target_attached == attached {
            return;
        }
        self.output_target_attached = attached;
        if attached {
            self.counters.presenter_attach_count =
                self.counters.presenter_attach_count.saturating_add(1);
        } else {
            self.counters.presenter_detach_count =
                self.counters.presenter_detach_count.saturating_add(1);
            self.bump_epoch();
        }
        self.refresh_lifecycle_state();
    }

    pub fn presenter_decoder_device_context(&self) -> Option<DecoderNativeDeviceContext> {
        self.presenter
            .as_ref()
            .and_then(|presenter| presenter.decoder_device_context())
    }

    pub fn close_decoder_for_rebind(&mut self) -> Result<(), NativeFramePipelineError> {
        let mut first_error = self.release_all_pending_frames(false).err();
        if let Some(mut decoder) = self.decoder.take()
            && let Err(error) = decoder.close()
            && first_error.is_none()
        {
            first_error = Some(error);
        }
        self.bump_epoch();
        self.refresh_lifecycle_state();
        match first_error {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }

    pub fn clear_presenter_for_detach(&mut self) -> Result<(), NativeFramePipelineError> {
        let mut first_error = self.release_all_pending_frames(false).err();
        if let Some(mut decoder) = self.decoder.take()
            && let Err(error) = decoder.close()
            && first_error.is_none()
        {
            first_error = Some(error);
        }
        if let Some(mut presenter) = self.presenter.take()
            && let Err(error) = presenter.close()
            && first_error.is_none()
        {
            first_error = Some(error);
        }
        self.set_output_target_attached(false);
        self.refresh_lifecycle_state();
        match first_error {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }

    pub fn advance(&mut self) -> Result<NativeFramePipelineFrameResult, NativeFramePipelineError> {
        self.refresh_lifecycle_state();
        if self.pending_frames.len() as u32 >= self.config.max_in_flight_frames {
            self.counters.backpressure_count = self.counters.backpressure_count.saturating_add(1);
            return Ok(NativeFramePipelineFrameResult {
                status: NativeFramePipelineFrameStatus::Pending,
                handle: None,
                frame: None,
                requires_host_release: false,
                message: Some("max in-flight native frames reached".to_owned()),
            });
        }

        if self.packet_source.is_none() {
            return Ok(NativeFramePipelineFrameResult {
                status: if self.end_of_stream {
                    NativeFramePipelineFrameStatus::EndOfStream
                } else {
                    NativeFramePipelineFrameStatus::Pending
                },
                handle: None,
                frame: None,
                requires_host_release: false,
                message: Some(self.config.missing_packet_source_message.clone()),
            });
        }

        let last_message = None;
        for _ in 0..self.config.packet_budget {
            if self.pending_packet.is_none() {
                match self.queue_next_video_packet()? {
                    NativeFramePipelinePacketStatus::NeedMoreData => {
                        return Ok(NativeFramePipelineFrameResult {
                            status: NativeFramePipelineFrameStatus::Pending,
                            handle: None,
                            frame: None,
                            requires_host_release: false,
                            message: last_message.or_else(|| {
                                Some("source normalizer packet source needs more data".to_owned())
                            }),
                        });
                    }
                    NativeFramePipelinePacketStatus::EndOfStream => {
                        self.end_of_stream = true;
                        if self.presenter_ready()
                            && let Some(decoder) = self.decoder.as_deref_mut()
                        {
                            let receive = decoder.receive_native_frame()?;
                            let output =
                                self.decoder_receive_result(receive, last_message.clone())?;
                            if output.status != NativeFramePipelineFrameStatus::Pending {
                                return Ok(output);
                            }
                        }
                        return Ok(NativeFramePipelineFrameResult {
                            status: NativeFramePipelineFrameStatus::EndOfStream,
                            handle: None,
                            frame: None,
                            requires_host_release: false,
                            message: last_message,
                        });
                    }
                    NativeFramePipelinePacketStatus::PacketQueued => {}
                }
            }

            if !self.presenter_ready() {
                return Ok(NativeFramePipelineFrameResult {
                    status: NativeFramePipelineFrameStatus::Pending,
                    handle: None,
                    frame: None,
                    requires_host_release: false,
                    message: Some(self.config.pending_presenter_message.clone()),
                });
            }

            let Some(pending) = self.pending_packet.as_ref() else {
                continue;
            };
            let decoder_packet =
                DecoderPacket::try_from(pending.packet.clone()).map_err(|error| {
                    NativeFramePipelineError::new(
                        "decodePacketFromSourceNormalizerPacket",
                        error.to_string(),
                    )
                })?;
            let Some(decoder) = self.decoder.as_deref_mut() else {
                return Ok(NativeFramePipelineFrameResult {
                    status: NativeFramePipelineFrameStatus::Pending,
                    handle: None,
                    frame: None,
                    requires_host_release: false,
                    message: Some("native-frame decoder is not configured".to_owned()),
                });
            };
            let result = decoder.send_packet(&decoder_packet, &pending.data)?;
            if result.accepted {
                self.counters.decoder_packets_sent =
                    self.counters.decoder_packets_sent.saturating_add(1);
                self.counters.decoder_packet_bytes = self
                    .counters
                    .decoder_packet_bytes
                    .saturating_add(pending.data.len() as u64);
                self.pending_packet = None;
                let receive = decoder.receive_native_frame()?;
                let output = self.decoder_receive_result(receive, last_message.clone())?;
                if output.status != NativeFramePipelineFrameStatus::Pending {
                    return Ok(output);
                }
                return Ok(output);
            }

            self.counters.backpressure_count = self.counters.backpressure_count.saturating_add(1);
            self.counters.decoder_backpressure_count =
                self.counters.decoder_backpressure_count.saturating_add(1);
            return Ok(NativeFramePipelineFrameResult {
                status: NativeFramePipelineFrameStatus::Pending,
                handle: None,
                frame: None,
                requires_host_release: false,
                message: Some("native-frame decoder did not accept the packet yet".to_owned()),
            });
        }

        Ok(NativeFramePipelineFrameResult {
            status: NativeFramePipelineFrameStatus::Pending,
            handle: None,
            frame: None,
            requires_host_release: false,
            message: last_message.or_else(|| Some(self.config.decoder_warmup_message.clone())),
        })
    }

    fn queue_next_video_packet(
        &mut self,
    ) -> Result<NativeFramePipelinePacketStatus, NativeFramePipelineError> {
        for _ in 0..self.config.packet_budget {
            let selected_video_stream_index = self
                .packet_source
                .as_ref()
                .and_then(|source| source.selected_video_stream_index());
            let read = self
                .packet_source
                .as_deref_mut()
                .ok_or_else(|| {
                    NativeFramePipelineError::new("readPacket", "packet source is not configured")
                })?
                .read_packet()?;
            match read {
                NativeFramePacketRead::NeedMoreData { .. } => {
                    return Ok(NativeFramePipelinePacketStatus::NeedMoreData);
                }
                NativeFramePacketRead::EndOfStream { .. } => {
                    return Ok(NativeFramePipelinePacketStatus::EndOfStream);
                }
                NativeFramePacketRead::Packet {
                    packet,
                    data,
                    message: _,
                } => {
                    if packet.media_kind != SourceNormalizerPacketMediaKind::Video {
                        self.increment_skipped_packet_counter(packet.media_kind);
                        continue;
                    }
                    if selected_video_stream_index
                        .is_some_and(|selected| packet.stream_index != selected)
                    {
                        self.counters.skipped_video_packets =
                            self.counters.skipped_video_packets.saturating_add(1);
                        continue;
                    }
                    self.counters.source_packets_read =
                        self.counters.source_packets_read.saturating_add(1);
                    self.counters.source_packet_bytes = self
                        .counters
                        .source_packet_bytes
                        .saturating_add(data.len() as u64);
                    self.pending_packet = Some(NativeFramePipelinePendingPacket { packet, data });
                    return Ok(NativeFramePipelinePacketStatus::PacketQueued);
                }
            }
        }
        Ok(NativeFramePipelinePacketStatus::NeedMoreData)
    }

    fn decoder_receive_result(
        &mut self,
        result: DecoderReceiveNativeFrameOutput,
        message: Option<String>,
    ) -> Result<NativeFramePipelineFrameResult, NativeFramePipelineError> {
        match result {
            DecoderReceiveNativeFrameOutput::Frame(frame) => {
                let (handle, frame) = self.process_decoder_frame(frame)?;
                self.submit_frame_to_presenter(handle, frame, message)
            }
            DecoderReceiveNativeFrameOutput::NeedMoreInput => Ok(NativeFramePipelineFrameResult {
                status: NativeFramePipelineFrameStatus::Pending,
                handle: None,
                frame: None,
                requires_host_release: false,
                message: message
                    .or_else(|| Some("native-frame decoder needs more input".to_owned())),
            }),
            DecoderReceiveNativeFrameOutput::Eof => {
                self.end_of_stream = true;
                Ok(NativeFramePipelineFrameResult {
                    status: NativeFramePipelineFrameStatus::EndOfStream,
                    handle: None,
                    frame: None,
                    requires_host_release: false,
                    message: message
                        .or_else(|| Some("native-frame decoder reached end of stream".to_owned())),
                })
            }
        }
    }

    fn process_decoder_frame(
        &mut self,
        decoder_frame: DecoderNativeFrame,
    ) -> Result<(u64, NativeFramePipelineFrame), NativeFramePipelineError> {
        if let Some(processor_chain) = self.processor_chain.as_mut() {
            match processor_chain.process_frame(decoder_frame.clone()) {
                Ok((processed_frame, delta)) => {
                    self.counters.processed_frames = self
                        .counters
                        .processed_frames
                        .saturating_add(delta.processed_frames);
                    self.counters.deadline_misses = self
                        .counters
                        .deadline_misses
                        .saturating_add(delta.deadline_misses);
                    self.counters.late_dropped = self
                        .counters
                        .late_dropped
                        .saturating_add(delta.late_dropped);
                    self.counters.backpressure_count = self
                        .counters
                        .backpressure_count
                        .saturating_add(delta.backpressure_count);
                    return Ok(self.store_processed_frame(processed_frame));
                }
                Err(error) => {
                    if let Some(decoder) = self.decoder.as_deref_mut() {
                        let _ = decoder.release_native_frame(decoder_frame, false);
                    }
                    return Err(error);
                }
            }
        }
        Ok(
            self.store_processed_frame(NativeFrameProcessorProcessedFrame {
                decoder_frame: decoder_frame.clone(),
                presentation_frame: decoder_frame,
                processor_outputs: Vec::new(),
            }),
        )
    }

    fn store_processed_frame(
        &mut self,
        processed_frame: NativeFrameProcessorProcessedFrame,
    ) -> (u64, NativeFramePipelineFrame) {
        let handle = self.next_frame_handle;
        self.next_frame_handle = self.next_frame_handle.saturating_add(1);
        let frame = native_frame_pipeline_frame_from_decoder(&processed_frame.presentation_frame);
        self.pending_frames.insert(
            handle,
            NativeFramePipelinePendingFrame {
                frame: frame.clone(),
                decoder_frame: Some(processed_frame.decoder_frame),
                processor_outputs: processed_frame.processor_outputs,
                epoch: self.epoch,
            },
        );
        self.counters.decoded_frames = self.counters.decoded_frames.saturating_add(1);
        (handle, frame)
    }

    fn submit_frame_to_presenter(
        &mut self,
        handle: u64,
        frame: NativeFramePipelineFrame,
        message: Option<String>,
    ) -> Result<NativeFramePipelineFrameResult, NativeFramePipelineError> {
        let Some(presenter) = self.presenter.as_deref_mut() else {
            let _ = self.release_frame(handle, false);
            return Ok(NativeFramePipelineFrameResult {
                status: NativeFramePipelineFrameStatus::Pending,
                handle: None,
                frame: None,
                requires_host_release: false,
                message: message.or_else(|| Some(self.config.pending_presenter_message.clone())),
            });
        };
        let submit = presenter.submit_frame(&NativeFramePresenterFrame {
            frame_handle: handle,
            frame: frame.clone(),
        })?;
        if !submit.accepted {
            self.counters.presenter_backpressure_count =
                self.counters.presenter_backpressure_count.saturating_add(1);
            self.counters.backpressure_count = self.counters.backpressure_count.saturating_add(1);
            let _ = self.release_frame(handle, false);
            return Ok(NativeFramePipelineFrameResult {
                status: NativeFramePipelineFrameStatus::Pending,
                handle: None,
                frame: None,
                requires_host_release: false,
                message: submit
                    .message
                    .or(message)
                    .or_else(|| Some("native-frame presenter backpressure".to_owned())),
            });
        }
        self.counters.presenter_submit_count =
            self.counters.presenter_submit_count.saturating_add(1);
        self.lifecycle_state = NativeFramePipelineLifecycleState::Presenting;
        if submit.requires_host_release {
            return Ok(NativeFramePipelineFrameResult {
                status: NativeFramePipelineFrameStatus::Frame,
                handle: Some(handle),
                frame: Some(frame),
                requires_host_release: true,
                message: submit.message.or(message),
            });
        }
        self.release_frame(handle, true)?;
        Ok(NativeFramePipelineFrameResult {
            status: NativeFramePipelineFrameStatus::Presented,
            handle: None,
            frame: None,
            requires_host_release: false,
            message: submit.message.or(message),
        })
    }

    pub fn release_frame(
        &mut self,
        frame_handle: u64,
        presented: bool,
    ) -> Result<(), NativeFramePipelineError> {
        let Some(mut pending) = self.pending_frames.remove(&frame_handle) else {
            return Err(NativeFramePipelineError::new(
                "releaseFrame",
                "invalid native-frame pending frame handle",
            ));
        };
        if pending.epoch != self.epoch {
            return Ok(());
        }
        let mut first_error = None;
        let mut released_cleanly = true;
        if !pending.processor_outputs.is_empty() {
            match self.processor_chain.as_mut() {
                Some(processor_chain) => {
                    if let Err(error) = processor_chain
                        .release_processor_outputs(std::mem::take(&mut pending.processor_outputs))
                    {
                        pending.processor_outputs = error.unreleased_outputs;
                        released_cleanly = false;
                        first_error = Some(error.error);
                    }
                }
                None => {
                    released_cleanly = false;
                    first_error = Some(NativeFramePipelineError::new(
                        "releaseProcessorFrame",
                        "processor outputs are pending but processor chain is missing",
                    ));
                }
            }
        }
        if first_error.is_none()
            && let Some(decoder_frame) = pending.decoder_frame.take()
        {
            match self.decoder.as_deref_mut() {
                Some(decoder) => {
                    if let Err(error) =
                        decoder.release_native_frame(decoder_frame.clone(), presented)
                    {
                        pending.decoder_frame = Some(decoder_frame);
                        released_cleanly = false;
                        if first_error.is_none() {
                            first_error = Some(error);
                        }
                    }
                }
                None => {
                    pending.decoder_frame = Some(decoder_frame);
                    released_cleanly = false;
                    if first_error.is_none() {
                        first_error = Some(NativeFramePipelineError::new(
                            "releaseDecoderFrame",
                            "decoder is missing for pending frame release",
                        ));
                    }
                }
            }
        }
        if released_cleanly {
            self.counters.released_frames = self.counters.released_frames.saturating_add(1);
            if presented {
                self.counters.presented_frames = self.counters.presented_frames.saturating_add(1);
            }
        } else {
            self.pending_frames.insert(frame_handle, pending);
        }
        match first_error {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }

    pub fn release_all_pending_frames(
        &mut self,
        presented: bool,
    ) -> Result<(), NativeFramePipelineError> {
        let pending_handles = self.pending_frames.keys().copied().collect::<Vec<_>>();
        let mut first_error = None;
        for handle in pending_handles {
            if let Err(error) = self.release_frame(handle, presented)
                && first_error.is_none()
            {
                first_error = Some(error);
            }
        }
        match first_error {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }

    pub fn flush(&mut self) -> Result<(), NativeFramePipelineError> {
        let mut first_error = self.release_all_pending_frames(false).err();
        if let Some(processor_chain) = self.processor_chain.as_mut()
            && let Err(error) = processor_chain.flush()
            && first_error.is_none()
        {
            first_error = Some(NativeFramePipelineError::new(
                "flushProcessor",
                error.to_string(),
            ));
        }
        if let Some(decoder) = self.decoder.as_deref_mut()
            && let Err(error) = decoder.flush()
            && first_error.is_none()
        {
            first_error = Some(error);
        }
        if let Some(presenter) = self.presenter.as_deref_mut()
            && let Err(error) = presenter.flush()
            && first_error.is_none()
        {
            first_error = Some(error);
        }
        if let Some(packet_source) = self.packet_source.as_deref_mut()
            && let Err(error) = packet_source.flush()
            && first_error.is_none()
        {
            first_error = Some(error);
        }
        self.pending_packet = None;
        self.end_of_stream = false;
        self.bump_epoch();
        if first_error.is_none() {
            self.counters.flush_count = self.counters.flush_count.saturating_add(1);
        }
        self.refresh_lifecycle_state();
        match first_error {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }

    pub fn seek(&mut self, position: Duration) -> Result<(), NativeFramePipelineError> {
        let mut first_error = self.release_all_pending_frames(false).err();
        if let Some(processor_chain) = self.processor_chain.as_mut()
            && let Err(error) = processor_chain.flush()
            && first_error.is_none()
        {
            first_error = Some(NativeFramePipelineError::new(
                "flushProcessor",
                error.to_string(),
            ));
        }
        if let Some(decoder) = self.decoder.as_deref_mut()
            && let Err(error) = decoder.flush()
            && first_error.is_none()
        {
            first_error = Some(error);
        }
        if let Some(presenter) = self.presenter.as_deref_mut()
            && let Err(error) = presenter.flush()
            && first_error.is_none()
        {
            first_error = Some(error);
        }
        if let Some(packet_source) = self.packet_source.as_deref_mut()
            && let Err(error) = packet_source.seek(position)
            && first_error.is_none()
        {
            first_error = Some(error);
        }
        self.pending_packet = None;
        self.end_of_stream = false;
        self.bump_epoch();
        if first_error.is_none() {
            self.counters.seek_count = self.counters.seek_count.saturating_add(1);
        }
        self.refresh_lifecycle_state();
        match first_error {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }

    pub fn close(&mut self) -> Result<(), NativeFramePipelineError> {
        if self.closed {
            return Ok(());
        }
        self.closed = true;
        let mut first_error = self.release_all_pending_frames(false).err();
        if let Some(processor_chain) = self.processor_chain.as_mut()
            && let Err(error) = processor_chain.close()
            && first_error.is_none()
        {
            first_error = Some(NativeFramePipelineError::new(
                "closeProcessor",
                error.to_string(),
            ));
        }
        if let Some(decoder) = self.decoder.as_deref_mut()
            && let Err(error) = decoder.close()
            && first_error.is_none()
        {
            first_error = Some(error);
        }
        if let Some(presenter) = self.presenter.as_deref_mut()
            && let Err(error) = presenter.close()
            && first_error.is_none()
        {
            first_error = Some(error);
        }
        if let Some(packet_source) = self.packet_source.as_deref_mut()
            && let Err(error) = packet_source.close()
            && first_error.is_none()
        {
            first_error = Some(error);
        }
        self.bump_epoch();
        match first_error {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }

    fn presenter_ready(&self) -> bool {
        self.output_target_attached && self.presenter.is_some() && self.decoder.is_some()
    }

    fn increment_skipped_packet_counter(&mut self, media_kind: SourceNormalizerPacketMediaKind) {
        match media_kind {
            SourceNormalizerPacketMediaKind::Audio => {
                self.counters.skipped_audio_packets =
                    self.counters.skipped_audio_packets.saturating_add(1);
            }
            SourceNormalizerPacketMediaKind::Video => {
                self.counters.skipped_video_packets =
                    self.counters.skipped_video_packets.saturating_add(1);
            }
            SourceNormalizerPacketMediaKind::Subtitle => {
                self.counters.skipped_other_packets =
                    self.counters.skipped_other_packets.saturating_add(1);
            }
        }
    }

    fn bump_epoch(&mut self) {
        self.epoch = self.epoch.wrapping_add(1);
    }

    fn refresh_lifecycle_state(&mut self) {
        self.lifecycle_state = if self.packet_source.is_none() {
            NativeFramePipelineLifecycleState::WaitingForSource
        } else if !self.output_target_attached || self.presenter.is_none() {
            NativeFramePipelineLifecycleState::WaitingForOutputTarget
        } else if self.decoder.is_none() {
            NativeFramePipelineLifecycleState::OpeningDecoder
        } else if self.counters.presented_frames > 0 {
            NativeFramePipelineLifecycleState::Presenting
        } else {
            NativeFramePipelineLifecycleState::WarmingUp
        };
    }
}

impl Drop for NativeFramePipelineCore {
    fn drop(&mut self) {
        let _ = self.close();
    }
}

pub fn native_frame_pipeline_frame_from_decoder(
    frame: &DecoderNativeFrame,
) -> NativeFramePipelineFrame {
    NativeFramePipelineFrame {
        handle: frame.handle,
        presentation_time_us: frame.metadata.pts_us.unwrap_or(0),
        duration_us: frame.metadata.duration_us,
        width: frame.metadata.width,
        height: frame.metadata.height,
        frame_id: frame.metadata.frame_id,
    }
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::sync::{Arc, Mutex};

    use player_plugin::{
        DecoderFrameFormat, DecoderMediaKind, DecoderNativeDeviceContext, DecoderNativeFrame,
        DecoderNativeFrameMetadata, DecoderNativeHandleKind, DecoderPacket, DecoderPacketResult,
        DecoderReceiveNativeFrameOutput, FrameProcessorFrameTimings, FrameProcessorInputFrame,
        FrameProcessorOutputFrame, FrameProcessorReceiveOutput, FrameProcessorSession,
        FrameProcessorSessionInfo, FrameProcessorSubmitFrame, FrameProcessorSubmitResult,
        FrameProcessorSubmitStatus, NativeFrameMetadata, NativeFramePipelineProfile,
        NativeFrameReleaseTracking, SourceNormalizerPacket, SourceNormalizerPacketMediaKind,
    };
    use player_runtime::{
        FrameProcessorPolicy, FrameProcessorPolicyAction, FrameProcessorWarningKind,
    };

    use super::*;

    #[derive(Debug, Default)]
    struct TestState {
        submit_status: Option<FrameProcessorSubmitStatus>,
        receive_outputs: VecDeque<FrameProcessorReceiveOutput>,
        released_handles: Vec<usize>,
        submitted_handles: Vec<usize>,
        flush_count: usize,
        close_count: usize,
        submit_error: Option<String>,
        submit_queue_depth: Option<u32>,
        submit_in_flight_frames: Option<u32>,
        receive_error: Option<String>,
        release_error: Option<String>,
        close_error: Option<String>,
    }

    #[derive(Debug)]
    struct TestSession {
        state: Arc<Mutex<TestState>>,
    }

    impl TestSession {
        fn new(state: Arc<Mutex<TestState>>) -> Self {
            Self { state }
        }
    }

    impl FrameProcessorSession for TestSession {
        fn session_info(&self) -> FrameProcessorSessionInfo {
            FrameProcessorSessionInfo::default()
        }

        fn submit_frame(
            &mut self,
            frame: FrameProcessorInputFrame<'_>,
            _submit: &FrameProcessorSubmitFrame,
        ) -> Result<FrameProcessorSubmitResult, player_plugin::FrameProcessorError> {
            let mut state = self.state.lock().expect("state");
            state.submitted_handles.push(frame.native_handle());
            if let Some(message) = state.submit_error.clone() {
                return Err(player_plugin::FrameProcessorError::internal(message));
            }
            Ok(FrameProcessorSubmitResult {
                status: state
                    .submit_status
                    .unwrap_or(FrameProcessorSubmitStatus::Accepted),
                queue_depth: state.submit_queue_depth.or(Some(1)),
                in_flight_frames: state.submit_in_flight_frames.or(Some(1)),
                message: Some("test submit".to_owned()),
            })
        }

        fn receive_frame(
            &mut self,
        ) -> Result<FrameProcessorReceiveOutput, player_plugin::FrameProcessorError> {
            let mut state = self.state.lock().expect("state");
            if let Some(message) = state.receive_error.clone() {
                return Err(player_plugin::FrameProcessorError::internal(message));
            }
            Ok(state
                .receive_outputs
                .pop_front()
                .unwrap_or(FrameProcessorReceiveOutput::Pending))
        }

        fn release_frame(
            &mut self,
            frame: NativeFrame,
        ) -> Result<(), player_plugin::FrameProcessorError> {
            let mut state = self.state.lock().expect("state");
            if let Some(message) = state.release_error.clone() {
                return Err(player_plugin::FrameProcessorError::internal(message));
            }
            state.released_handles.push(frame.handle);
            Ok(())
        }

        fn flush(&mut self) -> Result<(), player_plugin::FrameProcessorError> {
            self.state.lock().expect("state").flush_count += 1;
            Ok(())
        }

        fn close(&mut self) -> Result<(), player_plugin::FrameProcessorError> {
            let mut state = self.state.lock().expect("state");
            state.close_count += 1;
            if let Some(message) = state.close_error.clone() {
                return Err(player_plugin::FrameProcessorError::internal(message));
            }
            Ok(())
        }
    }

    #[derive(Debug, Default)]
    struct TestObserver {
        begin_count: usize,
        submitted_nodes: usize,
        processed_nodes: usize,
        bypasses: usize,
        backpressure: usize,
        pending: usize,
        deadline_misses: usize,
        dropped_outputs: usize,
        presented_processed: usize,
        presented_original: usize,
    }

    impl NativeFrameProcessorObserver for TestObserver {
        fn begin_frame(&mut self, _pts_us: Option<i64>, _node_count: usize) {
            self.begin_count += 1;
        }

        fn observe_submitted_node(&mut self) {
            self.submitted_nodes += 1;
        }

        fn observe_processed_node(&mut self) {
            self.processed_nodes += 1;
        }

        fn observe_bypass(&mut self) {
            self.bypasses += 1;
        }

        fn observe_backpressure(&mut self) {
            self.backpressure += 1;
        }

        fn observe_pending(&mut self) {
            self.pending += 1;
        }

        fn observe_timing(&mut self, deadline_missed: bool, dropped_output: bool) {
            if deadline_missed {
                self.deadline_misses += 1;
            }
            if dropped_output {
                self.dropped_outputs += 1;
            }
        }

        fn finish_frame(&mut self, _output_pts_us: Option<i64>, presented_processed: bool) {
            if presented_processed {
                self.presented_processed += 1;
            } else {
                self.presented_original += 1;
            }
        }
    }

    fn chain(
        mode: FrameProcessorMode,
        state: Arc<Mutex<TestState>>,
    ) -> NativeFrameProcessorChainCore {
        NativeFrameProcessorChainCore::new(
            vec![NativeFrameProcessorNode::new(
                "test-processor",
                0,
                Box::new(TestSession::new(state)),
            )],
            mode,
            FrameProcessorPolicy {
                frame_deadline: Duration::from_millis(16),
                late_output_tolerance: Duration::from_millis(4),
                ..FrameProcessorPolicy::default()
            },
        )
    }

    #[test]
    fn processor_chain_close_attempts_all_processors_and_returns_first_error() {
        let first = Arc::new(Mutex::new(TestState {
            close_error: Some("first close failed".to_owned()),
            ..TestState::default()
        }));
        let second = Arc::new(Mutex::new(TestState::default()));
        let mut chain = NativeFrameProcessorChainCore::new(
            vec![
                NativeFrameProcessorNode::new(
                    "first-processor",
                    0,
                    Box::new(TestSession::new(first.clone())),
                ),
                NativeFrameProcessorNode::new(
                    "second-processor",
                    1,
                    Box::new(TestSession::new(second.clone())),
                ),
            ],
            FrameProcessorMode::PreferProcessed,
            FrameProcessorPolicy::default(),
        );

        let error = chain
            .close()
            .expect_err("first close error should be returned");

        assert_eq!(error.processor_index, 0);
        assert!(error.message.contains("first close failed"));
        assert_eq!(first.lock().expect("first state").close_count, 1);
        assert_eq!(second.lock().expect("second state").close_count, 1);
    }

    fn decoder_frame(handle: usize, pts_us: Option<i64>) -> DecoderNativeFrame {
        DecoderNativeFrame {
            metadata: DecoderNativeFrameMetadata {
                media_kind: DecoderMediaKind::Video,
                format: DecoderFrameFormat::Nv12,
                codec: "h264".to_owned(),
                pts_us,
                duration_us: Some(33_333),
                width: 1_920,
                height: 1_080,
                coded_width: Some(1_920),
                coded_height: Some(1_080),
                visible_rect: None,
                handle_kind: DecoderNativeHandleKind::CvPixelBuffer,
                pipeline_profile: Some(NativeFramePipelineProfile::VideoToolboxCvPixelBuffer),
                color_space: None,
                hdr_metadata: None,
                color: None,
                hdr: None,
                sync_info: None,
                transform: None,
                frame_id: Some(handle as u64),
                release_tracking: Some(player_plugin::DecoderNativeFrameReleaseTracking {
                    frame_id: Some(handle as u64),
                    requires_release: true,
                }),
            },
            handle,
            lease_token: None,
        }
    }

    fn output_frame(
        input: &DecoderNativeFrame,
        offset: usize,
        requires_release: bool,
        submit_to_ready_us: Option<u64>,
    ) -> FrameProcessorReceiveOutput {
        let mut metadata = NativeFrameMetadata::from(input.metadata.clone());
        metadata.release_tracking = Some(NativeFrameReleaseTracking {
            frame_id: Some((input.handle + offset) as u64),
            requires_release,
        });
        metadata.frame_id = Some((input.handle + offset) as u64);
        FrameProcessorReceiveOutput::Frame(FrameProcessorOutputFrame {
            frame: NativeFrame {
                metadata,
                handle: input.handle + offset,
                lease_token: None,
            },
            timings: FrameProcessorFrameTimings {
                queue_wait_us: None,
                process_time_us: None,
                submit_to_ready_us,
            },
            source_frame_id: input.metadata.frame_id,
            message: None,
        })
    }

    fn frame_processor_warnings(
        events: &[PlayerRuntimeEvent],
        kind: FrameProcessorWarningKind,
    ) -> Vec<&FrameProcessorWarning> {
        events
            .iter()
            .filter_map(|event| match event {
                PlayerRuntimeEvent::Warning(PlayerRuntimeWarning::FrameProcessor(warning))
                    if warning.kind == kind =>
                {
                    Some(warning)
                }
                _ => None,
            })
            .collect()
    }

    #[test]
    fn prefer_mode_uses_processed_frame_and_releases_output() {
        let input = decoder_frame(10, Some(33_000));
        let state = Arc::new(Mutex::new(TestState {
            receive_outputs: VecDeque::from([output_frame(&input, 1_000, true, None)]),
            ..TestState::default()
        }));
        let mut chain = chain(FrameProcessorMode::PreferProcessed, state.clone());
        let mut observer = TestObserver::default();

        let processed = chain
            .process(input, &mut observer)
            .expect("process should succeed");

        assert_eq!(processed.presentation_frame.handle, 1_010);
        assert_eq!(processed.decoder_frame.handle, 10);
        assert_eq!(processed.processor_outputs.len(), 1);
        assert_eq!(chain.metrics().submitted_frame_count, 1);
        assert_eq!(chain.metrics().processed_frame_count, 1);
        assert_eq!(observer.presented_processed, 1);

        chain
            .release_processor_outputs(processed.processor_outputs)
            .expect("release outputs");
        assert_eq!(state.lock().expect("state").released_handles, vec![1_010]);
    }

    #[test]
    fn release_tracking_false_does_not_call_processor_release() {
        let input = decoder_frame(10, Some(33_000));
        let state = Arc::new(Mutex::new(TestState {
            receive_outputs: VecDeque::from([output_frame(&input, 0, false, None)]),
            ..TestState::default()
        }));
        let mut chain = chain(FrameProcessorMode::PreferProcessed, state.clone());

        let processed = chain
            .process(input, &mut TestObserver::default())
            .expect("process should succeed");

        assert_eq!(processed.presentation_frame.handle, 10);
        assert!(processed.processor_outputs.is_empty());
        assert!(state.lock().expect("state").released_handles.is_empty());
    }

    #[test]
    fn diagnostics_mode_runs_processor_but_presents_original() {
        let input = decoder_frame(13, Some(120_000));
        let state = Arc::new(Mutex::new(TestState {
            receive_outputs: VecDeque::from([output_frame(&input, 4_000, true, None)]),
            ..TestState::default()
        }));
        let mut chain = chain(FrameProcessorMode::DiagnosticsOnly, state.clone());
        let mut observer = TestObserver::default();

        let processed = chain
            .process(input, &mut observer)
            .expect("process should succeed");

        assert_eq!(processed.presentation_frame.handle, 13);
        assert_eq!(processed.processor_outputs.len(), 1);
        assert_eq!(observer.presented_original, 1);
        chain
            .release_processor_outputs(processed.processor_outputs)
            .expect("release outputs");
        assert_eq!(state.lock().expect("state").released_handles, vec![4_013]);
    }

    #[test]
    fn backpressure_bypasses_and_reports_queue_state() {
        let state = Arc::new(Mutex::new(TestState {
            submit_status: Some(FrameProcessorSubmitStatus::Backpressure),
            submit_queue_depth: Some(3),
            submit_in_flight_frames: Some(2),
            ..TestState::default()
        }));
        let mut chain = chain(FrameProcessorMode::PreferProcessed, state);
        let mut observer = TestObserver::default();

        let processed = chain
            .process(decoder_frame(14, Some(140_000)), &mut observer)
            .expect("backpressure should bypass");

        assert_eq!(processed.presentation_frame.handle, 14);
        assert_eq!(chain.metrics().bypassed_frame_count, 1);
        assert_eq!(chain.metrics().backpressure_count, 1);
        assert_eq!(observer.backpressure, 1);
        let events = chain.drain_events();
        assert!(events.iter().any(|event| matches!(
            event,
            PlayerRuntimeEvent::Warning(PlayerRuntimeWarning::FrameProcessor(warning))
                if warning.kind == FrameProcessorWarningKind::Backpressure
                    && warning.policy_action == FrameProcessorPolicyAction::BypassOriginalFrame
                    && warning.queue_depth == Some(3)
                    && warning.in_flight_frames == Some(2)
        )));
    }

    #[test]
    fn repeated_processor_failures_disable_and_prefer_bypasses_once() {
        let state = Arc::new(Mutex::new(TestState {
            submit_error: Some("submit exploded".to_owned()),
            ..TestState::default()
        }));
        let mut chain = chain(FrameProcessorMode::PreferProcessed, state.clone());

        for index in 0..5 {
            let input = decoder_frame(30 + index, Some(300_000 + index as i64));
            let processed = chain
                .process(input.clone(), &mut TestObserver::default())
                .expect("prefer mode should bypass plugin failures");
            assert_eq!(processed.presentation_frame.handle, input.handle);
        }
        let after_disabled = decoder_frame(40, Some(400_000));
        let processed = chain
            .process(after_disabled.clone(), &mut TestObserver::default())
            .expect("disabled processor should bypass without another plugin call");

        assert_eq!(processed.presentation_frame.handle, after_disabled.handle);
        assert_eq!(
            state.lock().expect("state").submitted_handles.len(),
            5,
            "disabled pre-check should skip the sixth submit"
        );
        assert_eq!(chain.metrics().disabled_processor_count, 1);
        let events = chain.drain_events();
        let disabled = frame_processor_warnings(&events, FrameProcessorWarningKind::Disabled);
        assert_eq!(disabled.len(), 1);
        assert_eq!(
            disabled[0].policy_action,
            FrameProcessorPolicyAction::DisableProcessor
        );
    }

    #[test]
    fn disabled_processor_fails_require_mode_before_next_submit() {
        let state = Arc::new(Mutex::new(TestState {
            submit_error: Some("submit exploded".to_owned()),
            ..TestState::default()
        }));
        let mut chain = chain(FrameProcessorMode::RequireProcessed, state.clone());

        for index in 0..5 {
            let error = chain
                .process(
                    decoder_frame(50 + index, Some(500_000 + index as i64)),
                    &mut TestObserver::default(),
                )
                .expect_err("require mode should fail plugin errors");
            assert!(error.error.to_string().contains("submit exploded"));
        }
        let error = chain
            .process(
                decoder_frame(60, Some(600_000)),
                &mut TestObserver::default(),
            )
            .expect_err("disabled strict processor should fail before submit");

        assert!(error.error.to_string().contains("disabled by policy"));
        assert_eq!(state.lock().expect("state").submitted_handles.len(), 5);
        let events = chain.drain_events();
        assert_eq!(
            frame_processor_warnings(&events, FrameProcessorWarningKind::Disabled).len(),
            1
        );
    }

    #[test]
    fn rejected_frame_fails_in_strict_mode() {
        let state = Arc::new(Mutex::new(TestState {
            submit_status: Some(FrameProcessorSubmitStatus::Rejected),
            ..TestState::default()
        }));
        let mut chain = chain(FrameProcessorMode::RequireProcessed, state);

        let error = chain
            .process(
                decoder_frame(15, Some(160_000)),
                &mut TestObserver::default(),
            )
            .expect_err("strict mode should fail");

        assert_eq!(error.decoder_frame.handle, 15);
        assert!(error.error.to_string().contains("strict mode"));
        let events = chain.drain_events();
        assert!(events.iter().any(|event| matches!(
            event,
            PlayerRuntimeEvent::Warning(PlayerRuntimeWarning::FrameProcessor(warning))
                if warning.kind == FrameProcessorWarningKind::Unsupported
                    && warning.policy_action == FrameProcessorPolicyAction::FailPlayback
                    && warning.processor_index == 0
        )));
    }

    #[test]
    fn pending_output_fails_in_strict_mode_with_original_decoder_frame() {
        let state = Arc::new(Mutex::new(TestState {
            receive_outputs: VecDeque::from([FrameProcessorReceiveOutput::Pending]),
            ..TestState::default()
        }));
        let mut chain = chain(FrameProcessorMode::RequireProcessed, state);

        let error = chain
            .process(
                decoder_frame(16, Some(160_000)),
                &mut TestObserver::default(),
            )
            .expect_err("strict mode should fail on pending output");

        assert_eq!(error.decoder_frame.handle, 16);
        assert!(error.error.to_string().contains("ready frame"));
    }

    #[test]
    fn late_output_is_released_and_dropped() {
        let input = decoder_frame(11, Some(66_000));
        let state = Arc::new(Mutex::new(TestState {
            receive_outputs: VecDeque::from([output_frame(&input, 2_000, true, Some(25_000))]),
            ..TestState::default()
        }));
        let mut chain = chain(FrameProcessorMode::PreferProcessed, state.clone());
        let mut observer = TestObserver::default();

        let processed = chain
            .process(input, &mut observer)
            .expect("late output should bypass");

        assert_eq!(processed.presentation_frame.handle, 11);
        assert!(processed.processor_outputs.is_empty());
        assert_eq!(chain.metrics().deadline_miss_count, 1);
        assert_eq!(chain.metrics().late_output_drop_count, 1);
        assert_eq!(chain.metrics().dropped_output_count, 1);
        assert_eq!(observer.deadline_misses, 1);
        assert_eq!(observer.dropped_outputs, 1);
        assert_eq!(state.lock().expect("state").released_handles, vec![2_011]);
    }

    #[test]
    fn over_budget_output_is_released_and_bypassed() {
        let input = decoder_frame(21, Some(120_000));
        let state = Arc::new(Mutex::new(TestState {
            submit_queue_depth: Some(2),
            receive_outputs: VecDeque::from([output_frame(&input, 4_000, true, None)]),
            ..TestState::default()
        }));
        let mut chain = chain(FrameProcessorMode::PreferProcessed, state.clone());
        let mut observer = TestObserver::default();

        let processed = chain
            .process(input, &mut observer)
            .expect("over-budget output should fall back to the decoder frame");

        assert_eq!(processed.presentation_frame.handle, 21);
        assert!(processed.processor_outputs.is_empty());
        assert_eq!(chain.metrics().backpressure_count, 1);
        assert_eq!(chain.metrics().dropped_output_count, 1);
        assert_eq!(observer.backpressure, 1);
        assert_eq!(observer.dropped_outputs, 1);
        assert_eq!(state.lock().expect("state").released_handles, vec![4_021]);
        let events = chain.drain_events();
        assert!(events.iter().any(|event| matches!(
            event,
            PlayerRuntimeEvent::Warning(PlayerRuntimeWarning::FrameProcessor(warning))
                if warning.kind == FrameProcessorWarningKind::OutputDropped
                    && warning.policy_action == FrameProcessorPolicyAction::DropOutput
                    && warning.queue_depth == Some(2)
        )));
    }

    #[test]
    fn strict_deadline_failure_releases_output_and_returns_decoder_frame() {
        let input = decoder_frame(12, Some(99_000));
        let state = Arc::new(Mutex::new(TestState {
            receive_outputs: VecDeque::from([output_frame(&input, 3_000, true, Some(17_000))]),
            ..TestState::default()
        }));
        let mut chain = chain(FrameProcessorMode::RequireProcessed, state.clone());

        let error = chain
            .process(input, &mut TestObserver::default())
            .expect_err("strict mode should fail");

        assert_eq!(error.decoder_frame.handle, 12);
        assert_eq!(state.lock().expect("state").released_handles, vec![3_012]);
    }

    #[test]
    fn repeated_deadline_misses_disable_and_skip_later_submits() {
        let mut receive_outputs = VecDeque::new();
        for index in 0..5 {
            let input = decoder_frame(70 + index, Some(700_000 + index as i64));
            receive_outputs.push_back(output_frame(&input, 1_000, false, Some(17_000)));
        }
        receive_outputs.push_back(output_frame(
            &decoder_frame(80, Some(800_000)),
            1_000,
            false,
            Some(17_000),
        ));
        let state = Arc::new(Mutex::new(TestState {
            receive_outputs,
            ..TestState::default()
        }));
        let mut chain = chain(FrameProcessorMode::PreferProcessed, state.clone());

        for index in 0..5 {
            let input = decoder_frame(70 + index, Some(700_000 + index as i64));
            let processed = chain
                .process(input.clone(), &mut TestObserver::default())
                .expect("deadline miss should remain recoverable in prefer mode");
            if index < 4 {
                assert_ne!(processed.presentation_frame.handle, input.handle);
            } else {
                assert_eq!(
                    processed.presentation_frame.handle, input.handle,
                    "the miss that trips the breaker should bypass original"
                );
            }
        }
        let after_disabled = decoder_frame(81, Some(810_000));
        let processed = chain
            .process(after_disabled.clone(), &mut TestObserver::default())
            .expect("disabled processor should bypass later frames");

        assert_eq!(processed.presentation_frame.handle, after_disabled.handle);
        assert_eq!(state.lock().expect("state").submitted_handles.len(), 5);
        assert_eq!(chain.metrics().deadline_miss_count, 5);
        assert_eq!(chain.metrics().disabled_processor_count, 1);

        let events = chain.drain_events();
        let deadline = frame_processor_warnings(&events, FrameProcessorWarningKind::DeadlineMissed);
        assert_eq!(deadline.len(), 5);
        assert_eq!(deadline[4].consecutive_miss_count, Some(5));
        assert_eq!(
            deadline[4].policy_action,
            FrameProcessorPolicyAction::DisableProcessor
        );
        let disabled = frame_processor_warnings(&events, FrameProcessorWarningKind::Disabled);
        assert_eq!(disabled.len(), 1);
        assert_eq!(disabled[0].consecutive_miss_count, Some(5));
    }

    #[test]
    fn release_failure_propagates_without_double_release() {
        let input = decoder_frame(18, Some(99_000));
        let state = Arc::new(Mutex::new(TestState {
            receive_outputs: VecDeque::from([output_frame(&input, 1_000, true, None)]),
            release_error: Some("release failed".to_owned()),
            ..TestState::default()
        }));
        let mut chain = chain(FrameProcessorMode::PreferProcessed, state);
        let processed = chain
            .process(input, &mut TestObserver::default())
            .expect("process should succeed");

        let error = chain
            .release_processor_outputs(processed.processor_outputs)
            .expect_err("release should fail");

        assert!(error.to_string().contains("release_frame failed"));
    }

    #[test]
    fn flush_and_close_forward_to_sessions() {
        let state = Arc::new(Mutex::new(TestState::default()));
        let mut chain = chain(FrameProcessorMode::DiagnosticsOnly, state.clone());

        chain.flush().expect("flush");
        chain.close().expect("close");
        chain.close().expect("second close should be idempotent");
        drop(chain);

        let state = state.lock().expect("state");
        assert_eq!(state.flush_count, 1);
        assert_eq!(state.close_count, 1);
    }

    #[test]
    fn helpers_cover_duration_deadline_and_conversion() {
        assert_eq!(duration_us_i64(Duration::from_millis(16)), 16_000);
        assert_eq!(
            present_deadline_us(Some(1_000), Duration::from_millis(16)),
            Some(17_000)
        );
        let frame = decoder_frame(22, Some(44_000));
        let native = decoder_frame_to_native_frame(&frame);
        let round_trip = native_frame_to_decoder_frame(&native);
        assert_eq!(round_trip, frame);
        assert!(output_frame_requires_processor_release(&native));
    }

    #[derive(Debug, Default)]
    struct PipelineTestState {
        packet_reads: VecDeque<NativeFramePacketRead>,
        packet_read_error: Option<String>,
        selected_video_stream_index: Option<u32>,
        sent_packet_streams: Vec<u32>,
        released_frames: Vec<(usize, bool)>,
        submitted_handles: Vec<u64>,
        flush_events: Vec<&'static str>,
        seek_events: Vec<Duration>,
        close_events: Vec<&'static str>,
        receive_outputs: VecDeque<DecoderReceiveNativeFrameOutput>,
        presenter_accepts: bool,
        presenter_requires_host_release: bool,
        decoder_accepts_packets: bool,
        decoder_send_error: Option<String>,
        decoder_receive_error: Option<String>,
        decoder_release_error: Option<String>,
    }

    #[derive(Debug)]
    struct PipelinePacketSource {
        state: Arc<Mutex<PipelineTestState>>,
    }

    impl NativeFramePacketSourceAdapter for PipelinePacketSource {
        fn selected_video_stream_index(&self) -> Option<u32> {
            self.state
                .lock()
                .expect("pipeline state")
                .selected_video_stream_index
        }

        fn read_packet(&mut self) -> Result<NativeFramePacketRead, NativeFramePipelineError> {
            let mut state = self.state.lock().expect("pipeline state");
            if let Some(message) = state.packet_read_error.clone() {
                return Err(NativeFramePipelineError::new("readPacket", message));
            }
            Ok(state
                .packet_reads
                .pop_front()
                .unwrap_or(NativeFramePacketRead::NeedMoreData { message: None }))
        }

        fn flush(&mut self) -> Result<(), NativeFramePipelineError> {
            self.state
                .lock()
                .expect("pipeline state")
                .flush_events
                .push("packet");
            Ok(())
        }

        fn seek(&mut self, position: Duration) -> Result<(), NativeFramePipelineError> {
            self.state
                .lock()
                .expect("pipeline state")
                .seek_events
                .push(position);
            Ok(())
        }

        fn close(&mut self) -> Result<(), NativeFramePipelineError> {
            self.state
                .lock()
                .expect("pipeline state")
                .close_events
                .push("packet");
            Ok(())
        }
    }

    #[derive(Debug)]
    struct PipelineDecoder {
        state: Arc<Mutex<PipelineTestState>>,
    }

    impl NativeFrameDecoderAdapter for PipelineDecoder {
        fn send_packet(
            &mut self,
            packet: &DecoderPacket,
            _data: &[u8],
        ) -> Result<DecoderPacketResult, NativeFramePipelineError> {
            let mut state = self.state.lock().expect("pipeline state");
            state.sent_packet_streams.push(packet.stream_index);
            if let Some(message) = state.decoder_send_error.clone() {
                return Err(NativeFramePipelineError::new("sendDecoderPacket", message));
            }
            Ok(DecoderPacketResult {
                accepted: state.decoder_accepts_packets,
            })
        }

        fn receive_native_frame(
            &mut self,
        ) -> Result<DecoderReceiveNativeFrameOutput, NativeFramePipelineError> {
            let mut state = self.state.lock().expect("pipeline state");
            if let Some(message) = state.decoder_receive_error.clone() {
                return Err(NativeFramePipelineError::new(
                    "receiveDecoderFrame",
                    message,
                ));
            }
            Ok(state
                .receive_outputs
                .pop_front()
                .unwrap_or(DecoderReceiveNativeFrameOutput::NeedMoreInput))
        }

        fn release_native_frame(
            &mut self,
            frame: DecoderNativeFrame,
            presented: bool,
        ) -> Result<(), NativeFramePipelineError> {
            let mut state = self.state.lock().expect("pipeline state");
            if let Some(message) = state.decoder_release_error.clone() {
                return Err(NativeFramePipelineError::new(
                    "releaseDecoderFrame",
                    message,
                ));
            }
            state.released_frames.push((frame.handle, presented));
            Ok(())
        }

        fn flush(&mut self) -> Result<(), NativeFramePipelineError> {
            self.state
                .lock()
                .expect("pipeline state")
                .flush_events
                .push("decoder");
            Ok(())
        }

        fn close(&mut self) -> Result<(), NativeFramePipelineError> {
            self.state
                .lock()
                .expect("pipeline state")
                .close_events
                .push("decoder");
            Ok(())
        }
    }

    #[derive(Debug)]
    struct PipelinePresenter {
        state: Arc<Mutex<PipelineTestState>>,
    }

    impl NativeFramePresenterAdapter for PipelinePresenter {
        fn submit_frame(
            &mut self,
            frame: &NativeFramePresenterFrame,
        ) -> Result<NativeFramePresenterSubmitResult, NativeFramePipelineError> {
            let mut state = self.state.lock().expect("pipeline state");
            state.submitted_handles.push(frame.frame_handle);
            Ok(NativeFramePresenterSubmitResult {
                accepted: state.presenter_accepts,
                requires_host_release: state.presenter_requires_host_release,
                message: Some("presenter submit".to_owned()),
            })
        }

        fn decoder_device_context(&self) -> Option<DecoderNativeDeviceContext> {
            Some(DecoderNativeDeviceContext::Unknown {
                name: "test".to_owned(),
            })
        }

        fn flush(&mut self) -> Result<(), NativeFramePipelineError> {
            self.state
                .lock()
                .expect("pipeline state")
                .flush_events
                .push("presenter");
            Ok(())
        }

        fn close(&mut self) -> Result<(), NativeFramePipelineError> {
            self.state
                .lock()
                .expect("pipeline state")
                .close_events
                .push("presenter");
            Ok(())
        }
    }

    fn source_packet(
        media_kind: SourceNormalizerPacketMediaKind,
        stream_index: u32,
    ) -> NativeFramePacketRead {
        NativeFramePacketRead::Packet {
            packet: SourceNormalizerPacket {
                stream_index,
                media_kind,
                pts_us: Some(1_000),
                dts_us: Some(1_000),
                duration_us: Some(33_333),
                key_frame: true,
                discontinuity: false,
                sample_rate: None,
                channels: None,
                channel_layout: None,
                sample_format: None,
                frame_count: None,
                end_of_stream: false,
            },
            data: vec![1, 2, 3, 4],
            message: None,
        }
    }

    fn pipeline_core(state: Arc<Mutex<PipelineTestState>>) -> NativeFramePipelineCore {
        NativeFramePipelineCore::with_components(
            NativeFramePipelineCoreConfig::default(),
            Some(Box::new(PipelinePacketSource {
                state: state.clone(),
            })),
            Some(Box::new(PipelineDecoder {
                state: state.clone(),
            })),
            None,
            Some(Box::new(PipelinePresenter { state })),
        )
    }

    #[test]
    fn pipeline_core_filters_packets_and_presents_with_host_release() {
        let state = Arc::new(Mutex::new(PipelineTestState {
            selected_video_stream_index: Some(2),
            packet_reads: VecDeque::from([
                source_packet(SourceNormalizerPacketMediaKind::Audio, 1),
                source_packet(SourceNormalizerPacketMediaKind::Video, 7),
                source_packet(SourceNormalizerPacketMediaKind::Video, 2),
            ]),
            receive_outputs: VecDeque::from([DecoderReceiveNativeFrameOutput::Frame(
                decoder_frame(40, Some(1_000)),
            )]),
            presenter_accepts: true,
            presenter_requires_host_release: true,
            decoder_accepts_packets: true,
            ..PipelineTestState::default()
        }));
        let mut core = pipeline_core(state.clone());
        core.set_output_target_attached(true);

        let output = core.advance().expect("advance");

        assert_eq!(output.status, NativeFramePipelineFrameStatus::Frame);
        assert!(output.requires_host_release);
        assert_eq!(output.handle, Some(1));
        let snapshot = core.status_snapshot();
        assert_eq!(snapshot.pending_frames, 1);
        assert_eq!(snapshot.counters.skipped_audio_packets, 1);
        assert_eq!(snapshot.counters.skipped_video_packets, 1);
        assert_eq!(snapshot.counters.source_packets_read, 1);
        assert_eq!(snapshot.counters.decoder_packets_sent, 1);
        assert_eq!(
            state.lock().expect("pipeline state").sent_packet_streams,
            vec![2]
        );

        core.release_frame(output.handle.unwrap(), true)
            .expect("host release");
        assert_eq!(core.counters().presented_frames, 1);
        assert_eq!(
            state.lock().expect("pipeline state").released_frames,
            vec![(40, true)]
        );
    }

    #[test]
    fn pipeline_core_bounds_non_video_packet_skips_per_advance() {
        let packet_budget = NativeFramePipelineCoreConfig::default().packet_budget;
        let packet_count = packet_budget.saturating_mul(4);
        let state = Arc::new(Mutex::new(PipelineTestState {
            packet_reads: (0..packet_count)
                .map(|_| source_packet(SourceNormalizerPacketMediaKind::Audio, 1))
                .collect(),
            presenter_accepts: true,
            decoder_accepts_packets: true,
            ..PipelineTestState::default()
        }));
        let mut core = pipeline_core(state.clone());

        let output = core.advance().expect("packet skip budget is recoverable");
        let remaining_packets = state.lock().expect("pipeline state").packet_reads.len();
        let skipped_audio_packets = core.counters().skipped_audio_packets;
        core.close().expect("close pipeline after bounded advance");

        assert_eq!(output.status, NativeFramePipelineFrameStatus::Pending);
        assert_eq!(remaining_packets, packet_count - packet_budget);
        assert_eq!(
            skipped_audio_packets,
            u64::try_from(packet_budget).expect("packet budget fits u64")
        );
    }

    #[test]
    fn pipeline_core_flush_releases_before_flush_order() {
        let state = Arc::new(Mutex::new(PipelineTestState {
            packet_reads: VecDeque::from([source_packet(
                SourceNormalizerPacketMediaKind::Video,
                0,
            )]),
            receive_outputs: VecDeque::from([DecoderReceiveNativeFrameOutput::Frame(
                decoder_frame(41, Some(1_000)),
            )]),
            presenter_accepts: true,
            presenter_requires_host_release: true,
            decoder_accepts_packets: true,
            ..PipelineTestState::default()
        }));
        let mut core = pipeline_core(state.clone());
        core.set_output_target_attached(true);
        let output = core.advance().expect("advance");
        assert_eq!(output.handle, Some(1));

        core.flush().expect("flush");

        let state = state.lock().expect("pipeline state");
        assert_eq!(state.released_frames, vec![(41, false)]);
        assert_eq!(state.flush_events, vec!["decoder", "presenter", "packet"]);
        assert_eq!(core.counters().flush_count, 1);
        assert_eq!(core.counters().released_frames, 1);
        assert_eq!(core.status_snapshot().pending_frames, 0);
    }

    #[test]
    fn pipeline_core_rejects_stale_host_release_after_seek_epoch() {
        let state = Arc::new(Mutex::new(PipelineTestState {
            packet_reads: VecDeque::from([source_packet(
                SourceNormalizerPacketMediaKind::Video,
                0,
            )]),
            receive_outputs: VecDeque::from([DecoderReceiveNativeFrameOutput::Frame(
                decoder_frame(42, Some(1_000)),
            )]),
            presenter_accepts: true,
            presenter_requires_host_release: true,
            decoder_accepts_packets: true,
            ..PipelineTestState::default()
        }));
        let mut core = pipeline_core(state.clone());
        core.set_output_target_attached(true);
        let output = core.advance().expect("advance");
        let handle = output.handle.unwrap();

        core.seek(Duration::from_millis(500)).expect("seek");
        let error = core
            .release_frame(handle, true)
            .expect_err("stale handle is no longer pending");

        assert!(error.message.contains("invalid"));
        assert_eq!(core.counters().seek_count, 1);
        assert_eq!(core.counters().presented_frames, 0);
        assert_eq!(
            state.lock().expect("pipeline state").released_frames,
            vec![(42, false)]
        );
    }

    #[test]
    fn protected_packet_source_need_more_data_does_not_trip_breaker() {
        let state = Arc::new(Mutex::new(PipelineTestState {
            packet_reads: VecDeque::from([
                NativeFramePacketRead::NeedMoreData {
                    message: Some("warming".to_owned()),
                },
                NativeFramePacketRead::NeedMoreData {
                    message: Some("still warming".to_owned()),
                },
                NativeFramePacketRead::NeedMoreData { message: None },
                NativeFramePacketRead::NeedMoreData { message: None },
                NativeFramePacketRead::NeedMoreData { message: None },
                NativeFramePacketRead::NeedMoreData { message: None },
            ]),
            presenter_accepts: true,
            decoder_accepts_packets: true,
            ..PipelineTestState::default()
        }));
        let mut core = pipeline_core(state);

        for _ in 0..6 {
            let output = core.advance().expect("need-more-data is recoverable");
            assert_eq!(output.status, NativeFramePipelineFrameStatus::Pending);
        }
    }

    #[test]
    fn protected_packet_source_disables_after_repeated_read_errors() {
        let state = Arc::new(Mutex::new(PipelineTestState {
            packet_read_error: Some("read exploded".to_owned()),
            presenter_accepts: true,
            decoder_accepts_packets: true,
            ..PipelineTestState::default()
        }));
        let mut core = pipeline_core(state);

        for _ in 0..5 {
            let error = core.advance().expect_err("read error should propagate");
            assert!(error.message.contains("read exploded"));
        }
        let error = core
            .advance()
            .expect_err("disabled packet source should short-circuit");

        assert_eq!(error.operation, "readPacket");
        assert!(error.message.contains("packet source disabled"));
    }

    #[test]
    fn protected_decoder_disables_after_repeated_send_errors() {
        let state = Arc::new(Mutex::new(PipelineTestState {
            packet_reads: VecDeque::from([source_packet(
                SourceNormalizerPacketMediaKind::Video,
                0,
            )]),
            presenter_accepts: true,
            decoder_accepts_packets: true,
            decoder_send_error: Some("send exploded".to_owned()),
            ..PipelineTestState::default()
        }));
        let mut core = pipeline_core(state.clone());
        core.set_output_target_attached(true);

        for _ in 0..5 {
            let error = core.advance().expect_err("send error should propagate");
            assert!(error.message.contains("send exploded"));
        }
        let error = core
            .advance()
            .expect_err("disabled decoder should short-circuit sends");

        assert_eq!(error.operation, "sendDecoderPacket");
        assert!(error.message.contains("decoder disabled"));
        assert_eq!(
            state
                .lock()
                .expect("pipeline state")
                .sent_packet_streams
                .len(),
            5
        );
    }

    #[test]
    fn successful_decoder_sends_reset_receive_failures() {
        let packets = (0..6)
            .map(|_| source_packet(SourceNormalizerPacketMediaKind::Video, 0))
            .collect();
        let state = Arc::new(Mutex::new(PipelineTestState {
            packet_reads: packets,
            presenter_accepts: true,
            decoder_accepts_packets: true,
            decoder_receive_error: Some("receive exploded".to_owned()),
            ..PipelineTestState::default()
        }));
        let mut core = pipeline_core(state.clone());
        core.set_output_target_attached(true);

        for _ in 0..6 {
            let error = core.advance().expect_err("receive error should propagate");
            assert!(error.message.contains("receive exploded"));
        }

        assert_eq!(
            state
                .lock()
                .expect("pipeline state")
                .sent_packet_streams
                .len(),
            6
        );
    }

    #[test]
    fn protected_decoder_successes_reset_consecutive_failure_count() {
        let state = Arc::new(Mutex::new(PipelineTestState {
            decoder_accepts_packets: true,
            ..PipelineTestState::default()
        }));
        let mut decoder = ProtectedNativeFrameDecoderAdapter::new(Box::new(PipelineDecoder {
            state: state.clone(),
        }));
        let packet = DecoderPacket::default();

        for _ in 0..6 {
            state.lock().expect("pipeline state").decoder_send_error =
                Some("send exploded".to_owned());
            let error = decoder
                .send_packet(&packet, &[])
                .expect_err("send error should propagate");
            assert!(error.message.contains("send exploded"));

            state.lock().expect("pipeline state").decoder_send_error = None;
            decoder
                .send_packet(&packet, &[])
                .expect("successful send should reset failures");

            state.lock().expect("pipeline state").decoder_receive_error =
                Some("receive exploded".to_owned());
            let error = decoder
                .receive_native_frame()
                .expect_err("receive error should propagate");
            assert!(error.message.contains("receive exploded"));

            state.lock().expect("pipeline state").decoder_receive_error = None;
            assert_eq!(
                decoder
                    .receive_native_frame()
                    .expect("need-more-input should reset failures"),
                DecoderReceiveNativeFrameOutput::NeedMoreInput
            );
        }

        assert!(!decoder.breaker.is_disabled());
        assert_eq!(decoder.breaker.consecutive_failures(), 0);
    }
}
