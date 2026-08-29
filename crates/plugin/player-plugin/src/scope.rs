//! Metadata-only plugin runtime scope lifecycle.

mod playback;

use std::num::NonZeroU64;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, Instant};

use thiserror::Error;

use crate::PluginPlan;

pub use playback::{
    MAX_PLUGIN_CORRELATION_ID_BYTES, PluginActivePlaybackCorrelation, PluginNextPrewarmCorrelation,
    PluginPlaybackAttachment, PluginPlaybackAttachmentToken, PluginPlaybackAuthority,
    PluginPlaybackError, PluginPlaybackRole, PluginPlaybackTransitionReport,
    PluginSessionCorrelation,
};

/// Maximum diagnostic bytes retained for a scope failure reason.
pub const MAX_PLUGIN_SCOPE_REASON_BYTES: usize = 512;
/// Maximum direct children retained by one scope.
pub const MAX_PLUGIN_SCOPE_CHILDREN: usize = 64;
/// Maximum owner disposers retained by one scope.
pub const MAX_PLUGIN_SCOPE_OWNERS: usize = 64;
/// Maximum scope nesting below the runtime root.
pub const MAX_PLUGIN_SCOPE_DEPTH: usize = 16;
/// Maximum scope registrations during one runtime lifetime.
pub const MAX_PLUGIN_RUNTIME_SCOPE_REGISTRATIONS: usize = 1_024;
/// Maximum owner registrations during one runtime lifetime.
pub const MAX_PLUGIN_RUNTIME_OWNER_REGISTRATIONS: usize = 1_024;
/// Maximum detailed quarantine records retained in one close report.
pub const MAX_PLUGIN_SCOPE_QUARANTINE_RECORDS: usize = 128;
/// Default total deadline for direct scope settlement.
pub const DEFAULT_PLUGIN_SCOPE_CLOSE_TIMEOUT: Duration = Duration::from_millis(250);
/// Default total deadline used when a runtime is dropped.
pub const DEFAULT_PLUGIN_RUNTIME_SHUTDOWN_TIMEOUT: Duration = Duration::from_millis(500);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Ord, PartialOrd)]
pub enum PluginScopeKind {
    Root,
    Player,
    Playback,
    NextPrewarm,
    Operation,
    Worker,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Ord, PartialOrd)]
pub enum PluginScopeState {
    Created,
    Starting,
    Running,
    Draining,
    Closed,
    Failed,
    Cancelled,
    Quarantined,
}

/// Non-zero disposer identity that is unique within one `PluginRuntime`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Ord, PartialOrd)]
pub struct PluginOwnerToken(NonZeroU64);

impl PluginOwnerToken {
    pub fn get(self) -> u64 {
        self.0.get()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Ord, PartialOrd)]
pub enum PluginScopeQuarantineReason {
    Failed,
    Panicked,
    TimedOut,
    WorkerUnavailable,
}

/// Signals that an owner cleanup completed but could not release its resource.
///
/// The scope records the typed failure without retaining an arbitrary external
/// error payload. Boundary adapters remain responsible for their own detailed
/// diagnostics.
#[derive(Debug, Error, Clone, Copy, PartialEq, Eq)]
#[error("plugin owner cleanup failed")]
pub struct PluginOwnerDisposalError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Ord, PartialOrd)]
pub struct PluginScopeQuarantine {
    pub owner_token: PluginOwnerToken,
    pub reason: PluginScopeQuarantineReason,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Ord, PartialOrd)]
pub enum PluginScopeResource {
    Children,
    Owners,
    Depth,
    ScopeRegistrations,
    OwnerRegistrations,
    ActivePlaybackSlot,
    NextPrewarmSlot,
}

/// Bounded aggregate from one settlement attempt.
///
/// A `Quarantined` final state means cleanup was isolated after a panic,
/// timeout, worker failure, or concurrent close. It does not prove that the
/// quarantined resource was released.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginScopeCloseReport {
    pub children_closed: usize,
    pub children_quarantined: usize,
    pub busy_scopes_quarantined: usize,
    pub disposers_run: usize,
    pub owners_settled: usize,
    pub owners_quarantined: usize,
    pub disposer_failures: usize,
    pub disposer_panics: usize,
    pub disposer_timeouts: usize,
    pub disposer_worker_failures: usize,
    pub quarantined_owners: Vec<PluginScopeQuarantine>,
    pub quarantine_records_dropped: usize,
    pub final_state: PluginScopeState,
}

