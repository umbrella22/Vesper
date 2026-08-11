use std::collections::{HashMap, HashSet, VecDeque};
use std::time::{Duration, Instant};

/// Default maximum number of items retained by one sequence session.
pub const DEFAULT_SEQUENCE_MAX_ITEMS: usize = 512;
/// Default maximum number of provider requests retained until acknowledgement.
pub const DEFAULT_SEQUENCE_MAX_PENDING_REQUESTS: usize = 32;
/// Default maximum number of transient events retained for host draining.
pub const DEFAULT_SEQUENCE_MAX_EVENTS: usize = 1_024;

macro_rules! string_id {
    ($name:ident, $description:literal) => {
        #[doc = $description]
        #[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
        pub struct $name(String);

        impl $name {
            /// Creates an identifier after trimming surrounding whitespace.
            pub fn new(value: impl Into<String>) -> Self {
                Self(value.into().trim().to_owned())
            }

            /// Returns the identifier value.
            pub fn as_str(&self) -> &str {
                &self.0
            }

            /// Returns whether the identifier is empty after normalization.
            pub fn is_empty(&self) -> bool {
                self.0.is_empty()
            }
        }
    };
}

string_id!(
    SequenceId,
    "Stable identifier for one playback sequence session."
);
string_id!(
    SequenceItemId,
    "Stable identifier for one queue occurrence."
);
string_id!(
    SequenceSourceReference,
    "Opaque host-registry reference for one resolved source."
);

/// Generation fencing all provider work for a sequence session.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SequenceSessionGeneration(u64);

impl SequenceSessionGeneration {
    /// Creates a generation from its wire representation.
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    /// Returns the encoded generation.
    pub fn get(self) -> u64 {
        self.0
    }
}

/// Monotonic identifier for one provider request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SequenceRequestId(u64);

impl SequenceRequestId {
    /// Creates a request identifier from its wire representation.
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    /// Returns the encoded request identifier.
    pub fn get(self) -> u64 {
        self.0
    }
}

/// Monotonic identifier for one source-resolution attempt.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SequenceResolutionAttemptId(u64);

impl SequenceResolutionAttemptId {
    /// Creates an attempt identifier from its wire representation.
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    /// Returns the encoded attempt identifier.
    pub fn get(self) -> u64 {
        self.0
    }
}

/// Provider source version accepted for one item.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SequenceSourceRevision(u64);

impl SequenceSourceRevision {
    /// Initial revision used by unresolved items.
    pub const UNRESOLVED: Self = Self(0);

    /// Creates a source revision.
    pub fn new(value: u64) -> Self {
        Self(value)
    }

    /// Returns the encoded revision.
    pub fn get(self) -> u64 {
        self.0
    }
}

/// Host activation epoch used to reject callbacks from an older source.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SequenceActivationEpoch(u64);

impl SequenceActivationEpoch {
    /// Creates an activation epoch from its wire representation.
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    /// Returns the encoded activation epoch.
    pub fn get(self) -> u64 {
        self.0
    }
}

/// Stable provider-scoped content identity.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SequenceContentIdentity {
    /// Reverse-DNS or otherwise collision-resistant provider namespace.
    pub provider_namespace: String,
    /// Provider-defined opaque content value.
    pub value: String,
}

impl SequenceContentIdentity {
    /// Creates a provider-scoped content identity.
    pub fn new(provider_namespace: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            provider_namespace: provider_namespace.into().trim().to_owned(),
            value: value.into().trim().to_owned(),
        }
    }
}

/// Stable cache identity that never contains a playback URL or credentials.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SequenceCacheIdentity {
    /// Provider namespace.
    pub provider_namespace: String,
    /// Provider-defined stable content identity.
    pub content_identity: String,
    /// Quality or representation identity.
    pub rendition_identity: String,
    /// Progressive resource or manifest/segment identity.
    pub resource_identity: String,
    /// Non-sensitive access partition supplied by the provider.
    pub access_partition: String,
    /// Source revision. v1 does not reuse cache entries across revisions.
    pub source_revision: SequenceSourceRevision,
}

impl SequenceCacheIdentity {
    /// Creates a cache identity.
    pub fn new(
        provider_namespace: impl Into<String>,
        content_identity: impl Into<String>,
        rendition_identity: impl Into<String>,
        resource_identity: impl Into<String>,
        access_partition: impl Into<String>,
        source_revision: SequenceSourceRevision,
    ) -> Self {
        Self {
            provider_namespace: provider_namespace.into().trim().to_owned(),
            content_identity: content_identity.into().trim().to_owned(),
            rendition_identity: rendition_identity.into().trim().to_owned(),
            resource_identity: resource_identity.into().trim().to_owned(),
            access_partition: access_partition.into().trim().to_owned(),
            source_revision,
        }
    }

    /// Returns a collision-resistant length-prefixed cache key representation.
    pub fn canonical_key(&self) -> String {
        let mut key = String::from("vesper-sequence-cache:v1");
        for value in [
            self.provider_namespace.as_str(),
            self.content_identity.as_str(),
            self.rendition_identity.as_str(),
            self.resource_identity.as_str(),
            self.access_partition.as_str(),
        ] {
            key.push(':');
            key.push_str(&value.len().to_string());
            key.push(':');
            key.push_str(value);
        }
        key.push(':');
        key.push_str(&self.source_revision.get().to_string());
        key
    }

    fn is_valid(&self) -> bool {
        !self.provider_namespace.is_empty()
            && !self.content_identity.is_empty()
            && !self.rendition_identity.is_empty()
            && !self.resource_identity.is_empty()
            && !self.access_partition.is_empty()
    }
}

/// Media timeline semantics attached to one sequence item.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SequenceMediaKind {
    /// Finite seekable media.
    Vod,
    /// Live media without a DVR window.
    Live,
    /// Live media with an explicit DVR timeline.
    LiveDvr,
}

/// Sequence refill behavior.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SequenceMode {
    /// All items are provided explicitly.
    Finite,
    /// Items may be requested at either queue boundary.
    Replenishable,
}

/// Direction used by navigation and provider refill requests.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SequenceDirection {
    /// Request or navigate toward older items.
    Previous,
    /// Request or navigate toward newer items.
    Next,
}

/// Reason a resolved source must be refreshed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SequenceSourceResolutionReason {
    /// The item has never had a source.
    Initial,
    /// The accepted source reached its wall-clock expiry threshold.
    Expired,
    /// The host classified a playback or warmup failure as source expiry.
    HostRejected,
    /// The provider or host explicitly requested a refresh.
    Refresh,
}

/// Source state for one item. Playback URLs and headers remain in the host.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SequenceSourceState {
    /// No source has been accepted.
    Unresolved,
    /// A provider request is pending.
    Resolving {
        /// Request identifier.
        request_id: SequenceRequestId,
        /// Resolution-attempt identifier.
        attempt_id: SequenceResolutionAttemptId,
        /// Revision current when the request was emitted.
        expected_revision: SequenceSourceRevision,
    },
    /// A safe host source reference has been accepted.
    Resolved {
        /// Opaque host registry key.
        source_reference: SequenceSourceReference,
        /// Accepted source revision.
        revision: SequenceSourceRevision,
        /// Stable cache identity.
        cache_identity: SequenceCacheIdentity,
        /// Optional absolute expiry timestamp.
        expires_at_epoch_ms: Option<u64>,
    },
    /// The previous source expired and requires resolution.
    Expired {
        /// Last accepted revision.
        revision: SequenceSourceRevision,
    },
    /// Source resolution failed.
    Failed {
        /// Revision current when resolution failed.
        revision: SequenceSourceRevision,
        /// Stable non-sensitive failure code.
        reason_code: String,
    },
}

impl SequenceSourceState {
    /// Returns the latest known source revision.
    pub fn revision(&self) -> SequenceSourceRevision {
        match self {
            Self::Unresolved => SequenceSourceRevision::UNRESOLVED,
            Self::Resolving {
                expected_revision, ..
            } => *expected_revision,
            Self::Resolved { revision, .. }
            | Self::Expired { revision }
            | Self::Failed { revision, .. } => *revision,
        }
    }
}

/// Cost hints for sequence-driven preload admission.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SequencePreloadProfile {
    /// Expected in-memory bytes.
    pub expected_memory_bytes: u64,
    /// Expected disk bytes.
    pub expected_disk_bytes: u64,
    /// Optional monotonic task TTL.
    pub ttl: Option<Duration>,
    /// Optional warmup deadline.
    pub warmup_window: Option<Duration>,
}

/// One queue occurrence in a playback sequence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SequenceItem {
    /// Queue-occurrence identifier.
    pub item_id: SequenceItemId,
    /// Stable provider-scoped content identity.
    pub content_identity: SequenceContentIdentity,
    /// Timeline semantics.
    pub media_kind: SequenceMediaKind,
    /// Current source state.
    pub source_state: SequenceSourceState,
    /// Optional opaque application metadata reference.
    pub provider_metadata_ref: Option<String>,
    /// Preload cost hints.
    pub preload_profile: SequencePreloadProfile,
}

impl SequenceItem {
    /// Creates an unresolved sequence item.
    pub fn unresolved(
        item_id: impl Into<String>,
        content_identity: SequenceContentIdentity,
        media_kind: SequenceMediaKind,
    ) -> Self {
        Self {
            item_id: SequenceItemId::new(item_id),
            content_identity,
            media_kind,
            source_state: SequenceSourceState::Unresolved,
            provider_metadata_ref: None,
            preload_profile: SequencePreloadProfile::default(),
        }
    }

    /// Creates a sequence item with an already resolved host source reference.
    pub fn resolved(
        item_id: impl Into<String>,
        content_identity: SequenceContentIdentity,
        media_kind: SequenceMediaKind,
        source_reference: impl Into<String>,
        cache_identity: SequenceCacheIdentity,
        expires_at_epoch_ms: Option<u64>,
    ) -> Self {
        let revision = cache_identity.source_revision;
        Self {
            item_id: SequenceItemId::new(item_id),
            content_identity,
            media_kind,
            source_state: SequenceSourceState::Resolved {
                source_reference: SequenceSourceReference::new(source_reference),
                revision,
                cache_identity,
                expires_at_epoch_ms,
            },
            provider_metadata_ref: None,
            preload_profile: SequencePreloadProfile::default(),
        }
    }
}

/// Bounds and behavior for one sequence coordinator.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SequenceConfig {
    /// Finite or provider-replenished behavior.
    pub mode: SequenceMode,
    /// Maximum number of prior items retained for session history.
    pub history_limit: usize,
    /// Number of forward items considered for preload.
    pub forward_window: usize,
    /// Remaining forward items that trigger refill.
    pub refill_threshold: usize,
    /// Maximum number of retained queue items.
    pub max_items: usize,
    /// Maximum number of outstanding provider requests.
    pub max_pending_requests: usize,
    /// Maximum number of transient events retained for draining.
    pub max_events: usize,
    /// Monotonic request timeout.
    pub request_timeout: Duration,
    /// Wall-clock lead applied before accepting source expiry.
    pub source_expiry_lead: Duration,
}

impl Default for SequenceConfig {
    fn default() -> Self {
        Self {
            mode: SequenceMode::Finite,
            history_limit: 16,
            forward_window: 1,
            refill_threshold: 1,
            max_items: DEFAULT_SEQUENCE_MAX_ITEMS,
            max_pending_requests: DEFAULT_SEQUENCE_MAX_PENDING_REQUESTS,
            max_events: DEFAULT_SEQUENCE_MAX_EVENTS,
            request_timeout: Duration::from_secs(15),
            source_expiry_lead: Duration::from_secs(15),
        }
    }
}

