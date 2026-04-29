#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ByteRange {
    pub start: u64,
    pub end: u64,
}

impl ByteRange {
    pub fn new(start: u64, end: u64) -> Self {
        Self { start, end }
    }

    pub fn len(&self) -> Option<u64> {
        self.end.checked_sub(self.start)?.checked_add(1)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DashManifest {
    pub duration_ms: Option<u64>,
    pub min_buffer_time_ms: Option<u64>,
    pub periods: Vec<DashPeriod>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DashPeriod {
    pub id: Option<String>,
    pub adaptation_sets: Vec<DashAdaptationSet>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DashAdaptationKind {
    Video,
    Audio,
    Subtitle,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DashAdaptationSet {
    pub id: Option<String>,
    pub kind: DashAdaptationKind,
    pub mime_type: Option<String>,
    pub language: Option<String>,
    pub representations: Vec<DashRepresentation>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DashRepresentation {
    pub id: String,
    pub base_url: String,
    pub mime_type: String,
    pub codecs: String,
    pub bandwidth: Option<u64>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub frame_rate: Option<String>,
    pub audio_sampling_rate: Option<String>,
    pub segment_base: Option<DashSegmentBase>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DashSegmentBase {
    pub initialization: ByteRange,
    pub index_range: ByteRange,
}
