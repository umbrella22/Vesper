//! Finite playback and next-item prewarm scope coordination.

use std::num::NonZeroU64;
use std::time::{Duration, Instant};

use thiserror::Error;

use super::{
    BusyChildPolicy, PluginRuntime, PluginScope, PluginScopeCloseReport, PluginScopeError,
    PluginScopeKind, PluginScopeState, fair_share_deadline,
};

/// Maximum UTF-8 bytes accepted for one non-secret correlation identifier.
///
/// Correlations share the protocol resource-identity bound because they are
/// intended for the same bounded diagnostic envelope.
pub const MAX_PLUGIN_CORRELATION_ID_BYTES: usize = crate::MAX_PLUGIN_RESOURCE_IDENTITY_BYTES;

/// Plan and sequence identity shared by active playback and next-item prewarm.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct PluginSessionCorrelation {
    plan_fingerprint: String,
    session_id: String,
    session_generation: u64,
}

impl PluginSessionCorrelation {
    pub fn new(
        plan_fingerprint: impl Into<String>,
        session_id: impl Into<String>,
        session_generation: u64,
    ) -> Result<Self, PluginPlaybackError> {
        let plan_fingerprint = plan_fingerprint.into();
        if !is_lowercase_sha256(&plan_fingerprint) {
            return Err(PluginPlaybackError::InvalidPlanFingerprint);
        }
        let session_id = validate_correlation_id("session_id", session_id.into())?;
        if session_generation == 0 {
            return Err(PluginPlaybackError::ZeroCorrelationValue {
                field: "session_generation",
            });
        }
        Ok(Self {
            plan_fingerprint,
            session_id,
            session_generation,
        })
    }

    pub fn plan_fingerprint(&self) -> &str {
        &self.plan_fingerprint
    }

    pub fn session_id(&self) -> &str {
        &self.session_id
    }

    pub const fn session_generation(&self) -> u64 {
        self.session_generation
    }
}

/// Correlation carried by one active playback scope.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct PluginActivePlaybackCorrelation {
    session: PluginSessionCorrelation,
    item_id: String,
    source_revision: u64,
    playback_generation: u64,
}

impl PluginActivePlaybackCorrelation {
    pub fn new(
        session: PluginSessionCorrelation,
        item_id: impl Into<String>,
        source_revision: u64,
        playback_generation: u64,
    ) -> Result<Self, PluginPlaybackError> {
        Ok(Self {
            session,
            item_id: validate_correlation_id("item_id", item_id.into())?,
            source_revision: validate_non_zero_correlation("source_revision", source_revision)?,
            playback_generation: validate_non_zero_correlation(
                "playback_generation",
                playback_generation,
            )?,
        })
    }

    pub fn session(&self) -> &PluginSessionCorrelation {
        &self.session
    }

    pub fn item_id(&self) -> &str {
        &self.item_id
    }

    pub const fn source_revision(&self) -> u64 {
        self.source_revision
    }

    pub const fn playback_generation(&self) -> u64 {
        self.playback_generation
    }
}

/// Correlation carried by the one allowed next-item prewarm scope.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct PluginNextPrewarmCorrelation {
    session: PluginSessionCorrelation,
    item_id: String,
    source_revision: u64,
    warmup_task_id: u64,
}

impl PluginNextPrewarmCorrelation {
    pub fn new(
        session: PluginSessionCorrelation,
        item_id: impl Into<String>,
        source_revision: u64,
        warmup_task_id: u64,
    ) -> Result<Self, PluginPlaybackError> {
        Ok(Self {
            session,
            item_id: validate_correlation_id("item_id", item_id.into())?,
            source_revision: validate_non_zero_correlation("source_revision", source_revision)?,
            warmup_task_id: validate_non_zero_correlation("warmup_task_id", warmup_task_id)?,
        })
    }

    pub fn session(&self) -> &PluginSessionCorrelation {
        &self.session
    }

    pub fn item_id(&self) -> &str {
        &self.item_id
    }

    pub const fn source_revision(&self) -> u64 {
        self.source_revision
    }

    pub const fn warmup_task_id(&self) -> u64 {
        self.warmup_task_id
    }
}

/// Runtime-local token that invalidates stale slot attachments.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Ord, PartialOrd)]
pub struct PluginPlaybackAttachmentToken(NonZeroU64);

impl PluginPlaybackAttachmentToken {
    pub const fn get(self) -> u64 {
        self.0.get()
    }
}

