//! Desktop SDK-managed offline download support shared by macOS, Windows, and Linux hosts.

use std::collections::HashMap;
use std::ffi::OsStr;
use std::fs;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, anyhow, bail};
use player_download::{DownloadPlanner, DownloadPlanningClient};
use player_model::{MediaSource, MediaSourceProtocol};
use player_plugin::{
    AssemblyMode, CompletedContentFormat, CompletedDownloadInfo, CompletedStream, DownloadMetadata,
    OutputFormat, PostDownloadProcessor, ProcessorOutput, ProcessorProgress, StreamKind,
};
use player_runtime::{
    DownloadAssetIndex, DownloadByteRange, DownloadContentFormat, DownloadManager,
    DownloadManagerConfig, DownloadPrepareResult, DownloadProfile, DownloadProgressSnapshot,
    DownloadSource, DownloadTaskId, DownloadTaskSnapshot, DownloadTaskStatus,
    DownloadTaskStatus::Completed, InMemoryDownloadStore, PlayerError, PlayerErrorCategory,
    PlayerErrorCode,
};
use vesper_remux_ffmpeg::FfmpegRemuxProcessor;

const CURL_BIN: &str = "curl";
const DOWNLOAD_ROOT_DIR: &str = "vesper-desktop-downloads";
const CURL_POLL_INTERVAL: Duration = Duration::from_millis(100);

#[derive(Debug, Clone, Copy, Default)]
pub struct DesktopDownloadPlanningClient;

impl DownloadPlanningClient for DesktopDownloadPlanningClient {
    fn fetch_text(&self, uri: &str) -> player_runtime::PlayerResult<String> {
        fetch_remote_text(uri).map_err(|error| source_error(error.to_string()))
    }

    fn content_length(&self, uri: &str) -> player_runtime::PlayerResult<Option<u64>> {
        if let Some(local_path) = local_path_from_uri(uri) {
            return fs::metadata(&local_path)
                .map(|metadata| Some(metadata.len()))
                .map_err(|error| {
                    source_error(format!(
                        "failed to inspect local resource `{}`: {error}",
                        local_path.display()
                    ))
                });
        }
        probe_remote_content_length(uri)
    }
}
#[derive(Debug, Clone)]
pub struct PreparedDownloadTask {
    pub source: DownloadSource,
    pub profile: DownloadProfile,
    pub asset_index: DownloadAssetIndex,
    pub resolved_label: String,
}

#[derive(Debug, Clone)]
pub struct PendingDownloadTask {
    pub asset_id: String,
    pub label: String,
    pub source_uri: String,
}

#[derive(Debug, Default)]
pub struct DesktopDownloadPollResult {
    pub messages: Vec<String>,
    pub changed: bool,
}

#[derive(Debug, Clone)]
struct DownloadWorkItem {
    uri: String,
    target_path: PathBuf,
    byte_range: Option<DownloadByteRange>,
    generated_text: Option<String>,
    expected_size_bytes: Option<u64>,
    counts_as_segment: bool,
}

#[derive(Debug)]
enum WorkerEvent {
    Progress {
        task_id: DownloadTaskId,
        received_bytes: u64,
        received_segments: u32,
    },
    Completed {
        task_id: DownloadTaskId,
        completed_path: Option<PathBuf>,
    },
    Failed {
        task_id: DownloadTaskId,
        error: PlayerError,
    },
}

#[derive(Debug)]
enum ExportEvent {
    Progress {
        task_id: DownloadTaskId,
        ratio: f32,
    },
    Completed {
        task_id: DownloadTaskId,
        output_path: PathBuf,
    },
    Failed {
        task_id: DownloadTaskId,
        error: String,
    },
}

#[derive(Debug, Default, Clone)]
pub struct ExportState {
    pub in_progress: bool,
    pub ratio: Option<f32>,
}

#[derive(Debug)]
struct DesktopDownloadExecutor {
    worker_tx: Sender<WorkerEvent>,
    cancellations: HashMap<DownloadTaskId, Arc<AtomicBool>>,
}

impl DesktopDownloadExecutor {
    fn new(worker_tx: Sender<WorkerEvent>) -> Self {
        Self {
            worker_tx,
            cancellations: HashMap::new(),
        }
    }

    fn spawn_worker(&mut self, task: DownloadTaskSnapshot) -> Result<(), PlayerError> {
        let cancel_flag = Arc::new(AtomicBool::new(false));
        self.cancellations.insert(task.task_id, cancel_flag.clone());
        let worker_tx = self.worker_tx.clone();
        thread::Builder::new()
            .name(format!("desktop-download-{}", task.task_id.get()))
            .spawn(move || run_download_task(task, cancel_flag, worker_tx))
            .map_err(|error| platform_error(format!("failed to spawn download worker: {error}")))?;
        Ok(())
    }

    fn cancel(&mut self, task_id: DownloadTaskId) {
        if let Some(cancel_flag) = self.cancellations.remove(&task_id) {
            cancel_flag.store(true, Ordering::SeqCst);
        }
    }
}

impl player_runtime::DownloadExecutor for DesktopDownloadExecutor {
    fn prepare(
        &mut self,
        task: &DownloadTaskSnapshot,
    ) -> player_runtime::PlayerResult<DownloadPrepareResult> {
        if let Some(target_directory) = task.profile.target_directory.as_ref() {
            fs::create_dir_all(target_directory).map_err(|error| {
                source_error(format!(
                    "failed to create download directory `{}`: {error}",
                    target_directory.display()
                ))
            })?;
        }
        Ok(DownloadPrepareResult::Ready(None))
    }

    fn start(&mut self, task: &DownloadTaskSnapshot) -> player_runtime::PlayerResult<()> {
        self.spawn_worker(task.clone())
    }

    fn pause(&mut self, task_id: DownloadTaskId) -> player_runtime::PlayerResult<()> {
        self.cancel(task_id);
        Ok(())
    }

    fn resume(&mut self, task: &DownloadTaskSnapshot) -> player_runtime::PlayerResult<()> {
        self.spawn_worker(task.clone())
    }

    fn remove(&mut self, task_id: DownloadTaskId) -> player_runtime::PlayerResult<()> {
        self.cancel(task_id);
        Ok(())
    }
}

pub struct DesktopDownloadController {
    manager: DownloadManager<InMemoryDownloadStore, DesktopDownloadExecutor>,
    worker_rx: Receiver<WorkerEvent>,
    export_tx: Sender<ExportEvent>,
    export_rx: Receiver<ExportEvent>,
    processor: Arc<dyn PostDownloadProcessor>,
    asset_labels: HashMap<String, String>,
    export_state: HashMap<DownloadTaskId, ExportState>,
    exported_paths: HashMap<DownloadTaskId, PathBuf>,
}

impl DesktopDownloadController {
    pub fn new() -> Self {
        Self::with_post_processor(Arc::new(FfmpegRemuxProcessor::new()))
    }

