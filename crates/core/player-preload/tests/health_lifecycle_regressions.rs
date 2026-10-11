use std::collections::HashSet;
use std::time::{Duration, Instant};

use player_model::{MediaSource, PlayerError, PlayerErrorCode, PlayerResult};
use player_preload::{
    InMemoryPreloadBudgetProvider, PreloadBudget, PreloadBudgetScope, PreloadCandidate,
    PreloadCandidateKind, PreloadConfig, PreloadEvent, PreloadExecutor, PreloadPlanner,
    PreloadPriority, PreloadSelectionHint, PreloadTaskId, PreloadTaskSnapshot, PreloadTaskStatus,
};

#[derive(Default)]
struct ControlledExecutor {
    reject_cancel: bool,
    live: HashSet<PreloadTaskId>,
    cancel_attempts: usize,
}

impl PreloadExecutor for ControlledExecutor {
    fn warmup(&mut self, task: &PreloadTaskSnapshot) -> PlayerResult<()> {
        assert!(self.live.insert(task.task_id));
        Ok(())
    }

    fn cancel(&mut self, task_id: PreloadTaskId) -> PlayerResult<()> {
        self.cancel_attempts += 1;
        if self.reject_cancel {
            return Err(PlayerError::new(
                PlayerErrorCode::InvalidState,
                "cancel rejected",
            ));
        }
        assert!(self.live.remove(&task_id));
        Ok(())
    }
}

fn candidate(uri: &str, ttl: Option<Duration>) -> PreloadCandidate {
    PreloadCandidate::from_media_source(
        MediaSource::new(uri),
        PreloadBudgetScope::App,
        PreloadCandidateKind::Current,
        PreloadSelectionHint::CurrentItem,
        PreloadConfig {
            priority: PreloadPriority::Critical,
            ttl,
            expected_memory_bytes: 1,
            expected_disk_bytes: 1,
            warmup_window: None,
        },
    )
}

#[test]
fn rejected_expiry_retains_budget_and_dedup_until_cancellation_is_accepted() {
    let budget = PreloadBudget {
        max_concurrent_tasks: 1,
        max_memory_bytes: 1,
        max_disk_bytes: 1,
        warmup_window: Duration::from_secs(1),
    };
    let mut planner = PreloadPlanner::new(
        InMemoryPreloadBudgetProvider::new(budget),
        ControlledExecutor {
            reject_cancel: true,
            ..Default::default()
        },
    );
    let now = Instant::now();
    let first = planner.plan(
        [candidate(
            "https://example.test/first",
            Some(Duration::from_secs(1)),
        )],
        now,
    )[0];
    planner.drain_events();
    let expired = now + Duration::from_secs(2);
    planner.expire_due_tasks(expired);
    assert_eq!(
        planner.task(first).unwrap().status,
        PreloadTaskStatus::Active
    );
    assert_eq!(planner.executor().live, HashSet::from([first]));
    assert!(
        planner
            .plan([candidate("https://example.test/second", None)], expired)
            .is_empty()
    );
    assert!(
        planner
            .plan([candidate("https://example.test/first", None)], expired)
            .is_empty()
    );
    assert!(
        planner.drain_events().is_empty(),
        "Rejected cancellation must not publish a terminal event"
    );

    planner.executor_mut().reject_cancel = false;
    let second = planner.plan([candidate("https://example.test/second", None)], expired)[0];
    assert_eq!(
        planner.task(first).unwrap().status,
        PreloadTaskStatus::Expired
    );
    assert_eq!(planner.executor().live, HashSet::from([second]));
    assert_eq!(
        planner
            .drain_events()
            .iter()
            .filter(|e| matches!(e, PreloadEvent::Expired(_)))
            .count(),
        1
    );
    let attempts = planner.executor().cancel_attempts;
    planner.expire_due_tasks(expired);
    assert_eq!(
        planner.executor().cancel_attempts,
        attempts,
        "Successful expiry settles once"
    );
}