/// Authority that only the current active playback slot may exercise.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Ord, PartialOrd)]
pub enum PluginPlaybackAuthority {
    MasterClock,
    VideoSurface,
    AudioSink,
    Participation,
}

/// Finite role held by one managed playback attachment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Ord, PartialOrd)]
pub enum PluginPlaybackRole {
    Active,
    NextPrewarm,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum PluginPlaybackCorrelation {
    Active(PluginActivePlaybackCorrelation),
    NextPrewarm(PluginNextPrewarmCorrelation),
}

/// One runtime-local attachment to a managed metadata-only scope.
#[derive(Clone)]
pub struct PluginPlaybackAttachment {
    token: PluginPlaybackAttachmentToken,
    correlation: PluginPlaybackCorrelation,
    scope: PluginScope,
}

impl PluginPlaybackAttachment {
    pub const fn token(&self) -> PluginPlaybackAttachmentToken {
        self.token
    }

    pub fn role(&self) -> PluginPlaybackRole {
        match self.correlation {
            PluginPlaybackCorrelation::Active(_) => PluginPlaybackRole::Active,
            PluginPlaybackCorrelation::NextPrewarm(_) => PluginPlaybackRole::NextPrewarm,
        }
    }

    pub fn session(&self) -> &PluginSessionCorrelation {
        match &self.correlation {
            PluginPlaybackCorrelation::Active(correlation) => correlation.session(),
            PluginPlaybackCorrelation::NextPrewarm(correlation) => correlation.session(),
        }
    }

    pub fn item_id(&self) -> &str {
        match &self.correlation {
            PluginPlaybackCorrelation::Active(correlation) => correlation.item_id(),
            PluginPlaybackCorrelation::NextPrewarm(correlation) => correlation.item_id(),
        }
    }

    pub const fn source_revision(&self) -> u64 {
        match &self.correlation {
            PluginPlaybackCorrelation::Active(correlation) => correlation.source_revision(),
            PluginPlaybackCorrelation::NextPrewarm(correlation) => correlation.source_revision(),
        }
    }

