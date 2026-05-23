use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::DecoderBitstreamFormat;

/// Normalization work level supported by a source normalizer plugin.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default, Serialize, Deserialize)]
pub enum SourceNormalizerNormalizeLevel {
    /// Remux/copy normalization with optional bitstream filters.
    #[default]
    #[serde(alias = "remux_only", alias = "remux-only")]
    RemuxOnly = 1,
    /// Packet repair that still does not decode media into frames.
    #[serde(alias = "packet_repair", alias = "packet-repair")]
    PacketRepair = 2,
}

/// FFmpeg-like build features required by a source normalizer profile.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct SourceNormalizerRequiredCapabilities {
    pub libraries: Vec<String>,
    pub demuxers: Vec<String>,
    pub muxers: Vec<String>,
    pub protocols: Vec<String>,
    pub parsers: Vec<String>,
    pub bitstream_filters: Vec<String>,
    #[serde(default)]
    pub tls: Option<String>,
    #[serde(default)]
    pub network: bool,
}

/// Capabilities advertised by a packet-stream source normalizer plugin.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct SourceNormalizerPacketCapabilities {
    pub supported_runtime_profiles: Vec<String>,
    pub max_level: SourceNormalizerNormalizeLevel,
    pub media_kinds: Vec<SourceNormalizerPacketMediaKind>,
    pub codecs: Vec<String>,
    pub bitstream_formats: Vec<DecoderBitstreamFormat>,
    pub supports_seek: bool,
    pub supports_flush: bool,
    pub required_capabilities: SourceNormalizerRequiredCapabilities,
    pub max_sessions: Option<u32>,
}

impl SourceNormalizerPacketCapabilities {
    /// Returns whether this plugin advertises a runtime profile.
    pub fn supports_runtime_profile(&self, runtime_profile: &str) -> bool {
        self.supported_runtime_profiles
            .iter()
            .any(|profile| profile.eq_ignore_ascii_case(runtime_profile))
    }

    /// Returns whether this plugin advertises a codec.
    pub fn supports_codec(&self, codec: &str) -> bool {
        self.codecs
            .iter()
            .any(|candidate| candidate.eq_ignore_ascii_case(codec))
    }
}

/// Packet stream media kind produced by a source normalizer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum SourceNormalizerPacketMediaKind {
    #[default]
    Video,
    Audio,
    Subtitle,
}

/// Configuration used to open one packet-stream source normalizer session.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceNormalizerPacketSessionConfig {
    pub runtime_profile: String,
    pub input: String,
    #[serde(default)]
    pub headers: Vec<(String, String)>,
    #[serde(default)]
    pub startup_timeout_ms: Option<u64>,
    #[serde(default)]
    pub session_timeout_ms: Option<u64>,
    #[serde(default)]
    pub preferred_media_kind: SourceNormalizerPacketMediaKind,
}

/// Track metadata exposed by a packet-stream source normalizer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SourceNormalizerPacketTrackInfo {
    pub stream_index: u32,
    pub media_kind: SourceNormalizerPacketMediaKind,
    pub codec: String,
    #[serde(default)]
    pub extradata: Vec<u8>,
    #[serde(default)]
    pub bitstream_format: Option<DecoderBitstreamFormat>,
    #[serde(default)]
    pub width: Option<u32>,
    #[serde(default)]
    pub height: Option<u32>,
    #[serde(default)]
    pub coded_width: Option<u32>,
    #[serde(default)]
    pub coded_height: Option<u32>,
    #[serde(default)]
    pub sample_rate: Option<u32>,
    #[serde(default)]
    pub channels: Option<u16>,
    #[serde(default)]
    pub frame_rate: Option<f64>,
    #[serde(default)]
    pub time_base_num: Option<i32>,
    #[serde(default)]
    pub time_base_den: Option<i32>,
}

/// Packet-stream metadata returned after opening a source normalizer session.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SourceNormalizerPacketStreamInfo {
    #[serde(default)]
    pub session_id: Option<String>,
    #[serde(default)]
    pub normalizer_name: Option<String>,
    #[serde(default)]
    pub runtime_profile: Option<String>,
    #[serde(default)]
    pub selected_backend: Option<String>,
    pub tracks: Vec<SourceNormalizerPacketTrackInfo>,
    #[serde(default)]
    pub selected_track_index: Option<u32>,
    #[serde(default)]
    pub duration_millis: Option<u64>,
    #[serde(default)]
    pub seekable: bool,
}

