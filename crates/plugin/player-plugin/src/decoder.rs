use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{
    NativeFrame, NativeFrameColorMetadata, NativeFrameHdrMetadata, NativeFrameMetadata,
    NativeFramePipelineProfile, NativeFrameReleaseTracking, NativeFrameSyncInfo,
    NativeFrameTransform, NativeHandleKind, SourceNormalizerPacketMediaKind,
    SourceNormalizerPacketTrackInfo, VisibleRect,
};

/// Media kind handled by a decoder plugin.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum DecoderMediaKind {
    #[default]
    Video,
    Audio,
}

/// Decoded frame formats advertised by decoder plugins.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum DecoderFrameFormat {
    Rgba8888,
    Bgra8888,
    Yuv420p,
    Nv12,
    /// 10-bit 4:2:0 bi-planar YUV, commonly exposed as P010.
    P010,
    /// 32-bit floating point PCM samples.
    F32,
    /// Signed 16-bit PCM samples.
    S16,
    Unknown(String),
}

/// PCM sample layout returned by audio decoder plugins.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum DecoderPcmSampleLayout {
    #[default]
    Interleaved,
    Planar,
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
    /// Supports decoded audio frames in plugin-managed audio sessions.
    pub supports_audio_frames: bool,
    /// Supports decoded PCM frame output through `receive_pcm_frame`.
    #[serde(default)]
    pub supports_pcm_frames: bool,
    pub supports_gpu_handles: bool,
    /// Supports release calls that distinguish presented frames from discarded frames.
    #[serde(default)]
    pub supports_presentation_release: bool,
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

/// Requirements a host session needs from a decoder plugin.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct DecoderSessionRequirements {
    pub codec: String,
    pub media_kind: DecoderMediaKind,
    #[serde(default)]
    pub native_handle_kind: Option<DecoderNativeHandleKind>,
    #[serde(default)]
    pub pipeline_profile: Option<NativeFramePipelineProfile>,
    #[serde(default)]
    pub native_device_context_kind: Option<DecoderNativeDeviceContextKind>,
    #[serde(default)]
    pub require_presentation_release: bool,
    #[serde(default)]
    pub require_pcm_output: bool,
}

impl DecoderSessionRequirements {
    /// Builds video native-frame requirements for an output handle/profile pair.
    pub fn native_video(
        codec: impl Into<String>,
        native_handle_kind: DecoderNativeHandleKind,
        pipeline_profile: NativeFramePipelineProfile,
    ) -> Self {
        Self {
            codec: codec.into(),
            media_kind: DecoderMediaKind::Video,
            native_handle_kind: Some(native_handle_kind),
            pipeline_profile: Some(pipeline_profile),
            ..Self::default()
        }
    }

    /// Returns missing capability names for this requirement.
    pub fn missing_capabilities(
        &self,
        capabilities: &DecoderCapabilities,
        native_requirements: &DecoderNativeRequirements,
    ) -> Vec<String> {
        let mut missing = Vec::new();
        if !capabilities.supports_codec(&self.codec, self.media_kind) {
            missing.push(format!("{:?} codec {}", self.media_kind, self.codec));
        }
        if self.require_pcm_output && !capabilities.supports_pcm_frames {
            missing.push("supportsPcmFrames".to_owned());
        }
        if self.require_presentation_release && !capabilities.supports_presentation_release {
            missing.push("supportsPresentationRelease".to_owned());
        }
        if let Some(handle_kind) = &self.native_handle_kind
            && !native_requirements
                .output_handle_kinds
                .contains(handle_kind)
        {
            missing.push(format!("outputHandleKind::{handle_kind:?}"));
        }
        if let Some(profile) = &self.pipeline_profile
            && !native_requirements
                .output_pipeline_profiles
                .contains(profile)
        {
            missing.push(format!("pipelineProfile::{profile:?}"));
        }
        if let Some(context_kind) = &self.native_device_context_kind
            && !native_requirements
                .required_device_context_kinds
                .contains(context_kind)
        {
            missing.push(format!("nativeDeviceContext::{context_kind:?}"));
        }
        missing
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
    #[serde(default)]
    pub reorder_depth: Option<u32>,
    pub sample_rate: Option<u32>,
    pub channels: Option<u16>,
    #[serde(default)]
    pub channel_layout: Option<String>,
    #[serde(default)]
    pub target_pcm_format: Option<DecoderFrameFormat>,
    #[serde(default)]
    pub target_pcm_sample_layout: Option<DecoderPcmSampleLayout>,
    #[serde(default)]
    pub codec_delay_samples: Option<u32>,
    #[serde(default)]
    pub priming_samples: Option<u32>,
    #[serde(default)]
    pub trailing_padding_samples: Option<u32>,
    #[serde(default)]
    pub seek_preroll_samples: Option<u32>,
    pub prefer_hardware: bool,
    pub require_cpu_output: bool,
    #[serde(default)]
    pub native_device_context: Option<DecoderNativeDeviceContext>,
    #[serde(default)]
    pub color: Option<NativeFrameColorMetadata>,
    #[serde(default)]
    pub hdr: Option<NativeFrameHdrMetadata>,
}

impl DecoderSessionConfig {
    /// Builds an audio decoder session config from a SourceNormalizer audio track.
    pub fn audio_from_source_normalizer_track(
        track: &SourceNormalizerPacketTrackInfo,
        target_pcm_format: DecoderFrameFormat,
        target_pcm_sample_layout: DecoderPcmSampleLayout,
    ) -> Result<Self, DecoderError> {
        if track.media_kind != SourceNormalizerPacketMediaKind::Audio {
            return Err(DecoderError::UnsupportedCapability {
                capability: "source-normalizer-audio-track".to_owned(),
            });
        }
        Ok(Self {
            codec: track.codec.clone(),
            media_kind: DecoderMediaKind::Audio,
            extradata: track.extradata.clone(),
            bitstream_format: track.bitstream_format.clone(),
            sample_rate: track.sample_rate,
            channels: track.channels,
            channel_layout: track.channel_layout.clone(),
            target_pcm_format: Some(target_pcm_format),
            target_pcm_sample_layout: Some(target_pcm_sample_layout),
            codec_delay_samples: track.codec_delay_samples,
            priming_samples: track.priming_samples,
            trailing_padding_samples: track.trailing_padding_samples,
            seek_preroll_samples: track.seek_preroll_samples,
            color: track.color.clone(),
            hdr: track.hdr.clone(),
            prefer_hardware: true,
            require_cpu_output: true,
            ..Self::default()
        })
    }