impl Default for PluginScopeCloseReport {
    fn default() -> Self {
        Self {
            children_closed: 0,
            children_quarantined: 0,
            busy_scopes_quarantined: 0,
            disposers_run: 0,
            owners_settled: 0,
            owners_quarantined: 0,
            disposer_failures: 0,
            disposer_panics: 0,
            disposer_timeouts: 0,
            disposer_worker_failures: 0,
            quarantined_owners: Vec::new(),
            quarantine_records_dropped: 0,
            final_state: PluginScopeState::Closed,
        }
    }
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum PluginScopeError {
    #[error("plugin scope `{kind:?}` cannot transition from {state:?}")]
    InvalidTransition {
        kind: PluginScopeKind,
        state: PluginScopeState,
    },
    #[error("plugin scope `{kind:?}` is already terminal in {state:?}")]
    Terminal {
        kind: PluginScopeKind,
        state: PluginScopeState,
    },
    #[error("plugin scope close is already in progress")]
    Busy,
    #[error("plugin scope failure reason must contain 1 to {limit} UTF-8 bytes")]
    InvalidFailureReason { limit: usize },
    #[error("plugin scope {resource:?} capacity exceeds {limit}")]
    CapacityExceeded {
        resource: PluginScopeResource,
        limit: usize,
    },
    #[error("plugin scope owner token space is exhausted")]
    OwnerTokenExhausted,
    #[error("plugin scope close timeout cannot be represented by the monotonic clock")]
    InvalidCloseTimeout,
}

/// Runtime owner for one immutable plan and its root scope.
pub struct PluginRuntime {
    plan: PluginPlan,
    root: PluginScope,
    playback: Mutex<playback::PluginPlaybackSlots>,
}

impl PluginRuntime {
    pub fn new(plan: PluginPlan) -> Self {
        Self {
            plan,
            root: PluginScope::new_root(),
            playback: Mutex::new(playback::PluginPlaybackSlots::default()),
        }
    }

    pub fn plan(&self) -> &PluginPlan {
        &self.plan
    }

    pub fn root_scope(&self) -> PluginScope {
        self.root.clone()
    }

    /// Closes the root with one total deadline shared by the complete scope tree.
    ///
    /// An already draining root is marked `Quarantined` so shutdown never waits
    /// on an unbounded concurrent close.
    pub fn shutdown(&self, timeout: Duration) -> Result<PluginScopeCloseReport, PluginScopeError> {
        self.begin_playback_shutdown();
        self.root.close_with_timeout(timeout)
    }
}

impl Drop for PluginRuntime {
    fn drop(&mut self) {
        let _ = self.shutdown(DEFAULT_PLUGIN_RUNTIME_SHUTDOWN_TIMEOUT);
    }
}

/// A hierarchical lifecycle coordinator that never carries media data.
#[derive(Clone)]
pub struct PluginScope {
    context: Arc<ScopeContext>,
    inner: Arc<Mutex<ScopeInner>>,
}

struct ScopeContext {
    next_owner_token: AtomicU64,
    registered_scopes: AtomicUsize,
    registered_owners: AtomicUsize,
}

struct ScopeInner {
    kind: PluginScopeKind,
    depth: usize,
    state: PluginScopeState,
    failure_reason: Option<String>,
    settlement_report: Option<PluginScopeCloseReport>,
    children: Vec<PluginScope>,
    owners: Vec<PluginOwnerDisposer>,
}

struct PluginOwnerDisposer {
    token: PluginOwnerToken,
    disposer: Box<dyn FnOnce() -> Result<(), PluginOwnerDisposalError> + Send + 'static>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BusyChildPolicy {
    Restore,
    Quarantine,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DisposerOutcome {
    Completed,
    Failed,
    Panicked,
    WorkerUnavailable,
}

impl PluginScope {
    fn new_root() -> Self {
        let context = Arc::new(ScopeContext {
            next_owner_token: AtomicU64::new(1),
            registered_scopes: AtomicUsize::new(1),
            registered_owners: AtomicUsize::new(0),
        });
        Self::new(PluginScopeKind::Root, 0, context)
    }