/// Snapshot of both wall and monotonic clocks at one transition boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SequenceClockSnapshot {
    /// Unix epoch milliseconds used only for signed-source expiry.
    pub wall_epoch_ms: u64,
    /// Monotonic time used for timeout and TTL behavior.
    pub monotonic: Instant,
}

/// Refill request retained until acknowledgement, timeout, or cancellation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SequenceItemsRequest {
    /// Sequence identifier.
    pub sequence_id: SequenceId,
    /// Session generation.
    pub session_generation: SequenceSessionGeneration,
    /// Request identifier.
    pub request_id: SequenceRequestId,
    /// Requested direction.
    pub direction: SequenceDirection,
    /// Current boundary anchor.
    pub anchor_item_id: Option<SequenceItemId>,
    /// Maximum number of items accepted in the response.
    pub max_count: usize,
    /// Monotonic deadline for this request.
    pub deadline: Instant,
}

/// Source-resolution request retained until acknowledgement, timeout, or cancellation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SequenceSourceRequest {
    /// Sequence identifier.
    pub sequence_id: SequenceId,
    /// Session generation.
    pub session_generation: SequenceSessionGeneration,
    /// Request identifier.
    pub request_id: SequenceRequestId,
    /// Resolution-attempt identifier.
    pub attempt_id: SequenceResolutionAttemptId,
    /// Item requiring a source.
    pub item_id: SequenceItemId,
    /// Revision current when the request was emitted.
    pub expected_revision: SequenceSourceRevision,
    /// Reason for resolution.
    pub reason: SequenceSourceResolutionReason,
    /// Monotonic deadline for this request.
    pub deadline: Instant,
}

/// Provider request payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SequenceRequestKind {
    /// Queue refill request.
    Items(SequenceItemsRequest),
    /// Source-resolution request.
    Source(SequenceSourceRequest),
}

/// Delivery state for a pending provider request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SequenceRequestDeliveryState {
    /// The request is retained but no notification has been drained.
    Pending,
    /// At least one notification has been drained by the host.
    Delivered,
}

/// Provider request retained by the authoritative sequence snapshot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SequencePendingRequest {
    /// Request payload.
    pub kind: SequenceRequestKind,
    /// Monotonic creation time.
    pub created_at: Instant,
    /// Monotonic deadline.
    pub deadline: Instant,
    /// Notification delivery state.
    pub delivery_state: SequenceRequestDeliveryState,
    /// Number of request notifications drained by hosts.
    pub delivery_count: u32,
}

impl SequencePendingRequest {
    /// Returns the request identifier.
    pub fn request_id(&self) -> SequenceRequestId {
        match &self.kind {
            SequenceRequestKind::Items(request) => request.request_id,
            SequenceRequestKind::Source(request) => request.request_id,
        }
    }
}

/// One item in a sequence snapshot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SequenceItemSnapshot {
    /// Item value.
    pub item: SequenceItem,
    /// Queue index.
    pub index: usize,
    /// Whether this item is active.
    pub is_active: bool,
}

/// Authoritative sequence state used for host resynchronization.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SequenceSnapshot {
    /// Sequence identifier.
    pub sequence_id: SequenceId,
    /// Session generation.
    pub session_generation: SequenceSessionGeneration,
    /// Current activation epoch.
    pub activation_epoch: SequenceActivationEpoch,
    /// Queue items in order.
    pub items: Vec<SequenceItemSnapshot>,
    /// Active item identifier.
    pub active_item_id: Option<SequenceItemId>,
    /// Outstanding requests, including requests whose notifications were dropped.
    pub pending_requests: Vec<SequencePendingRequest>,
    /// Recent terminal request failures retained for snapshot resynchronization.
    pub request_failures: Vec<SequenceRequestFailure>,
    /// Whether the provider reported the previous boundary complete.
    pub previous_end_reached: bool,
    /// Whether the provider reported the next boundary complete.
    pub next_end_reached: bool,
    /// Number of transient events dropped at capacity.
    pub dropped_events: u64,
    /// Bounded per-task warmup lifecycle ledger.
    pub warmup_tasks: Vec<SequenceWarmupTaskSnapshot>,
    /// Aggregate host warmup statistics.
    pub warmup_stats: SequenceWarmupStats,
}

/// Reason an item became active.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SequenceActivationReason {
    /// Initial or replacement activation.
    Replace,
    /// Direct item activation.
    SetActive,
    /// Manual next navigation.
    Next,
    /// Manual previous navigation.
    Previous,
    /// Queue removal selected a replacement item.
    Removal,
}

/// Result of a sequence navigation operation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SequenceNavigationOutcome {
    /// A resolved or unresolved queue item became active.
    Activated {
        /// Activated item.
        item_id: SequenceItemId,
        /// Host callback-fencing epoch.
        activation_epoch: SequenceActivationEpoch,
    },
    /// Navigation reached a finite or provider-confirmed boundary.
    ReachedEnd,
    /// Navigation requested provider items and retained the current item.
    AwaitingItems(SequenceRequestId),
    /// The queue is empty.
    Empty,
}

/// Response to an items request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SequenceItemsResponse {
    /// Session generation echoed from the request.
    pub session_generation: SequenceSessionGeneration,
    /// Request identifier echoed from the request.
    pub request_id: SequenceRequestId,
    /// Boundary anchor echoed from the request.
    pub anchor_item_id: Option<SequenceItemId>,
    /// Returned items.
    pub items: Vec<SequenceItem>,
    /// Whether the provider reached the requested boundary.
    pub end_reached: bool,
}

/// Response to a source-resolution request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SequenceResolvedSource {
    /// Session generation echoed from the request.
    pub session_generation: SequenceSessionGeneration,
    /// Request identifier echoed from the request.
    pub request_id: SequenceRequestId,
    /// Resolution-attempt identifier echoed from the request.
    pub attempt_id: SequenceResolutionAttemptId,
    /// Item receiving the source.
    pub item_id: SequenceItemId,
    /// Revision current when resolution started.
    pub expected_revision: SequenceSourceRevision,
    /// New provider source revision.
    pub source_revision: SequenceSourceRevision,
    /// Opaque host source-registry reference.
    pub source_reference: SequenceSourceReference,
    /// Stable cache identity.
    pub cache_identity: SequenceCacheIdentity,
    /// Optional absolute expiry timestamp.
    pub expires_at_epoch_ms: Option<u64>,
}

/// Preload priority relative to the active sequence item.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SequencePreloadPriority {
    /// Active item.
    Current,
    /// Next item in the forward window.
    Next,
    /// Previous retained item.
    Previous,
}

/// Protocol-level goal requested from a host warmup executor.
///
/// v1 deliberately exposes one bounded goal. Admission cost hints remain
/// separate from the physical byte range selected by the host.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SequenceWarmupGoal {
    /// Read a bounded prefix of a progressive resource.
    ProgressiveRange,
}

impl SequenceWarmupGoal {
    /// Returns the stable wire spelling.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ProgressiveRange => "progressiveRange",
        }
    }
}

/// Bounded host warmup lifecycle state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SequenceWarmupStatus {
    /// The host accepted the intent and started physical work.
    Started,
    /// The requested goal completed.
    Completed,
    /// The host canceled the goal before completion.
    Cancelled,
    /// The host failed the goal.
    Failed,
    /// The protocol or source is unsupported by this host.
    Unsupported,
}

/// Stable task token for one item/revision/warmup-goal identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SequenceWarmupTaskId(u64);

impl SequenceWarmupTaskId {
    /// Creates a task token from its wire representation.
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    /// Returns the encoded task token.
    pub const fn get(self) -> u64 {
        self.0
    }
}

/// One host-reported warmup lifecycle update.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SequenceWarmupReport {
    /// Session generation echoed from the preload intent.
    pub session_generation: SequenceSessionGeneration,
    /// Stable task token echoed from the preload intent.
    pub task_id: SequenceWarmupTaskId,
    /// Queue occurrence being warmed.
    pub item_id: SequenceItemId,
    /// Source revision being warmed.
    pub source_revision: SequenceSourceRevision,
    /// Protocol goal echoed by the host.
    pub warmup_goal: SequenceWarmupGoal,
    /// Lifecycle state.
    pub status: SequenceWarmupStatus,
    /// Physical bytes requested by the goal.
    pub expected_bytes: u64,
    /// Physical bytes actually read or served from cache.
    pub actual_bytes: u64,
    /// Whether the host served the goal entirely from cache.
    pub cache_hit: Option<bool>,
    /// Bounded physical cache inventory after the operation.
    pub cache_entries: u32,
    /// Physical cache bytes after the operation.
    pub cache_bytes: u64,
    /// Entries evicted by the operation.
    pub evicted_entries: u64,
    /// Stable non-sensitive failure code, when applicable.
    pub reason_code: Option<String>,
}

/// Last accepted warmup state retained for snapshot resynchronization.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SequenceWarmupTaskSnapshot {
    /// Stable task token.
    pub task_id: SequenceWarmupTaskId,
    /// Queue occurrence.
    pub item_id: SequenceItemId,
    /// Source revision.
    pub source_revision: SequenceSourceRevision,
    /// Warmup goal.
    pub warmup_goal: SequenceWarmupGoal,
    /// Last lifecycle state.
    pub status: SequenceWarmupStatus,
    /// Expected physical bytes.
    pub expected_bytes: u64,
    /// Actual physical bytes.
    pub actual_bytes: u64,
    /// Cache result.
    pub cache_hit: Option<bool>,
    /// Inventory entry count.
    pub cache_entries: u32,
    /// Inventory byte count.
    pub cache_bytes: u64,
    /// Entries evicted by the operation.
    pub evicted_entries: u64,
    /// Stable failure code.
    pub reason_code: Option<String>,
}

/// Bounded aggregate host warmup statistics.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SequenceWarmupStats {
    /// Number of accepted start reports.
    pub started: u64,
    /// Number of completed goals.
    pub completed: u64,
    /// Number of canceled goals.
    pub cancelled: u64,
    /// Number of failed goals.
    pub failed: u64,
    /// Number of unsupported goals.
    pub unsupported: u64,
    /// Number of cache hits.
    pub cache_hits: u64,
    /// Number of cache misses.
    pub cache_misses: u64,
    /// Sum of expected physical bytes.
    pub expected_bytes: u64,
    /// Sum of actual physical bytes.
    pub actual_bytes: u64,
    /// Sum of evicted entries.
    pub evicted_entries: u64,
}

/// Safe preload intent. The host resolves `source_reference` to a real source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SequencePreloadIntent {
    /// Session generation that owns this host work.
    pub session_generation: SequenceSessionGeneration,
    /// Item to warm.
    pub item_id: SequenceItemId,
    /// Opaque host source-registry reference.
    pub source_reference: SequenceSourceReference,
    /// Source revision.
    pub source_revision: SequenceSourceRevision,
    /// Stable lifecycle token for this item/revision/goal.
    pub warmup_task_id: SequenceWarmupTaskId,
    /// Stable cache identity.
    pub cache_identity: SequenceCacheIdentity,
    /// Relative priority.
    pub priority: SequencePreloadPriority,
    /// Physical warmup goal. This is not inferred from budget cost hints.
    pub warmup_goal: SequenceWarmupGoal,
    /// Cost hints.
    pub profile: SequencePreloadProfile,
}

