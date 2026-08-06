#![deny(unsafe_code)]

use std::collections::VecDeque;
use std::thread;
use std::time::Duration;

use player_plugin::{
    DecoderFrameFormat, DecoderMediaKind, FrameProcessorCapabilities, FrameProcessorError,
    FrameProcessorFrameTimings, FrameProcessorInputFrame, FrameProcessorOutputFrame,
    FrameProcessorPluginFactory, FrameProcessorReceiveOutput, FrameProcessorSession,
    FrameProcessorSessionConfig, FrameProcessorSessionInfo, FrameProcessorSubmitFrame,
    FrameProcessorSubmitResult, FrameProcessorSubmitStatus, NativeFrame, NativeFrameMetadata,
    NativeFramePipelineProfile, NativeHandleKind, Plugin, PluginBuildError,
};

const PLUGIN_ID: &str = "dev.vesper.frame-processor-diagnostic";
const INSTANCE_ID: &str = "dev.vesper.frame-processor-diagnostic.frame";
const PLUGIN_NAME: &str = "player-frame-processor-diagnostic";
const MODE_ENV: &str = "VESPER_FRAME_PROCESSOR_DIAGNOSTIC_MODE";
const SLOW_DELAY_MS_ENV: &str = "VESPER_FRAME_PROCESSOR_DIAGNOSTIC_SLOW_MS";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DiagnosticMode {
    Noop,
    Slow,
    UnsupportedHandle,
    LateOutput,
    Marker,
}

impl DiagnosticMode {
    fn from_env() -> Self {
        match std::env::var(MODE_ENV)
            .unwrap_or_default()
            .to_ascii_lowercase()
            .as_str()
        {
            "slow" => Self::Slow,
            "unsupported-handle" | "unsupported" => Self::UnsupportedHandle,
            "late-output" | "late" => Self::LateOutput,
            "marker" | "debug-marker" => Self::Marker,
            _ => Self::Noop,
        }
    }
}

#[derive(Debug, Default)]
struct DiagnosticFrameProcessorFactory;

impl FrameProcessorPluginFactory for DiagnosticFrameProcessorFactory {
    fn name(&self) -> &str {
        PLUGIN_NAME
    }

    fn capabilities(&self) -> FrameProcessorCapabilities {
        diagnostic_capabilities(DiagnosticMode::from_env())
    }

    fn open_session(
        &self,
        config: &FrameProcessorSessionConfig,
    ) -> Result<Box<dyn FrameProcessorSession>, FrameProcessorError> {
        let mode = DiagnosticMode::from_env();
        if !diagnostic_capabilities(mode).supports_input_metadata(&config.input_metadata) {
            return Err(FrameProcessorError::unsupported_handle(format!(
                "{:?}",
                config.input_metadata.effective_pipeline_profile()
            )));
        }

        Ok(Box::new(DiagnosticSession::new(
            mode,
            &config.input_metadata,
        )))
    }
}

#[derive(Debug)]
struct PendingOutput {
    frame: NativeFrame,
    source_frame_id: Option<u64>,
}

#[derive(Debug)]
struct DiagnosticSession {
    mode: DiagnosticMode,
    info: FrameProcessorSessionInfo,
    pending_outputs: VecDeque<PendingOutput>,
    counters: DiagnosticCounters,
}

impl DiagnosticSession {
    fn new(mode: DiagnosticMode, input_metadata: &NativeFrameMetadata) -> Self {
        Self {
            mode,
            info: FrameProcessorSessionInfo {
                processor_name: Some(PLUGIN_NAME.to_owned()),
                selected_backend: Some(format!("{mode:?}")),
                output_handle_kind: Some(input_metadata.handle_kind.clone()),
                output_pipeline_profile: Some(input_metadata.effective_pipeline_profile()),
                max_in_flight_frames: Some(1),
            },
            pending_outputs: VecDeque::new(),
            counters: DiagnosticCounters::default(),
        }
    }

    fn pending_count(&self) -> u32 {
        if self.pending_outputs.is_empty() {
            0
        } else {
            1
        }
    }
}

impl FrameProcessorSession for DiagnosticSession {
    fn session_info(&self) -> FrameProcessorSessionInfo {
        self.info.clone()
    }

