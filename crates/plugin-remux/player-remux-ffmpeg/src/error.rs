use thiserror::Error;

#[derive(Debug, Error)]
pub enum FfmpegProcessorError {
    #[error("failed to initialize FFmpeg: {0}")]
    Initialization(String),
    #[error("missing required demuxer `{0}` in linked FFmpeg build")]
    MissingDemuxer(&'static str),
    #[error("invalid path: {0}")]
    InvalidPath(String),
    #[error("i/o failure: {0}")]
    Io(String),
    #[error("ffmpeg remux failure: {0}")]
    Remux(String),
}
