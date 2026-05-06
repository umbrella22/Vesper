use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use player_runtime::{
    InMemoryPreloadBudgetProvider, PlayerRuntimeError, PlayerRuntimeErrorCategory,
    PlayerRuntimeErrorCode, PlayerRuntimeResult, PlaylistActiveItem, PlaylistAdvanceDecision,
    PlaylistCoordinator, PlaylistCoordinatorConfig, PlaylistEvent, PlaylistQueueItem,
    PlaylistSnapshot, PlaylistViewportHint, PreloadBudget, PreloadEvent, PreloadExecutor,
    PreloadTaskId, PreloadTaskSnapshot,
};

use crate::AndroidPreloadCommand;

#[derive(Debug, Clone)]
struct AndroidPlaylistExecutor {
    queue: Arc<Mutex<VecDeque<AndroidPreloadCommand>>>,
}

impl AndroidPlaylistExecutor {
    fn new(queue: Arc<Mutex<VecDeque<AndroidPreloadCommand>>>) -> Self {
        Self { queue }
    }

    fn push_command(&self, command: AndroidPreloadCommand) -> PlayerRuntimeResult<()> {
        let mut queue = self.queue.lock().map_err(|_| {
            PlayerRuntimeError::with_category(
                PlayerRuntimeErrorCode::BackendFailure,
                PlayerRuntimeErrorCategory::Platform,
                "android playlist preload command queue lock poisoned",
            )
        })?;
        queue.push_back(command);
        Ok(())
    }
}

impl PreloadExecutor for AndroidPlaylistExecutor {
    fn warmup(&mut self, task: &PreloadTaskSnapshot) -> PlayerRuntimeResult<()> {
        self.push_command(AndroidPreloadCommand::Start { task: task.clone() })
    }

    fn cancel(&mut self, task_id: PreloadTaskId) -> PlayerRuntimeResult<()> {
        self.push_command(AndroidPreloadCommand::Cancel { task_id })
    }
}

#[derive(Debug)]
pub struct AndroidPlaylistBridgeSession {
    coordinator: PlaylistCoordinator<InMemoryPreloadBudgetProvider, AndroidPlaylistExecutor>,
    command_queue: Arc<Mutex<VecDeque<AndroidPreloadCommand>>>,
}

impl AndroidPlaylistBridgeSession {
    pub fn new(
        playlist_id: impl Into<String>,
        config: PlaylistCoordinatorConfig,
        preload_budget: PreloadBudget,
    ) -> Self {
        let command_queue = Arc::new(Mutex::new(VecDeque::new()));
        let executor = AndroidPlaylistExecutor::new(command_queue.clone());

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
    ) -> PlayerRuntimeResult<Option<PreloadTaskSnapshot>> {
        self.coordinator.complete_preload_task(task_id)
    }

