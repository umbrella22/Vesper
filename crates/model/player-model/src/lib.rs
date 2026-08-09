#![deny(unsafe_code)]

mod error;
mod model;
mod session;

pub use error::{
    FixedTrackSelectionErrorDetails, PlayerError, PlayerErrorCategory, PlayerErrorCode,
    PlayerResult, SubtitleErrorDetails,
};
pub use model::{
    DecodedVideoFrame, MediaAbrMode, MediaAbrPolicy, MediaSource, MediaSourceKind,
    MediaSourceProtocol, MediaSubtitleStyle, MediaTrack, MediaTrackCatalog, MediaTrackKind,
    MediaTrackSelection, MediaTrackSelectionMode, MediaTrackSelectionSnapshot, MediaTrackSupport,
    MediaTrackSupportDiagnostics, MediaTrackSupportReason, MediaTrackSupportSource,
    MediaTrackSupportStatus, PlaybackState, VideoPixelFormat,
};
pub use session::{PlaybackProgress, PlaybackSessionModel, PresentationState};
