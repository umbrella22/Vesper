use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

use player_download::{PlayerError, PlayerResult};
use player_model::MediaSource;
use player_preload::{
    PreloadBudgetProvider, PreloadBudgetScope, PreloadCandidate, PreloadCandidateKind,
    PreloadConfig, PreloadEvent, PreloadExecutor, PreloadPlanner, PreloadPriority,
    PreloadSelectionHint, PreloadSnapshot, PreloadSourceIdentity, PreloadTaskId,
    PreloadTaskSnapshot, PreloadTaskStatus, preload_candidate_precedes_or_equal,
};

/// Stable playlist identifier.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct PlaylistId(String);

impl PlaylistId {
    /// Creates a playlist id by trimming the provided value.
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into().trim().to_owned())
    }

    /// Returns the normalized playlist id.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Stable identifier for one item in a playlist queue.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct PlaylistQueueItemId(String);

impl PlaylistQueueItemId {
    /// Creates a queue item id by trimming the provided value.
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into().trim().to_owned())
    }

    /// Returns the normalized queue item id.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Preload costs and timing attached to a playlist item.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PlaylistItemPreloadProfile {
    /// Expected memory bytes charged when the item is preloaded.
    pub expected_memory_bytes: u64,
    /// Expected disk bytes charged when the item is preloaded.
    pub expected_disk_bytes: u64,
    /// Optional time-to-live for preload tasks derived from this item.
    pub ttl: Option<Duration>,
    /// Optional warmup window overriding the preload budget default.
    pub warmup_window: Option<Duration>,
}

/// One media item in a playlist queue.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlaylistQueueItem {
    /// Stable item id.
    pub item_id: PlaylistQueueItemId,
    /// Media source to play.
    pub source: MediaSource,
    /// Preload configuration used when this item becomes desirable.
    pub preload_profile: PlaylistItemPreloadProfile,
}

impl PlaylistQueueItem {
    /// Creates an item with a default preload profile.
    pub fn new(item_id: impl Into<String>, source: MediaSource) -> Self {
        Self {
            item_id: PlaylistQueueItemId::new(item_id),
            source,
            preload_profile: PlaylistItemPreloadProfile::default(),
        }
    }

    /// Replaces the item's preload profile.
    pub fn with_preload_profile(mut self, preload_profile: PlaylistItemPreloadProfile) -> Self {
        self.preload_profile = preload_profile;
        self
    }
}

/// Number of adjacent items considered part of the playback neighborhood.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlaylistNeighborWindow {
    /// Number of previous items to preload near the active item.
    pub previous: usize,
    /// Number of next items to preload near the active item.
    pub next: usize,
}

impl Default for PlaylistNeighborWindow {
    fn default() -> Self {
        Self {
            previous: 1,
            next: 1,
        }
    }
}

/// Limits for viewport-driven preload candidates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlaylistPreloadWindow {
    /// Maximum number of near-visible items to preload.
    pub near_visible: usize,
    /// Maximum number of prefetch-only items to preload.
    pub prefetch_only: usize,
}

impl Default for PlaylistPreloadWindow {
    fn default() -> Self {
        Self {
            near_visible: 2,
            prefetch_only: 2,
        }
    }
}

/// Visibility class reported by a host viewport.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlaylistViewportHintKind {
    /// The item is visible and may become the active item.
    Visible,
    /// The item is close to the viewport and should be preloaded normally.
    NearVisible,
    /// The item should be considered only for background prefetch.
    PrefetchOnly,
    /// The item is hidden or should not influence selection.
    Hidden,
}

/// Host-provided visibility hint for one queue item.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlaylistViewportHint {
    /// Queue item the hint applies to.
    pub item_id: PlaylistQueueItemId,
    /// Visibility class.
    pub kind: PlaylistViewportHintKind,
    /// Stable ordering within hints of the same kind.
    pub order: u32,
}

impl PlaylistViewportHint {
    /// Creates a hint with order `0`.
    pub fn new(item_id: impl Into<String>, kind: PlaylistViewportHintKind) -> Self {
        Self {
            item_id: PlaylistQueueItemId::new(item_id),
            kind,
            order: 0,
        }
    }

    /// Sets the hint order used for visible and preload ordering.
    pub fn with_order(mut self, order: u32) -> Self {
        self.order = order;
        self
    }
}

/// Repeat behavior used when advancing past queue boundaries or completion.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlaylistRepeatMode {
    /// Do not repeat at boundaries.
    Off,
    /// Repeat the active item on playback completion.
    One,
    /// Wrap next/previous navigation at queue boundaries.
    All,
}

/// Behavior used when playback of an item fails.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlaylistFailureStrategy {
    /// Keep the current item active.
    Pause,
    /// Try to activate the next item.
    SkipToNext,
}

