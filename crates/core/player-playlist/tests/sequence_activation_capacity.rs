use std::time::Instant;

use player_playlist::{
    SequenceActivationReason, SequenceCacheIdentity, SequenceClockSnapshot, SequenceConfig,
    SequenceContentIdentity, SequenceCoordinator, SequenceErrorCode, SequenceEventKind,
    SequenceItem, SequenceItemId, SequenceMediaKind, SequenceNavigationOutcome,
    SequenceRequestKind, SequenceSourceRevision,
};

fn unresolved(id: &str) -> SequenceItem {
    SequenceItem::unresolved(
        id,
        SequenceContentIdentity::new("fixture", id),
        SequenceMediaKind::Vod,
    )
}

fn active_a_with_pending_c() -> (SequenceCoordinator, SequenceClockSnapshot) {
    let clock = SequenceClockSnapshot {
        wall_epoch_ms: 1_000_000,
        monotonic: Instant::now(),
    };
    let a = SequenceItem::resolved(
        "a",
        SequenceContentIdentity::new("fixture", "a"),
        SequenceMediaKind::Vod,
        "source-a",
        SequenceCacheIdentity::new(
            "fixture",
            "a",
            "default",
            "source-a",
            "session-1",
            SequenceSourceRevision::new(1),
        ),
        None,
    );
    let mut sequence = SequenceCoordinator::new(
        "capacity-fixture",
        SequenceConfig {
            max_pending_requests: 1,
            ..SequenceConfig::default()
        },
    )
    .unwrap();
    sequence
        .replace(vec![a, unresolved("b"), unresolved("c")], None, clock)
        .unwrap();
    sequence
        .set_active(&SequenceItemId::new("c"), clock)
        .unwrap();
    sequence
        .set_active(&SequenceItemId::new("a"), clock)
        .unwrap();
    sequence.drain_events();
    assert_eq!(sequence.snapshot().pending_requests.len(), 1);
    sequence
        .validate_activation_callback(
            &SequenceItemId::new("a"),
            sequence.activation_epoch(),
            SequenceSourceRevision::new(1),
        )
        .unwrap();
    (sequence, clock)
}

#[test]
fn capacity_rejection_preserves_the_live_activation_and_callback() {
    let (mut sequence, clock) = active_a_with_pending_c();
    let before = sequence.snapshot();
    let error = sequence
        .set_active(&SequenceItemId::new("b"), clock)
        .unwrap_err();
    assert_eq!(error.code, SequenceErrorCode::CapacityExceeded);
    assert_eq!(error.message, "pending request capacity reached");
    let callback = sequence.validate_activation_callback(
        &SequenceItemId::new("a"),
        before.activation_epoch,
        SequenceSourceRevision::new(1),
    );
    let after = sequence.snapshot();
    let events = sequence.drain_events();
    println!(
        "rejected B: code={:?}; active={:?}->{:?}; epoch={:?}->{:?}; A callback={:?}; events={:?}",
        error.code,
        before.active_item_id,
        after.active_item_id,
        before.activation_epoch,
        after.activation_epoch,
        callback,
        events,
    );
    assert!(
        callback.is_ok(),
        "capacity-rejected B must not stale live A"
    );
    assert_eq!(after, before);
    assert!(events.is_empty());
}

#[test]
fn full_capacity_still_allows_resolved_items_and_pending_request_reuse() {
    let (mut sequence, clock) = active_a_with_pending_c();
    let before = sequence.snapshot();
    let outcome = sequence
        .set_active(&SequenceItemId::new("c"), clock)
        .unwrap();
    let after = sequence.snapshot();
    assert_eq!(
        outcome,
        SequenceNavigationOutcome::Activated {
            item_id: SequenceItemId::new("c"),
            activation_epoch: after.activation_epoch,
        }
    );
    assert_ne!(after.activation_epoch, before.activation_epoch);
    assert_eq!(after.session_generation, before.session_generation);
    assert_eq!(after.pending_requests, before.pending_requests);
    let events = sequence.drain_events();
    assert_eq!(
        events
            .iter()
            .map(|event| event.kind.clone())
            .collect::<Vec<_>>(),
        vec![
            SequenceEventKind::ActiveItemChanged {
                item_id: SequenceItemId::new("c"),
                reason: SequenceActivationReason::SetActive,
                activation_epoch: after.activation_epoch,
            },
            SequenceEventKind::SnapshotChanged,
        ]
    );
    let stale_a = sequence
        .validate_activation_callback(
            &SequenceItemId::new("a"),
            before.activation_epoch,
            SequenceSourceRevision::new(1),
        )
        .unwrap_err();
    assert_eq!(stale_a.code, SequenceErrorCode::StaleActivation);
}

#[test]
fn admitted_new_request_keeps_success_event_order() {
    let (mut sequence, clock) = active_a_with_pending_c();
    let before = sequence.snapshot();
    let pending_c = &before.pending_requests[0];
    sequence
        .fail_request(
            before.session_generation,
            pending_c.request_id(),
            "fixture_rejected",
        )
        .unwrap();
    sequence.drain_events();
    sequence
        .set_active(&SequenceItemId::new("b"), clock)
        .unwrap();
    let after = sequence.snapshot();
    assert_eq!(after.pending_requests.len(), 1);
    assert_eq!(after.session_generation, before.session_generation);
    let SequenceRequestKind::Source(request_b) = &after.pending_requests[0].kind else {
        panic!("B must get a source request");
    };
    assert_eq!(request_b.item_id, SequenceItemId::new("b"));
    assert_ne!(request_b.request_id, pending_c.request_id());
    let events = sequence.drain_events();
    assert_eq!(
        events
            .iter()
            .map(|event| event.kind.clone())
            .collect::<Vec<_>>(),
        vec![
            SequenceEventKind::ActiveItemChanged {
                item_id: SequenceItemId::new("b"),
                reason: SequenceActivationReason::SetActive,
                activation_epoch: after.activation_epoch,
            },
            SequenceEventKind::Request(SequenceRequestKind::Source(request_b.clone())),
            SequenceEventKind::SnapshotChanged,
            SequenceEventKind::SnapshotChanged,
        ]
    );
    assert!(
        events
            .windows(2)
            .all(|pair| pair[0].event_sequence < pair[1].event_sequence)
    );
}