impl Default for SourceNormalizerPacketStreamInfo {
    fn default() -> Self {
        Self {
            session_id: None,
            normalizer_name: None,
            runtime_profile: None,
            selected_backend: None,
            tracks: Vec::new(),
            selected_track_index: None,
            duration_millis: None,
            seekable: false,
        }
    }
}

/// Packet read status encoded in source normalizer packet metadata.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SourceNormalizerReadPacketStatus {
    Packet,
    NeedMoreData,
    EndOfStream,
}

/// Compressed packet metadata returned by a packet-stream source normalizer.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct SourceNormalizerPacket {
    pub pts_us: Option<i64>,
    pub dts_us: Option<i64>,
    pub duration_us: Option<i64>,
    pub stream_index: u32,
    pub key_frame: bool,
    pub discontinuity: bool,
    #[serde(default)]
    pub end_of_stream: bool,
}

/// Metadata returned by `SourceNormalizerPacketSession::read_packet`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceNormalizerReadPacketMetadata {
    pub status: SourceNormalizerReadPacketStatus,
    #[serde(default)]
    pub packet: Option<SourceNormalizerPacket>,
    #[serde(default)]
    pub message: Option<String>,
}

impl SourceNormalizerReadPacketMetadata {
    pub fn packet(packet: SourceNormalizerPacket) -> Self {
        Self {
            status: SourceNormalizerReadPacketStatus::Packet,
            packet: Some(packet),
            message: None,
        }
    }

    pub fn need_more_data(message: Option<String>) -> Self {
        Self {
            status: SourceNormalizerReadPacketStatus::NeedMoreData,
            packet: None,
            message,
        }
    }

    pub fn end_of_stream() -> Self {
        Self {
            status: SourceNormalizerReadPacketStatus::EndOfStream,
            packet: None,
            message: None,
        }
    }
}

/// Seek request passed to an active packet-stream source normalizer session.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceNormalizerPacketSeek {
    pub position_millis: u64,
    #[serde(default)]
    pub exact: bool,
}

/// Empty success payload used by seek and close operations.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct SourceNormalizerOperationStatus {
    pub completed: bool,
    #[serde(default)]
    pub message: Option<String>,
}

/// Error payload shared by source normalizer plugins and host-side adapters.
#[derive(Debug, Error, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SourceNormalizerError {
    #[error("unsupported runtime profile: {profile}")]
    UnsupportedRuntimeProfile { profile: String },
    #[error("invalid source normalizer input: {message}")]
    InvalidInput { message: String },
    #[error("source normalizer payload codec error: {message}")]
    PayloadCodec { message: String },
    #[error("source normalizer configuration error: {message}")]
    Configuration { message: String },
    #[error("source normalizer ABI violation: {message}")]
    AbiViolation { message: String },
    #[error("source normalizer session is not configured")]
    NotConfigured,
    #[error("source normalizer operation is unsupported: {operation}")]
    UnsupportedOperation { operation: String },
    #[error("source normalizer timeout: {message}")]
    Timeout { message: String },
    #[error("source normalizer internal error: {message}")]
    Internal { message: String },
}

impl SourceNormalizerError {
    pub fn payload_codec(message: impl Into<String>) -> Self {
        Self::PayloadCodec {
            message: message.into(),
        }
    }

    pub fn configuration(message: impl Into<String>) -> Self {
        Self::Configuration {
            message: message.into(),
        }
    }

    pub fn abi_violation(message: impl Into<String>) -> Self {
        Self::AbiViolation {
            message: message.into(),
        }
    }

    pub fn invalid_input(message: impl Into<String>) -> Self {
        Self::InvalidInput {
            message: message.into(),
        }
    }

    pub fn unsupported_operation(operation: impl Into<String>) -> Self {
        Self::UnsupportedOperation {
            operation: operation.into(),
        }
    }

    pub fn internal(message: impl Into<String>) -> Self {
        Self::Internal {
            message: message.into(),
        }
    }
}

/// Creates packet-stream source normalizer sessions for one plugin.
pub trait SourceNormalizerPacketPluginFactory: Send + Sync {
    fn name(&self) -> &str;

    fn packet_capabilities(&self) -> SourceNormalizerPacketCapabilities;

    fn open_packet_session(
        &self,
        config: &SourceNormalizerPacketSessionConfig,
    ) -> Result<Box<dyn SourceNormalizerPacketSession>, SourceNormalizerError>;
}

