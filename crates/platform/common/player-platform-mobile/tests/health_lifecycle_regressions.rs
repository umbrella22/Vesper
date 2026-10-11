use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use player_model::MediaSource;
use player_platform_mobile::{
    MOBILE_COMMAND_QUEUE_CAPACITY, MobileCommandQueue, MobilePreloadBridgeSession,
    MobilePreloadCommand,
};
use player_runtime::{
    InMemoryPreloadBudgetProvider, PreloadBudget, PreloadBudgetScope, PreloadCandidate,
    PreloadCandidateKind, PreloadConfig, PreloadPriority, PreloadSelectionHint, PreloadTaskStatus,
};

#[test]
fn mapper_can_enqueue_the_next_batch_without_holding_the_queue_lock() {
    let queue = MobileCommandQueue::new("reentry");
    queue.push(1).unwrap();
    queue.push(2).unwrap();
    let producer = queue.clone();
    let (sender, receiver) = mpsc::channel();
    let worker = std::thread::spawn(move || {
        let batch = queue.drain_map(|value| {
            producer.push(value + 10).unwrap();
            value * 2
        });
        sender.send((batch, queue.drain())).unwrap();
    });
    let (batch, next) = receiver
        .recv_timeout(Duration::from_secs(3))
        .expect("A mapper must be able to reenter its queue");
    worker.join().unwrap();
    assert_eq!(batch, vec![2, 4]);
    assert_eq!(next, vec![11, 12]);
}

#[test]
fn mapper_panic_does_not_poison_the_shared_queue() {
    let queue = MobileCommandQueue::new("panic");
    queue.push(1).unwrap();
    assert!(
        catch_unwind(AssertUnwindSafe(
            || queue.drain_map(|_| panic!("mapper failed"))
        ))
        .is_err()
    );
    queue
        .push(2)
        .expect("The mapper must not unwind through a MutexGuard");
    assert_eq!(queue.drain(), vec![2]);
}

#[test]
fn full_mobile_command_queue_retains_expired_reservations_until_retry() {
    let count = MOBILE_COMMAND_QUEUE_CAPACITY;
    let mut session = MobilePreloadBridgeSession::new(
        InMemoryPreloadBudgetProvider::new(PreloadBudget {
            max_concurrent_tasks: count as u32,
            max_memory_bytes: count as u64,
            max_disk_bytes: count as u64,
            warmup_window: Duration::from_secs(1),
        }),
        "expiry-retry",
    );
    let now = Instant::now();
    let candidates = (0..count).map(|i| {
        PreloadCandidate::from_media_source(
            MediaSource::new(format!("https://example.test/{i}")),
            PreloadBudgetScope::App,
            PreloadCandidateKind::Current,
            PreloadSelectionHint::CurrentItem,
            PreloadConfig {
                priority: PreloadPriority::Critical,
                ttl: Some(Duration::from_secs(1)),
                expected_memory_bytes: 1,
                expected_disk_bytes: 1,
                warmup_window: None,
            },
        )
    });
    assert_eq!(session.plan(candidates, now).len(), count);
    session.drain_events();
    let expired = now + Duration::from_secs(2);
    session.expire_due_tasks(expired);
    assert!(
        session
            .snapshot()
            .tasks
            .iter()
            .all(|t| t.status == PreloadTaskStatus::Active)
    );
    assert!(session.drain_events().is_empty());
    let starts = session.drain_commands();
    assert_eq!(starts.len(), count);
    assert!(
        starts
            .iter()
            .all(|c| matches!(c, MobilePreloadCommand::Start { .. }))
    );
    session.expire_due_tasks(expired);
    assert!(
        session
            .snapshot()
            .tasks
            .iter()
            .all(|t| t.status == PreloadTaskStatus::Expired)
    );
    let cancels = session.drain_commands();
    assert_eq!(cancels.len(), count);
    assert!(
        cancels
            .iter()
            .all(|c| matches!(c, MobilePreloadCommand::Cancel { .. }))
    );
    session.expire_due_tasks(expired);
    assert!(session.drain_commands().is_empty());
}