    /// Builds the default Apple PCM output preference for native audio.
    pub fn apple_native_audio_from_source_normalizer_track(
        track: &SourceNormalizerPacketTrackInfo,
    ) -> Result<Self, DecoderError> {
        Self::audio_from_source_normalizer_track(
            track,
            DecoderFrameFormat::F32,
            DecoderPcmSampleLayout::Interleaved,
        )
    }
}

/// Optional session metadata returned by a plugin after opening a decoder.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct DecoderSessionInfo {
    pub decoder_name: Option<String>,
    pub selected_hardware_backend: Option<String>,
    pub output_format: Option<DecoderFrameFormat>,
}

/// Compressed packet metadata passed to `NativeDecoderSession::send_packet`.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct DecoderPacket {
    pub pts_us: Option<i64>,
    pub dts_us: Option<i64>,
    pub duration_us: Option<i64>,
    pub stream_index: u32,
    #[serde(default)]
    pub media_kind: DecoderMediaKind,
    pub key_frame: bool,
    pub discontinuity: bool,
    #[serde(default)]
    pub end_of_stream: bool,
}

impl TryFrom<crate::SourceNormalizerPacket> for DecoderPacket {
    type Error = DecoderError;

    fn try_from(packet: crate::SourceNormalizerPacket) -> Result<Self, Self::Error> {
        let media_kind = match packet.media_kind {
            SourceNormalizerPacketMediaKind::Audio => DecoderMediaKind::Audio,
            SourceNormalizerPacketMediaKind::Video => DecoderMediaKind::Video,
            SourceNormalizerPacketMediaKind::Subtitle => {
                return Err(DecoderError::UnsupportedCapability {
                    capability: "source-normalizer-subtitle-packet".to_owned(),
                });
            }
        };
        Ok(Self {
            pts_us: packet.pts_us,
            dts_us: packet.dts_us,
            duration_us: packet.duration_us,
            stream_index: packet.stream_index,
            media_kind,
            key_frame: packet.key_frame,
            discontinuity: packet.discontinuity,
            end_of_stream: packet.end_of_stream,
        })
    }
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

/// Receive state encoded in frame metadata over the C ABI.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DecoderReceiveFrameStatus {
    Frame,
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
    MediaCodecHardwareBuffer,
    MediaCodecSurfaceTexture,
    Unknown(String),
}

impl From<DecoderNativeHandleKind> for NativeHandleKind {
    fn from(value: DecoderNativeHandleKind) -> Self {
        match value {
            DecoderNativeHandleKind::CvPixelBuffer => Self::CvPixelBuffer,
            DecoderNativeHandleKind::IoSurface => Self::IoSurface,
            DecoderNativeHandleKind::MetalTexture => Self::MetalTexture,
            DecoderNativeHandleKind::DmaBuf => Self::DmaBuf,
            DecoderNativeHandleKind::VaapiSurface => Self::VaapiSurface,
            DecoderNativeHandleKind::D3D11Texture2D => Self::D3D11Texture2D,
            DecoderNativeHandleKind::DxgiSurface => Self::DxgiSurface,
            DecoderNativeHandleKind::VulkanImage => Self::VulkanImage,
            DecoderNativeHandleKind::MediaCodecHardwareBuffer => Self::MediaCodecHardwareBuffer,
            DecoderNativeHandleKind::MediaCodecSurfaceTexture => Self::MediaCodecSurfaceTexture,
            DecoderNativeHandleKind::Unknown(name) => Self::Unknown(name),
        }
    }
}

