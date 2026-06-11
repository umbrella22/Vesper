use std::path::PathBuf;
use std::time::Instant;

use player_platform_mobile::{
    MobileDownloadBridgeSession, MobileDownloadCommand, mobile_download_manager_config,
};
use player_plugin::ProcessorProgress;
use player_runtime::{
    DownloadAssetId, DownloadAssetIndex, DownloadEvent, DownloadManagerConfig, DownloadProfile,
    DownloadSnapshot, DownloadSource, DownloadTaskId, DownloadTaskSnapshot, PlayerError,
    PlayerResult,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AndroidDownloadCommand {
    Prepare { task: DownloadTaskSnapshot },
    Start { task: DownloadTaskSnapshot },
    Pause { task_id: DownloadTaskId },
    Resume { task: DownloadTaskSnapshot },
    Remove { task_id: DownloadTaskId },
}

#[derive(Debug)]
pub struct AndroidDownloadBridgeSession {
    inner: MobileDownloadBridgeSession,
}

impl AndroidDownloadBridgeSession {
    pub fn new(auto_start: bool) -> Self {
        let config = DownloadManagerConfig {
            auto_start,
            run_post_processors_on_completion: true,
            post_processors: Vec::new(),
            event_hooks: Vec::new(),
        };

        Self {
            inner: MobileDownloadBridgeSession::new(config, "android download"),
        }
    }

    pub fn new_with_plugin_library_paths(
        auto_start: bool,
        run_post_processors_on_completion: bool,
        plugin_library_paths: impl IntoIterator<Item = PathBuf>,
    ) -> PlayerResult<Self> {
        let config = mobile_download_manager_config(
            "android",
            auto_start,
            run_post_processors_on_completion,
            plugin_library_paths,
        )?;

        Ok(Self {
            inner: MobileDownloadBridgeSession::new(config, "android download"),
        })
    }

    pub fn create_task(
        &mut self,
        asset_id: impl Into<String>,
        source: DownloadSource,
        profile: DownloadProfile,
        asset_index: DownloadAssetIndex,
        now: Instant,
    ) -> PlayerResult<DownloadTaskId> {
        self.inner
            .create_task(asset_id, source, profile, asset_index, now)
    }

    pub fn restore_tasks(
        &mut self,
        tasks: impl IntoIterator<Item = DownloadTaskSnapshot>,
        now: Instant,
    ) -> PlayerResult<Vec<DownloadTaskSnapshot>> {
        self.inner.restore_tasks(tasks, now)
    }

    pub fn start_task(
        &mut self,
        task_id: DownloadTaskId,
        now: Instant,
    ) -> PlayerResult<Option<DownloadTaskSnapshot>> {
        self.inner.start_task(task_id, now)
    }

    pub fn pause_task(
        &mut self,
        task_id: DownloadTaskId,
        now: Instant,
    ) -> PlayerResult<Option<DownloadTaskSnapshot>> {
        self.inner.pause_task(task_id, now)
    }

    pub fn resume_task(
        &mut self,
        task_id: DownloadTaskId,
        now: Instant,
    ) -> PlayerResult<Option<DownloadTaskSnapshot>> {
        self.inner.resume_task(task_id, now)
    }

    pub fn update_progress(
        &mut self,
        task_id: DownloadTaskId,
        received_bytes: u64,
        received_segments: u32,
        now: Instant,
    ) -> PlayerResult<Option<DownloadTaskSnapshot>> {
        self.inner
            .update_progress(task_id, received_bytes, received_segments, now)
    }

    pub fn complete_preparation(
        &mut self,
        task_id: DownloadTaskId,
        asset_index: DownloadAssetIndex,
        now: Instant,
    ) -> PlayerResult<Option<DownloadTaskSnapshot>> {
        self.inner.complete_preparation(task_id, asset_index, now)
    }

    pub fn replace_task_plan(
        &mut self,
        task_id: DownloadTaskId,
        source: DownloadSource,
        profile: DownloadProfile,
        asset_index: DownloadAssetIndex,
        now: Instant,
    ) -> PlayerResult<Option<DownloadTaskSnapshot>> {
        self.inner
            .replace_task_plan(task_id, source, profile, asset_index, now)
    }

    pub fn complete_task(
        &mut self,
        task_id: DownloadTaskId,
        completed_path: Option<std::path::PathBuf>,
        now: Instant,
    ) -> PlayerResult<Option<DownloadTaskSnapshot>> {
        self.inner.complete_task(task_id, completed_path, now)
    }

    pub fn fail_task(
        &mut self,
        task_id: DownloadTaskId,
        error: PlayerError,
        now: Instant,
    ) -> PlayerResult<Option<DownloadTaskSnapshot>> {
        self.inner.fail_task(task_id, error, now)
    }

    pub fn remove_task(
        &mut self,
        task_id: DownloadTaskId,
        now: Instant,
    ) -> PlayerResult<Option<DownloadTaskSnapshot>> {
        self.inner.remove_task(task_id, now)
    }

    pub fn task(&self, task_id: DownloadTaskId) -> Option<DownloadTaskSnapshot> {
        self.inner.task(task_id)
    }

    pub fn tasks_for_asset(&self, asset_id: &DownloadAssetId) -> Vec<DownloadTaskSnapshot> {
        self.inner.tasks_for_asset(asset_id)
    }

    pub fn snapshot(&self) -> DownloadSnapshot {
        self.inner.snapshot()
    }

    pub fn export_task_output(
        &self,
        task_id: DownloadTaskId,
        output_path: Option<PathBuf>,
        progress: &dyn ProcessorProgress,
    ) -> PlayerResult<PathBuf> {
        self.inner
            .export_task_output(task_id, output_path, progress)
    }

    pub fn drain_events(&mut self) -> Vec<DownloadEvent> {
        self.inner.drain_events()
    }

    pub fn drain_commands(&mut self) -> Vec<AndroidDownloadCommand> {
        self.inner
            .drain_commands()
            .into_iter()
            .map(AndroidDownloadCommand::from)
            .collect()
    }
}

impl From<MobileDownloadCommand> for AndroidDownloadCommand {
    fn from(command: MobileDownloadCommand) -> Self {
        match command {
            MobileDownloadCommand::Prepare { task } => Self::Prepare { task },
            MobileDownloadCommand::Start { task } => Self::Start { task },
            MobileDownloadCommand::Pause { task_id } => Self::Pause { task_id },
            MobileDownloadCommand::Resume { task } => Self::Resume { task },
            MobileDownloadCommand::Remove { task_id } => Self::Remove { task_id },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{AndroidDownloadBridgeSession, AndroidDownloadCommand};
    use player_model::MediaSource;
    use player_runtime::{
        DownloadAssetId, DownloadAssetIndex, DownloadContentFormat, DownloadProfile,
        DownloadSource, DownloadTaskStatus, PlayerError, PlayerErrorCategory, PlayerErrorCode,
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
    fn android_download_bridge_emits_prepare_start_pause_resume_and_remove_commands() {
        let now = Instant::now();
        let mut session = AndroidDownloadBridgeSession::new(true);
        let task_id = session
            .create_task(
                "asset-a",
                source("https://example.com/a.m3u8"),
                DownloadProfile::default(),
                DownloadAssetIndex::default(),
                now,
            )
            .expect("task should be created");

        let commands = session.drain_commands();
        assert_eq!(commands.len(), 1);
        assert!(matches!(
            &commands[0],
            AndroidDownloadCommand::Prepare { task } if task.task_id == task_id
        ));

        let prepared = session
            .complete_preparation(task_id, asset_index(1_024), now)
            .expect("preparation should complete")
            .expect("task should exist");
        assert_eq!(prepared.status, DownloadTaskStatus::Downloading);
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
                PlayerError::new(PlayerErrorCode::BackendFailure, "android failed"),
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

        assert_eq!(error.code(), PlayerErrorCode::InvalidArgument);
        assert_eq!(error.category(), PlayerErrorCategory::Input);
    }
}