/// Event emitted by a sequence coordinator.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SequenceEvent {
    /// Monotonic event sequence within the session object.
    pub event_sequence: u64,
    /// Session generation active when the event was emitted.
    pub session_generation: SequenceSessionGeneration,
    /// Event payload.
    pub kind: SequenceEventKind,
}

/// Sequence event payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SequenceEventKind {
    /// The authoritative snapshot changed.
    SnapshotChanged,
    /// An item became active.
    ActiveItemChanged {
        /// Active item.
        item_id: SequenceItemId,
        /// Activation reason.
        reason: SequenceActivationReason,
        /// Host callback-fencing epoch.
        activation_epoch: SequenceActivationEpoch,
    },
    /// Provider work is pending. The same request may be re-emitted.
    Request(SequenceRequestKind),
    /// A provider request timed out.
    RequestTimedOut(SequenceRequestId),
    /// A provider explicitly rejected a request.
    RequestFailed(SequenceRequestFailure),
    /// A provider request was cancelled.
    RequestCancelled(SequenceRequestId),
    /// A source was accepted for an item.
    SourceAccepted {
        /// Item whose source changed.
        item_id: SequenceItemId,
        /// Accepted revision.
        source_revision: SequenceSourceRevision,
    },
    /// A source expired and requires refresh.
    SourceExpired {
        /// Item whose source expired.
        item_id: SequenceItemId,
        /// Expired revision.
        source_revision: SequenceSourceRevision,
    },
}

/// Stable error code for sequence operations and bridge mapping.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SequenceErrorCode {
    /// A required value was empty or inconsistent.
    InvalidArgument,
    /// A queue item identifier was duplicated.
    DuplicateItemId,
    /// A configured or runtime capacity was exceeded.
    CapacityExceeded,
    /// An item was not found.
    ItemNotFound,
    /// A provider response used an old session generation.
    StaleGeneration,
    /// A provider request was unknown or already terminal.
    UnknownRequest,
    /// A response did not match the pending request kind or direction.
    RequestMismatch,
    /// A source response used an old attempt or revision.
    StaleSource,
    /// A native player callback belongs to an older activation.
    StaleActivation,
    /// The requested transition is invalid for current state.
    InvalidState,
}

/// Sequence operation failure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SequenceError {
    /// Stable error code.
    pub code: SequenceErrorCode,
    /// Non-sensitive diagnostic message.
    pub message: String,
}

impl SequenceError {
    fn new(code: SequenceErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}

/// Result returned by sequence operations.
pub type SequenceResult<T> = Result<T, SequenceError>;

/// Bounded terminal failure retained in the authoritative snapshot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SequenceRequestFailure {
    /// Request identifier.
    pub request_id: SequenceRequestId,
    /// Stable non-sensitive failure code.
    pub reason_code: String,
    /// Whether the request ended because its monotonic deadline elapsed.
    pub timed_out: bool,
}

/// Coordinates queue, provider requests, source revisions, and preload intent.
#[derive(Debug)]
pub struct SequenceCoordinator {
    sequence_id: SequenceId,
    config: SequenceConfig,
    session_generation: SequenceSessionGeneration,
    activation_epoch: SequenceActivationEpoch,
    next_request_id: u64,
    next_attempt_id: u64,
    next_event_sequence: u64,
    items: Vec<SequenceItem>,
    active_item_id: Option<SequenceItemId>,
    pending_requests: HashMap<SequenceRequestId, SequencePendingRequest>,
    pending_order: VecDeque<SequenceRequestId>,
    request_failures: VecDeque<SequenceRequestFailure>,
    acknowledged_requests: VecDeque<SequenceRequestId>,
    previous_end_reached: bool,
    next_end_reached: bool,
    events: VecDeque<SequenceEvent>,
    dropped_events: u64,
    max_wall_epoch_ms_seen: u64,
    warmup_tasks: HashMap<SequenceWarmupTaskId, SequenceWarmupTaskSnapshot>,
    warmup_task_order: VecDeque<SequenceWarmupTaskId>,
    warmup_stats: SequenceWarmupStats,
}

impl SequenceCoordinator {
    /// Creates an empty sequence coordinator.
    pub fn new(sequence_id: impl Into<String>, config: SequenceConfig) -> SequenceResult<Self> {
        let sequence_id = SequenceId::new(sequence_id);
        if sequence_id.is_empty() {
            return Err(SequenceError::new(
                SequenceErrorCode::InvalidArgument,
                "sequence id must not be empty",
            ));
        }
        if config.max_items == 0
            || config.max_pending_requests == 0
            || config.max_events == 0
            || config.request_timeout.is_zero()
        {
            return Err(SequenceError::new(
                SequenceErrorCode::InvalidArgument,
                "sequence capacities and request timeout must be non-zero",
            ));
        }
        if config.history_limit >= config.max_items || config.forward_window >= config.max_items {
            return Err(SequenceError::new(
                SequenceErrorCode::InvalidArgument,
                "history and forward windows must be smaller than max_items",
            ));
        }

        Ok(Self {
            sequence_id,
            config,
            session_generation: SequenceSessionGeneration(1),
            activation_epoch: SequenceActivationEpoch(0),
            next_request_id: 1,
            next_attempt_id: 1,
            next_event_sequence: 1,
            items: Vec::new(),
            active_item_id: None,
            pending_requests: HashMap::new(),
            pending_order: VecDeque::new(),
            request_failures: VecDeque::new(),
            acknowledged_requests: VecDeque::new(),
            previous_end_reached: false,
            next_end_reached: false,
            events: VecDeque::new(),
            dropped_events: 0,
            max_wall_epoch_ms_seen: 0,
            warmup_tasks: HashMap::new(),
            warmup_task_order: VecDeque::new(),
            warmup_stats: SequenceWarmupStats::default(),
        })
    }

    /// Returns the active session generation.
    pub fn session_generation(&self) -> SequenceSessionGeneration {
        self.session_generation
    }

    /// Returns the active host callback-fencing epoch.
    pub fn activation_epoch(&self) -> SequenceActivationEpoch {
        self.activation_epoch
    }

    /// Returns the current authoritative snapshot.
    pub fn snapshot(&self) -> SequenceSnapshot {
        let mut pending_requests = self
            .pending_order
            .iter()
            .filter_map(|request_id| self.pending_requests.get(request_id).cloned())
            .collect::<Vec<_>>();
        pending_requests.sort_by_key(SequencePendingRequest::request_id);

        SequenceSnapshot {
            sequence_id: self.sequence_id.clone(),
            session_generation: self.session_generation,
            activation_epoch: self.activation_epoch,
            items: self
                .items
                .iter()
                .enumerate()
                .map(|(index, item)| SequenceItemSnapshot {
                    item: item.clone(),
                    index,
                    is_active: self.active_item_id.as_ref() == Some(&item.item_id),
                })
                .collect(),
            active_item_id: self.active_item_id.clone(),
            pending_requests,
            request_failures: self.request_failures.iter().cloned().collect(),
            previous_end_reached: self.previous_end_reached,
            next_end_reached: self.next_end_reached,
            dropped_events: self.dropped_events,
            warmup_tasks: self
                .warmup_task_order
                .iter()
                .filter_map(|task_id| self.warmup_tasks.get(task_id).cloned())
                .collect(),
            warmup_stats: self.warmup_stats.clone(),
        }
    }

    /// Drains transient events and marks request notifications delivered.
    pub fn drain_events(&mut self) -> Vec<SequenceEvent> {
        self.drain_events_bounded(self.config.max_events)
    }

    /// Drains at most `max_count` transient events.
    pub fn drain_events_bounded(&mut self, max_count: usize) -> Vec<SequenceEvent> {
        let count = max_count.min(self.config.max_events).min(self.events.len());
        let events = self.events.drain(..count).collect::<Vec<_>>();
        for event in &events {
            let SequenceEventKind::Request(kind) = &event.kind else {
                continue;
            };
            let request_id = request_id_for_kind(kind);
            if let Some(pending) = self.pending_requests.get_mut(&request_id) {
                pending.delivery_state = SequenceRequestDeliveryState::Delivered;
                pending.delivery_count = pending.delivery_count.saturating_add(1);
            }
        }
        events
    }

    /// Re-emits every pending provider request for EventChannel resynchronization.
    pub fn resync_pending_requests(&mut self) {
        let requests = self
            .pending_order
            .iter()
            .filter_map(|request_id| self.pending_requests.get(request_id))
            .map(|request| request.kind.clone())
            .collect::<Vec<_>>();
        for request in requests {
            self.record_event(SequenceEventKind::Request(request));
        }
    }

    /// Replaces the queue and starts a new provider-response generation.
    pub fn replace(
        &mut self,
        items: Vec<SequenceItem>,
        active_item_id: Option<SequenceItemId>,
        now: SequenceClockSnapshot,
    ) -> SequenceResult<()> {
        self.validate_items(&items)?;
        if items.len() > self.config.max_items {
            return Err(self.capacity_error("replacement queue exceeds max_items"));
        }
        if let Some(active_item_id) = &active_item_id
            && !items.iter().any(|item| item.item_id == *active_item_id)
        {
            return Err(SequenceError::new(
                SequenceErrorCode::ItemNotFound,
                "replacement active item is not in the queue",
            ));
        }

        self.pending_requests.clear();
        self.pending_order.clear();
        self.request_failures.clear();
        self.acknowledged_requests.clear();
        self.events.clear();
        self.dropped_events = 0;
        self.warmup_tasks.clear();
        self.warmup_task_order.clear();
        self.warmup_stats = SequenceWarmupStats::default();
        self.session_generation =
            SequenceSessionGeneration(next_non_zero(self.session_generation.get()));
        self.max_wall_epoch_ms_seen = now.wall_epoch_ms;
        self.items = items;
        self.active_item_id =
            active_item_id.or_else(|| self.items.first().map(|item| item.item_id.clone()));
        self.previous_end_reached = self.config.mode == SequenceMode::Finite;
        self.next_end_reached = self.config.mode == SequenceMode::Finite;
        self.record_event(SequenceEventKind::SnapshotChanged);
        if self.active_item_id.is_some() {
            self.bump_activation(SequenceActivationReason::Replace);
            self.ensure_active_source_request(SequenceSourceResolutionReason::Initial, now)?;
            self.maybe_request_next(now)?;
        }
        Ok(())
    }

    /// Appends a provider response and acknowledges the matching request.
    pub fn append(
        &mut self,
        response: SequenceItemsResponse,
        now: SequenceClockSnapshot,
    ) -> SequenceResult<usize> {
        self.apply_items_response(SequenceDirection::Next, response, now)
    }

    /// Prepends a provider response and acknowledges the matching request.
    pub fn prepend(
        &mut self,
        response: SequenceItemsResponse,
        now: SequenceClockSnapshot,
    ) -> SequenceResult<usize> {
        self.apply_items_response(SequenceDirection::Previous, response, now)
    }

