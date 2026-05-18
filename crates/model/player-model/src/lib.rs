#![deny(unsafe_code)]

mod error;
mod model;
mod session;

pub use error::{PlayerError, PlayerErrorCategory, PlayerErrorCode, PlayerResult};
pub use model::{
    DecodedVideoFrame, MediaAbrMode, MediaAbrPolicy, MediaSource, MediaSourceKind,
    MediaSourceProtocol, MediaTrack, MediaTrackCatalog, MediaTrackKind, MediaTrackSelection,
    MediaTrackSelectionMode, MediaTrackSelectionSnapshot, PlaybackState, VideoPixelFormat,
};
pub use session::{PlaybackProgress, PlaybackSessionModel, PresentationState};
