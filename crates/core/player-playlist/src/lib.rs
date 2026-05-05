#![deny(unsafe_code)]

mod playlist;

pub use playlist::{
    PlaylistActivationReason, PlaylistActiveItem, PlaylistAdvanceDecision, PlaylistAdvanceOutcome,
    PlaylistAdvanceTrigger, PlaylistCoordinator, PlaylistCoordinatorConfig, PlaylistEvent,
    PlaylistFailureStrategy, PlaylistId, PlaylistItemPreloadProfile, PlaylistNeighborWindow,
    PlaylistPreloadWindow, PlaylistQueueItem, PlaylistQueueItemId, PlaylistQueueItemSnapshot,
    PlaylistRepeatMode, PlaylistSnapshot, PlaylistSwitchPolicy, PlaylistViewportHint,
    PlaylistViewportHintKind,
};
