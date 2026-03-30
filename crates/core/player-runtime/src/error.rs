use std::error::Error;
use std::fmt::{self, Display, Formatter};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlayerRuntimeErrorCode {
    InvalidArgument,
    InvalidState,
    InvalidSource,
    BackendFailure,
    AudioOutputUnavailable,
    DecodeFailure,
    SeekFailure,
    Unsupported,
}

#[derive(Debug, Clone)]
pub struct PlayerRuntimeError {
    code: PlayerRuntimeErrorCode,
    message: String,
}

pub type PlayerRuntimeResult<T> = Result<T, PlayerRuntimeError>;

impl PlayerRuntimeError {
    pub fn new(code: PlayerRuntimeErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }

    pub fn code(&self) -> PlayerRuntimeErrorCode {
        self.code
    }

    pub fn message(&self) -> &str {
        &self.message
    }
}

impl Display for PlayerRuntimeError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "{} ({:?})", self.message, self.code)
    }
}

impl Error for PlayerRuntimeError {}
