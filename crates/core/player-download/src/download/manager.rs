use std::collections::HashSet;
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use player_plugin::{PipelineEvent, PipelineEventHook, PostDownloadProcessor, ProcessorProgress};

use crate::{PlayerError, PlayerErrorCategory, PlayerErrorCode, PlayerResult};

use super::executor::{DownloadExecutor, DownloadPrepareResult};
use super::post_processing::should_run_post_processors_on_completion;
use super::store::DownloadStore;
use super::types::{
    DownloadAssetId, DownloadAssetIndex, DownloadContentFormat, DownloadErrorSummary,
    DownloadEvent, DownloadProfile, DownloadProgressSnapshot, DownloadSnapshot, DownloadSource,
    DownloadTaskId, DownloadTaskSnapshot, DownloadTaskStatus, next_non_zero_task_id,
};

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

#[derive(Debug)]
pub struct DownloadManager<S, E> {
    pub(super) config: DownloadManagerConfig,
    store: S,
    executor: E,
    pub(super) next_task_id: u64,
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
    ) -> PlayerResult<DownloadTaskId> {
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
    ) -> PlayerResult<Vec<DownloadTaskSnapshot>> {
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
    ) -> PlayerResult<Option<DownloadTaskSnapshot>> {
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
    ) -> PlayerResult<Option<DownloadTaskSnapshot>> {
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
    ) -> PlayerResult<Option<DownloadTaskSnapshot>> {
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
    ) -> PlayerResult<Option<DownloadTaskSnapshot>> {
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
    ) -> PlayerResult<Option<DownloadTaskSnapshot>> {
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
    ) -> PlayerResult<Option<DownloadTaskSnapshot>> {
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
    ) -> PlayerResult<Option<DownloadTaskSnapshot>> {
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
    ) -> PlayerResult<PathBuf> {
        let Some(snapshot) = self.store.task(task_id) else {
            return Err(PlayerError::with_category(
                PlayerErrorCode::InvalidArgument,
                PlayerErrorCategory::Input,
                format!("download task {} was not found for export", task_id.get()),
            ));
        };

        if snapshot.status != DownloadTaskStatus::Completed {
            return Err(PlayerError::with_category(
                PlayerErrorCode::InvalidState,
                PlayerErrorCategory::Playback,
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
            DownloadContentFormat::Unknown => Err(PlayerError::with_category(
                PlayerErrorCode::Unsupported,
                PlayerErrorCategory::Capability,
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
        error: PlayerError,
        now: Instant,
    ) -> PlayerResult<Option<DownloadTaskSnapshot>> {
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
    ) -> PlayerResult<Option<DownloadTaskSnapshot>> {
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
    ) -> PlayerResult<Option<DownloadTaskSnapshot>> {
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
    ) -> PlayerResult<Option<DownloadTaskSnapshot>> {
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
    ) -> PlayerResult<Option<DownloadTaskSnapshot>> {
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
    ) -> PlayerResult<Option<DownloadTaskSnapshot>> {
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

    pub(super) fn dispatch_pipeline_event(&self, event: PipelineEvent) {
        for hook in &self.config.event_hooks {
            hook.on_event(&event);
        }
    }
}

fn task_needs_preparation(snapshot: &DownloadTaskSnapshot) -> bool {
    snapshot.asset_index.resources.is_empty()
        && snapshot.asset_index.segments.is_empty()
        && snapshot.asset_index.total_size_bytes.is_none()
        && snapshot.asset_index.completed_path.is_none()
}