    pub fn with_post_processor(processor: Arc<dyn PostDownloadProcessor>) -> Self {
        let (worker_tx, worker_rx) = mpsc::channel();
        let (export_tx, export_rx) = mpsc::channel();
        let manager = DownloadManager::new(
            DownloadManagerConfig {
                auto_start: true,
                run_post_processors_on_completion: false,
                post_processors: vec![processor.clone()],
                event_hooks: Vec::new(),
            },
            InMemoryDownloadStore::default(),
            DesktopDownloadExecutor::new(worker_tx),
        );

        Self {
            manager,
            worker_rx,
            export_tx,
            export_rx,
            processor,
            asset_labels: HashMap::new(),
            export_state: HashMap::new(),
            exported_paths: HashMap::new(),
        }
    }

    pub fn tasks(&self) -> Vec<DownloadTaskSnapshot> {
        self.manager.snapshot().tasks
    }

    pub fn label_for_asset(&self, asset_id: &str) -> Option<&str> {
        self.asset_labels.get(asset_id).map(String::as_str)
    }

    pub fn exported_path(&self, task_id: DownloadTaskId) -> Option<&Path> {
        self.exported_paths.get(&task_id).map(PathBuf::as_path)
    }

    pub fn export_state(&self, task_id: DownloadTaskId) -> ExportState {
        self.export_state.get(&task_id).cloned().unwrap_or_default()
    }

    pub fn export_plugin_installed(&self) -> bool {
        !self.processor.supported_input_formats().is_empty()
    }

    pub fn create_prepared_task(
        &mut self,
        asset_id: String,
        label: String,
        prepared: PreparedDownloadTask,
    ) -> player_runtime::PlayerResult<DownloadTaskId> {
        self.asset_labels.insert(asset_id.clone(), label);
        self.manager.create_task(
            asset_id,
            prepared.source,
            prepared.profile,
            prepared.asset_index,
            Instant::now(),
        )
    }

    pub fn trigger_primary_action(
        &mut self,
        task_id: DownloadTaskId,
    ) -> player_runtime::PlayerResult<()> {
        let Some(task) = self.manager.task(task_id) else {
            return Ok(());
        };

        match task.status {
            DownloadTaskStatus::Queued | DownloadTaskStatus::Failed => {
                let _ = self.manager.start_task(task_id, Instant::now())?;
            }
            DownloadTaskStatus::Preparing | DownloadTaskStatus::Downloading => {
                let _ = self.manager.pause_task(task_id, Instant::now())?;
            }
            DownloadTaskStatus::Paused => {
                let _ = self.manager.resume_task(task_id, Instant::now())?;
            }
            DownloadTaskStatus::Completed | DownloadTaskStatus::Removed => {}
        }
        Ok(())
    }

    pub fn remove_task(&mut self, task_id: DownloadTaskId) -> player_runtime::PlayerResult<()> {
        let Some(snapshot) = self.manager.task(task_id) else {
            return Ok(());
        };

        let target_directory = snapshot.profile.target_directory.clone();
        let _ = self.manager.remove_task(task_id, Instant::now())?;
        self.export_state.remove(&task_id);
        self.exported_paths.remove(&task_id);
        if let Some(target_directory) = target_directory {
            let _ = fs::remove_dir_all(target_directory);
        }
        Ok(())
    }

    pub fn request_export(&mut self, task_id: DownloadTaskId) -> player_runtime::PlayerResult<()> {
        let Some(snapshot) = self.manager.task(task_id) else {
            return Ok(());
        };
        if snapshot.status != Completed {
            return Err(playback_error(format!(
                "download task {} must complete before export",
                task_id.get()
            )));
        }
        if snapshot.source.content_format == DownloadContentFormat::SingleFile {
            return Err(capability_error(format!(
                "download task {} is already a single file",
                task_id.get()
            )));
        }
        if self
            .export_state
            .get(&task_id)
            .is_some_and(|state| state.in_progress)
        {
            return Ok(());
        }

        let output_path = derive_export_output_path(&snapshot)?;
        let processor = self.processor.clone();
        let export_tx = self.export_tx.clone();
        self.export_state.insert(
            task_id,
            ExportState {
                in_progress: true,
                ratio: Some(0.0),
            },
        );
        thread::Builder::new()
            .name(format!("desktop-export-{}", task_id.get()))
            .spawn(move || {
                let input = match completed_download_info(&snapshot) {
                    Ok(input) => input,
                    Err(error) => {
                        let _ = export_tx.send(ExportEvent::Failed {
                            task_id,
                            error: error.to_string(),
                        });
                        return;
                    }
                };
                let progress = ChannelProcessorProgress {
                    task_id,
                    export_tx: export_tx.clone(),
                    cancelled: Arc::new(AtomicBool::new(false)),
                };
                match processor.process(&input, &output_path, &progress) {
                    Ok(ProcessorOutput::MuxedFile { path, .. }) => {
                        let _ = export_tx.send(ExportEvent::Completed {
                            task_id,
                            output_path: path,
                        });
                    }
                    Ok(ProcessorOutput::Skipped) => {
                        let _ = export_tx.send(ExportEvent::Failed {
                            task_id,
                            error: "processor skipped export".to_owned(),
                        });
                    }
                    Err(error) => {
                        let _ = export_tx.send(ExportEvent::Failed {
                            task_id,
                            error: error.to_string(),
                        });
                    }
                }
            })
            .map_err(|error| platform_error(format!("failed to spawn export worker: {error}")))?;

        Ok(())
    }

    pub fn poll(&mut self) -> DesktopDownloadPollResult {
        let mut result = DesktopDownloadPollResult::default();

        while let Ok(event) = self.worker_rx.try_recv() {
            result.changed = true;
            match event {
                WorkerEvent::Progress {
                    task_id,
                    received_bytes,
                    received_segments,
                } => {
                    let _ = self.manager.update_progress(
                        task_id,
                        received_bytes,
                        received_segments,
                        Instant::now(),
                    );
                }
                WorkerEvent::Completed {
                    task_id,
                    completed_path,
                } => {
                    let _ = self
                        .manager
                        .complete_task(task_id, completed_path, Instant::now());
                }
                WorkerEvent::Failed { task_id, error } => {
                    let _ = self
                        .manager
                        .fail_task(task_id, error.clone(), Instant::now());
                    result.messages.push(error.to_string());
                }
            }
        }

        while let Ok(event) = self.export_rx.try_recv() {
            result.changed = true;
            match event {
                ExportEvent::Progress { task_id, ratio } => {
                    self.export_state.insert(
                        task_id,
                        ExportState {
                            in_progress: true,
                            ratio: Some(ratio),
                        },
                    );
                }
                ExportEvent::Completed {
                    task_id,
                    output_path,
                } => {
                    self.export_state.insert(
                        task_id,
                        ExportState {
                            in_progress: false,
                            ratio: Some(1.0),
                        },
                    );
                    self.exported_paths.insert(task_id, output_path.clone());
                    result.messages.push(format!(
                        "exported task {} to {}",
                        task_id.get(),
                        output_path.display()
                    ));
                }
                ExportEvent::Failed { task_id, error } => {
                    self.export_state.insert(
                        task_id,
                        ExportState {
                            in_progress: false,
                            ratio: None,
                        },
                    );
                    result.messages.push(error);
                }
            }
        }

        let _ = self.manager.drain_events();
        result
    }
}

pub fn make_asset_id(prefix: &str) -> String {
    let timestamp_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or(0);
    format!("{prefix}-{timestamp_ms}")
}

