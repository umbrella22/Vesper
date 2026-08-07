use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{NativeFrame, NativeFrameMetadata, NativeFramePipelineProfile, NativeHandleKind};

/// Call-scoped view of a host-owned native frame submitted to a processor.
///
/// The view is intentionally neither `Clone` nor `Copy`. Its native handle is
/// valid only for the synchronous submit/receive sequence. A plugin may echo
/// the handle through `borrowed_passthrough`, but it must retain the platform
/// resource through an explicit platform API before performing asynchronous
/// work or returning an owned output.
///
/// ```compile_fail
/// use player_plugin::FrameProcessorInputFrame;
///
/// fn retain_input(frame: FrameProcessorInputFrame<'_>) {
///     let _retained = frame.clone();
/// }
/// ```
#[must_use = "the borrowed native frame is valid only for the submit callback"]
pub struct FrameProcessorInputFrame<'a> {
    metadata: &'a NativeFrameMetadata,
    native_handle: usize,
}

impl<'a> FrameProcessorInputFrame<'a> {
    /// Borrows an existing native frame for one synchronous submit call.
    pub fn new(frame: &'a NativeFrame) -> Self {
        Self {
            metadata: &frame.metadata,
            native_handle: frame.handle,
        }
    }

    pub(crate) fn from_abi(metadata: &'a NativeFrameMetadata, native_handle: usize) -> Self {
        Self {
            metadata,
            native_handle,
        }
    }

    /// Returns metadata borrowed from the host-owned input frame.
    pub fn metadata(&self) -> &'a NativeFrameMetadata {
        self.metadata
    }

    /// Returns the call-scoped opaque native handle.
    ///
    /// Copying this integer does not retain the underlying platform resource;
    /// using it after `submit_frame` returns violates the plugin contract.
    pub fn native_handle(&self) -> usize {
        self.native_handle
    }

    /// Creates a host-owned passthrough result without transferring ownership.
    ///
    /// The returned frame must only be used as the immediate output for this
    /// input. The host keeps the upstream resource alive while consuming that
    /// output and will not call `FrameProcessorSession::release_frame` for it.
    pub fn borrowed_passthrough(&self) -> NativeFrame {
        let mut metadata = self.metadata.clone();
        metadata.release_tracking = Some(crate::NativeFrameReleaseTracking {
            frame_id: metadata.frame_id,
            requires_release: false,
        });
        NativeFrame {
            metadata,
            handle: self.native_handle,
            lease_token: None,
        }
    }
}

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
    #[serde(default)]
    pub accepted_input_pipeline_profiles: Vec<NativeFramePipelineProfile>,
    #[serde(default)]
    pub output_pipeline_profiles: Vec<NativeFramePipelineProfile>,
    pub supports_video_frames: bool,
    pub supports_in_place_passthrough: bool,
    pub preserves_dimensions: bool,
    pub may_change_dimensions: bool,
    #[serde(default)]
    pub preserves_color_metadata: bool,
    #[serde(default)]
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

    /// Returns whether the processor accepts a native-frame pipeline profile.
    pub fn supports_input_pipeline_profile(&self, profile: &NativeFramePipelineProfile) -> bool {
        self.accepted_input_pipeline_profiles.is_empty()
            || self
                .accepted_input_pipeline_profiles
                .iter()
                .any(|candidate| candidate == profile)
    }

    /// Returns whether both handle kind and pipeline profile match the metadata.
    pub fn supports_input_metadata(&self, metadata: &NativeFrameMetadata) -> bool {
        self.supports_input_handle_kind(&metadata.handle_kind)
            && self.supports_input_pipeline_profile(&metadata.effective_pipeline_profile())
    }

    /// Returns whether the processor can produce an output native handle kind.
    pub fn supports_output_handle_kind(&self, handle_kind: &NativeHandleKind) -> bool {
        self.output_handle_kinds.is_empty()
            || self
                .output_handle_kinds
                .iter()
                .any(|candidate| candidate == handle_kind)
    }

    /// Returns whether the processor can produce an output pipeline profile.
    pub fn supports_output_pipeline_profile(&self, profile: &NativeFramePipelineProfile) -> bool {
        self.output_pipeline_profiles.is_empty()
            || self
                .output_pipeline_profiles
                .iter()
                .any(|candidate| candidate == profile)
    }
}