/// Policy controlling automatic playlist switches.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlaylistSwitchPolicy {
    /// Whether completion should automatically advance when repeat mode allows it.
    pub auto_advance: bool,
    /// Repeat behavior for completion and queue boundaries.
    pub repeat_mode: PlaylistRepeatMode,
    /// Failure handling strategy.
    pub failure_strategy: PlaylistFailureStrategy,
}

impl Default for PlaylistSwitchPolicy {
    fn default() -> Self {
        Self {
            auto_advance: true,
            repeat_mode: PlaylistRepeatMode::Off,
            failure_strategy: PlaylistFailureStrategy::SkipToNext,
        }
    }
}

/// Configuration for playlist coordination.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PlaylistCoordinatorConfig {
    /// Adjacent items considered for high-priority preload.
    pub neighbor_window: PlaylistNeighborWindow,
    /// Viewport-driven preload limits.
    pub preload_window: PlaylistPreloadWindow,
    /// Active item switching policy.
    pub switch_policy: PlaylistSwitchPolicy,
}

/// Reason an item became active.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlaylistActivationReason {
    /// Initial queue activation.
    Initial,
    /// Queue replacement changed or confirmed the active item.
    QueueUpdate,
    /// Viewport hints changed the active item.
    Viewport,
    /// Manual next navigation.
    ManualNext,
    /// Manual previous navigation.
    ManualPrevious,
    /// Playback completion selected the item.
    PlaybackCompleted,
    /// Playback failure selected the item.
    PlaybackFailed,
    /// Repeat-one reused the current item.
    RepeatCurrent,
}

/// Active playlist item with its queue index and activation reason.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlaylistActiveItem {
    /// Active queue item id.
    pub item_id: PlaylistQueueItemId,
    /// Active item index in the current queue.
    pub index: usize,
    /// Media source for the active item.
    pub source: MediaSource,
    /// Reason this item is active.
    pub reason: PlaylistActivationReason,
}

/// External trigger that asks the coordinator to advance.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlaylistAdvanceTrigger {
    /// Manual next navigation.
    ManualNext,
    /// Manual previous navigation.
    ManualPrevious,
    /// Active item playback completed.
    PlaybackCompleted,
    /// Active item playback failed.
    PlaybackFailed,
}

/// Result of resolving an advance trigger.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlaylistAdvanceOutcome {
    /// A different item became active.
    Activated(PlaylistActiveItem),
    /// The active item should be replayed.
    RepeatedCurrent(PlaylistActiveItem),
    /// Policy kept the current state unchanged.
    NoChange,
    /// No item was available in the requested direction.
    ReachedEnd,
}

/// Full decision record for an advance trigger.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlaylistAdvanceDecision {
    /// Trigger that produced the decision.
    pub trigger: PlaylistAdvanceTrigger,
    /// Item active before the trigger, if any.
    pub from_item_id: Option<PlaylistQueueItemId>,
    /// Decision outcome.
    pub outcome: PlaylistAdvanceOutcome,
    /// Whether the decision wrapped around a queue boundary.
    pub wrapped: bool,
}

/// Snapshot of one queue item.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlaylistQueueItemSnapshot {
    /// Queue item id.
    pub item_id: PlaylistQueueItemId,
    /// Item index in the current queue.
    pub index: usize,
    /// Media source for the item.
    pub source: MediaSource,
    /// Latest viewport hint known for this item.
    pub viewport_hint: PlaylistViewportHintKind,
    /// Whether this item is currently active.
    pub is_active: bool,
}

/// Snapshot of playlist state and preload state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlaylistSnapshot {
    /// Playlist id.
    pub playlist_id: PlaylistId,
    /// Queue item snapshots in queue order.
    pub queue: Vec<PlaylistQueueItemSnapshot>,
    /// Current active item, if any.
    pub active_item: Option<PlaylistActiveItem>,
    /// Neighbor preload window in effect.
    pub neighbor_window: PlaylistNeighborWindow,
    /// Viewport preload window in effect.
    pub preload_window: PlaylistPreloadWindow,
    /// Switching policy in effect.
    pub switch_policy: PlaylistSwitchPolicy,
    /// Underlying preload planner snapshot.
    pub preload: PreloadSnapshot,
}

/// Event emitted by a playlist coordinator.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlaylistEvent {
    /// Queue state changed.
    QueueChanged(PlaylistSnapshot),
    /// Active item changed or was re-emitted during sync.
    ActiveItemChanged(PlaylistActiveItem),
    /// Viewport hints changed, sorted by hint order.
    ViewportHintsChanged(Vec<PlaylistViewportHint>),
    /// An advance trigger was resolved.
    AdvanceResolved(PlaylistAdvanceDecision),
    /// Event forwarded from the underlying preload planner.
    Preload(PreloadEvent),
}

