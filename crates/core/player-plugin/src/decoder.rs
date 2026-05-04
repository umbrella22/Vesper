use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Media kind handled by a decoder plugin.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum DecoderMediaKind {
    #[default]
    Video,
    Audio,
}

/// CPU frame formats supported by decoder plugin ABI v1.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum DecoderFrameFormat {
    Rgba8888,
    Bgra8888,
    Yuv420p,
    Nv12,
    F32,
    S16,
    Unknown(String),
}

/// Describes one codec a decoder plugin can open.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DecoderCodecCapability {
    pub codec: String,
    pub media_kind: DecoderMediaKind,
    pub profiles: Vec<String>,
    pub output_formats: Vec<DecoderFrameFormat>,
}

/// Decoder plugin capability payload returned through the dynamic ABI.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct DecoderCapabilities {
    pub codecs: Vec<DecoderCodecCapability>,
    pub supports_hardware_decode: bool,
    pub supports_cpu_video_frames: bool,
    pub supports_audio_frames: bool,
    pub supports_gpu_handles: bool,
    pub supports_flush: bool,
    pub supports_drain: bool,
    pub max_sessions: Option<u32>,
}

impl DecoderCapabilities {
    /// Returns whether this plugin advertises support for a codec/media pair.
    pub fn supports_codec(&self, codec: &str, media_kind: DecoderMediaKind) -> bool {
        self.codecs.iter().any(|capability| {
            capability.media_kind == media_kind && capability.codec.eq_ignore_ascii_case(codec)
        })
    }
}

/// Configuration used to open a decoder session.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct DecoderSessionConfig {
    pub codec: String,
    pub media_kind: DecoderMediaKind,
    pub extradata: Vec<u8>,
    #[serde(default)]
    pub bitstream_format: Option<DecoderBitstreamFormat>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    #[serde(default)]
    pub coded_width: Option<u32>,
    #[serde(default)]
    pub coded_height: Option<u32>,
    pub sample_rate: Option<u32>,
    pub channels: Option<u16>,
    pub prefer_hardware: bool,
    pub require_cpu_output: bool,
    #[serde(default)]
    pub native_device_context: Option<DecoderNativeDeviceContext>,
}

/// Optional session metadata returned by a plugin after opening a decoder.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct DecoderSessionInfo {
    pub decoder_name: Option<String>,
    pub selected_hardware_backend: Option<String>,
    pub output_format: Option<DecoderFrameFormat>,
}

/// Compressed packet metadata passed to `DecoderSession::send_packet`.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct DecoderPacket {
    pub pts_us: Option<i64>,
    pub dts_us: Option<i64>,
    pub duration_us: Option<i64>,
    pub stream_index: u32,
    pub key_frame: bool,
    pub discontinuity: bool,
    #[serde(default)]
    pub end_of_stream: bool,
}

/// Result returned after sending one compressed packet.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DecoderPacketResult {
    pub accepted: bool,
}

impl Default for DecoderPacketResult {
    fn default() -> Self {
        Self { accepted: true }
    }
}

/// Describes one plane inside a decoded CPU-frame payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DecoderFramePlane {
    pub offset: usize,
    pub len: usize,
    pub stride: Option<u32>,
}

/// Metadata for a decoded frame. Pixel or PCM bytes are transferred separately.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DecoderFrameMetadata {
    pub media_kind: DecoderMediaKind,
    pub format: DecoderFrameFormat,
    pub pts_us: Option<i64>,
    pub duration_us: Option<i64>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub sample_rate: Option<u32>,
    pub channels: Option<u16>,
    pub planes: Vec<DecoderFramePlane>,
}

/// A decoded frame returned by the Rust-side decoder session trait.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecoderFrame {
    pub metadata: DecoderFrameMetadata,
    pub data: Vec<u8>,
}

/// Receive state encoded in frame metadata over the C ABI.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DecoderReceiveFrameStatus {
    Frame,
    NeedMoreInput,
    Eof,
}

/// Metadata returned by the dynamic ABI receive-frame call.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DecoderReceiveFrameMetadata {
    pub status: DecoderReceiveFrameStatus,
    pub frame: Option<DecoderFrameMetadata>,
}

impl DecoderReceiveFrameMetadata {
    pub fn frame(frame: DecoderFrameMetadata) -> Self {
        Self {
            status: DecoderReceiveFrameStatus::Frame,
            frame: Some(frame),
        }
    }

    pub fn need_more_input() -> Self {
        Self {
            status: DecoderReceiveFrameStatus::NeedMoreInput,
            frame: None,
        }
    }

    pub fn eof() -> Self {
        Self {
            status: DecoderReceiveFrameStatus::Eof,
            frame: None,
        }
    }
}

/// Rust-side receive result returned by decoder sessions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DecoderReceiveFrameOutput {
    Frame(DecoderFrame),
    NeedMoreInput,
    Eof,
}

/// Native frame handle kinds returned by decoder plugin ABI v2.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum DecoderNativeHandleKind {
    CvPixelBuffer,
    IoSurface,
    MetalTexture,
    DmaBuf,
    VaapiSurface,
    D3D11Texture2D,
    DxgiSurface,
    VulkanImage,
    Unknown(String),
}

/// Native graphics device/context kinds that a host may share with a decoder plugin.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum DecoderNativeDeviceContextKind {
    D3D11Device,
    Unknown(String),
}

/// Compressed video bitstream representation expected by a native decoder.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum DecoderBitstreamFormat {
    AnnexB,
    Avcc,
    Hvcc,
    Unknown(String),
}

/// Borrowed native device/context pointer passed from host to decoder plugin.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DecoderNativeDeviceContext {
    pub kind: DecoderNativeDeviceContextKind,
    pub handle: usize,
}