    pub fn scope(&self) -> PluginScope {
        self.scope.clone()
    }
}

/// Bounded settlement result from promoting or replacing active playback.
#[derive(Clone)]
pub struct PluginPlaybackTransitionReport {
    pub active: PluginPlaybackAttachment,
    pub previous_active: Option<PluginScopeCloseReport>,
    pub discarded_next_prewarm: Option<PluginScopeCloseReport>,
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum PluginPlaybackError {
    #[error("plugin plan fingerprint must be one lowercase SHA-256 value")]
    InvalidPlanFingerprint,
    #[error("plugin correlation id `{field}` must contain 1 to {limit} UTF-8 bytes")]
    InvalidCorrelationId { field: &'static str, limit: usize },
    #[error("plugin correlation value `{field}` must be non-zero")]
    ZeroCorrelationValue { field: &'static str },
    #[error("plugin playback correlation does not match the runtime plan fingerprint")]
    PlanFingerprintMismatch,
    #[error("plugin playback correlation does not match the active session")]
    SessionMismatch,
    #[error("plugin playback session generation mismatch: expected {expected}, got {actual}")]
    SessionGenerationMismatch { expected: u64, actual: u64 },
    #[error("plugin playback item identity does not match the next prewarm slot")]
    ItemMismatch,
    #[error("plugin playback source revision mismatch: expected {expected}, got {actual}")]
    SourceRevisionMismatch { expected: u64, actual: u64 },
    #[error("plugin playback generation must advance beyond {previous}, got {actual}")]
    PlaybackGenerationNotAdvanced { previous: u64, actual: u64 },
    #[error("plugin active playback slot is already occupied")]
    ActiveSlotOccupied,
    #[error("plugin next-item prewarm slot is already occupied")]
    NextPrewarmSlotOccupied,
    #[error("plugin active playback slot is empty")]
    ActiveSlotMissing,
    #[error("plugin next-item prewarm slot is empty")]
    NextPrewarmSlotMissing,
    #[error("plugin playback slot transition is already in progress")]
    TransitionBusy,
    #[error("plugin playback attachment token space is exhausted")]
    AttachmentTokenExhausted,
    #[error("plugin runtime playback slots are shutting down")]
    RuntimeShuttingDown,
    #[error("plugin {role:?} attachment is stale")]
    StaleAttachment { role: PluginPlaybackRole },
    #[error("next-item prewarm cannot commit active authority {authority:?}")]
    NextPrewarmCannotCommit { authority: PluginPlaybackAuthority },
    #[error(transparent)]
    Scope(#[from] PluginScopeError),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PluginPlaybackSlotsState {
    Open,
    Transitioning,
    Shutdown,
}

pub(super) struct PluginPlaybackSlots {
    state: PluginPlaybackSlotsState,
    next_token: u64,
    active: Option<PluginPlaybackAttachment>,
    next_prewarm: Option<PluginPlaybackAttachment>,
}

impl Default for PluginPlaybackSlots {
    fn default() -> Self {
        Self {
            state: PluginPlaybackSlotsState::Open,
            next_token: 1,
            active: None,
            next_prewarm: None,
        }
    }
}

impl PluginRuntime {
    /// Attaches the first active playback scope for this runtime.
    pub fn attach_active_playback(
        &self,
        correlation: PluginActivePlaybackCorrelation,
    ) -> Result<PluginPlaybackAttachment, PluginPlaybackError> {
        self.validate_session(correlation.session())?;
        let mut slots = self.lock_playback();
        slots.ensure_open()?;
        if slots.active.is_some() {
            return Err(PluginPlaybackError::ActiveSlotOccupied);
        }
        let scope = self.create_started_scope(PluginScopeKind::Playback)?;
        let attachment = slots.attachment(PluginPlaybackCorrelation::Active(correlation), scope)?;
        slots.active = Some(attachment.clone());
        Ok(attachment)
    }

    /// Attaches the only allowed next-item prewarm scope.
    pub fn attach_next_prewarm(
        &self,
        correlation: PluginNextPrewarmCorrelation,
    ) -> Result<PluginPlaybackAttachment, PluginPlaybackError> {
        self.validate_session(correlation.session())?;
        let mut slots = self.lock_playback();
        slots.ensure_open()?;
        let active = slots
            .active
            .as_ref()
            .ok_or(PluginPlaybackError::ActiveSlotMissing)?;
        ensure_same_session(active.session(), correlation.session())?;
        if active.item_id() == correlation.item_id() {
            return Err(PluginPlaybackError::ItemMismatch);
        }
        if slots.next_prewarm.is_some() {
            return Err(PluginPlaybackError::NextPrewarmSlotOccupied);
        }
        let scope = self.create_started_scope(PluginScopeKind::NextPrewarm)?;
        let attachment =
            slots.attachment(PluginPlaybackCorrelation::NextPrewarm(correlation), scope)?;
        slots.next_prewarm = Some(attachment.clone());
        Ok(attachment)
    }

    /// Rejects active-only effects from prewarm or stale attachments.
    pub fn authorize_playback_authority(
        &self,
        attachment: &PluginPlaybackAttachment,
        authority: PluginPlaybackAuthority,
    ) -> Result<(), PluginPlaybackError> {
        self.validate_session(attachment.session())?;
        let slots = self.lock_playback();
        slots.ensure_open()?;
        if slots.matches_active(attachment) {
            if attachment.scope.state() == PluginScopeState::Running {
                return Ok(());
            }
            return Err(PluginPlaybackError::StaleAttachment {
                role: PluginPlaybackRole::Active,
            });
        }
        if slots.matches_next(attachment) {
            return Err(PluginPlaybackError::NextPrewarmCannotCommit { authority });
        }
        Err(PluginPlaybackError::StaleAttachment {
            role: attachment.role(),
        })
    }

    /// Promotes the current next prewarm after an authoritative activation.
    pub fn promote_next_prewarm(
        &self,
        prewarm: &PluginPlaybackAttachment,
        correlation: PluginActivePlaybackCorrelation,
        timeout: Duration,
    ) -> Result<PluginPlaybackTransitionReport, PluginPlaybackError> {
        self.validate_session(correlation.session())?;
        let deadline = Instant::now()
            .checked_add(timeout)
            .ok_or(PluginScopeError::InvalidCloseTimeout)?;
        let (previous_active, next_prewarm) = {
            let mut slots = self.lock_playback();
            slots.ensure_open()?;
            if !slots.matches_next(prewarm) {
                return Err(PluginPlaybackError::StaleAttachment {
                    role: PluginPlaybackRole::NextPrewarm,
                });
            }
            let next_prewarm = slots
                .next_prewarm
                .as_ref()
                .ok_or(PluginPlaybackError::NextPrewarmSlotMissing)?;
            validate_promotion(next_prewarm, &correlation)?;
            if let Some(active) = &slots.active {
                ensure_generation_advanced(active, &correlation)?;
            }
            let previous_active = slots.active.take();
            let next_prewarm = slots
                .next_prewarm
                .take()
                .ok_or(PluginPlaybackError::NextPrewarmSlotMissing)?;
            slots.state = PluginPlaybackSlotsState::Transitioning;
            (previous_active, next_prewarm)
        };

        let previous_active_report = match previous_active
            .as_ref()
            .map(|active| {
                active.scope.settle_until(
                    PluginScopeState::Closed,
                    None,
                    deadline,
                    BusyChildPolicy::Quarantine,
                )
            })
            .transpose()
        {
            Ok(report) => report,
            Err(error) => {
                self.recover_playback_transition(previous_active.as_ref(), Some(&next_prewarm));
                return Err(error.into());
            }
        };
        if let Err(error) = self.root.transition_child_kind(
            &next_prewarm.scope,
            PluginScopeKind::NextPrewarm,
            PluginScopeKind::Playback,
        ) {
            self.recover_playback_transition(previous_active.as_ref(), Some(&next_prewarm));
            return Err(error.into());
        }

        let mut slots = self.lock_playback();
        slots.finish_transition()?;
        let active = slots.attachment(
            PluginPlaybackCorrelation::Active(correlation),
            next_prewarm.scope,
        )?;
        slots.active = Some(active.clone());
        Ok(PluginPlaybackTransitionReport {
            active,
            previous_active: previous_active_report,
            discarded_next_prewarm: None,
        })
    }

    /// Replaces active playback and settles any obsolete next prewarm first.
    pub fn replace_active_playback(
        &self,
        correlation: PluginActivePlaybackCorrelation,
        timeout: Duration,
    ) -> Result<PluginPlaybackTransitionReport, PluginPlaybackError> {
        self.validate_session(correlation.session())?;
        let deadline = Instant::now()
            .checked_add(timeout)
            .ok_or(PluginScopeError::InvalidCloseTimeout)?;
        let (previous_active, discarded_next_prewarm) = {
            let mut slots = self.lock_playback();
            slots.ensure_open()?;
            if let Some(active) = &slots.active {
                ensure_generation_advanced(active, &correlation)?;
            }
            slots.state = PluginPlaybackSlotsState::Transitioning;
            (slots.active.take(), slots.next_prewarm.take())
        };

        let pending =
            usize::from(previous_active.is_some()) + usize::from(discarded_next_prewarm.is_some());
        let discarded_next_prewarm_report = match discarded_next_prewarm
            .as_ref()
            .map(|prewarm| {
                prewarm.scope.settle_until(
                    PluginScopeState::Cancelled,
                    None,
                    fair_share_deadline(deadline, pending.max(1)),
                    BusyChildPolicy::Quarantine,
                )
            })
            .transpose()
        {
            Ok(report) => report,
            Err(error) => {
                self.recover_playback_transition(
                    previous_active.as_ref(),
                    discarded_next_prewarm.as_ref(),
                );
                return Err(error.into());
            }
        };
        let previous_active_report = match previous_active
            .as_ref()
            .map(|active| {
                active.scope.settle_until(
                    PluginScopeState::Closed,
                    None,
                    deadline,
                    BusyChildPolicy::Quarantine,
                )
            })
            .transpose()
        {
            Ok(report) => report,
            Err(error) => {
                self.recover_playback_transition(
                    previous_active.as_ref(),
                    discarded_next_prewarm.as_ref(),
                );
                return Err(error.into());
            }
        };

        let scope = match self.create_started_scope(PluginScopeKind::Playback) {
            Ok(scope) => scope,
            Err(error) => {
                self.abort_playback_transition();
                return Err(error);
            }
        };
        let mut slots = self.lock_playback();
        slots.finish_transition()?;
        let active = slots.attachment(PluginPlaybackCorrelation::Active(correlation), scope)?;
        slots.active = Some(active.clone());
        Ok(PluginPlaybackTransitionReport {
            active,
            previous_active: previous_active_report,
            discarded_next_prewarm: discarded_next_prewarm_report,
        })
    }

    /// Cancels the exact next prewarm attachment and rejects stale callers.
    pub fn cancel_next_prewarm(
        &self,
        prewarm: &PluginPlaybackAttachment,
        timeout: Duration,
    ) -> Result<PluginScopeCloseReport, PluginPlaybackError> {
        let deadline = Instant::now()
            .checked_add(timeout)
            .ok_or(PluginScopeError::InvalidCloseTimeout)?;
        let attachment = {
            let mut slots = self.lock_playback();
            slots.begin_transition()?;
            if !slots.matches_next(prewarm) {
                slots.state = PluginPlaybackSlotsState::Open;
                return Err(PluginPlaybackError::StaleAttachment {
                    role: PluginPlaybackRole::NextPrewarm,
                });
            }
            slots
                .next_prewarm
                .take()
                .ok_or(PluginPlaybackError::NextPrewarmSlotMissing)?
        };
        let report = match attachment.scope.settle_until(
            PluginScopeState::Cancelled,
            None,
            deadline,
            BusyChildPolicy::Quarantine,
        ) {
            Ok(report) => report,
            Err(error) => {
                self.recover_playback_transition(None, Some(&attachment));
                return Err(error.into());
            }
        };
        self.lock_playback().finish_transition()?;
        Ok(report)
    }

    fn validate_session(
        &self,
        session: &PluginSessionCorrelation,
    ) -> Result<(), PluginPlaybackError> {
        if session.plan_fingerprint() != self.plan().fingerprint() {
            return Err(PluginPlaybackError::PlanFingerprintMismatch);
        }
        Ok(())
    }

    fn create_started_scope(
        &self,
        kind: PluginScopeKind,
    ) -> Result<PluginScope, PluginPlaybackError> {
        match self.root.state() {
            PluginScopeState::Created => self.root.start()?,
            PluginScopeState::Running => {}
            PluginScopeState::Starting | PluginScopeState::Draining => {
                return Err(PluginPlaybackError::TransitionBusy);
            }
            PluginScopeState::Closed
            | PluginScopeState::Failed
            | PluginScopeState::Cancelled
            | PluginScopeState::Quarantined => {
                return Err(PluginPlaybackError::RuntimeShuttingDown);
            }
        }
        let scope = self.root.create_child(kind)?;
        if let Err(error) = scope.start() {
            let _ = scope.close();
            return Err(error.into());
        }
        Ok(scope)
    }

    fn lock_playback(&self) -> std::sync::MutexGuard<'_, PluginPlaybackSlots> {
        self.playback
            .lock()
            .unwrap_or_else(|error| error.into_inner())
    }

    fn abort_playback_transition(&self) {
        self.recover_playback_transition(None, None);
    }

    fn recover_playback_transition(
        &self,
        active: Option<&PluginPlaybackAttachment>,
        next_prewarm: Option<&PluginPlaybackAttachment>,
    ) {
        let mut slots = self.lock_playback();
        if slots.state == PluginPlaybackSlotsState::Transitioning {
            if let Some(active) = active
                && active.role() == PluginPlaybackRole::Active
                && active.scope.state() == PluginScopeState::Running
            {
                slots.active = Some(active.clone());
            }
            if let Some(next_prewarm) = next_prewarm
                && next_prewarm.role() == PluginPlaybackRole::NextPrewarm
                && next_prewarm.scope.state() == PluginScopeState::Running
            {
                slots.next_prewarm = Some(next_prewarm.clone());
            }
            slots.state = PluginPlaybackSlotsState::Open;
        }
    }

    pub(super) fn begin_playback_shutdown(&self) {
        let mut slots = self.lock_playback();
        slots.state = PluginPlaybackSlotsState::Shutdown;
        slots.active = None;
        slots.next_prewarm = None;
    }
}

impl PluginPlaybackSlots {
    fn ensure_open(&self) -> Result<(), PluginPlaybackError> {
        match self.state {
            PluginPlaybackSlotsState::Open => Ok(()),
            PluginPlaybackSlotsState::Transitioning => Err(PluginPlaybackError::TransitionBusy),
            PluginPlaybackSlotsState::Shutdown => Err(PluginPlaybackError::RuntimeShuttingDown),
        }
    }

    fn begin_transition(&mut self) -> Result<(), PluginPlaybackError> {
        self.ensure_open()?;
        self.state = PluginPlaybackSlotsState::Transitioning;
        Ok(())
    }

    fn finish_transition(&mut self) -> Result<(), PluginPlaybackError> {
        match self.state {
            PluginPlaybackSlotsState::Transitioning => {
                self.state = PluginPlaybackSlotsState::Open;
                Ok(())
            }
            PluginPlaybackSlotsState::Open => Err(PluginPlaybackError::TransitionBusy),
            PluginPlaybackSlotsState::Shutdown => Err(PluginPlaybackError::RuntimeShuttingDown),
        }
    }

    fn attachment(
        &mut self,
        correlation: PluginPlaybackCorrelation,
        scope: PluginScope,
    ) -> Result<PluginPlaybackAttachment, PluginPlaybackError> {
        let raw = self.next_token;
        self.next_token = self
            .next_token
            .checked_add(1)
            .ok_or(PluginPlaybackError::AttachmentTokenExhausted)?;
        let token = NonZeroU64::new(raw).ok_or(PluginPlaybackError::AttachmentTokenExhausted)?;
        Ok(PluginPlaybackAttachment {
            token: PluginPlaybackAttachmentToken(token),
            correlation,
            scope,
        })
    }

    fn matches_active(&self, attachment: &PluginPlaybackAttachment) -> bool {
        self.active
            .as_ref()
            .is_some_and(|active| same_attachment(active, attachment))
    }

    fn matches_next(&self, attachment: &PluginPlaybackAttachment) -> bool {
        self.next_prewarm
            .as_ref()
            .is_some_and(|next| same_attachment(next, attachment))
    }
}

fn same_attachment(left: &PluginPlaybackAttachment, right: &PluginPlaybackAttachment) -> bool {
    left.token == right.token
        && left.correlation == right.correlation
        && left.scope.same_identity(&right.scope)
}

fn validate_promotion(
    prewarm: &PluginPlaybackAttachment,
    active: &PluginActivePlaybackCorrelation,
) -> Result<(), PluginPlaybackError> {
    ensure_same_session(prewarm.session(), active.session())?;
    if prewarm.item_id() != active.item_id() {
        return Err(PluginPlaybackError::ItemMismatch);
    }
    if prewarm.source_revision() != active.source_revision() {
        return Err(PluginPlaybackError::SourceRevisionMismatch {
            expected: prewarm.source_revision(),
            actual: active.source_revision(),
        });
    }
    Ok(())
}

fn ensure_generation_advanced(
    previous: &PluginPlaybackAttachment,
    next: &PluginActivePlaybackCorrelation,
) -> Result<(), PluginPlaybackError> {
    if previous.session().session_id() != next.session().session_id()
        || previous.session().session_generation() != next.session().session_generation()
    {
        return Ok(());
    }
    let PluginPlaybackCorrelation::Active(previous) = &previous.correlation else {
        return Ok(());
    };
    if next.playback_generation() <= previous.playback_generation() {
        return Err(PluginPlaybackError::PlaybackGenerationNotAdvanced {
            previous: previous.playback_generation(),
            actual: next.playback_generation(),
        });
    }
    Ok(())
}

fn ensure_same_session(
    expected: &PluginSessionCorrelation,
    actual: &PluginSessionCorrelation,
) -> Result<(), PluginPlaybackError> {
    if expected.plan_fingerprint() != actual.plan_fingerprint() {
        return Err(PluginPlaybackError::PlanFingerprintMismatch);
    }
    if expected.session_id() != actual.session_id() {
        return Err(PluginPlaybackError::SessionMismatch);
    }
    if expected.session_generation() != actual.session_generation() {
        return Err(PluginPlaybackError::SessionGenerationMismatch {
            expected: expected.session_generation(),
            actual: actual.session_generation(),
        });
    }
    Ok(())
}

fn validate_correlation_id(
    field: &'static str,
    value: String,
) -> Result<String, PluginPlaybackError> {
    if value.is_empty()
        || value.len() > MAX_PLUGIN_CORRELATION_ID_BYTES
        || value.chars().any(char::is_control)
    {
        return Err(PluginPlaybackError::InvalidCorrelationId {
            field,
            limit: MAX_PLUGIN_CORRELATION_ID_BYTES,
        });
    }
    Ok(value)
}

fn validate_non_zero_correlation(
    field: &'static str,
    value: u64,
) -> Result<u64, PluginPlaybackError> {
    if value == 0 {
        return Err(PluginPlaybackError::ZeroCorrelationValue { field });
    }
    Ok(value)
}

fn is_lowercase_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}
