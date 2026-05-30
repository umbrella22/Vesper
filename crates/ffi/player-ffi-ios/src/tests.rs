use std::ffi::{CString, c_char};
use std::ptr;

use super::{
    PlayerFfiCallStatus, PlayerFfiError, PlayerFfiErrorCategory, PlayerFfiErrorCode,
    map_player_error, player_error_to_ffi, player_ffi_ios_native_frame_pipeline_advance,
    player_ffi_ios_native_frame_pipeline_close, player_ffi_ios_native_frame_pipeline_open,
    player_ffi_ios_native_frame_pipeline_release_frame, player_ffi_ios_native_frame_pipeline_seek,
};
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

#[test]
fn ios_native_frame_pipeline_open_requires_source_normalizer_packet_plugin_path() {
    let source = test_c_string("file:///tmp/video.mp4");
    let decoder_path = test_c_string("/tmp/libdecoder.dylib");
    let mut decoder_paths = [decoder_path.as_ptr() as *mut c_char];
    let mut out_handle = 99_u64;
    let mut out_json: *mut c_char = ptr::null_mut();
    let mut error = PlayerFfiError::default();

    let status = unsafe {
        player_ffi_ios_native_frame_pipeline_open(
            source.as_ptr(),
            2,
            ptr::null_mut(),
            0,
            ptr::null(),
            3,
            decoder_paths.as_mut_ptr(),
            decoder_paths.len(),
            ptr::null_mut(),
            0,
            0,
            &mut out_handle,
            &mut out_json,
            &mut error,
        )
    };

    assert_eq!(status, PlayerFfiCallStatus::Error);
    assert_eq!(out_handle, 0);
    assert!(out_json.is_null());
    assert_eq!(error.code, PlayerFfiErrorCode::BackendFailure);
    let message = ffi_error_message(&error);
    assert!(message.contains("nativeFrameIssueKind=missingSourceNormalizerPacketPlugin"));
    assert!(message.contains("SourceNormalizer packet-stream plugin path"));
    unsafe { super::player_ffi_error_free(&mut error) };
}

#[test]
fn ios_native_frame_pipeline_open_requires_videotoolbox_decoder_plugin_path() {
    let source = test_c_string("file:///tmp/video.mp4");
    let source_path = test_c_string("/tmp/libsource_normalizer.dylib");
    let mut source_paths = [source_path.as_ptr() as *mut c_char];
    let mut out_handle = 99_u64;
    let mut out_json: *mut c_char = ptr::null_mut();
    let mut error = PlayerFfiError::default();

    let status = unsafe {
        player_ffi_ios_native_frame_pipeline_open(
            source.as_ptr(),
            2,
            source_paths.as_mut_ptr(),
            source_paths.len(),
            ptr::null(),
            3,
            ptr::null_mut(),
            0,
            ptr::null_mut(),
            0,
            0,
            &mut out_handle,
            &mut out_json,
            &mut error,
        )
    };

    assert_eq!(status, PlayerFfiCallStatus::Error);
    assert_eq!(out_handle, 0);
    assert!(out_json.is_null());
    assert_eq!(error.code, PlayerFfiErrorCode::BackendFailure);
    let message = ffi_error_message(&error);
    assert!(message.contains("nativeFrameIssueKind=missingVideoToolboxDecoderPlugin"));
    assert!(message.contains("VideoToolbox decoder plugin path"));
    unsafe { super::player_ffi_error_free(&mut error) };
}

#[test]
fn ios_native_frame_pipeline_invalid_handles_fail_and_close_is_idempotent() {
    let mut out_json: *mut c_char = ptr::null_mut();
    let mut error = PlayerFfiError::default();

    let advance_status = unsafe {
        player_ffi_ios_native_frame_pipeline_advance(0xDEAD_BEEF, &mut out_json, &mut error)
    };
    assert_eq!(advance_status, PlayerFfiCallStatus::Error);
    assert_eq!(error.code, PlayerFfiErrorCode::InvalidArgument);
    assert!(ffi_error_message(&error).contains("invalid native-frame pipeline handle"));
    unsafe { super::player_ffi_error_free(&mut error) };

    let release_status = unsafe {
        player_ffi_ios_native_frame_pipeline_release_frame(
            0xDEAD_BEEF,
            1,
            false,
            &mut out_json,
            &mut error,
        )
    };
    assert_eq!(release_status, PlayerFfiCallStatus::Error);
    assert_eq!(error.code, PlayerFfiErrorCode::InvalidArgument);
    assert!(ffi_error_message(&error).contains("invalid native-frame pipeline handle"));
    unsafe { super::player_ffi_error_free(&mut error) };

    let seek_status = unsafe {
        player_ffi_ios_native_frame_pipeline_seek(0xDEAD_BEEF, 1_000, &mut out_json, &mut error)
    };
    assert_eq!(seek_status, PlayerFfiCallStatus::Error);
    assert_eq!(error.code, PlayerFfiErrorCode::InvalidArgument);
    assert!(ffi_error_message(&error).contains("invalid native-frame pipeline handle"));
    unsafe { super::player_ffi_error_free(&mut error) };

    unsafe {
        player_ffi_ios_native_frame_pipeline_close(0xDEAD_BEEF);
        player_ffi_ios_native_frame_pipeline_close(0xDEAD_BEEF);
    }
}

#[test]
fn ios_native_frame_pipeline_seek_requires_json_output_pointer() {
    let mut error = PlayerFfiError::default();

    let status = unsafe {
        player_ffi_ios_native_frame_pipeline_seek(0xDEAD_BEEF, 1_000, ptr::null_mut(), &mut error)
    };

    assert_eq!(status, PlayerFfiCallStatus::Error);
    assert_eq!(error.code, PlayerFfiErrorCode::NullPointer);
    assert!(ffi_error_message(&error).contains("out_json was null"));
    unsafe { super::player_ffi_error_free(&mut error) };
}

fn ffi_error_message(error: &PlayerFfiError) -> String {
    if error.message.is_null() {
        return String::new();
    }
    unsafe {
        std::ffi::CStr::from_ptr(error.message)
            .to_string_lossy()
            .into_owned()
    }
}

fn test_c_string(value: &str) -> CString {
    match CString::new(value) {
        Ok(value) => value,
        Err(error) => panic!("test string contained an unexpected NUL byte: {error}"),
    }
}
