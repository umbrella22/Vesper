use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DashManifest {
    #[serde(rename = "type")]
    pub manifest_type: DashManifestType,
    pub duration_ms: Option<u64>,
    pub min_buffer_time_ms: Option<u64>,
    pub minimum_update_period_ms: Option<u64>,
    pub time_shift_buffer_depth_ms: Option<u64>,
    pub periods: Vec<DashPeriod>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DashManifestType {
    #[serde(rename = "static")]
    Static,
    #[serde(rename = "dynamic")]
    Dynamic,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DashPeriod {
    pub id: Option<String>,
    pub adaptation_sets: Vec<DashAdaptationSet>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DashAdaptationKind {
    Video,
    Audio,
    Subtitle,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DashAdaptationSet {
    pub id: Option<String>,
    pub kind: DashAdaptationKind,
    pub mime_type: Option<String>,
    pub language: Option<String>,
    pub representations: Vec<DashRepresentation>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DashRepresentation {
    pub id: String,
    #[serde(rename = "baseURL")]
    pub base_url: String,
    pub mime_type: String,
    pub codecs: String,
    pub bandwidth: Option<u64>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub frame_rate: Option<String>,
    pub audio_sampling_rate: Option<String>,
    pub segment_base: Option<DashSegmentBase>,
    pub segment_template: Option<DashSegmentTemplate>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DashSegmentBase {
    pub initialization: ByteRange,
    pub index_range: ByteRange,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DashSegmentTemplate {
    pub timescale: u64,
    pub duration: Option<u64>,
    pub start_number: u64,
    pub presentation_time_offset: u64,
    pub initialization: Option<String>,
    pub media: String,
    pub timeline: Vec<DashSegmentTimelineEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DashSegmentTimelineEntry {
    pub start_time: Option<u64>,
    pub duration: u64,
    pub repeat_count: i32,
}
