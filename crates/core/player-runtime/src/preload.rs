use std::cmp::Ordering;
use std::collections::HashMap;
use std::time::{Duration, Instant};

use player_core::MediaSource;

use crate::{
    PlayerResolvedPreloadBudgetPolicy, PlayerRuntimeError, PlayerRuntimeErrorCategory,
    PlayerRuntimeErrorCode, PlayerRuntimeResult,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PreloadTaskId(u64);

impl PreloadTaskId {
    pub fn from_raw(value: u64) -> Self {
        Self(value)
    }

    pub fn get(self) -> u64 {
        self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct PreloadSourceIdentity(String);

impl PreloadSourceIdentity {
    pub fn new(value: impl Into<String>) -> Self {
        Self(normalize_preload_key(value))
    }

    pub fn from_media_source(source: &MediaSource) -> Self {
        Self::new(source.uri())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct PreloadCacheKey(String);

impl PreloadCacheKey {
    pub fn new(value: impl Into<String>) -> Self {
        Self(normalize_preload_key(value))
    }

    pub fn from_media_source(source: &MediaSource) -> Self {
        Self::new(source.uri())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PreloadBudgetScope {
    App,
    Session(String),
    Playlist(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreloadBudget {
    pub max_concurrent_tasks: u32,
    pub max_memory_bytes: u64,
    pub max_disk_bytes: u64,
    pub warmup_window: Duration,
}

impl From<PlayerResolvedPreloadBudgetPolicy> for PreloadBudget {
    fn from(value: PlayerResolvedPreloadBudgetPolicy) -> Self {
        Self {
            max_concurrent_tasks: value.max_concurrent_tasks,
            max_memory_bytes: value.max_memory_bytes,
            max_disk_bytes: value.max_disk_bytes,
            warmup_window: value.warmup_window,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PreloadPriority {
    Critical,
    High,
    Normal,
    Low,
    Background,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum PreloadCandidateKind {
    Current,
    Neighbor,
    Recommended,
    Background,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PreloadSelectionHint {
    None,
    CurrentItem,
    NeighborItem,
    RecommendedItem,
    BackgroundFill,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreloadConfig {
    pub priority: PreloadPriority,
    pub ttl: Option<Duration>,
    pub expected_memory_bytes: u64,
    pub expected_disk_bytes: u64,
    pub warmup_window: Option<Duration>,
}

impl Default for PreloadConfig {
    fn default() -> Self {
        Self {
            priority: PreloadPriority::Normal,
            ttl: None,
            expected_memory_bytes: 0,
            expected_disk_bytes: 0,
            warmup_window: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreloadCandidate {
    pub source: MediaSource,
    pub scope: PreloadBudgetScope,
    pub kind: PreloadCandidateKind,
    pub selection_hint: PreloadSelectionHint,
    pub config: PreloadConfig,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PreloadTaskStatus {
    Planned,
    Active,
    Cancelled,
    Completed,
    Expired,
    Failed,
}

pub type PreloadTaskState = PreloadTaskStatus;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreloadErrorSummary {
    pub code: PlayerRuntimeErrorCode,
    pub category: PlayerRuntimeErrorCategory,
    pub retriable: bool,
    pub message: String,
}

impl From<PlayerRuntimeError> for PreloadErrorSummary {
    fn from(value: PlayerRuntimeError) -> Self {
        Self {
            code: value.code(),
            category: value.category(),
            retriable: value.is_retriable(),
            message: value.message().to_owned(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreloadTaskSnapshot {
    pub task_id: PreloadTaskId,
    pub source: MediaSource,
    pub source_identity: PreloadSourceIdentity,
    pub cache_key: PreloadCacheKey,
    pub scope: PreloadBudgetScope,
    pub kind: PreloadCandidateKind,
    pub selection_hint: PreloadSelectionHint,
    pub priority: PreloadPriority,
    pub status: PreloadTaskStatus,
    pub expected_memory_bytes: u64,
    pub expected_disk_bytes: u64,
    pub warmup_window: Duration,
    pub expires_at: Option<Instant>,
    pub error_summary: Option<PreloadErrorSummary>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreloadSnapshot {
    pub tasks: Vec<PreloadTaskSnapshot>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PreloadEvent {
    Planned(PreloadTaskSnapshot),
    Started(PreloadTaskSnapshot),
    Cancelled(PreloadTaskSnapshot),
    Completed(PreloadTaskSnapshot),
    Expired(PreloadTaskSnapshot),
    Failed(PreloadTaskSnapshot),
}

pub trait PreloadBudgetProvider {
    fn budget_for_scope(&self, scope: &PreloadBudgetScope) -> PreloadBudget;
}

#[derive(Debug, Clone)]
pub struct InMemoryPreloadBudgetProvider {
    app_budget: PreloadBudget,
    session_budgets: HashMap<String, PreloadBudget>,
    playlist_budgets: HashMap<String, PreloadBudget>,
}

impl InMemoryPreloadBudgetProvider {
    pub fn new(app_budget: PreloadBudget) -> Self {
        Self {
            app_budget,
            session_budgets: HashMap::new(),
            playlist_budgets: HashMap::new(),
        }
    }

    pub fn insert_session_budget(
        mut self,
        session_id: impl Into<String>,
        budget: PreloadBudget,
    ) -> Self {
        self.session_budgets.insert(session_id.into(), budget);
        self
    }

    pub fn insert_playlist_budget(
        mut self,
        playlist_id: impl Into<String>,
        budget: PreloadBudget,
    ) -> Self {
        self.playlist_budgets.insert(playlist_id.into(), budget);
        self
    }
}

impl PreloadBudgetProvider for InMemoryPreloadBudgetProvider {
    fn budget_for_scope(&self, scope: &PreloadBudgetScope) -> PreloadBudget {
        match scope {
            PreloadBudgetScope::App => self.app_budget.clone(),
            PreloadBudgetScope::Session(session_id) => self
                .session_budgets
                .get(session_id)
                .cloned()
                .unwrap_or_else(|| self.app_budget.clone()),
            PreloadBudgetScope::Playlist(playlist_id) => self
                .playlist_budgets
                .get(playlist_id)
                .cloned()
                .unwrap_or_else(|| self.app_budget.clone()),
        }
    }
}

pub trait PreloadExecutor {
    fn warmup(&mut self, task: &PreloadTaskSnapshot) -> PlayerRuntimeResult<()>;

    fn cancel(&mut self, task_id: PreloadTaskId) -> PlayerRuntimeResult<()>;
}

#[derive(Debug, Default)]
pub struct InMemoryPreloadExecutor {
    started: Vec<PreloadTaskId>,
    cancelled: Vec<PreloadTaskId>,
}

impl InMemoryPreloadExecutor {
    pub fn started(&self) -> &[PreloadTaskId] {
        &self.started
    }

    pub fn cancelled(&self) -> &[PreloadTaskId] {
        &self.cancelled
    }
}

impl PreloadExecutor for InMemoryPreloadExecutor {
    fn warmup(&mut self, task: &PreloadTaskSnapshot) -> PlayerRuntimeResult<()> {
        self.started.push(task.task_id);
        Ok(())
    }

    fn cancel(&mut self, task_id: PreloadTaskId) -> PlayerRuntimeResult<()> {
        self.cancelled.push(task_id);
        Ok(())
    }
}

#[derive(Debug, Clone)]
struct PreloadTaskRecord {
    task_id: PreloadTaskId,
    source: MediaSource,
    source_identity: PreloadSourceIdentity,
    cache_key: PreloadCacheKey,
    scope: PreloadBudgetScope,
    kind: PreloadCandidateKind,
    selection_hint: PreloadSelectionHint,
    config: PreloadConfig,
    status: PreloadTaskStatus,
    warmup_window: Duration,
    expires_at: Option<Instant>,
    error_summary: Option<PreloadErrorSummary>,
}

impl PreloadTaskRecord {
    fn snapshot(&self) -> PreloadTaskSnapshot {
        PreloadTaskSnapshot {
            task_id: self.task_id,
            source: self.source.clone(),
            source_identity: self.source_identity.clone(),
            cache_key: self.cache_key.clone(),
            scope: self.scope.clone(),
            kind: self.kind,
            selection_hint: self.selection_hint.clone(),
            priority: self.config.priority,
            status: self.status.clone(),
            expected_memory_bytes: self.config.expected_memory_bytes,
            expected_disk_bytes: self.config.expected_disk_bytes,
            warmup_window: self.warmup_window,
            expires_at: self.expires_at,
            error_summary: self.error_summary.clone(),
        }
    }

    fn is_terminal(&self) -> bool {
        matches!(
            self.status,
            PreloadTaskStatus::Cancelled
                | PreloadTaskStatus::Completed
                | PreloadTaskStatus::Expired
                | PreloadTaskStatus::Failed
        )
    }

    fn is_active(&self) -> bool {
        matches!(
            self.status,
            PreloadTaskStatus::Active | PreloadTaskStatus::Planned
        )
    }
}

#[derive(Debug)]
pub struct PreloadPlanner<P, E> {
    budget_provider: P,
    executor: E,
    next_task_id: u64,
    tasks: HashMap<PreloadTaskId, PreloadTaskRecord>,
    events: Vec<PreloadEvent>,
}

impl<P, E> PreloadPlanner<P, E>
where
    P: PreloadBudgetProvider,
    E: PreloadExecutor,
{
    pub fn new(budget_provider: P, executor: E) -> Self {
        Self {
            budget_provider,
            executor,
            next_task_id: 1,
            tasks: HashMap::new(),
            events: Vec::new(),
        }
    }

    pub fn executor(&self) -> &E {
        &self.executor
    }

    pub fn executor_mut(&mut self) -> &mut E {
        &mut self.executor
    }

    pub fn snapshot(&self) -> PreloadSnapshot {
        let mut tasks = self
            .tasks
            .values()
            .map(PreloadTaskRecord::snapshot)
            .collect::<Vec<_>>();
        tasks.sort_by_key(|task| task.task_id.get());
        PreloadSnapshot { tasks }
    }

    pub fn drain_events(&mut self) -> Vec<PreloadEvent> {
        self.events.drain(..).collect()
    }

    pub fn task(&self, task_id: PreloadTaskId) -> Option<PreloadTaskSnapshot> {
        self.tasks.get(&task_id).map(PreloadTaskRecord::snapshot)
    }

    pub fn plan(
        &mut self,
        candidates: impl IntoIterator<Item = PreloadCandidate>,
        now: Instant,
    ) -> Vec<PreloadTaskId> {
        self.expire_due_tasks(now);

        let mut candidates = candidates.into_iter().collect::<Vec<_>>();
        candidates.sort_by(compare_candidates);

        let mut planned = Vec::new();

        for candidate in candidates {
            let cache_key = PreloadCacheKey::from_media_source(&candidate.source);
            if self.has_live_task_for_cache_key(&cache_key) {
                continue;
            }

            let budget = self.budget_provider.budget_for_scope(&candidate.scope);
            if !self.can_schedule(&candidate.scope, &candidate.config, &budget) {
                continue;
            }

            let task_id = PreloadTaskId(self.next_task_id);
            self.next_task_id += 1;

            let record = PreloadTaskRecord {
                task_id,
                source_identity: PreloadSourceIdentity::from_media_source(&candidate.source),
                cache_key,
                source: candidate.source,
                scope: candidate.scope,
                kind: candidate.kind,
                selection_hint: candidate.selection_hint,
                warmup_window: candidate
                    .config
                    .warmup_window
                    .unwrap_or(budget.warmup_window),
                expires_at: candidate.config.ttl.map(|ttl| now + ttl),
                config: candidate.config,
                status: PreloadTaskStatus::Planned,
                error_summary: None,
            };

            let planned_snapshot = record.snapshot();
            self.events
                .push(PreloadEvent::Planned(planned_snapshot.clone()));

            match self.executor.warmup(&planned_snapshot) {
                Ok(()) => {
                    let mut started_record = record;
                    started_record.status = PreloadTaskStatus::Active;
                    let started_snapshot = started_record.snapshot();
                    self.events.push(PreloadEvent::Started(started_snapshot));
                    self.tasks.insert(task_id, started_record);
                    planned.push(task_id);
                }
                Err(error) => {
                    let mut failed_record = record;
                    failed_record.status = PreloadTaskStatus::Failed;
                    failed_record.error_summary = Some(error.into());
                    let failed_snapshot = failed_record.snapshot();
                    self.events.push(PreloadEvent::Failed(failed_snapshot));
                    self.tasks.insert(task_id, failed_record);
                }
            }
        }

        planned
    }

    pub fn cancel(
        &mut self,
        task_id: PreloadTaskId,
    ) -> PlayerRuntimeResult<Option<PreloadTaskSnapshot>> {
        let Some(record) = self.tasks.get_mut(&task_id) else {
            return Ok(None);
        };

        if !record.is_active() {
            return Ok(Some(record.snapshot()));
        }

        self.executor.cancel(task_id)?;
        record.status = PreloadTaskStatus::Cancelled;
        let snapshot = record.snapshot();
        self.events.push(PreloadEvent::Cancelled(snapshot.clone()));
        Ok(Some(snapshot))
    }

    pub fn complete(
        &mut self,
        task_id: PreloadTaskId,
    ) -> PlayerRuntimeResult<Option<PreloadTaskSnapshot>> {
        let Some(record) = self.tasks.get_mut(&task_id) else {
            return Ok(None);
        };

        if record.status != PreloadTaskStatus::Active {
            return Ok(Some(record.snapshot()));
        }

        record.status = PreloadTaskStatus::Completed;
        record.error_summary = None;
        let snapshot = record.snapshot();
        self.events.push(PreloadEvent::Completed(snapshot.clone()));
        Ok(Some(snapshot))
    }

    pub fn fail(
        &mut self,
        task_id: PreloadTaskId,
        error: PlayerRuntimeError,
    ) -> PlayerRuntimeResult<Option<PreloadTaskSnapshot>> {
        let Some(record) = self.tasks.get_mut(&task_id) else {
            return Ok(None);
        };

        if record.is_terminal() {
            return Ok(Some(record.snapshot()));
        }

        record.status = PreloadTaskStatus::Failed;
        record.error_summary = Some(error.into());
        let snapshot = record.snapshot();
        self.events.push(PreloadEvent::Failed(snapshot.clone()));
        Ok(Some(snapshot))
    }

    pub fn resume(
        &mut self,
        task_id: PreloadTaskId,
        now: Instant,
    ) -> PlayerRuntimeResult<Option<PreloadTaskSnapshot>> {
        self.expire_due_tasks(now);

        let Some(current) = self.tasks.get(&task_id).cloned() else {
            return Ok(None);
        };

        if current.status != PreloadTaskStatus::Cancelled {
            return Ok(Some(current.snapshot()));
        }

        if current
            .expires_at
            .is_some_and(|expires_at| expires_at <= now)
        {
            if let Some(record) = self.tasks.get_mut(&task_id) {
                record.status = PreloadTaskStatus::Expired;
                let snapshot = record.snapshot();
                self.events.push(PreloadEvent::Expired(snapshot.clone()));
                return Ok(Some(snapshot));
            }
        }

        let budget = self.budget_provider.budget_for_scope(&current.scope);
        if !self.can_schedule_without_task(task_id, &current.scope, &current.config, &budget) {
            return Ok(Some(current.snapshot()));
        }

        let snapshot = current.snapshot();
        self.executor.warmup(&snapshot)?;

        if let Some(record) = self.tasks.get_mut(&task_id) {
            record.status = PreloadTaskStatus::Active;
            let started_snapshot = record.snapshot();
            self.events
                .push(PreloadEvent::Started(started_snapshot.clone()));
            return Ok(Some(started_snapshot));
        }

        Ok(None)
    }

    pub fn expire_due_tasks(&mut self, now: Instant) {
        let expired_ids = self
            .tasks
            .iter()
            .filter_map(|(task_id, record)| {
                record
                    .expires_at
                    .filter(|expires_at| *expires_at <= now && !record.is_terminal())
                    .map(|_| *task_id)
            })
            .collect::<Vec<_>>();

        for task_id in expired_ids {
            let should_cancel = self
                .tasks
                .get(&task_id)
                .is_some_and(PreloadTaskRecord::is_active);
            if should_cancel {
                let _ = self.executor.cancel(task_id);
            }

            if let Some(record) = self.tasks.get_mut(&task_id) {
                record.status = PreloadTaskStatus::Expired;
                let snapshot = record.snapshot();
                self.events.push(PreloadEvent::Expired(snapshot));
            }
        }
    }

    fn has_live_task_for_cache_key(&self, cache_key: &PreloadCacheKey) -> bool {
        self.tasks.values().any(|record| {
            record.cache_key == *cache_key
                && matches!(
                    record.status,
                    PreloadTaskStatus::Planned | PreloadTaskStatus::Active
                )
        })
    }

    fn can_schedule(
        &self,
        scope: &PreloadBudgetScope,
        config: &PreloadConfig,
        budget: &PreloadBudget,
    ) -> bool {
        self.usage_for_scope(scope, None).fits(config, budget)
    }

    fn can_schedule_without_task(
        &self,
        ignored_task_id: PreloadTaskId,
        scope: &PreloadBudgetScope,
        config: &PreloadConfig,
        budget: &PreloadBudget,
    ) -> bool {
        self.usage_for_scope(scope, Some(ignored_task_id))
            .fits(config, budget)
    }

    fn usage_for_scope(
        &self,
        scope: &PreloadBudgetScope,
        ignored_task_id: Option<PreloadTaskId>,
    ) -> PreloadBudgetUsage {
        self.tasks
            .values()
            .filter(|record| Some(record.task_id) != ignored_task_id)
            .filter(|record| record.scope == *scope)
            .filter(|record| record.is_active())
            .fold(PreloadBudgetUsage::default(), |mut usage, record| {
                usage.active_tasks += 1;
                usage.memory_bytes += record.config.expected_memory_bytes;
                usage.disk_bytes += record.config.expected_disk_bytes;
                usage
            })
    }
}

#[derive(Debug, Default)]
struct PreloadBudgetUsage {
    active_tasks: u32,
    memory_bytes: u64,
    disk_bytes: u64,
}

impl PreloadBudgetUsage {
    fn fits(&self, config: &PreloadConfig, budget: &PreloadBudget) -> bool {
        self.active_tasks < budget.max_concurrent_tasks
            && self
                .memory_bytes
                .saturating_add(config.expected_memory_bytes)
                <= budget.max_memory_bytes
            && self.disk_bytes.saturating_add(config.expected_disk_bytes) <= budget.max_disk_bytes
    }
}

fn compare_candidates(left: &PreloadCandidate, right: &PreloadCandidate) -> Ordering {
    rank_candidate_kind(left.kind)
        .cmp(&rank_candidate_kind(right.kind))
        .then_with(|| {
            rank_priority(left.config.priority).cmp(&rank_priority(right.config.priority))
        })
}

fn rank_candidate_kind(kind: PreloadCandidateKind) -> u8 {
    match kind {
        PreloadCandidateKind::Current => 0,
        PreloadCandidateKind::Neighbor => 1,
        PreloadCandidateKind::Recommended => 2,
        PreloadCandidateKind::Background => 3,
    }
}

fn rank_priority(priority: PreloadPriority) -> u8 {
    match priority {
        PreloadPriority::Critical => 0,
        PreloadPriority::High => 1,
        PreloadPriority::Normal => 2,
        PreloadPriority::Low => 3,
        PreloadPriority::Background => 4,
    }
}

fn normalize_preload_key(value: impl Into<String>) -> String {
    value.into().trim().to_owned()
}

#[cfg(test)]
mod tests {
    use super::{
        InMemoryPreloadBudgetProvider, InMemoryPreloadExecutor, PreloadBudget, PreloadBudgetScope,
        PreloadCandidate, PreloadCandidateKind, PreloadConfig, PreloadEvent, PreloadPlanner,
        PreloadPriority, PreloadSelectionHint, PreloadTaskStatus,
    };
    use crate::{PlayerRuntimeError, PlayerRuntimeErrorCode};
    use player_core::MediaSource;
    use std::time::{Duration, Instant};

    fn budget(
        max_concurrent_tasks: u32,
        max_memory_bytes: u64,
        max_disk_bytes: u64,
    ) -> PreloadBudget {
        PreloadBudget {
            max_concurrent_tasks,
            max_memory_bytes,
            max_disk_bytes,
            warmup_window: Duration::from_secs(30),
        }
    }

    fn candidate(
        uri: &str,
        scope: PreloadBudgetScope,
        kind: PreloadCandidateKind,
        priority: PreloadPriority,
        memory_bytes: u64,
    ) -> PreloadCandidate {
        PreloadCandidate {
            source: MediaSource::new(uri),
            scope,
            kind,
            selection_hint: match kind {
                PreloadCandidateKind::Current => PreloadSelectionHint::CurrentItem,
                PreloadCandidateKind::Neighbor => PreloadSelectionHint::NeighborItem,
                PreloadCandidateKind::Recommended => PreloadSelectionHint::RecommendedItem,
                PreloadCandidateKind::Background => PreloadSelectionHint::BackgroundFill,
            },
            config: PreloadConfig {
                priority,
                ttl: None,
                expected_memory_bytes: memory_bytes,
                expected_disk_bytes: 0,
                warmup_window: None,
            },
        }
    }

    #[test]
    fn planner_prioritizes_current_and_neighbor_candidates_within_budget() {
        let provider = InMemoryPreloadBudgetProvider::new(budget(2, 32, 0));
        let executor = InMemoryPreloadExecutor::default();
        let mut planner = PreloadPlanner::new(provider, executor);
        let now = Instant::now();

        let task_ids = planner.plan(
            [
                candidate(
                    "https://example.com/recommended.m3u8",
                    PreloadBudgetScope::App,
                    PreloadCandidateKind::Recommended,
                    PreloadPriority::Normal,
                    8,
                ),
                candidate(
                    "https://example.com/current.m3u8",
                    PreloadBudgetScope::App,
                    PreloadCandidateKind::Current,
                    PreloadPriority::Critical,
                    8,
                ),
                candidate(
                    "https://example.com/neighbor.m3u8",
                    PreloadBudgetScope::App,
                    PreloadCandidateKind::Neighbor,
                    PreloadPriority::High,
                    8,
                ),
            ],
            now,
        );

        assert_eq!(task_ids.len(), 2);
        let snapshot = planner.snapshot();
        assert_eq!(snapshot.tasks.len(), 2);
        assert_eq!(snapshot.tasks[0].kind, PreloadCandidateKind::Current);
        assert_eq!(snapshot.tasks[1].kind, PreloadCandidateKind::Neighbor);
        assert_eq!(planner.executor().started().len(), 2);
    }

    #[test]
    fn planner_distinguishes_app_session_and_playlist_budget_scopes() {
        let provider = InMemoryPreloadBudgetProvider::new(budget(1, 64, 64))
            .insert_session_budget("session-a", budget(2, 64, 64))
            .insert_playlist_budget("playlist-a", budget(3, 64, 64));
        let executor = InMemoryPreloadExecutor::default();
        let mut planner = PreloadPlanner::new(provider, executor);

        let task_ids = planner.plan(
            [
                candidate(
                    "https://example.com/app-1.m3u8",
                    PreloadBudgetScope::App,
                    PreloadCandidateKind::Current,
                    PreloadPriority::Critical,
                    1,
                ),
                candidate(
                    "https://example.com/app-2.m3u8",
                    PreloadBudgetScope::App,
                    PreloadCandidateKind::Neighbor,
                    PreloadPriority::High,
                    1,
                ),
                candidate(
                    "https://example.com/session-1.m3u8",
                    PreloadBudgetScope::Session("session-a".to_owned()),
                    PreloadCandidateKind::Current,
                    PreloadPriority::Critical,
                    1,
                ),
                candidate(
                    "https://example.com/session-2.m3u8",
                    PreloadBudgetScope::Session("session-a".to_owned()),
                    PreloadCandidateKind::Neighbor,
                    PreloadPriority::High,
                    1,
                ),
                candidate(
                    "https://example.com/playlist-1.m3u8",
                    PreloadBudgetScope::Playlist("playlist-a".to_owned()),
                    PreloadCandidateKind::Current,
                    PreloadPriority::Critical,
                    1,
                ),
                candidate(
                    "https://example.com/playlist-2.m3u8",
                    PreloadBudgetScope::Playlist("playlist-a".to_owned()),
                    PreloadCandidateKind::Neighbor,
                    PreloadPriority::High,
                    1,
                ),
                candidate(
                    "https://example.com/playlist-3.m3u8",
                    PreloadBudgetScope::Playlist("playlist-a".to_owned()),
                    PreloadCandidateKind::Recommended,
                    PreloadPriority::Normal,
                    1,
                ),
            ],
            Instant::now(),
        );

        assert_eq!(task_ids.len(), 6);
        let snapshot = planner.snapshot();
        assert_eq!(snapshot.tasks.len(), 6);
    }

    #[test]
    fn planner_can_cancel_and_resume_active_tasks() {
        let provider = InMemoryPreloadBudgetProvider::new(budget(1, 64, 64));
        let executor = InMemoryPreloadExecutor::default();
        let mut planner = PreloadPlanner::new(provider, executor);
        let now = Instant::now();

        let task_id = planner
            .plan(
                [candidate(
                    "https://example.com/current.m3u8",
                    PreloadBudgetScope::App,
                    PreloadCandidateKind::Current,
                    PreloadPriority::Critical,
                    1,
                )],
                now,
            )
            .into_iter()
            .next()
            .expect("task should be planned");

        let cancelled = planner
            .cancel(task_id)
            .expect("cancel should succeed")
            .expect("task should exist");
        assert_eq!(cancelled.status, PreloadTaskStatus::Cancelled);
        assert_eq!(planner.executor().cancelled(), &[task_id]);

        let resumed = planner
            .resume(task_id, now + Duration::from_secs(1))
            .expect("resume should succeed")
            .expect("task should still exist");
        assert_eq!(resumed.status, PreloadTaskStatus::Active);
        assert_eq!(planner.executor().started().len(), 2);
    }

    #[test]
    fn planner_expires_tasks_and_cancels_warmup() {
        let provider = InMemoryPreloadBudgetProvider::new(budget(1, 64, 64));
        let executor = InMemoryPreloadExecutor::default();
        let mut planner = PreloadPlanner::new(provider, executor);
        let now = Instant::now();

        let task_id = planner
            .plan(
                [PreloadCandidate {
                    source: MediaSource::new("https://example.com/current.m3u8"),
                    scope: PreloadBudgetScope::App,
                    kind: PreloadCandidateKind::Current,
                    selection_hint: PreloadSelectionHint::CurrentItem,
                    config: PreloadConfig {
                        priority: PreloadPriority::Critical,
                        ttl: Some(Duration::from_secs(2)),
                        expected_memory_bytes: 1,
                        expected_disk_bytes: 0,
                        warmup_window: None,
                    },
                }],
                now,
            )
            .into_iter()
            .next()
            .expect("task should be planned");

        planner.expire_due_tasks(now + Duration::from_secs(3));

        let expired = planner.task(task_id).expect("task should exist");
        assert_eq!(expired.status, PreloadTaskStatus::Expired);
        assert_eq!(planner.executor().cancelled(), &[task_id]);

        let events = planner.drain_events();
        assert!(
            events.iter().any(
                |event| matches!(event, PreloadEvent::Expired(task) if task.task_id == task_id)
            )
        );
    }

    #[test]
    fn planner_deduplicates_live_cache_keys() {
        let provider = InMemoryPreloadBudgetProvider::new(budget(2, 64, 64));
        let executor = InMemoryPreloadExecutor::default();
        let mut planner = PreloadPlanner::new(provider, executor);

        let task_ids = planner.plan(
            [
                candidate(
                    "https://example.com/current.m3u8",
                    PreloadBudgetScope::App,
                    PreloadCandidateKind::Current,
                    PreloadPriority::Critical,
                    1,
                ),
                candidate(
                    "https://example.com/current.m3u8",
                    PreloadBudgetScope::Playlist("playlist-a".to_owned()),
                    PreloadCandidateKind::Neighbor,
                    PreloadPriority::High,
                    1,
                ),
            ],
            Instant::now(),
        );

        assert_eq!(task_ids.len(), 1);
        assert_eq!(planner.snapshot().tasks.len(), 1);
    }

    #[test]
    fn planner_completion_releases_budget_for_follow_up_candidates() {
        let provider = InMemoryPreloadBudgetProvider::new(budget(1, 64, 64));
        let executor = InMemoryPreloadExecutor::default();
        let mut planner = PreloadPlanner::new(provider, executor);
        let now = Instant::now();

        let first_task_id = planner
            .plan(
                [candidate(
                    "https://example.com/current.m3u8",
                    PreloadBudgetScope::App,
                    PreloadCandidateKind::Current,
                    PreloadPriority::Critical,
                    1,
                )],
                now,
            )
            .into_iter()
            .next()
            .expect("first task should be planned");

        assert!(
            planner
                .plan(
                    [candidate(
                        "https://example.com/neighbor.m3u8",
                        PreloadBudgetScope::App,
                        PreloadCandidateKind::Neighbor,
                        PreloadPriority::High,
                        1,
                    )],
                    now,
                )
                .is_empty()
        );

        let completed = planner
            .complete(first_task_id)
            .expect("complete should succeed")
            .expect("task should exist");
        assert_eq!(completed.status, PreloadTaskStatus::Completed);

        let next_task_ids = planner.plan(
            [candidate(
                "https://example.com/neighbor.m3u8",
                PreloadBudgetScope::App,
                PreloadCandidateKind::Neighbor,
                PreloadPriority::High,
                1,
            )],
            now,
        );
        assert_eq!(next_task_ids.len(), 1);
    }

    #[test]
    fn planner_failure_records_error_summary_and_releases_budget() {
        let provider = InMemoryPreloadBudgetProvider::new(budget(1, 64, 64));
        let executor = InMemoryPreloadExecutor::default();
        let mut planner = PreloadPlanner::new(provider, executor);
        let now = Instant::now();

        let task_id = planner
            .plan(
                [candidate(
                    "https://example.com/current.m3u8",
                    PreloadBudgetScope::App,
                    PreloadCandidateKind::Current,
                    PreloadPriority::Critical,
                    1,
                )],
                now,
            )
            .into_iter()
            .next()
            .expect("task should be planned");

        let failed = planner
            .fail(
                task_id,
                PlayerRuntimeError::new(
                    PlayerRuntimeErrorCode::BackendFailure,
                    "warmup request timed out",
                ),
            )
            .expect("fail should succeed")
            .expect("task should exist");
        assert_eq!(failed.status, PreloadTaskStatus::Failed);
        assert_eq!(
            failed.error_summary.expect("error summary").code,
            PlayerRuntimeErrorCode::BackendFailure
        );

        let next_task_ids = planner.plan(
            [candidate(
                "https://example.com/neighbor.m3u8",
                PreloadBudgetScope::App,
                PreloadCandidateKind::Neighbor,
                PreloadPriority::High,
                1,
            )],
            now,
        );
        assert_eq!(next_task_ids.len(), 1);
    }
}