    fn submit_frame(
        &mut self,
        frame: FrameProcessorInputFrame<'_>,
        _submit: &FrameProcessorSubmitFrame,
    ) -> Result<FrameProcessorSubmitResult, FrameProcessorError> {
        if frame.native_handle() == 0 {
            return Err(FrameProcessorError::abi_violation(
                "input frame handle must not be null",
            ));
        }
        if !diagnostic_capabilities(self.mode).supports_input_metadata(frame.metadata()) {
            self.counters.rejected = self.counters.rejected.saturating_add(1);
            return Ok(FrameProcessorSubmitResult {
                status: FrameProcessorSubmitStatus::Rejected,
                queue_depth: Some(self.pending_count()),
                in_flight_frames: Some(self.pending_count()),
                message: Some("unsupported input handle kind".to_owned()),
            });
        }
        if !self.pending_outputs.is_empty() {
            self.counters.backpressure = self.counters.backpressure.saturating_add(1);
            return Ok(FrameProcessorSubmitResult {
                status: FrameProcessorSubmitStatus::Backpressure,
                queue_depth: Some(self.pending_count()),
                in_flight_frames: Some(self.pending_count()),
                message: Some("diagnostic output is still pending".to_owned()),
            });
        }

        if self.mode == DiagnosticMode::Slow {
            thread::sleep(Duration::from_millis(slow_delay_ms()));
        }

        let source_frame_id = frame.metadata().frame_id;
        let output = allocate_output_frame(&frame);
        self.counters.submitted = self.counters.submitted.saturating_add(1);
        self.pending_outputs.push_back(PendingOutput {
            frame: output,
            source_frame_id,
        });

        Ok(FrameProcessorSubmitResult {
            status: FrameProcessorSubmitStatus::Accepted,
            queue_depth: Some(self.pending_count()),
            in_flight_frames: Some(self.pending_count()),
            message: None,
        })
    }

    fn receive_frame(&mut self) -> Result<FrameProcessorReceiveOutput, FrameProcessorError> {
        let Some(output) = self.pending_outputs.pop_front() else {
            return Ok(FrameProcessorReceiveOutput::Pending);
        };
        let process_time_us = match self.mode {
            DiagnosticMode::Slow => slow_delay_ms().saturating_mul(1_000),
            DiagnosticMode::LateOutput => 1_000_000,
            DiagnosticMode::Noop | DiagnosticMode::UnsupportedHandle | DiagnosticMode::Marker => {
                100
            }
        };
        self.counters.record_process_time(process_time_us);
        let message = if self.mode == DiagnosticMode::LateOutput {
            "diagnostic output intentionally reports late timing".to_owned()
        } else if self.mode == DiagnosticMode::Marker {
            format!("debug marker metadata-only; {}", self.counters.summary())
        } else {
            self.counters.summary()
        };

        Ok(FrameProcessorReceiveOutput::Frame(
            FrameProcessorOutputFrame {
                frame: output.frame,
                timings: FrameProcessorFrameTimings {
                    queue_wait_us: Some(0),
                    process_time_us: Some(process_time_us),
                    submit_to_ready_us: Some(process_time_us),
                },
                source_frame_id: output.source_frame_id,
                message: Some(message),
            },
        ))
    }

    fn release_frame(&mut self, frame: NativeFrame) -> Result<(), FrameProcessorError> {
        if frame.handle == 0 {
            return Err(FrameProcessorError::abi_violation(
                "release_frame handle must not be null",
            ));
        }
        if frame
            .metadata
            .release_tracking
            .as_ref()
            .is_some_and(|tracking| !tracking.requires_release)
        {
            return Err(FrameProcessorError::abi_violation(
                "borrowed passthrough frame must not be released",
            ));
        }
        self.counters.released = self.counters.released.saturating_add(1);
        Ok(())
    }

    fn flush(&mut self) -> Result<(), FrameProcessorError> {
        self.pending_outputs.clear();
        Ok(())
    }

    fn close(&mut self) -> Result<(), FrameProcessorError> {
        self.pending_outputs.clear();
        Ok(())
    }
}

#[derive(Debug, Clone, Default)]
struct DiagnosticCounters {
    submitted: u64,
    processed: u64,
    bypassed: u64,
    rejected: u64,
    backpressure: u64,
    late_dropped: u64,
    released: u64,
    total_process_time_us: u64,
    max_process_time_us: u64,
}

impl DiagnosticCounters {
    fn record_process_time(&mut self, process_time_us: u64) {
        self.processed = self.processed.saturating_add(1);
        self.total_process_time_us = self.total_process_time_us.saturating_add(process_time_us);
        self.max_process_time_us = self.max_process_time_us.max(process_time_us);
    }

