#![deny(unsafe_code)]

mod download;
mod error;
mod planner;

pub use download::{
    DownloadAssetId, DownloadAssetIndex, DownloadByteRange, DownloadContentFormat,
    DownloadErrorSummary, DownloadEvent, DownloadExecutor, DownloadManager, DownloadManagerConfig,
    DownloadPrepareResult, DownloadProfile, DownloadProgressSnapshot, DownloadResourceRecord,
    DownloadSegmentRecord, DownloadSnapshot, DownloadSource, DownloadStore, DownloadTaskId,
    DownloadTaskProgressPatch, DownloadTaskSnapshot, DownloadTaskState, DownloadTaskStatePatch,
    DownloadTaskStatus, InMemoryDownloadExecutor, InMemoryDownloadStore,
};
pub use error::{
    PlayerRuntimeError, PlayerRuntimeErrorCategory, PlayerRuntimeErrorCode, PlayerRuntimeResult,
};
pub use planner::{DownloadPlanner, DownloadPlanningClient};
