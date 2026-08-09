use std::error::Error;
use std::fmt::{self, Display, Formatter};

use serde::{Deserialize, Serialize};

/// Structured subtitle failure details shared by runtime and native bindings.
///
/// `code` and `phase` remain strings so newer values cross an older host
/// without being discarded. Consumers validate known values while preserving
/// the raw wire value for diagnostics and forward compatibility.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubtitleErrorDetails {
    pub code: String,
    pub phase: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub track_id: Option<String>,
    pub retriable: bool,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command_id: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_epoch: Option<u64>,
}

impl SubtitleErrorDetails {
    pub fn new(
        code: impl Into<String>,
        phase: impl Into<String>,
        track_id: Option<String>,
        retriable: bool,
        message: impl Into<String>,
    ) -> Self {
        Self {
            code: code.into(),
            phase: phase.into(),
            track_id,
            retriable,
            message: message.into(),
            command_id: None,
            source_epoch: None,
        }
    }

    pub fn with_transaction(mut self, command_id: Option<u64>, source_epoch: Option<u64>) -> Self {
        self.command_id = command_id;
        self.source_epoch = source_epoch;
        self
    }
}

impl Display for SubtitleErrorDetails {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}: {}", self.code, self.message)
    }
}

/// Structured failure details for an explicit fixed-video-track command.
///
/// The code is intentionally a string so newer host values can cross an older
/// binding without being silently rewritten to a different selection outcome.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FixedTrackSelectionErrorDetails {
    pub code: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub track_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected_catalog_revision: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actual_catalog_revision: Option<u64>,
    pub message: String,
}

impl FixedTrackSelectionErrorDetails {
    pub fn new(
        code: impl Into<String>,
        track_id: Option<String>,
        expected_catalog_revision: Option<u64>,
        actual_catalog_revision: Option<u64>,
        message: impl Into<String>,
    ) -> Self {
        Self {
            code: code.into(),
            track_id,
            expected_catalog_revision,
            actual_catalog_revision,
            message: message.into(),
        }
    }
}

impl Display for FixedTrackSelectionErrorDetails {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}: {}", self.code, self.message)
    }
}

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
    subtitle_details: Option<SubtitleErrorDetails>,
    fixed_track_selection_details: Option<FixedTrackSelectionErrorDetails>,
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
            subtitle_details: None,
            fixed_track_selection_details: None,
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
            subtitle_details: None,
            fixed_track_selection_details: None,
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
            subtitle_details: None,
            fixed_track_selection_details: None,
        }
    }

    /// Attaches a structured subtitle failure to this runtime error.
    pub fn with_subtitle_details(mut self, details: SubtitleErrorDetails) -> Self {
        self.retriable = details.retriable;
        self.message = details.message.clone();
        self.subtitle_details = Some(details);
        self
    }

    /// Returns structured subtitle details when this error crossed a subtitle boundary.
    pub fn subtitle_details(&self) -> Option<&SubtitleErrorDetails> {
        self.subtitle_details.as_ref()
    }

    /// Attaches a structured fixed-track selection failure to this runtime error.
    pub fn with_fixed_track_selection_details(
        mut self,
        details: FixedTrackSelectionErrorDetails,
    ) -> Self {
        self.message = details.message.clone();
        self.fixed_track_selection_details = Some(details);
        self
    }

    /// Returns structured fixed-track selection details when present.
    pub fn fixed_track_selection_details(&self) -> Option<&FixedTrackSelectionErrorDetails> {
        self.fixed_track_selection_details.as_ref()
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
    use super::{
        FixedTrackSelectionErrorDetails, PlayerError, PlayerErrorCategory, PlayerErrorCode,
        SubtitleErrorDetails,
    };

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

    #[test]
    fn subtitle_details_preserve_unknown_wire_values_and_transaction_identity() {
        let details = SubtitleErrorDetails::new(
            "future_subtitle_code",
            "future_phase",
            Some("opaque-track".to_owned()),
            true,
            "future subtitle failure",
        )
        .with_transaction(Some(42), Some(9));
        let json = serde_json::to_string(&details).expect("serialize details");
        let decoded: SubtitleErrorDetails =
            serde_json::from_str(&json).expect("deserialize details");

        assert_eq!(decoded, details);
        assert!(json.contains("future_subtitle_code"));
        assert!(json.contains("future_phase"));
    }

    #[test]
    fn attaching_subtitle_details_keeps_outer_error_consistent() {
        let error = PlayerError::new(PlayerErrorCode::InvalidArgument, "outer")
            .with_subtitle_details(SubtitleErrorDetails::new(
                "subtitle_selection_timeout",
                "selection",
                None,
                true,
                "typed timeout",
            ));

        assert_eq!(error.message(), "typed timeout");
        assert!(error.is_retriable());
        assert_eq!(
            error
                .subtitle_details()
                .map(|details| details.code.as_str()),
            Some("subtitle_selection_timeout")
        );
    }

    #[test]
    fn attaching_fixed_track_selection_details_keeps_message_and_revisions() {
        let error = PlayerError::new(PlayerErrorCode::Unsupported, "outer")
            .with_fixed_track_selection_details(FixedTrackSelectionErrorDetails::new(
                "trackExceedsCapabilities",
                Some("video:4k".to_owned()),
                Some(7),
                Some(8),
                "the requested track exceeds current capabilities",
            ));

        let details = error
            .fixed_track_selection_details()
            .expect("fixed-track details should be attached");
        assert_eq!(details.code, "trackExceedsCapabilities");
        assert_eq!(details.expected_catalog_revision, Some(7));
        assert_eq!(details.actual_catalog_revision, Some(8));
        assert_eq!(
            error.message(),
            "the requested track exceeds current capabilities"
        );
    }
}
