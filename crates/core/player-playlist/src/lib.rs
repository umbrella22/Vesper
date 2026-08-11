#![deny(unsafe_code)]
#![allow(
    clippy::result_large_err,
    reason = "PlayerError is a shared public API; boxing playlist errors would change public signatures"
)]
//! Playlist coordination for queue state, active item selection, and preload sync.
//!
//! This crate owns the pure scheduling logic for playlist queues. It chooses an
//! active item from queue and viewport hints, resolves next/previous decisions,
//! and translates the desired playback neighborhood into preload candidates.

mod playlist;
mod sequence;

pub use playlist::{
    MAX_PENDING_PLAYLIST_EVENTS, PlaylistActivationReason, PlaylistActiveItem,
    PlaylistAdvanceDecision, PlaylistAdvanceOutcome, PlaylistAdvanceTrigger, PlaylistCoordinator,
    PlaylistCoordinatorConfig, PlaylistEvent, PlaylistFailureStrategy, PlaylistId,
    PlaylistItemPreloadProfile, PlaylistNeighborWindow, PlaylistPreloadWindow, PlaylistQueueItem,
    PlaylistQueueItemId, PlaylistQueueItemSnapshot, PlaylistRepeatMode, PlaylistSnapshot,
    PlaylistSwitchPolicy, PlaylistViewportHint, PlaylistViewportHintKind,
};
pub use sequence::{
    DEFAULT_SEQUENCE_MAX_EVENTS, DEFAULT_SEQUENCE_MAX_ITEMS, DEFAULT_SEQUENCE_MAX_PENDING_REQUESTS,
    SequenceActivationEpoch, SequenceActivationReason, SequenceCacheIdentity,
    SequenceClockSnapshot, SequenceConfig, SequenceContentIdentity, SequenceCoordinator,
    SequenceDirection, SequenceError, SequenceErrorCode, SequenceEvent, SequenceEventKind,
    SequenceId, SequenceItem, SequenceItemId, SequenceItemSnapshot, SequenceItemsRequest,
    SequenceItemsResponse, SequenceMediaKind, SequenceMode, SequenceNavigationOutcome,
    SequencePendingRequest, SequencePreloadIntent, SequencePreloadPriority, SequencePreloadProfile,
    SequenceRequestDeliveryState, SequenceRequestFailure, SequenceRequestId, SequenceRequestKind,
    SequenceResolutionAttemptId, SequenceResolvedSource, SequenceResult, SequenceSessionGeneration,
    SequenceSnapshot, SequenceSourceReference, SequenceSourceRequest,
    SequenceSourceResolutionReason, SequenceSourceRevision, SequenceSourceState,
    SequenceWarmupGoal, SequenceWarmupReport, SequenceWarmupStats, SequenceWarmupStatus,
    SequenceWarmupTaskId, SequenceWarmupTaskSnapshot,
};
