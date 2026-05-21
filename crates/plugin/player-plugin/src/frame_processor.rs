use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{NativeFrame, NativeFrameMetadata, NativeHandleKind};

/// Frame metadata and scheduling hints submitted to a frame processor.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FrameProcessorSubmitFrame {
    pub metadata: NativeFrameMetadata,
    #[serde(default)]
    pub present_deadline_us: Option<i64>,
}

impl FrameProcessorSubmitFrame {
    pub fn new(metadata: NativeFrameMetadata) -> Self {
        Self {
            metadata,
            present_deadline_us: None,
        }
    }
}

/// Native-frame capabilities advertised by a frame processor plugin.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct FrameProcessorCapabilities {
    pub accepted_input_handle_kinds: Vec<NativeHandleKind>,
    pub output_handle_kinds: Vec<NativeHandleKind>,
    pub supports_video_frames: bool,
    pub supports_in_place_passthrough: bool,
    pub preserves_dimensions: bool,
    pub may_change_dimensions: bool,
    pub preserves_color_metadata: bool,
    pub preserves_hdr_metadata: bool,
    pub supports_flush: bool,
    pub max_sessions: Option<u32>,
    pub max_in_flight_frames: Option<u32>,
}

impl FrameProcessorCapabilities {
    /// Returns whether the processor accepts an input native handle kind.
    pub fn supports_input_handle_kind(&self, handle_kind: &NativeHandleKind) -> bool {
        self.accepted_input_handle_kinds.is_empty()
            || self
                .accepted_input_handle_kinds
                .iter()
                .any(|candidate| candidate == handle_kind)
    }
}

/// Configuration used to open one frame processor session.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FrameProcessorSessionConfig {
    pub processor_index: usize,
    pub input_metadata: NativeFrameMetadata,
    #[serde(default)]
    pub max_in_flight_frames: Option<u32>,
}

/// Optional session metadata returned after opening a frame processor session.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct FrameProcessorSessionInfo {
    pub processor_name: Option<String>,
    pub selected_backend: Option<String>,
    pub output_handle_kind: Option<NativeHandleKind>,
    pub max_in_flight_frames: Option<u32>,
}

/// Submit state returned after handing a frame to a processor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FrameProcessorSubmitStatus {
    Accepted,
    Bypassed,
    Backpressure,
    Rejected,
}

/// Structured result returned by a submit operation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FrameProcessorSubmitResult {
    pub status: FrameProcessorSubmitStatus,
    #[serde(default)]
    pub queue_depth: Option<u32>,
    #[serde(default)]
    pub in_flight_frames: Option<u32>,
    #[serde(default)]
    pub message: Option<String>,
}

impl Default for FrameProcessorSubmitResult {
    fn default() -> Self {
        Self {
            status: FrameProcessorSubmitStatus::Accepted,
            queue_depth: None,
            in_flight_frames: None,
            message: None,
        }
    }
}

/// Receive state encoded in frame processor output metadata over the C ABI.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FrameProcessorReceiveStatus {
    Frame,
    Pending,
    EndOfStream,
}

/// Timing metadata reported for one processed output.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct FrameProcessorFrameTimings {
    #[serde(default)]
    pub queue_wait_us: Option<u64>,
    #[serde(default)]
    pub process_time_us: Option<u64>,
    #[serde(default)]
    pub submit_to_ready_us: Option<u64>,
}

/// Metadata returned by the dynamic ABI receive call.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FrameProcessorReceiveFrameMetadata {
    pub status: FrameProcessorReceiveStatus,
    #[serde(default)]
    pub frame: Option<NativeFrameMetadata>,
    #[serde(default)]
    pub timings: FrameProcessorFrameTimings,
    #[serde(default)]
    pub source_frame_id: Option<u64>,
    #[serde(default)]
    pub message: Option<String>,
}

impl FrameProcessorReceiveFrameMetadata {
    pub fn frame(frame: NativeFrameMetadata) -> Self {
        Self {
            status: FrameProcessorReceiveStatus::Frame,
            frame: Some(frame),
            timings: FrameProcessorFrameTimings::default(),
            source_frame_id: None,
            message: None,
        }
    }

    pub fn pending() -> Self {
        Self {
            status: FrameProcessorReceiveStatus::Pending,
            frame: None,
            timings: FrameProcessorFrameTimings::default(),
            source_frame_id: None,
            message: None,
        }
    }

    pub fn end_of_stream() -> Self {
        Self {
            status: FrameProcessorReceiveStatus::EndOfStream,
            frame: None,
            timings: FrameProcessorFrameTimings::default(),
            source_frame_id: None,
            message: None,
        }
    }
}

/// Processor-owned output frame returned by a frame processor session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FrameProcessorOutputFrame {
    pub frame: NativeFrame,
    pub timings: FrameProcessorFrameTimings,
    pub source_frame_id: Option<u64>,
}

/// Rust-side receive result returned by frame processor sessions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FrameProcessorReceiveOutput {
    Frame(FrameProcessorOutputFrame),
    Pending,
    EndOfStream,
}

/// Empty success payload used by flush/close operations.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct FrameProcessorOperationStatus {
    pub completed: bool,
}