pub fn draft_download_label(source_label: &str, source_uri: &str) -> String {
    let trimmed = source_label.trim();
    if !trimmed.is_empty() {
        return trimmed.to_owned();
    }

    let uri = source_uri.split(['?', '#']).next().unwrap_or(source_uri);
    let file_name = uri.rsplit('/').find(|segment| !segment.is_empty());
    let host = source_uri
        .split("://")
        .nth(1)
        .and_then(|rest| rest.split('/').next())
        .unwrap_or(source_uri);
    let raw = match file_name {
        Some(name) if name.eq_ignore_ascii_case("master.m3u8") => uri
            .split('/')
            .rev()
            .nth(1)
            .filter(|value| !value.is_empty())
            .unwrap_or(host),
        Some(name) if name.contains('.') => name
            .rsplit_once('.')
            .map(|(value, _)| value)
            .unwrap_or(name),
        Some(name) => name,
        None => host,
    };

    let cleaned = raw.replace(['_', '-'], " ").trim().to_owned();
    if cleaned.is_empty() {
        source_uri.to_owned()
    } else {
        cleaned
    }
}

pub fn prepare_download_task(
    asset_id: &str,
    source: &MediaSource,
    source_label: &str,
) -> Result<PreparedDownloadTask> {
    let target_directory = desktop_download_target_directory(asset_id);
    fs::create_dir_all(&target_directory).with_context(|| {
        format!(
            "failed to create desktop download directory `{}`",
            target_directory.display()
        )
    })?;

    let content_format = resolved_download_content_format(source);
    let download_source = DownloadSource::new(source.clone(), content_format);
    let profile = DownloadProfile {
        target_output_format: default_desktop_output_format(content_format, source.uri()),
        target_directory: Some(target_directory),
        ..DownloadProfile::default()
    };
    let asset_index =
        DownloadPlanner::new(DesktopDownloadPlanningClient).plan(&download_source, &profile)?;

    Ok(PreparedDownloadTask {
        source: download_source,
        profile,
        asset_index,
        resolved_label: draft_download_label(source_label, source.uri()),
    })
}

fn default_desktop_output_format(
    content_format: DownloadContentFormat,
    source_uri: &str,
) -> Option<OutputFormat> {
    match content_format {
        DownloadContentFormat::HlsSegments
        | DownloadContentFormat::DashSegments
        | DownloadContentFormat::FlvSegments => Some(OutputFormat::Mp4),
        DownloadContentFormat::SingleFile
            if uri_extension(source_uri).as_deref() == Some("flv") =>
        {
            Some(OutputFormat::Mp4)
        }
        DownloadContentFormat::SingleFile | DownloadContentFormat::Unknown => None,
    }
}

fn resolved_download_content_format(source: &MediaSource) -> DownloadContentFormat {
    match source.protocol() {
        MediaSourceProtocol::Hls => DownloadContentFormat::HlsSegments,
        MediaSourceProtocol::Dash => DownloadContentFormat::DashSegments,
        _ => uri_download_content_format(source.uri()).unwrap_or(DownloadContentFormat::SingleFile),
    }
}

fn uri_download_content_format(uri: &str) -> Option<DownloadContentFormat> {
    match uri_extension(uri)?.as_str() {
        "m3u8" => Some(DownloadContentFormat::HlsSegments),
        "mpd" => Some(DownloadContentFormat::DashSegments),
        "flv" | "ffconcat" => Some(DownloadContentFormat::FlvSegments),
        _ => None,
    }
}

fn uri_extension(uri: &str) -> Option<String> {
    let path = uri
        .split(['?', '#'])
        .next()
        .unwrap_or(uri)
        .rsplit('/')
        .next()
        .unwrap_or(uri);
    path.rsplit_once('.')
        .map(|(_, extension)| extension.to_ascii_lowercase())
}

fn run_download_task(
    task: DownloadTaskSnapshot,
    cancel_flag: Arc<AtomicBool>,
    worker_tx: Sender<WorkerEvent>,
) {
    let result = (|| -> Result<Option<PathBuf>, PlayerError> {
        let work_items = build_work_items(&task)?;
        let mut received_bytes = 0_u64;
        let mut received_segments = 0_u32;

        for item in work_items {
            if cancel_flag.load(Ordering::SeqCst) {
                return Ok(None);
            }

            if let Some(parent) = item.target_path.parent() {
                fs::create_dir_all(parent).map_err(|error| {
                    source_error(format!(
                        "failed to create directory `{}`: {error}",
                        parent.display()
                    ))
                })?;
            }

            if let Some(generated_text) = item.generated_text.as_ref() {
                if item.target_path.exists() {
                    if item.counts_as_segment {
                        received_segments = received_segments.saturating_add(1);
                    }
                    let _ = worker_tx.send(WorkerEvent::Progress {
                        task_id: task.task_id,
                        received_bytes,
                        received_segments,
                    });
                    continue;
                }
                write_generated_text(generated_text, &item.target_path, &cancel_flag)?;
            } else if let Some(local_path) = local_path_from_uri(&item.uri) {
                if item.target_path.exists() {
                    fs::remove_file(&item.target_path).map_err(|error| {
                        source_error(format!(
                            "failed to replace partial local download `{}`: {error}",
                            item.target_path.display()
                        ))
                    })?;
                }
                copy_local_file(
                    &local_path,
                    &item.target_path,
                    item.byte_range,
                    &cancel_flag,
                )?;
            } else {
                let resume_from_bytes =
                    resumable_existing_bytes(&item.target_path, item.expected_size_bytes)?;
                download_remote_file(
                    &item.uri,
                    &item.target_path,
                    item.byte_range,
                    item.expected_size_bytes,
                    resume_from_bytes,
                    &cancel_flag,
                )?;
            }

            let metadata = fs::metadata(&item.target_path).map_err(|error| {
                source_error(format!(
                    "failed to inspect downloaded file `{}`: {error}",
                    item.target_path.display()
                ))
            })?;
            if item.generated_text.is_none() {
                if let Some(expected_size_bytes) = item.expected_size_bytes
                    && metadata.len() != expected_size_bytes
                {
                    return Err(source_error(format!(
                        "downloaded {} bytes for `{}`, expected {}",
                        metadata.len(),
                        item.uri,
                        expected_size_bytes
                    )));
                }
                received_bytes = received_bytes.saturating_add(metadata.len());
            }
            if item.counts_as_segment {
                received_segments = received_segments.saturating_add(1);
            }
            let _ = worker_tx.send(WorkerEvent::Progress {
                task_id: task.task_id,
                received_bytes,
                received_segments,
            });
        }

        Ok(resolve_completed_path(&task))
    })();

    match result {
        Ok(Some(completed_path)) => {
            let _ = worker_tx.send(WorkerEvent::Completed {
                task_id: task.task_id,
                completed_path: Some(completed_path),
            });
        }
        Ok(None) => {}
        Err(error) => {
            let _ = worker_tx.send(WorkerEvent::Failed {
                task_id: task.task_id,
                error,
            });
        }
    }
}