/// Coordinates playlist state and the preload tasks implied by that state.
#[derive(Debug)]
pub struct PlaylistCoordinator<P, E> {
    playlist_id: PlaylistId,
    config: PlaylistCoordinatorConfig,
    queue: Vec<PlaylistQueueItem>,
    active_item_id: Option<PlaylistQueueItemId>,
    viewport_hints: HashMap<PlaylistQueueItemId, PlaylistViewportHint>,
    preload_planner: PreloadPlanner<P, E>,
    events: Vec<PlaylistEvent>,
}

impl<P, E> PlaylistCoordinator<P, E>
where
    P: PreloadBudgetProvider,
    E: PreloadExecutor,
{
    /// Creates an empty coordinator with the provided preload adapters.
    pub fn new(
        playlist_id: impl Into<String>,
        config: PlaylistCoordinatorConfig,
        budget_provider: P,
        executor: E,
    ) -> Self {
        Self {
            playlist_id: PlaylistId::new(playlist_id),
            config,
            queue: Vec::new(),
            active_item_id: None,
            viewport_hints: HashMap::new(),
            preload_planner: PreloadPlanner::new(budget_provider, executor),
            events: Vec::new(),
        }
    }

    /// Returns the underlying preload executor.
    pub fn preload_executor(&self) -> &E {
        self.preload_planner.executor()
    }

    /// Returns the underlying preload executor mutably.
    pub fn preload_executor_mut(&mut self) -> &mut E {
        self.preload_planner.executor_mut()
    }

    /// Returns the current playlist and preload snapshot.
    pub fn snapshot(&self) -> PlaylistSnapshot {
        let active_item = self.active_item();
        let active_item_id = active_item.as_ref().map(|item| item.item_id.clone());
        let queue = self
            .queue
            .iter()
            .enumerate()
            .map(|(index, item)| PlaylistQueueItemSnapshot {
                item_id: item.item_id.clone(),
                index,
                source: item.source.clone(),
                viewport_hint: self
                    .viewport_hints
                    .get(&item.item_id)
                    .map(|hint| hint.kind)
                    .unwrap_or(PlaylistViewportHintKind::Hidden),
                is_active: active_item_id
                    .as_ref()
                    .is_some_and(|active_item_id| *active_item_id == item.item_id),
            })
            .collect::<Vec<_>>();

        PlaylistSnapshot {
            playlist_id: self.playlist_id.clone(),
            queue,
            active_item,
            neighbor_window: self.config.neighbor_window,
            preload_window: self.config.preload_window,
            switch_policy: self.config.switch_policy,
            preload: self.preload_planner.snapshot(),
        }
    }

    /// Drains pending playlist and forwarded preload events.
    pub fn drain_events(&mut self) -> Vec<PlaylistEvent> {
        std::mem::take(&mut self.events)
    }

    /// Replaces the queue and reconciles active item and preloads.
    ///
    /// The coordinator prefers the first visible hint, then the existing active
    /// item if it still exists, then the first queue item.
    pub fn replace_queue(
        &mut self,
        queue: impl IntoIterator<Item = PlaylistQueueItem>,
        now: Instant,
    ) {
        self.queue = queue.into_iter().collect();
        self.viewport_hints
            .retain(|item_id, _| self.queue.iter().any(|item| item.item_id == *item_id));

        let desired_active = self
            .preferred_visible_item_id()
            .or_else(|| {
                self.active_item_id.as_ref().and_then(|item_id| {
                    self.queue
                        .iter()
                        .any(|item| item.item_id == *item_id)
                        .then_some(item_id.clone())
                })
            })
            .or_else(|| self.queue.first().map(|item| item.item_id.clone()));

        let reason = if self.active_item_id.is_none() {
            PlaylistActivationReason::Initial
        } else {
            PlaylistActivationReason::QueueUpdate
        };
        self.active_item_id = desired_active;
        self.sync(reason, now);
        self.events
            .push(PlaylistEvent::QueueChanged(self.snapshot()));
    }

    /// Replaces viewport hints and reconciles active item and preloads.
    ///
    /// Hidden hints and hints for unknown items are ignored. The lowest-order
    /// visible hint becomes active when present.
    pub fn update_viewport_hints(
        &mut self,
        hints: impl IntoIterator<Item = PlaylistViewportHint>,
        now: Instant,
    ) {
        self.viewport_hints.clear();

        for hint in hints {
            if self.queue.iter().any(|item| item.item_id == hint.item_id)
                && hint.kind != PlaylistViewportHintKind::Hidden
            {
                self.viewport_hints.insert(hint.item_id.clone(), hint);
            }
        }

        if let Some(next_active_item_id) = self.preferred_visible_item_id() {
            self.active_item_id = Some(next_active_item_id);
        } else if self.active_item_id.is_none() {
            self.active_item_id = self.queue.first().map(|item| item.item_id.clone());
        }

        self.sync(PlaylistActivationReason::Viewport, now);

        let mut viewport_hints = self.viewport_hints.values().cloned().collect::<Vec<_>>();
        viewport_hints.sort_by_key(|hint| hint.order);
        self.events
            .push(PlaylistEvent::ViewportHintsChanged(viewport_hints));
    }

    /// Clears all viewport hints and reconciles preload state.
    pub fn clear_viewport_hints(&mut self, now: Instant) {
        if self.viewport_hints.is_empty() {
            return;
        }

        self.viewport_hints.clear();
        self.sync(PlaylistActivationReason::Viewport, now);
        self.events
            .push(PlaylistEvent::ViewportHintsChanged(Vec::new()));
    }

    /// Resolves manual next navigation.
    pub fn advance_to_next(&mut self, now: Instant) -> PlaylistAdvanceDecision {
        self.advance(PlaylistAdvanceTrigger::ManualNext, now)
    }

    /// Resolves manual previous navigation.
    pub fn advance_to_previous(&mut self, now: Instant) -> PlaylistAdvanceDecision {
        self.advance(PlaylistAdvanceTrigger::ManualPrevious, now)
    }

    /// Resolves playback completion according to the switch policy.
    pub fn handle_playback_completed(&mut self, now: Instant) -> PlaylistAdvanceDecision {
        self.advance(PlaylistAdvanceTrigger::PlaybackCompleted, now)
    }

    /// Resolves playback failure according to the failure strategy.
    pub fn handle_playback_failed(&mut self, now: Instant) -> PlaylistAdvanceDecision {
        self.advance(PlaylistAdvanceTrigger::PlaybackFailed, now)
    }

    /// Marks a forwarded preload task as completed.
    pub fn complete_preload_task(
        &mut self,
        task_id: PreloadTaskId,
    ) -> PlayerResult<Option<PreloadTaskSnapshot>> {
        let result = self.preload_planner.complete(task_id)?;
        self.collect_preload_events();
        Ok(result)
    }

    /// Marks a forwarded preload task as failed.
    pub fn fail_preload_task(
        &mut self,
        task_id: PreloadTaskId,
        error: PlayerError,
    ) -> PlayerResult<Option<PreloadTaskSnapshot>> {
        let result = self.preload_planner.fail(task_id, error)?;
        self.collect_preload_events();
        Ok(result)
    }

    /// Returns the current active item, if it still exists in the queue.
    pub fn active_item(&self) -> Option<PlaylistActiveItem> {
        let active_item_id = self.active_item_id.as_ref()?;
        let index = self
            .queue
            .iter()
            .position(|item| item.item_id == *active_item_id)?;
        let item = self.queue.get(index)?;

        Some(PlaylistActiveItem {
            item_id: item.item_id.clone(),
            index,
            source: item.source.clone(),
            reason: PlaylistActivationReason::QueueUpdate,
        })
    }

    fn advance(
        &mut self,
        trigger: PlaylistAdvanceTrigger,
        now: Instant,
    ) -> PlaylistAdvanceDecision {
        let from_item_id = self.active_item_id.clone();
        let decision = match trigger {
            PlaylistAdvanceTrigger::ManualNext => self.advance_in_direction(1, trigger, now),
            PlaylistAdvanceTrigger::ManualPrevious => self.advance_in_direction(-1, trigger, now),
            PlaylistAdvanceTrigger::PlaybackCompleted => self.advance_after_completion(now),
            PlaylistAdvanceTrigger::PlaybackFailed => self.advance_after_failure(now),
        };
        let decision = PlaylistAdvanceDecision {
            trigger,
            from_item_id,
            outcome: decision.outcome,
            wrapped: decision.wrapped,
        };
        self.events
            .push(PlaylistEvent::AdvanceResolved(decision.clone()));
        decision
    }

    fn advance_after_completion(&mut self, now: Instant) -> AdvanceResult {
        match self.config.switch_policy.repeat_mode {
            PlaylistRepeatMode::One => self.repeat_current(PlaylistActivationReason::RepeatCurrent),
            PlaylistRepeatMode::Off | PlaylistRepeatMode::All => {
                if !self.config.switch_policy.auto_advance {
                    return AdvanceResult::no_change();
                }
                self.advance_in_direction_inner(1, PlaylistActivationReason::PlaybackCompleted, now)
            }
        }
    }

    fn advance_after_failure(&mut self, now: Instant) -> AdvanceResult {
        match self.config.switch_policy.failure_strategy {
            PlaylistFailureStrategy::Pause => AdvanceResult::no_change(),
            PlaylistFailureStrategy::SkipToNext => {
                self.advance_in_direction_inner(1, PlaylistActivationReason::PlaybackFailed, now)
            }
        }
    }

    fn advance_in_direction(
        &mut self,
        direction: isize,
        trigger: PlaylistAdvanceTrigger,
        now: Instant,
    ) -> AdvanceResult {
        let reason = match trigger {
            PlaylistAdvanceTrigger::ManualNext => PlaylistActivationReason::ManualNext,
            PlaylistAdvanceTrigger::ManualPrevious => PlaylistActivationReason::ManualPrevious,
            PlaylistAdvanceTrigger::PlaybackCompleted => {
                PlaylistActivationReason::PlaybackCompleted
            }
            PlaylistAdvanceTrigger::PlaybackFailed => PlaylistActivationReason::PlaybackFailed,
        };
        self.advance_in_direction_inner(direction, reason, now)
    }

    fn advance_in_direction_inner(
        &mut self,
        direction: isize,
        reason: PlaylistActivationReason,
        now: Instant,
    ) -> AdvanceResult {
        let Some(current_index) = self.current_index() else {
            return AdvanceResult::reached_end();
        };
        let Some(next_index) = self.next_index(current_index, direction) else {
            return AdvanceResult::reached_end();
        };

        let Some(item) = self.queue.get(next_index) else {
            return AdvanceResult::reached_end();
        };

        let wrapped = if direction.is_negative() {
            next_index > current_index
        } else {
            next_index < current_index
        };
        let active_item = PlaylistActiveItem {
            item_id: item.item_id.clone(),
            index: next_index,
            source: item.source.clone(),
            reason,
        };
        self.active_item_id = Some(active_item.item_id.clone());
        self.sync(reason, now);
        AdvanceResult {
            outcome: PlaylistAdvanceOutcome::Activated(active_item),
            wrapped,
        }
    }

    fn repeat_current(&self, reason: PlaylistActivationReason) -> AdvanceResult {
        let outcome = self
            .current_index()
            .and_then(|index| self.queue.get(index))
            .map(|item| {
                PlaylistAdvanceOutcome::RepeatedCurrent(PlaylistActiveItem {
                    item_id: item.item_id.clone(),
                    index: self.current_index().unwrap_or_default(),
                    source: item.source.clone(),
                    reason,
                })
            })
            .unwrap_or(PlaylistAdvanceOutcome::NoChange);
        AdvanceResult {
            outcome,
            wrapped: false,
        }
    }

    fn current_index(&self) -> Option<usize> {
        let active_item_id = self.active_item_id.as_ref()?;
        self.queue
            .iter()
            .position(|item| item.item_id == *active_item_id)
    }

    fn next_index(&self, current_index: usize, direction: isize) -> Option<usize> {
        if self.queue.is_empty() {
            return None;
        }

        match direction.cmp(&0) {
            std::cmp::Ordering::Less => {
                if current_index > 0 {
                    Some(current_index - 1)
                } else if self.config.switch_policy.repeat_mode == PlaylistRepeatMode::All {
                    Some(self.queue.len().saturating_sub(1))
                } else {
                    None
                }
            }
            std::cmp::Ordering::Greater => {
                let next_index = current_index.saturating_add(1);
                if next_index < self.queue.len() {
                    Some(next_index)
                } else if self.config.switch_policy.repeat_mode == PlaylistRepeatMode::All {
                    Some(0)
                } else {
                    None
                }
            }
            std::cmp::Ordering::Equal => Some(current_index),
        }
    }

    fn sync(&mut self, reason: PlaylistActivationReason, now: Instant) {
        self.reconcile_active_item(reason);
        self.sync_preloads(now);
        self.push_active_item_event(reason);
        self.collect_preload_events();
    }

    fn reconcile_active_item(&mut self, reason: PlaylistActivationReason) {
        let Some(active_item_id) = self.active_item_id.clone() else {
            return;
        };

        if self.queue.iter().all(|item| item.item_id != active_item_id) {
            self.active_item_id = self
                .preferred_visible_item_id()
                .or_else(|| self.queue.first().map(|item| item.item_id.clone()));
        }

        if self.active_item_id.is_none() && !self.queue.is_empty() {
            self.active_item_id = Some(self.queue[0].item_id.clone());
        }

        if matches!(reason, PlaylistActivationReason::Viewport)
            && self.active_item_id.is_none()
            && !self.queue.is_empty()
        {
            self.active_item_id = Some(self.queue[0].item_id.clone());
        }
    }

    fn push_active_item_event(&mut self, reason: PlaylistActivationReason) {
        let Some(active_item_id) = self.active_item_id.as_ref() else {
            return;
        };
        let Some(index) = self
            .queue
            .iter()
            .position(|item| item.item_id == *active_item_id)
        else {
            return;
        };
        let Some(item) = self.queue.get(index) else {
            return;
        };

        self.events
            .push(PlaylistEvent::ActiveItemChanged(PlaylistActiveItem {
                item_id: item.item_id.clone(),
                index,
                source: item.source.clone(),
                reason,
            }));
    }

    fn sync_preloads(&mut self, now: Instant) {
        let desired_candidates = self.desired_candidates();
        let desired_keys = desired_candidates
            .values()
            .map(|candidate| PreloadSourceIdentity::from_media_source(&candidate.source))
            .map(|identity| identity.as_str().to_owned())
            .collect::<HashSet<_>>();

        let live_task_ids = self
            .preload_planner
            .snapshot()
            .tasks
            .into_iter()
            .filter(|task| {
                matches!(
                    task.status,
                    PreloadTaskStatus::Planned | PreloadTaskStatus::Active
                )
            })
            .filter(|task| !desired_keys.contains(task.source_identity.as_str()))
            .map(|task| task.task_id)
            .collect::<Vec<_>>();

        for task_id in live_task_ids {
            let _ = self.preload_planner.cancel(task_id);
        }

        self.preload_planner
            .plan(desired_candidates.into_values().collect::<Vec<_>>(), now);
    }

    fn desired_candidates(&self) -> HashMap<String, PreloadCandidate> {
        let mut desired = HashMap::new();
        let scope = PreloadBudgetScope::Playlist(self.playlist_id.as_str().to_owned());

        if let Some(active_index) = self.current_index() {
            if let Some(active_item) = self.queue.get(active_index) {
                self.insert_candidate(
                    &mut desired,
                    active_item,
                    scope.clone(),
                    PreloadCandidateKind::Current,
                    PreloadSelectionHint::CurrentItem,
                    PreloadPriority::Critical,
                );
            }

            let start = active_index.saturating_sub(self.config.neighbor_window.previous);
            let end = active_index
                .saturating_add(self.config.neighbor_window.next)
                .min(self.queue.len().saturating_sub(1));
            for index in start..=end {
                if index == active_index {
                    continue;
                }
                if let Some(item) = self.queue.get(index) {
                    self.insert_candidate(
                        &mut desired,
                        item,
                        scope.clone(),
                        PreloadCandidateKind::Neighbor,
                        PreloadSelectionHint::NeighborItem,
                        PreloadPriority::High,
                    );
                }
            }
        }

        let mut near_visible = self
            .viewport_hints
            .values()
            .filter(|hint| hint.kind == PlaylistViewportHintKind::NearVisible)
            .cloned()
            .collect::<Vec<_>>();
        near_visible.sort_by_key(|hint| hint.order);

        for hint in near_visible
            .into_iter()
            .take(self.config.preload_window.near_visible)
        {
            if let Some(item) = self.queue.iter().find(|item| item.item_id == hint.item_id) {
                self.insert_candidate(
                    &mut desired,
                    item,
                    scope.clone(),
                    PreloadCandidateKind::Recommended,
                    PreloadSelectionHint::RecommendedItem,
                    PreloadPriority::Normal,
                );
            }
        }

        let mut prefetch_only = self
            .viewport_hints
            .values()
            .filter(|hint| hint.kind == PlaylistViewportHintKind::PrefetchOnly)
            .cloned()
            .collect::<Vec<_>>();
        prefetch_only.sort_by_key(|hint| hint.order);

        for hint in prefetch_only
            .into_iter()
            .take(self.config.preload_window.prefetch_only)
        {
            if let Some(item) = self.queue.iter().find(|item| item.item_id == hint.item_id) {
                self.insert_candidate(
                    &mut desired,
                    item,
                    scope.clone(),
                    PreloadCandidateKind::Background,
                    PreloadSelectionHint::BackgroundFill,
                    PreloadPriority::Background,
                );
            }
        }

        desired
    }

    fn insert_candidate(
        &self,
        desired: &mut HashMap<String, PreloadCandidate>,
        item: &PlaylistQueueItem,
        scope: PreloadBudgetScope,
        kind: PreloadCandidateKind,
        selection_hint: PreloadSelectionHint,
        priority: PreloadPriority,
    ) {
        let candidate = PreloadCandidate {
            source: item.source.clone(),
            scope,
            kind,
            selection_hint,
            config: PreloadConfig {
                priority,
                ttl: item.preload_profile.ttl,
                expected_memory_bytes: item.preload_profile.expected_memory_bytes,
                expected_disk_bytes: item.preload_profile.expected_disk_bytes,
                warmup_window: item.preload_profile.warmup_window,
            },
        };
        let key = PreloadSourceIdentity::from_media_source(&item.source)
            .as_str()
            .to_owned();

        match desired.get(&key) {
            Some(existing) if candidate_precedes(existing, &candidate) => {}
            _ => {
                desired.insert(key, candidate);
            }
        }
    }

    fn preferred_visible_item_id(&self) -> Option<PlaylistQueueItemId> {
        self.viewport_hints
            .values()
            .filter(|hint| hint.kind == PlaylistViewportHintKind::Visible)
            .min_by_key(|hint| hint.order)
            .map(|hint| hint.item_id.clone())
    }

    fn collect_preload_events(&mut self) {
        self.events.extend(
            self.preload_planner
                .drain_events()
                .into_iter()
                .map(PlaylistEvent::Preload),
        );
    }
}

