use std::time::Duration;

use serde::{Deserialize, Serialize};
use thiserror::Error;
use url::Url;

use crate::{DecoderBitstreamFormat, NativeFrameColorMetadata, NativeFrameHdrMetadata};

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

    /// Returns whether this plugin advertises packet output for a media kind.
    pub fn supports_media_kind(&self, media_kind: SourceNormalizerPacketMediaKind) -> bool {
        self.media_kinds.contains(&media_kind)
    }

    /// Returns whether this plugin advertises a packet bitstream format.
    pub fn supports_bitstream_format(&self, format: &DecoderBitstreamFormat) -> bool {
        self.bitstream_formats.contains(format)
    }
}

/// Normalized output route produced by a source normalizer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SourceNormalizerOutputRoute {
    /// Disk-backed fragmented MP4 output intended to be exposed as a local stream.
    Fmp4LocalStream,
    /// Disk-backed short-window HLS output intended for nonstandard adaptive input.
    HlsShortWindow,
    /// Compressed packet stream intended for the SDK-controlled native frame lane.
    PacketStream,
}

impl SourceNormalizerOutputRoute {
    pub fn wire_name(self) -> &'static str {
        match self {
            Self::Fmp4LocalStream => "fmp4LocalStream",
            Self::HlsShortWindow => "hlsShortWindow",
            Self::PacketStream => "packetStream",
        }
    }
}

/// Resource session cache limits shared by plugin and platform hosts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceNormalizerResourceCachePolicy {
    /// Maximum bytes read into memory per active session.
    pub session_read_buffer_bytes: u64,
    /// Maximum bytes used for manifest and metadata snapshots per session.
    pub manifest_snapshot_bytes: u64,
    /// Soft disk limit for one resource session.
    pub session_disk_soft_cap_bytes: u64,
    /// Soft disk limit for all normalized-resource sessions owned by a host.
    pub global_disk_soft_cap_bytes: u64,
}

impl Default for SourceNormalizerResourceCachePolicy {
    fn default() -> Self {
        Self {
            session_read_buffer_bytes: 4 * 1024 * 1024,
            manifest_snapshot_bytes: 512 * 1024,
            session_disk_soft_cap_bytes: 512 * 1024 * 1024,
            global_disk_soft_cap_bytes: 1536 * 1024 * 1024,
        }
    }
}

/// Capabilities advertised by a resource-output source normalizer plugin.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct SourceNormalizerResourceCapabilities {
    pub supported_runtime_profiles: Vec<String>,
    pub supported_output_routes: Vec<SourceNormalizerOutputRoute>,
    pub max_level: SourceNormalizerNormalizeLevel,
    pub content_types: Vec<String>,
    pub supports_growing_resources: bool,
    pub supports_range_reads: bool,
    pub supports_cancel: bool,
    pub required_capabilities: SourceNormalizerRequiredCapabilities,
    pub cache_policy: SourceNormalizerResourceCachePolicy,
    pub max_sessions: Option<u32>,
}

impl SourceNormalizerResourceCapabilities {
    /// Returns whether this plugin advertises a runtime profile.
    pub fn supports_runtime_profile(&self, runtime_profile: &str) -> bool {
        self.supported_runtime_profiles
            .iter()
            .any(|profile| profile.eq_ignore_ascii_case(runtime_profile))
    }

    /// Returns whether this plugin advertises an output route.
    pub fn supports_output_route(&self, route: SourceNormalizerOutputRoute) -> bool {
        self.supported_output_routes.contains(&route)
    }
}

/// Capability requirements used when opening one source normalizer session.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SourceNormalizerSessionRequirements {
    Packet(SourceNormalizerPacketSessionRequirements),
    Resource(SourceNormalizerResourceSessionRequirements),
}

impl SourceNormalizerSessionRequirements {
    /// Returns missing capability names for this requirement.
    pub fn missing_capabilities(
        &self,
        capabilities: &SourceNormalizerSessionCapabilities<'_>,
    ) -> Vec<String> {
        match (self, capabilities) {
            (
                Self::Packet(requirements),
                SourceNormalizerSessionCapabilities::Packet(capabilities),
            ) => requirements.missing_capabilities(capabilities),
            (
                Self::Resource(requirements),
                SourceNormalizerSessionCapabilities::Resource(capabilities),
            ) => requirements.missing_capabilities(capabilities),
            (Self::Packet(_), SourceNormalizerSessionCapabilities::Resource(_)) => {
                vec!["packet stream route".to_owned()]
            }
            (Self::Resource(_), SourceNormalizerSessionCapabilities::Packet(_)) => {
                vec!["resource output route".to_owned()]
            }
        }
    }
}