    /// Removes an item and selects a deterministic replacement when necessary.
    pub fn remove(
        &mut self,
        item_id: &SequenceItemId,
        now: SequenceClockSnapshot,
    ) -> SequenceResult<bool> {
        let Some(index) = self.item_index(item_id) else {
            return Ok(false);
        };
        let was_active = self.active_item_id.as_ref() == Some(item_id);
        self.items.remove(index);
        self.cancel_source_requests_for(item_id);
        self.prune_warmup_tasks();
        if was_active {
            self.active_item_id = self
                .items
                .get(index)
                .or_else(|| {
                    index
                        .checked_sub(1)
                        .and_then(|previous| self.items.get(previous))
                })
                .map(|item| item.item_id.clone());
            if self.active_item_id.is_some() {
                self.bump_activation(SequenceActivationReason::Removal);
                self.ensure_active_source_request(SequenceSourceResolutionReason::Initial, now)?;
            }
        }
        self.record_event(SequenceEventKind::SnapshotChanged);
        Ok(true)
    }

    /// Activates one existing queue item without changing the session generation.
    pub fn set_active(
        &mut self,
        item_id: &SequenceItemId,
        now: SequenceClockSnapshot,
    ) -> SequenceResult<SequenceNavigationOutcome> {
        self.activate(item_id, SequenceActivationReason::SetActive, now)
    }

    /// Navigates to the next item or requests provider refill at the boundary.
    pub fn next(
        &mut self,
        now: SequenceClockSnapshot,
    ) -> SequenceResult<SequenceNavigationOutcome> {
        let Some(active_index) = self.active_index() else {
            return Ok(SequenceNavigationOutcome::Empty);
        };
        if let Some(next_item_id) = self
            .items
            .get(active_index + 1)
            .map(|item| item.item_id.clone())
        {
            let outcome = self.activate(&next_item_id, SequenceActivationReason::Next, now)?;
            self.trim_history();
            self.maybe_request_next(now)?;
            return Ok(outcome);
        }
        self.request_boundary(SequenceDirection::Next, now)
    }

    /// Navigates to the previous item or requests provider refill at the boundary.
    pub fn previous(
        &mut self,
        now: SequenceClockSnapshot,
    ) -> SequenceResult<SequenceNavigationOutcome> {
        let Some(active_index) = self.active_index() else {
            return Ok(SequenceNavigationOutcome::Empty);
        };
        if let Some(previous_index) = active_index.checked_sub(1) {
            let previous_item_id = self.items[previous_index].item_id.clone();
            return self.activate(&previous_item_id, SequenceActivationReason::Previous, now);
        }
        self.request_boundary(SequenceDirection::Previous, now)
    }

    /// Accepts a resolved source after validating request, attempt, and revision fences.
    pub fn submit_resolved_source(&mut self, source: SequenceResolvedSource) -> SequenceResult<()> {
        self.validate_generation(source.session_generation)?;
        if self.is_acknowledged(source.request_id) {
            return Ok(());
        }
        validate_source_reference(&source.source_reference)?;
        if !source.cache_identity.is_valid()
            || source.cache_identity.source_revision != source.source_revision
        {
            return Err(SequenceError::new(
                SequenceErrorCode::InvalidArgument,
                "cache identity must be complete and match source revision",
            ));
        }

        let pending = self
            .pending_requests
            .get(&source.request_id)
            .ok_or_else(|| self.unknown_request_error())?;
        let SequenceRequestKind::Source(request) = &pending.kind else {
            return Err(SequenceError::new(
                SequenceErrorCode::RequestMismatch,
                "request does not resolve a source",
            ));
        };
        if request.item_id != source.item_id
            || request.attempt_id != source.attempt_id
            || request.expected_revision != source.expected_revision
        {
            return Err(SequenceError::new(
                SequenceErrorCode::StaleSource,
                "source response does not match the pending resolution attempt",
            ));
        }
        if source.source_revision <= source.expected_revision {
            return Err(SequenceError::new(
                SequenceErrorCode::StaleSource,
                "source revision must advance",
            ));
        }
        let index = self.item_index(&source.item_id).ok_or_else(|| {
            SequenceError::new(SequenceErrorCode::ItemNotFound, "source item was removed")
        })?;
        if self.items[index].source_state.revision() != source.expected_revision {
            return Err(SequenceError::new(
                SequenceErrorCode::StaleSource,
                "item source revision changed while resolution was pending",
            ));
        }

        self.items[index].source_state = SequenceSourceState::Resolved {
            source_reference: source.source_reference,
            revision: source.source_revision,
            cache_identity: source.cache_identity,
            expires_at_epoch_ms: source.expires_at_epoch_ms,
        };
        self.prune_warmup_tasks();
        self.remove_pending_request(source.request_id);
        self.record_acknowledged_request(source.request_id);
        self.record_event(SequenceEventKind::SourceAccepted {
            item_id: source.item_id,
            source_revision: source.source_revision,
        });
        self.record_event(SequenceEventKind::SnapshotChanged);
        Ok(())
    }

    /// Fails one pending provider request without discarding the current item.
    pub fn fail_request(
        &mut self,
        session_generation: SequenceSessionGeneration,
        request_id: SequenceRequestId,
        reason_code: impl Into<String>,
    ) -> SequenceResult<()> {
        self.validate_generation(session_generation)?;
        let reason_code = normalize_reason_code(reason_code)?;
        let request = self
            .remove_pending_request(request_id)
            .ok_or_else(|| self.unknown_request_error())?;
        if let SequenceRequestKind::Source(source_request) = request.kind
            && let Some(index) = self.item_index(&source_request.item_id)
            && matches!(
                self.items[index].source_state,
                SequenceSourceState::Resolving { request_id: current, .. } if current == request_id
            )
        {
            self.items[index].source_state = SequenceSourceState::Failed {
                revision: source_request.expected_revision,
                reason_code: reason_code.clone(),
            };
        }
        let failure = SequenceRequestFailure {
            request_id,
            reason_code,
            timed_out: false,
        };
        self.record_request_failure(failure.clone());
        self.record_event(SequenceEventKind::RequestFailed(failure));
        self.record_event(SequenceEventKind::SnapshotChanged);
        Ok(())
    }

    /// Validates a native player callback against the current activation and source revision.
    pub fn validate_activation_callback(
        &self,
        item_id: &SequenceItemId,
        activation_epoch: SequenceActivationEpoch,
        source_revision: SequenceSourceRevision,
    ) -> SequenceResult<()> {
        if self.active_item_id.as_ref() != Some(item_id)
            || self.activation_epoch != activation_epoch
        {
            return Err(SequenceError::new(
                SequenceErrorCode::StaleActivation,
                "native callback belongs to an older activation",
            ));
        }
        let Some(index) = self.item_index(item_id) else {
            return Err(SequenceError::new(
                SequenceErrorCode::StaleActivation,
                "native callback item is no longer active",
            ));
        };
        if self.items[index].source_state.revision() != source_revision {
            return Err(SequenceError::new(
                SequenceErrorCode::StaleActivation,
                "native callback used an older source revision",
            ));
        }
        Ok(())
    }

    /// Marks one accepted source expired and requests a fresh provider source.
    pub fn mark_source_expired(
        &mut self,
        item_id: &SequenceItemId,
        source_revision: SequenceSourceRevision,
        now: SequenceClockSnapshot,
    ) -> SequenceResult<SequenceRequestId> {
        let index = self.item_index(item_id).ok_or_else(|| {
            SequenceError::new(SequenceErrorCode::ItemNotFound, "source item was not found")
        })?;
        if self.items[index].source_state.revision() != source_revision {
            return Err(SequenceError::new(
                SequenceErrorCode::StaleSource,
                "source expiry used a stale revision",
            ));
        }
        self.items[index].source_state = SequenceSourceState::Expired {
            revision: source_revision,
        };
        self.prune_warmup_tasks();
        self.record_event(SequenceEventKind::SourceExpired {
            item_id: item_id.clone(),
            source_revision,
        });
        let request_id =
            self.request_source(item_id, SequenceSourceResolutionReason::Expired, now)?;
        self.record_event(SequenceEventKind::SnapshotChanged);
        Ok(request_id)
    }

    /// Advances request timeouts and wall-clock source expiry.
    pub fn tick(&mut self, now: SequenceClockSnapshot) -> SequenceResult<()> {
        let mut snapshot_changed = false;
        let timed_out = self
            .pending_order
            .iter()
            .filter_map(|request_id| {
                self.pending_requests
                    .get(request_id)
                    .filter(|request| request.deadline <= now.monotonic)
                    .map(|_| *request_id)
            })
            .collect::<Vec<_>>();
        for request_id in timed_out {
            if let Some(request) = self.remove_pending_request(request_id) {
                if let SequenceRequestKind::Source(source_request) = request.kind
                    && let Some(index) = self.item_index(&source_request.item_id)
                    && matches!(
                        self.items[index].source_state,
                        SequenceSourceState::Resolving { request_id: current, .. } if current == request_id
                    )
                {
                    self.items[index].source_state = SequenceSourceState::Failed {
                        revision: source_request.expected_revision,
                        reason_code: "timeout".to_owned(),
                    };
                }
                self.record_request_failure(SequenceRequestFailure {
                    request_id,
                    reason_code: "timeout".to_owned(),
                    timed_out: true,
                });
                self.record_event(SequenceEventKind::RequestTimedOut(request_id));
                snapshot_changed = true;
            }
        }

        self.max_wall_epoch_ms_seen = self.max_wall_epoch_ms_seen.max(now.wall_epoch_ms);
        let expiry_lead_ms = duration_millis_saturating(self.config.source_expiry_lead);
        let expiry_threshold = self.max_wall_epoch_ms_seen.saturating_add(expiry_lead_ms);
        let expired = self
            .items
            .iter()
            .filter_map(|item| match &item.source_state {
                SequenceSourceState::Resolved {
                    revision,
                    expires_at_epoch_ms: Some(expires_at),
                    ..
                } if *expires_at <= expiry_threshold => Some((item.item_id.clone(), *revision)),
                _ => None,
            })
            .collect::<Vec<_>>();
        for (item_id, revision) in expired {
            let _ = self.mark_source_expired(&item_id, revision, now)?;
            snapshot_changed = true;
        }
        if snapshot_changed {
            self.record_event(SequenceEventKind::SnapshotChanged);
        }
        Ok(())
    }

    /// Returns v1 preload intents for the active item and one next item.
    ///
    /// Previous-item preloading is intentionally excluded from the first
    /// host execution slice. The enum remains available for a later policy
    /// extension without changing the wire model.
    pub fn preload_intents(&self, wall_epoch_ms: u64) -> Vec<SequencePreloadIntent> {
        let Some(active_index) = self.active_index() else {
            return Vec::new();
        };
        let mut intents = Vec::new();
        self.push_preload_intent(
            &mut intents,
            active_index,
            SequencePreloadPriority::Current,
            wall_epoch_ms,
        );
        if self.config.forward_window > 0
            && let Some(next_index) = active_index.checked_add(1)
            && next_index < self.items.len()
        {
            self.push_preload_intent(
                &mut intents,
                next_index,
                SequencePreloadPriority::Next,
                wall_epoch_ms,
            );
        }
        intents
    }

