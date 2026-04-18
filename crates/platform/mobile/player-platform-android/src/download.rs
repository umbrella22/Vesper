use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use player_plugin::ProcessorProgress;
use player_plugin_loader::LoadedDynamicPlugin;
use player_runtime::{
    DownloadAssetId, DownloadAssetIndex, DownloadEvent, DownloadExecutor, DownloadManager,
    DownloadManagerConfig, DownloadProfile, DownloadSnapshot, DownloadSource, DownloadTaskId,
    DownloadTaskSnapshot, InMemoryDownloadStore, PlayerRuntimeError, PlayerRuntimeErrorCategory,
    PlayerRuntimeErrorCode, PlayerRuntimeResult,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AndroidDownloadCommand {
    Start { task: DownloadTaskSnapshot },
    Pause { task_id: DownloadTaskId },
    Resume { task: DownloadTaskSnapshot },
    Remove { task_id: DownloadTaskId },
}

#[derive(Debug, Clone)]
struct AndroidDownloadExecutor {
    queue: Arc<Mutex<VecDeque<AndroidDownloadCommand>>>,
}

impl AndroidDownloadExecutor {
    fn new(queue: Arc<Mutex<VecDeque<AndroidDownloadCommand>>>) -> Self {
        Self { queue }
    }

    fn push_command(&self, command: AndroidDownloadCommand) -> PlayerRuntimeResult<()> {
        let mut queue = self.queue.lock().map_err(|_| {
            PlayerRuntimeError::with_category(
                PlayerRuntimeErrorCode::BackendFailure,
                PlayerRuntimeErrorCategory::Platform,
                "android download command queue lock poisoned",
            )
        })?;
        queue.push_back(command);
        Ok(())
    }
}

impl DownloadExecutor for AndroidDownloadExecutor {
    fn prepare(&mut self, _task: &DownloadTaskSnapshot) -> PlayerRuntimeResult<()> {
        Ok(())
    }

    fn start(&mut self, task: &DownloadTaskSnapshot) -> PlayerRuntimeResult<()> {
        self.push_command(AndroidDownloadCommand::Start { task: task.clone() })
    }

    fn pause(&mut self, task_id: DownloadTaskId) -> PlayerRuntimeResult<()> {
        self.push_command(AndroidDownloadCommand::Pause { task_id })
    }

    fn resume(&mut self, task: &DownloadTaskSnapshot) -> PlayerRuntimeResult<()> {
        self.push_command(AndroidDownloadCommand::Resume { task: task.clone() })
    }

    fn remove(&mut self, task_id: DownloadTaskId) -> PlayerRuntimeResult<()> {
        self.push_command(AndroidDownloadCommand::Remove { task_id })
    }
}

#[derive(Debug)]
pub struct AndroidDownloadBridgeSession {
    manager: DownloadManager<InMemoryDownloadStore, AndroidDownloadExecutor>,
    command_queue: Arc<Mutex<VecDeque<AndroidDownloadCommand>>>,
}

impl AndroidDownloadBridgeSession {
    pub fn new(auto_start: bool) -> Self {
        let command_queue = Arc::new(Mutex::new(VecDeque::new()));
        let executor = AndroidDownloadExecutor::new(command_queue.clone());
        let config = DownloadManagerConfig {
            auto_start,
            run_post_processors_on_completion: true,
            post_processors: Vec::new(),
            event_hooks: Vec::new(),
        };

        Self {
            manager: DownloadManager::new(config, InMemoryDownloadStore::default(), executor),
            command_queue,
        }
    }

    pub fn new_with_plugin_library_paths(
        auto_start: bool,
        run_post_processors_on_completion: bool,
        plugin_library_paths: impl IntoIterator<Item = PathBuf>,
    ) -> PlayerRuntimeResult<Self> {
        let command_queue = Arc::new(Mutex::new(VecDeque::new()));
        let executor = AndroidDownloadExecutor::new(command_queue.clone());
        let config = download_manager_config(
            auto_start,
            run_post_processors_on_completion,
            plugin_library_paths,
        )?;

        Ok(Self {
            manager: DownloadManager::new(config, InMemoryDownloadStore::default(), executor),
            command_queue,
        })
    }

    pub fn create_task(
        &mut self,
        asset_id: impl Into<String>,
        source: DownloadSource,
        profile: DownloadProfile,
        asset_index: DownloadAssetIndex,
        now: Instant,
    ) -> PlayerRuntimeResult<DownloadTaskId> {
        self.manager
            .create_task(asset_id, source, profile, asset_index, now)
    }

    pub fn start_task(
        &mut self,
        task_id: DownloadTaskId,
        now: Instant,
    ) -> PlayerRuntimeResult<Option<DownloadTaskSnapshot>> {
        self.manager.start_task(task_id, now)
    }

    pub fn pause_task(
        &mut self,
        task_id: DownloadTaskId,
        now: Instant,
    ) -> PlayerRuntimeResult<Option<DownloadTaskSnapshot>> {
        self.manager.pause_task(task_id, now)
    }

    pub fn resume_task(
        &mut self,
        task_id: DownloadTaskId,
        now: Instant,
    ) -> PlayerRuntimeResult<Option<DownloadTaskSnapshot>> {
        self.manager.resume_task(task_id, now)
    }

    pub fn update_progress(
        &mut self,
        task_id: DownloadTaskId,
        received_bytes: u64,
        received_segments: u32,
        now: Instant,
    ) -> PlayerRuntimeResult<Option<DownloadTaskSnapshot>> {
        self.manager
            .update_progress(task_id, received_bytes, received_segments, now)
    }

    pub fn complete_task(
        &mut self,
        task_id: DownloadTaskId,
        completed_path: Option<std::path::PathBuf>,
        now: Instant,
    ) -> PlayerRuntimeResult<Option<DownloadTaskSnapshot>> {
        self.manager.complete_task(task_id, completed_path, now)
    }

    pub fn fail_task(
        &mut self,
        task_id: DownloadTaskId,
        error: PlayerRuntimeError,
        now: Instant,
    ) -> PlayerRuntimeResult<Option<DownloadTaskSnapshot>> {
        self.manager.fail_task(task_id, error, now)
    }

    pub fn remove_task(
        &mut self,
        task_id: DownloadTaskId,
        now: Instant,
    ) -> PlayerRuntimeResult<Option<DownloadTaskSnapshot>> {
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
    ) -> PlayerRuntimeResult<PathBuf> {
        self.manager
            .export_task_output(task_id, output_path.as_deref(), progress)
    }

    pub fn drain_events(&mut self) -> Vec<DownloadEvent> {
        self.manager.drain_events()
    }

    pub fn drain_commands(&mut self) -> Vec<AndroidDownloadCommand> {
        self.command_queue
            .lock()
            .map(|mut queue| queue.drain(..).collect())
            .unwrap_or_default()
    }
}

fn download_manager_config(
    auto_start: bool,
    run_post_processors_on_completion: bool,
    plugin_library_paths: impl IntoIterator<Item = PathBuf>,
) -> PlayerRuntimeResult<DownloadManagerConfig> {
    let mut post_processors = Vec::new();
    let mut event_hooks = Vec::new();

    for path in plugin_library_paths {
        let plugin = LoadedDynamicPlugin::load(&path).map_err(|error| {
            PlayerRuntimeError::with_category(
                PlayerRuntimeErrorCode::InvalidArgument,
                PlayerRuntimeErrorCategory::Input,
                format!(
                    "failed to load android download plugin `{}`: {error}",
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

#[cfg(test)]
mod tests {
    use super::{AndroidDownloadBridgeSession, AndroidDownloadCommand};
    use player_core::MediaSource;
    use player_runtime::{
        DownloadAssetId, DownloadAssetIndex, DownloadContentFormat, DownloadProfile,
        DownloadSource, DownloadTaskStatus, PlayerRuntimeError, PlayerRuntimeErrorCategory,
        PlayerRuntimeErrorCode,
    };
    use std::path::PathBuf;
    use std::time::Instant;

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

    #[test]
    fn android_download_bridge_emits_start_pause_resume_and_remove_commands() {
        let now = Instant::now();
        let mut session = AndroidDownloadBridgeSession::new(true);
        let task_id = session
            .create_task(
                "asset-a",
                source("https://example.com/a.m3u8"),
                DownloadProfile::default(),
                asset_index(1_024),
                now,
            )
            .expect("task should be created");

        let commands = session.drain_commands();
        assert_eq!(commands.len(), 1);
        assert!(matches!(
            &commands[0],
            AndroidDownloadCommand::Start { task } if task.task_id == task_id
        ));

        let paused = session
            .pause_task(task_id, now)
            .expect("pause should succeed")
            .expect("task should exist");
        assert_eq!(paused.status, DownloadTaskStatus::Paused);
        assert_eq!(
            session.drain_commands(),
            vec![AndroidDownloadCommand::Pause { task_id }]
        );

        let resumed = session
            .resume_task(task_id, now)
            .expect("resume should succeed")
            .expect("task should exist");
        assert_eq!(resumed.status, DownloadTaskStatus::Downloading);
        let commands = session.drain_commands();
        assert_eq!(commands.len(), 1);
        assert!(matches!(
            &commands[0],
            AndroidDownloadCommand::Resume { task } if task.task_id == task_id
        ));

        let removed = session
            .remove_task(task_id, now)
            .expect("remove should succeed")
            .expect("task should exist");
        assert_eq!(removed.status, DownloadTaskStatus::Removed);
        assert_eq!(
            session.drain_commands(),
            vec![AndroidDownloadCommand::Remove { task_id }]
        );
    }

    #[test]
    fn android_download_bridge_tracks_progress_completion_and_asset_lookup() {
        let now = Instant::now();
        let mut session = AndroidDownloadBridgeSession::new(false);
        let task_id = session
            .create_task(
                "asset-a",
                source("https://example.com/a.m3u8"),
                DownloadProfile::default(),
                asset_index(4_096),
                now,
            )
            .expect("task should be created");

        let created = session.task(task_id).expect("task should exist");
        assert_eq!(created.status, DownloadTaskStatus::Queued);

        let _ = session
            .start_task(task_id, now)
            .expect("start should succeed");
        let _ = session.drain_commands();

        let progressed = session
            .update_progress(task_id, 2_048, 5, now)
            .expect("progress should succeed")
            .expect("task should exist");
        assert_eq!(progressed.progress.received_bytes, 2_048);
        assert_eq!(progressed.progress.received_segments, 5);

        let completed = session
            .complete_task(task_id, Some(PathBuf::from("downloads/offline.mp4")), now)
            .expect("complete should succeed")
            .expect("task should exist");
        assert_eq!(completed.status, DownloadTaskStatus::Completed);
        assert_eq!(
            completed.asset_index.completed_path,
            Some(PathBuf::from("downloads/offline.mp4"))
        );

        let tasks = session.tasks_for_asset(&DownloadAssetId::new("asset-a"));
        assert_eq!(tasks.len(), 1);
        assert_eq!(tasks[0].status, DownloadTaskStatus::Completed);
    }

    #[test]
    fn android_download_bridge_records_failure_events() {
        let now = Instant::now();
        let mut session = AndroidDownloadBridgeSession::new(true);
        let task_id = session
            .create_task(
                "asset-a",
                source("https://example.com/a.m3u8"),
                DownloadProfile::default(),
                asset_index(512),
                now,
            )
            .expect("task should be created");

        let failed = session
            .fail_task(
                task_id,
                PlayerRuntimeError::new(PlayerRuntimeErrorCode::BackendFailure, "android failed"),
                now,
            )
            .expect("fail should succeed")
            .expect("task should exist");
        assert_eq!(failed.status, DownloadTaskStatus::Failed);
        assert_eq!(
            failed
                .error_summary
                .as_ref()
                .map(|summary| summary.message.as_str()),
            Some("android failed")
        );

        let events = session.drain_events();
        assert!(events.iter().any(|event| matches!(
            event,
            player_runtime::DownloadEvent::StateChanged(task)
                if task.status == DownloadTaskStatus::Failed
        )));
    }

    #[test]
    fn android_download_bridge_rejects_missing_plugin_library() {
        let error = AndroidDownloadBridgeSession::new_with_plugin_library_paths(
            false,
            true,
            vec![PathBuf::from("/tmp/vesper-missing-plugin.so")],
        )
        .expect_err("missing plugin should fail");

        assert_eq!(error.code(), PlayerRuntimeErrorCode::InvalidArgument);
        assert_eq!(error.category(), PlayerRuntimeErrorCategory::Input);
    }
}