/// Borrowed packet returned by a packet-stream source normalizer.
pub struct SourceNormalizerPacketLease<'a> {
    pub metadata: SourceNormalizerReadPacketMetadata,
    pub data: &'a [u8],
    pub handle: usize,
}

impl std::fmt::Debug for SourceNormalizerPacketLease<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SourceNormalizerPacketLease")
            .field("metadata", &self.metadata)
            .field("data_len", &self.data.len())
            .field("handle", &self.handle)
            .finish()
    }
}

/// Stateful packet-stream source normalizer session.
pub trait SourceNormalizerPacketSession: Send {
    fn stream_info(&self) -> SourceNormalizerPacketStreamInfo;

    fn read_packet(&mut self) -> Result<SourceNormalizerPacketLease<'_>, SourceNormalizerError>;

    fn release_packet(&mut self, packet_handle: usize) -> Result<(), SourceNormalizerError>;

    fn seek(
        &mut self,
        seek: &SourceNormalizerPacketSeek,
    ) -> Result<SourceNormalizerOperationStatus, SourceNormalizerError>;

    fn flush(&mut self) -> Result<SourceNormalizerOperationStatus, SourceNormalizerError>;

    fn close(&mut self) -> Result<(), SourceNormalizerError>;
}

#[cfg(test)]
mod tests {
    use super::{
        SourceNormalizerPacket, SourceNormalizerPacketCapabilities,
        SourceNormalizerPacketMediaKind, SourceNormalizerPacketTrackInfo,
        SourceNormalizerReadPacketMetadata, SourceNormalizerReadPacketStatus,
        SourceNormalizerRequiredCapabilities,
    };
    use crate::{
        DecoderBitstreamFormat, VESPER_SOURCE_NORMALIZER_PLUGIN_ABI_VERSION_V2, VesperPluginKind,
    };

    #[test]
    fn source_normalizer_abi_constants_are_stable() {
        assert_eq!(VesperPluginKind::SourceNormalizer as u32, 6);
        assert_eq!(VESPER_SOURCE_NORMALIZER_PLUGIN_ABI_VERSION_V2, 2);
    }

    #[test]
    fn source_normalizer_packet_metadata_round_trips_through_json() {
        let metadata = SourceNormalizerReadPacketMetadata::packet(SourceNormalizerPacket {
            pts_us: Some(33_000),
            dts_us: Some(30_000),
            duration_us: Some(33_333),
            stream_index: 1,
            key_frame: true,
            discontinuity: false,
            end_of_stream: false,
        });

        let encoded = serde_json::to_string(&metadata).expect("serialize packet metadata");
        let decoded: SourceNormalizerReadPacketMetadata =
            serde_json::from_str(&encoded).expect("deserialize packet metadata");

        assert_eq!(decoded, metadata);
        assert_eq!(decoded.status, SourceNormalizerReadPacketStatus::Packet);
    }

    #[test]
    fn source_normalizer_packet_track_info_round_trips_through_json() {
        let track = SourceNormalizerPacketTrackInfo {
            stream_index: 0,
            media_kind: SourceNormalizerPacketMediaKind::Video,
            codec: "H264".to_owned(),
            extradata: vec![1, 2, 3],
            bitstream_format: Some(DecoderBitstreamFormat::Avcc),
            width: Some(960),
            height: Some(432),
            coded_width: Some(960),
            coded_height: Some(432),
            sample_rate: None,
            channels: None,
            frame_rate: Some(30.0),
            time_base_num: Some(1),
            time_base_den: Some(90_000),
        };

        let encoded = serde_json::to_string(&track).expect("serialize track");
        let decoded: SourceNormalizerPacketTrackInfo =
            serde_json::from_str(&encoded).expect("deserialize track");

        assert_eq!(decoded, track);
    }

    #[test]
    fn source_normalizer_packet_capabilities_support_case_insensitive_codecs() {
        let capabilities = SourceNormalizerPacketCapabilities {
            supported_runtime_profiles: vec!["diagnostic-packet".to_owned()],
            max_level: Default::default(),
            media_kinds: vec![SourceNormalizerPacketMediaKind::Video],
            codecs: vec!["H264".to_owned()],
            bitstream_formats: vec![DecoderBitstreamFormat::Avcc],
            supports_seek: true,
            supports_flush: true,
            required_capabilities: SourceNormalizerRequiredCapabilities::default(),
            max_sessions: Some(1),
        };

        assert!(capabilities.supports_runtime_profile("DIAGNOSTIC-PACKET"));
        assert!(capabilities.supports_codec("h264"));
    }
}