    /// Accepts one bounded host warmup lifecycle report.
    ///
    /// Reports are fenced by item, source revision, goal, and task token. An
    /// identical duplicate is idempotent; a different report for a terminal
    /// task is rejected as stale.
    pub fn report_warmup(&mut self, report: SequenceWarmupReport) -> SequenceResult<()> {
        self.validate_generation(report.session_generation)?;
        if report.warmup_goal != SequenceWarmupGoal::ProgressiveRange
            || report.cache_entries > 4_096
        {
            return Err(SequenceError::new(
                SequenceErrorCode::InvalidArgument,
                "warmup report used an unsupported goal or inventory size",
            ));
        }
        let index = self.item_index(&report.item_id).ok_or_else(|| {
            SequenceError::new(SequenceErrorCode::ItemNotFound, "warmup item was removed")
        })?;
        let SequenceSourceState::Resolved {
            revision,
            cache_identity,
            ..
        } = &self.items[index].source_state
        else {
            return Err(SequenceError::new(
                SequenceErrorCode::StaleSource,
                "warmup report requires a currently resolved source",
            ));
        };
        if *revision != report.source_revision
            || stable_warmup_task_id(
                self.session_generation,
                &report.item_id,
                *revision,
                cache_identity,
                report.warmup_goal,
            ) != report.task_id
        {
            return Err(SequenceError::new(
                SequenceErrorCode::StaleSource,
                "warmup report belongs to an older source revision or task",
            ));
        }
        let reason_code = report.reason_code.map(normalize_reason_code).transpose()?;
        let candidate = SequenceWarmupTaskSnapshot {
            task_id: report.task_id,
            item_id: report.item_id,
            source_revision: report.source_revision,
            warmup_goal: report.warmup_goal,
            status: report.status,
            expected_bytes: report.expected_bytes,
            actual_bytes: report.actual_bytes,
            cache_hit: report.cache_hit,
            cache_entries: report.cache_entries,
            cache_bytes: report.cache_bytes,
            evicted_entries: report.evicted_entries,
            reason_code,
        };
        self.prune_warmup_tasks();
        if let Some(previous) = self.warmup_tasks.get(&candidate.task_id) {
            if previous == &candidate {
                return Ok(());
            }
            if is_terminal_warmup_status(previous.status) {
                return Err(SequenceError::new(
                    SequenceErrorCode::StaleSource,
                    "warmup task already reached a terminal state",
                ));
            }
        }

        let previous_status = self
            .warmup_tasks
            .get(&candidate.task_id)
            .map(|task| task.status);
        if previous_status.is_none() {
            self.warmup_task_order.push_back(candidate.task_id);
        }
        self.update_warmup_stats(previous_status, &candidate);
        self.warmup_tasks.insert(candidate.task_id, candidate);
        self.record_event(SequenceEventKind::SnapshotChanged);
        Ok(())
    }

    fn update_warmup_stats(
        &mut self,
        previous_status: Option<SequenceWarmupStatus>,
        report: &SequenceWarmupTaskSnapshot,
    ) {
        if previous_status == Some(report.status) && is_terminal_warmup_status(report.status) {
            return;
        }
        if previous_status.is_none() && report.status == SequenceWarmupStatus::Started {
            self.warmup_stats.started = self.warmup_stats.started.saturating_add(1);
            return;
        }
        if !is_terminal_warmup_status(report.status) {
            return;
        }
        match report.status {
            SequenceWarmupStatus::Completed => {
                self.warmup_stats.completed = self.warmup_stats.completed.saturating_add(1)
            }
            SequenceWarmupStatus::Cancelled => {
                self.warmup_stats.cancelled = self.warmup_stats.cancelled.saturating_add(1)
            }
            SequenceWarmupStatus::Failed => {
                self.warmup_stats.failed = self.warmup_stats.failed.saturating_add(1)
            }
            SequenceWarmupStatus::Unsupported => {
                self.warmup_stats.unsupported = self.warmup_stats.unsupported.saturating_add(1)
            }
            SequenceWarmupStatus::Started => {}
        }
        if report.cache_hit == Some(true) {
            self.warmup_stats.cache_hits = self.warmup_stats.cache_hits.saturating_add(1);
        } else if report.cache_hit == Some(false) {
            self.warmup_stats.cache_misses = self.warmup_stats.cache_misses.saturating_add(1);
        }
        self.warmup_stats.expected_bytes = self
            .warmup_stats
            .expected_bytes
            .saturating_add(report.expected_bytes);
        self.warmup_stats.actual_bytes = self
            .warmup_stats
            .actual_bytes
            .saturating_add(report.actual_bytes);
        self.warmup_stats.evicted_entries = self
            .warmup_stats
            .evicted_entries
            .saturating_add(report.evicted_entries);
    }

    fn apply_items_response(
        &mut self,
        direction: SequenceDirection,
        response: SequenceItemsResponse,
        now: SequenceClockSnapshot,
    ) -> SequenceResult<usize> {
        self.validate_generation(response.session_generation)?;
        if self.is_acknowledged(response.request_id) {
            return Ok(0);
        }
        let pending = self
            .pending_requests
            .get(&response.request_id)
            .ok_or_else(|| self.unknown_request_error())?;
        let SequenceRequestKind::Items(request) = &pending.kind else {
            return Err(SequenceError::new(
                SequenceErrorCode::RequestMismatch,
                "request does not accept items",
            ));
        };
        if request.direction != direction {
            return Err(SequenceError::new(
                SequenceErrorCode::RequestMismatch,
                "items response direction does not match request",
            ));
        }
        if request.anchor_item_id != response.anchor_item_id {
            return Err(SequenceError::new(
                SequenceErrorCode::RequestMismatch,
                "items response anchor does not match request",
            ));
        }
        if response.items.len() > request.max_count {
            return Err(self.capacity_error("provider returned more items than requested"));
        }
        self.validate_items(&response.items)?;
        let existing = self
            .items
            .iter()
            .map(|item| item.item_id.clone())
            .collect::<HashSet<_>>();
        if response
            .items
            .iter()
            .any(|item| existing.contains(&item.item_id))
        {
            return Err(SequenceError::new(
                SequenceErrorCode::DuplicateItemId,
                "provider response contains an existing item id",
            ));
        }
        if self.items.len().saturating_add(response.items.len()) > self.config.max_items {
            return Err(self.capacity_error("provider response exceeds max_items"));
        }

        let accepted = response.items.len();
        match direction {
            SequenceDirection::Next => {
                self.items.extend(response.items);
                self.next_end_reached = response.end_reached;
            }
            SequenceDirection::Previous => {
                let mut items = response.items;
                items.extend(std::mem::take(&mut self.items));
                self.items = items;
                self.previous_end_reached = response.end_reached;
            }
        }
        self.remove_pending_request(response.request_id);
        self.record_acknowledged_request(response.request_id);
        if self.active_item_id.is_none() {
            self.active_item_id = self.items.first().map(|item| item.item_id.clone());
            if self.active_item_id.is_some() {
                self.bump_activation(SequenceActivationReason::Replace);
                self.ensure_active_source_request(SequenceSourceResolutionReason::Initial, now)?;
            }
        }
        self.record_event(SequenceEventKind::SnapshotChanged);
        Ok(accepted)
    }

    fn activate(
        &mut self,
        item_id: &SequenceItemId,
        reason: SequenceActivationReason,
        now: SequenceClockSnapshot,
    ) -> SequenceResult<SequenceNavigationOutcome> {
        if self.item_index(item_id).is_none() {
            return Err(SequenceError::new(
                SequenceErrorCode::ItemNotFound,
                "active item was not found",
            ));
        }
        self.active_item_id = Some(item_id.clone());
        self.bump_activation(reason);
        self.ensure_active_source_request(SequenceSourceResolutionReason::Initial, now)?;
        self.record_event(SequenceEventKind::SnapshotChanged);
        Ok(SequenceNavigationOutcome::Activated {
            item_id: item_id.clone(),
            activation_epoch: self.activation_epoch,
        })
    }

    fn request_boundary(
        &mut self,
        direction: SequenceDirection,
        now: SequenceClockSnapshot,
    ) -> SequenceResult<SequenceNavigationOutcome> {
        if self.config.mode == SequenceMode::Finite || self.end_reached(direction) {
            return Ok(SequenceNavigationOutcome::ReachedEnd);
        }
        let request_id = self.request_items(direction, self.refill_request_count(), now)?;
        Ok(SequenceNavigationOutcome::AwaitingItems(request_id))
    }

    fn maybe_request_next(&mut self, now: SequenceClockSnapshot) -> SequenceResult<()> {
        if self.config.mode != SequenceMode::Replenishable || self.next_end_reached {
            return Ok(());
        }
        let remaining = self
            .active_index()
            .map(|index| self.items.len().saturating_sub(index + 1))
            .unwrap_or(0);
        if remaining <= self.config.refill_threshold
            && self
                .pending_items_request(SequenceDirection::Next)
                .is_none()
        {
            let _ =
                self.request_items(SequenceDirection::Next, self.refill_request_count(), now)?;
        }
        Ok(())
    }

    fn request_items(
        &mut self,
        direction: SequenceDirection,
        max_count: usize,
        now: SequenceClockSnapshot,
    ) -> SequenceResult<SequenceRequestId> {
        if let Some(request_id) = self.pending_items_request(direction) {
            return Ok(request_id);
        }
        self.ensure_pending_capacity()?;
        let request_id = self.next_request_id()?;
        let deadline = self.request_deadline(now.monotonic)?;
        let anchor_item_id = match direction {
            SequenceDirection::Previous => self.items.first(),
            SequenceDirection::Next => self.items.last(),
        }
        .map(|item| item.item_id.clone());
        let request = SequenceItemsRequest {
            sequence_id: self.sequence_id.clone(),
            session_generation: self.session_generation,
            request_id,
            direction,
            anchor_item_id,
            max_count: max_count.max(1),
            deadline,
        };
        self.insert_pending(SequenceRequestKind::Items(request), now.monotonic, deadline);
        Ok(request_id)
    }

    fn request_source(
        &mut self,
        item_id: &SequenceItemId,
        reason: SequenceSourceResolutionReason,
        now: SequenceClockSnapshot,
    ) -> SequenceResult<SequenceRequestId> {
        if let Some(request_id) = self.pending_source_request(item_id) {
            return Ok(request_id);
        }
        self.ensure_pending_capacity()?;
        let index = self.item_index(item_id).ok_or_else(|| {
            SequenceError::new(SequenceErrorCode::ItemNotFound, "source item was not found")
        })?;
        let request_id = self.next_request_id()?;
        let deadline = self.request_deadline(now.monotonic)?;
        let attempt_id = SequenceResolutionAttemptId(self.next_attempt_id);
        self.next_attempt_id = next_non_zero(self.next_attempt_id);
        let expected_revision = self.items[index].source_state.revision();
        let request = SequenceSourceRequest {
            sequence_id: self.sequence_id.clone(),
            session_generation: self.session_generation,
            request_id,
            attempt_id,
            item_id: item_id.clone(),
            expected_revision,
            reason,
            deadline,
        };
        self.items[index].source_state = SequenceSourceState::Resolving {
            request_id,
            attempt_id,
            expected_revision,
        };
        self.insert_pending(
            SequenceRequestKind::Source(request),
            now.monotonic,
            deadline,
        );
        Ok(request_id)
    }

    fn ensure_active_source_request(
        &mut self,
        reason: SequenceSourceResolutionReason,
        now: SequenceClockSnapshot,
    ) -> SequenceResult<()> {
        let Some(item_id) = self.active_item_id.clone() else {
            return Ok(());
        };
        let Some(index) = self.item_index(&item_id) else {
            return Ok(());
        };
        if matches!(
            self.items[index].source_state,
            SequenceSourceState::Unresolved
                | SequenceSourceState::Expired { .. }
                | SequenceSourceState::Failed { .. }
        ) {
            let _ = self.request_source(&item_id, reason, now)?;
        }
        Ok(())
    }