/// Native-frame decoder requirements advertised through ABI v2.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct DecoderNativeRequirements {
    pub required_device_context_kinds: Vec<DecoderNativeDeviceContextKind>,
    pub output_handle_kinds: Vec<DecoderNativeHandleKind>,
    pub requires_native_device_context: bool,
    pub accepted_bitstream_formats: Vec<DecoderBitstreamFormat>,
}

/// Visible content rectangle within a coded native frame.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DecoderVisibleRect {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

/// Release tracking diagnostics attached to a native frame.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DecoderNativeFrameReleaseTracking {
    pub frame_id: Option<u64>,
    pub requires_release: bool,
}

/// Metadata for a decoded native frame. The native handle is transferred separately.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DecoderNativeFrameMetadata {
    pub media_kind: DecoderMediaKind,
    pub format: DecoderFrameFormat,
    pub codec: String,
    pub pts_us: Option<i64>,
    pub duration_us: Option<i64>,
    pub width: u32,
    pub height: u32,
    #[serde(default)]
    pub coded_width: Option<u32>,
    #[serde(default)]
    pub coded_height: Option<u32>,
    #[serde(default)]
    pub visible_rect: Option<DecoderVisibleRect>,
    pub handle_kind: DecoderNativeHandleKind,
    #[serde(default)]
    pub frame_id: Option<u64>,
    #[serde(default)]
    pub release_tracking: Option<DecoderNativeFrameReleaseTracking>,
}

/// A decoded native frame returned by the Rust-side decoder session trait.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecoderNativeFrame {
    pub metadata: DecoderNativeFrameMetadata,
    pub handle: usize,
}

/// Metadata returned by the dynamic ABI v2 native-frame receive call.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DecoderReceiveNativeFrameMetadata {
    pub status: DecoderReceiveFrameStatus,
    pub frame: Option<DecoderNativeFrameMetadata>,
}

impl DecoderReceiveNativeFrameMetadata {
    pub fn frame(frame: DecoderNativeFrameMetadata) -> Self {
        Self {
            status: DecoderReceiveFrameStatus::Frame,
            frame: Some(frame),
        }
    }

    pub fn need_more_input() -> Self {
        Self {
            status: DecoderReceiveFrameStatus::NeedMoreInput,
            frame: None,
        }
    }

    pub fn eof() -> Self {
        Self {
            status: DecoderReceiveFrameStatus::Eof,
            frame: None,
        }
    }
}

/// Rust-side receive result returned by native decoder sessions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DecoderReceiveNativeFrameOutput {
    Frame(DecoderNativeFrame),
    NeedMoreInput,
    Eof,
}

/// Empty success payload used by flush/close operations.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct DecoderOperationStatus {
    pub completed: bool,
}

/// Error payload shared by decoder plugins and host-side adapters.
#[derive(Debug, Error, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum DecoderError {
    #[error("unsupported codec: {codec}")]
    UnsupportedCodec { codec: String },
    #[error("decoder payload codec error: {message}")]
    PayloadCodec { message: String },
    #[error("decoder ABI violation: {message}")]
    AbiViolation { message: String },
    #[error("invalid packet: {message}")]
    InvalidPacket { message: String },
    #[error("decoder session is not configured")]
    NotConfigured,
    #[error("decoder needs more input")]
    NeedMoreInput,
    #[error("decoder reached end of stream")]
    Eof,
    #[error("decoder internal error: {message}")]
    Internal { message: String },
}

impl DecoderError {
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

    pub fn internal(message: impl Into<String>) -> Self {
        Self::Internal {
            message: message.into(),
        }
    }
}

/// Creates decoder sessions for one plugin.
pub trait DecoderPluginFactory: Send + Sync {
    fn name(&self) -> &str;

    fn capabilities(&self) -> DecoderCapabilities;

    fn open_session(
        &self,
        config: &DecoderSessionConfig,
    ) -> Result<Box<dyn DecoderSession>, DecoderError>;
}

/// Creates native-frame decoder sessions for one plugin.
pub trait NativeDecoderPluginFactory: Send + Sync {
    fn name(&self) -> &str;

    fn capabilities(&self) -> DecoderCapabilities;

    fn native_requirements(&self) -> DecoderNativeRequirements {
        DecoderNativeRequirements::default()
    }

    fn open_native_session(
        &self,
        config: &DecoderSessionConfig,
    ) -> Result<Box<dyn NativeDecoderSession>, DecoderError>;
}

/// Stateful decoder session created by a decoder plugin factory.
pub trait DecoderSession: Send {
    fn session_info(&self) -> DecoderSessionInfo;

    fn send_packet(
        &mut self,
        packet: &DecoderPacket,
        data: &[u8],
    ) -> Result<DecoderPacketResult, DecoderError>;

    fn receive_frame(&mut self) -> Result<DecoderReceiveFrameOutput, DecoderError>;

    fn flush(&mut self) -> Result<(), DecoderError>;

    fn close(&mut self) -> Result<(), DecoderError>;
}

/// Stateful native-frame decoder session created by a v2 decoder plugin factory.
pub trait NativeDecoderSession: Send {
    fn session_info(&self) -> DecoderSessionInfo;

    fn send_packet(
        &mut self,
        packet: &DecoderPacket,
        data: &[u8],
    ) -> Result<DecoderPacketResult, DecoderError>;

    fn receive_native_frame(&mut self) -> Result<DecoderReceiveNativeFrameOutput, DecoderError>;

    fn release_native_frame(&mut self, frame: DecoderNativeFrame) -> Result<(), DecoderError>;

    fn flush(&mut self) -> Result<(), DecoderError>;

    fn close(&mut self) -> Result<(), DecoderError>;
}
