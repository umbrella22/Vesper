use super::post_processing::NoopProcessorProgress;
use super::{
    DownloadAssetId, DownloadAssetIndex, DownloadContentFormat, DownloadEvent, DownloadManager,
    DownloadManagerConfig, DownloadPrepareResult, DownloadProfile, DownloadResourceRecord,
    DownloadSegmentRecord, DownloadSource, DownloadTaskId, DownloadTaskStatus,
    InMemoryDownloadExecutor, InMemoryDownloadStore,
};
use crate::{
    DownloadAssetStream, DownloadStreamKind, PlayerError, PlayerErrorCategory, PlayerErrorCode,
    PlayerResult,
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
    ) -> PlayerResult<DownloadPrepareResult> {
        self.prepared.push(task.task_id);
        Ok(DownloadPrepareResult::Pending)
    }

    fn start(&mut self, task: &super::DownloadTaskSnapshot) -> PlayerResult<()> {
        self.started.push(task.task_id);
        Ok(())
    }

    fn pause(&mut self, task_id: DownloadTaskId) -> PlayerResult<()> {
        self.paused.push(task_id);
        Ok(())
    }

    fn resume(&mut self, task: &super::DownloadTaskSnapshot) -> PlayerResult<()> {
        self.started.push(task.task_id);
        Ok(())
    }

    fn remove(&mut self, task_id: DownloadTaskId) -> PlayerResult<()> {
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
            PlayerError::with_category(
                PlayerErrorCode::BackendFailure,
                PlayerErrorCategory::Network,
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
    assert!(events.iter().any(
        |event| matches!(event, DownloadEvent::AssetIndexUpdated(task) if task.task_id == task_id)
    ));
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

    assert_eq!(error.code(), PlayerErrorCode::InvalidState);
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
            PlayerError::with_category(
                PlayerErrorCode::BackendFailure,
                PlayerErrorCategory::Network,
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
            PlayerError::with_category(
                PlayerErrorCode::BackendFailure,
                PlayerErrorCategory::Network,
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
            PlayerError::new(PlayerErrorCode::BackendFailure, "network failed"),
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