/// Borrowed source normalizer capabilities used for generic requirement matching.
#[derive(Debug, Clone, Copy)]
pub enum SourceNormalizerSessionCapabilities<'a> {
    Packet(&'a SourceNormalizerPacketCapabilities),
    Resource(&'a SourceNormalizerResourceCapabilities),
}

/// Packet-stream capability requirements.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceNormalizerPacketSessionRequirements {
    pub runtime_profile: String,
    #[serde(default)]
    pub media_kind: Option<SourceNormalizerPacketMediaKind>,
    #[serde(default)]
    pub codec: Option<String>,
    #[serde(default)]
    pub bitstream_format: Option<DecoderBitstreamFormat>,
    #[serde(default)]
    pub require_seek: bool,
    #[serde(default)]
    pub require_flush: bool,
    #[serde(default)]
    pub require_lease_cleanup: bool,
}

impl SourceNormalizerPacketSessionRequirements {
    /// Builds packet-stream requirements for native-frame video decode.
    pub fn native_video(runtime_profile: impl Into<String>, codec: impl Into<String>) -> Self {
        Self {
            runtime_profile: runtime_profile.into(),
            media_kind: Some(SourceNormalizerPacketMediaKind::Video),
            codec: Some(codec.into()),
            bitstream_format: None,
            require_seek: false,
            require_flush: true,
            require_lease_cleanup: true,
        }
    }

    /// Returns missing capability names for this requirement.
    pub fn missing_capabilities(
        &self,
        capabilities: &SourceNormalizerPacketCapabilities,
    ) -> Vec<String> {
        let mut missing = Vec::new();
        if !self.runtime_profile.is_empty()
            && !capabilities.supports_runtime_profile(&self.runtime_profile)
        {
            missing.push(format!("runtime profile {}", self.runtime_profile));
        }
        if let Some(media_kind) = self.media_kind
            && !capabilities.supports_media_kind(media_kind)
        {
            missing.push(format!("packet media kind {media_kind:?}"));
        }
        if let Some(codec) = &self.codec
            && !capabilities.supports_codec(codec)
        {
            missing.push(format!("packet codec {codec}"));
        }
        if let Some(format) = &self.bitstream_format
            && !capabilities.supports_bitstream_format(format)
        {
            missing.push(format!("packet bitstream format {format:?}"));
        }
        if self.require_seek && !capabilities.supports_seek {
            missing.push("packet seek support".to_owned());
        }
        if self.require_flush && !capabilities.supports_flush {
            missing.push("packet flush support".to_owned());
        }
        if self.require_lease_cleanup && !capabilities.supports_flush {
            missing.push("outstanding lease cleanup".to_owned());
        }
        missing
    }
}

/// Resource-output capability requirements.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceNormalizerResourceSessionRequirements {
    pub runtime_profile: String,
    pub output_route: SourceNormalizerOutputRoute,
    #[serde(default)]
    pub content_type: Option<String>,
    #[serde(default)]
    pub require_growing_resources: bool,
    #[serde(default)]
    pub require_range_reads: bool,
    #[serde(default)]
    pub require_cancel: bool,
}