    fn summary(&self) -> String {
        let average_process_time_us = if self.processed == 0 {
            0
        } else {
            self.total_process_time_us / self.processed
        };
        format!(
            "processed={} bypassed={} rejected={} backpressure={} lateDropped={} released={} avgProcessUs={} maxProcessUs={}",
            self.processed,
            self.bypassed,
            self.rejected,
            self.backpressure,
            self.late_dropped,
            self.released,
            average_process_time_us,
            self.max_process_time_us
        )
    }
}

fn diagnostic_capabilities(mode: DiagnosticMode) -> FrameProcessorCapabilities {
    FrameProcessorCapabilities {
        accepted_input_handle_kinds: match mode {
            DiagnosticMode::UnsupportedHandle => vec![NativeHandleKind::D3D11Texture2D],
            _ => vec![
                NativeHandleKind::CvPixelBuffer,
                NativeHandleKind::IoSurface,
                NativeHandleKind::D3D11Texture2D,
                NativeHandleKind::MediaCodecHardwareBuffer,
                NativeHandleKind::MediaCodecSurfaceTexture,
            ],
        },
        output_handle_kinds: vec![
            NativeHandleKind::CvPixelBuffer,
            NativeHandleKind::IoSurface,
            NativeHandleKind::D3D11Texture2D,
            NativeHandleKind::MediaCodecHardwareBuffer,
            NativeHandleKind::MediaCodecSurfaceTexture,
        ],
        accepted_input_pipeline_profiles: match mode {
            DiagnosticMode::UnsupportedHandle => vec![NativeFramePipelineProfile::D3D11Texture2D],
            _ => vec![
                NativeFramePipelineProfile::VideoToolboxCvPixelBuffer,
                NativeFramePipelineProfile::Unknown("io_surface".to_owned()),
                NativeFramePipelineProfile::D3D11Texture2D,
                NativeFramePipelineProfile::MediaCodecHardwareBuffer,
                NativeFramePipelineProfile::MediaCodecSurfaceTexture,
            ],
        },
        output_pipeline_profiles: vec![
            NativeFramePipelineProfile::VideoToolboxCvPixelBuffer,
            NativeFramePipelineProfile::Unknown("io_surface".to_owned()),
            NativeFramePipelineProfile::D3D11Texture2D,
            NativeFramePipelineProfile::MediaCodecHardwareBuffer,
            NativeFramePipelineProfile::MediaCodecSurfaceTexture,
        ],
        supports_video_frames: true,
        supports_in_place_passthrough: true,
        preserves_dimensions: true,
        may_change_dimensions: false,
        preserves_color_metadata: true,
        preserves_hdr_metadata: true,
        supports_flush: true,
        max_sessions: Some(1),
        max_in_flight_frames: Some(1),
    }
}

fn allocate_output_frame(frame: &FrameProcessorInputFrame<'_>) -> NativeFrame {
    let mut output = frame.borrowed_passthrough();
    output.metadata.frame_id = output
        .metadata
        .frame_id
        .or_else(|| u64::try_from(output.handle).ok());
    if let Some(tracking) = output.metadata.release_tracking.as_mut() {
        tracking.frame_id = output.metadata.frame_id;
    }
    output
}

fn slow_delay_ms() -> u64 {
    std::env::var(SLOW_DELAY_MS_ENV)
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(50)
}

#[allow(dead_code)]
fn sample_metadata() -> NativeFrameMetadata {
    NativeFrameMetadata {
        media_kind: DecoderMediaKind::Video,
        format: DecoderFrameFormat::Nv12,
        codec: "diagnostic-video".to_owned(),
        pts_us: Some(1_000),
        duration_us: Some(33_333),
        width: 2,
        height: 2,
        coded_width: Some(2),
        coded_height: Some(2),
        visible_rect: None,
        handle_kind: NativeHandleKind::IoSurface,
        pipeline_profile: Some(NativeFramePipelineProfile::Unknown("io_surface".to_owned())),
        color_space: Some("bt709".to_owned()),
        hdr_metadata: None,
        color: None,
        hdr: None,
        sync_info: None,
        transform: None,
        frame_id: Some(1),
        release_tracking: None,
    }
}

