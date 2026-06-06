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
    MediaCodecHardwareBuffer,
    MediaCodecSurfaceTexture,
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
            NativeHandleKind::MediaCodecHardwareBuffer => Self::MediaCodecHardwareBuffer,
            NativeHandleKind::MediaCodecSurfaceTexture => Self::MediaCodecSurfaceTexture,
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

/// Color characteristics that must be preserved for HDR native-frame playback.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NativeFrameColorMetadata {
    #[serde(default)]
    pub primaries: Option<String>,
    #[serde(default)]
    pub transfer: Option<String>,
    #[serde(default)]
    pub matrix: Option<String>,
    #[serde(default)]
    pub range: Option<String>,
    #[serde(default)]
    pub bit_depth: Option<u8>,
}

impl NativeFrameColorMetadata {
    /// Returns whether this metadata describes a known HDR transfer function.
    pub fn is_hdr_transfer(&self) -> bool {
        self.transfer
            .as_deref()
            .map(|transfer| {
                let transfer = transfer.to_ascii_lowercase();
                transfer.contains("pq")
                    || transfer.contains("st2084")
                    || transfer.contains("smpte2084")
                    || transfer.contains("hlg")
                    || transfer.contains("arib-std-b67")
                    || transfer.contains("arib_std_b67")
            })
            .unwrap_or(false)
    }

    /// Returns whether this color metadata requires explicit preservation.
    pub fn requires_preservation(&self) -> bool {
        self.bit_depth.is_some_and(|bit_depth| bit_depth > 8)
            || self.is_hdr_transfer()
            || self.primaries.as_deref().is_some_and(is_wide_color_label)
            || self.matrix.as_deref().is_some_and(is_wide_color_label)
            || self
                .transfer
                .as_deref()
                .is_some_and(is_wide_color_transfer_label)
    }
}

fn is_wide_color_label(label: &str) -> bool {
    let normalized = normalize_color_label(label);
    matches!(
        normalized.as_str(),
        "bt2020"
            | "rec2020"
            | "bt2020nc"
            | "bt2020ncl"
            | "bt2020c"
            | "bt2020cl"
            | "smpte431"
            | "smpte431p3"
            | "smpte432"
            | "smpte432p3"
            | "displayp3"
            | "displayp3d65"
            | "p3"
            | "dcip3"
            | "ictcp"
    )
}

fn is_wide_color_transfer_label(label: &str) -> bool {
    let normalized = normalize_color_label(label);
    matches!(
        normalized.as_str(),
        "bt2020" | "bt202010" | "bt202012" | "smpte2084" | "st2084" | "pq" | "hlg" | "aribstdb67"
    )
}

fn normalize_color_label(label: &str) -> String {
    label
        .trim()
        .to_ascii_lowercase()
        .chars()
        .filter(|character| character.is_ascii_alphanumeric())
        .collect()
}

/// Mastering display metadata carried by HDR10-style streams.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NativeFrameMasteringDisplayMetadata {
    #[serde(default)]
    pub display_primaries: Option<String>,
    #[serde(default)]
    pub white_point: Option<String>,
    #[serde(default)]
    pub max_luminance_nits: Option<u32>,
    #[serde(default)]
    pub min_luminance_nits: Option<u32>,
}

/// Content light metadata carried by HDR streams.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NativeFrameContentLightMetadata {
    #[serde(default)]
    pub max_content_light_level: Option<u32>,
    #[serde(default)]
    pub max_frame_average_light_level: Option<u32>,
}

/// Dolby Vision stream metadata used for diagnostics and route selection.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NativeFrameDolbyVisionMetadata {
    #[serde(default)]
    pub profile: Option<u8>,
    #[serde(default)]
    pub level: Option<u8>,
    #[serde(default)]
    pub compatibility_id: Option<u8>,
    #[serde(default)]
    pub has_rpu: bool,
    #[serde(default)]
    pub has_el: bool,
    #[serde(default)]
    pub has_bl: bool,
}

/// Structured HDR metadata attached to tracks and native frames.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NativeFrameHdrMetadata {
    pub kind: String,
    #[serde(default)]
    pub mastering_display: Option<NativeFrameMasteringDisplayMetadata>,
    #[serde(default)]
    pub content_light: Option<NativeFrameContentLightMetadata>,
    #[serde(default)]
    pub dolby_vision: Option<NativeFrameDolbyVisionMetadata>,
}