    fn insert_pending(
        &mut self,
        kind: SequenceRequestKind,
        created_at: Instant,
        deadline: Instant,
    ) {
        let request_id = request_id_for_kind(&kind);
        self.pending_requests.insert(
            request_id,
            SequencePendingRequest {
                kind: kind.clone(),
                created_at,
                deadline,
                delivery_state: SequenceRequestDeliveryState::Pending,
                delivery_count: 0,
            },
        );
        self.pending_order.push_back(request_id);
        self.record_event(SequenceEventKind::Request(kind));
        self.record_event(SequenceEventKind::SnapshotChanged);
    }

    fn request_deadline(&self, now: Instant) -> SequenceResult<Instant> {
        now.checked_add(self.config.request_timeout).ok_or_else(|| {
            SequenceError::new(
                SequenceErrorCode::InvalidArgument,
                "request deadline overflowed monotonic clock",
            )
        })
    }

    fn remove_pending_request(
        &mut self,
        request_id: SequenceRequestId,
    ) -> Option<SequencePendingRequest> {
        self.pending_order
            .retain(|candidate| *candidate != request_id);
        self.pending_requests.remove(&request_id)
    }

    fn cancel_source_requests_for(&mut self, item_id: &SequenceItemId) {
        let request_ids = self
            .pending_order
            .iter()
            .filter_map(|request_id| {
                self.pending_requests
                    .get(request_id)
                    .and_then(|pending| match &pending.kind {
                        SequenceRequestKind::Source(request) if request.item_id == *item_id => {
                            Some(*request_id)
                        }
                        _ => None,
                    })
            })
            .collect::<Vec<_>>();
        for request_id in request_ids {
            let _ = self.remove_pending_request(request_id);
            self.record_event(SequenceEventKind::RequestCancelled(request_id));
        }
    }

    fn validate_items(&self, items: &[SequenceItem]) -> SequenceResult<()> {
        let mut ids = HashSet::with_capacity(items.len());
        for item in items {
            if item.item_id.is_empty()
                || item.content_identity.provider_namespace.is_empty()
                || item.content_identity.value.is_empty()
            {
                return Err(SequenceError::new(
                    SequenceErrorCode::InvalidArgument,
                    "item and content identities must not be empty",
                ));
            }
            if !ids.insert(item.item_id.clone()) {
                return Err(SequenceError::new(
                    SequenceErrorCode::DuplicateItemId,
                    "queue contains a duplicate item id",
                ));
            }
            if let SequenceSourceState::Resolved {
                source_reference,
                revision,
                cache_identity,
                ..
            } = &item.source_state
            {
                validate_source_reference(source_reference)?;
                if !cache_identity.is_valid() || cache_identity.source_revision != *revision {
                    return Err(SequenceError::new(
                        SequenceErrorCode::InvalidArgument,
                        "resolved item cache identity must match source revision",
                    ));
                }
            }
        }
        Ok(())
    }

    fn validate_generation(&self, generation: SequenceSessionGeneration) -> SequenceResult<()> {
        if generation == self.session_generation {
            Ok(())
        } else {
            Err(SequenceError::new(
                SequenceErrorCode::StaleGeneration,
                "provider response used a stale session generation",
            ))
        }
    }

    fn ensure_pending_capacity(&self) -> SequenceResult<()> {
        if self.pending_requests.len() >= self.config.max_pending_requests {
            Err(self.capacity_error("pending request capacity reached"))
        } else {
            Ok(())
        }
    }

    fn record_request_failure(&mut self, failure: SequenceRequestFailure) {
        while self.request_failures.len() >= self.config.max_pending_requests {
            let _ = self.request_failures.pop_front();
        }
        self.request_failures.push_back(failure);
    }

    fn record_acknowledged_request(&mut self, request_id: SequenceRequestId) {
        while self.acknowledged_requests.len() >= self.config.max_pending_requests {
            let _ = self.acknowledged_requests.pop_front();
        }
        self.acknowledged_requests.push_back(request_id);
    }

    fn is_acknowledged(&self, request_id: SequenceRequestId) -> bool {
        self.acknowledged_requests.contains(&request_id)
    }

    fn next_request_id(&mut self) -> SequenceResult<SequenceRequestId> {
        let request_id = SequenceRequestId(self.next_request_id);
        self.next_request_id = next_non_zero(self.next_request_id);
        if self.pending_requests.contains_key(&request_id) {
            return Err(self.capacity_error("request id space is exhausted"));
        }
        Ok(request_id)
    }

    fn pending_items_request(&self, direction: SequenceDirection) -> Option<SequenceRequestId> {
        self.pending_order.iter().find_map(|request_id| {
            self.pending_requests
                .get(request_id)
                .and_then(|pending| match &pending.kind {
                    SequenceRequestKind::Items(request) if request.direction == direction => {
                        Some(*request_id)
                    }
                    _ => None,
                })
        })
    }

    fn pending_source_request(&self, item_id: &SequenceItemId) -> Option<SequenceRequestId> {
        self.pending_order.iter().find_map(|request_id| {
            self.pending_requests
                .get(request_id)
                .and_then(|pending| match &pending.kind {
                    SequenceRequestKind::Source(request) if request.item_id == *item_id => {
                        Some(*request_id)
                    }
                    _ => None,
                })
        })
    }

    fn item_index(&self, item_id: &SequenceItemId) -> Option<usize> {
        self.items.iter().position(|item| item.item_id == *item_id)
    }

    fn active_index(&self) -> Option<usize> {
        self.active_item_id
            .as_ref()
            .and_then(|item_id| self.item_index(item_id))
    }

    fn bump_activation(&mut self, reason: SequenceActivationReason) {
        self.activation_epoch = SequenceActivationEpoch(next_non_zero(self.activation_epoch.get()));
        if let Some(item_id) = self.active_item_id.clone() {
            self.record_event(SequenceEventKind::ActiveItemChanged {
                item_id,
                reason,
                activation_epoch: self.activation_epoch,
            });
        }
    }

    fn trim_history(&mut self) {
        let Some(active_index) = self.active_index() else {
            return;
        };
        let remove_count = active_index.saturating_sub(self.config.history_limit);
        if remove_count == 0 {
            return;
        }
        let removed_ids = self
            .items
            .drain(..remove_count)
            .map(|item| item.item_id)
            .collect::<Vec<_>>();
        for item_id in removed_ids {
            self.cancel_source_requests_for(&item_id);
        }
        self.prune_warmup_tasks();
        self.record_event(SequenceEventKind::SnapshotChanged);
    }

    fn push_preload_intent(
        &self,
        intents: &mut Vec<SequencePreloadIntent>,
        index: usize,
        priority: SequencePreloadPriority,
        wall_epoch_ms: u64,
    ) {
        let item = &self.items[index];
        let SequenceSourceState::Resolved {
            source_reference,
            revision,
            cache_identity,
            expires_at_epoch_ms,
        } = &item.source_state
        else {
            return;
        };
        let expiry_threshold = wall_epoch_ms
            .saturating_add(duration_millis_saturating(self.config.source_expiry_lead));
        if expires_at_epoch_ms.is_some_and(|expires_at| expires_at <= expiry_threshold) {
            return;
        }
        let warmup_task_id = stable_warmup_task_id(
            self.session_generation,
            &item.item_id,
            *revision,
            cache_identity,
            SequenceWarmupGoal::ProgressiveRange,
        );
        if self
            .warmup_tasks
            .get(&warmup_task_id)
            .is_some_and(|task| is_terminal_warmup_status(task.status))
        {
            return;
        }
        intents.push(SequencePreloadIntent {
            session_generation: self.session_generation,
            item_id: item.item_id.clone(),
            source_reference: source_reference.clone(),
            source_revision: *revision,
            warmup_task_id,
            cache_identity: cache_identity.clone(),
            priority,
            warmup_goal: SequenceWarmupGoal::ProgressiveRange,
            profile: item.preload_profile.clone(),
        });
    }

    fn prune_warmup_tasks(&mut self) {
        let valid = self
            .items
            .iter()
            .filter_map(|item| {
                let SequenceSourceState::Resolved {
                    revision,
                    cache_identity,
                    ..
                } = &item.source_state
                else {
                    return None;
                };
                Some(stable_warmup_task_id(
                    self.session_generation,
                    &item.item_id,
                    *revision,
                    cache_identity,
                    SequenceWarmupGoal::ProgressiveRange,
                ))
            })
            .collect::<HashSet<_>>();
        self.warmup_tasks
            .retain(|task_id, _| valid.contains(task_id));
        self.warmup_task_order
            .retain(|task_id| self.warmup_tasks.contains_key(task_id));
    }

    fn refill_request_count(&self) -> usize {
        self.config
            .forward_window
            .max(self.config.refill_threshold)
            .max(1)
            .min(self.config.max_items)
    }

    fn end_reached(&self, direction: SequenceDirection) -> bool {
        match direction {
            SequenceDirection::Previous => self.previous_end_reached,
            SequenceDirection::Next => self.next_end_reached,
        }
    }

    fn record_event(&mut self, kind: SequenceEventKind) {
        let event = SequenceEvent {
            event_sequence: self.next_event_sequence,
            session_generation: self.session_generation,
            kind,
        };
        self.next_event_sequence = next_non_zero(self.next_event_sequence);
        if self.events.len() >= self.config.max_events {
            self.dropped_events = self.dropped_events.saturating_add(1);
        } else {
            self.events.push_back(event);
        }
    }

    fn capacity_error(&self, message: &str) -> SequenceError {
        SequenceError::new(SequenceErrorCode::CapacityExceeded, message)
    }

    fn unknown_request_error(&self) -> SequenceError {
        SequenceError::new(
            SequenceErrorCode::UnknownRequest,
            "provider request is unknown or already terminal",
        )
    }
}

fn request_id_for_kind(kind: &SequenceRequestKind) -> SequenceRequestId {
    match kind {
        SequenceRequestKind::Items(request) => request.request_id,
        SequenceRequestKind::Source(request) => request.request_id,
    }
}

fn is_terminal_warmup_status(status: SequenceWarmupStatus) -> bool {
    !matches!(status, SequenceWarmupStatus::Started)
}

fn stable_warmup_task_id(
    session_generation: SequenceSessionGeneration,
    item_id: &SequenceItemId,
    revision: SequenceSourceRevision,
    cache_identity: &SequenceCacheIdentity,
    warmup_goal: SequenceWarmupGoal,
) -> SequenceWarmupTaskId {
    // FNV-1a is sufficient for an opaque bounded correlation token here. The
    // report still validates item, revision, and cache identity independently.
    let mut hash = 14_695_981_039_346_656_037u64;
    for byte in session_generation
        .get()
        .to_le_bytes()
        .into_iter()
        .chain([0])
        .chain(
            item_id
                .as_str()
                .bytes()
                .chain([0])
                .chain(cache_identity.canonical_key().bytes())
                .chain([0])
                .chain(revision.get().to_le_bytes())
                .chain([0])
                .chain(warmup_goal.as_str().bytes()),
        )
    {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(1_099_511_628_211);
    }
    SequenceWarmupTaskId::new(hash.max(1))
}