impl SourceNormalizerResourceSessionRequirements {
    /// Returns missing capability names for this requirement.
    pub fn missing_capabilities(
        &self,
        capabilities: &SourceNormalizerResourceCapabilities,
    ) -> Vec<String> {
        let mut missing = Vec::new();
        if !self.runtime_profile.is_empty()
            && !capabilities.supports_runtime_profile(&self.runtime_profile)
        {
            missing.push(format!("runtime profile {}", self.runtime_profile));
        }
        if !capabilities.supports_output_route(self.output_route) {
            missing.push(format!("resource output route {:?}", self.output_route));
        }
        if let Some(content_type) = &self.content_type {
            let supported = capabilities
                .content_types
                .iter()
                .any(|candidate| candidate.eq_ignore_ascii_case(content_type));
            if !supported {
                missing.push(format!("content type {content_type}"));
            }
        }
        if self.require_growing_resources && !capabilities.supports_growing_resources {
            missing.push("growing resources".to_owned());
        }
        if self.require_range_reads && !capabilities.supports_range_reads {
            missing.push("range reads".to_owned());
        }
        if self.require_cancel && !capabilities.supports_cancel {
            missing.push("cancel support".to_owned());
        }
        missing
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

/// Configuration used to open one disk-backed resource source normalizer session.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceNormalizerResourceSessionConfig {
    pub runtime_profile: String,
    pub input: String,
    #[serde(default)]
    pub headers: Vec<(String, String)>,
    pub output_root: String,
    #[serde(default)]
    pub cache_policy: SourceNormalizerResourceCachePolicy,
    #[serde(default)]
    pub preferred_route: Option<SourceNormalizerOutputRoute>,
    #[serde(default)]
    pub startup_timeout_ms: Option<u64>,
    #[serde(default)]
    pub read_idle_timeout_ms: Option<u64>,
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
    pub reorder_depth: Option<u32>,
    #[serde(default)]
    pub sample_rate: Option<u32>,
    #[serde(default)]
    pub channels: Option<u16>,
    #[serde(default)]
    pub channel_layout: Option<String>,
    #[serde(default)]
    pub codec_delay_samples: Option<u32>,
    #[serde(default)]
    pub priming_samples: Option<u32>,
    #[serde(default)]
    pub trailing_padding_samples: Option<u32>,
    #[serde(default)]
    pub seek_preroll_samples: Option<u32>,
    #[serde(default)]
    pub color: Option<NativeFrameColorMetadata>,
    #[serde(default)]
    pub hdr: Option<NativeFrameHdrMetadata>,
    #[serde(default)]
    pub frame_rate: Option<f64>,
    #[serde(default)]
    pub time_base_num: Option<i32>,
    #[serde(default)]
    pub time_base_den: Option<i32>,
}

/// Packet-stream metadata returned after opening a source normalizer session.
#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
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

/// Disk-backed resource produced by a source normalizer session.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceNormalizerResourceInfo {
    pub role: String,
    pub path: String,
    #[serde(default)]
    pub content_type: Option<String>,
    #[serde(default)]
    pub byte_length: Option<u64>,
    #[serde(default)]
    pub growing: bool,
}

/// Resource-output metadata returned after opening a source normalizer session.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SourceNormalizerResourceSessionInfo {
    #[serde(default)]
    pub session_id: Option<String>,
    #[serde(default)]
    pub normalizer_name: Option<String>,
    #[serde(default)]
    pub runtime_profile: Option<String>,
    #[serde(default)]
    pub selected_backend: Option<String>,
    pub output_route: SourceNormalizerOutputRoute,
    pub container: String,
    #[serde(default)]
    pub primary_resource_path: Option<String>,
    #[serde(default)]
    pub primary_content_type: Option<String>,
    #[serde(default)]
    pub resources: Vec<SourceNormalizerResourceInfo>,
    #[serde(default)]
    pub tracks: Vec<SourceNormalizerPacketTrackInfo>,
    #[serde(default)]
    pub duration_millis: Option<u64>,
    #[serde(default)]
    pub seekable: bool,
    #[serde(default)]
    pub disk_bytes_used: Option<u64>,
}

/// Resource-output worker state returned by `SourceNormalizerResourceSession::poll`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SourceNormalizerResourceSessionState {
    Starting,
    Ready,
    Running,
    Completed,
    Failed,
    Cancelled,
}

/// Resource-output worker status returned by a source normalizer session.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SourceNormalizerResourceSessionStatus {
    pub state: SourceNormalizerResourceSessionState,
    #[serde(default)]
    pub info: Option<SourceNormalizerResourceSessionInfo>,
    #[serde(default)]
    pub message: Option<String>,
    #[serde(default)]
    pub disk_bytes_used: Option<u64>,
}

