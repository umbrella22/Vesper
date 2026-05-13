use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::ProcessorCapabilities;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ContentFormatKind {
    HlsSegments,
    DashSegments,
    FlvSegments,
    SingleFile,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum OutputFormat {
    Mp4,
    Mkv,
    Original,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum StreamKind {
    Combined,
    Video,
    Audio,
    SecondaryAudio,
    Subtitle,
    Auxiliary,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum AssemblyMode {
    #[default]
    Single,
    SeparateAudioVideo,
    MultiAudio,
    WithSubtitles,
    Generic,
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
    #[serde(default)]
    pub streams: Vec<CompletedStream>,
    #[serde(default)]
    pub assembly_mode: AssemblyMode,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompletedStream {
    pub stream_id: Option<String>,
    pub kind: StreamKind,
    pub content_format: CompletedContentFormat,
    pub language: Option<String>,
    pub codec: Option<String>,
    pub label: Option<String>,
    pub metadata: DownloadMetadata,
    pub quality_rank: Option<u32>,
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
    FlvSegments {
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
            Self::FlvSegments { .. } => ContentFormatKind::FlvSegments,
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
    #[error("payload codec error: {0}")]
    PayloadCodec(String),
    #[error("plugin ABI violation: {0}")]
    AbiViolation(String),
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
            supports_assembly: false,
            supported_assembly_modes: Vec::new(),
        }
    }

    fn process(
        &self,
        input: &CompletedDownloadInfo,
        output_path: &Path,
        progress: &dyn ProcessorProgress,
    ) -> Result<ProcessorOutput, ProcessorError>;

    fn supports_assembly(&self) -> bool {
        self.capabilities().supports_assembly
    }

    fn assemble(
        &self,
        input: &CompletedDownloadInfo,
        _output_path: &Path,
        _progress: &dyn ProcessorProgress,
    ) -> Result<ProcessorOutput, ProcessorError> {
        Err(ProcessorError::UnsupportedFormat(
            input.content_format.kind(),
        ))
    }
}
