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
pub use playlist::IosPlaylistBridgeSession;
pub use preload::{IosPreloadBridgeSession, IosPreloadCommand};
