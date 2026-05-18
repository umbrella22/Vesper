use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use player_model::MediaSource;
use player_plugin::OutputFormat;

use crate::{PlayerError, PlayerErrorCategory, PlayerErrorCode, PlayerResult};

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct DownloadAssetId(String);

impl DownloadAssetId {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into().trim().to_owned())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct DownloadTaskId(pub(super) u64);

impl DownloadTaskId {
    pub fn from_raw(value: u64) -> Self {
        Self(value)
    }

    pub fn get(self) -> u64 {
        self.0
    }
}

pub(super) fn next_non_zero_task_id(current: u64) -> PlayerResult<u64> {
    current.checked_add(1).ok_or_else(|| {
        PlayerError::with_category(
            PlayerErrorCode::InvalidState,
            PlayerErrorCategory::Playback,
            "download task id space is exhausted",
        )
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DownloadContentFormat {
    HlsSegments,
    DashSegments,
    FlvSegments,
    SingleFile,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DownloadSource {
    pub source: MediaSource,
    pub content_format: DownloadContentFormat,
    pub manifest_uri: Option<String>,
    pub request_headers: HashMap<String, String>,
}

impl DownloadSource {
    pub fn new(source: MediaSource, content_format: DownloadContentFormat) -> Self {
        Self {
            source,
            content_format,
            manifest_uri: None,
            request_headers: HashMap::new(),
        }
    }

    pub fn with_manifest_uri(mut self, manifest_uri: impl Into<String>) -> Self {
        self.manifest_uri = Some(manifest_uri.into().trim().to_owned());
        self
    }

    pub fn with_request_headers(
        mut self,
        headers: impl IntoIterator<Item = (String, String)>,
    ) -> Self {
        self.request_headers = sanitize_request_headers(headers);
        self
    }
}

fn sanitize_request_headers(
    headers: impl IntoIterator<Item = (String, String)>,
) -> HashMap<String, String> {
    headers
        .into_iter()
        .filter_map(|(name, value)| {
            let name = name.trim().to_owned();
            if name.is_empty() || value.trim().is_empty() {
                return None;
            }
            Some((name, value))
        })
        .collect()
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DownloadProfile {
    pub variant_id: Option<String>,
    pub preferred_audio_language: Option<String>,
    pub preferred_subtitle_language: Option<String>,
    pub selected_track_ids: Vec<String>,
    pub target_output_format: Option<OutputFormat>,
    pub target_directory: Option<PathBuf>,
    pub allow_metered_network: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DownloadByteRange {
    pub offset: u64,
    pub length: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DownloadResourceRecord {
    pub resource_id: String,
    pub uri: String,
    pub relative_path: Option<PathBuf>,
    pub byte_range: Option<DownloadByteRange>,
    pub generated_text: Option<String>,
    pub size_bytes: Option<u64>,
    pub etag: Option<String>,
    pub checksum: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DownloadSegmentRecord {
    pub segment_id: String,
    pub uri: String,
    pub relative_path: Option<PathBuf>,
    pub sequence: Option<u64>,
    pub byte_range: Option<DownloadByteRange>,
    pub size_bytes: Option<u64>,
    pub checksum: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DownloadStreamKind {
    Combined,
    Video,
    Audio,
    SecondaryAudio,
    Subtitle,
    Auxiliary,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DownloadAssetStream {
    pub stream_id: String,
    pub kind: DownloadStreamKind,
    pub language: Option<String>,
    pub codec: Option<String>,
    pub label: Option<String>,
    pub quality_rank: Option<u32>,
    pub resource_ids: Vec<String>,
    pub segment_ids: Vec<String>,
    pub metadata: HashMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DownloadAssetIndex {
    pub content_format: DownloadContentFormat,
    pub version: Option<String>,
    pub etag: Option<String>,
    pub checksum: Option<String>,
    pub total_size_bytes: Option<u64>,
    pub resources: Vec<DownloadResourceRecord>,
    pub segments: Vec<DownloadSegmentRecord>,
    pub streams: Vec<DownloadAssetStream>,
    pub completed_path: Option<PathBuf>,
}

impl Default for DownloadAssetIndex {
    fn default() -> Self {
        Self {
            content_format: DownloadContentFormat::Unknown,
            version: None,
            etag: None,
            checksum: None,
            total_size_bytes: None,
            resources: Vec::new(),
            segments: Vec::new(),
            streams: Vec::new(),
            completed_path: None,
        }
    }
}

impl DownloadAssetIndex {
    pub fn inferred_total_size_bytes(&self) -> Option<u64> {
        if self.resources.is_empty() && self.segments.is_empty() {
            return self.total_size_bytes;
        }

        self.total_size_bytes.or_else(|| {
            let resource_sum = self.resources.iter().try_fold(0_u64, |sum, resource| {
                if resource.generated_text.is_some() {
                    Some(sum)
                } else {
                    resource.size_bytes.map(|size| sum + size)
                }
            });
            let segment_sum = self.segments.iter().try_fold(0_u64, |sum, segment| {
                segment.size_bytes.map(|size| sum + size)
            });

            match (resource_sum, segment_sum) {
                (Some(resource_sum), Some(segment_sum)) => Some(resource_sum + segment_sum),
                (Some(resource_sum), None) if self.segments.is_empty() => Some(resource_sum),
                (None, Some(segment_sum)) if self.resources.is_empty() => Some(segment_sum),
                _ => None,
            }
        })
    }

    pub fn total_segment_count(&self) -> Option<u32> {
        if self.segments.is_empty() {
            None
        } else {
            Some(self.segments.len() as u32)
        }
    }

    pub fn ensure_default_streams(&mut self) {
        if !self.streams.is_empty() {
            return;
        }

        self.streams.push(DownloadAssetStream {
            stream_id: "combined".to_owned(),
            kind: DownloadStreamKind::Combined,
            language: None,
            codec: None,
            label: None,
            quality_rank: None,
            resource_ids: self
                .resources
                .iter()
                .map(|resource| resource.resource_id.clone())
                .collect(),
            segment_ids: self
                .segments
                .iter()
                .map(|segment| segment.segment_id.clone())
                .collect(),
            metadata: HashMap::new(),
        });
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DownloadProgressSnapshot {
    pub received_bytes: u64,
    pub total_bytes: Option<u64>,
    pub received_segments: u32,
    pub total_segments: Option<u32>,
}

impl DownloadProgressSnapshot {
    pub(super) fn from_index(index: &DownloadAssetIndex) -> Self {
        Self {
            received_bytes: 0,
            total_bytes: index.inferred_total_size_bytes(),
            received_segments: 0,
            total_segments: index.total_segment_count(),
        }
    }

    pub fn completion_ratio(&self) -> Option<f32> {
        self.total_bytes
            .filter(|total_bytes| *total_bytes > 0)
            .map(|total_bytes| (self.received_bytes as f32 / total_bytes as f32).min(1.0))
    }

    pub(super) fn clamp_to_totals(&mut self) {
        if let Some(total_bytes) = self.total_bytes {
            self.received_bytes = self.received_bytes.min(total_bytes);
        }
        if let Some(total_segments) = self.total_segments {
            self.received_segments = self.received_segments.min(total_segments);
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DownloadTaskStatus {
    Queued,
    Preparing,
    Downloading,
    Paused,
    Completed,
    Failed,
    Removed,
}

pub type DownloadTaskState = DownloadTaskStatus;

impl DownloadTaskStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Queued => "Queued",
            Self::Preparing => "Preparing",
            Self::Downloading => "Downloading",
            Self::Paused => "Paused",
            Self::Completed => "Completed",
            Self::Failed => "Failed",
            Self::Removed => "Removed",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DownloadErrorSummary {
    pub code: PlayerErrorCode,
    pub category: PlayerErrorCategory,
    pub retriable: bool,
    pub message: String,
}

impl From<PlayerError> for DownloadErrorSummary {
    fn from(value: PlayerError) -> Self {
        Self {
            code: value.code(),
            category: value.category(),
            retriable: value.is_retriable(),
            message: value.message().to_owned(),
        }
    }
}

impl DownloadTaskSnapshot {
    pub(super) fn state_patch(&self) -> DownloadTaskStatePatch {
        DownloadTaskStatePatch {
            task_id: self.task_id,
            status: self.status,
            progress: self.progress.clone(),
            error_summary: self.error_summary.clone(),
            completed_path: self.asset_index.completed_path.clone(),
        }
    }

    pub(super) fn progress_patch(&self) -> DownloadTaskProgressPatch {
        DownloadTaskProgressPatch {
            task_id: self.task_id,
            progress: self.progress.clone(),
        }
    }

    pub(super) fn set_completed_path(&mut self, completed_path: Option<PathBuf>) {
        Arc::make_mut(&mut self.asset_index).completed_path = completed_path;
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DownloadTaskSnapshot {
    pub task_id: DownloadTaskId,
    pub asset_id: DownloadAssetId,
    pub source: DownloadSource,
    pub profile: DownloadProfile,
    pub status: DownloadTaskStatus,
    pub progress: DownloadProgressSnapshot,
    pub asset_index: Arc<DownloadAssetIndex>,
    pub created_at: Instant,
    pub updated_at: Instant,
    pub error_summary: Option<DownloadErrorSummary>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DownloadSnapshot {
    pub tasks: Vec<DownloadTaskSnapshot>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DownloadTaskStatePatch {
    pub task_id: DownloadTaskId,
    pub status: DownloadTaskStatus,
    pub progress: DownloadProgressSnapshot,
    pub error_summary: Option<DownloadErrorSummary>,
    pub completed_path: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DownloadTaskProgressPatch {
    pub task_id: DownloadTaskId,
    pub progress: DownloadProgressSnapshot,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DownloadEvent {
    Created(DownloadTaskSnapshot),
    StateChanged(DownloadTaskStatePatch),
    AssetIndexUpdated(DownloadTaskSnapshot),
    ProgressUpdated(DownloadTaskProgressPatch),
}
