use crate::dash::ByteRange;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HlsMasterInput {
    pub variants: Vec<HlsVariant>,
    pub audio_renditions: Vec<HlsAudioRendition>,
    pub independent_segments: bool,
}

impl Default for HlsMasterInput {
    fn default() -> Self {
        Self {
            variants: Vec::new(),
            audio_renditions: Vec::new(),
            independent_segments: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HlsVariant {
    pub uri: String,
    pub bandwidth: u64,
    pub average_bandwidth: Option<u64>,
    pub codecs: String,
    pub resolution: Option<HlsResolution>,
    pub frame_rate: Option<String>,
    pub audio_group_id: Option<String>,
    pub video_range: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HlsResolution {
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HlsAudioRendition {
    pub group_id: String,
    pub name: String,
    pub uri: String,
    pub language: Option<String>,
    pub is_default: bool,
    pub autoselect: bool,
    pub channels: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct HlsMediaInput {
    pub uri: String,
    pub initialization: ByteRange,
    pub segments: Vec<HlsMediaSegment>,
    pub independent_segments: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct HlsMediaSegment {
    pub duration_seconds: f64,
    pub byte_range: ByteRange,
}
