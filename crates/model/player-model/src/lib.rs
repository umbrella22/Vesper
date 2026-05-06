#![deny(unsafe_code)]

mod controller;
mod error;
mod model;
mod session;

pub use controller::{PlaybackCommand, Player, PlayerConfig, PlayerEvent, PlayerHandle};
pub use error::PlayerError;
pub use model::{
    DecodedVideoFrame, MediaAbrMode, MediaAbrPolicy, MediaSource, MediaSourceKind,
    MediaSourceProtocol, MediaTrack, MediaTrackCatalog, MediaTrackKind, MediaTrackSelection,
    MediaTrackSelectionMode, MediaTrackSelectionSnapshot, PlaybackState, VideoPixelFormat,
};
pub use session::{PlaybackProgress, PlaybackSessionModel, PresentationState};
