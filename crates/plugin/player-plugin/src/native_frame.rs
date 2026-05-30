use serde::{Deserialize, Serialize};

use crate::{DecoderFrameFormat, DecoderMediaKind};

/// Native frame handle kinds shared by decoder, frame processor, and presenter paths.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum NativeHandleKind {
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

/// Cross-component native-frame pipeline profiles used for decoder/processor/presenter matching.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum NativeFramePipelineProfile {
    VideoToolboxCvPixelBuffer,
    MetalTexture,
    D3D11Texture2D,
    MediaCodecHardwareBuffer,
    MediaCodecSurfaceTexture,
    Unknown(String),
}

impl NativeFramePipelineProfile {
    /// Returns the best-known pipeline profile implied by a native handle kind.
    pub fn from_handle_kind(handle_kind: &NativeHandleKind) -> Self {
        match handle_kind {
            NativeHandleKind::CvPixelBuffer => Self::VideoToolboxCvPixelBuffer,
            NativeHandleKind::MetalTexture => Self::MetalTexture,
            NativeHandleKind::D3D11Texture2D => Self::D3D11Texture2D,
            NativeHandleKind::IoSurface => Self::Unknown("io_surface".to_owned()),
            NativeHandleKind::DmaBuf => Self::Unknown("dma_buf".to_owned()),
            NativeHandleKind::VaapiSurface => Self::Unknown("vaapi_surface".to_owned()),
            NativeHandleKind::DxgiSurface => Self::Unknown("dxgi_surface".to_owned()),
            NativeHandleKind::VulkanImage => Self::Unknown("vulkan_image".to_owned()),
            NativeHandleKind::Unknown(name) => Self::Unknown(name.clone()),
        }
    }

    /// Returns the stable diagnostics label used by runtime and platform bridges.
    pub fn label(&self) -> String {
        match self {
            Self::VideoToolboxCvPixelBuffer => "video_toolbox_cv_pixel_buffer".to_owned(),
            Self::MetalTexture => "metal_texture".to_owned(),
            Self::D3D11Texture2D => "d3d11_texture_2d".to_owned(),
            Self::MediaCodecHardwareBuffer => "media_codec_hardware_buffer".to_owned(),
            Self::MediaCodecSurfaceTexture => "media_codec_surface_texture".to_owned(),
            Self::Unknown(name) => name.clone(),
        }
    }
}

/// Visible content rectangle within a coded native frame.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VisibleRect {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

/// Release tracking diagnostics attached to a native frame.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NativeFrameReleaseTracking {
    pub frame_id: Option<u64>,
    pub requires_release: bool,
}

/// Platform synchronization information associated with a native frame.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NativeFrameSyncInfo {
    pub kind: String,
    #[serde(default)]
    pub handle: Option<u64>,
    #[serde(default)]
    pub value: Option<u64>,
}

/// Display transform metadata that must be preserved across native-frame stages.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NativeFrameTransform {
    pub rotation_degrees: u16,
    #[serde(default)]
    pub mirrored_horizontal: bool,
    #[serde(default)]
    pub mirrored_vertical: bool,
}

/// Metadata shared by native frame producers, processors, and consumers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NativeFrameMetadata {
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
    pub visible_rect: Option<VisibleRect>,
    pub handle_kind: NativeHandleKind,
    #[serde(default)]
    pub pipeline_profile: Option<NativeFramePipelineProfile>,
    #[serde(default)]
    pub color_space: Option<String>,
    #[serde(default)]
    pub hdr_metadata: Option<String>,
    #[serde(default)]
    pub sync_info: Option<NativeFrameSyncInfo>,
    #[serde(default)]
    pub transform: Option<NativeFrameTransform>,
    #[serde(default)]
    pub frame_id: Option<u64>,
    #[serde(default)]
    pub release_tracking: Option<NativeFrameReleaseTracking>,
}

impl NativeFrameMetadata {
    /// Returns the explicit pipeline profile or derives one from the handle kind.
    pub fn effective_pipeline_profile(&self) -> NativeFramePipelineProfile {
        self.pipeline_profile
            .clone()
            .unwrap_or_else(|| NativeFramePipelineProfile::from_handle_kind(&self.handle_kind))
    }
}

/// A native frame handle plus metadata.
#[must_use = "native frames may own externally retained resources and must be released through the producing session"]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeFrame {
    pub metadata: NativeFrameMetadata,
    pub handle: usize,
}

#[cfg(test)]
mod tests {
    use super::{
        NativeFrameMetadata, NativeFramePipelineProfile, NativeFrameReleaseTracking,
        NativeFrameSyncInfo, NativeFrameTransform, NativeHandleKind, VisibleRect,
    };
    use crate::{DecoderFrameFormat, DecoderMediaKind};

    fn test_metadata() -> NativeFrameMetadata {
        NativeFrameMetadata {
            media_kind: DecoderMediaKind::Video,
            format: DecoderFrameFormat::Nv12,
            codec: "h264".to_owned(),
            pts_us: Some(42_000),
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
            hdr_metadata: Some("hdr10".to_owned()),
            sync_info: Some(NativeFrameSyncInfo {
                kind: "test_fence".to_owned(),
                handle: Some(12),
                value: Some(34),
            }),
            transform: Some(NativeFrameTransform {
                rotation_degrees: 90,
                mirrored_horizontal: false,
                mirrored_vertical: true,
            }),
            frame_id: Some(7),
            release_tracking: Some(NativeFrameReleaseTracking {
                frame_id: Some(7),
                requires_release: true,
            }),
        }
    }

    #[test]
    fn native_frame_metadata_round_trips_through_json() {
        let metadata = test_metadata();

        let encoded = serde_json::to_string(&metadata).expect("serialize metadata");
        let decoded: NativeFrameMetadata =
            serde_json::from_str(&encoded).expect("deserialize metadata");

        assert_eq!(decoded, metadata);
    }

    #[test]
    fn native_frame_metadata_derives_pipeline_profile_from_handle_kind() {
        let mut metadata = test_metadata();
        metadata.pipeline_profile = None;

        assert_eq!(
            metadata.effective_pipeline_profile(),
            NativeFramePipelineProfile::VideoToolboxCvPixelBuffer
        );
    }

    #[test]
    fn native_frame_pipeline_profile_has_stable_diagnostic_label() {
        assert_eq!(
            NativeFramePipelineProfile::MediaCodecHardwareBuffer.label(),
            "media_codec_hardware_buffer"
        );
        assert_eq!(
            NativeFramePipelineProfile::Unknown("fixture".to_owned()).label(),
            "fixture"
        );
    }
}
