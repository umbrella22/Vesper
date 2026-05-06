use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum PipelineEvent {
    DownloadTaskCreated {
        task_id: String,
        asset_id: String,
    },
    DownloadTaskStateChanged {
        task_id: String,
        new_state: String,
    },
    DownloadTaskCompleted {
        task_id: String,
    },
    DownloadTaskFailed {
        task_id: String,
        error: String,
    },
    PostProcessStarted {
        task_id: String,
        processor: String,
    },
    PostProcessCompleted {
        task_id: String,
        output_path: String,
    },
    PostProcessFailed {
        task_id: String,
        error: String,
    },
    PreloadScheduled {
        candidate_id: String,
    },
    PreloadCompleted {
        candidate_id: String,
    },
    PreloadCancelled {
        candidate_id: String,
        reason: String,
    },
    PlaybackStarted {
        source_id: String,
    },
    PlaybackError {
        source_id: String,
        error: String,
    },
}

pub trait PipelineEventHook: Send + Sync {
    fn on_event(&self, event: &PipelineEvent);
}
