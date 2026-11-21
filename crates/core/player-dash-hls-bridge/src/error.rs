use thiserror::Error;

pub use player_model::SubtitleErrorDetails;

pub type DashHlsResult<T> = Result<T, DashHlsError>;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum DashHlsError {
    #[error("subtitle error: {details}")]
    Subtitle { details: SubtitleErrorDetails },
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
    #[error(
        "invalid HLS input: segment exceeds the playlist target duration of {target_duration} seconds"
    )]
    TargetDurationExceeded { target_duration: u64 },
}

impl DashHlsError {
    pub fn subtitle_details(&self) -> Option<&SubtitleErrorDetails> {
        match self {
            Self::Subtitle { details } => Some(details),
            _ => None,
        }
    }
}
