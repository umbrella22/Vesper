use super::{PlayerFfiErrorCategory, PlayerFfiErrorCode, map_player_error, player_error_to_ffi};
use crate::handles::HandleRegistry;
use player_runtime::{PlayerError, PlayerErrorCategory, PlayerErrorCode};

#[test]
fn ffi_handle_registry_reuses_slot_with_new_generation_and_rejects_stale_handle() {
    let mut registry = HandleRegistry::default();
    let first = registry.insert(7_u32);

    assert_eq!(registry.get(first), Some(&7));
    assert_eq!(registry.remove(first), Some(7));

    let second = registry.insert(9_u32);
    assert_ne!(first, second);
    assert!(registry.get(first).is_none());
    assert_eq!(registry.get(second), Some(&9));
}

#[test]
fn ffi_error_code_ordinals_append_new_player_error_codes() {
    assert_eq!(PlayerFfiErrorCode::None as i32, 0);
    assert_eq!(PlayerFfiErrorCode::NullPointer as i32, 1);
    assert_eq!(PlayerFfiErrorCode::InvalidUtf8 as i32, 2);
    assert_eq!(PlayerFfiErrorCode::InvalidArgument as i32, 3);
    assert_eq!(PlayerFfiErrorCode::InvalidState as i32, 4);
    assert_eq!(PlayerFfiErrorCode::InvalidSource as i32, 5);
    assert_eq!(PlayerFfiErrorCode::BackendFailure as i32, 6);
    assert_eq!(PlayerFfiErrorCode::AudioOutputUnavailable as i32, 7);
    assert_eq!(PlayerFfiErrorCode::DecodeFailure as i32, 8);
    assert_eq!(PlayerFfiErrorCode::SeekFailure as i32, 9);
    assert_eq!(PlayerFfiErrorCode::Unsupported as i32, 10);
    assert_eq!(PlayerFfiErrorCode::CommandChannelClosed as i32, 11);
    assert_eq!(PlayerFfiErrorCode::EventChannelClosed as i32, 12);
    assert_eq!(PlayerFfiErrorCode::Cancelled as i32, 13);
    assert_eq!(PlayerFfiErrorCode::Timeout as i32, 14);
}

#[test]
fn player_error_mapping_preserves_legacy_and_appended_values() {
    let cases = [
        (
            PlayerErrorCode::InvalidArgument,
            PlayerErrorCategory::Input,
            PlayerFfiErrorCode::InvalidArgument,
            PlayerFfiErrorCategory::Input,
        ),
        (
            PlayerErrorCode::InvalidState,
            PlayerErrorCategory::Playback,
            PlayerFfiErrorCode::InvalidState,
            PlayerFfiErrorCategory::Playback,
        ),
        (
            PlayerErrorCode::InvalidSource,
            PlayerErrorCategory::Source,
            PlayerFfiErrorCode::InvalidSource,
            PlayerFfiErrorCategory::Source,
        ),
        (
            PlayerErrorCode::BackendFailure,
            PlayerErrorCategory::Platform,
            PlayerFfiErrorCode::BackendFailure,
            PlayerFfiErrorCategory::Platform,
        ),
        (
            PlayerErrorCode::AudioOutputUnavailable,
            PlayerErrorCategory::AudioOutput,
            PlayerFfiErrorCode::AudioOutputUnavailable,
            PlayerFfiErrorCategory::AudioOutput,
        ),
        (
            PlayerErrorCode::DecodeFailure,
            PlayerErrorCategory::Decode,
            PlayerFfiErrorCode::DecodeFailure,
            PlayerFfiErrorCategory::Decode,
        ),
        (
            PlayerErrorCode::SeekFailure,
            PlayerErrorCategory::Playback,
            PlayerFfiErrorCode::SeekFailure,
            PlayerFfiErrorCategory::Playback,
        ),
        (
            PlayerErrorCode::Unsupported,
            PlayerErrorCategory::Capability,
            PlayerFfiErrorCode::Unsupported,
            PlayerFfiErrorCategory::Capability,
        ),
        (
            PlayerErrorCode::CommandChannelClosed,
            PlayerErrorCategory::Playback,
            PlayerFfiErrorCode::CommandChannelClosed,
            PlayerFfiErrorCategory::Playback,
        ),
        (
            PlayerErrorCode::EventChannelClosed,
            PlayerErrorCategory::Playback,
            PlayerFfiErrorCode::EventChannelClosed,
            PlayerFfiErrorCategory::Playback,
        ),
        (
            PlayerErrorCode::Cancelled,
            PlayerErrorCategory::Playback,
            PlayerFfiErrorCode::Cancelled,
            PlayerFfiErrorCategory::Playback,
        ),
        (
            PlayerErrorCode::Timeout,
            PlayerErrorCategory::Playback,
            PlayerFfiErrorCode::Timeout,
            PlayerFfiErrorCategory::Playback,
        ),
    ];

    for (player_code, player_category, ffi_code, ffi_category) in cases {
        let player_error = PlayerError::with_category(player_code, player_category, "error");
        assert_eq!(map_player_error(&player_error), (ffi_code, ffi_category));

        let mut ffi_error = player_error_to_ffi(player_error);
        assert_eq!(ffi_error.code, ffi_code);
        assert_eq!(ffi_error.category, ffi_category);
        unsafe { super::player_ffi_error_free(&mut ffi_error) };
    }
}

#[test]
fn ffi_error_code_direct_mapping_preserves_legacy_and_appended_values() {
    let cases = [
        (
            PlayerFfiErrorCode::InvalidArgument,
            PlayerErrorCode::InvalidArgument,
        ),
        (
            PlayerFfiErrorCode::InvalidState,
            PlayerErrorCode::InvalidState,
        ),
        (
            PlayerFfiErrorCode::InvalidSource,
            PlayerErrorCode::InvalidSource,
        ),
        (
            PlayerFfiErrorCode::BackendFailure,
            PlayerErrorCode::BackendFailure,
        ),
        (
            PlayerFfiErrorCode::AudioOutputUnavailable,
            PlayerErrorCode::AudioOutputUnavailable,
        ),
        (
            PlayerFfiErrorCode::DecodeFailure,
            PlayerErrorCode::DecodeFailure,
        ),
        (
            PlayerFfiErrorCode::SeekFailure,
            PlayerErrorCode::SeekFailure,
        ),
        (
            PlayerFfiErrorCode::Unsupported,
            PlayerErrorCode::Unsupported,
        ),
        (
            PlayerFfiErrorCode::CommandChannelClosed,
            PlayerErrorCode::CommandChannelClosed,
        ),
        (
            PlayerFfiErrorCode::EventChannelClosed,
            PlayerErrorCode::EventChannelClosed,
        ),
        (PlayerFfiErrorCode::Cancelled, PlayerErrorCode::Cancelled),
        (PlayerFfiErrorCode::Timeout, PlayerErrorCode::Timeout),
    ];

    for (ffi_code, code) in cases {
        assert_eq!(PlayerErrorCode::from(ffi_code), code);
    }
}