impl From<NativeHandleKind> for DecoderNativeHandleKind {
    fn from(value: NativeHandleKind) -> Self {
        match value {
            NativeHandleKind::CvPixelBuffer => Self::CvPixelBuffer,
            NativeHandleKind::IoSurface => Self::IoSurface,
            NativeHandleKind::MetalTexture => Self::MetalTexture,
            NativeHandleKind::DmaBuf => Self::DmaBuf,
            NativeHandleKind::VaapiSurface => Self::VaapiSurface,
            NativeHandleKind::D3D11Texture2D => Self::D3D11Texture2D,
            NativeHandleKind::DxgiSurface => Self::DxgiSurface,
            NativeHandleKind::VulkanImage => Self::VulkanImage,
            NativeHandleKind::MediaCodecHardwareBuffer => Self::MediaCodecHardwareBuffer,
            NativeHandleKind::MediaCodecSurfaceTexture => Self::MediaCodecSurfaceTexture,
            NativeHandleKind::Unknown(name) => Self::Unknown(name),
        }
    }
}

/// Native graphics device/context kinds that a host may share with a decoder plugin.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum DecoderNativeDeviceContextKind {
    D3D11Device,
    AndroidNativeWindow,
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
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DecoderNativeDeviceContext {
    #[serde(rename = "d3d11_device")]
    D3D11Device {
        device_ptr: usize,
    },
    #[serde(rename = "android_native_window")]
    AndroidNativeWindow {
        window_ptr: usize,
    },
    Unknown {
        name: String,
    },
}

impl DecoderNativeDeviceContext {
    pub fn kind(&self) -> DecoderNativeDeviceContextKind {
        match self {
            Self::D3D11Device { .. } => DecoderNativeDeviceContextKind::D3D11Device,
            Self::AndroidNativeWindow { .. } => DecoderNativeDeviceContextKind::AndroidNativeWindow,
            Self::Unknown { name } => DecoderNativeDeviceContextKind::Unknown(name.clone()),
        }
    }

    pub fn d3d11_device_ptr(&self) -> Option<usize> {
        match self {
            Self::D3D11Device { device_ptr } => Some(*device_ptr),
            Self::AndroidNativeWindow { .. } | Self::Unknown { .. } => None,
        }
    }

    pub fn android_native_window_ptr(&self) -> Option<usize> {
        match self {
            Self::AndroidNativeWindow { window_ptr } => Some(*window_ptr),
            Self::D3D11Device { .. } | Self::Unknown { .. } => None,
        }
    }
}

/// Native-frame decoder requirements advertised through ABI v2.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct DecoderNativeRequirements {
    pub required_device_context_kinds: Vec<DecoderNativeDeviceContextKind>,
    pub output_handle_kinds: Vec<DecoderNativeHandleKind>,
    #[serde(default)]
    pub output_pipeline_profiles: Vec<NativeFramePipelineProfile>,
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

impl From<DecoderVisibleRect> for VisibleRect {
    fn from(value: DecoderVisibleRect) -> Self {
        Self {
            x: value.x,
            y: value.y,
            width: value.width,
            height: value.height,
        }
    }
}

impl From<VisibleRect> for DecoderVisibleRect {
    fn from(value: VisibleRect) -> Self {
        Self {
            x: value.x,
            y: value.y,
            width: value.width,
            height: value.height,
        }
    }
}

/// Release tracking diagnostics attached to a native frame.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DecoderNativeFrameReleaseTracking {
    pub frame_id: Option<u64>,
    pub requires_release: bool,
}

impl From<DecoderNativeFrameReleaseTracking> for NativeFrameReleaseTracking {
    fn from(value: DecoderNativeFrameReleaseTracking) -> Self {
        Self {
            frame_id: value.frame_id,
            requires_release: value.requires_release,
        }
    }
}

