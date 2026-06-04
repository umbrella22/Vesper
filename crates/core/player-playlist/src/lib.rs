#![deny(unsafe_code)]
//! Playlist coordination for queue state, active item selection, and preload sync.
//!
//! This crate owns the pure scheduling logic for playlist queues. It chooses an
//! active item from queue and viewport hints, resolves next/previous decisions,
//! and translates the desired playback neighborhood into preload candidates.

mod playlist;

pub use playlist::{
    PlaylistActivationReason, PlaylistActiveItem, PlaylistAdvanceDecision, PlaylistAdvanceOutcome,
    PlaylistAdvanceTrigger, PlaylistCoordinator, PlaylistCoordinatorConfig, PlaylistEvent,
    PlaylistFailureStrategy, PlaylistId, PlaylistItemPreloadProfile, PlaylistNeighborWindow,
    PlaylistPreloadWindow, PlaylistQueueItem, PlaylistQueueItemId, PlaylistQueueItemSnapshot,
    PlaylistRepeatMode, PlaylistSnapshot, PlaylistSwitchPolicy, PlaylistViewportHint,
    PlaylistViewportHintKind,
};