/// Result returned after waiting for a resource-output session update.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceNormalizerResourceSessionWaitStatus {
    pub updated: bool,
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
    #[serde(default)]
    pub media_kind: SourceNormalizerPacketMediaKind,
    pub key_frame: bool,
    pub discontinuity: bool,
    #[serde(default)]
    pub sample_rate: Option<u32>,
    #[serde(default)]
    pub channels: Option<u16>,
    #[serde(default)]
    pub channel_layout: Option<String>,
    #[serde(default)]
    pub sample_format: Option<String>,
    #[serde(default)]
    pub frame_count: Option<u32>,
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

/// Success payload used by source-normalizer session operations.
///
/// Resource cancellation may report `completed = false` after accepting the
/// request while background work is still terminating. Callers observe the
/// terminal state through `poll` or `wait_for_update`.
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
    #[error("source normalizer resource exhausted: {message}")]
    ResourceExhausted { message: String },
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

    pub fn resource_exhausted(message: impl Into<String>) -> Self {
        Self::ResourceExhausted {
            message: message.into(),
        }
    }
}

/// Validates the intentionally restricted locator surface exposed to plugins.
///
/// This shared host/adapter boundary rejects sensitive data instead of
/// rewriting the locator, because query and fragment removal would change its
/// identity. Local paths retain literal `?` and `#` characters.
#[doc(hidden)]
pub fn validate_source_normalizer_plugin_input(
    input: &str,
    headers: &[(String, String)],
) -> Result<(), SourceNormalizerError> {
    if !headers.is_empty() {
        return Err(SourceNormalizerError::invalid_input(
            "Plugin sessions do not receive HTTP headers",
        ));
    }
    if is_windows_drive_absolute_path(input) {
        return Ok(());
    }
    if let Ok(url) = Url::parse(input)
        && (!url.username().is_empty()
            || url.password().is_some()
            || url.query().is_some()
            || url.fragment().is_some())
    {
        return Err(SourceNormalizerError::invalid_input(
            "Plugin sessions do not receive URL credentials, query strings, or fragments",
        ));
    }
    Ok(())
}

