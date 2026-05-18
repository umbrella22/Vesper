mod executor;
mod manager;
mod post_processing;
mod store;
#[cfg(test)]
mod tests;
mod types;

pub use executor::{DownloadExecutor, DownloadPrepareResult, InMemoryDownloadExecutor};
pub use manager::{DownloadManager, DownloadManagerConfig};
pub use store::{DownloadStore, InMemoryDownloadStore};
pub use types::{
    DownloadAssetId, DownloadAssetIndex, DownloadAssetStream, DownloadByteRange,
    DownloadContentFormat, DownloadErrorSummary, DownloadEvent, DownloadProfile,
    DownloadProgressSnapshot, DownloadResourceRecord, DownloadSegmentRecord, DownloadSnapshot,
    DownloadSource, DownloadStreamKind, DownloadTaskId, DownloadTaskProgressPatch,
    DownloadTaskSnapshot, DownloadTaskState, DownloadTaskStatePatch, DownloadTaskStatus,
};
