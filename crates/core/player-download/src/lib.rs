#![deny(unsafe_code)]
#![allow(
    clippy::result_large_err,
    reason = "PlayerError is a shared public API; boxing download errors would change public signatures"
)]

mod download;
mod error;
mod planner;

pub use download::{
    DownloadAssetId, DownloadAssetIndex, DownloadAssetStream, DownloadByteRange,
    DownloadContentFormat, DownloadErrorSummary, DownloadEvent, DownloadEventBatch,
    DownloadExecutor, DownloadExportPlan, DownloadManager, DownloadManagerConfig,
    DownloadPrepareResult, DownloadProfile, DownloadProgressSnapshot, DownloadResourceRecord,
    DownloadSegmentRecord, DownloadSnapshot, DownloadSource, DownloadStore, DownloadStreamKind,
    DownloadTaskId, DownloadTaskProgressPatch, DownloadTaskSnapshot, DownloadTaskState,
    DownloadTaskStatePatch, DownloadTaskStatus, InMemoryDownloadExecutor, InMemoryDownloadStore,
    MAX_PENDING_DOWNLOAD_EVENTS, MAX_PENDING_PIPELINE_EVENT_REPORTS, MAX_PENDING_PIPELINE_EVENTS,
    MAX_PIPELINE_EVENT_HOOKS, PipelineEventDispatcher, PipelineEventHookRegistration,
    PipelineEventHookReport, PipelineEventHookReportBatch, PostDownloadProcessorRegistration,
};
pub use error::{PlayerError, PlayerErrorCategory, PlayerErrorCode, PlayerResult};
pub use planner::{DownloadPlanner, DownloadPlanningClient};