fn is_windows_drive_absolute_path(input: &str) -> bool {
    let bytes = input.as_bytes();
    bytes.len() >= 3
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && matches!(bytes[2], b'/' | b'\\')
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

/// Creates resource-output source normalizer sessions for one plugin.
pub trait SourceNormalizerResourcePluginFactory: Send + Sync {
    fn name(&self) -> &str;

    fn resource_capabilities(&self) -> SourceNormalizerResourceCapabilities;

    fn open_resource_session(
        &self,
        config: &SourceNormalizerResourceSessionConfig,
    ) -> Result<Box<dyn SourceNormalizerResourceSession>, SourceNormalizerError>;
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

/// Stateful resource-output source normalizer session.
pub trait SourceNormalizerResourceSession: Send {
    fn session_info(&self) -> SourceNormalizerResourceSessionInfo;

    fn poll(&mut self) -> Result<SourceNormalizerResourceSessionStatus, SourceNormalizerError>;

    fn wait_for_update(
        &mut self,
        timeout: Duration,
    ) -> Result<SourceNormalizerResourceSessionWaitStatus, SourceNormalizerError>;

    fn cancel(&mut self) -> Result<SourceNormalizerOperationStatus, SourceNormalizerError>;

    fn close(&mut self) -> Result<(), SourceNormalizerError>;
}

#[cfg(test)]
mod tests {
    use super::{
        SourceNormalizerOutputRoute, SourceNormalizerPacket, SourceNormalizerPacketCapabilities,
        SourceNormalizerPacketMediaKind, SourceNormalizerPacketSessionRequirements,
        SourceNormalizerPacketTrackInfo, SourceNormalizerReadPacketMetadata,
        SourceNormalizerReadPacketStatus, SourceNormalizerRequiredCapabilities,
        SourceNormalizerResourceCachePolicy, SourceNormalizerResourceCapabilities,
        SourceNormalizerResourceSessionRequirements, SourceNormalizerSessionCapabilities,
        SourceNormalizerSessionRequirements,
    };
    use crate::{DecoderBitstreamFormat, NativeFrameColorMetadata, NativeFrameHdrMetadata};

    #[test]
    fn source_normalizer_resource_wait_status_round_trips_through_json() {
        let status = super::SourceNormalizerResourceSessionWaitStatus { updated: true };
        let json = serde_json::to_string(&status).expect("serialize wait status");
        assert_eq!(json, r#"{"updated":true}"#);
        let decoded: super::SourceNormalizerResourceSessionWaitStatus =
            serde_json::from_str(&json).expect("decode wait status");
        assert_eq!(decoded, status);
    }

    #[test]
    fn source_normalizer_resource_capabilities_round_trip_through_json() {
        let capabilities = SourceNormalizerResourceCapabilities {
            supported_runtime_profiles: vec!["local-stream".to_owned()],
            supported_output_routes: vec![
                SourceNormalizerOutputRoute::Fmp4LocalStream,
                SourceNormalizerOutputRoute::HlsShortWindow,
            ],
            max_level: Default::default(),
            content_types: vec![
                "video/mp4".to_owned(),
                "application/vnd.apple.mpegurl".to_owned(),
            ],
            supports_growing_resources: true,
            supports_range_reads: true,
            supports_cancel: true,
            required_capabilities: SourceNormalizerRequiredCapabilities::default(),
            cache_policy: SourceNormalizerResourceCachePolicy::default(),
            max_sessions: Some(2),
        };

        let encoded = serde_json::to_string(&capabilities).expect("serialize capabilities");
        let decoded: SourceNormalizerResourceCapabilities =
            serde_json::from_str(&encoded).expect("deserialize capabilities");

        assert_eq!(decoded, capabilities);
        assert!(decoded.supports_runtime_profile("LOCAL-STREAM"));
        assert!(decoded.supports_output_route(SourceNormalizerOutputRoute::Fmp4LocalStream));
        assert_eq!(
            SourceNormalizerOutputRoute::HlsShortWindow.wire_name(),
            "hlsShortWindow"
        );
    }

    #[test]
    fn source_normalizer_packet_metadata_round_trips_through_json() {
        let metadata = SourceNormalizerReadPacketMetadata::packet(SourceNormalizerPacket {
            pts_us: Some(33_000),
            dts_us: Some(30_000),
            duration_us: Some(33_333),
            stream_index: 1,
            media_kind: SourceNormalizerPacketMediaKind::Video,
            key_frame: true,
            discontinuity: false,
            sample_rate: None,
            channels: None,
            channel_layout: None,
            sample_format: None,
            frame_count: None,
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
            reorder_depth: Some(4),
            sample_rate: None,
            channels: None,
            channel_layout: None,
            codec_delay_samples: None,
            priming_samples: None,
            trailing_padding_samples: None,
            seek_preroll_samples: None,
            color: Some(NativeFrameColorMetadata {
                primaries: Some("bt2020".to_owned()),
                transfer: Some("smpte2084".to_owned()),
                matrix: Some("bt2020-ncl".to_owned()),
                range: Some("limited".to_owned()),
                bit_depth: Some(10),
            }),
            hdr: Some(NativeFrameHdrMetadata {
                kind: "hdr10".to_owned(),
                mastering_display: None,
                content_light: None,
                dolby_vision: None,
            }),
            frame_rate: Some(30.0),
            time_base_num: Some(1),
            time_base_den: Some(90_000),
        };

        let encoded = serde_json::to_string(&track).expect("serialize track");
        let decoded: SourceNormalizerPacketTrackInfo =
            serde_json::from_str(&encoded).expect("deserialize track");

        assert_eq!(decoded, track);
        assert_eq!(
            decoded.color.as_ref().and_then(|color| color.bit_depth),
            Some(10)
        );
        assert_eq!(
            decoded.hdr.as_ref().map(|hdr| hdr.kind.as_str()),
            Some("hdr10")
        );
    }

    #[test]
    fn source_normalizer_audio_packet_track_info_round_trips_through_json() {
        let track = SourceNormalizerPacketTrackInfo {
            stream_index: 1,
            media_kind: SourceNormalizerPacketMediaKind::Audio,
            codec: "AAC".to_owned(),
            extradata: vec![0x12, 0x10],
            bitstream_format: Some(DecoderBitstreamFormat::Unknown("AAC".to_owned())),
            width: None,
            height: None,
            coded_width: None,
            coded_height: None,
            reorder_depth: None,
            sample_rate: Some(48_000),
            channels: Some(2),
            channel_layout: Some("stereo".to_owned()),
            codec_delay_samples: Some(0),
            priming_samples: Some(2_112),
            trailing_padding_samples: Some(512),
            seek_preroll_samples: Some(1_024),
            color: None,
            hdr: None,
            frame_rate: None,
            time_base_num: Some(1),
            time_base_den: Some(48_000),
        };

        let encoded = serde_json::to_string(&track).expect("serialize audio track");
        let decoded: SourceNormalizerPacketTrackInfo =
            serde_json::from_str(&encoded).expect("deserialize audio track");

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
        assert!(capabilities.supports_media_kind(SourceNormalizerPacketMediaKind::Video));
        assert!(capabilities.supports_bitstream_format(&DecoderBitstreamFormat::Avcc));
    }

    #[test]
    fn source_normalizer_packet_requirements_report_missing_capabilities() {
        let requirements = SourceNormalizerPacketSessionRequirements {
            require_seek: true,
            bitstream_format: Some(DecoderBitstreamFormat::Avcc),
            ..SourceNormalizerPacketSessionRequirements::native_video("native-frame-vod", "h264")
        };
        let capabilities = SourceNormalizerPacketCapabilities {
            supported_runtime_profiles: vec!["other".to_owned()],
            media_kinds: vec![SourceNormalizerPacketMediaKind::Audio],
            codecs: vec!["hevc".to_owned()],
            bitstream_formats: vec![DecoderBitstreamFormat::AnnexB],
            supports_seek: false,
            supports_flush: false,
            ..Default::default()
        };

        let missing = requirements.missing_capabilities(&capabilities);

        assert!(
            missing
                .iter()
                .any(|item| item == "runtime profile native-frame-vod")
        );
        assert!(missing.iter().any(|item| item == "packet media kind Video"));
        assert!(missing.iter().any(|item| item == "packet codec h264"));
        assert!(
            missing
                .iter()
                .any(|item| item == "packet bitstream format Avcc")
        );
        assert!(missing.iter().any(|item| item == "packet seek support"));
        assert!(missing.iter().any(|item| item == "packet flush support"));
        assert!(
            missing
                .iter()
                .any(|item| item == "outstanding lease cleanup")
        );
    }

    #[test]
    fn source_normalizer_packet_requirements_treat_empty_profile_as_auto_detected() {
        let requirements = SourceNormalizerPacketSessionRequirements {
            runtime_profile: String::new(),
            media_kind: Some(SourceNormalizerPacketMediaKind::Video),
            codec: None,
            bitstream_format: None,
            require_seek: false,
            require_flush: true,
            require_lease_cleanup: true,
        };
        let capabilities = SourceNormalizerPacketCapabilities {
            supported_runtime_profiles: vec!["native-frame-vod".to_owned()],
            media_kinds: vec![SourceNormalizerPacketMediaKind::Video],
            supports_flush: true,
            ..Default::default()
        };

        assert!(requirements.missing_capabilities(&capabilities).is_empty());
    }

    #[test]
    fn source_normalizer_session_requirements_match_route_kind() {
        let requirements = SourceNormalizerSessionRequirements::Resource(
            SourceNormalizerResourceSessionRequirements {
                runtime_profile: "vod-resource".to_owned(),
                output_route: SourceNormalizerOutputRoute::Fmp4LocalStream,
                content_type: Some("video/mp4".to_owned()),
                require_growing_resources: true,
                require_range_reads: true,
                require_cancel: true,
            },
        );
        let packet_capabilities = SourceNormalizerPacketCapabilities::default();
        let missing = requirements.missing_capabilities(
            &SourceNormalizerSessionCapabilities::Packet(&packet_capabilities),
        );
        assert_eq!(missing, vec!["resource output route".to_owned()]);

        let resource_capabilities = SourceNormalizerResourceCapabilities {
            supported_runtime_profiles: vec!["vod-resource".to_owned()],
            supported_output_routes: vec![SourceNormalizerOutputRoute::Fmp4LocalStream],
            content_types: vec!["video/mp4".to_owned()],
            supports_growing_resources: true,
            supports_range_reads: true,
            supports_cancel: true,
            ..Default::default()
        };
        let missing = requirements.missing_capabilities(
            &SourceNormalizerSessionCapabilities::Resource(&resource_capabilities),
        );
        assert!(missing.is_empty());
    }
}