impl NativeFrameHdrMetadata {
    /// Returns whether the metadata describes Dolby Vision.
    pub fn is_dolby_vision(&self) -> bool {
        self.kind.eq_ignore_ascii_case("dolbyVision") || self.dolby_vision.is_some()
    }
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
    pub release_tracking: Option<NativeFrameReleaseTracking>,
}

impl NativeFrameMetadata {
    /// Returns the explicit pipeline profile or derives one from the handle kind.
    pub fn effective_pipeline_profile(&self) -> NativeFramePipelineProfile {
        self.pipeline_profile
            .clone()
            .unwrap_or_else(|| NativeFramePipelineProfile::from_handle_kind(&self.handle_kind))
    }

    /// Returns whether this frame should be treated as HDR by native-frame gates.
    pub fn requires_hdr_preservation(&self) -> bool {
        self.hdr.is_some()
            || self
                .hdr_metadata
                .as_deref()
                .map(|metadata| !metadata.trim().is_empty())
                .unwrap_or(false)
            || self
                .color
                .as_ref()
                .map(NativeFrameColorMetadata::is_hdr_transfer)
                .unwrap_or(false)
    }

    /// Returns whether this frame carries color metadata that should be preserved.
    pub fn requires_color_preservation(&self) -> bool {
        self.color
            .as_ref()
            .is_some_and(NativeFrameColorMetadata::requires_preservation)
            || self.color_space.as_deref().is_some_and(is_wide_color_label)
            || self.requires_hdr_preservation()
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
        NativeFrameColorMetadata, NativeFrameContentLightMetadata, NativeFrameDolbyVisionMetadata,
        NativeFrameHdrMetadata, NativeFrameMasteringDisplayMetadata, NativeFrameMetadata,
        NativeFramePipelineProfile, NativeFrameReleaseTracking, NativeFrameSyncInfo,
        NativeFrameTransform, NativeHandleKind, VisibleRect,
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
            color: Some(NativeFrameColorMetadata {
                primaries: Some("bt2020".to_owned()),
                transfer: Some("smpte2084".to_owned()),
                matrix: Some("bt2020-ncl".to_owned()),
                range: Some("limited".to_owned()),
                bit_depth: Some(10),
            }),
            hdr: Some(NativeFrameHdrMetadata {
                kind: "hdr10".to_owned(),
                mastering_display: Some(NativeFrameMasteringDisplayMetadata {
                    display_primaries: Some("bt2020".to_owned()),
                    white_point: Some("d65".to_owned()),
                    max_luminance_nits: Some(1_000),
                    min_luminance_nits: Some(0),
                }),
                content_light: Some(NativeFrameContentLightMetadata {
                    max_content_light_level: Some(1_000),
                    max_frame_average_light_level: Some(400),
                }),
                dolby_vision: None,
            }),
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
    fn native_frame_metadata_detects_hdr_preservation_requirement() {
        let mut metadata = test_metadata();

        assert!(metadata.requires_color_preservation());
        assert!(metadata.requires_hdr_preservation());

        metadata.color = Some(NativeFrameColorMetadata {
            primaries: Some("bt2020".to_owned()),
            transfer: Some("arib-std-b67".to_owned()),
            matrix: Some("bt2020-ncl".to_owned()),
            range: Some("limited".to_owned()),
            bit_depth: Some(10),
        });
        metadata.hdr = None;
        metadata.hdr_metadata = None;

        assert!(metadata.requires_hdr_preservation());

        metadata.color = None;
        metadata.color_space = None;

        assert!(!metadata.requires_color_preservation());
        assert!(!metadata.requires_hdr_preservation());
    }

    #[test]
    fn native_frame_metadata_does_not_require_color_preservation_for_ordinary_sdr() {
        let mut metadata = test_metadata();
        metadata.color_space = Some("bt709".to_owned());
        metadata.color = Some(NativeFrameColorMetadata {
            primaries: Some("bt709".to_owned()),
            transfer: Some("sdr-video".to_owned()),
            matrix: Some("bt709".to_owned()),
            range: Some("limited".to_owned()),
            bit_depth: Some(8),
        });
        metadata.hdr = None;
        metadata.hdr_metadata = None;

        assert!(!metadata.requires_color_preservation());
        assert!(!metadata.requires_hdr_preservation());

        metadata.color_space = Some("bt2020".to_owned());
        assert!(metadata.requires_color_preservation());
    }

    #[test]
    fn native_frame_metadata_accepts_ffmpeg_sdr_color_spellings() {
        let mut metadata = test_metadata();
        metadata.color_space = None;
        metadata.hdr = None;
        metadata.hdr_metadata = None;

        for label in ["bt470bg", "fcc", "smpte240m"] {
            metadata.color = Some(NativeFrameColorMetadata {
                primaries: Some(label.to_owned()),
                transfer: Some("bt709".to_owned()),
                matrix: Some(label.to_owned()),
                range: Some("limited".to_owned()),
                bit_depth: Some(8),
            });

            assert!(
                !metadata.requires_color_preservation(),
                "{label} should be treated as ordinary SDR"
            );
            assert!(!metadata.requires_hdr_preservation());
        }
    }

    #[test]
    fn native_frame_metadata_requires_preservation_for_wide_color_labels() {
        let mut metadata = test_metadata();
        metadata.color_space = None;
        metadata.hdr = None;
        metadata.hdr_metadata = None;

        for label in ["bt2020", "display-p3", "ictcp"] {
            metadata.color = Some(NativeFrameColorMetadata {
                primaries: Some(label.to_owned()),
                transfer: Some("sdr-video".to_owned()),
                matrix: Some(label.to_owned()),
                range: Some("limited".to_owned()),
                bit_depth: Some(8),
            });

            assert!(
                metadata.requires_color_preservation(),
                "{label} should require preservation"
            );
        }
    }

    #[test]
    fn native_frame_color_metadata_recognizes_android_hdr_transfer_labels() {
        let mut color = NativeFrameColorMetadata {
            primaries: Some("bt2020".to_owned()),
            transfer: Some("st2084".to_owned()),
            matrix: Some("bt2020-ncl".to_owned()),
            range: Some("limited".to_owned()),
            bit_depth: Some(10),
        };

        assert!(color.is_hdr_transfer());
        assert!(color.requires_preservation());

        color.transfer = Some("hlg".to_owned());
        assert!(color.is_hdr_transfer());
        assert!(color.requires_preservation());
    }

    #[test]
    fn native_frame_hdr_metadata_identifies_dolby_vision() {
        let metadata = NativeFrameHdrMetadata {
            kind: "dolbyVision".to_owned(),
            mastering_display: None,
            content_light: None,
            dolby_vision: Some(NativeFrameDolbyVisionMetadata {
                profile: Some(8),
                level: Some(6),
                compatibility_id: Some(1),
                has_rpu: true,
                has_el: false,
                has_bl: true,
            }),
        };

        assert!(metadata.is_dolby_vision());
    }

    #[test]
    fn native_frame_metadata_derives_pipeline_profile_from_handle_kind() {
        let mut metadata = test_metadata();
        metadata.pipeline_profile = None;

        assert_eq!(
            metadata.effective_pipeline_profile(),
            NativeFramePipelineProfile::VideoToolboxCvPixelBuffer
        );

        metadata.handle_kind = NativeHandleKind::MediaCodecHardwareBuffer;
        assert_eq!(
            metadata.effective_pipeline_profile(),
            NativeFramePipelineProfile::MediaCodecHardwareBuffer
        );

        metadata.handle_kind = NativeHandleKind::MediaCodecSurfaceTexture;
        assert_eq!(
            metadata.effective_pipeline_profile(),
            NativeFramePipelineProfile::MediaCodecSurfaceTexture
        );
    }

    #[test]
    fn native_frame_pipeline_profile_has_stable_diagnostic_label() {
        assert_eq!(
            NativeFramePipelineProfile::MediaCodecHardwareBuffer.label(),
            "media_codec_hardware_buffer"
        );
        assert_eq!(
            NativeFramePipelineProfile::MediaCodecSurfaceTexture.label(),
            "media_codec_surface_texture"
        );
        assert_eq!(
            NativeFramePipelineProfile::Unknown("fixture".to_owned()).label(),
            "fixture"
        );
    }
}
