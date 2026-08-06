mod executor;
mod manager;
mod pipeline_event;
mod post_processing;
mod store;
#[cfg(test)]
mod tests;
mod types;

pub use executor::{DownloadExecutor, DownloadPrepareResult, InMemoryDownloadExecutor};
pub use manager::{
    DownloadExportPlan, DownloadManager, DownloadManagerConfig, MAX_PENDING_DOWNLOAD_EVENTS,
};
pub use pipeline_event::{
    MAX_PENDING_PIPELINE_EVENT_REPORTS, MAX_PENDING_PIPELINE_EVENTS, MAX_PIPELINE_EVENT_HOOKS,
    PipelineEventDispatcher, PipelineEventHookRegistration, PipelineEventHookReport,
    PipelineEventHookReportBatch,
};
pub use post_processing::PostDownloadProcessorRegistration;
pub use store::{DownloadStore, InMemoryDownloadStore};
pub use types::{
    DownloadAssetId, DownloadAssetIndex, DownloadAssetStream, DownloadByteRange,
    DownloadContentFormat, DownloadErrorSummary, DownloadEvent, DownloadEventBatch,
    DownloadProfile, DownloadProgressSnapshot, DownloadResourceRecord, DownloadSegmentRecord,
    DownloadSnapshot, DownloadSource, DownloadStreamKind, DownloadTaskId,
    DownloadTaskProgressPatch, DownloadTaskSnapshot, DownloadTaskState, DownloadTaskStatePatch,
    DownloadTaskStatus,
};
