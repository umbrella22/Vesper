use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::ProcessorCapabilities;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ContentFormatKind {
    HlsSegments,
    DashSegments,
    SingleFile,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum OutputFormat {
    Mp4,
    Mkv,
    Original,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct DownloadMetadata {
    pub source_uri: Option<String>,
    pub manifest_uri: Option<String>,
    pub total_bytes: Option<u64>,
    pub version: Option<String>,
    pub etag: Option<String>,
    pub checksum: Option<String>,
    pub mime_type: Option<String>,
    pub custom: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompletedDownloadInfo {
    pub asset_id: String,
    pub task_id: Option<String>,
    pub content_format: CompletedContentFormat,
    pub metadata: DownloadMetadata,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum CompletedContentFormat {
    HlsSegments {
        manifest_path: PathBuf,
        segment_paths: Vec<PathBuf>,
    },
    DashSegments {
        manifest_path: PathBuf,
        segment_paths: Vec<PathBuf>,
    },
    SingleFile {
        path: PathBuf,
    },
}

impl CompletedContentFormat {
    pub fn kind(&self) -> ContentFormatKind {
        match self {
            Self::HlsSegments { .. } => ContentFormatKind::HlsSegments,
            Self::DashSegments { .. } => ContentFormatKind::DashSegments,
            Self::SingleFile { .. } => ContentFormatKind::SingleFile,
        }
    }
}

pub trait ProcessorProgress: Send + Sync {
    fn on_progress(&self, ratio: f32);

    fn is_cancelled(&self) -> bool {
        false
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProcessorOutput {
    MuxedFile { path: PathBuf, format: OutputFormat },
    Skipped,
}

#[derive(Debug, Error, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProcessorError {
    #[error("unsupported input format: {0:?}")]
    UnsupportedFormat(ContentFormatKind),
    #[error("mux failed: {0}")]
    MuxFailed(String),
    #[error("output path error: {0}")]
    OutputPath(String),
    #[error("cancelled")]
    Cancelled,
}

pub trait PostDownloadProcessor: Send + Sync {
    fn name(&self) -> &str;

    fn supported_input_formats(&self) -> &[ContentFormatKind];

    fn capabilities(&self) -> ProcessorCapabilities {
        ProcessorCapabilities {
            supported_input_formats: self.supported_input_formats().to_vec(),
            output_formats: Vec::new(),
            supports_cancellation: true,
        }
    }

    fn process(
        &self,
        input: &CompletedDownloadInfo,
        output_path: &Path,
        progress: &dyn ProcessorProgress,
    ) -> Result<ProcessorOutput, ProcessorError>;
}
