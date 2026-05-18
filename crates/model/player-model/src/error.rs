use std::error::Error;
use std::fmt::{self, Display, Formatter};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlayerErrorCode {
    InvalidArgument,
    InvalidState,
    InvalidSource,
    BackendFailure,
    AudioOutputUnavailable,
    DecodeFailure,
    SeekFailure,
    Unsupported,
    CommandChannelClosed,
    EventChannelClosed,
    Cancelled,
    Timeout,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlayerErrorCategory {
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
pub struct PlayerError {
    code: PlayerErrorCode,
    category: PlayerErrorCategory,
    retriable: bool,
    message: String,
}

pub type PlayerResult<T> = Result<T, PlayerError>;

impl PlayerError {
    pub fn new(code: PlayerErrorCode, message: impl Into<String>) -> Self {
        let (category, retriable) = default_taxonomy_for_code(code);
        Self {
            code,
            category,
            retriable,
            message: message.into(),
        }
    }

    pub fn with_category(
        code: PlayerErrorCode,
        category: PlayerErrorCategory,
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
        code: PlayerErrorCode,
        category: PlayerErrorCategory,
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

    pub fn command_channel_closed() -> Self {
        Self::new(
            PlayerErrorCode::CommandChannelClosed,
            "player command channel closed",
        )
    }

    pub fn event_channel_closed() -> Self {
        Self::new(
            PlayerErrorCode::EventChannelClosed,
            "player event channel closed",
        )
    }

    pub fn code(&self) -> PlayerErrorCode {
        self.code
    }

    pub fn category(&self) -> PlayerErrorCategory {
        self.category
    }

    pub fn is_retriable(&self) -> bool {
        self.retriable
    }

    pub fn message(&self) -> &str {
        &self.message
    }
}

impl Display for PlayerError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} ({:?}/{:?}, retriable={})",
            self.message, self.code, self.category, self.retriable
        )
    }
}

impl Error for PlayerError {}

fn default_taxonomy_for_code(code: PlayerErrorCode) -> (PlayerErrorCategory, bool) {
    let category = match code {
        PlayerErrorCode::InvalidArgument => PlayerErrorCategory::Input,
        PlayerErrorCode::InvalidState => PlayerErrorCategory::Playback,
        PlayerErrorCode::InvalidSource => PlayerErrorCategory::Source,
        PlayerErrorCode::BackendFailure => PlayerErrorCategory::Platform,
        PlayerErrorCode::AudioOutputUnavailable => PlayerErrorCategory::AudioOutput,
        PlayerErrorCode::DecodeFailure => PlayerErrorCategory::Decode,
        PlayerErrorCode::SeekFailure => PlayerErrorCategory::Playback,
        PlayerErrorCode::Unsupported => PlayerErrorCategory::Capability,
        PlayerErrorCode::CommandChannelClosed
        | PlayerErrorCode::EventChannelClosed
        | PlayerErrorCode::Cancelled
        | PlayerErrorCode::Timeout => PlayerErrorCategory::Playback,
    };
    (category, default_retriable_for_category(category))
}

fn default_retriable_for_category(category: PlayerErrorCategory) -> bool {
    matches!(category, PlayerErrorCategory::Network)
}

#[cfg(test)]
mod tests {
    use super::{PlayerError, PlayerErrorCategory, PlayerErrorCode};

    #[test]
    fn player_error_defaults_to_legacy_code_taxonomy() {
        let cases = [
            (
                PlayerErrorCode::InvalidArgument,
                PlayerErrorCategory::Input,
                false,
            ),
            (
                PlayerErrorCode::InvalidState,
                PlayerErrorCategory::Playback,
                false,
            ),
            (
                PlayerErrorCode::InvalidSource,
                PlayerErrorCategory::Source,
                false,
            ),
            (
                PlayerErrorCode::BackendFailure,
                PlayerErrorCategory::Platform,
                false,
            ),
            (
                PlayerErrorCode::AudioOutputUnavailable,
                PlayerErrorCategory::AudioOutput,
                false,
            ),
            (
                PlayerErrorCode::DecodeFailure,
                PlayerErrorCategory::Decode,
                false,
            ),
            (
                PlayerErrorCode::SeekFailure,
                PlayerErrorCategory::Playback,
                false,
            ),
            (
                PlayerErrorCode::Unsupported,
                PlayerErrorCategory::Capability,
                false,
            ),
        ];

        for (code, category, retriable) in cases {
            let error = PlayerError::new(code, "error");

            assert_eq!(error.code(), code);
            assert_eq!(error.category(), category);
            assert_eq!(error.is_retriable(), retriable);
        }
    }

    #[test]
    fn player_error_can_override_taxonomy() {
        let error = PlayerError::with_taxonomy(
            PlayerErrorCode::BackendFailure,
            PlayerErrorCategory::Network,
            true,
            "network timed out",
        );

        assert_eq!(error.code(), PlayerErrorCode::BackendFailure);
        assert_eq!(error.category(), PlayerErrorCategory::Network);
        assert!(error.is_retriable());
        assert_eq!(error.message(), "network timed out");
    }

    #[test]
    fn channel_errors_have_playback_taxonomy() {
        let command = PlayerError::command_channel_closed();
        let event = PlayerError::event_channel_closed();

        assert_eq!(command.code(), PlayerErrorCode::CommandChannelClosed);
        assert_eq!(command.category(), PlayerErrorCategory::Playback);
        assert!(!command.is_retriable());
        assert_eq!(event.code(), PlayerErrorCode::EventChannelClosed);
        assert_eq!(event.category(), PlayerErrorCategory::Playback);
        assert!(!event.is_retriable());
    }
}
