#![deny(unsafe_code)]

use std::collections::{HashMap, VecDeque};
use std::fs;
use std::io::Read;
use std::path::PathBuf;
use std::sync::{Arc, LazyLock, Mutex};
use std::time::{Duration, Instant, UNIX_EPOCH};

use player_model::MediaSource;
use player_plugin::{
    DecoderBitstreamFormat, NativeFrameColorMetadata, NativeFrameHdrMetadata, ProcessorProgress,
    SourceNormalizerNormalizeLevel, SourceNormalizerOutputRoute, SourceNormalizerPacketMediaKind,
    SourceNormalizerPacketSession, SourceNormalizerPacketSessionConfig,
    SourceNormalizerPacketSessionRequirements, SourceNormalizerPacketStreamInfo,
    SourceNormalizerPacketTrackInfo, SourceNormalizerResourceCachePolicy,
    SourceNormalizerResourceSession, SourceNormalizerResourceSessionConfig,
    SourceNormalizerResourceSessionInfo, SourceNormalizerResourceSessionRequirements,
    SourceNormalizerResourceSessionState, SourceNormalizerResourceSessionStatus,
};
use player_plugin_loader::{
    FrameProcessorPluginCapabilitySummary, LoadedDynamicPlugin, PluginCapabilitySummary,
    PluginDiagnosticRecord, PluginDiagnosticStatus, PluginRegistry,
    SourceNormalizerPacketPluginCapabilitySummary, SourceNormalizerResourcePluginCapabilitySummary,
};
use player_runtime::{
    DownloadAssetId, DownloadAssetIndex, DownloadEvent, DownloadExecutor, DownloadManager,
    DownloadManagerConfig, DownloadPrepareResult, DownloadProfile, DownloadSnapshot,
    DownloadSource, DownloadTaskId, DownloadTaskSnapshot, FrameProcessorMode,
    InMemoryDownloadStore, InMemoryPreloadBudgetProvider, NativeFramePipelineMode, PlayerError,
    PlayerErrorCategory, PlayerErrorCode, PlayerPlaybackRoute, PlayerPluginCapabilitySummary,
    PlayerPluginDiagnostic, PlayerPluginDiagnosticStatus,
    PlayerPluginFrameProcessorCapabilitySummary, PlayerPluginParticipation,
    PlayerPluginSourceNormalizerCapabilitySummary, PlayerResult, PlayerRuntimeEvent,
    PlayerRuntimeOptions, PlayerRuntimeStartup, PlaylistActiveItem, PlaylistAdvanceDecision,
    PlaylistCoordinator, PlaylistCoordinatorConfig, PlaylistEvent, PlaylistQueueItem,
    PlaylistSnapshot, PlaylistViewportHint, PreloadBudget, PreloadCandidate, PreloadEvent,
    PreloadExecutor, PreloadPlanner, PreloadSnapshot, PreloadTaskId, PreloadTaskSnapshot,
    SourceNormalizerMode,
};
use serde::Serialize;
use serde::ser::{SerializeMap, Serializer};

const SOURCE_NORMALIZER_STARTUP_TIMEOUT: Duration = Duration::from_secs(10);
const SOURCE_NORMALIZER_SESSION_TIMEOUT: Duration = Duration::from_secs(30);
const SOURCE_NORMALIZER_RESOURCE_READY_TIMEOUT: Duration = Duration::from_secs(10);
const SOURCE_NORMALIZER_PREFLIGHT_CACHE_CAPACITY: usize = 64;
const SOURCE_NORMALIZER_PREFLIGHT_SUCCESS_TTL: Duration = Duration::from_secs(5 * 60);
const SOURCE_NORMALIZER_PREFLIGHT_FAILURE_TTL: Duration = Duration::from_secs(30);
const FMP4_BOX_MARKER_SCAN_LIMIT_BYTES: u64 = 1024 * 1024;