fn build_work_items(task: &DownloadTaskSnapshot) -> Result<Vec<DownloadWorkItem>, PlayerError> {
    let Some(target_directory) = task.profile.target_directory.as_ref() else {
        return Err(playback_error(format!(
            "download task {} is missing a target directory",
            task.task_id.get()
        )));
    };

    let mut items = Vec::new();
    for resource in &task.asset_index.resources {
        let relative_path = resource
            .relative_path
            .clone()
            .unwrap_or_else(|| PathBuf::from(relative_path_for_uri(&resource.uri)));
        items.push(DownloadWorkItem {
            uri: resource.uri.clone(),
            target_path: target_directory.join(relative_path),
            byte_range: resource.byte_range,
            generated_text: resource.generated_text.clone(),
            expected_size_bytes: resource.size_bytes,
            counts_as_segment: false,
        });
    }
    for segment in &task.asset_index.segments {
        let relative_path = segment
            .relative_path
            .clone()
            .unwrap_or_else(|| PathBuf::from(relative_path_for_uri(&segment.uri)));
        items.push(DownloadWorkItem {
            uri: segment.uri.clone(),
            target_path: target_directory.join(relative_path),
            byte_range: segment.byte_range,
            generated_text: None,
            expected_size_bytes: segment.size_bytes,
            counts_as_segment: true,
        });
    }
    Ok(items)
}

fn resolve_completed_path(task: &DownloadTaskSnapshot) -> Option<PathBuf> {
    let target_directory = task.profile.target_directory.as_ref()?;
    match task.source.content_format {
        DownloadContentFormat::SingleFile => task
            .asset_index
            .resources
            .first()
            .and_then(|resource| resource.relative_path.clone())
            .map(|relative_path| target_directory.join(relative_path)),
        DownloadContentFormat::HlsSegments
        | DownloadContentFormat::DashSegments
        | DownloadContentFormat::FlvSegments => task
            .asset_index
            .resources
            .iter()
            .find(|resource| {
                resource
                    .relative_path
                    .as_ref()
                    .and_then(|path| path.extension())
                    .and_then(OsStr::to_str)
                    .is_some_and(|extension| matches!(extension, "m3u8" | "mpd" | "ffconcat"))
            })
            .and_then(|resource| resource.relative_path.clone())
            .map(|relative_path| target_directory.join(relative_path)),
        DownloadContentFormat::Unknown => None,
    }
}

fn download_remote_file(
    uri: &str,
    output_path: &Path,
    byte_range: Option<DownloadByteRange>,
    expected_size_bytes: Option<u64>,
    resume_from_bytes: u64,
    cancel_flag: &Arc<AtomicBool>,
) -> Result<(), PlayerError> {
    if expected_size_bytes.is_some_and(|expected| resume_from_bytes >= expected) {
        return Ok(());
    }

    let requested_start = requested_range_start(byte_range, resume_from_bytes);
    let requested_header = requested_range_header(byte_range, resume_from_bytes);
    let transfer = run_curl_download(uri, requested_header.as_deref(), output_path, cancel_flag)?;

    match transfer.status_code {
        206 => {
            let content_range_start = parse_content_range_start(&transfer.headers);
            if requested_start.is_none() || content_range_start != requested_start {
                let _ = fs::remove_file(&transfer.output_path);
                return Err(source_error(format!(
                    "remote server returned an unexpected Content-Range for `{uri}`"
                )));
            }
        }
        200 => {
            if requested_start.is_some() {
                let _ = fs::remove_file(&transfer.output_path);
                if byte_range.is_none() && resume_from_bytes > 0 {
                    let _ = fs::remove_file(output_path);
                    return download_remote_file(
                        uri,
                        output_path,
                        None,
                        expected_size_bytes,
                        0,
                        cancel_flag,
                    );
                }
                return Err(source_error(format!(
                    "remote server did not honor the requested byte range for `{uri}`"
                )));
            }
        }
        416 => {
            let _ = fs::remove_file(&transfer.output_path);
            if resume_from_bytes > 0 {
                let _ = fs::remove_file(output_path);
                return download_remote_file(
                    uri,
                    output_path,
                    byte_range,
                    expected_size_bytes,
                    0,
                    cancel_flag,
                );
            }
            return Err(source_error(format!(
                "remote resource rejected the requested byte range for `{uri}`"
            )));
        }
        401 | 403 | 404 | 410 => {
            let _ = fs::remove_file(&transfer.output_path);
            return Err(source_error(format!(
                "offline download resource is stale or expired (HTTP {}) for `{uri}`; refresh the media link and prepare the task again",
                transfer.status_code
            )));
        }
        200..=299 => {}
        status => {
            let _ = fs::remove_file(&transfer.output_path);
            return Err(network_error(format!(
                "remote resource returned HTTP {status} for `{uri}`"
            )));
        }
    }

    if transfer.append {
        append_file(&transfer.output_path, output_path)?;
        let _ = fs::remove_file(&transfer.output_path);
    } else {
        if output_path.exists() {
            fs::remove_file(output_path).map_err(|error| {
                source_error(format!(
                    "failed to replace `{}` before download: {error}",
                    output_path.display()
                ))
            })?;
        }
        fs::rename(&transfer.output_path, output_path).map_err(|error| {
            source_error(format!(
                "failed to move downloaded temp file `{}` to `{}`: {error}",
                transfer.output_path.display(),
                output_path.display()
            ))
        })?;
    }
    Ok(())
}

fn run_curl_download(
    uri: &str,
    range_header: Option<&str>,
    output_path: &Path,
    cancel_flag: &Arc<AtomicBool>,
) -> Result<CurlDownloadOutput, PlayerError> {
    let temp_path = temporary_sibling_path(output_path, "part");
    let headers_path = temporary_sibling_path(output_path, "headers");
    let mut command = Command::new(CURL_BIN);
    command
        .arg("-L")
        .arg("--silent")
        .arg("--show-error")
        .arg("--dump-header")
        .arg(&headers_path)
        .arg("--write-out")
        .arg("%{http_code}");
    if let Some(range_header) = range_header {
        command.arg("--range").arg(range_header);
    }
    let mut child = command
        .arg("--output")
        .arg(&temp_path)
        .arg(uri)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| network_error(format!("failed to spawn curl: {error}")))?;

    let status = wait_for_child(&mut child, uri, cancel_flag)?;
    let mut stdout = String::new();
    if let Some(mut stream) = child.stdout.take() {
        stream.read_to_string(&mut stdout).map_err(|error| {
            network_error(format!(
                "failed to read curl status output for `{uri}`: {error}"
            ))
        })?;
    }
    let mut stderr = String::new();
    if let Some(mut stream) = child.stderr.take() {
        stream.read_to_string(&mut stderr).map_err(|error| {
            network_error(format!(
                "failed to read curl error output for `{uri}`: {error}"
            ))
        })?;
    }

    if !status.success() {
        let _ = fs::remove_file(&temp_path);
        let _ = fs::remove_file(&headers_path);
        return Err(network_error(format!(
            "curl failed for `{uri}` with status {status}: {}",
            stderr.trim()
        )));
    }

    let headers = fs::read_to_string(&headers_path).unwrap_or_default();
    let _ = fs::remove_file(&headers_path);
    let status_code = stdout.trim().parse::<u16>().unwrap_or(0);
    Ok(CurlDownloadOutput {
        status_code,
        headers,
        output_path: temp_path,
        append: range_header.is_some(),
    })
}