impl From<NativeFrameReleaseTracking> for DecoderNativeFrameReleaseTracking {
    fn from(value: NativeFrameReleaseTracking) -> Self {
        Self {
            frame_id: value.frame_id,
            requires_release: value.requires_release,
        }
    }
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
    pub pipeline_profile: Option<NativeFramePipelineProfile>,
    #[serde(default)]
    pub color_space: Option<String>,
    #[serde(default)]
    pub hdr_metadata: Option<String>,
    #[serde(default)]
    pub color: Option<NativeFrameColorMetadata>,
    #[serde(default)]
    pub hdr: Option<NativeFrameHdrMetadata>,
    #[serde(default)]
    pub sync_info: Option<NativeFrameSyncInfo>,
    #[serde(default)]
    pub transform: Option<NativeFrameTransform>,
    #[serde(default)]
    pub frame_id: Option<u64>,
    #[serde(default)]
    pub release_tracking: Option<DecoderNativeFrameReleaseTracking>,
}

impl From<DecoderNativeFrameMetadata> for NativeFrameMetadata {
    fn from(value: DecoderNativeFrameMetadata) -> Self {
        Self {
            media_kind: value.media_kind,
            format: value.format,
            codec: value.codec,
            pts_us: value.pts_us,
            duration_us: value.duration_us,
            width: value.width,
            height: value.height,
            coded_width: value.coded_width,
            coded_height: value.coded_height,
            visible_rect: value.visible_rect.map(Into::into),
            handle_kind: value.handle_kind.into(),
            pipeline_profile: value.pipeline_profile,
            color_space: value.color_space,
            hdr_metadata: value.hdr_metadata,
            color: value.color,
            hdr: value.hdr,
            sync_info: value.sync_info,
            transform: value.transform,
            frame_id: value.frame_id,
            release_tracking: value.release_tracking.map(Into::into),
        }
    }
}

impl From<NativeFrameMetadata> for DecoderNativeFrameMetadata {
    fn from(value: NativeFrameMetadata) -> Self {
        Self {
            media_kind: value.media_kind,
            format: value.format,
            codec: value.codec,
            pts_us: value.pts_us,
            duration_us: value.duration_us,
            width: value.width,
            height: value.height,
            coded_width: value.coded_width,
            coded_height: value.coded_height,
            visible_rect: value.visible_rect.map(Into::into),
            handle_kind: value.handle_kind.into(),
            pipeline_profile: value.pipeline_profile,
            color_space: value.color_space,
            hdr_metadata: value.hdr_metadata,
            color: value.color,
            hdr: value.hdr,
            sync_info: value.sync_info,
            transform: value.transform,
            frame_id: value.frame_id,
            release_tracking: value.release_tracking.map(Into::into),
        }
    }
}

/// A decoded native frame returned by the Rust-side decoder session trait.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecoderNativeFrame {
    pub metadata: DecoderNativeFrameMetadata,
    pub handle: usize,
}

impl From<DecoderNativeFrame> for NativeFrame {
    fn from(value: DecoderNativeFrame) -> Self {
        Self {
            metadata: value.metadata.into(),
            handle: value.handle,
        }
    }
}

impl From<NativeFrame> for DecoderNativeFrame {
    fn from(value: NativeFrame) -> Self {
        Self {
            metadata: value.metadata.into(),
            handle: value.handle,
        }
    }
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

/// Metadata for a decoded PCM audio frame.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DecoderPcmFrameMetadata {
    pub media_kind: DecoderMediaKind,
    pub format: DecoderFrameFormat,
    pub codec: String,
    pub pts_us: Option<i64>,
    pub duration_us: Option<i64>,
    pub sample_rate: u32,
    pub channels: u16,
    #[serde(default)]
    pub channel_layout: Option<String>,
    pub sample_layout: DecoderPcmSampleLayout,
    pub frame_count: u32,
    #[serde(default)]
    pub discontinuity: bool,
}

impl DecoderPcmFrameMetadata {
    /// Creates PCM metadata and pins `media_kind` to audio.
    pub fn audio(
        codec: impl Into<String>,
        format: DecoderFrameFormat,
        sample_rate: u32,
        channels: u16,
        sample_layout: DecoderPcmSampleLayout,
        frame_count: u32,
    ) -> Self {
        Self {
            media_kind: DecoderMediaKind::Audio,
            format,
            codec: codec.into(),
            pts_us: None,
            duration_us: None,
            sample_rate,
            channels,
            channel_layout: None,
            sample_layout,
            frame_count,
            discontinuity: false,
        }
    }
}

/// A decoded PCM audio frame returned by an audio decoder session.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DecoderPcmFrame {
    pub metadata: DecoderPcmFrameMetadata,
    pub data: Vec<u8>,
}

/// Receive state encoded in PCM frame metadata over the future audio decoder ABI.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DecoderReceivePcmFrameMetadata {
    pub status: DecoderReceiveFrameStatus,
    pub frame: Option<DecoderPcmFrameMetadata>,
}

