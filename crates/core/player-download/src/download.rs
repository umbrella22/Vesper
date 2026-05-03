use std::collections::HashMap;
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use player_core::MediaSource;
use player_plugin::{
    CompletedContentFormat, CompletedDownloadInfo, DownloadMetadata, OutputFormat, PipelineEvent,
    PipelineEventHook, PostDownloadProcessor, ProcessorError, ProcessorOutput, ProcessorProgress,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DownloadContentFormat {
    HlsSegments,
    DashSegments,
    SingleFile,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DownloadSource {
    pub source: MediaSource,
    pub content_format: DownloadContentFormat,
    pub manifest_uri: Option<String>,
}

impl DownloadSource {
    pub fn new(source: MediaSource, content_format: DownloadContentFormat) -> Self {
        Self {
            source,
            content_format,
            manifest_uri: None,
        }
    }

    pub fn with_manifest_uri(mut self, manifest_uri: impl Into<String>) -> Self {
        self.manifest_uri = Some(manifest_uri.into().trim().to_owned());
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DownloadProfile {
    pub variant_id: Option<String>,
    pub preferred_audio_language: Option<String>,
    pub preferred_subtitle_language: Option<String>,
    pub selected_track_ids: Vec<String>,
    pub target_directory: Option<PathBuf>,
    pub allow_metered_network: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DownloadResourceRecord {
    pub resource_id: String,
    pub uri: String,
    pub relative_path: Option<PathBuf>,
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
    pub size_bytes: Option<u64>,
    pub checksum: Option<String>,
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
            completed_path: None,
        }
    }
}

impl DownloadAssetIndex {
    pub fn inferred_total_size_bytes(&self) -> Option<u64> {
        self.total_size_bytes.or_else(|| {
            let resource_sum = self.resources.iter().try_fold(0_u64, |sum, resource| {
                resource.size_bytes.map(|size| sum + size)
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
            .map(|total_bytes| self.received_bytes as f32 / total_bytes as f32)
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DownloadTaskSnapshot {
    pub task_id: DownloadTaskId,
    pub asset_id: DownloadAssetId,
    pub source: DownloadSource,
    pub profile: DownloadProfile,
    pub status: DownloadTaskStatus,
    pub progress: DownloadProgressSnapshot,
    pub asset_index: DownloadAssetIndex,
    pub created_at: Instant,
    pub updated_at: Instant,
    pub error_summary: Option<DownloadErrorSummary>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DownloadSnapshot {
    pub tasks: Vec<DownloadTaskSnapshot>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DownloadEvent {
    Created(DownloadTaskSnapshot),
    StateChanged(DownloadTaskSnapshot),
    ProgressUpdated(DownloadTaskSnapshot),
}

#[derive(Default)]
// 若未来新增必填字段，需同步更新 player-platform-{android,ios}/src/download.rs::new()
// 的默认构造，避免空配置路径与 new_with_plugin_library_paths 路径再次漂移。
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
    fn prepare(&mut self, task: &DownloadTaskSnapshot) -> PlayerRuntimeResult<()>;

    fn start(&mut self, task: &DownloadTaskSnapshot) -> PlayerRuntimeResult<()>;

    fn pause(&mut self, task_id: DownloadTaskId) -> PlayerRuntimeResult<()>;

    fn resume(&mut self, task: &DownloadTaskSnapshot) -> PlayerRuntimeResult<()>;

    fn remove(&mut self, task_id: DownloadTaskId) -> PlayerRuntimeResult<()>;
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
    fn prepare(&mut self, task: &DownloadTaskSnapshot) -> PlayerRuntimeResult<()> {
        self.prepared.push(task.task_id);
        Ok(())
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
        self.next_task_id += 1;

        asset_index.content_format = source.content_format;

        let snapshot = DownloadTaskSnapshot {
            task_id,
            asset_id: DownloadAssetId::new(asset_id),
            source,
            profile,
            status: DownloadTaskStatus::Queued,
            progress: DownloadProgressSnapshot::from_index(&asset_index),
            asset_index,
            created_at: now,
            updated_at: now,
            error_summary: None,
        };

        self.store.save_task(snapshot.clone())?;
        self.emit_event(DownloadEvent::Created(snapshot.clone()));
        self.emit_event(DownloadEvent::StateChanged(snapshot));

        if self.config.auto_start {
            let _ = self.start_task(task_id, now)?;
        }

        Ok(task_id)
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

        let Some(preparing) = preparing else {
            return Ok(None);
        };

        if let Err(error) = self.executor.prepare(&preparing) {
            return self.fail_task(task_id, error, now);
        }

        let downloading = self.update_task(task_id, now, |task| {
            task.status = DownloadTaskStatus::Downloading;
            task.error_summary = None;
        })?;

        let Some(downloading) = downloading else {
            return Ok(None);
        };

        if let Err(error) = self.executor.start(&downloading) {
            return self.fail_task(task_id, error, now);
        }

        Ok(Some(downloading))
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
        snapshot.updated_at = now;
        self.store.save_task(snapshot.clone())?;
        self.emit_event(DownloadEvent::ProgressUpdated(snapshot.clone()));
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
        finalized.asset_index.completed_path = completed_path
            .clone()
            .or_else(|| finalized.asset_index.completed_path.clone());
        if let Some(total_bytes) = finalized.progress.total_bytes {
            finalized.progress.received_bytes = total_bytes;
        }
        if let Some(total_segments) = finalized.progress.total_segments {
            finalized.progress.received_segments = total_segments;
        }

        let processed_output_path = if self.config.run_post_processors_on_completion {
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
            task.asset_index.completed_path = processed_output_path
                .clone()
                .or_else(|| task.asset_index.completed_path.clone());
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
            DownloadContentFormat::SingleFile => resolve_single_file_path(&snapshot),
            DownloadContentFormat::HlsSegments | DownloadContentFormat::DashSegments => {
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
        let error_summary = DownloadErrorSummary::from(error);
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
        self.emit_event(DownloadEvent::StateChanged(snapshot.clone()));
        Ok(Some(snapshot))
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
            DownloadEvent::StateChanged(snapshot) => {
                self.dispatch_pipeline_event(PipelineEvent::DownloadTaskStateChanged {
                    task_id: snapshot.task_id.get().to_string(),
                    new_state: snapshot.status.as_str().to_owned(),
                });

                if snapshot.status == DownloadTaskStatus::Completed {
                    self.dispatch_pipeline_event(PipelineEvent::DownloadTaskCompleted {
                        task_id: snapshot.task_id.get().to_string(),
                    });
                }

                if snapshot.status == DownloadTaskStatus::Failed {
                    self.dispatch_pipeline_event(PipelineEvent::DownloadTaskFailed {
                        task_id: snapshot.task_id.get().to_string(),
                        error: snapshot
                            .error_summary
                            .as_ref()
                            .map(|summary| summary.message.clone())
                            .unwrap_or_else(|| "download failed".to_owned()),
                    });
                }
            }
            DownloadEvent::ProgressUpdated(_) => {}
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
            return Ok(snapshot.asset_index.completed_path.clone());
        }

        let mut current_input = self.completed_download_info(snapshot)?;
        let mut current_completed_path = snapshot.asset_index.completed_path.clone();
        let progress = NoopProcessorProgress;

        for processor in &self.config.post_processors {
            let input_kind = current_input.content_format.kind();
            if !processor.supported_input_formats().contains(&input_kind) {
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

            match processor.process(&current_input, &output_path, &progress) {
                Ok(ProcessorOutput::MuxedFile { path, .. }) => {
                    self.dispatch_pipeline_event(PipelineEvent::PostProcessCompleted {
                        task_id: snapshot.task_id.get().to_string(),
                        output_path: path.display().to_string(),
                    });
                    current_completed_path = Some(path.clone());
                    current_input = CompletedDownloadInfo {
                        asset_id: snapshot.asset_id.as_str().to_owned(),
                        task_id: Some(snapshot.task_id.get().to_string()),
                        content_format: CompletedContentFormat::SingleFile { path },
                        metadata: current_input.metadata.clone(),
                    };
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

        for processor in &self.config.post_processors {
            let input_kind = current_input.content_format.kind();
            if !processor.supported_input_formats().contains(&input_kind) {
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

            match processor.process(&current_input, &resolved_output_path, progress) {
                Ok(ProcessorOutput::MuxedFile { path, .. }) => {
                    self.dispatch_pipeline_event(PipelineEvent::PostProcessCompleted {
                        task_id: snapshot.task_id.get().to_string(),
                        output_path: path.display().to_string(),
                    });
                    current_completed_path = Some(path.clone());
                    current_input = CompletedDownloadInfo {
                        asset_id: snapshot.asset_id.as_str().to_owned(),
                        task_id: Some(snapshot.task_id.get().to_string()),
                        content_format: CompletedContentFormat::SingleFile { path },
                        metadata: current_input.metadata.clone(),
                    };
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
        })
    }
}

struct NoopProcessorProgress;

impl ProcessorProgress for NoopProcessorProgress {
    fn on_progress(&self, _ratio: f32) {}
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
        && matches!(
            path.extension().and_then(|extension| extension.to_str()),
            Some("m3u8" | "mpd")
        )
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
        .filter(|path| {
            matches!(
                path.extension().and_then(|extension| extension.to_str()),
                Some("m3u8" | "mpd")
            )
        })
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
        DownloadManagerConfig, DownloadProfile, DownloadResourceRecord, DownloadSegmentRecord,
        DownloadSource, DownloadTaskStatus, InMemoryDownloadExecutor, InMemoryDownloadStore,
    };
    use crate::download::NoopProcessorProgress;
    use crate::{PlayerRuntimeError, PlayerRuntimeErrorCode};
    use player_core::MediaSource;
    use player_plugin::{
        CompletedDownloadInfo, ContentFormatKind, OutputFormat, PipelineEvent, PipelineEventHook,
        PostDownloadProcessor, ProcessorCapabilities, ProcessorError, ProcessorOutput,
        ProcessorProgress,
    };
    use std::path::PathBuf;
    use std::sync::{Arc, Mutex};
    use std::time::Instant;

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
    }

    #[derive(Debug, Default)]
    struct FailingProcessor;

    #[derive(Debug, Default)]
    struct RecordingProgress {
        ratios: Mutex<Vec<f32>>,
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
                    size_bytes: Some(512),
                    checksum: None,
                },
                DownloadSegmentRecord {
                    segment_id: "seg-2".to_owned(),
                    uri: "seg-2.ts".to_owned(),
                    relative_path: Some(PathBuf::from("seg-2.ts")),
                    sequence: Some(2),
                    size_bytes: Some(512),
                    checksum: None,
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

        let task_id = manager
            .create_task(
                "asset-a",
                DownloadSource::new(
                    MediaSource::new("file:///tmp/input.mp4"),
                    DownloadContentFormat::SingleFile,
                ),
                DownloadProfile::default(),
                DownloadAssetIndex::default(),
                now,
            )
            .expect("create task should succeed");

        let _ = manager
            .complete_task(task_id, Some(PathBuf::from("/tmp/input.mp4")), now)
            .expect("complete should succeed");

        let exported = manager
            .export_task_output(
                task_id,
                Some(PathBuf::from("/tmp/ignored.mp4").as_path()),
                &NoopProcessorProgress,
            )
            .expect("single-file export should reuse original path");

        assert_eq!(exported, PathBuf::from("/tmp/input.mp4"));
    }
}