fn copy_local_file(
    source_path: &Path,
    output_path: &Path,
    byte_range: Option<DownloadByteRange>,
    cancel_flag: &Arc<AtomicBool>,
) -> Result<(), PlayerError> {
    if cancel_flag.load(Ordering::SeqCst) {
        return Ok(());
    }
    if let Some(byte_range) = byte_range {
        let mut input = fs::File::open(source_path).map_err(|error| {
            source_error(format!(
                "failed to open `{}` for ranged copy: {error}",
                source_path.display()
            ))
        })?;
        input
            .seek(SeekFrom::Start(byte_range.offset))
            .map_err(|error| {
                source_error(format!(
                    "failed to seek `{}` for ranged copy: {error}",
                    source_path.display()
                ))
            })?;
        let mut output = fs::File::create(output_path).map_err(|error| {
            source_error(format!(
                "failed to create `{}` for ranged copy: {error}",
                output_path.display()
            ))
        })?;
        let mut limited = input.take(byte_range.length);
        std::io::copy(&mut limited, &mut output).map_err(|error| {
            source_error(format!(
                "failed to copy byte range from `{}` to `{}`: {error}",
                source_path.display(),
                output_path.display()
            ))
        })?;
        return Ok(());
    }
    fs::copy(source_path, output_path).map_err(|error| {
        source_error(format!(
            "failed to copy `{}` to `{}`: {error}",
            source_path.display(),
            output_path.display()
        ))
    })?;
    Ok(())
}

fn write_generated_text(
    text: &str,
    output_path: &Path,
    cancel_flag: &Arc<AtomicBool>,
) -> Result<(), PlayerError> {
    if cancel_flag.load(Ordering::SeqCst) {
        return Ok(());
    }
    let mut output = fs::File::create(output_path).map_err(|error| {
        source_error(format!(
            "failed to create generated download resource `{}`: {error}",
            output_path.display()
        ))
    })?;
    output.write_all(text.as_bytes()).map_err(|error| {
        source_error(format!(
            "failed to write generated download resource `{}`: {error}",
            output_path.display()
        ))
    })
}

fn resumable_existing_bytes(
    output_path: &Path,
    expected_size_bytes: Option<u64>,
) -> Result<u64, PlayerError> {
    if !output_path.exists() {
        return Ok(0);
    }
    let Some(expected) = expected_size_bytes else {
        fs::remove_file(output_path).map_err(|error| {
            source_error(format!(
                "failed to remove unverified partial download `{}`: {error}",
                output_path.display()
            ))
        })?;
        return Ok(0);
    };
    let existing = fs::metadata(output_path)
        .map_err(|error| {
            source_error(format!(
                "failed to inspect partial download `{}`: {error}",
                output_path.display()
            ))
        })?
        .len();

    if existing == expected {
        return Ok(existing);
    }
    if existing > 0 && existing < expected {
        return Ok(existing);
    }
    fs::remove_file(output_path).map_err(|error| {
        source_error(format!(
            "failed to remove invalid partial download `{}`: {error}",
            output_path.display()
        ))
    })?;
    Ok(0)
}

fn requested_range_header(
    byte_range: Option<DownloadByteRange>,
    resume_from_bytes: u64,
) -> Option<String> {
    if let Some(byte_range) = byte_range {
        if resume_from_bytes >= byte_range.length {
            return None;
        }
        let start = byte_range.offset.saturating_add(resume_from_bytes);
        let remaining = byte_range.length.saturating_sub(resume_from_bytes);
        let end = start.saturating_add(remaining.saturating_sub(1));
        return Some(format!("{start}-{end}"));
    }
    (resume_from_bytes > 0).then(|| format!("{resume_from_bytes}-"))
}

fn requested_range_start(
    byte_range: Option<DownloadByteRange>,
    resume_from_bytes: u64,
) -> Option<u64> {
    if let Some(byte_range) = byte_range {
        return Some(byte_range.offset.saturating_add(resume_from_bytes));
    }
    (resume_from_bytes > 0).then_some(resume_from_bytes)
}

fn parse_content_range_start(headers: &str) -> Option<u64> {
    let content_range = last_header_value(headers, "content-range")?;
    let (_, range) = content_range.split_once(' ')?;
    if range.starts_with('*') {
        return None;
    }
    range.split_once('-')?.0.parse::<u64>().ok()
}

fn append_file(source_path: &Path, output_path: &Path) -> Result<(), PlayerError> {
    let mut input = fs::File::open(source_path).map_err(|error| {
        source_error(format!(
            "failed to open temp download `{}` for append: {error}",
            source_path.display()
        ))
    })?;
    let mut output = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(output_path)
        .map_err(|error| {
            source_error(format!(
                "failed to open partial download `{}` for append: {error}",
                output_path.display()
            ))
        })?;
    std::io::copy(&mut input, &mut output).map_err(|error| {
        source_error(format!(
            "failed to append `{}` to `{}`: {error}",
            source_path.display(),
            output_path.display()
        ))
    })?;
    Ok(())
}

fn temporary_sibling_path(output_path: &Path, extension: &str) -> PathBuf {
    let file_name = output_path
        .file_name()
        .and_then(OsStr::to_str)
        .unwrap_or("download");
    output_path.with_file_name(format!("{file_name}.{}.{}", unique_suffix(), extension))
}

fn unique_suffix() -> String {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos().to_string())
        .unwrap_or_else(|_| "0".to_owned())
}

#[derive(Debug)]
struct CurlDownloadOutput {
    status_code: u16,
    headers: String,
    output_path: PathBuf,
    append: bool,
}

fn wait_for_child(
    child: &mut Child,
    uri: &str,
    cancel_flag: &Arc<AtomicBool>,
) -> Result<std::process::ExitStatus, PlayerError> {
    loop {
        if cancel_flag.load(Ordering::SeqCst) {
            let _ = child.kill();
            let _ = child.wait();
            return Err(playback_error(format!("download cancelled for `{uri}`")));
        }

        match child.try_wait() {
            Ok(Some(status)) => return Ok(status),
            Ok(None) => thread::sleep(CURL_POLL_INTERVAL),
            Err(error) => {
                return Err(network_error(format!(
                    "failed to monitor curl process: {error}"
                )));
            }
        }
    }
}