static SOURCE_NORMALIZER_PREFLIGHT_CACHE: LazyLock<Mutex<PreflightDiagnosticCache>> =
    LazyLock::new(|| {
        Mutex::new(PreflightDiagnosticCache::new(
            SOURCE_NORMALIZER_PREFLIGHT_CACHE_CAPACITY,
        ))
    });

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MobilePreloadCommand {
    Start { task: PreloadTaskSnapshot },
    Cancel { task_id: PreloadTaskId },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MobileDownloadCommand {
    Prepare { task: DownloadTaskSnapshot },
    Start { task: DownloadTaskSnapshot },
    Pause { task_id: DownloadTaskId },
    Resume { task: DownloadTaskSnapshot },
    Remove { task_id: DownloadTaskId },
}

#[derive(Debug, Clone)]
pub struct MobileCommandQueue<T> {
    label: &'static str,
    queue: Arc<Mutex<VecDeque<T>>>,
}

impl<T> MobileCommandQueue<T> {
    pub fn new(label: &'static str) -> Self {
        Self {
            label,
            queue: Arc::new(Mutex::new(VecDeque::new())),
        }
    }

    #[doc(hidden)]
    pub fn from_shared_for_tests(label: &'static str, queue: Arc<Mutex<VecDeque<T>>>) -> Self {
        Self { label, queue }
    }

    pub fn push(&self, command: T) -> PlayerResult<()> {
        let mut queue = self.queue.lock().map_err(|_| {
            PlayerError::with_category(
                PlayerErrorCode::BackendFailure,
                PlayerErrorCategory::Platform,
                format!("{} command queue lock poisoned", self.label),
            )
        })?;
        queue.push_back(command);
        Ok(())
    }

    pub fn drain(&self) -> Vec<T> {
        self.queue
            .lock()
            .map(|mut queue| queue.drain(..).collect())
            .unwrap_or_default()
    }

    pub fn drain_map<U>(&self, map: impl FnMut(T) -> U) -> Vec<U> {
        self.queue
            .lock()
            .map(|mut queue| queue.drain(..).map(map).collect())
            .unwrap_or_default()
    }
}

pub fn drain_runtime_events<T>(
    extra_events: &mut VecDeque<PlayerRuntimeEvent>,
    runtime_events: impl IntoIterator<Item = PlayerRuntimeEvent>,
    map: impl FnMut(&PlayerRuntimeEvent) -> Option<T>,
) -> Vec<T> {
    let mut raw_events: Vec<PlayerRuntimeEvent> = extra_events.drain(..).collect();
    raw_events.extend(runtime_events);
    raw_events.iter().filter_map(map).collect()
}

pub fn push_video_surface_event(
    extra_events: &mut VecDeque<PlayerRuntimeEvent>,
    previous: &mut bool,
    attached: bool,
) -> bool {
    if *previous == attached {
        return false;
    }
    *previous = attached;
    extra_events.push_back(PlayerRuntimeEvent::VideoSurfaceChanged { attached });
    true
}

#[derive(Debug, Clone)]
struct MobilePreloadExecutor {
    queue: MobileCommandQueue<MobilePreloadCommand>,
}

impl MobilePreloadExecutor {
    fn new(queue: MobileCommandQueue<MobilePreloadCommand>) -> Self {
        Self { queue }
    }
}

impl PreloadExecutor for MobilePreloadExecutor {
    fn warmup(&mut self, task: &PreloadTaskSnapshot) -> PlayerResult<()> {
        self.queue
            .push(MobilePreloadCommand::Start { task: task.clone() })
    }

    fn cancel(&mut self, task_id: PreloadTaskId) -> PlayerResult<()> {
        self.queue.push(MobilePreloadCommand::Cancel { task_id })
    }
}

#[derive(Debug)]
pub struct MobilePreloadBridgeSession {
    planner: PreloadPlanner<InMemoryPreloadBudgetProvider, MobilePreloadExecutor>,
    command_queue: MobileCommandQueue<MobilePreloadCommand>,
}

impl MobilePreloadBridgeSession {
    pub fn new(budget_provider: InMemoryPreloadBudgetProvider, label: &'static str) -> Self {
        let command_queue = MobileCommandQueue::new(label);
        let executor = MobilePreloadExecutor::new(command_queue.clone());

        Self {
            planner: PreloadPlanner::new(budget_provider, executor),
            command_queue,
        }
    }

    pub fn plan(
        &mut self,
        candidates: impl IntoIterator<Item = PreloadCandidate>,
        now: Instant,
    ) -> Vec<PreloadTaskId> {
        self.planner.plan(candidates, now)
    }

    pub fn cancel(&mut self, task_id: PreloadTaskId) -> PlayerResult<Option<PreloadTaskSnapshot>> {
        self.planner.cancel(task_id)
    }

    pub fn complete(
        &mut self,
        task_id: PreloadTaskId,
    ) -> PlayerResult<Option<PreloadTaskSnapshot>> {
        self.planner.complete(task_id)
    }

    pub fn fail(
        &mut self,
        task_id: PreloadTaskId,
        error: PlayerError,
    ) -> PlayerResult<Option<PreloadTaskSnapshot>> {
        self.planner.fail(task_id, error)
    }

    pub fn expire_due_tasks(&mut self, now: Instant) {
        self.planner.expire_due_tasks(now);
    }

    pub fn snapshot(&self) -> PreloadSnapshot {
        self.planner.snapshot()
    }

    pub fn drain_events(&mut self) -> Vec<PreloadEvent> {
        self.planner.drain_events()
    }

    pub fn drain_commands(&mut self) -> Vec<MobilePreloadCommand> {
        self.command_queue.drain()
    }
}

#[derive(Debug, Clone)]
struct MobileDownloadExecutor {
    queue: MobileCommandQueue<MobileDownloadCommand>,
}

impl MobileDownloadExecutor {
    fn new(queue: MobileCommandQueue<MobileDownloadCommand>) -> Self {
        Self { queue }
    }
}

impl DownloadExecutor for MobileDownloadExecutor {
    fn prepare(&mut self, task: &DownloadTaskSnapshot) -> PlayerResult<DownloadPrepareResult> {
        self.queue
            .push(MobileDownloadCommand::Prepare { task: task.clone() })?;
        Ok(DownloadPrepareResult::Pending)
    }

    fn start(&mut self, task: &DownloadTaskSnapshot) -> PlayerResult<()> {
        self.queue
            .push(MobileDownloadCommand::Start { task: task.clone() })
    }

    fn pause(&mut self, task_id: DownloadTaskId) -> PlayerResult<()> {
        self.queue.push(MobileDownloadCommand::Pause { task_id })
    }

    fn resume(&mut self, task: &DownloadTaskSnapshot) -> PlayerResult<()> {
        self.queue
            .push(MobileDownloadCommand::Resume { task: task.clone() })
    }

    fn remove(&mut self, task_id: DownloadTaskId) -> PlayerResult<()> {
        self.queue.push(MobileDownloadCommand::Remove { task_id })
    }
}

#[derive(Debug)]
pub struct MobileDownloadBridgeSession {
    manager: DownloadManager<InMemoryDownloadStore, MobileDownloadExecutor>,
    command_queue: MobileCommandQueue<MobileDownloadCommand>,
}

impl MobileDownloadBridgeSession {
    pub fn new(config: DownloadManagerConfig, label: &'static str) -> Self {
        let command_queue = MobileCommandQueue::new(label);
        let executor = MobileDownloadExecutor::new(command_queue.clone());

        Self {
            manager: DownloadManager::new(config, InMemoryDownloadStore::default(), executor),
            command_queue,
        }
    }

    pub fn create_task(
        &mut self,
        asset_id: impl Into<String>,
        source: DownloadSource,
        profile: DownloadProfile,
        asset_index: DownloadAssetIndex,
        now: Instant,
    ) -> PlayerResult<DownloadTaskId> {
        self.manager
            .create_task(asset_id, source, profile, asset_index, now)
    }

    pub fn restore_tasks(
        &mut self,
        tasks: impl IntoIterator<Item = DownloadTaskSnapshot>,
        now: Instant,
    ) -> PlayerResult<Vec<DownloadTaskSnapshot>> {
        self.manager.restore_tasks(tasks, now)
    }

    pub fn start_task(
        &mut self,
        task_id: DownloadTaskId,
        now: Instant,
    ) -> PlayerResult<Option<DownloadTaskSnapshot>> {
        self.manager.start_task(task_id, now)
    }

    pub fn pause_task(
        &mut self,
        task_id: DownloadTaskId,
        now: Instant,
    ) -> PlayerResult<Option<DownloadTaskSnapshot>> {
        self.manager.pause_task(task_id, now)
    }

    pub fn resume_task(
        &mut self,
        task_id: DownloadTaskId,
        now: Instant,
    ) -> PlayerResult<Option<DownloadTaskSnapshot>> {
        self.manager.resume_task(task_id, now)
    }

    pub fn update_progress(
        &mut self,
        task_id: DownloadTaskId,
        received_bytes: u64,
        received_segments: u32,
        now: Instant,
    ) -> PlayerResult<Option<DownloadTaskSnapshot>> {
        self.manager
            .update_progress(task_id, received_bytes, received_segments, now)
    }

    pub fn complete_preparation(
        &mut self,
        task_id: DownloadTaskId,
        asset_index: DownloadAssetIndex,
        now: Instant,
    ) -> PlayerResult<Option<DownloadTaskSnapshot>> {
        self.manager.complete_preparation(task_id, asset_index, now)
    }

    pub fn replace_task_plan(
        &mut self,
        task_id: DownloadTaskId,
        source: DownloadSource,
        profile: DownloadProfile,
        asset_index: DownloadAssetIndex,
        now: Instant,
    ) -> PlayerResult<Option<DownloadTaskSnapshot>> {
        self.manager
            .replace_task_plan(task_id, source, profile, asset_index, now)
    }

    pub fn complete_task(
        &mut self,
        task_id: DownloadTaskId,
        completed_path: Option<PathBuf>,
        now: Instant,
    ) -> PlayerResult<Option<DownloadTaskSnapshot>> {
        self.manager.complete_task(task_id, completed_path, now)
    }

    pub fn fail_task(
        &mut self,
        task_id: DownloadTaskId,
        error: PlayerError,
        now: Instant,
    ) -> PlayerResult<Option<DownloadTaskSnapshot>> {
        self.manager.fail_task(task_id, error, now)
    }

    pub fn remove_task(
        &mut self,
        task_id: DownloadTaskId,
        now: Instant,
    ) -> PlayerResult<Option<DownloadTaskSnapshot>> {
        self.manager.remove_task(task_id, now)
    }

    pub fn task(&self, task_id: DownloadTaskId) -> Option<DownloadTaskSnapshot> {
        self.manager.task(task_id)
    }

    pub fn tasks_for_asset(&self, asset_id: &DownloadAssetId) -> Vec<DownloadTaskSnapshot> {
        self.manager.tasks_for_asset(asset_id)
    }

    pub fn snapshot(&self) -> DownloadSnapshot {
        self.manager.snapshot()
    }

    pub fn export_task_output(
        &self,
        task_id: DownloadTaskId,
        output_path: Option<PathBuf>,
        progress: &dyn ProcessorProgress,
    ) -> PlayerResult<PathBuf> {
        self.manager
            .export_task_output(task_id, output_path.as_deref(), progress)
    }

    pub fn drain_events(&mut self) -> Vec<DownloadEvent> {
        self.manager.drain_events()
    }

    pub fn drain_commands(&mut self) -> Vec<MobileDownloadCommand> {
        self.command_queue.drain()
    }
}

pub fn mobile_download_manager_config(
    platform_label: &str,
    auto_start: bool,
    run_post_processors_on_completion: bool,
    plugin_library_paths: impl IntoIterator<Item = PathBuf>,
) -> PlayerResult<DownloadManagerConfig> {
    let mut post_processors = Vec::new();
    let mut event_hooks = Vec::new();

    for path in plugin_library_paths {
        let plugin = LoadedDynamicPlugin::load(&path).map_err(|error| {
            PlayerError::with_category(
                PlayerErrorCode::InvalidArgument,
                PlayerErrorCategory::Input,
                format!(
                    "failed to load {platform_label} download plugin `{}`: {error}",
                    path.display()
                ),
            )
        })?;
        if let Some(processor) = plugin.post_download_processor() {
            post_processors.push(processor);
        }
        if let Some(hook) = plugin.pipeline_event_hook() {
            event_hooks.push(hook);
        }
    }

    Ok(DownloadManagerConfig {
        auto_start,
        run_post_processors_on_completion,
        post_processors,
        event_hooks,
    })
}

#[derive(Debug)]
pub struct MobilePlaylistBridgeSession {
    coordinator: PlaylistCoordinator<InMemoryPreloadBudgetProvider, MobilePreloadExecutor>,
    command_queue: MobileCommandQueue<MobilePreloadCommand>,
}

impl MobilePlaylistBridgeSession {
    pub fn new(
        playlist_id: impl Into<String>,
        config: PlaylistCoordinatorConfig,
        preload_budget: PreloadBudget,
        label: &'static str,
    ) -> Self {
        let command_queue = MobileCommandQueue::new(label);
        let executor = MobilePreloadExecutor::new(command_queue.clone());

        Self {
            coordinator: PlaylistCoordinator::new(
                playlist_id,
                config,
                InMemoryPreloadBudgetProvider::new(preload_budget),
                executor,
            ),
            command_queue,
        }
    }

    pub fn replace_queue(
        &mut self,
        queue: impl IntoIterator<Item = PlaylistQueueItem>,
        now: Instant,
    ) {
        self.coordinator.replace_queue(queue, now);
    }

    pub fn update_viewport_hints(
        &mut self,
        hints: impl IntoIterator<Item = PlaylistViewportHint>,
        now: Instant,
    ) {
        self.coordinator.update_viewport_hints(hints, now);
    }

    pub fn clear_viewport_hints(&mut self, now: Instant) {
        self.coordinator.clear_viewport_hints(now);
    }

    pub fn advance_to_next(&mut self, now: Instant) -> PlaylistAdvanceDecision {
        self.coordinator.advance_to_next(now)
    }

    pub fn advance_to_previous(&mut self, now: Instant) -> PlaylistAdvanceDecision {
        self.coordinator.advance_to_previous(now)
    }

    pub fn handle_playback_completed(&mut self, now: Instant) -> PlaylistAdvanceDecision {
        self.coordinator.handle_playback_completed(now)
    }

    pub fn handle_playback_failed(&mut self, now: Instant) -> PlaylistAdvanceDecision {
        self.coordinator.handle_playback_failed(now)
    }

    pub fn complete_preload_task(
        &mut self,
        task_id: PreloadTaskId,
    ) -> PlayerResult<Option<PreloadTaskSnapshot>> {
        self.coordinator.complete_preload_task(task_id)
    }

    pub fn fail_preload_task(
        &mut self,
        task_id: PreloadTaskId,
        error: PlayerError,
    ) -> PlayerResult<Option<PreloadTaskSnapshot>> {
        self.coordinator.fail_preload_task(task_id, error)
    }

    pub fn active_item(&self) -> Option<PlaylistActiveItem> {
        self.coordinator.active_item()
    }

    pub fn snapshot(&self) -> PlaylistSnapshot {
        self.coordinator.snapshot()
    }

    pub fn drain_events(&mut self) -> Vec<PlaylistEvent> {
        self.coordinator.drain_events()
    }

    pub fn drain_preload_events(&mut self) -> Vec<PreloadEvent> {
        self.coordinator
            .drain_events()
            .into_iter()
            .filter_map(|event| match event {
                PlaylistEvent::Preload(preload) => Some(preload),
                _ => None,
            })
            .collect()
    }

    pub fn drain_commands(&mut self) -> Vec<MobilePreloadCommand> {
        self.command_queue.drain()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MobileSourceNormalizerConfiguration {
    pub mode: SourceNormalizerMode,
    pub plugin_library_paths: Vec<PathBuf>,
    pub runtime_profile: Option<String>,
}

impl Default for MobileSourceNormalizerConfiguration {
    fn default() -> Self {
        Self {
            mode: SourceNormalizerMode::Disabled,
            plugin_library_paths: Vec::new(),
            runtime_profile: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct PreflightDiagnosticCacheKey {
    source_uri: String,
    runtime_profile: Option<String>,
    mode: &'static str,
    plugin_paths: Vec<PreflightPluginPathFingerprint>,
}

impl PreflightDiagnosticCacheKey {
    fn from_source(
        source: &MediaSource,
        configuration: &MobileSourceNormalizerConfiguration,
    ) -> Self {
        Self {
            source_uri: source.uri().to_owned(),
            runtime_profile: configuration.runtime_profile.clone(),
            mode: source_normalizer_mode_cache_label(configuration.mode),
            plugin_paths: configuration
                .plugin_library_paths
                .iter()
                .map(PreflightPluginPathFingerprint::from_path)
                .collect(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct PreflightPluginPathFingerprint {
    path: String,
    len: Option<u64>,
    modified_ms: Option<u128>,
}

impl PreflightPluginPathFingerprint {
    fn from_path(path: &PathBuf) -> Self {
        let metadata = fs::metadata(path).ok();
        let len = metadata.as_ref().map(fs::Metadata::len);
        let modified_ms = metadata
            .and_then(|metadata| metadata.modified().ok())
            .and_then(|modified| modified.duration_since(UNIX_EPOCH).ok())
            .map(|duration| duration.as_millis());

        Self {
            path: path.display().to_string(),
            len,
            modified_ms,
        }
    }
}

#[derive(Debug, Clone)]
struct PreflightDiagnosticCacheEntry {
    diagnostic: PlayerPluginDiagnostic,
    inserted_at: Instant,
    ttl: Duration,
    original_ready_ms: Option<u128>,
}

#[derive(Debug)]
struct PreflightDiagnosticCache {
    capacity: usize,
    entries: HashMap<PreflightDiagnosticCacheKey, PreflightDiagnosticCacheEntry>,
    lru: VecDeque<PreflightDiagnosticCacheKey>,
}

impl PreflightDiagnosticCache {
    fn new(capacity: usize) -> Self {
        Self {
            capacity,
            entries: HashMap::new(),
            lru: VecDeque::new(),
        }
    }

    fn get(
        &mut self,
        key: &PreflightDiagnosticCacheKey,
        now: Instant,
    ) -> Option<PlayerPluginDiagnostic> {
        let Some(entry) = self.entries.get(key) else {
            return None;
        };
        let age = now.saturating_duration_since(entry.inserted_at);
        if age > entry.ttl {
            self.remove(key);
            return None;
        }

        let mut diagnostic = entry.diagnostic.clone();
        diagnostic
            .details
            .push(player_plugin_detail("cached", "true"));
        diagnostic.details.push(player_plugin_detail(
            "cacheAgeMs",
            age.as_millis().to_string(),
        ));
        diagnostic.details.push(player_plugin_detail(
            "originalReadyMs",
            entry
                .original_ready_ms
                .map(|value| value.to_string())
                .unwrap_or_else(|| "unknown".to_owned()),
        ));
        self.touch(key);
        Some(diagnostic)
    }

    fn insert(
        &mut self,
        key: PreflightDiagnosticCacheKey,
        diagnostic: PlayerPluginDiagnostic,
        now: Instant,
    ) {
        if self.capacity == 0 {
            return;
        }

        self.remove(&key);
        let ttl = if diagnostic.status == PlayerPluginDiagnosticStatus::SourceNormalizerSupported {
            SOURCE_NORMALIZER_PREFLIGHT_SUCCESS_TTL
        } else {
            SOURCE_NORMALIZER_PREFLIGHT_FAILURE_TTL
        };
        let original_ready_ms = diagnostic
            .message
            .as_deref()
            .and_then(parse_ready_ms_from_diagnostic_message);
        self.entries.insert(
            key.clone(),
            PreflightDiagnosticCacheEntry {
                diagnostic,
                inserted_at: now,
                ttl,
                original_ready_ms,
            },
        );
        self.lru.push_back(key);
        while self.entries.len() > self.capacity {
            let Some(oldest) = self.lru.pop_front() else {
                break;
            };
            self.entries.remove(&oldest);
        }
    }

    #[cfg(test)]
    fn len(&self) -> usize {
        self.entries.len()
    }

    #[cfg(test)]
    fn clear(&mut self) {
        self.entries.clear();
        self.lru.clear();
    }

    fn remove(&mut self, key: &PreflightDiagnosticCacheKey) {
        self.entries.remove(key);
        if let Some(index) = self.lru.iter().position(|candidate| candidate == key) {
            self.lru.remove(index);
        }
    }

    fn touch(&mut self, key: &PreflightDiagnosticCacheKey) {
        if let Some(index) = self.lru.iter().position(|candidate| candidate == key) {
            if let Some(key) = self.lru.remove(index) {
                self.lru.push_back(key);
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MobileFrameProcessorConfiguration {
    pub mode: FrameProcessorMode,
    pub plugin_library_paths: Vec<PathBuf>,
}

impl Default for MobileFrameProcessorConfiguration {
    fn default() -> Self {
        Self {
            mode: FrameProcessorMode::Disabled,
            plugin_library_paths: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MobileNativeFramePipelineConfiguration {
    pub mode: NativeFramePipelineMode,
    pub decoder_plugin_library_paths: Vec<PathBuf>,
    pub frame_processor_plugin_library_paths: Vec<PathBuf>,
    pub max_in_flight_frames: Option<u32>,
}

impl Default for MobileNativeFramePipelineConfiguration {
    fn default() -> Self {
        Self {
            mode: NativeFramePipelineMode::Disabled,
            decoder_plugin_library_paths: Vec::new(),
            frame_processor_plugin_library_paths: Vec::new(),
            max_in_flight_frames: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct MobilePluginConfiguration {
    pub source_normalizer: MobileSourceNormalizerConfiguration,
    pub frame_processor: MobileFrameProcessorConfiguration,
    pub native_frame_pipeline: MobileNativeFramePipelineConfiguration,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MobileSourceNormalizerRouteDecision {
    NativeFirst,
    Force,
}

pub struct MobileSourceNormalizerResourceOpen {
    pub plugin_name: Option<String>,
    pub plugin_path: String,
    pub session: Box<dyn SourceNormalizerResourceSession>,
    pub info: SourceNormalizerResourceSessionInfo,
    pub status: SourceNormalizerResourceSessionStatus,
    pub cache_policy: SourceNormalizerResourceCachePolicy,
    pub diagnostics: Vec<PlayerPluginDiagnostic>,
}

pub struct MobileSourceNormalizerResourceOpenOutcome {
    pub opened: Option<MobileSourceNormalizerResourceOpen>,
    pub diagnostics: Vec<PlayerPluginDiagnostic>,
}

pub struct MobileSourceNormalizerPacketOpen {
    pub plugin_name: Option<String>,
    pub plugin_path: String,
    pub session: Box<dyn SourceNormalizerPacketSession>,
    pub info: SourceNormalizerPacketStreamInfo,
    pub diagnostics: Vec<PlayerPluginDiagnostic>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MobileSourceNormalizerPlaybackDecision {
    pub action: MobileSourceNormalizerPlaybackAction,
    pub reason: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MobileSourceNormalizerPlaybackAction {
    BypassNativeFirst,
    TryNormalized,
    Disabled,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MobileSourceNormalizerResourceWire {
    pub handle: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub plugin_name: Option<String>,
    pub plugin_path: String,
    pub output_route: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub selected_profile: Option<String>,
    pub container: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub primary_resource_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub primary_content_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub playback_uri: Option<String>,
    pub resources: Vec<MobileSourceNormalizerResourceInfoWire>,
    pub state: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub disk_bytes_used: Option<u64>,
    pub cache_policy: MobileSourceNormalizerCachePolicyWire,
    pub route: String,
    pub participation: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fallback_reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache_quota: Option<u64>,
    pub diagnostics: Vec<MobilePluginDiagnosticOwnedWire>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MobileSourceNormalizerResourceInfoWire {
    pub role: String,
    pub path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub byte_length: Option<u64>,
    pub growing: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MobileSourceNormalizerCachePolicyWire {
    pub session_read_buffer_bytes: u64,
    pub manifest_snapshot_bytes: u64,
    pub session_disk_soft_cap_bytes: u64,
    pub global_disk_soft_cap_bytes: u64,
}

impl MobilePluginConfiguration {
    pub fn from_runtime_options(options: &PlayerRuntimeOptions) -> Self {
        Self {
            source_normalizer: MobileSourceNormalizerConfiguration {
                mode: options.source_normalizer_mode,
                plugin_library_paths: options.source_normalizer_plugin_library_paths.clone(),
                runtime_profile: None,
            },
            frame_processor: MobileFrameProcessorConfiguration {
                mode: options.frame_processor_mode,
                plugin_library_paths: options.frame_processor_library_paths.clone(),
            },
            native_frame_pipeline: MobileNativeFramePipelineConfiguration {
                mode: match options.decoder_plugin_video_mode {
                    player_runtime::PlayerDecoderPluginVideoMode::PreferNativeFrame => {
                        NativeFramePipelineMode::PreferNativeFrame
                    }
                    player_runtime::PlayerDecoderPluginVideoMode::DiagnosticsOnly => {
                        if options.decoder_plugin_library_paths.is_empty()
                            && options.frame_processor_library_paths.is_empty()
                        {
                            NativeFramePipelineMode::Disabled
                        } else {
                            NativeFramePipelineMode::DiagnosticsOnly
                        }
                    }
                },
                decoder_plugin_library_paths: options.decoder_plugin_library_paths.clone(),
                frame_processor_plugin_library_paths: options.frame_processor_library_paths.clone(),
                max_in_flight_frames: Some(
                    options
                        .frame_processor_policy
                        .max_in_flight_frames_per_processor,
                ),
            },
        }
    }

    pub fn apply_to_runtime_options(&self, options: &mut PlayerRuntimeOptions) {
        options.source_normalizer_mode = self.source_normalizer.mode;
        options.source_normalizer_plugin_library_paths =
            self.source_normalizer.plugin_library_paths.clone();
        options.frame_processor_mode = self.frame_processor.mode;
        options.frame_processor_library_paths = self.frame_processor.plugin_library_paths.clone();
        options.decoder_plugin_video_mode = match self.native_frame_pipeline.mode {
            NativeFramePipelineMode::PreferNativeFrame
            | NativeFramePipelineMode::RequireNativeFrame => {
                player_runtime::PlayerDecoderPluginVideoMode::PreferNativeFrame
            }
            NativeFramePipelineMode::Disabled | NativeFramePipelineMode::DiagnosticsOnly => {
                player_runtime::PlayerDecoderPluginVideoMode::DiagnosticsOnly
            }
        };
        if !self
            .native_frame_pipeline
            .decoder_plugin_library_paths
            .is_empty()
        {
            options.decoder_plugin_library_paths = self
                .native_frame_pipeline
                .decoder_plugin_library_paths
                .clone();
        }
        if !self
            .native_frame_pipeline
            .frame_processor_plugin_library_paths
            .is_empty()
        {
            options.frame_processor_library_paths = self
                .native_frame_pipeline
                .frame_processor_plugin_library_paths
                .clone();
        }
        if let Some(max_in_flight_frames) = self.native_frame_pipeline.max_in_flight_frames {
            options
                .frame_processor_policy
                .max_in_flight_frames_per_processor = max_in_flight_frames.max(1);
        }
    }
}

pub fn apply_mobile_plugin_diagnostics(
    mut startup: PlayerRuntimeStartup,
    source: &MediaSource,
    configuration: &MobilePluginConfiguration,
) -> PlayerRuntimeStartup {
    startup
        .plugin_diagnostics
        .extend(source_normalizer_diagnostics(
            source,
            &configuration.source_normalizer,
        ));
    startup
        .plugin_diagnostics
        .extend(frame_processor_diagnostics(&configuration.frame_processor));
    startup
        .plugin_diagnostics
        .extend(native_frame_pipeline_diagnostics(
            &configuration.native_frame_pipeline,
        ));
    startup
}

/// Opens a normalized-resource source normalizer session.
///
/// This helper may synchronously wait for source-normalizer resource readiness
/// updates. Mobile hosts should call it from a worker thread, or wrap it in
/// platform async scheduling before returning to UI code.
pub fn open_mobile_source_normalizer_resource(
    source: &MediaSource,
    configuration: &MobileSourceNormalizerConfiguration,
    output_root: impl Into<String>,
    decision: MobileSourceNormalizerRouteDecision,
) -> Result<Option<MobileSourceNormalizerResourceOpen>, String> {
    open_mobile_source_normalizer_resource_with_diagnostics(
        source,
        configuration,
        output_root,
        decision,
    )
    .map(|outcome| outcome.opened)
}

/// Opens a normalized-resource source normalizer session with diagnostics.
///
/// This helper may synchronously wait for source-normalizer resource readiness
/// updates. Mobile hosts should call it from a worker thread, or wrap it in
/// platform async scheduling before returning to UI code.
pub fn open_mobile_source_normalizer_resource_with_diagnostics(
    source: &MediaSource,
    configuration: &MobileSourceNormalizerConfiguration,
    output_root: impl Into<String>,
    decision: MobileSourceNormalizerRouteDecision,
) -> Result<MobileSourceNormalizerResourceOpenOutcome, String> {
    let playback_decision =
        mobile_source_normalizer_playback_decision(source, configuration, decision);
    if playback_decision.action == MobileSourceNormalizerPlaybackAction::Disabled {
        return Ok(MobileSourceNormalizerResourceOpenOutcome {
            opened: None,
            diagnostics: Vec::new(),
        });
    }
    if playback_decision.action == MobileSourceNormalizerPlaybackAction::BypassNativeFirst {
        return Ok(MobileSourceNormalizerResourceOpenOutcome {
            opened: None,
            diagnostics: Vec::new(),
        });
    }

    let output_root = output_root.into();
    let mut diagnostics = Vec::new();
    if configuration.plugin_library_paths.is_empty() {
        let message = "source normalizer normalized-resource open skipped because no plugin paths were provided";
        diagnostics.push(runtime_source_normalizer_diagnostic(
            String::new(),
            None,
            PlayerPluginDiagnosticStatus::SourceNormalizerUnsupported,
            message,
            PlayerPluginParticipation::Bypassed,
        ));
        return match configuration.mode {
            SourceNormalizerMode::RequireNormalized => Err(message.to_owned()),
            _ => Ok(MobileSourceNormalizerResourceOpenOutcome {
                opened: None,
                diagnostics,
            }),
        };
    }

    let registry =
        PluginRegistry::inspect_source_normalizer_support(&configuration.plugin_library_paths);
    diagnostics.extend(
        registry
            .records()
            .iter()
            .map(|record| diagnostic_from_record(record, source_normalizer_participation(record))),
    );
    let Some(record) = best_mobile_source_normalizer_resource(&registry, configuration) else {
        let message = source_normalizer_resource_selection_failure_message(
            "normalized-resource open",
            &registry,
            configuration,
        );
        diagnostics.push(runtime_source_normalizer_diagnostic(
            String::new(),
            None,
            PlayerPluginDiagnosticStatus::SourceNormalizerUnsupported,
            message.clone(),
            PlayerPluginParticipation::Bypassed,
        ));
        return match configuration.mode {
            SourceNormalizerMode::RequireNormalized => Err(message),
            _ => Ok(MobileSourceNormalizerResourceOpenOutcome {
                opened: None,
                diagnostics,
            }),
        };
    };

    match open_source_normalizer_resource_session(source, configuration, output_root, record) {
        Ok(mut opened) => {
            opened.diagnostics.splice(0..0, diagnostics);
            if let Some(reason) = hdr_resource_metadata_not_preserved_reason(&opened.info) {
                opened
                    .diagnostics
                    .push(runtime_source_normalizer_diagnostic(
                        record.path.display().to_string(),
                        opened
                            .plugin_name
                            .clone()
                            .or_else(|| record.plugin_name.clone()),
                        PlayerPluginDiagnosticStatus::SourceNormalizerUnsupported,
                        reason.clone(),
                        PlayerPluginParticipation::Bypassed,
                    ));
                if let Err(error) = opened.session.close() {
                    opened.diagnostics.push(runtime_source_normalizer_diagnostic(
                        record.path.display().to_string(),
                        opened.plugin_name.clone().or_else(|| record.plugin_name.clone()),
                        PlayerPluginDiagnosticStatus::SourceNormalizerUnsupported,
                        format!(
                            "source normalizer normalized-resource close after HDR bypass failed: {error}"
                        ),
                        PlayerPluginParticipation::Bypassed,
                    ));
                }
                return match configuration.mode {
                    SourceNormalizerMode::RequireNormalized => Err(reason),
                    _ => Ok(MobileSourceNormalizerResourceOpenOutcome {
                        opened: None,
                        diagnostics: opened.diagnostics,
                    }),
                };
            }
            Ok(MobileSourceNormalizerResourceOpenOutcome {
                opened: Some(opened),
                diagnostics: Vec::new(),
            })
        }
        Err(error) => {
            diagnostics.push(runtime_source_normalizer_diagnostic(
                record.path.display().to_string(),
                record.plugin_name.clone(),
                PlayerPluginDiagnosticStatus::SourceNormalizerUnsupported,
                format!(
                    "source normalizer normalized-resource open failed; route={}; error={error}",
                    PlayerPlaybackRoute::SystemPlayer.wire_name()
                ),
                PlayerPluginParticipation::Bypassed,
            ));
            match configuration.mode {
                SourceNormalizerMode::RequireNormalized => Err(error),
                _ => Ok(MobileSourceNormalizerResourceOpenOutcome {
                    opened: None,
                    diagnostics,
                }),
            }
        }
    }
}

pub const HDR_PROGRAMMABLE_PROCESSING_NOT_SUPPORTED: &str = "hdrProgrammableProcessingNotSupported";
const HDR_RESOURCE_METADATA_NOT_PRESERVED: &str = "HdrResourceMetadataNotPreserved";

pub fn hdr_programmable_processing_not_supported_reason(
    track: &SourceNormalizerPacketTrackInfo,
) -> Option<String> {
    if !track_requires_hdr_or_dolby_vision_metadata(track) {
        return None;
    }
    Some(format!(
        "{HDR_PROGRAMMABLE_PROCESSING_NOT_SUPPORTED}: HDR10, HLG, Dolby Vision, and 10-bit sources use system playback; SDK-managed native-frame processing is SDR-only"
    ))
}

fn hdr_resource_metadata_not_preserved_reason(
    info: &SourceNormalizerResourceSessionInfo,
) -> Option<String> {
    if info.output_route != SourceNormalizerOutputRoute::Fmp4LocalStream {
        return None;
    }
    let track = info
        .tracks
        .iter()
        .find(|track| track.media_kind == SourceNormalizerPacketMediaKind::Video)?;
    if !track_requires_hdr_or_dolby_vision_metadata(track) {
        return None;
    }
    Some(format!(
        "{HDR_RESOURCE_METADATA_NOT_PRESERVED}: source normalizer fMP4 resource route cannot currently guarantee HDR/Dolby Vision metadata preservation for system playback"
    ))
}

pub fn track_requires_hdr_or_dolby_vision_metadata(
    track: &SourceNormalizerPacketTrackInfo,
) -> bool {
    codec_is_dolby_vision(&track.codec)
        || track.hdr.as_ref().is_some_and(hdr_requires_preservation)
        || track
            .color
            .as_ref()
            .is_some_and(color_requires_hdr_preservation)
}

fn hdr_requires_preservation(hdr: &NativeFrameHdrMetadata) -> bool {
    hdr.is_dolby_vision() || !hdr.kind.trim().is_empty()
}

fn color_requires_hdr_preservation(color: &NativeFrameColorMetadata) -> bool {
    color.bit_depth.is_some_and(|bit_depth| bit_depth >= 10) || color.is_hdr_transfer()
}

fn codec_is_dolby_vision(codec: &str) -> bool {
    codec
        .split(',')
        .map(|value| value.trim().to_ascii_lowercase())
        .any(|value| {
            let normalized = value.strip_prefix("video/").unwrap_or(&value);
            normalized.starts_with("dvh1") || normalized.starts_with("dvhe")
        })
}

pub fn open_mobile_source_normalizer_packet_session(
    source: &MediaSource,
    configuration: &MobileSourceNormalizerConfiguration,
) -> Result<MobileSourceNormalizerPacketOpen, String> {
    let registry =
        PluginRegistry::inspect_source_normalizer_support(&configuration.plugin_library_paths);
    let Some(record) = registry.best_source_normalizer_packet() else {
        return Err(format!(
            "no packet-stream source normalizer plugin is available{}",
            mobile_source_normalizer_registry_notes(&registry)
        ));
    };
    open_source_normalizer_packet_session(source, configuration, record)
}

pub fn mobile_source_normalizer_playback_decision(
    source: &MediaSource,
    configuration: &MobileSourceNormalizerConfiguration,
    decision: MobileSourceNormalizerRouteDecision,
) -> MobileSourceNormalizerPlaybackDecision {
    if !matches!(
        configuration.mode,
        SourceNormalizerMode::PreferNormalized | SourceNormalizerMode::RequireNormalized
    ) {
        return MobileSourceNormalizerPlaybackDecision {
            action: MobileSourceNormalizerPlaybackAction::Disabled,
            reason: "source normalizer normalized playback is disabled",
        };
    }

    if decision == MobileSourceNormalizerRouteDecision::NativeFirst
        && native_first_source_normalizer_bypass(source, configuration)
        && configuration.mode != SourceNormalizerMode::RequireNormalized
    {
        return MobileSourceNormalizerPlaybackDecision {
            action: MobileSourceNormalizerPlaybackAction::BypassNativeFirst,
            reason: "standard source stays native-first",
        };
    }

    MobileSourceNormalizerPlaybackDecision {
        action: MobileSourceNormalizerPlaybackAction::TryNormalized,
        reason: "normalized playback requested",
    }
}

pub fn mobile_source_normalizer_resource_open_json(
    handle: u64,
    opened: &MobileSourceNormalizerResourceOpen,
    playback_uri: Option<String>,
) -> Result<String, serde_json::Error> {
    serde_json::to_string(&MobileSourceNormalizerResourceWire::from_open(
        handle,
        opened,
        playback_uri,
    ))
}

pub fn mobile_source_normalizer_resource_status_json(
    handle: u64,
    opened: &MobileSourceNormalizerResourceOpen,
    playback_uri: Option<String>,
) -> Result<String, serde_json::Error> {
    serde_json::to_string(&MobileSourceNormalizerResourceWire::from_open(
        handle,
        opened,
        playback_uri,
    ))
}

pub fn mobile_source_normalizer_resource_bypass_diagnostics_json(
    diagnostics: &[PlayerPluginDiagnostic],
) -> Result<String, serde_json::Error> {
    serde_json::to_string(
        &diagnostics
            .iter()
            .map(MobilePluginDiagnosticOwnedWire::from)
            .collect::<Vec<_>>(),
    )
}

pub fn source_normalizer_diagnostics(
    source: &MediaSource,
    configuration: &MobileSourceNormalizerConfiguration,
) -> Vec<PlayerPluginDiagnostic> {
    if configuration.mode == SourceNormalizerMode::Disabled
        && configuration.plugin_library_paths.is_empty()
    {
        return Vec::new();
    }

    if configuration.plugin_library_paths.is_empty() {
        return vec![runtime_source_normalizer_diagnostic(
            String::new(),
            None,
            PlayerPluginDiagnosticStatus::SourceNormalizerUnsupported,
            "source normalizer mobile configuration is enabled, but no plugin paths were provided",
            PlayerPluginParticipation::Unknown,
        )];
    }

    let registry =
        PluginRegistry::inspect_source_normalizer_support(&configuration.plugin_library_paths);
    let mut diagnostics = registry
        .records()
        .iter()
        .map(|record| diagnostic_from_record(record, source_normalizer_participation(record)))
        .collect::<Vec<_>>();

    if configuration.mode == SourceNormalizerMode::PreferNormalized
        || configuration.mode == SourceNormalizerMode::RequireNormalized
    {
        let decision = mobile_source_normalizer_playback_decision(
            source,
            configuration,
            MobileSourceNormalizerRouteDecision::NativeFirst,
        );
        if decision.action == MobileSourceNormalizerPlaybackAction::BypassNativeFirst {
            diagnostics.push(runtime_source_normalizer_diagnostic(
                String::new(),
                None,
                PlayerPluginDiagnosticStatus::SourceNormalizerSupported,
                format!(
                    "source normalizer normalized-resource probe bypassed; route={}; fallbackReason={}",
                    PlayerPlaybackRoute::SystemPlayer.wire_name(),
                    decision.reason
                ),
                PlayerPluginParticipation::Bypassed,
            ));
            return diagnostics;
        }

        let Some(record) = best_mobile_source_normalizer_resource(&registry, configuration) else {
            diagnostics.push(runtime_source_normalizer_diagnostic(
                String::new(),
                None,
                PlayerPluginDiagnosticStatus::SourceNormalizerUnsupported,
                source_normalizer_resource_selection_failure_message(
                    "resource probe",
                    &registry,
                    configuration,
                ),
                PlayerPluginParticipation::Bypassed,
            ));
            return diagnostics;
        };

        diagnostics.push(probe_source_normalizer_resource(
            source,
            configuration,
            record,
        ));
        return diagnostics;
    }

    if configuration.mode != SourceNormalizerMode::PreflightOnly {
        return diagnostics;
    }

    let Some(record) = registry.best_source_normalizer_packet() else {
        diagnostics.push(runtime_source_normalizer_diagnostic(
            String::new(),
            None,
            PlayerPluginDiagnosticStatus::SourceNormalizerUnsupported,
            "source normalizer preflight skipped because no packet-stream source normalizer plugin is available",
            PlayerPluginParticipation::Unknown,
        ));
        return diagnostics;
    };

    diagnostics.push(cached_preflight_source_normalizer(
        source,
        configuration,
        record,
    ));
    diagnostics
}

pub fn frame_processor_diagnostics(
    configuration: &MobileFrameProcessorConfiguration,
) -> Vec<PlayerPluginDiagnostic> {
    if configuration.mode == FrameProcessorMode::Disabled
        && configuration.plugin_library_paths.is_empty()
    {
        return Vec::new();
    }

    if configuration.plugin_library_paths.is_empty() {
        return vec![runtime_frame_processor_diagnostic(
            String::new(),
            None,
            "frame processor mobile diagnostics are enabled, but no plugin paths were provided",
            PlayerPluginParticipation::Unknown,
        )];
    }

    PluginRegistry::inspect_frame_processor_support(&configuration.plugin_library_paths)
        .records()
        .iter()
        .map(|record| diagnostic_from_record(record, frame_processor_participation(record)))
        .collect()
}

pub fn native_frame_pipeline_diagnostics(
    configuration: &MobileNativeFramePipelineConfiguration,
) -> Vec<PlayerPluginDiagnostic> {
    if configuration.mode == NativeFramePipelineMode::Disabled
        && configuration.decoder_plugin_library_paths.is_empty()
        && configuration
            .frame_processor_plugin_library_paths
            .is_empty()
    {
        return Vec::new();
    }

    let participation = match configuration.mode {
        NativeFramePipelineMode::PreferNativeFrame
        | NativeFramePipelineMode::RequireNativeFrame => PlayerPluginParticipation::Selected,
        NativeFramePipelineMode::DiagnosticsOnly => PlayerPluginParticipation::Available,
        NativeFramePipelineMode::Disabled => PlayerPluginParticipation::Available,
    };
    let message = match configuration.mode {
        NativeFramePipelineMode::Disabled => {
            "mobile native-frame pipeline is disabled; system player remains selected"
        }
        NativeFramePipelineMode::DiagnosticsOnly => {
            "mobile native-frame pipeline diagnostics are enabled; playback still uses the system player"
        }
        NativeFramePipelineMode::PreferNativeFrame => {
            "mobile native-frame pipeline is explicitly preferred; selected sdkManagedNativeFrame route when platform requirements are available"
        }
        NativeFramePipelineMode::RequireNativeFrame => {
            "mobile native-frame pipeline is explicitly required; sdkManagedNativeFrame route must fail visibly when unavailable"
        }
    };
    let route = match configuration.mode {
        NativeFramePipelineMode::PreferNativeFrame
        | NativeFramePipelineMode::RequireNativeFrame => {
            PlayerPlaybackRoute::SdkManagedNativeFrame.wire_name()
        }
        NativeFramePipelineMode::Disabled | NativeFramePipelineMode::DiagnosticsOnly => {
            PlayerPlaybackRoute::SystemPlayer.wire_name()
        }
    };

    let path = configuration
        .decoder_plugin_library_paths
        .iter()
        .chain(configuration.frame_processor_plugin_library_paths.iter())
        .map(|path| path.display().to_string())
        .collect::<Vec<_>>()
        .join(std::path::MAIN_SEPARATOR_STR);
    vec![PlayerPluginDiagnostic {
        path,
        plugin_name: Some("vesper-mobile-native-frame-pipeline".to_owned()),
        plugin_kind: Some("native_frame_pipeline".to_owned()),
        status: PlayerPluginDiagnosticStatus::Loaded,
        message: Some(format!(
            "{message}; route={route}; decoder_plugins={}; frame_processors={}; max_in_flight_frames={}",
            configuration.decoder_plugin_library_paths.len(),
            configuration.frame_processor_plugin_library_paths.len(),
            configuration
                .max_in_flight_frames
                .map(|value| value.to_string())
                .unwrap_or_else(|| "default".to_owned())
        )),
        capability: None,
        participation,
        details: vec![
            player_plugin_detail("route", route),
            player_plugin_detail(
                "decoderPlugins",
                configuration.decoder_plugin_library_paths.len().to_string(),
            ),
            player_plugin_detail(
                "frameProcessors",
                configuration
                    .frame_processor_plugin_library_paths
                    .len()
                    .to_string(),
            ),
            player_plugin_detail(
                "maxInFlightFrames",
                configuration
                    .max_in_flight_frames
                    .map(|value| value.to_string())
                    .unwrap_or_else(|| "default".to_owned()),
            ),
        ],
    }]
}

fn preflight_source_normalizer(
    source: &MediaSource,
    configuration: &MobileSourceNormalizerConfiguration,
    record: &PluginDiagnosticRecord,
) -> PlayerPluginDiagnostic {
    let started = Instant::now();
    let mut opened = match open_source_normalizer_packet_session(source, configuration, record) {
        Ok(opened) => opened,
        Err(error) => {
            let status = if error.contains("load failed") {
                PlayerPluginDiagnosticStatus::LoadFailed
            } else {
                PlayerPluginDiagnosticStatus::SourceNormalizerUnsupported
            };
            return runtime_source_normalizer_diagnostic(
                record.path.display().to_string(),
                record.plugin_name.clone(),
                status,
                format!("source normalizer preflight open failed: {error}"),
                PlayerPluginParticipation::Bypassed,
            );
        }
    };
    let stream_info = opened.info.clone();
    let close_message = match opened.session.close() {
        Ok(()) => None,
        Err(error) => Some(format!("; close failed: {error}")),
    };
    let track_summary = stream_info
        .tracks
        .iter()
        .map(|track| format!("{}:{}", media_kind_label(track.media_kind), track.codec))
        .collect::<Vec<_>>();
    runtime_source_normalizer_diagnostic(
        opened.plugin_path.clone(),
        stream_info
            .normalizer_name
            .clone()
            .or_else(|| opened.plugin_name.clone()),
        PlayerPluginDiagnosticStatus::SourceNormalizerSupported,
        format!(
            "source normalizer preflight opened and closed packet session; profile={}; tracks={}; ready_ms={}{}",
            stream_info
                .runtime_profile
                .as_deref()
                .unwrap_or("auto-detected"),
            if track_summary.is_empty() {
                "none".to_owned()
            } else {
                track_summary.join(",")
            },
            started.elapsed().as_millis(),
            close_message.unwrap_or_default()
        ),
        PlayerPluginParticipation::Bypassed,
    )
}

fn cached_preflight_source_normalizer(
    source: &MediaSource,
    configuration: &MobileSourceNormalizerConfiguration,
    record: &PluginDiagnosticRecord,
) -> PlayerPluginDiagnostic {
    let key = PreflightDiagnosticCacheKey::from_source(source, configuration);
    let now = Instant::now();
    if let Some(diagnostic) = SOURCE_NORMALIZER_PREFLIGHT_CACHE
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .get(&key, now)
    {
        return diagnostic;
    }

    let diagnostic = preflight_source_normalizer(source, configuration, record);
    SOURCE_NORMALIZER_PREFLIGHT_CACHE
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .insert(key, diagnostic.clone(), Instant::now());
    diagnostic
}

#[cfg(test)]
fn cached_preflight_diagnostic_with(
    cache: &mut PreflightDiagnosticCache,
    key: PreflightDiagnosticCacheKey,
    now: Instant,
    open: impl FnOnce() -> PlayerPluginDiagnostic,
) -> PlayerPluginDiagnostic {
    if let Some(diagnostic) = cache.get(&key, now) {
        return diagnostic;
    }

    let diagnostic = open();
    cache.insert(key, diagnostic.clone(), now);
    diagnostic
}

fn open_source_normalizer_packet_session(
    source: &MediaSource,
    configuration: &MobileSourceNormalizerConfiguration,
    record: &PluginDiagnosticRecord,
) -> Result<MobileSourceNormalizerPacketOpen, String> {
    let path = record.path.display().to_string();
    let plugin = LoadedDynamicPlugin::load(&record.path)
        .map_err(|error| format!("source normalizer packet load failed: {error}"))?;
    let factory = plugin
        .source_normalizer_packet_plugin_factory()
        .ok_or_else(|| {
            format!(
                "{} is not a packet-stream source normalizer plugin",
                plugin.plugin_name()
            )
        })?;
    let runtime_profile = configuration.runtime_profile.clone().unwrap_or_default();
    let requirements = SourceNormalizerPacketSessionRequirements {
        runtime_profile: runtime_profile.clone(),
        media_kind: Some(SourceNormalizerPacketMediaKind::Video),
        codec: None,
        bitstream_format: None,
        require_seek: false,
        require_flush: true,
        require_lease_cleanup: true,
    };
    let missing_capabilities = requirements.missing_capabilities(&factory.packet_capabilities());
    if !missing_capabilities.is_empty() {
        return Err(format!(
            "source normalizer packet plugin `{}` does not satisfy session requirements: missing {}",
            factory.name(),
            missing_capabilities.join(", ")
        ));
    }
    let config = SourceNormalizerPacketSessionConfig {
        runtime_profile,
        input: source.uri().to_owned(),
        headers: Vec::new(),
        startup_timeout_ms: Some(SOURCE_NORMALIZER_STARTUP_TIMEOUT.as_millis() as u64),
        session_timeout_ms: Some(SOURCE_NORMALIZER_SESSION_TIMEOUT.as_millis() as u64),
        preferred_media_kind: SourceNormalizerPacketMediaKind::Video,
    };
    let session = factory
        .open_packet_session(&config)
        .map_err(|error| format!("open_packet_session failed: {error}"))?;
    let info = session.stream_info();
    let diagnostic = runtime_source_normalizer_diagnostic(
        path.clone(),
        info.normalizer_name
            .clone()
            .or_else(|| Some(factory.name().to_owned())),
        PlayerPluginDiagnosticStatus::SourceNormalizerSupported,
        format!(
            "source normalizer packet session opened; profile={}; tracks={}",
            info.runtime_profile.as_deref().unwrap_or("auto-detected"),
            if info.tracks.is_empty() {
                "none".to_owned()
            } else {
                info.tracks
                    .iter()
                    .map(|track| format!("{}:{}", media_kind_label(track.media_kind), track.codec))
                    .collect::<Vec<_>>()
                    .join(",")
            }
        ),
        PlayerPluginParticipation::Selected,
    );
    Ok(MobileSourceNormalizerPacketOpen {
        plugin_name: info
            .normalizer_name
            .clone()
            .or_else(|| Some(factory.name().to_owned())),
        plugin_path: path,
        session,
        info,
        diagnostics: vec![diagnostic],
    })
}

fn probe_source_normalizer_resource(
    source: &MediaSource,
    configuration: &MobileSourceNormalizerConfiguration,
    record: &PluginDiagnosticRecord,
) -> PlayerPluginDiagnostic {
    let started = Instant::now();
    let path = record.path.display().to_string();
    let plugin = match LoadedDynamicPlugin::load(&record.path) {
        Ok(plugin) => plugin,
        Err(error) => {
            return runtime_source_normalizer_diagnostic(
                path,
                record.plugin_name.clone(),
                PlayerPluginDiagnosticStatus::LoadFailed,
                format!("source normalizer resource probe load failed: {error}"),
                PlayerPluginParticipation::Bypassed,
            );
        }
    };
    let Some(factory) = plugin.source_normalizer_resource_plugin_factory() else {
        return runtime_source_normalizer_diagnostic(
            path,
            Some(plugin.plugin_name().to_owned()),
            PlayerPluginDiagnosticStatus::SourceNormalizerUnsupported,
            format!(
                "{} is not a resource-output source normalizer plugin",
                plugin.plugin_name()
            ),
            PlayerPluginParticipation::Bypassed,
        );
    };

    let runtime_profile = configuration.runtime_profile.clone().unwrap_or_default();
    let preferred_route = preferred_resource_route_for_source(source);
    let requirements = SourceNormalizerResourceSessionRequirements {
        runtime_profile: runtime_profile.clone(),
        output_route: preferred_route.unwrap_or(SourceNormalizerOutputRoute::Fmp4LocalStream),
        content_type: None,
        require_growing_resources: false,
        require_range_reads: true,
        require_cancel: true,
    };
    let missing_capabilities = requirements.missing_capabilities(&factory.resource_capabilities());
    if !missing_capabilities.is_empty() {
        return runtime_source_normalizer_diagnostic(
            path,
            Some(factory.name().to_owned()),
            PlayerPluginDiagnosticStatus::SourceNormalizerUnsupported,
            format!(
                "source normalizer resource probe requirements failed: missing {}",
                missing_capabilities.join(", ")
            ),
            PlayerPluginParticipation::Bypassed,
        );
    }
    let output_root = std::env::temp_dir()
        .join("vesper-source-normalizer-probe")
        .display()
        .to_string();
    let config = SourceNormalizerResourceSessionConfig {
        runtime_profile,
        input: source.uri().to_owned(),
        headers: Vec::new(),
        output_root,
        cache_policy: SourceNormalizerResourceCachePolicy::default(),
        preferred_route,
        startup_timeout_ms: Some(SOURCE_NORMALIZER_STARTUP_TIMEOUT.as_millis() as u64),
        read_idle_timeout_ms: Some(SOURCE_NORMALIZER_SESSION_TIMEOUT.as_millis() as u64),
    };
    let mut session = match factory.open_resource_session(&config) {
        Ok(session) => session,
        Err(error) => {
            return runtime_source_normalizer_diagnostic(
                path,
                Some(factory.name().to_owned()),
                PlayerPluginDiagnosticStatus::SourceNormalizerUnsupported,
                format!(
                    "source normalizer resource probe open failed; route={}; error={error}",
                    PlayerPlaybackRoute::SystemPlayer.wire_name()
                ),
                PlayerPluginParticipation::Bypassed,
            );
        }
    };
    let session_info = session.session_info();
    let metadata_preservation_reason = hdr_resource_metadata_not_preserved_reason(&session_info);
    let poll_summary = match session.poll() {
        Ok(status) => {
            let route = status
                .info
                .as_ref()
                .map(|info| info.output_route.wire_name())
                .unwrap_or_else(|| session_info.output_route.wire_name());
            format!("state={:?}; route={route}", status.state)
        }
        Err(error) => format!("poll failed: {error}"),
    };
    let close_message = match session.close() {
        Ok(()) => None,
        Err(error) => Some(format!("; close failed: {error}")),
    };
    if let Some(reason) = metadata_preservation_reason {
        return runtime_source_normalizer_diagnostic(
            path,
            session_info
                .normalizer_name
                .clone()
                .or_else(|| Some(factory.name().to_owned())),
            PlayerPluginDiagnosticStatus::SourceNormalizerUnsupported,
            format!(
                "{reason}; {poll_summary}{}",
                close_message.unwrap_or_default()
            ),
            PlayerPluginParticipation::Bypassed,
        );
    }
    let participation = match session_info.output_route {
        SourceNormalizerOutputRoute::PacketStream => PlayerPluginParticipation::Bypassed,
        SourceNormalizerOutputRoute::Fmp4LocalStream
        | SourceNormalizerOutputRoute::HlsShortWindow => {
            if configuration.mode == SourceNormalizerMode::RequireNormalized {
                PlayerPluginParticipation::Selected
            } else {
                PlayerPluginParticipation::Bypassed
            }
        }
    };

    runtime_source_normalizer_diagnostic(
        path,
        session_info
            .normalizer_name
            .clone()
            .or_else(|| Some(factory.name().to_owned())),
        PlayerPluginDiagnosticStatus::SourceNormalizerSupported,
        format!(
            "source normalizer resource probe opened disk-backed session; route={}; profile={}; container={}; content_type={}; {}; ready_ms={}{}",
            session_info.output_route.wire_name(),
            session_info
                .runtime_profile
                .as_deref()
                .unwrap_or("auto-detected"),
            session_info.container,
            session_info
                .primary_content_type
                .as_deref()
                .unwrap_or("unknown"),
            poll_summary,
            started.elapsed().as_millis(),
            close_message.unwrap_or_default()
        ),
        participation,
    )
}

fn open_source_normalizer_resource_session(
    source: &MediaSource,
    configuration: &MobileSourceNormalizerConfiguration,
    output_root: String,
    record: &PluginDiagnosticRecord,
) -> Result<MobileSourceNormalizerResourceOpen, String> {
    let started = Instant::now();
    let path = record.path.display().to_string();
    let plugin = LoadedDynamicPlugin::load(&record.path)
        .map_err(|error| format!("source normalizer resource load failed: {error}"))?;
    let factory = plugin
        .source_normalizer_resource_plugin_factory()
        .ok_or_else(|| {
            format!(
                "{} is not a resource-output source normalizer plugin",
                plugin.plugin_name()
            )
        })?;
    let runtime_profile = configuration.runtime_profile.clone().unwrap_or_default();
    let preferred_route = preferred_resource_route_for_source(source);
    let requirements = SourceNormalizerResourceSessionRequirements {
        runtime_profile: runtime_profile.clone(),
        output_route: preferred_route.unwrap_or(SourceNormalizerOutputRoute::Fmp4LocalStream),
        content_type: None,
        require_growing_resources: false,
        require_range_reads: true,
        require_cancel: true,
    };
    let missing_capabilities = requirements.missing_capabilities(&factory.resource_capabilities());
    if !missing_capabilities.is_empty() {
        return Err(format!(
            "source normalizer resource plugin `{}` does not satisfy session requirements: missing {}",
            factory.name(),
            missing_capabilities.join(", ")
        ));
    }
    let config = SourceNormalizerResourceSessionConfig {
        runtime_profile,
        input: source.uri().to_owned(),
        headers: Vec::new(),
        output_root,
        cache_policy: SourceNormalizerResourceCachePolicy::default(),
        preferred_route,
        startup_timeout_ms: Some(SOURCE_NORMALIZER_STARTUP_TIMEOUT.as_millis() as u64),
        read_idle_timeout_ms: Some(SOURCE_NORMALIZER_SESSION_TIMEOUT.as_millis() as u64),
    };
    let mut session = factory
        .open_resource_session(&config)
        .map_err(|error| format!("open_resource_session failed: {error}"))?;
    let mut status = wait_for_resource_session_ready(session.as_mut())?;
    let info = status
        .info
        .clone()
        .unwrap_or_else(|| session.session_info());
    let participation = match info.output_route {
        SourceNormalizerOutputRoute::PacketStream => PlayerPluginParticipation::Bypassed,
        SourceNormalizerOutputRoute::Fmp4LocalStream
        | SourceNormalizerOutputRoute::HlsShortWindow => PlayerPluginParticipation::Selected,
    };
    let diagnostic = runtime_source_normalizer_diagnostic(
        path.clone(),
        info.normalizer_name
            .clone()
            .or_else(|| Some(factory.name().to_owned())),
        PlayerPluginDiagnosticStatus::SourceNormalizerSupported,
        format!(
            "source normalizer normalized-resource opened; route={}; profile={}; container={}; content_type={}; disk_bytes={}; ready_ms={}",
            info.output_route.wire_name(),
            info.runtime_profile.as_deref().unwrap_or("auto-detected"),
            info.container,
            info.primary_content_type.as_deref().unwrap_or("unknown"),
            status
                .disk_bytes_used
                .or(info.disk_bytes_used)
                .map(|bytes| bytes.to_string())
                .unwrap_or_else(|| "unknown".to_owned()),
            started.elapsed().as_millis()
        ),
        participation,
    );
    status.info = Some(info.clone());
    Ok(MobileSourceNormalizerResourceOpen {
        plugin_name: info.normalizer_name.clone(),
        plugin_path: path,
        session,
        info,
        status,
        cache_policy: SourceNormalizerResourceCachePolicy::default(),
        diagnostics: vec![diagnostic],
    })
}

fn wait_for_resource_session_ready(
    session: &mut dyn SourceNormalizerResourceSession,
) -> Result<SourceNormalizerResourceSessionStatus, String> {
    wait_for_resource_session_ready_with_policy(session, SOURCE_NORMALIZER_RESOURCE_READY_TIMEOUT)
}

fn wait_for_resource_session_ready_with_policy(
    session: &mut dyn SourceNormalizerResourceSession,
    ready_timeout: Duration,
) -> Result<SourceNormalizerResourceSessionStatus, String> {
    let started = Instant::now();
    loop {
        let status = session
            .poll()
            .map_err(|error| format!("poll_resource_session failed: {error}"))?;
        match status.state {
            SourceNormalizerResourceSessionState::Ready
            | SourceNormalizerResourceSessionState::Completed => return Ok(status),
            SourceNormalizerResourceSessionState::Failed => {
                return Err(status
                    .message
                    .unwrap_or_else(|| "source normalizer resource session failed".to_owned()));
            }
            SourceNormalizerResourceSessionState::Cancelled => {
                return Err(status.message.unwrap_or_else(|| {
                    "source normalizer resource session was cancelled".to_owned()
                }));
            }
            SourceNormalizerResourceSessionState::Starting
            | SourceNormalizerResourceSessionState::Running => {
                if resource_status_has_primary_bytes(&status) {
                    return Ok(status);
                }
                if started.elapsed() >= ready_timeout {
                    return Err(status.message.unwrap_or_else(|| {
                        "source normalizer resource did not produce a readable primary resource before startup timeout".to_owned()
                    }));
                }
                let remaining = ready_timeout
                    .checked_sub(started.elapsed())
                    .unwrap_or(Duration::ZERO);
                if remaining.is_zero() {
                    continue;
                }
                session
                    .wait_for_update(remaining)
                    .map_err(|error| format!("wait_resource_session_update failed: {error}"))?;
            }
        }
    }
}

fn resource_status_has_primary_bytes(status: &SourceNormalizerResourceSessionStatus) -> bool {
    let Some(info) = status.info.as_ref() else {
        return false;
    };
    let Some(primary_path) = info.primary_resource_path.as_deref() else {
        return false;
    };
    if primary_path.is_empty() {
        return false;
    }
    match info.output_route {
        SourceNormalizerOutputRoute::Fmp4LocalStream => fmp4_local_stream_ready(primary_path),
        SourceNormalizerOutputRoute::HlsShortWindow => {
            hls_short_window_ready(primary_path, &info.resources)
        }
        SourceNormalizerOutputRoute::PacketStream => false,
    }
}

fn fmp4_local_stream_ready(primary_path: &str) -> bool {
    let primary_len = std::fs::metadata(primary_path)
        .ok()
        .map(|metadata| metadata.len())
        .unwrap_or_default();
    primary_len > 32
        && file_contains_box_marker(primary_path, &[b"ftyp", b"moov"])
        && file_contains_box_marker(primary_path, &[b"moof", b"mdat"])
}

fn hls_short_window_ready(
    primary_path: &str,
    resources: &[player_plugin::SourceNormalizerResourceInfo],
) -> bool {
    let playlist = match std::fs::read_to_string(primary_path) {
        Ok(playlist) => playlist,
        Err(_) => return false,
    };
    if !playlist.contains("#EXTM3U") || !playlist.contains("#EXTINF") {
        return false;
    }
    let has_init = playlist.contains("#EXT-X-MAP")
        || resources.iter().any(|resource| {
            resource.path != primary_path
                && resource.byte_length.unwrap_or_default() > 0
                && resource
                    .path
                    .rsplit(std::path::MAIN_SEPARATOR)
                    .next()
                    .map(|name| name == "init.mp4")
                    .unwrap_or(false)
        });
    let has_media_segment = resources.iter().any(|resource| {
        let file_name = resource.path.rsplit(std::path::MAIN_SEPARATOR).next();
        resource.path != primary_path
            && resource.byte_length.unwrap_or_default() > 0
            && file_name != Some("init.mp4")
            && (resource.role == "segment"
                || resource
                    .content_type
                    .as_deref()
                    .map(|content_type| content_type.starts_with("video/"))
                    .unwrap_or(false))
            && file_name
                .map(|name| playlist.contains(name))
                .unwrap_or(false)
    });

    has_init && has_media_segment
}

fn file_contains_box_marker(path: &str, markers: &[&[u8; 4]]) -> bool {
    let Ok(file) = std::fs::File::open(path) else {
        return false;
    };
    let mut bytes = Vec::new();
    if file
        .take(FMP4_BOX_MARKER_SCAN_LIMIT_BYTES)
        .read_to_end(&mut bytes)
        .is_err()
    {
        return false;
    }
    bytes
        .windows(4)
        .any(|window| markers.iter().any(|marker| window == marker.as_slice()))
}

fn resource_participation(route: SourceNormalizerOutputRoute) -> PlayerPluginParticipation {
    match route {
        SourceNormalizerOutputRoute::Fmp4LocalStream
        | SourceNormalizerOutputRoute::HlsShortWindow => PlayerPluginParticipation::Selected,
        SourceNormalizerOutputRoute::PacketStream => PlayerPluginParticipation::Bypassed,
    }
}

fn diagnostic_from_record(
    record: &PluginDiagnosticRecord,
    participation: PlayerPluginParticipation,
) -> PlayerPluginDiagnostic {
    PlayerPluginDiagnostic {
        path: record.path.display().to_string(),
        plugin_name: record.plugin_name.clone(),
        plugin_kind: record.plugin_kind.map(plugin_kind_label).map(str::to_owned),
        status: runtime_status_from_loader(record.status),
        message: record.message.clone(),
        capability: record
            .capability_summary
            .as_ref()
            .map(capability_summary_from_loader),
        participation,
        details: Vec::new(),
    }
}

fn runtime_status_from_loader(status: PluginDiagnosticStatus) -> PlayerPluginDiagnosticStatus {
    PlayerPluginDiagnosticStatus::from_wire_name(status.wire_name())
        .unwrap_or(PlayerPluginDiagnosticStatus::UnsupportedKind)
}

fn capability_summary_from_loader(
    summary: &PluginCapabilitySummary,
) -> PlayerPluginCapabilitySummary {
    match summary {
        PluginCapabilitySummary::Decoder(summary) => PlayerPluginCapabilitySummary::Decoder(
            player_runtime::PlayerPluginDecoderCapabilitySummary {
                codecs: summary
                    .typed_codecs
                    .iter()
                    .map(|codec| player_runtime::PlayerPluginCodecCapability {
                        media_kind: match codec.media_kind {
                            player_plugin::DecoderMediaKind::Video => "video",
                            player_plugin::DecoderMediaKind::Audio => "audio",
                        }
                        .to_owned(),
                        codec: codec.codec.clone(),
                    })
                    .collect(),
                legacy_codecs: summary.codecs.clone(),
                supports_native_frame_output: summary.supports_native_frame_output,
                supports_hardware_decode: summary.supports_hardware_decode,
                supports_cpu_video_frames: summary.supports_cpu_video_frames,
                supports_audio_packets: summary.supports_audio_packets,
                supports_audio_frames: summary.supports_audio_frames,
                supports_pcm_frames: summary.supports_pcm_frames,
                supports_gpu_handles: summary.supports_gpu_handles,
                supports_flush: summary.supports_flush,
                supports_drain: summary.supports_drain,
                max_sessions: summary.max_sessions,
            },
        ),
        PluginCapabilitySummary::FrameProcessor(summary) => {
            PlayerPluginCapabilitySummary::FrameProcessor(frame_processor_summary_from_loader(
                summary,
            ))
        }
        PluginCapabilitySummary::SourceNormalizerPacket(summary) => {
            PlayerPluginCapabilitySummary::SourceNormalizer(source_normalizer_summary_from_loader(
                summary,
            ))
        }
        PluginCapabilitySummary::SourceNormalizerResource(summary) => {
            PlayerPluginCapabilitySummary::SourceNormalizer(
                source_normalizer_resource_summary_from_loader(summary),
            )
        }
    }
}

fn frame_processor_summary_from_loader(
    summary: &FrameProcessorPluginCapabilitySummary,
) -> PlayerPluginFrameProcessorCapabilitySummary {
    PlayerPluginFrameProcessorCapabilitySummary {
        accepted_input_handle_kinds: summary
            .accepted_input_handle_kinds
            .iter()
            .map(|kind| format!("{kind:?}"))
            .collect(),
        output_handle_kinds: summary
            .output_handle_kinds
            .iter()
            .map(|kind| format!("{kind:?}"))
            .collect(),
        accepted_input_pipeline_profiles: summary
            .accepted_input_pipeline_profiles
            .iter()
            .map(|profile| profile.label())
            .collect(),
        output_pipeline_profiles: summary
            .output_pipeline_profiles
            .iter()
            .map(|profile| profile.label())
            .collect(),
        supports_video_frames: summary.supports_video_frames,
        supports_in_place_passthrough: summary.supports_in_place_passthrough,
        preserves_dimensions: summary.preserves_dimensions,
        may_change_dimensions: summary.may_change_dimensions,
        preserves_color_metadata: summary.preserves_color_metadata,
        preserves_hdr_metadata: summary.preserves_hdr_metadata,
        supports_flush: summary.supports_flush,
        max_sessions: summary.max_sessions,
        max_in_flight_frames: summary.max_in_flight_frames,
    }
}

fn source_normalizer_summary_from_loader(
    summary: &SourceNormalizerPacketPluginCapabilitySummary,
) -> PlayerPluginSourceNormalizerCapabilitySummary {
    PlayerPluginSourceNormalizerCapabilitySummary {
        supported_runtime_profiles: summary.supported_runtime_profiles.clone(),
        supported_output_routes: vec![
            SourceNormalizerOutputRoute::PacketStream
                .wire_name()
                .to_owned(),
        ],
        max_level: normalize_level_label(summary.max_level).to_owned(),
        media_kinds: summary
            .media_kinds
            .iter()
            .map(|kind| media_kind_label(*kind).to_owned())
            .collect(),
        codecs: summary.codecs.clone(),
        bitstream_formats: summary
            .bitstream_formats
            .iter()
            .map(|format| bitstream_format_label(format).to_owned())
            .collect(),
        supports_seek: summary.supports_seek,
        supports_flush: summary.supports_flush,
        supports_growing_resources: false,
        supports_range_reads: false,
        supports_cancel: false,
        content_types: Vec::new(),
        required_libraries: summary.required_capabilities.libraries.clone(),
        required_demuxers: summary.required_capabilities.demuxers.clone(),
        required_muxers: summary.required_capabilities.muxers.clone(),
        required_protocols: summary.required_capabilities.protocols.clone(),
        required_parsers: summary.required_capabilities.parsers.clone(),
        required_bitstream_filters: summary.required_capabilities.bitstream_filters.clone(),
        required_tls: summary.required_capabilities.tls.clone(),
        requires_network: summary.required_capabilities.network,
        session_read_buffer_bytes: None,
        manifest_snapshot_bytes: None,
        session_disk_soft_cap_bytes: None,
        global_disk_soft_cap_bytes: None,
        max_sessions: summary.max_sessions,
    }
}

fn source_normalizer_resource_summary_from_loader(
    summary: &SourceNormalizerResourcePluginCapabilitySummary,
) -> PlayerPluginSourceNormalizerCapabilitySummary {
    PlayerPluginSourceNormalizerCapabilitySummary {
        supported_runtime_profiles: summary.supported_runtime_profiles.clone(),
        supported_output_routes: summary.supported_output_routes.clone(),
        max_level: normalize_level_label(summary.max_level).to_owned(),
        media_kinds: Vec::new(),
        codecs: Vec::new(),
        bitstream_formats: Vec::new(),
        supports_seek: false,
        supports_flush: false,
        supports_growing_resources: summary.supports_growing_resources,
        supports_range_reads: summary.supports_range_reads,
        supports_cancel: summary.supports_cancel,
        content_types: summary.content_types.clone(),
        required_libraries: summary.required_capabilities.libraries.clone(),
        required_demuxers: summary.required_capabilities.demuxers.clone(),
        required_muxers: summary.required_capabilities.muxers.clone(),
        required_protocols: summary.required_capabilities.protocols.clone(),
        required_parsers: summary.required_capabilities.parsers.clone(),
        required_bitstream_filters: summary.required_capabilities.bitstream_filters.clone(),
        required_tls: summary.required_capabilities.tls.clone(),
        requires_network: summary.required_capabilities.network,
        session_read_buffer_bytes: Some(summary.cache_policy.session_read_buffer_bytes),
        manifest_snapshot_bytes: Some(summary.cache_policy.manifest_snapshot_bytes),
        session_disk_soft_cap_bytes: Some(summary.cache_policy.session_disk_soft_cap_bytes),
        global_disk_soft_cap_bytes: Some(summary.cache_policy.global_disk_soft_cap_bytes),
        max_sessions: summary.max_sessions,
    }
}

fn source_normalizer_participation(record: &PluginDiagnosticRecord) -> PlayerPluginParticipation {
    if record.status == PluginDiagnosticStatus::SourceNormalizerSupported {
        PlayerPluginParticipation::Available
    } else {
        PlayerPluginParticipation::Unknown
    }
}

fn frame_processor_participation(record: &PluginDiagnosticRecord) -> PlayerPluginParticipation {
    if record.status == PluginDiagnosticStatus::FrameProcessorSupported {
        PlayerPluginParticipation::Available
    } else {
        PlayerPluginParticipation::Unknown
    }
}

fn runtime_source_normalizer_diagnostic(
    path: String,
    plugin_name: Option<String>,
    status: PlayerPluginDiagnosticStatus,
    message: impl Into<String>,
    participation: PlayerPluginParticipation,
) -> PlayerPluginDiagnostic {
    PlayerPluginDiagnostic {
        path,
        plugin_name,
        plugin_kind: Some("source_normalizer".to_owned()),
        status,
        message: Some(message.into()),
        capability: None,
        participation,
        details: Vec::new(),
    }
}

fn runtime_frame_processor_diagnostic(
    path: String,
    plugin_name: Option<String>,
    message: impl Into<String>,
    participation: PlayerPluginParticipation,
) -> PlayerPluginDiagnostic {
    PlayerPluginDiagnostic {
        path,
        plugin_name,
        plugin_kind: Some("frame_processor".to_owned()),
        status: PlayerPluginDiagnosticStatus::FrameProcessorUnsupported,
        message: Some(message.into()),
        capability: None,
        participation,
        details: Vec::new(),
    }
}

fn player_plugin_detail(
    key: impl Into<String>,
    value: impl Into<String>,
) -> player_runtime::PlayerPluginDiagnosticDetail {
    player_runtime::PlayerPluginDiagnosticDetail {
        key: key.into(),
        value: value.into(),
    }
}

fn parse_ready_ms_from_diagnostic_message(message: &str) -> Option<u128> {
    let (_, suffix) = message.split_once("ready_ms=")?;
    let digits = suffix
        .chars()
        .take_while(|character| character.is_ascii_digit())
        .collect::<String>();
    if digits.is_empty() {
        return None;
    }
    digits.parse().ok()
}

fn source_normalizer_mode_cache_label(mode: SourceNormalizerMode) -> &'static str {
    match mode {
        SourceNormalizerMode::Disabled => "disabled",
        SourceNormalizerMode::DiagnosticsOnly => "diagnosticsOnly",
        SourceNormalizerMode::PreflightOnly => "preflightOnly",
        SourceNormalizerMode::PreferNormalized => "preferNormalized",
        SourceNormalizerMode::RequireNormalized => "requireNormalized",
    }
}

fn plugin_kind_label(kind: player_plugin::VesperPluginKind) -> &'static str {
    match kind {
        player_plugin::VesperPluginKind::PostDownloadProcessor => "post_download_processor",
        player_plugin::VesperPluginKind::PipelineEventHook => "pipeline_event_hook",
        player_plugin::VesperPluginKind::Decoder => "decoder",
        player_plugin::VesperPluginKind::BenchmarkSink => "benchmark_sink",
        player_plugin::VesperPluginKind::FrameProcessor => "frame_processor",
        player_plugin::VesperPluginKind::SourceNormalizer => "source_normalizer",
    }
}

fn normalize_level_label(level: SourceNormalizerNormalizeLevel) -> &'static str {
    match level {
        SourceNormalizerNormalizeLevel::RemuxOnly => "remux_only",
        SourceNormalizerNormalizeLevel::PacketRepair => "packet_repair",
    }
}

fn media_kind_label(kind: SourceNormalizerPacketMediaKind) -> &'static str {
    match kind {
        SourceNormalizerPacketMediaKind::Video => "video",
        SourceNormalizerPacketMediaKind::Audio => "audio",
        SourceNormalizerPacketMediaKind::Subtitle => "subtitle",
    }
}

fn preferred_resource_route_for_source(
    source: &MediaSource,
) -> Option<SourceNormalizerOutputRoute> {
    match source.protocol() {
        player_runtime::MediaSourceProtocol::Hls | player_runtime::MediaSourceProtocol::Dash => {
            Some(SourceNormalizerOutputRoute::HlsShortWindow)
        }
        _ => Some(SourceNormalizerOutputRoute::Fmp4LocalStream),
    }
}

fn native_first_source_normalizer_bypass(
    source: &MediaSource,
    configuration: &MobileSourceNormalizerConfiguration,
) -> bool {
    match source.protocol() {
        player_runtime::MediaSourceProtocol::Hls | player_runtime::MediaSourceProtocol::Dash => {
            true
        }
        player_runtime::MediaSourceProtocol::File
        | player_runtime::MediaSourceProtocol::Progressive => {
            // Only ordinary MP4/M4V/MOV sources without an explicit runtime
            // profile stay native-first. A configured profile such as `flv` or
            // `generic-fallback` is caller intent to try the normalized route.
            configuration.runtime_profile.is_none() && is_standard_progressive_mp4_source(source)
        }
        _ => false,
    }
}

fn configured_runtime_profile(configuration: &MobileSourceNormalizerConfiguration) -> Option<&str> {
    configuration
        .runtime_profile
        .as_deref()
        .map(str::trim)
        .filter(|profile| !profile.is_empty())
}

fn best_mobile_source_normalizer_resource<'a>(
    registry: &'a PluginRegistry,
    configuration: &MobileSourceNormalizerConfiguration,
) -> Option<&'a PluginDiagnosticRecord> {
    match configured_runtime_profile(configuration) {
        Some(profile) => registry.best_source_normalizer_resource_for_profile(profile),
        None => registry.best_source_normalizer_resource(),
    }
}

fn source_normalizer_resource_selection_failure_message(
    operation: &str,
    registry: &PluginRegistry,
    configuration: &MobileSourceNormalizerConfiguration,
) -> String {
    match configured_runtime_profile(configuration) {
        Some(profile) => format!(
            "source normalizer {operation} skipped because no resource-output plugin supports runtime profile '{profile}': {}",
            mobile_source_normalizer_registry_notes(registry)
        ),
        None => format!(
            "source normalizer {operation} skipped because no resource-output plugin is available: {}",
            mobile_source_normalizer_registry_notes(registry)
        ),
    }
}

fn is_standard_progressive_mp4_source(source: &MediaSource) -> bool {
    let lower = source.uri().to_ascii_lowercase();
    let lower_without_fragment = lower
        .split_once('#')
        .map(|(path, _)| path)
        .unwrap_or(lower.as_str());
    let lower_path = lower_without_fragment
        .split_once('?')
        .map(|(path, _)| path)
        .unwrap_or(lower_without_fragment);
    matches!(
        lower_path.rsplit_once('.').map(|(_, extension)| extension),
        Some("mp4" | "m4v" | "mov")
    )
}

fn mobile_source_normalizer_registry_notes(registry: &PluginRegistry) -> String {
    let records = registry.records();
    if records.is_empty() {
        return "no plugin records".to_owned();
    }
    records
        .iter()
        .map(|record| {
            format!(
                "{}:{}",
                record.path.display(),
                record
                    .message
                    .as_deref()
                    .unwrap_or_else(|| status_note(record.status))
            )
        })
        .collect::<Vec<_>>()
        .join("; ")
}

fn status_note(status: PluginDiagnosticStatus) -> &'static str {
    match status {
        PluginDiagnosticStatus::Loaded => "loaded",
        PluginDiagnosticStatus::LoadFailed => "load failed",
        PluginDiagnosticStatus::UnsupportedKind => "unsupported kind",
        PluginDiagnosticStatus::DecoderSupported => "decoder supported",
        PluginDiagnosticStatus::DecoderUnsupported => "decoder unsupported",
        PluginDiagnosticStatus::FrameProcessorSupported => "frame processor supported",
        PluginDiagnosticStatus::FrameProcessorUnsupported => "frame processor unsupported",
        PluginDiagnosticStatus::SourceNormalizerSupported => "source normalizer supported",
        PluginDiagnosticStatus::SourceNormalizerUnsupported => "source normalizer unsupported",
    }
}

fn bitstream_format_label(format: &DecoderBitstreamFormat) -> &'static str {
    match format {
        DecoderBitstreamFormat::AnnexB => "annex_b",
        DecoderBitstreamFormat::Avcc => "avcc",
        DecoderBitstreamFormat::Hvcc => "hvcc",
        DecoderBitstreamFormat::Unknown(_) => "unknown",
    }
}

pub fn mobile_plugin_diagnostics_json(
    source: &MediaSource,
    source_normalizer: &MobileSourceNormalizerConfiguration,
    frame_processor: &MobileFrameProcessorConfiguration,
) -> Result<String, serde_json::Error> {
    let mut diagnostics = source_normalizer_diagnostics(source, source_normalizer);
    diagnostics.extend(frame_processor_diagnostics(frame_processor));
    serde_json::to_string(
        &diagnostics
            .iter()
            .map(MobilePluginDiagnosticWire::from)
            .collect::<Vec<_>>(),
    )
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MobilePluginDiagnosticWire<'a> {
    path: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    plugin_name: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    plugin_kind: Option<&'a str>,
    status: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    message: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    capability: Option<WirePluginCapability<'a>>,
    participation: &'static str,
    #[serde(skip_serializing_if = "PluginDiagnosticDetailsWire::is_empty")]
    details: PluginDiagnosticDetailsWire<'a>,
}

impl MobileSourceNormalizerResourceWire {
    pub fn from_open(
        handle: u64,
        opened: &MobileSourceNormalizerResourceOpen,
        playback_uri: Option<String>,
    ) -> Self {
        let info = opened.status.info.as_ref().unwrap_or(&opened.info);
        Self {
            handle,
            plugin_name: opened.plugin_name.clone(),
            plugin_path: opened.plugin_path.clone(),
            output_route: info.output_route.wire_name().to_owned(),
            selected_profile: info.runtime_profile.clone(),
            container: info.container.clone(),
            primary_resource_path: info.primary_resource_path.clone(),
            primary_content_type: info.primary_content_type.clone(),
            playback_uri,
            resources: info
                .resources
                .iter()
                .map(MobileSourceNormalizerResourceInfoWire::from)
                .collect(),
            state: resource_state_wire_name(opened.status.state).to_owned(),
            message: opened.status.message.clone(),
            disk_bytes_used: opened.status.disk_bytes_used.or(info.disk_bytes_used),
            cache_policy: MobileSourceNormalizerCachePolicyWire::from(&opened.cache_policy),
            route: info.output_route.wire_name().to_owned(),
            participation: participation_wire_name(resource_participation(info.output_route))
                .to_owned(),
            fallback_reason: None,
            cache_quota: Some(opened.cache_policy.session_disk_soft_cap_bytes),
            diagnostics: opened
                .diagnostics
                .iter()
                .map(MobilePluginDiagnosticOwnedWire::from)
                .collect(),
        }
    }
}

impl From<&SourceNormalizerResourceCachePolicy> for MobileSourceNormalizerCachePolicyWire {
    fn from(policy: &SourceNormalizerResourceCachePolicy) -> Self {
        Self {
            session_read_buffer_bytes: policy.session_read_buffer_bytes,
            manifest_snapshot_bytes: policy.manifest_snapshot_bytes,
            session_disk_soft_cap_bytes: policy.session_disk_soft_cap_bytes,
            global_disk_soft_cap_bytes: policy.global_disk_soft_cap_bytes,
        }
    }
}

impl From<&player_plugin::SourceNormalizerResourceInfo> for MobileSourceNormalizerResourceInfoWire {
    fn from(value: &player_plugin::SourceNormalizerResourceInfo) -> Self {
        Self {
            role: value.role.clone(),
            path: value.path.clone(),
            content_type: value.content_type.clone(),
            byte_length: value.byte_length,
            growing: value.growing,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MobilePluginDiagnosticOwnedWire {
    path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    plugin_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    plugin_kind: Option<String>,
    status: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    message: Option<String>,
    participation: &'static str,
    #[serde(skip_serializing_if = "OwnedPluginDiagnosticDetailsWire::is_empty")]
    details: OwnedPluginDiagnosticDetailsWire,
}

impl From<&PlayerPluginDiagnostic> for MobilePluginDiagnosticOwnedWire {
    fn from(value: &PlayerPluginDiagnostic) -> Self {
        Self {
            path: value.path.clone(),
            plugin_name: value.plugin_name.clone(),
            plugin_kind: value.plugin_kind.clone(),
            status: status_wire_name(value.status),
            message: value.message.clone(),
            participation: participation_wire_name(value.participation),
            details: OwnedPluginDiagnosticDetailsWire::from(value.details.as_slice()),
        }
    }
}

impl<'a> From<&'a PlayerPluginDiagnostic> for MobilePluginDiagnosticWire<'a> {
    fn from(value: &'a PlayerPluginDiagnostic) -> Self {
        Self {
            path: value.path.as_str(),
            plugin_name: value.plugin_name.as_deref(),
            plugin_kind: value.plugin_kind.as_deref(),
            status: status_wire_name(value.status),
            message: value.message.as_deref(),
            capability: value.capability.as_ref().map(WirePluginCapability::from),
            participation: participation_wire_name(value.participation),
            details: PluginDiagnosticDetailsWire::from(value.details.as_slice()),
        }
    }
}

#[derive(Debug)]
pub struct PluginDiagnosticDetailsWire<'a> {
    details: &'a [player_runtime::PlayerPluginDiagnosticDetail],
}

impl<'a> PluginDiagnosticDetailsWire<'a> {
    fn is_empty(&self) -> bool {
        self.details.is_empty()
    }
}

impl<'a> From<&'a [player_runtime::PlayerPluginDiagnosticDetail]>
    for PluginDiagnosticDetailsWire<'a>
{
    fn from(details: &'a [player_runtime::PlayerPluginDiagnosticDetail]) -> Self {
        Self { details }
    }
}

impl Serialize for PluginDiagnosticDetailsWire<'_> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut map = serializer.serialize_map(Some(self.details.len()))?;
        for detail in self.details {
            map.serialize_entry(detail.key.as_str(), detail.value.as_str())?;
        }
        map.end()
    }
}

#[derive(Debug, Clone)]
pub struct OwnedPluginDiagnosticDetailsWire {
    details: Vec<player_runtime::PlayerPluginDiagnosticDetail>,
}

impl OwnedPluginDiagnosticDetailsWire {
    fn is_empty(&self) -> bool {
        self.details.is_empty()
    }
}

impl From<&[player_runtime::PlayerPluginDiagnosticDetail]> for OwnedPluginDiagnosticDetailsWire {
    fn from(details: &[player_runtime::PlayerPluginDiagnosticDetail]) -> Self {
        Self {
            details: details.to_vec(),
        }
    }
}

impl Serialize for OwnedPluginDiagnosticDetailsWire {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        PluginDiagnosticDetailsWire::from(self.details.as_slice()).serialize(serializer)
    }
}

pub fn resource_state_wire_name(state: SourceNormalizerResourceSessionState) -> &'static str {
    match state {
        SourceNormalizerResourceSessionState::Starting => "starting",
        SourceNormalizerResourceSessionState::Ready => "ready",
        SourceNormalizerResourceSessionState::Running => "running",
        SourceNormalizerResourceSessionState::Completed => "completed",
        SourceNormalizerResourceSessionState::Failed => "failed",
        SourceNormalizerResourceSessionState::Cancelled => "cancelled",
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct WirePluginCapability<'a> {
    kind: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    decoder: Option<WireDecoderCapability<'a>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    frame_processor: Option<WireFrameProcessorCapability<'a>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    source_normalizer: Option<WireSourceNormalizerCapability<'a>>,
}

impl<'a> From<&'a PlayerPluginCapabilitySummary> for WirePluginCapability<'a> {
    fn from(value: &'a PlayerPluginCapabilitySummary) -> Self {
        match value {
            PlayerPluginCapabilitySummary::Decoder(summary) => Self {
                kind: "decoder",
                decoder: Some(WireDecoderCapability::from(summary)),
                frame_processor: None,
                source_normalizer: None,
            },
            PlayerPluginCapabilitySummary::FrameProcessor(summary) => Self {
                kind: "frameProcessor",
                decoder: None,
                frame_processor: Some(WireFrameProcessorCapability::from(summary)),
                source_normalizer: None,
            },
            PlayerPluginCapabilitySummary::SourceNormalizer(summary) => Self {
                kind: "sourceNormalizer",
                decoder: None,
                frame_processor: None,
                source_normalizer: Some(WireSourceNormalizerCapability::from(summary)),
            },
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct WirePluginCodecCapability<'a> {
    media_kind: &'a str,
    codec: &'a str,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct WireDecoderCapability<'a> {
    codecs: Vec<WirePluginCodecCapability<'a>>,
    legacy_codecs: &'a [String],
    supports_native_frame_output: bool,
    supports_hardware_decode: bool,
    supports_cpu_video_frames: bool,
    supports_audio_frames: bool,
    supports_gpu_handles: bool,
    supports_flush: bool,
    supports_drain: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_sessions: Option<u32>,
}

impl<'a> From<&'a player_runtime::PlayerPluginDecoderCapabilitySummary>
    for WireDecoderCapability<'a>
{
    fn from(value: &'a player_runtime::PlayerPluginDecoderCapabilitySummary) -> Self {
        Self {
            codecs: value
                .codecs
                .iter()
                .map(|codec| WirePluginCodecCapability {
                    media_kind: codec.media_kind.as_str(),
                    codec: codec.codec.as_str(),
                })
                .collect(),
            legacy_codecs: &value.legacy_codecs,
            supports_native_frame_output: value.supports_native_frame_output,
            supports_hardware_decode: value.supports_hardware_decode,
            supports_cpu_video_frames: value.supports_cpu_video_frames,
            supports_audio_frames: value.supports_audio_frames,
            supports_gpu_handles: value.supports_gpu_handles,
            supports_flush: value.supports_flush,
            supports_drain: value.supports_drain,
            max_sessions: value.max_sessions,
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct WireFrameProcessorCapability<'a> {
    accepted_input_handle_kinds: &'a [String],
    output_handle_kinds: &'a [String],
    accepted_input_pipeline_profiles: &'a [String],
    output_pipeline_profiles: &'a [String],
    supports_video_frames: bool,
    supports_in_place_passthrough: bool,
    preserves_dimensions: bool,
    may_change_dimensions: bool,
    preserves_color_metadata: bool,
    preserves_hdr_metadata: bool,
    supports_flush: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_sessions: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_in_flight_frames: Option<u32>,
}

impl<'a> From<&'a PlayerPluginFrameProcessorCapabilitySummary>
    for WireFrameProcessorCapability<'a>
{
    fn from(value: &'a PlayerPluginFrameProcessorCapabilitySummary) -> Self {
        Self {
            accepted_input_handle_kinds: &value.accepted_input_handle_kinds,
            output_handle_kinds: &value.output_handle_kinds,
            accepted_input_pipeline_profiles: &value.accepted_input_pipeline_profiles,
            output_pipeline_profiles: &value.output_pipeline_profiles,
            supports_video_frames: value.supports_video_frames,
            supports_in_place_passthrough: value.supports_in_place_passthrough,
            preserves_dimensions: value.preserves_dimensions,
            may_change_dimensions: value.may_change_dimensions,
            preserves_color_metadata: value.preserves_color_metadata,
            preserves_hdr_metadata: value.preserves_hdr_metadata,
            supports_flush: value.supports_flush,
            max_sessions: value.max_sessions,
            max_in_flight_frames: value.max_in_flight_frames,
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct WireSourceNormalizerCapability<'a> {
    supported_runtime_profiles: &'a [String],
    supported_output_routes: &'a [String],
    max_level: &'a str,
    media_kinds: &'a [String],
    codecs: &'a [String],
    bitstream_formats: &'a [String],
    supports_seek: bool,
    supports_flush: bool,
    supports_growing_resources: bool,
    supports_range_reads: bool,
    supports_cancel: bool,
    content_types: &'a [String],
    required_libraries: &'a [String],
    required_demuxers: &'a [String],
    required_muxers: &'a [String],
    required_protocols: &'a [String],
    required_parsers: &'a [String],
    required_bitstream_filters: &'a [String],
    #[serde(skip_serializing_if = "Option::is_none")]
    required_tls: Option<&'a str>,
    requires_network: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    session_read_buffer_bytes: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    manifest_snapshot_bytes: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    session_disk_soft_cap_bytes: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    global_disk_soft_cap_bytes: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_sessions: Option<u32>,
}

impl<'a> From<&'a PlayerPluginSourceNormalizerCapabilitySummary>
    for WireSourceNormalizerCapability<'a>
{
    fn from(value: &'a PlayerPluginSourceNormalizerCapabilitySummary) -> Self {
        Self {
            supported_runtime_profiles: &value.supported_runtime_profiles,
            supported_output_routes: &value.supported_output_routes,
            max_level: value.max_level.as_str(),
            media_kinds: &value.media_kinds,
            codecs: &value.codecs,
            bitstream_formats: &value.bitstream_formats,
            supports_seek: value.supports_seek,
            supports_flush: value.supports_flush,
            supports_growing_resources: value.supports_growing_resources,
            supports_range_reads: value.supports_range_reads,
            supports_cancel: value.supports_cancel,
            content_types: &value.content_types,
            required_libraries: &value.required_libraries,
            required_demuxers: &value.required_demuxers,
            required_muxers: &value.required_muxers,
            required_protocols: &value.required_protocols,
            required_parsers: &value.required_parsers,
            required_bitstream_filters: &value.required_bitstream_filters,
            required_tls: value.required_tls.as_deref(),
            requires_network: value.requires_network,
            session_read_buffer_bytes: value.session_read_buffer_bytes,
            manifest_snapshot_bytes: value.manifest_snapshot_bytes,
            session_disk_soft_cap_bytes: value.session_disk_soft_cap_bytes,
            global_disk_soft_cap_bytes: value.global_disk_soft_cap_bytes,
            max_sessions: value.max_sessions,
        }
    }
}

fn status_wire_name(status: PlayerPluginDiagnosticStatus) -> &'static str {
    status.wire_name()
}

fn participation_wire_name(participation: PlayerPluginParticipation) -> &'static str {
    participation.wire_name()
}

#[cfg(test)]
mod tests {
    use super::*;
    use player_plugin::SourceNormalizerRequiredCapabilities;
    use player_plugin::SourceNormalizerResourceSessionWaitStatus;
    use serde_json::Value;

    #[test]
    fn disabled_configs_emit_no_diagnostics() {
        let diagnostics = apply_mobile_plugin_diagnostics(
            PlayerRuntimeStartup {
                ffmpeg_initialized: false,
                audio_output: None,
                decoded_audio: None,
                video_decode: None,
                plugin_diagnostics: Vec::new(),
            },
            &MediaSource::new("placeholder.mp4"),
            &MobilePluginConfiguration::default(),
        )
        .plugin_diagnostics;

        assert!(diagnostics.is_empty());
    }

    #[test]
    fn native_frame_pipeline_configuration_applies_to_runtime_options() {
        let mut options = PlayerRuntimeOptions::default();
        MobilePluginConfiguration {
            native_frame_pipeline: MobileNativeFramePipelineConfiguration {
                mode: NativeFramePipelineMode::RequireNativeFrame,
                decoder_plugin_library_paths: vec![PathBuf::from("/tmp/libdecoder.dylib")],
                frame_processor_plugin_library_paths: vec![PathBuf::from("/tmp/libframe.dylib")],
                max_in_flight_frames: Some(3),
            },
            ..MobilePluginConfiguration::default()
        }
        .apply_to_runtime_options(&mut options);

        assert_eq!(
            options.decoder_plugin_video_mode,
            player_runtime::PlayerDecoderPluginVideoMode::PreferNativeFrame
        );
        assert_eq!(
            options.decoder_plugin_library_paths,
            vec![PathBuf::from("/tmp/libdecoder.dylib")]
        );
        assert_eq!(
            options.frame_processor_library_paths,
            vec![PathBuf::from("/tmp/libframe.dylib")]
        );
        assert_eq!(
            options
                .frame_processor_policy
                .max_in_flight_frames_per_processor,
            3
        );
    }

    #[test]
    fn runtime_options_preserve_native_frame_pipeline_diagnostic_intent() {
        let options = PlayerRuntimeOptions::default()
            .with_decoder_plugin_video_mode(
                player_runtime::PlayerDecoderPluginVideoMode::PreferNativeFrame,
            )
            .with_decoder_plugin_library_paths([PathBuf::from("/tmp/libmediacodec.so")])
            .with_frame_processor_library_paths([PathBuf::from("/tmp/libframe.so")]);

        let configuration = MobilePluginConfiguration::from_runtime_options(&options);

        assert_eq!(
            configuration.native_frame_pipeline.mode,
            NativeFramePipelineMode::PreferNativeFrame
        );
        assert_eq!(
            configuration
                .native_frame_pipeline
                .decoder_plugin_library_paths,
            vec![PathBuf::from("/tmp/libmediacodec.so")]
        );
        assert_eq!(
            configuration
                .native_frame_pipeline
                .frame_processor_plugin_library_paths,
            vec![PathBuf::from("/tmp/libframe.so")]
        );
    }

    #[test]
    fn runtime_options_with_decoder_paths_remain_diagnostics_only_until_opted_in() {
        let options = PlayerRuntimeOptions::default()
            .with_decoder_plugin_library_paths([PathBuf::from("/tmp/libmediacodec.so")]);

        let configuration = MobilePluginConfiguration::from_runtime_options(&options);

        assert_eq!(
            configuration.native_frame_pipeline.mode,
            NativeFramePipelineMode::DiagnosticsOnly
        );
        let diagnostics = native_frame_pipeline_diagnostics(&configuration.native_frame_pipeline);
        assert_eq!(
            diagnostics[0]
                .details
                .iter()
                .find(|detail| detail.key == "route")
                .map(|detail| detail.value.as_str()),
            Some("systemPlayer")
        );
    }

    #[test]
    fn source_normalizer_missing_paths_are_non_blocking() {
        let diagnostics = source_normalizer_diagnostics(
            &MediaSource::new("placeholder.mp4"),
            &MobileSourceNormalizerConfiguration {
                mode: SourceNormalizerMode::PreflightOnly,
                ..MobileSourceNormalizerConfiguration::default()
            },
        );

        assert_eq!(diagnostics.len(), 1);
        assert_eq!(
            diagnostics[0].plugin_kind.as_deref(),
            Some("source_normalizer")
        );
        assert_eq!(
            diagnostics[0].participation,
            PlayerPluginParticipation::Unknown
        );
    }

    #[test]
    fn preflight_cache_hit_avoids_second_packet_session_open() {
        let source = MediaSource::new("https://cdn.example.test/video.flv");
        let configuration = MobileSourceNormalizerConfiguration {
            mode: SourceNormalizerMode::PreflightOnly,
            plugin_library_paths: vec![PathBuf::from("/plugins/source-normalizer.so")],
            runtime_profile: Some("flv".to_owned()),
        };
        let key = PreflightDiagnosticCacheKey::from_source(&source, &configuration);
        let mut cache = PreflightDiagnosticCache::new(64);
        let opened = std::cell::Cell::new(0);
        let now = Instant::now();

        let first = cached_preflight_diagnostic_with(&mut cache, key.clone(), now, || {
            opened.set(opened.get() + 1);
            test_source_normalizer_diagnostic(
                PlayerPluginDiagnosticStatus::SourceNormalizerSupported,
                "source normalizer preflight opened and closed packet session; ready_ms=42",
            )
        });
        let second = cached_preflight_diagnostic_with(
            &mut cache,
            key,
            now + Duration::from_millis(12),
            || {
                opened.set(opened.get() + 1);
                test_source_normalizer_diagnostic(
                    PlayerPluginDiagnosticStatus::SourceNormalizerUnsupported,
                    "unexpected second open",
                )
            },
        );

        assert_eq!(opened.get(), 1);
        assert_eq!(first.status, second.status);
        assert_eq!(first.capability, second.capability);
        assert_eq!(
            second
                .details
                .iter()
                .find(|detail| detail.key == "cached")
                .map(|detail| detail.value.as_str()),
            Some("true")
        );
        assert_eq!(
            second
                .details
                .iter()
                .find(|detail| detail.key == "cacheAgeMs")
                .map(|detail| detail.value.as_str()),
            Some("12")
        );
        assert_eq!(
            second
                .details
                .iter()
                .find(|detail| detail.key == "originalReadyMs")
                .map(|detail| detail.value.as_str()),
            Some("42")
        );
    }

    #[test]
    fn preflight_cache_key_changes_with_runtime_profile_and_plugin_metadata() {
        let source = MediaSource::new("https://cdn.example.test/video.flv");
        let path = std::env::temp_dir().join(format!(
            "vesper-preflight-cache-key-{}-{}.so",
            std::process::id(),
            Instant::now().elapsed().as_nanos()
        ));
        std::fs::write(&path, b"one").expect("write plugin fingerprint fixture");
        let generic = MobileSourceNormalizerConfiguration {
            mode: SourceNormalizerMode::PreflightOnly,
            plugin_library_paths: vec![path.clone()],
            runtime_profile: Some("generic-fallback".to_owned()),
        };
        let flv = MobileSourceNormalizerConfiguration {
            runtime_profile: Some("flv".to_owned()),
            ..generic.clone()
        };
        let generic_key = PreflightDiagnosticCacheKey::from_source(&source, &generic);
        let flv_key = PreflightDiagnosticCacheKey::from_source(&source, &flv);

        assert_ne!(generic_key, flv_key);

        std::fs::write(&path, b"one-two").expect("rewrite plugin fingerprint fixture");
        let rewritten_key = PreflightDiagnosticCacheKey::from_source(&source, &generic);
        let _ = std::fs::remove_file(&path);

        assert_ne!(generic_key, rewritten_key);
    }

    #[test]
    fn preflight_failure_cache_expires_before_success_cache() {
        let source = MediaSource::new("https://cdn.example.test/video.flv");
        let configuration = MobileSourceNormalizerConfiguration {
            mode: SourceNormalizerMode::PreflightOnly,
            plugin_library_paths: vec![PathBuf::from("/plugins/source-normalizer.so")],
            runtime_profile: None,
        };
        let key = PreflightDiagnosticCacheKey::from_source(&source, &configuration);
        let mut cache = PreflightDiagnosticCache::new(64);
        let opened = std::cell::Cell::new(0);
        let now = Instant::now();

        let _ = cached_preflight_diagnostic_with(&mut cache, key.clone(), now, || {
            opened.set(opened.get() + 1);
            test_source_normalizer_diagnostic(
                PlayerPluginDiagnosticStatus::SourceNormalizerUnsupported,
                "source normalizer preflight open failed: boom",
            )
        });
        let cached = cached_preflight_diagnostic_with(
            &mut cache,
            key.clone(),
            now + Duration::from_secs(29),
            || {
                opened.set(opened.get() + 1);
                test_source_normalizer_diagnostic(
                    PlayerPluginDiagnosticStatus::SourceNormalizerSupported,
                    "unexpected second open; ready_ms=1",
                )
            },
        );
        let refreshed = cached_preflight_diagnostic_with(
            &mut cache,
            key,
            now + Duration::from_secs(31),
            || {
                opened.set(opened.get() + 1);
                test_source_normalizer_diagnostic(
                    PlayerPluginDiagnosticStatus::SourceNormalizerSupported,
                    "source normalizer preflight opened and closed packet session; ready_ms=7",
                )
            },
        );

        assert_eq!(opened.get(), 2);
        assert_eq!(
            cached.status,
            PlayerPluginDiagnosticStatus::SourceNormalizerUnsupported
        );
        assert_eq!(
            refreshed.status,
            PlayerPluginDiagnosticStatus::SourceNormalizerSupported
        );
    }

    #[test]
    fn normalized_resource_modes_are_not_served_from_preflight_cache() {
        SOURCE_NORMALIZER_PREFLIGHT_CACHE
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .clear();
        let diagnostics = source_normalizer_diagnostics(
            &MediaSource::new("https://cdn.example.test/video.flv"),
            &MobileSourceNormalizerConfiguration {
                mode: SourceNormalizerMode::RequireNormalized,
                plugin_library_paths: vec![PathBuf::from("/missing/source-normalizer.so")],
                runtime_profile: Some("flv".to_owned()),
            },
        );

        assert!(
            diagnostics.iter().all(|diagnostic| diagnostic
                .details
                .iter()
                .all(|detail| detail.key != "cached")),
            "live normalized-resource diagnostics must not come from the preflight cache"
        );
        assert_eq!(
            SOURCE_NORMALIZER_PREFLIGHT_CACHE
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .len(),
            0
        );
    }

    #[test]
    fn frame_processor_missing_paths_are_diagnostic_only() {
        let diagnostics = frame_processor_diagnostics(&MobileFrameProcessorConfiguration {
            mode: FrameProcessorMode::DiagnosticsOnly,
            ..MobileFrameProcessorConfiguration::default()
        });

        assert_eq!(diagnostics.len(), 1);
        assert_eq!(
            diagnostics[0].plugin_kind.as_deref(),
            Some("frame_processor")
        );
        assert_ne!(
            diagnostics[0].participation,
            PlayerPluginParticipation::Participated
        );
    }

    #[test]
    fn native_frame_pipeline_diagnostics_are_explicit_opt_in() {
        let diagnostics =
            native_frame_pipeline_diagnostics(&MobileNativeFramePipelineConfiguration {
                mode: NativeFramePipelineMode::PreferNativeFrame,
                decoder_plugin_library_paths: vec![PathBuf::from("/tmp/libdecoder.dylib")],
                frame_processor_plugin_library_paths: vec![PathBuf::from("/tmp/libframe.dylib")],
                max_in_flight_frames: Some(2),
            });

        assert_eq!(diagnostics.len(), 1);
        assert_eq!(
            diagnostics[0].plugin_kind.as_deref(),
            Some("native_frame_pipeline")
        );
        assert_eq!(
            diagnostics[0].participation,
            PlayerPluginParticipation::Selected
        );
        assert!(
            diagnostics[0]
                .message
                .as_deref()
                .unwrap_or_default()
                .contains("explicitly preferred")
        );
        assert!(
            diagnostics[0]
                .message
                .as_deref()
                .unwrap_or_default()
                .contains("route=sdkManagedNativeFrame")
        );
    }

    #[test]
    fn native_frame_pipeline_diagnostics_only_keeps_system_player_route() {
        let diagnostics =
            native_frame_pipeline_diagnostics(&MobileNativeFramePipelineConfiguration {
                mode: NativeFramePipelineMode::DiagnosticsOnly,
                decoder_plugin_library_paths: vec![PathBuf::from("/tmp/libdecoder.dylib")],
                frame_processor_plugin_library_paths: Vec::new(),
                max_in_flight_frames: None,
            });

        assert_eq!(diagnostics.len(), 1);
        assert_eq!(
            diagnostics[0].participation,
            PlayerPluginParticipation::Available
        );
        assert!(
            diagnostics[0]
                .message
                .as_deref()
                .unwrap_or_default()
                .contains("route=systemPlayer")
        );
    }

    #[test]
    fn diagnostics_json_uses_shared_flutter_wire_names() {
        let json = mobile_plugin_diagnostics_json(
            &MediaSource::new("placeholder.mp4"),
            &MobileSourceNormalizerConfiguration {
                mode: SourceNormalizerMode::DiagnosticsOnly,
                ..MobileSourceNormalizerConfiguration::default()
            },
            &MobileFrameProcessorConfiguration {
                mode: FrameProcessorMode::DiagnosticsOnly,
                ..MobileFrameProcessorConfiguration::default()
            },
        )
        .expect("serialize diagnostics");

        assert!(json.contains("sourceNormalizerUnsupported"));
        assert!(json.contains("frameProcessorUnsupported"));
        assert!(json.contains("participation"));
    }

    #[test]
    fn loader_statuses_convert_through_shared_wire_contract() {
        assert_eq!(
            runtime_status_from_loader(PluginDiagnosticStatus::Loaded),
            PlayerPluginDiagnosticStatus::Loaded
        );
        assert_eq!(
            runtime_status_from_loader(PluginDiagnosticStatus::LoadFailed),
            PlayerPluginDiagnosticStatus::LoadFailed
        );
        assert_eq!(
            runtime_status_from_loader(PluginDiagnosticStatus::UnsupportedKind),
            PlayerPluginDiagnosticStatus::UnsupportedKind
        );
        assert_eq!(
            runtime_status_from_loader(PluginDiagnosticStatus::DecoderSupported),
            PlayerPluginDiagnosticStatus::DecoderSupported
        );
        assert_eq!(
            runtime_status_from_loader(PluginDiagnosticStatus::DecoderUnsupported),
            PlayerPluginDiagnosticStatus::DecoderUnsupported
        );
        assert_eq!(
            runtime_status_from_loader(PluginDiagnosticStatus::FrameProcessorSupported),
            PlayerPluginDiagnosticStatus::FrameProcessorSupported
        );
        assert_eq!(
            runtime_status_from_loader(PluginDiagnosticStatus::FrameProcessorUnsupported),
            PlayerPluginDiagnosticStatus::FrameProcessorUnsupported
        );
        assert_eq!(
            runtime_status_from_loader(PluginDiagnosticStatus::SourceNormalizerSupported),
            PlayerPluginDiagnosticStatus::SourceNormalizerSupported
        );
        assert_eq!(
            runtime_status_from_loader(PluginDiagnosticStatus::SourceNormalizerUnsupported),
            PlayerPluginDiagnosticStatus::SourceNormalizerUnsupported
        );
    }

    #[test]
    fn participation_wire_names_match_shared_diagnostics_contract() {
        assert_eq!(
            participation_wire_name(PlayerPluginParticipation::Unknown),
            "unknown"
        );
        assert_eq!(
            participation_wire_name(PlayerPluginParticipation::Available),
            "available"
        );
        assert_eq!(
            participation_wire_name(PlayerPluginParticipation::Selected),
            "selected"
        );
        assert_eq!(
            participation_wire_name(PlayerPluginParticipation::Participated),
            "participated"
        );
        assert_eq!(
            participation_wire_name(PlayerPluginParticipation::Bypassed),
            "bypassed"
        );
        assert_eq!(
            participation_wire_name(PlayerPluginParticipation::Fallback),
            "fallback"
        );
    }

    #[test]
    fn prefer_normalized_native_first_bypasses_standard_adaptive_sources() {
        let opened = open_mobile_source_normalizer_resource(
            &MediaSource::new("https://cdn.example.test/master.m3u8"),
            &MobileSourceNormalizerConfiguration {
                mode: SourceNormalizerMode::PreferNormalized,
                plugin_library_paths: vec![PathBuf::from("/missing/source-normalizer.so")],
                runtime_profile: None,
            },
            std::env::temp_dir().display().to_string(),
            MobileSourceNormalizerRouteDecision::NativeFirst,
        )
        .expect("prefer native-first bypass should not error");

        assert!(opened.is_none());
    }

    #[test]
    fn prefer_normalized_native_first_bypasses_standard_progressive_mp4() {
        let decision = mobile_source_normalizer_playback_decision(
            &MediaSource::new("https://cdn.example.test/video.mp4?token=abc"),
            &MobileSourceNormalizerConfiguration {
                mode: SourceNormalizerMode::PreferNormalized,
                runtime_profile: None,
                plugin_library_paths: vec![PathBuf::from("/missing/source-normalizer.so")],
            },
            MobileSourceNormalizerRouteDecision::NativeFirst,
        );

        assert_eq!(
            decision.action,
            MobileSourceNormalizerPlaybackAction::BypassNativeFirst
        );
    }

    #[test]
    fn explicit_profile_can_try_normalized_progressive_source() {
        let decision = mobile_source_normalizer_playback_decision(
            &MediaSource::new("https://cdn.example.test/video.mp4"),
            &MobileSourceNormalizerConfiguration {
                mode: SourceNormalizerMode::PreferNormalized,
                runtime_profile: Some("generic-fallback".to_owned()),
                plugin_library_paths: vec![PathBuf::from("/missing/source-normalizer.so")],
            },
            MobileSourceNormalizerRouteDecision::NativeFirst,
        );

        assert_eq!(
            decision.action,
            MobileSourceNormalizerPlaybackAction::TryNormalized
        );
    }

    #[test]
    fn mobile_resource_selection_uses_explicit_runtime_profile() {
        let registry = PluginRegistry::from_records(vec![
            mobile_resource_record("generic-resource", &["generic-fallback"]),
            mobile_resource_record("flv-resource", &["flv"]),
        ]);
        let configuration = MobileSourceNormalizerConfiguration {
            mode: SourceNormalizerMode::PreferNormalized,
            runtime_profile: Some("FLV".to_owned()),
            plugin_library_paths: vec![PathBuf::from("/plugins/generic.so")],
        };

        assert_eq!(
            best_mobile_source_normalizer_resource(&registry, &configuration)
                .and_then(|record| record.plugin_name.as_deref()),
            Some("flv-resource")
        );
    }

    #[test]
    fn mobile_resource_selection_ignores_packet_only_profile_match() {
        let registry = PluginRegistry::from_records(vec![
            mobile_packet_record("packet-only", &["flv"]),
            mobile_resource_record("resource-generic", &["generic-fallback"]),
        ]);
        let configuration = MobileSourceNormalizerConfiguration {
            mode: SourceNormalizerMode::PreferNormalized,
            runtime_profile: Some("flv".to_owned()),
            plugin_library_paths: vec![PathBuf::from("/plugins/packet.so")],
        };

        assert!(
            best_mobile_source_normalizer_resource(&registry, &configuration).is_none(),
            "mobile resource playback must not use a packet-only SourceNormalizer"
        );
        assert!(
            source_normalizer_resource_selection_failure_message(
                "resource probe",
                &registry,
                &configuration,
            )
            .contains("runtime profile 'flv'")
        );
    }

    #[test]
    fn source_normalizer_probe_bypasses_standard_hls_without_opening_resource() {
        let diagnostics = source_normalizer_diagnostics(
            &MediaSource::new("https://cdn.example.test/master.m3u8"),
            &MobileSourceNormalizerConfiguration {
                mode: SourceNormalizerMode::PreferNormalized,
                plugin_library_paths: vec![PathBuf::from("/missing/source-normalizer.so")],
                runtime_profile: Some("generic-fallback".to_owned()),
            },
        );

        assert!(
            diagnostics.iter().any(|diagnostic| {
                diagnostic.participation == PlayerPluginParticipation::Bypassed
                    && diagnostic
                        .message
                        .as_deref()
                        .map(|message| {
                            message.contains("route=systemPlayer")
                                && message.contains("standard source stays native-first")
                        })
                        .unwrap_or(false)
            }),
            "standard HLS should stay native-first in preferNormalized diagnostics"
        );
    }

    #[test]
    fn prefer_normalized_native_first_bypasses_standard_dash_sources() {
        let opened = open_mobile_source_normalizer_resource(
            &MediaSource::new("https://cdn.example.test/manifest.mpd"),
            &MobileSourceNormalizerConfiguration {
                mode: SourceNormalizerMode::PreferNormalized,
                plugin_library_paths: vec![PathBuf::from("/missing/source-normalizer.so")],
                runtime_profile: Some("generic-fallback".to_owned()),
            },
            std::env::temp_dir().display().to_string(),
            MobileSourceNormalizerRouteDecision::NativeFirst,
        )
        .expect("prefer native-first DASH bypass should not error");

        assert!(
            opened.is_none(),
            "standard DASH stays native-first; generic fallback must not force normalization"
        );
    }

    #[test]
    fn require_normalized_overrides_native_first_dash_bypass() {
        let result = open_mobile_source_normalizer_resource(
            &MediaSource::new("https://cdn.example.test/manifest.mpd"),
            &MobileSourceNormalizerConfiguration {
                mode: SourceNormalizerMode::RequireNormalized,
                plugin_library_paths: Vec::new(),
                runtime_profile: Some("generic-fallback".to_owned()),
            },
            std::env::temp_dir().display().to_string(),
            MobileSourceNormalizerRouteDecision::NativeFirst,
        );
        let Err(error) = result else {
            panic!("requireNormalized must try normalized route even for standard DASH");
        };

        assert!(error.contains("no plugin paths"));
    }

    #[test]
    fn require_normalized_errors_when_plugin_paths_are_missing() {
        let result = open_mobile_source_normalizer_resource(
            &MediaSource::new("file:///tmp/input.flv"),
            &MobileSourceNormalizerConfiguration {
                mode: SourceNormalizerMode::RequireNormalized,
                ..MobileSourceNormalizerConfiguration::default()
            },
            std::env::temp_dir().display().to_string(),
            MobileSourceNormalizerRouteDecision::Force,
        );
        let Err(error) = result else {
            panic!("requireNormalized must fail when no plugin is available");
        };

        assert!(error.contains("no plugin paths"));
    }

    #[test]
    fn source_normalizer_smoke_matrix_documents_full_loop_expectations() {
        let manifest =
            include_str!("../../../../../fixtures/media/source-normalizer-smoke-matrix.json");
        let value: Value = serde_json::from_str(manifest).expect("parse smoke matrix");
        assert_eq!(
            value["generatedBy"],
            "scripts/media/generate-source-normalizer-smoke-fixtures.sh"
        );
        let cases = value["cases"].as_array().expect("cases array");

        for required in [
            "standard-progressive-mp4",
            "standard-hls-native-first",
            "standard-dash-native-first",
            "flv-to-fmp4-local-stream",
            "hevc-flv-to-fmp4-local-stream",
            "broken-progressive-mp4-to-fmp4-local-stream",
            "nonstandard-hls-short-window",
            "weird-dash-short-window-fallback",
            "require-normalized-missing-plugin-fails",
        ] {
            assert!(
                cases.iter().any(|case| case["id"] == required),
                "missing smoke matrix case {required}"
            );
        }

        let standard_hls = cases
            .iter()
            .find(|case| case["id"] == "standard-hls-native-first")
            .expect("standard HLS case");
        assert_eq!(standard_hls["expectedRoute"], "native");
        assert_eq!(standard_hls["expectedParticipation"], "bypassed");

        let flv = cases
            .iter()
            .find(|case| case["id"] == "flv-to-fmp4-local-stream")
            .expect("FLV case");
        assert_eq!(flv["expectedRoute"], "fmp4LocalStream");
        assert_eq!(flv["expectedParticipation"], "participated");

        let weird_dash = cases
            .iter()
            .find(|case| case["id"] == "weird-dash-short-window-fallback")
            .expect("weird DASH case");
        assert_eq!(weird_dash["expectedRoute"], "hlsShortWindow");
        assert_eq!(weird_dash["decision"], "force");

        for case in cases
            .iter()
            .filter(|case| case.get("fixtureGeneration").is_some())
        {
            let required_files = case["requiredFiles"]
                .as_array()
                .unwrap_or_else(|| panic!("generated case {} must list requiredFiles", case["id"]));
            assert!(
                !required_files.is_empty(),
                "generated case {} must document generated artifacts",
                case["id"]
            );
            for required_file in required_files {
                let path = required_file.as_str().expect("required file path");
                assert!(
                    path.starts_with("fixtures/media/generated/"),
                    "generated fixture path must stay under fixtures/media/generated/: {path}"
                );
                let repo_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("../../../../../")
                    .join(path);
                if !repo_path.exists() {
                    eprintln!(
                        "skipping generated smoke fixture {path}; run scripts/media/generate-source-normalizer-smoke-fixtures.sh"
                    );
                }
            }
        }
    }

    #[test]
    fn fmp4_readiness_requires_init_and_first_fragment_markers() {
        let directory =
            std::env::temp_dir().join(format!("vesper-fmp4-readiness-{}", std::process::id()));
        std::fs::create_dir_all(&directory).expect("create temp dir");
        let path = directory.join("normalized.mp4");

        std::fs::write(&path, b"\0\0\0 ftyp\0\0\0 moov").expect("write init-only fmp4");
        assert!(!fmp4_local_stream_ready(path.to_str().expect("utf8 path")));

        std::fs::write(
            &path,
            b"\0\0\0 ftyp\0\0\0 moov\0\0\0 moof\0\0\0 mdatpayload",
        )
        .expect("write fragmented fmp4");
        assert!(fmp4_local_stream_ready(path.to_str().expect("utf8 path")));

        let _ = std::fs::remove_dir_all(directory);
    }

    #[test]
    fn fmp4_box_marker_scan_is_bounded() {
        let directory = std::env::temp_dir().join(format!(
            "vesper-fmp4-readiness-scan-limit-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&directory).expect("create temp dir");
        let path = directory.join("late-marker.mp4");
        let mut bytes = vec![0_u8; FMP4_BOX_MARKER_SCAN_LIMIT_BYTES as usize + 16];
        bytes[FMP4_BOX_MARKER_SCAN_LIMIT_BYTES as usize + 4..][..4].copy_from_slice(b"ftyp");
        std::fs::write(&path, bytes).expect("write oversized marker fixture");

        assert!(!file_contains_box_marker(
            path.to_str().expect("utf8 path"),
            &[b"ftyp"]
        ));

        let _ = std::fs::remove_dir_all(directory);
    }

    #[test]
    fn hls_short_window_readiness_requires_playlist_init_and_media_segment() {
        let directory =
            std::env::temp_dir().join(format!("vesper-hls-readiness-{}", std::process::id()));
        std::fs::create_dir_all(&directory).expect("create temp dir");
        let playlist = directory.join("index.m3u8");
        let init = directory.join("init.mp4");
        let segment = directory.join("segment_00001.m4s");
        std::fs::write(
            &playlist,
            "#EXTM3U\n#EXT-X-MAP:URI=\"init.mp4\"\n#EXTINF:3,\nsegment_00001.m4s\n",
        )
        .expect("write playlist");
        std::fs::write(&init, b"init").expect("write init");

        let init_only = vec![player_plugin::SourceNormalizerResourceInfo {
            role: "segment".to_owned(),
            path: init.display().to_string(),
            content_type: Some("video/mp4".to_owned()),
            byte_length: Some(4),
            growing: false,
        }];
        assert!(!hls_short_window_ready(
            playlist.to_str().expect("utf8 path"),
            &init_only
        ));

        std::fs::write(&segment, b"segment").expect("write segment");
        let ready = vec![
            init_only[0].clone(),
            player_plugin::SourceNormalizerResourceInfo {
                role: "segment".to_owned(),
                path: segment.display().to_string(),
                content_type: Some("video/mp4".to_owned()),
                byte_length: Some(7),
                growing: false,
            },
        ];
        assert!(hls_short_window_ready(
            playlist.to_str().expect("utf8 path"),
            &ready
        ));

        let _ = std::fs::remove_dir_all(directory);
    }

    #[test]
    fn resource_readiness_wait_has_total_timeout_without_sleep_polling() {
        let mut session = TestResourceSession::running();
        let started = Instant::now();
        let result =
            wait_for_resource_session_ready_with_policy(&mut session, Duration::from_millis(5));

        let Err(error) = result else {
            panic!("never-ready session should time out");
        };
        assert!(error.contains("readable primary resource"));
        assert!(
            started.elapsed() < Duration::from_secs(1),
            "test timeout path should not wait for the production 10s timeout"
        );
        assert!(session.polls > 0);
        assert!(session.waits > 0);
    }

    #[test]
    fn resource_readiness_wait_returns_after_notified_ready_status() {
        let mut session = TestResourceSession::running();
        let signal = session.signal();
        let updater = std::thread::spawn(move || {
            signal.wait_until_waiting(Duration::from_secs(1));
            signal.set_status(test_resource_status(
                SourceNormalizerResourceSessionState::Ready,
                Some(resource_info_with_video_track(None)),
                None,
            ));
        });

        let status =
            wait_for_resource_session_ready_with_policy(&mut session, Duration::from_secs(1))
                .expect("ready update should return");

        updater.join().expect("updater joins");
        assert_eq!(status.state, SourceNormalizerResourceSessionState::Ready);
        assert!(session.waits > 0);
    }

    #[test]
    fn resource_readiness_wait_returns_running_status_when_primary_bytes_are_ready() {
        let mut session = TestResourceSession::running();
        let directory = std::env::temp_dir().join(format!(
            "vesper-mobile-resource-ready-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&directory).expect("create resource readiness fixture directory");
        let primary = directory.join("normalized.mp4");
        std::fs::write(&primary, fmp4_ready_fixture_bytes()).expect("write fmp4 readiness fixture");
        let signal = session.signal();
        let primary_path = primary.display().to_string();
        let updater = std::thread::spawn(move || {
            signal.wait_until_waiting(Duration::from_secs(1));
            signal.set_status(test_resource_status(
                SourceNormalizerResourceSessionState::Running,
                Some(resource_info_with_primary_file(&primary_path)),
                None,
            ));
        });

        let status =
            wait_for_resource_session_ready_with_policy(&mut session, Duration::from_secs(1))
                .expect("primary bytes should make running status readable");

        updater.join().expect("updater joins");
        assert_eq!(status.state, SourceNormalizerResourceSessionState::Running);
        assert!(resource_status_has_primary_bytes(&status));
        let _ = std::fs::remove_dir_all(directory);
    }

    #[test]
    fn resource_readiness_wait_reports_notified_failed_status() {
        let mut session = TestResourceSession::running();
        let signal = session.signal();
        let updater = std::thread::spawn(move || {
            signal.wait_until_waiting(Duration::from_secs(1));
            signal.set_status(test_resource_status(
                SourceNormalizerResourceSessionState::Failed,
                None,
                Some("fixture failure".to_owned()),
            ));
        });

        let error =
            wait_for_resource_session_ready_with_policy(&mut session, Duration::from_secs(1))
                .expect_err("failed status should return error");

        updater.join().expect("updater joins");
        assert_eq!(error, "fixture failure");
    }

    #[test]
    fn resource_readiness_wait_reports_notified_cancelled_status() {
        let mut session = TestResourceSession::running();
        let signal = session.signal();
        let updater = std::thread::spawn(move || {
            signal.wait_until_waiting(Duration::from_secs(1));
            signal.set_status(test_resource_status(
                SourceNormalizerResourceSessionState::Cancelled,
                None,
                Some("fixture cancelled".to_owned()),
            ));
        });

        let error =
            wait_for_resource_session_ready_with_policy(&mut session, Duration::from_secs(1))
                .expect_err("cancelled status should return error");

        updater.join().expect("updater joins");
        assert_eq!(error, "fixture cancelled");
    }

    #[test]
    fn fmp4_resource_hdr_metadata_requires_bypass() {
        let mut info = resource_info_with_video_track(Some(SourceNormalizerPacketTrackInfo {
            color: Some(NativeFrameColorMetadata {
                primaries: Some("bt2020".to_owned()),
                transfer: Some("smpte2084".to_owned()),
                matrix: Some("bt2020-ncl".to_owned()),
                range: Some("limited".to_owned()),
                bit_depth: Some(10),
            }),
            hdr: Some(NativeFrameHdrMetadata {
                kind: "hdr10".to_owned(),
                mastering_display: None,
                content_light: None,
                dolby_vision: None,
            }),
            ..video_track("hevc")
        }));

        let reason = hdr_resource_metadata_not_preserved_reason(&info)
            .expect("HDR fMP4 resource should be bypassed");
        assert!(reason.contains(HDR_RESOURCE_METADATA_NOT_PRESERVED));

        info.output_route = SourceNormalizerOutputRoute::HlsShortWindow;
        assert!(hdr_resource_metadata_not_preserved_reason(&info).is_none());
    }

    #[test]
    fn fmp4_resource_dolby_vision_metadata_requires_bypass() {
        let info = resource_info_with_video_track(Some(SourceNormalizerPacketTrackInfo {
            hdr: Some(NativeFrameHdrMetadata {
                kind: "dolbyVision".to_owned(),
                mastering_display: None,
                content_light: None,
                dolby_vision: None,
            }),
            ..video_track("hevc")
        }));

        assert!(
            hdr_resource_metadata_not_preserved_reason(&info)
                .is_some_and(|reason| reason.contains("Dolby Vision"))
        );
    }

    #[test]
    fn fmp4_resource_dolby_vision_codec_alias_requires_bypass() {
        for codec in ["dvh1.05.06", "dvhe.08.04", "video/dvh1.05.06"] {
            let info = resource_info_with_video_track(Some(SourceNormalizerPacketTrackInfo {
                hdr: None,
                color: None,
                ..video_track(codec)
            }));

            assert!(
                hdr_resource_metadata_not_preserved_reason(&info)
                    .is_some_and(|reason| reason.contains("Dolby Vision")),
                "{codec} should bypass fMP4 resource playback"
            );
        }
    }

    #[test]
    fn dolby_vision_codec_alias_requires_native_frame_hdr_bypass() {
        let track = SourceNormalizerPacketTrackInfo {
            hdr: None,
            color: None,
            ..video_track("dvhe.05.06")
        };

        assert!(track_requires_hdr_or_dolby_vision_metadata(&track));
        assert!(
            hdr_programmable_processing_not_supported_reason(&track)
                .is_some_and(|reason| reason.contains(HDR_PROGRAMMABLE_PROCESSING_NOT_SUPPORTED))
        );
    }

    #[test]
    fn fmp4_resource_sdr_metadata_stays_supported() {
        let info = resource_info_with_video_track(Some(SourceNormalizerPacketTrackInfo {
            color: Some(NativeFrameColorMetadata {
                primaries: Some("bt709".to_owned()),
                transfer: Some("bt709".to_owned()),
                matrix: Some("bt709".to_owned()),
                range: Some("limited".to_owned()),
                bit_depth: Some(8),
            }),
            ..video_track("h264")
        }));

        assert!(hdr_resource_metadata_not_preserved_reason(&info).is_none());
    }

    fn test_source_normalizer_diagnostic(
        status: PlayerPluginDiagnosticStatus,
        message: &str,
    ) -> PlayerPluginDiagnostic {
        runtime_source_normalizer_diagnostic(
            "/plugins/source-normalizer.so".to_owned(),
            Some("source-normalizer".to_owned()),
            status,
            message,
            PlayerPluginParticipation::Bypassed,
        )
    }

    fn mobile_resource_record(name: &str, profiles: &[&str]) -> PluginDiagnosticRecord {
        PluginDiagnosticRecord {
            path: PathBuf::from(format!("/plugins/{name}.so")),
            status: PluginDiagnosticStatus::SourceNormalizerSupported,
            plugin_name: Some(name.to_owned()),
            plugin_kind: Some(player_plugin::VesperPluginKind::SourceNormalizer),
            capability_summary: Some(PluginCapabilitySummary::SourceNormalizerResource(
                SourceNormalizerResourcePluginCapabilitySummary {
                    supported_runtime_profiles: profiles
                        .iter()
                        .map(|profile| (*profile).to_owned())
                        .collect(),
                    supported_output_routes: vec!["fmp4LocalStream".to_owned()],
                    max_level: SourceNormalizerNormalizeLevel::RemuxOnly,
                    content_types: vec!["video/mp4".to_owned()],
                    supports_growing_resources: true,
                    supports_range_reads: true,
                    supports_cancel: true,
                    required_capabilities: SourceNormalizerRequiredCapabilities::default(),
                    cache_policy: SourceNormalizerResourceCachePolicy::default(),
                    max_sessions: Some(1),
                },
            )),
            message: Some("source normalizer resource route".to_owned()),
        }
    }

    fn mobile_packet_record(name: &str, profiles: &[&str]) -> PluginDiagnosticRecord {
        PluginDiagnosticRecord {
            path: PathBuf::from(format!("/plugins/{name}.so")),
            status: PluginDiagnosticStatus::SourceNormalizerSupported,
            plugin_name: Some(name.to_owned()),
            plugin_kind: Some(player_plugin::VesperPluginKind::SourceNormalizer),
            capability_summary: Some(PluginCapabilitySummary::SourceNormalizerPacket(
                SourceNormalizerPacketPluginCapabilitySummary {
                    supported_runtime_profiles: profiles
                        .iter()
                        .map(|profile| (*profile).to_owned())
                        .collect(),
                    max_level: SourceNormalizerNormalizeLevel::RemuxOnly,
                    media_kinds: vec![SourceNormalizerPacketMediaKind::Video],
                    codecs: vec!["h264".to_owned()],
                    bitstream_formats: vec![DecoderBitstreamFormat::AnnexB],
                    supports_seek: true,
                    supports_flush: true,
                    required_capabilities: SourceNormalizerRequiredCapabilities::default(),
                    max_sessions: Some(1),
                },
            )),
            message: Some("source normalizer packet route".to_owned()),
        }
    }

    fn resource_info_with_video_track(
        track: Option<SourceNormalizerPacketTrackInfo>,
    ) -> SourceNormalizerResourceSessionInfo {
        SourceNormalizerResourceSessionInfo {
            session_id: Some("resource".to_owned()),
            normalizer_name: Some("test".to_owned()),
            runtime_profile: Some("test".to_owned()),
            selected_backend: None,
            output_route: SourceNormalizerOutputRoute::Fmp4LocalStream,
            container: "mp4".to_owned(),
            primary_resource_path: Some("/tmp/normalized.mp4".to_owned()),
            primary_content_type: Some("video/mp4".to_owned()),
            resources: Vec::new(),
            tracks: track.into_iter().collect(),
            duration_millis: Some(1_000),
            seekable: true,
            disk_bytes_used: Some(128),
        }
    }

    fn video_track(codec: &str) -> SourceNormalizerPacketTrackInfo {
        SourceNormalizerPacketTrackInfo {
            stream_index: 0,
            media_kind: SourceNormalizerPacketMediaKind::Video,
            codec: codec.to_owned(),
            extradata: Vec::new(),
            bitstream_format: None,
            width: Some(1920),
            height: Some(1080),
            coded_width: Some(1920),
            coded_height: Some(1080),
            sample_rate: None,
            channels: None,
            channel_layout: None,
            codec_delay_samples: None,
            priming_samples: None,
            trailing_padding_samples: None,
            seek_preroll_samples: None,
            color: None,
            hdr: None,
            frame_rate: Some(30.0),
            time_base_num: Some(1),
            time_base_den: Some(90_000),
        }
    }

    #[derive(Clone)]
    struct TestResourceSessionSignal {
        shared: Arc<TestResourceSessionShared>,
    }

    struct TestResourceSession {
        shared: Arc<TestResourceSessionShared>,
        polls: usize,
        waits: usize,
    }

    struct TestResourceSessionShared {
        state: Mutex<TestResourceSessionState>,
        changed: std::sync::Condvar,
        wait_entered: Mutex<bool>,
        wait_entered_changed: std::sync::Condvar,
    }

    struct TestResourceSessionState {
        status: SourceNormalizerResourceSessionStatus,
        sequence: u64,
    }

    impl TestResourceSession {
        fn running() -> Self {
            Self {
                shared: Arc::new(TestResourceSessionShared {
                    state: Mutex::new(TestResourceSessionState {
                        status: test_resource_status(
                            SourceNormalizerResourceSessionState::Running,
                            Some(resource_info_with_video_track(None)),
                            None,
                        ),
                        sequence: 0,
                    }),
                    changed: std::sync::Condvar::new(),
                    wait_entered: Mutex::new(false),
                    wait_entered_changed: std::sync::Condvar::new(),
                }),
                polls: 0,
                waits: 0,
            }
        }

        fn signal(&self) -> TestResourceSessionSignal {
            TestResourceSessionSignal {
                shared: self.shared.clone(),
            }
        }
    }

    impl TestResourceSessionSignal {
        fn set_status(&self, status: SourceNormalizerResourceSessionStatus) {
            let mut state = self
                .shared
                .state
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            state.status = status;
            state.sequence = state.sequence.wrapping_add(1);
            self.shared.changed.notify_all();
        }

        fn wait_until_waiting(&self, timeout: Duration) {
            let waiting = self
                .shared
                .wait_entered
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            if *waiting {
                return;
            }
            let (waiting, _result) = self
                .shared
                .wait_entered_changed
                .wait_timeout(waiting, timeout)
                .unwrap_or_else(|error| error.into_inner());
            assert!(*waiting, "resource readiness wait was not entered");
        }
    }

    impl SourceNormalizerResourceSession for TestResourceSession {
        fn session_info(&self) -> SourceNormalizerResourceSessionInfo {
            resource_info_with_video_track(None)
        }

        fn poll(
            &mut self,
        ) -> Result<SourceNormalizerResourceSessionStatus, player_plugin::SourceNormalizerError>
        {
            self.polls += 1;
            Ok(self
                .shared
                .state
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .status
                .clone())
        }

        fn wait_for_update(
            &mut self,
            timeout: Duration,
        ) -> Result<SourceNormalizerResourceSessionWaitStatus, player_plugin::SourceNormalizerError>
        {
            self.waits += 1;
            let state = self
                .shared
                .state
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            let observed = state.sequence;
            if timeout.is_zero() {
                return Ok(SourceNormalizerResourceSessionWaitStatus { updated: false });
            }
            {
                let mut wait_entered = self
                    .shared
                    .wait_entered
                    .lock()
                    .unwrap_or_else(|error| error.into_inner());
                *wait_entered = true;
                self.shared.wait_entered_changed.notify_all();
            }
            let (state, _result) = self
                .shared
                .changed
                .wait_timeout(state, timeout)
                .unwrap_or_else(|error| error.into_inner());
            Ok(SourceNormalizerResourceSessionWaitStatus {
                updated: state.sequence != observed,
            })
        }

        fn cancel(
            &mut self,
        ) -> Result<
            player_plugin::SourceNormalizerOperationStatus,
            player_plugin::SourceNormalizerError,
        > {
            Ok(player_plugin::SourceNormalizerOperationStatus {
                completed: true,
                message: None,
            })
        }

        fn close(&mut self) -> Result<(), player_plugin::SourceNormalizerError> {
            Ok(())
        }
    }

    fn test_resource_status(
        state: SourceNormalizerResourceSessionState,
        info: Option<SourceNormalizerResourceSessionInfo>,
        message: Option<String>,
    ) -> SourceNormalizerResourceSessionStatus {
        SourceNormalizerResourceSessionStatus {
            state,
            info,
            message,
            disk_bytes_used: Some(0),
        }
    }

    fn resource_info_with_primary_file(primary_path: &str) -> SourceNormalizerResourceSessionInfo {
        let mut info = resource_info_with_video_track(None);
        info.primary_resource_path = Some(primary_path.to_owned());
        info.resources = vec![player_plugin::SourceNormalizerResourceInfo {
            role: "media".to_owned(),
            path: primary_path.to_owned(),
            content_type: Some("video/mp4".to_owned()),
            byte_length: Some(48),
            growing: true,
        }];
        info
    }

    fn fmp4_ready_fixture_bytes() -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"\0\0\0\x18ftypisom\0\0\0\0isomiso2");
        bytes.extend_from_slice(b"\0\0\0\x08moov");
        bytes.extend_from_slice(b"\0\0\0\x08moof");
        bytes.extend_from_slice(b"\0\0\0\x08mdat");
        bytes
    }
}