#[derive(Debug, Clone)]
struct AdvanceResult {
    outcome: PlaylistAdvanceOutcome,
    wrapped: bool,
}

impl AdvanceResult {
    fn no_change() -> Self {
        Self {
            outcome: PlaylistAdvanceOutcome::NoChange,
            wrapped: false,
        }
    }

    fn reached_end() -> Self {
        Self {
            outcome: PlaylistAdvanceOutcome::ReachedEnd,
            wrapped: false,
        }
    }
}

fn candidate_precedes(current: &PreloadCandidate, next: &PreloadCandidate) -> bool {
    preload_candidate_precedes_or_equal(current, next)
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use player_model::MediaSource;

    use super::{
        PlaylistActivationReason, PlaylistAdvanceOutcome, PlaylistCoordinator,
        PlaylistCoordinatorConfig, PlaylistFailureStrategy, PlaylistItemPreloadProfile,
        PlaylistPreloadWindow, PlaylistQueueItem, PlaylistRepeatMode, PlaylistSwitchPolicy,
        PlaylistViewportHint, PlaylistViewportHintKind,
    };
    use player_preload::{
        InMemoryPreloadBudgetProvider, InMemoryPreloadExecutor, PreloadBudget, PreloadTaskStatus,
    };

    fn test_budget(max_concurrent_tasks: u32) -> PreloadBudget {
        PreloadBudget {
            max_concurrent_tasks,
            max_memory_bytes: 128,
            max_disk_bytes: 128,
            warmup_window: Duration::from_secs(30),
        }
    }

    fn coordinator() -> PlaylistCoordinator<InMemoryPreloadBudgetProvider, InMemoryPreloadExecutor>
    {
        PlaylistCoordinator::new(
            "playlist-a",
            PlaylistCoordinatorConfig::default(),
            InMemoryPreloadBudgetProvider::new(test_budget(8)),
            InMemoryPreloadExecutor::default(),
        )
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

    fn active_task_uris(
        coordinator: &PlaylistCoordinator<InMemoryPreloadBudgetProvider, InMemoryPreloadExecutor>,
    ) -> Vec<String> {
        coordinator
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
            .collect::<Vec<_>>()
    }

    #[test]
    fn playlist_replace_queue_plans_active_and_neighbor_items() {
        let mut coordinator = coordinator();
        coordinator.replace_queue(
            [
                item("item-1", "https://example.com/1.m3u8"),
                item("item-2", "https://example.com/2.m3u8"),
                item("item-3", "https://example.com/3.m3u8"),
            ],
            Instant::now(),
        );

        let snapshot = coordinator.snapshot();
        let active_item = snapshot.active_item.expect("active item should exist");
        assert_eq!(active_item.item_id.as_str(), "item-1");

        let uris = active_task_uris(&coordinator);
        assert_eq!(
            uris,
            vec![
                "https://example.com/1.m3u8".to_owned(),
                "https://example.com/2.m3u8".to_owned(),
            ]
        );
    }

    #[test]
    fn visible_hint_promotes_item_to_active_and_cancels_old_preloads() {
        let mut coordinator = coordinator();
        let now = Instant::now();
        coordinator.replace_queue(
            [
                item("item-1", "https://example.com/1.m3u8"),
                item("item-2", "https://example.com/2.m3u8"),
                item("item-3", "https://example.com/3.m3u8"),
                item("item-4", "https://example.com/4.m3u8"),
            ],
            now,
        );

        coordinator.update_viewport_hints(
            [PlaylistViewportHint::new(
                "item-4",
                PlaylistViewportHintKind::Visible,
            )],
            now,
        );

        let snapshot = coordinator.snapshot();
        let active_item = snapshot.active_item.expect("active item should exist");
        assert_eq!(active_item.item_id.as_str(), "item-4");

        let mut uris = active_task_uris(&coordinator);
        uris.sort();
        assert_eq!(
            uris,
            vec![
                "https://example.com/3.m3u8".to_owned(),
                "https://example.com/4.m3u8".to_owned(),
            ]
        );

        let cancelled = snapshot
            .preload
            .tasks
            .into_iter()
            .filter(|task| task.status == PreloadTaskStatus::Cancelled)
            .map(|task| task.source.uri().to_owned())
            .collect::<Vec<_>>();
        assert!(cancelled.contains(&"https://example.com/1.m3u8".to_owned()));
    }

    #[test]
    fn preload_window_limits_near_visible_and_prefetch_only_candidates() {
        let mut coordinator = PlaylistCoordinator::new(
            "playlist-a",
            PlaylistCoordinatorConfig {
                preload_window: PlaylistPreloadWindow {
                    near_visible: 1,
                    prefetch_only: 1,
                },
                ..PlaylistCoordinatorConfig::default()
            },
            InMemoryPreloadBudgetProvider::new(test_budget(8)),
            InMemoryPreloadExecutor::default(),
        );
        let now = Instant::now();
        coordinator.replace_queue(
            [
                item("item-1", "https://example.com/1.m3u8"),
                item("item-2", "https://example.com/2.m3u8"),
                item("item-3", "https://example.com/3.m3u8"),
                item("item-4", "https://example.com/4.m3u8"),
                item("item-5", "https://example.com/5.m3u8"),
            ],
            now,
        );

        coordinator.update_viewport_hints(
            [
                PlaylistViewportHint::new("item-4", PlaylistViewportHintKind::NearVisible)
                    .with_order(0),
                PlaylistViewportHint::new("item-5", PlaylistViewportHintKind::NearVisible)
                    .with_order(1),
                PlaylistViewportHint::new("item-3", PlaylistViewportHintKind::PrefetchOnly)
                    .with_order(2),
            ],
            now,
        );

        let mut uris = active_task_uris(&coordinator);
        uris.sort();
        assert_eq!(
            uris,
            vec![
                "https://example.com/1.m3u8".to_owned(),
                "https://example.com/2.m3u8".to_owned(),
                "https://example.com/3.m3u8".to_owned(),
                "https://example.com/4.m3u8".to_owned(),
            ]
        );
        assert!(!uris.contains(&"https://example.com/5.m3u8".to_owned()));
    }

    #[test]
    fn switch_policy_covers_auto_advance_repeat_and_failure_fallback() {
        let mut coordinator = PlaylistCoordinator::new(
            "playlist-a",
            PlaylistCoordinatorConfig::default(),
            InMemoryPreloadBudgetProvider::new(test_budget(8)),
            InMemoryPreloadExecutor::default(),
        );
        let now = Instant::now();
        coordinator.replace_queue(
            [
                item("item-1", "https://example.com/1.m3u8"),
                item("item-2", "https://example.com/2.m3u8"),
            ],
            now,
        );

        let completed = coordinator.handle_playback_completed(now);
        match completed.outcome {
            PlaylistAdvanceOutcome::Activated(active_item) => {
                assert_eq!(active_item.item_id.as_str(), "item-2");
                assert_eq!(
                    active_item.reason,
                    PlaylistActivationReason::PlaybackCompleted
                );
            }
            other => panic!("unexpected completion outcome: {other:?}"),
        }

        let failed = coordinator.handle_playback_failed(now);
        assert_eq!(failed.outcome, PlaylistAdvanceOutcome::ReachedEnd);

        let mut repeat_one = PlaylistCoordinator::new(
            "playlist-b",
            PlaylistCoordinatorConfig {
                switch_policy: PlaylistSwitchPolicy {
                    auto_advance: true,
                    repeat_mode: PlaylistRepeatMode::One,
                    failure_strategy: PlaylistFailureStrategy::Pause,
                },
                ..PlaylistCoordinatorConfig::default()
            },
            InMemoryPreloadBudgetProvider::new(test_budget(8)),
            InMemoryPreloadExecutor::default(),
        );
        repeat_one.replace_queue([item("item-1", "https://example.com/1.m3u8")], now);

        let repeated = repeat_one.handle_playback_completed(now);
        match repeated.outcome {
            PlaylistAdvanceOutcome::RepeatedCurrent(active_item) => {
                assert_eq!(active_item.item_id.as_str(), "item-1");
            }
            other => panic!("unexpected repeat outcome: {other:?}"),
        }
    }
}
