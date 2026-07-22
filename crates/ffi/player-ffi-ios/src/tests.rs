use std::ffi::{CStr, CString, c_char};
use std::ptr;

use super::{
    PlayerFfiBufferingPolicy, PlayerFfiCachePolicy, PlayerFfiCallStatus, PlayerFfiError,
    PlayerFfiErrorCategory, PlayerFfiErrorCode, PlayerFfiResolvedResiliencePolicy,
    PlayerFfiRetryPolicy, dash_bridge_error_to_ffi, map_player_error, player_error_to_ffi,
    player_ffi_dash_bridge_parse_sidx, player_ffi_ios_native_frame_pipeline_advance,
    player_ffi_ios_native_frame_pipeline_close, player_ffi_ios_native_frame_pipeline_open,
    player_ffi_ios_native_frame_pipeline_release_frame, player_ffi_ios_native_frame_pipeline_seek,
    player_ffi_ios_plugin_abi_summary_json, player_ffi_preload_session_fail,
    player_ffi_resolve_resilience_policy, player_ffi_source_normalizer_resource_open,
};
use crate::handles::HandleRegistry;
use player_plugin::{
    VESPER_DECODER_PLUGIN_ABI_VERSION_CURRENT, VESPER_FRAME_PROCESSOR_PLUGIN_ABI_VERSION_CURRENT,
    VESPER_SOURCE_NORMALIZER_PLUGIN_ABI_VERSION_CURRENT,
};
use player_runtime::{PlayerError, PlayerErrorCategory, PlayerErrorCode, SubtitleErrorDetails};

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
fn resolve_resilience_policy_rejects_invalid_raw_source_kind() {
    let buffering = PlayerFfiBufferingPolicy::default();
    let retry = PlayerFfiRetryPolicy::default();
    let cache = PlayerFfiCachePolicy::default();
    let mut policy = PlayerFfiResolvedResiliencePolicy::default();
    let mut error = PlayerFfiError::default();

    let status = unsafe {
        player_ffi_resolve_resilience_policy(
            99,
            0,
            &buffering,
            &retry,
            &cache,
            &mut policy,
            &mut error,
        )
    };

    assert_eq!(status, PlayerFfiCallStatus::Error);
    assert_eq!(error.code, PlayerFfiErrorCode::InvalidArgument);
    assert!(ffi_error_message(&error).contains("source_kind"));
    unsafe { super::player_ffi_error_free(&mut error) };
}

#[test]
fn preload_fail_rejects_invalid_raw_error_code_before_handle_lookup() {
    let message = CString::new("boom").expect("message");
    let mut error = PlayerFfiError::default();

    let status = unsafe {
        player_ffi_preload_session_fail(
            0xDEAD_BEEF,
            1,
            999,
            PlayerFfiErrorCategory::Playback as u32,
            true,
            message.as_ptr(),
            &mut error,
        )
    };

    assert_eq!(status, PlayerFfiCallStatus::Error);
    assert_eq!(error.code, PlayerFfiErrorCode::InvalidArgument);
    assert!(ffi_error_message(&error).contains("error code"));
    unsafe { super::player_ffi_error_free(&mut error) };
}

#[test]
fn output_c_strings_replace_embedded_nul_with_space() {
    let mut value = super::conversions::into_c_string_ptr("hello\0world".to_owned());

    let text = unsafe { CStr::from_ptr(value) }
        .to_str()
        .expect("sanitized output should be UTF-8");
    assert_eq!(text, "hello world");

    super::conversions::free_c_string(&mut value);
    assert!(value.is_null());
}

