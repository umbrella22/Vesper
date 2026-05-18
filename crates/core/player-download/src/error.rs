pub use player_model::{PlayerError, PlayerErrorCategory, PlayerErrorCode, PlayerResult};

#[cfg(test)]
mod tests {
    use super::{PlayerError, PlayerErrorCategory, PlayerErrorCode};

    #[test]
    fn player_error_reexport_defaults_to_code_taxonomy() {
        let error = PlayerError::new(PlayerErrorCode::DecodeFailure, "decoder init failed");

        assert_eq!(error.code(), PlayerErrorCode::DecodeFailure);
        assert_eq!(error.category(), PlayerErrorCategory::Decode);
        assert!(!error.is_retriable());
    }

    #[test]
    fn player_error_reexport_can_override_taxonomy() {
        let error = PlayerError::with_taxonomy(
            PlayerErrorCode::BackendFailure,
            PlayerErrorCategory::Network,
            true,
            "network timed out",
        );

        assert_eq!(error.code(), PlayerErrorCode::BackendFailure);
        assert_eq!(error.category(), PlayerErrorCategory::Network);
        assert!(error.is_retriable());
    }
}