    pub fn fail_preload_task(
        &mut self,
        task_id: PreloadTaskId,
        error: PlayerRuntimeError,
    ) -> PlayerRuntimeResult<Option<PreloadTaskSnapshot>> {
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

    pub fn drain_commands(&mut self) -> Vec<AndroidPreloadCommand> {
        self.command_queue
            .lock()
            .map(|mut queue| queue.drain(..).collect())
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use player_model::MediaSource;
    use player_runtime::{
        PlaylistActivationReason, PlaylistCoordinatorConfig, PlaylistFailureStrategy,
        PlaylistItemPreloadProfile, PlaylistPreloadWindow, PlaylistQueueItem, PlaylistRepeatMode,
        PlaylistSwitchPolicy, PlaylistViewportHint, PlaylistViewportHintKind, PreloadBudget,
        PreloadTaskStatus,
    };

    use super::AndroidPlaylistBridgeSession;
    use crate::AndroidPreloadCommand;

    fn test_budget(max_concurrent_tasks: u32) -> PreloadBudget {
        PreloadBudget {
            max_concurrent_tasks,
            max_memory_bytes: 64,
            max_disk_bytes: 64,
            warmup_window: Duration::from_secs(30),
        }
    }

    fn item(id: &str, uri: &str) -> PlaylistQueueItem {
        PlaylistQueueItem::new(id, MediaSource::new(uri)).with_preload_profile(
            PlaylistItemPreloadProfile {
                expected_memory_bytes: 1,
                expected_disk_bytes: 1,
                ttl: None,
                warmup_window: None,
            },
        )
    }

    #[test]
    fn android_playlist_bridge_tracks_active_item_from_viewport_hints() {
        let mut session = AndroidPlaylistBridgeSession::new(
            "android-playlist",
            PlaylistCoordinatorConfig::default(),
            test_budget(8),
        );
        let now = Instant::now();

        session.replace_queue(
            [
                item("item-1", "https://example.com/1.m3u8"),
                item("item-2", "https://example.com/2.m3u8"),
                item("item-3", "https://example.com/3.m3u8"),
            ],
            now,
        );
        let _ = session.drain_commands();

        session.update_viewport_hints(
            [
                PlaylistViewportHint::new("item-2", PlaylistViewportHintKind::Visible),
                PlaylistViewportHint::new("item-3", PlaylistViewportHintKind::NearVisible)
                    .with_order(1),
            ],
            now,
        );

        let active_item = session.active_item().expect("active item should exist");
        assert_eq!(active_item.item_id.as_str(), "item-2");
        assert_eq!(active_item.index, 1);
    }

    #[test]
    fn android_playlist_bridge_emits_preload_commands_for_active_and_neighbors() {
        let mut session = AndroidPlaylistBridgeSession::new(
            "android-playlist",
            PlaylistCoordinatorConfig::default(),
            test_budget(8),
        );
        let now = Instant::now();

        session.replace_queue(
            [
                item("item-1", "https://example.com/1.m3u8"),
                item("item-2", "https://example.com/2.m3u8"),
                item("item-3", "https://example.com/3.m3u8"),
            ],
            now,
        );

        let commands = session.drain_commands();
        assert_eq!(commands.len(), 2);
        assert!(commands.iter().any(|command| matches!(
            command,
            AndroidPreloadCommand::Start { task }
                if task.source.uri() == "https://example.com/1.m3u8"
        )));
        assert!(commands.iter().any(|command| matches!(
            command,
            AndroidPreloadCommand::Start { task }
                if task.source.uri() == "https://example.com/2.m3u8"
        )));
    }

    #[test]
    fn android_playlist_bridge_advance_to_next_switches_active_item() {
        let mut session = AndroidPlaylistBridgeSession::new(
            "android-playlist",
            PlaylistCoordinatorConfig {
                switch_policy: PlaylistSwitchPolicy {
                    auto_advance: true,
                    repeat_mode: PlaylistRepeatMode::Off,
                    failure_strategy: PlaylistFailureStrategy::SkipToNext,
                },
                ..PlaylistCoordinatorConfig::default()
            },
            test_budget(8),
        );
        let now = Instant::now();

        session.replace_queue(
            [
                item("item-1", "https://example.com/1.m3u8"),
                item("item-2", "https://example.com/2.m3u8"),
            ],
            now,
        );
        let _ = session.drain_commands();

        let decision = session.advance_to_next(now);
        let active_item = session.active_item().expect("active item should exist");

        assert_eq!(active_item.item_id.as_str(), "item-2");
        assert!(matches!(
            decision.outcome,
            player_runtime::PlaylistAdvanceOutcome::Activated(ref item)
                if item.reason == PlaylistActivationReason::ManualNext
        ));
    }

    #[test]
    fn android_playlist_bridge_applies_prefetch_window_limits() {
        let mut session = AndroidPlaylistBridgeSession::new(
            "android-playlist",
            PlaylistCoordinatorConfig {
                preload_window: PlaylistPreloadWindow {
                    near_visible: 1,
                    prefetch_only: 1,
                },
                ..PlaylistCoordinatorConfig::default()
            },
            test_budget(8),
        );
        let now = Instant::now();

        session.replace_queue(
            [
                item("item-1", "https://example.com/1.m3u8"),
                item("item-2", "https://example.com/2.m3u8"),
                item("item-3", "https://example.com/3.m3u8"),
                item("item-4", "https://example.com/4.m3u8"),
            ],
            now,
        );
        let _ = session.drain_commands();

        session.update_viewport_hints(
            [
                PlaylistViewportHint::new("item-2", PlaylistViewportHintKind::NearVisible)
                    .with_order(0),
                PlaylistViewportHint::new("item-3", PlaylistViewportHintKind::NearVisible)
                    .with_order(1),
                PlaylistViewportHint::new("item-4", PlaylistViewportHintKind::PrefetchOnly)
                    .with_order(2),
            ],
            now,
        );

        let tasks = session
            .snapshot()
            .preload
            .tasks
            .into_iter()
            .filter(|task| {
                matches!(
                    task.status,
                    PreloadTaskStatus::Planned | PreloadTaskStatus::Active
                )
            })
            .map(|task| task.source.uri().to_owned())
            .collect::<Vec<_>>();

        assert!(tasks.contains(&"https://example.com/1.m3u8".to_owned()));
        assert!(tasks.contains(&"https://example.com/2.m3u8".to_owned()));
        assert!(!tasks.contains(&"https://example.com/3.m3u8".to_owned()));
        assert!(tasks.contains(&"https://example.com/4.m3u8".to_owned()));
    }
}
