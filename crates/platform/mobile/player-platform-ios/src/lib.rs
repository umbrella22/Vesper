#![allow(
    clippy::result_large_err,
    reason = "PlayerError is a shared public API; boxing iOS platform errors would change public signatures"
)]

mod download;
mod native;
mod playlist;
mod preload;

pub use download::{IosDownloadBridgeSession, IosDownloadCommand};
pub use native::*;
pub use player_platform_mobile::{
    MAX_MOBILE_SEQUENCE_BATCH_ITEMS, MAX_MOBILE_SEQUENCE_JSON_BYTES, MobileSequenceBridgeError,
    MobileSequenceBridgeSession as IosSequenceBridgeSession,
};
pub use playlist::IosPlaylistBridgeSession;
pub use preload::{IosPreloadBridgeSession, IosPreloadCommand};
