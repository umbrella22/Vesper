#![allow(
    clippy::result_large_err,
    reason = "PlayerError is a shared public API; boxing Android platform errors would change public signatures"
)]

mod download;
mod native;
mod playlist;
mod preload;

pub use download::{AndroidDownloadBridgeSession, AndroidDownloadCommand};
pub use native::*;
pub use playlist::AndroidPlaylistBridgeSession;
pub use preload::{AndroidPreloadBridgeSession, AndroidPreloadCommand};
