use std::time::Instant;

use player_playlist::{
    SequenceCacheIdentity, SequenceClockSnapshot, SequenceConfig, SequenceContentIdentity,
    SequenceCoordinator, SequenceEventKind, SequenceItem, SequenceItemId, SequenceMediaKind,
    SequenceRequestKind, SequenceResolvedSource, SequenceSourceReference, SequenceSourceRevision,
    SequenceWarmupGoal,
};

fn item(id: &str) -> SequenceItem {
    SequenceItem::unresolved(
        id,
        SequenceContentIdentity::new("fixture", id),
        SequenceMediaKind::Vod,
    )
}

fn now() -> SequenceClockSnapshot {
    SequenceClockSnapshot {
        wall_epoch_ms: 1_000_000,
        monotonic: Instant::now(),
    }
}

#[test]
fn list_replacement_reordering_and_removal_do_not_activate_playback() {
    let mut sequence = SequenceCoordinator::new("test", SequenceConfig::default()).unwrap();
    let clock = now();
    let initial_epoch = sequence.snapshot().activation_epoch;
    sequence
        .replace(vec![item("a"), item("b")], None, clock)
        .unwrap();
    assert_eq!(sequence.snapshot().active_item_id, None);
    assert_eq!(sequence.snapshot().activation_epoch, initial_epoch);
    assert!(sequence.snapshot().pending_requests.is_empty());
    sequence
        .set_active(&SequenceItemId::new("a"), clock)
        .unwrap();
    let active_epoch = sequence.snapshot().activation_epoch;
    sequence.drain_events();

    sequence
        .replace(vec![item("b"), item("a")], None, clock)
        .unwrap();
    assert_eq!(
        sequence.snapshot().active_item_id,
        Some(SequenceItemId::new("a"))
    );
    assert_eq!(sequence.snapshot().activation_epoch, active_epoch);
    sequence.remove(&SequenceItemId::new("a"), clock).unwrap();
    assert_eq!(sequence.snapshot().active_item_id, None);
    assert_eq!(sequence.snapshot().activation_epoch, active_epoch);
    assert!(
        !sequence
            .drain_events()
            .iter()
            .any(|event| matches!(event.kind, SequenceEventKind::ActiveItemChanged { .. }))
    );

    sequence.next(clock).unwrap();
    assert_eq!(
        sequence.snapshot().active_item_id,
        Some(SequenceItemId::new("b"))
    );
    assert_ne!(sequence.snapshot().activation_epoch, active_epoch);
}

#[test]
fn accepting_a_resolved_source_preserves_the_explicit_activation_epoch() {
    let mut sequence = SequenceCoordinator::new("test", SequenceConfig::default()).unwrap();
    let clock = now();
    sequence.replace(vec![item("a")], None, clock).unwrap();
    sequence
        .set_active(&SequenceItemId::new("a"), clock)
        .unwrap();
    let epoch = sequence.snapshot().activation_epoch;
    let request = sequence
        .snapshot()
        .pending_requests
        .into_iter()
        .find_map(|request| {
            if let SequenceRequestKind::Source(source) = request.kind {
                Some(source)
            } else {
                None
            }
        })
        .unwrap();
    sequence.drain_events();
    sequence
        .submit_resolved_source(SequenceResolvedSource {
            session_generation: request.session_generation,
            request_id: request.request_id,
            attempt_id: request.attempt_id,
            item_id: request.item_id,
            expected_revision: request.expected_revision,
            source_revision: SequenceSourceRevision::new(1),
            source_reference: SequenceSourceReference::new("source-1"),
            cache_identity: SequenceCacheIdentity::new(
                "fixture",
                "a",
                "default",
                "source-1",
                "session-1",
                SequenceSourceRevision::new(1),
            ),
            warmup_goal: SequenceWarmupGoal::DashSegmentBaseStartup,
            expires_at_epoch_ms: None,
        })
        .unwrap();
    assert_eq!(sequence.snapshot().activation_epoch, epoch);
    assert!(
        !sequence
            .drain_events()
            .iter()
            .any(|event| matches!(event.kind, SequenceEventKind::ActiveItemChanged { .. }))
    );
}
