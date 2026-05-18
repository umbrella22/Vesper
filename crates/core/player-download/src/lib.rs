#![deny(unsafe_code)]

mod download;
mod error;
mod planner;

pub use download::{
    DownloadAssetId, DownloadAssetIndex, DownloadAssetStream, DownloadByteRange,
    DownloadContentFormat, DownloadErrorSummary, DownloadEvent, DownloadExecutor, DownloadManager,
    DownloadManagerConfig, DownloadPrepareResult, DownloadProfile, DownloadProgressSnapshot,
    DownloadResourceRecord, DownloadSegmentRecord, DownloadSnapshot, DownloadSource, DownloadStore,
    DownloadStreamKind, DownloadTaskId, DownloadTaskProgressPatch, DownloadTaskSnapshot,
    DownloadTaskState, DownloadTaskStatePatch, DownloadTaskStatus, InMemoryDownloadExecutor,
    InMemoryDownloadStore,
};
pub use error::{PlayerError, PlayerErrorCategory, PlayerErrorCode, PlayerResult};
pub use planner::{DownloadPlanner, DownloadPlanningClient};