fn validate_source_reference(reference: &SequenceSourceReference) -> SequenceResult<()> {
    let value = reference.as_str();
    if value.is_empty()
        || value.contains("://")
        || value.contains('?')
        || value.contains('#')
        || value.chars().any(char::is_control)
    {
        return Err(SequenceError::new(
            SequenceErrorCode::InvalidArgument,
            "source reference must be an opaque non-URL registry key",
        ));
    }
    Ok(())
}

fn normalize_reason_code(reason_code: impl Into<String>) -> SequenceResult<String> {
    let reason_code = reason_code.into().trim().to_owned();
    if reason_code.is_empty()
        || reason_code.len() > 128
        || reason_code.chars().any(char::is_control)
        || reason_code.contains("://")
        || reason_code.contains('?')
    {
        return Err(SequenceError::new(
            SequenceErrorCode::InvalidArgument,
            "request failure reason must be a short non-sensitive code",
        ));
    }
    Ok(reason_code)
}

fn duration_millis_saturating(duration: Duration) -> u64 {
    duration.as_millis().min(u128::from(u64::MAX)) as u64
}

fn next_non_zero(value: u64) -> u64 {
    value.wrapping_add(1).max(1)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn now() -> SequenceClockSnapshot {
        SequenceClockSnapshot {
            wall_epoch_ms: 1_000_000,
            monotonic: Instant::now(),
        }
    }

    fn content(value: &str) -> SequenceContentIdentity {
        SequenceContentIdentity::new("example.provider", value)
    }

    fn cache(value: &str, revision: u64) -> SequenceCacheIdentity {
        SequenceCacheIdentity::new(
            "example.provider",
            value,
            "1080p",
            "media",
            "public",
            SequenceSourceRevision::new(revision),
        )
    }

    fn resolved_item(id: &str, content_value: &str, revision: u64) -> SequenceItem {
        SequenceItem::resolved(
            id,
            content(content_value),
            SequenceMediaKind::Vod,
            format!("source-ref-{id}-{revision}"),
            cache(content_value, revision),
            None,
        )
    }

    fn replenishable(max_events: usize) -> SequenceCoordinator {
        SequenceCoordinator::new(
            "feed",
            SequenceConfig {
                mode: SequenceMode::Replenishable,
                max_events,
                ..SequenceConfig::default()
            },
        )
        .expect("valid sequence config")
    }

    fn warmup_report(
        intent: &SequencePreloadIntent,
        status: SequenceWarmupStatus,
    ) -> SequenceWarmupReport {
        SequenceWarmupReport {
            session_generation: intent.session_generation,
            task_id: intent.warmup_task_id,
            item_id: intent.item_id.clone(),
            source_revision: intent.source_revision,
            warmup_goal: intent.warmup_goal,
            status,
            expected_bytes: 64 * 1024,
            actual_bytes: if status == SequenceWarmupStatus::Completed {
                64 * 1024
            } else {
                0
            },
            cache_hit: (status == SequenceWarmupStatus::Completed).then_some(false),
            cache_entries: 1,
            cache_bytes: 64 * 1024,
            evicted_entries: 0,
            reason_code: None,
        }
    }

    #[test]
    fn append_allows_same_content_with_distinct_item_ids() {
        let mut coordinator = replenishable(32);
        let clock = now();
        coordinator
            .replace(vec![resolved_item("a", "same", 1)], None, clock)
            .expect("replace queue");
        let request_id = match coordinator.next(clock).expect("request next") {
            SequenceNavigationOutcome::AwaitingItems(request_id) => request_id,
            outcome => panic!("expected provider request, got {outcome:?}"),
        };

        let accepted = coordinator
            .append(
                SequenceItemsResponse {
                    session_generation: coordinator.session_generation(),
                    request_id,
                    anchor_item_id: Some(SequenceItemId::new("a")),
                    items: vec![resolved_item("b", "same", 1)],
                    end_reached: true,
                },
                clock,
            )
            .expect("append response");

        assert_eq!(accepted, 1);
        assert_eq!(coordinator.snapshot().items.len(), 2);
    }

    #[test]
    fn duplicate_item_id_is_rejected_atomically() {
        let mut coordinator = replenishable(32);
        let clock = now();
        let error = coordinator
            .replace(
                vec![resolved_item("same", "a", 1), resolved_item("same", "b", 1)],
                None,
                clock,
            )
            .expect_err("duplicate item id must fail");

        assert_eq!(error.code, SequenceErrorCode::DuplicateItemId);
        assert!(coordinator.snapshot().items.is_empty());
    }

    #[test]
    fn rapid_active_switch_does_not_invalidate_refill_generation() {
        let mut coordinator = replenishable(64);
        let clock = now();
        coordinator
            .replace(
                vec![resolved_item("a", "a", 1), resolved_item("b", "b", 1)],
                None,
                clock,
            )
            .expect("replace queue");
        let generation = coordinator.session_generation();
        let request_id = coordinator
            .snapshot()
            .pending_requests
            .iter()
            .find_map(|request| match &request.kind {
                SequenceRequestKind::Items(items) => {
                    Some((items.request_id, items.anchor_item_id.clone()))
                }
                _ => None,
            })
            .expect("prefetch refill request");
        let (request_id, anchor_item_id) = request_id;

        let item_b = SequenceItemId::new("b");
        coordinator.set_active(&item_b, clock).expect("activate b");
        let item_a = SequenceItemId::new("a");
        coordinator.set_active(&item_a, clock).expect("activate a");

        assert_eq!(coordinator.session_generation(), generation);
        coordinator
            .append(
                SequenceItemsResponse {
                    session_generation: generation,
                    request_id,
                    anchor_item_id,
                    items: vec![resolved_item("c", "c", 1)],
                    end_reached: false,
                },
                clock,
            )
            .expect("refill remains valid");
    }

    #[test]
    fn refill_anchor_stays_on_queue_boundary_during_active_switches() {
        let mut coordinator = replenishable(64);
        let clock = now();
        coordinator
            .replace(
                vec![resolved_item("a", "a", 1), resolved_item("b", "b", 1)],
                None,
                clock,
            )
            .expect("replace queue");
        let request = coordinator
            .snapshot()
            .pending_requests
            .iter()
            .find_map(|pending| match &pending.kind {
                SequenceRequestKind::Items(request) => Some(request.clone()),
                _ => None,
            })
            .expect("refill request");

        coordinator
            .set_active(&SequenceItemId::new("a"), clock)
            .expect("activate a");
        assert_eq!(request.anchor_item_id, Some(SequenceItemId::new("b")));
    }

    #[test]
    fn duplicate_refill_response_is_idempotent() {
        let mut coordinator = replenishable(64);
        let clock = now();
        coordinator
            .replace(vec![resolved_item("a", "a", 1)], None, clock)
            .expect("replace queue");
        let request_id = match coordinator.next(clock).expect("request next") {
            SequenceNavigationOutcome::AwaitingItems(request_id) => request_id,
            outcome => panic!("expected provider request, got {outcome:?}"),
        };
        let response = SequenceItemsResponse {
            session_generation: coordinator.session_generation(),
            request_id,
            anchor_item_id: Some(SequenceItemId::new("a")),
            items: vec![resolved_item("b", "b", 1)],
            end_reached: true,
        };

        assert_eq!(
            coordinator
                .append(response.clone(), clock)
                .expect("first response"),
            1
        );
        assert_eq!(
            coordinator
                .append(response, clock)
                .expect("duplicate response"),
            0
        );
        assert_eq!(coordinator.snapshot().items.len(), 2);
    }

    #[test]
    fn pending_request_survives_event_queue_overflow_and_resync() {
        let mut coordinator = replenishable(1);
        let clock = now();
        coordinator
            .replace(vec![resolved_item("a", "a", 1)], None, clock)
            .expect("replace queue");

        let snapshot = coordinator.snapshot();
        assert_eq!(snapshot.pending_requests.len(), 1);
        assert!(snapshot.dropped_events > 0);

        let _ = coordinator.drain_events();
        coordinator.resync_pending_requests();
        let events = coordinator.drain_events();
        assert!(
            events
                .iter()
                .any(|event| matches!(event.kind, SequenceEventKind::Request(_)))
        );
        assert_eq!(coordinator.snapshot().pending_requests.len(), 1);
    }

    #[test]
    fn stale_generation_response_is_rejected() {
        let mut coordinator = replenishable(32);
        let clock = now();
        coordinator
            .replace(vec![resolved_item("a", "a", 1)], None, clock)
            .expect("first replace");
        let stale_generation = coordinator.session_generation();
        coordinator
            .replace(vec![resolved_item("b", "b", 1)], None, clock)
            .expect("second replace");

        let error = coordinator
            .append(
                SequenceItemsResponse {
                    session_generation: stale_generation,
                    request_id: SequenceRequestId(1),
                    anchor_item_id: None,
                    items: Vec::new(),
                    end_reached: false,
                },
                clock,
            )
            .expect_err("stale response must fail");
        assert_eq!(error.code, SequenceErrorCode::StaleGeneration);
    }

    #[test]
    fn mismatched_refill_anchor_is_rejected_atomically() {
        let mut coordinator = replenishable(32);
        let clock = now();
        coordinator
            .replace(vec![resolved_item("a", "a", 1)], None, clock)
            .expect("replace queue");
        let request_id = match coordinator.next(clock).expect("request next") {
            SequenceNavigationOutcome::AwaitingItems(request_id) => request_id,
            outcome => panic!("expected provider request, got {outcome:?}"),
        };

        let error = coordinator
            .append(
                SequenceItemsResponse {
                    session_generation: coordinator.session_generation(),
                    request_id,
                    anchor_item_id: Some(SequenceItemId::new("other")),
                    items: vec![resolved_item("b", "b", 1)],
                    end_reached: false,
                },
                clock,
            )
            .expect_err("mismatched anchor must fail");

        assert_eq!(error.code, SequenceErrorCode::RequestMismatch);
        assert_eq!(coordinator.snapshot().items.len(), 1);
        assert_eq!(coordinator.snapshot().pending_requests.len(), 1);
    }

    #[test]
    fn stale_resolution_attempt_and_revision_are_rejected() {
        let mut coordinator = replenishable(64);
        let clock = now();
        coordinator
            .replace(
                vec![SequenceItem::unresolved(
                    "a",
                    content("a"),
                    SequenceMediaKind::Vod,
                )],
                None,
                clock,
            )
            .expect("replace queue");
        let request = coordinator
            .snapshot()
            .pending_requests
            .iter()
            .find_map(|pending| match &pending.kind {
                SequenceRequestKind::Source(request) => Some(request.clone()),
                _ => None,
            })
            .expect("source request");

        let error = coordinator
            .submit_resolved_source(SequenceResolvedSource {
                session_generation: request.session_generation,
                request_id: request.request_id,
                attempt_id: SequenceResolutionAttemptId(request.attempt_id.get() + 1),
                item_id: request.item_id.clone(),
                expected_revision: request.expected_revision,
                source_revision: SequenceSourceRevision::new(1),
                source_reference: SequenceSourceReference::new("source-ref-a-1"),
                cache_identity: cache("a", 1),
                expires_at_epoch_ms: None,
            })
            .expect_err("stale attempt must fail");
        assert_eq!(error.code, SequenceErrorCode::StaleSource);

        coordinator
            .submit_resolved_source(SequenceResolvedSource {
                session_generation: request.session_generation,
                request_id: request.request_id,
                attempt_id: request.attempt_id,
                item_id: request.item_id,
                expected_revision: request.expected_revision,
                source_revision: SequenceSourceRevision::new(1),
                source_reference: SequenceSourceReference::new("source-ref-a-1"),
                cache_identity: cache("a", 1),
                expires_at_epoch_ms: None,
            })
            .expect("accept current attempt");
    }

    #[test]
    fn provider_failure_is_retained_in_snapshot_without_sensitive_text() {
        let mut coordinator = replenishable(32);
        let clock = now();
        coordinator
            .replace(
                vec![SequenceItem::unresolved(
                    "a",
                    content("a"),
                    SequenceMediaKind::Vod,
                )],
                None,
                clock,
            )
            .expect("replace queue");
        let request_id = coordinator
            .snapshot()
            .pending_requests
            .iter()
            .find_map(|pending| match &pending.kind {
                SequenceRequestKind::Source(request) => Some(request.request_id),
                _ => None,
            })
            .expect("source request");

        coordinator
            .fail_request(
                coordinator.session_generation(),
                request_id,
                "provider_unavailable",
            )
            .expect("fail request");

        let snapshot = coordinator.snapshot();
        assert!(matches!(
            snapshot.items[0].item.source_state,
            SequenceSourceState::Failed { .. }
        ));
        assert_eq!(snapshot.request_failures.len(), 1);
        assert_eq!(
            snapshot.request_failures[0].reason_code,
            "provider_unavailable"
        );
        assert!(
            coordinator
                .fail_request(
                    coordinator.session_generation(),
                    SequenceRequestId::new(99),
                    "https://example.com/?token=secret",
                )
                .is_err()
        );
    }

    #[test]
    fn wall_clock_expiry_does_not_use_monotonic_deadline() {
        let mut coordinator = replenishable(64);
        let start = now();
        let mut item = resolved_item("a", "a", 1);
        if let SequenceSourceState::Resolved {
            expires_at_epoch_ms,
            ..
        } = &mut item.source_state
        {
            *expires_at_epoch_ms = Some(start.wall_epoch_ms + 20_000);
        }
        coordinator
            .replace(vec![item], None, start)
            .expect("replace");

        coordinator
            .tick(SequenceClockSnapshot {
                wall_epoch_ms: start.wall_epoch_ms,
                monotonic: start.monotonic + Duration::from_secs(60),
            })
            .expect("advance monotonic time");
        assert!(matches!(
            coordinator.snapshot().items[0].item.source_state,
            SequenceSourceState::Resolved { .. }
        ));

        coordinator
            .tick(SequenceClockSnapshot {
                wall_epoch_ms: start.wall_epoch_ms + 6_000,
                monotonic: start.monotonic + Duration::from_secs(61),
            })
            .expect("advance wall clock into expiry lead");
        assert!(matches!(
            coordinator.snapshot().items[0].item.source_state,
            SequenceSourceState::Resolving { .. }
        ));
    }

    #[test]
    fn wall_clock_rollback_does_not_extend_source_lifetime() {
        let mut coordinator = replenishable(64);
        let start = now();
        let mut item = resolved_item("a", "a", 1);
        if let SequenceSourceState::Resolved {
            expires_at_epoch_ms,
            ..
        } = &mut item.source_state
        {
            *expires_at_epoch_ms = Some(start.wall_epoch_ms + 20_000);
        }
        coordinator
            .replace(vec![item], None, start)
            .expect("replace");
        coordinator
            .tick(SequenceClockSnapshot {
                wall_epoch_ms: start.wall_epoch_ms + 10_000,
                monotonic: start.monotonic + Duration::from_secs(1),
            })
            .expect("record forward wall clock");
        coordinator
            .tick(SequenceClockSnapshot {
                wall_epoch_ms: start.wall_epoch_ms.saturating_sub(10_000),
                monotonic: start.monotonic + Duration::from_secs(2),
            })
            .expect("process rollback");

        assert!(matches!(
            coordinator.snapshot().items[0].item.source_state,
            SequenceSourceState::Resolving { .. }
        ));
    }

    #[test]
    fn stale_activation_callback_is_rejected() {
        let mut coordinator = replenishable(64);
        let clock = now();
        coordinator
            .replace(
                vec![resolved_item("a", "a", 1), resolved_item("b", "b", 1)],
                None,
                clock,
            )
            .expect("replace");
        let old_epoch = coordinator.activation_epoch();
        coordinator.next(clock).expect("activate the second item");

        let error = coordinator
            .validate_activation_callback(
                &SequenceItemId::new("a"),
                old_epoch,
                SequenceSourceRevision::new(1),
            )
            .expect_err("old callback must fail");
        assert_eq!(error.code, SequenceErrorCode::StaleActivation);
        coordinator
            .validate_activation_callback(
                &SequenceItemId::new("b"),
                coordinator.activation_epoch(),
                SequenceSourceRevision::new(1),
            )
            .expect("current callback is accepted");
    }

    #[test]
    fn cache_key_is_length_prefixed_and_does_not_use_source_reference() {
        let identity = cache("content", 7);
        let key = identity.canonical_key();

        assert!(key.starts_with("vesper-sequence-cache:v1"));
        assert!(!key.contains("source-ref"));
        assert!(!key.contains("https://"));
        assert!(key.ends_with(":7"));
    }

    #[test]
    fn source_reference_rejects_urls_and_query_tokens() {
        let mut coordinator = replenishable(32);
        let clock = now();
        let item = SequenceItem::resolved(
            "a",
            content("a"),
            SequenceMediaKind::Vod,
            "https://example.com/video?token=secret",
            cache("a", 1),
            None,
        );

        let error = coordinator
            .replace(vec![item], None, clock)
            .expect_err("URL source reference must fail");
        assert_eq!(error.code, SequenceErrorCode::InvalidArgument);
    }

    #[test]
    fn preload_intents_include_current_and_next_without_expired_sources() {
        let mut coordinator = replenishable(64);
        let clock = now();
        coordinator
            .replace(
                vec![
                    resolved_item("a", "a", 1),
                    resolved_item("b", "b", 1),
                    resolved_item("c", "c", 1),
                ],
                Some(SequenceItemId::new("b")),
                clock,
            )
            .expect("replace queue");

        let intents = coordinator.preload_intents(clock.wall_epoch_ms);
        assert_eq!(intents.len(), 2);
        assert_eq!(intents[0].priority, SequencePreloadPriority::Current);
        assert_eq!(intents[1].priority, SequencePreloadPriority::Next);
        assert_eq!(intents[0].warmup_goal, SequenceWarmupGoal::ProgressiveRange);
        assert_eq!(intents[1].warmup_goal, SequenceWarmupGoal::ProgressiveRange);
    }

    #[test]
    fn warmup_started_then_completed_updates_bounded_snapshot_once() {
        let mut coordinator = replenishable(64);
        let clock = now();
        coordinator
            .replace(vec![resolved_item("a", "a", 1)], None, clock)
            .expect("replace queue");
        let intent = coordinator
            .preload_intents(clock.wall_epoch_ms)
            .into_iter()
            .next()
            .expect("current warmup intent");
        let started = warmup_report(&intent, SequenceWarmupStatus::Started);
        coordinator
            .report_warmup(started)
            .expect("accept start report");

        let completed = warmup_report(&intent, SequenceWarmupStatus::Completed);
        coordinator
            .report_warmup(completed.clone())
            .expect("accept terminal report");
        coordinator
            .report_warmup(completed.clone())
            .expect("identical terminal report is idempotent");

        let snapshot = coordinator.snapshot();
        assert_eq!(snapshot.warmup_tasks.len(), 1);
        assert_eq!(
            snapshot.warmup_tasks[0].status,
            SequenceWarmupStatus::Completed
        );
        assert_eq!(snapshot.warmup_stats.started, 1);
        assert_eq!(snapshot.warmup_stats.completed, 1);
        assert_eq!(snapshot.warmup_stats.cache_misses, 1);
        assert_eq!(snapshot.warmup_stats.actual_bytes, 64 * 1024);

        let mut conflicting = completed;
        conflicting.actual_bytes = 1;
        assert_eq!(
            coordinator
                .report_warmup(conflicting)
                .expect_err("terminal report cannot be rewritten")
                .code,
            SequenceErrorCode::StaleSource
        );
    }

    #[test]
    fn terminal_warmup_task_is_not_reissued() {
        let mut coordinator = replenishable(64);
        let clock = now();
        coordinator
            .replace(vec![resolved_item("a", "a", 1)], None, clock)
            .expect("replace queue");
        let intent = coordinator
            .preload_intents(clock.wall_epoch_ms)
            .into_iter()
            .next()
            .expect("current warmup intent");

        coordinator
            .report_warmup(warmup_report(&intent, SequenceWarmupStatus::Started))
            .expect("accept start report");
        assert_eq!(
            coordinator.preload_intents(clock.wall_epoch_ms),
            vec![intent.clone()]
        );

        coordinator
            .report_warmup(warmup_report(&intent, SequenceWarmupStatus::Completed))
            .expect("accept terminal report");

        assert!(coordinator.preload_intents(clock.wall_epoch_ms).is_empty());
    }

    #[test]
    fn warmup_report_rejects_old_session_and_old_revision() {
        let mut coordinator = replenishable(64);
        let clock = now();
        coordinator
            .replace(vec![resolved_item("a", "a", 1)], None, clock)
            .expect("first queue");
        let old_intent = coordinator.preload_intents(clock.wall_epoch_ms).remove(0);

        coordinator
            .replace(vec![resolved_item("a", "a", 1)], None, clock)
            .expect("new session");
        let new_intent = coordinator.preload_intents(clock.wall_epoch_ms).remove(0);
        assert_ne!(old_intent.session_generation, new_intent.session_generation);
        assert_ne!(old_intent.warmup_task_id, new_intent.warmup_task_id);
        assert_eq!(
            coordinator
                .report_warmup(warmup_report(&old_intent, SequenceWarmupStatus::Completed,))
                .expect_err("old session report must fail")
                .code,
            SequenceErrorCode::StaleGeneration
        );

        coordinator
            .mark_source_expired(&new_intent.item_id, new_intent.source_revision, clock)
            .expect("expire current source");
        assert_eq!(
            coordinator
                .report_warmup(warmup_report(&new_intent, SequenceWarmupStatus::Completed,))
                .expect_err("old revision report must fail")
                .code,
            SequenceErrorCode::StaleSource
        );
    }

    #[test]
    fn warmup_ledger_is_per_current_item_not_event_capacity() {
        let mut coordinator = replenishable(1);
        let clock = now();
        coordinator
            .replace(
                vec![resolved_item("a", "a", 1), resolved_item("b", "b", 1)],
                None,
                clock,
            )
            .expect("replace queue");
        let intents = coordinator.preload_intents(clock.wall_epoch_ms);
        assert_eq!(intents.len(), 2);
        for intent in &intents {
            coordinator
                .report_warmup(warmup_report(intent, SequenceWarmupStatus::Completed))
                .expect("accept terminal report");
        }
        coordinator
            .report_warmup(warmup_report(&intents[0], SequenceWarmupStatus::Completed))
            .expect("old terminal remains idempotent");

        let snapshot = coordinator.snapshot();
        assert_eq!(snapshot.warmup_tasks.len(), 2);
        assert_eq!(snapshot.warmup_stats.completed, 2);
        coordinator
            .remove(&SequenceItemId::new("a"), clock)
            .expect("remove warmed item");
        assert_eq!(coordinator.snapshot().warmup_tasks.len(), 1);
    }
}