    fn new(kind: PluginScopeKind, depth: usize, context: Arc<ScopeContext>) -> Self {
        Self {
            context,
            inner: Arc::new(Mutex::new(ScopeInner {
                kind,
                depth,
                state: PluginScopeState::Created,
                failure_reason: None,
                settlement_report: None,
                children: Vec::new(),
                owners: Vec::new(),
            })),
        }
    }

    pub fn kind(&self) -> PluginScopeKind {
        self.lock().kind
    }

    pub fn state(&self) -> PluginScopeState {
        self.lock().state
    }

    pub fn failure_reason(&self) -> Option<String> {
        self.lock().failure_reason.clone()
    }

    /// Returns the most recent bounded terminal settlement aggregate.
    pub fn last_close_report(&self) -> Option<PluginScopeCloseReport> {
        self.lock().settlement_report.clone()
    }

    pub fn start(&self) -> Result<(), PluginScopeError> {
        let mut inner = self.lock();
        if inner.state != PluginScopeState::Created {
            return Err(PluginScopeError::InvalidTransition {
                kind: inner.kind,
                state: inner.state,
            });
        }
        inner.state = PluginScopeState::Starting;
        inner.state = PluginScopeState::Running;
        Ok(())
    }

    pub fn create_child(&self, kind: PluginScopeKind) -> Result<PluginScope, PluginScopeError> {
        let mut inner = self.lock();
        Self::ensure_mutable(&inner)?;
        inner.children.retain(|child| {
            !matches!(
                child.state(),
                PluginScopeState::Closed
                    | PluginScopeState::Failed
                    | PluginScopeState::Cancelled
                    | PluginScopeState::Quarantined
            )
        });
        let finite_slot = match kind {
            PluginScopeKind::Playback => Some(PluginScopeResource::ActivePlaybackSlot),
            PluginScopeKind::NextPrewarm => Some(PluginScopeResource::NextPrewarmSlot),
            PluginScopeKind::Root
            | PluginScopeKind::Player
            | PluginScopeKind::Operation
            | PluginScopeKind::Worker => None,
        };
        if let Some(resource) = finite_slot
            && inner.children.iter().any(|child| child.kind() == kind)
        {
            return Err(PluginScopeError::CapacityExceeded { resource, limit: 1 });
        }
        if inner.children.len() >= MAX_PLUGIN_SCOPE_CHILDREN {
            return Err(PluginScopeError::CapacityExceeded {
                resource: PluginScopeResource::Children,
                limit: MAX_PLUGIN_SCOPE_CHILDREN,
            });
        }
        if inner.depth >= MAX_PLUGIN_SCOPE_DEPTH {
            return Err(PluginScopeError::CapacityExceeded {
                resource: PluginScopeResource::Depth,
                limit: MAX_PLUGIN_SCOPE_DEPTH,
            });
        }
        if !reserve_bounded(
            &self.context.registered_scopes,
            MAX_PLUGIN_RUNTIME_SCOPE_REGISTRATIONS,
        ) {
            return Err(PluginScopeError::CapacityExceeded {
                resource: PluginScopeResource::ScopeRegistrations,
                limit: MAX_PLUGIN_RUNTIME_SCOPE_REGISTRATIONS,
            });
        }
        let child = Self::new(kind, inner.depth + 1, self.context.clone());
        inner.children.push(child.clone());
        Ok(child)
    }

    pub fn add_disposer<F>(&self, disposer: F) -> Result<(), PluginScopeError>
    where
        F: FnOnce() + Send + 'static,
    {
        self.add_owner_disposer(disposer).map(|_| ())
    }

    /// Registers one owner cleanup and returns its runtime-local quarantine token.
    pub fn add_owner_disposer<F>(&self, disposer: F) -> Result<PluginOwnerToken, PluginScopeError>
    where
        F: FnOnce() + Send + 'static,
    {
        self.add_fallible_owner_disposer(move || {
            disposer();
            Ok(())
        })
    }