/// Error payload shared by frame processor plugins and host-side adapters.
#[derive(Debug, Error, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum FrameProcessorError {
    #[error("unsupported native handle kind: {handle_kind}")]
    UnsupportedHandle { handle_kind: String },
    #[error("frame processor payload codec error: {message}")]
    PayloadCodec { message: String },
    #[error("frame processor ABI violation: {message}")]
    AbiViolation { message: String },
    #[error("frame processor session is not configured")]
    NotConfigured,
    #[error("frame processor backpressure: {message}")]
    Backpressure { message: String },
    #[error("frame processor timeout: {message}")]
    Timeout { message: String },
    #[error("frame processor internal error: {message}")]
    Internal { message: String },
}

impl FrameProcessorError {
    pub fn payload_codec(message: impl Into<String>) -> Self {
        Self::PayloadCodec {
            message: message.into(),
        }
    }

    pub fn abi_violation(message: impl Into<String>) -> Self {
        Self::AbiViolation {
            message: message.into(),
        }
    }

    pub fn unsupported_handle(handle_kind: impl Into<String>) -> Self {
        Self::UnsupportedHandle {
            handle_kind: handle_kind.into(),
        }
    }

    pub fn internal(message: impl Into<String>) -> Self {
        Self::Internal {
            message: message.into(),
        }
    }
}

/// Creates frame processor sessions for one plugin.
pub trait FrameProcessorPluginFactory: Send + Sync {
    fn name(&self) -> &str;

    fn capabilities(&self) -> FrameProcessorCapabilities;

    fn open_session(
        &self,
        config: &FrameProcessorSessionConfig,
    ) -> Result<Box<dyn FrameProcessorSession>, FrameProcessorError>;
}

/// Stateful native-frame processor session created by a frame processor plugin.
pub trait FrameProcessorSession: Send {
    fn session_info(&self) -> FrameProcessorSessionInfo;

    fn submit_frame(
        &mut self,
        frame: &NativeFrame,
        submit: &FrameProcessorSubmitFrame,
    ) -> Result<FrameProcessorSubmitResult, FrameProcessorError>;

    fn receive_frame(&mut self) -> Result<FrameProcessorReceiveOutput, FrameProcessorError>;

    fn release_frame(&mut self, frame: NativeFrame) -> Result<(), FrameProcessorError>;

    fn flush(&mut self) -> Result<(), FrameProcessorError>;

    fn close(&mut self) -> Result<(), FrameProcessorError>;
}

#[cfg(test)]
mod tests {
    use super::{
        FrameProcessorCapabilities, FrameProcessorFrameTimings, FrameProcessorReceiveFrameMetadata,
        FrameProcessorReceiveStatus, FrameProcessorSubmitResult, FrameProcessorSubmitStatus,
    };
    use crate::{
        DecoderFrameFormat, DecoderMediaKind, NativeFrameMetadata, NativeHandleKind, VisibleRect,
    };

    fn metadata() -> NativeFrameMetadata {
        NativeFrameMetadata {
            media_kind: DecoderMediaKind::Video,
            format: DecoderFrameFormat::Nv12,
            codec: "h264".to_owned(),
            pts_us: Some(1_000),
            duration_us: Some(16_667),
            width: 1_920,
            height: 1_080,
            coded_width: Some(1_920),
            coded_height: Some(1_088),
            visible_rect: Some(VisibleRect {
                x: 0,
                y: 0,
                width: 1_920,
                height: 1_080,
            }),
            handle_kind: NativeHandleKind::CvPixelBuffer,
            frame_id: Some(42),
            release_tracking: None,
        }
    }

    #[test]
    fn frame_processor_submit_result_round_trips_through_json() {
        let result = FrameProcessorSubmitResult {
            status: FrameProcessorSubmitStatus::Backpressure,
            queue_depth: Some(2),
            in_flight_frames: Some(1),
            message: Some("queue full".to_owned()),
        };

        let encoded = serde_json::to_string(&result).expect("serialize submit result");
        let decoded: FrameProcessorSubmitResult =
            serde_json::from_str(&encoded).expect("deserialize submit result");

        assert_eq!(decoded, result);
    }

    #[test]
    fn frame_processor_receive_metadata_round_trips_through_json() {
        let receive = FrameProcessorReceiveFrameMetadata {
            status: FrameProcessorReceiveStatus::Frame,
            frame: Some(metadata()),
            timings: FrameProcessorFrameTimings {
                queue_wait_us: Some(10),
                process_time_us: Some(20),
                submit_to_ready_us: Some(30),
            },
            source_frame_id: Some(42),
            message: None,
        };

        let encoded = serde_json::to_string(&receive).expect("serialize receive metadata");
        let decoded: FrameProcessorReceiveFrameMetadata =
            serde_json::from_str(&encoded).expect("deserialize receive metadata");

        assert_eq!(decoded, receive);
    }

    #[test]
    fn frame_processor_capabilities_accept_empty_handle_kind_list_as_wildcard() {
        let capabilities = FrameProcessorCapabilities::default();

        assert!(capabilities.supports_input_handle_kind(&NativeHandleKind::D3D11Texture2D));
    }
}
