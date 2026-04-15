use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use player_runtime::{
    InMemoryPreloadBudgetProvider, PlayerRuntimeError, PlayerRuntimeErrorCategory,
    PlayerRuntimeErrorCode, PlayerRuntimeResult, PreloadCandidate, PreloadEvent, PreloadExecutor,
    PreloadPlanner, PreloadSnapshot, PreloadTaskId, PreloadTaskSnapshot,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AndroidPreloadCommand {
    Start { task: PreloadTaskSnapshot },
    Cancel { task_id: PreloadTaskId },
}

#[derive(Debug, Clone)]
struct AndroidPreloadExecutor {
    queue: Arc<Mutex<VecDeque<AndroidPreloadCommand>>>,
}

impl AndroidPreloadExecutor {
    fn new(queue: Arc<Mutex<VecDeque<AndroidPreloadCommand>>>) -> Self {
        Self { queue }
    }

    fn push_command(&self, command: AndroidPreloadCommand) -> PlayerRuntimeResult<()> {
        let mut queue = self.queue.lock().map_err(|_| {
            PlayerRuntimeError::with_category(
                PlayerRuntimeErrorCode::BackendFailure,
                PlayerRuntimeErrorCategory::Platform,
                "android preload command queue lock poisoned",
            )
        })?;
        queue.push_back(command);
        Ok(())
    }
}

impl PreloadExecutor for AndroidPreloadExecutor {
    fn warmup(&mut self, task: &PreloadTaskSnapshot) -> PlayerRuntimeResult<()> {
        self.push_command(AndroidPreloadCommand::Start { task: task.clone() })
    }

    fn cancel(&mut self, task_id: PreloadTaskId) -> PlayerRuntimeResult<()> {
        self.push_command(AndroidPreloadCommand::Cancel { task_id })
    }
}

#[derive(Debug)]
pub struct AndroidPreloadBridgeSession {
    planner: PreloadPlanner<InMemoryPreloadBudgetProvider, AndroidPreloadExecutor>,
    command_queue: Arc<Mutex<VecDeque<AndroidPreloadCommand>>>,
}

impl AndroidPreloadBridgeSession {
    pub fn new(budget_provider: InMemoryPreloadBudgetProvider) -> Self {
        let command_queue = Arc::new(Mutex::new(VecDeque::new()));
        let executor = AndroidPreloadExecutor::new(command_queue.clone());

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

    pub fn cancel(
        &mut self,
        task_id: PreloadTaskId,
    ) -> PlayerRuntimeResult<Option<PreloadTaskSnapshot>> {
        self.planner.cancel(task_id)
    }

    pub fn complete(
        &mut self,
        task_id: PreloadTaskId,
    ) -> PlayerRuntimeResult<Option<PreloadTaskSnapshot>> {
        self.planner.complete(task_id)
    }

    pub fn fail(
        &mut self,
        task_id: PreloadTaskId,
        error: PlayerRuntimeError,
    ) -> PlayerRuntimeResult<Option<PreloadTaskSnapshot>> {
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

    pub fn drain_commands(&mut self) -> Vec<AndroidPreloadCommand> {
        self.command_queue
            .lock()
            .map(|mut queue| queue.drain(..).collect())
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::{AndroidPreloadBridgeSession, AndroidPreloadCommand};
    use player_core::MediaSource;
    use player_runtime::{
        InMemoryPreloadBudgetProvider, PlayerRuntimeError, PlayerRuntimeErrorCode, PreloadBudget,
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
        PreloadCandidate {
            source: MediaSource::new(uri),
            scope: PreloadBudgetScope::App,
            kind: PreloadCandidateKind::Current,
            selection_hint: PreloadSelectionHint::CurrentItem,
            config: PreloadConfig {
                priority: PreloadPriority::Critical,
                ttl: None,
                expected_memory_bytes: 1,
                expected_disk_bytes: 1,
                warmup_window: None,
            },
        }
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
                PlayerRuntimeError::new(
                    PlayerRuntimeErrorCode::BackendFailure,
                    "android warmup failed",
                ),
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
