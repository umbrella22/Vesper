use thiserror::Error;

pub type DashHlsResult<T> = Result<T, DashHlsError>;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum DashHlsError {
    #[error("invalid MPD: {0}")]
    InvalidMpd(String),
    #[error("unsupported MPD: {0}")]
    UnsupportedMpd(String),
    #[error("invalid MP4: {0}")]
    InvalidMp4(String),
    #[error("unsupported MP4: {0}")]
    UnsupportedMp4(String),
    #[error("invalid HLS input: {0}")]
    InvalidHlsInput(String),
}