    /// Registers one owner cleanup that can report a typed settlement failure.
    pub fn add_fallible_owner_disposer<F>(
        &self,
        disposer: F,
    ) -> Result<PluginOwnerToken, PluginScopeError>
    where
        F: FnOnce() -> Result<(), PluginOwnerDisposalError> + Send + 'static,
    {
        let mut inner = self.lock();
        Self::ensure_mutable(&inner)?;
        if inner.owners.len() >= MAX_PLUGIN_SCOPE_OWNERS {
            return Err(PluginScopeError::CapacityExceeded {
                resource: PluginScopeResource::Owners,
                limit: MAX_PLUGIN_SCOPE_OWNERS,
            });
        }
        if !reserve_bounded(
            &self.context.registered_owners,
            MAX_PLUGIN_RUNTIME_OWNER_REGISTRATIONS,
        ) {
            return Err(PluginScopeError::CapacityExceeded {
                resource: PluginScopeResource::OwnerRegistrations,
                limit: MAX_PLUGIN_RUNTIME_OWNER_REGISTRATIONS,
            });
        }
        let token = match self.next_owner_token() {
            Ok(token) => token,
            Err(error) => {
                self.context
                    .registered_owners
                    .fetch_sub(1, Ordering::Relaxed);
                return Err(error);
            }
        };
        inner.owners.push(PluginOwnerDisposer {
            token,
            disposer: Box::new(disposer),
        });
        Ok(token)
    }

    pub fn fail(
        &self,
        reason: impl Into<String>,
    ) -> Result<PluginScopeCloseReport, PluginScopeError> {
        self.fail_with_timeout(reason, DEFAULT_PLUGIN_SCOPE_CLOSE_TIMEOUT)
    }

    pub fn fail_with_timeout(
        &self,
        reason: impl Into<String>,
        timeout: Duration,
    ) -> Result<PluginScopeCloseReport, PluginScopeError> {
        let reason = validate_failure_reason(reason.into())?;
        self.settle_with_timeout(
            PluginScopeState::Failed,
            Some(reason),
            timeout,
            BusyChildPolicy::Restore,
        )
    }

    pub fn cancel(&self) -> Result<PluginScopeCloseReport, PluginScopeError> {
        self.cancel_with_timeout(DEFAULT_PLUGIN_SCOPE_CLOSE_TIMEOUT)
    }

    pub fn cancel_with_timeout(
        &self,
        timeout: Duration,
    ) -> Result<PluginScopeCloseReport, PluginScopeError> {
        self.settle_with_timeout(
            PluginScopeState::Cancelled,
            None,
            timeout,
            BusyChildPolicy::Restore,
        )
    }

    pub fn close(&self) -> Result<PluginScopeCloseReport, PluginScopeError> {
        self.close_with_timeout_and_policy(
            DEFAULT_PLUGIN_SCOPE_CLOSE_TIMEOUT,
            BusyChildPolicy::Restore,
        )
    }

    /// Closes the scope with one total deadline shared by descendants and owners.
    ///
    /// Remaining time is divided across pending cleanup so one stalled owner
    /// cannot consume every sibling's budget. Concurrent draining work is
    /// isolated as `Quarantined`; `close` retains the retryable `Busy` behavior.
    pub fn close_with_timeout(
        &self,
        timeout: Duration,
    ) -> Result<PluginScopeCloseReport, PluginScopeError> {
        self.close_with_timeout_and_policy(timeout, BusyChildPolicy::Quarantine)
    }

    fn close_with_timeout_and_policy(
        &self,
        timeout: Duration,
        busy_child_policy: BusyChildPolicy,
    ) -> Result<PluginScopeCloseReport, PluginScopeError> {
        match self.settle_with_timeout(PluginScopeState::Closed, None, timeout, busy_child_policy) {
            Err(PluginScopeError::Busy) if busy_child_policy == BusyChildPolicy::Quarantine => {
                self.quarantine_busy_or_settle_now()
            }
            result => result,
        }
    }