#[test]
fn read_string_list_rejects_null_elements() {
    let item = CString::new("one").expect("test string");
    let mut values = [item.as_ptr() as *mut c_char, ptr::null_mut()];

    let mut error =
        super::conversions::read_string_list(values.as_mut_ptr(), values.len(), "items")
            .expect_err("null list elements should be rejected");

    assert_eq!(error.code, PlayerFfiErrorCode::InvalidArgument);
    assert!(ffi_error_message(&error).contains("items[1]"));
    unsafe { super::player_ffi_error_free(&mut error) };
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
fn player_error_to_ffi_preserves_structured_subtitle_details() {
    let error = PlayerError::new(PlayerErrorCode::Timeout, "outer timeout").with_subtitle_details(
        SubtitleErrorDetails::new(
            "future_subtitle_code",
            "future_phase",
            Some("opaque-track".to_owned()),
            true,
            "typed timeout",
        )
        .with_transaction(Some(42), Some(9)),
    );
    let mut ffi_error = player_error_to_ffi(error);
    assert!(!ffi_error.details_json.is_null());
    // SAFETY: player_error_to_ffi returns a valid owned NUL-terminated string.
    let json = unsafe { CStr::from_ptr(ffi_error.details_json) }
        .to_string_lossy()
        .into_owned();
    let details: SubtitleErrorDetails = serde_json::from_str(&json).expect("subtitle details");
    let payload: serde_json::Value = serde_json::from_str(&json).expect("details envelope");
    assert_eq!(payload["domain"], "subtitle");
    assert_eq!(details.code, "future_subtitle_code");
    assert_eq!(details.phase, "future_phase");
    assert_eq!(details.command_id, Some(42));
    assert_eq!(details.source_epoch, Some(9));
    unsafe { super::player_ffi_error_free(&mut ffi_error) };
}

#[test]
fn dash_bridge_error_to_ffi_emits_structured_non_subtitle_envelope() {
    let error =
        player_dash_hls_bridge::DashHlsError::UnsupportedMpd("multi-period manifest".to_owned());
    let mut ffi_error = dash_bridge_error_to_ffi(&error);
    assert!(!ffi_error.details_json.is_null());
    // SAFETY: dash_bridge_error_to_ffi returns a valid owned C string.
    let json = unsafe { CStr::from_ptr(ffi_error.details_json) }
        .to_string_lossy()
        .into_owned();
    let payload: serde_json::Value = serde_json::from_str(&json).expect("DASH details");
    assert_eq!(payload["domain"], "dash");
    assert_eq!(payload["code"], "dash_manifest_unsupported");
    unsafe { super::player_ffi_error_free(&mut ffi_error) };
}

#[test]
fn dash_bridge_parse_sidx_ffi_preserves_structured_error_details() {
    let truncated_sidx = [0, 0, 0, 16, b's', b'i', b'd', b'x'];
    let mut response_json = ptr::null_mut();
    let mut ffi_error = PlayerFfiError::default();

    let status = unsafe {
        player_ffi_dash_bridge_parse_sidx(
            truncated_sidx.as_ptr(),
            truncated_sidx.len(),
            &mut response_json,
            &mut ffi_error,
        )
    };

    assert_eq!(status, PlayerFfiCallStatus::Error);
    assert!(response_json.is_null());
    assert!(!ffi_error.details_json.is_null());
    // SAFETY: the FFI entry point returned an owned NUL-terminated details string.
    let details = unsafe { CStr::from_ptr(ffi_error.details_json) }
        .to_string_lossy()
        .into_owned();
    let payload: serde_json::Value = serde_json::from_str(&details).expect("DASH details");
    assert_eq!(payload["domain"], "dash");
    assert_eq!(payload["code"], "dash_mp4_invalid");
    unsafe { super::player_ffi_error_free(&mut ffi_error) };
}

#[test]
fn plugin_abi_summary_reports_current_signature_versions() {
    let mut out_json: *mut c_char = ptr::null_mut();
    let mut error = PlayerFfiError::default();

    let status = unsafe { player_ffi_ios_plugin_abi_summary_json(&mut out_json, &mut error) };

    assert_eq!(status, PlayerFfiCallStatus::Ok);
    assert!(error.message.is_null());
    assert!(!out_json.is_null());
    let json = unsafe {
        std::ffi::CStr::from_ptr(out_json)
            .to_string_lossy()
            .into_owned()
    };
    unsafe { super::player_ffi_mobile_plugin_diagnostics_string_free(out_json) };
    let value: serde_json::Value = serde_json::from_str(&json).expect("parse ABI summary JSON");
    assert_eq!(
        value["decoderAbiVersion"].as_u64(),
        Some(VESPER_DECODER_PLUGIN_ABI_VERSION_CURRENT as u64)
    );
    assert_eq!(
        value["frameProcessorAbiVersion"].as_u64(),
        Some(VESPER_FRAME_PROCESSOR_PLUGIN_ABI_VERSION_CURRENT as u64)
    );
    assert_eq!(
        value["sourceNormalizerAbiVersion"].as_u64(),
        Some(VESPER_SOURCE_NORMALIZER_PLUGIN_ABI_VERSION_CURRENT as u64)
    );
    assert_eq!(value["abiSemantics"].as_str(), Some("signature-only"));
    assert_eq!(
        value["capabilityMatching"].as_str(),
        Some("requirements-first")
    );
}

#[test]
fn source_normalizer_resource_open_can_return_bypass_diagnostics_without_handle() {
    let source = test_c_string("file:///tmp/video.flv");
    let output_root = test_c_string("/tmp/vesper-source-normalizer");
    let mut out_handle = 99_u64;
    let mut out_json: *mut c_char = ptr::null_mut();
    let mut error = PlayerFfiError::default();

    let status = unsafe {
        player_ffi_source_normalizer_resource_open(
            source.as_ptr(),
            3,
            ptr::null_mut(),
            0,
            ptr::null(),
            output_root.as_ptr(),
            false,
            &mut out_handle,
            &mut out_json,
            &mut error,
        )
    };

    assert_eq!(status, PlayerFfiCallStatus::Ok);
    assert_eq!(out_handle, 0);
    assert!(error.message.is_null());
    assert!(
        !out_json.is_null(),
        "preferNormalized bypass diagnostics should be returned even without a resource handle"
    );
    let json = unsafe {
        std::ffi::CStr::from_ptr(out_json)
            .to_string_lossy()
            .into_owned()
    };
    unsafe { super::player_ffi_mobile_plugin_diagnostics_string_free(out_json) };
    let value: serde_json::Value = serde_json::from_str(&json).expect("parse diagnostics JSON");
    let diagnostics = value.as_array().expect("diagnostics should be an array");
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic["status"].as_str() == Some("sourceNormalizerUnsupported")
            && diagnostic["participation"].as_str() == Some("bypassed")
            && diagnostic["message"]
                .as_str()
                .is_some_and(|message| message.contains("no plugin paths"))
    }));
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