/// Capability requirements used when opening one frame processor session.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FrameProcessorSessionRequirements {
    pub input_metadata: NativeFrameMetadata,
    #[serde(default)]
    pub output_handle_kind: Option<NativeHandleKind>,
    #[serde(default)]
    pub output_pipeline_profile: Option<NativeFramePipelineProfile>,
    #[serde(default)]
    pub require_video_frames: bool,
    #[serde(default)]
    pub require_native_first: bool,
    #[serde(default)]
    pub require_explicit_native_input: bool,
    #[serde(default)]
    pub require_flush: bool,
    #[serde(default)]
    pub reject_dimension_changes: bool,
    #[serde(default)]
    pub require_color_metadata_preservation: bool,
    #[serde(default)]
    pub require_hdr_metadata_preservation: bool,
    #[serde(default)]
    pub max_in_flight_frames: Option<u32>,
}

impl FrameProcessorSessionRequirements {
    /// Builds native video requirements for a decoded frame processor chain.
    pub fn native_video(input_metadata: NativeFrameMetadata) -> Self {
        Self {
            output_handle_kind: Some(input_metadata.handle_kind.clone()),
            output_pipeline_profile: Some(input_metadata.effective_pipeline_profile()),
            require_color_metadata_preservation: input_metadata.requires_color_preservation(),
            require_hdr_metadata_preservation: input_metadata.requires_hdr_preservation(),
            input_metadata,
            require_video_frames: true,
            require_native_first: true,
            require_explicit_native_input: false,
            require_flush: false,
            reject_dimension_changes: true,
            max_in_flight_frames: None,
        }
    }