    fn settle_with_timeout(
        &self,
        terminal: PluginScopeState,
        failure_reason: Option<String>,
        timeout: Duration,
        busy_child_policy: BusyChildPolicy,
    ) -> Result<PluginScopeCloseReport, PluginScopeError> {
        let deadline = Instant::now()
            .checked_add(timeout)
            .ok_or(PluginScopeError::InvalidCloseTimeout)?;
        self.settle_until(terminal, failure_reason, deadline, busy_child_policy)
    }

    fn settle_until(
        &self,
        terminal: PluginScopeState,
        failure_reason: Option<String>,
        deadline: Instant,
        busy_child_policy: BusyChildPolicy,
    ) -> Result<PluginScopeCloseReport, PluginScopeError> {
        let (previous_state, previous_failure_reason, children, owners) = {
            let mut inner = self.lock();
            match inner.state {
                PluginScopeState::Closed
                | PluginScopeState::Failed
                | PluginScopeState::Cancelled
                | PluginScopeState::Quarantined => {
                    return Ok(PluginScopeCloseReport {
                        final_state: inner.state,
                        ..PluginScopeCloseReport::default()
                    });
                }
                PluginScopeState::Draining => return Err(PluginScopeError::Busy),
                PluginScopeState::Created
                | PluginScopeState::Starting
                | PluginScopeState::Running => {}
            }
            let previous_state = inner.state;
            let previous_failure_reason = inner.failure_reason.clone();
            inner.state = PluginScopeState::Draining;
            inner.failure_reason = failure_reason;
            (
                previous_state,
                previous_failure_reason,
                std::mem::take(&mut inner.children),
                std::mem::take(&mut inner.owners),
            )
        };

        let mut report = PluginScopeCloseReport::default();
        let mut pending_children = children;
        while let Some(child) = pending_children.pop() {
            let remaining_items = pending_children.len() + owners.len() + 1;
            let child_deadline = fair_share_deadline(deadline, remaining_items);
            match child.settle_until(
                PluginScopeState::Closed,
                None,
                child_deadline,
                busy_child_policy,
            ) {
                Ok(child_report) => report.merge_child(child_report),
                Err(PluginScopeError::Busy) if busy_child_policy == BusyChildPolicy::Quarantine => {
                    match child.quarantine_busy_or_settle_now() {
                        Ok(child_report) => report.merge_child(child_report),
                        Err(error) => {
                            self.restore_after_child_error(
                                previous_state,
                                previous_failure_reason,
                                pending_children,
                                child,
                                owners,
                            );
                            return Err(error);
                        }
                    }
                }
                Err(error) => {
                    let inner = self.lock();
                    if inner.state == PluginScopeState::Quarantined {
                        drop(inner);
                        match child.quarantine_busy_or_settle_now() {
                            Ok(child_report) => {
                                report.merge_child(child_report);
                                continue;
                            }
                            Err(quarantine_error) => {
                                self.restore_after_child_error(
                                    previous_state,
                                    previous_failure_reason,
                                    pending_children,
                                    child,
                                    owners,
                                );
                                return Err(quarantine_error);
                            }
                        }
                    }
                    drop(inner);
                    self.restore_after_child_error(
                        previous_state,
                        previous_failure_reason,
                        pending_children,
                        child,
                        owners,
                    );
                    return Err(error);
                }
            }
        }

        let mut pending_owners = owners;
        while let Some(owner) = pending_owners.pop() {
            let owner_deadline = fair_share_deadline(deadline, pending_owners.len() + 1);
            report.run_owner(owner, owner_deadline);
        }

        let mut inner = self.lock();
        if inner.state == PluginScopeState::Draining {
            inner.state = if report.requires_quarantine() {
                PluginScopeState::Quarantined
            } else {
                terminal
            };
        }
        report.final_state = inner.state;
        if let Some(existing) = inner.settlement_report.take() {
            report.merge_aggregate(existing);
        }
        inner.settlement_report = Some(report.clone());
        Ok(report)
    }

    fn next_owner_token(&self) -> Result<PluginOwnerToken, PluginScopeError> {
        let raw = self
            .context
            .next_owner_token
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |current| {
                current.checked_add(1)
            })
            .map_err(|_| PluginScopeError::OwnerTokenExhausted)?;
        NonZeroU64::new(raw)
            .map(PluginOwnerToken)
            .ok_or(PluginScopeError::OwnerTokenExhausted)
    }