impl DecoderReceivePcmFrameMetadata {
    pub fn frame(frame: DecoderPcmFrameMetadata) -> Self {
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

/// Rust-side receive result returned by audio decoder sessions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DecoderReceivePcmFrameOutput {
    Frame(DecoderPcmFrame),
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
    #[error("unsupported decoder capability: {capability}")]
    UnsupportedCapability { capability: String },
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

/// Creates native-frame decoder sessions for one plugin.
pub trait NativeDecoderPluginFactory: Send + Sync {
    fn name(&self) -> &str;

    fn capabilities(&self) -> DecoderCapabilities;

    fn native_requirements(&self) -> DecoderNativeRequirements {
        DecoderNativeRequirements::default()
    }

    fn supports_native_frame_presentation_release(&self) -> bool {
        self.capabilities().supports_presentation_release
    }

    fn open_native_session(
        &self,
        config: &DecoderSessionConfig,
    ) -> Result<Box<dyn NativeDecoderSession>, DecoderError>;
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

    fn receive_pcm_frame(&mut self) -> Result<DecoderReceivePcmFrameOutput, DecoderError> {
        Err(DecoderError::UnsupportedCapability {
            capability: "audio-pcm-output".to_owned(),
        })
    }

    fn release_native_frame(&mut self, frame: DecoderNativeFrame) -> Result<(), DecoderError>;

    fn release_native_frame_with_presentation(
        &mut self,
        _frame: DecoderNativeFrame,
        _presented: bool,
    ) -> Result<(), DecoderError> {
        Err(DecoderError::UnsupportedCapability {
            capability: "presentation-aware-native-frame-release".to_owned(),
        })
    }

    fn flush(&mut self) -> Result<(), DecoderError>;

    fn close(&mut self) -> Result<(), DecoderError>;
}

#[cfg(test)]
mod tests {
    use super::{
        DecoderBitstreamFormat, DecoderError, DecoderFrameFormat, DecoderMediaKind,
        DecoderNativeDeviceContext, DecoderNativeDeviceContextKind, DecoderNativeFrame,
        DecoderNativeFrameMetadata, DecoderNativeFrameReleaseTracking, DecoderNativeHandleKind,
        DecoderPacket, DecoderPacketResult, DecoderPcmFrame, DecoderPcmFrameMetadata,
        DecoderPcmSampleLayout, DecoderReceiveFrameStatus, DecoderReceiveNativeFrameOutput,
        DecoderReceivePcmFrameMetadata, DecoderSessionConfig, DecoderSessionInfo,
        DecoderVisibleRect, NativeDecoderSession,
    };
    use crate::{
        NativeFrame, NativeFrameColorMetadata, NativeFrameHdrMetadata, NativeFrameMetadata,
        NativeFramePipelineProfile, NativeFrameSyncInfo, NativeFrameTransform, NativeHandleKind,
    };

    fn decoder_native_frame() -> DecoderNativeFrame {
        DecoderNativeFrame {
            metadata: DecoderNativeFrameMetadata {
                media_kind: DecoderMediaKind::Video,
                format: DecoderFrameFormat::Nv12,
                codec: "hevc".to_owned(),
                pts_us: Some(125_000),
                duration_us: Some(41_667),
                width: 3_840,
                height: 2_160,
                coded_width: Some(3_840),
                coded_height: Some(2_176),
                visible_rect: Some(DecoderVisibleRect {
                    x: 0,
                    y: 0,
                    width: 3_840,
                    height: 2_160,
                }),
                handle_kind: DecoderNativeHandleKind::D3D11Texture2D,
                pipeline_profile: Some(NativeFramePipelineProfile::D3D11Texture2D),
                color_space: Some("bt709".to_owned()),
                hdr_metadata: Some("hdr10".to_owned()),
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
                sync_info: Some(NativeFrameSyncInfo {
                    kind: "d3d11_keyed_mutex".to_owned(),
                    handle: None,
                    value: Some(1),
                }),
                transform: Some(NativeFrameTransform {
                    rotation_degrees: 0,
                    mirrored_horizontal: false,
                    mirrored_vertical: false,
                }),
                frame_id: Some(99),
                release_tracking: Some(DecoderNativeFrameReleaseTracking {
                    frame_id: Some(99),
                    requires_release: true,
                }),
            },
            handle: 0xfeed,
        }
    }

    #[test]
    fn decoder_native_frame_converts_to_shared_native_frame() {
        let decoder_frame = decoder_native_frame();
        let native_frame = NativeFrame::from(decoder_frame.clone());

        assert_eq!(native_frame.handle, decoder_frame.handle);
        assert_eq!(
            native_frame.metadata.handle_kind,
            NativeHandleKind::D3D11Texture2D
        );
        assert_eq!(
            native_frame
                .metadata
                .visible_rect
                .as_ref()
                .map(|rect| rect.height),
            Some(2_160)
        );
        assert_eq!(
            native_frame
                .metadata
                .release_tracking
                .as_ref()
                .map(|tracking| tracking.requires_release),
            Some(true)
        );
    }

    #[test]
    fn shared_native_frame_converts_back_to_decoder_native_frame() {
        let original = decoder_native_frame();
        let native_frame = NativeFrame::from(original.clone());
        let recovered = DecoderNativeFrame::from(native_frame);

        assert_eq!(recovered, original);
    }

    #[test]
    fn native_frame_metadata_converts_to_decoder_metadata() {
        let metadata = NativeFrameMetadata::from(decoder_native_frame().metadata);
        let decoder_metadata = DecoderNativeFrameMetadata::from(metadata);

        assert_eq!(
            decoder_metadata.handle_kind,
            DecoderNativeHandleKind::D3D11Texture2D
        );
        assert_eq!(
            decoder_metadata.pipeline_profile,
            Some(NativeFramePipelineProfile::D3D11Texture2D)
        );
        assert_eq!(decoder_metadata.color_space.as_deref(), Some("bt709"));
        assert_eq!(decoder_metadata.frame_id, Some(99));
        assert_eq!(
            decoder_metadata
                .visible_rect
                .as_ref()
                .map(|rect| rect.width),
            Some(3_840)
        );
    }

    #[test]
    fn android_native_handle_kinds_round_trip_between_decoder_and_shared_frames() {
        for handle_kind in [
            DecoderNativeHandleKind::MediaCodecHardwareBuffer,
            DecoderNativeHandleKind::MediaCodecSurfaceTexture,
        ] {
            let shared = NativeHandleKind::from(handle_kind.clone());
            let recovered = DecoderNativeHandleKind::from(shared);

            assert_eq!(recovered, handle_kind);
        }
    }

    #[test]
    fn android_native_window_device_context_round_trips_json_and_kind() {
        let context = DecoderNativeDeviceContext::AndroidNativeWindow { window_ptr: 0xabc };

        let encoded = serde_json::to_string(&context).expect("serialize Android native context");
        let decoded: DecoderNativeDeviceContext =
            serde_json::from_str(&encoded).expect("deserialize Android native context");

        assert_eq!(
            decoded.kind(),
            DecoderNativeDeviceContextKind::AndroidNativeWindow
        );
        assert_eq!(decoded.android_native_window_ptr(), Some(0xabc));
        assert_eq!(decoded.d3d11_device_ptr(), None);
    }

    #[test]
    fn pcm_frame_metadata_pins_media_kind_to_audio_and_round_trips_json() {
        let mut metadata = DecoderPcmFrameMetadata::audio(
            "aac",
            DecoderFrameFormat::F32,
            48_000,
            2,
            DecoderPcmSampleLayout::Planar,
            1_024,
        );
        metadata.pts_us = Some(1_000_000);
        metadata.duration_us = Some(21_333);
        metadata.channel_layout = Some("stereo".to_owned());
        metadata.discontinuity = true;
        let frame = DecoderPcmFrame {
            metadata,
            data: vec![0, 1, 2, 3],
        };

        let encoded = serde_json::to_vec(&frame).expect("pcm frame json encode");
        let decoded: DecoderPcmFrame =
            serde_json::from_slice(&encoded).expect("pcm frame json decode");

        assert_eq!(decoded.metadata.media_kind, DecoderMediaKind::Audio);
        assert_eq!(decoded.metadata.codec, "aac");
        assert_eq!(decoded.metadata.format, DecoderFrameFormat::F32);
        assert_eq!(
            decoded.metadata.sample_layout,
            DecoderPcmSampleLayout::Planar
        );
        assert_eq!(decoded.metadata.frame_count, 1_024);
        assert_eq!(decoded.metadata.channel_layout.as_deref(), Some("stereo"));
        assert!(decoded.metadata.discontinuity);
        assert_eq!(decoded.data, vec![0, 1, 2, 3]);
    }

    #[test]
    fn pcm_receive_metadata_uses_shared_receive_statuses() {
        let frame = DecoderPcmFrameMetadata::audio(
            "aac",
            DecoderFrameFormat::F32,
            48_000,
            2,
            DecoderPcmSampleLayout::Interleaved,
            512,
        );

        assert_eq!(
            DecoderReceivePcmFrameMetadata::frame(frame.clone()).status,
            DecoderReceiveFrameStatus::Frame
        );
        assert_eq!(
            DecoderReceivePcmFrameMetadata::frame(frame)
                .frame
                .map(|metadata| metadata.media_kind),
            Some(DecoderMediaKind::Audio)
        );
        assert_eq!(
            DecoderReceivePcmFrameMetadata::need_more_input().status,
            DecoderReceiveFrameStatus::NeedMoreInput
        );
        assert_eq!(
            DecoderReceivePcmFrameMetadata::eof().status,
            DecoderReceiveFrameStatus::Eof
        );
    }

    #[test]
    fn decoder_packet_preserves_source_normalizer_media_kind() {
        let video = crate::SourceNormalizerPacket {
            pts_us: Some(1_000),
            dts_us: Some(900),
            duration_us: Some(33_333),
            stream_index: 0,
            media_kind: crate::SourceNormalizerPacketMediaKind::Video,
            key_frame: true,
            discontinuity: true,
            ..crate::SourceNormalizerPacket::default()
        };
        let video_packet = DecoderPacket::try_from(video).expect("video packet maps");
        assert_eq!(video_packet.media_kind, DecoderMediaKind::Video);
        assert_eq!(video_packet.stream_index, 0);
        assert!(video_packet.key_frame);
        assert!(video_packet.discontinuity);

        let audio = crate::SourceNormalizerPacket {
            pts_us: Some(2_000),
            dts_us: Some(2_000),
            duration_us: Some(21_333),
            stream_index: 1,
            media_kind: crate::SourceNormalizerPacketMediaKind::Audio,
            sample_rate: Some(48_000),
            channels: Some(2),
            ..crate::SourceNormalizerPacket::default()
        };
        let audio_packet = DecoderPacket::try_from(audio).expect("audio packet maps");
        assert_eq!(audio_packet.media_kind, DecoderMediaKind::Audio);
        assert_eq!(audio_packet.stream_index, 1);
        assert_eq!(audio_packet.duration_us, Some(21_333));
    }

    #[test]
    fn decoder_packet_rejects_source_normalizer_subtitle_packet() {
        let subtitle = crate::SourceNormalizerPacket {
            stream_index: 2,
            media_kind: crate::SourceNormalizerPacketMediaKind::Subtitle,
            ..crate::SourceNormalizerPacket::default()
        };

        let error = DecoderPacket::try_from(subtitle)
            .expect_err("subtitle packets are not decoder packet input");

        assert!(matches!(
            error,
            DecoderError::UnsupportedCapability { capability }
                if capability == "source-normalizer-subtitle-packet"
        ));
    }

    #[test]
    fn audio_decoder_session_config_round_trips_pcm_output_preferences() {
        let config = DecoderSessionConfig {
            codec: "aac".to_owned(),
            media_kind: DecoderMediaKind::Audio,
            extradata: vec![0x12, 0x10],
            bitstream_format: Some(DecoderBitstreamFormat::Unknown("adts".to_owned())),
            sample_rate: Some(48_000),
            channels: Some(2),
            channel_layout: Some("stereo".to_owned()),
            target_pcm_format: Some(DecoderFrameFormat::F32),
            target_pcm_sample_layout: Some(DecoderPcmSampleLayout::Interleaved),
            codec_delay_samples: Some(0),
            priming_samples: Some(2_112),
            trailing_padding_samples: Some(512),
            seek_preroll_samples: Some(1_024),
            color: Some(NativeFrameColorMetadata {
                primaries: Some("bt709".to_owned()),
                transfer: Some("bt709".to_owned()),
                matrix: Some("bt709".to_owned()),
                range: Some("limited".to_owned()),
                bit_depth: Some(8),
            }),
            hdr: None,
            ..DecoderSessionConfig::default()
        };

        let encoded = serde_json::to_vec(&config).expect("audio config json encode");
        let decoded: DecoderSessionConfig =
            serde_json::from_slice(&encoded).expect("audio config json decode");

        assert_eq!(decoded.media_kind, DecoderMediaKind::Audio);
        assert_eq!(decoded.sample_rate, Some(48_000));
        assert_eq!(decoded.channels, Some(2));
        assert_eq!(decoded.channel_layout.as_deref(), Some("stereo"));
        assert_eq!(decoded.target_pcm_format, Some(DecoderFrameFormat::F32));
        assert_eq!(
            decoded.target_pcm_sample_layout,
            Some(DecoderPcmSampleLayout::Interleaved)
        );
        assert_eq!(decoded.codec_delay_samples, Some(0));
        assert_eq!(decoded.priming_samples, Some(2_112));
        assert_eq!(decoded.trailing_padding_samples, Some(512));
        assert_eq!(decoded.seek_preroll_samples, Some(1_024));
        assert_eq!(
            decoded.color.as_ref().and_then(|color| color.bit_depth),
            Some(8)
        );
    }

    #[test]
    fn audio_decoder_session_config_maps_source_normalizer_audio_track() {
        let track = crate::SourceNormalizerPacketTrackInfo {
            stream_index: 1,
            media_kind: crate::SourceNormalizerPacketMediaKind::Audio,
            codec: "AAC".to_owned(),
            extradata: vec![0x12, 0x10],
            bitstream_format: Some(DecoderBitstreamFormat::Unknown("adts".to_owned())),
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
            color: Some(NativeFrameColorMetadata {
                primaries: Some("bt709".to_owned()),
                transfer: Some("bt709".to_owned()),
                matrix: Some("bt709".to_owned()),
                range: Some("limited".to_owned()),
                bit_depth: Some(8),
            }),
            hdr: None,
            frame_rate: None,
            time_base_num: Some(1),
            time_base_den: Some(48_000),
        };

        let config = DecoderSessionConfig::apple_native_audio_from_source_normalizer_track(&track)
            .expect("audio track maps to decoder config");

        assert_eq!(config.codec, "AAC");
        assert_eq!(config.media_kind, DecoderMediaKind::Audio);
        assert_eq!(config.extradata, vec![0x12, 0x10]);
        assert_eq!(
            config.bitstream_format,
            Some(DecoderBitstreamFormat::Unknown("adts".to_owned()))
        );
        assert_eq!(config.sample_rate, Some(48_000));
        assert_eq!(config.channels, Some(2));
        assert_eq!(config.channel_layout.as_deref(), Some("stereo"));
        assert_eq!(config.target_pcm_format, Some(DecoderFrameFormat::F32));
        assert_eq!(
            config.target_pcm_sample_layout,
            Some(DecoderPcmSampleLayout::Interleaved)
        );
        assert_eq!(config.codec_delay_samples, Some(0));
        assert_eq!(config.priming_samples, Some(2_112));
        assert_eq!(config.trailing_padding_samples, Some(512));
        assert_eq!(config.seek_preroll_samples, Some(1_024));
        assert_eq!(
            config.color.as_ref().and_then(|color| color.bit_depth),
            Some(8)
        );
        assert!(config.prefer_hardware);
        assert!(config.require_cpu_output);
    }

    #[test]
    fn audio_decoder_session_config_rejects_source_normalizer_video_track() {
        let track = crate::SourceNormalizerPacketTrackInfo {
            stream_index: 0,
            media_kind: crate::SourceNormalizerPacketMediaKind::Video,
            codec: "H264".to_owned(),
            extradata: Vec::new(),
            bitstream_format: Some(DecoderBitstreamFormat::Avcc),
            width: Some(1_920),
            height: Some(1_080),
            coded_width: Some(1_920),
            coded_height: Some(1_080),
            reorder_depth: None,
            sample_rate: None,
            channels: None,
            channel_layout: None,
            codec_delay_samples: None,
            priming_samples: None,
            trailing_padding_samples: None,
            seek_preroll_samples: None,
            color: None,
            hdr: None,
            frame_rate: Some(30.0),
            time_base_num: Some(1),
            time_base_den: Some(90_000),
        };

        let error = DecoderSessionConfig::apple_native_audio_from_source_normalizer_track(&track)
            .expect_err("video track is not an audio decoder input");

        assert!(matches!(
            error,
            DecoderError::UnsupportedCapability { capability }
                if capability == "source-normalizer-audio-track"
        ));
    }

    #[test]
    fn native_decoder_session_defaults_pcm_receive_to_capability_error() {
        let mut session = PcmUnsupportedDecoderSession;
        let error = session
            .receive_pcm_frame()
            .expect_err("default PCM receive should be unsupported");

        assert!(matches!(
            error,
            DecoderError::UnsupportedCapability { capability }
                if capability == "audio-pcm-output"
        ));
    }

    #[test]
    fn native_decoder_session_defaults_presentation_release_to_capability_error() {
        let mut session = PcmUnsupportedDecoderSession;
        let error = session
            .release_native_frame_with_presentation(decoder_native_frame(), true)
            .expect_err("default presentation release should be unsupported");

        assert!(matches!(
            error,
            DecoderError::UnsupportedCapability { capability }
                if capability == "presentation-aware-native-frame-release"
        ));
    }

    struct PcmUnsupportedDecoderSession;

    impl NativeDecoderSession for PcmUnsupportedDecoderSession {
        fn session_info(&self) -> DecoderSessionInfo {
            DecoderSessionInfo::default()
        }

        fn send_packet(
            &mut self,
            _packet: &DecoderPacket,
            _data: &[u8],
        ) -> Result<DecoderPacketResult, DecoderError> {
            Ok(DecoderPacketResult::default())
        }

        fn receive_native_frame(
            &mut self,
        ) -> Result<DecoderReceiveNativeFrameOutput, DecoderError> {
            Ok(DecoderReceiveNativeFrameOutput::NeedMoreInput)
        }

        fn release_native_frame(&mut self, _frame: DecoderNativeFrame) -> Result<(), DecoderError> {
            Ok(())
        }

        fn flush(&mut self) -> Result<(), DecoderError> {
            Ok(())
        }

        fn close(&mut self) -> Result<(), DecoderError> {
            Ok(())
        }
    }
}