#[player_plugin::export]
fn diagnostic_frame_processor_plugin() -> Result<Plugin, PluginBuildError> {
    Plugin::builder(PLUGIN_ID, PLUGIN_NAME)?
        .with_frame_processor(INSTANCE_ID, DiagnosticFrameProcessorFactory)?
        .build()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_frame(handle: usize) -> NativeFrame {
        NativeFrame {
            metadata: sample_metadata(),
            handle,
            lease_token: None,
        }
    }

    #[test]
    fn exports_plugin_entry() {
        assert!(!vesper_plugin_entry().is_null());
    }

    #[test]
    fn default_mode_is_noop() {
        assert_eq!(DiagnosticMode::from_env(), DiagnosticMode::Noop);
    }

    #[test]
    fn default_slow_delay_is_stable() {
        assert_eq!(slow_delay_ms(), 50);
    }

    #[test]
    fn unsupported_mode_advertises_mismatched_handle() {
        let capabilities = diagnostic_capabilities(DiagnosticMode::UnsupportedHandle);

        assert!(!capabilities.supports_input_handle_kind(&NativeHandleKind::IoSurface));
        assert!(capabilities.supports_input_handle_kind(&NativeHandleKind::D3D11Texture2D));
    }

    #[test]
    fn default_mode_accepts_android_mediacodec_native_frame_handles() {
        let capabilities = diagnostic_capabilities(DiagnosticMode::Noop);

        assert!(
            capabilities.supports_input_handle_kind(&NativeHandleKind::MediaCodecHardwareBuffer)
        );
        assert!(
            capabilities.supports_input_handle_kind(&NativeHandleKind::MediaCodecSurfaceTexture)
        );
        assert!(capabilities.supports_input_pipeline_profile(
            &NativeFramePipelineProfile::MediaCodecHardwareBuffer
        ));
        assert!(capabilities.supports_input_pipeline_profile(
            &NativeFramePipelineProfile::MediaCodecSurfaceTexture
        ));
    }

    #[test]
    fn diagnostic_output_preserves_shape_and_marks_borrowed_passthrough() {
        let input = sample_frame(99);
        let output = allocate_output_frame(&FrameProcessorInputFrame::new(&input));

        assert_eq!(output.metadata.width, input.metadata.width);
        assert_eq!(output.metadata.height, input.metadata.height);
        assert_eq!(output.metadata.handle_kind, input.metadata.handle_kind);
        assert_eq!(output.metadata.frame_id, input.metadata.frame_id);
        assert_eq!(output.handle, 99);
        assert_eq!(
            output
                .metadata
                .release_tracking
                .as_ref()
                .map(|tracking| tracking.requires_release),
            Some(false)
        );
    }

    #[test]
    fn marker_session_preserves_counter_message_and_timing() {
        let input = sample_frame(99);
        let mut session = DiagnosticSession::new(DiagnosticMode::Marker, &input.metadata);
        let submit = FrameProcessorSubmitFrame::new(input.metadata.clone());
        let result = session
            .submit_frame(FrameProcessorInputFrame::new(&input), &submit)
            .expect("submit marker frame");
        assert_eq!(result.status, FrameProcessorSubmitStatus::Accepted);

        let FrameProcessorReceiveOutput::Frame(output) =
            session.receive_frame().expect("receive marker frame")
        else {
            panic!("expected marker output");
        };
        assert_eq!(output.timings.submit_to_ready_us, Some(100));
        assert_eq!(output.source_frame_id, Some(1));
        assert!(
            output
                .message
                .as_deref()
                .is_some_and(|message| message.contains("processed=1"))
        );
    }

    #[test]
    fn diagnostic_session_reports_backpressure_and_flushes_pending_output() {
        let input = sample_frame(99);
        let mut session = DiagnosticSession::new(DiagnosticMode::Noop, &input.metadata);
        let submit = FrameProcessorSubmitFrame::new(input.metadata.clone());
        session
            .submit_frame(FrameProcessorInputFrame::new(&input), &submit)
            .expect("submit first frame");
        let result = session
            .submit_frame(FrameProcessorInputFrame::new(&input), &submit)
            .expect("submit backpressured frame");
        assert_eq!(result.status, FrameProcessorSubmitStatus::Backpressure);

        session.flush().expect("flush pending output");
        assert_eq!(
            session.receive_frame().expect("receive after flush"),
            FrameProcessorReceiveOutput::Pending
        );
    }

    #[test]
    fn diagnostic_counter_summary_reports_processing_state() {
        let mut counters = DiagnosticCounters::default();
        counters.record_process_time(100);
        counters.record_process_time(300);
        counters.backpressure = 1;

        let summary = counters.summary();

        assert!(summary.contains("processed=2"));
        assert!(summary.contains("backpressure=1"));
        assert!(summary.contains("avgProcessUs=200"));
        assert!(summary.contains("maxProcessUs=300"));
    }
}