fn fetch_remote_text(uri: &str) -> Result<String> {
    if let Some(local_path) = local_path_from_uri(uri) {
        return fs::read_to_string(&local_path)
            .with_context(|| format!("failed to read local manifest `{}`", local_path.display()));
    }

    let output = Command::new(CURL_BIN)
        .arg("-L")
        .arg("--fail")
        .arg("--silent")
        .arg("--show-error")
        .arg(uri)
        .output()
        .with_context(|| format!("failed to spawn curl for `{uri}`"))?;
    if !output.status.success() {
        bail!(
            "failed to fetch remote manifest `{uri}`: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    String::from_utf8(output.stdout)
        .map_err(|error| anyhow!("manifest `{uri}` was not valid UTF-8: {error}"))
}

fn probe_remote_content_length(uri: &str) -> player_runtime::PlayerResult<Option<u64>> {
    if let Some(size) = probe_remote_head_content_length(uri)? {
        return Ok(Some(size));
    }
    probe_remote_range_content_length(uri)
}

fn probe_remote_head_content_length(uri: &str) -> player_runtime::PlayerResult<Option<u64>> {
    let output = Command::new(CURL_BIN)
        .arg("-L")
        .arg("--head")
        .arg("--silent")
        .arg("--show-error")
        .arg("--write-out")
        .arg("\n%{http_code}")
        .arg(uri)
        .output()
        .map_err(|error| {
            network_error(format!("failed to spawn curl HEAD for `{uri}`: {error}"))
        })?;
    if !output.status.success() {
        return Ok(None);
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let Some((headers, status_text)) = stdout.rsplit_once('\n') else {
        return Ok(None);
    };
    let status_code = status_text.trim().parse::<u16>().unwrap_or(0);
    if !(200..=299).contains(&status_code) {
        return Ok(None);
    }
    Ok(last_header_value(headers, "content-length").and_then(|value| value.parse::<u64>().ok()))
}

fn probe_remote_range_content_length(uri: &str) -> player_runtime::PlayerResult<Option<u64>> {
    let output_path =
        std::env::temp_dir().join(format!("vesper-content-length-{}", unique_suffix()));
    let transfer = run_curl_download(
        uri,
        Some("0-0"),
        &output_path,
        &Arc::new(AtomicBool::new(false)),
    )?;
    let size = if transfer.status_code == 206 {
        last_header_value(&transfer.headers, "content-range").and_then(|value| {
            value
                .rsplit_once('/')
                .and_then(|(_, total)| total.trim().parse::<u64>().ok())
        })
    } else {
        None
    };
    let _ = fs::remove_file(&transfer.output_path);
    Ok(size)
}

fn last_header_value<'a>(headers: &'a str, name: &str) -> Option<&'a str> {
    headers.lines().rev().find_map(|line| {
        let (header_name, value) = line.split_once(':')?;
        header_name
            .trim()
            .eq_ignore_ascii_case(name)
            .then(|| value.trim())
    })
}

fn desktop_download_target_directory(asset_id: &str) -> PathBuf {
    std::env::temp_dir().join(DOWNLOAD_ROOT_DIR).join(asset_id)
}

fn local_path_from_uri(uri: &str) -> Option<PathBuf> {
    if uri.starts_with("http://") || uri.starts_with("https://") {
        None
    } else if let Some(path) = uri.strip_prefix("file://") {
        Some(file_uri_path(path))
    } else {
        Some(PathBuf::from(uri))
    }
}

fn file_uri_path(path: &str) -> PathBuf {
    #[cfg(target_os = "windows")]
    {
        if path.starts_with('/') && path.chars().nth(2) == Some(':') {
            return PathBuf::from(&path[1..]);
        }
    }

    PathBuf::from(path)
}

fn relative_path_for_uri(uri: &str) -> String {
    if let Some(local_path) = local_path_from_uri(uri) {
        return local_path
            .file_name()
            .and_then(OsStr::to_str)
            .map(sanitize_path_segment)
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| "resource.bin".to_owned());
    }

    let without_scheme = uri.split_once("://").map(|(_, rest)| rest).unwrap_or(uri);
    let host = without_scheme
        .split('/')
        .next()
        .filter(|value| !value.is_empty())
        .map(sanitize_path_segment)
        .unwrap_or_else(|| "remote".to_owned());
    let path_segments = without_scheme
        .split('/')
        .skip(1)
        .filter(|segment| !segment.is_empty())
        .map(|segment| sanitize_path_segment(segment.split(['?', '#']).next().unwrap_or(segment)))
        .collect::<Vec<_>>();

    if path_segments.is_empty() {
        host
    } else {
        format!("{host}/{}", path_segments.join("/"))
    }
}

fn sanitize_path_segment(segment: &str) -> String {
    let sanitized = segment
        .chars()
        .map(|character| match character {
            'a'..='z' | 'A'..='Z' | '0'..='9' | '-' | '_' | '.' => character,
            _ => '_',
        })
        .collect::<String>();
    if sanitized.is_empty() {
        "item".to_owned()
    } else {
        sanitized
    }
}

fn derive_export_output_path(snapshot: &DownloadTaskSnapshot) -> Result<PathBuf, PlayerError> {
    if let Some(completed_path) = snapshot.asset_index.completed_path.as_ref() {
        return Ok(completed_path.with_extension("mp4"));
    }
    let Some(target_directory) = snapshot.profile.target_directory.as_ref() else {
        return Err(playback_error(format!(
            "download task {} is missing a target directory for export",
            snapshot.task_id.get()
        )));
    };
    Ok(target_directory.join(format!(
        "{}.mp4",
        sanitize_path_segment(snapshot.asset_id.as_str())
    )))
}

fn completed_download_info(
    snapshot: &DownloadTaskSnapshot,
) -> Result<CompletedDownloadInfo, PlayerError> {
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
            return Err(capability_error(format!(
                "download task {} has unknown content format",
                snapshot.task_id.get()
            )));
        }
    };

    Ok(CompletedDownloadInfo {
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
    })
}

fn resolve_manifest_path(snapshot: &DownloadTaskSnapshot) -> Result<PathBuf, PlayerError> {
    let Some(target_directory) = snapshot.profile.target_directory.as_ref() else {
        return Err(playback_error(format!(
            "download task {} is missing target directory",
            snapshot.task_id.get()
        )));
    };
    snapshot
        .asset_index
        .resources
        .iter()
        .find_map(|resource| {
            resource.relative_path.as_ref().and_then(|relative_path| {
                relative_path
                    .extension()
                    .and_then(OsStr::to_str)
                    .is_some_and(|extension| matches!(extension, "m3u8" | "mpd" | "ffconcat"))
                    .then(|| target_directory.join(relative_path))
            })
        })
        .ok_or_else(|| {
            source_error(format!(
                "download task {} is missing a local manifest",
                snapshot.task_id.get()
            ))
        })
}

fn resolve_segment_paths(snapshot: &DownloadTaskSnapshot) -> Vec<PathBuf> {
    let Some(target_directory) = snapshot.profile.target_directory.as_ref() else {
        return Vec::new();
    };
    snapshot
        .asset_index
        .segments
        .iter()
        .filter_map(|segment| {
            segment
                .relative_path
                .as_ref()
                .map(|path| target_directory.join(path))
        })
        .collect()
}

fn resolve_single_file_path(snapshot: &DownloadTaskSnapshot) -> Result<PathBuf, PlayerError> {
    let Some(target_directory) = snapshot.profile.target_directory.as_ref() else {
        return Err(playback_error(format!(
            "download task {} is missing target directory",
            snapshot.task_id.get()
        )));
    };
    snapshot
        .asset_index
        .resources
        .first()
        .and_then(|resource| resource.relative_path.as_ref())
        .map(|relative_path| target_directory.join(relative_path))
        .ok_or_else(|| {
            source_error(format!(
                "download task {} is missing a local file path",
                snapshot.task_id.get()
            ))
        })
}

struct ChannelProcessorProgress {
    task_id: DownloadTaskId,
    export_tx: Sender<ExportEvent>,
    cancelled: Arc<AtomicBool>,
}

impl ProcessorProgress for ChannelProcessorProgress {
    fn on_progress(&self, ratio: f32) {
        let _ = self.export_tx.send(ExportEvent::Progress {
            task_id: self.task_id,
            ratio,
        });
    }

    fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::SeqCst)
    }
}

pub fn download_status_label(status: DownloadTaskStatus) -> &'static str {
    match status {
        DownloadTaskStatus::Queued => "QUEUED",
        DownloadTaskStatus::Preparing => "PREPARING",
        DownloadTaskStatus::Downloading => "DOWNLOADING",
        DownloadTaskStatus::Paused => "PAUSED",
        DownloadTaskStatus::Completed => "COMPLETED",
        DownloadTaskStatus::Failed => "FAILED",
        DownloadTaskStatus::Removed => "REMOVED",
    }
}

pub fn download_primary_action_label(status: DownloadTaskStatus) -> Option<&'static str> {
    match status {
        DownloadTaskStatus::Queued | DownloadTaskStatus::Failed => Some("START"),
        DownloadTaskStatus::Preparing | DownloadTaskStatus::Downloading => Some("PAUSE"),
        DownloadTaskStatus::Paused => Some("RESUME"),
        DownloadTaskStatus::Completed | DownloadTaskStatus::Removed => None,
    }
}

pub fn download_progress_summary(snapshot: &DownloadTaskSnapshot) -> String {
    let progress = &snapshot.progress;
    if let Some(total_segments) = progress.total_segments {
        return format!("{}/{} SEG", progress.received_segments, total_segments);
    }
    if let Some(total_bytes) = progress.total_bytes {
        return format!(
            "{} / {}",
            format_bytes(progress.received_bytes),
            format_bytes(total_bytes)
        );
    }
    format_bytes(progress.received_bytes)
}

fn format_bytes(value: u64) -> String {
    if value >= 1024 * 1024 * 1024 {
        format!("{:.1} GB", value as f64 / 1024.0 / 1024.0 / 1024.0)
    } else if value >= 1024 * 1024 {
        format!("{:.1} MB", value as f64 / 1024.0 / 1024.0)
    } else if value >= 1024 {
        format!("{:.0} KB", value as f64 / 1024.0)
    } else {
        format!("{value} B")
    }
}

pub fn normalized_progress_ratio(progress: &DownloadProgressSnapshot) -> Option<f32> {
    progress
        .completion_ratio()
        .or_else(|| match progress.total_segments {
            Some(total_segments) if total_segments > 0 => {
                Some(progress.received_segments as f32 / total_segments as f32)
            }
            _ => None,
        })
}

fn source_error(message: impl Into<String>) -> PlayerError {
    PlayerError::with_category(
        PlayerErrorCode::InvalidSource,
        PlayerErrorCategory::Source,
        message,
    )
}

fn network_error(message: impl Into<String>) -> PlayerError {
    PlayerError::with_taxonomy(
        PlayerErrorCode::BackendFailure,
        PlayerErrorCategory::Network,
        true,
        message,
    )
}

fn playback_error(message: impl Into<String>) -> PlayerError {
    PlayerError::with_category(
        PlayerErrorCode::InvalidState,
        PlayerErrorCategory::Playback,
        message,
    )
}

fn capability_error(message: impl Into<String>) -> PlayerError {
    PlayerError::with_category(
        PlayerErrorCode::Unsupported,
        PlayerErrorCategory::Capability,
        message,
    )
}

fn platform_error(message: impl Into<String>) -> PlayerError {
    PlayerError::with_category(
        PlayerErrorCode::BackendFailure,
        PlayerErrorCategory::Platform,
        message,
    )
}

#[cfg(test)]
mod tests {
    use super::{
        DesktopDownloadController, download_progress_summary, local_path_from_uri,
        prepare_download_task,
    };
    use player_model::MediaSource;
    use player_plugin_loader::LoadedDynamicPlugin;
    use player_runtime::{DownloadTaskId, DownloadTaskStatus};
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::process::Command;
    use std::thread;
    use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

    const TEST_TIMEOUT: Duration = Duration::from_secs(20);

    #[test]
    fn local_path_from_uri_supports_file_scheme() {
        let path = local_path_from_uri("file:///tmp/vesper-fixture/master.m3u8")
            .expect("file uri should map to a local path");
        assert_eq!(path, PathBuf::from("/tmp/vesper-fixture/master.m3u8"));
    }

    #[test]
    fn prepare_download_task_detects_local_hls_manifest_path() {
        let workspace = TestWorkspace::new("local-hls-prepare");
        let manifest_path = workspace.path().join("fixture.m3u8");
        fs::write(workspace.path().join("segment_000.ts"), b"segment")
            .expect("write local segment");
        fs::write(
            &manifest_path,
            "#EXTM3U\n#EXT-X-VERSION:3\n#EXT-X-TARGETDURATION:1\n#EXT-X-MEDIA-SEQUENCE:0\n#EXTINF:1.0,\nsegment_000.ts\n#EXT-X-ENDLIST\n",
        )
        .expect("write local hls manifest");

        let prepared = prepare_download_task(
            "local-hls-prepare",
            &MediaSource::new(manifest_path.display().to_string()),
            "LOCAL HLS",
        )
        .expect("prepare local hls task");

        assert_eq!(
            prepared.source.content_format,
            player_runtime::DownloadContentFormat::HlsSegments
        );
        assert_eq!(prepared.asset_index.resources.len(), 1);
        assert_eq!(prepared.asset_index.segments.len(), 1);
        assert!(
            prepared.asset_index.segments[0]
                .uri
                .ends_with("segment_000.ts")
        );
    }

    #[test]
    #[ignore = "requires a built player-remux-ffmpeg shared library artifact and local ffmpeg/ffprobe CLIs"]
    fn desktop_export_remuxes_downloaded_hls_fixture_to_mp4_via_dynamic_plugin() {
        ensure_media_tool_available("ffmpeg");
        ensure_media_tool_available("ffprobe");

        let workspace = TestWorkspace::new("desktop-remux");
        let manifest_path = create_local_hls_fixture(workspace.path());
        let plugin = LoadedDynamicPlugin::load(resolve_vesper_remux_ffmpeg_plugin_path())
            .unwrap_or_else(|error| {
                panic!("failed to load player-remux-ffmpeg plugin for desktop remux test: {error}")
            });
        let processor = plugin
            .post_download_processor()
            .expect("player-remux-ffmpeg plugin should export a post-download processor");

        let mut controller = DesktopDownloadController::with_post_processor(processor);
        let asset_id = format!("desktop-remux-{}", unique_suffix());
        let prepared = prepare_download_task(
            &asset_id,
            &MediaSource::new(manifest_path.display().to_string()),
            "LOCAL HLS",
        )
        .expect("prepare local hls download task");
        let task_id = controller
            .create_prepared_task(asset_id.clone(), "LOCAL HLS".to_owned(), prepared)
            .expect("create desktop download task");

        let completed =
            wait_for_task_status(&mut controller, task_id, DownloadTaskStatus::Completed);
        assert!(
            completed
                .asset_index
                .completed_path
                .as_ref()
                .is_some_and(|path| path
                    .extension()
                    .is_some_and(|extension| extension == "m3u8"))
        );

        controller
            .request_export(task_id)
            .expect("request desktop export");
        let exported_path = wait_for_export_path(&mut controller, task_id);

        assert_eq!(
            exported_path.extension().and_then(|value| value.to_str()),
            Some("mp4")
        );
        assert!(exported_path.is_file(), "exported MP4 should exist on disk");
        let metadata = fs::metadata(&exported_path).expect("stat exported mp4");
        assert!(metadata.len() > 0, "exported MP4 should not be empty");
        assert_eq!(
            probe_container_format(&exported_path),
            "mov,mp4,m4a,3gp,3g2,mj2"
        );
        assert_eq!(controller.export_state(task_id).ratio, Some(1.0));
    }

