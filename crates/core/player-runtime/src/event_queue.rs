use std::collections::VecDeque;

use super::{MAX_PENDING_RUNTIME_EVENTS, PlayerRuntimeEvent};

/// Pushes one runtime event while enforcing the shared adapter queue bound.
pub fn push_runtime_event_bounded(
    queue: &mut VecDeque<PlayerRuntimeEvent>,
    dropped_events: &mut u64,
    event: PlayerRuntimeEvent,
) {
    if queue.len() >= MAX_PENDING_RUNTIME_EVENTS {
        *dropped_events = dropped_events.saturating_add(1);
        return;
    }
    queue.push_back(event);
}

/// Extends a runtime event queue while enforcing the shared adapter queue bound.
pub fn extend_runtime_events_bounded(
    queue: &mut VecDeque<PlayerRuntimeEvent>,
    dropped_events: &mut u64,
    events: impl IntoIterator<Item = PlayerRuntimeEvent>,
) {
    for event in events {
        push_runtime_event_bounded(queue, dropped_events, event);
    }
}
