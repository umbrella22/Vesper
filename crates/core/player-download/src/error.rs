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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlayerRuntimeErrorCategory {
    Input,
    Source,
    Network,
    Decode,
    AudioOutput,
    Playback,
    Capability,
    Platform,
}

#[derive(Debug, Clone)]
pub struct PlayerRuntimeError {
    code: PlayerRuntimeErrorCode,
    category: PlayerRuntimeErrorCategory,
    retriable: bool,
    message: String,
}

pub type PlayerRuntimeResult<T> = Result<T, PlayerRuntimeError>;

impl PlayerRuntimeError {
    pub fn new(code: PlayerRuntimeErrorCode, message: impl Into<String>) -> Self {
        let (category, retriable) = default_taxonomy_for_code(code);
        Self {
            code,
            category,
            retriable,
            message: message.into(),
        }
    }

    pub fn with_category(
        code: PlayerRuntimeErrorCode,
        category: PlayerRuntimeErrorCategory,
        message: impl Into<String>,
    ) -> Self {
        Self {
            code,
            category,
            retriable: default_retriable_for_category(category),
            message: message.into(),
        }
    }

    pub fn with_taxonomy(
        code: PlayerRuntimeErrorCode,
        category: PlayerRuntimeErrorCategory,
        retriable: bool,
        message: impl Into<String>,
    ) -> Self {
        Self {
            code,
            category,
            retriable,
            message: message.into(),
        }
    }

    pub fn code(&self) -> PlayerRuntimeErrorCode {
        self.code
    }

    pub fn category(&self) -> PlayerRuntimeErrorCategory {
        self.category
    }

    pub fn is_retriable(&self) -> bool {
        self.retriable
    }

    pub fn message(&self) -> &str {
        &self.message
    }
}

impl Display for PlayerRuntimeError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} ({:?}/{:?}, retriable={})",
            self.message, self.code, self.category, self.retriable
        )
    }
}

impl Error for PlayerRuntimeError {}

fn default_taxonomy_for_code(code: PlayerRuntimeErrorCode) -> (PlayerRuntimeErrorCategory, bool) {
    let category = match code {
        PlayerRuntimeErrorCode::InvalidArgument => PlayerRuntimeErrorCategory::Input,
        PlayerRuntimeErrorCode::InvalidState => PlayerRuntimeErrorCategory::Playback,
        PlayerRuntimeErrorCode::InvalidSource => PlayerRuntimeErrorCategory::Source,
        PlayerRuntimeErrorCode::BackendFailure => PlayerRuntimeErrorCategory::Platform,
        PlayerRuntimeErrorCode::AudioOutputUnavailable => PlayerRuntimeErrorCategory::AudioOutput,
        PlayerRuntimeErrorCode::DecodeFailure => PlayerRuntimeErrorCategory::Decode,
        PlayerRuntimeErrorCode::SeekFailure => PlayerRuntimeErrorCategory::Playback,
        PlayerRuntimeErrorCode::Unsupported => PlayerRuntimeErrorCategory::Capability,
    };
    (category, default_retriable_for_category(category))
}

fn default_retriable_for_category(category: PlayerRuntimeErrorCategory) -> bool {
    matches!(category, PlayerRuntimeErrorCategory::Network)
}

#[cfg(test)]
mod tests {
    use super::{PlayerRuntimeError, PlayerRuntimeErrorCategory, PlayerRuntimeErrorCode};

    #[test]
    fn runtime_error_defaults_to_code_taxonomy() {
        let error =
            PlayerRuntimeError::new(PlayerRuntimeErrorCode::DecodeFailure, "decoder init failed");

        assert_eq!(error.code(), PlayerRuntimeErrorCode::DecodeFailure);
        assert_eq!(error.category(), PlayerRuntimeErrorCategory::Decode);
        assert!(!error.is_retriable());
    }

    #[test]
    fn runtime_error_can_override_taxonomy() {
        let error = PlayerRuntimeError::with_taxonomy(
            PlayerRuntimeErrorCode::BackendFailure,
            PlayerRuntimeErrorCategory::Network,
            true,
            "network timed out",
        );

        assert_eq!(error.code(), PlayerRuntimeErrorCode::BackendFailure);
        assert_eq!(error.category(), PlayerRuntimeErrorCategory::Network);
        assert!(error.is_retriable());
    }
}