    fn wait_for_task_status(
        controller: &mut DesktopDownloadController,
        task_id: DownloadTaskId,
        expected_status: DownloadTaskStatus,
    ) -> player_runtime::DownloadTaskSnapshot {
        let deadline = Instant::now() + TEST_TIMEOUT;
        let mut last_messages = Vec::new();

        loop {
            let poll_result = controller.poll();
            last_messages.extend(poll_result.messages);

            if let Some(snapshot) = controller
                .tasks()
                .into_iter()
                .find(|task| task.task_id == task_id)
            {
                if snapshot.status == expected_status {
                    return snapshot;
                }
                if snapshot.status == DownloadTaskStatus::Failed {
                    panic!(
                        "desktop download task {} failed before reaching {:?}: {}",
                        task_id.get(),
                        expected_status,
                        last_messages.join(" | ")
                    );
                }
            }

            if Instant::now() >= deadline {
                panic!(
                    "timed out waiting for desktop download task {} to reach {:?}; last messages: {}",
                    task_id.get(),
                    expected_status,
                    last_messages.join(" | ")
                );
            }

            thread::sleep(Duration::from_millis(25));
        }
    }

    fn wait_for_export_path(
        controller: &mut DesktopDownloadController,
        task_id: DownloadTaskId,
    ) -> PathBuf {
        let deadline = Instant::now() + TEST_TIMEOUT;
        let mut last_messages = Vec::new();

        loop {
            let poll_result = controller.poll();
            last_messages.extend(poll_result.messages);

            if let Some(exported_path) = controller.exported_path(task_id) {
                return exported_path.to_path_buf();
            }

            if Instant::now() >= deadline {
                let progress_summary = controller
                    .tasks()
                    .into_iter()
                    .find(|task| task.task_id == task_id)
                    .map(|task| download_progress_summary(&task))
                    .unwrap_or_else(|| "task missing".to_owned());
                panic!(
                    "timed out waiting for desktop export of task {} to finish; progress: {}; last messages: {}",
                    task_id.get(),
                    progress_summary,
                    last_messages.join(" | ")
                );
            }

            thread::sleep(Duration::from_millis(25));
        }
    }

    fn create_local_hls_fixture(root: &Path) -> PathBuf {
        let input_path = workspace_root().join("fixtures/media/tiny-h264-aac.m4v");
        let manifest_path = root.join("fixture.m3u8");
        let segment_pattern = root.join("segment_%03d.ts");

        // Build a tiny local HLS fixture from the local MP4 so tests do not depend on public samples.
        let status = Command::new("ffmpeg")
            .arg("-y")
            .arg("-loglevel")
            .arg("error")
            .arg("-i")
            .arg(&input_path)
            .arg("-t")
            .arg("3")
            .arg("-map")
            .arg("0:v:0")
            .arg("-an")
            .arg("-c:v")
            .arg("libx264")
            .arg("-preset")
            .arg("ultrafast")
            .arg("-g")
            .arg("24")
            .arg("-sc_threshold")
            .arg("0")
            .arg("-pix_fmt")
            .arg("yuv420p")
            .arg("-f")
            .arg("hls")
            .arg("-hls_time")
            .arg("1")
            .arg("-hls_list_size")
            .arg("0")
            .arg("-hls_playlist_type")
            .arg("vod")
            .arg("-hls_segment_filename")
            .arg(&segment_pattern)
            .arg(&manifest_path)
            .status()
            .expect("spawn ffmpeg to create local hls fixture");

        assert!(
            status.success(),
            "ffmpeg should generate a local hls fixture"
        );
        manifest_path
    }

    fn probe_container_format(path: &Path) -> String {
        let output = Command::new("ffprobe")
            .arg("-v")
            .arg("error")
            .arg("-show_entries")
            .arg("format=format_name")
            .arg("-of")
            .arg("default=nokey=1:noprint_wrappers=1")
            .arg(path)
            .output()
            .expect("spawn ffprobe for exported mp4");

        assert!(
            output.status.success(),
            "ffprobe should parse exported media: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout)
            .expect("ffprobe output utf8")
            .trim()
            .to_owned()
    }

    fn ensure_media_tool_available(binary: &str) {
        let status = Command::new(binary)
            .arg("-version")
            .status()
            .unwrap_or_else(|error| {
                panic!("required media tool `{binary}` is unavailable: {error}")
            });
        assert!(status.success(), "media tool `{binary}` should be callable");
    }

    fn resolve_vesper_remux_ffmpeg_plugin_path() -> PathBuf {
        if let Some(path) = std::env::var_os("VESPER_PLAYER_REMUX_FFMPEG_PLUGIN_PATH") {
            let path = PathBuf::from(path);
            assert!(
                path.is_file(),
                "VESPER_PLAYER_REMUX_FFMPEG_PLUGIN_PATH points to a missing file `{}`",
                path.display()
            );
            return path;
        }

        let workspace_root = workspace_root();
        let target_dir = std::env::var_os("CARGO_TARGET_DIR")
            .map(PathBuf::from)
            .map(|path| {
                if path.is_absolute() {
                    path
                } else {
                    workspace_root.join(path)
                }
            })
            .unwrap_or_else(|| workspace_root.join("target"));
        let library_name = shared_library_name("vesper_remux_ffmpeg");
        let candidates = [
            target_dir.join("debug").join(&library_name),
            target_dir.join("debug").join("deps").join(&library_name),
            target_dir.join("release").join(&library_name),
            target_dir.join("release").join("deps").join(&library_name),
        ];

        candidates
            .into_iter()
            .find(|path| path.is_file())
            .unwrap_or_else(|| {
                panic!(
                    "could not find `{library_name}` under `{}`; build it first with `cargo build -p player-remux-ffmpeg` or set VESPER_PLAYER_REMUX_FFMPEG_PLUGIN_PATH",
                    target_dir.display()
                )
            })
    }

    fn shared_library_name(stem: &str) -> String {
        if cfg!(target_os = "windows") {
            format!("{stem}.dll")
        } else if cfg!(target_os = "macos") {
            format!("lib{stem}.dylib")
        } else {
            format!("lib{stem}.so")
        }
    }

    fn unique_suffix() -> String {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_nanos().to_string())
            .unwrap_or_else(|_| "0".to_owned())
    }

    fn workspace_root() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../../..")
            .canonicalize()
            .expect("derive workspace root")
    }

    struct TestWorkspace {
        path: PathBuf,
    }

    impl TestWorkspace {
        fn new(name: &str) -> Self {
            let path = std::env::temp_dir()
                .join(format!("vesper-basic-player-{name}-{}", unique_suffix()));
            fs::create_dir_all(&path).expect("create temporary test workspace");
            Self { path }
        }

        fn path(&self) -> &Path {
            &self.path
        }
    }

    impl Drop for TestWorkspace {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }
}