    /// Returns missing capability names for this requirement.
    pub fn missing_capabilities(&self, capabilities: &FrameProcessorCapabilities) -> Vec<String> {
        let mut missing = Vec::new();
        if self.require_video_frames && !capabilities.supports_video_frames {
            missing.push("video frames".to_owned());
        }
        if self.require_native_first
            && !capabilities.supports_input_handle_kind(&self.input_metadata.handle_kind)
        {
            missing.push(format!(
                "input handle kind {:?}",
                self.input_metadata.handle_kind
            ));
        }
        let input_profile = self.input_metadata.effective_pipeline_profile();
        if self.require_native_first
            && !capabilities.supports_input_pipeline_profile(&input_profile)
        {
            missing.push(format!("input pipeline profile {:?}", input_profile));
        }
        if self.require_explicit_native_input
            && !capabilities
                .accepted_input_handle_kinds
                .contains(&self.input_metadata.handle_kind)
        {
            missing.push(format!(
                "explicit input handle kind {:?}",
                self.input_metadata.handle_kind
            ));
        }
        if self.require_explicit_native_input
            && !capabilities
                .accepted_input_pipeline_profiles
                .contains(&input_profile)
        {
            missing.push(format!(
                "explicit input pipeline profile {:?}",
                input_profile
            ));
        }
        if let Some(handle_kind) = &self.output_handle_kind
            && !capabilities.supports_output_handle_kind(handle_kind)
        {
            missing.push(format!("output handle kind {handle_kind:?}"));
        }
        if let Some(profile) = &self.output_pipeline_profile
            && !capabilities.supports_output_pipeline_profile(profile)
        {
            missing.push(format!("output pipeline profile {profile:?}"));
        }
        if self.require_flush && !capabilities.supports_flush {
            missing.push("flush support".to_owned());
        }
        if self.reject_dimension_changes && capabilities.may_change_dimensions {
            missing.push("stable dimensions".to_owned());
        }
        if self.require_color_metadata_preservation && !capabilities.preserves_color_metadata {
            missing.push("preservesColorMetadata".to_owned());
        }
        if self.require_hdr_metadata_preservation && !capabilities.preserves_hdr_metadata {
            missing.push("preservesHdrMetadata".to_owned());
        }
        if let (Some(required), Some(limit)) =
            (self.max_in_flight_frames, capabilities.max_in_flight_frames)
            && limit < required
        {
            missing.push(format!("max in-flight frames >= {required}"));
        }
        missing
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
    #[serde(default)]
    pub output_pipeline_profile: Option<NativeFramePipelineProfile>,
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
    pub message: Option<String>,
}

/// Rust-side receive result returned by frame processor sessions.
#[allow(
    clippy::large_enum_variant,
    reason = "boxing Frame would break the public frame processor session API"
)]
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
        frame: FrameProcessorInputFrame<'_>,
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
        FrameProcessorReceiveStatus, FrameProcessorSessionRequirements, FrameProcessorSubmitResult,
        FrameProcessorSubmitStatus,
    };
    use crate::{
        DecoderFrameFormat, DecoderMediaKind, NativeFrameColorMetadata, NativeFrameHdrMetadata,
        NativeFrameMetadata, NativeFramePipelineProfile, NativeHandleKind, VisibleRect,
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
            pipeline_profile: Some(NativeFramePipelineProfile::VideoToolboxCvPixelBuffer),
            color_space: Some("bt709".to_owned()),
            hdr_metadata: None,
            color: Some(NativeFrameColorMetadata {
                primaries: Some("bt709".to_owned()),
                transfer: Some("bt709".to_owned()),
                matrix: Some("bt709".to_owned()),
                range: Some("limited".to_owned()),
                bit_depth: Some(8),
            }),
            hdr: None,
            sync_info: None,
            transform: None,
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

    #[test]
    fn frame_processor_capabilities_accept_empty_pipeline_profile_list_as_wildcard() {
        let capabilities = FrameProcessorCapabilities::default();

        assert!(
            capabilities
                .supports_input_pipeline_profile(&NativeFramePipelineProfile::D3D11Texture2D)
        );
    }

    #[test]
    fn frame_processor_capabilities_default_metadata_preservation_fields() {
        let decoded: FrameProcessorCapabilities = serde_json::from_str(
            r#"{
                "accepted_input_handle_kinds": [],
                "output_handle_kinds": [],
                "supports_video_frames": true,
                "supports_in_place_passthrough": true,
                "preserves_dimensions": true,
                "may_change_dimensions": false,
                "supports_flush": false,
                "max_sessions": null,
                "max_in_flight_frames": null
            }"#,
        )
        .expect("legacy capabilities should deserialize without preservation fields");

        assert!(!decoded.preserves_color_metadata);
        assert!(!decoded.preserves_hdr_metadata);
    }

    #[test]
    fn frame_processor_capabilities_match_input_metadata_by_handle_and_profile() {
        let capabilities = FrameProcessorCapabilities {
            accepted_input_handle_kinds: vec![NativeHandleKind::CvPixelBuffer],
            output_handle_kinds: vec![NativeHandleKind::CvPixelBuffer],
            accepted_input_pipeline_profiles: vec![
                NativeFramePipelineProfile::VideoToolboxCvPixelBuffer,
            ],
            output_pipeline_profiles: vec![NativeFramePipelineProfile::VideoToolboxCvPixelBuffer],
            supports_video_frames: true,
            ..Default::default()
        };

        assert!(capabilities.supports_input_metadata(&metadata()));

        let mut mismatched = metadata();
        mismatched.pipeline_profile = Some(NativeFramePipelineProfile::D3D11Texture2D);
        assert!(!capabilities.supports_input_metadata(&mismatched));
    }

    #[test]
    fn frame_processor_session_requirements_report_missing_capabilities() {
        let requirements = FrameProcessorSessionRequirements {
            require_flush: true,
            require_explicit_native_input: true,
            max_in_flight_frames: Some(4),
            ..FrameProcessorSessionRequirements::native_video(metadata())
        };
        let capabilities = FrameProcessorCapabilities {
            accepted_input_handle_kinds: vec![NativeHandleKind::D3D11Texture2D],
            output_handle_kinds: vec![NativeHandleKind::D3D11Texture2D],
            accepted_input_pipeline_profiles: vec![NativeFramePipelineProfile::D3D11Texture2D],
            output_pipeline_profiles: vec![NativeFramePipelineProfile::D3D11Texture2D],
            supports_video_frames: false,
            supports_flush: false,
            may_change_dimensions: true,
            preserves_color_metadata: false,
            preserves_hdr_metadata: false,
            max_in_flight_frames: Some(1),
            ..Default::default()
        };

        let missing = requirements.missing_capabilities(&capabilities);

        assert!(missing.iter().any(|item| item == "video frames"));
        assert!(
            missing
                .iter()
                .any(|item| item.contains("input handle kind CvPixelBuffer"))
        );
        assert!(
            missing
                .iter()
                .any(|item| item.contains("explicit input handle kind CvPixelBuffer"))
        );
        assert!(
            missing
                .iter()
                .any(|item| item.contains("output pipeline profile VideoToolboxCvPixelBuffer"))
        );
        assert!(missing.iter().any(|item| item == "flush support"));
        assert!(missing.iter().any(|item| item == "stable dimensions"));
        assert!(!missing.iter().any(|item| item == "preservesColorMetadata"));
        assert!(
            missing
                .iter()
                .any(|item| item == "max in-flight frames >= 4")
        );
    }

    #[test]
    fn frame_processor_session_requirements_report_color_preservation_for_wide_color() {
        let mut metadata = metadata();
        metadata.color_space = Some("bt2020".to_owned());
        metadata.color = Some(NativeFrameColorMetadata {
            primaries: Some("bt2020".to_owned()),
            transfer: Some("sdr-video".to_owned()),
            matrix: Some("bt2020-ncl".to_owned()),
            range: Some("limited".to_owned()),
            bit_depth: Some(8),
        });
        let requirements = FrameProcessorSessionRequirements::native_video(metadata);
        let capabilities = FrameProcessorCapabilities {
            accepted_input_handle_kinds: vec![NativeHandleKind::CvPixelBuffer],
            output_handle_kinds: vec![NativeHandleKind::CvPixelBuffer],
            accepted_input_pipeline_profiles: vec![
                NativeFramePipelineProfile::VideoToolboxCvPixelBuffer,
            ],
            output_pipeline_profiles: vec![NativeFramePipelineProfile::VideoToolboxCvPixelBuffer],
            supports_video_frames: true,
            preserves_color_metadata: false,
            ..Default::default()
        };

        let missing = requirements.missing_capabilities(&capabilities);

        assert!(missing.iter().any(|item| item == "preservesColorMetadata"));
    }

    #[test]
    fn frame_processor_session_requirements_report_hdr_preservation() {
        let mut metadata = metadata();
        metadata.hdr = Some(NativeFrameHdrMetadata {
            kind: "hlg".to_owned(),
            mastering_display: None,
            content_light: None,
            dolby_vision: None,
        });
        let requirements = FrameProcessorSessionRequirements::native_video(metadata);
        let capabilities = FrameProcessorCapabilities {
            accepted_input_handle_kinds: vec![NativeHandleKind::CvPixelBuffer],
            output_handle_kinds: vec![NativeHandleKind::CvPixelBuffer],
            accepted_input_pipeline_profiles: vec![
                NativeFramePipelineProfile::VideoToolboxCvPixelBuffer,
            ],
            output_pipeline_profiles: vec![NativeFramePipelineProfile::VideoToolboxCvPixelBuffer],
            supports_video_frames: true,
            preserves_color_metadata: true,
            preserves_hdr_metadata: false,
            ..Default::default()
        };

        let missing = requirements.missing_capabilities(&capabilities);

        assert!(missing.iter().any(|item| item == "preservesHdrMetadata"));
    }
}
