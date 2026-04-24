use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Media kind handled by a decoder plugin.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum DecoderMediaKind {
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
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub sample_rate: Option<u32>,
    pub channels: Option<u16>,
    pub prefer_hardware: bool,
    pub require_cpu_output: bool,
}

impl Default for DecoderMediaKind {
    fn default() -> Self {
        Self::Video
    }
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
