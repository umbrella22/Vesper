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
    pub frame_id: Option<u64>,
    #[serde(default)]
    pub release_tracking: Option<NativeFrameReleaseTracking>,
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
    use super::{NativeFrameMetadata, NativeFrameReleaseTracking, NativeHandleKind, VisibleRect};
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
}
