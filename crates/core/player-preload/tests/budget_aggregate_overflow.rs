use std::time::{Duration, Instant};

use player_model::MediaSource;
use player_preload::{
    InMemoryPreloadBudgetProvider, InMemoryPreloadExecutor, PreloadBudget, PreloadBudgetScope,
    PreloadCacheKey, PreloadCandidate, PreloadCandidateKind, PreloadConfig, PreloadEvent,
    PreloadPlanner, PreloadSelectionHint, PreloadSourceIdentity, PreloadTaskStatus,
};

type Planner = PreloadPlanner<InMemoryPreloadBudgetProvider, InMemoryPreloadExecutor>;

fn planner(tasks: u32, memory: u64, disk: u64) -> Planner {
    PreloadPlanner::new(
        InMemoryPreloadBudgetProvider::new(PreloadBudget {
            max_concurrent_tasks: tasks,
            max_memory_bytes: memory,
            max_disk_bytes: disk,
            warmup_window: Duration::from_secs(30),
        }),
        InMemoryPreloadExecutor::default(),
    )
}

fn candidate(key: &str, memory: u64, disk: u64) -> PreloadCandidate {
    PreloadCandidate::from_media_source(
        MediaSource::new(format!("https://example.invalid/{key}.mp4")),
        PreloadBudgetScope::App,
        PreloadCandidateKind::Current,
        PreloadSelectionHint::CurrentItem,
        PreloadConfig {
            expected_memory_bytes: memory,
            expected_disk_bytes: disk,
            ..PreloadConfig::default()
        },
    )
}

fn assert_overflow_candidates_are_skipped(memory: bool) {
    let mut planner = if memory {
        planner(3, u64::MAX, 8)
    } else {
        planner(3, 8, u64::MAX)
    };
    let now = Instant::now();
    let maximum = if memory {
        candidate("maximum", u64::MAX, 1)
    } else {
        candidate("maximum", 1, u64::MAX)
    };
    let first = planner.plan([maximum], now);
    assert_eq!(first.len(), 1, "an exact maximum is representable");

    // A third distinct key forces public plan to recompute admitted usage.
    // The in-memory executor records metadata; it allocates no charged bytes.
    let later = planner.plan([candidate("overflow", 1, 1), candidate("next", 1, 1)], now);
    assert!(later.is_empty(), "over-budget candidates must be skipped");
    assert_eq!(planner.executor().started(), first);
    let snapshot = planner.snapshot();
    assert_eq!(snapshot.tasks.len(), 1);
    assert_eq!(snapshot.tasks[0].task_id, first[0]);
    assert_eq!(snapshot.tasks[0].status, PreloadTaskStatus::Active);
    let events = planner.drain_events();
    assert_eq!(events.len(), 2);
    assert!(matches!(&events[0], PreloadEvent::Planned(task) if task.task_id == first[0]));
    assert!(matches!(&events[1], PreloadEvent::Started(task) if task.task_id == first[0]));
}

#[test]
fn memory_overflow_is_skipped_before_the_next_public_candidate() {
    assert_overflow_candidates_are_skipped(true);
}

#[test]
fn disk_overflow_is_skipped_before_the_next_public_candidate() {
    assert_overflow_candidates_are_skipped(false);
}

#[test]
fn exact_maximum_totals_zero_cost_and_cache_identity_remain_valid() {
    let mut planner = planner(4, u64::MAX, u64::MAX);
    let now = Instant::now();
    let first = planner.plan([candidate("almost", u64::MAX - 1, u64::MAX - 1)], now);
    assert_eq!(first.len(), 1);
    let second = planner.plan([candidate("last-byte", 1, 1)], now);
    assert_eq!(second.len(), 1);
    let zero = candidate("zero", 0, 0);
    let zero_key = zero.cache_key.clone();
    let third = planner.plan([zero], now);
    assert_eq!(third.len(), 1);

    let duplicate = candidate("different-source", 0, 0).with_identity(
        PreloadSourceIdentity::new("different-source"),
        PreloadCacheKey::new(zero_key.as_str()),
    );
    assert!(planner.plan([duplicate], now).is_empty());
    assert_eq!(planner.executor().started().len(), 3);
    let fourth = planner.plan([candidate("distinct-zero", 0, 0)], now);
    assert_eq!(fourth.len(), 1);
    assert_eq!(planner.executor().started().len(), 4);
    assert!(planner.snapshot().tasks.iter().all(|task| {
        task.status == PreloadTaskStatus::Active
            && task.cache_key.as_str() != "https://example.invalid/different-source.mp4"
    }));
}

fn assert_resume_obeys_aggregate_budget(memory: bool) {
    let mut planner = if memory {
        planner(3, u64::MAX, 8)
    } else {
        planner(3, 8, u64::MAX)
    };
    let now = Instant::now();
    let small = planner.plan([candidate("small", 1, 1)], now);
    assert_eq!(small.len(), 1);
    let cancelled = planner
        .cancel(small[0])
        .expect("public cancel must accept the existing task");
    assert!(matches!(cancelled, Some(task) if task.status == PreloadTaskStatus::Cancelled));
    let maximum = if memory {
        candidate("maximum", u64::MAX, 1)
    } else {
        candidate("maximum", 1, u64::MAX)
    };
    let active = planner.plan([maximum], now);
    assert_eq!(active.len(), 1);
    let resumed = planner
        .resume(small[0], now)
        .expect("public resume must report budget rejection without an error");
    assert!(matches!(resumed, Some(task) if task.status == PreloadTaskStatus::Cancelled));
    assert_eq!(planner.executor().started(), &[small[0], active[0]]);

    planner
        .complete(active[0])
        .expect("public complete must release the active task");
    let resumed = planner
        .resume(small[0], now)
        .expect("public resume must accept the released capacity");
    assert!(matches!(resumed, Some(task) if task.status == PreloadTaskStatus::Active));
    assert_eq!(
        planner.executor().started(),
        &[small[0], active[0], small[0]]
    );
}

#[test]
fn resume_rejects_unrepresentable_memory_until_capacity_is_released() {
    assert_resume_obeys_aggregate_budget(true);
}

#[test]
fn resume_rejects_unrepresentable_disk_until_capacity_is_released() {
    assert_resume_obeys_aggregate_budget(false);
}

#[test]
fn ordinary_budget_limits_still_skip_candidates() {
    let now = Instant::now();
    let mut planner = planner(3, 4, 4);
    assert_eq!(planner.plan([candidate("within", 4, 4)], now).len(), 1);
    assert!(planner.plan([candidate("over", 1, 1)], now).is_empty());
    assert_eq!(planner.executor().started().len(), 1);
}
