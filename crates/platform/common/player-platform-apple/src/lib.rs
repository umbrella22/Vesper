pub const VIDEOTOOLBOX_BACKEND_NAME: &str = "VideoToolbox";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppleSystemVideoCodec {
    H264,
    Hevc,
    Unknown,
}

#[derive(Debug, Clone)]
pub struct AppleHardwareDecodeSupport {
    pub codec_name: String,
    pub codec: AppleSystemVideoCodec,
    pub hardware_backend: Option<String>,
    pub hardware_available: bool,
    pub checked_via_system_framework: bool,
    pub fallback_reason: Option<String>,
}

impl AppleSystemVideoCodec {
    pub fn from_codec_name(codec_name: &str) -> Self {
        match codec_name.trim().to_ascii_uppercase().as_str() {
            "H264" | "AVC" | "AVC1" => Self::H264,
            "HEVC" | "H265" | "HVC1" | "HEV1" => Self::Hevc,
            _ => Self::Unknown,
        }
    }

    pub fn as_cm_video_codec_type(self) -> Option<u32> {
        match self {
            Self::H264 => Some(fourcc(*b"avc1")),
            Self::Hevc => Some(fourcc(*b"hvc1")),
            Self::Unknown => None,
        }
    }
}

pub fn probe_videotoolbox_hardware_decode(codec_name: &str) -> AppleHardwareDecodeSupport {
    let codec = AppleSystemVideoCodec::from_codec_name(codec_name);

    let Some(codec_type) = codec.as_cm_video_codec_type() else {
        return AppleHardwareDecodeSupport {
            codec_name: codec_name.to_owned(),
            codec,
            hardware_backend: Some(VIDEOTOOLBOX_BACKEND_NAME.to_owned()),
            hardware_available: false,
            checked_via_system_framework: false,
            fallback_reason: Some(format!(
                "codec {codec_name} is outside the current Apple VideoToolbox candidate set"
            )),
        };
    };

    let hardware_available = videotoolbox_is_hardware_decode_supported(codec_type);

    AppleHardwareDecodeSupport {
        codec_name: codec_name.to_owned(),
        codec,
        hardware_backend: Some(VIDEOTOOLBOX_BACKEND_NAME.to_owned()),
        hardware_available,
        checked_via_system_framework: cfg!(any(target_os = "macos", target_os = "ios")),
        fallback_reason: if hardware_available {
            None
        } else {
            Some(format!(
                "system VideoToolbox reported no hardware decode support for codec {codec_name}"
            ))
        },
    }
}

const fn fourcc(bytes: [u8; 4]) -> u32 {
    u32::from_be_bytes(bytes)
}

#[cfg(any(target_os = "macos", target_os = "ios"))]
fn videotoolbox_is_hardware_decode_supported(codec_type: u32) -> bool {
    unsafe { VTIsHardwareDecodeSupported(codec_type) }
}

#[cfg(not(any(target_os = "macos", target_os = "ios")))]
fn videotoolbox_is_hardware_decode_supported(_codec_type: u32) -> bool {
    false
}

#[cfg(any(target_os = "macos", target_os = "ios"))]
#[link(name = "VideoToolbox", kind = "framework")]
unsafe extern "C" {
    fn VTIsHardwareDecodeSupported(codec_type: u32) -> bool;
}

#[cfg(test)]
mod tests {
    use super::{
        AppleSystemVideoCodec, VIDEOTOOLBOX_BACKEND_NAME, probe_videotoolbox_hardware_decode,
    };

    #[test]
    fn codec_mapping_normalizes_common_apple_codecs() {
        assert_eq!(
            AppleSystemVideoCodec::from_codec_name("H264"),
            AppleSystemVideoCodec::H264
        );
        assert_eq!(
            AppleSystemVideoCodec::from_codec_name("avc1"),
            AppleSystemVideoCodec::H264
        );
        assert_eq!(
            AppleSystemVideoCodec::from_codec_name("HEVC"),
            AppleSystemVideoCodec::Hevc
        );
        assert_eq!(
            AppleSystemVideoCodec::from_codec_name("h265"),
            AppleSystemVideoCodec::Hevc
        );
        assert_eq!(
            AppleSystemVideoCodec::from_codec_name("vp9"),
            AppleSystemVideoCodec::Unknown
        );
    }

    #[test]
    fn unsupported_codec_reports_videotoolbox_backend_with_reason() {
        let support = probe_videotoolbox_hardware_decode("VP8");

        assert!(!support.hardware_available);
        assert_eq!(
            support.hardware_backend.as_deref(),
            Some(VIDEOTOOLBOX_BACKEND_NAME)
        );
        assert!(
            support
                .fallback_reason
                .as_deref()
                .unwrap_or_default()
                .contains("VP8")
        );
    }

    #[test]
    fn h264_probe_uses_videotoolbox_backend() {
        let support = probe_videotoolbox_hardware_decode("H264");

        assert_eq!(support.codec, AppleSystemVideoCodec::H264);
        assert_eq!(
            support.hardware_backend.as_deref(),
            Some(VIDEOTOOLBOX_BACKEND_NAME)
        );
    }
}
