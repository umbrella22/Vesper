use std::collections::{HashMap, HashSet};
use std::fmt;
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use player_model::MediaSource;
use player_plugin::{
    AssemblyMode, CompletedContentFormat, CompletedDownloadInfo, CompletedStream, DownloadMetadata,
    OutputFormat, PipelineEvent, PipelineEventHook, PostDownloadProcessor, ProcessorError,
    ProcessorOutput, ProcessorProgress, StreamKind,
};

use crate::{
    PlayerRuntimeError, PlayerRuntimeErrorCategory, PlayerRuntimeErrorCode, PlayerRuntimeResult,
};

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
pub struct DownloadTaskId(u64);

impl DownloadTaskId {
    pub fn from_raw(value: u64) -> Self {
        Self(value)
    }

    pub fn get(self) -> u64 {
        self.0
    }
}

fn next_non_zero_task_id(current: u64) -> PlayerRuntimeResult<u64> {
    current.checked_add(1).ok_or_else(|| {
        PlayerRuntimeError::with_category(
            PlayerRuntimeErrorCode::InvalidState,
            PlayerRuntimeErrorCategory::Playback,
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
    fn from_index(index: &DownloadAssetIndex) -> Self {
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

    fn clamp_to_totals(&mut self) {
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
    pub code: PlayerRuntimeErrorCode,
    pub category: PlayerRuntimeErrorCategory,
    pub retriable: bool,
    pub message: String,
}

impl From<PlayerRuntimeError> for DownloadErrorSummary {
    fn from(value: PlayerRuntimeError) -> Self {
        Self {
            code: value.code(),
            category: value.category(),
            retriable: value.is_retriable(),
            message: value.message().to_owned(),
        }
    }
}

impl DownloadTaskSnapshot {
    fn state_patch(&self) -> DownloadTaskStatePatch {
        DownloadTaskStatePatch {
            task_id: self.task_id,
            status: self.status,
            progress: self.progress.clone(),
            error_summary: self.error_summary.clone(),
            completed_path: self.asset_index.completed_path.clone(),
        }
    }

    fn progress_patch(&self) -> DownloadTaskProgressPatch {
        DownloadTaskProgressPatch {
            task_id: self.task_id,
            progress: self.progress.clone(),
        }
    }

    fn set_completed_path(&mut self, completed_path: Option<PathBuf>) {
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

#[derive(Default)]
// Keep platform default constructors aligned when adding required fields.
pub struct DownloadManagerConfig {
    pub auto_start: bool,
    pub run_post_processors_on_completion: bool,
    pub post_processors: Vec<Arc<dyn PostDownloadProcessor>>,
    pub event_hooks: Vec<Arc<dyn PipelineEventHook>>,
}

impl fmt::Debug for DownloadManagerConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DownloadManagerConfig")
            .field("auto_start", &self.auto_start)
            .field(
                "run_post_processors_on_completion",
                &self.run_post_processors_on_completion,
            )
            .field("post_processors_len", &self.post_processors.len())
            .field("event_hooks_len", &self.event_hooks.len())
            .finish()
    }
}

pub trait DownloadStore {
    fn save_task(&mut self, task: DownloadTaskSnapshot) -> PlayerRuntimeResult<()>;

    fn task(&self, task_id: DownloadTaskId) -> Option<DownloadTaskSnapshot>;

    fn tasks(&self) -> Vec<DownloadTaskSnapshot>;

    fn tasks_for_asset(&self, asset_id: &DownloadAssetId) -> Vec<DownloadTaskSnapshot>;
}

#[derive(Debug, Default)]
pub struct InMemoryDownloadStore {
    tasks: HashMap<DownloadTaskId, DownloadTaskSnapshot>,
    asset_index: HashMap<DownloadAssetId, Vec<DownloadTaskId>>,
}

impl DownloadStore for InMemoryDownloadStore {
    fn save_task(&mut self, task: DownloadTaskSnapshot) -> PlayerRuntimeResult<()> {
        let task_id = task.task_id;
        let asset_id = task.asset_id.clone();

        if let Some(previous) = self.tasks.insert(task_id, task.clone())
            && previous.asset_id != asset_id
        {
            self.remove_from_asset_index(&previous.asset_id, task_id);
        }

        let entry = self.asset_index.entry(asset_id).or_default();
        if !entry.contains(&task_id) {
            entry.push(task_id);
        }
        entry.sort_by_key(|task_id| task_id.get());

        Ok(())
    }

    fn task(&self, task_id: DownloadTaskId) -> Option<DownloadTaskSnapshot> {
        self.tasks.get(&task_id).cloned()
    }

    fn tasks(&self) -> Vec<DownloadTaskSnapshot> {
        let mut tasks = self.tasks.values().cloned().collect::<Vec<_>>();
        tasks.sort_by_key(|task| task.task_id.get());
        tasks
    }

    fn tasks_for_asset(&self, asset_id: &DownloadAssetId) -> Vec<DownloadTaskSnapshot> {
        let mut tasks = self
            .asset_index
            .get(asset_id)
            .into_iter()
            .flat_map(|task_ids| task_ids.iter())
            .filter_map(|task_id| self.tasks.get(task_id))
            .cloned()
            .collect::<Vec<_>>();
        tasks.sort_by_key(|task| task.task_id.get());
        tasks
    }
}

impl InMemoryDownloadStore {
    fn remove_from_asset_index(&mut self, asset_id: &DownloadAssetId, task_id: DownloadTaskId) {
        if let Some(task_ids) = self.asset_index.get_mut(asset_id) {
            task_ids.retain(|existing| *existing != task_id);
            if task_ids.is_empty() {
                self.asset_index.remove(asset_id);
            }
        }
    }
}

pub trait DownloadExecutor {
    fn prepare(
        &mut self,
        task: &DownloadTaskSnapshot,
    ) -> PlayerRuntimeResult<DownloadPrepareResult>;

    fn start(&mut self, task: &DownloadTaskSnapshot) -> PlayerRuntimeResult<()>;

    fn pause(&mut self, task_id: DownloadTaskId) -> PlayerRuntimeResult<()>;

    fn resume(&mut self, task: &DownloadTaskSnapshot) -> PlayerRuntimeResult<()>;

    fn remove(&mut self, task_id: DownloadTaskId) -> PlayerRuntimeResult<()>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DownloadPrepareResult {
    Ready(Option<DownloadAssetIndex>),
    Pending,
}

#[derive(Debug, Default)]
pub struct InMemoryDownloadExecutor {
    prepared: Vec<DownloadTaskId>,
    started: Vec<DownloadTaskId>,
    paused: Vec<DownloadTaskId>,
    resumed: Vec<DownloadTaskId>,
    removed: Vec<DownloadTaskId>,
}

impl InMemoryDownloadExecutor {
    pub fn prepared(&self) -> &[DownloadTaskId] {
        &self.prepared
    }

    pub fn started(&self) -> &[DownloadTaskId] {
        &self.started
    }

    pub fn paused(&self) -> &[DownloadTaskId] {
        &self.paused
    }

    pub fn resumed(&self) -> &[DownloadTaskId] {
        &self.resumed
    }

    pub fn removed(&self) -> &[DownloadTaskId] {
        &self.removed
    }
}

impl DownloadExecutor for InMemoryDownloadExecutor {
    fn prepare(
        &mut self,
        task: &DownloadTaskSnapshot,
    ) -> PlayerRuntimeResult<DownloadPrepareResult> {
        self.prepared.push(task.task_id);
        Ok(DownloadPrepareResult::Ready(None))
    }

    fn start(&mut self, task: &DownloadTaskSnapshot) -> PlayerRuntimeResult<()> {
        self.started.push(task.task_id);
        Ok(())
    }

    fn pause(&mut self, task_id: DownloadTaskId) -> PlayerRuntimeResult<()> {
        self.paused.push(task_id);
        Ok(())
    }

    fn resume(&mut self, task: &DownloadTaskSnapshot) -> PlayerRuntimeResult<()> {
        self.resumed.push(task.task_id);
        Ok(())
    }

    fn remove(&mut self, task_id: DownloadTaskId) -> PlayerRuntimeResult<()> {
        self.removed.push(task_id);
        Ok(())
    }
}

#[derive(Debug)]
pub struct DownloadManager<S, E> {
    config: DownloadManagerConfig,
    store: S,
    executor: E,
    next_task_id: u64,
    events: Vec<DownloadEvent>,
    pending_preparation_tasks: HashSet<DownloadTaskId>,
}

impl<S, E> DownloadManager<S, E>
where
    S: DownloadStore,
    E: DownloadExecutor,
{
    pub fn new(config: DownloadManagerConfig, store: S, executor: E) -> Self {
        Self {
            config,
            store,
            executor,
            next_task_id: 1,
            events: Vec::new(),
            pending_preparation_tasks: HashSet::new(),
        }
    }

    pub fn config(&self) -> &DownloadManagerConfig {
        &self.config
    }

    pub fn store(&self) -> &S {
        &self.store
    }

    pub fn store_mut(&mut self) -> &mut S {
        &mut self.store
    }

    pub fn executor(&self) -> &E {
        &self.executor
    }

    pub fn executor_mut(&mut self) -> &mut E {
        &mut self.executor
    }

    pub fn snapshot(&self) -> DownloadSnapshot {
        DownloadSnapshot {
            tasks: self.store.tasks(),
        }
    }

    pub fn drain_events(&mut self) -> Vec<DownloadEvent> {
        self.events.drain(..).collect()
    }

    pub fn task(&self, task_id: DownloadTaskId) -> Option<DownloadTaskSnapshot> {
        self.store.task(task_id)
    }

    pub fn tasks_for_asset(&self, asset_id: &DownloadAssetId) -> Vec<DownloadTaskSnapshot> {
        self.store.tasks_for_asset(asset_id)
    }

    pub fn create_task(
        &mut self,
        asset_id: impl Into<String>,
        source: DownloadSource,
        profile: DownloadProfile,
        mut asset_index: DownloadAssetIndex,
        now: Instant,
    ) -> PlayerRuntimeResult<DownloadTaskId> {
        let task_id = DownloadTaskId(self.next_task_id);
        self.next_task_id = next_non_zero_task_id(self.next_task_id)?;

        asset_index.content_format = source.content_format;
        asset_index.ensure_default_streams();

        let snapshot = DownloadTaskSnapshot {
            task_id,
            asset_id: DownloadAssetId::new(asset_id),
            source,
            profile,
            status: DownloadTaskStatus::Queued,
            progress: DownloadProgressSnapshot::from_index(&asset_index),
            asset_index: Arc::new(asset_index),
            created_at: now,
            updated_at: now,
            error_summary: None,
        };

        self.store.save_task(snapshot.clone())?;
        self.emit_event(DownloadEvent::Created(snapshot.clone()));
        self.emit_event(DownloadEvent::StateChanged(snapshot.state_patch()));

        if self.config.auto_start {
            let _ = self.start_task(task_id, now)?;
        }

        Ok(task_id)
    }

    pub fn restore_tasks(
        &mut self,
        tasks: impl IntoIterator<Item = DownloadTaskSnapshot>,
        now: Instant,
    ) -> PlayerRuntimeResult<Vec<DownloadTaskSnapshot>> {
        let mut restored = Vec::new();
        let mut max_task_id = self.next_task_id.saturating_sub(1);

        for mut task in tasks {
            if task.status == DownloadTaskStatus::Removed {
                continue;
            }

            let restored_status = task.status;
            task.status = match restored_status {
                DownloadTaskStatus::Preparing | DownloadTaskStatus::Downloading => {
                    DownloadTaskStatus::Paused
                }
                status => status,
            };
            task.progress.total_bytes = task
                .progress
                .total_bytes
                .or_else(|| task.asset_index.inferred_total_size_bytes());
            task.progress.total_segments = task
                .progress
                .total_segments
                .or_else(|| task.asset_index.total_segment_count());
            task.progress.clamp_to_totals();
            Arc::make_mut(&mut task.asset_index).ensure_default_streams();
            task.updated_at = now;

            max_task_id = max_task_id.max(task.task_id.get());
            if restored_status == DownloadTaskStatus::Preparing {
                self.pending_preparation_tasks.insert(task.task_id);
            }
            self.store.save_task(task.clone())?;
            self.emit_event(DownloadEvent::StateChanged(task.state_patch()));
            restored.push(task);
        }

        self.next_task_id = self.next_task_id.max(max_task_id.saturating_add(1));
        Ok(restored)
    }

    pub fn start_task(
        &mut self,
        task_id: DownloadTaskId,
        now: Instant,
    ) -> PlayerRuntimeResult<Option<DownloadTaskSnapshot>> {
        let Some(snapshot) = self.store.task(task_id) else {
            return Ok(None);
        };

        if snapshot.status != DownloadTaskStatus::Queued {
            return Ok(Some(snapshot));
        }

        let preparing = self.update_task(task_id, now, |task| {
            task.status = DownloadTaskStatus::Preparing;
            task.error_summary = None;
        })?;
        self.pending_preparation_tasks.insert(task_id);

        let Some(preparing) = preparing else {
            return Ok(None);
        };

        match self.executor.prepare(&preparing) {
            Ok(prepare_result) => {
                self.apply_prepare_result(task_id, preparing, prepare_result, now)
            }
            Err(error) => self.fail_task(task_id, error, now),
        }
    }

    pub fn complete_preparation(
        &mut self,
        task_id: DownloadTaskId,
        asset_index: DownloadAssetIndex,
        now: Instant,
    ) -> PlayerRuntimeResult<Option<DownloadTaskSnapshot>> {
        let Some(snapshot) = self.store.task(task_id) else {
            return Ok(None);
        };

        if snapshot.status != DownloadTaskStatus::Preparing {
            return Ok(Some(snapshot));
        }

        let Some(_) = self.update_asset_index(task_id, asset_index, now)? else {
            return Ok(None);
        };

        self.start_prepared_task(task_id, now)
    }

    pub fn replace_task_plan(
        &mut self,
        task_id: DownloadTaskId,
        source: DownloadSource,
        profile: DownloadProfile,
        mut asset_index: DownloadAssetIndex,
        now: Instant,
    ) -> PlayerRuntimeResult<Option<DownloadTaskSnapshot>> {
        let Some(snapshot) = self.store.task(task_id) else {
            return Ok(None);
        };

        if matches!(
            snapshot.status,
            DownloadTaskStatus::Completed | DownloadTaskStatus::Removed
        ) {
            return Ok(Some(snapshot));
        }

        asset_index.content_format = source.content_format;
        asset_index.ensure_default_streams();
        let mut replaced = snapshot;
        replaced.source = source;
        replaced.profile = profile;
        replaced.progress = DownloadProgressSnapshot::from_index(&asset_index);
        replaced.asset_index = Arc::new(asset_index);
        replaced.status = DownloadTaskStatus::Preparing;
        replaced.error_summary = None;
        replaced.updated_at = now;

        self.pending_preparation_tasks.insert(task_id);
        self.store.save_task(replaced.clone())?;
        self.emit_event(DownloadEvent::AssetIndexUpdated(replaced.clone()));
        self.emit_event(DownloadEvent::StateChanged(replaced.state_patch()));
        Ok(Some(replaced))
    }

    pub fn pause_task(
        &mut self,
        task_id: DownloadTaskId,
        now: Instant,
    ) -> PlayerRuntimeResult<Option<DownloadTaskSnapshot>> {
        let Some(snapshot) = self.store.task(task_id) else {
            return Ok(None);
        };

        if !matches!(
            snapshot.status,
            DownloadTaskStatus::Preparing | DownloadTaskStatus::Downloading
        ) {
            return Ok(Some(snapshot));
        }

        self.executor.pause(task_id)?;
        if snapshot.status == DownloadTaskStatus::Preparing {
            self.pending_preparation_tasks.insert(task_id);
        }
        self.update_task(task_id, now, |task| {
            task.status = DownloadTaskStatus::Paused;
        })
    }

    pub fn resume_task(
        &mut self,
        task_id: DownloadTaskId,
        now: Instant,
    ) -> PlayerRuntimeResult<Option<DownloadTaskSnapshot>> {
        let Some(snapshot) = self.store.task(task_id) else {
            return Ok(None);
        };

        if snapshot.status != DownloadTaskStatus::Paused {
            return Ok(Some(snapshot));
        }

        let should_prepare =
            task_needs_preparation(&snapshot) || self.pending_preparation_tasks.remove(&task_id);

        if should_prepare {
            let preparing = self.update_task(task_id, now, |task| {
                task.status = DownloadTaskStatus::Preparing;
                task.error_summary = None;
            })?;
            self.pending_preparation_tasks.insert(task_id);

            let Some(preparing) = preparing else {
                return Ok(None);
            };

            return match self.executor.prepare(&preparing) {
                Ok(prepare_result) => {
                    self.apply_prepare_result(task_id, preparing, prepare_result, now)
                }
                Err(error) => self.fail_task(task_id, error, now),
            };
        }

        self.executor.resume(&snapshot)?;
        self.update_task(task_id, now, |task| {
            task.status = DownloadTaskStatus::Downloading;
            task.error_summary = None;
        })
    }

    pub fn update_progress(
        &mut self,
        task_id: DownloadTaskId,
        received_bytes: u64,
        received_segments: u32,
        now: Instant,
    ) -> PlayerRuntimeResult<Option<DownloadTaskSnapshot>> {
        let Some(mut snapshot) = self.store.task(task_id) else {
            return Ok(None);
        };

        snapshot.progress.received_bytes = received_bytes;
        snapshot.progress.received_segments = received_segments;
        snapshot.progress.clamp_to_totals();
        snapshot.updated_at = now;
        self.store.save_task(snapshot.clone())?;
        self.emit_event(DownloadEvent::ProgressUpdated(snapshot.progress_patch()));
        Ok(Some(snapshot))
    }

    pub fn complete_task(
        &mut self,
        task_id: DownloadTaskId,
        completed_path: Option<PathBuf>,
        now: Instant,
    ) -> PlayerRuntimeResult<Option<DownloadTaskSnapshot>> {
        let Some(existing) = self.store.task(task_id) else {
            return Ok(None);
        };

        let mut finalized = existing.clone();
        let finalized_completed_path = completed_path
            .clone()
            .or_else(|| finalized.asset_index.completed_path.clone());
        finalized.set_completed_path(finalized_completed_path);
        if let Some(total_bytes) = finalized.progress.total_bytes {
            finalized.progress.received_bytes = total_bytes;
        }
        if let Some(total_segments) = finalized.progress.total_segments {
            finalized.progress.received_segments = total_segments;
        }

        let processed_output_path = if self.config.run_post_processors_on_completion
            && should_run_post_processors_on_completion(&finalized)
        {
            match self.run_post_processors(&finalized) {
                Ok(path) => path,
                Err(error) => return self.fail_task(task_id, error, now),
            }
        } else {
            finalized.asset_index.completed_path.clone()
        };

        self.update_task(task_id, now, |task| {
            task.status = DownloadTaskStatus::Completed;
            task.error_summary = None;
            let completed_path = processed_output_path
                .clone()
                .or_else(|| task.asset_index.completed_path.clone());
            task.set_completed_path(completed_path);
            if let Some(total_bytes) = task.progress.total_bytes {
                task.progress.received_bytes = total_bytes;
            }
            if let Some(total_segments) = task.progress.total_segments {
                task.progress.received_segments = total_segments;
            }
        })
    }

    pub fn export_task_output(
        &self,
        task_id: DownloadTaskId,
        output_path: Option<&Path>,
        progress: &dyn ProcessorProgress,
    ) -> PlayerRuntimeResult<PathBuf> {
        let Some(snapshot) = self.store.task(task_id) else {
            return Err(PlayerRuntimeError::with_category(
                PlayerRuntimeErrorCode::InvalidArgument,
                PlayerRuntimeErrorCategory::Input,
                format!("download task {} was not found for export", task_id.get()),
            ));
        };

        if snapshot.status != DownloadTaskStatus::Completed {
            return Err(PlayerRuntimeError::with_category(
                PlayerRuntimeErrorCode::InvalidState,
                PlayerRuntimeErrorCategory::Playback,
                format!(
                    "download task {} must be completed before export",
                    snapshot.task_id.get()
                ),
            ));
        }

        match snapshot.source.content_format {
            DownloadContentFormat::SingleFile
                if should_run_post_processors_on_completion(&snapshot) =>
            {
                self.export_processed_output(&snapshot, output_path, progress)
            }
            DownloadContentFormat::SingleFile => {
                self.export_single_file_output(&snapshot, output_path, progress)
            }
            DownloadContentFormat::HlsSegments
            | DownloadContentFormat::DashSegments
            | DownloadContentFormat::FlvSegments => {
                self.export_processed_output(&snapshot, output_path, progress)
            }
            DownloadContentFormat::Unknown => Err(PlayerRuntimeError::with_category(
                PlayerRuntimeErrorCode::Unsupported,
                PlayerRuntimeErrorCategory::Capability,
                format!(
                    "download task {} has unknown content format for export",
                    snapshot.task_id.get()
                ),
            )),
        }
    }

    pub fn fail_task(
        &mut self,
        task_id: DownloadTaskId,
        error: PlayerRuntimeError,
        now: Instant,
    ) -> PlayerRuntimeResult<Option<DownloadTaskSnapshot>> {
        let Some(snapshot) = self.store.task(task_id) else {
            return Ok(None);
        };

        if matches!(
            snapshot.status,
            DownloadTaskStatus::Paused
                | DownloadTaskStatus::Completed
                | DownloadTaskStatus::Removed
        ) {
            return Ok(Some(snapshot));
        }

        let error_summary = DownloadErrorSummary::from(error);
        self.pending_preparation_tasks.remove(&task_id);
        self.update_task(task_id, now, |task| {
            task.status = DownloadTaskStatus::Failed;
            task.error_summary = Some(error_summary.clone());
        })
    }

    pub fn remove_task(
        &mut self,
        task_id: DownloadTaskId,
        now: Instant,
    ) -> PlayerRuntimeResult<Option<DownloadTaskSnapshot>> {
        let Some(snapshot) = self.store.task(task_id) else {
            return Ok(None);
        };

        if snapshot.status == DownloadTaskStatus::Removed {
            return Ok(Some(snapshot));
        }

        self.executor.remove(task_id)?;
        self.pending_preparation_tasks.remove(&task_id);
        self.update_task(task_id, now, |task| {
            task.status = DownloadTaskStatus::Removed;
        })
    }

    fn update_task(
        &mut self,
        task_id: DownloadTaskId,
        now: Instant,
        mut mutate: impl FnMut(&mut DownloadTaskSnapshot),
    ) -> PlayerRuntimeResult<Option<DownloadTaskSnapshot>> {
        let Some(mut snapshot) = self.store.task(task_id) else {
            return Ok(None);
        };

        mutate(&mut snapshot);
        snapshot.updated_at = now;
        self.store.save_task(snapshot.clone())?;
        self.emit_event(DownloadEvent::StateChanged(snapshot.state_patch()));
        Ok(Some(snapshot))
    }

    fn update_asset_index(
        &mut self,
        task_id: DownloadTaskId,
        mut asset_index: DownloadAssetIndex,
        now: Instant,
    ) -> PlayerRuntimeResult<Option<DownloadTaskSnapshot>> {
        let Some(mut snapshot) = self.store.task(task_id) else {
            return Ok(None);
        };

        asset_index.content_format = snapshot.source.content_format;
        asset_index.ensure_default_streams();
        snapshot.progress = DownloadProgressSnapshot::from_index(&asset_index);
        snapshot.asset_index = Arc::new(asset_index);
        snapshot.updated_at = now;
        self.store.save_task(snapshot.clone())?;
        self.emit_event(DownloadEvent::AssetIndexUpdated(snapshot.clone()));
        Ok(Some(snapshot))
    }

    fn apply_prepare_result(
        &mut self,
        task_id: DownloadTaskId,
        preparing: DownloadTaskSnapshot,
        prepare_result: DownloadPrepareResult,
        now: Instant,
    ) -> PlayerRuntimeResult<Option<DownloadTaskSnapshot>> {
        match prepare_result {
            DownloadPrepareResult::Ready(asset_index) => {
                if let Some(asset_index) = asset_index {
                    let _ = self.update_asset_index(task_id, asset_index, now)?;
                }
                self.start_prepared_task(task_id, now)
            }
            DownloadPrepareResult::Pending => Ok(Some(preparing)),
        }
    }

    fn start_prepared_task(
        &mut self,
        task_id: DownloadTaskId,
        now: Instant,
    ) -> PlayerRuntimeResult<Option<DownloadTaskSnapshot>> {
        let downloading = self.update_task(task_id, now, |task| {
            task.status = DownloadTaskStatus::Downloading;
            task.error_summary = None;
        })?;
        self.pending_preparation_tasks.remove(&task_id);

        let Some(downloading) = downloading else {
            return Ok(None);
        };

        if let Err(error) = self.executor.start(&downloading) {
            return self.fail_task(task_id, error, now);
        }

        Ok(Some(downloading))
    }

    fn emit_event(&mut self, event: DownloadEvent) {
        self.dispatch_pipeline_events(&event);
        self.events.push(event);
    }

    fn dispatch_pipeline_events(&self, event: &DownloadEvent) {
        match event {
            DownloadEvent::Created(snapshot) => {
                self.dispatch_pipeline_event(PipelineEvent::DownloadTaskCreated {
                    task_id: snapshot.task_id.get().to_string(),
                    asset_id: snapshot.asset_id.as_str().to_owned(),
                });
            }
            DownloadEvent::StateChanged(patch) => {
                self.dispatch_pipeline_event(PipelineEvent::DownloadTaskStateChanged {
                    task_id: patch.task_id.get().to_string(),
                    new_state: patch.status.as_str().to_owned(),
                });

                if patch.status == DownloadTaskStatus::Completed {
                    self.dispatch_pipeline_event(PipelineEvent::DownloadTaskCompleted {
                        task_id: patch.task_id.get().to_string(),
                    });
                }

                if patch.status == DownloadTaskStatus::Failed {
                    self.dispatch_pipeline_event(PipelineEvent::DownloadTaskFailed {
                        task_id: patch.task_id.get().to_string(),
                        error: patch
                            .error_summary
                            .as_ref()
                            .map(|summary| summary.message.clone())
                            .unwrap_or_else(|| "download failed".to_owned()),
                    });
                }
            }
            DownloadEvent::AssetIndexUpdated(_) | DownloadEvent::ProgressUpdated(_) => {}
        }
    }

    fn dispatch_pipeline_event(&self, event: PipelineEvent) {
        for hook in &self.config.event_hooks {
            hook.on_event(&event);
        }
    }

    fn run_post_processors(
        &self,
        snapshot: &DownloadTaskSnapshot,
    ) -> PlayerRuntimeResult<Option<PathBuf>> {
        if self.config.post_processors.is_empty() {
            if download_streams_require_assembly(&snapshot.asset_index.streams) {
                let assembly_mode =
                    infer_assembly_mode_from_download_streams(&snapshot.asset_index.streams);
                return Err(assembly_required_error_for_mode(
                    snapshot,
                    assembly_mode,
                    false,
                ));
            }
            return Ok(snapshot.asset_index.completed_path.clone());
        }

        let mut current_input = self.completed_download_info(snapshot)?;
        let mut current_completed_path = snapshot.asset_index.completed_path.clone();
        let mut required_assembly = completed_info_requires_assembly(&current_input);
        let mut ran_processor = false;
        let progress = NoopProcessorProgress;

        for processor in &self.config.post_processors {
            let input_kind = current_input.content_format.kind();
            let can_process = if completed_info_requires_assembly(&current_input) {
                processor_can_assemble(processor, &current_input)
            } else {
                processor.supported_input_formats().contains(&input_kind)
            };
            if !can_process {
                continue;
            }

            let output_path = derive_processor_output_path(
                snapshot,
                current_completed_path.as_deref(),
                processor,
            )?;
            self.dispatch_pipeline_event(PipelineEvent::PostProcessStarted {
                task_id: snapshot.task_id.get().to_string(),
                processor: processor.name().to_owned(),
            });

            ran_processor = true;
            let result = if completed_info_requires_assembly(&current_input) {
                processor.assemble(&current_input, &output_path, &progress)
            } else {
                processor.process(&current_input, &output_path, &progress)
            };

            match result {
                Ok(ProcessorOutput::MuxedFile { path, .. }) => {
                    self.dispatch_pipeline_event(PipelineEvent::PostProcessCompleted {
                        task_id: snapshot.task_id.get().to_string(),
                        output_path: path.display().to_string(),
                    });
                    current_completed_path = Some(path.clone());
                    current_input = completed_info_for_processed_output(
                        snapshot,
                        path,
                        current_input.metadata.clone(),
                    );
                    required_assembly = completed_info_requires_assembly(&current_input);
                }
                Ok(ProcessorOutput::Skipped) => {}
                Err(error) => {
                    self.dispatch_pipeline_event(PipelineEvent::PostProcessFailed {
                        task_id: snapshot.task_id.get().to_string(),
                        error: error.to_string(),
                    });
                    return Err(map_processor_error(processor.name(), error));
                }
            }
        }

        if required_assembly {
            return Err(assembly_required_error(
                snapshot,
                &current_input,
                ran_processor,
            ));
        }

        Ok(current_completed_path)
    }

    fn export_processed_output(
        &self,
        snapshot: &DownloadTaskSnapshot,
        output_path: Option<&Path>,
        progress: &dyn ProcessorProgress,
    ) -> PlayerRuntimeResult<PathBuf> {
        let mut current_input = self.completed_download_info(snapshot)?;
        let mut current_completed_path = snapshot.asset_index.completed_path.clone();
        let mut ran_processor = false;
        let mut required_assembly = completed_info_requires_assembly(&current_input);

        for processor in &self.config.post_processors {
            let input_kind = current_input.content_format.kind();
            let can_process = if completed_info_requires_assembly(&current_input) {
                processor_can_assemble(processor, &current_input)
            } else {
                processor.supported_input_formats().contains(&input_kind)
            };
            if !can_process {
                continue;
            }

            let resolved_output_path = if ran_processor {
                derive_processor_output_path(
                    snapshot,
                    current_completed_path.as_deref(),
                    processor,
                )?
            } else if let Some(output_path) = output_path {
                output_path.to_path_buf()
            } else {
                derive_processor_output_path(
                    snapshot,
                    current_completed_path.as_deref(),
                    processor,
                )?
            };

            ran_processor = true;
            self.dispatch_pipeline_event(PipelineEvent::PostProcessStarted {
                task_id: snapshot.task_id.get().to_string(),
                processor: processor.name().to_owned(),
            });

            let result = if completed_info_requires_assembly(&current_input) {
                processor.assemble(&current_input, &resolved_output_path, progress)
            } else {
                processor.process(&current_input, &resolved_output_path, progress)
            };

            match result {
                Ok(ProcessorOutput::MuxedFile { path, .. }) => {
                    self.dispatch_pipeline_event(PipelineEvent::PostProcessCompleted {
                        task_id: snapshot.task_id.get().to_string(),
                        output_path: path.display().to_string(),
                    });
                    current_completed_path = Some(path.clone());
                    current_input = completed_info_for_processed_output(
                        snapshot,
                        path,
                        current_input.metadata.clone(),
                    );
                    required_assembly = completed_info_requires_assembly(&current_input);
                }
                Ok(ProcessorOutput::Skipped) => {}
                Err(error) => {
                    self.dispatch_pipeline_event(PipelineEvent::PostProcessFailed {
                        task_id: snapshot.task_id.get().to_string(),
                        error: error.to_string(),
                    });
                    return Err(map_processor_error(processor.name(), error));
                }
            }
        }

        if required_assembly {
            return Err(assembly_required_error(
                snapshot,
                &current_input,
                ran_processor,
            ));
        }

        if ran_processor && let Some(path) = current_completed_path {
            return Ok(path);
        }

        Err(PlayerRuntimeError::with_category(
            PlayerRuntimeErrorCode::Unsupported,
            PlayerRuntimeErrorCategory::Capability,
            format!(
                "download task {} has no post-download processor available for export",
                snapshot.task_id.get()
            ),
        ))
    }

    fn export_single_file_output(
        &self,
        snapshot: &DownloadTaskSnapshot,
        output_path: Option<&Path>,
        progress: &dyn ProcessorProgress,
    ) -> PlayerRuntimeResult<PathBuf> {
        let source_path = resolve_single_file_path(snapshot)?;
        let Some(output_path) = output_path else {
            return Ok(source_path);
        };
        if paths_refer_to_same_file(&source_path, output_path) {
            return Ok(source_path);
        }

        copy_single_file_export(&source_path, output_path, progress)?;
        Ok(output_path.to_path_buf())
    }

    fn completed_download_info(
        &self,
        snapshot: &DownloadTaskSnapshot,
    ) -> PlayerRuntimeResult<CompletedDownloadInfo> {
        let metadata = DownloadMetadata {
            source_uri: Some(snapshot.source.source.uri().to_owned()),
            manifest_uri: snapshot.source.manifest_uri.clone(),
            total_bytes: snapshot.progress.total_bytes,
            version: snapshot.asset_index.version.clone(),
            etag: snapshot.asset_index.etag.clone(),
            checksum: snapshot.asset_index.checksum.clone(),
            mime_type: None,
            custom: Default::default(),
        };

        let content_format = match snapshot.source.content_format {
            DownloadContentFormat::HlsSegments => CompletedContentFormat::HlsSegments {
                manifest_path: resolve_manifest_path(snapshot)?,
                segment_paths: resolve_segment_paths(snapshot),
            },
            DownloadContentFormat::DashSegments => CompletedContentFormat::DashSegments {
                manifest_path: resolve_manifest_path(snapshot)?,
                segment_paths: resolve_segment_paths(snapshot),
            },
            DownloadContentFormat::FlvSegments => CompletedContentFormat::FlvSegments {
                manifest_path: resolve_manifest_path(snapshot)?,
                segment_paths: resolve_segment_paths(snapshot),
            },
            DownloadContentFormat::SingleFile => CompletedContentFormat::SingleFile {
                path: resolve_single_file_path(snapshot)?,
            },
            DownloadContentFormat::Unknown => {
                return Err(PlayerRuntimeError::with_category(
                    PlayerRuntimeErrorCode::Unsupported,
                    PlayerRuntimeErrorCategory::Capability,
                    format!(
                        "download task {} has unknown content format for post-processing",
                        snapshot.task_id.get()
                    ),
                ));
            }
        };

        Ok(CompletedDownloadInfo {
            asset_id: snapshot.asset_id.as_str().to_owned(),
            task_id: Some(snapshot.task_id.get().to_string()),
            content_format,
            metadata,
            streams: completed_streams_for_snapshot(snapshot)?,
            assembly_mode: infer_assembly_mode_from_download_streams(&snapshot.asset_index.streams),
        })
    }
}

struct NoopProcessorProgress;

impl ProcessorProgress for NoopProcessorProgress {
    fn on_progress(&self, _ratio: f32) {}
}

fn completed_info_requires_assembly(input: &CompletedDownloadInfo) -> bool {
    input.assembly_mode != AssemblyMode::Single && input.streams.len() > 1
}

fn download_streams_require_assembly(streams: &[DownloadAssetStream]) -> bool {
    infer_assembly_mode_from_download_streams(streams) != AssemblyMode::Single && streams.len() > 1
}

fn processor_can_assemble(
    processor: &Arc<dyn PostDownloadProcessor>,
    input: &CompletedDownloadInfo,
) -> bool {
    if !processor.supports_assembly() {
        return false;
    }
    let capabilities = processor.capabilities();
    capabilities
        .supported_assembly_modes
        .contains(&input.assembly_mode)
        && input.streams.iter().all(|stream| {
            capabilities
                .supported_input_formats
                .contains(&stream.content_format.kind())
        })
}

fn assembly_required_error(
    snapshot: &DownloadTaskSnapshot,
    input: &CompletedDownloadInfo,
    ran_processor: bool,
) -> PlayerRuntimeError {
    assembly_required_error_for_mode(snapshot, input.assembly_mode, ran_processor)
}

fn assembly_required_error_for_mode(
    snapshot: &DownloadTaskSnapshot,
    assembly_mode: AssemblyMode,
    ran_processor: bool,
) -> PlayerRuntimeError {
    let detail = if ran_processor {
        "no processor produced an assembled output"
    } else {
        "no processor supports this assembly mode"
    };
    PlayerRuntimeError::with_category(
        PlayerRuntimeErrorCode::Unsupported,
        PlayerRuntimeErrorCategory::Capability,
        format!(
            "download task {} requires post-download assembly for {:?}: {detail}",
            snapshot.task_id.get(),
            assembly_mode
        ),
    )
}

fn completed_info_for_processed_output(
    snapshot: &DownloadTaskSnapshot,
    path: PathBuf,
    metadata: DownloadMetadata,
) -> CompletedDownloadInfo {
    let content_format = CompletedContentFormat::SingleFile { path: path.clone() };
    CompletedDownloadInfo {
        asset_id: snapshot.asset_id.as_str().to_owned(),
        task_id: Some(snapshot.task_id.get().to_string()),
        content_format: content_format.clone(),
        metadata: metadata.clone(),
        streams: vec![CompletedStream {
            stream_id: Some("combined".to_owned()),
            kind: StreamKind::Combined,
            content_format,
            language: None,
            codec: None,
            label: None,
            metadata,
            quality_rank: None,
        }],
        assembly_mode: AssemblyMode::Single,
    }
}

fn completed_streams_for_snapshot(
    snapshot: &DownloadTaskSnapshot,
) -> PlayerRuntimeResult<Vec<CompletedStream>> {
    let streams = effective_asset_streams(&snapshot.asset_index);
    streams
        .iter()
        .map(|stream| {
            Ok(CompletedStream {
                stream_id: Some(stream.stream_id.clone()),
                kind: download_stream_kind_to_plugin(stream.kind),
                content_format: content_format_for_stream(snapshot, stream)?,
                language: stream.language.clone(),
                codec: stream.codec.clone(),
                label: stream.label.clone(),
                metadata: DownloadMetadata {
                    source_uri: None,
                    manifest_uri: None,
                    total_bytes: None,
                    version: None,
                    etag: None,
                    checksum: None,
                    mime_type: None,
                    custom: stream.metadata.clone().into_iter().collect(),
                },
                quality_rank: stream.quality_rank,
            })
        })
        .collect()
}

fn effective_asset_streams(asset_index: &DownloadAssetIndex) -> Vec<DownloadAssetStream> {
    if !asset_index.streams.is_empty() {
        return asset_index.streams.clone();
    }
    vec![DownloadAssetStream {
        stream_id: "combined".to_owned(),
        kind: DownloadStreamKind::Combined,
        language: None,
        codec: None,
        label: None,
        quality_rank: None,
        resource_ids: asset_index
            .resources
            .iter()
            .map(|resource| resource.resource_id.clone())
            .collect(),
        segment_ids: asset_index
            .segments
            .iter()
            .map(|segment| segment.segment_id.clone())
            .collect(),
        metadata: HashMap::new(),
    }]
}

fn content_format_for_stream(
    snapshot: &DownloadTaskSnapshot,
    stream: &DownloadAssetStream,
) -> PlayerRuntimeResult<CompletedContentFormat> {
    match snapshot.source.content_format {
        DownloadContentFormat::HlsSegments => Ok(CompletedContentFormat::HlsSegments {
            manifest_path: resolve_stream_manifest_path(snapshot, stream, "m3u8")?,
            segment_paths: resolve_stream_segment_paths(snapshot, stream),
        }),
        DownloadContentFormat::DashSegments => Ok(CompletedContentFormat::DashSegments {
            manifest_path: resolve_stream_manifest_path(snapshot, stream, "mpd")?,
            segment_paths: resolve_stream_segment_paths(snapshot, stream),
        }),
        DownloadContentFormat::FlvSegments => Ok(CompletedContentFormat::FlvSegments {
            manifest_path: resolve_stream_manifest_path(snapshot, stream, "ffconcat")?,
            segment_paths: resolve_stream_segment_paths(snapshot, stream),
        }),
        DownloadContentFormat::SingleFile => Ok(CompletedContentFormat::SingleFile {
            path: resolve_stream_single_file_path(snapshot, stream)?,
        }),
        DownloadContentFormat::Unknown => Err(PlayerRuntimeError::with_category(
            PlayerRuntimeErrorCode::Unsupported,
            PlayerRuntimeErrorCategory::Capability,
            format!(
                "download task {} has unknown content format for stream assembly",
                snapshot.task_id.get()
            ),
        )),
    }
}

fn resolve_stream_manifest_path(
    snapshot: &DownloadTaskSnapshot,
    stream: &DownloadAssetStream,
    extension: &str,
) -> PlayerRuntimeResult<PathBuf> {
    let target_directory = target_directory_for_snapshot(snapshot)?;
    stream
        .resource_ids
        .iter()
        .filter_map(|resource_id| {
            snapshot
                .asset_index
                .resources
                .iter()
                .find(|resource| &resource.resource_id == resource_id)
        })
        .find_map(|resource| {
            let relative_path = resource.relative_path.as_ref()?;
            relative_path
                .extension()
                .and_then(|value| value.to_str())
                .is_some_and(|value| value.eq_ignore_ascii_case(extension))
                .then(|| target_directory.join(relative_path))
        })
        .or_else(|| resolve_manifest_path(snapshot).ok())
        .ok_or_else(|| {
            PlayerRuntimeError::with_category(
                PlayerRuntimeErrorCode::InvalidState,
                PlayerRuntimeErrorCategory::Playback,
                format!(
                    "download task {} is missing a stream manifest for `{}`",
                    snapshot.task_id.get(),
                    stream.stream_id
                ),
            )
        })
}

fn resolve_stream_segment_paths(
    snapshot: &DownloadTaskSnapshot,
    stream: &DownloadAssetStream,
) -> Vec<PathBuf> {
    let Some(target_directory) = snapshot.profile.target_directory.as_ref() else {
        return Vec::new();
    };
    if stream.segment_ids.is_empty() {
        return resolve_segment_paths(snapshot);
    }
    stream
        .segment_ids
        .iter()
        .filter_map(|segment_id| {
            snapshot
                .asset_index
                .segments
                .iter()
                .find(|segment| &segment.segment_id == segment_id)
                .and_then(|segment| segment.relative_path.as_ref())
                .map(|relative_path| target_directory.join(relative_path))
        })
        .collect()
}

fn resolve_stream_single_file_path(
    snapshot: &DownloadTaskSnapshot,
    stream: &DownloadAssetStream,
) -> PlayerRuntimeResult<PathBuf> {
    let target_directory = target_directory_for_snapshot(snapshot)?;
    stream
        .resource_ids
        .iter()
        .filter_map(|resource_id| {
            snapshot
                .asset_index
                .resources
                .iter()
                .find(|resource| &resource.resource_id == resource_id)
        })
        .find_map(|resource| {
            resource
                .relative_path
                .as_ref()
                .map(|relative_path| target_directory.join(relative_path))
        })
        .or_else(|| resolve_single_file_path(snapshot).ok())
        .ok_or_else(|| {
            PlayerRuntimeError::with_category(
                PlayerRuntimeErrorCode::InvalidState,
                PlayerRuntimeErrorCategory::Playback,
                format!(
                    "download task {} is missing a stream file for `{}`",
                    snapshot.task_id.get(),
                    stream.stream_id
                ),
            )
        })
}

fn target_directory_for_snapshot(snapshot: &DownloadTaskSnapshot) -> PlayerRuntimeResult<&Path> {
    snapshot.profile.target_directory.as_deref().ok_or_else(|| {
        PlayerRuntimeError::with_category(
            PlayerRuntimeErrorCode::InvalidState,
            PlayerRuntimeErrorCategory::Playback,
            format!(
                "download task {} is missing target directory",
                snapshot.task_id.get()
            ),
        )
    })
}

fn infer_assembly_mode_from_download_streams(streams: &[DownloadAssetStream]) -> AssemblyMode {
    if streams.len() <= 1 {
        return AssemblyMode::Single;
    }

    let has_video = streams
        .iter()
        .any(|stream| matches!(stream.kind, DownloadStreamKind::Video));
    let audio_count = streams
        .iter()
        .filter(|stream| {
            matches!(
                stream.kind,
                DownloadStreamKind::Audio | DownloadStreamKind::SecondaryAudio
            )
        })
        .count();
    let has_subtitle = streams
        .iter()
        .any(|stream| matches!(stream.kind, DownloadStreamKind::Subtitle));

    if has_subtitle {
        AssemblyMode::WithSubtitles
    } else if has_video && audio_count > 1 {
        AssemblyMode::MultiAudio
    } else if has_video && audio_count == 1 {
        AssemblyMode::SeparateAudioVideo
    } else {
        AssemblyMode::Generic
    }
}

fn download_stream_kind_to_plugin(kind: DownloadStreamKind) -> StreamKind {
    match kind {
        DownloadStreamKind::Combined => StreamKind::Combined,
        DownloadStreamKind::Video => StreamKind::Video,
        DownloadStreamKind::Audio => StreamKind::Audio,
        DownloadStreamKind::SecondaryAudio => StreamKind::SecondaryAudio,
        DownloadStreamKind::Subtitle => StreamKind::Subtitle,
        DownloadStreamKind::Auxiliary => StreamKind::Auxiliary,
    }
}

fn derive_processor_output_path(
    snapshot: &DownloadTaskSnapshot,
    current_completed_path: Option<&Path>,
    processor: &Arc<dyn PostDownloadProcessor>,
) -> PlayerRuntimeResult<PathBuf> {
    let extension = processor
        .capabilities()
        .output_formats
        .first()
        .map(output_format_extension)
        .or_else(|| {
            current_completed_path
                .and_then(Path::extension)
                .and_then(|extension| extension.to_str())
                .map(str::to_owned)
        })
        .unwrap_or_else(|| "bin".to_owned());

    if let Some(path) = current_completed_path {
        if path.extension().is_some() {
            return Ok(path.with_extension(&extension));
        }
        return Ok(path.join(format!(
            "{}.{extension}",
            sanitize_asset_id(snapshot.asset_id.as_str())
        )));
    }

    if let Some(base_dir) = snapshot.profile.target_directory.as_ref() {
        return Ok(base_dir.join(format!(
            "{}.{extension}",
            sanitize_asset_id(snapshot.asset_id.as_str())
        )));
    }

    Err(PlayerRuntimeError::with_category(
        PlayerRuntimeErrorCode::InvalidState,
        PlayerRuntimeErrorCategory::Playback,
        format!(
            "download task {} has no completed path or target directory for processor `{}` output",
            snapshot.task_id.get(),
            processor.name(),
        ),
    ))
}

fn output_format_extension(format: &OutputFormat) -> String {
    match format {
        OutputFormat::Mp4 => "mp4".to_owned(),
        OutputFormat::Mkv => "mkv".to_owned(),
        OutputFormat::Original => "bin".to_owned(),
    }
}

fn should_run_post_processors_on_completion(snapshot: &DownloadTaskSnapshot) -> bool {
    match snapshot.source.content_format {
        DownloadContentFormat::HlsSegments
        | DownloadContentFormat::DashSegments
        | DownloadContentFormat::FlvSegments => snapshot
            .profile
            .target_output_format
            .as_ref()
            .is_none_or(|format| *format == OutputFormat::Mp4),
        DownloadContentFormat::SingleFile => snapshot
            .profile
            .target_output_format
            .as_ref()
            .is_some_and(|format| *format == OutputFormat::Mp4),
        DownloadContentFormat::Unknown => false,
    }
}

fn task_needs_preparation(snapshot: &DownloadTaskSnapshot) -> bool {
    snapshot.asset_index.resources.is_empty()
        && snapshot.asset_index.segments.is_empty()
        && snapshot.asset_index.total_size_bytes.is_none()
        && snapshot.asset_index.completed_path.is_none()
}

fn sanitize_asset_id(asset_id: &str) -> String {
    let sanitized = asset_id
        .chars()
        .map(|character| match character {
            'a'..='z' | 'A'..='Z' | '0'..='9' | '-' | '_' => character,
            _ => '_',
        })
        .collect::<String>();

    if sanitized.is_empty() {
        "download".to_owned()
    } else {
        sanitized
    }
}

fn resolve_manifest_path(snapshot: &DownloadTaskSnapshot) -> PlayerRuntimeResult<PathBuf> {
    if let Some(path) = snapshot.asset_index.completed_path.as_ref()
        && is_manifest_path_for_format(path, snapshot.source.content_format)
    {
        return Ok(path.clone());
    }

    if let Some(path) = snapshot
        .asset_index
        .resources
        .iter()
        .find_map(|resource| {
            resolve_index_path(snapshot, resource.relative_path.as_deref(), &resource.uri)
        })
        .filter(|path| is_manifest_path_for_format(path, snapshot.source.content_format))
    {
        return Ok(path);
    }

    if let Some(path) = snapshot
        .source
        .manifest_uri
        .as_deref()
        .and_then(resolve_uri_to_path)
    {
        return Ok(path);
    }

    Err(PlayerRuntimeError::with_category(
        PlayerRuntimeErrorCode::InvalidSource,
        PlayerRuntimeErrorCategory::Source,
        format!(
            "download task {} is missing a local manifest path for post-processing",
            snapshot.task_id.get()
        ),
    ))
}

fn is_manifest_path_for_format(path: &Path, content_format: DownloadContentFormat) -> bool {
    let extension = path
        .extension()
        .and_then(|extension| extension.to_str())
        .unwrap_or_default();
    match content_format {
        DownloadContentFormat::HlsSegments => extension.eq_ignore_ascii_case("m3u8"),
        DownloadContentFormat::DashSegments => extension.eq_ignore_ascii_case("mpd"),
        DownloadContentFormat::FlvSegments => {
            extension.eq_ignore_ascii_case("ffconcat")
                || extension.eq_ignore_ascii_case("txt")
                || extension.eq_ignore_ascii_case("flv")
        }
        DownloadContentFormat::SingleFile | DownloadContentFormat::Unknown => false,
    }
}

fn resolve_segment_paths(snapshot: &DownloadTaskSnapshot) -> Vec<PathBuf> {
    snapshot
        .asset_index
        .segments
        .iter()
        .filter_map(|segment| {
            resolve_index_path(snapshot, segment.relative_path.as_deref(), &segment.uri)
        })
        .collect()
}

fn resolve_single_file_path(snapshot: &DownloadTaskSnapshot) -> PlayerRuntimeResult<PathBuf> {
    if let Some(path) = snapshot.asset_index.completed_path.as_ref() {
        return Ok(path.clone());
    }

    if let Some(path) = snapshot.asset_index.resources.iter().find_map(|resource| {
        resolve_index_path(snapshot, resource.relative_path.as_deref(), &resource.uri)
    }) {
        return Ok(path);
    }

    if let Some(path) = resolve_uri_to_path(snapshot.source.source.uri()) {
        return Ok(path);
    }

    Err(PlayerRuntimeError::with_category(
        PlayerRuntimeErrorCode::InvalidSource,
        PlayerRuntimeErrorCategory::Source,
        format!(
            "download task {} is missing a local completed file path for post-processing",
            snapshot.task_id.get()
        ),
    ))
}

fn copy_single_file_export(
    source_path: &Path,
    output_path: &Path,
    progress: &dyn ProcessorProgress,
) -> PlayerRuntimeResult<()> {
    if progress.is_cancelled() {
        return Err(PlayerRuntimeError::with_category(
            PlayerRuntimeErrorCode::BackendFailure,
            PlayerRuntimeErrorCategory::Platform,
            "download export was cancelled",
        ));
    }

    let total_bytes = fs::metadata(source_path)
        .map_err(|error| export_io_error(source_path, "read source metadata", error))?
        .len();
    if let Some(parent) = output_path.parent()
        && !parent.as_os_str().is_empty()
    {
        fs::create_dir_all(parent)
            .map_err(|error| export_io_error(parent, "create export directory", error))?;
    }

    let mut input = File::open(source_path)
        .map_err(|error| export_io_error(source_path, "open export source", error))?;
    let mut output = File::create(output_path)
        .map_err(|error| export_io_error(output_path, "create export output", error))?;
    let mut copied_bytes = 0_u64;
    let mut buffer = [0_u8; 1024 * 1024];

    loop {
        if progress.is_cancelled() {
            return Err(PlayerRuntimeError::with_category(
                PlayerRuntimeErrorCode::BackendFailure,
                PlayerRuntimeErrorCategory::Platform,
                "download export was cancelled",
            ));
        }
        let read = input
            .read(&mut buffer)
            .map_err(|error| export_io_error(source_path, "read export source", error))?;
        if read == 0 {
            break;
        }
        output
            .write_all(&buffer[..read])
            .map_err(|error| export_io_error(output_path, "write export output", error))?;
        copied_bytes = copied_bytes.saturating_add(read as u64);
        if total_bytes > 0 {
            progress.on_progress((copied_bytes as f32 / total_bytes as f32).clamp(0.0, 1.0));
        }
    }
    output
        .sync_all()
        .map_err(|error| export_io_error(output_path, "flush export output", error))?;
    progress.on_progress(1.0);
    Ok(())
}

fn paths_refer_to_same_file(left: &Path, right: &Path) -> bool {
    if left == right {
        return true;
    }
    match (left.canonicalize(), right.canonicalize()) {
        (Ok(left), Ok(right)) => left == right,
        _ => false,
    }
}

fn export_io_error(path: &Path, operation: &str, error: std::io::Error) -> PlayerRuntimeError {
    PlayerRuntimeError::with_category(
        PlayerRuntimeErrorCode::BackendFailure,
        PlayerRuntimeErrorCategory::Platform,
        format!("failed to {operation} `{}`: {error}", path.display()),
    )
}

fn resolve_index_path(
    snapshot: &DownloadTaskSnapshot,
    relative_path: Option<&Path>,
    uri: &str,
) -> Option<PathBuf> {
    if let Some(relative_path) = relative_path {
        if relative_path.is_absolute() {
            return Some(relative_path.to_path_buf());
        }
        if let Some(base_dir) = snapshot.profile.target_directory.as_ref() {
            return Some(base_dir.join(relative_path));
        }
    }

    resolve_uri_to_path(uri)
}

fn resolve_uri_to_path(uri: &str) -> Option<PathBuf> {
    if let Some(path) = uri.strip_prefix("file://") {
        if path.is_empty() {
            return None;
        }
        return Some(PathBuf::from(path));
    }

    if uri.contains("://") {
        return None;
    }

    if uri.trim().is_empty() {
        None
    } else {
        Some(PathBuf::from(uri))
    }
}

fn map_processor_error(processor_name: &str, error: ProcessorError) -> PlayerRuntimeError {
    match error {
        ProcessorError::UnsupportedFormat(_) => PlayerRuntimeError::with_category(
            PlayerRuntimeErrorCode::Unsupported,
            PlayerRuntimeErrorCategory::Capability,
            format!("post-processor `{processor_name}` does not support this download format"),
        ),
        ProcessorError::PayloadCodec(message) => PlayerRuntimeError::with_category(
            PlayerRuntimeErrorCode::BackendFailure,
            PlayerRuntimeErrorCategory::Platform,
            format!("post-processor `{processor_name}` exchanged invalid payload: {message}"),
        ),
        ProcessorError::AbiViolation(message) => PlayerRuntimeError::with_category(
            PlayerRuntimeErrorCode::BackendFailure,
            PlayerRuntimeErrorCategory::Platform,
            format!("post-processor `{processor_name}` violated plugin ABI: {message}"),
        ),
        ProcessorError::OutputPath(message) => PlayerRuntimeError::with_category(
            PlayerRuntimeErrorCode::InvalidArgument,
            PlayerRuntimeErrorCategory::Input,
            format!("post-processor `{processor_name}` output path error: {message}"),
        ),
        ProcessorError::Cancelled => PlayerRuntimeError::with_category(
            PlayerRuntimeErrorCode::BackendFailure,
            PlayerRuntimeErrorCategory::Playback,
            format!("post-processor `{processor_name}` was cancelled"),
        ),
        ProcessorError::MuxFailed(message) => PlayerRuntimeError::with_category(
            PlayerRuntimeErrorCode::BackendFailure,
            PlayerRuntimeErrorCategory::Platform,
            format!("post-processor `{processor_name}` failed: {message}"),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        DownloadAssetId, DownloadAssetIndex, DownloadContentFormat, DownloadEvent, DownloadManager,
        DownloadManagerConfig, DownloadPrepareResult, DownloadProfile, DownloadResourceRecord,
        DownloadSegmentRecord, DownloadSource, DownloadTaskId, DownloadTaskStatus,
        InMemoryDownloadExecutor, InMemoryDownloadStore,
    };
    use crate::download::NoopProcessorProgress;
    use crate::{
        DownloadAssetStream, DownloadStreamKind, PlayerRuntimeError, PlayerRuntimeErrorCategory,
        PlayerRuntimeErrorCode, PlayerRuntimeResult,
    };
    use player_model::MediaSource;
    use player_plugin::{
        AssemblyMode, CompletedDownloadInfo, ContentFormatKind, OutputFormat, PipelineEvent,
        PipelineEventHook, PostDownloadProcessor, ProcessorCapabilities, ProcessorError,
        ProcessorOutput, ProcessorProgress,
    };
    use std::collections::HashMap;
    use std::fs;
    use std::path::PathBuf;
    use std::sync::{Arc, Mutex};
    use std::time::{Instant, SystemTime, UNIX_EPOCH};

    #[derive(Debug, Default)]
    struct RecordingHook {
        events: Mutex<Vec<PipelineEvent>>,
    }

    #[derive(Debug, Default)]
    struct RecordingProcessor {
        invocations: Mutex<Vec<(CompletedDownloadInfo, PathBuf)>>,
    }

    impl RecordingProcessor {
        fn invocations(&self) -> Vec<(CompletedDownloadInfo, PathBuf)> {
            match self.invocations.lock() {
                Ok(invocations) => invocations.clone(),
                Err(poisoned) => poisoned.into_inner().clone(),
            }
        }
    }

    impl PostDownloadProcessor for RecordingProcessor {
        fn name(&self) -> &str {
            "recording-processor"
        }

        fn supported_input_formats(&self) -> &[ContentFormatKind] {
            static SUPPORTED: [ContentFormatKind; 1] = [ContentFormatKind::HlsSegments];
            &SUPPORTED
        }

        fn capabilities(&self) -> ProcessorCapabilities {
            ProcessorCapabilities {
                supported_input_formats: vec![ContentFormatKind::HlsSegments],
                output_formats: vec![OutputFormat::Mp4],
                supports_cancellation: false,
                supports_assembly: true,
                supported_assembly_modes: vec![AssemblyMode::SeparateAudioVideo],
            }
        }

        fn process(
            &self,
            input: &CompletedDownloadInfo,
            output_path: &std::path::Path,
            progress: &dyn ProcessorProgress,
        ) -> Result<ProcessorOutput, ProcessorError> {
            progress.on_progress(1.0);
            match self.invocations.lock() {
                Ok(mut invocations) => invocations.push((input.clone(), output_path.to_path_buf())),
                Err(poisoned) => poisoned
                    .into_inner()
                    .push((input.clone(), output_path.to_path_buf())),
            }
            Ok(ProcessorOutput::MuxedFile {
                path: output_path.to_path_buf(),
                format: OutputFormat::Mp4,
            })
        }

        fn assemble(
            &self,
            input: &CompletedDownloadInfo,
            output_path: &std::path::Path,
            progress: &dyn ProcessorProgress,
        ) -> Result<ProcessorOutput, ProcessorError> {
            self.process(input, output_path, progress)
        }
    }

    #[derive(Debug, Default)]
    struct FailingProcessor;

    #[derive(Debug, Default)]
    struct SkippingAssemblyProcessor;

    #[derive(Debug, Default)]
    struct RecordingProgress {
        ratios: Mutex<Vec<f32>>,
    }

    #[derive(Debug, Default)]
    struct PendingPrepareExecutor {
        prepared: Vec<DownloadTaskId>,
        started: Vec<DownloadTaskId>,
        paused: Vec<DownloadTaskId>,
        removed: Vec<DownloadTaskId>,
    }

    impl RecordingProgress {
        fn ratios(&self) -> Vec<f32> {
            match self.ratios.lock() {
                Ok(ratios) => ratios.clone(),
                Err(poisoned) => poisoned.into_inner().clone(),
            }
        }
    }

    impl PostDownloadProcessor for FailingProcessor {
        fn name(&self) -> &str {
            "failing-processor"
        }

        fn supported_input_formats(&self) -> &[ContentFormatKind] {
            static SUPPORTED: [ContentFormatKind; 1] = [ContentFormatKind::HlsSegments];
            &SUPPORTED
        }

        fn capabilities(&self) -> ProcessorCapabilities {
            ProcessorCapabilities {
                supported_input_formats: vec![ContentFormatKind::HlsSegments],
                output_formats: vec![OutputFormat::Mp4],
                supports_cancellation: false,
                supports_assembly: false,
                supported_assembly_modes: Vec::new(),
            }
        }

        fn process(
            &self,
            _input: &CompletedDownloadInfo,
            _output_path: &std::path::Path,
            _progress: &dyn ProcessorProgress,
        ) -> Result<ProcessorOutput, ProcessorError> {
            Err(ProcessorError::MuxFailed("ffmpeg remux failed".to_owned()))
        }
    }

    impl PostDownloadProcessor for SkippingAssemblyProcessor {
        fn name(&self) -> &str {
            "skipping-assembly-processor"
        }

        fn supported_input_formats(&self) -> &[ContentFormatKind] {
            static SUPPORTED: [ContentFormatKind; 1] = [ContentFormatKind::HlsSegments];
            &SUPPORTED
        }

        fn capabilities(&self) -> ProcessorCapabilities {
            ProcessorCapabilities {
                supported_input_formats: vec![ContentFormatKind::HlsSegments],
                output_formats: vec![OutputFormat::Mp4],
                supports_cancellation: false,
                supports_assembly: true,
                supported_assembly_modes: vec![AssemblyMode::SeparateAudioVideo],
            }
        }

        fn process(
            &self,
            _input: &CompletedDownloadInfo,
            _output_path: &std::path::Path,
            _progress: &dyn ProcessorProgress,
        ) -> Result<ProcessorOutput, ProcessorError> {
            Ok(ProcessorOutput::Skipped)
        }

        fn assemble(
            &self,
            _input: &CompletedDownloadInfo,
            _output_path: &std::path::Path,
            _progress: &dyn ProcessorProgress,
        ) -> Result<ProcessorOutput, ProcessorError> {
            Ok(ProcessorOutput::Skipped)
        }
    }

    impl super::DownloadExecutor for PendingPrepareExecutor {
        fn prepare(
            &mut self,
            task: &super::DownloadTaskSnapshot,
        ) -> PlayerRuntimeResult<DownloadPrepareResult> {
            self.prepared.push(task.task_id);
            Ok(DownloadPrepareResult::Pending)
        }

        fn start(&mut self, task: &super::DownloadTaskSnapshot) -> PlayerRuntimeResult<()> {
            self.started.push(task.task_id);
            Ok(())
        }

        fn pause(&mut self, task_id: DownloadTaskId) -> PlayerRuntimeResult<()> {
            self.paused.push(task_id);
            Ok(())
        }

        fn resume(&mut self, task: &super::DownloadTaskSnapshot) -> PlayerRuntimeResult<()> {
            self.started.push(task.task_id);
            Ok(())
        }

        fn remove(&mut self, task_id: DownloadTaskId) -> PlayerRuntimeResult<()> {
            self.removed.push(task_id);
            Ok(())
        }
    }

    impl RecordingHook {
        fn events(&self) -> Vec<PipelineEvent> {
            match self.events.lock() {
                Ok(events) => events.clone(),
                Err(poisoned) => poisoned.into_inner().clone(),
            }
        }
    }

    impl PipelineEventHook for RecordingHook {
        fn on_event(&self, event: &PipelineEvent) {
            match self.events.lock() {
                Ok(mut events) => events.push(event.clone()),
                Err(poisoned) => poisoned.into_inner().push(event.clone()),
            }
        }
    }

    impl ProcessorProgress for RecordingProgress {
        fn on_progress(&self, ratio: f32) {
            match self.ratios.lock() {
                Ok(mut ratios) => ratios.push(ratio),
                Err(poisoned) => poisoned.into_inner().push(ratio),
            }
        }
    }

    fn source(uri: &str) -> DownloadSource {
        DownloadSource::new(MediaSource::new(uri), DownloadContentFormat::HlsSegments)
            .with_manifest_uri(uri)
    }

    fn asset_index(total_size_bytes: u64) -> DownloadAssetIndex {
        DownloadAssetIndex {
            total_size_bytes: Some(total_size_bytes),
            ..DownloadAssetIndex::default()
        }
    }

    fn segmented_asset_index(total_size_bytes: u64) -> DownloadAssetIndex {
        DownloadAssetIndex {
            total_size_bytes: Some(total_size_bytes),
            resources: vec![DownloadResourceRecord {
                resource_id: "manifest".to_owned(),
                uri: "playlist.m3u8".to_owned(),
                relative_path: Some(PathBuf::from("playlist.m3u8")),
                byte_range: None,
                generated_text: None,
                size_bytes: None,
                etag: None,
                checksum: None,
            }],
            segments: vec![
                DownloadSegmentRecord {
                    segment_id: "seg-1".to_owned(),
                    uri: "seg-1.ts".to_owned(),
                    relative_path: Some(PathBuf::from("seg-1.ts")),
                    sequence: Some(1),
                    byte_range: None,
                    size_bytes: Some(512),
                    checksum: None,
                },
                DownloadSegmentRecord {
                    segment_id: "seg-2".to_owned(),
                    uri: "seg-2.ts".to_owned(),
                    relative_path: Some(PathBuf::from("seg-2.ts")),
                    sequence: Some(2),
                    byte_range: None,
                    size_bytes: Some(512),
                    checksum: None,
                },
            ],
            ..DownloadAssetIndex::default()
        }
    }

    fn multi_stream_hls_asset_index(total_size_bytes: u64) -> DownloadAssetIndex {
        DownloadAssetIndex {
            total_size_bytes: Some(total_size_bytes),
            resources: vec![
                DownloadResourceRecord {
                    resource_id: "video-playlist".to_owned(),
                    uri: "video.m3u8".to_owned(),
                    relative_path: Some(PathBuf::from("video.m3u8")),
                    byte_range: None,
                    generated_text: None,
                    size_bytes: None,
                    etag: None,
                    checksum: None,
                },
                DownloadResourceRecord {
                    resource_id: "audio-playlist".to_owned(),
                    uri: "audio.m3u8".to_owned(),
                    relative_path: Some(PathBuf::from("audio.m3u8")),
                    byte_range: None,
                    generated_text: None,
                    size_bytes: None,
                    etag: None,
                    checksum: None,
                },
            ],
            segments: vec![
                DownloadSegmentRecord {
                    segment_id: "video-seg-1".to_owned(),
                    uri: "video-1.ts".to_owned(),
                    relative_path: Some(PathBuf::from("video-1.ts")),
                    sequence: Some(1),
                    byte_range: None,
                    size_bytes: Some(768),
                    checksum: None,
                },
                DownloadSegmentRecord {
                    segment_id: "audio-seg-1".to_owned(),
                    uri: "audio-1.aac".to_owned(),
                    relative_path: Some(PathBuf::from("audio-1.aac")),
                    sequence: Some(1),
                    byte_range: None,
                    size_bytes: Some(256),
                    checksum: None,
                },
            ],
            streams: vec![
                DownloadAssetStream {
                    stream_id: "video".to_owned(),
                    kind: DownloadStreamKind::Video,
                    language: None,
                    codec: Some("avc1.640028".to_owned()),
                    label: Some("1080p".to_owned()),
                    quality_rank: Some(0),
                    resource_ids: vec!["video-playlist".to_owned()],
                    segment_ids: vec!["video-seg-1".to_owned()],
                    metadata: HashMap::new(),
                },
                DownloadAssetStream {
                    stream_id: "audio".to_owned(),
                    kind: DownloadStreamKind::Audio,
                    language: Some("en".to_owned()),
                    codec: Some("mp4a.40.2".to_owned()),
                    label: Some("English".to_owned()),
                    quality_rank: None,
                    resource_ids: vec!["audio-playlist".to_owned()],
                    segment_ids: vec!["audio-seg-1".to_owned()],
                    metadata: HashMap::new(),
                },
            ],
            ..DownloadAssetIndex::default()
        }
    }

    #[test]
    fn manager_creates_and_auto_starts_tasks() {
        let config = DownloadManagerConfig {
            auto_start: true,
            run_post_processors_on_completion: true,
            ..DownloadManagerConfig::default()
        };
        let store = InMemoryDownloadStore::default();
        let executor = InMemoryDownloadExecutor::default();
        let mut manager = DownloadManager::new(config, store, executor);

        let task_id = manager
            .create_task(
                "asset-a",
                source("https://example.com/a.m3u8"),
                DownloadProfile::default(),
                asset_index(1024),
                Instant::now(),
            )
            .expect("create task should succeed");

        let snapshot = manager.task(task_id).expect("task should exist");
        assert_eq!(snapshot.status, DownloadTaskStatus::Downloading);
        assert_eq!(snapshot.progress.total_bytes, Some(1024));
        assert_eq!(manager.executor().prepared(), &[task_id]);
        assert_eq!(manager.executor().started(), &[task_id]);

        let events = manager.drain_events();
        assert_eq!(events.len(), 4);
        assert!(matches!(events[0], DownloadEvent::Created(_)));
    }

    #[test]
    fn manager_replaces_task_plan_and_resets_progress_for_recovery() {
        let config = DownloadManagerConfig {
            auto_start: false,
            run_post_processors_on_completion: true,
            ..DownloadManagerConfig::default()
        };
        let store = InMemoryDownloadStore::default();
        let executor = InMemoryDownloadExecutor::default();
        let mut manager = DownloadManager::new(config, store, executor);
        let now = Instant::now();

        let task_id = manager
            .create_task(
                "asset-a",
                source("https://example.com/old.m3u8"),
                DownloadProfile::default(),
                asset_index(1024),
                now,
            )
            .expect("create task should succeed");
        manager
            .update_progress(task_id, 512, 0, now)
            .expect("progress update should succeed");
        manager
            .fail_task(
                task_id,
                PlayerRuntimeError::with_category(
                    PlayerRuntimeErrorCode::BackendFailure,
                    PlayerRuntimeErrorCategory::Network,
                    "stale resource",
                ),
                now,
            )
            .expect("failure should succeed");

        let replaced = manager
            .replace_task_plan(
                task_id,
                source("https://example.com/new.m3u8"),
                DownloadProfile::default(),
                segmented_asset_index(2048),
                now,
            )
            .expect("replace should succeed")
            .expect("task should exist");

        assert_eq!(replaced.status, DownloadTaskStatus::Preparing);
        assert_eq!(replaced.source.source.uri(), "https://example.com/new.m3u8");
        assert_eq!(replaced.progress.received_bytes, 0);
        assert_eq!(replaced.progress.total_bytes, Some(2048));
        assert!(replaced.error_summary.is_none());
        let events = manager.drain_events();
        assert!(events
            .iter()
            .any(|event| matches!(event, DownloadEvent::AssetIndexUpdated(task) if task.task_id == task_id)));
        assert!(events
            .iter()
            .any(|event| matches!(event, DownloadEvent::StateChanged(patch) if patch.task_id == task_id && patch.status == DownloadTaskStatus::Preparing)));
    }

    #[test]
    fn manager_can_pause_resume_and_remove_tasks() {
        let config = DownloadManagerConfig {
            auto_start: true,
            run_post_processors_on_completion: true,
            ..DownloadManagerConfig::default()
        };
        let store = InMemoryDownloadStore::default();
        let executor = InMemoryDownloadExecutor::default();
        let mut manager = DownloadManager::new(config, store, executor);
        let now = Instant::now();

        let task_id = manager
            .create_task(
                "asset-a",
                source("https://example.com/a.m3u8"),
                DownloadProfile::default(),
                asset_index(2048),
                now,
            )
            .expect("create task should succeed");

        let paused = manager
            .pause_task(task_id, now)
            .expect("pause should succeed")
            .expect("task should exist");
        assert_eq!(paused.status, DownloadTaskStatus::Paused);
        assert_eq!(manager.executor().paused(), &[task_id]);

        let resumed = manager
            .resume_task(task_id, now)
            .expect("resume should succeed")
            .expect("task should exist");
        assert_eq!(resumed.status, DownloadTaskStatus::Downloading);
        assert_eq!(manager.executor().resumed(), &[task_id]);

        let removed = manager
            .remove_task(task_id, now)
            .expect("remove should succeed")
            .expect("task should exist");
        assert_eq!(removed.status, DownloadTaskStatus::Removed);
        assert_eq!(manager.executor().removed(), &[task_id]);
    }

    #[test]
    fn manager_restores_persisted_tasks_as_resumable_snapshots() {
        let config = || DownloadManagerConfig {
            auto_start: false,
            run_post_processors_on_completion: true,
            ..DownloadManagerConfig::default()
        };
        let now = Instant::now();
        let mut manager = DownloadManager::new(
            config(),
            InMemoryDownloadStore::default(),
            InMemoryDownloadExecutor::default(),
        );
        let task_id = manager
            .create_task(
                "asset-a",
                source("https://example.com/a.m3u8"),
                DownloadProfile::default(),
                asset_index(2048),
                now,
            )
            .expect("create task should succeed");
        let mut persisted = manager.task(task_id).expect("task should exist");
        persisted.status = DownloadTaskStatus::Downloading;
        persisted.progress.received_bytes = 512;
        persisted.progress.total_bytes = None;

        let mut restored_manager = DownloadManager::new(
            config(),
            InMemoryDownloadStore::default(),
            InMemoryDownloadExecutor::default(),
        );
        let restored = restored_manager
            .restore_tasks(vec![persisted], now)
            .expect("restore should succeed");

        assert_eq!(restored.len(), 1);
        assert_eq!(restored[0].status, DownloadTaskStatus::Paused);
        assert_eq!(restored[0].progress.received_bytes, 512);
        assert_eq!(restored[0].progress.total_bytes, Some(2048));
        let new_task_id = restored_manager
            .create_task(
                "asset-b",
                source("https://example.com/b.m3u8"),
                DownloadProfile::default(),
                asset_index(1024),
                now,
            )
            .expect("create task after restore should succeed");
        assert_eq!(new_task_id.get(), task_id.get() + 1);
    }

    #[test]
    fn manager_updates_progress_tracks_asset_index_and_completes() {
        let config = DownloadManagerConfig {
            auto_start: false,
            run_post_processors_on_completion: true,
            ..DownloadManagerConfig::default()
        };
        let store = InMemoryDownloadStore::default();
        let executor = InMemoryDownloadExecutor::default();
        let mut manager = DownloadManager::new(config, store, executor);
        let now = Instant::now();

        let task_id = manager
            .create_task(
                "asset-a",
                source("https://example.com/a.m3u8"),
                DownloadProfile::default(),
                asset_index(4096),
                now,
            )
            .expect("create task should succeed");

        let queued = manager.task(task_id).expect("task should exist");
        assert_eq!(queued.status, DownloadTaskStatus::Queued);

        let _ = manager
            .start_task(task_id, now)
            .expect("start should succeed");
        let progress = manager
            .update_progress(task_id, 1024, 3, now)
            .expect("progress should succeed")
            .expect("task should exist");
        assert_eq!(progress.progress.received_bytes, 1024);
        assert_eq!(progress.progress.received_segments, 3);

        let completed = manager
            .complete_task(task_id, Some(PathBuf::from("offline/output.mp4")), now)
            .expect("complete should succeed")
            .expect("task should exist");
        assert_eq!(completed.status, DownloadTaskStatus::Completed);
        assert_eq!(
            completed.asset_index.completed_path,
            Some(PathBuf::from("offline/output.mp4"))
        );
        assert_eq!(completed.progress.received_bytes, 4096);

        let tasks = manager.tasks_for_asset(&DownloadAssetId::new("asset-a"));
        assert_eq!(tasks.len(), 1);
        assert_eq!(tasks[0].status, DownloadTaskStatus::Completed);
    }

    #[test]
    fn manager_clamps_misreported_progress_to_known_totals() {
        let config = DownloadManagerConfig {
            auto_start: false,
            run_post_processors_on_completion: true,
            ..DownloadManagerConfig::default()
        };
        let mut manager = DownloadManager::new(
            config,
            InMemoryDownloadStore::default(),
            InMemoryDownloadExecutor::default(),
        );
        let now = Instant::now();

        let task_id = manager
            .create_task(
                "asset-a",
                source("https://example.com/a.m3u8"),
                DownloadProfile::default(),
                segmented_asset_index(2048),
                now,
            )
            .expect("create task should succeed");

        let progress = manager
            .update_progress(task_id, 999_999, 99, now)
            .expect("progress update should succeed")
            .expect("task should exist");

        assert_eq!(progress.progress.received_bytes, 2048);
        assert_eq!(progress.progress.received_segments, 2);
        assert_eq!(progress.progress.completion_ratio(), Some(1.0));
    }

    #[test]
    fn manager_reports_task_id_exhaustion_instead_of_wrapping() {
        let config = DownloadManagerConfig {
            auto_start: false,
            run_post_processors_on_completion: true,
            ..DownloadManagerConfig::default()
        };
        let mut manager = DownloadManager::new(
            config,
            InMemoryDownloadStore::default(),
            InMemoryDownloadExecutor::default(),
        );
        manager.next_task_id = u64::MAX;

        let error = manager
            .create_task(
                "asset-a",
                source("https://example.com/a.m3u8"),
                DownloadProfile::default(),
                asset_index(2048),
                Instant::now(),
            )
            .expect_err("task id exhaustion should be reported");

        assert_eq!(error.code(), PlayerRuntimeErrorCode::InvalidState);
        assert!(error.message().contains("task id space is exhausted"));
    }

    #[test]
    fn manager_snapshot_reuses_asset_index_storage() {
        let config = DownloadManagerConfig {
            auto_start: false,
            run_post_processors_on_completion: true,
            ..DownloadManagerConfig::default()
        };
        let mut manager = DownloadManager::new(
            config,
            InMemoryDownloadStore::default(),
            InMemoryDownloadExecutor::default(),
        );
        let now = Instant::now();

        let task_id = manager
            .create_task(
                "asset-a",
                source("https://example.com/a.m3u8"),
                DownloadProfile::default(),
                multi_stream_hls_asset_index(2048),
                now,
            )
            .expect("create task should succeed");

        let task = manager.task(task_id).expect("task should exist");
        let snapshot = manager.snapshot();

        assert_eq!(snapshot.tasks.len(), 1);
        assert!(Arc::ptr_eq(
            &task.asset_index,
            &snapshot.tasks[0].asset_index
        ));
    }

    #[test]
    fn manager_waits_for_pending_prepare_and_starts_after_completion() {
        let config = DownloadManagerConfig {
            auto_start: true,
            run_post_processors_on_completion: true,
            ..DownloadManagerConfig::default()
        };
        let store = InMemoryDownloadStore::default();
        let executor = PendingPrepareExecutor::default();
        let mut manager = DownloadManager::new(config, store, executor);
        let now = Instant::now();

        let task_id = manager
            .create_task(
                "asset-a",
                source("https://example.com/a.m3u8"),
                DownloadProfile::default(),
                DownloadAssetIndex::default(),
                now,
            )
            .expect("create task should succeed");

        let preparing = manager.task(task_id).expect("task should exist");
        assert_eq!(preparing.status, DownloadTaskStatus::Preparing);
        assert_eq!(preparing.progress.total_bytes, None);
        assert_eq!(manager.executor().prepared, vec![task_id]);
        assert!(manager.executor().started.is_empty());

        let downloading = manager
            .complete_preparation(task_id, segmented_asset_index(2048), now)
            .expect("preparation completion should succeed")
            .expect("task should exist");

        assert_eq!(downloading.status, DownloadTaskStatus::Downloading);
        assert_eq!(downloading.progress.total_bytes, Some(2048));
        assert_eq!(downloading.progress.total_segments, Some(2));
        assert_eq!(manager.executor().started, vec![task_id]);

        let events = manager.drain_events();
        assert!(events.iter().any(|event| {
            matches!(
                event,
                DownloadEvent::AssetIndexUpdated(snapshot)
                    if snapshot.task_id == task_id
                        && snapshot.progress.total_bytes == Some(2048)
            )
        }));
    }

    #[test]
    fn manager_resumes_paused_pending_prepare_by_preparing_again() {
        let config = DownloadManagerConfig {
            auto_start: true,
            run_post_processors_on_completion: true,
            ..DownloadManagerConfig::default()
        };
        let store = InMemoryDownloadStore::default();
        let executor = PendingPrepareExecutor::default();
        let mut manager = DownloadManager::new(config, store, executor);
        let now = Instant::now();

        let task_id = manager
            .create_task(
                "asset-a",
                source("https://example.com/a.m3u8"),
                DownloadProfile::default(),
                DownloadAssetIndex::default(),
                now,
            )
            .expect("create task should succeed");

        let paused = manager
            .pause_task(task_id, now)
            .expect("pause should succeed")
            .expect("task should exist");
        assert_eq!(paused.status, DownloadTaskStatus::Paused);

        let preparing = manager
            .resume_task(task_id, now)
            .expect("resume should succeed")
            .expect("task should exist");

        assert_eq!(preparing.status, DownloadTaskStatus::Preparing);
        assert_eq!(manager.executor().prepared, vec![task_id, task_id]);
        assert!(manager.executor().started.is_empty());

        let downloading = manager
            .complete_preparation(task_id, segmented_asset_index(2048), now)
            .expect("preparation completion should succeed")
            .expect("task should exist");

        assert_eq!(downloading.status, DownloadTaskStatus::Downloading);
        assert_eq!(manager.executor().started, vec![task_id]);
    }

    #[test]
    fn manager_restores_preparing_tasks_as_requiring_prepare_on_resume() {
        let config = || DownloadManagerConfig {
            auto_start: false,
            run_post_processors_on_completion: true,
            ..DownloadManagerConfig::default()
        };
        let now = Instant::now();
        let mut manager = DownloadManager::new(
            config(),
            InMemoryDownloadStore::default(),
            InMemoryDownloadExecutor::default(),
        );

        let task_id = manager
            .create_task(
                "asset-a",
                source("https://example.com/a.m3u8"),
                DownloadProfile::default(),
                segmented_asset_index(2048),
                now,
            )
            .expect("create task should succeed");
        let mut persisted = manager.task(task_id).expect("task should exist");
        persisted.status = DownloadTaskStatus::Preparing;

        let mut restored_manager = DownloadManager::new(
            config(),
            InMemoryDownloadStore::default(),
            InMemoryDownloadExecutor::default(),
        );
        let restored = restored_manager
            .restore_tasks(vec![persisted], now)
            .expect("restore should succeed");
        assert_eq!(restored[0].status, DownloadTaskStatus::Paused);

        let resumed = restored_manager
            .resume_task(task_id, now)
            .expect("resume should succeed")
            .expect("task should exist");

        assert_eq!(resumed.status, DownloadTaskStatus::Downloading);
        assert_eq!(restored_manager.executor().prepared(), &[task_id]);
        assert_eq!(restored_manager.executor().started(), &[task_id]);
        assert!(restored_manager.executor().resumed().is_empty());
    }

    #[test]
    fn manager_can_remove_task_while_preparing() {
        let config = DownloadManagerConfig {
            auto_start: true,
            run_post_processors_on_completion: true,
            ..DownloadManagerConfig::default()
        };
        let store = InMemoryDownloadStore::default();
        let executor = PendingPrepareExecutor::default();
        let mut manager = DownloadManager::new(config, store, executor);
        let now = Instant::now();

        let task_id = manager
            .create_task(
                "asset-a",
                source("https://example.com/a.m3u8"),
                DownloadProfile::default(),
                DownloadAssetIndex::default(),
                now,
            )
            .expect("create task should succeed");

        let removed = manager
            .remove_task(task_id, now)
            .expect("remove should succeed")
            .expect("task should exist");

        assert_eq!(removed.status, DownloadTaskStatus::Removed);
        assert_eq!(manager.executor().removed, vec![task_id]);
        assert!(manager.executor().started.is_empty());
    }

    #[test]
    fn manager_ignores_late_failure_after_pause_or_remove() {
        let config = DownloadManagerConfig {
            auto_start: true,
            run_post_processors_on_completion: true,
            ..DownloadManagerConfig::default()
        };
        let store = InMemoryDownloadStore::default();
        let executor = InMemoryDownloadExecutor::default();
        let mut manager = DownloadManager::new(config, store, executor);
        let now = Instant::now();

        let task_id = manager
            .create_task(
                "asset-a",
                source("https://example.com/a.m3u8"),
                DownloadProfile::default(),
                asset_index(2048),
                now,
            )
            .expect("create task should succeed");
        let _ = manager
            .pause_task(task_id, now)
            .expect("pause should succeed")
            .expect("task should exist");

        let paused = manager
            .fail_task(
                task_id,
                PlayerRuntimeError::with_category(
                    PlayerRuntimeErrorCode::BackendFailure,
                    PlayerRuntimeErrorCategory::Network,
                    "late worker failure",
                ),
                now,
            )
            .expect("late fail should be accepted")
            .expect("task should exist");
        assert_eq!(paused.status, DownloadTaskStatus::Paused);
        assert!(paused.error_summary.is_none());

        let _ = manager
            .remove_task(task_id, now)
            .expect("remove should succeed")
            .expect("task should exist");
        let removed = manager
            .fail_task(
                task_id,
                PlayerRuntimeError::with_category(
                    PlayerRuntimeErrorCode::BackendFailure,
                    PlayerRuntimeErrorCategory::Network,
                    "later worker failure",
                ),
                now,
            )
            .expect("late fail should be accepted")
            .expect("task should exist");
        assert_eq!(removed.status, DownloadTaskStatus::Removed);
        assert!(removed.error_summary.is_none());
    }

    #[test]
    fn manager_dispatches_pipeline_hook_events_for_state_changes() {
        let hook = Arc::new(RecordingHook::default());
        let config = DownloadManagerConfig {
            auto_start: true,
            run_post_processors_on_completion: true,
            event_hooks: vec![hook.clone()],
            ..DownloadManagerConfig::default()
        };
        let store = InMemoryDownloadStore::default();
        let executor = InMemoryDownloadExecutor::default();
        let mut manager = DownloadManager::new(config, store, executor);
        let now = Instant::now();

        let task_id = manager
            .create_task(
                "asset-a",
                source("https://example.com/a.m3u8"),
                DownloadProfile::default(),
                asset_index(512),
                now,
            )
            .expect("create task should succeed");

        let _ = manager
            .fail_task(
                task_id,
                PlayerRuntimeError::new(PlayerRuntimeErrorCode::BackendFailure, "network failed"),
                now,
            )
            .expect("fail should succeed");

        let events = hook.events();
        assert!(events.iter().any(|event| matches!(
            event,
            PipelineEvent::DownloadTaskCreated { asset_id, .. } if asset_id == "asset-a"
        )));
        assert!(
            !events
                .iter()
                .any(|event| matches!(event, PipelineEvent::DownloadTaskCompleted { .. }))
        );
        assert!(events.iter().any(|event| matches!(
            event,
            PipelineEvent::DownloadTaskFailed { error, .. } if error == "network failed"
        )));
    }

    #[test]
    fn manager_runs_post_processor_and_updates_completed_path() {
        let hook = Arc::new(RecordingHook::default());
        let processor = Arc::new(RecordingProcessor::default());
        let config = DownloadManagerConfig {
            auto_start: false,
            run_post_processors_on_completion: true,
            post_processors: vec![processor.clone()],
            event_hooks: vec![hook.clone()],
        };
        let store = InMemoryDownloadStore::default();
        let executor = InMemoryDownloadExecutor::default();
        let mut manager = DownloadManager::new(config, store, executor);
        let now = Instant::now();

        let task_id = manager
            .create_task(
                "asset-a",
                source("https://example.com/a.m3u8"),
                DownloadProfile {
                    target_directory: Some(PathBuf::from("/tmp/offline")),
                    ..DownloadProfile::default()
                },
                segmented_asset_index(1024),
                now,
            )
            .expect("create task should succeed");

        let completed = manager
            .complete_task(
                task_id,
                Some(PathBuf::from("/tmp/offline/playlist.m3u8")),
                now,
            )
            .expect("complete should succeed")
            .expect("task should exist");

        assert_eq!(completed.status, DownloadTaskStatus::Completed);
        assert_eq!(
            completed.asset_index.completed_path,
            Some(PathBuf::from("/tmp/offline/playlist.mp4"))
        );

        let invocations = processor.invocations();
        assert_eq!(invocations.len(), 1);
        assert!(matches!(
            &invocations[0].0.content_format,
            player_plugin::CompletedContentFormat::HlsSegments {
                manifest_path,
                segment_paths,
            } if manifest_path == &PathBuf::from("/tmp/offline/playlist.m3u8")
                && segment_paths == &vec![
                    PathBuf::from("/tmp/offline/seg-1.ts"),
                    PathBuf::from("/tmp/offline/seg-2.ts"),
                ]
        ));
        assert_eq!(invocations[0].1, PathBuf::from("/tmp/offline/playlist.mp4"));

        let events = hook.events();
        assert!(events.iter().any(|event| matches!(
            event,
            PipelineEvent::PostProcessStarted { processor, .. } if processor == "recording-processor"
        )));
        assert!(events.iter().any(|event| matches!(
            event,
            PipelineEvent::PostProcessCompleted { output_path, .. }
                if output_path == "/tmp/offline/playlist.mp4"
        )));
        assert!(events.iter().any(|event| matches!(
            event,
            PipelineEvent::DownloadTaskCompleted { task_id: completed_task_id }
                if completed_task_id == "1"
        )));
    }

    #[test]
    fn manager_marks_task_failed_when_post_processor_fails() {
        let hook = Arc::new(RecordingHook::default());
        let processor = Arc::new(FailingProcessor);
        let config = DownloadManagerConfig {
            auto_start: false,
            run_post_processors_on_completion: true,
            post_processors: vec![processor],
            event_hooks: vec![hook.clone()],
        };
        let store = InMemoryDownloadStore::default();
        let executor = InMemoryDownloadExecutor::default();
        let mut manager = DownloadManager::new(config, store, executor);
        let now = Instant::now();

        let task_id = manager
            .create_task(
                "asset-a",
                source("https://example.com/a.m3u8"),
                DownloadProfile {
                    target_directory: Some(PathBuf::from("/tmp/offline")),
                    ..DownloadProfile::default()
                },
                segmented_asset_index(1024),
                now,
            )
            .expect("create task should succeed");

        let failed = manager
            .complete_task(
                task_id,
                Some(PathBuf::from("/tmp/offline/playlist.m3u8")),
                now,
            )
            .expect("complete should return state")
            .expect("task should exist");

        assert_eq!(failed.status, DownloadTaskStatus::Failed);
        assert!(
            failed
                .error_summary
                .as_ref()
                .is_some_and(|summary| summary.message.contains("failing-processor"))
        );

        let events = hook.events();
        assert!(events.iter().any(|event| matches!(
            event,
            PipelineEvent::PostProcessFailed { error, .. }
                if error == "mux failed: ffmpeg remux failed"
        )));
        assert!(events.iter().any(|event| matches!(
            event,
            PipelineEvent::DownloadTaskFailed { error, .. }
                if error.contains("failing-processor")
        )));
    }

    #[test]
    fn manager_assembles_multi_stream_downloads_with_capable_processor() {
        let processor = Arc::new(RecordingProcessor::default());
        let config = DownloadManagerConfig {
            auto_start: false,
            run_post_processors_on_completion: true,
            post_processors: vec![processor.clone()],
            ..DownloadManagerConfig::default()
        };
        let store = InMemoryDownloadStore::default();
        let executor = InMemoryDownloadExecutor::default();
        let mut manager = DownloadManager::new(config, store, executor);
        let now = Instant::now();

        let task_id = manager
            .create_task(
                "asset-a",
                source("https://example.com/master.m3u8"),
                DownloadProfile {
                    target_directory: Some(PathBuf::from("/tmp/offline")),
                    ..DownloadProfile::default()
                },
                multi_stream_hls_asset_index(1024),
                now,
            )
            .expect("create task should succeed");

        let completed = manager
            .complete_task(
                task_id,
                Some(PathBuf::from("/tmp/offline/master.m3u8")),
                now,
            )
            .expect("complete should succeed")
            .expect("task should exist");

        assert_eq!(completed.status, DownloadTaskStatus::Completed);
        assert_eq!(
            completed.asset_index.completed_path,
            Some(PathBuf::from("/tmp/offline/master.mp4"))
        );

        let invocations = processor.invocations();
        assert_eq!(invocations.len(), 1);
        assert_eq!(
            invocations[0].0.assembly_mode,
            AssemblyMode::SeparateAudioVideo
        );
        assert_eq!(invocations[0].0.streams.len(), 2);
    }

    #[test]
    fn manager_fails_multi_stream_completion_without_assembly_processor() {
        let config = DownloadManagerConfig {
            auto_start: false,
            run_post_processors_on_completion: true,
            ..DownloadManagerConfig::default()
        };
        let store = InMemoryDownloadStore::default();
        let executor = InMemoryDownloadExecutor::default();
        let mut manager = DownloadManager::new(config, store, executor);
        let now = Instant::now();

        let task_id = manager
            .create_task(
                "asset-a",
                source("https://example.com/master.m3u8"),
                DownloadProfile {
                    target_directory: Some(PathBuf::from("/tmp/offline")),
                    ..DownloadProfile::default()
                },
                multi_stream_hls_asset_index(1024),
                now,
            )
            .expect("create task should succeed");

        let failed = manager
            .complete_task(
                task_id,
                Some(PathBuf::from("/tmp/offline/master.m3u8")),
                now,
            )
            .expect("complete should return failed state")
            .expect("task should exist");

        assert_eq!(failed.status, DownloadTaskStatus::Failed);
        let message = failed
            .error_summary
            .as_ref()
            .map(|error| error.message.as_str())
            .unwrap_or_default();
        assert!(message.contains("requires post-download assembly"));
        assert!(message.contains("no processor supports this assembly mode"));
    }

    #[test]
    fn manager_fails_multi_stream_completion_when_assembly_processor_skips() {
        let config = DownloadManagerConfig {
            auto_start: false,
            run_post_processors_on_completion: true,
            post_processors: vec![Arc::new(SkippingAssemblyProcessor)],
            ..DownloadManagerConfig::default()
        };
        let store = InMemoryDownloadStore::default();
        let executor = InMemoryDownloadExecutor::default();
        let mut manager = DownloadManager::new(config, store, executor);
        let now = Instant::now();

        let task_id = manager
            .create_task(
                "asset-a",
                source("https://example.com/master.m3u8"),
                DownloadProfile {
                    target_directory: Some(PathBuf::from("/tmp/offline")),
                    ..DownloadProfile::default()
                },
                multi_stream_hls_asset_index(1024),
                now,
            )
            .expect("create task should succeed");

        let failed = manager
            .complete_task(
                task_id,
                Some(PathBuf::from("/tmp/offline/master.m3u8")),
                now,
            )
            .expect("complete should return failed state")
            .expect("task should exist");

        assert_eq!(failed.status, DownloadTaskStatus::Failed);
        let message = failed
            .error_summary
            .as_ref()
            .map(|error| error.message.as_str())
            .unwrap_or_default();
        assert!(message.contains("requires post-download assembly"));
        assert!(message.contains("no processor produced an assembled output"));
    }

    #[test]
    fn manager_exports_completed_segment_download_with_progress() {
        let hook = Arc::new(RecordingHook::default());
        let processor = Arc::new(RecordingProcessor::default());
        let config = DownloadManagerConfig {
            auto_start: false,
            run_post_processors_on_completion: false,
            post_processors: vec![processor.clone()],
            event_hooks: vec![hook.clone()],
        };
        let store = InMemoryDownloadStore::default();
        let executor = InMemoryDownloadExecutor::default();
        let mut manager = DownloadManager::new(config, store, executor);
        let now = Instant::now();

        let task_id = manager
            .create_task(
                "asset-a",
                source("https://example.com/a.m3u8"),
                DownloadProfile {
                    target_directory: Some(PathBuf::from("/tmp/offline")),
                    ..DownloadProfile::default()
                },
                segmented_asset_index(1024),
                now,
            )
            .expect("create task should succeed");

        let _ = manager
            .complete_task(
                task_id,
                Some(PathBuf::from("/tmp/offline/playlist.m3u8")),
                now,
            )
            .expect("complete should succeed");

        let progress = RecordingProgress::default();
        let exported = manager
            .export_task_output(
                task_id,
                Some(PathBuf::from("/tmp/gallery/exported.mp4").as_path()),
                &progress,
            )
            .expect("export should succeed");

        assert_eq!(exported, PathBuf::from("/tmp/gallery/exported.mp4"));
        assert_eq!(progress.ratios(), vec![1.0]);

        let invocations = processor.invocations();
        assert_eq!(invocations.len(), 1);
        assert!(matches!(
            &invocations[0].0.content_format,
            player_plugin::CompletedContentFormat::HlsSegments { manifest_path, .. }
                if manifest_path == &PathBuf::from("/tmp/offline/playlist.m3u8")
        ));
        assert_eq!(invocations[0].1, PathBuf::from("/tmp/gallery/exported.mp4"));

        let events = hook.events();
        assert!(events.iter().any(|event| matches!(
            event,
            PipelineEvent::PostProcessCompleted { output_path, .. }
                if output_path == "/tmp/gallery/exported.mp4"
        )));
    }

    #[test]
    fn manager_exports_completed_single_file_download_without_processor() {
        let config = DownloadManagerConfig {
            auto_start: false,
            run_post_processors_on_completion: true,
            ..DownloadManagerConfig::default()
        };
        let store = InMemoryDownloadStore::default();
        let executor = InMemoryDownloadExecutor::default();
        let mut manager = DownloadManager::new(config, store, executor);
        let now = Instant::now();
        let temp_dir = unique_test_dir("vesper-single-file-export");
        fs::create_dir_all(&temp_dir).expect("temp directory should be created");
        let source_path = temp_dir.join("input.mp4");
        let output_path = temp_dir.join("exported.mp4");
        fs::write(&source_path, b"vesper media bytes").expect("source file should be written");

        let task_id = manager
            .create_task(
                "asset-a",
                DownloadSource::new(
                    MediaSource::new(format!("file://{}", source_path.display())),
                    DownloadContentFormat::SingleFile,
                ),
                DownloadProfile::default(),
                DownloadAssetIndex::default(),
                now,
            )
            .expect("create task should succeed");

        let _ = manager
            .complete_task(task_id, Some(source_path.clone()), now)
            .expect("complete should succeed");

        let exported = manager
            .export_task_output(task_id, Some(output_path.as_path()), &NoopProcessorProgress)
            .expect("single-file export should copy original file");

        assert_eq!(exported, output_path);
        assert_eq!(
            fs::read(&source_path).expect("source file should remain readable"),
            b"vesper media bytes"
        );
        assert_eq!(
            fs::read(&exported).expect("export file should be readable"),
            b"vesper media bytes"
        );

        fs::remove_dir_all(temp_dir).expect("temp directory should be removed");
    }

    fn unique_test_dir(prefix: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or(0);
        std::env::temp_dir().join(format!("{prefix}-{nanos}"))
    }
}