    fn restore_after_child_error(
        &self,
        previous_state: PluginScopeState,
        previous_failure_reason: Option<String>,
        pending_children: Vec<PluginScope>,
        child: PluginScope,
        owners: Vec<PluginOwnerDisposer>,
    ) {
        let mut inner = self.lock();
        inner.children.extend(pending_children);
        inner.children.push(child);
        inner.owners.extend(owners);
        if inner.state == PluginScopeState::Draining {
            inner.state = previous_state;
            inner.failure_reason = previous_failure_reason;
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, ScopeInner> {
        self.inner.lock().unwrap_or_else(|error| error.into_inner())
    }

    fn same_identity(&self, other: &PluginScope) -> bool {
        Arc::ptr_eq(&self.inner, &other.inner)
    }

    fn quarantine_busy_scope(&self) -> Result<PluginScopeCloseReport, PluginScopeError> {
        let mut inner = self.lock();
        match inner.state {
            PluginScopeState::Draining => {
                inner.state = PluginScopeState::Quarantined;
                let report = PluginScopeCloseReport {
                    busy_scopes_quarantined: 1,
                    final_state: PluginScopeState::Quarantined,
                    ..PluginScopeCloseReport::default()
                };
                inner.settlement_report = Some(report.clone());
                Ok(report)
            }
            PluginScopeState::Closed
            | PluginScopeState::Failed
            | PluginScopeState::Cancelled
            | PluginScopeState::Quarantined => Ok(PluginScopeCloseReport {
                final_state: inner.state,
                ..PluginScopeCloseReport::default()
            }),
            PluginScopeState::Created | PluginScopeState::Starting | PluginScopeState::Running => {
                Err(PluginScopeError::InvalidTransition {
                    kind: inner.kind,
                    state: inner.state,
                })
            }
        }
    }

    fn quarantine_busy_or_settle_now(&self) -> Result<PluginScopeCloseReport, PluginScopeError> {
        for _ in 0..2 {
            match self.quarantine_busy_scope() {
                Ok(report) => return Ok(report),
                Err(PluginScopeError::InvalidTransition { .. }) => {
                    match self.settle_with_timeout(
                        PluginScopeState::Closed,
                        None,
                        Duration::ZERO,
                        BusyChildPolicy::Quarantine,
                    ) {
                        Err(PluginScopeError::Busy) => continue,
                        result => return result,
                    }
                }
                Err(error) => return Err(error),
            }
        }
        self.quarantine_busy_scope()
    }

    fn ensure_mutable(inner: &ScopeInner) -> Result<(), PluginScopeError> {
        match inner.state {
            PluginScopeState::Created | PluginScopeState::Starting | PluginScopeState::Running => {
                Ok(())
            }
            PluginScopeState::Draining => Err(PluginScopeError::Busy),
            PluginScopeState::Closed
            | PluginScopeState::Failed
            | PluginScopeState::Cancelled
            | PluginScopeState::Quarantined => Err(PluginScopeError::Terminal {
                kind: inner.kind,
                state: inner.state,
            }),
        }
    }

    fn transition_kind(
        &self,
        expected: PluginScopeKind,
        target: PluginScopeKind,
    ) -> Result<(), PluginScopeError> {
        let mut inner = self.lock();
        Self::ensure_mutable(&inner)?;
        if inner.kind != expected {
            return Err(PluginScopeError::InvalidTransition {
                kind: inner.kind,
                state: inner.state,
            });
        }
        inner.kind = target;
        Ok(())
    }

    fn transition_child_kind(
        &self,
        child: &PluginScope,
        expected: PluginScopeKind,
        target: PluginScopeKind,
    ) -> Result<(), PluginScopeError> {
        let mut inner = self.lock();
        Self::ensure_mutable(&inner)?;
        inner.children.retain(|candidate| {
            !matches!(
                candidate.state(),
                PluginScopeState::Closed
                    | PluginScopeState::Failed
                    | PluginScopeState::Cancelled
                    | PluginScopeState::Quarantined
            )
        });
        if !inner
            .children
            .iter()
            .any(|candidate| Arc::ptr_eq(&candidate.inner, &child.inner))
        {
            return Err(PluginScopeError::InvalidTransition {
                kind: child.kind(),
                state: child.state(),
            });
        }
        let finite_slot = match target {
            PluginScopeKind::Playback => Some(PluginScopeResource::ActivePlaybackSlot),
            PluginScopeKind::NextPrewarm => Some(PluginScopeResource::NextPrewarmSlot),
            PluginScopeKind::Root
            | PluginScopeKind::Player
            | PluginScopeKind::Operation
            | PluginScopeKind::Worker => None,
        };
        if let Some(resource) = finite_slot
            && inner.children.iter().any(|candidate| {
                !Arc::ptr_eq(&candidate.inner, &child.inner) && candidate.kind() == target
            })
        {
            return Err(PluginScopeError::CapacityExceeded { resource, limit: 1 });
        }
        child.transition_kind(expected, target)
    }
}

impl PluginScopeCloseReport {
    fn merge_child(&mut self, child: Self) {
        if child.final_state == PluginScopeState::Quarantined {
            self.children_quarantined += 1;
        } else {
            self.children_closed += 1;
        }
        self.merge_aggregate(child);
    }

    fn merge_aggregate(&mut self, other: Self) {
        self.children_closed += other.children_closed;
        self.children_quarantined += other.children_quarantined;
        self.busy_scopes_quarantined += other.busy_scopes_quarantined;
        self.disposers_run += other.disposers_run;
        self.owners_settled += other.owners_settled;
        self.owners_quarantined += other.owners_quarantined;
        self.disposer_panics += other.disposer_panics;
        self.disposer_timeouts += other.disposer_timeouts;
        self.disposer_worker_failures += other.disposer_worker_failures;
        self.disposer_failures += other.disposer_failures;
        self.quarantine_records_dropped += other.quarantine_records_dropped;
        for quarantine in other.quarantined_owners {
            self.push_quarantine(quarantine);
        }
    }

    fn run_owner(&mut self, owner: PluginOwnerDisposer, deadline: Instant) {
        let token = owner.token;
        if Instant::now() >= deadline {
            // Quarantine retains the captured owner so its Drop cannot run on the
            // caller after the close budget has already expired.
            std::mem::forget(owner);
            self.disposer_timeouts += 1;
            self.quarantine_owner(token, PluginScopeQuarantineReason::TimedOut);
            return;
        }
        let owner_holder = Arc::new(Mutex::new(Some(owner)));
        let worker_owner_holder = owner_holder.clone();
        let (sender, receiver) = mpsc::sync_channel(1);
        let thread_name = format!("vesper-plugin-dispose-{}", token.get());
        let spawn_result = std::thread::Builder::new()
            .name(thread_name)
            .spawn(move || {
                let owner = worker_owner_holder
                    .lock()
                    .unwrap_or_else(|error| error.into_inner())
                    .take();
                let outcome = match owner {
                    Some(owner) => match catch_unwind(AssertUnwindSafe(owner.disposer)) {
                        Ok(Ok(())) => DisposerOutcome::Completed,
                        Ok(Err(_)) => DisposerOutcome::Failed,
                        Err(_) => DisposerOutcome::Panicked,
                    },
                    None => DisposerOutcome::WorkerUnavailable,
                };
                let _ = sender.send(outcome);
            });

        if spawn_result.is_err() {
            // The failed worker drops only its Arc clone. Retain the original
            // holder so a captured native owner is not destroyed on this thread.
            std::mem::forget(owner_holder);
            self.disposer_worker_failures += 1;
            self.quarantine_owner(token, PluginScopeQuarantineReason::WorkerUnavailable);
            return;
        }
        drop(owner_holder);

        self.disposers_run += 1;
        let remaining = deadline.saturating_duration_since(Instant::now());
        match receiver.recv_timeout(remaining) {
            Ok(DisposerOutcome::Completed) => self.owners_settled += 1,
            Ok(DisposerOutcome::Failed) => {
                self.disposer_failures += 1;
                self.quarantine_owner(token, PluginScopeQuarantineReason::Failed);
            }
            Ok(DisposerOutcome::Panicked) => {
                self.disposer_panics += 1;
                self.quarantine_owner(token, PluginScopeQuarantineReason::Panicked);
            }
            Ok(DisposerOutcome::WorkerUnavailable) => {
                self.disposer_worker_failures += 1;
                self.quarantine_owner(token, PluginScopeQuarantineReason::WorkerUnavailable);
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                self.disposer_timeouts += 1;
                self.quarantine_owner(token, PluginScopeQuarantineReason::TimedOut);
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                self.disposer_worker_failures += 1;
                self.quarantine_owner(token, PluginScopeQuarantineReason::WorkerUnavailable);
            }
        }
    }

    fn quarantine_owner(
        &mut self,
        owner_token: PluginOwnerToken,
        reason: PluginScopeQuarantineReason,
    ) {
        self.owners_quarantined += 1;
        self.push_quarantine(PluginScopeQuarantine {
            owner_token,
            reason,
        });
    }

    fn push_quarantine(&mut self, quarantine: PluginScopeQuarantine) {
        if self.quarantined_owners.len() < MAX_PLUGIN_SCOPE_QUARANTINE_RECORDS {
            self.quarantined_owners.push(quarantine);
        } else {
            self.quarantine_records_dropped += 1;
        }
    }

    fn requires_quarantine(&self) -> bool {
        self.children_quarantined > 0
            || self.busy_scopes_quarantined > 0
            || self.owners_quarantined > 0
    }
}

fn validate_failure_reason(reason: String) -> Result<String, PluginScopeError> {
    if reason.is_empty() || reason.len() > MAX_PLUGIN_SCOPE_REASON_BYTES {
        return Err(PluginScopeError::InvalidFailureReason {
            limit: MAX_PLUGIN_SCOPE_REASON_BYTES,
        });
    }
    Ok(reason)
}

fn reserve_bounded(counter: &AtomicUsize, limit: usize) -> bool {
    counter
        .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |current| {
            (current < limit).then_some(current + 1)
        })
        .is_ok()
}

fn fair_share_deadline(deadline: Instant, remaining_items: usize) -> Instant {
    let now = Instant::now();
    let divisor = u32::try_from(remaining_items.max(1)).unwrap_or(u32::MAX);
    now.checked_add(deadline.saturating_duration_since(now) / divisor)
        .unwrap_or(deadline)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failed_child_restoration_preserves_concurrent_quarantine() {
        let parent = PluginScope::new_root();
        let child = match parent.create_child(PluginScopeKind::Worker) {
            Ok(child) => child,
            Err(error) => panic!("child fixture failed: {error}"),
        };
        if let Err(error) = parent.add_owner_disposer(|| {}) {
            panic!("owner fixture failed: {error}");
        }
        let owners = {
            let mut inner = parent.lock();
            inner.children.clear();
            inner.state = PluginScopeState::Quarantined;
            inner.failure_reason = Some("concurrent shutdown".to_owned());
            std::mem::take(&mut inner.owners)
        };

        parent.restore_after_child_error(
            PluginScopeState::Running,
            Some("previous failure".to_owned()),
            Vec::new(),
            child,
            owners,
        );

        let inner = parent.lock();
        assert_eq!(inner.state, PluginScopeState::Quarantined);
        assert_eq!(inner.failure_reason.as_deref(), Some("concurrent shutdown"));
        assert_eq!(inner.children.len(), 1);
        assert_eq!(inner.owners.len(), 1);
    }

    #[test]
    fn fallible_owner_cleanup_is_quarantined_without_panicking() {
        let scope = PluginScope::new_root();
        let token = scope
            .add_fallible_owner_disposer(|| Err(PluginOwnerDisposalError))
            .expect("fallible owner");

        let report = scope
            .close_with_timeout(Duration::from_secs(1))
            .expect("scope close");

        assert_eq!(report.final_state, PluginScopeState::Quarantined);
        assert_eq!(report.owners_settled, 0);
        assert_eq!(report.owners_quarantined, 1);
        assert_eq!(report.disposer_failures, 1);
        assert_eq!(report.disposer_panics, 0);
        assert!(report.quarantined_owners.iter().any(|entry| {
            entry.owner_token == token && entry.reason == PluginScopeQuarantineReason::Failed
        }));
    }
}
