use std::time::Instant;

use player_platform_mobile::{MobilePreloadBridgeSession, MobilePreloadCommand};
use player_runtime::{
    InMemoryPreloadBudgetProvider, PlayerError, PlayerResult, PreloadCandidate, PreloadEvent,
    PreloadSnapshot, PreloadTaskId, PreloadTaskSnapshot,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AndroidPreloadCommand {
    Start { task: PreloadTaskSnapshot },
    Cancel { task_id: PreloadTaskId },
}

#[derive(Debug)]
pub struct AndroidPreloadBridgeSession {
    inner: MobilePreloadBridgeSession,
}

impl AndroidPreloadBridgeSession {
    pub fn new(budget_provider: InMemoryPreloadBudgetProvider) -> Self {
        Self {
            inner: MobilePreloadBridgeSession::new(budget_provider, "android preload"),
        }
    }

    pub fn plan(
        &mut self,
        candidates: impl IntoIterator<Item = PreloadCandidate>,
        now: Instant,
    ) -> Vec<PreloadTaskId> {
        self.inner.plan(candidates, now)
    }

    pub fn cancel(&mut self, task_id: PreloadTaskId) -> PlayerResult<Option<PreloadTaskSnapshot>> {
        self.inner.cancel(task_id)
    }

    pub fn complete(
        &mut self,
        task_id: PreloadTaskId,
    ) -> PlayerResult<Option<PreloadTaskSnapshot>> {
        self.inner.complete(task_id)
    }

    pub fn fail(
        &mut self,
        task_id: PreloadTaskId,
        error: PlayerError,
    ) -> PlayerResult<Option<PreloadTaskSnapshot>> {
        self.inner.fail(task_id, error)
    }

    pub fn expire_due_tasks(&mut self, now: Instant) {
        self.inner.expire_due_tasks(now);
    }

    pub fn snapshot(&self) -> PreloadSnapshot {
        self.inner.snapshot()
    }

    pub fn drain_events(&mut self) -> Vec<PreloadEvent> {
        self.inner.drain_events()
    }

    pub fn drain_commands(&mut self) -> Vec<AndroidPreloadCommand> {
        self.inner
            .drain_commands()
            .into_iter()
            .map(AndroidPreloadCommand::from)
            .collect()
    }
}

impl From<MobilePreloadCommand> for AndroidPreloadCommand {
    fn from(command: MobilePreloadCommand) -> Self {
        match command {
            MobilePreloadCommand::Start { task } => Self::Start { task },
            MobilePreloadCommand::Cancel { task_id } => Self::Cancel { task_id },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{AndroidPreloadBridgeSession, AndroidPreloadCommand};
    use player_model::MediaSource;
    use player_runtime::{
        InMemoryPreloadBudgetProvider, PlayerError, PlayerErrorCode, PreloadBudget,
        PreloadBudgetScope, PreloadCandidate, PreloadCandidateKind, PreloadConfig, PreloadEvent,
        PreloadPriority, PreloadSelectionHint, PreloadTaskStatus,
    };
    use std::time::{Duration, Instant};

    fn test_budget(max_concurrent_tasks: u32) -> PreloadBudget {
        PreloadBudget {
            max_concurrent_tasks,
            max_memory_bytes: 64,
            max_disk_bytes: 64,
            warmup_window: Duration::from_secs(30),
        }
    }

    fn candidate(uri: &str) -> PreloadCandidate {
        PreloadCandidate::from_media_source(
            MediaSource::new(uri),
            PreloadBudgetScope::App,
            PreloadCandidateKind::Current,
            PreloadSelectionHint::CurrentItem,
            PreloadConfig {
                priority: PreloadPriority::Critical,
                ttl: None,
                expected_memory_bytes: 1,
                expected_disk_bytes: 1,
                warmup_window: None,
            },
        )
    }

    #[test]
    fn android_preload_bridge_emits_start_and_cancel_commands() {
        let provider = InMemoryPreloadBudgetProvider::new(test_budget(1));
        let mut session = AndroidPreloadBridgeSession::new(provider);

        let task_id = session
            .plan(
                [candidate("https://example.com/current.m3u8")],
                Instant::now(),
            )
            .into_iter()
            .next()
            .expect("task should be planned");

        let commands = session.drain_commands();
        assert_eq!(commands.len(), 1);
        assert!(matches!(
            &commands[0],
            AndroidPreloadCommand::Start { task } if task.task_id == task_id
        ));

        session.cancel(task_id).expect("cancel should succeed");
        assert_eq!(
            session.drain_commands(),
            vec![AndroidPreloadCommand::Cancel { task_id }]
        );
    }

    #[test]
    fn android_preload_bridge_releases_budget_after_completion() {
        let provider = InMemoryPreloadBudgetProvider::new(test_budget(1));
        let mut session = AndroidPreloadBridgeSession::new(provider);
        let now = Instant::now();

        let first_task_id = session
            .plan([candidate("https://example.com/current.m3u8")], now)
            .into_iter()
            .next()
            .expect("first task should be planned");
        let _ = session.drain_commands();

        assert!(
            session
                .plan([candidate("https://example.com/neighbor.m3u8")], now)
                .is_empty()
        );

        let completed = session
            .complete(first_task_id)
            .expect("complete should succeed")
            .expect("task should exist");
        assert_eq!(completed.status, PreloadTaskStatus::Completed);

        let next_task_ids = session.plan([candidate("https://example.com/neighbor.m3u8")], now);
        assert_eq!(next_task_ids.len(), 1);
    }

    #[test]
    fn android_preload_bridge_records_failure_event() {
        let provider = InMemoryPreloadBudgetProvider::new(test_budget(1));
        let mut session = AndroidPreloadBridgeSession::new(provider);

        let task_id = session
            .plan(
                [candidate("https://example.com/current.m3u8")],
                Instant::now(),
            )
            .into_iter()
            .next()
            .expect("task should be planned");

        let failed = session
            .fail(
                task_id,
                PlayerError::new(PlayerErrorCode::BackendFailure, "android warmup failed"),
            )
            .expect("fail should succeed")
            .expect("task should exist");
        assert_eq!(failed.status, PreloadTaskStatus::Failed);

        let events = session.drain_events();
        assert!(
            events.iter().any(
                |event| matches!(event, PreloadEvent::Failed(task) if task.task_id == task_id)
            )
        );
    }
}
